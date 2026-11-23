import { Ban, BadgeCheck, Bell, BellRing, Bug, CloudOff, Cpu, FileCheck, Gauge, Timer, HardDrive, HeartPulse, Languages, LifeBuoy, Network, ScrollText, PlugZap, Power, RefreshCw, RotateCcw, ShieldCheck, SunMoon, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, HealthStatus, inProcessEngine, NotificationSettings, Preferences, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Appearance, appearances } from "../theme";
import { factLabel } from "../labels";
import { Card, Loading, MoreMenu, Row, Run, Segmented, StatusDot, TileColor, ToggleRow } from "../ui";
import { currentLanguage, LanguageChoice, languages, t } from "../i18n";
import { PrivacyViewer, TermsViewer, termsVersion } from "../terms";
import { AppMode, modes } from "../mode";
import { OnDemandCard } from "./OnDemandCard";

type StartupKey = "startAfterLogin" | "autoConnect" | "autoRecoverCore" | "autoSubscriptionRefresh" | "autoSelectFastestNode";

const startupOptions: Array<{ key: StartupKey; label: string; hint?: string }> = [
  { key: "startAfterLogin", label: "登录 Windows 后启动" },
  { key: "autoConnect", label: "启动后自动连接" },
  { key: "autoRecoverCore", label: "内核意外退出时自动恢复一次", hint: "只恢复由本应用启动的内核" },
  { key: "autoSubscriptionRefresh", label: "每小时自动更新订阅" },
  { key: "autoSelectFastestNode", label: "每 30 分钟自动选择最快节点" },
];

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
        ? !android && <LinuxProxyCard busy={busy} run={run} snapshot={snapshot} />
        : <Card title={t("运行方式")} description={t("内核在客户端窗口进程里运行，关闭窗口会断开连接。")}>
          <ToggleRow
            icon={Network}
            color="purple"
            label={t("TUN 模式")}
            hint={t("用虚拟网卡接管全部流量，不走系统代理的游戏和命令行程序也会经过代理。")}
            checked={Boolean(snapshot.tunModeEnabled)}
            disabled={Boolean(busy)}
            onChange={() => void run("TUN", () => desktop.setTunMode(!snapshot.tunModeEnabled))}
          />
        </Card>}

      {snapshot.platform !== "linux" && <OnDemandCard busy={busy} run={run} android={android} />}

      <NotificationsCard busy={busy} run={run} />

      <Card title={t("网络")}>
        <ToggleRow
          icon={Ban}
          color="red"
          label={t("屏蔽 QUIC")}
          hint={t("拒绝发往 UDP 443 端口的 QUIC（HTTP/3）流量，浏览器和应用会改用 TCP，这样 MITM 插件和按域名分流的规则都能生效。")}
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

      <HealthCard busy={busy} run={run} />

      {!linux && <SystemCard busy={busy} run={run} />}

      <AboutCard />
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
        <button className="secondary compact" onClick={() => setReading(true)}>{t("查看")}</button>
      </Row>
      <Row icon={ShieldCheck} color="green" label={t("隐私政策")} hint={t("我们收集什么、数据保存在哪里、如何删除")}>
        <button className="secondary compact" onClick={() => setReadingPrivacy(true)}>{t("查看")}</button>
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

/** Linux desktop: TUN mode through the polkit helper, or the desktop's proxy settings pointed at the local listener. */
function LinuxProxyCard({ busy, run, snapshot }: { busy: string | null; run: Run; snapshot: RuntimeSnapshot }) {
  const tun = Boolean(snapshot.tunModeEnabled);
  return (
    <Card
      title={t("运行方式")}
      description={tun
        ? t("连接时请求一次管理员授权，创建虚拟网卡并接管全部流量；应用本身不以管理员身份运行。")
        : t("连接时把 GNOME 或 KDE 的系统代理指向本地端口，断开或退出时改回原来的设置。")}
    >
      <ToggleRow
        icon={Network}
        color="purple"
        label={t("TUN 模式")}
        hint={t("用虚拟网卡接管全部流量，不走系统代理的游戏和命令行程序也会经过代理。需要先断开连接才能切换。")}
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

type NotificationKey = "scripts" | "connection" | "subscriptions";

const notificationOptions: Array<{ key: NotificationKey; label: string; hint: string; icon: LucideIcon; color: TileColor }> = [
  { key: "scripts", label: "脚本通知", hint: "插件脚本发出的通知，例如签到结果。", icon: BellRing, color: "orange" },
  { key: "connection", label: "连接意外断开", hint: "内核意外退出或 VPN 被别的应用占用时提醒。", icon: Bell, color: "red" },
  { key: "subscriptions", label: "订阅自动更新失败", hint: "按间隔自动更新订阅失败时提醒，手动更新的结果直接显示在窗口里。", icon: CloudOff, color: "sky" },
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
    <Card title={t("通知")} description={t("通过系统通知提醒，系统里关掉本应用的通知后不会显示。")}>
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
