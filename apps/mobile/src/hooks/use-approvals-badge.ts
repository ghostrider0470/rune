import { useMemo } from "react";
import { useApprovals } from "./use-approvals";

export function useApprovalsBadge() {
  const { approvals, loading } = useApprovals();
  const count = useMemo(() => approvals.length, [approvals]);

  return {
    count,
    loading,
    visible: count > 0,
  };
}
