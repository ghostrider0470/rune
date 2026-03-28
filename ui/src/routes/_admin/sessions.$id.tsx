import { createFileRoute, Link } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import { formatDistanceToNow } from "date-fns";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ChatMessage } from "@/components/chat/ChatMessage";
import { ToolCard } from "@/components/chat/ToolCard";
import { InlineApprovalActions } from "@/components/chat/InlineApprovalActions";
import { MarkdownRenderer } from "@/components/chat/MarkdownRenderer";
import {
  useSession,
  useSessionStatus,
  useSessionTranscript,
  useSendMessage,
  type SessionTranscriptFilters,
} from "@/hooks/use-sessions";
import { useSessionEvents } from "@/lib/websocket";
import { getPayloadText, normalizeTranscriptKind } from "@/components/chat/chat-utils";
import type { SessionEvent, TranscriptEntry } from "@/lib/api-types";
import {
  ArrowDown,
  Clock,
  Cpu,
  Filter,
  Hash,
  LoaderCircle,
  MessageSquare,
  Radio,
  Send,
  Wifi,
  WifiOff,
  Wrench,
  Zap,
} from "lucide-react";

export const Route = createFileRoute("/_admin/sessions/$id")({
  component: SessionDetailPage,
});

type TranscriptKindFilter = "all" | "messages" | "tools" | "approvals" | "system";

type SortDirection = "newest" | "oldest";

interface ToolPair {
  request?: TranscriptEntry;
  result?: TranscriptEntry;
}

function isObjectRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function formatNumber(value: number | null | undefined): string {
  if (typeof value !== "number" || Number.isNaN(value)) return "—";
  return new Intl.NumberFormat().format(value);
}

function formatDateTime(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString();
}

function relativeDate(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return formatDistanceToNow(date, { addSuffix: true });
}

function eventToTranscriptEntry(event: SessionEvent, index: number): TranscriptEntry {
  return {
    id: `ws-${event.session_id}-${index}`,
    turn_id: null,
    seq: Number.MAX_SAFE_INTEGER - index,
    kind: event.kind,
    payload: event.payload,
    created_at: new Date().toISOString(),
  };
}

function getEventText(event: SessionEvent): string {
  const payload = event.payload;
  if (typeof payload === "string") return payload;
  if (isObjectRecord(payload)) {
    const candidates = [payload.delta, payload.content, payload.text, payload.message, payload.output];
    const directText = candidates.find((value) => typeof value === "string");
    if (typeof directText === "string") return directText;
  }
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return String(payload ?? "");
  }
}

function getLiveAssistantText(events: SessionEvent[]): string {
  return events
    .filter((event) => {
      const kind = event.kind.toLowerCase();
      return kind.includes("assistant") || kind.includes("token") || kind.includes("delta") || kind.includes("stream");
    })
    .map(getEventText)
    .filter((text) => text.trim().length > 0)
    .join("");
}

function toolCallIdFromPayload(payload: unknown): string | null {
  if (!isObjectRecord(payload)) return null;
  const direct = payload.tool_call_id;
  if (typeof direct === "string" && direct.length > 0) return direct;
  return null;
}

function buildToolPairMap(entries: TranscriptEntry[]): Map<string, ToolPair> {
  const pairs = new Map<string, ToolPair>();

  for (const entry of entries) {
    const normalizedKind = normalizeTranscriptKind(entry.kind);
    if (!["tool_request", "tool_use", "tool_result"].includes(normalizedKind)) continue;

    const key = toolCallIdFromPayload(entry.payload) ?? `${entry.turn_id ?? "no-turn"}:${normalizedKind}:${entry.seq}`;
    const pair = pairs.get(key) ?? {};

    if (normalizedKind === "tool_result") {
      pair.result = entry;
    } else {
      pair.request = entry;
    }

    pairs.set(key, pair);
  }

  return pairs;
}

function entryMatchesKindFilter(entry: TranscriptEntry, filter: TranscriptKindFilter): boolean {
  if (filter === "all") return true;
  const kind = normalizeTranscriptKind(entry.kind);

  switch (filter) {
    case "messages":
      return kind === "user" || kind === "assistant";
    case "tools":
      return kind === "tool_request" || kind === "tool_use" || kind === "tool_result";
    case "approvals":
      return kind.includes("approval");
    case "system":
      return !(kind === "user" || kind === "assistant" || kind.startsWith("tool_") || kind.includes("approval"));
    default:
      return true;
  }
}

