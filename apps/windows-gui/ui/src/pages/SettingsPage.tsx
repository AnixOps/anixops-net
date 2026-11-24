import { Ban, BadgeCheck, Bell, BellRing, Bug, CloudOff, Cpu, FileCheck, Gauge, Timer, HardDrive, HeartPulse, Languages, LifeBuoy, Lock, Network, ScrollText, PlugZap, Power, RefreshCw, RotateCcw, ShieldCheck, SlidersHorizontal, SunMoon, Wifi, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { desktop, HealthStatus, inProcessEngine, NotificationSettings, OnDemandSettings, Preferences, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Appearance, appearances } from "../theme";
import { factLabel } from "../labels";
import { Card, Loading, MoreMenu, Row, Run, Segmented, StatusDot, TileColor, ToggleRow } from "../ui";
import { currentLanguage, LanguageChoice, languages, t } from "../i18n";
import { PrivacyViewer, TermsViewer, termsVersion } from "../terms";
import { AppMode, modes } from "../mode";
import { OnDemandCard } from "./OnDemandCard";
import { BackBar } from "./CaptureViews";

type StartupKey = "startAfterLogin" | "autoConnect" | "autoRecoverCore" | "autoSubscriptionRefresh" | "autoSelectFastestNode";

const startupOptions: Array<{ key: StartupKey; label: string; hint?: string }> = [
  { key: "startAfterLogin", label: "登录 Windows 后启动" },
  { key: "autoConnect", label: "启动后自动连接" },
  { key: "autoRecoverCore", label: "内核意外退出时自动恢复一次", hint: "只恢复由本应用启动的内核" },
  { key: "autoSubscriptionRefresh", label: "每小时自动更新订阅" },
  { key: "autoSelectFastestNode", label: "每 30 分钟自动选择最快节点" },
];

/** The sub-views of the settings page: everyday items stay on the first screen, the rest sits one level down. */
type SettingsView = "main" | "advanced" | "privacy" | "notifications" | "wifi";

/** The toggles that stay on the first screen; the other startup options are about recovery and live under Advanced. */
const everydayStartup: StartupKey[] = ["startAfterLogin", "autoConnect", "autoSubscriptionRefresh"];

export function SettingsPage({ snapshot, busy, run, appearance, setAppearance, language, setLanguage, mode, setMode }: {
  snapshot: RuntimeSnapshot | null;
  busy: string | null;
  run: Run;
  appearance: Appearance;
  setAppearance: (value: Appearance) => void;
  language: LanguageChoice;
  setLanguage: (value: LanguageChoice) => void;
  mode: AppMode;
  setMode: (value: AppMode) => void;
}) {
  // The view lives in the page rather than in App: the window title stays "设置" and a back bar names the sub-page,
  // so no other page needs to know about it. App remounts the page on every navigation, which resets it to the list.
  const [view, setView] = useState<SettingsView>("main");
  const [wifi, setWifi] = useState<OnDemandSettings | null>(null);
  const hasWifiRules = Boolean(snapshot) && snapshot?.platform !== "linux";
  // The first screen summarises the Wi-Fi rules; read again on every return, since the sub-view may have changed them.
  useEffect(() => {
    if (view !== "main" || !hasWifiRules) return;
    let active = true;
    void desktop.onDemandSettings().then(
      (settings) => { if (active) setWifi(settings && Array.isArray(settings.wifi) ? settings : null); },
      () => undefined,
    );
    return () => { active = false; };
  }, [view, hasWifiRules]);
  // Escape goes back, unless it is closing something that sits on top (a dialog, a menu or a list).
  useEffect(() => {
    if (view === "main") return;
    const back = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.querySelector('[role="dialog"], [role="menu"], [role="listbox"]')) return;
      setView("main");
    };
    // Capture phase: a popup that closes itself on this key is still in the document when the check runs.
    window.addEventListener("keydown", back, true);
    return () => window.removeEventListener("keydown", back, true);
  }, [view]);
  if (!snapshot) return <Loading />;
  const save = (changes: Partial<Preferences>) => void run("preferences", () => desktop.savePreferences(preferencesFrom(snapshot, changes)));
  // The Linux client runs the engine in-process and has no service or driver.
  const linux = inProcessEngine(snapshot);
  // Android asks for the VPN consent in the window, so it neither starts after login nor connects on its own.
  const android = snapshot.platform === "android";
  const available = android
    ? []
    : linux
      ? startupOptions.filter((option) => option.key === "startAfterLogin" || option.key === "autoConnect")
      : startupOptions;
  const startup = available.filter((option) => everydayStartup.includes(option.key));
  const recovery = available.filter((option) => !everydayStartup.includes(option.key));
  const toggleOptions = (options: typeof startupOptions) => options.map((option) => (
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
  ));
  const back = () => setView("main");

  if (view === "wifi") {
    return <SubPage title={t("Wi-Fi 规则")} onBack={back}><OnDemandCard busy={busy} run={run} android={android} /></SubPage>;
  }
  if (view === "notifications") {
    return <SubPage title={t("通知")} onBack={back}><NotificationsCard busy={busy} run={run} /></SubPage>;
  }
  if (view === "privacy") {
    return <SubPage title={t("隐私与诊断")} onBack={back}><PrivacyCard busy={busy} run={run} /></SubPage>;
  }
  if (view === "advanced") {
    return (
      <SubPage title={t("高级")} onBack={back}>
        {linux
          ? !android && <LinuxProxyCard busy={busy} run={run} snapshot={snapshot} />
          : <Card title={t("运行方式")} description={t("内核在客户端窗口进程里运行，关闭窗口会断开连接。")}>
            <ToggleRow
              icon={Network}
              color="purple"
              label={t("TUN 模式")}
              hint={t("通过虚拟网卡接管全部流量，包括不使用系统代理的应用。")}
              checked={Boolean(snapshot.tunModeEnabled)}
              disabled={Boolean(busy)}
              onChange={() => void run("TUN", () => desktop.setTunMode(!snapshot.tunModeEnabled))}
            />
          </Card>}

        {recovery.length > 0 && <Card title={t("自动化")}>{toggleOptions(recovery)}</Card>}

        <Card title={t("网络")}>
          <ToggleRow
            icon={Ban}
            color="red"
            label={t("屏蔽 QUIC")}
            hint={t("拒绝发往 UDP 443 端口的 QUIC（HTTP/3）流量，浏览器和应用改用 TCP。")}
            checked={Boolean(snapshot.blockQuic)}
            disabled={Boolean(busy) || snapshot.blockQuic === undefined}
            onChange={() => void run("QUIC", () => desktop.setBlockQuic(!snapshot.blockQuic))}
          />
        </Card>

        <Card title={t("代理内核")} description={t("内核随应用一起运行，无需单独安装。")}>
          <Row icon={Cpu} color="gray" label={t("代理内核")} value={<><StatusDot tone={snapshot.core.tone} />{factLabel(snapshot.core.label)}</>} />
          <Row icon={ShieldCheck} color="green" label={t("系统代理")} value={<><StatusDot tone={snapshot.proxy.tone} />{factLabel(snapshot.proxy.label)}</>} />
          <Row icon={BadgeCheck} color="gray" label={t("校验配置")}>
            <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("validate", desktop.validate)}>{t("校验")}</button>
          </Row>
          <Row icon={RotateCcw} color="gray" label={t("重启内核")}>
            <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("restart", desktop.restart)}>{t("重启")}</button>
          </Row>
        </Card>

        <MetricsCard busy={busy} run={run} />

        {!linux && <SystemCard busy={busy} run={run} />}
      </SubPage>
    );
  }

  return (
    <div className="page-stack" data-settings-first-screen>
      <Card title={t("通用")}>
        <Row icon={Bug} color="pink" label={t("使用方式")} hint={t(modes.find((option) => option.value === mode)?.hint ?? "")}>
          <Segmented label={t("使用方式")} options={modes.map((option) => ({ value: option.value, label: t(option.label) }))} value={mode} onChange={setMode} />
        </Row>
        <Row icon={SunMoon} color="indigo" label={t("外观")}>
          <Segmented label={t("外观")} options={appearances.map((option) => ({ ...option, label: t(option.label) }))} value={appearance} onChange={setAppearance} />
        </Row>
        <Row icon={Languages} color="sky" label={t("语言")}>
          <Segmented label={t("语言")} options={languages.map((option) => ({ ...option, label: t(option.label) }))} value={language} onChange={setLanguage} />
        </Row>
      </Card>

      {startup.length > 0 && <Card title={t("启动与自动化")}>{toggleOptions(startup)}</Card>}

      <Card>
        {hasWifiRules && (
          <Row
            icon={Wifi}
            color="sky"
            label={t("Wi-Fi 规则")}
            value={wifi ? (wifi.enabled ? t("{count} 条规则", { count: wifi.wifi.length }) : t("已关闭")) : undefined}
            onClick={() => setView("wifi")}
          />
        )}
        <Row icon={Bell} color="red" label={t("通知")} onClick={() => setView("notifications")} />
      </Card>

      <Card>
        <Row icon={Lock} color="green" label={t("隐私与诊断")} onClick={() => setView("privacy")} />
        <Row icon={SlidersHorizontal} color="gray" label={t("高级")} onClick={() => setView("advanced")} />
      </Card>

      <AboutCard />
    </div>
  );
}

