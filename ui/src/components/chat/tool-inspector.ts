import { getPayloadText } from "./chat-utils";
import type { TranscriptEntry } from "@/lib/api-types";

export interface ToolInspectorData {
  toolName: string;
  callId: string | null;
  args: string | null;
  result: string | null;
  error: string | null;
  durationMs: number | null;
  summary: string | null;
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

function stringifyBlock(value: unknown): string | null {
  if (value == null) return null;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function getToolName(payload: unknown): string {
  if (!payload || typeof payload !== "object") return "tool";
  const p = payload as ToolRequestPayload;
  return p.tool_name ?? p.tool_name_human ?? p.name ?? "tool";
}

function getToolCallId(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const value = (payload as ToolRequestPayload).tool_call_id;
  return typeof value === "string" ? value : null;
}

function getToolArgs(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as ToolRequestPayload;
  return stringifyBlock(p.arguments ?? p.args ?? p.input ?? null);
}

function getToolResult(payload: unknown) {
  if (!payload || typeof payload !== "object") {
    return {
      result: stringifyBlock(payload),
      error: null,
      durationMs: null,
      isError: false,
    };
  }

  const p = payload as ToolResultPayload;
  return {
    result: stringifyBlock(p.result ?? p.output ?? payload),
    error: p.error ?? null,
    durationMs: p.duration_ms ?? p.latency_ms ?? null,
    isError: Boolean(p.is_error) || typeof p.error === "string",
  };
}

export function buildToolInspectorData(
  entry: TranscriptEntry,
  pairedEntry?: TranscriptEntry,
): ToolInspectorData {
  const isRequest = entry.kind === "tool_request" || entry.kind === "tool_use";
  const isResult = entry.kind === "tool_result";

  const ownName = getToolName(entry.payload);
  const pairedName = pairedEntry ? getToolName(pairedEntry.payload) : "tool";
  const ownCallId = getToolCallId(entry.payload);
  const pairedCallId = pairedEntry ? getToolCallId(pairedEntry.payload) : null;

  const ownResult = isResult ? getToolResult(entry.payload) : null;
  const pairedResult = pairedEntry?.kind === "tool_result" ? getToolResult(pairedEntry.payload) : null;

  const args = isRequest
    ? getToolArgs(entry.payload)
    : pairedEntry && (pairedEntry.kind === "tool_request" || pairedEntry.kind === "tool_use")
      ? getToolArgs(pairedEntry.payload)
      : null;

  const result = isResult
    ? ownResult?.result ?? null
    : pairedResult?.result ?? null;

  const error = isResult ? ownResult?.error ?? null : pairedResult?.error ?? null;
  const durationMs = isResult ? ownResult?.durationMs ?? null : pairedResult?.durationMs ?? null;

  return {
    toolName: isRequest || ownName !== "tool" ? ownName : pairedName,
    callId: ownCallId ?? pairedCallId,
    args,
    result,
    error,
    durationMs,
    summary: getPayloadText(entry.payload),
  };
}
