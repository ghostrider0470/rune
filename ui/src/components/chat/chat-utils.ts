import type { TranscriptEntry } from "@/lib/api-types";

export function normalizeTranscriptKind(kind: string): string {
  switch (kind) {
    case "user_message":
      return "user";
    case "assistant_message":
      return "assistant";
    default:
      return kind;
  }
}

export function getPayloadText(payload: unknown): string {
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    const p = payload as Record<string, unknown>;
    if (typeof p.content === "string") return p.content;
    if (typeof p.text === "string") return p.text;
    if (typeof p.message === "string") return p.message;
    if (typeof p.error === "string") return p.error;
    return JSON.stringify(payload, null, 2);
  }
  return String(payload ?? "");
}

export function getEntrySignature(entry: TranscriptEntry): string {
  return JSON.stringify({
    kind: normalizeTranscriptKind(entry.kind),
    payload: entry.payload,
    turn_id: entry.turn_id,
  });
}

export function isLiveEntry(entry: TranscriptEntry): boolean {
  return entry.id.startsWith("ws-") || entry.id.startsWith("optimistic-");
}
