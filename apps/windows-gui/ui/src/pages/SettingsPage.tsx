import { AppWindow, BadgeCheck, Cpu, HardDrive, HeartPulse, Languages, LifeBuoy, Network, PlugZap, Power, RefreshCw, RotateCcw, ServerCog, ShieldCheck, SunMoon, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState } from "react";
import { desktop, Preferences, preferencesFrom, RuntimeSnapshot } from "../bridge";
import { Appearance, appearances } from "../theme";
import { factLabel } from "../labels";
import { Card, Loading, Row, Run, Segmented, StatusDot, TileColor, ToggleRow } from "../ui";
import { LanguageChoice, languages, t } from "../i18n";

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
  const linux = snapshot.platform === "linux";
  const startup = linux
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

      <Card title={t("启动与自动化")}>
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
      </Card>

      {linux
        ? <LinuxProxyCard busy={busy} run={run} />
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

      {!linux && <ServiceCard snapshot={snapshot} busy={busy} run={run} />}
      {!linux && <SystemCard busy={busy} run={run} />}
    </div>
  );
}

const startupIcons: Record<StartupKey, { icon: LucideIcon; color: TileColor }> = {
  startAfterLogin: { icon: Power, color: "green" },
  autoConnect: { icon: PlugZap, color: "green" },
  autoRecoverCore: { icon: HeartPulse, color: "red" },
  autoSubscriptionRefresh: { icon: RefreshCw, color: "orange" },
  autoSelectFastestNode: { icon: Zap, color: "orange" },
};

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
        <span className="button-group">
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("service", desktop.installService)}>{t("安装")}</button>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("service", desktop.startService)}>{t("启动")}</button>
          <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("service", desktop.stopService)}>{t("停止")}</button>
        </span>
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
        <button className="secondary compact" disabled={Boolean(busy)} onClick={() => void run("driver", desktop.removeDriver)}>{t("移除驱动")}</button>
      </Row>
    </Card>
  );
}
