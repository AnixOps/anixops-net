import { AppWindow, BadgeCheck, Bug, Cpu, FileCheck, Gauge, Timer, HardDrive, HeartPulse, Languages, LifeBuoy, Network, ScrollText, PlugZap, Power, RefreshCw, RotateCcw, ServerCog, ShieldCheck, SunMoon, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, HealthStatus, inProcessEngine, Preferences, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Appearance, appearances } from "../theme";
import { factLabel } from "../labels";
import { Card, Loading, MoreMenu, Row, Run, Segmented, StatusDot, TileColor, ToggleRow } from "../ui";
import { LanguageChoice, languages, t } from "../i18n";
import { TermsViewer, termsVersion } from "../terms";

type StartupKey = "startAfterLogin" | "autoConnect" | "autoRecoverCore" | "autoSubscriptionRefresh" | "autoSelectFastestNode";

const startupOptions: Array<{ key: StartupKey; label: string; hint?: string }> = [
  { key: "startAfterLogin", label: "登录 Windows 后启动" },
  { key: "autoConnect", label: "启动后自动连接" },
  { key: "autoRecoverCore", label: "内核意外退出时自动恢复一次", hint: "只恢复由本应用启动的内核" },
  { key: "autoSubscriptionRefresh", label: "每小时自动更新订阅" },
  { key: "autoSelectFastestNode", label: "每 30 分钟自动选择最快节点" },
];

export function SettingsPage({ snapshot, busy, run, appearance, setAppearance, language, setLanguage }: {
  snapshot: RuntimeSnapshot | null;
  busy: string | null;
  run: Run;
  appearance: Appearance;
  setAppearance: (value: Appearance) => void;
  language: LanguageChoice;
  setLanguage: (value: LanguageChoice) => void;
}) {
  if (!snapshot) return <Loading />;
  const save = (changes: Partial<Preferences>) => void run("preferences", () => desktop.savePreferences(preferencesFrom(snapshot, changes)));
  // The Linux client runs the engine in-process and has no service or driver.
  const linux = inProcessEngine(snapshot);
  // Android asks for the VPN consent in the window, so it neither starts after login nor connects on its own.
  const android = snapshot.platform === "android";
  const startup = android
    ? []
    : linux
      ? startupOptions.filter((option) => option.key === "startAfterLogin" || option.key === "autoConnect")
      : startupOptions;

  return (
    <div className="page-stack">
      <Card title={t("通用")}>
        <Row icon={SunMoon} color="indigo" label={t("外观")}>
          <Segmented label={t("外观")} options={appearances.map((option) => ({ ...option, label: t(option.label) }))} value={appearance} onChange={setAppearance} />
        </Row>
        <Row icon={Languages} color="sky" label={t("语言")}>
          <Segmented label={t("语言")} options={languages.map((option) => ({ ...option, label: t(option.label) }))} value={language} onChange={setLanguage} />
        </Row>
      </Card>

      {startup.length > 0 && <Card title={t("启动与自动化")}>
        {startup.map((option) => (
          <ToggleRow
            key={option.key}
            icon={startupIcons[option.key].icon}
            color={startupIcons[option.key].color}
            label={linux && option.key === "startAfterLogin" ? t("登录后启动") : t(option.label)}
            hint={option.hint && t(option.hint)}
            checked={snapshot[option.key]}
            disabled={Boolean(busy)}
            onChange={() => save({ [option.key]: !snapshot[option.key] } as Partial<Preferences>)}
          />
        ))}
      </Card>}

      {linux
        ? !android && <LinuxProxyCard busy={busy} run={run} />
        : <Card title={t("运行方式")} description={t("桌面模式由客户端直接运行内核和 HTTPS 解密；Windows 服务模式在后台常驻，关掉窗口也不会断开。")}>
          <Row icon={AppWindow} color="purple" label={t("运行方式")}>
            <Segmented
              label={t("运行方式")}
              options={[{ value: "desktop", label: t("桌面模式") }, { value: "service", label: t("Windows 服务") }]}
              value={snapshot.runtimeMode}
              disabled={Boolean(busy)}
              onChange={(runtimeMode) => save({ runtimeMode })}
            />
          </Row>
          {snapshot.runtimeMode === "desktop" && (
            <ToggleRow
              icon={Network}
              color="purple"
              label={t("TUN 模式")}
              hint={t("用虚拟网卡接管全部流量，不走系统代理的游戏和命令行程序也会经过代理。")}
              checked={Boolean(snapshot.tunModeEnabled)}
              disabled={Boolean(busy)}
              onChange={() => void run("TUN", () => desktop.setTunMode(!snapshot.tunModeEnabled))}
            />
          )}
        </Card>}

      <Card title={t("代理内核")} description={t("内核随应用一起运行，无需单独安装。")}>
        <Row icon={Cpu} color="gray" label={t("代理内核")} value={<><StatusDot tone={snapshot.core.tone} />{factLabel(snapshot.core.label)}</>} />
        <Row icon={ShieldCheck} color="green" label={t("系统代理")} value={<><StatusDot tone={snapshot.proxy.tone} />{factLabel(snapshot.proxy.label)}</>} />
        <Row icon={BadgeCheck} color="gray" label={t("校验配置")}>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("validate", desktop.validate)}>{t("校验")}</button>
        </Row>
        <Row icon={RotateCcw} color="gray" label={snapshot.runtimeMode === "desktop" ? t("重启内核") : t("重启服务")}>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("restart", desktop.restart)}>{t("重启")}</button>
        </Row>
      </Card>

      <HealthCard busy={busy} run={run} />

      {!linux && <ServiceCard snapshot={snapshot} busy={busy} run={run} />}
      {!linux && <SystemCard busy={busy} run={run} />}

      <AboutCard />
    </div>
  );
}

