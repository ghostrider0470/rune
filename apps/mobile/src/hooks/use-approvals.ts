import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApprovalRequestResponse } from "../api/api-types";
import { apiFetch } from "../api/client";
import { notifyPendingApprovalsIncreased } from "../lib/notifications";
import { useSessionEvents } from "../lib/websocket";

export interface UseApprovalsResult {
  approvals: ApprovalRequestResponse[];
  loading: boolean;
  decidingId: string | null;
  refresh: () => Promise<void>;
  decide: (id: string, decision: "allow_once" | "deny") => Promise<void>;
}

function sortApprovals(items: ApprovalRequestResponse[]): ApprovalRequestResponse[] {
  return [...items].sort((a, b) => b.created_at.localeCompare(a.created_at));
}

export function useApprovals(): UseApprovalsResult {
  const [approvals, setApprovals] = useState<ApprovalRequestResponse[]>([]);
  const [loading, setLoading] = useState(true);
  const [decidingId, setDecidingId] = useState<string | null>(null);
  const { events } = useSessionEvents(undefined, { enabled: true, clearOnSessionChange: false });
  const previousCountRef = useRef(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const response = await apiFetch("/v1/approvals");
      if (!response.ok) {
        throw new Error(`Failed to load approvals (${response.status})`);
      }
      const data = (await response.json()) as ApprovalRequestResponse[];
      const sorted = sortApprovals(data);
      const previousCount = previousCountRef.current;
      previousCountRef.current = sorted.length;
      setApprovals(sorted);
      await notifyPendingApprovalsIncreased(previousCount, sorted.length);
    } finally {
      setLoading(false);
    }
  }, []);

  const decide = useCallback(async (id: string, decision: "allow_once" | "deny") => {
    setDecidingId(id);
    try {
      const response = await apiFetch("/v1/approvals", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id, decision }),
      });

      if (!response.ok) {
        throw new Error(`Failed to submit approval decision (${response.status})`);
      }

      setApprovals((current) => {
        const next = current.filter((approval) => approval.id !== id);
        previousCountRef.current = next.length;
        return next;
      });
    } finally {
      setDecidingId(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (events.length === 0) return;
    const latest = events[events.length - 1];
    if (latest.kind.startsWith("approval.")) {
      void refresh();
    }
  }, [events, refresh]);

  return useMemo(() => ({ approvals, loading, decidingId, refresh, decide }), [approvals, loading, decidingId, refresh, decide]);
}
