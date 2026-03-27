import { useCallback, useEffect, useMemo, useState } from "react";
import type { MessageResponse, TranscriptEntry } from "../api/api-types";
import { apiFetch } from "../api/client";
import { getPayloadText, normalizeTranscriptKind } from "../lib/chat-utils";
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

export function useChat(sessionId: string | null) {
  const [transcript, setTranscript] = useState<TranscriptEntry[]>([]);
  const [sending, setSending] = useState(false);
  const [loading, setLoading] = useState(false);
  const { events } = useSessionEvents(sessionId ?? undefined, { enabled: !!sessionId });

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
    if (!sessionId || events.length === 0) return;
    void loadTranscript();
  }, [events, loadTranscript, sessionId]);

  const sendMessage = useCallback(
    async (content: string) => {
      if (!sessionId) {
        throw new Error("No active session");
      }

      setSending(true);
      try {
        const response = await apiFetch(`/v1/sessions/${sessionId}/messages`, {
          body: JSON.stringify({ content }),
          headers: { "Content-Type": "application/json" },
          method: "POST",
        });

        if (!response.ok) {
          throw new Error(`Failed to send message (${response.status})`);
        }

        await response.json() as MessageResponse;
        await loadTranscript();
      } finally {
        setSending(false);
      }
    },
    [loadTranscript, sessionId],
  );

  const messages = useMemo(() => transcript.map(mapEntry), [transcript]);

  return {
    loading,
    messages,
    sendMessage,
    sending,
  };
}
