import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { useChannelStatus } from "@/hooks/use-dashboard";
import { MessageSquare, Radio } from "lucide-react";

export const Route = createFileRoute("/_admin/channels")({
  component: ChannelsPage,
});

function ChannelsPage() {
  const { data: status, isLoading } = useChannelStatus();
  const channels = status?.configured ?? [];

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Channels</h1>
        <p className="mt-1 text-muted-foreground">Configured communication channels</p>
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Configured adapters</CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-7 w-10" /> : <p className="text-2xl font-bold">{channels.length}</p>}
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">Active routed sessions</CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-7 w-10" /> : <p className="text-2xl font-bold">{status?.active_sessions ?? 0}</p>}
          </CardContent>
        </Card>
      </div>

      {isLoading ? (
        <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-32" />
          ))}
        </div>
      ) : !channels.length ? (
        <Card>
          <CardContent className="py-8 text-center">
            <Radio className="mx-auto h-8 w-8 text-muted-foreground" />
            <p className="mt-2 text-sm text-muted-foreground">No channels configured</p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {channels.map((channel) => (
            <Card key={channel.kind}>
              <CardHeader className="flex flex-row items-center gap-3 pb-2">
                <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
                  <Radio className="h-5 w-5 text-primary" />
                </div>
                <div>
                  <CardTitle className="text-base capitalize">{channel.name}</CardTitle>
                  <Badge variant={channel.enabled ? "default" : "secondary"} className="mt-1 text-xs">
                    {channel.enabled ? "Connected" : "Disabled"}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="space-y-2 text-sm text-muted-foreground">
                <p>Adapter kind: {channel.kind}</p>
                <div className="flex items-center gap-2 text-foreground">
                  <MessageSquare className="h-4 w-4 text-muted-foreground" />
                  <span>{status?.active_sessions ?? 0} active routed session(s)</span>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
