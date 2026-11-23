import { FileKey, KeyRound, Network, Smartphone } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, LanShareStatus } from "../bridge";
import { Card, Row, Run, StatusPill, Switch } from "../ui";
import { Select } from "../controls";
import { t } from "../i18n";

const DEFAULT_PORT = 8899;

/** A port the share may listen on, or null. */
export function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text.trim())) return null;
  const port = Number(text.trim());
  return port >= 1024 && port <= 65535 ? port : null;
}

/** The setup page address for one of the computer's addresses. */
export function setupUrl(address: string, port: number): string {
  return "http://" + address + ":" + port + "/";
}

/**
 * Phone debugging: the engine opens a second proxy port to the local network, the phone sets it as its Wi-Fi proxy
 * and scans the QR code for the setup page, which also hands out the root certificate. Then the user's own CA.
 */
export function PhoneCaptureCards({ busy, run }: { busy: string | null; run: Run }) {
  const [status, setStatus] = useState<LanShareStatus | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [portText, setPortText] = useState(String(DEFAULT_PORT));
  const [address, setAddress] = useState<string | null>(null);
  const [qr, setQr] = useState<string | null>(null);
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [certificatePem, setCertificatePem] = useState("");
  const [privateKeyPem, setPrivateKeyPem] = useState("");

  const refresh = useCallback(() => {
    desktop.lanShare()
      .then((next) => { setStatus(next); setUnavailable(false); })
      .catch(() => { setStatus(null); setUnavailable(true); });
    desktop.mitmRootFingerprint().then(setFingerprint).catch(() => setFingerprint(null));
  }, []);
  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const running = Boolean(status?.running && status.port);
  const addresses = status?.addresses ?? [];
  const chosen = address && addresses.includes(address) ? address : addresses[0] ?? null;
  const url = running && chosen && status?.port ? setupUrl(chosen, status.port) : null;
  useEffect(() => {
    if (!url) { setQr(null); return; }
    desktop.phoneQrCode(url).then(setQr).catch(() => setQr(null));
  }, [url]);

  const port = parsePort(portText);
  const toggle = () => void run("phone", async () => {
    if (running) {
      setStatus(await desktop.stopLanShare());
      return { message: t("已关闭手机抓包。记得把手机的 Wi-Fi 代理改回关闭。") };
    }
    if (port === null) throw new Error(t("端口要在 1024 到 65535 之间。"));
    setStatus(await desktop.startLanShare(port));
    return { message: t("手机抓包已开启。用手机扫码打开设置页。") };
  });
  const importRoot = () => void run("certificate", async () => {
    const result = await desktop.importMitmRoot(certificatePem, privateKeyPem);
    setCertificatePem("");
    setPrivateKeyPem("");
    refresh();
    return result;
  });

  return (
    <>
      <Card
        title={t("手机抓包")}
        description={t("手机和电脑连同一个 Wi-Fi，把手机的 Wi-Fi 代理设成电脑，手机的请求就会出现在活动页。只接受局域网里的设备。")}
        actions={running && status ? <span className="count">{t("{count} 条手机连接", { count: status.active })}</span> : undefined}
      >
        <Row icon={Smartphone} color="teal" label={t("允许手机通过这台电脑上网")} hint={unavailable ? t("先连接，再开启手机抓包。Windows 需要以桌面模式连接。") : t("在局域网里开一个代理端口")} className="toggle-row">
          {!running && (
            <input
              className="inline-input port-input"
              aria-label={t("端口")}
              inputMode="numeric"
              value={portText}
              disabled={Boolean(busy) || unavailable}
              onChange={(event) => setPortText(event.target.value)}
            />
          )}
          <Switch label={t("允许手机通过这台电脑上网")} checked={running} disabled={Boolean(busy) || unavailable || (!running && port === null)} onChange={toggle} />
        </Row>
        {running && status && (
          <>
            {addresses.length === 0 ? (
              <Row icon={Network} color="orange" label={t("没有找到局域网地址")} hint={t("确认电脑连着 Wi-Fi 或有线局域网。")} />
            ) : (
              <Row icon={Network} color="sky" label={t("代理地址")} hint={t("在手机的 Wi-Fi 设置里选“手动”，填这个服务器和端口")}>
                {addresses.length > 1 ? (
                  <Select label={t("代理地址")} className="mono" value={chosen ?? ""} onChange={setAddress} choices={addresses.map((item) => ({ value: item, label: item + ":" + status.port }))} />
                ) : (
                  <code className="row-value selectable">{chosen + ":" + status.port}</code>
                )}
              </Row>
            )}
            {url && (
              <div className="phone-qr">
                {qr && <div className="phone-qr-code" role="img" aria-label={t("设置页二维码")} dangerouslySetInnerHTML={{ __html: qr }} />}
                <div className="phone-qr-text">
                  <strong>{t("用手机相机扫码，打开设置页")}</strong>
                  <code className="selectable">{url}</code>
                  <small>{t("设置页里有 iPhone 和 Android 的步骤和根证书下载。设好代理后在手机上打开 http://{host} 检查是否生效。", { host: status.page_host })}</small>
                  <StatusPill
                    label={status.https_decryption ? t("HTTPS 解密已开启") : t("HTTPS 解密未开启，只能看到连接")}
                    tone={status.https_decryption ? "success" : "warning"}
                  />
                </div>
              </div>
            )}
          </>
        )}
      </Card>

      <Card
        title={t("自己的根证书")}
        description={t("默认每台电脑生成自己的根证书。团队已有 CA 时可以换成它：粘贴 PEM 格式的 CA 证书和它的 PKCS#8 私钥（BEGIN PRIVATE KEY）。换完后这台电脑要重新信任，手机和其他设备需要重新安装。")}
      >
        <Row icon={KeyRound} color="gray" label={t("当前根证书指纹（SHA-256）")} hint={fingerprint ?? t("开启 HTTPS 解密后生成")}/>
        <div className="root-import">
          <textarea aria-label={t("CA 证书 PEM")} placeholder={"-----BEGIN CERTIFICATE-----"} value={certificatePem} onChange={(event) => setCertificatePem(event.target.value)} spellCheck={false} />
          <textarea aria-label={t("CA 私钥 PEM")} placeholder={"-----BEGIN PRIVATE KEY-----"} value={privateKeyPem} onChange={(event) => setPrivateKeyPem(event.target.value)} spellCheck={false} />
          <button className="secondary" disabled={Boolean(busy) || !certificatePem.trim() || !privateKeyPem.trim()} onClick={importRoot}><FileKey size={14} />{t("使用这张根证书")}</button>
        </div>
      </Card>
    </>
  );
}
