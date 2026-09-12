/** Theme presets and UI appearance helpers. */

export type ThemeId = "midnight" | "slate" | "light" | "ocean" | "warm";
export type AccentId = "blue" | "violet" | "amber" | "rose" | "teal";
export type DensityId = "comfortable" | "compact";
export type ContentLayoutId = "single" | "split";

export interface UiSettings {
  theme: ThemeId;
  accent: AccentId;
  density: DensityId;
  content_layout: ContentLayoutId;
}

export const DEFAULT_UI: UiSettings = {
  theme: "midnight",
  accent: "blue",
  density: "comfortable",
  content_layout: "split",
};

export const THEME_PRESETS: {
  id: ThemeId;
  name: string;
  blurb: string;
  swatch: string;
}[] = [
  { id: "midnight", name: "Midnight", blurb: "Blu profondo, pulito e professionale", swatch: "#1a2332" },
  { id: "slate", name: "Slate", blurb: "Grigio neutro, sobrio", swatch: "#1e2128" },
  { id: "light", name: "Chiaro", blurb: "Sfondo chiaro per ambienti luminosi", swatch: "#f4f6fa" },
  { id: "ocean", name: "Ocean", blurb: "Blu mare, rilassante", swatch: "#0f1c2e" },
  { id: "warm", name: "Warm", blurb: "Toni caldi e accoglienti", swatch: "#1c1814" },
];

export const ACCENT_PRESETS: { id: AccentId; name: string; swatch: string }[] = [
  { id: "blue", name: "Blu", swatch: "#5b8def" },
  { id: "violet", name: "Viola", swatch: "#9b7bff" },
  { id: "amber", name: "Ambra", swatch: "#e5a84a" },
  { id: "rose", name: "Rosa", swatch: "#e86b8a" },
  { id: "teal", name: "Teal", swatch: "#3db89a" },
];

/** Per-theme CSS variable maps applied on `document.documentElement`. */
const THEME_VARS: Record<ThemeId, Record<string, string>> = {
  midnight: {
    "--bg0": "#121820",
    "--bg1": "#1a2332",
    "--bg2": "#243044",
    "--main-bg": "#121820",
    "--ink": "#e8edf5",
    "--muted": "#8b9ab0",
    "--line": "rgba(232, 237, 245, 0.1)",
    "--shell-gradient-a": "rgba(91, 141, 239, 0.12)",
    "--shell-gradient-b": "rgba(155, 123, 255, 0.08)",
    "--panel-bg": "#1a2332",
    "--rail-bg": "#0e131a",
    "--user-bubble": "rgba(91, 141, 239, 0.16)",
    "--success-bg": "rgba(61, 184, 154, 0.12)",
    "--warn-bg": "rgba(229, 168, 74, 0.12)",
  },
  slate: {
    "--bg0": "#141519",
    "--bg1": "#1e2128",
    "--bg2": "#2a2f38",
    "--main-bg": "#141519",
    "--ink": "#eceef2",
    "--muted": "#949aa8",
    "--line": "rgba(236, 238, 242, 0.1)",
    "--shell-gradient-a": "rgba(148, 154, 168, 0.1)",
    "--shell-gradient-b": "rgba(91, 141, 239, 0.06)",
    "--panel-bg": "#1e2128",
    "--rail-bg": "#101216",
    "--user-bubble": "rgba(148, 154, 168, 0.14)",
    "--success-bg": "rgba(61, 184, 154, 0.12)",
    "--warn-bg": "rgba(229, 168, 74, 0.12)",
  },
  light: {
    "--bg0": "#eef1f6",
    "--bg1": "#ffffff",
    "--bg2": "#f0f3f8",
    "--ink": "#1a2233",
    "--muted": "#5c677a",
    "--line": "rgba(26, 34, 51, 0.1)",
    "--shell-gradient-a": "rgba(91, 141, 239, 0.08)",
    "--shell-gradient-b": "rgba(155, 123, 255, 0.05)",
    "--panel-bg": "rgba(255, 255, 255, 0.92)",
    "--rail-bg": "rgba(255, 255, 255, 0.95)",
    "--main-bg": "#eef1f6",
    "--user-bubble": "rgba(91, 141, 239, 0.1)",
    "--success-bg": "rgba(61, 184, 154, 0.1)",
    "--warn-bg": "rgba(229, 168, 74, 0.1)",
  },
  ocean: {
    "--bg0": "#0a1420",
    "--bg1": "#0f1c2e",
    "--bg2": "#162840",
    "--main-bg": "#0a1420",
    "--ink": "#dce8f5",
    "--muted": "#7a95b0",
    "--line": "rgba(220, 232, 245, 0.1)",
    "--shell-gradient-a": "rgba(61, 184, 217, 0.14)",
    "--shell-gradient-b": "rgba(91, 141, 239, 0.1)",
    "--panel-bg": "#0f1c2e",
    "--rail-bg": "#081018",
    "--user-bubble": "rgba(61, 184, 217, 0.14)",
    "--success-bg": "rgba(61, 184, 154, 0.12)",
    "--warn-bg": "rgba(229, 168, 74, 0.12)",
  },
  warm: {
    "--bg0": "#141210",
    "--bg1": "#1c1814",
    "--bg2": "#2a241e",
    "--main-bg": "#141210",
    "--ink": "#f0ebe4",
    "--muted": "#a89a8c",
    "--line": "rgba(240, 235, 228, 0.1)",
    "--shell-gradient-a": "rgba(229, 168, 74, 0.12)",
    "--shell-gradient-b": "rgba(232, 107, 138, 0.08)",
    "--panel-bg": "#1c1814",
    "--rail-bg": "#100e0c",
    "--user-bubble": "rgba(229, 168, 74, 0.12)",
    "--success-bg": "rgba(61, 184, 154, 0.12)",
    "--warn-bg": "rgba(229, 168, 74, 0.15)",
  },
};

