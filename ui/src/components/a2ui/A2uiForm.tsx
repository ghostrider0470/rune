import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { A2uiComponent } from "@/lib/api-types";

export function A2uiForm({ component }: { component: A2uiComponent }) {
  return (
    <Card className="border-dashed">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">
          {component.title ? String(component.title) : "Form"}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <pre className="overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs">
          {JSON.stringify(component, null, 2)}
        </pre>
      </CardContent>
    </Card>
  );
}
