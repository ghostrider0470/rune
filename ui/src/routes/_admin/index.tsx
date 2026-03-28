import { createFileRoute, Link } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import {
  useDashboardSummary,
  useDashboardModels,
  useDashboardSessions,
  useDashboardDiagnostics,
  useChannelStatus,
  useGatewayRestart,
  useDashboardLiveUpdates,
} from "@/hooks/use-dashboard";
import { cn } from "@/lib/utils";
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Clock,
  Cpu,
  Info,
  MessageSquare,
  Radio,
  RefreshCw,
  Settings,
  Shield,
  Users,
  Wrench,
  XCircle,
  Zap,
} from "lucide-react";
import { toast } from "sonner";

export const Route = createFileRoute("/_admin/")({
  component: DashboardPage,
});

function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function formatRelativeTime(timestamp: string): string {
  const date = new Date(timestamp);
  const diffMs = Date.now() - date.getTime();
  const diffMin = Math.max(0, Math.floor(diffMs / 60000));
  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffH = Math.floor(diffMin / 60);
  if (diffH < 24) return `${diffH}h ago`;
  const diffD = Math.floor(diffH / 24);
  return `${diffD}d ago`;
}

function DiagnosticIcon({ level }: { level: string }) {
  if (level === "error") {
    return <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />;
  }
  if (level === "warn") {
    return <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-500" />;
  }
  return <Info className="mt-0.5 h-4 w-4 shrink-0 text-blue-500" />;
}

function statusTone(level: string) {
  if (level === "error") return "destructive" as const;
  if (level === "warn") return "secondary" as const;
  return "outline" as const;
}

function describeDashboardEvent(kind: string): { title: string; detail: string } {
  switch (kind) {
    case "turn.started":
      return { title: "Turn started", detail: "A session started a new model turn." };
    case "turn.completed":
      return { title: "Turn completed", detail: "A session completed a model turn." };
    case "turn.failed":
      return { title: "Turn failed", detail: "A session reported a failed model turn." };
    case "tool.approval_required":
      return { title: "Approval pending", detail: "A tool call is waiting for operator approval." };
    case "tool.completed":
      return { title: "Tool completed", detail: "A tool invocation completed successfully." };
    case "tool.failed":
      return { title: "Tool failed", detail: "A tool invocation failed and needs inspection." };
    case "approval.created":
      return { title: "Approval created", detail: "A new approval request was emitted." };
    case "approval.resolved":
      return { title: "Approval resolved", detail: "An approval request was resolved." };
    default:
      return { title: kind.replace(/\./g, " "), detail: "Live runtime event received over WebSocket." };
  }
}

