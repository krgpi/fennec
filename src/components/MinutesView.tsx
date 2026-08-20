import { save } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, MinutesDocument } from "../api/commands";
import { events } from "../api/events";
import { useJobs } from "../stores/jobs";
import { useSettings } from "../stores/settings";
import type { RecordingSession } from "../types";
import { formatSessionDate, sanitizeFileName } from "../utils/format";
import { useDebouncedSave } from "../utils/useDebouncedSave";
import MarkdownView from "./MarkdownView";
import MinutesDialog from "./MinutesDialog";

function joinPath(dir: string, name: string): string {
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

export default function MinutesView({ session }: { session: RecordingSession }) {
  const { t } = useTranslation();
  const sessionId = session.id;
  const minutesJob = useJobs((s) => s.minutes);
  const settings = useSettings((s) => s.settings);
  const [doc, setDoc] = useState<MinutesDocument | null>(null);
  const [text, setText] = useState("");
  const [mode, setMode] = useState<"view" | "edit">("view");
  const [showDialog, setShowDialog] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);

  const generating = minutesJob.running && minutesJob.sessionId === sessionId;

  const applyDoc = useCallback((next: MinutesDocument) => {
    setDoc(next);
    setText(next.content ?? "");
  }, []);

  const saveMinutes = useCallback(
    (value: string) => {
      api.saveMinutes(sessionId, value).then(setDoc).catch(console.error);
    },
    [sessionId],
  );
  const saver = useDebouncedSave(1000, saveMinutes);

  useEffect(() => {
    setDoc(null);
    setText("");
    setMode("view");
    setExportError(null);
    api.readMinutes(sessionId).then(applyDoc).catch(console.error);
    return () => saver.flush();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  useEffect(() => {
    const unlisten = events.onMinutesDone((p) => {
      if (p.sessionId === sessionId && !p.error) {
        api.readMinutes(sessionId).then(applyDoc).catch(console.error);
        setMode("view");
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [sessionId, applyDoc]);

  const preset = settings?.minutesPresets.find((p) => p.id === session.minutesPresetId);
  const exportedAt = doc?.exportedAt ?? null;
  const exportedPath = doc?.exportedPath ?? null;
  const hasUnsavedEdits = doc != null && text !== (doc.content ?? "");
  const staleExport = exportedAt != null && ((doc?.dirty ?? false) || hasUnsavedEdits);
  const hasContent = text.trim() !== "";

  const runExport = async () => {
    setExportError(null);
    const defaultName =
      session.summary != null && session.summary.trim() !== ""
        ? `${sanitizeFileName(session.summary)}.md`
        : `minutes_${sessionId}.md`;
    const defaultPath =
      exportedPath ??
      (preset?.outputFolder ? joinPath(preset.outputFolder, defaultName) : defaultName);
    const target = await save({
      defaultPath,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (typeof target !== "string") return;
    try {
      setDoc(await api.exportMinutes(sessionId, text, target));
    } catch (e) {
      setExportError(String(e));
    }
  };

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <div className="flex items-center gap-2 px-5 pt-3">
        <div className="flex rounded-md bg-neutral-100 dark:bg-neutral-800 p-0.5">
          {(
            [
              ["view", t("表示")],
              ["edit", t("編集")],
            ] as ["view" | "edit", string][]
          ).map(([key, label]) => (
            <button
              key={key}
              className={`px-2.5 py-0.5 text-xs rounded ${
                mode === key
                  ? "bg-white dark:bg-neutral-600 shadow-sm font-medium"
                  : "text-neutral-500"
              }`}
              onClick={() => setMode(key)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="flex-1" />
        {generating ? (
          <div className="flex items-center gap-3 text-sm text-blue-500">
            <span>{t("議事録を生成中...")}</span>
            <button
              className="text-xs text-neutral-400 underline"
              onClick={() => api.cancelMinutes()}
            >
              {t("キャンセル")}
            </button>
          </div>
        ) : (
          <button
            className={`px-3 py-1 rounded-md text-xs disabled:opacity-50 ${
              hasContent
                ? "border border-neutral-300 dark:border-neutral-600 hover:bg-neutral-100 dark:hover:bg-neutral-700"
                : "bg-blue-500 text-white hover:bg-blue-600"
            }`}
            disabled={!session.hasTranscript || minutesJob.running}
            onClick={() => setShowDialog(true)}
          >
            {hasContent ? t("議事録を再生成") : t("議事録を作成")}
          </button>
        )}
      </div>

      {minutesJob.error && minutesJob.sessionId === sessionId && (
        <p className="px-5 pt-2 text-xs text-red-500">{minutesJob.error}</p>
      )}

      <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4">
        {mode === "edit" ? (
          <textarea
            className="w-full h-full min-h-64 bg-transparent outline-none resize-none text-sm font-mono select-text"
            placeholder={t("議事録を入力...")}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              saver.schedule(e.target.value);
            }}
            onBlur={() => saver.flush()}
          />
        ) : hasContent ? (
          <MarkdownView content={text} />
        ) : (
          doc != null && (
            <p className="text-sm text-neutral-500 italic">
              {t("議事録はまだ作成されていません")}
            </p>
          )
        )}
      </div>

      {(hasContent || exportedAt != null) && (
        <div
          className={`flex items-center gap-3 px-5 py-3 border-t ${
            staleExport
              ? "border-amber-300 dark:border-amber-500/40 bg-amber-50 dark:bg-amber-500/10"
              : "border-neutral-200 dark:border-neutral-700"
          }`}
        >
          <div className="min-w-0 flex-1">
            {exportedAt == null ? (
              <>
                <div className="text-xs font-medium">{t("まだ書き出していません")}</div>
                <div className="text-[11px] text-neutral-500">
                  {t("議事録は録音データと同じフォルダにのみ保存されています。")}
                </div>
              </>
            ) : staleExport ? (
              <>
                <div className="text-xs font-medium text-amber-700 dark:text-amber-400">
                  {t("編集内容はまだ書き出されていません")}
                </div>
                <div className="text-[11px] text-amber-700/80 dark:text-amber-400/80 truncate">
                  {t("書き出し先のファイルは {{date}} に書き出した内容のままです", {
                    date: formatSessionDate(exportedAt),
                  })}
                  {exportedPath && ` · ${exportedPath}`}
                </div>
              </>
            ) : (
              <>
                <div className="text-xs font-medium text-neutral-600 dark:text-neutral-300">
                  {t("書き出し済み · {{date}}", { date: formatSessionDate(exportedAt) })}
                </div>
                <div className="text-[11px] text-neutral-500 truncate" title={exportedPath ?? ""}>
                  {exportedPath}
                </div>
              </>
            )}
            {exportError && <div className="text-[11px] text-red-500">{exportError}</div>}
          </div>
          <button
            className={`shrink-0 px-3 py-1.5 rounded-md text-sm disabled:opacity-50 ${
              staleExport
                ? "bg-amber-500 text-white hover:bg-amber-600"
                : exportedAt == null
                  ? "bg-blue-500 text-white hover:bg-blue-600"
                  : "border border-neutral-300 dark:border-neutral-600 hover:bg-neutral-100 dark:hover:bg-neutral-700"
            }`}
            disabled={!hasContent}
            onClick={runExport}
          >
            {staleExport ? t("書き出して更新...") : t("書き出す...")}
          </button>
        </div>
      )}

      {showDialog && (
        <MinutesDialog sessionId={sessionId} onClose={() => setShowDialog(false)} />
      )}
    </div>
  );
}
