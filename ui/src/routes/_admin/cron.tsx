import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { toast } from "sonner";
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
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Sheet,
  SheetContent,
  SheetDescription,
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
import type { CronJobRequest, CronJobResponse, CronRunResponse } from "@/lib/api-types";

export const Route = createFileRoute("/_admin/cron")({
  component: CronPage,
});

const SESSION_TARGETS = ["isolated", "main"] as const;

function formatDateTime(value: string | null) {
  return value ? new Date(value).toLocaleString() : "—";
}

function formatSchedule(job: CronJobResponse) {
  return job.schedule.kind === "cron"
    ? job.schedule.expr
    : job.schedule.kind === "every"
      ? `every ${job.schedule.every_ms}ms`
      : `at ${job.schedule.at}`;
}

function formatRunDuration(run: CronRunResponse) {
  if (!run.finished_at) return "In progress";
  const durationMs = new Date(run.finished_at).getTime() - new Date(run.started_at).getTime();
  if (Number.isNaN(durationMs) || durationMs < 0) return "—";
  if (durationMs < 1_000) return `${durationMs}ms`;
  if (durationMs < 60_000) return `${(durationMs / 1_000).toFixed(1)}s`;
  return `${(durationMs / 60_000).toFixed(1)}m`;
}

function summarizeRunOutput(output: string | null) {
  if (!output) return "No output recorded";
  const singleLine = output.replace(/\s+/g, " ").trim();
  return singleLine.length > 120 ? `${singleLine.slice(0, 117)}...` : singleLine;
}

