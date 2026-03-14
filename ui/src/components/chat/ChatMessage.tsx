import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { ThinkingBlock } from "./ThinkingBlock";
import { extractThinkingBlocks } from "./thinking-utils";
import { CopyMarkdown } from "./CopyMarkdown";
import { getPayloadText, normalizeTranscriptKind } from "./chat-utils";
import type { PendingAttachment, TranscriptEntry } from "@/lib/api-types";

interface ChatMessageProps {
  entry: TranscriptEntry;
  isLive?: boolean;
}

function getMessageAttachments(payload: unknown): PendingAttachment[] {
  if (!payload || typeof payload !== "object") return [];
  const attachments = (payload as { attachments?: unknown }).attachments;
  if (!Array.isArray(attachments)) return [];

  return attachments.filter((item): item is PendingAttachment => {
    if (!item || typeof item !== "object") return false;
    return typeof (item as PendingAttachment).name === "string";
  });
}

export function ChatMessage({ entry, isLive }: ChatMessageProps) {
  const normalizedKind = normalizeTranscriptKind(entry.kind);
  const isUser = normalizedKind === "user";
  const isAssistant = normalizedKind === "assistant";
  const rawText = getPayloadText(entry.payload);
  const attachments = getMessageAttachments(entry.payload);

  const { thinking, cleaned } = isAssistant
    ? extractThinkingBlocks(rawText)
    : { thinking: [], cleaned: rawText };

  return (
    <div
      className={cn(
        "group relative flex flex-col gap-1",
        isUser ? "items-end" : "items-start",
      )}
    >
      <div
        className={cn(
          "relative max-w-[90%] overflow-hidden rounded-[1.6rem] px-4 py-3 text-sm shadow-sm transition-colors sm:max-w-[85%]",
          isUser &&
            "border border-primary/20 bg-gradient-to-br from-primary/14 via-primary/10 to-accent/10 text-foreground",
          isAssistant &&
            "border border-border/70 bg-card/95 text-card-foreground shadow-[0_12px_32px_rgba(15,23,42,0.08)]",
          !isUser && !isAssistant && "border border-dashed bg-muted/30 text-xs",
          isLive && "ring-1 ring-primary/20",
        )}
      >
        {!isUser && !isAssistant && (
          <Badge variant="outline" className="mb-2 text-[10px]">
            {normalizedKind}
          </Badge>
        )}

        {thinking.map((block, i) => (
          <ThinkingBlock key={i} content={block} className="mb-2" />
        ))}

        {cleaned.trim().length > 0 ? (
          isAssistant ? (
            <MarkdownRenderer content={cleaned} />
          ) : (
            <div className="whitespace-pre-wrap">{cleaned}</div>
          )
        ) : thinking.length > 0 ? (
          <p className="text-xs italic text-muted-foreground">
            Response only contained hidden thinking.
          </p>
        ) : attachments.length > 0 ? null : (
          <p className="text-xs italic text-muted-foreground">Empty message payload.</p>
        )}

        {attachments.length > 0 && (
          <div className="mt-3 space-y-2 rounded-2xl border border-border/60 bg-background/60 p-3">
            <p className="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
              Attachments queued
            </p>
            <div className="space-y-2">
              {attachments.map((attachment, index) => (
                <div
                  key={`${attachment.name}-${index}`}
                  className="flex items-center justify-between gap-3 rounded-xl border border-border/60 bg-background/80 px-3 py-2 text-xs"
                >
                  <div className="min-w-0">
                    <p className="truncate font-medium text-foreground">{attachment.name}</p>
                    <p className="truncate text-muted-foreground">
                      {attachment.mime_type ?? "image"}
                      {typeof attachment.size_bytes === "number"
                        ? ` · ${attachment.size_bytes} bytes`
                        : ""}
                    </p>
                  </div>
                  <Badge variant="outline" className="shrink-0 text-[10px]">
                    metadata only
                  </Badge>
                </div>
              ))}
            </div>
          </div>
        )}

        {(isUser || isAssistant) && rawText.length > 0 && (
          <div
            className={cn(
              "absolute -top-2 opacity-0 transition-opacity group-hover:opacity-100",
              isUser ? "left-1" : "right-1",
            )}
          >
            <CopyMarkdown content={rawText} />
          </div>
        )}
      </div>

      <span
        className={cn(
          "px-2 text-[10px] text-muted-foreground",
          isUser ? "text-right" : "text-left",
        )}
      >
        {new Date(entry.created_at).toLocaleTimeString()}
        {isLive && (
          <Badge variant="outline" className="ml-1.5 text-[9px]">
            live
          </Badge>
        )}
      </span>
    </div>
  );
}
