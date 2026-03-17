import { Card, CardContent } from "@/components/ui/card";
import type { A2uiComponent } from "@/lib/api-types";

export function A2uiChart({ component }: { component: A2uiComponent }) {
  return (
    <Card className="border-dashed">
      <CardContent className="py-6 text-center text-sm text-muted-foreground">
        Chart rendering not yet supported
        {component.title ? ` — ${String(component.title)}` : ""}
      </CardContent>
    </Card>
  );
}
