import React, { createContext, useContext, useEffect, useMemo, useState } from "react";
import { isAuthenticated } from "../lib/auth";
import { getGatewayUrl, useAppStore } from "../store/app-store";

interface GatewayContextValue {
  gatewayUrl: string | null;
  connected: boolean;
  authenticated: boolean;
  refreshAuth: () => Promise<boolean>;
}

const GatewayContext = createContext<GatewayContextValue | null>(null);

export function GatewayProvider({ children }: { children: React.ReactNode }) {
  const gatewayUrl = useAppStore((state) => state.gatewayUrl);
  const [authenticated, setAuthenticated] = useState(false);

  const refreshAuth = async (): Promise<boolean> => {
    const next = !!getGatewayUrl() && (await isAuthenticated());
    setAuthenticated(next);
    return next;
  };

  useEffect(() => {
    void refreshAuth();
  }, [gatewayUrl]);

  const value = useMemo<GatewayContextValue>(
    () => ({
      gatewayUrl,
      connected: !!gatewayUrl,
      authenticated,
      refreshAuth,
    }),
    [authenticated, gatewayUrl],
  );

  return <GatewayContext.Provider value={value}>{children}</GatewayContext.Provider>;
}

export function useGateway(): GatewayContextValue {
  const context = useContext(GatewayContext);
  if (!context) {
    throw new Error("useGateway must be used within GatewayProvider");
  }
  return context;
}
