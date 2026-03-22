import { createFileRoute, Link } from "@tanstack/react-router";
import { useState, useRef, useEffect } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { useSession, useSessionStatus, useSessionTranscript, useSendMessage } from "@/hooks/use-sessions";
import { useSessionEvents } from "@/lib/websocket";
import { cn } from "@/lib/utils";
import {
  MessageSquare,
  Send,
  Clock,
  Cpu,
  Hash,
  Zap,
  Wifi,
  WifiOff,
} from "lucide-react";

export const Route = createFileRoute("/_admin/sessions/$id")({
  component: SessionDetailPage,
});

function SessionDetailPage() {
  const { id } = Route.useParams();
  const [message, setMessage] = useState("");
  const transcriptEndRef = useRef<HTMLDivElement>(null);

  const { data: session, isLoading: sessionLoading } = useSession(id);
  const { data: status } = useSessionStatus(id);
  const { data: transcript, isLoading: transcriptLoading } = useSessionTranscript(id);
  const sendMessage = useSendMessage(id);
  const { events, connected } = useSessionEvents(id);

  useEffect(() => {
    transcriptEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [transcript, events]);

  const handleSend = () => {
    if (!message.trim()) return;
    sendMessage.mutate(message.trim(), {
      onSuccess: () => setMessage(""),
    });
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Extract human-readable content from transcript entry payloads
  const renderPayload = (entry: { kind: string; payload: unknown }) => {
    const p = entry.payload as Record<string, unknown> | null;
    if (!p) return "";
    if (typeof p === "string") return p;

    // User message
    if (p.content && typeof p.content === "string") return p.content;
    if (p.message && typeof p.message === "object") {
      const msg = p.message as Record<string, unknown>;
      if (msg.content && typeof msg.content === "string") return msg.content;
    }

    // Tool request — show name + truncated args
    if (p.tool_name && typeof p.tool_name === "string") {
      const args = typeof p.arguments === "string"
        ? p.arguments
        : JSON.stringify(p.arguments ?? {});
      const truncated = args.length > 200 ? args.slice(0, 200) + "..." : args;
      return `${p.tool_name}(${truncated})`;
    }

    // Tool result — show output truncated
    if (p.output && typeof p.output === "string") {
      return p.output.length > 500 ? p.output.slice(0, 500) + "\n..." : p.output;
    }

    // Approval
    if (p.summary && typeof p.summary === "string") return p.summary;

    // Fallback — compact JSON
    const json = JSON.stringify(p, null, 2);
    return json.length > 300 ? json.slice(0, 300) + "\n..." : json;
  };

  return (
    <div className="space-y-8">
      <div className="flex items-center gap-3">
        <h1 className="text-3xl font-bold tracking-tight">Session</h1>
        <code className="rounded bg-muted px-2 py-1 text-sm">{id ? `${id.slice(0, 12)}...` : "unknown"}</code>
        <Link
          to="/chat"
          search={{ session: id }}
          className="rounded-md border border-primary/20 bg-primary/5 px-2.5 py-1 text-xs font-medium text-primary transition-colors hover:bg-primary/10"
        >
          Open in chat console
        </Link>
        <div className="ml-auto flex items-center gap-1 text-xs">
          {connected ? (
            <>
              <Wifi className="h-3 w-3 text-green-500" />
              <span className="text-green-600">Live</span>
            </>
          ) : (
            <>
              <WifiOff className="h-3 w-3 text-muted-foreground" />
              <span className="text-muted-foreground">Disconnected</span>
            </>
          )}
        </div>
      </div>

      {/* Session info header */}
      {sessionLoading ? (
        <Skeleton className="h-24 w-full" />
      ) : session ? (
        <Card>
          <CardContent className="flex flex-wrap gap-x-6 gap-y-2 pt-6 text-sm">
            <div className="flex items-center gap-1.5">
              <Hash className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Kind:</span>
              <Badge variant="outline">{session.kind}</Badge>
            </div>
            <div className="flex items-center gap-1.5">
              <Zap className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Status:</span>
              <Badge variant={session.status === "active" ? "default" : "secondary"}>
                {session.status}
              </Badge>
            </div>
            <div className="flex items-center gap-1.5">
              <MessageSquare className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Turns:</span>
              <span>{session.turn_count}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Cpu className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Model:</span>
              <span className="font-mono text-xs">{session.latest_model ?? "—"}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Clock className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Created:</span>
              <span className="text-xs">
                {session.created_at && !Number.isNaN(new Date(session.created_at).getTime())
                  ? new Date(session.created_at).toLocaleString()
                  : "—"}
              </span>
            </div>
            {status && (
              <>
                <div className="flex items-center gap-1.5">
                  <span className="text-muted-foreground">Tokens:</span>
                  <span className="font-mono text-xs">
                    {status.prompt_tokens}p / {status.completion_tokens}c
                  </span>
                </div>
                {status.estimated_cost && (
                  <div className="flex items-center gap-1.5">
                    <span className="text-muted-foreground">Cost:</span>
                    <span className="font-mono text-xs">{status.estimated_cost}</span>
                  </div>
                )}
              </>
            )}
          </CardContent>
        </Card>
      ) : null}

      {/* Transcript */}
      <Card className="flex flex-col">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <MessageSquare className="h-4 w-4" />
            Transcript
          </CardTitle>
        </CardHeader>
        <CardContent className="flex-1">
          <div className="h-[60vh] overflow-y-auto rounded-md border bg-muted/20 p-4">
            {transcriptLoading ? (
              <div className="space-y-3">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-16 w-full" />
                ))}
              </div>
            ) : !transcript?.length ? (
              <p className="text-center text-sm text-muted-foreground">
                No transcript entries yet
              </p>
            ) : (
              <div className="space-y-3">
                {transcript.map((entry) => (
                  <div
                    key={entry.id}
                    className={cn(
                      "rounded-lg p-3 text-sm",
                      entry.kind === "user"
                        ? "ml-8 bg-primary/10"
                        : entry.kind === "assistant"
                          ? "mr-8 bg-card border"
                          : "border border-dashed bg-muted/30 text-xs"
                    )}
                  >
                    <div className="mb-1 flex items-center gap-2">
                      <Badge variant="outline" className="text-xs">
                        {entry.kind}
                      </Badge>
                      <span className="text-xs text-muted-foreground">
                        {entry.created_at && !Number.isNaN(new Date(entry.created_at).getTime())
                          ? new Date(entry.created_at).toLocaleTimeString()
                          : "—"}
                      </span>
                    </div>
                    <pre className="whitespace-pre-wrap font-sans">
                      {renderPayload(entry)}
                    </pre>
                  </div>
                ))}
                {events.map((evt, i) => (
                  <div
                    key={`ws-${i}`}
                    className="rounded-lg border border-primary/20 bg-primary/5 p-3 text-sm"
                  >
                    <div className="mb-1 flex items-center gap-2">
                      <Badge className="text-xs">live: {evt.kind}</Badge>
                    </div>
                    <pre className="whitespace-pre-wrap font-sans text-xs">
                      {JSON.stringify(evt.payload, null, 2)}
                    </pre>
                  </div>
                ))}
                <div ref={transcriptEndRef} />
              </div>
            )}
          </div>

          <Separator className="my-4" />

          {/* Message input */}
          <div className="flex gap-3">
            <Textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Send a message..."
              className="min-h-[60px] flex-1 resize-none"
            />
            <Button
              onClick={handleSend}
              disabled={!message.trim() || sendMessage.isPending}
              size="icon"
              className="h-[60px] w-[60px]"
            >
              <Send className="h-4 w-4" />
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
