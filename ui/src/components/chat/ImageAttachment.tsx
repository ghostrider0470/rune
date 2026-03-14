import { useMemo, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { X, Image as ImageIcon } from "lucide-react";
import { cn } from "@/lib/utils";

interface ImageAttachmentProps {
  file: File;
  onRemove: () => void;
  className?: string;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function ImageAttachment({
  file,
  onRemove,
  className,
}: ImageAttachmentProps) {
  const previewUrl = useMemo(() => URL.createObjectURL(file), [file]);

  useEffect(() => {
    return () => {
      URL.revokeObjectURL(previewUrl);
    };
  }, [previewUrl]);

  return (
    <div
      className={cn(
        "group relative flex items-center gap-3 rounded-lg border bg-card p-2 pr-8 shadow-sm",
        className,
      )}
    >
      {/* Thumbnail */}
      <div className="relative h-12 w-12 shrink-0 overflow-hidden rounded-md bg-muted">
        <img
          src={previewUrl}
          alt={file.name}
          className="h-full w-full object-cover"
          onLoad={() => {
            // Revoke on load to free memory while keeping display
            // Note: we don't revoke immediately because the img still needs it
          }}
        />
        <div className="absolute inset-0 flex items-center justify-center bg-black/20 opacity-0 transition-opacity group-hover:opacity-100">
          <ImageIcon className="h-4 w-4 text-white" />
        </div>
      </div>

      {/* File info */}
      <div className="min-w-0 flex-1">
        <p className="truncate text-xs font-medium">{file.name}</p>
        <p className="text-[10px] text-muted-foreground">
          {formatFileSize(file.size)}
        </p>
      </div>

      {/* Remove button */}
      <Button
        variant="ghost"
        size="icon-xs"
        onClick={onRemove}
        className="absolute right-1 top-1 opacity-100 transition-opacity sm:opacity-0 sm:group-hover:opacity-100"
        aria-label="Remove attachment"
      >
        <X className="h-3 w-3" />
      </Button>
    </div>
  );
}
