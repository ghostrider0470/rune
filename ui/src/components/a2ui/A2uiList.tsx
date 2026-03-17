import type { A2uiComponent } from "@/lib/api-types";

export function A2uiList({ component }: { component: A2uiComponent }) {
  const items = Array.isArray(component.items) ? component.items : [];
  const ordered = component.ordered === true;

  const Tag = ordered ? "ol" : "ul";

  return (
    <Tag className={`space-y-1 pl-5 text-sm ${ordered ? "list-decimal" : "list-disc"}`}>
      {items.map((item, i) => (
        <li key={i}>{String(item)}</li>
      ))}
    </Tag>
  );
}
