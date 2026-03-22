import { useRef, useEffect, useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { ArrowDown, Bot, Hammer, User } from "lucide-react";
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
  onInspectTool?: (entry: TranscriptEntry, pairedEntry?: TranscriptEntry) => void;
  selectedToolEntryId?: string | null;
  showThinking?: boolean;
  focusMode?: boolean;
}

const TOOL_KINDS = new Set(["tool_request", "tool_use", "tool_result"]);
const MESSAGE_KINDS = new Set(["user", "assistant"]);
const INTERMEDIATE_ASSISTANT_KINDS = new Set(["assistant_thought", "assistant_reasoning"]);

interface EntryGroup {
  type: "message" | "tool_pair" | "other" | "date_divider";
  entries: TranscriptEntry[];
  dateLabel?: string;
}

function isEmptyAssistantMessage(entry: TranscriptEntry): boolean {
  if (normalizeTranscriptKind(entry.kind) !== "assistant") {
    return false;
  }

  const text = getPayloadText(entry.payload).trim();
  return text.length === 0 || /^<thinking>[\s\S]*<\/thinking>$/i.test(text);
}

function getToolCallId(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const value = (payload as Record<string, unknown>).tool_call_id;
  return typeof value === "string" ? value : null;
}

function areLikelySameTurn(left: TranscriptEntry, right: TranscriptEntry): boolean {
  const leftToolCallId = getToolCallId(left.payload);
  const rightToolCallId = getToolCallId(right.payload);

  if (leftToolCallId && rightToolCallId) {
    return leftToolCallId === rightToolCallId;
  }

  if (left.turn_id && right.turn_id) {
    return left.turn_id === right.turn_id;
  }

  return Math.abs(left.seq - right.seq) <= 2;
}

function formatDayLabel(value: string): string {
  const date = new Date(value);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const target = new Date(date.getFullYear(), date.getMonth(), date.getDate());

  if (target.getTime() === today.getTime()) return "Today";
  if (target.getTime() === yesterday.getTime()) return "Yesterday";

  return date.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
}

function entryDayKey(entry: TranscriptEntry): string {
  const date = new Date(entry.created_at);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

function groupEntries(entries: TranscriptEntry[]): EntryGroup[] {
  const groups: EntryGroup[] = [];
  let i = 0;
  let lastDayKey: string | null = null;

  while (i < entries.length) {
    const entry = entries[i];
    const dayKey = entryDayKey(entry);

    if (dayKey !== lastDayKey) {
      groups.push({
        type: "date_divider",
        entries: [entry],
        dateLabel: formatDayLabel(entry.created_at),
      });
      lastDayKey = dayKey;
    }

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
      groups.push({ type: "other", entries: [entry] });
      i += 1;
      continue;
    }

    groups.push({ type: "other", entries: [entry] });
    i += 1;
  }

  return groups;
}

function getLaneMeta(entry: TranscriptEntry) {
  const kind = normalizeTranscriptKind(entry.kind);

  if (kind === "user") {
    return {
      label: "Operator",
      icon: <User className="h-3.5 w-3.5" />,
      badgeClass: "border-border/70 bg-background/80 text-foreground",
    };
  }

  if (kind === "assistant") {
    return {
      label: "Rune",
      icon: <Bot className="h-3.5 w-3.5" />,
      badgeClass: "border-primary/25 bg-primary/10 text-primary",
    };
  }

  return {
    label: "Tooling",
    icon: <Hammer className="h-3.5 w-3.5" />,
    badgeClass: "border-border/70 bg-muted/50 text-muted-foreground",
  };
}

const SCROLL_THRESHOLD = 80;

