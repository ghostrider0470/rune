export interface AppPreferences {
  gatewayUrl: string | null;
}

let state: AppPreferences = {
  gatewayUrl: null,
};

export function getGatewayUrl(): string | null {
  return state.gatewayUrl;
}

export function setGatewayUrl(gatewayUrl: string | null): void {
  state = { ...state, gatewayUrl };
}

export function getAppState(): AppPreferences {
  return state;
}
