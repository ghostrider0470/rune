import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { ThinkingBlock, extractThinkingBlocks } from "./ThinkingBlock";
import { CopyMarkdown } from "./CopyMarkdown";
import { getPayloadText, normalizeTranscriptKind } from "./chat-utils";
import type { TranscriptEntry } from "@/lib/api-types";

interface ChatMessageProps {
  entry: TranscriptEntry;
  isLive?: boolean;
}

export function ChatMessage({ entry, isLive }: ChatMessageProps) {
  const normalizedKind = normalizeTranscriptKind(entry.kind);
  const isUser = normalizedKind === "user";
  const isAssistant = normalizedKind === "assistant";
  const rawText = getPayloadText(entry.payload);

  // For assistant messages, extract thinking blocks
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
      {/* Message bubble */}
      <div
        className={cn(
          "relative max-w-[85%] rounded-2xl px-4 py-3 text-sm",
          isUser && "bg-primary/10 text-foreground",
          isAssistant && "border bg-card text-card-foreground shadow-sm",
          !isUser && !isAssistant && "border border-dashed bg-muted/30 text-xs",
          isLive && "ring-1 ring-primary/20",
        )}
      >
        {/* Kind badge for non-user/assistant entries */}
        {!isUser && !isAssistant && (
          <Badge variant="outline" className="mb-2 text-[10px]">
            {normalizedKind}
          </Badge>
        )}

        {/* Thinking blocks (assistant only) */}
        {thinking.map((block, i) => (
          <ThinkingBlock key={i} content={block} className="mb-2" />
        ))}

        {/* Message content */}
        {isAssistant ? (
          <MarkdownRenderer content={cleaned} />
        ) : (
          <div className="whitespace-pre-wrap">{cleaned}</div>
        )}

        {/* Copy button – visible on hover */}
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

      {/* Timestamp */}
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
