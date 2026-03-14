import { useState, useRef, useCallback, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Send, Loader2, CornerDownLeft, ImagePlus } from "lucide-react";
import { cn } from "@/lib/utils";
import { ImageAttachment } from "./ImageAttachment";

interface ChatInputProps {
  onSend: (message: string, attachments?: File[]) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}

const MIN_HEIGHT = 60;
const MAX_HEIGHT = 200;

export function ChatInput({
  onSend,
  disabled = false,
  placeholder = "Send a message...",
  className,
}: ChatInputProps) {
  const [value, setValue] = useState("");
  const [attachment, setAttachment] = useState<File | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const resize = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const next = Math.min(Math.max(el.scrollHeight, MIN_HEIGHT), MAX_HEIGHT);
    el.style.height = `${next}px`;
  }, []);

  useEffect(() => {
    resize();
  }, [value, resize]);

  const handleRemoveAttachment = useCallback(() => {
    setAttachment(null);
  }, []);

  const handleSend = useCallback(() => {
    const trimmed = value.trim();
    if ((!trimmed && !attachment) || disabled) return;

    onSend(trimmed, attachment ? [attachment] : undefined);
    setValue("");
    setAttachment(null);
    requestAnimationFrame(() => {
      const el = textareaRef.current;
      if (el) el.style.height = `${MIN_HEIGHT}px`;
    });
  }, [value, attachment, disabled, onSend]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  const handlePaste = useCallback((e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const imageItem = Array.from(e.clipboardData.items).find((item) =>
      item.type.startsWith("image/"),
    );

    if (!imageItem) {
      return;
    }

    const file = imageItem.getAsFile();
    if (!file) {
      return;
    }

    e.preventDefault();
    const extension = file.type.split("/")[1] || "png";
    const namedFile = new File([file], `pasted-image-${Date.now()}.${extension}`, {
      type: file.type,
      lastModified: Date.now(),
    });
    setAttachment(namedFile);
  }, []);

  const sendDisabled = disabled || (!value.trim() && !attachment);

  return (
    <div className={cn("space-y-2", className)}>
      {attachment && (
        <ImageAttachment
          file={attachment}
          onRemove={handleRemoveAttachment}
          className="max-w-sm"
        />
      )}

      <div
        className={cn(
          "rounded-[1.75rem] border border-border/70 bg-gradient-to-br from-background via-background to-primary/5 p-2 shadow-[0_12px_40px_rgba(15,23,42,0.08)] transition-colors",
          "focus-within:border-primary/30 focus-within:ring-[3px] focus-within:ring-primary/10",
          disabled && "opacity-60",
        )}
      >
        <div className="flex items-end gap-2">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <textarea
              ref={textareaRef}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder={placeholder}
              disabled={disabled}
              rows={1}
              className={cn(
                "flex-1 resize-none bg-transparent px-3 py-2.5 text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed",
              )}
              style={{ minHeight: MIN_HEIGHT, maxHeight: MAX_HEIGHT }}
            />
            <div className="flex flex-wrap items-center justify-between gap-2 px-3 pb-1 text-[11px] text-muted-foreground">
              <span className="inline-flex items-center gap-1">
                <ImagePlus className="h-3 w-3" />
                Paste image to attach
              </span>
              <span className="inline-flex items-center gap-1">
                <CornerDownLeft className="h-3 w-3" />
                Enter sends · Shift + Enter newline
              </span>
              {attachment && (
                <span className="w-full text-[10px] text-amber-600 dark:text-amber-400">
                  Image is sent as attachment metadata for now; binary upload is not wired yet.
                </span>
              )}
            </div>
          </div>
          <Button
            onClick={handleSend}
            disabled={sendDisabled}
            size="icon"
            className="mb-1 h-11 w-11 shrink-0 rounded-2xl"
          >
            {disabled ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Send className="h-4 w-4" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
