import { useRef, useEffect, useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ArrowDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { ChatMessage } from "./ChatMessage";
import { ToolCard } from "./ToolCard";
import { isLiveEntry, normalizeTranscriptKind } from "./chat-utils";
import type { TranscriptEntry } from "@/lib/api-types";

interface ChatThreadProps {
  entries: TranscriptEntry[];
  isLoading?: boolean;
  className?: string;
}

const TOOL_KINDS = new Set([
  "tool_request",
  "tool_use",
  "tool_result",
]);

const MESSAGE_KINDS = new Set(["user", "assistant"]);

/** Group consecutive tool_request + tool_result pairs by turn_id or adjacency */
interface EntryGroup {
  type: "message" | "tool_pair" | "other";
  entries: TranscriptEntry[];
}

function groupEntries(entries: TranscriptEntry[]): EntryGroup[] {
  const groups: EntryGroup[] = [];
  let i = 0;

  while (i < entries.length) {
    const entry = entries[i];

    const normalizedKind = normalizeTranscriptKind(entry.kind);

    if (MESSAGE_KINDS.has(normalizedKind)) {
      groups.push({ type: "message", entries: [entry] });
      i++;
      continue;
    }

    // Group tool_request followed by tool_result(s)
    if (entry.kind === "tool_request" || entry.kind === "tool_use") {
      const toolGroup: TranscriptEntry[] = [entry];
      let j = i + 1;
      // Collect adjacent tool_result entries
      while (j < entries.length && entries[j].kind === "tool_result") {
        toolGroup.push(entries[j]);
        j++;
      }
      groups.push({ type: "tool_pair", entries: toolGroup });
      i = j;
      continue;
    }

    if (TOOL_KINDS.has(entry.kind)) {
      groups.push({ type: "tool_pair", entries: [entry] });
      i++;
      continue;
    }

    // Any other kind (system, error, etc.)
    groups.push({ type: "other", entries: [entry] });
    i++;
  }

  return groups;
}

// Threshold in pixels for considering "scrolled to bottom"
const SCROLL_THRESHOLD = 80;

export function ChatThread({ entries, isLoading, className }: ChatThreadProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [hasNewMessages, setHasNewMessages] = useState(false);
  const prevLengthRef = useRef(entries.length);

  const checkScrollPosition = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const atBottom =
      el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
    setIsAtBottom(atBottom);
    if (atBottom) setHasNewMessages(false);
  }, []);

  // Auto-scroll when new messages arrive and user is at bottom
  useEffect(() => {
    if (entries.length > prevLengthRef.current) {
      if (isAtBottom) {
        requestAnimationFrame(() => {
          endRef.current?.scrollIntoView({ behavior: "smooth" });
        });
      } else {
        setHasNewMessages(true);
      }
    }
    prevLengthRef.current = entries.length;
  }, [entries.length, isAtBottom]);

  const scrollToBottom = useCallback(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
    setHasNewMessages(false);
  }, []);

  const groups = groupEntries(entries);

  if (isLoading) {
    return (
      <div className={cn("space-y-4 p-4", className)}>
        {Array.from({ length: 4 }).map((_, i) => (
          <div
            key={i}
            className={cn(
              "flex",
              i % 2 === 0 ? "justify-start" : "justify-end",
            )}
          >
            <Skeleton
              className={cn(
                "rounded-2xl",
                i % 2 === 0 ? "h-20 w-3/4" : "h-12 w-1/2",
              )}
            />
          </div>
        ))}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div
        className={cn(
          "flex flex-1 items-center justify-center p-8",
          className,
        )}
      >
        <p className="text-sm text-muted-foreground">
          No messages yet. Send a message to start the conversation.
        </p>
      </div>
    );
  }

  return (
    <div className={cn("relative flex-1", className)}>
      <div
        ref={containerRef}
        onScroll={checkScrollPosition}
        className="flex h-full flex-col gap-3 overflow-y-auto p-4"
      >
        {groups.map((group, gi) => {
          if (group.type === "message") {
            const entry = group.entries[0];
            return <ChatMessage key={entry.id} entry={entry} isLive={isLiveEntry(entry)} />;
          }

          if (group.type === "tool_pair") {
            return (
              <div
                key={`tool-group-${gi}`}
                className="mx-4 flex flex-col gap-1"
              >
                {group.entries.map((entry, ei) => (
                  <ToolCard
                    key={entry.id}
                    entry={entry}
                    pairedEntry={
                      ei === 0 && group.entries.length > 1
                        ? group.entries[1]
                        : ei > 0
                          ? group.entries[0]
                          : undefined
                    }
                  />
                ))}
              </div>
            );
          }

          // "other" type: render as a generic ChatMessage
          return group.entries.map((entry) => (
            <ChatMessage key={entry.id} entry={entry} isLive={isLiveEntry(entry)} />
          ));
        })}
        <div ref={endRef} />
      </div>

      {/* New messages indicator */}
      {hasNewMessages && (
        <div className="absolute bottom-4 left-1/2 -translate-x-1/2">
          <Button
            variant="secondary"
            size="sm"
            onClick={scrollToBottom}
            className="gap-1.5 rounded-full shadow-lg"
          >
            <ArrowDown className="h-3 w-3" />
            New messages
          </Button>
        </div>
      )}
    </div>
  );
}
