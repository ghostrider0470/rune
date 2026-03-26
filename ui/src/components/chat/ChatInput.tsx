import { useState, useRef, useCallback, useEffect, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Send, Loader2, CornerDownLeft, ImagePlus, Paperclip, AlertCircle, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { ImageAttachment } from "./ImageAttachment";
import {
  clipboardImagesFromEvent,
  dedupeFiles,
  fileListToArray,
  sanitizeIncomingAttachments,
} from "./attachment-utils";

interface ChatInputProps {
  onSend: (message: string, attachments?: File[]) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  maxAttachments?: number;
  sessionId?: string;
}

const MIN_HEIGHT = 60;
const MAX_HEIGHT = 200;

export function ChatInput({
  onSend,
  disabled = false,
  placeholder = "Send a message...",
  className,
  maxAttachments = 4,
  sessionId,
}: ChatInputProps) {
  const draftStorageKey = useMemo(
    () => (sessionId ? `rune.chat.draft.${sessionId}` : null),
    [sessionId],
  );
  const storedDraftValue = useMemo(() => {
    if (!draftStorageKey || typeof window === "undefined") {
      return "";
    }

    return window.localStorage.getItem(draftStorageKey) ?? "";
  }, [draftStorageKey]);
  const attachmentInputRef = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(storedDraftValue);
  const [attachments, setAttachments] = useState<File[]>([]);
  const [attachmentNotice, setAttachmentNotice] = useState<string | null>(null);
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

  useEffect(() => {
    setAttachments([]);
    setAttachmentNotice(null);
    if (attachmentInputRef.current) {
      attachmentInputRef.current.value = "";
    }
  }, [draftStorageKey]);

  useEffect(() => {
    setValue(storedDraftValue);
  }, [storedDraftValue]);

  useEffect(() => {
    if (!draftStorageKey || typeof window === "undefined") {
      return;
    }

    const trimmedValue = value.trim();
    if (trimmedValue.length === 0) {
      window.localStorage.removeItem(draftStorageKey);
      return;
    }

    window.localStorage.setItem(draftStorageKey, value);
  }, [draftStorageKey, value]);

  const appendAttachments = useCallback(
    (incoming: File[]) => {
      setAttachments((current) => {
        const merged = dedupeFiles([...current, ...incoming]);
        const { accepted, rejected } = sanitizeIncomingAttachments(merged, maxAttachments);

        if (rejected.length > 0) {
          const nonImages = rejected.filter((file) => !file.type.startsWith("image/")).length;
          const overLimit = rejected.length - nonImages;
          const messages = [
            nonImages > 0 ? `${nonImages} non-image file${nonImages === 1 ? " was" : "s were"} skipped` : null,
            overLimit > 0 ? `${overLimit} image${overLimit === 1 ? "" : "s"} ignored past the ${maxAttachments}-image limit` : null,
          ].filter(Boolean);
          setAttachmentNotice(messages.join(" · "));
        } else {
          setAttachmentNotice(null);
        }

        return accepted;
      });
    },
    [maxAttachments],
  );

  const handleRemoveAttachment = useCallback((index: number) => {
    setAttachments((current) => current.filter((_, currentIndex) => currentIndex !== index));
    setAttachmentNotice(null);
  }, []);

  const handleClearAttachments = useCallback(() => {
    setAttachments([]);
    setAttachmentNotice(null);
    if (attachmentInputRef.current) {
      attachmentInputRef.current.value = "";
    }
  }, []);

  const handleSend = useCallback(() => {
    const trimmed = value.trim();
    if ((!trimmed && attachments.length === 0) || disabled) return;

    onSend(trimmed, attachments.length > 0 ? attachments : undefined);
    setValue("");
    setAttachments([]);
    setAttachmentNotice(null);
    if (draftStorageKey && typeof window !== "undefined") {
      window.localStorage.removeItem(draftStorageKey);
    }
    if (attachmentInputRef.current) {
      attachmentInputRef.current.value = "";
    }
    requestAnimationFrame(() => {
      const el = textareaRef.current;
      if (el) {
        el.style.height = `${MIN_HEIGHT}px`;
        el.focus();
      }
    });
  }, [value, attachments, disabled, onSend, draftStorageKey]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  const handlePaste = useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const imageFiles = clipboardImagesFromEvent(e);

      if (imageFiles.length === 0) {
        return;
      }

      e.preventDefault();
      appendAttachments(imageFiles);
    },
    [appendAttachments],
  );

  const handleFileSelection = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      appendAttachments(fileListToArray(event.target.files));
      event.target.value = "";
    },
    [appendAttachments],
  );

  const sendDisabled = disabled || (!value.trim() && attachments.length === 0);

  return (
    <div className={cn("space-y-2", className)}>
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {attachments.map((attachment, index) => (
            <ImageAttachment
              key={`${attachment.name}-${attachment.size}-${attachment.lastModified}-${index}`}
              file={attachment}
              onRemove={() => handleRemoveAttachment(index)}
              className="w-full sm:max-w-sm"
            />
          ))}
        </div>
      )}

      <div
        className={cn(
          "rounded-[1.75rem] border border-border/70 bg-gradient-to-br from-background via-background to-primary/5 p-2 shadow-[0_12px_40px_rgba(15,23,42,0.08)] transition-colors",
          "focus-within:border-primary/30 focus-within:ring-[3px] focus-within:ring-primary/10",
          disabled && "opacity-60",
        )}
      >
        <input
          ref={attachmentInputRef}
          type="file"
          accept="image/*"
          multiple
          className="sr-only"
          onChange={handleFileSelection}
          disabled={disabled}
        />
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
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
                "flex-1 resize-none bg-transparent px-3 py-2.5 text-[15px] outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed sm:text-sm",
              )}
              style={{ minHeight: MIN_HEIGHT, maxHeight: MAX_HEIGHT }}
            />
            <div className="flex flex-wrap items-center justify-between gap-2 px-1 pb-1 text-[11px] text-muted-foreground sm:px-3">
              <span className="inline-flex items-center gap-1">
                <ImagePlus className="h-3 w-3" />
                Paste or attach images
              </span>
              <span className="hidden items-center gap-1 sm:inline-flex">
                <CornerDownLeft className="h-3 w-3" />
                Enter sends · Shift + Enter newline
              </span>
              {attachments.length > 0 && (
                <span className="w-full text-[10px] text-muted-foreground sm:hidden">
                  {attachments.length}/{maxAttachments} image attachment{attachments.length === 1 ? "" : "s"} ready.
                </span>
              )}
              {attachments.length > 0 && (
                <span className="hidden w-full items-center justify-between gap-2 text-[10px] text-muted-foreground sm:flex">
                  <span>
                    {attachments.length}/{maxAttachments} image attachment{attachments.length === 1 ? "" : "s"} queued for this send.
                  </span>
                  <button
                    type="button"
                    onClick={handleClearAttachments}
                    disabled={disabled}
                    className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <X className="h-3 w-3" />
                    Clear
                  </button>
                </span>
              )}
              {attachmentNotice && (
                <span className="inline-flex w-full items-center gap-1 text-[10px] text-amber-600 dark:text-amber-400">
                  <AlertCircle className="h-3 w-3" />
                  {attachmentNotice}
                </span>
              )}
            </div>
          </div>
          <div className="flex items-center justify-between gap-2 sm:mb-1 sm:shrink-0 sm:justify-end">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => attachmentInputRef.current?.click()}
              disabled={disabled || attachments.length >= maxAttachments}
              className="h-10 rounded-2xl px-3 text-muted-foreground sm:h-11 sm:w-11 sm:px-0"
              aria-label="Attach image"
            >
              <Paperclip className="h-4 w-4" />
              <span className="ml-1.5 text-xs sm:hidden">Attach</span>
            </Button>
            <Button
              onClick={handleSend}
              disabled={sendDisabled}
              size="sm"
              className="h-10 min-w-[7rem] flex-1 rounded-2xl px-4 sm:h-11 sm:w-11 sm:flex-none sm:min-w-0 sm:px-0"
            >
              {disabled ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <>
                  <Send className="h-4 w-4" />
                  <span className="ml-1.5 text-sm sm:hidden">Send</span>
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
