import { Ban, ClipboardPaste, Download, Eye, FileUp, KeyRound, LockKeyhole, Puzzle, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { DebugMapping, desktop, inProcessEngine, MitmPlugin, MitmPluginDetails, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Card, EmptyState, MoreMenu, Row, Run, StatusPill, Switch, ToggleRow } from "../ui";
import { t } from "../i18n";
import { PhoneCaptureCards } from "./PhoneCaptureCards";
import { useDecryptionConsent } from "../decryption-consent";
import { MappingEditor, MappingsCard } from "./DebugTools";

export function ExtensionsPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const linux = inProcessEngine(snapshot);
  const [certificatePath, setCertificatePath] = useState("");
  const [pluginUrl, setPluginUrl] = useState("");
  const [plugins, setPlugins] = useState<MitmPlugin[]>([]);
  const consent = useDecryptionConsent();
  const [mapping, setMapping] = useState<DebugMapping | null>(null);
  const [pasting, setPasting] = useState(false);
  const [pastedText, setPastedText] = useState("");
  const [inspected, setInspected] = useState<{ plugin: MitmPlugin; details: MitmPluginDetails } | null>(null);
  const filePicker = useRef<HTMLInputElement>(null);

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

  const importText = (text: string, source: string) => void run("plugin", () => desktop.importMitmPluginText(text, source)).then(() => {
    setPasting(false);
    setPastedText("");
    loadPlugins();
  });
  const importFile = (file: File | undefined) => {
    if (!file) return;
    void file.text().then((text) => importText(text, file.name), (error: unknown) => run("plugin", () => Promise.reject(error)));
  };
  const inspect = (plugin: MitmPlugin) => void desktop.mitmPluginDetails(plugin.id).then(
    (details) => setInspected({ plugin, details }),
    (error: unknown) => run("plugin", () => Promise.reject(error)),
  );

  if (mapping) {
    return <MappingEditor initial={mapping} busy={busy} run={run} onDone={() => setMapping(null)} />;
  }

  return (
    <div className="page-stack">
      {consent.dialog}
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
          onChange={() => (decrypting ? change("mitm", desktop.disableHttpsMitm) : consent.guard(() => change("mitm", desktop.enableHttpsMitm)))}
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

      {snapshot?.platform !== "android" && <PhoneCaptureCards busy={busy} run={run} />}

      <Card
        title={t("插件")}
        description={t("支持 Loon、Surge 和 Quantumult X 的插件。导入时会一并下载插件里的脚本，导入即表示允许这些脚本运行。")}
        actions={<>
          {plugins.length > 0 && <span className="count">{t("{count} 个插件", { count: plugins.length })}</span>}
          <MoreMenu
            label={t("更多导入方式")}
            disabled={Boolean(busy)}
            items={[
              { label: t("从文件导入"), icon: FileUp, onSelect: () => filePicker.current?.click() },
              { label: t("粘贴插件内容"), icon: ClipboardPaste, onSelect: () => setPasting(true) },
            ]}
          />
        </>}
      >
        <input
          ref={filePicker}
          type="file"
          hidden
          accept=".plugin,.sgmodule,.lpx,.conf,.txt,text/plain"
          onChange={(event) => {
            importFile(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
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
        {pasting && (
          <div className="rule-editor">
            <label className="editor-label" htmlFor="pasted-plugin">{t("插件内容")}</label>
            <textarea
              id="pasted-plugin"
              rows={8}
              spellCheck={false}
              placeholder={"#!name = …\n[Rewrite]\n…\n[MITM]\nhostname = …"}
              value={pastedText}
              onChange={(event) => setPastedText(event.target.value)}
            />
            <div className="button-row">
              <button className="secondary" disabled={Boolean(busy)} onClick={() => { setPasting(false); setPastedText(""); }}>{t("取消")}</button>
              <button className="secondary" disabled={Boolean(busy) || !pastedText.trim()} onClick={() => importText(pastedText, "")}>{t("导入")}</button>
            </div>
          </div>
        )}
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
                      label: t("查看内容"),
                      icon: Eye,
                      onSelect: () => inspect(plugin),
                    }, {
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

      {inspected && <PluginInspector plugin={inspected.plugin} details={inspected.details} close={() => setInspected(null)} />}

      <MappingsCard busy={busy} run={run} onEdit={setMapping} />

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

/** What one plugin holds: the domains it decrypts, its scripts and files, and how much of it the engine runs. */
function PluginInspector({ plugin, details, close }: { plugin: MitmPlugin; details: MitmPluginDetails; close: () => void }) {
  const lines = [
    t("{count} 行支持", { count: details.supportedLines }),
    details.partialLines > 0 ? t("{count} 行部分支持", { count: details.partialLines }) : null,
    details.unsupportedLines > 0 ? t("{count} 行不支持", { count: details.unsupportedLines }) : null,
    details.deferredLines > 0 ? t("{count} 行暂不处理", { count: details.deferredLines }) : null,
  ].filter(Boolean).join(" · ");
  return (
    <Card title={plugin.name} description={plugin.url || t("本地导入")} actions={<button className="link-button" onClick={close}>{t("收起")}</button>}>
      <Row label={t("规则行")} value={lines} />
      <Row label={t("解密域名")} hint={plugin.hostnames.join(", ") || t("没有解密域名")} />
      {details.excludedHostnames.length > 0 && <Row label={t("排除的域名")} hint={details.excludedHostnames.join(", ")} />}
      <Row label={t("脚本")} hint={details.scripts.length > 0 ? <span className="url-list">{details.scripts.map((url) => <span key={url}>{url}</span>)}</span> : t("无脚本")} />
      {details.resources.length > 0 && <Row label={t("数据文件")} hint={<span className="url-list">{details.resources.map((url) => <span key={url}>{url}</span>)}</span>} />}
      {details.report && <pre className="plugin-report">{details.report}</pre>}
    </Card>
  );
}
