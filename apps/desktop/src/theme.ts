// Material Design 3 color scheme generated from a single seed color with Google's
// material-color-utilities, exposed as --md-sys-color-* CSS custom properties.
import {
  argbFromHex,
  hexFromArgb,
  Hct,
  MaterialDynamicColors,
  SchemeTonalSpot,
} from "@material/material-color-utilities";

export type ThemeMode = "system" | "light" | "dark";

export const ACCENTS = [
  { name: "Blue", hex: "#1a73e8" },
  { name: "Teal", hex: "#00897b" },
  { name: "Green", hex: "#188038" },
  { name: "Purple", hex: "#7e57c2" },
  { name: "Orange", hex: "#e8710a" },
  { name: "Pink", hex: "#d01884" },
] as const;

const TOKENS = [
  "primary",
  "onPrimary",
  "primaryContainer",
  "onPrimaryContainer",
  "secondary",
  "onSecondary",
  "secondaryContainer",
  "onSecondaryContainer",
  "tertiary",
  "onTertiary",
  "tertiaryContainer",
  "onTertiaryContainer",
  "error",
  "onError",
  "errorContainer",
  "onErrorContainer",
  "surface",
  "onSurface",
  "onSurfaceVariant",
  "surfaceContainerLowest",
  "surfaceContainerLow",
  "surfaceContainer",
  "surfaceContainerHigh",
  "surfaceContainerHighest",
  "outline",
  "outlineVariant",
  "inverseSurface",
  "inverseOnSurface",
  "inversePrimary",
] as const;

const kebab = (s: string) => s.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());

export function prefersDark(mode: ThemeMode): boolean {
  if (mode === "system") {
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
  }
  return mode === "dark";
}

export function applyTheme(seedHex: string, mode: ThemeMode) {
  const dark = prefersDark(mode);
  const scheme = new SchemeTonalSpot(Hct.fromInt(argbFromHex(seedHex)), dark, 0);
  const root = document.documentElement;
  for (const token of TOKENS) {
    const color = MaterialDynamicColors[token];
    root.style.setProperty(`--md-sys-color-${kebab(token)}`, hexFromArgb(color.getArgb(scheme)));
  }
  root.style.colorScheme = dark ? "dark" : "light";
  root.dataset.theme = dark ? "dark" : "light";
}

// Per-device UI preferences. Storage can be unavailable (private mode), so every access is guarded.
export interface Preferences {
  mode: ThemeMode;
  accent: string;
}

const KEY = "infermark.preferences";
export const DEFAULT_PREFERENCES: Preferences = { mode: "system", accent: ACCENTS[0].hex };

export function loadPreferences(): Preferences {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...DEFAULT_PREFERENCES, ...JSON.parse(raw) };
  } catch {
    /* fall back to defaults */
  }
  return DEFAULT_PREFERENCES;
}

export function savePreferences(p: Preferences) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    /* not persisted; still applied for this session */
  }
}
