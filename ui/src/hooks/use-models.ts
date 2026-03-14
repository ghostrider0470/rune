import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { DashboardModelItem, ActionResponse } from "@/lib/api-types";

export function useModels() {
  return useQuery({
    queryKey: ["models"],
    queryFn: () => api.get<DashboardModelItem[]>("/models"),
    refetchInterval: 60_000,
  });
}

export function useScanModels() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/models/scan"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["models"] });
      queryClient.invalidateQueries({ queryKey: ["dashboard"] });
    },
  });
}
