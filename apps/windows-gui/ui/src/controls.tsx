import { Check, ChevronsUpDown, Ellipsis } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import {
  Button,
  Dialog,
  Header,
  ListBox,
  ListBoxItem,
  ListBoxSection,
  Menu,
  MenuItem,
  MenuTrigger,
  Modal,
  ModalOverlay,
  Popover,
  Select as AriaSelect,
  SelectValue,
  Separator,
  Tooltip,
  TooltipTrigger,
} from "react-aria-components";

// The controls whose behaviour is more than their looks: pop-up lists, menus, tooltips and dialogs. Keyboard
// navigation, type-ahead, focus handling, placement inside the window and screen-reader roles come from React Aria
// Components; everything visible is drawn by styles.css. Pages use these instead of the browser's own <select>,
// `title` tooltips and hand-placed overlays.

/** One entry of a `Select`. */
export type Choice<T extends string = string> = { value: T; label: string; disabled?: boolean };

/** Entries of a `Select` under a heading, for lists that come in groups (nodes by subscription). */
export type ChoiceSection<T extends string = string> = { title: string; choices: Array<Choice<T>> };

function option<T extends string>(choice: Choice<T>) {
  return (
    <ListBoxItem key={choice.value} id={choice.value} textValue={choice.label} isDisabled={choice.disabled} className="option">
      <Check className="check" size={12} aria-hidden="true" />
      <span>{choice.label}</span>
    </ListBoxItem>
  );
}

/**
 * A pop-up list for picking one of several values. `value` that matches no entry (or is empty) shows `placeholder`.
 * `children` replaces the selected entry's label on the button, for a trigger that shows more than a name.
 */
export function Select<T extends string>({ label, value, onChange, choices = [], sections, disabled = false, placeholder, className = "", children }: {
  label: string;
  value: T | "" | undefined;
  onChange: (value: T) => void;
  choices?: Array<Choice<T>>;
  sections?: Array<ChoiceSection<T>>;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <AriaSelect
      className="select"
      aria-label={label}
      selectedKey={value ? value : null}
      onSelectionChange={(key) => {
        if (key !== null && key !== value) onChange(String(key) as T);
      }}
      isDisabled={disabled}
      placeholder={placeholder}
    >
      <Button className={`select-trigger ${className}`}>
        {children ?? (
          // Only the entry's text: its own content also carries the check mark of the list.
          <SelectValue className="select-value">
            {({ isPlaceholder, selectedText, defaultChildren }) => (isPlaceholder ? defaultChildren : selectedText)}
          </SelectValue>
        )}
        <ChevronsUpDown className="select-arrows" size={12} aria-hidden="true" />
      </Button>
      <Popover className="popover" offset={4}>
        <ListBox className="listbox">
          {sections
            ? sections.map((section) => (
              <ListBoxSection key={section.title} id={`section:${section.title}`} className="option-section">
                <Header>{section.title}</Header>
                {section.choices.map(option)}
              </ListBoxSection>
            ))
            : choices.map(option)}
        </ListBox>
      </Popover>
    </AriaSelect>
  );
}

/** A short label that appears next to its control on hover or keyboard focus. The child must be a React Aria control. */
export function Tip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <TooltipTrigger delay={500} closeDelay={0}>
      {children}
      <Tooltip className="tooltip" offset={6}>{label}</Tooltip>
    </TooltipTrigger>
  );
}

export function IconButton({ label, children, onClick, disabled = false }: {
  label: string;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <Tip label={label}>
      <Button className="icon-button" aria-label={label} onPress={() => onClick()} isDisabled={disabled}>{children}</Button>
    </Tip>
  );
}

/** One entry of a `MoreMenu`. A `danger` entry is listed last, in red, and with `confirm` asks once more in place. */
export type MenuEntry = {
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  disabled?: boolean;
  danger?: boolean;
  /** Shown instead of the label after the first choice; choosing it again runs the action. Names what is lost. */
  confirm?: string;
};

/**
 * The "more" menu of a row or a section header (AnixOps Design, actions S3 and D1): everything beyond the one
 * labelled and the one icon action goes here, destructive entries last after a separator.
 */
export function MoreMenu({ label, items, disabled = false }: { label: string; items: MenuEntry[]; disabled?: boolean }) {
  const [confirming, setConfirming] = useState<string | null>(null);
  const everyday = items.filter((item) => !item.danger);
  const dangerous = items.filter((item) => item.danger);

  const entry = (item: MenuEntry) => {
    const asking = confirming === item.label;
    const Icon = item.icon;
    const asksFirst = Boolean(item.confirm) && !asking;
    return (
      <MenuItem
        key={item.label}
        id={item.label}
        textValue={item.label}
        className={item.danger ? "menu-item danger" : "menu-item"}
        isDisabled={item.disabled}
        // The first choice of an entry that asks keeps the menu open and turns the entry into the question.
        shouldCloseOnSelect={!asksFirst}
        onAction={() => {
          if (asksFirst) {
            setConfirming(item.label);
            return;
          }
          item.onSelect();
        }}
      >
        {Icon && <Icon size={14} aria-hidden="true" />}
        <span>{asking ? item.confirm : item.label}</span>
      </MenuItem>
    );
  };

  return (
    <MenuTrigger onOpenChange={() => setConfirming(null)}>
      <Button className="icon-button" aria-label={label} isDisabled={disabled}>
        <Ellipsis size={15} />
      </Button>
      <Popover className="popover" placement="bottom end" offset={4}>
        <Menu className="menu" aria-label={label}>
          {everyday.map(entry)}
          {everyday.length > 0 && dangerous.length > 0 && <Separator className="menu-separator" />}
          {dangerous.map(entry)}
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}

/**
 * A dialog over the whole window. Focus stays inside it while it is open and returns to where it was afterwards.
 * With `onClose`, Escape closes it; without, it stays until one of its own buttons is used. Give the title a
 * `<Heading slot="title">` so the dialog is announced by it.
 */
export function ModalDialog({ onClose, className = "", children }: { onClose?: () => void; className?: string; children: ReactNode }) {
  return (
    <ModalOverlay
      className="terms-overlay"
      isOpen
      isKeyboardDismissDisabled={!onClose}
      onOpenChange={(open) => {
        if (!open) onClose?.();
      }}
    >
      <Modal className={`terms-dialog ${className}`}>
        <Dialog className="dialog">{children}</Dialog>
      </Modal>
    </ModalOverlay>
  );
}
