import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../stores/settings";
import AutomationTab from "./AutomationTab";
import GeneralTab from "./GeneralTab";
import RecordingTab from "./RecordingTab";
import TranscriptionTab from "./TranscriptionTab";

type TabId = "general" | "recording" | "transcription" | "automation";

export default function SettingsDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const settings = useSettings((s) => s.settings);
  const [tab, setTab] = useState<TabId>("general");

  if (!settings) return null;

  const tabs: [TabId, string][] = [
    ["general", t("一般")],
    ["recording", t("settings.tab.recording")],
    ["transcription", t("文字起こし")],
    ["automation", t("オートメーション")],
  ];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="flex max-h-[80vh] w-[560px] max-w-[90vw] flex-col rounded-xl bg-white p-5 shadow-2xl dark:bg-neutral-800"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-base font-semibold">{t("設定")}</h2>
          <button
            className="rounded px-2 py-0.5 text-sm text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-700"
            onClick={onClose}
          >
            {t("閉じる")}
          </button>
        </div>

        <div className="mb-4 flex gap-1">
          {tabs.map(([key, label]) => (
            <button
              key={key}
              className={`px-3 py-1 text-sm rounded-md ${
                tab === key
                  ? "bg-neutral-200 dark:bg-neutral-700 font-medium"
                  : "text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-700"
              }`}
              onClick={() => setTab(key)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto">
          {tab === "general" && <GeneralTab />}
          {tab === "recording" && <RecordingTab />}
          {tab === "transcription" && <TranscriptionTab />}
          {tab === "automation" && <AutomationTab />}
        </div>
      </div>
    </div>
  );
}
