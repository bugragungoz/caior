import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeSwitcher } from "./ThemeSwitcher";
import type { ThemeName } from "../types/manifest";

interface TitleBarProps {
  theme: ThemeName;
  onThemeChange: (theme: ThemeName) => void;
}

const appWindow = getCurrentWindow();

export function TitleBar({ theme, onThemeChange }: TitleBarProps) {
  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        height: "48px",
        padding: "0 16px 0 20px",
        background: "var(--md-surface-container-low)",
        borderBottom: "1px solid var(--md-outline-variant)",
        flexShrink: 0,
        userSelect: "none",
      }}
      data-tauri-drag-region
    >
      <div style={{ display: "flex", alignItems: "center", gap: "10px", flex: 1 }}>
        <span className="material-symbols-rounded" style={{ color: "var(--md-primary)", fontSize: "20px" }}>
          speed
        </span>
        <span
          style={{
            fontSize: "15px",
            fontWeight: 500,
            color: "var(--md-on-surface)",
            letterSpacing: "0.1px",
          }}
        >
          croxz runtimes
        </span>
      </div>

      <ThemeSwitcher current={theme} onChange={onThemeChange} />

      <div style={{ display: "flex", marginLeft: "16px" }}>
        <button
          onClick={() => appWindow.minimize()}
          title="Minimize"
          style={{
            width: "32px",
            height: "32px",
            borderRadius: "6px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--md-on-surface-variant)",
            transition: "background 0.15s",
          }}
          onMouseEnter={(e) =>
            ((e.currentTarget as HTMLElement).style.background =
              "var(--md-surface-container-high)")
          }
          onMouseLeave={(e) =>
            ((e.currentTarget as HTMLElement).style.background = "transparent")
          }
        >
          <span className="material-symbols-rounded sz-18">remove</span>
        </button>
        <button
          onClick={() => appWindow.close()}
          title="Close"
          style={{
            width: "32px",
            height: "32px",
            borderRadius: "6px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--md-on-surface-variant)",
            transition: "background 0.15s, color 0.15s",
          }}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLElement).style.background =
              "var(--md-error-container)";
            (e.currentTarget as HTMLElement).style.color = "var(--md-error)";
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = "transparent";
            (e.currentTarget as HTMLElement).style.color =
              "var(--md-on-surface-variant)";
          }}
        >
          <span className="material-symbols-rounded sz-18">close</span>
        </button>
      </div>
    </header>
  );
}
