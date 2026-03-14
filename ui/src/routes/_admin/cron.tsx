import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
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
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import {
  useCronJobs,
  useCronStatus,
  useCronRuns,
  useUpdateCronJob,
  useDeleteCronJob,
  useRunCronJob,
  useCronWake,
  useCreateCronJob,
} from "@/hooks/use-cron";
import {
  Clock,
  Play,
  Trash2,
  History,
  Plus,
  Zap,
  CheckCircle2,
  XCircle,
  Loader2,
} from "lucide-react";
import type { CronJobResponse } from "@/lib/api-types";

export const Route = createFileRoute("/_admin/cron")({
  component: CronPage,
});

function RunHistorySheet({ job }: { job: CronJobResponse }) {
  const { data: runs, isLoading } = useCronRuns(job.id);

  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon" className="h-8 w-8" title="Run history">
          <History className="h-4 w-4" />
        </Button>
      </SheetTrigger>
      <SheetContent className="w-96">
        <SheetHeader>
          <SheetTitle>Run History: {job.name ?? job.id.slice(0, 8)}</SheetTitle>
        </SheetHeader>
        <div className="mt-4 space-y-2">
          {isLoading ? (
            <Skeleton className="h-20 w-full" />
          ) : !runs?.length ? (
            <p className="text-sm text-muted-foreground">No runs yet</p>
          ) : (
            runs.map((run, i) => (
              <div key={i} className="rounded-md border p-3 text-sm">
                <div className="flex items-center gap-2">
                  {run.status === "completed" ? (
                    <CheckCircle2 className="h-4 w-4 text-green-500" />
                  ) : run.status === "failed" ? (
                    <XCircle className="h-4 w-4 text-destructive" />
                  ) : (
                    <Loader2 className="h-4 w-4 animate-spin text-primary" />
                  )}
                  <Badge variant="outline">{run.status}</Badge>
                  <span className="ml-auto text-xs text-muted-foreground">
                    {new Date(run.started_at).toLocaleString()}
                  </span>
                </div>
                {run.output && (
                  <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap rounded bg-muted/50 p-2 text-xs">
                    {run.output}
                  </pre>
                )}
              </div>
            ))
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function CronPage() {
  const { data: jobs, isLoading } = useCronJobs();
  const { data: status } = useCronStatus();
  const updateJob = useUpdateCronJob();
  const deleteJob = useDeleteCronJob();
  const runJob = useRunCronJob();
  const wake = useCronWake();
  const createJob = useCreateCronJob();

  const [createOpen, setCreateOpen] = useState(false);
  const [wakeOpen, setWakeOpen] = useState(false);
  const [wakeText, setWakeText] = useState("");
  const [newName, setNewName] = useState("");
  const [newExpr, setNewExpr] = useState("0 */5 * * * *");
  const [newTarget, setNewTarget] = useState("isolated");
  const [newMessage, setNewMessage] = useState("");

  const handleCreate = () => {
    createJob.mutate(
      {
        name: newName || undefined,
        schedule: { kind: "cron", expr: newExpr },
        payload:
          newTarget === "main"
            ? { kind: "system_event", text: newMessage }
            : { kind: "agent_turn", message: newMessage },
        sessionTarget: newTarget,
        enabled: true,
      },
      {
        onSuccess: () => {
          setCreateOpen(false);
          setNewName("");
          setNewExpr("0 */5 * * * *");
          setNewTarget("isolated");
          setNewMessage("");
        },
      }
    );
  };

  const handleWake = () => {
    wake.mutate(
      { text: wakeText },
      {
        onSuccess: () => {
          setWakeOpen(false);
          setWakeText("");
        },
      }
    );
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Cron Jobs</h1>
          <p className="text-muted-foreground">Scheduled tasks and automation</p>
        </div>
        <div className="flex gap-2">
          <Dialog open={wakeOpen} onOpenChange={setWakeOpen}>
            <DialogTrigger asChild>
              <Button variant="outline" className="gap-2">
                <Zap className="h-4 w-4" />
                Wake
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Wake Trigger</DialogTitle>
              </DialogHeader>
              <div className="space-y-4 pt-4">
                <div className="space-y-2">
                  <Label>Message</Label>
                  <Textarea
                    value={wakeText}
                    onChange={(e) => setWakeText(e.target.value)}
                    placeholder="Wake message..."
                  />
                </div>
                <Button
                  onClick={handleWake}
                  disabled={!wakeText || wake.isPending}
                  className="w-full"
                >
                  {wake.isPending ? "Sending..." : "Send Wake"}
                </Button>
              </div>
            </DialogContent>
          </Dialog>

          <Dialog open={createOpen} onOpenChange={setCreateOpen}>
            <DialogTrigger asChild>
              <Button className="gap-2">
                <Plus className="h-4 w-4" />
                New Job
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Create Cron Job</DialogTitle>
              </DialogHeader>
              <div className="space-y-4 pt-4">
                <div className="space-y-2">
                  <Label>Name</Label>
                  <Input
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    placeholder="Job name"
                  />
                </div>
                <div className="space-y-2">
                  <Label>Cron Expression</Label>
                  <Input
                    value={newExpr}
                    onChange={(e) => setNewExpr(e.target.value)}
                    placeholder="0 */5 * * * *"
                  />
                </div>
                <div className="space-y-2">
                  <Label>Session Target</Label>
                  <Input
                    value={newTarget}
                    onChange={(e) => setNewTarget(e.target.value)}
                    placeholder="isolated or main"
                  />
                </div>
                <div className="space-y-2">
                  <Label>Message</Label>
                  <Textarea
                    value={newMessage}
                    onChange={(e) => setNewMessage(e.target.value)}
                    placeholder="Agent prompt for isolated jobs, or system-event text for main-session jobs..."
                  />
                </div>
                <Button
                  onClick={handleCreate}
                  disabled={!newExpr || !newMessage || createJob.isPending}
                  className="w-full"
                >
                  {createJob.isPending ? "Creating..." : "Create"}
                </Button>
              </div>
            </DialogContent>
          </Dialog>
        </div>
      </div>

      {/* Status bar */}
      {status && (
        <div className="flex flex-wrap gap-3">
          <Badge variant="outline" className="gap-1">
            Total: {status.total_jobs}
          </Badge>
          <Badge variant="outline" className="gap-1">
            Enabled: {status.enabled_jobs}
          </Badge>
          <Badge variant={status.due_jobs > 0 ? "default" : "outline"} className="gap-1">
            Due: {status.due_jobs}
          </Badge>
        </div>
      )}

      {/* Jobs table */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Clock className="h-4 w-4" />
            Jobs ({jobs?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-12 w-full" />
              ))}
            </div>
          ) : !jobs?.length ? (
            <p className="text-sm text-muted-foreground">No cron jobs configured</p>
          ) : (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Schedule</TableHead>
                    <TableHead>Target</TableHead>
                    <TableHead>Enabled</TableHead>
                    <TableHead>Runs</TableHead>
                    <TableHead>Last Run</TableHead>
                    <TableHead>Next Run</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {jobs.map((job) => (
                    <TableRow key={job.id}>
                      <TableCell className="font-medium">
                        {job.name ?? (
                          <span className="font-mono text-xs text-muted-foreground">
                            {job.id.slice(0, 8)}
                          </span>
                        )}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {job.schedule.kind === "cron"
                          ? job.schedule.expr
                          : job.schedule.kind === "every"
                            ? `every ${job.schedule.every_ms}ms`
                            : `at ${job.schedule.at}`}
                      </TableCell>
                      <TableCell className="text-sm">{job.session_target}</TableCell>
                      <TableCell>
                        <Switch
                          checked={job.enabled}
                          onCheckedChange={(enabled) =>
                            updateJob.mutate({ id: job.id, data: { enabled } })
                          }
                        />
                      </TableCell>
                      <TableCell className="text-sm">{job.run_count}</TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {job.last_run_at
                          ? new Date(job.last_run_at).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {job.next_run_at
                          ? new Date(job.next_run_at).toLocaleString()
                          : "—"}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            title="Run now"
                            onClick={() => runJob.mutate(job.id)}
                          >
                            <Play className="h-4 w-4" />
                          </Button>
                          <RunHistorySheet job={job} />
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-destructive"
                            title="Delete"
                            onClick={() => {
                              if (confirm("Delete this job?")) {
                                deleteJob.mutate(job.id);
                              }
                            }}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
