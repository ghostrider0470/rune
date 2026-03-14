import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type {
  CronJobResponse,
  CronStatusResponse,
  CronRunResponse,
  CronMutationResponse,
  CronWakeResponse,
  CronJobRequest,
  CronUpdateRequest,
  CronWakeRequest,
} from "@/lib/api-types";

export function useCronJobs(includeDisabled = true) {
  return useQuery({
    queryKey: ["cron", "jobs", { includeDisabled }],
    queryFn: () =>
      api.get<CronJobResponse[]>(`/cron?includeDisabled=${includeDisabled}`),
    refetchInterval: 30_000,
  });
}

export function useCronStatus() {
  return useQuery({
    queryKey: ["cron", "status"],
    queryFn: () => api.get<CronStatusResponse>("/cron/status"),
    refetchInterval: 30_000,
  });
}

export function useCronRuns(jobId: string) {
  return useQuery({
    queryKey: ["cron", "runs", jobId],
    queryFn: () => api.get<CronRunResponse[]>(`/cron/${jobId}/runs`),
    enabled: !!jobId,
  });
}

export function useCreateCronJob() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: CronJobRequest) =>
      api.post<CronMutationResponse>("/cron", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron"] });
    },
  });
}

export function useUpdateCronJob() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: CronUpdateRequest }) =>
      api.post<CronMutationResponse>(`/cron/${id}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron"] });
    },
  });
}

export function useDeleteCronJob() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) =>
      api.delete<CronMutationResponse>(`/cron/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron"] });
    },
  });
}

export function useRunCronJob() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) =>
      api.post<CronMutationResponse>(`/cron/${id}/run`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron"] });
    },
  });
}

export function useCronWake() {
  return useMutation({
    mutationFn: (data: CronWakeRequest) =>
      api.post<CronWakeResponse>("/cron/wake", data),
  });
}
