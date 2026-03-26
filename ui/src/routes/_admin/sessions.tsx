import { createFileRoute, Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { formatDistanceToNow } from "date-fns";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useSessions, useCreateSession } from "@/hooks/use-sessions";
import type { CreateSessionRequest, SessionListItem } from "@/lib/api-types";
import {
  Plus,
  Search,
  MessageSquare,
  Cpu,
  Clock,
  ArrowRight,
  Filter,
  Workflow,
  Radio,
} from "lucide-react";

export const Route = createFileRoute("/_admin/sessions")({
  component: SessionsPage,
});

type SessionKindFilter = "all" | "direct" | "subagent" | "scheduled";
type SessionStatusFilter = "all" | "active" | "idle" | "error";
type SessionSort = "newest" | "oldest" | "turns" | "tokens";

function relativeDate(value?: string) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return formatDistanceToNow(date, { addSuffix: true });
}

function formatNumber(value: number) {
  return new Intl.NumberFormat().format(value);
}

function SessionCard({ session }: { session: SessionListItem }) {
  const totalTokens = session.usage_prompt_tokens + session.usage_completion_tokens;

  return (
    <Card className="transition-colors hover:border-primary/40">
      <CardHeader className="pb-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <CardTitle className="text-base font-semibold">
                <code className="rounded bg-muted px-2 py-1 text-xs">{session.id.slice(0, 12)}...</code>
              </CardTitle>
              <Badge variant="outline">{session.kind}</Badge>
              <Badge variant={session.status === "active" ? "default" : "secondary"}>{session.status}</Badge>
              {session.channel && <Badge variant="secondary">{session.channel}</Badge>}
            </div>
            <CardDescription className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs">
              <span className="inline-flex items-center gap-1">
                <Clock className="h-3.5 w-3.5" />
                created {relativeDate(session.created_at)}
              </span>
              {session.requester_session_id && (
                <span className="inline-flex items-center gap-1 font-mono text-[11px]">
                  <Workflow className="h-3.5 w-3.5" />
                  parent {session.requester_session_id.slice(0, 12)}...
                </span>
              )}
            </CardDescription>
          </div>

          <Button asChild size="sm" variant="outline">
            <Link to="/sessions/$id" params={{ id: session.id }}>
              Open
              <ArrowRight className="ml-1 h-4 w-4" />
            </Link>
          </Button>
        </div>
      </CardHeader>

      <CardContent>
        <div className="grid gap-3 sm:grid-cols-3">
          <div className="rounded-lg border bg-muted/20 p-3">
            <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
              <MessageSquare className="h-3.5 w-3.5" />
              Turns
            </div>
            <div className="text-lg font-semibold">{formatNumber(session.turn_count)}</div>
          </div>

          <div className="rounded-lg border bg-muted/20 p-3">
            <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
              <Cpu className="h-3.5 w-3.5" />
              Latest model
            </div>
            <div className="truncate text-sm font-medium">{session.latest_model ?? "—"}</div>
          </div>

          <div className="rounded-lg border bg-muted/20 p-3">
            <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
              <Radio className="h-3.5 w-3.5" />
              Total tokens
            </div>
            <div className="text-lg font-semibold">{formatNumber(totalTokens)}</div>
            <div className="text-xs text-muted-foreground">
              {formatNumber(session.usage_prompt_tokens)} in / {formatNumber(session.usage_completion_tokens)} out
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function SessionsPage() {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<SessionKindFilter>("all");
  const [status, setStatus] = useState<SessionStatusFilter>("all");
  const [sort, setSort] = useState<SessionSort>("newest");

  const [newKind, setNewKind] = useState<CreateSessionRequest["kind"]>("direct");
  const [newChannel, setNewChannel] = useState("");
  const [newWorkspaceRoot, setNewWorkspaceRoot] = useState("");

  const { data: sessions, isLoading } = useSessions();
  const createSession = useCreateSession();

  const filteredSessions = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    const items = [...(sessions ?? [])]
      .filter((session) => {
        if (kind !== "all" && session.kind !== kind) return false;
        if (status !== "all" && session.status !== status) return false;
        if (!normalizedQuery) return true;
        return [
          session.id,
          session.kind,
          session.status,
          session.channel ?? "",
          session.latest_model ?? "",
          session.requester_session_id ?? "",
        ].some((value) => value.toLowerCase().includes(normalizedQuery));
      })
      .sort((a, b) => {
        switch (sort) {
          case "oldest":
            return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
          case "turns":
            return b.turn_count - a.turn_count;
          case "tokens":
            return (b.usage_prompt_tokens + b.usage_completion_tokens) - (a.usage_prompt_tokens + a.usage_completion_tokens);
          case "newest":
          default:
            return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
        }
      });

    return items;
  }, [sessions, query, kind, status, sort]);

  const counts = useMemo(() => {
    const source = sessions ?? [];
    return {
      total: source.length,
      active: source.filter((session) => session.status === "active").length,
      direct: source.filter((session) => session.kind === "direct").length,
      subagent: source.filter((session) => session.kind === "subagent").length,
    };
  }, [sessions]);

  const handleCreateSession = () => {
    createSession.mutate({
      kind: newKind,
      channel_ref: newChannel.trim() || undefined,
      workspace_root: newWorkspaceRoot.trim() || undefined,
    }, {
      onSuccess: () => {
        setNewChannel("");
        setNewWorkspaceRoot("");
      },
    });
  };

  return (
    <div className="space-y-8">
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold tracking-tight">Sessions</h1>
        <p className="text-muted-foreground">
          Inspect runtime sessions, create new direct or background lanes, and jump into transcript detail.
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-4">
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Total sessions</CardDescription>
            <CardTitle className="text-2xl">{formatNumber(counts.total)}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Active</CardDescription>
            <CardTitle className="text-2xl">{formatNumber(counts.active)}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Direct</CardDescription>
            <CardTitle className="text-2xl">{formatNumber(counts.direct)}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Subagents</CardDescription>
            <CardTitle className="text-2xl">{formatNumber(counts.subagent)}</CardTitle>
          </CardHeader>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Plus className="h-5 w-5" />
            Create session
          </CardTitle>
          <CardDescription>Spin up a new session directly from the admin UI.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4 lg:grid-cols-[180px,1fr,1fr,auto]">
          <div className="space-y-2">
            <label className="text-sm font-medium">Kind</label>
            <Select value={newKind ?? "direct"} onValueChange={(value) => setNewKind(value)}>
              <SelectTrigger>
                <SelectValue placeholder="Session kind" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="direct">direct</SelectItem>
                <SelectItem value="subagent">subagent</SelectItem>
                <SelectItem value="scheduled">scheduled</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Channel ref</label>
            <Input
              placeholder="telegram, discord, web..."
              value={newChannel}
              onChange={(e) => setNewChannel(e.target.value)}
            />
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Workspace root</label>
            <Input
              placeholder="/home/hamza/Development/project"
              value={newWorkspaceRoot}
              onChange={(e) => setNewWorkspaceRoot(e.target.value)}
            />
          </div>

          <div className="flex items-end">
            <Button onClick={handleCreateSession} disabled={createSession.isPending} className="w-full lg:w-auto">
              {createSession.isPending ? "Creating..." : "Create"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Filter className="h-5 w-5" />
            Filter sessions
          </CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4 lg:grid-cols-[1.5fr,180px,180px,180px]">
          <div className="space-y-2">
            <label className="text-sm font-medium">Search</label>
            <div className="relative">
              <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search by session id, model, channel, parent..."
                className="pl-9"
              />
            </div>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Kind</label>
            <Select value={kind} onValueChange={(value) => setKind(value as SessionKindFilter)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">all</SelectItem>
                <SelectItem value="direct">direct</SelectItem>
                <SelectItem value="subagent">subagent</SelectItem>
                <SelectItem value="scheduled">scheduled</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Status</label>
            <Select value={status} onValueChange={(value) => setStatus(value as SessionStatusFilter)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">all</SelectItem>
                <SelectItem value="active">active</SelectItem>
                <SelectItem value="idle">idle</SelectItem>
                <SelectItem value="error">error</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Sort</label>
            <Select value={sort} onValueChange={(value) => setSort(value as SessionSort)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="newest">newest first</SelectItem>
                <SelectItem value="oldest">oldest first</SelectItem>
                <SelectItem value="turns">most turns</SelectItem>
                <SelectItem value="tokens">most tokens</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <div className="space-y-4">
        {isLoading ? (
          Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-48 w-full" />)
        ) : filteredSessions.length === 0 ? (
          <Card>
            <CardContent className="flex min-h-40 items-center justify-center text-sm text-muted-foreground">
              No sessions match the current filters.
            </CardContent>
          </Card>
        ) : (
          filteredSessions.map((session) => <SessionCard key={session.id} session={session} />)
        )}
      </div>
    </div>
  );
}
