import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type {
  SessionListItem,
  SessionResponse,
  SessionStatusResponse,
  TranscriptEntry,
  CreateSessionRequest,
  MessageResponse,
} from "@/lib/api-types";

interface SessionFilters {
  active_minutes?: number;
  channel?: string;
  limit?: number;
}

export function useSessions(filters?: SessionFilters) {
  const params = new URLSearchParams();
  if (filters?.active_minutes) params.set("active", String(filters.active_minutes));
  if (filters?.channel) params.set("channel", filters.channel);
  if (filters?.limit) params.set("limit", String(filters.limit));
  const qs = params.toString();

  return useQuery({
    queryKey: ["sessions", filters],
    queryFn: () => api.get<SessionListItem[]>(`/sessions${qs ? `?${qs}` : ""}`),
    refetchInterval: 15_000,
  });
}

export function useSession(id: string) {
  return useQuery({
    queryKey: ["sessions", id],
    queryFn: () => api.get<SessionResponse>(`/sessions/${id}`),
    enabled: !!id,
  });
}

export function useSessionStatus(id: string) {
  return useQuery({
    queryKey: ["sessions", id, "status"],
    queryFn: () => api.get<SessionStatusResponse>(`/sessions/${id}/status`),
    enabled: !!id,
    refetchInterval: 10_000,
  });
}

export function useSessionTranscript(id: string) {
  return useQuery({
    queryKey: ["sessions", id, "transcript"],
    queryFn: () => api.get<TranscriptEntry[]>(`/sessions/${id}/transcript`),
    enabled: !!id,
    refetchInterval: 5_000,
  });
}

export function useCreateSession() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: CreateSessionRequest) =>
      api.post<SessionResponse>("/sessions", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["sessions"] });
      queryClient.invalidateQueries({ queryKey: ["dashboard"] });
    },
  });
}

export function useSendMessage(sessionId: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (content: string) =>
      api.post<MessageResponse>(`/sessions/${sessionId}/messages`, { content }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["sessions", sessionId, "transcript"] });
      queryClient.invalidateQueries({ queryKey: ["sessions", sessionId] });
    },
  });
}
