import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardSummary } from "@/hooks/use-dashboard";
import { Radio } from "lucide-react";

export const Route = createFileRoute("/_admin/channels")({
  component: ChannelsPage,
});

function ChannelsPage() {
  const { data: summary, isLoading } = useDashboardSummary();

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Channels</h1>
        <p className="mt-1 text-muted-foreground">Configured communication channels</p>
      </div>

      {isLoading ? (
        <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-32" />
          ))}
        </div>
      ) : !summary?.channels.length ? (
        <Card>
          <CardContent className="py-8 text-center">
            <Radio className="mx-auto h-8 w-8 text-muted-foreground" />
            <p className="mt-2 text-sm text-muted-foreground">
              No channels configured
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {summary.channels.map((channel) => (
            <Card key={channel}>
              <CardHeader className="flex flex-row items-center gap-3 pb-2">
                <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
                  <Radio className="h-5 w-5 text-primary" />
                </div>
                <div>
                  <CardTitle className="text-base">{channel}</CardTitle>
                  <Badge variant="default" className="mt-1 text-xs">
                    Active
                  </Badge>
                </div>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  Channel is configured and receiving messages
                </p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
