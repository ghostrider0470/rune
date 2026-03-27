import { create } from "zustand";

export type ThemePreference = "system" | "light" | "dark";

export interface AppPreferences {
  autoTtsEnabled: boolean;
  gatewayUrl: string | null;
  themePreference: ThemePreference;
}

interface AppState extends AppPreferences {
  setAutoTtsEnabled: (autoTtsEnabled: boolean) => void;
  setGatewayUrl: (gatewayUrl: string | null) => void;
  setThemePreference: (themePreference: ThemePreference) => void;
}

export const useAppStore = create<AppState>((set) => ({
  autoTtsEnabled: false,
  gatewayUrl: null,
  themePreference: "dark",
  setAutoTtsEnabled: (autoTtsEnabled) => set({ autoTtsEnabled }),
  setGatewayUrl: (gatewayUrl) => set({ gatewayUrl }),
  setThemePreference: (themePreference) => set({ themePreference }),
}));

export function getGatewayUrl(): string | null {
  return useAppStore.getState().gatewayUrl;
}

export function setGatewayUrl(gatewayUrl: string | null): void {
  useAppStore.getState().setGatewayUrl(gatewayUrl);
}

export function getAutoTtsEnabled(): boolean {
  return useAppStore.getState().autoTtsEnabled;
}

export function setAutoTtsEnabled(autoTtsEnabled: boolean): void {
  useAppStore.getState().setAutoTtsEnabled(autoTtsEnabled);
}

export function getThemePreference(): ThemePreference {
  return useAppStore.getState().themePreference;
}

export function setThemePreference(themePreference: ThemePreference): void {
  useAppStore.getState().setThemePreference(themePreference);
}

export function getAppState(): AppPreferences {
  const { autoTtsEnabled, gatewayUrl, themePreference } = useAppStore.getState();
  return { autoTtsEnabled, gatewayUrl, themePreference };
}
