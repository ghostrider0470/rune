import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import {
  useHeartbeatStatus,
  useHeartbeatEnable,
  useHeartbeatDisable,
  useGatewayStart,
  useGatewayStop,
  useGatewayRestart,
} from "@/hooks/use-system";
import { getToken } from "@/lib/auth";
import {
  Heart,
  Server,
  Key,
  Play,
  Square,
  RotateCw,
} from "lucide-react";

export const Route = createFileRoute("/_admin/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  const { data: heartbeat, isLoading: heartbeatLoading } = useHeartbeatStatus();
  const enableHeartbeat = useHeartbeatEnable();
  const disableHeartbeat = useHeartbeatDisable();
  const gatewayStart = useGatewayStart();
  const gatewayStop = useGatewayStop();
  const gatewayRestart = useGatewayRestart();

  const token = getToken();

  const handleHeartbeatToggle = (enabled: boolean) => {
    if (enabled) {
      enableHeartbeat.mutate();
    } else {
      disableHeartbeat.mutate();
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Settings</h1>
        <p className="text-muted-foreground">Gateway configuration and controls</p>
      </div>

      {/* Heartbeat */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Heart className="h-4 w-4" />
            Heartbeat
          </CardTitle>
        </CardHeader>
        <CardContent>
          {heartbeatLoading ? (
            <Skeleton className="h-12 w-full" />
          ) : heartbeat ? (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label>Heartbeat Enabled</Label>
                  <p className="text-sm text-muted-foreground">
                    Send periodic heartbeat pings to keep sessions alive
                  </p>
                </div>
                <Switch
                  checked={heartbeat.enabled}
                  onCheckedChange={handleHeartbeatToggle}
                  disabled={enableHeartbeat.isPending || disableHeartbeat.isPending}
                />
              </div>
              <Separator />
              <dl className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Interval</dt>
                  <dd>{heartbeat.interval_seconds}s</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-muted-foreground">Last Heartbeat</dt>
                  <dd className="text-xs">
                    {heartbeat.last_heartbeat_at
                      ? new Date(heartbeat.last_heartbeat_at).toLocaleString()
                      : "Never"}
                  </dd>
                </div>
              </dl>
            </div>
          ) : null}
        </CardContent>
      </Card>

      {/* Gateway control */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Server className="h-4 w-4" />
            Gateway Control
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => {
                if (confirm("Start the gateway?")) gatewayStart.mutate();
              }}
              disabled={gatewayStart.isPending}
            >
              <Play className="h-4 w-4" />
              Start
            </Button>
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => {
                if (confirm("Stop the gateway?")) gatewayStop.mutate();
              }}
              disabled={gatewayStop.isPending}
            >
              <Square className="h-4 w-4" />
              Stop
            </Button>
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => {
                if (confirm("Restart the gateway?")) gatewayRestart.mutate();
              }}
              disabled={gatewayRestart.isPending}
            >
              <RotateCw className="h-4 w-4" />
              Restart
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Connection info */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Key className="h-4 w-4" />
            Connection
          </CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="space-y-3 text-sm">
            <div className="flex items-center justify-between">
              <dt className="text-muted-foreground">API URL</dt>
              <dd className="font-mono text-xs">{window.location.origin}</dd>
            </div>
            <div className="flex items-center justify-between">
              <dt className="text-muted-foreground">Auth Token</dt>
              <dd>
                {token ? (
                  <Badge variant="outline" className="font-mono text-xs">
                    {token.slice(0, 4)}{"****"}{token.slice(-4)}
                  </Badge>
                ) : (
                  <span className="text-muted-foreground">Not set</span>
                )}
              </dd>
            </div>
          </dl>
        </CardContent>
      </Card>
    </div>
  );
}
