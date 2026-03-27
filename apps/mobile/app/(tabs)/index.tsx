import React from "react";
import { ActivityIndicator, Pressable, SafeAreaView, ScrollView, Text, View } from "react-native";
import { ChatFlatList } from "../../src/components/chat/ChatFlatList";
import { ChatInputBar } from "../../src/components/chat/ChatInputBar";
import { useChat } from "../../src/hooks/use-chat";
import { useSessions } from "../../src/hooks/use-sessions";
import { useTheme } from "../../src/hooks/use-theme";

export default function ChatScreen() {
  const colors = useTheme();
  const { sessions, activeSessionId, setActiveSessionId, createSession, loading: sessionsLoading } = useSessions();
  const { messages, sendMessage, sending, loading: transcriptLoading } = useChat(activeSessionId);

  return (
    <SafeAreaView style={{ backgroundColor: colors.background, flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: colors.border, gap: 12, padding: 12 }}>
        <Text style={{ color: colors.text, fontSize: 24, fontWeight: "700" }}>Rune Chat</Text>
        <ScrollView horizontal showsHorizontalScrollIndicator={false}>
          <View style={{ flexDirection: "row", gap: 8 }}>
            {sessions.map((session) => {
              const active = session.id === activeSessionId;
              return (
                <Pressable
                  key={session.id}
                  onPress={() => setActiveSessionId(session.id)}
                  style={{
                    backgroundColor: active ? colors.primary : colors.surfaceMuted,
                    borderRadius: 999,
                    paddingHorizontal: 12,
                    paddingVertical: 8,
                  }}
                >
                  <Text style={{ color: active ? colors.onPrimary : colors.text }}>
                    {session.preview || session.id.slice(0, 8)}
                  </Text>
                </Pressable>
              );
            })}
            <Pressable
              onPress={() => void createSession()}
              style={{ backgroundColor: colors.text, borderRadius: 999, paddingHorizontal: 12, paddingVertical: 8 }}
            >
              <Text style={{ color: colors.background }}>+ New</Text>
            </Pressable>
          </View>
        </ScrollView>
      </View>

      {sessionsLoading || transcriptLoading ? (
        <View style={{ alignItems: "center", flex: 1, justifyContent: "center" }}>
          <ActivityIndicator color={colors.primary} />
        </View>
      ) : (
        <ChatFlatList messages={messages} />
      )}

      <ChatInputBar disabled={!activeSessionId} sending={sending} onSend={sendMessage} />
    </SafeAreaView>
  );
}
