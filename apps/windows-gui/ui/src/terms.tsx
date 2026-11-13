import { useState } from "react";
import { ScrollText } from "lucide-react";
import termsMarkdown from "../../../../TERMS.md?raw";
import { t } from "./i18n";

// The user agreement (TERMS.md at the repository root) ships inside the app, so it can be read offline and is always
// the version this build was made with. Someone accepts it once; a new major version of the terms asks again.

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

function TermsText() {
  return (
    <div className="terms-text" tabIndex={0} aria-label={t("用户协议全文")}>
      {termsBlocks(termsMarkdown).map((block, index) => {
        if (block.kind === "h1") return <h3 key={index}>{block.text}</h3>;
        if (block.kind === "h2") return <h4 key={index}>{block.text}</h4>;
        return <p key={index} className={block.kind === "li" ? "terms-item" : undefined}>{block.text}</p>;
      })}
    </div>
  );
}

/**
 * First start: the agreement in full, one primary action to accept it, and a way to decline. Declining keeps the
 * app closed off: nothing else is usable until the terms are accepted.
 */
export function TermsConsent({ onAccept }: { onAccept: () => void }) {
  const [declined, setDeclined] = useState(false);
  return (
    <div className="terms-overlay" role="dialog" aria-modal="true" aria-labelledby="terms-title">
      <div className="terms-dialog">
        <header className="terms-head">
          <ScrollText size={22} aria-hidden />
          <div>
            <h2 id="terms-title">{t("使用前请阅读用户协议")}</h2>
            <p>{t("只在自己的设备上使用；HTTPS 解密只用于你自己的流量，根证书由你手动信任；不得用于入侵、窃取数据或绕过他人的安全机制。")}</p>
          </div>
        </header>
        <TermsText />
        {declined && <p className="terms-declined" role="status">{t("不同意就不能使用 AnixOps Net。你可以关闭窗口并卸载，或重新阅读后同意。")}</p>}
        <footer className="terms-actions">
          <button className="secondary" onClick={() => setDeclined(true)}>{t("不同意")}</button>
          <button className="primary" onClick={onAccept}>{t("同意并继续")}</button>
        </footer>
      </div>
    </div>
  );
}

/** Settings: the same agreement to read again; closing it changes nothing. */
export function TermsViewer({ onClose }: { onClose: () => void }) {
  return (
    <div className="terms-overlay" role="dialog" aria-modal="true" aria-labelledby="terms-title" onKeyDown={(event) => event.key === "Escape" && onClose()}>
      <div className="terms-dialog">
        <header className="terms-head">
          <ScrollText size={22} aria-hidden />
          <div>
            <h2 id="terms-title">{t("用户协议")}</h2>
            <p>{t("版本 {version}，已同意。", { version: termsVersion })}</p>
          </div>
        </header>
        <TermsText />
        <footer className="terms-actions">
          <button className="secondary" autoFocus onClick={onClose}>{t("关闭")}</button>
        </footer>
      </div>
    </div>
  );
}
