import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { A2uiComponent } from "@/lib/api-types";

const palette = ["#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#06b6d4"];

function points(data: number[], width: number, height: number) {
  const max = Math.max(...data, 1);
  return data
    .map((value, index) => {
      const x = (index / Math.max(data.length - 1, 1)) * width;
      const y = height - (value / max) * height;
      return `${x},${y}`;
    })
    .join(" ");
}

export function A2uiChart({ component }: { component: A2uiComponent }) {
  const title = component.title != null ? String(component.title) : null;
  const chartType = String(component.chart_type ?? "bar");
  const series = Array.isArray(component.series) ? component.series : [];
  const xLabels = Array.isArray(component.x_labels) ? component.x_labels.map(String) : [];
  const firstSeries = series[0] && typeof series[0] === "object" ? (series[0] as Record<string, unknown>) : null;
  const firstData = Array.isArray(firstSeries?.data) ? firstSeries.data.map((value) => Number(value) || 0) : [];

  const content = (() => {
    if (firstData.length === 0) {
      return <div className="py-8 text-center text-sm text-muted-foreground">No data</div>;
    }

    if (chartType === "line" || chartType === "area") {
      const polyline = points(firstData, 260, 120);
      return (
        <svg className="h-36 w-full" viewBox="0 0 260 140">
          {chartType === "area" ? (
            <polygon fill="rgba(59,130,246,0.2)" points={`0,120 ${polyline} 260,120`} />
          ) : null}
          <polyline fill="none" points={polyline} stroke={palette[0]} strokeWidth="3" />
        </svg>
      );
    }

    if (chartType === "pie") {
      const total = firstData.reduce((sum, value) => sum + value, 0) || 1;
      let offset = 0;
      return (
        <svg className="h-40 w-full" viewBox="0 0 160 160">
          {firstData.map((value, index) => {
            const circumference = 2 * Math.PI * 50;
            const length = (value / total) * circumference;
            const strokeDasharray = `${length} ${circumference - length}`;
            const circle = (
              <circle
                key={`${component.id}-${index}`}
                cx="80"
                cy="80"
                fill="transparent"
                r="50"
                stroke={palette[index % palette.length]}
                strokeDasharray={strokeDasharray}
                strokeDashoffset={-offset}
                strokeWidth="18"
                transform="rotate(-90 80 80)"
              />
            );
            offset += length;
            return circle;
          })}
        </svg>
      );
    }

    const max = Math.max(...firstData, 1);
    const barWidth = 220 / firstData.length;
    return (
      <svg className="h-36 w-full" viewBox="0 0 260 140">
        {firstData.map((value, index) => {
          const height = (value / max) * 100;
          return (
            <rect
              key={`${component.id}-${index}`}
              fill={palette[index % palette.length]}
              height={height}
              width={Math.max(barWidth - 8, 8)}
              x={20 + index * barWidth}
              y={120 - height}
              rx="4"
            />
          );
        })}
      </svg>
    );
  })();

  return (
    <Card>
      {title ? (
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{title}</CardTitle>
        </CardHeader>
      ) : null}
      <CardContent className={title ? "space-y-3" : "space-y-3 pt-4"}>
        {content}
        {xLabels.length > 0 ? (
          <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
            {xLabels.map((label, index) => (
              <span key={`${component.id}-label-${index}`}>{label}</span>
            ))}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
