import { Bug, Globe } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { AppMode, modes } from "./mode";
import { t } from "./i18n";

const icons: Record<AppMode, LucideIcon> = { proxy: Globe, debug: Bug };

/**
 * First launch, after the user agreement: how the app is going to be used. Either answer can be changed later in
 * Settings, so the dialog says so and asks nothing else.
 */
export function ModeChoice({ onChoose }: { onChoose: (mode: AppMode) => void }) {
  return (
    <div className="terms-overlay" role="dialog" aria-modal="true" aria-labelledby="mode-title">
      <div className="terms-dialog mode-dialog">
        <header className="terms-head">
          <div>
            <h2 id="mode-title">{t("你主要用 AnixOps Net 做什么？")}</h2>
            <p>{t("两种模式用同一个内核，只是界面不同。之后可以随时在设置里切换。")}</p>
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
      </div>
    </div>
  );
}
