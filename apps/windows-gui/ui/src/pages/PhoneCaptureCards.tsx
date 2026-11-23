import { Eye, EyeOff, FileKey, KeyRound, LockKeyhole, Network, RefreshCw, ShieldAlert, Smartphone, UserRound } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { desktop, LanShareAuth, LanShareStatus } from "../bridge";
import { Card, CopyButton, IconButton, Row, Run, StatusPill, Switch } from "../ui";
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

/** A password as dots of the same length, shown until the user asks to see it. */
export function maskPassword(password: string): string {
  return "\u2022".repeat(password.length);
}

/**
 * Phone debugging: the engine opens a second proxy port to the local network, the phone sets it as its Wi-Fi proxy
 * and scans the QR code for the setup page, which also hands out the root certificate. Then the user's own CA.
 * Phones must send the proxy username and password shown here (made and kept by the backend, which adds them to
 * every start) unless the user turns that off.
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
  const [auth, setAuth] = useState<LanShareAuth | null>(null);
  const [revealed, setRevealed] = useState(false);

  useEffect(() => {
    desktop.lanShareAuth().then(setAuth).catch(() => setAuth(null));
  }, []);

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
  // A running share takes new credentials, or the switch, only when it starts again on the same port.
  const restart = async () => {
    if (running && status?.port) setStatus(await desktop.startLanShare(status.port));
  };
  const regenerate = () => void run("phone", async () => {
    setAuth(await desktop.regenerateLanShareAuth());
    await restart();
    return { message: t("已生成新的用户名和密码。已经设置过的手机要重新填写。") };
  });
  const toggleRequired = () => void run("phone", async () => {
    const next = await desktop.setLanShareAuthRequired(!(auth?.required ?? true));
    setAuth(next);
    await restart();
    return { message: next.required ? t("已要求手机填写用户名和密码。") : t("已关闭密码。同一网络里的任何人都能使用这个代理。") };
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
        description={t("手机和电脑连同一个 Wi-Fi，把手机的 Wi-Fi 代理设成电脑，手机的请求就会出现在活动页。只接受局域网里的设备，默认还要填用户名和密码。")}
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
        <Row icon={LockKeyhole} color="green" label={t("需要密码")} hint={t("手机要在 Wi-Fi 代理里填用户名和密码才能使用这个代理")} className="toggle-row">
          <Switch label={t("需要密码")} checked={auth?.required ?? true} disabled={Boolean(busy) || auth === null} onChange={toggleRequired} />
        </Row>
        {auth && !auth.required && (
          <Row icon={ShieldAlert} color="orange" label={t("任何人都能使用这个代理")} hint={t("同一网络里的任何设备都能通过这台电脑和你的节点上网，它们的请求也会进入抓包。只在可信的网络里关闭密码。")} />
        )}
        {auth && auth.required && (
          <>
            <Row icon={UserRound} color="sky" label={t("用户名")}>
              <code className="row-value selectable">{auth.username}</code>
              <CopyButton label={t("拷贝用户名")} text={auth.username} />
            </Row>
            <Row icon={KeyRound} color="sky" label={t("密码")} hint={t("用随机数生成，只保存在这台电脑的系统密钥库里")}>
              <code className="row-value selectable">{revealed ? auth.password : maskPassword(auth.password)}</code>
              <IconButton label={revealed ? t("隐藏密码") : t("显示密码")} onClick={() => setRevealed(!revealed)}>
                {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
              </IconButton>
              <CopyButton label={t("拷贝密码")} text={auth.password} />
              <IconButton label={t("重新生成用户名和密码")} onClick={regenerate} disabled={Boolean(busy)}>
                <RefreshCw size={14} />
              </IconButton>
            </Row>
          </>
        )}
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
                  {status.credentials && (
                    <small>{t("iPhone：在 Wi-Fi 代理设置里打开认证，填上面的用户名和密码。Android 的 Wi-Fi 代理没有密码栏，浏览器会在第一次访问时询问，其他应用可能连不上。")}</small>
                  )}
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
