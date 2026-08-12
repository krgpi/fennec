import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, WhisperModelStatus } from "../api/commands";
import { useJobs } from "../stores/jobs";

export default function ModelBanner() {
  const { t } = useTranslation();
  const [models, setModels] = useState<WhisperModelStatus[]>([]);
  const download = useJobs((s) => s.modelDownload);

  const refresh = () => api.listWhisperModels().then(setModels).catch(console.error);

  useEffect(() => {
    refresh();
  }, [download.running]);

  const selected = models.find((m) => m.selected);
  if (!selected || selected.downloaded) return null;

  return (
    <div className="flex items-center gap-3 px-3 py-2 text-sm bg-amber-100 dark:bg-amber-900/40 text-amber-900 dark:text-amber-100 border-b border-amber-200 dark:border-amber-800">
      {download.running ? (
        <>
          <span>
            {t("ダウンロード中...")} {selected.name} (
            {Math.round(download.fraction * 100)}%)
          </span>
          <div className="flex-1 h-1.5 rounded bg-amber-200 dark:bg-amber-800 overflow-hidden">
            <div
              className="h-full bg-amber-500 transition-[width]"
              style={{ width: `${Math.round(download.fraction * 100)}%` }}
            />
          </div>
          <button
            className="text-xs underline"
            onClick={() => api.cancelModelDownload()}
          >
            {t("キャンセル")}
          </button>
        </>
      ) : (
        <>
          <span>
            {t("Whisperモデルが必要です")}: {selected.name}（{selected.size}）
          </span>
          {download.error && (
            <span className="text-xs text-red-600 truncate">{download.error}</span>
          )}
          <div className="flex-1" />
          <button
            className="px-2 py-1 rounded bg-amber-500 text-white text-xs font-medium hover:bg-amber-600"
            onClick={() => api.downloadWhisperModel(selected.id)}
          >
            {t("ダウンロード")}
          </button>
        </>
      )}
    </div>
  );
}
