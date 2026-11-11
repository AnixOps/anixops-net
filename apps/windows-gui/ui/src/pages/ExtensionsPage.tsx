import { Ban, Download, KeyRound, LockKeyhole, Puzzle, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, MitmPlugin, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Card, EmptyState, MoreMenu, Row, Run, StatusPill, Switch, ToggleRow } from "../ui";
import { t } from "../i18n";

export function ExtensionsPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const linux = snapshot?.platform === "linux";
  const [certificatePath, setCertificatePath] = useState("");
  const [pluginUrl, setPluginUrl] = useState("");
  const [plugins, setPlugins] = useState<MitmPlugin[]>([]);

  const loadPlugins = useCallback(() => {
    desktop.mitmPlugins().then(setPlugins).catch(() => setPlugins([]));
  }, []);
  useEffect(loadPlugins, [loadPlugins]);

  const decrypting = Boolean(snapshot?.httpsDecryptionEnabled);
  const locked = Boolean(busy) || !snapshot;
  const change = (operation: string, task: () => Promise<{ message: string }>) => void run(operation, task).then(loadPlugins);
  const importPlugin = () => {
    const url = pluginUrl.trim();
    if (!url) return;
    void run("plugin", () => desktop.importMitmPlugin(url)).then(() => {
      setPluginUrl("");
      loadPlugins();
    });
  };

  return (
    <div className="page-stack">
      <Card
        title={t("HTTPS 解密")}
        description={t("只解密插件和内置规则列出的域名，其余网站原样转发。首次开启会生成并信任一张本机根证书，关闭后移除。")}
      >
        <ToggleRow
          icon={LockKeyhole}
          color="pink"
          label={t("解密 HTTPS 流量")}
          hint={t("插件和脚本需要它才能改写网页请求")}
          checked={decrypting}
          disabled={locked}
          onChange={() => change("mitm", decrypting ? desktop.disableHttpsMitm : desktop.enableHttpsMitm)}
        />
        {!linux && (
          <ToggleRow
            icon={Ban}
            color="sky"
            label={t("屏蔽哔哩哔哩网页版推广请求（试用）")}
            hint={t("改动后需要重新开启 HTTPS 解密才会生效")}
            checked={Boolean(snapshot?.bilibiliWebAdBlockEnabled)}
            disabled={locked}
            onChange={() => void run("Bilibili web trial", async () => {
              if (!snapshot) throw new Error(t("还没有读取到客户端状态。"));
              await desktop.savePreferences(preferencesFrom(snapshot, { bilibiliWebAdBlockEnabled: !snapshot.bilibiliWebAdBlockEnabled }));
              return { message: t("已保存。重新开启 HTTPS 解密后生效。") };
            })}
          />
        )}
      </Card>

      <Card
        title={t("插件")}
        description={t("支持 Loon、Surge 和 Quantumult X 的插件。导入时会一并下载插件里的脚本，导入即表示允许这些脚本运行。")}
        actions={plugins.length > 0 && <span className="count">{t("{count} 个插件", { count: plugins.length })}</span>}
      >
        <div className="import-row">
          <input
            aria-label={t("插件网址")}
            placeholder={t("https:// 插件网址")}
            value={pluginUrl}
            onChange={(event) => setPluginUrl(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") importPlugin(); }}
          />
          <button className="primary" disabled={Boolean(busy) || !pluginUrl.trim()} onClick={importPlugin}><Download size={14} />{t("导入")}</button>
        </div>
        {plugins.length === 0 ? (
          <EmptyState icon={Puzzle} title={t("还没有插件")} message={t("粘贴一个插件的 https 网址并导入，开启 HTTPS 解密后生效。")} />
        ) : (
          <div className="rows">
            {plugins.map((plugin) => (
              <Row
                key={plugin.id}
                icon={Puzzle}
                color={plugin.enabled ? "purple" : "gray"}
                label={<span title={plugin.url}>{plugin.name}</span>}
                hint={<span title={plugin.hostnames.join(", ")}>{pluginDetail(plugin)}</span>}
                className="plugin-row"
              >
                {plugin.level !== "full" && <StatusPill label={plugin.level === "partial" ? t("部分支持") : t("不支持")} tone={plugin.level === "partial" ? "warning" : "danger"} />}
                <span className="row-actions">
                  <MoreMenu
                    label={t("更多")}
                    disabled={Boolean(busy)}
                    items={[{
                      label: t("删除插件"),
                      icon: Trash2,
                      danger: true,
                      confirm: t("确认删除 {name}", { name: plugin.name }),
                      onSelect: () => change("plugin", () => desktop.removeMitmPlugin(plugin.id)),
                    }]}
                  />
                </span>
                <Switch
                  label={plugin.name}
                  checked={plugin.enabled}
                  disabled={Boolean(busy)}
                  onChange={() => change("plugin", () => desktop.setMitmPluginEnabled(plugin.id, !plugin.enabled))}
                />
              </Row>
            ))}
          </div>
        )}
      </Card>

      {!linux && (
        <Card title={t("根证书")} description={t("HTTPS 解密的根证书会自动生成并安装。只有要让系统信任你自己的证书时才需要在这里填写。")}>
          <Row icon={KeyRound} color="gray" label={t("证书 PEM 路径")}>
            <input className="inline-input" aria-label={t("证书 PEM 路径")} placeholder={"C:\\certs\\ca.pem"} value={certificatePath} onChange={(event) => setCertificatePath(event.target.value)} />
            <button className="secondary compact" disabled={Boolean(busy) || !certificatePath.trim()} onClick={() => void run("certificate", () => desktop.installCertificate(certificatePath.trim()))}>{t("安装证书")}</button>
            <MoreMenu
              label={t("更多")}
              disabled={Boolean(busy)}
              items={[{ label: t("移除证书"), danger: true, confirm: t("确认移除证书"), onSelect: () => void run("certificate", desktop.removeCertificate) }]}
            />
          </Row>
        </Card>
      )}
    </div>
  );
}

function pluginDetail(plugin: MitmPlugin): string {
  const hosts = plugin.hostnames.length > 0 ? plugin.hostnames.slice(0, 3).join(", ") : t("没有解密域名");
  const more = plugin.hostnames.length > 3 ? t("等 {count} 个域名", { count: plugin.hostnames.length }) : "";
  const scripts = plugin.scriptCount > 0 ? t("{count} 个脚本", { count: plugin.scriptCount }) : t("无脚本");
  return [hosts + (more ? " " + more : ""), scripts].join(" · ");
}
