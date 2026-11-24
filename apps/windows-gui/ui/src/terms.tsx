import { useState } from "react";
import { ScrollText, ShieldCheck } from "lucide-react";
import { Heading } from "react-aria-components";
import { ModalDialog } from "./controls";
import termsMarkdown from "../../../../TERMS.md?raw";
import privacyMarkdown from "../../../../PRIVACY.md?raw";
import { currentLanguage, t } from "./i18n";

// The user agreement (TERMS.md at the repository root) ships inside the app, so it can be read offline and is always
// the version this build was made with. Someone accepts it once; a new major version of the terms asks again.
// The privacy policy (PRIVACY.md) ships the same way and can be read from the consent screen and from Settings.

const storageKey = "anixops.terms-accepted";

/** The terms version from the "基准版本 x.y.z" line of TERMS.md, or null when the file has none. */
export function termsVersionOf(markdown: string): string | null {
  return /基准版本\s+(\d+\.\d+\.\d+)/.exec(markdown)?.[1] ?? null;
}

export const termsVersion = termsVersionOf(termsMarkdown) ?? "0.0.0";

function major(version: string | null | undefined): string | null {
  return version ? (/^(\d+)\./.exec(version)?.[1] ?? null) : null;
}

/** True when nothing was accepted yet, or what was accepted is an older major version than `current`. */
export function needsConsent(accepted: string | null | undefined, current: string = termsVersion): boolean {
  return major(accepted) === null || major(accepted) !== major(current);
}

function loadAccepted(): string | null {
  try {
    return window.localStorage.getItem(storageKey);
  } catch {
    return null;
  }
}

/** Whether the consent screen has to be shown, and a function that records acceptance of this build's terms. */
export function useTermsConsent(): [boolean, () => void] {
  const [accepted, setAccepted] = useState<string | null>(loadAccepted);
  const accept = () => {
    setAccepted(termsVersion);
    try {
      window.localStorage.setItem(storageKey, termsVersion);
    } catch {
      // Accepted for this session; the screen comes back next start.
    }
  };
  return [needsConsent(accepted), accept];
}

interface Block {
  kind: "h1" | "h2" | "p" | "li";
  text: string;
}

