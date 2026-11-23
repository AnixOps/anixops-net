import { Archive, FolderOpen, Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { CaptureSession, CaptureStatus, desktop, SessionProtection } from "../bridge";
import { formatBytes } from "../subscription-format";
import { Card, EmptyState, MoreMenu, Row, Run } from "../ui";
import { t } from "../i18n";

/** How the key of saved sessions is kept, for the card's note. */
export function protectionText(protection: SessionProtection | null, days: number): string {
  const kept = t("会话只保存在本机，{days} 天后自动删除。", { days });
  switch (protection) {
    case "dpapi":
      return [kept, t("用 Windows 数据保护（DPAPI）加密，只有当前 Windows 账户能打开。")].join(" ");
    case "keystore":
      return [kept, t("用 Android Keystore 中的密钥加密。")].join(" ");
    case "secret-service":
      return [kept, t("密钥保存在系统钥匙环（Secret Service）里。")].join(" ");
    case "file":
      return [kept, t("没有找到系统钥匙环（Secret Service），密钥保存在只有你能读取的文件里。")].join(" ");
    default:
      return kept;
  }
}

/** A session's title: its name, or when it was saved. */
export function sessionTitle(session: CaptureSession): string {
  return session.name?.trim() || new Date(session.saved_at_ms).toLocaleString();
}

/** What is in a session and when it goes. */
export function sessionHint(session: CaptureSession): string {
  if (session.unreadable) return t("本机的密钥打不开这个会话（可能是重装前保存的），到期后自动删除");
  return [
    t("{count} 个请求", { count: session.requests ?? 0 }),
    t("{count} 个连接", { count: session.connections ?? 0 }),
    formatBytes(session.size),
    t("{time} 删除", { time: new Date(session.expires_at_ms).toLocaleDateString() }),
  ].join(" · ");
}

/**
 * Saved capture sessions (debug mode): seal what capture holds into an encrypted file, load one back to look at its
 * requests and export them, delete one. The engine does the sealing; the backend keeps the key.
 */
export function CaptureSessions({ capture, run, onLoaded }: {
  capture: CaptureStatus | null;
  run: Run;
  /** A session replaced what capture held: a request selected before it is gone. */
  onLoaded?: () => void;
}) {
  const [sessions, setSessions] = useState<CaptureSession[] | null>(null);
  const [protection, setProtection] = useState<SessionProtection | null>(null);
  const [days, setDays] = useState(7);
  const [failure, setFailure] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);

  const load = useCallback(() => {
    desktop.captureSessions()
      .then((answer) => {
        setSessions(answer.sessions);
        setProtection(answer.protection);
        setDays(answer.retention_days);
        setFailure(null);
      })
      .catch((error: unknown) => setFailure(String(error)));
  }, []);
  useEffect(load, [load]);

  const captured = capture?.connections ?? 0;
  const save = () => void run("capture session", async () => {
    const answer = await desktop.saveCaptureSession(name.trim() || undefined);
    setName("");
    load();
    return { message: t("已保存会话：{name}", { name: sessionTitle(answer.session) }) };
  });
  const open = (session: CaptureSession) => {
    // Loading replaces what capture holds; ask once when that is not empty.
    if (captured > 0 && confirming !== session.session) {
      setConfirming(session.session);
      return;
    }
    setConfirming(null);
    void run("capture session", async () => {
      await desktop.loadCaptureSession(session.session);
      onLoaded?.();
      return { message: t("已载入会话：{name}。实时抓包已停止。", { name: sessionTitle(session) }) };
    });
  };
  const remove = (session: CaptureSession) => void run("capture session", async () => {
    await desktop.deleteCaptureSession(session.session);
    load();
    return { message: t("已删除会话：{name}", { name: sessionTitle(session) }) };
  });

  return (
    <Card title={t("已保存的会话")} description={protectionText(protection, days)}>
      <Row icon={Save} color="indigo" label={t("保存当前抓包")} hint={captured > 0 ? t("{count} 个连接", { count: captured }) : t("还没有抓到内容")}>
        <input
          className="inline-input"
          aria-label={t("会话名称（可选）")}
          placeholder={t("会话名称（可选）")}
          maxLength={120}
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
        <button className="secondary compact" disabled={captured === 0} onClick={save}>{t("保存")}</button>
      </Row>
      {failure && <p className="muted danger-text">{failure}</p>}
      {sessions && sessions.length === 0 && (
        <EmptyState icon={Archive} title={t("还没有保存的会话")} message={t("保存后可以在这里载入，查看当时的请求并导出。")} />
      )}
      {sessions?.map((session) => (
        <Row key={session.session} icon={FolderOpen} color={session.unreadable ? "gray" : "sky"} label={sessionTitle(session)} hint={sessionHint(session)}>
          {!session.unreadable && (
            <button
              className="secondary compact"
              onBlur={() => setConfirming(null)}
              onClick={() => open(session)}
            >
              {confirming === session.session ? t("替换当前抓包并载入") : t("载入")}
            </button>
          )}
          <MoreMenu
            label={t("更多")}
            items={[{ label: t("删除会话"), danger: true, confirm: t("确认删除会话 {name}", { name: sessionTitle(session) }), onSelect: () => remove(session) }]}
          />
        </Row>
      ))}
    </Card>
  );
}
