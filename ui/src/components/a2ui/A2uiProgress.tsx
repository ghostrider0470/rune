import type { A2uiComponent } from "@/lib/api-types";

export function A2uiProgress({ component }: { component: A2uiComponent }) {
  const percent = typeof component.percent === "number" ? component.percent : 0;
  const label = component.label ? String(component.label) : null;

  return (
    <div className="space-y-1">
      {label && (
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{label}</span>
          <span>{Math.round(percent)}%</span>
        </div>
      )}
      <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="h-full rounded-full bg-primary transition-all"
          style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
        />
      </div>
    </div>
  );
}
