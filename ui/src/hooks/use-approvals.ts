import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type {
  ApprovalRequestResponse,
  ApprovalPolicyResponse,
  SubmitApprovalDecisionRequest,
  SetApprovalPolicyRequest,
  ActionResponse,
} from "@/lib/api-types";

export function usePendingApprovals() {
  return useQuery({
    queryKey: ["approvals", "pending"],
    queryFn: () => api.get<ApprovalRequestResponse[]>("/approvals"),
    refetchInterval: 10_000,
  });
}

export function useApprovalPolicies() {
  return useQuery({
    queryKey: ["approvals", "policies"],
    queryFn: () => api.get<ApprovalPolicyResponse[]>("/approvals/policies"),
  });
}

export function useSubmitApprovalDecision() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: SubmitApprovalDecisionRequest) =>
      api.post<ActionResponse>("/approvals", data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["approvals"] });
    },
  });
}

export function useSetApprovalPolicy() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ tool, data }: { tool: string; data: SetApprovalPolicyRequest }) =>
      api.put<ActionResponse>(`/approvals/policies/${tool}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["approvals", "policies"] });
    },
  });
}

export function useClearApprovalPolicy() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (tool: string) =>
      api.delete<ActionResponse>(`/approvals/policies/${tool}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["approvals", "policies"] });
    },
  });
}
