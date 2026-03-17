import { useReducer, useMemo } from "react";
import type { A2uiComponent, A2uiTarget, SessionEvent } from "@/lib/api-types";

interface A2uiState {
  inline: A2uiComponent[];
  panel: A2uiComponent[];
}

type A2uiAction =
  | { type: "push"; component: A2uiComponent; target: A2uiTarget }
  | { type: "remove"; component_id: string }
  | { type: "reset" };

function a2uiReducer(state: A2uiState, action: A2uiAction): A2uiState {
  switch (action.type) {
    case "push": {
      const list = action.target === "panel" ? "panel" : "inline";
      const filtered = state[list].filter((c) => c.id !== action.component.id);
      return { ...state, [list]: [...filtered, action.component] };
    }
    case "remove":
      return {
        inline: state.inline.filter((c) => c.id !== action.component_id),
        panel: state.panel.filter((c) => c.id !== action.component_id),
      };
    case "reset":
      return { inline: [], panel: [] };
    default:
      return state;
  }
}

export function useA2ui(events: SessionEvent[]) {
  const [state, dispatch] = useReducer(a2uiReducer, {
    inline: [],
    panel: [],
  });

  // Process any a2ui events from the stream.
  useMemo(() => {
    for (const event of events) {
      if (event.kind !== "a2ui") continue;

      const payload = event.payload as Record<string, unknown> | null;
      if (!payload || typeof payload.action !== "string") continue;

      switch (payload.action) {
        case "push": {
          const component = payload.component as A2uiComponent | undefined;
          const target = (payload.target as A2uiTarget) || "inline";
          if (component && typeof component.id === "string" && typeof component.type === "string") {
            dispatch({ type: "push", component, target });
          }
          break;
        }
        case "remove": {
          const componentId = payload.component_id as string | undefined;
          if (componentId) {
            dispatch({ type: "remove", component_id: componentId });
          }
          break;
        }
        case "reset":
          dispatch({ type: "reset" });
          break;
      }
    }
  }, [events]);

  return { state };
}