const ACCENT_VARS: Record<AccentId, Record<string, string>> = {
  blue: { "--accent": "#5b8def", "--accent-2": "#3db89a", "--accent-soft": "rgba(91, 141, 239, 0.14)" },
  violet: { "--accent": "#9b7bff", "--accent-2": "#c084fc", "--accent-soft": "rgba(155, 123, 255, 0.14)" },
  amber: { "--accent": "#e5a84a", "--accent-2": "#f0c060", "--accent-soft": "rgba(229, 168, 74, 0.14)" },
  rose: { "--accent": "#e86b8a", "--accent-2": "#f093a8", "--accent-soft": "rgba(232, 107, 138, 0.14)" },
  teal: { "--accent": "#3db89a", "--accent-2": "#5b8def", "--accent-soft": "rgba(61, 184, 154, 0.14)" },
};

export function normalizeUi(raw: Partial<UiSettings> | null | undefined): UiSettings {
  const theme = THEME_PRESETS.some((t) => t.id === raw?.theme) ? (raw!.theme as ThemeId) : DEFAULT_UI.theme;
  const accent = ACCENT_PRESETS.some((a) => a.id === raw?.accent) ? (raw!.accent as AccentId) : DEFAULT_UI.accent;
  const density =
    raw?.density === "compact" || raw?.density === "comfortable" ? raw.density : DEFAULT_UI.density;
  const content_layout =
    raw?.content_layout === "single" || raw?.content_layout === "split"
      ? raw.content_layout
      : DEFAULT_UI.content_layout;
  return { theme, accent, density, content_layout };
}

export function applyUiSettings(ui: UiSettings): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.dataset.theme = ui.theme;
  root.dataset.accent = ui.accent;
  root.dataset.density = ui.density;
  root.dataset.contentLayout = ui.content_layout;

  const themeVars = THEME_VARS[ui.theme] ?? THEME_VARS.midnight;
  const accentVars = ACCENT_VARS[ui.accent] ?? ACCENT_VARS.blue;
  const merged = { ...themeVars, ...accentVars };

  if (!merged["--main-bg"]) {
    merged["--main-bg"] = merged["--bg0"];
  }

  for (const [key, val] of Object.entries(merged)) {
    root.style.setProperty(key, val);
  }

  root.style.colorScheme = ui.theme === "light" ? "light" : "dark";

  void syncMainWindowBackground(ui.theme);
}

async function syncMainWindowBackground(theme: ThemeId): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("apply_main_window_theme", { theme });
  } catch {
    /* browser preview */
  }
}
