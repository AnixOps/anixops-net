import { ChevronRight, Ellipsis, LoaderCircle } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
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

export function IconButton({ label, children, onClick, disabled = false }: {
  label: string;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return <button className="icon-button" title={label} aria-label={label} onClick={onClick} disabled={disabled}>{children}</button>;
}

/** One entry of a `MoreMenu`. A `danger` entry is listed last, in red, and with `confirm` asks once more in place. */
export type MenuItem = {
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  disabled?: boolean;
  danger?: boolean;
  /** Shown instead of the label after the first click; the second click runs the action. Names what is lost. */
  confirm?: string;
};

/**
 * The "more" menu of a row or a section header (AnixOps Design, actions S3 and D1): everything beyond the one
 * labelled and the one icon action goes here, destructive entries last after a separator.
 */
export function MoreMenu({ label, items, disabled = false }: { label: string; items: MenuItem[]; disabled?: boolean }) {
  const [place, setPlace] = useState<{ top: number; right: number } | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const root = useRef<HTMLSpanElement>(null);
  const everyday = items.filter((item) => !item.danger);
  const dangerous = items.filter((item) => item.danger);

  useEffect(() => {
    if (!place) return;
    const close = () => setPlace(null);
    const outside = (event: Event) => {
      if (!root.current?.contains(event.target as Node)) close();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", key);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", key);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [place]);

  const toggle = () => {
    setConfirming(null);
    if (place) {
      setPlace(null);
      return;
    }
    const box = root.current?.getBoundingClientRect();
    // Fixed to the window, so a rounded group that clips its rows does not clip the menu.
    if (box) setPlace({ top: box.bottom + 4, right: window.innerWidth - box.right });
  };

  const entry = (item: MenuItem) => {
    const asking = confirming === item.label;
    const Icon = item.icon;
    return (
      <button
        key={item.label}
        role="menuitem"
        className={item.danger ? "menu-item danger" : "menu-item"}
        disabled={item.disabled}
        onClick={() => {
          if (item.confirm && !asking) {
            setConfirming(item.label);
            return;
          }
          setPlace(null);
          item.onSelect();
        }}
      >
        {Icon && <Icon size={14} aria-hidden="true" />}
        <span>{asking ? item.confirm : item.label}</span>
      </button>
    );
  };

  return (
    <span className="more-menu" ref={root}>
      <button className="icon-button" title={label} aria-label={label} aria-haspopup="menu" aria-expanded={Boolean(place)} disabled={disabled} onClick={toggle}>
        <Ellipsis size={15} />
      </button>
      {place && (
        <span className="menu" role="menu" style={{ top: place.top, right: place.right }}>
          {everyday.map(entry)}
          {everyday.length > 0 && dangerous.length > 0 && <span className="menu-separator" role="separator" />}
          {dangerous.map(entry)}
        </span>
      )}
    </span>
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
