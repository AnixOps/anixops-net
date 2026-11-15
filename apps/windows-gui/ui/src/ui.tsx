import { Check, ChevronRight, Copy, LoaderCircle } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { createContext, useContext, useEffect, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { Tone } from "./bridge";
import { t } from "./i18n";

/** Runs one named operation, shows its result message and refreshes the runtime state. */
export type Run = (operation: string, task: () => Promise<{ message: string }>) => Promise<void>;

/**
 * One section of a page, laid out like the grouped lists of macOS System Settings: the title and actions sit above
 * a rounded group, the description below it. Rows, fields and lists go inside the group.
 */
export function Card({ title, description, actions, children, className = "" }: {
  title?: string;
  description?: string;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card ${className}`}>
      {(title || actions) && (
        <header className="card-header">
          {title && <h2>{title}</h2>}
          {actions && <div className="card-actions">{actions}</div>}
        </header>
      )}
      {children && <div className="group">{children}</div>}
      {description && <p className="card-footer">{description}</p>}
    </section>
  );
}

/**
 * One card of a dashboard: a small caption with its icon and an optional action on the right, then the content.
 * Unlike `Card` it has no rows; the overview and the proxies page lay several of them out in a grid.
 */
export function Panel({ icon: Icon, title, action, children, className = "" }: {
  icon?: LucideIcon;
  title?: ReactNode;
  action?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel ${className}`}>
      {(title || action) && (
        <header className="panel-head">
          {Icon && <Icon size={12} aria-hidden="true" />}
          {title && <h2>{title}</h2>}
          {action && <span className="panel-action">{action}</span>}
        </header>
      )}
      {children}
    </section>
  );
}

/** A name's first letter on a colored tile, for sites and programs that have no icon of their own. */
export function LetterTile({ name, color }: { name: string; color: TileColor }) {
  return <span className={`tile letter ${color}`} aria-hidden="true">{[...name][0]?.toUpperCase() ?? "?"}</span>;
}

/** The tile colors for row and sidebar icons, after the colored glyph tiles of macOS System Settings. */
export type TileColor = "indigo" | "sky" | "green" | "orange" | "purple" | "pink" | "teal" | "gray" | "red";

export function Tile({ icon: Icon, color }: { icon: LucideIcon; color: TileColor }) {
  return <span className={`tile ${color}`} aria-hidden="true"><Icon size={14} strokeWidth={2} /></span>;
}

/**
 * One row of a group: an optional icon tile, the label with an optional hint, then whatever goes on the right.
 * With `onClick` the whole row is a button and shows a chevron.
 */
export function Row({ icon, color = "gray", label, hint, value, children, onClick, disabled = false, selected = false, className = "" }: {
  icon?: LucideIcon;
  color?: TileColor;
  label: ReactNode;
  hint?: ReactNode;
  value?: ReactNode;
  children?: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  selected?: boolean;
  className?: string;
}) {
  const content = (
    <>
      {icon && <Tile icon={icon} color={color} />}
      <span className="row-label">
        <strong>{label}</strong>
        {hint && <small>{hint}</small>}
      </span>
      {value !== undefined && <span className="row-value">{value}</span>}
      {children}
      {onClick && <ChevronRight size={15} className="chevron" aria-hidden="true" />}
    </>
  );
  const classes = `row ${icon ? "with-icon" : ""} ${selected ? "selected" : ""} ${className}`;
  return onClick
    ? <button className={`${classes} interactive`} onClick={onClick} disabled={disabled}>{content}</button>
    : <div className={classes}>{content}</div>;
}

/** An on/off switch; `tone="go"` is the green of the connection switch. */
export function Switch({ label, checked, onChange, disabled = false, size = "regular", tone = "accent", busy = false }: {
  label: string;
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
  size?: "regular" | "large";
  tone?: "accent" | "go";
  busy?: boolean;
}) {
  return (
    <button
      className={`switch ${size} ${tone} ${checked ? "on" : ""} ${busy ? "busy" : ""}`}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onChange}
    >
      <i aria-hidden="true" />
    </button>
  );
}

export function StatusDot({ tone }: { tone: Tone | string }) {
  return <i className={`status-dot ${tone}`} aria-hidden="true" />;
}

export function StatusPill({ label, tone }: { label: string; tone: Tone | string }) {
  return <span className={`status-pill ${tone}`}>{label}</span>;
}

export { IconButton, MoreMenu, type MenuEntry as MenuItem } from "./controls";

