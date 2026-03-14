import { useState } from "react";
import { Button } from "@/components/ui/button";
import { ChevronRight, Brain } from "lucide-react";
import { cn } from "@/lib/utils";

interface ThinkingBlockProps {
  content: string;
  className?: string;
}

export function ThinkingBlock({ content, className }: ThinkingBlockProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div
      className={cn(
        "rounded-lg border border-dashed border-muted-foreground/30 bg-muted/20",
        className,
      )}
    >
      <Button
        variant="ghost"
        size="sm"
        onClick={() => setExpanded(!expanded)}
        className="w-full justify-start gap-2 px-3 py-2 text-xs font-medium text-muted-foreground hover:text-foreground"
      >
        <ChevronRight
          className={cn(
            "h-3 w-3 transition-transform duration-200",
            expanded && "rotate-90",
          )}
        />
        <Brain className="h-3 w-3" />
        <span>Thinking...</span>
      </Button>

      {expanded && (
        <div className="border-t border-dashed border-muted-foreground/30 px-4 py-3">
          <p className="whitespace-pre-wrap text-sm italic text-muted-foreground">
            {content}
          </p>
        </div>
      )}
    </div>
  );
}
