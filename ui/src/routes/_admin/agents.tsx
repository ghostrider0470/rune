import { createFileRoute } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { AgentListItem } from "@/lib/api-types";
import { Bot, Star } from "lucide-react";

export const Route = createFileRoute("/_admin/agents")({
  component: AgentsPage,
});

function AgentsPage() {
  const { data: agents, isLoading } = useQuery({
    queryKey: ["agents"],
    queryFn: () => api.get<AgentListItem[]>("/agents"),
    refetchInterval: 30_000,
  });

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Agents</h1>
        <p className="mt-1 text-muted-foreground">
          Configured agents and their settings
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Bot className="h-4 w-4" />
            Agents ({agents?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-12 w-full" />
              ))}
            </div>
          ) : !agents?.length ? (
            <p className="text-sm text-muted-foreground">
              No agents configured. Add agents in your configuration file.
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="py-3.5">ID</TableHead>
                  <TableHead className="py-3.5">Model</TableHead>
                  <TableHead className="py-3.5">Workspace</TableHead>
                  <TableHead className="py-3.5">System Prompt</TableHead>
                  <TableHead className="py-3.5">Default</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {agents.map((agent) => (
                  <TableRow key={agent.id}>
                    <TableCell className="py-3 font-medium">{agent.id}</TableCell>
                    <TableCell className="py-3">
                      {agent.model ? (
                        <Badge variant="outline" className="font-mono text-xs">
                          {agent.model}
                        </Badge>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </TableCell>
                    <TableCell className="max-w-[200px] truncate py-3 font-mono text-xs">
                      {agent.workspace ?? "—"}
                    </TableCell>
                    <TableCell className="max-w-[200px] truncate py-3 text-xs">
                      {agent.system_prompt ? (
                        <span title={agent.system_prompt}>
                          {agent.system_prompt.slice(0, 80)}
                          {agent.system_prompt.length > 80 ? "..." : ""}
                        </span>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </TableCell>
                    <TableCell className="py-3">
                      {agent.default && (
                        <Star className="h-4 w-4 fill-yellow-400 text-yellow-400" />
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
