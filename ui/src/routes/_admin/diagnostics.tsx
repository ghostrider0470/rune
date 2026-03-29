import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { useHealth, useInstanceHealth, useStatus } from "@/hooks/use-system";
import { useDashboardDiagnostics } from "@/hooks/use-dashboard";
import {
  Activity,
  Server,
  FolderOpen,
  AlertTriangle,
  XCircle,
  Info,
  CheckCircle2,
  Globe,
  Network,
  WifiOff,
} from "lucide-react";

export const Route = createFileRoute("/_admin/diagnostics")({
  component: DiagnosticsPage,
});

function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function DiagnosticsPage() {
  const { data: health, isLoading: healthLoading } = useHealth();
  const { data: status, isLoading: statusLoading } = useStatus();
  const { data: diagnostics, isLoading: diagLoading } = useDashboardDiagnostics();
  const { data: instanceHealth, isLoading: instanceHealthLoading } = useInstanceHealth();

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Diagnostics</h1>
        <p className="mt-1 text-muted-foreground">System health and configuration details</p>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        {/* Health card */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Activity className="h-4 w-4" />
              Health
            </CardTitle>
          </CardHeader>
          <CardContent>
            {healthLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-5 w-full" />
                <Skeleton className="h-5 w-3/4" />
              </div>
            ) : health ? (
              <dl className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Status</dt>
                  <dd>
                    <Badge variant={health.status === "ok" ? "default" : "destructive"}>
                      {health.status}
                    </Badge>
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Service</dt>
                  <dd className="font-mono text-xs">{health.service}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Version</dt>
                  <dd className="font-mono text-xs">{health.version}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Uptime</dt>
                  <dd>{formatUptime(health.uptime_seconds)}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Sessions</dt>
                  <dd>{health.session_count}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">WS Subscribers</dt>
                  <dd>{health.ws_subscribers}</dd>
                </div>
              </dl>
            ) : null}
          </CardContent>
        </Card>

        {/* Status card */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Server className="h-4 w-4" />
              Status
            </CardTitle>
          </CardHeader>
          <CardContent>
            {statusLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-5 w-full" />
                <Skeleton className="h-5 w-3/4" />
                <Skeleton className="h-5 w-1/2" />
              </div>
            ) : status ? (
              <dl className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Bind</dt>
                  <dd className="font-mono text-xs">{status.bind}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Version</dt>
                  <dd className="font-mono text-xs">{status.version}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Auth</dt>
                  <dd>
                    <Badge variant={status.auth_enabled ? "default" : "secondary"}>
                      {status.auth_enabled ? "enabled" : "disabled"}
                    </Badge>
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Model Providers</dt>
                  <dd>{status.configured_model_providers}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Backend</dt>
                  <dd className="font-mono text-xs">{status.active_model_backend}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Tools</dt>
                  <dd>{status.registered_tools}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Cron Jobs</dt>
                  <dd>{status.cron_job_count}</dd>
                </div>
              </dl>
            ) : null}
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Globe className="h-4 w-4" />
              Instance Identity & Capabilities
            </CardTitle>
          </CardHeader>
          <CardContent>
            {instanceHealthLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-5 w-full" />
                <Skeleton className="h-5 w-3/4" />
                <Skeleton className="h-5 w-1/2" />
              </div>
            ) : instanceHealth ? (
              <div className="space-y-4 text-sm">
                <dl className="space-y-2">
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Instance</dt>
                    <dd className="text-right font-medium">{instanceHealth.capabilities.identity.name}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Instance ID</dt>
                    <dd className="font-mono text-xs">{instanceHealth.capabilities.identity.id}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Advertised Address</dt>
                    <dd className="font-mono text-xs">{instanceHealth.capabilities.identity.advertised_addr ?? "not configured"}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Roles</dt>
                    <dd className="text-right">
                      <div className="flex flex-wrap justify-end gap-1">
                        {instanceHealth.capabilities.identity.roles.length ? instanceHealth.capabilities.identity.roles.map((role) => (
                          <Badge key={role} variant="outline">{role}</Badge>
                        )) : <span className="text-muted-foreground">none</span>}
                      </div>
                    </dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Peer Count</dt>
                    <dd>{instanceHealth.capabilities.peer_count}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Comms</dt>
                    <dd className="font-mono text-xs">{instanceHealth.capabilities.comms_transport}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Memory</dt>
                    <dd>{instanceHealth.capabilities.memory_mode}</dd>
                  </div>
                  <div className="flex justify-between gap-4">
                    <dt className="text-muted-foreground">Models</dt>
                    <dd>{instanceHealth.capabilities.configured_models.length}</dd>
                  </div>
                </dl>
                <div className="space-y-2">
                  <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Enabled channels</p>
                  <div className="flex flex-wrap gap-1">
                    {instanceHealth.capabilities.channels.length ? instanceHealth.capabilities.channels.map((channel) => (
                      <Badge key={channel} variant="secondary">{channel}</Badge>
                    )) : <span className="text-sm text-muted-foreground">No active channels</span>}
                  </div>
                </div>
              </div>
            ) : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Network className="h-4 w-4" />
              Peer Health
            </CardTitle>
          </CardHeader>
          <CardContent>
            {instanceHealthLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-12 w-full" />
                <Skeleton className="h-12 w-full" />
              </div>
            ) : !instanceHealth?.peers.length ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <WifiOff className="h-4 w-4" />
                No peers configured
              </div>
            ) : (
              <div className="space-y-3">
                {instanceHealth.peers.map((peer) => (
                  <div key={peer.id} className="rounded-lg border p-3 text-sm">
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <div className="font-medium">{peer.name}</div>
                        <div className="font-mono text-xs text-muted-foreground">{peer.id}</div>
                      </div>
                      <Badge variant={peer.status === "healthy" ? "default" : peer.status === "degraded" ? "secondary" : "destructive"}>
                        {peer.status}
                      </Badge>
                    </div>
                    <div className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
                      <div>
                        <p className="text-xs text-muted-foreground">Health URL</p>
                        <p className="font-mono text-xs break-all">{peer.health_url}</p>
                      </div>
                      <div>
                        <p className="text-xs text-muted-foreground">Checked</p>
                        <p>{new Date(peer.checked_at).toLocaleString()}</p>
                      </div>
                      <div>
                        <p className="text-xs text-muted-foreground">Latency</p>
                        <p>{peer.latency_ms != null ? `${peer.latency_ms} ms` : "n/a"}</p>
                      </div>
                      <div>
                        <p className="text-xs text-muted-foreground">Detail</p>
                        <p className="break-words">{peer.detail}</p>
                      </div>
                      <div>
                        <p className="text-xs text-muted-foreground">Load</p>
                        <p>{peer.load ? `${peer.load.session_count} sessions · ${peer.load.ws_connections} ws` : "n/a"}</p>
                      </div>
                      <div>
                        <p className="text-xs text-muted-foreground">Models</p>
                        <p>{peer.configured_models.length ? peer.configured_models.join(", ") : "none"}</p>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Paths */}
      {status?.config_paths && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <FolderOpen className="h-4 w-4" />
              Config Paths
            </CardTitle>
          </CardHeader>
          <CardContent>
            <dl className="space-y-2 text-sm">
              <div className="flex justify-between">
                <dt className="text-muted-foreground">Sessions</dt>
                <dd className="font-mono text-xs">{status.config_paths.sessions_dir}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-muted-foreground">Memory</dt>
                <dd className="font-mono text-xs">{status.config_paths.memory_dir}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-muted-foreground">Logs</dt>
                <dd className="font-mono text-xs">{status.config_paths.logs_dir}</dd>
              </div>
            </dl>
          </CardContent>
        </Card>
      )}

      {/* Diagnostics items */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <AlertTriangle className="h-4 w-4" />
            Diagnostic Items
          </CardTitle>
        </CardHeader>
        <CardContent>
          {diagLoading ? (
            <div className="space-y-2">
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-12 w-full" />
            </div>
          ) : !diagnostics?.items.length ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <CheckCircle2 className="h-4 w-4 text-green-500" />
              All clear — no diagnostics to report
            </div>
          ) : (
            <div className="space-y-2">
              {diagnostics.items.map((item, i) => (
                <div
                  key={i}
                  className="flex items-start gap-3 rounded-lg border p-3 text-sm"
                >
                  {item.level === "error" ? (
                    <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                  ) : item.level === "warn" ? (
                    <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-500" />
                  ) : (
                    <Info className="mt-0.5 h-4 w-4 shrink-0 text-blue-500" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{item.source}</span>
                      <Badge
                        variant={
                          item.level === "error"
                            ? "destructive"
                            : item.level === "warn"
                              ? "secondary"
                              : "outline"
                        }
                        className="text-xs"
                      >
                        {item.level}
                      </Badge>
                    </div>
                    <p className="mt-1 text-muted-foreground">{item.message}</p>
                    <p className="mt-1 text-xs text-muted-foreground">{item.observed_at}</p>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
