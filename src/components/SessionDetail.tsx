import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api/commands";
import { events } from "../api/events";
import { useJobs } from "../stores/jobs";
import { useRecording } from "../stores/recording";
import { useSessions } from "../stores/sessions";
import type { RecordingSession, TranscriptEntry } from "../types";
import { formatSessionDate } from "../utils/format";
import { useDebouncedSave } from "../utils/useDebouncedSave";
import MinutesView from "./MinutesView";
import PlayerBar from "./PlayerBar";
import TranscriptView from "./TranscriptView";

type Tab = "note" | "minutes" | "transcript";

export default function SessionDetail({ session }: { session: RecordingSession }) {
  const { t } = useTranslation();
  const load = useSessions((s) => s.load);
  const transcribeJob = useJobs((s) => s.transcribe);
  const [tab, setTab] = useState<Tab>("transcript");
  const [entries, setEntries] = useState<TranscriptEntry[]>([]);
  const [note, setNote] = useState("");
  const [title, setTitle] = useState(session.summary ?? "");

  const recordingState = useRecording();
  const sessionId = session.id;
  const isTranscribing = transcribeJob.running && transcribeJob.sessionId === sessionId;
  const isRecordingThis =
    recordingState.recording && recordingState.sessionId === sessionId;

  const saveTitle = useCallback(
    (value: string) => {
      api.updateSummary(sessionId, value).then(load).catch(console.error);
    },
    [sessionId, load],
  );
  const titleSaver = useDebouncedSave(1000, saveTitle);

  const saveNote = useCallback(
    (value: string) => {
      api.saveNote(sessionId, value).catch(console.error);
    },
    [sessionId],
  );
  const noteSaver = useDebouncedSave(1000, saveNote);

  useEffect(() => {
    setTitle(session.summary ?? "");
    setEntries([]);
    api.readTranscript(sessionId).then(setEntries).catch(console.error);
    api.readNote(sessionId).then(setNote).catch(console.error);
    setTab(session.minutesFile ? "minutes" : "transcript");
    return () => {
      titleSaver.flush();
      noteSaver.flush();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  useEffect(() => {
    const unlistenTranscribe = events.onTranscribeDone((p) => {
      if (p.sessionId === sessionId && !p.error) {
        api.readTranscript(sessionId).then(setEntries).catch(console.error);
        setTab("transcript");
      }
    });
    const unlistenMinutes = events.onMinutesDone((p) => {
      if (p.sessionId === sessionId && !p.error) setTab("minutes");
    });
    return () => {
      unlistenTranscribe.then((f) => f());
      unlistenMinutes.then((f) => f());
    };
  }, [sessionId]);

  const engineLabel = (() => {
    if (!session.engineType) return "—";
    if (session.engineType === "whisper") return "Whisper";
    if (session.engineType === "apple") return t("Apple 音声認識");
    return session.engineType;
  })();

  return (
    <div className="flex flex-col h-full">
      <div className="px-5 pt-4 pb-3 border-b border-neutral-200 dark:border-neutral-700">
        <input
          className="w-full bg-transparent text-lg font-semibold outline-none placeholder-neutral-400"
          value={title}
          placeholder={session.id}
          onChange={(e) => {
            setTitle(e.target.value);
            titleSaver.schedule(e.target.value);
          }}
          onBlur={() => titleSaver.flush()}
        />
        <div className="flex items-center gap-3 mt-1 text-xs text-neutral-500">
          <span>{formatSessionDate(session.createdAt)}</span>
          <span>{engineLabel}</span>
          <div className="flex-1" />
          <button
            className="hover:text-neutral-800 dark:hover:text-neutral-200"
            onClick={() => api.revealSessionFolder(sessionId)}
          >
            {t("Finderで開く")}
          </button>
        </div>
      </div>

      {(session.systemAudioFile || session.micAudioFile) && (
        <PlayerBar key={sessionId} sessionId={sessionId} />
      )}

      <div className="flex gap-1 px-5 pt-3">
        {(
          [
            ["note", t("メモ")],
            ["minutes", t("議事録")],
            ["transcript", t("文字起こし")],
          ] as [Tab, string][]
        ).map(([key, label]) => (
          <button
            key={key}
            className={`px-3 py-1 text-sm rounded-md ${
              tab === key
                ? "bg-neutral-200 dark:bg-neutral-700 font-medium"
                : "text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"
            }`}
            onClick={() => setTab(key)}
          >
            {label}
          </button>
        ))}
      </div>

      <div className={tab === "minutes" ? "flex-1 min-h-0 flex flex-col" : "hidden"}>
        <MinutesView session={session} />
      </div>

      <div className={tab === "minutes" ? "hidden" : "flex-1 overflow-y-auto px-5 py-4"}>
        {tab === "transcript" && isRecordingThis ? (
          <div className="flex flex-col gap-4">
            {(["system", "mic"] as const).map((source) => {
              const live = recordingState.live[source];
              if (!live || (!live.finalized && !live.volatile)) return null;
              const translation = recordingState.liveTranslation[source];
              return (
                <div key={source}>
                  <div
                    className={`text-xs font-semibold mb-1 ${
                      source === "system" ? "text-blue-500" : "text-green-500"
                    }`}
                  >
                    {source === "system"
                      ? t("transcript.source.system", "PC音声")
                      : t("transcript.source.mic", "マイク")}
                  </div>
                  <p className="text-sm whitespace-pre-wrap">
                    {live.finalized}
                    {live.volatile && (
                      <span className="text-neutral-400"> {live.volatile}</span>
                    )}
                  </p>
                  {translation && (
                    <p className="text-sm text-neutral-500 italic mt-1">→ {translation}</p>
                  )}
                </div>
              );
            })}
            {!recordingState.live.system && !recordingState.live.mic && (
              <p className="text-sm text-neutral-500 italic">{t("録音中...")}</p>
            )}
          </div>
        ) : tab === "transcript" && (
          <>
            {isTranscribing && (
              <div className="mb-3 text-sm text-blue-500">
                {t("文字起こし中...")}
                {transcribeJob.fraction != null &&
                  ` ${Math.round(transcribeJob.fraction * 100)}%`}
              </div>
            )}
            {entries.length === 0 && !isTranscribing && session.hasTranscript === false ? (
              <button
                className="px-3 py-1.5 rounded-md bg-blue-500 text-white text-sm hover:bg-blue-600 disabled:opacity-50"
                disabled={transcribeJob.running}
                onClick={() =>
                  api.startTranscription(sessionId).catch((e) => alert(String(e)))
                }
              >
                {t("文字起こしを作成")}
              </button>
            ) : (
              <TranscriptView entries={entries} />
            )}
          </>
        )}
        {tab === "note" && (
          <textarea
            className="w-full h-full min-h-64 bg-transparent outline-none resize-none text-sm font-mono"
            placeholder={t("メモを入力...")}
            value={note}
            onChange={(e) => {
              setNote(e.target.value);
              noteSaver.schedule(e.target.value);
            }}
            onBlur={() => noteSaver.flush()}
          />
        )}
      </div>
    </div>
  );
}
