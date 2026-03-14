import { useMemo } from "react";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import { cn } from "@/lib/utils";

interface MarkdownRendererProps {
  content: string;
  className?: string;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function sanitizeUrl(rawHref: string | null | undefined, allowedProtocols?: string[]): string | null {
  if (!rawHref) {
    return null;
  }

  const trimmed = rawHref.trim();
  if (!trimmed) {
    return null;
  }

  if (trimmed.startsWith("#")) {
    return trimmed;
  }

  try {
    const url = new URL(trimmed, window.location.origin);
    if (allowedProtocols && !allowedProtocols.includes(url.protocol)) {
      return null;
    }

    if (url.origin === window.location.origin && /^\/(?!\/)/.test(trimmed)) {
      return `${url.pathname}${url.search}${url.hash}`;
    }

    return url.toString();
  } catch {
    return null;
  }
}

function renderTableCell(
  parser: Marked["Renderer"]["prototype"]["parser"],
  cell: { tokens: unknown[]; header: boolean; align: "center" | "left" | "right" | null },
): string {
  const tag = cell.header ? "th" : "td";
  const alignClass =
    cell.align === "right"
      ? " text-right"
      : cell.align === "center"
        ? " text-center"
        : " text-left";

  return `<${tag} class="border-b border-border/70 px-3 py-2 align-top${alignClass}">${parser.parseInline(cell.tokens as never[])}</${tag}>`;
}

const marked = new Marked({
  breaks: true,
  gfm: true,
});

marked.use({
  renderer: {
    link({ href, title, tokens }) {
      const safeHref = sanitizeUrl(href, ["http:", "https:", "mailto:", "tel:"]);
      const text = this.parser.parseInline(tokens);

      if (!safeHref) {
        return `<span class="text-muted-foreground">${text}</span>`;
      }

      const titleAttr = title ? ` title="${escapeHtml(title)}"` : "";
      return `<a href="${escapeHtml(safeHref)}" target="_blank" rel="noreferrer noopener"${titleAttr}>${text}</a>`;
    },
    image({ href, title, text, tokens }) {
      const altText = tokens
        ? (this.parser.parseInline(tokens, this.parser.textRenderer) as string)
        : text;
      const safeSrc = sanitizeUrl(href, ["http:", "https:"]);

      if (!safeSrc) {
        return `<span class="text-xs text-muted-foreground">[image omitted: unsupported source]</span>`;
      }

      const titleMarkup = title
        ? `<figcaption class="mt-2 text-xs text-muted-foreground">${escapeHtml(title)}</figcaption>`
        : "";

      return `
        <figure class="my-4">
          <img
            src="${escapeHtml(safeSrc)}"
            alt="${escapeHtml(altText || "Image") }"
            loading="lazy"
            decoding="async"
          />
          ${titleMarkup}
        </figure>
      `;
    },
    table(token) {
      const header = token.header
        .map((cell) => renderTableCell(this.parser, cell))
        .join("");

      const rows = token.rows
        .map(
          (row) =>
            `<tr class="border-b border-border/50 last:border-b-0">${row
              .map((cell) => renderTableCell(this.parser, cell))
              .join("")}</tr>`,
        )
        .join("");

      return `
        <div class="my-4 overflow-x-auto rounded-2xl border border-border/70 bg-background/80">
          <table class="min-w-full border-collapse text-sm">
            <thead class="bg-muted/60">
              <tr>${header}</tr>
            </thead>
            <tbody>${rows}</tbody>
          </table>
        </div>
      `;
    },
  },
});

export function MarkdownRenderer({ content, className }: MarkdownRendererProps) {
  const html = useMemo(() => {
    const raw = marked.parse(content);
    const rawStr = typeof raw === "string" ? raw : "";
    return DOMPurify.sanitize(rawStr, {
      ADD_ATTR: ["target", "rel", "loading", "decoding", "class", "align"],
    });
  }, [content]);

  return (
    <div
      className={cn(
        "markdown-body text-sm leading-relaxed",
        "[&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:text-xl [&_h1]:font-bold",
        "[&_h2]:mt-3 [&_h2]:mb-2 [&_h2]:text-lg [&_h2]:font-semibold",
        "[&_h3]:mt-3 [&_h3]:mb-1 [&_h3]:text-base [&_h3]:font-semibold",
        "[&_h4]:mt-2 [&_h4]:mb-1 [&_h4]:text-sm [&_h4]:font-semibold",
        "[&_p]:my-2 [&_p]:leading-relaxed",
        "[&_p:first-child]:mt-0 [&_p:last-child]:mb-0",
        "[&_ul]:my-2 [&_ul]:ml-4 [&_ul]:list-disc [&_ul]:space-y-1",
        "[&_ol]:my-2 [&_ol]:ml-4 [&_ol]:list-decimal [&_ol]:space-y-1",
        "[&_li]:leading-relaxed",
        "[&_li>p]:my-1",
        "[&_blockquote]:my-3 [&_blockquote]:rounded-r-2xl [&_blockquote]:border-l-2 [&_blockquote]:border-primary/35 [&_blockquote]:bg-muted/30 [&_blockquote]:pl-4 [&_blockquote]:pr-3 [&_blockquote]:py-2 [&_blockquote]:italic [&_blockquote]:text-muted-foreground",
        "[&_hr]:my-4 [&_hr]:border-border",
        "[&_strong]:font-semibold",
        "[&_em]:italic",
        "[&_a]:break-words [&_a]:text-primary [&_a]:underline [&_a]:underline-offset-4 hover:[&_a]:text-primary/80",
        "[&_pre]:my-4 [&_pre]:overflow-x-auto [&_pre]:rounded-2xl [&_pre]:border [&_pre]:border-border/70 [&_pre]:bg-zinc-950/95 [&_pre]:px-4 [&_pre]:py-3 [&_pre]:text-xs [&_pre]:leading-6 [&_pre]:text-zinc-100",
        "[&_pre>code]:block [&_pre>code]:bg-transparent [&_pre>code]:p-0 [&_pre>code]:text-inherit",
        "[&_code]:rounded-md [&_code]:bg-muted/80 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.9em]",
        "[&_img]:max-h-[28rem] [&_img]:max-w-full [&_img]:rounded-2xl [&_img]:border [&_img]:border-border/70 [&_img]:bg-background/80 [&_img]:shadow-sm",
        "[&_figure]:overflow-hidden",
        "[&_figcaption]:px-1",
        className,
      )}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
