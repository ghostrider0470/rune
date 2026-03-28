import React from "react";
import { Text, View } from "react-native";
import { useTheme } from "../../hooks/use-theme";

interface MessageBubbleProps {
  role: "user" | "assistant" | "system";
  text: string;
}

export function MessageBubble({ role, text }: MessageBubbleProps) {
  const colors = useTheme();
  const isUser = role === "user";
  const isSystem = role === "system";
  const backgroundColor = isUser ? colors.primary : isSystem ? colors.surface : colors.surfaceMuted;
  const textColor = isUser ? colors.onPrimary : colors.text;
  const alignSelf = isUser ? "flex-end" : "flex-start" as const;

  return (
    <View
      style={{
        alignSelf,
        backgroundColor,
        borderColor: isSystem ? colors.border : backgroundColor,
        borderRadius: 16,
        borderWidth: isSystem ? 1 : 0,
        marginBottom: 8,
        maxWidth: "88%",
        paddingHorizontal: 12,
        paddingVertical: 10,
      }}
    >
      <Text
        style={{
          color: colors.textMuted,
          fontSize: 11,
          fontWeight: "700",
          marginBottom: 4,
          textTransform: "uppercase",
        }}
      >
        {role}
      </Text>
      <Text style={{ color: textColor }}>{text}</Text>
    </View>
  );
}
