import { useEffect, useState } from "react";
import { api, InputDeviceInfo } from "../api/commands";
import { useRecording } from "../stores/recording";
import { useSettings } from "../stores/settings";
import { formatTime } from "../utils/format";
import { useTranslation } from "react-i18next";
import SettingsDialog from "./settings/SettingsDialog";

function MicSelect() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const recording = useRecording((s) => s.recording);
  const [devices, setDevices] = useState<InputDeviceInfo[]>([]);

  const refresh = () => {
    api.listInputDevices().then(setDevices).catch(console.error);
  };

  useEffect(refresh, []);

  if (!settings) return null;

  const value = settings.useSystemDefaultMic ? "" : settings.micDeviceId ?? "";
  const defaultDevice = devices.find((d) => d.isDefault);

  return (
    <select
      className="max-w-56 rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1 text-xs disabled:opacity-50"
      title={t("マイク")}
      value={value}
      disabled={recording}
      onFocus={refresh}
      onChange={(e) => {
        const id = e.target.value;
        if (id === "") {
          update({ useSystemDefaultMic: true, micDeviceId: null });
        } else {
          update({ useSystemDefaultMic: false, micDeviceId: id });
        }
      }}
    >
      <option value="">
        {defaultDevice
          ? `${t("システムデフォルト")}(${defaultDevice.name})`
          : t("システムデフォルト")}
      </option>
      {value !== "" && !devices.some((d) => d.id === value) && (
        <option value={value}>{value}</option>
      )}
      {devices.map((d) => (
        <option key={d.id} value={d.id}>
          {d.name}
        </option>
      ))}
    </select>
  );
}

function LevelMeter({ level, label }: { level: number; label: string }) {
  return (
    <div className="flex items-center gap-1" title={label}>
      <span className="text-[10px] text-neutral-400 w-7">{label}</span>
      <div className="w-16 h-1.5 rounded bg-neutral-300 dark:bg-neutral-700 overflow-hidden">
        <div
          className="h-full bg-green-500 transition-[width] duration-150"
          style={{ width: `${Math.round(level * 100)}%` }}
        />
      </div>
    </div>
  );
}

export default function Toolbar() {
  const { t } = useTranslation();
  const { recording, duration, levels, busy, start, stop, error, clearError, noInput } =
    useRecording();
  const [showSettings, setShowSettings] = useState(false);

  const noInputMessage = noInput
    ? noInput.sys && noInput.mic
      ? t("音声が検出されていません。システム設定でマイクとシステム音声の権限を確認してください。")
      : noInput.mic
        ? t("マイク入力が検出されていません。権限とデバイスを確認してください。")
        : t("システム音声が検出されていません。権限を確認してください。")
    : null;

  return (
    <div className="flex items-center gap-3 px-3 h-12 border-b border-neutral-200 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800">
      <button
        className={`flex items-center gap-2 px-3 py-1.5 rounded-md text-sm font-medium text-white disabled:opacity-50 ${
          recording ? "bg-red-600 hover:bg-red-700" : "bg-red-500 hover:bg-red-600"
        }`}
        disabled={busy}
        onClick={() => (recording ? stop() : start())}
      >
        <span
          className={`inline-block w-2.5 h-2.5 rounded-full bg-white ${
            recording ? "animate-pulse" : ""
          }`}
        />
        {recording ? t("録音停止") : t("録音開始")}
      </button>
      {recording && (
        <span className="text-sm tabular-nums text-red-600 dark:text-red-400 font-medium">
          {formatTime(duration)}
        </span>
      )}
      {recording && (
        <div className="flex flex-col gap-0.5">
          <LevelMeter level={levels.sys} label="PC" />
          <LevelMeter level={levels.mic} label="Mic" />
        </div>
      )}
      <div className="flex-1" />
      {recording && noInputMessage && (
        <span className="text-xs text-amber-600 dark:text-amber-400 truncate max-w-96">
          {noInputMessage}
        </span>
      )}
      {error && (
        <button
          className="text-xs text-red-500 truncate max-w-96"
          title={error}
          onClick={clearError}
        >
          {error}
        </button>
      )}
      <MicSelect />
      <button
        className="px-2 py-1 rounded-md text-lg text-neutral-500 hover:bg-neutral-200 dark:hover:bg-neutral-700"
        title={t("設定")}
        onClick={() => setShowSettings(true)}
      >
        ⚙
      </button>
      {showSettings && <SettingsDialog onClose={() => setShowSettings(false)} />}
    </div>
  );
}
