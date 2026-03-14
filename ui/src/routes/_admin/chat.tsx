import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useCallback, useEffect, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
} from "lucide-react";
import {
  useChatSessions,
  useChatSend,
  useChatMergedTranscript,
} from "@/hooks/use-chat";
import { ChatSidebar } from "@/components/chat/ChatSidebar";
import { ChatThread } from "@/components/chat/ChatThread";
import { ChatInput } from "@/components/chat/ChatInput";

export const Route = createFileRoute("/_admin/chat")({
  validateSearch: (search: Record<string, unknown>) => ({
    session: typeof search.session === "string" ? search.session : undefined,
  }),
  component: ChatPage,
});

function ChatPage() {
  const navigate = useNavigate({ from: Route.fullPath });
  const search = Route.useSearch();
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);

  const activeSessionId = search.session;

  const {
    data: sessions,
    isLoading: sessionsLoading,
    createSession,
  } = useChatSessions();

  const { entries, isLoading: transcriptLoading, connected } =
    useChatMergedTranscript(activeSessionId);

  const sendMutation = useChatSend(activeSessionId);

  const activeSession = useMemo(
    () => sessions?.find((session) => session.id === activeSessionId),
    [activeSessionId, sessions],
  );

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
    (message: string) => {
      if (!activeSessionId) return;
      sendMutation.mutate({ content: message });
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

  const handleSelectSession = useCallback(
    (id: string) => {
      setActiveSessionId(id);
      setMobileDrawerOpen(false);
    },
    [setActiveSessionId],
  );

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

  return (
    <div className="-mx-4 -mt-6 -mb-24 flex h-[calc(100dvh-4rem)] sm:-mx-6 lg:-mx-8 lg:-mb-8">
      <div className="hidden w-[280px] shrink-0 lg:block">
        <ChatSidebar
          sessions={sessions}
          isLoading={sessionsLoading}
          activeSessionId={activeSessionId}
          onSelectSession={handleSelectSession}
          onCreateSession={handleCreateSession}
          isCreating={createSession.isPending}
        />
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-3 border-b bg-card/50 px-4 py-2.5">
          <Sheet open={mobileDrawerOpen} onOpenChange={setMobileDrawerOpen}>
            <SheetTrigger asChild className="lg:hidden">
              <Button variant="ghost" size="icon-sm">
                <PanelLeft className="h-4 w-4" />
                <span className="sr-only">Toggle sessions</span>
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-[300px] p-0">
              <SheetHeader className="border-b px-4 py-3">
                <SheetTitle>Sessions</SheetTitle>
              </SheetHeader>
              <ChatSidebar
                sessions={sessions}
                isLoading={sessionsLoading}
                activeSessionId={activeSessionId}
                onSelectSession={handleSelectSession}
                onCreateSession={handleCreateSession}
                isCreating={createSession.isPending}
                className="border-r-0"
              />
            </SheetContent>
          </Sheet>

          <div className="flex min-w-0 flex-1 items-center gap-2">
            <MessageSquare className="h-4 w-4 shrink-0 text-muted-foreground" />
            {activeSessionId ? (
              <div className="min-w-0">
                <code className="block truncate text-xs text-muted-foreground">
                  {activeSessionId}
                </code>
                <div className="mt-0.5 flex items-center gap-2 text-[10px] text-muted-foreground">
                  {activeSession?.channel && <span>{activeSession.channel}</span>}
                  {typeof activeSession?.turn_count === "number" && (
                    <span>{activeSession.turn_count} turns</span>
                  )}
                  {activeSession?.latest_model && (
                    <span className="truncate">{activeSession.latest_model}</span>
                  )}
                </div>
              </div>
            ) : (
              <span className="text-xs text-muted-foreground">
                Select a session
              </span>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            {activeSessionId ? (
              connected ? (
                <>
                  <Wifi className="h-3 w-3 text-green-500" />
                  <Badge variant="outline" className="text-[10px] text-green-600">
                    Live
                  </Badge>
                </>
              ) : (
                <>
                  <WifiOff className="h-3 w-3 text-muted-foreground" />
                  <Badge variant="outline" className="text-[10px]">
                    Offline
                  </Badge>
                </>
              )
            ) : null}
          </div>
        </header>

        {activeSessionId ? (
          <ChatThread
            entries={entries}
            isLoading={transcriptLoading}
            className="min-h-0 flex-1"
          />
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <div className="text-center">
              <MessageSquare className="mx-auto mb-3 h-10 w-10 text-muted-foreground/40" />
              <p className="text-sm font-medium text-muted-foreground">
                No session selected
              </p>
              <p className="mt-1 text-xs text-muted-foreground">
                Choose a session from the sidebar or create a new one.
              </p>
              <Button
                variant="outline"
                size="sm"
                onClick={handleCreateSession}
                disabled={createSession.isPending}
                className="mt-4"
              >
                {createSession.isPending ? "Creating..." : "New Session"}
              </Button>
            </div>
          </div>
        )}

        {activeSessionId && (
          <div className="border-t bg-card/30 p-3">
            <ChatInput
              onSend={handleSend}
              disabled={sendMutation.isPending}
              placeholder={
                sendMutation.isPending
                  ? "Waiting for response..."
                  : "Send a message..."
              }
            />
          </div>
        )}
      </div>
    </div>
  );
}
