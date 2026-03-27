import { useCallback, useEffect, useMemo, useState } from "react";
import type { MessageResponse, TranscriptEntry } from "../api/api-types";
import { apiFetch } from "../api/client";
import { getPayloadText, normalizeTranscriptKind } from "../lib/chat-utils";
import { enqueueChatMessage, getQueuedMessageCount, getQueuedChatMessages, removeQueuedChatMessage } from "../lib/offline-queue";
import { useSessionEvents } from "../lib/websocket";
import type { ChatMessageItem } from "../components/chat/ChatFlatList";

function mapEntry(entry: TranscriptEntry): ChatMessageItem {
  const role = normalizeTranscriptKind(entry.kind);
  return {
    id: entry.id,
    role: role === "user" || role === "assistant" ? role : "system",
    text: getPayloadText(entry.payload),
  };
}

async function postMessage(sessionId: string, content: string): Promise<MessageResponse> {
  const response = await apiFetch(`/v1/sessions/${sessionId}/messages`, {
    body: JSON.stringify({ content }),
    headers: { "Content-Type": "application/json" },
    method: "POST",
  });

  if (!response.ok) {
    throw new Error(`Failed to send message (${response.status})`);
  }

  return await response.json() as MessageResponse;
}

export function useChat(sessionId: string | null) {
  const [transcript, setTranscript] = useState<TranscriptEntry[]>([]);
  const [sending, setSending] = useState(false);
  const [loading, setLoading] = useState(false);
  const [queuedCount, setQueuedCount] = useState(0);
  const [queueDraining, setQueueDraining] = useState(false);
  const { events, connected } = useSessionEvents(sessionId ?? undefined, { enabled: !!sessionId });

  const refreshQueuedCount = useCallback(async () => {
    const count = await getQueuedMessageCount(sessionId);
    setQueuedCount(count);
  }, [sessionId]);

  const loadTranscript = useCallback(async () => {
    if (!sessionId) {
      setTranscript([]);
      return;
    }

    setLoading(true);
    try {
      const response = await apiFetch(`/v1/sessions/${sessionId}/transcript`);
      if (!response.ok) {
        throw new Error(`Failed to load transcript (${response.status})`);
      }
      const data = (await response.json()) as TranscriptEntry[];
      setTranscript(data);
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    void loadTranscript();
  }, [loadTranscript]);

  useEffect(() => {
    void refreshQueuedCount();
  }, [refreshQueuedCount]);

  useEffect(() => {
    if (!sessionId || events.length === 0) return;
    void loadTranscript();
  }, [events, loadTranscript, sessionId]);

  const drainQueuedMessages = useCallback(async () => {
    if (!sessionId || queueDraining) return;

    setQueueDraining(true);
    try {
      const queued = await getQueuedChatMessages();
      const pending = queued.filter((item) => item.sessionId === sessionId);

      for (const item of pending) {
        try {
          await postMessage(item.sessionId, item.content);
          await removeQueuedChatMessage(item.id);
        } catch {
          break;
        }
      }
    } finally {
      setQueueDraining(false);
      await refreshQueuedCount();
      await loadTranscript();
    }
  }, [loadTranscript, queueDraining, refreshQueuedCount, sessionId]);

  useEffect(() => {
    if (!connected || !sessionId) return;
    void drainQueuedMessages();
  }, [connected, drainQueuedMessages, sessionId]);

  const sendMessage = useCallback(
    async (content: string) => {
      if (!sessionId) {
        throw new Error("No active session");
      }

      setSending(true);
      try {
        try {
          await postMessage(sessionId, content);
          await loadTranscript();
          await refreshQueuedCount();
          return;
        } catch {
          await enqueueChatMessage(sessionId, content);
          await refreshQueuedCount();
          return;
        }
      } finally {
        setSending(false);
      }
    },
    [loadTranscript, refreshQueuedCount, sessionId],
  );

  const messages = useMemo(() => transcript.map(mapEntry), [transcript]);

  return {
    connected,
    loading,
    messages,
    queuedCount,
    queueDraining,
    sendMessage,
    sending,
  };
}
