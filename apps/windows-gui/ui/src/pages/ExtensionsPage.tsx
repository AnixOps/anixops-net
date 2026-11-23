import { ClipboardPaste, Download, Eye, FileUp, KeyRound, LockKeyhole, Puzzle, ShieldCheck, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { DebugMapping, desktop, inProcessEngine, MitmPlugin, MitmPluginDetails, MitmRootStatus, OperationResult, RuntimeSnapshot } from "../bridge";
import { Card, EmptyState, MoreMenu, Row, Run, StatusPill, Switch, ToggleRow } from "../ui";
import { t } from "../i18n";
import { PhoneCaptureCards } from "./PhoneCaptureCards";
import { useDecryptionConsent } from "../decryption-consent";
import { MappingEditor, MappingsCard } from "./DebugTools";
import { DecryptHostsCard } from "./DecryptHosts";
import { TrafficRedirectsCard } from "./TrafficRedirects";
import { decryptsBroadly } from "../plugin-review";

/** How many plugins show before the rest fold behind "显示全部". */
const collapsedPluginCount = 5;

export function ExtensionsPage({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const linux = inProcessEngine(snapshot);
  const [certificatePath, setCertificatePath] = useState("");
  const [pluginUrl, setPluginUrl] = useState("");
  const [plugins, setPlugins] = useState<MitmPlugin[]>([]);
  const consent = useDecryptionConsent();
  const [mapping, setMapping] = useState<DebugMapping | null>(null);
  const [pasting, setPasting] = useState(false);
  // A long plugin list folds to its first few; the choice is for this visit only.
  const [allPlugins, setAllPlugins] = useState(false);
  const [pastedText, setPastedText] = useState("");
  // `review`: opened before the plugin is turned on, with the button that turns it on.
  const [inspected, setInspected] = useState<{ plugin: MitmPlugin; details: MitmPluginDetails; review: boolean } | null>(null);
  const filePicker = useRef<HTMLInputElement>(null);

  const loadPlugins = useCallback(() => {
    desktop.mitmPlugins().then(setPlugins).catch(() => setPlugins([]));
  }, []);
  useEffect(loadPlugins, [loadPlugins]);

  // Null where the platform has nothing to show (Android installs the root in its own settings).
  const [root, setRoot] = useState<MitmRootStatus | null>(null);
  const loadRoot = useCallback(() => {
    desktop.mitmRootStatus().then(setRoot).catch(() => setRoot(null));
  }, []);
  useEffect(() => {
    loadRoot();
    // Coming back from the system's certificate window is when trust usually changes.
    window.addEventListener("focus", loadRoot);
    return () => window.removeEventListener("focus", loadRoot);
  }, [loadRoot]);

  const decrypting = Boolean(snapshot?.httpsDecryptionEnabled);
  const locked = Boolean(busy) || !snapshot;
  const change = (operation: string, task: () => Promise<{ message: string }>) => void run(operation, task).then(() => { loadPlugins(); loadRoot(); });
  const inspect = (plugin: MitmPlugin, review = false) => void desktop.mitmPluginDetails(plugin.id).then(
    (details) => setInspected({ plugin, details, review }),
    (error: unknown) => run("plugin", () => Promise.reject(error)),
  );
  /**
   * Imports a plugin, which the backend saves off, then shows what it holds with the button that turns it on: nothing
   * in it runs before the user has seen its domains, rules and scripts.
   */
  const importThenReview = (task: () => Promise<OperationResult>, done: () => void) => {
    let imported: OperationResult | null = null;
    void run("plugin", async () => {
      imported = await task();
      return imported;
    }).then(async () => {
      const result = imported as OperationResult | null;
      if (!result) {
        // A failed import can still have turned a plugin off (its new scripts were being written), so show the list as saved.
        loadPlugins();
        return;
      }
      done();
      const list = await desktop.mitmPlugins().catch(() => [] as MitmPlugin[]);
      setPlugins(list);
      const plugin = list.find((candidate) => candidate.id === result.params?.id);
      if (plugin && !plugin.enabled) inspect(plugin, true);
    });
  };
  const importPlugin = () => {
    const url = pluginUrl.trim();
    if (!url) return;
    importThenReview(() => desktop.importMitmPlugin(url), () => setPluginUrl(""));
  };

  const importText = (text: string, source: string) => importThenReview(() => desktop.importMitmPluginText(text, source), () => {
    setPasting(false);
    setPastedText("");
  });
  const importFile = (file: File | undefined) => {
    if (!file) return;
    void file.text().then((text) => importText(text, file.name), (error: unknown) => run("plugin", () => Promise.reject(error)));
  };
  // Turning a plugin on goes through its contents first; turning it off is immediate.
  const togglePlugin = (plugin: MitmPlugin) => plugin.enabled
    ? change("plugin", () => desktop.setMitmPluginEnabled(plugin.id, false))
    : inspect(plugin, true);
  const enableReviewed = (plugin: MitmPlugin) => {
    setInspected(null);
    change("plugin", () => desktop.setMitmPluginEnabled(plugin.id, true));
  };

  if (mapping) {
    return <MappingEditor initial={mapping} busy={busy} run={run} onDone={() => setMapping(null)} />;
  }

  return (
    <div className="page-stack">
      {consent.dialog}
      <Card
        title={t("HTTPS 解密")}
        description={t("只解密下面“解密域名”、插件和映射里列出的域名，其余网站原样转发。根证书在本机生成，需要你按系统提示手动信任，App 不会自动信任。")}
      >
        {root && (
          <Row
            icon={ShieldCheck}
            color={root.trusted ? "green" : "gray"}
            label={t("根证书")}
            hint={root.trusted ? (linux ? t("已由你手动信任。关闭解密后仍保留，不再使用时请移除信任") : t("已由你手动信任。关闭解密后仍保留，不再使用时请在 certmgr.msc 的“受信任的根证书颁发机构”里删除")) : linux ? t("点“信任根证书”后，系统会请你授权写入浏览器和系统证书库") : t("点“信任根证书”会打开 Windows 证书窗口：选“安装证书”→“当前用户”→“将所有的证书都放入下列存储”→“受信任的根证书颁发机构”，再确认安全警告")}
          >
            <StatusPill label={root.trusted ? t("已信任") : t("未信任")} tone={root.trusted ? "success" : "warning"} />
            {!root.trusted && (
              <button className="secondary compact" disabled={locked} onClick={() => change("certificate", desktop.openMitmRoot)}>{t("信任根证书")}</button>
            )}
            {root.trusted && linux && (
              <MoreMenu
                label={t("更多")}
                disabled={locked}
                items={[{ label: t("移除信任"), danger: true, confirm: t("确认移除根证书的信任"), onSelect: () => change("certificate", desktop.removeMitmRootTrust) }]}
              />
            )}
          </Row>
        )}
        <ToggleRow
          icon={LockKeyhole}
          color="pink"
          label={t("解密 HTTPS 流量")}
          hint={t("插件和脚本需要它才能改写网页请求")}
          checked={decrypting}
          disabled={locked}
          onChange={() => (decrypting ? change("mitm", desktop.disableHttpsMitm) : consent.guard(() => change("mitm", desktop.enableHttpsMitm)))}
        />
      </Card>

      <DecryptHostsCard busy={busy} run={run} />

      {snapshot?.platform !== "android" && <PhoneCaptureCards busy={busy} run={run} />}

      <Card
        title={t("插件")}
        description={t("兼容常见插件格式。导入后先关着：查看它解密的域名、改写规则和脚本，确认后再打开，脚本才会运行。")}
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
            {(allPlugins ? plugins : plugins.slice(0, collapsedPluginCount)).map((plugin) => (
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
                  onChange={() => togglePlugin(plugin)}
                />
              </Row>
            ))}
            {plugins.length > collapsedPluginCount && (
              <div className="button-row">
                <button className="link-button more" aria-expanded={allPlugins} onClick={() => setAllPlugins(!allPlugins)}>
                  {allPlugins ? t("收起") : t("显示全部 {count} 个插件", { count: plugins.length })}
                </button>
              </div>
            )}
          </div>
        )}
      </Card>

      {inspected && (
        <PluginInspector
          plugin={inspected.plugin}
          details={inspected.details}
          review={inspected.review}
          busy={Boolean(busy)}
          enable={() => enableReviewed(inspected.plugin)}
          close={() => setInspected(null)}
        />
      )}

      <MappingsCard busy={busy} run={run} onEdit={setMapping} />

      <TrafficRedirectsCard busy={busy} run={run} platform={snapshot?.platform} />

      {!linux && (
        <Card title={t("自定义证书")} description={t("要让 Windows 信任你自己的证书时填写路径。证书会在 Windows 证书窗口中打开，由你手动安装。")}>
          <Row icon={KeyRound} color="gray" label={t("证书 PEM 路径")}>
            <input className="inline-input" aria-label={t("证书 PEM 路径")} placeholder={"C:\\certs\\ca.pem"} value={certificatePath} onChange={(event) => setCertificatePath(event.target.value)} />
            <button className="secondary compact" disabled={Boolean(busy) || !certificatePath.trim()} onClick={() => void run("certificate", () => desktop.installCertificate(certificatePath.trim()))}>{t("打开证书")}</button>
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

/**
 * What one plugin holds: the domains it decrypts, its scripts and files, and how much of it the engine runs. In
 * `review` it is shown before the plugin is turned on, says what turning it on allows and has the button that does.
 */
function PluginInspector({ plugin, details, review, busy, enable, close }: {
  plugin: MitmPlugin;
  details: MitmPluginDetails;
  review: boolean;
  busy: boolean;
  enable: () => void;
  close: () => void;
}) {
  const lines = [
    t("{count} 行支持", { count: details.supportedLines }),
    details.partialLines > 0 ? t("{count} 行部分支持", { count: details.partialLines }) : null,
    details.unsupportedLines > 0 ? t("{count} 行不支持", { count: details.unsupportedLines }) : null,
    details.deferredLines > 0 ? t("{count} 行暂不处理", { count: details.deferredLines }) : null,
  ].filter(Boolean).join(" · ");
  return (
    <Card title={plugin.name} description={plugin.url || t("本地导入")} actions={<button className="link-button" onClick={close}>{review ? t("暂不打开") : t("收起")}</button>}>
      {review && (
        <p className="card-note" role="note">
          {t("打开后，这个插件会解密下面的域名、按规则改写请求和响应，并运行下面的脚本。脚本可以查看和改写解密后的流量，也可以自行联网。只打开你信任的来源。")}
        </p>
      )}
      {review && decryptsBroadly(plugin.hostnames) && (
        <p className="card-note danger" role="alert">{t("这个插件要解密所有网站（通配 *），包括银行、邮箱等敏感网站的流量。")}</p>
      )}
      <Row label={t("规则行")} value={lines} />
      <Row label={t("解密域名")} hint={plugin.hostnames.join(", ") || t("没有解密域名")} />
      {details.excludedHostnames.length > 0 && <Row label={t("排除的域名")} hint={details.excludedHostnames.join(", ")} />}
      <Row label={t("脚本")} hint={details.scripts.length > 0 ? <span className="url-list">{details.scripts.map((url) => <span key={url}>{url}</span>)}</span> : t("无脚本")} />
      {details.resources.length > 0 && <Row label={t("数据文件")} hint={<span className="url-list">{details.resources.map((url) => <span key={url}>{url}</span>)}</span>} />}
      {details.report && <pre className="plugin-report">{details.report}</pre>}
      {review && (
        <div className="button-row">
          <button className="secondary" disabled={busy} onClick={close}>{t("暂不打开")}</button>
          <button className="primary" disabled={busy} onClick={enable}>{t("打开插件")}</button>
        </div>
      )}
    </Card>
  );
}
