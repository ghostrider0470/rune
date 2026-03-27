import React from "react";
import { FlatList } from "react-native";
import { MessageBubble } from "./MessageBubble";

export interface ChatMessageItem {
  id: string;
  role: "user" | "assistant" | "system";
  text: string;
}

export function ChatFlatList({ messages }: { messages: ChatMessageItem[] }) {
  return (
    <FlatList
      contentContainerStyle={{ padding: 12 }}
      data={messages}
      keyExtractor={(item) => item.id}
      renderItem={({ item }) => <MessageBubble role={item.role} text={item.text} />}
    />
  );
}
