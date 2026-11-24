import { useState, type ReactElement } from "react";
import { ShieldAlert } from "lucide-react";
import { Heading } from "react-aria-components";
import { ModalDialog } from "./controls";
import { t } from "./i18n";

// The first time someone turns HTTPS decryption on, the app asks once more that they will only capture and decrypt
// traffic they are entitled to see (TERMS.md section 4). The answer is kept, so it is asked once per installation.

const storageKey = "anixops.https-decryption-consent";

/** True until someone has confirmed once; any stored value counts as confirmed. */
export function decryptionConsentNeeded(stored: string | null | undefined): boolean {
  return !stored;
}

function loadStored(): string | null {
  try {
    return window.localStorage.getItem(storageKey);
  } catch {
    return null;
  }
}

/** The four points the confirmation shows, in the order they are read. */
export const decryptionConsentPoints = [
  "仅解密本人设备上的流量，或已获授权调试的应用、设备和网络的流量。",
  "不抓取、不解密他人的流量。将其他设备接入抓包前，须取得设备所有者的同意。",
  "解密后的内容可能含有密码、令牌和个人信息，只保存在本机；导出或分享前请先脱敏。",
  "不得用于入侵、窃取数据或绕过他人的安全机制。详见用户协议第 4 节。",
];

/**
 * `guard(enable)` runs `enable` right away once confirmed before; otherwise it shows the confirmation and runs it only
 * when the person confirms. Render `dialog` somewhere in the page.
 */
export function useDecryptionConsent(): { guard: (enable: () => void) => void; dialog: ReactElement | null } {
  const [pending, setPending] = useState<(() => void) | null>(null);
  const guard = (enable: () => void) => {
    if (!decryptionConsentNeeded(loadStored())) enable();
    else setPending(() => enable);
  };
  const confirm = () => {
    try {
      window.localStorage.setItem(storageKey, new Date().toISOString());
    } catch {
      // Confirmed for this session; the question comes back next start.
    }
    const enable = pending;
    setPending(null);
    enable?.();
  };
  const dialog = pending ? <DecryptionConsentDialog onConfirm={confirm} onCancel={() => setPending(null)} /> : null;
  return { guard, dialog };
}

function DecryptionConsentDialog({ onConfirm, onCancel }: { onConfirm: () => void; onCancel: () => void }) {
  return (
    <ModalDialog onClose={onCancel}>
      <header className="terms-head">
        <ShieldAlert size={22} aria-hidden />
        <div>
          <Heading slot="title" level={2}>{t("开启 HTTPS 解密前请确认")}</Heading>
          <p>{t("HTTPS 解密可查看加密连接中的明文，仅限下列范围使用：")}</p>
        </div>
      </header>
      <div className="terms-text">
        {decryptionConsentPoints.map((point) => <p key={point} className="terms-item">{t(point)}</p>)}
      </div>
      <footer className="terms-actions">
        <button className="secondary" autoFocus onClick={onCancel}>{t("取消")}</button>
        <button className="primary" onClick={onConfirm}>{t("我确认，开启")}</button>
      </footer>
    </ModalDialog>
  );
}
