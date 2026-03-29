import { useMemo } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { usePendingApprovals, useSubmitApprovalDecision } from "@/hooks/use-approvals";
import type { ApprovalRequestResponse, TranscriptEntry } from "@/lib/api-types";
import { CheckCircle2, ShieldCheck, XCircle } from "lucide-react";

interface InlineApprovalActionsProps {
  entry: TranscriptEntry;
  className?: string;
}

function decisionLabel(decision: string | null | undefined): string {
  switch (decision) {
    case "allow_once":
      return "Approved once";
    case "allow_always":
      return "Always allow";
    case "deny":
      return "Denied";
    default:
      return "Pending approval";
  }
}

function decisionBadgeVariant(
  decision: string | null | undefined,
): "default" | "secondary" | "destructive" | "outline" {
  switch (decision) {
    case "allow_once":
    case "allow_always":
      return "default";
    case "deny":
      return "destructive";
    case null:
    case undefined:
      return "secondary";
    default:
      return "outline";
  }
}

function extractApprovalId(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;

  const direct = record.approval_id;
  if (typeof direct === "string" && direct.length > 0) {
    return direct;
  }

  const nestedPayload = record.payload;
  if (nestedPayload && typeof nestedPayload === "object") {
    const nested = (nestedPayload as Record<string, unknown>).approval_id;
    if (typeof nested === "string" && nested.length > 0) {
      return nested;
    }
  }

  return null;
}

function findMatchingApproval(
  approvals: ApprovalRequestResponse[] | undefined,
  approvalId: string | null,
  entry: TranscriptEntry,
): ApprovalRequestResponse | null {
  if (!approvals?.length) return null;

  if (approvalId) {
    const direct = approvals.find((approval) => approval.id === approvalId);
    if (direct) return direct;
  }

  return approvals.find((approval) => {
    if (!approval.handle_ref || approval.handle_ref !== entry.turn_id) {
      return false;
    }

    const payload = approval.presented_payload;
    if (!payload || typeof payload !== "object") {
      return false;
    }

    const payloadToolCallId = (payload as Record<string, unknown>).tool_call_id;
    const entryToolCallId = (entry.payload && typeof entry.payload === "object"
      ? (entry.payload as Record<string, unknown>).tool_call_id
      : undefined) as unknown;

    return typeof payloadToolCallId === "string" && payloadToolCallId === entryToolCallId;
  }) ?? null;
}

export function InlineApprovalActions({ entry, className }: InlineApprovalActionsProps) {
  const approvalId = extractApprovalId(entry.payload);
  const { data: approvals } = usePendingApprovals();
  const submitDecision = useSubmitApprovalDecision();

  const approval = useMemo(
    () => findMatchingApproval(approvals, approvalId, entry),
    [approvalId, approvals, entry],
  );

  if (!approval) {
    return null;
  }

  const isResolved = Boolean(approval.decision);
  const isBusy = submitDecision.isPending;

  const decide = (decision: "allow_once" | "allow_always" | "deny") => {
    submitDecision.mutate({
      id: approval.id,
      decision,
      decided_by: "chat-inline",
    });
  };

  return (
    <div
      className={cn(
        "mt-2 rounded-2xl border border-amber-500/30 bg-amber-500/5 p-3",
        className,
      )}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline" className="border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300">
          Approval required
        </Badge>
        <Badge variant={decisionBadgeVariant(approval.decision)}>
          {decisionLabel(approval.decision)}
        </Badge>
        <span className="text-[11px] text-muted-foreground">{approval.reason}</span>
      </div>

      {!isResolved && (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            type="button"
            size="sm"
            onClick={() => decide("allow_once")}
            disabled={isBusy}
            className="gap-1.5"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            Approve once
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => decide("allow_always")}
            disabled={isBusy}
            className="gap-1.5"
          >
            <ShieldCheck className="h-3.5 w-3.5" />
            Always allow
          </Button>
          <Button
            type="button"
            size="sm"
            variant="destructive"
            onClick={() => decide("deny")}
            disabled={isBusy}
            className="gap-1.5"
          >
            <XCircle className="h-3.5 w-3.5" />
            Deny
          </Button>
        </div>
      )}
    </div>
  );
}
