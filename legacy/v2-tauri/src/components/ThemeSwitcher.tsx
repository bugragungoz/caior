import type { ThemeName } from "../types/manifest";

const THEMES: Array<{ name: ThemeName; label: string; dot: string }> = [
  { name: "violet", label: "Violet", dot: "#d0bcff" },
  { name: "ocean", label: "Ocean", dot: "#90caff" },
  { name: "ember", label: "Ember", dot: "#ffb4a9" },
  { name: "forest", label: "Forest", dot: "#78dc77" },
];

interface ThemeSwitcherProps {
  current: ThemeName;
  onChange: (theme: ThemeName) => void;
}

export function ThemeSwitcher({ current, onChange }: ThemeSwitcherProps) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "6px",
        padding: "4px 8px",
        background: "var(--md-surface-container)",
        borderRadius: "20px",
      }}
    >
      {THEMES.map((t) => (
        <button
          key={t.name}
          title={t.label}
          onClick={() => onChange(t.name)}
          style={{
            width: "16px",
            height: "16px",
            borderRadius: "50%",
            background: t.dot,
            border: current === t.name ? "2px solid var(--md-on-surface)" : "2px solid transparent",
            outline: current === t.name ? "1px solid var(--md-on-surface)" : "none",
            outlineOffset: "1px",
            transition: "border 0.15s, outline 0.15s",
            flexShrink: 0,
          }}
        />
      ))}
    </div>
  );
}
