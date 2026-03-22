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
import { useDashboardModels } from "@/hooks/use-dashboard";
import { Cpu, Star } from "lucide-react";

export const Route = createFileRoute("/_admin/models")({
  component: ModelsPage,
});

function ModelsPage() {
  const { data: models, isLoading } = useDashboardModels();

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Models</h1>
        <p className="mt-1 text-muted-foreground">Configured model providers and backends</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Cpu className="h-4 w-4" />
            Models ({models?.length ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !models?.length ? (
            <p className="text-sm text-muted-foreground">No models configured</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Provider</TableHead>
                  <TableHead>Kind</TableHead>
                  <TableHead>Model ID</TableHead>
                  <TableHead>Raw Model</TableHead>
                  <TableHead>Default</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {models.map((m, i) => (
                  <TableRow key={i}>
                    <TableCell className="font-medium">{m.provider_name}</TableCell>
                    <TableCell>
                      <Badge variant="outline">{m.provider_kind}</Badge>
                    </TableCell>
                    <TableCell className="font-mono text-sm">{m.model_id}</TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {m.raw_model}
                    </TableCell>
                    <TableCell>
                      {m.is_default && (
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
