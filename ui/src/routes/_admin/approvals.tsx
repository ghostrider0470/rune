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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  usePendingApprovals,
  useApprovalPolicies,
  useSubmitApprovalDecision,
  useSetApprovalPolicy,
  useClearApprovalPolicy,
} from "@/hooks/use-approvals";
import {
  ShieldCheck,
  CheckCircle2,
  XCircle,
  Trash2,
  Plus,
} from "lucide-react";

export const Route = createFileRoute("/_admin/approvals")({
  component: ApprovalsPage,
});

function ApprovalsPage() {
  const { data: pending, isLoading: pendingLoading } = usePendingApprovals();
  const { data: policies, isLoading: policiesLoading } = useApprovalPolicies();
  const submitDecision = useSubmitApprovalDecision();
  const setPolicy = useSetApprovalPolicy();
  const clearPolicy = useClearApprovalPolicy();

  const [policyOpen, setPolicyOpen] = useState(false);
  const [policyTool, setPolicyTool] = useState("");
  const [policyDecision, setPolicyDecision] = useState("allow");

  const handleSetPolicy = () => {
    setPolicy.mutate(
      { tool: policyTool, data: { decision: policyDecision } },
      {
        onSuccess: () => {
          setPolicyOpen(false);
          setPolicyTool("");
          setPolicyDecision("allow");
        },
      }
    );
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Approvals</h1>
        <p className="text-muted-foreground">Pending approvals and tool policies</p>
      </div>

      {/* Pending approvals */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <ShieldCheck className="h-4 w-4" />
            Pending Approvals ({pending?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {pendingLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 2 }).map((_, i) => (
                <Skeleton key={i} className="h-20 w-full" />
              ))}
            </div>
          ) : !pending?.length ? (
            <p className="text-sm text-muted-foreground">No pending approvals</p>
          ) : (
            <div className="space-y-3">
              {pending.map((approval) => (
                <div
                  key={approval.id}
                  className="rounded-lg border p-4"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <Badge variant="outline">{approval.subject_type}</Badge>
                        <code className="text-xs">{approval.subject_id.slice(0, 12)}</code>
                      </div>
                      <p className="mt-1 text-sm">{approval.reason}</p>
                      {approval.command && (
                        <pre className="mt-2 rounded bg-muted/50 p-2 text-xs">
                          {approval.command}
                        </pre>
                      )}
                      <p className="mt-1 text-xs text-muted-foreground">
                        {new Date(approval.created_at).toLocaleString()}
                      </p>
                    </div>
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        onClick={() =>
                          submitDecision.mutate({
                            id: approval.id,
                            decision: "approve",
                            decided_by: "admin-ui",
                          })
                        }
                        disabled={submitDecision.isPending}
                        className="gap-1"
                      >
                        <CheckCircle2 className="h-3.5 w-3.5" />
                        Approve
                      </Button>
                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() =>
                          submitDecision.mutate({
                            id: approval.id,
                            decision: "deny",
                            decided_by: "admin-ui",
                          })
                        }
                        disabled={submitDecision.isPending}
                        className="gap-1"
                      >
                        <XCircle className="h-3.5 w-3.5" />
                        Deny
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Policies table */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle className="flex items-center gap-2 text-base">
            Policies ({policies?.length ?? 0})
          </CardTitle>
          <Dialog open={policyOpen} onOpenChange={setPolicyOpen}>
            <DialogTrigger asChild>
              <Button variant="outline" size="sm" className="gap-1">
                <Plus className="h-3.5 w-3.5" />
                Set Policy
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Set Approval Policy</DialogTitle>
              </DialogHeader>
              <div className="space-y-4 pt-4">
                <div className="space-y-2">
                  <Label>Tool Name</Label>
                  <Input
                    value={policyTool}
                    onChange={(e) => setPolicyTool(e.target.value)}
                    placeholder="e.g. bash, write_file"
                  />
                </div>
                <div className="space-y-2">
                  <Label>Decision</Label>
                  <Select value={policyDecision} onValueChange={setPolicyDecision}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="allow">Allow</SelectItem>
                      <SelectItem value="deny">Deny</SelectItem>
                      <SelectItem value="ask">Ask</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <Button
                  onClick={handleSetPolicy}
                  disabled={!policyTool || setPolicy.isPending}
                  className="w-full"
                >
                  {setPolicy.isPending ? "Setting..." : "Set Policy"}
                </Button>
              </div>
            </DialogContent>
          </Dialog>
        </CardHeader>
        <CardContent>
          {policiesLoading ? (
            <Skeleton className="h-20 w-full" />
          ) : !policies?.length ? (
            <p className="text-sm text-muted-foreground">No policies configured</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Tool</TableHead>
                  <TableHead>Policy</TableHead>
                  <TableHead>Set At</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {policies.map((p) => (
                  <TableRow key={p.tool_name}>
                    <TableCell className="font-mono text-sm">{p.tool_name}</TableCell>
                    <TableCell>
                      <Badge
                        variant={
                          p.decision === "allow"
                            ? "default"
                            : p.decision === "deny"
                              ? "destructive"
                              : "secondary"
                        }
                      >
                        {p.decision}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {new Date(p.decided_at).toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-destructive"
                        onClick={() => clearPolicy.mutate(p.tool_name)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
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
