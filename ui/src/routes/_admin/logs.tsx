import { createFileRoute } from "@tanstack/react-router";
import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { buildWebSocketUrl } from "@/lib/websocket";
import {
  ScrollText,
  Download,
  Trash2,
  Search,
  Pause,
  Play,
  RefreshCw,
  Wifi,
  WifiOff,
  Info,
  ArrowDown,
} from "lucide-react";

export const Route = createFileRoute("/_admin/logs")({
  component: LogsPage,
});

interface LogEntry {
  timestamp: string;
  level: string;
  target: string;
  message: string;
  fields?: Record<string, unknown>;
}

type ConnectionState = "connecting" | "connected" | "reconnecting" | "disconnected";

const MAX_LOGS = 2000;
const RECONNECT_DELAY_MS = 3000;

const levelColors: Record<string, string> = {
  ERROR: "text-red-500 bg-red-500/10",
  WARN: "text-yellow-600 bg-yellow-500/10",
  INFO: "text-blue-500 bg-blue-500/10",
  DEBUG: "text-muted-foreground bg-muted",
  TRACE: "text-muted-foreground/70 bg-muted/60",
};

function normalizeLevel(level: unknown): string {
  return typeof level === "string" && level.trim()
    ? level.trim().toUpperCase()
    : "INFO";
}

function parseLogEntry(raw: unknown): LogEntry {
  if (raw && typeof raw === "object") {
    const candidate = raw as Partial<LogEntry> & {
      fields?: unknown;
    };

    return {
      timestamp:
        typeof candidate.timestamp === "string" && candidate.timestamp.trim()
          ? candidate.timestamp
          : new Date().toISOString(),
      level: normalizeLevel(candidate.level),
      target:
        typeof candidate.target === "string" && candidate.target.trim()
          ? candidate.target
          : "gateway",
      message:
        typeof candidate.message === "string"
          ? candidate.message
          : JSON.stringify(raw),
      fields:
        candidate.fields && typeof candidate.fields === "object"
          ? (candidate.fields as Record<string, unknown>)
          : undefined,
    };
  }

  return {
    timestamp: new Date().toISOString(),
    level: "INFO",
    target: "gateway",
    message: String(raw),
  };
}

function formatTimestamp(timestamp: string): string {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) return "—";
  return parsed.toLocaleString();
}

