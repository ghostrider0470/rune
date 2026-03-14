import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type {
  DashboardSummaryResponse,
  DashboardModelItem,
  DashboardSessionItem,
  DashboardDiagnosticsResponse,
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
