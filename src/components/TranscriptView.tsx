import { useTranslation } from "react-i18next";
import type { TranscriptEntry } from "../types";
import { formatTime } from "../utils/format";

const SPEAKER_COLORS = [
  "text-blue-500",
  "text-green-500",
  "text-orange-500",
  "text-purple-500",
  "text-pink-500",
  "text-cyan-500",
  "text-emerald-400",
  "text-indigo-500",
];

export default function TranscriptView({ entries }: { entries: TranscriptEntry[] }) {
  const { t } = useTranslation();

  if (entries.length === 0) {
    return (
      <p className="text-sm text-neutral-500 italic">{t("（文字起こし結果がありません）")}</p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {entries.map((entry, i) => {
        const speakerColor =
          entry.speakerId != null
            ? SPEAKER_COLORS[Math.abs(entry.speakerId) % SPEAKER_COLORS.length]
            : entry.source === "system"
              ? "text-blue-500"
              : "text-green-500";
        const label =
          entry.speakerId != null
            ? t("話者{{n}}", { n: entry.speakerId + 1, defaultValue: `話者${entry.speakerId + 1}` })
            : entry.source === "system"
              ? t("transcript.source.system", "PC音声")
              : t("transcript.source.mic", "マイク");
        return (
          <div key={i}>
            <div className={`flex items-center gap-2 text-xs font-semibold ${speakerColor}`}>
              <span>{label}</span>
              <span className="text-neutral-400 font-normal tabular-nums">
                {formatTime(entry.startTime)}
              </span>
            </div>
            <p className="text-sm whitespace-pre-wrap select-text cursor-text">{entry.text}</p>
            {entry.translation && (
              <p className="text-sm text-neutral-500 italic select-text">
                → {entry.translation}
              </p>
            )}
          </div>
        );
      })}
    </div>
  );
}
