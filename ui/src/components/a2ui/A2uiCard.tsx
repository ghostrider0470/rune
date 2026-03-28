import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { MarkdownRenderer } from "@/components/chat/MarkdownRenderer";
import { Button } from "@/components/ui/button";
import type { A2uiComponent } from "@/lib/api-types";

interface Props {
  component: A2uiComponent;
  onAction?: (componentId: string, actionTarget: string) => void | Promise<void>;
}

export function A2uiCard({ component, onAction }: Props) {
  const title = component.title != null ? String(component.title) : null;
  const body = component.body != null ? String(component.body) : null;
  const actions = Array.isArray(component.actions)
    ? component.actions.filter(
        (action): action is { label: string; action_type: string; target: string; variant?: string } =>
          !!action &&
          typeof action === "object" &&
          typeof (action as { label?: unknown }).label === "string" &&
          typeof (action as { action_type?: unknown }).action_type === "string" &&
          typeof (action as { target?: unknown }).target === "string",
      )
    : [];

  return (
    <Card>
      {title && (
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{title}</CardTitle>
        </CardHeader>
      )}
      <CardContent className={title ? "space-y-3" : "space-y-3 pt-4"}>
        {body ? <MarkdownRenderer content={body} /> : null}
        {actions.length > 0 ? (
          <div className="flex flex-wrap gap-2">
            {actions.map((action) => {
              if (action.action_type === "link") {
                return (
                  <Button asChild key={`${component.id}-${action.target}`} size="sm" variant="outline">
                    <a href={action.target} rel="noreferrer" target="_blank">
                      {action.label}
                    </a>
                  </Button>
                );
              }

              return (
                <Button
                  key={`${component.id}-${action.target}`}
                  size="sm"
                  variant={action.variant === "destructive" ? "destructive" : "outline"}
                  onClick={() => onAction?.(component.id, action.target)}
                >
                  {action.label}
                </Button>
              );
            })}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
