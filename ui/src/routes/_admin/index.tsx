import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Link } from "@tanstack/react-router";
import {
  useDashboardSummary,
  useDashboardModels,
  useDashboardSessions,
  useDashboardDiagnostics,
  useGatewayRestartAction,
} from "@/hooks/use-dashboard";
import {
  Activity,
  Clock,
  Cpu,
  MessageSquare,
  Shield,
  Users,
  Radio,
  AlertTriangle,
  CheckCircle2,
  XCircle,
  Info,
} from "lucide-react";

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

function DashboardPage() {
  const { data: summary, isLoading: summaryLoading } = useDashboardSummary();
  const { data: models } = useDashboardModels();
  const { data: sessions } = useDashboardSessions();
  const { data: diagnostics } = useDashboardDiagnostics();
  const restartGateway = useGatewayRestartAction();

  return (
    <div className="space-y-6 sm:space-y-8">
      <div>
        <h1 className="text-2xl font-bold tracking-tight sm:text-3xl">Dashboard</h1>
        <p className="mt-1 text-muted-foreground">Gateway overview and system health</p>
      </div>

      {/* Stat cards */}
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
              <div className="flex items-center gap-2">
                <Badge variant={summary?.gateway_status === "ok" ? "default" : "destructive"}>
                  {summary?.gateway_status ?? "unknown"}
                </Badge>
                <span className="text-xs text-muted-foreground">{summary?.bind}</span>
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
            <CardTitle className="text-sm font-medium">Models</CardTitle>
            <Cpu className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-12" />
            ) : (
              <>
                <p className="text-2xl font-bold">{summary?.configured_model_count ?? 0}</p>
                <p className="text-xs text-muted-foreground">
                  {summary?.provider_count ?? 0} provider(s)
                </p>
              </>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Sessions</CardTitle>
            <MessageSquare className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {summaryLoading ? (
              <Skeleton className="h-7 w-12" />
            ) : (
              <p className="text-2xl font-bold">{summary?.session_count ?? 0}</p>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Info badges */}
      {summary && (
        <div className="space-y-3">
          <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
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
            <Button
              variant="outline"
              onClick={() => restartGateway.mutate()}
              disabled={restartGateway.isPending}
            >
              {restartGateway.isPending ? "Restarting…" : "Restart gateway"}
            </Button>
          </div>
          {restartGateway.isError ? (
            <p className="text-sm text-destructive">
              {(restartGateway.error as Error).message || "Failed to restart gateway"}
            </p>
          ) : null}
          {restartGateway.isSuccess ? (
            <p className="text-sm text-emerald-600 dark:text-emerald-400">
              Gateway restart requested successfully.
            </p>
          ) : null}
        </div>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 lg:gap-6">
        {/* Diagnostics panel */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <AlertTriangle className="h-4 w-4" />
              Diagnostics
            </CardTitle>
          </CardHeader>
          <CardContent>
            {!diagnostics?.items.length ? (
              <p className="text-sm text-muted-foreground">No diagnostics to report</p>
            ) : (
              <div className="space-y-2">
                {diagnostics.items.slice(0, 5).map((item, i) => (
                  <div
                    key={i}
                    className="flex items-start gap-2 rounded-md border p-2 text-sm"
                  >
                    {item.level === "error" ? (
                      <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                    ) : item.level === "warn" ? (
                      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-500" />
                    ) : (
                      <Info className="mt-0.5 h-4 w-4 shrink-0 text-blue-500" />
                    )}
                    <div className="min-w-0 flex-1">
                      <p className="font-medium">{item.source}</p>
                      <p className="text-muted-foreground">{item.message}</p>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Recent sessions */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <MessageSquare className="h-4 w-4" />
              Recent Sessions
            </CardTitle>
          </CardHeader>
          <CardContent>
            {!sessions?.length ? (
              <p className="text-sm text-muted-foreground">No sessions</p>
            ) : (
              <div className="overflow-x-auto">
              <Table className="min-w-[24rem]">
                <TableHeader>
                  <TableRow>
                    <TableHead>ID</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Kind</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {sessions.slice(0, 5).map((s) => (
                    <TableRow key={s.id}>
                      <TableCell>
                        <Link
                          to="/chat"
                          search={{ session: s.id }}
                          className="font-mono text-xs text-primary hover:underline"
                        >
                          {s.id.slice(0, 8)}...
                        </Link>
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
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Models summary */}
      {models && models.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Cpu className="h-4 w-4" />
              Configured Models
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