function RunHistorySheet({ job }: { job: CronJobResponse }) {
  const { data: runs, isLoading } = useCronRuns(job.id);

  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon" className="h-8 w-8" title="Run history">
          <History className="h-4 w-4" />
        </Button>
      </SheetTrigger>
      <SheetContent className="w-full sm:max-w-xl">
        <SheetHeader>
          <SheetTitle>Run history: {job.name ?? job.id.slice(0, 8)}</SheetTitle>
          <SheetDescription>
            Recent executions for this cron job, including duration and captured output.
          </SheetDescription>
        </SheetHeader>
        <div className="mt-4 space-y-3">
          {isLoading ? (
            <Skeleton className="h-24 w-full" />
          ) : !runs?.length ? (
            <p className="text-sm text-muted-foreground">No runs yet</p>
          ) : (
            runs.map((run, i) => (
              <div key={`${run.started_at}-${i}`} className="rounded-md border p-3 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  {run.status === "completed" ? (
                    <CheckCircle2 className="h-4 w-4 text-green-500" />
                  ) : run.status === "failed" ? (
                    <XCircle className="h-4 w-4 text-destructive" />
                  ) : (
                    <Loader2 className="h-4 w-4 animate-spin text-primary" />
                  )}
                  <Badge variant="outline">{run.status}</Badge>
                  <Badge variant="secondary">{formatRunDuration(run)}</Badge>
                  <span className="ml-auto text-xs text-muted-foreground">
                    {formatDateTime(run.started_at)}
                  </span>
                </div>
                <p className="mt-2 text-xs text-muted-foreground">
                  {summarizeRunOutput(run.output)}
                </p>
                {run.output && (
                  <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap rounded bg-muted/50 p-2 text-xs">
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
  const [newTarget, setNewTarget] = useState<(typeof SESSION_TARGETS)[number]>("isolated");
  const [newMessage, setNewMessage] = useState("");
  const [search, setSearch] = useState("");
  const [showDisabled, setShowDisabled] = useState(true);

  const filteredJobs = useMemo(() => {
    const query = search.trim().toLowerCase();
    return (jobs ?? []).filter((job) => {
      if (!showDisabled && !job.enabled) return false;
      if (!query) return true;
      return [job.name ?? "", job.id, job.session_target, formatSchedule(job)]
        .join(" ")
        .toLowerCase()
        .includes(query);
    });
  }, [jobs, search, showDisabled]);

  const handleCreate = () => {
    const message = newMessage.trim();
    const name = newName.trim();
    const expr = newExpr.trim();

    if (!expr || !message) {
      toast.error("Cron expression and message are required");
      return;
    }

    const payload: CronJobRequest["payload"] =
      newTarget === "main"
        ? { kind: "system_event", text: message }
        : { kind: "agent_turn", message };

    createJob.mutate(
      {
        name: name || undefined,
        schedule: { kind: "cron", expr },
        payload,
        sessionTarget: newTarget,
        enabled: true,
      },
      {
        onSuccess: (result) => {
          toast.success(result.message || "Cron job created");
          setCreateOpen(false);
          setNewName("");
          setNewExpr("0 */5 * * * *");
          setNewTarget("isolated");
          setNewMessage("");
        },
        onError: (error) => toast.error(error.message),
      }
    );
  };

  const handleWake = () => {
    const text = wakeText.trim();
    if (!text) {
      toast.error("Wake message is required");
      return;
    }

    wake.mutate(
      { text },
      {
        onSuccess: (result) => {
          toast.success(result.message || "Wake sent");
          setWakeOpen(false);
          setWakeText("");
        },
        onError: (error) => toast.error(error.message),
      }
    );
  };

  const handleToggleEnabled = (job: CronJobResponse, enabled: boolean) => {
    updateJob.mutate(
      { id: job.id, data: { enabled } },
      {
        onSuccess: (result) => toast.success(result.message || `Job ${enabled ? "enabled" : "disabled"}`),
        onError: (error) => toast.error(error.message),
      }
    );
  };

  const handleRunNow = (job: CronJobResponse) => {
    runJob.mutate(job.id, {
      onSuccess: (result) => toast.success(result.message || "Job queued"),
      onError: (error) => toast.error(error.message),
    });
  };

  const handleDelete = (job: CronJobResponse) => {
    if (!confirm(`Delete cron job \"${job.name ?? job.id.slice(0, 8)}\"?`)) return;
    deleteJob.mutate(job.id, {
      onSuccess: (result) => toast.success(result.message || "Job deleted"),
      onError: (error) => toast.error(error.message),
    });
  };

  return (
    <div className="space-y-8">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Cron Jobs</h1>
          <p className="mt-1 text-muted-foreground">Scheduled tasks and automation</p>
        </div>
        <div className="flex flex-wrap gap-3">
          <Dialog open={wakeOpen} onOpenChange={setWakeOpen}>
            <DialogTrigger asChild>
              <Button variant="outline" className="gap-2">
                <Zap className="h-4 w-4" />
                Wake
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Wake trigger</DialogTitle>
                <DialogDescription>
                  Send an immediate wake message to the cron subsystem.
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-4 pt-4">
                <div className="space-y-3">
                  <Label htmlFor="wake-message">Message</Label>
                  <Textarea
                    id="wake-message"
                    value={wakeText}
                    onChange={(e) => setWakeText(e.target.value)}
                    placeholder="Wake message..."
                  />
                </div>
                <Button onClick={handleWake} disabled={!wakeText.trim() || wake.isPending} className="w-full">
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
                <DialogTitle>Create cron job</DialogTitle>
                <DialogDescription>
                  Define a schedule and target session for automated execution.
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-4 pt-4">
                <div className="space-y-3">
                  <Label htmlFor="job-name">Name</Label>
                  <Input
                    id="job-name"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    placeholder="Job name"
                  />
                </div>
                <div className="space-y-3">
                  <Label htmlFor="cron-expression">Cron expression</Label>
                  <Input
                    id="cron-expression"
                    value={newExpr}
                    onChange={(e) => setNewExpr(e.target.value)}
                    placeholder="0 */5 * * * *"
                  />
                </div>
                <div className="space-y-3">
                  <Label htmlFor="session-target">Session target</Label>
                  <select
                    id="session-target"
                    className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]"
                    value={newTarget}
                    onChange={(e) => setNewTarget(e.target.value as (typeof SESSION_TARGETS)[number])}
                  >
                    {SESSION_TARGETS.map((target) => (
                      <option key={target} value={target}>
                        {target}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="space-y-3">
                  <Label htmlFor="job-message">Message</Label>
                  <Textarea
                    id="job-message"
                    value={newMessage}
                    onChange={(e) => setNewMessage(e.target.value)}
                    placeholder="Agent prompt for isolated jobs, or system-event text for main-session jobs..."
                  />
                </div>
                <Button
                  onClick={handleCreate}
                  disabled={!newExpr.trim() || !newMessage.trim() || createJob.isPending}
                  className="w-full"
                >
                  {createJob.isPending ? "Creating..." : "Create"}
                </Button>
              </div>
            </DialogContent>
          </Dialog>
        </div>
      </div>

      {status && (
        <div className="grid gap-3 sm:grid-cols-3">
          <Card>
            <CardContent className="flex items-center justify-between p-4">
              <span className="text-sm text-muted-foreground">Total jobs</span>
              <span className="text-2xl font-semibold">{status.total_jobs}</span>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="flex items-center justify-between p-4">
              <span className="text-sm text-muted-foreground">Enabled</span>
              <span className="text-2xl font-semibold">{status.enabled_jobs}</span>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="flex items-center justify-between p-4">
              <span className="text-sm text-muted-foreground">Due now</span>
              <Badge variant={status.due_jobs > 0 ? "default" : "outline"}>{status.due_jobs}</Badge>
            </CardContent>
          </Card>
        </div>
      )}

      <Card>
        <CardHeader className="gap-4">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <CardTitle className="flex items-center gap-2 text-base">
              <Clock className="h-4 w-4" />
              Jobs ({filteredJobs.length})
            </CardTitle>
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search jobs"
                className="sm:w-64"
              />
              <div className="flex items-center gap-2">
                <Switch checked={showDisabled} onCheckedChange={setShowDisabled} />
                <Label>Show disabled</Label>
              </div>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-12 w-full" />
              ))}
            </div>
          ) : !filteredJobs.length ? (
            <p className="text-sm text-muted-foreground">
              {jobs?.length ? "No cron jobs match the current filters" : "No cron jobs configured"}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Schedule</TableHead>
                    <TableHead>Target</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Runs</TableHead>
                    <TableHead>Last Run</TableHead>
                    <TableHead>Next Run</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredJobs.map((job) => (
                    <TableRow key={job.id}>
                      <TableCell className="font-medium">
                        <div className="space-y-1">
                          <div>{job.name ?? `Job ${job.id.slice(0, 8)}`}</div>
                          <div className="font-mono text-[11px] text-muted-foreground">{job.id}</div>
                        </div>
                      </TableCell>
                      <TableCell className="font-mono text-xs">{formatSchedule(job)}</TableCell>
                      <TableCell>
                        <Badge variant="secondary">{job.session_target}</Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-3">
                          <Switch
                            checked={job.enabled}
                            onCheckedChange={(enabled) => handleToggleEnabled(job, enabled)}
                            disabled={updateJob.isPending}
                          />
                          <Badge variant={job.enabled ? "default" : "outline"}>
                            {job.enabled ? "Enabled" : "Disabled"}
                          </Badge>
                        </div>
                      </TableCell>
                      <TableCell className="text-sm">{job.run_count}</TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatDateTime(job.last_run_at)}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatDateTime(job.next_run_at)}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex items-center justify-end gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            title="Run now"
                            onClick={() => handleRunNow(job)}
                            disabled={runJob.isPending}
                          >
                            <Play className="h-4 w-4" />
                          </Button>
                          <RunHistorySheet job={job} />
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-destructive"
                            title="Delete"
                            onClick={() => handleDelete(job)}
                            disabled={deleteJob.isPending}
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
