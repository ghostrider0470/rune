import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { SkillItem } from "@/lib/api-types";
import { Wrench, FolderOpen } from "lucide-react";

export const Route = createFileRoute("/_admin/skills")({
  component: SkillsPage,
});

function SkillsPage() {
  const queryClient = useQueryClient();

  const { data: skills, isLoading } = useQuery({
    queryKey: ["skills"],
    queryFn: () => api.get<SkillItem[]>("/skills"),
    refetchInterval: 15_000,
  });

  const toggleSkill = useMutation({
    mutationFn: ({ name, enable }: { name: string; enable: boolean }) =>
      api.post(`/skills/${name}/${enable ? "enable" : "disable"}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Skills</h1>
        <p className="mt-1 text-muted-foreground">
          Hot-reloading skills from SKILL.md files
        </p>
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
                <div className="flex items-start justify-between">
                  <CardTitle className="flex items-center gap-2 text-base">
                    <Wrench className="h-4 w-4 shrink-0" />
                    {skill.name}
                  </CardTitle>
                  <Switch
                    checked={skill.enabled}
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
                <p className="text-sm text-muted-foreground">
                  {skill.description}
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
                  <FolderOpen className="h-3 w-3" />
                  <span className="truncate font-mono">
                    {skill.source_dir}
                  </span>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
