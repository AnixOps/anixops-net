import { Cable, Smartphone, Trash2, Wifi, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { desktop, OnDemandAction, OnDemandSettings } from "../bridge";
import { Card, FoldedRows, IconButton, Row, Run, ToggleRow } from "../ui";
import { Select } from "../controls";
import { t } from "../i18n";

const actions: Array<{ value: OnDemandAction; label: string }> = [
  { value: "connect", label: "连接|verb" },
  { value: "disconnect", label: "断开" },
  { value: "ignore", label: "保持不变" },
];

/** Longest Wi-Fi name (IEEE 802.11), as the backend checks. */
const maxSsidBytes = 32;

/**
 * Whether a saved name holds replacement characters: earlier Windows versions read the Wi-Fi name in the console's
 * code page, which turned a Chinese name into these, so the rule never matches the network it was added for.
 */
function misread(ssid: string): boolean {
  return ssid.includes("\uFFFD");
}

function ssidFits(ssid: string): boolean {
  const bytes = new TextEncoder().encode(ssid).length;
  return bytes > 0 && bytes <= maxSsidBytes;
}

function ActionPicker({ label, value, disabled, onChange }: {
  label: string;
  value: OnDemandAction;
  disabled: boolean;
  onChange: (value: OnDemandAction) => void;
}) {
  return (
    <Select label={label} value={value} disabled={disabled} onChange={onChange} choices={actions.map((action) => ({ value: action.value, label: t(action.label) }))} />
  );
}

/**
 * The Wi-Fi rules, as in the iOS On Demand settings: connect or disconnect when the device joins a network. Windows
 * and Android; the app applies them while it runs. Android asks for the location permission only when they are
 * turned on, because it gives the Wi-Fi name only with it.
 */
export function OnDemandCard({ busy, run, android }: { busy: string | null; run: Run; android: boolean }) {
  const [settings, setSettings] = useState<OnDemandSettings | null>(null);
  const [draft, setDraft] = useState("");
  useEffect(() => {
    void desktop.onDemandSettings().then(setSettings).catch(() => setSettings(null));
  }, []);
  if (!settings) return null;
  const disabled = Boolean(busy);

  const save = (next: OnDemandSettings, message: string) => void run("Wi-Fi rules", async () => {
    setSettings(await desktop.onDemandSettings(next));
    return { message };
  });
  const toggle = () => void run("Wi-Fi rules", async () => {
    const next = await desktop.onDemandSettings({ ...settings, enabled: !settings.enabled });
    setSettings(next);
    // Asks once for the permission that reveals the Wi-Fi name; the rules for other networks work without it.
    if (next.enabled) await desktop.currentNetwork(true).catch(() => undefined);
    return { message: next.enabled ? t("已打开 Wi-Fi 规则") : t("已关闭 Wi-Fi 规则") };
  });
  const addRule = (ssid: string) => {
    const name = ssid.trim();
    if (!ssidFits(name) || settings.wifi.some((rule) => rule.ssid === name)) return;
    setDraft("");
    save({ ...settings, wifi: [...settings.wifi, { ssid: name, action: "disconnect" }] }, t("已添加 Wi-Fi 规则"));
  };
  const addCurrent = () => void run("Wi-Fi rules", async () => {
    const network = await desktop.currentNetwork(true);
    if (network.kind !== "wifi") return { message: t("现在没有连接 Wi-Fi") };
    if (!network.ssid) return { message: android ? t("无法读取 Wi-Fi 名称，请在系统设置中允许本应用使用位置信息") : t("无法读取 Wi-Fi 名称，请在 Windows 设置的“隐私和安全性 > 位置”中允许桌面应用访问位置") };
    setDraft(network.ssid);
    if (!ssidFits(network.ssid)) return { message: t("Wi-Fi 名称超过 32 字节，无法添加规则") };
    return { message: t("已填入当前 Wi-Fi 名称") };
  });
  const setRule = (ssid: string, action: OnDemandAction) =>
    save({ ...settings, wifi: settings.wifi.map((rule) => (rule.ssid === ssid ? { ...rule, action } : rule)) }, t("已保存 Wi-Fi 规则"));
  const removeRule = (ssid: string) =>
    save({ ...settings, wifi: settings.wifi.filter((rule) => rule.ssid !== ssid) }, t("已删除 Wi-Fi 规则"));
  const canAdd = ssidFits(draft.trim()) && !settings.wifi.some((rule) => rule.ssid === draft.trim());

  return (
    <>
      <Card
        title={t("Wi-Fi 规则")}
        description={android
          ? t("加入网络时自动连接或断开，应用在后台运行时生效。读取 Wi-Fi 名称需要位置权限。")
          : t("加入网络时自动连接或断开，应用运行时生效。读取 Wi-Fi 名称需在系统设置中允许桌面应用访问位置。")}
      >
        <ToggleRow icon={Wifi} color="sky" label={t("按网络自动连接")} checked={settings.enabled} disabled={disabled} onChange={toggle} />
      </Card>
      {settings.enabled && (
        <Card>
          <FoldedRows
            items={settings.wifi}
            label={(count) => t("显示全部 {count} 条 Wi-Fi 规则", { count })}
            render={(rule) => (
              <Row key={rule.ssid} icon={Wifi} color="indigo" label={rule.ssid} hint={misread(rule.ssid) ? t("名称读取有误，请删除后用“当前网络”重新添加") : undefined}>
                <ActionPicker label={rule.ssid} value={rule.action} disabled={disabled} onChange={(action) => setRule(rule.ssid, action)} />
                <IconButton label={t("删除") + " " + rule.ssid} disabled={disabled} onClick={() => removeRule(rule.ssid)}>
                  <Trash2 size={14} aria-hidden="true" />
                </IconButton>
              </Row>
            )}
          />
          <Row icon={Wifi} color="gray" label={t("Wi-Fi 名称")}>
            <input
              className="inline-input"
              aria-label={t("Wi-Fi 名称")}
              value={draft}
              spellCheck={false}
              autoCapitalize="off"
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") addRule(draft); }}
            />
            <button className="secondary compact" disabled={disabled} onClick={addCurrent}>{t("当前网络")}</button>
            <button className="secondary compact" disabled={disabled || !canAdd} onClick={() => addRule(draft)}>{t("添加")}</button>
          </Row>
          <Row icon={WifiOff} color="gray" label={t("其他 Wi-Fi")}>
            <ActionPicker label={t("其他 Wi-Fi")} value={settings.otherWifi} disabled={disabled} onChange={(otherWifi) => save({ ...settings, otherWifi }, t("已保存 Wi-Fi 规则"))} />
          </Row>
          {android && (
            <Row icon={Smartphone} color="green" label={t("蜂窝网络")}>
              <ActionPicker label={t("蜂窝网络")} value={settings.cellular} disabled={disabled} onChange={(cellular) => save({ ...settings, cellular }, t("已保存 Wi-Fi 规则"))} />
            </Row>
          )}
          <Row icon={Cable} color="gray" label={t("有线和其他网络")}>
            <ActionPicker label={t("有线和其他网络")} value={settings.other} disabled={disabled} onChange={(other) => save({ ...settings, other }, t("已保存 Wi-Fi 规则"))} />
          </Row>
        </Card>
      )}
    </>
  );
}
