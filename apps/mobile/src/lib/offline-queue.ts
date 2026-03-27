import AsyncStorage from "@react-native-async-storage/async-storage";

export interface QueuedChatMessage {
  id: string;
  sessionId: string;
  content: string;
  createdAt: string;
}

const STORAGE_KEY = "rune.mobile.offline-chat-queue";

function normalizeQueue(value: unknown): QueuedChatMessage[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const candidate = item as Partial<QueuedChatMessage>;
    if (
      typeof candidate.id !== "string" ||
      typeof candidate.sessionId !== "string" ||
      typeof candidate.content !== "string" ||
      typeof candidate.createdAt !== "string"
    ) {
      return [];
    }

    return [{
      id: candidate.id,
      sessionId: candidate.sessionId,
      content: candidate.content,
      createdAt: candidate.createdAt,
    }];
  });
}

async function readQueue(): Promise<QueuedChatMessage[]> {
  const raw = await AsyncStorage.getItem(STORAGE_KEY);
  if (!raw) return [];

  try {
    return normalizeQueue(JSON.parse(raw));
  } catch {
    return [];
  }
}

async function writeQueue(queue: QueuedChatMessage[]): Promise<void> {
  await AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(queue));
}

function createQueueId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

export async function enqueueChatMessage(sessionId: string, content: string): Promise<QueuedChatMessage> {
  const entry: QueuedChatMessage = {
    id: createQueueId(),
    sessionId,
    content,
    createdAt: new Date().toISOString(),
  };

  const queue = await readQueue();
  queue.push(entry);
  await writeQueue(queue);
  return entry;
}

export async function getQueuedChatMessages(): Promise<QueuedChatMessage[]> {
  return readQueue();
}

export async function removeQueuedChatMessage(id: string): Promise<void> {
  const queue = await readQueue();
  const nextQueue = queue.filter((item) => item.id !== id);
  if (nextQueue.length === queue.length) return;
  await writeQueue(nextQueue);
}

export async function getQueuedMessageCount(sessionId?: string | null): Promise<number> {
  const queue = await readQueue();
  if (!sessionId) return queue.length;
  return queue.filter((item) => item.sessionId === sessionId).length;
}