/** TERMS.md as headings, paragraphs and list items, without comments and inline Markdown marks. */
export function termsBlocks(markdown: string): Block[] {
  const plain = (text: string) =>
    text
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
      .replace(/\*\*|`/g, "")
      .trim();
  const blocks: Block[] = [];
  let open = false;
  for (const raw of markdown.replace(/<!--[\s\S]*?-->/g, "").split("\n")) {
    const line = raw.replace(/^>\s?/, "");
    if (!line.trim() || /^-{3,}$/.test(line.trim())) {
      open = false;
      continue;
    }
    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    const item = /^\s*(?:[-*]|\d+\.)\s+(.*)$/.exec(line);
    const last = blocks[blocks.length - 1];
    if (heading) {
      blocks.push({ kind: heading[1].length === 1 ? "h1" : "h2", text: plain(heading[2]) });
      open = false;
      continue;
    }
    if (item) blocks.push({ kind: "li", text: plain(item[1]) });
    // A wrapped line continues the paragraph or list item right above it.
    else if (open && last) last.text += plain(line);
    else blocks.push({ kind: "p", text: plain(line) });
    open = true;
  }
  return blocks;
}

/**
 * The privacy policy in English when `english`, else in Chinese: the shared title and effective date, then that
 * language's half. The line linking the two halves is dropped, since the viewer cannot follow in-page anchors. A file
 * without both halves comes back whole. Same rule as Privacy.forLanguage in the Android app.
 */
export function privacyForLanguage(markdown: string, english: boolean): string {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  const englishAt = lines.findIndex((line) => line.trim() === "## English");
  // "## 中文", PRIVACY.md's heading for its Chinese half: a marker in the file, not UI text to translate.
  const chineseAt = lines.findIndex((line) => line.trim() === "## \u4e2d\u6587");
  if (englishAt < 0 || chineseAt < 0) return markdown;
  const head = lines.slice(0, Math.min(englishAt, chineseAt)).filter((line) => !line.includes("](#"));
  const body = english
    ? lines.slice(englishAt + 1, englishAt < chineseAt ? chineseAt : lines.length)
    : lines.slice(chineseAt + 1, chineseAt < englishAt ? englishAt : lines.length);
  return [...head, "", ...body].join("\n").trim();
}

function DocumentText({ markdown, label }: { markdown: string; label: string }) {
  return (
    <div className="terms-text" tabIndex={0} aria-label={label}>
      {termsBlocks(markdown).map((block, index) => {
        if (block.kind === "h1") return <h3 key={index}>{block.text}</h3>;
        if (block.kind === "h2") return <h4 key={index}>{block.text}</h4>;
        return <p key={index} className={block.kind === "li" ? "terms-item" : undefined}>{block.text}</p>;
      })}
    </div>
  );
}

function TermsText() {
  return <DocumentText markdown={termsMarkdown} label={t("用户协议全文")} />;
}

function PrivacyText() {
  return <DocumentText markdown={privacyForLanguage(privacyMarkdown, currentLanguage() === "en")} label={t("隐私政策全文")} />;
}

/**
 * First start: the agreement in full, one primary action to accept it, and a way to decline. Declining keeps the
 * app closed off: nothing else is usable until the terms are accepted.
 */
export function TermsConsent({ onAccept }: { onAccept: () => void }) {
  const [declined, setDeclined] = useState(false);
  const [privacy, setPrivacy] = useState(false);
  return (
    <ModalDialog>
      <header className="terms-head">
        <ScrollText size={22} aria-hidden />
        <div>
          <Heading slot="title" level={2}>{t("使用前请阅读用户协议")}</Heading>
          <p>{t("仅在本人设备上使用；HTTPS 解密仅用于本人流量，根证书由本人手动信任；不得用于入侵、窃取数据或绕过他人的安全机制。")}</p>
          <p>{t("流量仅在本机按规则转发到所配置的服务器，我们不查看也不收集。App 不含统计、广告或崩溃上报 SDK，数据仅保存在本设备上。")}</p>
        </div>
      </header>
      {privacy ? <PrivacyText /> : <TermsText />}
      {declined && <p className="terms-declined" role="status">{t("未同意则无法使用 AnixOps Net。可关闭窗口并卸载，或重新阅读后同意。")}</p>}
      <footer className="terms-actions">
        <button className="secondary" onClick={() => setPrivacy(!privacy)}>{privacy ? t("阅读用户协议") : t("阅读隐私政策")}</button>
        <button className="secondary" onClick={() => setDeclined(true)}>{t("不同意")}</button>
        <button className="primary" onClick={onAccept}>{t("同意并继续")}</button>
      </footer>
    </ModalDialog>
  );
}

/** Settings: the same agreement to read again; closing it changes nothing. */
export function TermsViewer({ onClose }: { onClose: () => void }) {
  return (
    <ModalDialog onClose={onClose}>
      <header className="terms-head">
        <ScrollText size={22} aria-hidden />
        <div>
          <Heading slot="title" level={2}>{t("用户协议")}</Heading>
          <p>{t("版本 {version}，已同意。", { version: termsVersion })}</p>
        </div>
      </header>
      <TermsText />
      <footer className="terms-actions">
        <button className="secondary" autoFocus onClick={onClose}>{t("关闭")}</button>
      </footer>
    </ModalDialog>
  );
}

/** Settings: the privacy policy this build ships with. */
export function PrivacyViewer({ onClose }: { onClose: () => void }) {
  return (
    <ModalDialog onClose={onClose}>
      <header className="terms-head">
        <ShieldCheck size={22} aria-hidden />
        <div>
          <Heading slot="title" level={2}>{t("隐私政策")}</Heading>
          <p>{t("不收集数据：无统计、广告或崩溃上报 SDK，数据仅保存在本设备上。")}</p>
        </div>
      </header>
      <PrivacyText />
      <footer className="terms-actions">
        <button className="secondary" autoFocus onClick={onClose}>{t("关闭")}</button>
      </footer>
    </ModalDialog>
  );
}
