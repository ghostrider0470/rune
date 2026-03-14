import { useRef, useEffect, useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ArrowDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { ChatMessage } from "./ChatMessage";
import { ToolCard } from "./ToolCard";
import {
  getPayloadText,
  isLiveEntry,
  normalizeTranscriptKind,
} from "./chat-utils";
import type { TranscriptEntry } from "@/lib/api-types";

interface ChatThreadProps {
  entries: TranscriptEntry[];
  isLoading?: boolean;
  className?: string;
}

const TOOL_KINDS = new Set(["tool_request", "tool_use", "tool_result"]);
const MESSAGE_KINDS = new Set(["user", "assistant"]);
const INTERMEDIATE_ASSISTANT_KINDS = new Set(["assistant_thought", "assistant_reasoning"]);

interface EntryGroup {
  type: "message" | "tool_pair" | "other";
  entries: TranscriptEntry[];
}

function isEmptyAssistantMessage(entry: TranscriptEntry): boolean {
  return (
    normalizeTranscriptKind(entry.kind) === "assistant" &&
    getPayloadText(entry.payload).trim().length === 0
  );
}

function areLikelySameTurn(left: TranscriptEntry, right: TranscriptEntry): boolean {
  if (left.turn_id && right.turn_id) {
    return left.turn_id === right.turn_id;
  }

  return Math.abs(left.seq - right.seq) <= 2;
}

function groupEntries(entries: TranscriptEntry[]): EntryGroup[] {
  const groups: EntryGroup[] = [];
  let i = 0;

  while (i < entries.length) {
    const entry = entries[i];
    const normalizedKind = normalizeTranscriptKind(entry.kind);

    if (MESSAGE_KINDS.has(normalizedKind)) {
      if (isEmptyAssistantMessage(entry)) {
        i += 1;
        continue;
      }

      groups.push({ type: "message", entries: [entry] });
      i += 1;
      continue;
    }

    if (entry.kind === "tool_request" || entry.kind === "tool_use") {
      const toolGroup: TranscriptEntry[] = [entry];
      let j = i + 1;

      while (j < entries.length) {
        const candidate = entries[j];
        const candidateKind = normalizeTranscriptKind(candidate.kind);

        if (
          INTERMEDIATE_ASSISTANT_KINDS.has(candidate.kind) ||
          (candidateKind === "assistant" && isEmptyAssistantMessage(candidate))
        ) {
          j += 1;
          continue;
        }

        if (candidate.kind === "tool_result" && areLikelySameTurn(entry, candidate)) {
          toolGroup.push(candidate);
          j += 1;
          continue;
        }

        break;
      }

      groups.push({ type: "tool_pair", entries: toolGroup });
      i = j;
      continue;
    }

    if (TOOL_KINDS.has(entry.kind)) {
      groups.push({ type: "tool_pair", entries: [entry] });
      i += 1;
      continue;
    }

    if (INTERMEDIATE_ASSISTANT_KINDS.has(entry.kind)) {
      i += 1;
      continue;
    }

    groups.push({ type: "other", entries: [entry] });
    i += 1;
  }

  return groups;
}

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
            className={cn("flex", i % 2 === 0 ? "justify-start" : "justify-end")}
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
      <div className={cn("flex flex-1 items-center justify-center p-8", className)}>
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
            return (
              <ChatMessage
                key={entry.id}
                entry={entry}
                isLive={isLiveEntry(entry)}
              />
            );
          }

          if (group.type === "tool_pair") {
            return (
              <div key={`tool-group-${gi}`} className="mx-4 flex flex-col gap-1">
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

          return group.entries.map((entry) => (
            <ChatMessage key={entry.id} entry={entry} isLive={isLiveEntry(entry)} />
          ));
        })}
        <div ref={endRef} />
      </div>

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
