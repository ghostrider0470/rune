import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import {
  Wifi,
  WifiOff,
  PanelLeft,
  MessageSquare,
  RefreshCw,
  Sparkles,
  Bot,
  Activity,
  Clock3,
  PanelRight,
  Focus,
  Brain,
} from "lucide-react";
import {
  useChatSessions,
  useChatSend,
  useChatMergedTranscript,
} from "@/hooks/use-chat";
import { useDeleteSession } from "@/hooks/use-sessions";
import { useA2ui } from "@/hooks/use-a2ui";
import { ChatSidebar } from "@/components/chat/ChatSidebar";
import { ChatThread } from "@/components/chat/ChatThread";
import { ChatInput } from "@/components/chat/ChatInput";
import { A2uiRenderer } from "@/components/a2ui/A2uiRenderer";
import {
  CHAT_FOCUS_MODE_KEY,
  CHAT_SHOW_THINKING_KEY,
  loadChatFocusMode,
  loadChatShowThinking,
} from "@/components/chat/chat-preferences";
import { designSystem } from "@/lib/design-system";
import type { TranscriptEntry } from "@/lib/api-types";

const INSPECTOR_RATIO_KEY = "rune.chat.inspectorRatio";
const DEFAULT_INSPECTOR_RATIO = 0.62;
const MIN_INSPECTOR_RATIO = 0.4;
const MAX_INSPECTOR_RATIO = 0.7;

export const Route = createFileRoute("/_admin/chat")({
  validateSearch: (search: Record<string, unknown>) => ({
    session: typeof search.session === "string" ? search.session : undefined,
  }),
  component: ChatPage,
});

