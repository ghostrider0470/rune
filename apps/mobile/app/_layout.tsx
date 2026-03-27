import React, { useEffect } from "react";
import { Slot, useRouter, useSegments } from "expo-router";
import { GatewayProvider, useGateway } from "../src/providers/GatewayProvider";

function AuthGate() {
  const router = useRouter();
  const segments = useSegments();
  const { authenticated } = useGateway();

  useEffect(() => {
    const inAuthGroup = segments[0] === "(auth)";
    if (!authenticated && !inAuthGroup) {
      router.replace("/(auth)/connect");
    }
  }, [authenticated, router, segments]);

  return <Slot />;
}

export default function RootLayout() {
  return (
    <GatewayProvider>
      <AuthGate />
    </GatewayProvider>
  );
}
