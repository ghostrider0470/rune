import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  usePendingApprovals,
  useApprovalPolicies,
  useSubmitApprovalDecision,
  useSetApprovalPolicy,
  useClearApprovalPolicy,
} from "@/hooks/use-approvals";
import { useSessionEvents } from "@/lib/websocket";
import type { ApprovalRequestResponse } from "@/lib/api-types";
import {
  ShieldCheck,
  CheckCircle2,
  XCircle,
  Trash2,
  Plus,
  Clock3,
  TerminalSquare,
  RefreshCw,
  Eye,
  Search,
} from "lucide-react";

export const Route = createFileRoute("/_admin/approvals")({
  component: ApprovalsPage,
});

type ApprovalFilter = "all" | "tool_call" | "process";
type DecisionValue = "allow_once" | "allow_always" | "deny";

const LIVE_EVENT_LIMIT = 30;

function decisionLabel(decision: string | null | undefined): string {
  switch (decision) {
    case "allow_once":
      return "Approve once";
    case "allow_always":
      return "Always allow";
    case "deny":
      return "Deny";
    default:
      return decision ? decision.replace(/_/g, " ") : "Pending";
  }
}

function decisionBadgeVariant(decision: string | null | undefined): "default" | "secondary" | "destructive" | "outline" {
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

function statusBadgeVariant(status: string | null | undefined): "default" | "secondary" | "destructive" | "outline" {
  switch (status) {
    case "completed":
    case "resumed":
      return "default";
    case "denied":
    case "failed":
      return "destructive";
    case "pending":
    case "waiting":
      return "secondary";
    default:
      return "outline";
  }
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatRelative(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  const diffMs = Date.now() - date.getTime();
  if (Number.isNaN(diffMs)) return value;

  const seconds = Math.max(1, Math.round(diffMs / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

function normalizeText(value: string | null | undefined): string {
  return (value ?? "").toLowerCase();
}

function getCommandPreview(approval: ApprovalRequestResponse): string {
  if (approval.command) return approval.command;

  if (approval.presented_payload && typeof approval.presented_payload === "object") {
    const payload = approval.presented_payload as Record<string, unknown>;
    const command = payload.command;
    if (typeof command === "string" && command.trim()) {
      return command;
    }
  }

  return "No command preview available.";
}

function matchesFilter(approval: ApprovalRequestResponse, filter: ApprovalFilter): boolean {
  if (filter === "all") return true;
  return approval.subject_type === filter;
}

function matchesSearch(approval: ApprovalRequestResponse, search: string): boolean {
  if (!search.trim()) return true;
  const needle = search.trim().toLowerCase();
  const haystacks = [
    approval.reason,
    approval.subject_type,
    approval.subject_id,
    approval.command,
    approval.decision,
    approval.approval_status,
    approval.resume_result_summary,
    approval.handle_ref,
    approval.host_ref,
  ].map(normalizeText);

  return haystacks.some((value) => value.includes(needle));
}

function ApprovalDetailDialog({
  approval,
  open,
  onOpenChange,
}: {
  approval: ApprovalRequestResponse | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const payloadText = useMemo(() => {
    if (!approval) return "";
    return JSON.stringify(approval.presented_payload ?? {}, null, 2);
  }, [approval]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Approval details</DialogTitle>
          <DialogDescription>
            Exact command preview, payload, and lifecycle metadata for operator review.
          </DialogDescription>
        </DialogHeader>

        {!approval ? null : (
          <div className="grid gap-4 overflow-y-auto pr-1">
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">Subject</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <Badge variant="outline">{approval.subject_type}</Badge>
                  <p className="font-mono text-xs break-all">{approval.subject_id}</p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">Decision</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <Badge variant={decisionBadgeVariant(approval.decision)}>
                    {decisionLabel(approval.decision)}
                  </Badge>
                  <p className="text-xs text-muted-foreground">
                    {approval.decided_by ? `by ${approval.decided_by}` : "Not decided yet"}
                  </p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">Resume status</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <Badge variant={statusBadgeVariant(approval.approval_status)}>
                    {approval.approval_status ?? "pending"}
                  </Badge>
                  <p className="text-xs text-muted-foreground">
                    {formatDateTime(approval.approval_status_updated_at)}
                  </p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">Created</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <p>{formatDateTime(approval.created_at)}</p>
                  <p className="text-xs text-muted-foreground">{formatRelative(approval.created_at)}</p>
                </CardContent>
              </Card>
            </div>

            <Card>
              <CardHeader>
                <CardTitle className="text-sm">Reason</CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm leading-6">{approval.reason}</p>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-sm">
                  <TerminalSquare className="h-4 w-4" />
                  Exact command preview
                </CardTitle>
              </CardHeader>
              <CardContent>
                <pre className="max-h-72 overflow-auto rounded-lg bg-muted px-3 py-3 text-xs leading-5">
                  {getCommandPreview(approval)}
                </pre>
              </CardContent>
            </Card>

            <div className="grid gap-4 lg:grid-cols-[1.1fr,0.9fr]">
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">Payload</CardTitle>
                </CardHeader>
                <CardContent>
                  <Textarea
                    readOnly
                    value={payloadText}
                    className="min-h-[22rem] font-mono text-xs"
                  />
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">Lifecycle</CardTitle>
                </CardHeader>
                <CardContent className="space-y-3 text-sm">
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Decided at</p>
                    <p>{formatDateTime(approval.decided_at)}</p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Resumed at</p>
                    <p>{formatDateTime(approval.resumed_at)}</p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Completed at</p>
                    <p>{formatDateTime(approval.completed_at)}</p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Result summary</p>
                    <p className="leading-6 text-muted-foreground">
                      {approval.resume_result_summary ?? "—"}
                    </p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Handle ref</p>
                    <p className="font-mono text-xs break-all">{approval.handle_ref ?? "—"}</p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-wide text-muted-foreground">Host ref</p>
                    <p className="font-mono text-xs break-all">{approval.host_ref ?? "—"}</p>
                  </div>
                </CardContent>
              </Card>
            </div>
          </div>
        )}

        <DialogFooter showCloseButton />
      </DialogContent>
    </Dialog>
  );
}

function ApprovalsPage() {
  const { data: pending, isLoading: pendingLoading, refetch: refetchPending, isFetching: pendingRefreshing } = usePendingApprovals();
  const { data: policies, isLoading: policiesLoading } = useApprovalPolicies();
  const submitDecision = useSubmitApprovalDecision();
  const setPolicy = useSetApprovalPolicy();
  const clearPolicy = useClearApprovalPolicy();
  const { events, connected } = useSessionEvents(undefined, { enabled: true, clearOnSessionChange: false });

  const [policyOpen, setPolicyOpen] = useState(false);
  const [policyTool, setPolicyTool] = useState("");
  const [policyDecision, setPolicyDecision] = useState("allow_always");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ApprovalFilter>("all");
  const [selectedApproval, setSelectedApproval] = useState<ApprovalRequestResponse | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [liveEvents, setLiveEvents] = useState<Array<{ id: string; label: string; createdAt: string }>>([]);

  useEffect(() => {
    if (!events.length) return;

    const latest = events[events.length - 1];
    if (!latest.kind.startsWith("approval.")) return;

    setLiveEvents((current) => {
      const next = [
        {
          id: `${latest.session_id}:${latest.kind}:${events.length}`,
          label: `${latest.kind} · ${latest.session_id.slice(0, 8)}`,
          createdAt: new Date().toISOString(),
        },
        ...current,
      ];
      return next.slice(0, LIVE_EVENT_LIMIT);
    });

    void refetchPending();
  }, [events, refetchPending]);

  const filteredPending = useMemo(() => {
    const items = pending ?? [];
    return items.filter((approval) => matchesFilter(approval, filter) && matchesSearch(approval, search));
  }, [filter, pending, search]);

  const pendingCount = pending?.length ?? 0;
  const processCount = (pending ?? []).filter((approval) => approval.subject_type === "process").length;
  const toolCallCount = (pending ?? []).filter((approval) => approval.subject_type === "tool_call").length;

  const handleSetPolicy = () => {
    setPolicy.mutate(
      { tool: policyTool.trim(), data: { decision: policyDecision } },
      {
        onSuccess: () => {
          setPolicyOpen(false);
          setPolicyTool("");
          setPolicyDecision("allow_always");
        },
      }
    );
  };

  const submitApprovalDecision = (id: string, decision: DecisionValue) => {
    submitDecision.mutate({
      id,
      decision,
      decided_by: "admin-ui",
    });
  };

  const openDetails = (approval: ApprovalRequestResponse) => {
    setSelectedApproval(approval);
    setDetailOpen(true);
  };

  return (
    <>
      <div className="space-y-6 sm:space-y-8">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div className="space-y-1">
            <h1 className="text-3xl font-bold tracking-tight">Approval Center</h1>
            <p className="mt-1 text-muted-foreground">
              Pending approvals front and center with exact command preview, live status, and one-click operator actions.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={connected ? "default" : "secondary"} className="gap-1.5 px-3 py-1">
              <RefreshCw className={`h-3 w-3 ${pendingRefreshing ? "animate-spin" : ""}`} />
              {connected ? "Live events connected" : "Live events reconnecting"}
            </Badge>
            <Button variant="outline" size="sm" onClick={() => void refetchPending()} disabled={pendingRefreshing}>
              <RefreshCw className={`mr-1.5 h-3.5 w-3.5 ${pendingRefreshing ? "animate-spin" : ""}`} />
              Refresh
            </Button>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm text-muted-foreground">Pending approvals</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-3xl font-semibold">{pendingCount}</div>
              <p className="mt-1 text-xs text-muted-foreground">Needs explicit operator action now</p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm text-muted-foreground">Tool calls</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-3xl font-semibold">{toolCallCount}</div>
              <p className="mt-1 text-xs text-muted-foreground">Command execution and tool resumes</p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm text-muted-foreground">Processes</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-3xl font-semibold">{processCount}</div>
              <p className="mt-1 text-xs text-muted-foreground">Long-running or durable process approvals</p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm text-muted-foreground">Policy count</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-3xl font-semibold">{policies?.length ?? 0}</div>
              <p className="mt-1 text-xs text-muted-foreground">Saved tool decisions</p>
            </CardContent>
          </Card>
        </div>

        <div className="grid gap-6 xl:grid-cols-[1.65fr,0.95fr]">
          <Card>
            <CardHeader className="gap-4">
              <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
                <CardTitle className="flex flex-wrap items-center gap-2 text-base">
                  <ShieldCheck className="h-4 w-4" />
                  Pending Approvals ({filteredPending.length})
                </CardTitle>
                <div className="flex flex-col gap-2 sm:flex-row">
                  <div className="relative min-w-[16rem] flex-1">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      value={search}
                      onChange={(e) => setSearch(e.target.value)}
                      placeholder="Search reason, command, status, handle..."
                      className="pl-9"
                    />
                  </div>
                  <Select value={filter} onValueChange={(value) => setFilter(value as ApprovalFilter)}>
                    <SelectTrigger className="w-full sm:w-[12rem]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All approvals</SelectItem>
                      <SelectItem value="tool_call">Tool calls</SelectItem>
                      <SelectItem value="process">Processes</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {pendingLoading ? (
                <div className="space-y-2">
                  {Array.from({ length: 3 }).map((_, i) => (
                    <Skeleton key={i} className="h-28 w-full" />
                  ))}
                </div>
              ) : !filteredPending.length ? (
                <div className="rounded-xl border border-dashed p-8 text-center">
                  <p className="text-sm font-medium">No matching approvals</p>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {pendingCount === 0 ? "Queue is clear." : "Try a different filter or search."}
                  </p>
                </div>
              ) : (
                <div className="space-y-3">
                  {filteredPending.map((approval) => (
                    <div key={approval.id} className="rounded-xl border bg-muted/20 p-4">
                      <div className="flex flex-col gap-4">
                        <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
                          <div className="min-w-0 space-y-2">
                            <div className="flex flex-wrap items-center gap-2">
                              <Badge variant="outline">{approval.subject_type.replace(/_/g, " ")}</Badge>
                              <Badge variant={statusBadgeVariant(approval.approval_status)}>
                                {approval.approval_status ?? "pending"}
                              </Badge>
                              {approval.decision ? (
                                <Badge variant={decisionBadgeVariant(approval.decision)}>
                                  {decisionLabel(approval.decision)}
                                </Badge>
                              ) : null}
                              <span className="text-xs text-muted-foreground xl:ml-auto">
                                <Clock3 className="mr-1 inline h-3 w-3" />
                                {formatDateTime(approval.created_at)}
                              </span>
                            </div>
                            <p className="text-sm font-medium leading-6">{approval.reason}</p>
                            <pre className="max-h-40 overflow-auto rounded-lg bg-muted px-3 py-2 text-xs leading-5">
                              {getCommandPreview(approval)}
                            </pre>
                            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                              <span>Subject: <span className="font-mono">{approval.subject_id.slice(0, 12)}</span></span>
                              <span>Updated: {formatDateTime(approval.approval_status_updated_at)}</span>
                              <span>Handle: <span className="font-mono">{approval.handle_ref?.slice(0, 12) ?? "—"}</span></span>
                            </div>
                          </div>
                          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:w-[17rem]">
                            <Button
                              size="sm"
                              onClick={() => submitApprovalDecision(approval.id, "allow_once")}
                              disabled={submitDecision.isPending}
                              className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
                            >
                              <CheckCircle2 className="h-3.5 w-3.5" />
                              Approve once
                            </Button>
                            <Button
                              size="sm"
                              variant="outline"
                              onClick={() => submitApprovalDecision(approval.id, "allow_always")}
                              disabled={submitDecision.isPending}
                              className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
                            >
                              <ShieldCheck className="h-3.5 w-3.5" />
                              Always allow
                            </Button>
                            <Button
                              size="sm"
                              variant="destructive"
                              onClick={() => submitApprovalDecision(approval.id, "deny")}
                              disabled={submitDecision.isPending}
                              className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
                            >
                              <XCircle className="h-3.5 w-3.5" />
                              Deny
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              onClick={() => openDetails(approval)}
                              className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
                            >
                              <Eye className="h-3.5 w-3.5" />
                              Details
                            </Button>
                          </div>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>

          <div className="space-y-6">
            <Card>
              <CardHeader>
                <CardTitle className="text-base">Live approval activity</CardTitle>
              </CardHeader>
              <CardContent>
                {!liveEvents.length ? (
                  <p className="text-sm text-muted-foreground">Waiting for approval.* websocket events.</p>
                ) : (
                  <div className="space-y-3">
                    {liveEvents.map((event) => (
                      <div key={event.id} className="rounded-lg border px-3 py-2">
                        <p className="text-sm font-medium">{event.label}</p>
                        <p className="text-xs text-muted-foreground">{formatDateTime(event.createdAt)}</p>
                      </div>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <CardTitle className="flex flex-wrap items-center gap-2 text-base">
                  Policies ({policies?.length ?? 0})
                </CardTitle>
                <Dialog open={policyOpen} onOpenChange={setPolicyOpen}>
                  <DialogTrigger asChild>
                    <Button variant="outline" size="sm" className="w-full gap-1 sm:w-auto">
                      <Plus className="h-3.5 w-3.5" />
                      Set Policy
                    </Button>
                  </DialogTrigger>
                  <DialogContent>
                    <DialogHeader>
                      <DialogTitle>Set Approval Policy</DialogTitle>
                      <DialogDescription>
                        Save a default tool decision so repeat requests stop interrupting operators.
                      </DialogDescription>
                    </DialogHeader>
                    <div className="space-y-4 pt-4">
                      <div className="space-y-3">
                        <Label>Tool Name</Label>
                        <Input
                          value={policyTool}
                          onChange={(e) => setPolicyTool(e.target.value)}
                          placeholder="e.g. exec, write, web_fetch"
                        />
                      </div>
                      <div className="space-y-3">
                        <Label>Decision</Label>
                        <Select value={policyDecision} onValueChange={setPolicyDecision}>
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="allow_always">Always allow</SelectItem>
                            <SelectItem value="deny">Deny</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      <Button
                        onClick={handleSetPolicy}
                        disabled={!policyTool.trim() || setPolicy.isPending}
                        className="w-full"
                      >
                        {setPolicy.isPending ? "Setting..." : "Set Policy"}
                      </Button>
                    </div>
                  </DialogContent>
                </Dialog>
              </CardHeader>
              <CardContent>
                {policiesLoading ? (
                  <Skeleton className="h-20 w-full" />
                ) : !policies?.length ? (
                  <p className="text-sm text-muted-foreground">No policies configured</p>
                ) : (
                  <div className="-mx-4 overflow-x-auto px-4 sm:mx-0 sm:px-0">
                    <Table className="min-w-[32rem]">
                      <TableHeader>
                        <TableRow>
                          <TableHead className="py-3.5">Tool</TableHead>
                          <TableHead className="py-3.5">Policy</TableHead>
                          <TableHead className="py-3.5">Set At</TableHead>
                          <TableHead className="py-3.5 text-right">Actions</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {policies.map((p) => (
                          <TableRow key={p.tool_name}>
                            <TableCell className="py-3 font-mono text-sm">{p.tool_name}</TableCell>
                            <TableCell>
                              <Badge variant={p.decision === "deny" ? "destructive" : "default"}>
                                {decisionLabel(p.decision)}
                              </Badge>
                            </TableCell>
                            <TableCell className="py-3 text-xs text-muted-foreground">
                              {formatDateTime(p.decided_at)}
                            </TableCell>
                            <TableCell className="py-3 text-right">
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-9 w-9 text-destructive"
                                onClick={() => clearPolicy.mutate(p.tool_name)}
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                )}
              </CardContent>
            </Card>
          </div>
        </div>
      </div>

      <ApprovalDetailDialog approval={selectedApproval} open={detailOpen} onOpenChange={setDetailOpen} />
    </>
  );
}
