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

const componentMap: Record<string, React.ComponentType<{ component: A2uiComponent }>> = {
  card: A2uiCard,
  table: A2uiTable,
  kv: A2uiKv,
  progress: A2uiProgress,
  code: A2uiCode,
  list: A2uiList,
  form: A2uiForm,
  chart: A2uiChart,
};

function A2uiFallback({ component }: { component: A2uiComponent }) {
  return (
    <Card className="border-dashed">
      <CardContent className="py-3">
        <pre className="overflow-x-auto text-xs text-muted-foreground">
          {JSON.stringify(component, null, 2)}
        </pre>
      </CardContent>
    </Card>
  );
}

export function A2uiRenderer({ components }: { components: A2uiComponent[] }) {
  if (components.length === 0) return null;

  return (
    <div className="space-y-3">
      {components.map((component) => {
        const Component = componentMap[component.type] ?? A2uiFallback;
        return <Component key={component.id} component={component} />;
      })}
    </div>
  );
}
