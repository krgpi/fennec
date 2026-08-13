import { useTranslation } from "react-i18next";
import { setAppLanguage } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { isMac } from "../../utils/platform";
import { inputClass, Section, ToggleRow } from "./shared";

export default function GeneralTab() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  if (!settings) return null;

  const language =
    settings.appLanguage === "ja" || settings.appLanguage === "en"
      ? settings.appLanguage
      : "system";

  return (
    <div className="flex flex-col gap-4">
      <Section>
        <label className="flex items-center gap-2 text-sm">
          <span className="w-28 shrink-0 text-neutral-500">{t("言語")}</span>
          <select
            className={`flex-1 ${inputClass}`}
            value={language}
            onChange={(e) => {
              const lang = e.target.value as "system" | "ja" | "en";
              setAppLanguage(lang);
              update({ appLanguage: lang });
            }}
          >
            <option value="system">{t("システムに従う")}</option>
            <option value="ja">日本語</option>
            <option value="en">English</option>
          </select>
        </label>
      </Section>

      <Section>
        <ToggleRow
          label={t("ログイン時に起動")}
          description={
            isMac
              ? t("Macの起動時に自動的にアプリを起動します。")
              : t("システムの起動時に自動的にアプリを起動します。")
          }
          checked={settings.launchAtLogin}
          onChange={(checked) => update({ launchAtLogin: checked })}
        />
        {isMac ? (
          <>
            <div className="flex flex-col gap-1">
              <ToggleRow
                label={t("メニューバーに表示")}
                description={t(
                  "メニューバーからすばやく録音の開始・停止ができます。録音中は経過時間が表示されます。",
                )}
                checked={settings.showInMenuBar || settings.hideFromDock}
                disabled={settings.hideFromDock}
                onChange={(checked) => update({ showInMenuBar: checked })}
              />
              {settings.hideFromDock && (
                <p className="pl-6 text-xs text-amber-600 dark:text-amber-500">
                  {t("Dockアイコンを非表示にしている場合、メニューバー表示は必須です。")}
                </p>
              )}
            </div>
            <ToggleRow
              label={t("Dockアイコンを非表示")}
              description={t("ウィンドウが表示されていないときはDockアイコンを非表示にします。")}
              checked={settings.hideFromDock}
              onChange={(checked) =>
                update(
                  checked
                    ? { hideFromDock: true, showInMenuBar: true }
                    : { hideFromDock: false },
                )
              }
            />
          </>
        ) : (
          <ToggleRow
            label={t("通知領域に表示")}
            description={t(
              "通知領域のアイコンからすばやく録音の開始・停止ができます。ウィンドウを閉じてもアプリは常駐します。",
            )}
            checked={settings.showInMenuBar}
            onChange={(checked) => update({ showInMenuBar: checked })}
          />
        )}
      </Section>
    </div>
  );
}
