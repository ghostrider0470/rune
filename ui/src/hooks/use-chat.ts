import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef } from "react";
import { api } from "@/lib/api-client";
import { useSessionEvents } from "@/lib/websocket";
import { useSessions, useCreateSession } from "@/hooks/use-sessions";
import {
  getEntrySignature,
  getPayloadText,
  normalizeTranscriptKind,
} from "@/components/chat/chat-utils";
import type {
  TranscriptEntry,
  MessageResponse,
  SendMessageRequest,
  SessionEvent,
} from "@/lib/api-types";

// ---------------------------------------------------------------------------
// useChatSessions – thin wrapper around useSessions for the sidebar
// ---------------------------------------------------------------------------
export function useChatSessions() {
  const query = useSessions({ limit: 100 });
  const createSession = useCreateSession();
  return { ...query, createSession };
}

// ---------------------------------------------------------------------------
// useChatTranscript – polls the transcript at 3 s when a session is active
// ---------------------------------------------------------------------------
export function useChatTranscript(sessionId: string | undefined) {
  return useQuery({
    queryKey: ["sessions", sessionId, "transcript"],
    queryFn: () =>
      api.get<TranscriptEntry[]>(`/sessions/${sessionId}/transcript`),
    enabled: !!sessionId,
    refetchInterval: 3_000,
  });
}

// ---------------------------------------------------------------------------
// useChatSend – send a message with optimistic update
// ---------------------------------------------------------------------------
export function useChatSend(sessionId: string | undefined) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: SendMessageRequest) => {
      if (!sessionId) {
        throw new Error("Select a session before sending a message.");
      }

      return api.post<MessageResponse>(
        `/sessions/${sessionId}/messages`,
        data,
      );
    },
    onMutate: async (variables) => {
      if (!sessionId) return;

      await queryClient.cancelQueries({
        queryKey: ["sessions", sessionId, "transcript"],
      });

      const previousTranscript = queryClient.getQueryData<TranscriptEntry[]>([
        "sessions",
        sessionId,
        "transcript",
      ]);

      const optimisticEntry: TranscriptEntry = {
        id: `optimistic-${Date.now()}`,
        turn_id: null,
        seq: (previousTranscript?.length ?? 0) + 1,
        kind: "user",
        payload: variables.content,
        created_at: new Date().toISOString(),
      };

      queryClient.setQueryData<TranscriptEntry[]>(
        ["sessions", sessionId, "transcript"],
        (old) => [...(old ?? []), optimisticEntry],
      );

      return { previousTranscript };
    },
    onError: (_err, _variables, context) => {
      if (context?.previousTranscript && sessionId) {
        queryClient.setQueryData(
          ["sessions", sessionId, "transcript"],
          context.previousTranscript,
        );
      }
    },
    onSettled: () => {
      if (!sessionId) return;
      queryClient.invalidateQueries({
        queryKey: ["sessions", sessionId, "transcript"],
      });
      queryClient.invalidateQueries({
        queryKey: ["sessions", sessionId],
      });
      queryClient.invalidateQueries({
        queryKey: ["sessions"],
      });
    },
  });
}

function eventToTranscriptEntry(
  event: SessionEvent,
  index: number,
): TranscriptEntry | null {
  if (event.kind === "turn_completed") {
    const payload =
      event.payload && typeof event.payload === "object"
        ? (event.payload as Record<string, unknown>)
        : null;
    const assistantReply = payload?.assistant_reply;

    if (typeof assistantReply !== "string" || assistantReply.trim().length === 0) {
      return null;
    }

    return {
      id: `ws-turn-${typeof payload?.turn_id === "string" ? payload.turn_id : `${event.session_id}-${index}`}`,
      turn_id: typeof payload?.turn_id === "string" ? payload.turn_id : null,
      seq: Number.MAX_SAFE_INTEGER - 1,
      kind: "assistant_message",
      payload: { content: assistantReply },
      created_at: new Date().toISOString(),
    };
  }

  if (event.kind === "transcript_item") {
    const payload =
      event.payload && typeof event.payload === "object"
        ? (event.payload as Record<string, unknown>)
        : null;

    if (!payload || typeof payload.kind !== "string") {
      return null;
    }

    return {
      id:
        typeof payload.id === "string"
          ? `ws-${payload.id}`
          : `ws-${event.session_id}-${index}`,
      turn_id: typeof payload.turn_id === "string" ? payload.turn_id : null,
      seq: typeof payload.seq === "number" ? payload.seq : Number.MAX_SAFE_INTEGER,
      kind: payload.kind,
      payload: payload.payload ?? payload,
      created_at:
        typeof payload.created_at === "string"
          ? payload.created_at
          : new Date().toISOString(),
    };
  }

  return null;
}