function LogsPage() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [levelFilter, setLevelFilter] = useState<string>("all");
  const [targetFilter, setTargetFilter] = useState<string>("all");
  const [autoScroll, setAutoScroll] = useState(true);
  const [paused, setPaused] = useState(false);
  const [connectionState, setConnectionState] = useState<ConnectionState>("connecting");
  const [selectedLogIndex, setSelectedLogIndex] = useState<number | null>(null);
  const [reconnectCount, setReconnectCount] = useState(0);

  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<number | null>(null);
  const pausedRef = useRef(paused);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimerRef.current !== null) {
      window.clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
  }, []);

  const disconnectWebSocket = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.onopen = null;
      wsRef.current.onmessage = null;
      wsRef.current.onclose = null;
      wsRef.current.onerror = null;
      wsRef.current.close();
      wsRef.current = null;
    }
  }, []);

  const appendLogEntry = useCallback((entry: LogEntry) => {
    setLogs((prev) => {
      const next = [...prev, entry];
      return next.length > MAX_LOGS ? next.slice(-MAX_LOGS) : next;
    });
  }, []);

  const connectRef = useRef<() => void>(() => undefined);

  useEffect(() => {
    let disposed = false;

    connectRef.current = () => {
      if (disposed) return;

      clearReconnectTimer();
      disconnectWebSocket();
      setConnectionState((current) =>
        current === "connected" ? "reconnecting" : "connecting",
      );

      try {
        const ws = new WebSocket(buildWebSocketUrl("/ws/logs"));
        wsRef.current = ws;

        ws.onopen = () => {
          if (disposed || wsRef.current !== ws) return;
          setConnectionState("connected");
        };

        ws.onmessage = (event) => {
          if (disposed || wsRef.current !== ws || pausedRef.current) return;

          try {
            appendLogEntry(parseLogEntry(JSON.parse(event.data) as unknown));
          } catch {
            appendLogEntry(parseLogEntry(event.data));
          }
        };

        ws.onclose = () => {
          if (wsRef.current === ws) {
            wsRef.current = null;
          }

          if (disposed) return;

          setConnectionState("reconnecting");
          setReconnectCount((count) => count + 1);
          reconnectTimerRef.current = window.setTimeout(() => {
            connectRef.current();
          }, RECONNECT_DELAY_MS);
        };

        ws.onerror = () => {
          ws.close();
        };
      } catch {
        setConnectionState("disconnected");
      }
    };

    connectRef.current();

    return () => {
      disposed = true;
      clearReconnectTimer();
      disconnectWebSocket();
      setConnectionState("disconnected");
    };
  }, [appendLogEntry, clearReconnectTimer, disconnectWebSocket]);

  useEffect(() => {
    if (!autoScroll || !containerRef.current) return;
    containerRef.current.scrollTop = containerRef.current.scrollHeight;
  }, [logs, autoScroll]);

  const targets = useMemo(() => {
    const values = Array.from(
      new Set(logs.map((log) => log.target).filter((target) => target.length > 0)),
    );
    return values.sort((a, b) => a.localeCompare(b));
  }, [logs]);

  const levelCounts = useMemo(() => {
    return logs.reduce<Record<string, number>>((acc, log) => {
      acc[log.level] = (acc[log.level] ?? 0) + 1;
      return acc;
    }, {});
  }, [logs]);

  const filteredLogs = useMemo(() => {
    const query = filter.trim().toLowerCase();

    return logs.filter((log) => {
      if (levelFilter !== "all" && log.level !== levelFilter) return false;
      if (targetFilter !== "all" && log.target !== targetFilter) return false;

      if (!query) return true;

      const haystacks = [
        log.message,
        log.target,
        log.level,
        log.timestamp,
        log.fields ? JSON.stringify(log.fields) : "",
      ];

      return haystacks.some((value) => value.toLowerCase().includes(query));
    });
  }, [filter, levelFilter, logs, targetFilter]);

  const effectiveSelectedLogIndex =
    selectedLogIndex === null
      ? null
      : selectedLogIndex >= filteredLogs.length
        ? filteredLogs.length - 1
        : selectedLogIndex;

  const selectedLog =
    effectiveSelectedLogIndex !== null &&
    effectiveSelectedLogIndex >= 0 &&
    effectiveSelectedLogIndex < filteredLogs.length
      ? filteredLogs[effectiveSelectedLogIndex]
      : null;

  const exportLogs = useCallback(() => {
    if (!filteredLogs.length) return;

    const jsonl = filteredLogs.map((entry) => JSON.stringify(entry)).join("\n");
    const blob = new Blob([jsonl], { type: "application/jsonl" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `rune-logs-${new Date().toISOString().slice(0, 19)}.jsonl`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [filteredLogs]);

  const clearLogs = useCallback(() => {
    setLogs([]);
    setSelectedLogIndex(null);
  }, []);

  const jumpToLatest = useCallback(() => {
    if (!containerRef.current) return;
    containerRef.current.scrollTop = containerRef.current.scrollHeight;
  }, []);

  const reconnect = useCallback(() => {
    setReconnectCount(0);
    connectRef.current();
  }, []);

  const connectionBadge =
    connectionState === "connected"
      ? { label: "Connected", variant: "default" as const, icon: Wifi }
      : connectionState === "reconnecting"
        ? { label: "Reconnecting", variant: "secondary" as const, icon: RefreshCw }
        : connectionState === "connecting"
          ? { label: "Connecting", variant: "secondary" as const, icon: RefreshCw }
          : { label: "Disconnected", variant: "outline" as const, icon: WifiOff };

  const ConnectionIcon = connectionBadge.icon;

  return (
    <div className="space-y-8">
      <div className="flex flex-col gap-6 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Logs</h1>
          <p className="mt-1 text-muted-foreground">Real-time gateway log stream with local filtering</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Badge variant={connectionBadge.variant} className="gap-1.5">
            <ConnectionIcon
              className={cn(
                "h-3.5 w-3.5",
                connectionState !== "connected" && connectionState !== "disconnected" &&
                  "animate-spin",
              )}
            />
            {connectionBadge.label}
          </Badge>
          {reconnectCount > 0 && (
            <Badge variant="outline" className="text-xs">
              reconnects {reconnectCount}
            </Badge>
          )}
          <Button variant="outline" size="sm" onClick={() => setPaused((value) => !value)}>
            {paused ? <Play className="mr-2 h-4 w-4" /> : <Pause className="mr-2 h-4 w-4" />}
            {paused ? "Resume" : "Pause"}
          </Button>
          <Button variant="outline" size="sm" onClick={reconnect}>
            <RefreshCw className="mr-2 h-4 w-4" />
            Reconnect
          </Button>
          <Button variant="outline" size="sm" onClick={exportLogs} disabled={!filteredLogs.length}>
            <Download className="mr-2 h-4 w-4" />
            Export
          </Button>
          <Button variant="outline" size="sm" onClick={clearLogs} disabled={!logs.length}>
            <Trash2 className="mr-2 h-4 w-4" />
            Clear
          </Button>
        </div>
      </div>

      <div className="grid gap-6 sm:grid-cols-2 xl:grid-cols-5">
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Buffered entries</div>
            <div className="mt-2 text-2xl font-semibold">{logs.length.toLocaleString()}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Filtered view</div>
            <div className="mt-2 text-2xl font-semibold">{filteredLogs.length.toLocaleString()}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Errors</div>
            <div className="mt-2 text-2xl font-semibold">{(levelCounts.ERROR ?? 0).toLocaleString()}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Warnings</div>
            <div className="mt-2 text-2xl font-semibold">{(levelCounts.WARN ?? 0).toLocaleString()}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Targets</div>
            <div className="mt-2 text-2xl font-semibold">{targets.length.toLocaleString()}</div>
          </CardContent>
        </Card>
      </div>

      <div className="flex flex-col gap-3 xl:flex-row xl:items-center">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Filter by message, target, level, timestamp, or JSON fields..."
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            className="pl-10"
          />
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Select value={levelFilter} onValueChange={setLevelFilter}>
            <SelectTrigger className="w-[150px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All levels</SelectItem>
              <SelectItem value="ERROR">Error</SelectItem>
              <SelectItem value="WARN">Warn</SelectItem>
              <SelectItem value="INFO">Info</SelectItem>
              <SelectItem value="DEBUG">Debug</SelectItem>
              <SelectItem value="TRACE">Trace</SelectItem>
            </SelectContent>
          </Select>
          <Select value={targetFilter} onValueChange={setTargetFilter}>
            <SelectTrigger className="w-[180px]">
              <SelectValue placeholder="All targets" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All targets</SelectItem>
              {targets.map((target) => (
                <SelectItem key={target} value={target}>
                  {target}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <div className="flex items-center gap-2">
            <Switch id="auto-scroll" checked={autoScroll} onCheckedChange={setAutoScroll} />
            <Label htmlFor="auto-scroll">Auto-scroll</Label>
          </div>
        </div>
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.6fr)_minmax(320px,0.9fr)]">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-4 space-y-0">
            <CardTitle className="flex items-center gap-2 text-base">
              <ScrollText className="h-4 w-4" />
              Stream
              {paused && (
                <Badge variant="secondary" className="text-xs">
                  Paused
                </Badge>
              )}
            </CardTitle>
            <Button variant="ghost" size="sm" onClick={jumpToLatest} disabled={!filteredLogs.length}>
              <ArrowDown className="mr-2 h-4 w-4" />
              Latest
            </Button>
          </CardHeader>
          <CardContent>
            <div
              ref={containerRef}
              className="max-h-[68vh] overflow-y-auto rounded-md border bg-muted/20 p-2 font-mono text-xs"
            >
              {filteredLogs.length === 0 ? (
                <p className="p-4 text-center text-muted-foreground">
                  {logs.length === 0
                    ? "Waiting for log entries on /ws/logs..."
                    : "No logs match the current filter"}
                </p>
              ) : (
                filteredLogs.map((log, index) => {
                  const isSelected = index === effectiveSelectedLogIndex;
                  return (
                    <button
                      key={`${log.timestamp}-${index}`}
                      type="button"
                      onClick={() => setSelectedLogIndex(index)}
                      className={cn(
                        "flex w-full gap-2 rounded-md border border-transparent px-2 py-1 text-left transition-colors hover:bg-muted/40",
                        isSelected && "border-primary/40 bg-primary/5",
                      )}
                    >
                      <span className="shrink-0 text-muted-foreground">
                        {new Date(log.timestamp).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                          second: "2-digit",
                        })}
                      </span>
                      <span
                        className={cn(
                          "shrink-0 rounded px-1 font-semibold",
                          levelColors[log.level] ?? "text-foreground",
                        )}
                      >
                        {log.level.padEnd(5)}
                      </span>
                      <span className="shrink-0 text-muted-foreground">{log.target}</span>
                      <span className="line-clamp-2 flex-1 break-all">{log.message || ""}</span>
                    </button>
                  );
                })
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Info className="h-4 w-4" />
              Entry details
            </CardTitle>
          </CardHeader>
          <CardContent>
            {!selectedLog ? (
              <p className="text-sm text-muted-foreground">
                Select a log entry to inspect its full payload.
              </p>
            ) : (
              <div className="space-y-4 text-sm">
                <div className="grid gap-3 sm:grid-cols-2">
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Timestamp</div>
                    <div className="mt-1 font-mono text-xs">{formatTimestamp(selectedLog.timestamp)}</div>
                  </div>
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Target</div>
                    <div className="mt-1 font-mono text-xs">{selectedLog.target}</div>
                  </div>
                </div>
                <div>
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Level</div>
                  <div className="mt-1">
                    <Badge variant="outline" className={cn(levelColors[selectedLog.level])}>
                      {selectedLog.level}
                    </Badge>
                  </div>
                </div>
                <div>
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Message</div>
                  <pre className="mt-1 whitespace-pre-wrap rounded-md bg-muted p-3 font-mono text-xs">
                    {selectedLog.message}
                  </pre>
                </div>
                <div>
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Fields</div>
                  <pre className="mt-1 max-h-[320px] overflow-auto rounded-md bg-muted p-3 font-mono text-xs">
                    {JSON.stringify(selectedLog.fields ?? {}, null, 2)}
                  </pre>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
