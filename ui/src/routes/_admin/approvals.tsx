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

  const submitApprovalDecision = (id: string, decision: "approve" | "deny") => {
    submitDecision.mutate({
      id,
      decision,
      decided_by: "admin-ui",
    });
  };

  return (
    <div className="space-y-4 sm:space-y-6">
      <div className="space-y-1">
        <h1 className="text-2xl font-bold tracking-tight">Approvals</h1>
        <p className="text-muted-foreground">Pending approvals and tool policies</p>
      </div>

      {/* Pending approvals */}
      <Card>
        <CardHeader>
          <CardTitle className="flex flex-wrap items-center gap-2 text-base">
            <ShieldCheck className="h-4 w-4" />
            Pending Approvals ({pending?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
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
                  className="rounded-xl border bg-muted/20 p-3 sm:p-4"
                >
                  <div className="flex flex-col gap-3 sm:gap-4">
                    <div className="min-w-0 space-y-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge variant="outline">{approval.subject_type}</Badge>
                        <code className="max-w-full truncate text-xs text-muted-foreground">
                          {approval.subject_id.slice(0, 12)}
                        </code>
                        <span className="text-xs text-muted-foreground sm:ml-auto">
                          {new Date(approval.created_at).toLocaleString()}
                        </span>
                      </div>
                      <p className="text-sm leading-6">{approval.reason}</p>
                      {approval.command && (
                        <pre className="overflow-x-auto rounded-lg bg-muted px-3 py-2 text-xs leading-5">
                          {approval.command}
                        </pre>
                      )}
                    </div>
                    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                      <Button
                        size="sm"
                        onClick={() => submitApprovalDecision(approval.id, "approve")}
                        disabled={submitDecision.isPending}
                        className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
                      >
                        <CheckCircle2 className="h-3.5 w-3.5" />
                        Approve
                      </Button>
                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() => submitApprovalDecision(approval.id, "deny")}
                        disabled={submitDecision.isPending}
                        className="min-h-11 gap-1.5 rounded-lg text-sm font-semibold sm:min-h-9"
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
        <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <CardTitle className="flex flex-wrap items-center gap-2 text-base">
            Policies ({policies?.length ?? 0})
          </CardTitle>
          <Dialog open={policyOpen} onOpenChange={setPolicyOpen}>
            <DialogTrigger asChild>
              <Button variant="outline" size="sm" className="w-full gap-1 sm:w-auto">
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
            <div className="-mx-4 overflow-x-auto px-4 sm:mx-0 sm:px-0">
              <Table className="min-w-[36rem]">
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
                          className="h-9 w-9 text-destructive"
                          onClick={() => clearPolicy.mutate(p.tool_name)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
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
