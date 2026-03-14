import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUsage } from "@/hooks/use-usage";
import { BarChart3, Download, Coins, Zap, Hash } from "lucide-react";

export const Route = createFileRoute("/_admin/usage")({
  component: UsagePage,
});

function UsagePage() {
  const { data: usage, isLoading } = useUsage();
  const [groupBy, setGroupBy] = useState<"model" | "date">("model");

  const exportCsv = () => {
    if (!usage?.entries.length) return;
    const headers = [
      "date",
      "model",
      "provider",
      "prompt_tokens",
      "completion_tokens",
      "total_tokens",
      "request_count",
      "estimated_cost",
    ];
    const rows = usage.entries.map((e) =>
      [
        e.date,
        e.model,
        e.provider,
        e.prompt_tokens,
        e.completion_tokens,
        e.total_tokens,
        e.request_count,
        e.estimated_cost ?? "",
      ].join(",")
    );
    const csv = [headers.join(","), ...rows].join("\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `rune-usage-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const grouped = usage?.entries.reduce(
    (acc, entry) => {
      const key = groupBy === "model" ? entry.model : entry.date;
      if (!acc[key]) {
        acc[key] = {
          label: key,
          prompt_tokens: 0,
          completion_tokens: 0,
          total_tokens: 0,
          request_count: 0,
        };
      }
      acc[key].prompt_tokens += entry.prompt_tokens;
      acc[key].completion_tokens += entry.completion_tokens;
      acc[key].total_tokens += entry.total_tokens;
      acc[key].request_count += entry.request_count;
      return acc;
    },
    {} as Record<
      string,
      {
        label: string;
        prompt_tokens: number;
        completion_tokens: number;
        total_tokens: number;
        request_count: number;
      }
    >
  );

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Usage</h1>
          <p className="text-muted-foreground">
            Token usage and cost analytics
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={exportCsv}
          disabled={!usage?.entries.length}
        >
          <Download className="mr-2 h-4 w-4" />
          Export CSV
        </Button>
      </div>

      {/* Summary cards */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Zap className="h-4 w-4" />
              Prompt Tokens
            </div>
            <p className="mt-1 text-2xl font-bold">
              {isLoading ? (
                <Skeleton className="h-8 w-24" />
              ) : (
                (usage?.total_prompt_tokens ?? 0).toLocaleString()
              )}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Hash className="h-4 w-4" />
              Completion Tokens
            </div>
            <p className="mt-1 text-2xl font-bold">
              {isLoading ? (
                <Skeleton className="h-8 w-24" />
              ) : (
                (usage?.total_completion_tokens ?? 0).toLocaleString()
              )}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <BarChart3 className="h-4 w-4" />
              Total Tokens
            </div>
            <p className="mt-1 text-2xl font-bold">
              {isLoading ? (
                <Skeleton className="h-8 w-24" />
              ) : (
                (usage?.total_tokens ?? 0).toLocaleString()
              )}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Coins className="h-4 w-4" />
              Estimated Cost
            </div>
            <p className="mt-1 text-2xl font-bold">
              {isLoading ? (
                <Skeleton className="h-8 w-24" />
              ) : (
                usage?.total_estimated_cost ?? "—"
              )}
            </p>
          </CardContent>
        </Card>
      </div>

      {/* Breakdown table */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2 text-base">
              <BarChart3 className="h-4 w-4" />
              Breakdown
            </CardTitle>
            <Select
              value={groupBy}
              onValueChange={(v) => setGroupBy(v as "model" | "date")}
            >
              <SelectTrigger className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="model">By Model</SelectItem>
                <SelectItem value="date">By Date</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !grouped || !Object.keys(grouped).length ? (
            <p className="text-sm text-muted-foreground">
              No usage data available yet
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>
                    {groupBy === "model" ? "Model" : "Date"}
                  </TableHead>
                  <TableHead className="text-right">Prompt</TableHead>
                  <TableHead className="text-right">Completion</TableHead>
                  <TableHead className="text-right">Total</TableHead>
                  <TableHead className="text-right">Requests</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {Object.values(grouped)
                  .sort((a, b) => b.total_tokens - a.total_tokens)
                  .map((row) => (
                    <TableRow key={row.label}>
                      <TableCell>
                        <Badge variant="outline" className="font-mono text-xs">
                          {row.label}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        {row.prompt_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        {row.completion_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm font-medium">
                        {row.total_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        {row.request_count.toLocaleString()}
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
