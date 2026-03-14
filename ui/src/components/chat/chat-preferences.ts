export const CHAT_FOCUS_MODE_KEY = "rune.chat.focusMode";
export const CHAT_SHOW_THINKING_KEY = "rune.chat.showThinking";

function readStoredBoolean(key: string, fallback: boolean): boolean {
  if (typeof window === "undefined") {
    return fallback;
  }

  const raw = window.localStorage.getItem(key);
  if (raw === "true") return true;
  if (raw === "false") return false;
  return fallback;
}

export function loadChatFocusMode() {
  return readStoredBoolean(CHAT_FOCUS_MODE_KEY, false);
}

export function loadChatShowThinking() {
  return readStoredBoolean(CHAT_SHOW_THINKING_KEY, true);
}
