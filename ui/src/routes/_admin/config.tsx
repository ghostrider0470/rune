import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useStatus } from "@/hooks/use-system";
import {
  Settings2,
  RotateCcw,
  Search,
  FileJson,
  AlertTriangle,
  Ban,
} from "lucide-react";

export const Route = createFileRoute("/_admin/config")({
  component: ConfigPage,
});

function ConfigPage() {
  const { data: status, isLoading } = useStatus();
  const [search, setSearch] = useState("");

  const derivedConfig = useMemo<Record<string, unknown> | null>(() => {
    if (!status) return null;

    return {
      gateway: {
        bind: status.bind,
        auth_enabled: status.auth_enabled,
        active_model_backend: status.active_model_backend,
      },
      counts: {
        configured_model_providers: status.configured_model_providers,
        registered_tools: status.registered_tools,
        session_count: status.session_count,
        cron_job_count: status.cron_job_count,
        ws_subscribers: status.ws_subscribers,
      },
      paths: status.config_paths,
      runtime: {
        version: status.version,
        uptime_seconds: status.uptime_seconds,
        status: status.status,
      },
    };
  }, [status]);

  const configSections = useMemo(() => {
    if (!derivedConfig) return [];

    return Object.entries(derivedConfig).filter(([key]) =>
      search ? key.toLowerCase().includes(search.toLowerCase()) : true,
    );
  }, [derivedConfig, search]);

  const rawJson = useMemo(
    () => (derivedConfig ? JSON.stringify(derivedConfig, null, 2) : ""),
    [derivedConfig],
  );

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Configuration</h1>
          <p className="text-muted-foreground">
            Runtime-derived configuration view until config read/write endpoints land.
          </p>
        </div>
        <Badge variant="outline" className="gap-1 text-yellow-700">
          <Ban className="h-3.5 w-3.5" />
          Read-only
        </Badge>
      </div>

      <Card className="border-yellow-500/30 bg-yellow-500/5">
        <CardContent className="flex items-start gap-3 pt-6 text-sm text-muted-foreground">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-600" />
          <div className="space-y-1">
            <p className="font-medium text-foreground">Config endpoints are not wired yet.</p>
            <p>
              This page now reflects live gateway status and resolved paths instead of calling
              missing <code>/config</code> and <code>/config/schema</code> routes.
            </p>
          </div>
        </CardContent>
      </Card>

      <Tabs defaultValue="form">
        <TabsList>
          <TabsTrigger value="form">
            <Settings2 className="mr-2 h-4 w-4" />
            Sections
          </TabsTrigger>
          <TabsTrigger value="json">
            <FileJson className="mr-2 h-4 w-4" />
            JSON
          </TabsTrigger>
        </TabsList>

        <TabsContent value="form" className="space-y-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search configuration sections..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-10"
            />
          </div>

          {isLoading ? (
            <div className="space-y-4">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-32 w-full" />
              ))}
            </div>
          ) : !configSections.length ? (
            <p className="text-sm text-muted-foreground">
              {search ? "No matching configuration sections" : "No runtime configuration available"}
            </p>
          ) : (
            configSections.map(([section, value]) => (
              <Card key={section}>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2 text-base">
                    <Settings2 className="h-4 w-4" />
                    {section}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="overflow-x-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(value, null, 2)}
                  </pre>
                </CardContent>
              </Card>
            ))
          )}
        </TabsContent>

        <TabsContent value="json">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-3">
              <CardTitle className="text-base">Resolved Runtime Snapshot</CardTitle>
              <Button variant="outline" size="sm" disabled>
                <RotateCcw className="mr-2 h-4 w-4" />
                Snapshot current
              </Button>
            </CardHeader>
            <CardContent>
              <Textarea
                value={rawJson}
                readOnly
                className="min-h-[500px] font-mono text-sm"
                placeholder="Loading runtime configuration..."
              />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
