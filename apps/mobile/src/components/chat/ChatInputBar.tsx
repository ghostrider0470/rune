import React, { useState } from "react";
import { Pressable, Text, TextInput, View } from "react-native";

interface ChatInputBarProps {
  disabled?: boolean;
  sending?: boolean;
  onSend: (value: string) => Promise<void> | void;
}

export function ChatInputBar({ disabled = false, sending = false, onSend }: ChatInputBarProps) {
  const [value, setValue] = useState("");

  const submit = async () => {
    const next = value.trim();
    if (!next || disabled || sending) return;
    setValue("");
    await onSend(next);
  };

  return (
    <View style={{ borderTopWidth: 1, borderColor: "#e5e7eb", flexDirection: "row", gap: 8, padding: 12 }}>
      <TextInput
        editable={!disabled && !sending}
        onChangeText={setValue}
        placeholder="Message Rune"
        style={{
          backgroundColor: "#fff",
          borderColor: "#d1d5db",
          borderRadius: 12,
          borderWidth: 1,
          flex: 1,
          paddingHorizontal: 12,
          paddingVertical: 10,
        }}
        value={value}
      />
      <Pressable
        onPress={() => void submit()}
        style={{
          alignItems: "center",
          backgroundColor: disabled || sending ? "#93c5fd" : "#2563eb",
          borderRadius: 12,
          justifyContent: "center",
          paddingHorizontal: 16,
        }}
      >
        <Text style={{ color: "#fff", fontWeight: "600" }}>{sending ? "..." : "Send"}</Text>
      </Pressable>
    </View>
  );
}