function DashboardPage() {
  const { data: summary, isLoading: summaryLoading } = useDashboardSummary();
  const { data: models } = useDashboardModels();
  const { data: sessions } = useDashboardSessions();
  const { data: diagnostics } = useDashboardDiagnostics();
  const { data: channelStatus } = useChannelStatus();
  const restartGateway = useGatewayRestart();
  const { connected: liveConnected, activity: liveActivity } = useDashboardLiveUpdates();

  const topDiagnostics = diagnostics?.items.slice(0, 5) ?? [];
  const activeChannels = channelStatus?.configured ?? [];

  const quickActions = [
    {
      title: "Open live chat",
      description: "Jump into the operator workspace and inspect a running session.",
      to: "/chat" as const,
      icon: MessageSquare,
    },
    {
      title: "Inspect channels",
      description: "Review adapter status and active routed sessions.",
      to: "/channels" as const,
      icon: Radio,
    },
    {
      title: "Adjust settings",
      description: "Change media, auth, and runtime defaults.",
      to: "/settings" as const,
      icon: Settings,
    },
    {
      title: "Run diagnostics",
      description: "Open debug surfaces and validate system health.",
      to: "/diagnostics" as const,
      icon: Wrench,
    },
  ];

  return (
    <div className="space-y-6 sm:space-y-8">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight sm:text-3xl">Dashboard</h1>
          <p className="mt-1 text-muted-foreground">
            Health, activity, and operator shortcuts for the Rune gateway
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button asChild variant="outline">
            <a href="/health" target="_blank" rel="noreferrer">Health JSON</a>
          </Button>
          <Button
            onClick={() =>
              restartGateway.mutate(undefined, {
                onSuccess: () => toast.success("Gateway restart requested"),
                onError: (error) => toast.error(error.message),
              })
            }
            disabled={restartGateway.isPending}
          >
            <RefreshCw className={cn("h-4 w-4", restartGateway.isPending && "animate-spin")} />
            {restartGateway.isPending ? "Restarting..." : "Restart gateway"}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 sm:gap-6 lg:grid-cols-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Gateway Status</CardTitle>
            <Activity className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-20" />
            ) : (
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <Badge variant={summary?.gateway_status === "running" ? "default" : "destructive"}>
                    {summary?.gateway_status ?? "unknown"}
                  </Badge>
                  <span className="text-xs text-muted-foreground">{summary?.bind}</span>
                </div>
                <p className="text-xs text-muted-foreground">
                  {summary?.auth_enabled ? "Auth required" : "Auth disabled"}
                </p>
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Uptime</CardTitle>
            <Clock className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-20" />
            ) : (
              <p className="text-2xl font-bold">{formatUptime(summary?.uptime_seconds ?? 0)}</p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Tooling & Models</CardTitle>
            <Cpu className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-12" />
            ) : (
              <>
                <p className="text-2xl font-bold">{summary?.configured_model_count ?? 0}</p>
                <p className="text-xs text-muted-foreground">
                  {summary?.provider_count ?? 0} provider(s), default {summary?.default_model ?? "unset"}
                </p>
              </>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Sessions</CardTitle>
            <Users className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-12" />
            ) : (
              <>
                <p className="text-2xl font-bold">{summary?.session_count ?? 0}</p>
                <p className="text-xs text-muted-foreground">
                  {summary?.ws_subscribers ?? 0} live websocket subscriber(s)
                </p>
              </>
            )}
          </CardContent>
        </Card>
      </div>

      {summary && (
        <div className="flex flex-wrap gap-2">
          <Badge variant="outline" className="gap-2">
            <Shield className="h-3 w-3" />
            Auth: {summary.auth_enabled ? "enabled" : "disabled"}
          </Badge>
          <Badge variant="outline" className="gap-2">
            <Users className="h-3 w-3" />
            WS subscribers: {summary.ws_subscribers}
          </Badge>
          {summary.channels.map((ch) => (
            <Badge key={ch} variant="outline" className="gap-2">
              <Radio className="h-3 w-3" />
              {ch}
            </Badge>
          ))}
          {summary.default_model && (
            <Badge variant="outline" className="gap-2">
              <Cpu className="h-3 w-3" />
              {summary.default_model}
            </Badge>
          )}
        </div>
      )}

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.15fr_0.85fr] xl:gap-6">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-4">
            <div>
              <CardTitle className="flex items-center gap-2 text-base">
                <Zap className="h-4 w-4" />
                Quick actions
              </CardTitle>
              <p className="text-sm text-muted-foreground">
                Common operator entry points for the current control plane.
              </p>
            </div>
          </CardHeader>
          <CardContent className="grid gap-3 sm:grid-cols-2">
            {quickActions.map((action) => {
              const Icon = action.icon;
              return (
                <Link
                  key={action.title}
                  to={action.to}
                  className="group rounded-lg border p-4 transition-colors hover:border-primary/40 hover:bg-accent/40"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="space-y-2">
                      <div className="flex items-center gap-2 font-medium">
                        <Icon className="h-4 w-4 text-primary" />
                        {action.title}
                      </div>
                      <p className="text-sm text-muted-foreground">{action.description}</p>
                    </div>
                    <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
                  </div>
                </Link>
              );
            })}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Radio className="h-4 w-4" />
              Connection status
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="flex items-center justify-between rounded-md border px-3 py-2 text-sm">
              <span className="text-muted-foreground">Configured channels</span>
              <Badge variant="outline">{activeChannels.length}</Badge>
            </div>
            <div className="flex items-center justify-between rounded-md border px-3 py-2 text-sm">
              <span className="text-muted-foreground">Active routed sessions</span>
              <Badge variant="outline">{channelStatus?.active_sessions ?? 0}</Badge>
            </div>
            {!activeChannels.length ? (
              <p className="text-sm text-muted-foreground">No channel adapters configured yet.</p>
            ) : (
              activeChannels.map((channel) => (
                <div
                  key={channel.kind}
                  className="flex items-center justify-between rounded-md border px-3 py-2"
                >
                  <div>
                    <p className="font-medium capitalize">{channel.name}</p>
                    <p className="text-xs text-muted-foreground">Adapter kind: {channel.kind}</p>
                  </div>
                  <Badge variant={channel.enabled ? "default" : "secondary"}>
                    {channel.enabled ? "connected" : "disabled"}
                  </Badge>
                </div>
              ))
            )}
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 lg:gap-6">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-4">
            <div>
              <CardTitle className="flex items-center gap-2 text-base">
                <AlertTriangle className="h-4 w-4" />
                Activity feed
              </CardTitle>
              <p className="text-sm text-muted-foreground">
                Recent diagnostics and runtime events. Updates stream over WebSocket.
              </p>
            </div>
            <Badge variant={liveConnected ? "default" : "secondary"}>
              {liveConnected ? "live" : "reconnecting"}
            </Badge>
            <Button asChild variant="ghost" size="sm">
              <Link to="/diagnostics">Open diagnostics</Link>
            </Button>
          </CardHeader>
          <CardContent>
            {!liveActivity.length && !topDiagnostics.length ? (
              <p className="text-sm text-muted-foreground">No diagnostics to report.</p>
            ) : (
              <div className="space-y-2">
                {liveActivity.map((item) => {
                  const eventCopy = describeDashboardEvent(item.kind);
                  return (
                    <div key={item.id} className="rounded-md border p-3">
                      <div className="flex items-start gap-2 text-sm">
                        <Activity className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <p className="font-medium">{eventCopy.title}</p>
                            <Badge variant="outline">{item.kind}</Badge>
                            <span className="text-xs text-muted-foreground">
                              {formatRelativeTime(item.observed_at)}
                            </span>
                          </div>
                          <p className="mt-1 text-muted-foreground">
                            {eventCopy.detail} Session {item.session_id.slice(0, 8)}...
                          </p>
                        </div>
                      </div>
                    </div>
                  );
                })}
                {topDiagnostics.map((item, i) => (
                  <div key={`${item.source}-${item.observed_at}-${i}`} className="rounded-md border p-3">
                    <div className="flex items-start gap-2 text-sm">
                      <DiagnosticIcon level={item.level} />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <p className="font-medium">{item.source}</p>
                          <Badge variant={statusTone(item.level)}>{item.level}</Badge>
                          <span className="text-xs text-muted-foreground">
                            {formatRelativeTime(item.observed_at)}
                          </span>
                        </div>
                        <p className="mt-1 text-muted-foreground">{item.message}</p>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <MessageSquare className="h-4 w-4" />
              Recent sessions
            </CardTitle>
          </CardHeader>
          <CardContent>
            {!sessions?.length ? (
              <p className="text-sm text-muted-foreground">No sessions.</p>
            ) : (
              <div className="overflow-x-auto">
                <Table className="min-w-[32rem]">
                  <TableHeader>
                    <TableRow>
                      <TableHead>Session</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Kind</TableHead>
                      <TableHead>Last activity</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {sessions.slice(0, 6).map((s) => (
                      <TableRow key={s.id}>
                        <TableCell>
                          <div className="flex flex-col gap-1">
                            <Link
                              to="/chat"
                              search={{ session: s.id }}
                              className="font-mono text-xs text-primary hover:underline"
                            >
                              {s.id.slice(0, 8)}...
                            </Link>
                            {(s.channel_ref || s.routing_ref) && (
                              <span className="text-xs text-muted-foreground">
                                {s.channel_ref ?? s.routing_ref}
                              </span>
                            )}
                          </div>
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={
                              s.status === "active"
                                ? "default"
                                : s.status === "idle"
                                  ? "secondary"
                                  : "outline"
                            }
                          >
                            {s.status}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-sm">{s.kind}</TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatRelativeTime(s.last_activity_at)}
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

      {models && models.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Cpu className="h-4 w-4" />
              Configured models
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-2">
              {models.map((m, i) => (
                <Badge key={i} variant={m.is_default ? "default" : "outline"} className="gap-2">
                  {m.is_default && <CheckCircle2 className="h-3 w-3" />}
                  {m.model_id}
                  <span className="text-muted-foreground">({m.provider_kind})</span>
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
