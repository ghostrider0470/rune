import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import {
  useSkills,
  useToggleSkill,
  useReloadSkills,
} from "@/hooks/use-operators";
import { Wrench, FolderOpen, RefreshCw } from "lucide-react";

export const Route = createFileRoute("/_admin/skills")({
  component: SkillsPage,
});

function SkillsPage() {
  const { data: skills, isLoading } = useSkills();
  const toggleSkill = useToggleSkill();
  const reloadSkills = useReloadSkills();

  return (
    <div className="space-y-8">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Skills</h1>
          <p className="mt-1 text-muted-foreground">
            Hot-reloading skills from SKILL.md files
          </p>
        </div>
        <Button
          variant="outline"
          onClick={() => reloadSkills.mutate()}
          disabled={reloadSkills.isPending}
          className="sm:self-start"
        >
          <RefreshCw
            className={`mr-2 h-4 w-4 ${reloadSkills.isPending ? "animate-spin" : ""}`}
          />
          Reload skills
        </Button>
      </div>

      {isLoading ? (
        <div className="grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-40" />
          ))}
        </div>
      ) : !skills?.length ? (
        <Card>
          <CardContent className="py-12 text-center">
            <Wrench className="mx-auto h-10 w-10 text-muted-foreground/50" />
            <p className="mt-3 text-sm text-muted-foreground">
              No skills found. Create a <code>skills/*/SKILL.md</code> file to
              add skills.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {skills.map((skill) => (
            <Card key={skill.name} className="relative">
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between gap-3">
                  <CardTitle className="flex items-center gap-2 text-base">
                    <Wrench className="h-4 w-4 shrink-0" />
                    <span className="break-all">{skill.name}</span>
                  </CardTitle>
                  <Switch
                    checked={skill.enabled}
                    disabled={toggleSkill.isPending || reloadSkills.isPending}
                    onCheckedChange={(checked) =>
                      toggleSkill.mutate({
                        name: skill.name,
                        enable: checked,
                      })
                    }
                  />
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                <p className="min-h-[2.5rem] text-sm text-muted-foreground">
                  {skill.description || "No description provided."}
                </p>

                <div className="flex flex-wrap gap-2">
                  <Badge variant={skill.enabled ? "default" : "secondary"}>
                    {skill.enabled ? "Enabled" : "Disabled"}
                  </Badge>
                  {skill.binary_path && (
                    <Badge variant="outline" className="text-xs">
                      Has binary
                    </Badge>
                  )}
                </div>

                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <FolderOpen className="h-3 w-3 shrink-0" />
                  <span className="truncate font-mono">{skill.source_dir}</span>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