function entryMatchesQuery(entry: TranscriptEntry, query: string): boolean {
  if (!query) return true;
  const haystack = [entry.kind, entry.turn_id ?? "", getPayloadText(entry.payload)]
    .join("\n")
    .toLowerCase();
  return haystack.includes(query);
}

function renderSystemEntry(entry: TranscriptEntry, isLive = false) {
  const payloadText = getPayloadText(entry.payload);

  return (
    <div className="rounded-2xl border border-dashed bg-muted/30 p-4 text-sm">
      <div className="mb-2 flex items-center gap-2">
        <Badge variant="outline">{normalizeTranscriptKind(entry.kind)}</Badge>
        <span className="text-[11px] text-muted-foreground">{formatDateTime(entry.created_at)}</span>
        {isLive && <Badge variant="secondary">live</Badge>}
      </div>
      <pre className="whitespace-pre-wrap break-words font-sans text-xs text-muted-foreground">{payloadText}</pre>
      <InlineApprovalActions entry={entry} className="mt-3" />
    </div>
  );
}

function SessionDetailPage() {
  const { id } = Route.useParams();
  const [message, setMessage] = useState("");
  const [query, setQuery] = useState("");
  const [kindFilter, setKindFilter] = useState<TranscriptKindFilter>("all");
  const [sortDirection, setSortDirection] = useState<SortDirection>("oldest");
  const [limit, setLimit] = useState(200);
  const transcriptEndRef = useRef<HTMLDivElement>(null);

  const transcriptFilters = useMemo<SessionTranscriptFilters>(() => ({ limit }), [limit]);

  const { data: session, isLoading: sessionLoading } = useSession(id);
  const { data: status } = useSessionStatus(id);
  const { data: transcript, isLoading: transcriptLoading, isFetching: transcriptFetching } = useSessionTranscript(id, transcriptFilters);
  const sendMessage = useSendMessage(id);
  const { events, connected } = useSessionEvents(id);

  const normalizedQuery = query.trim().toLowerCase();
  const liveAssistantText = useMemo(() => getLiveAssistantText(events), [events]);

  const mergedEntries = useMemo(() => {
    const baseEntries = transcript ?? [];
    const liveEntries = events.map((event, index) => eventToTranscriptEntry(event, index));
    const combined = [...baseEntries, ...liveEntries];
    combined.sort((a, b) => {
      if (sortDirection === "newest") {
        return b.seq - a.seq || new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
      }
      return a.seq - b.seq || new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
    });
    return combined;
  }, [events, sortDirection, transcript]);

  const filteredEntries = useMemo(
    () => mergedEntries.filter((entry) => entryMatchesKindFilter(entry, kindFilter) && entryMatchesQuery(entry, normalizedQuery)),
    [kindFilter, mergedEntries, normalizedQuery],
  );

  const toolPairs = useMemo(() => buildToolPairMap(transcript ?? []), [transcript]);

  const stats = useMemo(() => {
    const source = transcript ?? [];
    return {
      messages: source.filter((entry) => {
        const kind = normalizeTranscriptKind(entry.kind);
        return kind === "user" || kind === "assistant";
      }).length,
      tools: source.filter((entry) => normalizeTranscriptKind(entry.kind).startsWith("tool_")).length,
      approvals: source.filter((entry) => normalizeTranscriptKind(entry.kind).includes("approval")).length,
      total: source.length,
    };
  }, [transcript]);

  useEffect(() => {
    transcriptEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [filteredEntries.length, liveAssistantText]);

  const handleSend = () => {
    if (!message.trim()) return;
    sendMessage.mutate(message.trim(), {
      onSuccess: () => setMessage(""),
    });
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="space-y-8">
      <div className="flex flex-wrap items-center gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <h1 className="text-3xl font-bold tracking-tight">Session live view</h1>
            <Badge variant="outline">#{id.slice(0, 12)}</Badge>
          </div>
          <p className="text-sm text-muted-foreground">Streaming transcript, tool activity, approvals, and quick reply controls.</p>
        </div>
        <div className="ml-auto flex items-center gap-2 text-xs">
          <Button asChild variant="outline" size="sm">
            <Link to="/chat" search={{ session: id }}>Open in chat console</Link>
          </Button>
          {connected ? (
            <Badge className="gap-1.5"><Wifi className="h-3 w-3" /> Live websocket</Badge>
          ) : (
            <Badge variant="secondary" className="gap-1.5"><WifiOff className="h-3 w-3" /> Offline</Badge>
          )}
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-4">
          {sessionLoading ? (
            <Skeleton className="h-36 w-full" />
          ) : session ? (
            <Card>
              <CardContent className="grid gap-3 pt-6 sm:grid-cols-2 xl:grid-cols-4">
                <div className="rounded-2xl border bg-muted/20 p-4">
                  <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground"><Hash className="h-3.5 w-3.5" /> Session type</div>
                  <div className="flex items-center gap-2 text-sm font-medium">
                    <Badge variant="outline">{session.kind}</Badge>
                    <Badge variant={session.status === "active" ? "default" : "secondary"}>{session.status}</Badge>
                  </div>
                </div>
                <div className="rounded-2xl border bg-muted/20 p-4">
                  <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground"><MessageSquare className="h-3.5 w-3.5" /> Transcript entries</div>
                  <div className="text-2xl font-semibold">{formatNumber(stats.total)}</div>
                  <div className="text-xs text-muted-foreground">{formatNumber(stats.messages)} messages · {formatNumber(stats.tools)} tool events</div>
                </div>
                <div className="rounded-2xl border bg-muted/20 p-4">
                  <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground"><Cpu className="h-3.5 w-3.5" /> Model + usage</div>
                  <div className="truncate text-sm font-medium">{session.latest_model ?? status?.current_model ?? "—"}</div>
                  <div className="text-xs text-muted-foreground">{formatNumber(status?.total_tokens ?? session.usage_prompt_tokens + session.usage_completion_tokens)} total tokens</div>
                </div>
                <div className="rounded-2xl border bg-muted/20 p-4">
                  <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground"><Clock className="h-3.5 w-3.5" /> Activity</div>
                  <div className="text-sm font-medium">{relativeDate(session.updated_at ?? session.created_at)}</div>
                  <div className="text-xs text-muted-foreground">Started {formatDateTime(session.created_at)}</div>
                </div>
              </CardContent>
            </Card>
          ) : null}

          <Card>
            <CardHeader className="space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <CardTitle className="flex items-center gap-2 text-base">
                  <Radio className="h-4 w-4" />
                  Transcript stream
                </CardTitle>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  {transcriptFetching && <LoaderCircle className="h-3.5 w-3.5 animate-spin" />}
                  <span>{filteredEntries.length} shown</span>
                </div>
              </div>

              <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_180px_160px_120px]">
                <div className="relative">
                  <Filter className="pointer-events-none absolute left-3 top-3 h-4 w-4 text-muted-foreground" />
                  <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search transcript, tools, args, output..." className="pl-9" />
                </div>
                <Select value={kindFilter} onValueChange={(value) => setKindFilter(value as TranscriptKindFilter)}>
                  <SelectTrigger>
                    <SelectValue placeholder="Entry type" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">All entries</SelectItem>
                    <SelectItem value="messages">Messages</SelectItem>
                    <SelectItem value="tools">Tool calls</SelectItem>
                    <SelectItem value="approvals">Approvals</SelectItem>
                    <SelectItem value="system">System</SelectItem>
                  </SelectContent>
                </Select>
                <Select value={sortDirection} onValueChange={(value) => setSortDirection(value as SortDirection)}>
                  <SelectTrigger>
                    <SelectValue placeholder="Sort" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="oldest">Oldest first</SelectItem>
                    <SelectItem value="newest">Newest first</SelectItem>
                  </SelectContent>
                </Select>
                <Select value={String(limit)} onValueChange={(value) => setLimit(Number(value))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Limit" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="100">100</SelectItem>
                    <SelectItem value="200">200</SelectItem>
                    <SelectItem value="500">500</SelectItem>
                    <SelectItem value="1000">1000</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </CardHeader>

            <CardContent>
              <div className="h-[65vh] overflow-y-auto rounded-2xl border bg-muted/15 p-4">
                {transcriptLoading ? (
                  <div className="space-y-3">
                    {Array.from({ length: 5 }).map((_, index) => (
                      <Skeleton key={index} className="h-24 w-full" />
                    ))}
                  </div>
                ) : filteredEntries.length === 0 ? (
                  <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-sm text-muted-foreground">
                    <MessageSquare className="h-8 w-8" />
                    <div>
                      <p className="font-medium text-foreground">No transcript entries match the current filter.</p>
                      <p>Adjust search, type, or increase the loaded transcript limit.</p>
                    </div>
                  </div>
                ) : (
                  <div className="space-y-4">
                    {filteredEntries.map((entry) => {
                      const normalizedKind = normalizeTranscriptKind(entry.kind);
                      const isLive = entry.id.startsWith("ws-");

                      if (normalizedKind === "user" || normalizedKind === "assistant") {
                        return <ChatMessage key={entry.id} entry={entry} isLive={isLive} />;
                      }

                      if (["tool_request", "tool_use", "tool_result"].includes(normalizedKind)) {
                        const toolCallId = toolCallIdFromPayload(entry.payload);
                        const pair = toolCallId ? toolPairs.get(toolCallId) : undefined;
                        return (
                          <ToolCard
                            key={entry.id}
                            entry={entry}
                            pairedEntry={normalizedKind === "tool_result" ? pair?.request : pair?.result}
                          />
                        );
                      }

                      if (normalizedKind.includes("approval")) {
                        return (
                          <div key={entry.id} className="rounded-2xl border bg-card/80 p-4 shadow-sm">
                            <div className="mb-2 flex items-center gap-2">
                              <Badge variant="outline">{normalizedKind}</Badge>
                              <span className="text-[11px] text-muted-foreground">{formatDateTime(entry.created_at)}</span>
                            </div>
                            <MarkdownRenderer content={`\`\`\`json\n${getPayloadText(entry.payload)}\n\`\`\``} />
                            <InlineApprovalActions entry={entry} className="mt-3" />
                          </div>
                        );
                      }

                      return <div key={entry.id}>{renderSystemEntry(entry, isLive)}</div>;
                    })}

                    {liveAssistantText.trim().length > 0 && (
                      <div className="rounded-2xl border border-primary/25 bg-primary/5 p-4 shadow-sm">
                        <div className="mb-2 flex items-center gap-2">
                          <Badge>streaming</Badge>
                          <span className="text-[11px] text-muted-foreground">Token-by-token live assistant output</span>
                        </div>
                        <MarkdownRenderer content={liveAssistantText} />
                      </div>
                    )}

                    <div ref={transcriptEndRef} />
                  </div>
                )}
              </div>

              <div className="mt-4 flex justify-end">
                <Button variant="outline" size="sm" onClick={() => transcriptEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" })}>
                  <ArrowDown className="mr-1.5 h-4 w-4" />
                  Jump to latest
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">Reply to session</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <Textarea
                value={message}
                onChange={(e) => setMessage(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Send a follow-up into this session..."
                className="min-h-[100px] resize-y"
              />
              <div className="flex items-center justify-between gap-3">
                <p className="text-xs text-muted-foreground">Enter sends immediately. Shift+Enter adds a newline.</p>
                <Button onClick={handleSend} disabled={!message.trim() || sendMessage.isPending}>
                  {sendMessage.isPending ? <LoaderCircle className="mr-2 h-4 w-4 animate-spin" /> : <Send className="mr-2 h-4 w-4" />}
                  Send message
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Session stats</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3 text-sm">
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Turn count</span>
                <span className="font-medium">{formatNumber(session?.turn_count)}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Prompt / completion</span>
                <span className="font-mono text-xs">{formatNumber(session?.usage_prompt_tokens)} / {formatNumber(session?.usage_completion_tokens)}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Last turn</span>
                <span className="text-right text-xs">{relativeDate(status?.last_turn_ended_at ?? session?.updated_at)}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Estimated cost</span>
                <span className="font-mono text-xs">{status?.estimated_cost ?? "—"}</span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base"><Wrench className="h-4 w-4" /> Tool + approval breakdown</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3 text-sm">
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Tool events</span>
                <span className="font-medium">{formatNumber(stats.tools)}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Approval events</span>
                <span className="font-medium">{formatNumber(stats.approvals)}</span>
              </div>
              <div className="rounded-xl border bg-muted/20 p-3 text-xs text-muted-foreground">
                Tool request/result cards are expandable and approvals can be decided inline from the transcript.
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base"><Zap className="h-4 w-4" /> Runtime details</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3 text-sm">
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Runtime</span>
                <span className="font-medium">{status?.runtime ?? "—"}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Reasoning</span>
                <span className="font-medium">{status?.reasoning ?? "—"}</span>
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Security / approvals</span>
                <span className="font-mono text-xs">{status?.security_mode ?? "—"} · {status?.approval_mode ?? "—"}</span>
              </div>
              {!!status?.unresolved?.length && (
                <div className="rounded-xl border border-amber-500/30 bg-amber-500/5 p-3 text-xs text-amber-800 dark:text-amber-300">
                  <p className="mb-1 font-medium">Outstanding runtime notes</p>
                  <ul className="list-disc space-y-1 pl-4">
                    {status.unresolved.map((item) => <li key={item}>{item}</li>)}
                  </ul>
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      <Separator />
    </div>
  );
}
