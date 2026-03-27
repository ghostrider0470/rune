import { create } from "zustand";

export interface AppPreferences {
  autoTtsEnabled: boolean;
  gatewayUrl: string | null;
}

interface AppState extends AppPreferences {
  setAutoTtsEnabled: (autoTtsEnabled: boolean) => void;
  setGatewayUrl: (gatewayUrl: string | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  autoTtsEnabled: false,
  gatewayUrl: null,
  setAutoTtsEnabled: (autoTtsEnabled) => set({ autoTtsEnabled }),
  setGatewayUrl: (gatewayUrl) => set({ gatewayUrl }),
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

export function getAppState(): AppPreferences {
  const { autoTtsEnabled, gatewayUrl } = useAppStore.getState();
  return { autoTtsEnabled, gatewayUrl };
}
