import { useState, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import {
  Plus,
  Search,
  Cpu,
  Hash,
  CircleDot,
  Sparkles,
  ChevronRight,
  Wrench,
  Clock3,
  Copy,
  PanelRightClose,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { CopyMarkdown } from "./CopyMarkdown";
import { buildToolInspectorData } from "./tool-inspector";
import type { ChatSessionListItem } from "@/hooks/use-chat";
import type { TranscriptEntry } from "@/lib/api-types";

interface ChatSidebarProps {
  sessions?: ChatSessionListItem[];
  isLoading?: boolean;
  activeSessionId?: string;
  onSelectSession?: (id: string) => void;
  onCreateSession?: () => void;
  isCreating?: boolean;
  className?: string;
  mode?: "sessions" | "inspector";
  selectedToolEntry?: TranscriptEntry | null;
  selectedToolPair?: TranscriptEntry | null;
  onCloseInspector?: () => void;
  compactHeader?: boolean;
}

function statusBadgeVariant(
  status: string,
): "default" | "secondary" | "outline" | "destructive" {
  switch (status) {
    case "active":
      return "default";
    case "idle":
      return "secondary";
    case "error":
      return "destructive";
    default:
      return "outline";
  }
}

function formatRelativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = Date.now();
  const diffMs = now - date.getTime();
  const diffMin = Math.floor(diffMs / 60_000);

  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 7) return `${diffDay}d ago`;
  return date.toLocaleDateString();
}

function ToolInspectorPanel({
  selectedToolEntry,
  selectedToolPair,
  onCloseInspector,
}: Pick<
  ChatSidebarProps,
  "selectedToolEntry" | "selectedToolPair" | "onCloseInspector"
>) {
  if (!selectedToolEntry) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-6 py-10 text-center">
        <div className="rounded-2xl border border-dashed border-border/70 bg-background/70 p-4 text-muted-foreground">
          <Wrench className="h-8 w-8" />
        </div>
        <div>
          <p className="text-sm font-medium text-foreground">No tool output selected</p>
          <p className="mt-1 text-xs text-muted-foreground">
            Click any tool card in the transcript to pin its arguments and result here.
          </p>
        </div>
      </div>
    );
  }

  const details = buildToolInspectorData(selectedToolEntry, selectedToolPair ?? undefined);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="border-b border-border/70 bg-gradient-to-br from-background via-background to-primary/5 px-4 py-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <Badge variant="outline" className="border-primary/25 bg-primary/10 text-primary">
              <PanelRightClose className="mr-1 h-3 w-3" />
              Inspector
            </Badge>
            <h2 className="mt-3 text-sm font-semibold font-mono">{details.toolName}</h2>
            <p className="mt-1 text-xs text-muted-foreground">
              Full request + result payload without expanding cards inline.
            </p>
          </div>
          {onCloseInspector ? (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onCloseInspector}
              aria-label="Close inspector"
              className="rounded-xl"
            >
              <PanelRightClose className="h-4 w-4" />
            </Button>
          ) : null}
        </div>

        <div className="mt-4 grid grid-cols-2 gap-2">
          <div className="rounded-2xl border border-border/70 bg-background/80 px-3 py-2">
            <p className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Call id</p>
            <p className="mt-1 truncate font-mono text-[11px]">
              {details.callId ?? "Unavailable"}
            </p>
          </div>
          <div className="rounded-2xl border border-border/70 bg-background/80 px-3 py-2">
            <p className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Duration</p>
            <p className="mt-1 text-[11px] font-medium">
              {details.durationMs !== null ? `${details.durationMs} ms` : "Unavailable"}
            </p>
          </div>
        </div>
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
        <section className="space-y-2 rounded-2xl border border-border/70 bg-background/75 p-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
              <Clock3 className="h-3.5 w-3.5" />
              Selected event
            </div>
            <CopyMarkdown content={JSON.stringify(selectedToolEntry.payload, null, 2)} />
          </div>
          <p className="text-xs text-muted-foreground">
            {new Date(selectedToolEntry.created_at).toLocaleString()}
          </p>
          <pre className="overflow-x-auto rounded-xl bg-muted/40 p-3 font-mono text-xs leading-relaxed">
            {JSON.stringify(selectedToolEntry.payload, null, 2)}
          </pre>
        </section>

        {details.args && (
          <section className="space-y-2 rounded-2xl border border-border/70 bg-background/75 p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
                <Hash className="h-3.5 w-3.5" />
                Tool arguments
              </div>
              <CopyMarkdown content={details.args} />
            </div>
            <pre className="overflow-x-auto rounded-xl bg-muted/40 p-3 font-mono text-xs leading-relaxed">
              {details.args}
            </pre>
          </section>
        )}

        {selectedToolPair && (
          <section className="space-y-2 rounded-2xl border border-border/70 bg-background/75 p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
                <Copy className="h-3.5 w-3.5" />
                Paired event
              </div>
              <CopyMarkdown content={JSON.stringify(selectedToolPair.payload, null, 2)} />
            </div>
            <pre className="overflow-x-auto rounded-xl bg-muted/40 p-3 font-mono text-xs leading-relaxed">
              {JSON.stringify(selectedToolPair.payload, null, 2)}
            </pre>
          </section>
        )}

        {details.result && (
          <section className="space-y-2 rounded-2xl border border-border/70 bg-background/75 p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
                <Cpu className="h-3.5 w-3.5" />
                Result
              </div>
              <CopyMarkdown content={details.result} />
            </div>
            <pre className="max-h-[22rem] overflow-auto rounded-xl bg-muted/40 p-3 font-mono text-xs leading-relaxed">
              {details.result}
            </pre>
          </section>
        )}

        {details.error && (
          <section className="space-y-2 rounded-2xl border border-red-500/25 bg-red-500/5 p-3">
            <div className="flex items-center justify-between gap-2 text-xs font-medium uppercase tracking-[0.16em] text-red-600 dark:text-red-400">
              <span>Tool error</span>
              <CopyMarkdown content={details.error} />
            </div>
            <pre className="max-h-[16rem] overflow-auto rounded-xl bg-red-500/10 p-3 font-mono text-xs leading-relaxed text-red-700 dark:text-red-300">
              {details.error}
            </pre>
          </section>
        )}
      </div>
    </div>
  );
}

