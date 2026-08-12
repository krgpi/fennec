import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api/commands";
import { useJobs } from "../stores/jobs";
import { useRecording } from "../stores/recording";
import { useSessions } from "../stores/sessions";
import type { RecordingSession } from "../types";
import { relativeDateLabel, sectionLabel } from "../utils/format";

function groupSessions(sessions: RecordingSession[], lang: string) {
  const groups: { label: string; items: RecordingSession[] }[] = [];
  for (const s of sessions) {
    const label = sectionLabel(s.createdAt, lang);
    const last = groups[groups.length - 1];
    if (last && last.label === label) {
      last.items.push(s);
    } else {
      groups.push({ label, items: [s] });
    }
  }
  return groups;
}

interface MenuState {
  sessionId: string;
  x: number;
  y: number;
  hasAudio: boolean;
}

export default function SessionList() {
  const { t, i18n } = useTranslation();
  const { sessions, selectedId, select, load } = useSessions();
  const recording = useRecording();
  const transcribe = useJobs((s) => s.transcribe);
  const lang = i18n.language;
  const [menu, setMenu] = useState<MenuState | null>(null);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("contextmenu", close, { capture: true, once: true });
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close, { capture: true });
    };
  }, [menu]);

  const retranscribe = async (id: string) => {
    setMenu(null);
    try {
      await api.startTranscription(id);
    } catch (e) {
      alert(String(e));
    }
  };

  const deleteSession = async (id: string) => {
    setMenu(null);
    const ok = await confirm(t("この録音を削除しますか？"), { kind: "warning" });
    if (!ok) return;
    try {
      await api.deleteSession(id);
      await load();
    } catch (e) {
      alert(String(e));
    }
  };

  if (sessions.length === 0) {
    return (
      <p className="p-4 text-sm text-neutral-500">{t("録音データがありません")}</p>
    );
  }

  return (
    <div className="py-1">
      {groupSessions(sessions, lang).map((group) => (
        <div key={group.label}>
          <div className="px-3 pt-3 pb-1 text-[11px] font-semibold text-neutral-400 uppercase">
            {group.label}
          </div>
          {group.items.map((s) => {
            const isRecording = recording.recording && recording.sessionId === s.id;
            const isTranscribing = transcribe.running && transcribe.sessionId === s.id;
            return (
              <button
                key={s.id}
                className={`w-full px-3 py-2 text-left hover:bg-neutral-200/70 dark:hover:bg-neutral-800 ${
                  s.id === selectedId ? "bg-neutral-200 dark:bg-neutral-800" : ""
                }`}
                onClick={() => select(s.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  select(s.id);
                  setMenu({
                    sessionId: s.id,
                    x: e.clientX,
                    y: e.clientY,
                    hasAudio: !!(s.systemAudioFile || s.micAudioFile),
                  });
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="flex-1 text-sm font-medium truncate">
                    {s.summary ?? s.id}
                  </span>
                  <span className="text-[11px] text-neutral-400 shrink-0">
                    {relativeDateLabel(s.createdAt, lang)}
                  </span>
                </div>
                {isRecording ? (
                  <div className="flex items-center gap-1.5 text-xs text-red-500">
                    <span className="inline-block w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse" />
                    {t("録音中")}
                  </div>
                ) : isTranscribing ? (
                  <div className="text-xs text-blue-500">
                    {t("文字起こし中...")}
                    {transcribe.fraction != null &&
                      ` ${Math.round(transcribe.fraction * 100)}%`}
                  </div>
                ) : (
                  s.transcriptPreview && (
                    <div className="text-xs text-neutral-500 truncate">
                      {s.transcriptPreview}
                    </div>
                  )
                )}
              </button>
            );
          })}
        </div>
      ))}
      {menu && (
        <div
          className="fixed z-50 min-w-40 rounded-md border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 shadow-lg py-1 text-sm"
          style={{ left: menu.x, top: menu.y }}
        >
          {menu.hasAudio && (
            <button
              className="w-full px-3 py-1.5 text-left hover:bg-neutral-100 dark:hover:bg-neutral-700 disabled:opacity-50"
              disabled={transcribe.running}
              onClick={() => retranscribe(menu.sessionId)}
            >
              {t("再文字起こし")}
            </button>
          )}
          <button
            className="w-full px-3 py-1.5 text-left hover:bg-neutral-100 dark:hover:bg-neutral-700"
            onClick={() => {
              setMenu(null);
              api.revealSessionFolder(menu.sessionId).catch(console.error);
            }}
          >
            {t("Finderで開く")}
          </button>
          <button
            className="w-full px-3 py-1.5 text-left text-red-500 hover:bg-neutral-100 dark:hover:bg-neutral-700"
            onClick={() => deleteSession(menu.sessionId)}
          >
            {t("削除")}
          </button>
        </div>
      )}
    </div>
  );
}
