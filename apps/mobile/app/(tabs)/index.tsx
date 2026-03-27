import React from "react";
import { ActivityIndicator, Pressable, SafeAreaView, ScrollView, Text, View } from "react-native";
import { ChatFlatList } from "../../src/components/chat/ChatFlatList";
import { ChatInputBar } from "../../src/components/chat/ChatInputBar";
import { useChat } from "../../src/hooks/use-chat";
import { useSessions } from "../../src/hooks/use-sessions";

export default function ChatScreen() {
  const { sessions, activeSessionId, setActiveSessionId, createSession, loading: sessionsLoading } = useSessions();
  const { messages, sendMessage, sending, loading: transcriptLoading } = useChat(activeSessionId);

  return (
    <SafeAreaView style={{ backgroundColor: "#f9fafb", flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: "#e5e7eb", gap: 12, padding: 12 }}>
        <Text style={{ fontSize: 24, fontWeight: "700" }}>Rune Chat</Text>
        <ScrollView horizontal showsHorizontalScrollIndicator={false}>
          <View style={{ flexDirection: "row", gap: 8 }}>
            {sessions.map((session) => {
              const active = session.id === activeSessionId;
              return (
                <Pressable
                  key={session.id}
                  onPress={() => setActiveSessionId(session.id)}
                  style={{
                    backgroundColor: active ? "#2563eb" : "#e5e7eb",
                    borderRadius: 999,
                    paddingHorizontal: 12,
                    paddingVertical: 8,
                  }}
                >
                  <Text style={{ color: active ? "#fff" : "#111827" }}>
                    {session.preview || session.id.slice(0, 8)}
                  </Text>
                </Pressable>
              );
            })}
            <Pressable
              onPress={() => void createSession()}
              style={{ backgroundColor: "#111827", borderRadius: 999, paddingHorizontal: 12, paddingVertical: 8 }}
            >
              <Text style={{ color: "#fff" }}>+ New</Text>
            </Pressable>
          </View>
        </ScrollView>
      </View>

      {sessionsLoading || transcriptLoading ? (
        <View style={{ alignItems: "center", flex: 1, justifyContent: "center" }}>
          <ActivityIndicator />
        </View>
      ) : (
        <ChatFlatList messages={messages} />
      )}

      <ChatInputBar disabled={!activeSessionId} sending={sending} onSend={sendMessage} />
    </SafeAreaView>
  );
}
