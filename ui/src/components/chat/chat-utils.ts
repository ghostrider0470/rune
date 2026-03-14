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
    id: entry.id.startsWith("ws-") || entry.id.startsWith("optimistic-") ? null : entry.id,
    seq: entry.seq,
    kind: normalizeTranscriptKind(entry.kind),
    payload: entry.payload,
    turn_id: entry.turn_id,
    created_at: entry.created_at,
  });
}

export function isLiveEntry(entry: TranscriptEntry): boolean {
  return entry.id.startsWith("ws-") || entry.id.startsWith("optimistic-");
}

export function isTranscriptTextualEntry(entry: TranscriptEntry): boolean {
  const kind = normalizeTranscriptKind(entry.kind);
  return kind === "user" || kind === "assistant";
}

export function getSessionPreviewText(entries: TranscriptEntry[], fallback = "No transcript yet"): string {
  const candidate = [...entries]
    .sort((a, b) => b.seq - a.seq)
    .find((entry) => {
      if (!isTranscriptTextualEntry(entry)) return false;
      const text = getPayloadText(entry.payload).replace(/<thinking>[\s\S]*?<\/thinking>/gi, " ").trim();
      return text.length > 0;
    });

  if (!candidate) return fallback;

  const cleaned = getPayloadText(candidate.payload)
    .replace(/<thinking>[\s\S]*?<\/thinking>/gi, " ")
    .replace(/\s+/g, " ")
    .trim();

  return cleaned.length > 0 ? cleaned : fallback;
}

export function truncatePreview(text: string, maxLength = 120): string {
  const compact = text.replace(/\s+/g, " ").trim();
  if (compact.length <= maxLength) return compact;
  return `${compact.slice(0, maxLength - 1)}…`;
}
