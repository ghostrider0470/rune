import { useMemo } from "react";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import { cn } from "@/lib/utils";

interface MarkdownRendererProps {
  content: string;
  className?: string;
}

const marked = new Marked({
  breaks: true,
  gfm: true,
});

marked.use({
  renderer: {
    link({ href, title, tokens }) {
      const text = this.parser.parseInline(tokens);
      const safeHref = href || "#";
      const titleAttr = title ? ` title="${title}"` : "";
      return `<a href="${safeHref}" target="_blank" rel="noreferrer noopener"${titleAttr}>${text}</a>`;
    },
  },
});

export function MarkdownRenderer({ content, className }: MarkdownRendererProps) {
  const html = useMemo(() => {
    const raw = marked.parse(content);
    const rawStr = typeof raw === "string" ? raw : "";
    return DOMPurify.sanitize(rawStr, {
      ADD_ATTR: ["target", "rel"],
    });
  }, [content]);

  return (
    <div
      className={cn(
        // Prose-like styling without @tailwindcss/typography
        "markdown-body text-sm leading-relaxed",
        // Headings
        "[&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:text-xl [&_h1]:font-bold",
        "[&_h2]:mt-3 [&_h2]:mb-2 [&_h2]:text-lg [&_h2]:font-semibold",
        "[&_h3]:mt-3 [&_h3]:mb-1 [&_h3]:text-base [&_h3]:font-semibold",
        "[&_h4]:mt-2 [&_h4]:mb-1 [&_h4]:text-sm [&_h4]:font-semibold",
        // Paragraphs
        "[&_p]:my-2 [&_p]:leading-relaxed",
        "[&_p:first-child]:mt-0 [&_p:last-child]:mb-0",
        // Lists
        "[&_ul]:my-2 [&_ul]:ml-4 [&_ul]:list-disc [&_ul]:space-y-1",
        "[&_ol]:my-2 [&_ol]:ml-4 [&_ol]:list-decimal [&_ol]:space-y-1",
        "[&_li]:leading-relaxed",
        // Blockquotes
        "[&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-muted-foreground/40 [&_blockquote]:pl-4 [&_blockquote]:italic [&_blockquote]:text-muted-foreground",
        // Horizontal rules
        "[&_hr]:my-4 [&_hr]:border-border",
        // Strong / em
        "[&_strong]:font-semibold",
        "[&_em]:italic",
        className,
      )}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
