import { Card, CardContent } from "@/components/ui/card";
import type { A2uiComponent } from "@/lib/api-types";
import { A2uiCard } from "./A2uiCard";
import { A2uiTable } from "./A2uiTable";
import { A2uiKv } from "./A2uiKv";
import { A2uiProgress } from "./A2uiProgress";
import { A2uiCode } from "./A2uiCode";
import { A2uiList } from "./A2uiList";
import { A2uiForm } from "./A2uiForm";
import { A2uiChart } from "./A2uiChart";

interface RendererProps {
  components: A2uiComponent[];
  onAction?: (componentId: string, actionTarget: string) => void | Promise<void>;
  onSubmit?: (callbackId: string, data: Record<string, unknown>) => void | Promise<void>;
}

function A2uiFallback({ component }: { component: A2uiComponent }) {
  return (
    <Card className="border-dashed">
      <CardContent className="space-y-2 py-3">
        <p className="text-sm font-medium">Unsupported component type: {component.type}</p>
        <pre className="overflow-x-auto text-xs text-muted-foreground">
          {JSON.stringify(component, null, 2)}
        </pre>
      </CardContent>
    </Card>
  );
}

export function A2uiRenderer({ components, onAction, onSubmit }: RendererProps) {
  if (components.length === 0) return null;

  return (
    <div className="space-y-3">
      {components.map((component) => {
        switch (component.type) {
          case "card":
            return <A2uiCard component={component} key={component.id} onAction={onAction} />;
          case "table":
            return <A2uiTable component={component} key={component.id} />;
          case "kv":
            return <A2uiKv component={component} key={component.id} />;
          case "progress":
            return <A2uiProgress component={component} key={component.id} />;
          case "code":
            return <A2uiCode component={component} key={component.id} />;
          case "list":
            return <A2uiList component={component} key={component.id} />;
          case "form":
            return <A2uiForm component={component} key={component.id} onSubmit={onSubmit} />;
          case "chart":
            return <A2uiChart component={component} key={component.id} />;
          default:
            return <A2uiFallback component={component} key={component.id} />;
        }
      })}
    </div>
  );
}