/**
 * Copies text and shows a tick for a moment, so the click has an answer. `text` may be a function, read on click,
 * when working the text out is not free.
 */
export function CopyButton({ label, text, disabled = false }: { label: string; text: string | (() => string); disabled?: boolean }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1200);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const copy = () => {
    const value = typeof text === "function" ? text() : text;
    void navigator.clipboard?.writeText(value).then(() => setCopied(true)).catch(() => undefined);
  };
  return (
    <button className={copied ? "icon-button copied" : "icon-button"} title={copied ? t("已拷贝") : label} aria-label={label} onClick={copy} disabled={disabled}>
      {copied ? <Check size={14} /> : <Copy size={14} />}
      <span className="visually-hidden" role="status">{copied ? t("已拷贝") : ""}</span>
    </button>
  );
}

/**
 * Tabs over one pane (the request inspector): one is shown at a time. Left and right arrows move between them, as
 * the ARIA tabs pattern asks; only the selected tab is in the Tab order.
 */
export function Tabs<T extends string>({ label, options, value, onChange }: {
  label: string;
  options: Array<{ value: T; label: string; badge?: string | number }>;
  value: T;
  onChange: (value: T) => void;
}) {
  const move = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    const edge = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1 : null;
    if (step === 0 && edge === null) return;
    event.preventDefault();
    const at = options.findIndex((option) => option.value === value);
    const next = edge ?? (at + step + options.length) % options.length;
    onChange(options[next].value);
    const buttons = event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]');
    buttons[next]?.focus();
  };
  return (
    <div className="tabs" role="tablist" aria-label={label} onKeyDown={move}>
      {options.map((option) => (
        <button
          key={option.value}
          role="tab"
          aria-selected={value === option.value}
          tabIndex={value === option.value ? 0 : -1}
          className={value === option.value ? "selected" : ""}
          onClick={() => onChange(option.value)}
        >
          {option.label}
          {option.badge !== undefined && <span className="tab-badge">{option.badge}</span>}
        </button>
      ))}
    </div>
  );
}

/**
 * A destructive action that stands on its own (actions D2): quiet red text that asks once more in place, naming what
 * is lost, before it runs. It goes back to its label when the pointer or focus leaves.
 */
export function ConfirmButton({ label, confirm, onConfirm, disabled = false }: { label: string; confirm: string; onConfirm: () => void; disabled?: boolean }) {
  const [asking, setAsking] = useState(false);
  return (
    <button
      className="link-button danger"
      disabled={disabled}
      onBlur={() => setAsking(false)}
      onMouseLeave={() => setAsking(false)}
      onClick={() => {
        if (!asking) {
          setAsking(true);
          return;
        }
        setAsking(false);
        onConfirm();
      }}
    >
      {asking ? confirm : label}
    </button>
  );
}

export function ToggleRow({ label, hint, checked, onChange, disabled = false, icon, color }: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
  icon?: LucideIcon;
  color?: TileColor;
}) {
  return (
    <Row icon={icon} color={color} label={label} hint={hint} className="toggle-row">
      <Switch label={label} checked={checked} onChange={onChange} disabled={disabled} />
    </Row>
  );
}

export function Segmented<T extends string>({ label, options, value, onChange, disabled = false, className = "" }: {
  label: string;
  options: Array<{ value: T; label: string; hint?: string }>;
  value: T | undefined;
  onChange: (value: T) => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <div className={`segmented ${className}`} role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          role="radio"
          aria-checked={value === option.value}
          className={value === option.value ? "selected" : ""}
          title={option.hint}
          disabled={disabled}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="field"><span>{label}</span>{children}</label>;
}

export function EmptyState({ icon: Icon, title, message, children }: {
  icon: LucideIcon;
  title: string;
  message: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon"><Icon size={24} strokeWidth={1.5} /></div>
      <strong>{title}</strong>
      <p>{message}</p>
      {children}
    </div>
  );
}

export function Loading() {
  return <div className="loading"><LoaderCircle className="spin" size={18} />{t("正在读取客户端状态")}</div>;
}

/** Where a page puts its own controls in the title bar: the element App renders next to the page title. */
export const ToolbarSlot = createContext<HTMLElement | null>(null);

/** Search fields and buttons that belong to the page as a whole, shown next to its title. */
export function PageTools({ children }: { children: ReactNode }) {
  const slot = useContext(ToolbarSlot);
  return slot ? createPortal(children, slot) : null;
}
