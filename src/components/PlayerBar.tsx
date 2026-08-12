import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, PlayerPosition } from "../api/commands";
import { events } from "../api/events";
import { formatTime } from "../utils/format";

const RATES = [1.0, 1.25, 1.5, 2.0];

export default function PlayerBar({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<PlayerPosition | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rate, setRate] = useState(1.0);
  const [sysMuted, setSysMuted] = useState(false);
  const [micMuted, setMicMuted] = useState(false);
  const seeking = useRef(false);

  useEffect(() => {
    const unlisten = events.onPlayerPosition((p) => {
      if (p.sessionId === sessionId && !seeking.current) setStatus(p);
    });
    return () => {
      unlisten.then((f) => f());
      api.playerUnload().catch(() => {});
    };
  }, [sessionId]);

  const ensureLoaded = async (): Promise<boolean> => {
    if (status) return true;
    setLoading(true);
    setError(null);
    try {
      const p = await api.playerLoad(sessionId);
      setStatus(p);
      setSysMuted(false);
      setMicMuted(false);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setLoading(false);
    }
  };

  const toggle = async () => {
    if (!(await ensureLoaded())) return;
    if (status?.playing) {
      await api.playerPause();
    } else {
      await api.playerPlay();
    }
  };

  return (
    <div className="flex items-center gap-3 px-5 py-2 border-b border-neutral-200 dark:border-neutral-700 text-sm">
      <button
        className="w-8 h-8 flex items-center justify-center rounded-full bg-neutral-200 dark:bg-neutral-700 hover:bg-neutral-300 dark:hover:bg-neutral-600 disabled:opacity-50"
        disabled={loading}
        onClick={toggle}
        title={status?.playing ? t("一時停止") : t("再生")}
      >
        {status?.playing ? "⏸" : "▶"}
      </button>

      <span className="tabular-nums text-xs text-neutral-500 w-12 text-right">
        {formatTime(status?.position ?? 0)}
      </span>
      <input
        type="range"
        className="flex-1 accent-blue-500"
        min={0}
        max={status?.duration ?? 1}
        step={0.1}
        value={status?.position ?? 0}
        disabled={!status}
        onMouseDown={() => (seeking.current = true)}
        onChange={(e) =>
          setStatus((s) => (s ? { ...s, position: Number(e.target.value) } : s))
        }
        onMouseUp={(e) => {
          seeking.current = false;
          api.playerSeek(Number((e.target as HTMLInputElement).value));
        }}
      />
      <span className="tabular-nums text-xs text-neutral-500 w-12">
        {formatTime(status?.duration ?? 0)}
      </span>

      <select
        className="bg-transparent text-xs border border-neutral-300 dark:border-neutral-600 rounded px-1 py-0.5"
        value={rate}
        onChange={(e) => {
          const r = Number(e.target.value);
          setRate(r);
          api.playerSetRate(r);
        }}
      >
        {RATES.map((r) => (
          <option key={r} value={r}>
            {r}x
          </option>
        ))}
      </select>

      {status?.hasSystem && (
        <label className="flex items-center gap-1 text-xs text-neutral-500">
          <input
            type="checkbox"
            checked={!sysMuted}
            onChange={(e) => {
              setSysMuted(!e.target.checked);
              api.playerSetTrackMuted(0, !e.target.checked);
            }}
          />
          {t("transcript.source.system", "PC音声")}
        </label>
      )}
      {status?.hasMic && (
        <label className="flex items-center gap-1 text-xs text-neutral-500">
          <input
            type="checkbox"
            checked={!micMuted}
            onChange={(e) => {
              setMicMuted(!e.target.checked);
              api.playerSetTrackMuted(1, !e.target.checked);
            }}
          />
          {t("transcript.source.mic", "マイク")}
        </label>
      )}
      {error && <span className="text-xs text-red-500 truncate max-w-48">{error}</span>}
    </div>
  );
}
