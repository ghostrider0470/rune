import React from "react";
import { Pressable, SafeAreaView, Text, View } from "react-native";
import { useNotificationStore } from "../../src/store/notification-store";
import { useAppStore } from "../../src/store/app-store";
import { useTheme } from "../../src/hooks/use-theme";

function ToggleCard({
  title,
  description,
  enabled,
  onToggle,
  colors,
}: {
  title: string;
  description: string;
  enabled: boolean;
  onToggle: () => void;
  colors: ReturnType<typeof useTheme>;
}) {
  return (
    <View
      style={{
        backgroundColor: colors.surface,
        borderColor: colors.border,
        borderRadius: 16,
        borderWidth: 1,
        gap: 8,
        padding: 16,
      }}
    >
      <Text style={{ color: colors.text, fontSize: 18, fontWeight: "700" }}>{title}</Text>
      <Text style={{ color: colors.textMuted }}>{description}</Text>
      <Pressable
        onPress={onToggle}
        style={{
          alignItems: "center",
          backgroundColor: enabled ? colors.primary : colors.surfaceMuted,
          borderRadius: 12,
          paddingVertical: 12,
        }}
      >
        <Text style={{ color: enabled ? colors.onPrimary : colors.text, fontWeight: "700" }}>
          {enabled ? "Enabled" : "Disabled"}
        </Text>
      </Pressable>
    </View>
  );
}

function ThemeCard() {
  const colors = useTheme();
  const themePreference = useAppStore((state) => state.themePreference);
  const setThemePreference = useAppStore((state) => state.setThemePreference);
  const options = [
    { label: "Dark", value: "dark" },
    { label: "Light", value: "light" },
    { label: "System", value: "system" },
  ] as const;

  return (
    <View
      style={{
        backgroundColor: colors.surface,
        borderColor: colors.border,
        borderRadius: 16,
        borderWidth: 1,
        gap: 12,
        padding: 16,
      }}
    >
      <Text style={{ color: colors.text, fontSize: 18, fontWeight: "700" }}>Theme</Text>
      <Text style={{ color: colors.textMuted }}>Dark is the default. Switch to light or follow the system theme.</Text>
      <View style={{ flexDirection: "row", gap: 8 }}>
        {options.map((option) => {
          const active = option.value === themePreference;
          return (
            <Pressable
              key={option.value}
              onPress={() => setThemePreference(option.value)}
              style={{
                backgroundColor: active ? colors.primary : colors.surfaceMuted,
                borderRadius: 999,
                paddingHorizontal: 12,
                paddingVertical: 10,
              }}
            >
              <Text style={{ color: active ? colors.onPrimary : colors.text, fontWeight: "700" }}>{option.label}</Text>
            </Pressable>
          );
        })}
      </View>
    </View>
  );
}

export default function SettingsScreen() {
  const colors = useTheme();
  const approvalsEnabled = useNotificationStore((state) => state.approvalsEnabled);
  const setApprovalsEnabled = useNotificationStore((state) => state.setApprovalsEnabled);
  const autoTtsEnabled = useAppStore((state) => state.autoTtsEnabled);
  const setAutoTtsEnabled = useAppStore((state) => state.setAutoTtsEnabled);

  return (
    <SafeAreaView style={{ backgroundColor: colors.background, flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: colors.border, gap: 4, padding: 16 }}>
        <Text style={{ color: colors.text, fontSize: 24, fontWeight: "700" }}>Settings</Text>
        <Text style={{ color: colors.textMuted }}>Notification, voice, and display preferences for operator alerts.</Text>
      </View>

      <View style={{ gap: 12, padding: 16 }}>
        <ThemeCard />

        <ToggleCard
          title="Approval notifications"
          description="Fire a local notification when the pending approval count increases."
          enabled={approvalsEnabled}
          onToggle={() => setApprovalsEnabled(!approvalsEnabled)}
          colors={colors}
        />

        <ToggleCard
          title="Auto-play TTS"
          description="Automatically play synthesized assistant audio replies when voice mode is enabled."
          enabled={autoTtsEnabled}
          onToggle={() => setAutoTtsEnabled(!autoTtsEnabled)}
          colors={colors}
        />
      </View>
    </SafeAreaView>
  );
}
