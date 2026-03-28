import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api } from "@/lib/api-client";
import { useSessionEvents } from "@/lib/websocket";
import type {
  DashboardSummaryResponse,
  DashboardModelItem,
  DashboardSessionItem,
  DashboardDiagnosticsResponse,
  ChannelStatusResponse,
  ActionResponse,
} from "@/lib/api-types";

export function useDashboardSummary() {
  return useQuery({
    queryKey: ["dashboard", "summary"],
    queryFn: () => api.get<DashboardSummaryResponse>("/api/dashboard/summary"),
    refetchInterval: 30_000,
  });
}

export function useDashboardModels() {
  return useQuery({
    queryKey: ["dashboard", "models"],
    queryFn: () => api.get<DashboardModelItem[]>("/api/dashboard/models"),
    staleTime: 60_000,
  });
}

export function useDashboardSessions() {
  return useQuery({
    queryKey: ["dashboard", "sessions"],
    queryFn: () => api.get<DashboardSessionItem[]>("/api/dashboard/sessions"),
    refetchInterval: 15_000,
  });
}

export function useDashboardDiagnostics() {
  return useQuery({
    queryKey: ["dashboard", "diagnostics"],
    queryFn: () => api.get<DashboardDiagnosticsResponse>("/api/dashboard/diagnostics"),
    refetchInterval: 60_000,
  });
}

export function useChannelStatus() {
  return useQuery({
    queryKey: ["channels", "status"],
    queryFn: () => api.get<ChannelStatusResponse>("/api/channels/status"),
    refetchInterval: 30_000,
  });
}

export function useGatewayRestart() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/gateway/restart"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      queryClient.invalidateQueries({ queryKey: ["health"] });
      queryClient.invalidateQueries({ queryKey: ["status"] });
      queryClient.invalidateQueries({ queryKey: ["channels"] });
    },
  });
}

function parseActivityTimestamp(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const candidate = payload as Record<string, unknown>;
  const value = candidate.timestamp ?? candidate.observed_at ?? candidate.finished_at ?? candidate.started_at;
  return typeof value === "string" ? value : null;
}

function dashboardQueryKeys() {
  return [
    ["dashboard", "summary"],
    ["dashboard", "sessions"],
    ["dashboard", "diagnostics"],
    ["channels"],
    ["health"],
    ["status"],
  ] as const;
}

export interface DashboardLiveActivityItem {
  id: string;
  session_id: string;
  kind: string;
  observed_at: string;
  message: string;
}

export function useDashboardLiveUpdates() {
  const queryClient = useQueryClient();
  const { events, connected } = useSessionEvents("dashboard", {
    enabled: true,
    clearOnSessionChange: false,
  });

  useEffect(() => {
    if (!events.length) return;

    const latest = events[events.length - 1];
    const kind = latest.kind;

    if (kind.startsWith("turn.") || kind.startsWith("approval.") || kind.startsWith("tool.")) {
      for (const key of dashboardQueryKeys()) {
        queryClient.invalidateQueries({ queryKey: key });
      }
    }
  }, [events, queryClient]);

  const activity = events
    .slice(-24)
    .reverse()
    .map((event, index) => ({
      id: `${event.session_id}-${event.kind}-${index}`,
      session_id: event.session_id,
      kind: event.kind,
      observed_at: parseActivityTimestamp(event.payload) ?? new Date().toISOString(),
      message: event.kind.replace(/\./g, " "),
    } satisfies DashboardLiveActivityItem));

  return { connected, activity };
}
