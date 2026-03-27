import { useCallback, useEffect, useState } from "react";
import type { SessionListItem, SessionResponse } from "../api/api-types";
import { apiFetch } from "../api/client";

export function useSessions() {
  const [sessions, setSessions] = useState<SessionListItem[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const loadSessions = useCallback(async () => {
    setLoading(true);
    try {
      const response = await apiFetch("/v1/sessions");
      if (!response.ok) {
        throw new Error(`Failed to load sessions (${response.status})`);
      }
      const data = (await response.json()) as SessionListItem[];
      setSessions(data);
      setActiveSessionId((current) => current ?? data[0]?.id ?? null);
    } finally {
      setLoading(false);
    }
  }, []);

  const createSession = useCallback(async () => {
    const response = await apiFetch("/v1/sessions", {
      body: JSON.stringify({ kind: "direct" }),
      headers: { "Content-Type": "application/json" },
      method: "POST",
    });

    if (!response.ok) {
      throw new Error(`Failed to create session (${response.status})`);
    }

    const session = (await response.json()) as SessionResponse;
    await loadSessions();
    setActiveSessionId(session.id);
    return session;
  }, [loadSessions]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  return {
    activeSessionId,
    createSession,
    loading,
    sessions,
    setActiveSessionId,
  };
}
