import { createFileRoute } from "@tanstack/react-router";
import { useState, useCallback } from "react";
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
import { api } from "@/lib/api-client";
import {
  Bug,
  Play,
  Activity,
  Server,
  Radio,
  Trash2,
  Copy,
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

function DebugPage() {
  const { data: status, isLoading: statusLoading } = useStatus();
  const { data: health, isLoading: healthLoading } = useHealth();

  // API tester state
  const [method, setMethod] = useState("GET");
  const [path, setPath] = useState("/health");
  const [body, setBody] = useState("");
  const [result, setResult] = useState<ApiTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  // WS event log
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
      let response: unknown;
      switch (method) {
        case "GET":
          response = await api.get(path);
          break;
        case "POST":
          response = await api.post(path, body ? JSON.parse(body) : undefined);
          break;
        case "PUT":
          response = await api.put(path, body ? JSON.parse(body) : undefined);
          break;
        case "DELETE":
          response = await api.delete(path);
          break;
      }
      const latency = Math.round(performance.now() - start);
      setResult({
        status: 200,
        statusText: "OK",
        headers: {},
        body: response,
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

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Debug</h1>
        <p className="text-muted-foreground">
          System inspection and API testing
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

        {/* Status tab */}
        <TabsContent value="status" className="space-y-4">
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Activity className="h-4 w-4" />
                  /health
                </CardTitle>
              </CardHeader>
              <CardContent>
                {healthLoading ? (
                  <Skeleton className="h-40" />
                ) : (
                  <pre className="overflow-x-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(health, null, 2)}
                  </pre>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Server className="h-4 w-4" />
                  /status
                </CardTitle>
              </CardHeader>
              <CardContent>
                {statusLoading ? (
                  <Skeleton className="h-40" />
                ) : (
                  <pre className="overflow-x-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(status, null, 2)}
                  </pre>
                )}
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        {/* API Tester tab */}
        <TabsContent value="api" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Request</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex gap-2">
                <Select value={method} onValueChange={setMethod}>
                  <SelectTrigger className="w-28">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="GET">GET</SelectItem>
                    <SelectItem value="POST">POST</SelectItem>
                    <SelectItem value="PUT">PUT</SelectItem>
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

              {(method === "POST" || method === "PUT") && (
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
                  <div className="flex items-center gap-2">
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
                          JSON.stringify(result.body, null, 2)
                        )
                      }
                    >
                      <Copy className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              </CardHeader>
              <CardContent>
                <pre className="max-h-[400px] overflow-auto rounded-md bg-muted p-4 text-sm">
                  {typeof result.body === "string"
                    ? result.body
                    : JSON.stringify(result.body, null, 2)}
                </pre>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        {/* WebSocket tab */}
        <TabsContent value="ws" className="space-y-4">
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="flex items-center gap-2 text-base">
                  <Radio className="h-4 w-4" />
                  WebSocket Events
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
                </CardTitle>
                <div className="flex items-center gap-2">
                  <Input
                    value={wsSessionId}
                    onChange={(e) => setWsSessionId(e.target.value)}
                    placeholder="Session ID to subscribe..."
                    className="w-64 font-mono text-sm"
                  />
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={clearEvents}
                  >
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
                ) : events.length === 0 ? (
                  <p className="p-4 text-center text-muted-foreground">
                    No events received yet
                  </p>
                ) : (
                  events.map((evt, i) => (
                    <div
                      key={i}
                      className="border-b border-border/30 px-1 py-1 hover:bg-muted/40"
                    >
                      <div className="flex items-center gap-2">
                        <Badge variant="outline" className="text-[10px]">
                          {evt.kind}
                        </Badge>
                        <span className="text-muted-foreground">
                          {evt.session_id ? `${evt.session_id.slice(0, 8)}...` : "unknown"}
                        </span>
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
