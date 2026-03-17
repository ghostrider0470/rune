import type { A2uiComponent } from "@/lib/api-types";

export function A2uiKv({ component }: { component: A2uiComponent }) {
  const entries = Array.isArray(component.entries)
    ? component.entries
    : typeof component.data === "object" && component.data
      ? Object.entries(component.data as Record<string, unknown>)
      : [];

  return (
    <div className="rounded-md border">
      <table className="w-full text-sm">
        <tbody>
          {entries.map((entry, i) => {
            const [key, value] = Array.isArray(entry) ? entry : [String(i), entry];
            return (
              <tr key={i} className="border-b last:border-0">
                <td className="px-3 py-2 font-medium text-muted-foreground">{String(key)}</td>
                <td className="px-3 py-2">{String(value)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
