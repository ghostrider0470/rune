import { useState, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import {
  Plus,
  Search,
  MessageSquare,
  Cpu,
  Hash,
  CircleDot,
  Sparkles,
  ChevronRight,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { ChatSessionListItem } from "@/hooks/use-chat";

interface ChatSidebarProps {
  sessions: ChatSessionListItem[] | undefined;
  isLoading: boolean;
  activeSessionId: string | undefined;
  onSelectSession: (id: string) => void;
  onCreateSession: () => void;
  isCreating?: boolean;
  className?: string;
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

export function ChatSidebar({
  sessions,
  isLoading,
  activeSessionId,
  onSelectSession,
  onCreateSession,
  isCreating,
  className,
}: ChatSidebarProps) {
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!sessions) return [];
    if (!search.trim()) return sessions;
    const q = search.toLowerCase();
    return sessions.filter(
      (s) =>
        s.id.toLowerCase().includes(q) ||
        s.status.toLowerCase().includes(q) ||
        s.preview.toLowerCase().includes(q) ||
        (s.channel && s.channel.toLowerCase().includes(q)) ||
        (s.latest_model && s.latest_model.toLowerCase().includes(q)),
    );
  }, [sessions, search]);

  const summary = useMemo(() => {
    const all = sessions ?? [];
    return {
      total: all.length,
      active: all.filter((session) => session.status === "active").length,
      withModels: all.filter((session) => Boolean(session.latest_model)).length,
    };
  }, [sessions]);

  return (
    <div
      className={cn(
        "flex h-full min-h-0 flex-col overflow-hidden rounded-3xl border border-border/70 bg-card/80 shadow-[0_18px_50px_rgba(15,23,42,0.08)] backdrop-blur",
        className,
      )}
    >
      <div className="border-b border-border/70 bg-gradient-to-br from-background via-background to-primary/5 px-4 py-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <Badge variant="outline" className="border-primary/25 bg-primary/10 text-primary">
                <Sparkles className="mr-1 h-3 w-3" />
                Queue
              </Badge>
            </div>
            <h2 className="mt-3 text-sm font-semibold">Session control</h2>
            <p className="mt-1 text-xs text-muted-foreground">
              Jump between live work, inspect model context, and open a fresh thread fast.
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
      </div>

      <div className="px-4 py-3">
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

      <Separator />

      <div className="flex-1 overflow-y-auto px-2 pb-2">
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
              const isActive = session.id === activeSessionId;
              return (
                <button
                  key={session.id}
                  onClick={() => onSelectSession(session.id)}
                  className={cn(
                    "group flex w-full flex-col gap-2 rounded-2xl border px-3 py-3 text-left transition-all",
                    isActive
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
                        <code className="block truncate text-xs font-medium">
                          {session.id}
                        </code>
                      </div>
                      <p className="mt-1 text-[11px] text-muted-foreground">
                        {formatRelativeTime(
                          session.last_activity_at ?? session.updated_at ?? session.created_at,
                        )}
                      </p>
                      <p className="mt-2 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground/90">
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
                          isActive ? "translate-x-0.5 text-primary" : "group-hover:translate-x-0.5",
                        )}
                      />
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-1.5 text-[10px] text-muted-foreground">
                    {session.channel && (
                      <span className="inline-flex items-center gap-1 rounded-full border border-border/70 bg-background/70 px-2 py-1">
                        <Hash className="h-2.5 w-2.5" />
                        {session.channel}
                      </span>
                    )}
                    <span className="inline-flex items-center gap-1 rounded-full border border-border/70 bg-background/70 px-2 py-1">
                      <MessageSquare className="h-2.5 w-2.5" />
                      {session.turn_count} turns
                    </span>
                    {session.latest_model && (
                      <span className="inline-flex max-w-full items-center gap-1 rounded-full border border-border/70 bg-background/70 px-2 py-1">
                        <Cpu className="h-2.5 w-2.5 shrink-0" />
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
