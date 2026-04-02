import { createFileRoute } from "@tanstack/react-router";
import { useState, useCallback, useMemo } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { useStatus, useHealth } from "@/hooks/use-system";
import { useSessionEvents } from "@/lib/websocket";
import { getToken } from "@/lib/auth";
import {
  Bug,
  Play,
  Server,
  Radio,
  Trash2,
  Copy,
  RefreshCw,
  Clock3,
} from "lucide-react";

export const Route = createFileRoute("/_admin/debug")({
  component: DebugPage,
});

interface ApiTestResult {
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: unknown;
  latency_ms: number;
}

function JsonTree({ value, level = 0 }: { value: unknown; level?: number }) {
  if (value === null || typeof value !== "object") {
    return <span className="text-foreground">{JSON.stringify(value)}</span>;
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return <span>[]</span>;
    }

    return (
      <div className="space-y-1">
        <span>[</span>
        <div className="space-y-1 pl-4">
          {value.map((item, index) => (
            <div key={index} className="flex gap-2">
              <span className="text-muted-foreground">{index}:</span>
              <JsonTree value={item} level={level + 1} />
            </div>
          ))}
        </div>
        <span>]</span>
      </div>
    );
  }

  const entries = Object.entries(value);
  if (entries.length === 0) {
    return <span>{"{}"}</span>;
  }

  return (
    <div className="space-y-1">
      <span>{"{"}</span>
      <div className="space-y-1 pl-4">
        {entries.map(([key, child]) => (
          <div key={`${level}-${key}`} className="flex gap-2">
            <span className="text-sky-600 dark:text-sky-400">{key}:</span>
            <JsonTree value={child} level={level + 1} />
          </div>
        ))}
      </div>
      <span>{"}"}</span>
    </div>
  );
}