/**
 * A sub-page of the settings: a way back above its title, then its cards. The title takes focus on arrival, so the
 * keyboard does not start from the button that was just replaced.
 */
function SubPage({ title, onBack, children }: { title: string; onBack: () => void; children: ReactNode }) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, []);
  return (
    <div className="page-stack">
      <BackBar label={t("设置")} onBack={onBack} />
      <h2 className="subpage-title" tabIndex={-1} ref={heading}>{title}</h2>
      {children}
    </div>
  );
}

/** The user agreement and privacy policy this build ships with, to read again at any time. */
function AboutCard() {
  const [reading, setReading] = useState(false);
  const [readingPrivacy, setReadingPrivacy] = useState(false);
  return (
    <Card title={t("关于")}>
      <Row icon={ScrollText} color="gray" label={t("用户协议")} hint={t("用户协议、禁止用途与免责声明，版本 {version}", { version: termsVersion })}>
        <button className="secondary compact" aria-label={t("查看用户协议")} onClick={() => setReading(true)}>{t("查看")}</button>
      </Row>
      <Row icon={ShieldCheck} color="green" label={t("隐私政策")} hint={t("我们收集什么、数据保存在哪里、如何删除")}>
        <button className="secondary compact" aria-label={t("查看隐私政策")} onClick={() => setReadingPrivacy(true)}>{t("查看")}</button>
      </Row>
      {reading && <TermsViewer onClose={() => setReading(false)} />}
      {readingPrivacy && <PrivacyViewer onClose={() => setReadingPrivacy(false)} />}
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

/** The health status, read again every few seconds; each view that shows it owns one poll. */
function useHealth() {
  const [health, setHealth] = useState<HealthStatus | null>(null);
  useEffect(() => {
    let active = true;
    const load = () => void desktop.health().then((status) => { if (active) setHealth(status); }, () => undefined);
    load();
    // The install check finishes in the background shortly after start.
    const timer = window.setInterval(load, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  return [health, setHealth] as const;
}

/** Crash reports (off deletes the kept ones), the install check and the cold start. */
function PrivacyCard({ busy, run }: { busy: string | null; run: Run }) {
  const [health, setHealth] = useHealth();
  if (!health) return null;
  const integrity = integrityLabels[health.integrity] ?? integrityLabels.checking;
  const toggle = () => void run("crash reports", async () => {
    const result = await desktop.setCrashReports(!health.crashReports);
    setHealth(await desktop.health());
    return result;
  });
  return (
    <Card>
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
      <Row icon={FileCheck} color="green" label={t("安装完整性")} hint={health.integrityDetail ?? undefined} value={<><StatusDot tone={integrity.tone} />{t(integrity.label)}</>} />
      {health.coldStartMs != null && <Row icon={Timer} color="gray" label={t("启动耗时")} value={`${health.coldStartMs} ms`} />}
    </Card>
  );
}

/** The local Prometheus endpoint switch, under Advanced. */
function MetricsCard({ busy, run }: { busy: string | null; run: Run }) {
  const [health, setHealth] = useHealth();
  if (!health) return null;
  const toggleMetrics = () => void run("metrics", async () => {
    const result = await desktop.setMetrics(!health.metricsEnabled);
    setHealth(await desktop.health());
    return result;
  });
  const metricsHint = !health.metricsEnabled
    ? t("默认关闭。打开后在本机回环地址提供指标，其中有策略组、所选节点和规则集的名称。")
    : health.metricsAddress
      ? t("正在监听 {address}，只接受本机回环地址的请求。指标里有策略组、所选节点和规则集的名称。", { address: health.metricsAddress })
      : t("端口被占用");
  return (
    <Card>
      <ToggleRow
        icon={Gauge}
        color="indigo"
        label={t("Prometheus 指标")}
        hint={metricsHint}
        checked={Boolean(health.metricsEnabled)}
        disabled={Boolean(busy)}
        onChange={toggleMetrics}
      />
    </Card>
  );
}

/** Linux desktop: TUN mode through the polkit helper, or the desktop's proxy settings pointed at the local listener. */
function LinuxProxyCard({ busy, run, snapshot }: { busy: string | null; run: Run; snapshot: RuntimeSnapshot }) {
  const tun = Boolean(snapshot.tunModeEnabled);
  return (
    <Card
      title={t("运行方式")}
      description={tun
        ? t("连接时请求一次管理员授权，创建虚拟网卡并接管全部流量；应用本身不以管理员身份运行。")
        : t("连接时修改 GNOME 或 KDE 的系统代理，断开或退出时恢复。")}
    >
      <ToggleRow
        icon={Network}
        color="purple"
        label={t("TUN 模式")}
        hint={t("通过虚拟网卡接管全部流量。切换前需断开连接。")}
        checked={tun}
        disabled={Boolean(busy)}
        onChange={() => void run("TUN", () => desktop.setTunMode(!tun))}
      />
      <Row icon={LifeBuoy} color="gray" label={t("恢复网络设置")} hint={t("只恢复本应用这次改过、且仍归它管理的代理设置。")}>
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("network recovery", desktop.restoreNetworkSettings)}>{t("恢复")}</button>
      </Row>
    </Card>
  );
}

function SystemCard({ busy, run }: { busy: string | null; run: Run }) {
  const [driverPath, setDriverPath] = useState("");
  return (
    <Card title={t("系统与网络")} description={t("这些操作会改动系统设置，仅填写已确认的本地文件。")}>
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

type NotificationKey = "scripts" | "connection" | "subscriptions";

const notificationOptions: Array<{ key: NotificationKey; label: string; hint: string; icon: LucideIcon; color: TileColor }> = [
  { key: "scripts", label: "脚本通知", hint: "插件脚本发出的通知，例如签到结果。", icon: BellRing, color: "orange" },
  { key: "connection", label: "连接意外断开", hint: "内核意外退出或 VPN 被其他应用占用时提醒。", icon: Bell, color: "red" },
  { key: "subscriptions", label: "订阅自动更新失败", hint: "自动更新订阅失败时提醒。", icon: CloudOff, color: "sky" },
];

/** The system notification switches, as in the iOS settings. */
function NotificationsCard({ busy, run }: { busy: string | null; run: Run }) {
  const [settings, setSettings] = useState<NotificationSettings | null>(null);
  useEffect(() => {
    void desktop.notificationSettings(currentLanguage()).then(setSettings).catch(() => setSettings(null));
  }, []);
  if (!settings) return null;
  const toggle = (key: NotificationKey) => void run("notifications", async () => {
    const next = await desktop.notificationSettings(currentLanguage(), { ...settings, [key]: !settings[key] });
    setSettings(next);
    return { message: next[key] ? t("已打开这类通知") : t("已关闭这类通知") };
  });
  return (
    <Card title={t("通知")} description={t("通过系统通知提醒。")}>
      {notificationOptions.map((option) => (
        <ToggleRow
          key={option.key}
          icon={option.icon}
          color={option.color}
          label={t(option.label)}
          hint={t(option.hint)}
          checked={settings[option.key]}
          disabled={Boolean(busy)}
          onChange={() => toggle(option.key)}
        />
      ))}
    </Card>
  );
}
