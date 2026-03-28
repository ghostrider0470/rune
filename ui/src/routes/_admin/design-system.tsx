import { createFileRoute } from "@tanstack/react-router";
import { Check, Contrast, Layers3, Palette, Sparkles, Type } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { designSystem } from "@/lib/design-system";

export const Route = createFileRoute("/_admin/design-system" as never)({
  component: DesignSystemPage,
});

const semanticTokens = [
  { name: "Success", token: designSystem.color.palette.semantic.success, preview: "bg-emerald-500" },
  { name: "Warning", token: designSystem.color.palette.semantic.warning, preview: "bg-amber-500" },
  { name: "Error", token: designSystem.color.palette.semantic.error, preview: "bg-red-500" },
  { name: "Info", token: designSystem.color.palette.semantic.info, preview: "bg-sky-500" },
];

const componentCards = [
  {
    icon: Layers3,
    title: "Core components",
    description: "Buttons, cards, badges, inputs, textareas, switches, tables, dropdowns, tabs, sheets, dialogs.",
  },
  {
    icon: Contrast,
    title: "Theme control",
    description: "Dark mode stays the default, light mode is available, and system sync remains supported.",
  },
  {
    icon: Sparkles,
    title: "Motion",
    description: "Short transitions, subtle hover elevation, no jarring movement or gimmicks.",
  },
];

function TokenSwatch({ name, value, className }: { name: string; value: string; className: string }) {
  return (
    <div className="rounded-xl border bg-card p-4 shadow-sm">
      <div className={`h-14 rounded-lg border ${className}`} />
      <div className="mt-3 space-y-1">
        <p className="text-sm font-medium">{name}</p>
        <p className="font-mono text-xs text-muted-foreground">{value}</p>
      </div>
    </div>
  );
}

function DesignSystemPage() {
  return (
    <div className="space-y-8 sm:space-y-10">
      <section className="rounded-3xl border border-primary/20 bg-gradient-to-br from-primary/10 via-background to-accent/10 p-6 shadow-sm sm:p-8">
        <Badge variant="outline" className="mb-4 gap-2">
          <Palette className="h-3.5 w-3.5" />
          Issue #435
        </Badge>
        <div className="max-w-3xl space-y-4">
          <h1 className={designSystem.typography.display.pageTitle}>Design system</h1>
          <p className="text-base leading-7 text-muted-foreground sm:text-lg">
            Shared visual language for the operator UI: generous spacing, dark-default theming,
            semantic color tokens, and reusable shadcn-based primitives.
          </p>
        </div>
      </section>

      <section className="grid gap-4 lg:grid-cols-3 lg:gap-6">
        {componentCards.map((item) => {
          const Icon = item.icon;
          return (
            <Card key={item.title} className="border-primary/10 bg-card/90">
              <CardHeader>
                <div className="flex items-center gap-3">
                  <div className="rounded-xl bg-primary/10 p-2 text-primary">
                    <Icon className="h-5 w-5" />
                  </div>
                  <CardTitle className="text-base">{item.title}</CardTitle>
                </div>
                <CardDescription>{item.description}</CardDescription>
              </CardHeader>
            </Card>
          );
        })}
      </section>

      <section className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr]">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Palette className="h-4 w-4" />
              Color tokens
            </CardTitle>
            <CardDescription>Primary, accent, surface, and semantic tokens for both themes.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-6">
            <div className="space-y-3">
              <p className="text-sm font-medium">Light / dark foundations</p>
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                <TokenSwatch name="Light background" value={designSystem.color.palette.light.background} className="bg-[var(--background)]" />
                <TokenSwatch name="Light primary" value={designSystem.color.palette.light.primary} className="bg-primary" />
                <TokenSwatch name="Dark background" value={designSystem.color.palette.dark.background} className="bg-slate-950" />
                <TokenSwatch name="Dark accent" value={designSystem.color.palette.dark.accent} className="bg-accent" />
              </div>
            </div>
            <div className="space-y-3">
              <p className="text-sm font-medium">Semantic states</p>
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                {semanticTokens.map((token) => (
                  <TokenSwatch key={token.name} name={token.name} value={token.token} className={token.preview} />
                ))}
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Type className="h-4 w-4" />
              Typography + spacing
            </CardTitle>
            <CardDescription>Large headings, comfortable rhythm, readable body copy.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <div className="space-y-2">
              <p className={designSystem.typography.display.eyebrow}>Display</p>
              <h2 className={designSystem.typography.heading.h1}>Operator control surfaces</h2>
              <p className="text-muted-foreground">
                Base spacing scale ranges from 4px to 96px with page sections optimized around 24–64px gaps.
              </p>
            </div>
            <div className="grid gap-3 rounded-xl border bg-muted/20 p-4 text-sm sm:grid-cols-2">
              {Object.entries(designSystem.spacing.scale).map(([key, value]) => (
                <div key={key} className="flex items-center justify-between rounded-lg border bg-background px-3 py-2">
                  <span className="font-medium">space-{key}</span>
                  <span className="font-mono text-xs text-muted-foreground">{value}</span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </section>

      <section className="grid gap-6 xl:grid-cols-[1fr_1fr]">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Component previews</CardTitle>
            <CardDescription>Reference treatments for buttons, badges, and form controls.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-6">
            <div className="flex flex-wrap gap-3">
              <Button>Primary action</Button>
              <Button variant="outline">Secondary</Button>
              <Button variant="secondary">Muted</Button>
              <Button variant="ghost">Ghost</Button>
            </div>
            <div className="flex flex-wrap gap-2">
              <Badge>Default</Badge>
              <Badge variant="secondary">Secondary</Badge>
              <Badge variant="outline">Outline</Badge>
              <Badge variant="destructive">Destructive</Badge>
            </div>
            <div className="grid gap-4">
              <div className="grid gap-2">
                <Label htmlFor="ds-name">Operator label</Label>
                <Input id="ds-name" defaultValue="Rune control plane" />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ds-notes">Notes</Label>
                <Textarea id="ds-notes" defaultValue="Use consistent spacing and semantic colors across all admin routes." />
              </div>
              <div className="flex items-center justify-between rounded-xl border bg-muted/20 px-4 py-3">
                <div>
                  <p className="text-sm font-medium">Dark mode default</p>
                  <p className="text-xs text-muted-foreground">Persisted in local storage with system fallback.</p>
                </div>
                <Switch checked aria-label="Dark mode default enabled" />
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Acceptance coverage</CardTitle>
            <CardDescription>What is shipped in this slice for #435.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            {[
              "Design tokens exported in ui/src/lib/design-system.ts",
              "Dark/light/system theme switching already wired through ThemeProvider and ThemeToggle",
              "Shared shadcn component set covers 10+ primitives used across admin routes",
              "Design system reference page added at /design-system",
              "Dashboard and nav already consume the shared tokens and primitives",
            ].map((item) => (
              <div key={item} className="flex items-start gap-3 rounded-xl border bg-muted/10 px-4 py-3">
                <Check className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
                <span>{item}</span>
              </div>
            ))}
          </CardContent>
        </Card>
      </section>
    </div>
  );
}
