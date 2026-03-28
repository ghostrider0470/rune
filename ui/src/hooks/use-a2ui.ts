import { useCallback, useEffect, useMemo, useReducer } from "react";
import type {
  A2uiActionRequest,
  A2uiComponent,
  A2uiFormSubmitRequest,
  A2uiTarget,
  SessionEvent,
} from "@/lib/api-types";
import { buildWebSocketUrl } from "@/lib/websocket";

interface A2uiState {
  inline: A2uiComponent[];
  panel: A2uiComponent[];
}

type ReducerAction =
  | { type: "push"; component: A2uiComponent; target: A2uiTarget }
  | { type: "remove"; componentId: string }
  | { type: "reset" };

function upsert(components: A2uiComponent[], next: A2uiComponent, maxComponents: number) {
  const filtered = components.filter((component) => component.id !== next.id);
  const merged = [...filtered, next];
  return merged.slice(Math.max(merged.length - maxComponents, 0));
}

function reducer(state: A2uiState, action: ReducerAction, maxComponents: number): A2uiState {
  switch (action.type) {
    case "push": {
      const key = action.target === "panel" ? "panel" : "inline";
      return {
        ...state,
        [key]: upsert(state[key], action.component, maxComponents),
      };
    }
    case "remove":
      return {
        inline: state.inline.filter((component) => component.id !== action.componentId),
        panel: state.panel.filter((component) => component.id !== action.componentId),
      };
    case "reset":
      return { inline: [], panel: [] };
    default:
      return state;
  }
}

function payloadAction(payload: unknown): string | null {
  return payload && typeof payload === "object" && typeof (payload as { action?: unknown }).action === "string"
    ? ((payload as { action: string }).action)
    : null;
}

function openRpcSocket(): WebSocket {
  return new WebSocket(buildWebSocketUrl("/ws"));
}

function sendRpc(method: string, params: Record<string, unknown>) {
  return new Promise<void>((resolve, reject) => {
    const ws = openRpcSocket();
    const requestId = typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `rpc-${Date.now()}-${Math.random().toString(16).slice(2)}`;

    const cleanup = () => {
      ws.onopen = null;
      ws.onmessage = null;
      ws.onerror = null;
      ws.onclose = null;
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
        ws.close();
      }
    };

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: "req", id: requestId, method, params }));
    };

    ws.onmessage = (event) => {
      try {
        const message = JSON.parse(event.data) as {
          type?: string;
          id?: string;
          ok?: boolean;
          error?: { message?: string };
        };
        if (message.type !== "res" || message.id !== requestId) {
          return;
        }
        cleanup();
        if (message.ok) {
          resolve();
        } else {
          reject(new Error(message.error?.message ?? `RPC failed: ${method}`));
        }
      } catch (error) {
        cleanup();
        reject(error instanceof Error ? error : new Error("Invalid RPC response"));
      }
    };

    ws.onerror = () => {
      cleanup();
      reject(new Error(`WebSocket RPC failed: ${method}`));
    };

    ws.onclose = () => {
      cleanup();
    };
  });
}

export function useA2ui(events: SessionEvent[], sessionId?: string, maxComponents = 50) {
  const [state, dispatchBase] = useReducer(
    (current: A2uiState, action: ReducerAction) => reducer(current, action, maxComponents),
    { inline: [], panel: [] },
  );

  const dispatch = useCallback((action: ReducerAction) => dispatchBase(action), [dispatchBase]);

  useEffect(() => {
    for (const event of events) {
      if (event.kind !== "a2ui") continue;
      const payload = event.payload;
      const action = payloadAction(payload);
      if (!action || !payload || typeof payload !== "object") continue;

      if (action === "push") {
        const push = payload as {
          component?: A2uiComponent;
          target?: A2uiTarget;
        };
        if (push.component?.id && push.component?.type) {
          dispatch({
            type: "push",
            component: push.component,
            target: push.target === "panel" ? "panel" : "inline",
          });
        }
      } else if (action === "remove") {
        const componentId = (payload as { component_id?: unknown }).component_id;
        if (typeof componentId === "string") {
          dispatch({ type: "remove", componentId });
        }
      } else if (action === "reset") {
        dispatch({ type: "reset" });
      }
    }
  }, [dispatch, events]);

  const submitForm = useCallback(
    async (callbackId: string, data: Record<string, unknown>) => {
      if (!sessionId) throw new Error("No active session selected.");
      const request: A2uiFormSubmitRequest = { session_id: sessionId, callback_id: callbackId, data };
      await sendRpc("a2ui.form_submit", { ...request });
    },
    [sessionId],
  );

  const triggerAction = useCallback(
    async (componentId: string, actionTarget: string) => {
      if (!sessionId) throw new Error("No active session selected.");
      const request: A2uiActionRequest = {
        session_id: sessionId,
        component_id: componentId,
        action_target: actionTarget,
      };
      await sendRpc("a2ui.action", { ...request });
    },
    [sessionId],
  );

  const clear = useCallback(() => dispatch({ type: "reset" }), [dispatch]);

  return useMemo(
    () => ({ state, components: [...state.inline, ...state.panel], submitForm, triggerAction, clear }),
    [clear, state, submitForm, triggerAction],
  );
}
