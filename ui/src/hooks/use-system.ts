import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type {
  HealthResponse,
  StatusResponse,
  HeartbeatState,
  ActionResponse,
  ReminderResponse,
  ReminderAddRequest,
  TtsStatusResponse,
  SttStatusResponse,
  DoctorReport,
  ConfigSchemaResponse,
} from "@/lib/api-types";

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => api.get<HealthResponse>("/health"),
    refetchInterval: 15_000,
  });
}

export function useStatus() {
  return useQuery({
    queryKey: ["status"],
    queryFn: () => api.get<StatusResponse>("/status"),
  });
}

export function useHeartbeatStatus() {
  return useQuery({
    queryKey: ["heartbeat", "status"],
    queryFn: () => api.get<HeartbeatState>("/heartbeat/status"),
    refetchInterval: 30_000,
  });
}

export function useHeartbeatEnable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/heartbeat/enable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["heartbeat"] });
    },
  });
}

export function useHeartbeatDisable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/heartbeat/disable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["heartbeat"] });
    },
  });
}

export function useGatewayStart() {
  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/gateway/start"),
  });
}

export function useGatewayStop() {
  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/gateway/stop"),
  });
}

export function useGatewayRestart() {
  return useMutation({
    mutationFn: () => api.post<ActionResponse>("/gateway/restart"),
  });
}

export function useReminders(includeDelivered = false) {
  return useQuery({
    queryKey: ["reminders", { includeDelivered }],
    queryFn: () =>
      api.get<ReminderResponse[]>(`/reminders?includeDelivered=${includeDelivered}`),
    refetchInterval: 30_000,
  });
}

export function useCreateReminder() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: ReminderAddRequest) =>
      api.post<ActionResponse>("/reminders", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["reminders"] });
    },
  });
}

export function useCancelReminder() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) =>
      api.delete<ActionResponse>(`/reminders/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["reminders"] });
    },
  });
}

export function useConfig() {
  return useQuery({
    queryKey: ["config"],
    queryFn: () => api.get<Record<string, unknown>>("/config"),
  });
}


export function useConfigSchema() {
  return useQuery({
    queryKey: ["config", "schema"],
    queryFn: () => api.get<ConfigSchemaResponse>("/config/schema"),
  });
}

export function useUpdateConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: Record<string, unknown>) =>
      api.put<Record<string, unknown>>("/config", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}

export function useDoctorResults() {
  return useQuery({
    queryKey: ["doctor", "results"],
    queryFn: () => api.get<DoctorReport>("/api/doctor/results"),
    refetchInterval: 30_000,
  });
}

export function useDoctorRun() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<DoctorReport>("/api/doctor/run"),
    onSuccess: (report) => {
      queryClient.setQueryData(["doctor", "results"], report);
      queryClient.invalidateQueries({ queryKey: ["doctor"] });
      queryClient.invalidateQueries({ queryKey: ["status"] });
      queryClient.invalidateQueries({ queryKey: ["health"] });
    },
  });
}

export function useTtsStatus() {
  return useQuery({
    queryKey: ["tts", "status"],
    queryFn: () => api.get<TtsStatusResponse>("/tts/status"),
    refetchInterval: 30_000,
  });
}

export function useTtsEnable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<{ enabled: boolean }>("/tts/enable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tts"] });
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}

export function useTtsDisable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<{ enabled: boolean }>("/tts/disable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tts"] });
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}

export function useSttStatus() {
  return useQuery({
    queryKey: ["stt", "status"],
    queryFn: () => api.get<SttStatusResponse>("/stt/status"),
    refetchInterval: 30_000,
  });
}

export function useSttEnable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<{ enabled: boolean }>("/stt/enable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["stt"] });
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}

export function useSttDisable() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => api.post<{ enabled: boolean }>("/stt/disable"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["stt"] });
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}
