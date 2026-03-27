import React, { useState } from "react";
import { Button, Text, TextInput, View } from "react-native";
import { router } from "expo-router";
import { setToken } from "../../src/lib/auth";
import { setGatewayUrl } from "../../src/store/app-store";

async function verifyGateway(baseUrl: string): Promise<void> {
  const normalized = baseUrl.replace(/\/+$/, "");
  const response = await fetch(`${normalized}/health`, {
    headers: { Accept: "application/json" },
  });

  if (!response.ok) {
    throw new Error(`Gateway health check failed with ${response.status}`);
  }
}

export default function ConnectScreen() {
  const [gatewayUrl, setGatewayUrlInput] = useState("");
  const [bearerToken, setBearerToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const onSubmit = async () => {
    try {
      setSubmitting(true);
      setError(null);
      await verifyGateway(gatewayUrl);
      setGatewayUrl(gatewayUrl);
      await setToken(bearerToken.trim());
      router.replace("/");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to connect to gateway");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <View style={{ flex: 1, justifyContent: "center", padding: 24, gap: 12 }}>
      <Text style={{ fontSize: 24, fontWeight: "600" }}>Connect to Rune Gateway</Text>
      <TextInput
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="https://gateway.example.com"
        value={gatewayUrl}
        onChangeText={setGatewayUrlInput}
        style={{ borderWidth: 1, borderColor: "#ccc", borderRadius: 8, padding: 12 }}
      />
      <TextInput
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="Bearer token"
        secureTextEntry
        value={bearerToken}
        onChangeText={setBearerToken}
        style={{ borderWidth: 1, borderColor: "#ccc", borderRadius: 8, padding: 12 }}
      />
      {error ? <Text style={{ color: "#c00" }}>{error}</Text> : null}
      <Button title={submitting ? "Connecting..." : "Connect"} onPress={() => void onSubmit()} />
    </View>
  );
}
