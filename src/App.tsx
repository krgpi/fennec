import { useEffect } from "react";
import { events } from "./api/events";
import ModelBanner from "./components/ModelBanner";
import SessionDetail from "./components/SessionDetail";
import SessionList from "./components/SessionList";
import Toolbar from "./components/Toolbar";
import { useJobs } from "./stores/jobs";
import { useRecording } from "./stores/recording";
import { useSessions } from "./stores/sessions";
import { useSettings } from "./stores/settings";

export default function App() {
  const { sessions, selectedId, load } = useSessions();

  useEffect(() => {
    load();
    useSettings.getState().load();
    const jobs = useJobs.getState();

    const unlisteners = [
      events.onRecordingLevels((p) => useRecording.getState().setLevels(p)),
      events.onRecordingState((p) => useRecording.getState().setState(p)),
      events.onRecordingNoInput((p) => useRecording.getState().setNoInput(p)),
      events.onLiveTranscript((p) =>
        useRecording.getState().setLive(p.source, {
          finalized: p.finalized,
          volatile: p.volatile,
        }),
      ),
      events.onLiveTranslation((p) =>
        useRecording.getState().setLiveTranslation(p.source, p.text),
      ),
      events.onSessionsChanged(() => useSessions.getState().load()),
      events.onTranscribeProgress((p) =>
        jobs.setTranscribe({
          running: true,
          sessionId: p.sessionId,
          phase: p.phase,
          fraction: p.fraction,
        }),
      ),
      events.onTranscribeDone((p) =>
        jobs.setTranscribe({
          running: false,
          sessionId: p.sessionId,
          phase: null,
          fraction: null,
          lastError: p.error,
        }),
      ),
      events.onModelProgress((p) =>
        jobs.setModelDownload({
          running: !p.done,
          modelId: p.modelId,
          fraction: p.fraction,
          error: p.error,
        }),
      ),
      events.onMinutesDone((p) =>
        jobs.setMinutes({ running: false, sessionId: p.sessionId, error: p.error }),
      ),
    ];

    return () => {
      unlisteners.forEach((u) => u.then((f) => f()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selected = sessions.find((s) => s.id === selectedId) ?? null;

  return (
    <div className="flex flex-col h-screen bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100">
      <Toolbar />
      <ModelBanner />
      <div className="flex flex-1 min-h-0">
        <aside className="w-64 shrink-0 border-r border-neutral-200 dark:border-neutral-700 overflow-y-auto bg-neutral-50 dark:bg-neutral-900">
          <SessionList />
        </aside>
        <main className="flex-1 min-w-0">
          {selected ? (
            <SessionDetail key={selected.id} session={selected} />
          ) : (
            <div className="flex items-center justify-center h-full text-neutral-400">
              Fennec
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