/** The user agreement this build ships with, to read again at any time. */
function AboutCard() {
  const [reading, setReading] = useState(false);
  return (
    <Card title={t("关于")}>
      <Row icon={ScrollText} color="gray" label={t("用户协议")} hint={t("用户协议、禁止用途与免责声明，版本 {version}", { version: termsVersion })}>
        <button className="secondary compact" onClick={() => setReading(true)}>{t("查看")}</button>
      </Row>
      {reading && <TermsViewer onClose={() => setReading(false)} />}
    </Card>
  );
}

const startupIcons: Record<StartupKey, { icon: LucideIcon; color: TileColor }> = {
  startAfterLogin: { icon: Power, color: "green" },
  autoConnect: { icon: PlugZap, color: "green" },
  autoRecoverCore: { icon: HeartPulse, color: "red" },
  autoSubscriptionRefresh: { icon: RefreshCw, color: "orange" },
  autoSelectFastestNode: { icon: Zap, color: "orange" },
};

const integrityLabels: Record<HealthStatus["integrity"], { label: string; tone: "success" | "warning" | "danger" | "neutral" }> = {
  checking: { label: "检查中", tone: "neutral" },
  verified: { label: "完整", tone: "success" },
  unverified: { label: "未校验（开发版本）", tone: "neutral" },
  damaged: { label: "文件缺失或被改动", tone: "danger" },
  invalid: { label: "校验清单无法读取", tone: "warning" },
};

