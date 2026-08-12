import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../stores/settings";
import type { AutomationHook, HookEvent } from "../../types";
import { buttonClass, inputClass } from "./shared";

const HOOK_EVENTS: HookEvent[] = [
  "recordingStarted",
  "recordingStopped",
  "transcriptionCompleted",
  "minutesGenerated",
];

function useEventLabels(): Record<HookEvent, string> {
  const { t } = useTranslation();
  return {
    recordingStarted: t("録音開始時"),
    recordingStopped: t("録音停止時"),
    transcriptionCompleted: t("文字起こし完了時"),
    minutesGenerated: t("議事録生成時"),
  };
}

function HookRow({
  hook,
  onChange,
  onRemove,
}: {
  hook: AutomationHook;
  onChange: (patch: Partial<AutomationHook>) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const eventLabels = useEventLabels();
  const [command, setCommand] = useState(hook.command);

  useEffect(() => {
    setCommand(hook.command);
  }, [hook.command]);

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-neutral-200 dark:border-neutral-700 p-3">
      <div className="flex items-center gap-2">
        <select
          className={inputClass}
          value={hook.event}
          onChange={(e) => onChange({ event: e.target.value as HookEvent })}
        >
          {HOOK_EVENTS.map((event) => (
            <option key={event} value={event}>
              {eventLabels[event]}
            </option>
          ))}
        </select>
        <div className="flex-1" />
        <label className="flex items-center gap-1 text-xs">
          <input
            type="checkbox"
            checked={hook.enabled}
            onChange={(e) => onChange({ enabled: e.target.checked })}
          />
          {t("有効")}
        </label>
        <button
          className="px-1 text-xs text-neutral-400 hover:text-red-500"
          title={t("削除")}
          onClick={onRemove}
        >
          ✕
        </button>
      </div>
      <input
        className={`${inputClass} font-mono text-xs`}
        placeholder={t("コマンド (例: echo $FENNEC_SESSION_DIR >> ~/fennec.log)")}
        value={command}
        onChange={(e) => setCommand(e.target.value)}
        onBlur={() => {
          if (command !== hook.command) onChange({ command });
        }}
      />
    </div>
  );
}

export default function AutomationTab() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  if (!settings) return null;

  const hooks = settings.automationHooks;

  const updateHook = (id: string, patch: Partial<AutomationHook>) => {
    update({
      automationHooks: hooks.map((h) => (h.id === id ? { ...h, ...patch } : h)),
    });
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-neutral-500">
        {t("イベント発生時に実行するシェルコマンドを設定します。")}
      </p>

      {hooks.map((hook) => (
        <HookRow
          key={hook.id}
          hook={hook}
          onChange={(patch) => updateHook(hook.id, patch)}
          onRemove={() =>
            update({ automationHooks: hooks.filter((h) => h.id !== hook.id) })
          }
        />
      ))}

      <button
        className={`self-start ${buttonClass}`}
        onClick={() =>
          update({
            automationHooks: [
              ...hooks,
              {
                id: crypto.randomUUID(),
                event: "recordingStopped",
                command: "",
                enabled: true,
              },
            ],
          })
        }
      >
        + {t("フックを追加")}
      </button>

      <div className="mt-2 flex items-center gap-2 border-t border-neutral-200 pt-3 text-sm dark:border-neutral-700">
        <span>{t("タイムアウト")}</span>
        <input
          type="number"
          min={1}
          className={`w-20 ${inputClass}`}
          value={settings.hookTimeoutSeconds}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isInteger(v) && v >= 1) update({ hookTimeoutSeconds: v });
          }}
        />
        <span>{t("秒")}</span>
      </div>

      <p className="select-text text-[11px] text-neutral-500">
        {t(
          "環境変数: FENNEC_EVENT, FENNEC_SESSION_ID, FENNEC_SESSION_DIR, FENNEC_TRANSCRIPT_FILE, FENNEC_MINUTES_FILE",
        )}
      </p>
    </div>
  );
}
