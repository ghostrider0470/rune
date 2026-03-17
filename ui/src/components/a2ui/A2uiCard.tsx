import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { A2uiComponent } from "@/lib/api-types";

export function A2uiCard({ component }: { component: A2uiComponent }) {
  const title = component.title != null ? String(component.title) : null;
  const body = component.body != null ? String(component.body) : null;

  return (
    <Card>
      {title && (
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{title}</CardTitle>
        </CardHeader>
      )}
      <CardContent className={title ? "" : "pt-4"}>
        {body ? (
          <p className="text-sm text-muted-foreground">{body}</p>
        ) : null}
      </CardContent>
    </Card>
  );
}
