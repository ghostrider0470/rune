import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
import { Skeleton } from "@/components/ui/skeleton";
import { useReminders, useCreateReminder, useCancelReminder } from "@/hooks/use-system";
import { Bell, Plus, Trash2 } from "lucide-react";

export const Route = createFileRoute("/_admin/reminders")({
  component: RemindersPage,
});

function RemindersPage() {
  const { data: reminders, isLoading } = useReminders();
  const createReminder = useCreateReminder();
  const cancelReminder = useCancelReminder();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [message, setMessage] = useState("");
  const [fireAt, setFireAt] = useState("");

  const handleCreate = () => {
    createReminder.mutate(
      {
        message,
        fire_at: new Date(fireAt).toISOString(),
      },
      {
        onSuccess: () => {
          setDialogOpen(false);
          setMessage("");
          setFireAt("");
        },
      }
    );
  };

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Reminders</h1>
          <p className="mt-1 text-muted-foreground">Scheduled reminder notifications</p>
        </div>
        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogTrigger asChild>
            <Button className="gap-2">
              <Plus className="h-4 w-4" />
              New Reminder
            </Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Create Reminder</DialogTitle>
            </DialogHeader>
            <div className="space-y-4 pt-4">
              <div className="space-y-3">
                <Label>Message</Label>
                <Textarea
                  value={message}
                  onChange={(e) => setMessage(e.target.value)}
                  placeholder="Reminder message..."
                />
              </div>
              <div className="space-y-3">
                <Label>Fire At</Label>
                <Input
                  type="datetime-local"
                  value={fireAt}
                  onChange={(e) => setFireAt(e.target.value)}
                />
              </div>
              <Button
                onClick={handleCreate}
                disabled={!message || !fireAt || createReminder.isPending}
                className="w-full"
              >
                {createReminder.isPending ? "Creating..." : "Create Reminder"}
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Bell className="h-4 w-4" />
            Reminders ({reminders?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !reminders?.length ? (
            <p className="text-sm text-muted-foreground">No reminders</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="py-3.5">Message</TableHead>
                  <TableHead className="py-3.5">Target</TableHead>
                  <TableHead className="py-3.5">Fire At</TableHead>
                  <TableHead className="py-3.5">Status</TableHead>
                  <TableHead className="py-3.5 text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {reminders.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="max-w-xs truncate py-3 text-sm">
                      {r.message}
                    </TableCell>
                    <TableCell className="py-3">
                      <Badge variant="outline">{r.target}</Badge>
                    </TableCell>
                    <TableCell className="py-3 text-xs text-muted-foreground">
                      {new Date(r.fire_at).toLocaleString()}
                    </TableCell>
                    <TableCell className="py-3">
                      <Badge variant={r.delivered ? "secondary" : "default"}>
                        {r.delivered ? "Delivered" : "Pending"}
                      </Badge>
                    </TableCell>
                    <TableCell className="py-3 text-right">
                      {!r.delivered && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-destructive"
                          onClick={() => cancelReminder.mutate(r.id)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      )}
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
