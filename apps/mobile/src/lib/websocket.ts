import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import type { MutableRefObject } from "react";
import { getToken } from "./auth";
import { getGatewayUrl } from "../store/app-store";
import type { SessionEvent } from "../api/api-types";

interface UseSessionEventsOptions {
  enabled?: boolean;
  clearOnSessionChange?: boolean;
}

interface EventFramePayload {
  session_id?: unknown;
  kind?: unknown;
  data?: unknown;
  payload?: unknown;
}

interface EventFrame {
  type?: unknown;
  event?: unknown;
  payload?: EventFramePayload;
}

function normalizeBaseUrl(baseUrl: string): URL {
  return new URL(baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`);
}

export async function buildWebSocketUrl(path = "/ws") {
  const baseUrl = getGatewayUrl();
  if (!baseUrl) {
    throw new Error("Gateway base URL is not configured");
  }

  const url = normalizeBaseUrl(baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = path;

  const token = await getToken();
  if (token) {
    url.searchParams.set("token", token);
  }

  return url.toString();
}

function isSessionEvent(value: unknown): value is SessionEvent {
  if (!value || typeof value !== "object") return false;

  const candidate = value as Partial<SessionEvent>;
  return (
    typeof candidate.session_id === "string" &&
    typeof candidate.kind === "string" &&
    "payload" in candidate
  );
}

function isEventFrame(value: unknown): value is EventFrame {
  if (!value || typeof value !== "object") return false;

  const candidate = value as EventFrame;
  return (
    candidate.type === "event" &&
    typeof candidate.event === "string" &&
    !!candidate.payload &&
    typeof candidate.payload === "object"
  );
}

function parseSessionEvent(value: unknown): SessionEvent | null {
  if (isSessionEvent(value)) {
    return value;
  }

  if (!isEventFrame(value)) {
    return null;
  }

  const payload = value.payload;
  if (!payload) {
    return null;
  }

  const sessionId = typeof payload.session_id === "string" ? payload.session_id : null;
  const kind = typeof payload.kind === "string" ? payload.kind : value.event;

  if (!sessionId || typeof kind !== "string") {
    return null;
  }

  return {
    session_id: sessionId,
    kind,
    payload: payload.data ?? payload.payload ?? payload,
  };
}

function closeWebSocket(wsRef: MutableRefObject<WebSocket | null>) {
  wsRef.current?.close();
  wsRef.current = null;
}

function scheduleReconnect(
  connect: () => void,
  reconnectTimerRef: MutableRefObject<ReturnType<typeof setTimeout> | null>,
  retriesRef: MutableRefObject<number>,
) {
  const delay = Math.min(1000 * 2 ** retriesRef.current, 30000);
  retriesRef.current += 1;
  reconnectTimerRef.current = setTimeout(connect, delay);
}

export function useSessionEvents(
  sessionId: string | undefined,
  options?: UseSessionEventsOptions,
) {
  const enabled = options?.enabled ?? !!sessionId;
  const clearOnSessionChange = options?.clearOnSessionChange ?? true;

  const [sessionEvents, setSessionEvents] = useState<Record<string, SessionEvent[]>>({});
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const retriesRef = useRef(0);

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimerRef.current !== null) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
  }, []);

  const clearEvents = useCallback(() => {
    if (!sessionId) return;
    setSessionEvents((prev) => {
      if (!(sessionId in prev)) return prev;
      return { ...prev, [sessionId]: [] };
    });
  }, [sessionId]);

  const shouldSubscribe = useMemo(
    () => enabled && typeof sessionId === "string" && sessionId.length > 0,
    [enabled, sessionId],
  );

  useEffect(() => {
    if (!enabled) {
      clearReconnectTimer();
      closeWebSocket(wsRef);
      return;
    }

    if (clearOnSessionChange && sessionId) {
      setSessionEvents((prev) => {
        if (!(sessionId in prev)) return prev;
        return { ...prev, [sessionId]: [] };
      });
    }

    let disposed = false;

    const connect = async () => {
      if (disposed) return;

      clearReconnectTimer();
      closeWebSocket(wsRef);

      let socketUrl: string;
      try {
        socketUrl = await buildWebSocketUrl("/ws");
      } catch {
        scheduleReconnect(connect, reconnectTimerRef, retriesRef);
        return;
      }

      const ws = new WebSocket(socketUrl);
      wsRef.current = ws;
      setConnected(false);

      ws.onopen = () => {
        if (disposed || wsRef.current !== ws) return;
        setConnected(true);
        retriesRef.current = 0;

        if (shouldSubscribe && sessionId) {
          ws.send(JSON.stringify({ type: "subscribe", session_id: sessionId }));
        }
      };

      ws.onmessage = (e) => {
        if (disposed || wsRef.current !== ws) return;

        try {
          const parsed = JSON.parse(e.data) as unknown;
          const event = parseSessionEvent(parsed);
          if (!event) return;
          if (sessionId && event.session_id !== sessionId) return;
          setSessionEvents((prev) => ({
            ...prev,
            [event.session_id]: [...(prev[event.session_id] ?? []), event],
          }));
        } catch {
          // ignore non-JSON or unsupported frame shapes
        }
      };

      ws.onclose = () => {
        if (wsRef.current === ws) {
          wsRef.current = null;
        }
        setConnected(false);

        if (disposed) return;

        scheduleReconnect(connect, reconnectTimerRef, retriesRef);
      };

      ws.onerror = () => {
        ws.close();
      };
    };

    void connect();

    return () => {
      disposed = true;
      clearReconnectTimer();
      closeWebSocket(wsRef);
    };
  }, [clearOnSessionChange, clearReconnectTimer, enabled, sessionId, shouldSubscribe]);

  const events = sessionId ? (sessionEvents[sessionId] ?? []) : [];

  return { events, connected, clearEvents };
}

export { parseSessionEvent };