function JsonTreeCard({
  title,
  loading,
  value,
}: {
  title: string;
  loading: boolean;
  value: unknown;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Server className="h-4 w-4" />
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {loading ? (
          <Skeleton className="h-40" />
        ) : (
          <div className="max-h-[30rem] overflow-auto rounded-md border bg-muted/20 p-4 font-mono text-sm">
            <JsonTree value={value} />
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function formatTimestamp(value: unknown) {
  if (typeof value !== "string") return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function DebugPage() {
  const { data: status, isLoading: statusLoading, refetch: refetchStatus } = useStatus();
  const { data: health, isLoading: healthLoading, refetch: refetchHealth } = useHealth();

  const [method, setMethod] = useState("GET");
  const [path, setPath] = useState("/health");
  const [body, setBody] = useState("");
  const [result, setResult] = useState<ApiTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  const [wsSessionId, setWsSessionId] = useState("");
  const normalizedWsSessionId = wsSessionId.trim() || undefined;
  const { events, connected, clearEvents } = useSessionEvents(
    normalizedWsSessionId,
    {
      enabled: !!normalizedWsSessionId,
      clearOnSessionChange: true,
    },
  );

  const runApiTest = useCallback(async () => {
    setTesting(true);
    const start = performance.now();

    try {
      const token = getToken();
      const headers: Record<string, string> = {
        Accept: "application/json",
      };
      let requestBody: string | undefined;

      if (token) {
        headers.Authorization = `Bearer ${token}`;
      }

      if (body.trim().length > 0 && method !== "GET" && method !== "DELETE") {
        JSON.parse(body);
        headers["Content-Type"] = "application/json";
        requestBody = body;
      }

      const response = await fetch(path, {
        method,
        headers,
        body: requestBody,
      });

      const text = await response.text();
      let parsedBody: unknown = text;
      try {
        parsedBody = text ? JSON.parse(text) : null;
      } catch {
        parsedBody = text;
      }

      const latency = Math.round(performance.now() - start);
      setResult({
        status: response.status,
        statusText: response.statusText,
        headers: Object.fromEntries(response.headers.entries()),
        body: parsedBody,
        latency_ms: latency,
      });
    } catch (error) {
      const latency = Math.round(performance.now() - start);
      setResult({
        status: 0,
        statusText: "Error",
        headers: {},
        body: error instanceof Error ? error.message : String(error),
        latency_ms: latency,
      });
    } finally {
      setTesting(false);
    }
  }, [method, path, body]);

  const wsEvents = useMemo(() => events.slice(-100).reverse(), [events]);
  const wsEventCount = events.length;

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Debug</h1>
        <p className="mt-1 text-muted-foreground">
          Live JSON inspection, raw API testing, and WebSocket event tracing.
        </p>
      </div>

      <Tabs defaultValue="status">
        <TabsList>
          <TabsTrigger value="status">
            <Server className="mr-2 h-4 w-4" />
            Status
          </TabsTrigger>
          <TabsTrigger value="api">
            <Bug className="mr-2 h-4 w-4" />
            API Tester
          </TabsTrigger>
          <TabsTrigger value="ws">
            <Radio className="mr-2 h-4 w-4" />
            WebSocket
          </TabsTrigger>
        </TabsList>

        <TabsContent value="status" className="space-y-6">
          <div className="flex items-center justify-end gap-3">
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                void refetchHealth();
                void refetchStatus();
              }}
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              Refresh
            </Button>
          </div>
          <div className="grid gap-6 lg:grid-cols-2">
            <JsonTreeCard title="/health" loading={healthLoading} value={health} />
            <JsonTreeCard title="/status" loading={statusLoading} value={status} />
          </div>
        </TabsContent>

        <TabsContent value="api" className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Request</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex gap-3">
                <Select value={method} onValueChange={setMethod}>
                  <SelectTrigger className="w-28">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="GET">GET</SelectItem>
                    <SelectItem value="POST">POST</SelectItem>
                    <SelectItem value="PUT">PUT</SelectItem>
                    <SelectItem value="PATCH">PATCH</SelectItem>
                    <SelectItem value="DELETE">DELETE</SelectItem>
                  </SelectContent>
                </Select>
                <Input
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  placeholder="/health"
                  className="flex-1 font-mono"
                />
                <Button onClick={runApiTest} disabled={testing}>
                  <Play className="mr-2 h-4 w-4" />
                  Send
                </Button>
              </div>

              {(method === "POST" || method === "PUT" || method === "PATCH") && (
                <Textarea
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                  placeholder='{"key": "value"}'
                  className="min-h-[100px] font-mono text-sm"
                />
              )}
            </CardContent>
          </Card>

          {result && (
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base">Response</CardTitle>
                  <div className="flex items-center gap-3">
                    <Badge
                      variant={
                        result.status >= 200 && result.status < 300
                          ? "default"
                          : "destructive"
                      }
                    >
                      {result.status} {result.statusText}
                    </Badge>
                    <Badge variant="outline" className="text-xs">
                      {result.latency_ms}ms
                    </Badge>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      onClick={() =>
                        navigator.clipboard.writeText(
                          JSON.stringify(
                            {
                              status: result.status,
                              statusText: result.statusText,
                              headers: result.headers,
                              body: result.body,
                            },
                            null,
                            2,
                          ),
                        )
                      }
                    >
                      <Copy className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-4">
                <div>
                  <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    Headers
                  </h3>
                  <pre className="max-h-[160px] overflow-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(result.headers, null, 2)}
                  </pre>
                </div>
                <div>
                  <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    Body
                  </h3>
                  <pre className="max-h-[400px] overflow-auto rounded-md bg-muted p-4 text-sm">
                    {typeof result.body === "string"
                      ? result.body
                      : JSON.stringify(result.body, null, 2)}
                  </pre>
                </div>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="ws" className="space-y-6">
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between gap-4">
                <CardTitle className="flex items-center gap-2 text-base">
                  <Radio className="h-4 w-4" />
                  WebSocket Event Log
                  {normalizedWsSessionId ? (
                    connected ? (
                      <Badge className="bg-green-500 text-xs">Connected</Badge>
                    ) : (
                      <Badge variant="secondary" className="text-xs">
                        Connecting
                      </Badge>
                    )
                  ) : (
                    <Badge variant="outline" className="text-xs">
                      Idle
                    </Badge>
                  )}
                  <Badge variant="outline" className="text-xs">
                    Ring buffer: {wsEventCount}
                  </Badge>
                </CardTitle>
                <div className="flex items-center gap-3">
                  <Input
                    value={wsSessionId}
                    onChange={(e) => setWsSessionId(e.target.value)}
                    placeholder="Session ID to subscribe..."
                    className="w-64 font-mono text-sm"
                  />
                  <Button variant="outline" size="sm" onClick={clearEvents}>
                    <Trash2 className="mr-2 h-4 w-4" />
                    Clear
                  </Button>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="max-h-[500px] overflow-y-auto rounded-md border bg-muted/20 p-2 font-mono text-xs">
                {!normalizedWsSessionId ? (
                  <p className="p-4 text-center text-muted-foreground">
                    Enter a session ID to subscribe.
                  </p>
                ) : wsEvents.length === 0 ? (
                  <p className="p-4 text-center text-muted-foreground">
                    No events received yet.
                  </p>
                ) : (
                  wsEvents.map((evt, i) => (
                    <div
                      key={`${evt.session_id}-${evt.kind}-${i}`}
                      className="border-b border-border/30 px-2 py-2 hover:bg-muted/40"
                    >
                      <div className="flex flex-wrap items-center gap-2.5">
                        <Badge variant="outline" className="text-[10px]">
                          {evt.kind}
                        </Badge>
                        <span className="text-muted-foreground">
                          {evt.session_id ? `${evt.session_id.slice(0, 8)}...` : "unknown"}
                        </span>
                        {typeof evt.payload === "object" && evt.payload && "timestamp" in (evt.payload as Record<string, unknown>) ? (
                          <span className="inline-flex items-center gap-1 text-muted-foreground">
                            <Clock3 className="h-3 w-3" />
                            {formatTimestamp((evt.payload as Record<string, unknown>).timestamp)}
                          </span>
                        ) : null}
                      </div>
                      <pre className="mt-1 whitespace-pre-wrap text-[11px]">
                        {JSON.stringify(evt.payload, null, 2)}
                      </pre>
                    </div>
                  ))
                )}
              </div>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