/** Crash reports (off deletes the kept ones), the local Prometheus endpoint, the install check and the cold start. */
function HealthCard({ busy, run }: { busy: string | null; run: Run }) {
  const [health, setHealth] = useState<HealthStatus | null>(null);
  useEffect(() => {
    let active = true;
    const load = () => void desktop.health().then((status) => { if (active) setHealth(status); }, () => undefined);
    load();
    // The install check finishes in the background shortly after start.
    const timer = window.setInterval(load, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  if (!health) return null;
  const integrity = integrityLabels[health.integrity] ?? integrityLabels.checking;
  const toggle = () => void run("crash reports", async () => {
    const result = await desktop.setCrashReports(!health.crashReports);
    setHealth(await desktop.health());
    return result;
  });
  return (
    <Card title={t("诊断与隐私")}>
      <ToggleRow
        icon={Bug}
        color="red"
        label={t("崩溃报告")}
        hint={health.crashUploadConfigured
          ? t("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），下次启动时发送。关闭会删除已保存的报告。")
          : t("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），只保存在本机，生成诊断报告时附上。关闭会删除已保存的报告。")}
        checked={health.crashReports}
        disabled={Boolean(busy)}
        onChange={toggle}
      />
      <Row icon={Gauge} color="indigo" label={t("Prometheus 指标")} hint={t("只监听本机回环地址。")} value={health.metricsAddress ?? (health.metricsError ? t("端口被占用") : t("已关闭"))} />
      <Row icon={FileCheck} color="green" label={t("安装完整性")} hint={health.integrityDetail ?? undefined} value={<><StatusDot tone={integrity.tone} />{t(integrity.label)}</>} />
      {health.coldStartMs != null && <Row icon={Timer} color="gray" label={t("启动耗时")} value={`${health.coldStartMs} ms`} />}
    </Card>
  );
}

/** Linux: the desktop's proxy settings, which connecting points at the local listener. */
function LinuxProxyCard({ busy, run }: { busy: string | null; run: Run }) {
  return (
    <Card title={t("系统代理")} description={t("连接时把 GNOME 或 KDE 的系统代理指向本地端口，断开或退出时改回原来的设置。")}>
      <Row icon={LifeBuoy} color="gray" label={t("恢复网络设置")} hint={t("只恢复本应用这次改过、且仍归它管理的代理设置。")}>
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("network recovery", desktop.restoreNetworkSettings)}>{t("恢复")}</button>
      </Row>
    </Card>
  );
}

function ServiceCard({ snapshot, busy, run }: { snapshot: RuntimeSnapshot; busy: string | null; run: Run }) {
  return (
    <Card title={t("Windows 服务")} description={t("管理已安装的 AnixOps Net 服务。")}>
      <Row icon={ServerCog} color="gray" label={t("Windows 服务")} value={<><StatusDot tone={snapshot.service.tone} />{factLabel(snapshot.service.label)}</>}>
        <ServiceAction state={snapshot.service.label} busy={busy} run={run} />
      </Row>
    </Card>
  );
}

/**
 * The one service action that applies now (actions V1): install when it is missing, start when it is stopped, stop
 * when it runs. When the state is not known (desktop mode does not watch the service) all three are in the menu.
 */
function ServiceAction({ state, busy, run }: { state: string; busy: string | null; run: Run }) {
  const install = () => void run("service", desktop.installService);
  const start = () => void run("service", desktop.startService);
  const stop = () => void run("service", desktop.stopService);
  const disabled = Boolean(busy);
  if (state === "NotInstalled") return <button className="secondary compact" disabled={disabled} onClick={install}>{t("安装")}</button>;
  if (state === "Stopped" || state === "Paused") return <button className="secondary compact" disabled={disabled} onClick={start}>{t("启动")}</button>;
  if (state === "Running") return <button className="secondary compact" disabled={disabled} onClick={stop}>{t("停止")}</button>;
  if (state === "StartPending" || state === "StopPending") return null;
  return (
    <MoreMenu
      label={t("更多")}
      disabled={disabled}
      items={[
        { label: t("安装服务"), onSelect: install },
        { label: t("启动服务"), onSelect: start },
        { label: t("停止服务"), onSelect: stop },
      ]}
    />
  );
}

function SystemCard({ busy, run }: { busy: string | null; run: Run }) {
  const [driverPath, setDriverPath] = useState("");
  return (
    <Card title={t("系统与网络")} description={t("这些操作会改动系统设置，只填写你确认过的本地文件。")}>
      <Row icon={LifeBuoy} color="gray" label={t("恢复网络设置")} hint={t("只恢复本应用这次改过、且仍归它管理的代理设置。")}>
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("network recovery", desktop.restoreNetworkSettings)}>{t("恢复")}</button>
      </Row>
      <Row icon={HardDrive} color="gray" label={t("驱动 INF 路径")}>
        <input className="inline-input" aria-label={t("驱动 INF 路径")} value={driverPath} onChange={(event) => setDriverPath(event.target.value)} />
        <button className="secondary compact" disabled={Boolean(busy) || !driverPath.trim()} onClick={() => void run("driver", () => desktop.installDriver(driverPath.trim()))}>{t("安装驱动")}</button>
        <MoreMenu
          label={t("更多")}
          disabled={Boolean(busy)}
          items={[{ label: t("移除驱动"), danger: true, confirm: t("确认移除驱动"), onSelect: () => void run("driver", desktop.removeDriver) }]}
        />
      </Row>
    </Card>
  );
}
