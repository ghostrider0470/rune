import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { Link } from "@tanstack/react-router";
import { useSessions, useCreateSession } from "@/hooks/use-sessions";
import { MessageSquare, Plus } from "lucide-react";

export const Route = createFileRoute("/_admin/sessions")({
  component: SessionsPage,
});

function SessionsPage() {
  const [activeMinutes, setActiveMinutes] = useState<number | undefined>();
  const [channel, setChannel] = useState("");
  const [limit, setLimit] = useState(50);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [newKind, setNewKind] = useState("interactive");
  const [newChannel, setNewChannel] = useState("");

  const { data: sessions, isLoading } = useSessions({
    active_minutes: activeMinutes,
    channel: channel || undefined,
    limit,
  });

  const createSession = useCreateSession();

  const handleCreate = () => {
    createSession.mutate(
      {
        kind: newKind,
        channel_ref: newChannel || undefined,
      },
      {
        onSuccess: () => {
          setDialogOpen(false);
          setNewKind("interactive");
          setNewChannel("");
        },
      }
    );
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Sessions</h1>
          <p className="text-muted-foreground">Active and recent sessions</p>
        </div>
        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogTrigger asChild>
            <Button className="gap-2">
              <Plus className="h-4 w-4" />
              New Session
            </Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Create Session</DialogTitle>
            </DialogHeader>
            <div className="space-y-4 pt-4">
              <div className="space-y-2">
                <Label>Kind</Label>
                <Input
                  value={newKind}
                  onChange={(e) => setNewKind(e.target.value)}
                  placeholder="interactive"
                />
              </div>
              <div className="space-y-2">
                <Label>Channel (optional)</Label>
                <Input
                  value={newChannel}
                  onChange={(e) => setNewChannel(e.target.value)}
                  placeholder="e.g. telegram"
                />
              </div>
              <Button
                onClick={handleCreate}
                disabled={createSession.isPending}
                className="w-full"
              >
                {createSession.isPending ? "Creating..." : "Create"}
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </div>

      {/* Filters */}
      <Card>
        <CardContent className="flex flex-wrap gap-4 pt-6">
          <div className="space-y-1">
            <Label className="text-xs">Active (minutes)</Label>
            <Input
              type="number"
              className="w-28"
              placeholder="Any"
              value={activeMinutes ?? ""}
              onChange={(e) =>
                setActiveMinutes(e.target.value ? Number(e.target.value) : undefined)
              }
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs">Channel</Label>
            <Input
              className="w-36"
              placeholder="All"
              value={channel}
              onChange={(e) => setChannel(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs">Limit</Label>
            <Input
              type="number"
              className="w-20"
              value={limit}
              onChange={(e) => setLimit(Number(e.target.value) || 50)}
            />
          </div>
        </CardContent>
      </Card>

      {/* Sessions table */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <MessageSquare className="h-4 w-4" />
            Sessions ({sessions?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !sessions?.length ? (
            <p className="text-sm text-muted-foreground">No sessions found</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Channel</TableHead>
                  <TableHead>Turns</TableHead>
                  <TableHead>Model</TableHead>
                  <TableHead>Created</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {sessions.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell>
                      <div className="space-y-1">
                        <Link
                          to="/chat"
                          search={{ session: s.id }}
                          className="font-mono text-xs text-primary hover:underline"
                        >
                          {s.id.slice(0, 8)}...
                        </Link>
                        {s.preview && (
                          <p className="max-w-[28rem] truncate text-xs text-muted-foreground">
                            {s.preview}
                          </p>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={
                          s.status === "active"
                            ? "default"
                            : s.status === "idle"
                              ? "secondary"
                              : "outline"
                        }
                      >
                        {s.status}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-sm">
                      {s.channel ? (
                        <Badge variant="outline">{s.channel}</Badge>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </TableCell>
                    <TableCell className="text-sm">{s.turn_count}</TableCell>
                    <TableCell className="font-mono text-xs">
                      {s.latest_model ?? "—"}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {new Date(s.created_at).toLocaleString()}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
