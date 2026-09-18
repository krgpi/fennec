import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AgentsConfigInfo, api } from "../../api/commands";
import { buttonClass, Section } from "./shared";

export default function MinutesTab() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AgentsConfigInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const reload = (refresh: boolean) =>
    api
      .getAgentsConfig(refresh)
      .then(setInfo)
      .catch((e) => setError(String(e)));

  useEffect(() => {
    reload(false);
  }, []);

  const createAndOpen = async () => {
    try {
      setInfo(await api.createAgentsConfig());
      await api.openAgentsConfig();
    } catch (e) {
      setError(String(e));
    }
  };

  const copyPath = async () => {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.path);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      setError(String(e));
    }
  };

  if (!info) return null;

  return (
    <div className="flex flex-col gap-3">
      <Section title={t("エージェント")}>
        <p className="text-xs text-neutral-500">
          {t(
            "議事録の生成に使う CLI の一覧です。設定ファイルを編集すると、エージェントの追加・組み込み定義の上書き・無効化ができます。",
          )}
        </p>
        <ul className="flex flex-col gap-1 text-sm">
          {info.agents.map((a) => (
            <li key={a.backend} className="flex items-baseline gap-2">
              <span className="w-28 shrink-0 truncate">{a.displayName}</span>
              <span className="select-text truncate font-mono text-xs text-neutral-500">
                {a.binaryPath ?? `${a.binary}: ${t("見つかりません")}`}
              </span>
            </li>
          ))}
        </ul>
        <div className="flex items-center gap-2">
          {info.exists ? (
            <button className={buttonClass} onClick={() => api.openAgentsConfig()}>
              {t("設定ファイルを開く")}
            </button>
          ) : (
            <button className={buttonClass} onClick={createAndOpen}>
              {t("設定ファイルを作成")}
            </button>
          )}
          <button className={buttonClass} onClick={() => reload(true)}>
            {t("再読み込み")}
          </button>
        </div>
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 select-text break-all font-mono text-[11px] text-neutral-500">
            {info.path}
          </p>
          <button className={`shrink-0 ${buttonClass}`} onClick={copyPath}>
            {copied ? t("コピー済み") : t("コピー")}
          </button>
        </div>
        {info.error && <p className="text-xs text-red-500">{info.error}</p>}
        {error && <p className="text-xs text-red-500">{error}</p>}
      </Section>
      <p className="text-[11px] text-neutral-500">
        {t(
          "引数テンプレートでは {prompt} {model} {modelArgs} {output} が使えます。promptVia を stdin にすると標準入力でプロンプトを渡し、outputVia を file にすると {output} のファイルから結果を読みます。",
        )}
      </p>
    </div>
  );
}
