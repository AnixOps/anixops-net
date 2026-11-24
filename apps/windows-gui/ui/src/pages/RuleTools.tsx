import { Database, FlaskConical, Gauge, Route, Smartphone, TriangleAlert, X } from "lucide-react";
import { useEffect, useState } from "react";
import { DataSource, desktop, DeviceName, RouteTest, RuntimeSnapshot } from "../bridge";
import { busiestRules, dataName, dataRule } from "../rule-sets";
import { liveEngine, usePolling } from "../live";
import { Card, Field, IconButton, Row, Run, Segmented } from "../ui";
import { t } from "../i18n";
import { errorLine } from "../error-text";

/**
 * The GEOIP and IP-ASN data lists: the client builds in only the mainland China lists, so `GEOIP,US` or
 * `IP-ASN,13335` need a list the user adds by URL. Lists the running rules use and nobody provides are shown first,
 * so a rule that matches nothing is never silent.
 */
export function DataLists({ missing, busy, run }: { missing: string[]; busy: string | null; run: Run }) {
  const [sources, setSources] = useState<DataSource[]>([]);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");

  const reload = () => void desktop.dataSources().then((list) => setSources(Array.isArray(list) ? list : [])).catch(() => setSources([]));
  useEffect(reload, []);

  const added = new Set(sources.map((source) => source.name));
  const lacking = missing.filter((item) => !added.has(item));
  const target = dataName(name);
  const save = (list: string, link: string) => void run("data list", async () => {
    const result = await desktop.saveDataSource(list, link.trim());
    reload();
    setName("");
    setUrl("");
    return result;
  });

  return (
    <Card
      title={t("GeoIP 与 ASN 数据")}
      description={t("仅内置中国大陆的 GeoIP 列表。其他国家的 GEOIP 和 IP-ASN 规则需按 URL 添加开源列表，经 PROXY 每天下载一次。")}
    >
      {lacking.map((item) => (
        <Row
          key={item}
          icon={TriangleAlert}
          color="orange"
          label={t("缺少 {rule} 的数据", { rule: dataRule(item) })}
          hint={t("这条规则现在不会匹配任何连接。")}
        >
          <button className="link-button" disabled={Boolean(busy)} onClick={() => setName(item)}>{t("添加列表")}</button>
        </Row>
      ))}
      {sources.map((source) => (
        <Row key={source.name} icon={Database} color="sky" label={dataRule(source.name)} hint={source.url}>
          <IconButton
            label={t("移除 {name}", { name: dataRule(source.name) })}
            disabled={Boolean(busy)}
            onClick={() => void run("data list", async () => {
              const result = await desktop.removeDataSource(source.name);
              reload();
              return result;
            })}
          ><X size={14} /></IconButton>
        </Row>
      ))}
      <div className="data-list-form">
        <Field label={t("国家代码或 AS 号")}>
          <input value={name} spellCheck={false} placeholder="US, AS13335" onChange={(event) => setName(event.target.value)} />
        </Field>
        <Field label={t("列表地址")}>
          <input value={url} spellCheck={false} placeholder="https://" onChange={(event) => setUrl(event.target.value)} />
        </Field>
        {name.trim() && !target && <p className="muted">{t("写两个字母的国家代码（如 US）或 AS 号（如 AS13335）。")}</p>}
        <div className="button-row">
          <button
            className="secondary compact"
            disabled={Boolean(busy) || !target || !url.trim().startsWith("https://")}
            onClick={() => target && save(target, url)}
          >{t("添加")}</button>
        </div>
      </div>
    </Card>
  );
}

/**
 * Names for the devices that share this computer's connection (LAN sharing, or the tunnel as their gateway), by the
 * address their connections come from, for `DEVICE-NAME` rules.
 */
export function DeviceNames({ busy, run }: { busy: string | null; run: Run }) {
  const [devices, setDevices] = useState<DeviceName[]>([]);
  const [address, setAddress] = useState("");
  const [name, setName] = useState("");

  const reload = () => void desktop.devices().then((list) => setDevices(Array.isArray(list) ? list : [])).catch(() => setDevices([]));
  useEffect(reload, []);

  const save = () => void run("device name", async () => {
    const result = await desktop.saveDevice(address.trim(), name.trim());
    reload();
    setAddress("");
    setName("");
    return result;
  });

  return (
    <Card
      title={t("设备名")}
      description={t("为经局域网共享上网的设备命名，DEVICE-NAME 规则按名称匹配（不区分大小写）。")}
    >
      {devices.map((device) => (
        <Row key={device.address} icon={Smartphone} color="indigo" label={device.name} hint={device.address}>
          <IconButton
            label={t("移除 {name}", { name: device.name })}
            disabled={Boolean(busy)}
            onClick={() => void run("device name", async () => {
              const result = await desktop.removeDevice(device.address);
              reload();
              return result;
            })}
          ><X size={14} /></IconButton>
        </Row>
      ))}
      <div className="data-list-form">
        <Field label={t("设备 IP")}>
          <input value={address} spellCheck={false} placeholder="192.168.1.30" onChange={(event) => setAddress(event.target.value)} />
        </Field>
        <Field label={t("名字")}>
          <input value={name} spellCheck={false} placeholder="Apple TV" onChange={(event) => setName(event.target.value)} />
        </Field>
        <div className="button-row">
          <button
            className="secondary compact"
            disabled={Boolean(busy) || !address.trim() || !name.trim() || name.includes(",")}
            onClick={save}
          >{t("添加")}</button>
        </div>
      </div>
    </Card>
  );
}