function matchesTranscriptEntry(candidate: TranscriptEntry, liveEntry: TranscriptEntry): boolean {
  if (candidate.id === liveEntry.id) {
    return true;
  }

  const candidateKind = normalizeTranscriptKind(candidate.kind);
  const liveKind = normalizeTranscriptKind(liveEntry.kind);

  if (candidateKind !== liveKind) {
    return false;
  }

  if (candidate.turn_id && liveEntry.turn_id && candidate.turn_id === liveEntry.turn_id) {
    if (candidateKind === "assistant") {
      return getPayloadText(candidate.payload).trim() === getPayloadText(liveEntry.payload).trim();
    }

    return true;
  }

  if (candidate.seq === liveEntry.seq) {
    return getPayloadText(candidate.payload).trim() === getPayloadText(liveEntry.payload).trim();
  }

  return false;
}

export function useChatWebSocket(sessionId: string | undefined) {
  const { events, connected, clearEvents } = useSessionEvents(sessionId, {
    enabled: !!sessionId,
    clearOnSessionChange: true,
  });

  const liveEntries = useMemo(
    () =>
      events
        .map((evt, i) => eventToTranscriptEntry(evt, i))
        .filter((entry): entry is TranscriptEntry => entry !== null),
    [events],
  );

  return { liveEntries, connected, clearEvents };
}

export function useChatMergedTranscript(sessionId: string | undefined) {
  const queryClient = useQueryClient();
  const transcript = useChatTranscript(sessionId);
  const { liveEntries, connected, clearEvents } = useChatWebSocket(sessionId);
  const lastInvalidatedLiveKeyRef = useRef<string | null>(null);

  useEffect(() => {
    if (!sessionId || liveEntries.length === 0) {
      lastInvalidatedLiveKeyRef.current = null;
      return;
    }

    const lastEntry = liveEntries[liveEntries.length - 1];
    const invalidationKey = getEntrySignature(lastEntry);

    if (lastInvalidatedLiveKeyRef.current === invalidationKey) {
      return;
    }

    lastInvalidatedLiveKeyRef.current = invalidationKey;

    void queryClient.invalidateQueries({
      queryKey: ["sessions", sessionId, "transcript"],
    });
    void queryClient.invalidateQueries({
      queryKey: ["sessions", sessionId],
    });
    void queryClient.invalidateQueries({
      queryKey: ["sessions"],
    });
  }, [liveEntries, queryClient, sessionId]);

  const mergedEntries = useMemo(() => {
    const base = (transcript.data ?? []).slice().sort((a, b) => a.seq - b.seq);
    const merged = new Map<string, TranscriptEntry>();

    for (const entry of base) {
      merged.set(getEntrySignature(entry), entry);
    }

    for (const entry of liveEntries) {
      const duplicate = base.some((candidate) => matchesTranscriptEntry(candidate, entry));

      if (duplicate) {
        continue;
      }

      merged.set(getEntrySignature(entry), entry);
    }

    return Array.from(merged.values()).sort((a, b) => {
      if (a.seq !== b.seq) return a.seq - b.seq;
      return normalizeTranscriptKind(a.kind).localeCompare(normalizeTranscriptKind(b.kind));
    });
  }, [transcript.data, liveEntries]);

  return {
    entries: mergedEntries,
    isLoading: transcript.isLoading,
    isFetching: transcript.isFetching,
    isError: transcript.isError,
    connected,
    clearEvents,
    refetch: transcript.refetch,
  };
}
