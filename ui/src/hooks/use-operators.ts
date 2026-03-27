import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { ActionResponse } from "@/lib/api-types";

export interface AgentItem {
  id: string;
  default: boolean;
  model: string | null;
  workspace: string | null;
  system_prompt: string | null;
}

export interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
  binary_path: string | null;
  source_dir: string;
  parameters: unknown;
}

export interface LogEntry {
  timestamp: string;
  level: string;
  target: string;
  message: string;
  fields?: Record<string, unknown>;
}

export interface LogsQueryResponse {
  entries: LogEntry[];
  message: string;
}

export interface UsageEntry {
  date: string;
  model: string;
  provider: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  request_count: number;
  estimated_cost: string | null;
}

export interface UsageSummary {
  entries: UsageEntry[];
  total_prompt_tokens: number;
  total_completion_tokens: number;
  total_tokens: number;
  total_requests: number;
  total_estimated_cost: string | null;
  usage_cached_prompt_tokens: number;
  cache_hit_ratio: number;
}

export function useAgents() {
  return useQuery({
    queryKey: ["agents"],
    queryFn: () => api.get<AgentItem[]>("/agents"),
    refetchInterval: 30_000,
  });
}

export function useSkills() {
  return useQuery({
    queryKey: ["skills"],
    queryFn: () => api.get<SkillItem[]>("/skills"),
    refetchInterval: 15_000,
  });
}

export function useToggleSkill() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ name, enable }: { name: string; enable: boolean }) =>
      api.post<ActionResponse>(`/skills/${name}/${enable ? "enable" : "disable"}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });
}

export function useReloadSkills() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/skills/reload"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });
}

export function useUsage(period?: string) {
  const params = period ? `?period=${period}` : "";
  return useQuery({
    queryKey: ["usage", period],
    queryFn: () => api.get<UsageSummary>(`/api/dashboard/usage${params}`),
    refetchInterval: 60_000,
  });
}

export function useLogs(query: {
  level?: string;
  source?: string;
  limit?: number;
  since?: string;
}) {
  const params = new URLSearchParams();

  if (query.level && query.level !== "all") params.set("level", query.level);
  if (query.source && query.source !== "all") params.set("source", query.source);
  if (typeof query.limit === "number") params.set("limit", String(query.limit));
  if (query.since) params.set("since", query.since);

  const suffix = params.toString();

  return useQuery({
    queryKey: ["logs", query],
    queryFn: () =>
      api.get<LogsQueryResponse>(suffix ? `/api/logs?${suffix}` : "/api/logs"),
    refetchInterval: 15_000,
  });
}
