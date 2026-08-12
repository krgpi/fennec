import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, WhisperModelStatus } from "../../api/commands";
import { useJobs } from "../../stores/jobs";
import { useSettings } from "../../stores/settings";
import { buttonClass, inputClass, Section, ToggleRow } from "./shared";

export default function TranscriptionTab() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const modelDownload = useJobs((s) => s.modelDownload);
  const [models, setModels] = useState<WhisperModelStatus[]>([]);
  const [locale, setLocale] = useState(settings?.transcriptionLocale ?? "");
  const [diarizationReady, setDiarizationReady] = useState(false);

  useEffect(() => {
    if (!modelDownload.running) {
      api.listWhisperModels().then(setModels).catch(console.error);
      api.diarizationStatus().then(setDiarizationReady).catch(console.error);
    }
  }, [modelDownload.running]);

  if (!settings) return null;

  const live = settings.liveTranscriptionEnabled;
  const isMac = navigator.userAgent.includes("Mac");

  const commitLocale = () => {
    const v = locale.trim();
    update({ transcriptionLocale: v === "" ? null : v });
  };

  return (
    <div className="flex flex-col gap-4">
      <Section>
        <ToggleRow
          label={t("録音中にリアルタイム文字起こし")}
          description={t(
            "録音中にリアルタイムで文字起こしを表示します（Whisperモデルのダウンロードが必要です）。",
          )}
          checked={live}
          onChange={(v) => update({ liveTranscriptionEnabled: v })}
        />
        {live && (
          <p className="pl-6 text-xs text-neutral-500">
            {t("オンの間は録音後の自動文字起こしを行いません。")}
          </p>
        )}
        {live && isMac && (
          <label className="flex items-center gap-2 text-sm">
            <span className="w-28 shrink-0 text-neutral-500">{t("エンジン")}</span>
            <select
              className={`flex-1 ${inputClass}`}
              value={settings.liveTranscriptionEngine}
              onChange={(e) => update({ liveTranscriptionEngine: e.target.value })}
            >
              <option value="apple">{t("Apple 音声認識")}</option>
              <option value="whisper">Whisper</option>
            </select>
          </label>
        )}
      </Section>

      <Section>
        <ToggleRow
          label={t("録音後に自動で文字起こし")}
          description={t("録音終了後に自動的に文字起こしを実行します。")}
          checked={!live && settings.autoTranscribeEnabled}
          disabled={live}
          onChange={(v) => update({ autoTranscribeEnabled: v })}
        />
        {live && (
          <p className="pl-6 text-xs text-neutral-500">
            {t("リアルタイム文字起こしがオンのため選択できません。")}
          </p>
        )}
        {isMac && (
          <label className="flex items-center gap-2 text-sm">
            <span className="w-28 shrink-0 text-neutral-500">{t("エンジン")}</span>
            <select
              className={`flex-1 ${inputClass}`}
              value={settings.autoTranscribeEngine}
              onChange={(e) => update({ autoTranscribeEngine: e.target.value })}
            >
              <option value="apple">{t("Apple 音声認識")}</option>
              <option value="whisper">Whisper</option>
            </select>
          </label>
        )}
      </Section>

      <Section>
        <label className="flex items-center gap-2 text-sm">
          <span className="w-28 shrink-0 text-neutral-500">{t("文字起こし言語")}</span>
          <input
            className={`flex-1 ${inputClass}`}
            placeholder="ja-JP"
            value={locale}
            onChange={(e) => setLocale(e.target.value)}
            onBlur={commitLocale}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitLocale();
            }}
          />
        </label>
        <p className="text-xs text-neutral-500">
          {t("音声認識に使用する言語を選択します。")} {t("空の場合はシステムの言語を使用します。")}
        </p>
      </Section>

      <Section title="Whisper">
        {models.map((m) => {
          const downloading = modelDownload.running && modelDownload.modelId === m.id;
          return (
            <div key={m.id} className="flex items-center gap-3">
              <input
                type="radio"
                name="whisperModel"
                checked={settings.whisperModelId === m.id}
                onChange={() => update({ whisperModelId: m.id })}
              />
              <div className="min-w-0 flex-1">
                <div className="text-sm">
                  {m.name}{" "}
                  <span className="text-xs text-neutral-500">{m.size}</span>
                </div>
                <div className="text-xs text-neutral-500">{m.detail}</div>
              </div>
              {downloading ? (
                <div className="flex items-center gap-2">
                  <div className="h-1.5 w-24 overflow-hidden rounded bg-neutral-300 dark:bg-neutral-700">
                    <div
                      className="h-full bg-blue-500 transition-[width] duration-150"
                      style={{ width: `${Math.round(modelDownload.fraction * 100)}%` }}
                    />
                  </div>
                  <span className="text-xs tabular-nums text-neutral-500">
                    {Math.round(modelDownload.fraction * 100)}%
                  </span>
                  <button className={buttonClass} onClick={() => api.cancelModelDownload()}>
                    {t("キャンセル")}
                  </button>
                </div>
              ) : m.downloaded ? (
                <button
                  className={buttonClass}
                  onClick={() =>
                    api
                      .deleteWhisperModel(m.id)
                      .then(() => api.listWhisperModels().then(setModels))
                      .catch(console.error)
                  }
                >
                  {t("削除")}
                </button>
              ) : (
                <button
                  className={buttonClass}
                  disabled={modelDownload.running}
                  onClick={() => api.downloadWhisperModel(m.id).catch(console.error)}
                >
                  {t("ダウンロード")}
                </button>
              )}
            </div>
          );
        })}
        {modelDownload.error && (
          <p className="text-xs text-red-500">{modelDownload.error}</p>
        )}
      </Section>

      <Section title={t("話者識別")}>
        <ToggleRow
          label={t("話者識別")}
          description={t(
            "録音後の文字起こしで話者を識別してラベル付けします（別途モデルのダウンロードが必要、約45MB）。",
          )}
          checked={settings.diarizationEnabled}
          onChange={(v) => update({ diarizationEnabled: v })}
        />
        <div className="flex items-center gap-3 pl-6">
          {modelDownload.running && modelDownload.modelId === "diarization" ? (
            <>
              <div className="h-1.5 w-24 overflow-hidden rounded bg-neutral-300 dark:bg-neutral-700">
                <div
                  className="h-full bg-blue-500 transition-[width] duration-150"
                  style={{ width: `${Math.round(modelDownload.fraction * 100)}%` }}
                />
              </div>
              <span className="text-xs tabular-nums text-neutral-500">
                {Math.round(modelDownload.fraction * 100)}%
              </span>
            </>
          ) : diarizationReady ? (
            <>
              <span className="text-xs text-green-600">{t("モデル導入済み")}</span>
              <button
                className={buttonClass}
                onClick={() =>
                  api
                    .deleteDiarizationModels()
                    .then(() => setDiarizationReady(false))
                    .catch(console.error)
                }
              >
                {t("削除")}
              </button>
            </>
          ) : (
            <button
              className={buttonClass}
              disabled={modelDownload.running}
              onClick={() => api.downloadDiarizationModels().catch(console.error)}
            >
              {t("モデルをダウンロード")}
            </button>
          )}
        </div>
      </Section>
    </div>
  );
}
