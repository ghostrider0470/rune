import React from "react";
import { Pressable, SafeAreaView, Text, View } from "react-native";
import { useNotificationStore } from "../../src/store/notification-store";
import { useAppStore } from "../../src/store/app-store";

function ToggleCard({
  title,
  description,
  enabled,
  onToggle,
}: {
  title: string;
  description: string;
  enabled: boolean;
  onToggle: () => void;
}) {
  return (
    <View
      style={{
        backgroundColor: "#fff",
        borderColor: "#e5e7eb",
        borderRadius: 16,
        borderWidth: 1,
        gap: 8,
        padding: 16,
      }}
    >
      <Text style={{ color: "#111827", fontSize: 18, fontWeight: "700" }}>{title}</Text>
      <Text style={{ color: "#6b7280" }}>{description}</Text>
      <Pressable
        onPress={onToggle}
        style={{
          alignItems: "center",
          backgroundColor: enabled ? "#2563eb" : "#e5e7eb",
          borderRadius: 12,
          paddingVertical: 12,
        }}
      >
        <Text style={{ color: enabled ? "#fff" : "#111827", fontWeight: "700" }}>
          {enabled ? "Enabled" : "Disabled"}
        </Text>
      </Pressable>
    </View>
  );
}

export default function SettingsScreen() {
  const approvalsEnabled = useNotificationStore((state) => state.approvalsEnabled);
  const setApprovalsEnabled = useNotificationStore((state) => state.setApprovalsEnabled);
  const autoTtsEnabled = useAppStore((state) => state.autoTtsEnabled);
  const setAutoTtsEnabled = useAppStore((state) => state.setAutoTtsEnabled);

  return (
    <SafeAreaView style={{ backgroundColor: "#f9fafb", flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: "#e5e7eb", gap: 4, padding: 16 }}>
        <Text style={{ fontSize: 24, fontWeight: "700" }}>Settings</Text>
        <Text style={{ color: "#6b7280" }}>Notification and voice preferences for operator alerts.</Text>
      </View>

      <View style={{ gap: 12, padding: 16 }}>
        <ToggleCard
          title="Approval notifications"
          description="Fire a local notification when the pending approval count increases."
          enabled={approvalsEnabled}
          onToggle={() => setApprovalsEnabled(!approvalsEnabled)}
        />

        <ToggleCard
          title="Auto-play TTS"
          description="Automatically play synthesized assistant audio replies when voice mode is enabled."
          enabled={autoTtsEnabled}
          onToggle={() => setAutoTtsEnabled(!autoTtsEnabled)}
        />
      </View>
    </SafeAreaView>
  );
}
