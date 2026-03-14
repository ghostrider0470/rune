import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ChevronRight, Wrench, CheckCircle, XCircle, Clock, FileText, PanelRightOpen } from "lucide-react";
import { cn } from "@/lib/utils";
import type { TranscriptEntry } from "@/lib/api-types";

interface ToolCardProps {
  entry: TranscriptEntry;
  pairedEntry?: TranscriptEntry;
  className?: string;
  onInspect?: (entry: TranscriptEntry, pairedEntry?: TranscriptEntry) => void;
  isSelected?: boolean;
}

interface ToolRequestPayload {
  tool_name?: string;
  tool_name_human?: string;
  name?: string;
  tool_call_id?: string;
  arguments?: Record<string, unknown>;
  args?: Record<string, unknown>;
  input?: Record<string, unknown>;
}

interface ToolResultPayload {
  tool_name?: string;
  tool_name_human?: string;
  name?: string;
  tool_call_id?: string;
  result?: unknown;
  output?: unknown;
  error?: string | null;
  is_error?: boolean;
  duration_ms?: number;
  latency_ms?: number;
}

function getToolName(payload: unknown): string {
  if (!payload || typeof payload !== "object") return "tool";
  const p = payload as ToolRequestPayload;
  return p.tool_name ?? p.tool_name_human ?? p.name ?? "tool";
}

function getToolArgs(payload: unknown): Record<string, unknown> | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as ToolRequestPayload;
  return p.arguments ?? p.args ?? p.input ?? null;
}

function getToolResult(payload: unknown): {
  result: unknown;
  error: string | null;
  durationMs: number | null;
  isError: boolean;
} {
  if (!payload || typeof payload !== "object") {
    return { result: payload, error: null, durationMs: null, isError: false };
  }
  const p = payload as ToolResultPayload;
  return {
    result: p.result ?? p.output ?? payload,
    error: p.error ?? null,
    durationMs: p.duration_ms ?? p.latency_ms ?? null,
    isError: Boolean(p.is_error) || typeof p.error === "string",
  };
}

function summarizeArgs(args: Record<string, unknown> | null): string {
  if (!args) return "";
  const keys = Object.keys(args);
  if (keys.length === 0) return "";
  if (keys.length <= 3) return keys.join(", ");
  return `${keys.slice(0, 3).join(", ")} +${keys.length - 3}`;
}

function stringifyPreview(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function clampPreview(text: string, maxLength = 220): string {
  const compact = text.replace(/\s+/g, " ").trim();
  if (compact.length <= maxLength) return compact;
  return `${compact.slice(0, maxLength - 1)}…`;
}

export function ToolCard({
  entry,
  pairedEntry,
  className,
  onInspect,
  isSelected,
}: ToolCardProps) {
  const [expanded, setExpanded] = useState(false);

  const isRequest = entry.kind === "tool_request" || entry.kind === "tool_use";
  const isResult = entry.kind === "tool_result";

  const toolName = getToolName(entry.payload);
  const args = isRequest ? getToolArgs(entry.payload) : null;
  const { result, error, durationMs, isError } = isResult
    ? getToolResult(entry.payload)
    : { result: null, error: null, durationMs: null, isError: false };

  const preview = useMemo(() => {
    if (isRequest) {
      return args ? clampPreview(stringifyPreview(args), 140) : null;
    }

    if (!isResult) return null;
    if (error) return clampPreview(error, 160);
    if (result == null) return "No output";
    return clampPreview(stringifyPreview(result), 160);
  }, [args, error, isRequest, isResult, result]);

  const colorScheme = isError
    ? "border-red-500/30 bg-red-500/5"
    : isResult
      ? "border-green-500/30 bg-green-500/5"
      : "border-blue-500/30 bg-blue-500/5";

  const iconColor = isError
    ? "text-red-500"
    : isResult
      ? "text-green-500"
      : "text-blue-500";

  return (
    <div
      className={cn(
        "rounded-lg border text-sm transition-colors",
        colorScheme,
        isSelected && "ring-2 ring-primary/25",
        className,
      )}
    >
      <div className="flex items-start gap-2 px-2 py-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setExpanded(!expanded)}
          className="min-w-0 flex-1 justify-start gap-2 px-1 py-1 text-xs hover:bg-transparent"
        >
          <ChevronRight
            className={cn(
              "h-3 w-3 shrink-0 transition-transform duration-200",
              expanded && "rotate-90",
            )}
          />
          {isResult ? (
            isError ? (
              <XCircle className={cn("h-3.5 w-3.5 shrink-0", iconColor)} />
            ) : (
              <CheckCircle className={cn("h-3.5 w-3.5 shrink-0", iconColor)} />
            )
          ) : (
            <Wrench className={cn("h-3.5 w-3.5 shrink-0", iconColor)} />
          )}
          <span className="truncate font-mono font-medium">{toolName}</span>
          {args && <span className="truncate text-muted-foreground">({summarizeArgs(args)})</span>}
          {isResult && (
            <Badge
              variant={isError ? "destructive" : "secondary"}
              className="ml-auto shrink-0 text-[10px]"
            >
              {isError ? "error" : "ok"}
            </Badge>
          )}
        </Button>

        <div className="flex shrink-0 items-center gap-1">
          {durationMs !== null && (
            <span className="hidden items-center gap-0.5 text-[10px] text-muted-foreground sm:inline-flex">
              <Clock className="h-2.5 w-2.5" />
              {durationMs}ms
            </span>
          )}
          {onInspect && (
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              onClick={() => onInspect(entry, pairedEntry)}
              className={cn("rounded-lg", isSelected && "text-primary")}
              aria-label="Inspect tool payload"
            >
              <PanelRightOpen className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </div>

      {preview && !expanded && (
        <div className="border-t border-dashed px-4 py-2 text-xs text-muted-foreground">
          <div className="flex items-start gap-2">
            <FileText className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            <span className="line-clamp-3 whitespace-pre-wrap break-words">{preview}</span>
          </div>
        </div>
      )}

      {expanded && (
        <div className="space-y-2 border-t border-dashed px-4 py-3">
          {isRequest && args && (
            <div>
              <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                Arguments
              </p>
              <pre className="overflow-x-auto rounded-md bg-muted/40 p-2 font-mono text-xs">
                {JSON.stringify(args, null, 2)}
              </pre>
            </div>
          )}

          {isResult && (
            <div>
              {error ? (
                <>
                  <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-red-500">
                    Error
                  </p>
                  <pre className="overflow-x-auto rounded-md bg-red-500/10 p-2 font-mono text-xs text-red-600 dark:text-red-400">
                    {error}
                  </pre>
                </>
              ) : (
                <>
                  <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                    Result
                  </p>
                  <pre className="overflow-x-auto rounded-md bg-muted/40 p-2 font-mono text-xs">
                    {typeof result === "string" ? result : JSON.stringify(result, null, 2)}
                  </pre>
                </>
              )}
            </div>
          )}

          {pairedEntry && (
            <div>
              <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                {isRequest ? "Result" : "Request"}
              </p>
              <pre className="overflow-x-auto rounded-md bg-muted/40 p-2 font-mono text-xs">
                {typeof pairedEntry.payload === "string"
                  ? pairedEntry.payload
                  : JSON.stringify(pairedEntry.payload, null, 2)}
              </pre>
            </div>
          )}

          <p className="text-[10px] text-muted-foreground">
            {new Date(entry.created_at).toLocaleTimeString()}
          </p>
        </div>
      )}
    </div>
  );
}