export function ChatThread({
  entries,
  isLoading,
  className,
  onInspectTool,
  selectedToolEntryId,
  showThinking = true,
  focusMode = false,
}: ChatThreadProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [hasNewMessages, setHasNewMessages] = useState(false);
  const prevLengthRef = useRef(entries.length);

  const checkScrollPosition = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
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
        requestAnimationFrame(() => {
          setHasNewMessages(true);
        });
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
          <div key={i} className={cn("flex", i % 2 === 0 ? "justify-start" : "justify-end")}>
            <Skeleton
              className={cn("rounded-3xl", i % 2 === 0 ? "h-24 w-3/4" : "h-14 w-1/2")}
            />
          </div>
        ))}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className={cn("flex flex-1 items-center justify-center p-8", className)}>
        <div className="max-w-sm text-center">
          <Bot className="mx-auto mb-3 h-10 w-10 text-muted-foreground/40" />
          <p className="text-sm font-medium">No messages yet</p>
          <p className="mt-1 text-sm text-muted-foreground">
            Send a message to start the session and the transcript will build here in real time.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className={cn("relative min-h-0 flex-1 overflow-hidden bg-gradient-to-b from-background/40 to-background", className)}>
      <div
        ref={containerRef}
        onScroll={checkScrollPosition}
        className={cn(
          "flex h-full flex-col gap-3 overflow-y-auto py-3 scroll-pb-28 sm:gap-4 sm:py-4 sm:scroll-pb-6",
          focusMode ? "px-3 sm:px-8 lg:px-12" : "px-2.5 sm:px-4",
        )}
      >
        {groups.map((group, gi) => {
          if (group.type === "date_divider") {
            return (
              <div key={`date-${gi}`} className="flex items-center gap-2 px-0.5 py-1 sm:gap-3 sm:px-1">
                <Separator className="flex-1 bg-border/60" />
                <span className="rounded-full border border-border/70 bg-background/85 px-2.5 py-1 text-[9px] font-medium uppercase tracking-[0.16em] text-muted-foreground sm:px-3 sm:text-[10px]">
                  {group.dateLabel}
                </span>
                <Separator className="flex-1 bg-border/60" />
              </div>
            );
          }

          const leadEntry = group.entries[0];
          const lane = getLaneMeta(leadEntry);

          if (group.type === "message") {
            const entry = group.entries[0];
            return (
              <div key={entry.id} className="space-y-1.5 sm:space-y-2">
                <div
                  className={cn(
                    "hidden px-1 sm:flex",
                    normalizeTranscriptKind(entry.kind) === "user" ? "justify-end" : "justify-start",
                  )}
                >
                  <Badge
                    variant="outline"
                    className={cn("gap-1 rounded-full px-2.5 py-1 text-[10px]", lane.badgeClass)}
                  >
                    {lane.icon}
                    {lane.label}
                  </Badge>
                </div>
                <ChatMessage
                  entry={entry}
                  isLive={isLiveEntry(entry)}
                  showThinking={showThinking}
                />
              </div>
            );
          }

          if (group.type === "tool_pair") {
            return (
              <div key={`tool-group-${gi}`} className="space-y-1.5 px-0.5 sm:space-y-2 sm:px-2">
                <Badge
                  variant="outline"
                  className={cn("gap-1 rounded-full px-2.5 py-1 text-[10px]", lane.badgeClass)}
                >
                  {lane.icon}
                  {lane.label}
                </Badge>
                <div className="flex flex-col gap-1">
                  {group.entries.map((entry, ei) => {
                    const pairedEntry =
                      ei === 0 && group.entries.length > 1
                        ? group.entries[1]
                        : ei > 0
                          ? group.entries[0]
                          : undefined;

                    return (
                      <ToolCard
                        key={entry.id}
                        entry={entry}
                        pairedEntry={pairedEntry}
                        onInspect={onInspectTool}
                        isSelected={selectedToolEntryId === entry.id}
                      />
                    );
                  })}
                </div>
              </div>
            );
          }

          return (
            <div key={`other-${gi}`} className="space-y-1.5 sm:space-y-2">
              <Badge
                variant="outline"
                className={cn("gap-1 rounded-full px-2.5 py-1 text-[10px]", lane.badgeClass)}
              >
                {lane.icon}
                {lane.label}
              </Badge>
              {group.entries.map((entry) => (
                <ChatMessage
                  key={entry.id}
                  entry={entry}
                  isLive={isLiveEntry(entry)}
                  showThinking={showThinking}
                />
              ))}
            </div>
          );
        })}
        <div ref={endRef} />
      </div>

      {hasNewMessages && (
        <div className="absolute bottom-3 left-1/2 -translate-x-1/2 sm:bottom-4">
          <Button
            variant="secondary"
            size="sm"
            onClick={scrollToBottom}
            className="gap-1.5 rounded-full border border-primary/20 bg-background/90 shadow-lg backdrop-blur"
          >
            <ArrowDown className="h-3 w-3" />
            New messages
          </Button>
        </div>
      )}
    </div>
  );
}
