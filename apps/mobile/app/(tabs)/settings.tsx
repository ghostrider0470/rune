import React from "react";
import { Pressable, SafeAreaView, Text, View } from "react-native";
import { useNotificationStore } from "../../src/store/notification-store";

export default function SettingsScreen() {
  const approvalsEnabled = useNotificationStore((state) => state.approvalsEnabled);
  const setApprovalsEnabled = useNotificationStore((state) => state.setApprovalsEnabled);

  return (
    <SafeAreaView style={{ backgroundColor: "#f9fafb", flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: "#e5e7eb", gap: 4, padding: 16 }}>
        <Text style={{ fontSize: 24, fontWeight: "700" }}>Settings</Text>
        <Text style={{ color: "#6b7280" }}>Notification preferences for operator alerts.</Text>
      </View>

      <View style={{ gap: 12, padding: 16 }}>
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
          <Text style={{ color: "#111827", fontSize: 18, fontWeight: "700" }}>Approval notifications</Text>
          <Text style={{ color: "#6b7280" }}>
            Fire a local notification when the pending approval count increases.
          </Text>
          <Pressable
            onPress={() => setApprovalsEnabled(!approvalsEnabled)}
            style={{
              alignItems: "center",
              backgroundColor: approvalsEnabled ? "#2563eb" : "#e5e7eb",
              borderRadius: 12,
              paddingVertical: 12,
            }}
          >
            <Text style={{ color: approvalsEnabled ? "#fff" : "#111827", fontWeight: "700" }}>
              {approvalsEnabled ? "Enabled" : "Disabled"}
            </Text>
          </Pressable>
        </View>
      </View>
    </SafeAreaView>
  );
}
