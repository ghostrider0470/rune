import { create } from "zustand";

export interface AppPreferences {
  gatewayUrl: string | null;
}

interface AppState extends AppPreferences {
  setGatewayUrl: (gatewayUrl: string | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  gatewayUrl: null,
  setGatewayUrl: (gatewayUrl) => set({ gatewayUrl }),
}));

export function getGatewayUrl(): string | null {
  return useAppStore.getState().gatewayUrl;
}

export function setGatewayUrl(gatewayUrl: string | null): void {
  useAppStore.getState().setGatewayUrl(gatewayUrl);
}

export function getAppState(): AppPreferences {
  const { gatewayUrl } = useAppStore.getState();
  return { gatewayUrl };
}
