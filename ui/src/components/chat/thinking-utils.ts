export function extractThinkingBlocks(
  text: string,
): { thinking: string[]; cleaned: string } {
  const thinking: string[] = [];
  const cleaned = text.replace(
    /<thinking>([\s\S]*?)<\/thinking>/gi,
    (_match, inner: string) => {
      thinking.push(inner.trim());
      return "";
    },
  );
  return { thinking, cleaned: cleaned.trim() };
}
