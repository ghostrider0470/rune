import React from "react";
import { Text, View } from "react-native";

interface MessageBubbleProps {
  role: "user" | "assistant" | "system";
  text: string;
}

export function MessageBubble({ role, text }: MessageBubbleProps) {
  const isUser = role === "user";

  return (
    <View
      style={{
        alignSelf: isUser ? "flex-end" : "flex-start",
        backgroundColor: isUser ? "#2563eb" : "#e5e7eb",
        borderRadius: 16,
        marginBottom: 8,
        maxWidth: "88%",
        paddingHorizontal: 12,
        paddingVertical: 10,
      }}
    >
      <Text style={{ color: isUser ? "#fff" : "#111827" }}>{text}</Text>
    </View>
  );
}
