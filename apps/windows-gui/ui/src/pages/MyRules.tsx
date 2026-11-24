import { useEffect, useState } from "react";
import { desktop } from "../bridge";
import { Card, Run } from "../ui";
import { t } from "../i18n";

/** The example shown in the empty editor, in common rule syntax. */
const PLACEHOLDER = "DOMAIN-SUFFIX,example.com,PROXY\nDOMAIN-KEYWORD,ads,REJECT\nIP-CIDR,10.0.0.0/8,DIRECT,no-resolve\nPROCESS-NAME,curl,DIRECT";

/**
 * "我的规则": rule lines the user writes, matched before the routing mode's or the subscription's rules, as on iOS
 * and macOS. A running engine takes a saved list in place, so open connections stay.
 */
export function MyRules({ busy, run }: { busy: string | null; run: Run }) {
  const [saved, setSaved] = useState<{ text: string; count: number } | null>(null);
  const [text, setText] = useState("");

  useEffect(() => {
    void desktop.myRules().then((rules) => {
      setSaved(rules);
      setText(rules.text);
    }).catch(() => setSaved({ text: "", count: 0 }));
  }, []);

  const changed = saved !== null && text !== saved.text;
  const save = () => void run("my rules", async () => {
    const result = await desktop.saveMyRules(text);
    setSaved(await desktop.myRules());
    return result;
  });

  return (
    <Card
      title={t("我的规则")}
      description={t("每行一条，自上而下匹配，先于分流模式和订阅规则；策略为 PROXY、DIRECT 或 REJECT（含 REJECT-DROP、REJECT-TINYGIF、REJECT-NO-DROP），# 开头为注释。支持 DOMAIN、DOMAIN-SUFFIX、DOMAIN-KEYWORD、DOMAIN-WILDCARD、IP-CIDR、IP-CIDR6、DST-PORT、SRC-IP、IN-PORT、SRC-PORT、PROCESS-NAME、USER-AGENT、URL-REGEX、PROTOCOL、GEOIP、IP-ASN、GEOSITE、HOSTNAME-TYPE、SUBNET、DEVICE-NAME、CELLULAR-RADIO 和 RULE-SET；GEOSITE,cn 与 GEOIP,CN 使用内置公开列表（gaoyifan/china-operator-ip、Loyalsoldier/v2ray-rules-dat）。")}
      actions={saved && <span className="count">{t("{count} 条规则", { count: saved.count })}</span>}
    >
      <div className="rule-editor">
        <textarea
          aria-label={t("我的规则")}
          spellCheck={false}
          placeholder={PLACEHOLDER}
          value={text}
          disabled={saved === null}
          onChange={(event) => setText(event.target.value)}
        />
        <div className="button-row">
          <button className="link-button" disabled={!changed || Boolean(busy)} onClick={() => saved && setText(saved.text)}>{t("还原")}</button>
          <button className="secondary compact" disabled={!changed || Boolean(busy)} onClick={save}>{t("保存")}</button>
        </div>
      </div>
    </Card>
  );
}