function formatRelativeTime(dateStr?: string | null) {
  if (!dateStr) return "No activity yet";

  const date = new Date(dateStr);
  const diffMs = Date.now() - date.getTime();
  const diffMin = Math.floor(diffMs / 60_000);

  if (diffMin < 1) return "Updated just now";
  if (diffMin < 60) return `Updated ${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `Updated ${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  return `Updated ${diffDay}d ago`;
}

function clampInspectorRatio(value: number) {
  return Math.min(Math.max(value, MIN_INSPECTOR_RATIO), MAX_INSPECTOR_RATIO);
}

function loadInspectorRatio() {
  if (typeof window === "undefined") {
    return DEFAULT_INSPECTOR_RATIO;
  }

  const raw = window.localStorage.getItem(INSPECTOR_RATIO_KEY);
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? clampInspectorRatio(parsed) : DEFAULT_INSPECTOR_RATIO;
}

function ChatPage() {
  const navigate = useNavigate({ from: Route.fullPath });
  const search = Route.useSearch();
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
  const [selectedToolEntry, setSelectedToolEntry] = useState<TranscriptEntry | null>(null);
  const [selectedToolPair, setSelectedToolPair] = useState<TranscriptEntry | null>(null);
  const [inspectorRatio, setInspectorRatio] = useState(loadInspectorRatio);
  const [showMobileInspector, setShowMobileInspector] = useState(false);
  const [focusMode, setFocusMode] = useState(loadChatFocusMode);
  const [showThinking, setShowThinking] = useState(loadChatShowThinking);
  const splitContainerRef = useRef<HTMLDivElement>(null);

  const activeSessionId = search.session;

  const {
    data: sessions,
    isLoading: sessionsLoading,
    createSession,
  } = useChatSessions();

  const {
    entries,
    rawEvents,
    isLoading: transcriptLoading,
    isFetching: transcriptFetching,
    isError: transcriptError,
    connected,
    refetch: refetchTranscript,
  } = useChatMergedTranscript(activeSessionId);

  const { state: a2uiState } = useA2ui(rawEvents);
  const sendMutation = useChatSend(activeSessionId);
  const deleteMutation = useDeleteSession();

  const activeSession = useMemo(
    () => sessions?.find((session) => session.id === activeSessionId),
    [activeSessionId, sessions],
  );

  const activeStats = useMemo(() => {
    const assistantMessages = entries.filter(
      (entry) => entry.kind === "assistant_message" || entry.kind === "assistant",
    ).length;

    const toolEvents = entries.filter((entry) => entry.kind.startsWith("tool_")).length;

    return {
      messageCount: entries.length,
      assistantMessages,
      toolEvents,
    };
  }, [entries]);

  const setActiveSessionId = useCallback(
    (sessionId: string | undefined, replace = false) => {
      navigate({
        search: (prev) => ({ ...prev, session: sessionId }),
        replace,
      });
    },
    [navigate],
  );

  const handleSend = useCallback(
    (message: string, attachments?: File[]) => {
      if (!activeSessionId) return;
      sendMutation.mutate({ content: message, attachments });
    },
    [activeSessionId, sendMutation],
  );

  const handleCreateSession = useCallback(() => {
    createSession.mutate(
      { kind: "interactive" },
      {
        onSuccess: (data) => {
          setActiveSessionId(data.id);
          setMobileDrawerOpen(false);
        },
      },
    );
  }, [createSession, setActiveSessionId]);

  const handleDeleteSession = useCallback(
    (id: string) => {
      deleteMutation.mutate(id, {
        onSuccess: () => {
          if (activeSessionId === id) {
            const remaining = sessions?.filter((s) => s.id !== id);
            setActiveSessionId(remaining?.[0]?.id, true);
          }
        },
      });
    },
    [activeSessionId, deleteMutation, sessions, setActiveSessionId],
  );

  const handleSelectSession = useCallback(
    (id: string) => {
      setActiveSessionId(id);
      setMobileDrawerOpen(false);
      setSelectedToolEntry(null);
      setSelectedToolPair(null);
      setShowMobileInspector(false);
    },
    [setActiveSessionId],
  );

  const handleInspectTool = useCallback((entry: TranscriptEntry, pairedEntry?: TranscriptEntry) => {
    setSelectedToolEntry(entry);
    setSelectedToolPair(pairedEntry ?? null);
    setShowMobileInspector(true);
  }, []);

  const clearInspector = useCallback(() => {
    setSelectedToolEntry(null);
    setSelectedToolPair(null);
    setShowMobileInspector(false);
  }, []);

  useEffect(() => {
    if (sessionsLoading || !sessions?.length) {
      return;
    }

    const hasActiveSelection =
      typeof activeSessionId === "string" &&
      sessions.some((session) => session.id === activeSessionId);

    if (!hasActiveSelection) {
      setActiveSessionId(sessions[0].id, true);
    }
  }, [activeSessionId, sessions, sessionsLoading, setActiveSessionId]);

  useEffect(() => {
    if (!selectedToolEntry) {
      return;
    }

    const exists = entries.some((entry) => entry.id === selectedToolEntry.id);
    if (!exists) {
      const timer = window.setTimeout(() => {
        clearInspector();
      }, 0);
      return () => window.clearTimeout(timer);
    }
  }, [clearInspector, entries, selectedToolEntry]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      clearInspector();
    }, 0);

    return () => window.clearTimeout(timer);
  }, [activeSessionId, clearInspector]);

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(INSPECTOR_RATIO_KEY, String(inspectorRatio));
    }
  }, [inspectorRatio]);

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(CHAT_FOCUS_MODE_KEY, String(focusMode));
    }
  }, [focusMode]);

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(CHAT_SHOW_THINKING_KEY, String(showThinking));
    }
  }, [showThinking]);

  const inspectorOpen = Boolean(selectedToolEntry) || a2uiState.panel.length > 0;

  const handleResizeStart = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const container = splitContainerRef.current;
    if (!container) return;

    event.preventDefault();
    const pointerId = event.pointerId;
    event.currentTarget.setPointerCapture(pointerId);

    const updateRatio = (clientX: number) => {
      const rect = container.getBoundingClientRect();
      const ratio = (clientX - rect.left) / rect.width;
      setInspectorRatio(clampInspectorRatio(ratio));
    };

    updateRatio(event.clientX);

    const handlePointerMove = (moveEvent: PointerEvent) => {
      updateRatio(moveEvent.clientX);
    };

    const handlePointerUp = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: true });
  }, []);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <section className="shrink-0 overflow-hidden rounded-3xl border border-primary/20 bg-gradient-to-br from-background via-background to-primary/5 shadow-[0_24px_80px_rgba(249,115,22,0.10)]">
        <div className="grid gap-3 px-4 py-3 sm:px-6 sm:py-4 lg:grid-cols-[minmax(0,1fr)_360px] lg:px-8 lg:py-5">
          <div className="space-y-3 sm:space-y-4">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="outline" className="border-primary/25 bg-primary/10 text-primary">
                <Sparkles className="mr-1 h-3 w-3" />
                Control Surface
              </Badge>
              <Badge variant="outline" className="gap-1 border-border/70 bg-background/70">
                {connected ? (
                  <>
                    <Wifi className="h-3 w-3 text-green-500" />
                    Live stream
                  </>
                ) : (
                  <>
                    <WifiOff className="h-3 w-3 text-muted-foreground" />
                    Polling fallback
                  </>
                )}
              </Badge>
              {activeSession?.status && (
                <Badge variant="outline" className="gap-1 border-border/70 bg-background/70">
                  <Activity className="h-3 w-3" />
                  {activeSession.status}
                </Badge>
              )}
              {inspectorOpen && (
                <Badge variant="outline" className="gap-1 border-primary/25 bg-primary/10 text-primary">
                  <PanelRight className="h-3 w-3" />
                  Inspector pinned
                </Badge>
              )}
            </div>

            <div className="space-y-1.5 sm:space-y-2">
              <p className={designSystem.typography.display.eyebrow}>Admin Chat</p>
              <div>
                <h1 className="text-xl font-semibold tracking-tight sm:text-3xl">
                  Run sessions like an ops console, not a toy inbox.
                </h1>
                <p className="mt-2 text-sm text-muted-foreground sm:hidden">
                  Keep the active transcript readable, switch sessions quickly, and reply without leaving the thread.
                </p>
                <p className="mt-2 hidden max-w-2xl text-sm text-muted-foreground sm:block sm:text-base">
                  Keep the active transcript, session queue, and runtime signal in one place.
                  Fast switching, live updates, and admin-first context without copying OpenClaw’s layout.
                </p>
              </div>
            </div>
          </div>

          <div className="-mx-1 flex gap-3 overflow-x-auto px-1 pb-1 sm:mx-0 sm:grid sm:px-0 sm:pb-0 lg:grid-cols-1 xl:grid-cols-3">
            <Card className="min-w-[15rem] border-primary/20 bg-background/75 py-0 shadow-none backdrop-blur sm:min-w-0">
              <CardContent className="px-4 py-4">
                <div className="flex items-center gap-3">
                  <div className="rounded-2xl bg-primary/10 p-2 text-primary">
                    <Bot className="h-4 w-4" />
                  </div>
                  <div>
                    <p className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
                      Active session
                    </p>
                    <p className="mt-1 text-sm font-medium">
                      {activeSession ? `${activeSession.id.slice(0, 12)}…` : "No session selected"}
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card className="min-w-[15rem] border-primary/20 bg-background/75 py-0 shadow-none backdrop-blur sm:min-w-0">
              <CardContent className="px-4 py-4">
                <p className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
                  Transcript load
                </p>
                <p className="mt-1 text-2xl font-semibold">{activeStats.messageCount}</p>
                <p className="mt-1 text-xs text-muted-foreground">
                  {activeStats.assistantMessages} assistant replies · {activeStats.toolEvents} tool events
                </p>
              </CardContent>
            </Card>

            <Card className="min-w-[15rem] border-primary/20 bg-background/75 py-0 shadow-none backdrop-blur sm:min-w-0">
              <CardContent className="px-4 py-4">
                <div className="flex items-center gap-3">
                  <div className="rounded-2xl bg-accent/10 p-2 text-accent">
                    <Clock3 className="h-4 w-4" />
                  </div>
                  <div>
                    <p className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
                      Activity
                    </p>
                    <p className="mt-1 text-sm font-medium">
                      {formatRelativeTime(
                        activeSession?.last_activity_at ??
                          activeSession?.updated_at ??
                          activeSession?.created_at ??
                          null,
                      )}
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>
        </div>
      </section>

      <div className="grid min-h-0 flex-1 gap-4 overflow-hidden lg:grid-cols-[320px_minmax(0,1fr)]">
        <div className="space-y-3 lg:hidden">
          <div className="grid grid-cols-1 gap-3 rounded-3xl border border-border/70 bg-card/80 p-3 shadow-[0_18px_50px_rgba(15,23,42,0.08)] backdrop-blur sm:grid-cols-2">
            <div className="flex items-center justify-between rounded-2xl border border-border/70 bg-background/70 px-3 py-2">
              <Label htmlFor="chat-focus-mode-mobile" className="gap-1.5 text-xs text-muted-foreground">
                <Focus className="h-3.5 w-3.5" />
                Focus mode
              </Label>
              <Switch
                id="chat-focus-mode-mobile"
                size="sm"
                checked={focusMode}
                onCheckedChange={setFocusMode}
                aria-label="Toggle focus mode"
              />
            </div>
            <div className="flex items-center justify-between rounded-2xl border border-border/70 bg-background/70 px-3 py-2">
              <Label htmlFor="chat-show-thinking-mobile" className="gap-1.5 text-xs text-muted-foreground">
                <Brain className="h-3.5 w-3.5" />
                Show thinking
              </Label>
              <Switch
                id="chat-show-thinking-mobile"
                size="sm"
                checked={showThinking}
                onCheckedChange={setShowThinking}
                aria-label="Toggle thinking visibility"
              />
            </div>
          </div>
        </div>

        <div className="hidden min-h-0 lg:block">
          <ChatSidebar
            sessions={sessions}
            isLoading={sessionsLoading}
            activeSessionId={activeSessionId}
            onSelectSession={handleSelectSession}
            onDeleteSession={handleDeleteSession}
            onCreateSession={handleCreateSession}
            isCreating={createSession.isPending}
            className="h-full"
          />
        </div>

        <section className="flex h-full min-h-0 flex-col overflow-hidden rounded-3xl border border-border/70 bg-card/80 shadow-[0_20px_60px_rgba(15,23,42,0.10)] backdrop-blur">
          <header className="border-b border-border/70 bg-background/80 px-4 py-3 sm:px-5">
            <div className="flex flex-wrap items-center gap-3">
              <Sheet open={mobileDrawerOpen} onOpenChange={setMobileDrawerOpen}>
                <SheetTrigger asChild className="lg:hidden">
                  <Button variant="outline" size="sm" className="gap-2 rounded-xl">
                    <PanelLeft className="h-4 w-4" />
                    Sessions
                  </Button>
                </SheetTrigger>
                <SheetContent side="left" className="w-[min(100vw,24rem)] border-r p-0">
                  <SheetHeader className="border-b px-4 py-4">
                    <SheetTitle>Session queue</SheetTitle>
                  </SheetHeader>
                  <ChatSidebar
                    sessions={sessions}
                    isLoading={sessionsLoading}
                    activeSessionId={activeSessionId}
                    onSelectSession={handleSelectSession}
            onDeleteSession={handleDeleteSession}
                    onCreateSession={handleCreateSession}
                    isCreating={createSession.isPending}
                    className="border-r-0"
                    compactHeader
                  />
                </SheetContent>
              </Sheet>

              <div className="min-w-0 flex-1">
                {activeSessionId ? (
                  <div className="space-y-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="text-sm font-semibold sm:text-base">Live transcript</h2>
                      <Badge variant="outline" className="max-w-[14rem] font-mono text-[10px] sm:max-w-full">
                        <span className="block truncate">
                          {activeSessionId}
                        </span>
                      </Badge>
                    </div>
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground sm:text-xs">
                      {activeSession?.channel && <span>Channel: {activeSession.channel}</span>}
                      {typeof activeSession?.turn_count === "number" && (
                        <span>{activeSession.turn_count} turns</span>
                      )}
                      {activeSession?.latest_model && (
                        <span className="truncate">Model: {activeSession.latest_model}</span>
                      )}
                    </div>
                  </div>
                ) : (
                  <div>
                    <h2 className="text-sm font-semibold sm:text-base">Live transcript</h2>
                    <p className="text-xs text-muted-foreground">Pick a session to inspect and drive.</p>
                  </div>
                )}
              </div>

              <div className="flex w-full flex-wrap items-center justify-start gap-3 sm:w-auto sm:justify-end">
                <div className="hidden items-center gap-3 rounded-2xl border border-border/70 bg-background/70 px-3 py-2 lg:flex">
                  <Label htmlFor="chat-focus-mode" className="gap-1.5 text-[11px] text-muted-foreground">
                    <Focus className="h-3.5 w-3.5" />
                    Focus mode
                  </Label>
                  <Switch
                    id="chat-focus-mode"
                    size="sm"
                    checked={focusMode}
                    onCheckedChange={setFocusMode}
                    aria-label="Toggle focus mode"
                  />
                  <Label htmlFor="chat-show-thinking" className="gap-1.5 text-[11px] text-muted-foreground">
                    <Brain className="h-3.5 w-3.5" />
                    Show thinking
                  </Label>
                  <Switch
                    id="chat-show-thinking"
                    size="sm"
                    checked={showThinking}
                    onCheckedChange={setShowThinking}
                    aria-label="Toggle thinking visibility"
                  />
                </div>
                {selectedToolEntry && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setShowMobileInspector(true)}
                    className="gap-2 rounded-xl lg:hidden"
                  >
                    <PanelRight className="h-4 w-4" />
                    Inspect tool
                  </Button>
                )}
                {activeSessionId ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void refetchTranscript()}
                    aria-label="Refresh transcript"
                    className="gap-2 rounded-xl"
                  >
                    <RefreshCw className={transcriptFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                    <span className="lg:hidden">Refresh</span>
                  </Button>
                ) : null}
                <Button
                  variant="default"
                  size="sm"
                  onClick={handleCreateSession}
                  disabled={createSession.isPending}
                  className="rounded-xl"
                >
                  {createSession.isPending ? "Creating..." : "New session"}
                </Button>
              </div>
            </div>
          </header>

          {activeSessionId ? (
            transcriptError ? (
              <div className="flex flex-1 items-center justify-center px-6">
                <div className="max-w-sm text-center">
                  <MessageSquare className="mx-auto mb-3 h-10 w-10 text-muted-foreground/40" />
                  <p className="text-sm font-medium">Couldn't load transcript</p>
                  <p className="mt-1 text-xs text-muted-foreground">
                    The session exists, but the transcript request failed. Try refreshing.
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void refetchTranscript()}
                    className="mt-4"
                  >
                    Retry
                  </Button>
                </div>
              </div>
            ) : (
              <>
                <div ref={splitContainerRef} className="hidden min-h-0 flex-1 lg:flex">
                  <div style={{ width: inspectorOpen ? `${inspectorRatio * 100}%` : "100%" }} className="min-w-0">
                    <ChatThread
                      entries={entries}
                      isLoading={transcriptLoading}
                      className="min-h-0 h-full flex-1"
                      onInspectTool={handleInspectTool}
                      selectedToolEntryId={selectedToolEntry?.id ?? null}
                      showThinking={showThinking}
                      focusMode={focusMode}
                    />
                  </div>

                  {inspectorOpen && (
                    <>
                      <div
                        role="separator"
                        aria-orientation="vertical"
                        aria-label="Resize inspector"
                        onPointerDown={handleResizeStart}
                        className="group relative w-3 shrink-0 cursor-col-resize touch-none"
                      >
                        <div className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-border/80 transition-colors group-hover:bg-primary/50" />
                        <div className="absolute left-1/2 top-1/2 h-10 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-border/80 transition-colors group-hover:bg-primary/50" />
                      </div>
                      <div style={{ width: `${(1 - inspectorRatio) * 100}%` }} className="min-w-[280px] max-w-[520px]">
                        {selectedToolEntry ? (
                          <ChatSidebar
                            mode="inspector"
                            selectedToolEntry={selectedToolEntry}
                            selectedToolPair={selectedToolPair}
                            onCloseInspector={clearInspector}
                            className="h-full rounded-none border-y-0 border-r-0 shadow-none"
                          />
                        ) : (
                          <div className="h-full overflow-y-auto border-l border-border/70 p-4">
                            <A2uiRenderer components={a2uiState.panel} />
                          </div>
                        )}
                      </div>
                    </>
                  )}
                </div>

                <div className="min-h-0 flex-1 lg:hidden">
                  <ChatThread
                    entries={entries}
                    isLoading={transcriptLoading}
                    className="min-h-0 h-full flex-1"
                    onInspectTool={handleInspectTool}
                    selectedToolEntryId={selectedToolEntry?.id ?? null}
                    showThinking={showThinking}
                    focusMode={focusMode}
                  />
                </div>
              </>
            )
          ) : (
            <div className="flex flex-1 items-center justify-center px-6 py-10">
              <div className="max-w-md text-center">
                <MessageSquare className="mx-auto mb-3 h-10 w-10 text-muted-foreground/40" />
                <p className="text-sm font-medium text-foreground">No session selected</p>
                <p className="mt-2 text-sm text-muted-foreground">
                  Use the queue to the left to jump between active work, or spin up a fresh interactive session.
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleCreateSession}
                  disabled={createSession.isPending}
                  className="mt-4 rounded-xl"
                >
                  {createSession.isPending ? "Creating..." : "Create session"}
                </Button>
              </div>
            </div>
          )}

          {activeSessionId && a2uiState.inline.length > 0 && (
            <div className="border-t border-border/70 bg-background/80 px-4 py-2">
              <A2uiRenderer components={a2uiState.inline} />
            </div>
          )}

          {activeSessionId && (
            <div className="border-t border-border/70 bg-background/80 p-3 sm:p-4">
              {sendMutation.isError && (
                <div className="mb-3 rounded-2xl border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                  {sendMutation.error instanceof Error
                    ? sendMutation.error.message
                    : "Failed to send message."}
                </div>
              )}
              <ChatInput
                sessionId={activeSessionId}
                onSend={handleSend}
                disabled={sendMutation.isPending}
                placeholder={
                  sendMutation.isPending
                    ? "Waiting for response..."
                    : connected
                      ? "Send an admin message..."
                      : "Send an admin message (live updates offline, polling still active)..."
                }
              />
            </div>
          )}
        </section>
      </div>

      <Sheet open={showMobileInspector && Boolean(selectedToolEntry)} onOpenChange={setShowMobileInspector}>
        <SheetContent side="right" className="w-full max-w-xl p-0 sm:max-w-xl">
          <SheetHeader className="sr-only">
            <SheetTitle>Tool inspector</SheetTitle>
          </SheetHeader>
          <ChatSidebar
            mode="inspector"
            selectedToolEntry={selectedToolEntry}
            selectedToolPair={selectedToolPair}
            onCloseInspector={clearInspector}
            className="h-full rounded-none border-0 shadow-none"
          />
        </SheetContent>
      </Sheet>
    </div>
  );
}
