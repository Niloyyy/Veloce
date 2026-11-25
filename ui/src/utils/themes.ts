import type { ThemeId } from "../types/veloce";

export interface ThemeOption {
  id: ThemeId;
  label: string;
  bg: string;
  accent: string;
}

export const THEME_OPTIONS: ThemeOption[] = [
  { id: "dark", label: "Obsidian", bg: "#0c0f14", accent: "#3b82f6" },
  { id: "light", label: "Daylight", bg: "#f4f6fa", accent: "#2563eb" },
  { id: "midnight", label: "Midnight OLED", bg: "#000000", accent: "#38bdf8" },
  { id: "nord", label: "Nord Frost", bg: "#1b222c", accent: "#5eead4" },
  { id: "emerald", label: "Emerald", bg: "#09130f", accent: "#10b981" },
  { id: "sunset", label: "Sunset Ember", bg: "#140e12", accent: "#f97316" },
];

export function nextThemeId(current: ThemeId): ThemeId {
  const idx = THEME_OPTIONS.findIndex((t) => t.id === current);
  return THEME_OPTIONS[(idx + 1) % THEME_OPTIONS.length].id;
}

export function themeLabel(id: ThemeId): string {
  return THEME_OPTIONS.find((t) => t.id === id)?.label ?? "Obsidian";
}