/** Asks the running engine which rule decides a connection and where it goes, without opening one. */
export function RuleTester({ snapshot, busy }: { snapshot: RuntimeSnapshot | null; busy: string | null }) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("443");
  const [network, setNetwork] = useState<"tcp" | "udp">("tcp");
  const [address, setAddress] = useState("");
  const [process, setProcess] = useState("");
  const [answer, setAnswer] = useState<{ mode: string; test: RouteTest } | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (!liveEngine(snapshot)) return null;

  const test = () => {
    setError(null);
    void desktop.testRoute({
      host: host.trim(),
      port: Number(port) || 443,
      network,
      address: address.trim() || undefined,
      process: process.trim() || undefined,
    }).then(setAnswer).catch((reason) => {
      setAnswer(null);
      setError(errorLine(reason));
    });
  };

  return (
    <Card title={t("规则测试")} description={t("输入域名或 IP，查看匹配的规则和出口。不发起真实连接，不计入命中次数。")}>
      <div className="data-list-form">
        <Field label={t("域名或 IP")}>
          <input value={host} spellCheck={false} placeholder="example.com" onChange={(event) => setHost(event.target.value)} />
        </Field>
        <Field label={t("端口")}>
          <input type="number" min={1} max={65535} value={port} onChange={(event) => setPort(event.target.value)} />
        </Field>
        <Segmented
          label={t("协议")}
          options={[{ value: "tcp", label: "TCP" }, { value: "udp", label: "UDP" }]}
          value={network}
          onChange={setNetwork}
        />
        <Field label={t("域名解析到的 IP（可选）")}>
          <input value={address} spellCheck={false} onChange={(event) => setAddress(event.target.value)} />
        </Field>
        <Field label={t("程序名（可选）")}>
          <input value={process} spellCheck={false} placeholder="chrome.exe" onChange={(event) => setProcess(event.target.value)} />
        </Field>
        <div className="button-row">
          <button className="primary" disabled={Boolean(busy) || !host.trim()} onClick={test}><FlaskConical size={14} />{t("测试")}</button>
        </div>
      </div>
      {error && <Row icon={TriangleAlert} color="red" label={error} />}
      {answer && (
        <>
          <Row icon={Route} color="indigo" label={answer.test.rule} hint={t("匹配的规则")} />
          <Row label={t("出口")} value={[...answer.test.chain, answer.test.outbound].filter((step, index, all) => all.indexOf(step) === index).join(" → ")} />
          {answer.test.needs_address && (
            <Row icon={TriangleAlert} color="orange" label={t("该规则按目标 IP 匹配，需填写域名解析到的 IP；解析失败时连接被拒绝。")} />
          )}
          {answer.mode !== "rule" && (
            <Row icon={TriangleAlert} color="orange" label={answer.mode === "global" ? t("当前是全局模式，实际连接不按规则分流。") : t("当前是直连模式，实际连接不按规则分流。")} />
          )}
        </>
      )}
    </Card>
  );
}

/** How often each rule decided a connection since the rules were loaded, busiest first. */
export function RuleHits({ snapshot, busy, run }: { snapshot: RuntimeSnapshot | null; busy: string | null; run: Run }) {
  const [view, reload] = usePolling(desktop.ruleHits, liveEngine(snapshot));
  if (!view) return null;
  const busiest = busiestRules(view.rules ?? []);
  const unused = (view.rules ?? []).filter((rule) => rule.hits === 0).length;

  return (
    <Card
      title={t("规则命中")}
      description={t("自规则加载以来各规则处理的连接数，修改规则后重新计数。")}
      actions={<button className="link-button" disabled={Boolean(busy)} onClick={() => void run("rule hits", async () => {
        await desktop.clearRuleHits();
        reload();
        return { message: t("已清零命中次数") };
      })}>{t("清零")}</button>}
    >
      {busiest.length === 0 && view.final.hits === 0 && <p className="muted">{t("暂无连接经过规则")}</p>}
      {busiest.map((rule, index) => (
        <Row key={String(index) + "-" + rule.rule} icon={Gauge} color="teal" label={rule.rule} hint={rule.target} value={t("{count} 次", { count: rule.hits })} />
      ))}
      <Row icon={Gauge} color="gray" label="FINAL" hint={view.final.target} value={t("{count} 次", { count: view.final.hits })} />
      {unused > 0 && <p className="muted">{t("另有 {count} 条规则暂无命中。", { count: unused })}</p>}
    </Card>
  );
}
