export const palette = {
  light: {
    background: "#f9fafb",
    surface: "#ffffff",
    surfaceMuted: "#e5e7eb",
    border: "#e5e7eb",
    text: "#111827",
    textMuted: "#6b7280",
    primary: "#2563eb",
    danger: "#dc2626",
    onPrimary: "#ffffff",
  },
  dark: {
    background: "#030712",
    surface: "#111827",
    surfaceMuted: "#1f2937",
    border: "#374151",
    text: "#f9fafb",
    textMuted: "#9ca3af",
    primary: "#60a5fa",
    danger: "#f87171",
    onPrimary: "#030712",
  },
} as const;

export type ThemeMode = keyof typeof palette;
export type ThemeColors = (typeof palette)[ThemeMode];

export function getTheme(mode: ThemeMode): ThemeColors {
  return palette[mode];
}
