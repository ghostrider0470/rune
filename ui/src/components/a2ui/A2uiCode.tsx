import type { A2uiComponent } from "@/lib/api-types";

export function A2uiCode({ component }: { component: A2uiComponent }) {
  const code = typeof component.code === "string" ? component.code : "";
  const language = typeof component.language === "string" ? component.language : null;

  return (
    <div className="space-y-1">
      {language && (
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground">{language}</span>
      )}
      <pre className="overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs">{code}</pre>
    </div>
  );
}
