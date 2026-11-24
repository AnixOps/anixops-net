import { Bug, Globe } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Heading } from "react-aria-components";
import { ModalDialog } from "./controls";
import { AppMode, modes } from "./mode";
import { t } from "./i18n";

const icons: Record<AppMode, LucideIcon> = { proxy: Globe, debug: Bug };

/**
 * First launch, after the user agreement: how the app is going to be used. Either answer can be changed later in
 * Settings, so the dialog says so and asks nothing else.
 */
export function ModeChoice({ onChoose }: { onChoose: (mode: AppMode) => void }) {
  return (
    <ModalDialog className="mode-dialog">
      <header className="terms-head">
        <div>
          <Heading slot="title" level={2}>{t("主要用途")}</Heading>
          <p>{t("两种模式使用同一内核，仅界面不同，可随时在设置中切换。")}</p>
        </div>
      </header>
      <div className="mode-options">
        {modes.map((option) => {
          const Icon = icons[option.value];
          return (
            <button key={option.value} className="mode-option" onClick={() => onChoose(option.value)}>
              <span className={`tile ${option.value === "debug" ? "pink" : "sky"}`} aria-hidden="true"><Icon size={18} /></span>
              <strong>{t(option.label)}</strong>
              <small>{t(option.hint)}</small>
            </button>
          );
        })}
      </div>
    </ModalDialog>
  );
}