export function ChatSidebar({
  sessions,
  isLoading = false,
  activeSessionId,
  onSelectSession,
  onCreateSession,
  isCreating,
  className,
  mode = "sessions",
  selectedToolEntry,
  selectedToolPair,
  onCloseInspector,
  compactHeader = false,
}: ChatSidebarProps) {
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!sessions) return [];
    if (!search.trim()) return sessions;
    const q = search.toLowerCase();
    return sessions.filter((s) => {
      const preview = s.preview ?? "";
      return (
        s.id.toLowerCase().includes(q) ||
        s.status.toLowerCase().includes(q) ||
        preview.toLowerCase().includes(q) ||
        (s.channel && s.channel.toLowerCase().includes(q)) ||
        (s.latest_model && s.latest_model.toLowerCase().includes(q))
      );
    });
  }, [sessions, search]);

  const summary = useMemo(() => {
    const all = sessions ?? [];
    return {
      total: all.length,
      active: all.filter((session) => session.status === "active").length,
      withModels: all.filter((session) => Boolean(session.latest_model)).length,
    };
  }, [sessions]);

  const activeSession = useMemo(
    () => sessions?.find((session) => session.id === activeSessionId),
    [activeSessionId, sessions],
  );

  if (mode === "inspector") {
    return (
      <div
        className={cn(
          "flex h-full min-h-0 flex-col overflow-hidden rounded-3xl border border-border/70 bg-card/80 shadow-[0_18px_50px_rgba(15,23,42,0.08)] backdrop-blur",
          className,
        )}
      >
        <ToolInspectorPanel
          selectedToolEntry={selectedToolEntry}
          selectedToolPair={selectedToolPair}
          onCloseInspector={onCloseInspector}
        />
      </div>
    );
  }

  return (
    <div
      className={cn(
        "flex h-full min-h-0 flex-col overflow-hidden rounded-3xl border border-border/70 bg-card/80 shadow-[0_18px_50px_rgba(15,23,42,0.08)] backdrop-blur",
        className,
      )}
    >
      <div
        className={cn(
          "border-b border-border/70 bg-gradient-to-br from-background via-background to-primary/5 px-4 py-4",
          compactHeader && "px-3 py-3",
        )}
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <Badge variant="outline" className="border-primary/25 bg-primary/10 text-primary">
                <Sparkles className="mr-1 h-3 w-3" />
                Queue
              </Badge>
            </div>
            <h2 className="mt-3 text-sm font-semibold">
              {compactHeader ? "Switch sessions" : "Session control"}
            </h2>
            <p className="mt-1 text-xs text-muted-foreground">
              {compactHeader
                ? "Keep the active thread in view and jump without losing place."
                : "Jump between live work, inspect model context, and open a fresh thread fast."}
            </p>
          </div>
          <Button
            variant="default"
            size="icon-sm"
            onClick={onCreateSession}
            disabled={isCreating}
            aria-label="New session"
            className="rounded-xl"
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>

        {compactHeader ? (
          <div className="mt-4 flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
            <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1">
              {summary.total} sessions
            </span>
            <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1">
              {summary.active} active
            </span>
            <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1">
              {summary.withModels} models
            </span>
          </div>
        ) : (
          <div className="mt-4 grid grid-cols-3 gap-2">
            <div className="rounded-2xl border border-border/70 bg-background/80 px-3 py-2">
              <p className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Total</p>
              <p className="mt-1 text-sm font-semibold">{summary.total}</p>
            </div>
            <div className="rounded-2xl border border-border/70 bg-background/80 px-3 py-2">
              <p className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Active</p>
              <p className="mt-1 text-sm font-semibold">{summary.active}</p>
            </div>
            <div className="rounded-2xl border border-border/70 bg-background/80 px-3 py-2">
              <p className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Models</p>
              <p className="mt-1 text-sm font-semibold">{summary.withModels}</p>
            </div>
          </div>
        )}
      </div>

      <div className={cn("px-4 py-3", compactHeader && "px-3 py-2.5")}>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter by id, channel, model..."
            className="h-10 rounded-xl border-border/70 bg-background/80 pl-9 text-xs"
          />
        </div>
      </div>

      {compactHeader && activeSession && (
        <div className="px-3 pb-2">
          <div className="rounded-2xl border border-primary/20 bg-primary/5 px-3 py-2.5">
            <div className="flex items-center justify-between gap-2">
              <p className="text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">
                Current session
              </p>
              <Badge variant={statusBadgeVariant(activeSession.status)} className="text-[10px]">
                {activeSession.status}
              </Badge>
            </div>
            <code className="mt-1 block truncate text-xs font-medium text-foreground">
              {activeSession.id}
            </code>
            {activeSession.preview && (
              <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">
                {activeSession.preview}
              </p>
            )}
          </div>
        </div>
      )}

      <Separator />

      <div className={cn("flex-1 overflow-y-auto px-2 pb-2", compactHeader && "px-1.5 pb-1.5")}>
        {isLoading ? (
          <div className="space-y-2 p-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <Skeleton key={i} className="h-24 w-full rounded-2xl" />
            ))}
          </div>
        ) : filtered.length === 0 ? (
          <div className="p-5 text-center text-xs text-muted-foreground">
            {search ? "No matching sessions" : "No sessions yet"}
          </div>
        ) : (
          <div className="space-y-2 pt-2">
            {filtered.map((session) => {
              const isSessionActive = session.id === activeSessionId;
              return (
                <button
                  key={session.id}
                  onClick={() => onSelectSession?.(session.id)}
                  className={cn(
                    "group flex w-full flex-col gap-2 rounded-2xl border px-3 py-3 text-left transition-all",
                    compactHeader && "gap-1.5 px-3 py-2.5",
                    isSessionActive
                      ? "border-primary/40 bg-primary/10 text-foreground shadow-[0_12px_30px_rgba(249,115,22,0.12)]"
                      : "border-border/70 bg-background/70 hover:border-primary/25 hover:bg-primary/5",
                  )}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <CircleDot
                          className={cn(
                            "h-3 w-3 shrink-0",
                            session.status === "active"
                              ? "text-green-500"
                              : session.status === "error"
                                ? "text-red-500"
                                : "text-muted-foreground",
                          )}
                        />
                        <code className="block truncate text-xs font-medium">{session.id}</code>
                      </div>
                      <p className="mt-1 text-[11px] text-muted-foreground">
                        {formatRelativeTime(
                          session.last_activity_at ?? session.updated_at ?? session.created_at,
                        )}
                      </p>
                      <p
                        className={cn(
                          "mt-2 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground/90",
                          compactHeader && "mt-1.5 line-clamp-3",
                        )}
                      >
                        {session.preview}
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <Badge
                        variant={statusBadgeVariant(session.status)}
                        className="shrink-0 text-[10px]"
                      >
                        {session.status}
                      </Badge>
                      <ChevronRight
                        className={cn(
                          "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                          isSessionActive ? "translate-x-0.5 text-primary" : "group-hover:translate-x-0.5",
                        )}
                      />
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground">
                    {session.channel && (
                      <span className="inline-flex items-center gap-1 rounded-full border border-border/70 bg-background/80 px-2 py-1">
                        <Hash className="h-3 w-3" />
                        {session.channel}
                      </span>
                    )}
                    {session.latest_model && (
                      <span className="inline-flex max-w-full items-center gap-1 rounded-full border border-border/70 bg-background/80 px-2 py-1">
                        <Cpu className="h-3 w-3" />
                        <span className="truncate">{session.latest_model}</span>
                      </span>
                    )}
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
