import { useState, useEffect } from "react";
import type { ThemeName } from "../types/manifest";

const STORAGE_KEY = "croxz-theme";
const VALID_THEMES: ThemeName[] = ["violet", "ocean", "ember", "forest"];

function applyTheme(theme: ThemeName) {
  const html = document.documentElement;
  VALID_THEMES.forEach((t) => html.classList.remove(`theme-${t}`));
  html.classList.add(`theme-${theme}`);
}

export function useTheme() {
  const [theme, setTheme] = useState<ThemeName>(() => {
    const stored = localStorage.getItem(STORAGE_KEY) as ThemeName | null;
    return stored && VALID_THEMES.includes(stored) ? stored : "violet";
  });

  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(STORAGE_KEY, theme);
  }, [theme]);

  return { theme, setTheme };
}
