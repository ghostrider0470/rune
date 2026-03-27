import React, { useState } from "react";
import { Button, Text, TextInput, View } from "react-native";
import { router } from "expo-router";
import { setToken } from "../../src/lib/auth";
import { setGatewayUrl } from "../../src/store/app-store";
import { useTheme } from "../../src/hooks/use-theme";

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
  const colors = useTheme();
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
    <View style={{ backgroundColor: colors.background, flex: 1, justifyContent: "center", padding: 24, gap: 12 }}>
      <Text style={{ color: colors.text, fontSize: 24, fontWeight: "600" }}>Connect to Rune Gateway</Text>
      <TextInput
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="https://gateway.example.com"
        placeholderTextColor={colors.textMuted}
        value={gatewayUrl}
        onChangeText={setGatewayUrlInput}
        style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, padding: 12, color: colors.text }}
      />
      <TextInput
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="Bearer token"
        placeholderTextColor={colors.textMuted}
        secureTextEntry
        value={bearerToken}
        onChangeText={setBearerToken}
        style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, padding: 12, color: colors.text }}
      />
      {error ? <Text style={{ color: colors.danger }}>{error}</Text> : null}
      <Button title={submitting ? "Connecting..." : "Connect"} onPress={() => void onSubmit()} />
    </View>
  );
}
