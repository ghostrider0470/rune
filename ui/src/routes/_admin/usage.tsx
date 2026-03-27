import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { format } from "date-fns";
import type { DateRange } from "react-day-picker";
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
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Calendar } from "@/components/ui/calendar";
import { useUsage } from "@/hooks/use-usage";
import {
  BarChart3,
  Download,
  Coins,
  Zap,
  Hash,
  CalendarIcon,
} from "lucide-react";

export const Route = createFileRoute("/_admin/usage")({
  component: UsagePage,
});

const PRESETS = [
  { value: "24h", label: "24h" },
  { value: "7d", label: "7d" },
  { value: "30d", label: "30d" },
  { value: "90d", label: "90d" },
  { value: "all", label: "All" },
  { value: "custom", label: "Custom" },
] as const;

type PresetValue = (typeof PRESETS)[number]["value"];

function usePeriodParams(
  preset: PresetValue,
  dateRange: DateRange | undefined
) {
  if (preset === "custom" && dateRange?.from) {
    return {
      from: dateRange.from.toISOString(),
      to: dateRange.to?.toISOString(),
    };
  }
  if (preset === "all") return {};
  const map: Record<string, string> = {
    "24h": "1d",
    "7d": "7d",
    "30d": "30d",
    "90d": "90d",
  };
  return { period: map[preset] };
}

function UsagePage() {
  const [preset, setPreset] = useState<PresetValue>("7d");
  const [dateRange, setDateRange] = useState<DateRange | undefined>();
  const params = usePeriodParams(preset, dateRange);
  const { data: usage, isLoading } = useUsage(params);
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

  const rangeLabel =
    dateRange?.from && preset === "custom"
      ? dateRange.to
        ? `${format(dateRange.from, "MMM d")} - ${format(dateRange.to, "MMM d")}`
        : format(dateRange.from, "MMM d, yyyy")
      : "Pick dates";

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Usage</h1>
          <p className="mt-1 text-muted-foreground">
            Token usage and cost analytics
          </p>
        </div>
        <div className="flex items-center gap-3">
          {/* Pill toggle bar */}
          <div className="inline-flex items-center rounded-lg border bg-muted p-0.5">
            {PRESETS.filter((p) => p.value !== "custom").map((p) => (
              <button
                key={p.value}
                onClick={() => setPreset(p.value)}
                className={`rounded-md px-3 py-1.5 text-xs font-medium transition-all ${
                  preset === p.value
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {p.label}
              </button>
            ))}
          </div>

          {/* Custom date range */}
          <Popover>
            <PopoverTrigger asChild>
              <Button
                variant={preset === "custom" ? "default" : "outline"}
                size="sm"
                className="gap-2"
              >
                <CalendarIcon className="h-3.5 w-3.5" />
                <span className="text-xs">{rangeLabel}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-auto p-0" align="end">
              <Calendar
                mode="range"
                selected={dateRange}
                onSelect={(range) => {
                  setDateRange(range);
                  if (range?.from) setPreset("custom");
                }}
                numberOfMonths={2}
                disabled={{ after: new Date() }}
              />
            </PopoverContent>
          </Popover>

          <Button
            variant="outline"
            size="sm"
            onClick={exportCsv}
            disabled={!usage?.entries.length}
          >
            <Download className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Summary cards */}
      <div className="grid gap-6 sm:grid-cols-2 lg:grid-cols-5">
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
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Zap className="h-4 w-4" />
              Cache Hit Ratio
            </div>
            <p className="mt-1 text-2xl font-bold">
              {isLoading ? (
                <Skeleton className="h-8 w-24" />
              ) : (
                `${((usage?.cache_hit_ratio ?? 0) * 100).toFixed(1)}%`
              )}
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {(usage?.usage_cached_prompt_tokens ?? 0).toLocaleString()} cached of{" "}
              {(usage?.total_prompt_tokens ?? 0).toLocaleString()} prompt tokens
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
                  <TableHead className="py-3.5">
                    {groupBy === "model" ? "Model" : "Date"}
                  </TableHead>
                  <TableHead className="py-3.5 text-right">Prompt</TableHead>
                  <TableHead className="py-3.5 text-right">Completion</TableHead>
                  <TableHead className="py-3.5 text-right">Total</TableHead>
                  <TableHead className="py-3.5 text-right">Requests</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {Object.values(grouped)
                  .sort((a, b) => b.total_tokens - a.total_tokens)
                  .map((row) => (
                    <TableRow key={row.label}>
                      <TableCell className="py-3">
                        <Badge variant="outline" className="font-mono text-xs">
                          {row.label}
                        </Badge>
                      </TableCell>
                      <TableCell className="py-3 text-right font-mono text-sm">
                        {row.prompt_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="py-3 text-right font-mono text-sm">
                        {row.completion_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="py-3 text-right font-mono text-sm font-medium">
                        {row.total_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="py-3 text-right font-mono text-sm">
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
