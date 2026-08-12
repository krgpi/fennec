import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, BackendInfo } from "../api/commands";
import { useJobs } from "../stores/jobs";
import { useSettings } from "../stores/settings";
import type { MinutesBackend, MinutesPreset } from "../types";

interface Props {
  sessionId: string;
  onClose: () => void;
}

export default function MinutesDialog({ sessionId, onClose }: Props) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const minutesJob = useJobs((s) => s.minutes);
  const [backends, setBackends] = useState<BackendInfo[]>([]);
  const [presetId, setPresetId] = useState<string>("");
  const [backend, setBackend] = useState<MinutesBackend>("claude");
  const [model, setModel] = useState("");
  const [contextFolder, setContextFolder] = useState<string | null>(null);
  const [outputFolder, setOutputFolder] = useState<string | null>(null);
  const [saveAsPreset, setSaveAsPreset] = useState(false);
  const [presetName, setPresetName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const presets = settings?.minutesPresets ?? [];
  const current = backends.find((b) => b.backend === backend);

  useEffect(() => {
    api.listMinutesBackends().then((list) => {
      setBackends(list);
      if (list.length > 0) {
        setBackend(list[0].backend);
        setModel(list[0].defaultModel);
      }
    });
  }, []);

  const applyPreset = (preset: MinutesPreset | null) => {
    if (!preset) {
      setPresetId("");
      return;
    }
    setPresetId(preset.id);
    setBackend(preset.backend);
    setModel(preset.model);
    setContextFolder(preset.contextFolder ?? null);
    setOutputFolder(preset.outputFolder ?? null);
  };

  const pickFolder = async (set: (v: string | null) => void) => {
    const result = await open({ directory: true, multiple: false });
    if (typeof result === "string") set(result);
  };

  const running = minutesJob.running;
  const canGenerate = useMemo(
    () => backends.length > 0 && !running && (!saveAsPreset || presetName.trim() !== ""),
    [backends, running, saveAsPreset, presetName],
  );

  const generate = async () => {
    setError(null);
    let usedPresetId = presetId || null;
    if (saveAsPreset && settings) {
      const newPreset: MinutesPreset = {
        id: crypto.randomUUID(),
        name: presetName.trim(),
        contextFolder,
        outputFolder,
        backend,
        model,
        createdAt: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
        lastUsedAt: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
      };
      await update({ minutesPresets: [...presets, newPreset] });
      usedPresetId = newPreset.id;
    }
    try {
      await api.generateMinutes({
        sessionId,
        backend,
        model,
        contextFolder,
        outputFolder,
        presetId: usedPresetId,
      });
      useJobs.getState().setMinutes({ running: true, sessionId, error: null });
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[520px] max-w-[90vw] rounded-xl bg-white dark:bg-neutral-800 shadow-2xl p-5"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-base font-semibold mb-4">{t("議事録を作成")}</h2>

        {backends.length === 0 ? (
          <p className="text-sm text-neutral-500">
            {t("Claude Code / Codex / Gemini CLI が見つかりません。いずれかをインストールしてください。")}
          </p>
        ) : (
          <div className="flex flex-col gap-3 text-sm">
            {presets.length > 0 && (
              <label className="flex items-center gap-2">
                <span className="w-28 text-neutral-500">{t("プリセット")}</span>
                <select
                  className="flex-1 rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1"
                  value={presetId}
                  onChange={(e) =>
                    applyPreset(presets.find((p) => p.id === e.target.value) ?? null)
                  }
                >
                  <option value="">{t("プリセットなし")}</option>
                  {presets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>
            )}

            <label className="flex items-center gap-2">
              <span className="w-28 text-neutral-500">{t("バックエンド")}</span>
              <select
                className="flex-1 rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1"
                value={backend}
                onChange={(e) => {
                  const b = e.target.value as MinutesBackend;
                  setBackend(b);
                  const info = backends.find((x) => x.backend === b);
                  if (info) setModel(info.defaultModel);
                }}
              >
                {backends.map((b) => (
                  <option key={b.backend} value={b.backend}>
                    {b.displayName}
                  </option>
                ))}
              </select>
            </label>

            <label className="flex items-center gap-2">
              <span className="w-28 text-neutral-500">{t("モデル")}</span>
              <select
                className="flex-1 rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                {(current?.models ?? []).map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
            </label>

            <FolderRow
              label={t("コンテキスト")}
              value={contextFolder}
              onPick={() => pickFolder(setContextFolder)}
              onClear={() => setContextFolder(null)}
            />
            <FolderRow
              label={t("出力先")}
              value={outputFolder}
              onPick={() => pickFolder(setOutputFolder)}
              onClear={() => setOutputFolder(null)}
            />
            <p className="pl-30 -mt-2 text-[11px] text-neutral-500">
              {t(
                "生成した議事録をここに書き出します。以降アプリ内で編集した内容は、議事録タブから書き出すまで反映されません。",
              )}
            </p>

            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={saveAsPreset}
                onChange={(e) => setSaveAsPreset(e.target.checked)}
              />
              <span>{t("プリセットとして保存")}</span>
              {saveAsPreset && (
                <input
                  className="flex-1 rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1"
                  placeholder={t("プリセット名")}
                  value={presetName}
                  onChange={(e) => setPresetName(e.target.value)}
                />
              )}
            </label>

            {error && <p className="text-xs text-red-500">{error}</p>}

            <div className="flex justify-end gap-2 mt-2">
              <button
                className="px-3 py-1.5 rounded text-sm hover:bg-neutral-100 dark:hover:bg-neutral-700"
                onClick={onClose}
              >
                {t("キャンセル")}
              </button>
              <button
                className="px-3 py-1.5 rounded bg-blue-500 text-white text-sm hover:bg-blue-600 disabled:opacity-50"
                disabled={!canGenerate}
                onClick={generate}
              >
                {t("作成")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function FolderRow({
  label,
  value,
  onPick,
  onClear,
}: {
  label: string;
  value: string | null;
  onPick: () => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2">
      <span className="w-28 text-neutral-500">{label}</span>
      <span className="flex-1 truncate text-xs text-neutral-600 dark:text-neutral-300">
        {value ?? t("未設定")}
      </span>
      <button
        className="px-2 py-0.5 rounded border border-neutral-300 dark:border-neutral-600 text-xs"
        onClick={onPick}
      >
        {t("選択...")}
      </button>
      {value && (
        <button className="text-xs text-neutral-400" onClick={onClear}>
          ✕
        </button>
      )}
    </div>
  );
}
