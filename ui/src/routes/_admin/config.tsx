import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useConfig, useUpdateConfig } from "@/hooks/use-system";
import {
  Settings2,
  Save,
  Search,
  FileJson,
  CheckCircle2,
} from "lucide-react";

export const Route = createFileRoute("/_admin/config")({
  component: ConfigPage,
});

function ConfigPage() {
  const { data: config, isLoading } = useConfig();
  const updateConfig = useUpdateConfig();
  const [search, setSearch] = useState("");
  const [editMode, setEditMode] = useState(false);
  const [editJson, setEditJson] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);

  const configSections = useMemo(() => {
    if (!config) return [];

    return Object.entries(config).filter(([key]) =>
      search ? key.toLowerCase().includes(search.toLowerCase()) : true,
    );
  }, [config, search]);

  const rawJson = useMemo(
    () => (config ? JSON.stringify(config, null, 2) : ""),
    [config],
  );

  function handleEditToggle() {
    if (!editMode) {
      setEditJson(rawJson);
      setSaveError(null);
    }
    setEditMode(!editMode);
  }

  function handleSave() {
    try {
      const parsed = JSON.parse(editJson);
      setSaveError(null);
      updateConfig.mutate(parsed, {
        onSuccess: () => {
          setEditMode(false);
        },
        onError: (err) => {
          setSaveError(err instanceof Error ? err.message : "Save failed");
        },
      });
    } catch {
      setSaveError("Invalid JSON");
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Configuration</h1>
          <p className="text-muted-foreground">
            Live application configuration (secrets redacted).
          </p>
        </div>
        <Badge variant="outline" className="gap-1 text-green-700">
          <CheckCircle2 className="h-3.5 w-3.5" />
          Live
        </Badge>
      </div>

      <Tabs defaultValue="form">
        <TabsList>
          <TabsTrigger value="form">
            <Settings2 className="mr-2 h-4 w-4" />
            Sections
          </TabsTrigger>
          <TabsTrigger value="json">
            <FileJson className="mr-2 h-4 w-4" />
            JSON
          </TabsTrigger>
        </TabsList>

        <TabsContent value="form" className="space-y-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search configuration sections..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-10"
            />
          </div>

          {isLoading ? (
            <div className="space-y-4">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-32 w-full" />
              ))}
            </div>
          ) : !configSections.length ? (
            <p className="text-sm text-muted-foreground">
              {search ? "No matching configuration sections" : "No configuration available"}
            </p>
          ) : (
            configSections.map(([section, value]) => (
              <Card key={section}>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2 text-base">
                    <Settings2 className="h-4 w-4" />
                    {section}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="overflow-x-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(value, null, 2)}
                  </pre>
                </CardContent>
              </Card>
            ))
          )}
        </TabsContent>

        <TabsContent value="json">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-3">
              <CardTitle className="text-base">
                {editMode ? "Edit Configuration" : "Full Configuration"}
              </CardTitle>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleEditToggle}
                >
                  {editMode ? "Cancel" : "Edit"}
                </Button>
                {editMode && (
                  <Button
                    size="sm"
                    onClick={handleSave}
                    disabled={updateConfig.isPending}
                  >
                    <Save className="mr-2 h-4 w-4" />
                    {updateConfig.isPending ? "Saving..." : "Save"}
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-2">
              {saveError && (
                <p className="text-sm text-red-600">{saveError}</p>
              )}
              <Textarea
                value={editMode ? editJson : rawJson}
                readOnly={!editMode}
                onChange={editMode ? (e) => setEditJson(e.target.value) : undefined}
                className="min-h-[500px] font-mono text-sm"
                placeholder="Loading configuration..."
              />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
