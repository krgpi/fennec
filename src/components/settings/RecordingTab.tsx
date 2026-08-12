import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { calendarApi, CalendarInfo } from "../../api/calendar";
import { useSettings } from "../../stores/settings";
import { buttonClass, inputClass, Section, ToggleRow } from "./shared";

export default function RecordingTab() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const [calendars, setCalendars] = useState<CalendarInfo[]>([]);
  const [calendarDenied, setCalendarDenied] = useState(false);
  const [icsDraft, setIcsDraft] = useState<string | null>(null);

  const reminderEnabled = settings?.meetingReminderEnabled ?? false;

  useEffect(() => {
    if (reminderEnabled) {
      calendarApi.listCalendars().then(setCalendars).catch(console.error);
    }
  }, [reminderEnabled]);

  if (!settings) return null;

  const pickSaveDirectory = async () => {
    const result = await open({ directory: true, multiple: false });
    if (typeof result === "string") update({ saveDirectory: result });
  };

  const toggleMeetingReminder = async (enabled: boolean) => {
    if (enabled) {
      const granted = await calendarApi.requestAccess();
      if (!granted) {
        setCalendarDenied(true);
        return;
      }
    }
    setCalendarDenied(false);
    update({ meetingReminderEnabled: enabled });
  };

  const selectedCalendarIds = settings.meetingReminderCalendarIdentifiers ?? null;
  const isCalendarSelected = (id: string) =>
    selectedCalendarIds === null || selectedCalendarIds.includes(id);
  const toggleCalendar = (id: string, checked: boolean) => {
    const ids = new Set(selectedCalendarIds ?? calendars.map((c) => c.id));
    if (checked) {
      ids.add(id);
    } else {
      ids.delete(id);
    }
    update({
      meetingReminderCalendarIdentifiers:
        ids.size === calendars.length ? null : [...ids],
    });
  };

  const calendarGroups = calendars.reduce<[string, CalendarInfo[]][]>((groups, cal) => {
    const group = groups.find(([source]) => source === cal.source);
    if (group) {
      group[1].push(cal);
    } else {
      groups.push([cal.source, [cal]]);
    }
    return groups;
  }, []);
  calendarGroups.sort(([a], [b]) => a.localeCompare(b));

  const commitIcsUrls = () => {
    if (icsDraft === null) return;
    const urls = icsDraft
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line !== "");
    update({ calendarIcsUrls: urls });
    setIcsDraft(null);
  };

  const [minutesBeforePrefix, minutesBeforeSuffix] = t("開始 %lld分前から通知").split("%lld");

  return (
    <div className="flex flex-col gap-4">
      <Section>
        <ToggleRow
          label={t("無音で自動停止")}
          description={t(
            "一定時間音声が検出されないと自動的に録音を停止し、通知でお知らせします。",
          )}
          checked={settings.silenceAutoStopEnabled}
          onChange={(v) => update({ silenceAutoStopEnabled: v })}
        />
        {settings.silenceAutoStopEnabled && (
          <label className="flex items-center gap-2 pl-6 text-sm">
            <span>{t("停止までの時間")}</span>
            <input
              type="number"
              min={1}
              max={60}
              className={`w-16 ${inputClass}`}
              value={settings.silenceAutoStopMinutes}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isInteger(v) && v >= 1 && v <= 60) {
                  update({ silenceAutoStopMinutes: v });
                }
              }}
            />
            <span>{t("分")}</span>
          </label>
        )}
      </Section>

      <Section title={t("会議リマインダー")}>
        <ToggleRow
          label={t("会議リマインダー")}
          description={t("カレンダーの会議予定に合わせて録音開始を提案します。")}
          checked={settings.meetingReminderEnabled}
          onChange={(v) => void toggleMeetingReminder(v)}
        />
        {calendarDenied && (
          <p className="pl-6 text-xs text-red-500">
            {t(
              "カレンダーへのアクセスが許可されていません。システム設定 > プライバシーとセキュリティ > カレンダー で許可してください。",
            )}
          </p>
        )}
        {settings.meetingReminderEnabled && (
          <div className="flex flex-col gap-3 pl-6">
            <label className="flex items-center gap-2 text-sm">
              <span>{minutesBeforePrefix}</span>
              <input
                type="number"
                min={0}
                max={30}
                className={`w-16 ${inputClass}`}
                value={settings.meetingReminderMinutesBefore}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  if (Number.isInteger(v) && v >= 0 && v <= 30) {
                    update({ meetingReminderMinutesBefore: v });
                  }
                }}
              />
              <span>{minutesBeforeSuffix}</span>
            </label>
            <ToggleRow
              label={t("会議URLを含むイベントのみ")}
              checked={settings.meetingReminderRequireMeetingUrl}
              onChange={(v) => update({ meetingReminderRequireMeetingUrl: v })}
            />
            {calendarGroups.length > 0 && (
              <div className="flex flex-col gap-1">
                <span className="text-sm">{t("対象カレンダー")}</span>
                {calendarGroups.map(([source, cals]) => (
                  <div key={source} className="flex flex-col gap-1">
                    <span className="text-xs text-neutral-500">{source}</span>
                    {cals.map((cal) => (
                      <label key={cal.id} className="flex items-center gap-2 pl-3 text-sm">
                        <input
                          type="checkbox"
                          checked={isCalendarSelected(cal.id)}
                          onChange={(e) => toggleCalendar(cal.id, e.target.checked)}
                        />
                        <span
                          className="inline-block h-2.5 w-2.5 shrink-0 rounded-full"
                          style={{ backgroundColor: cal.color ?? "#999999" }}
                        />
                        <span>{cal.title}</span>
                      </label>
                    ))}
                  </div>
                ))}
              </div>
            )}
            <div className="flex flex-col gap-1">
              <span className="text-sm">{t("ICS購読URL")}</span>
              <textarea
                className={`min-h-20 ${inputClass}`}
                placeholder="https://example.com/calendar.ics"
                value={icsDraft ?? settings.calendarIcsUrls.join("\n")}
                onChange={(e) => setIcsDraft(e.target.value)}
                onBlur={commitIcsUrls}
              />
              <p className="text-xs text-neutral-500">{t("1行に1つのURLを入力します。")}</p>
            </div>
          </div>
        )}
      </Section>

      <Section title={t("保存先")}>
        <p className="text-xs text-neutral-500">
          {t(
            "変更すると録音一覧の表示も新しい保存先に切り替わります。既存の録音ファイルは自動では移動しません。",
          )}
        </p>
        <div className="flex items-center gap-2">
          <span
            className="flex-1 truncate text-sm"
            title={settings.saveDirectory ?? undefined}
          >
            {settings.saveDirectory ?? t("デフォルト")}
          </span>
          <button className={buttonClass} onClick={pickSaveDirectory}>
            {t("変更…")}
          </button>
          {settings.saveDirectory && (
            <button className={buttonClass} onClick={() => update({ saveDirectory: null })}>
              {t("デフォルトに戻す")}
            </button>
          )}
        </div>
      </Section>
    </div>
  );
}
