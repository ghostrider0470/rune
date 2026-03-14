import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api-client";

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
}

export function useUsage() {
  return useQuery({
    queryKey: ["usage"],
    queryFn: () => api.get<UsageSummary>("/api/dashboard/usage"),
    refetchInterval: 60_000,
  });
}
