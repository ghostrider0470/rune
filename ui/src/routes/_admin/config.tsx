import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useConfig, useConfigSchema, useDoctorResults, useDoctorRun, useUpdateConfig } from "@/hooks/use-system";
import type { ConfigSchemaResponse, DoctorBackendMatrixEntry, DoctorCheck } from "@/lib/api-types";
import {
  Settings2,
  Save,
  Search,
  FileJson,
  CheckCircle2,
  Eye,
  EyeOff,
  Shield,
  Stethoscope,
  RefreshCw,
  AlertTriangle,
  XCircle,
  Info,
  GitCompareArrows,
  Braces,
} from "lucide-react";

export const Route = createFileRoute("/_admin/config")({
  component: ConfigPage,
});

type JsonEntry = {
  key: string;
  value: unknown;
  source: string;
  sensitive: boolean;
};

const SOURCE_LABELS: Array<[string, string]> = [
  ["auth_token", "global config · gateway auth override"],
  ["api_key", "provider config · credential override"],
  ["token", "provider config · token override"],
  ["secret", "secret store / config secret"],
  ["password", "secret store / config secret"],
  ["key", "config or env override"],
  ["paths", "profile-derived runtime paths"],
  ["provider", "provider block override"],
  ["providers", "provider registry"],
  ["default", "default + explicit config merge"],
];

function flattenConfig(value: unknown, prefix = ""): JsonEntry[] {
  if (Array.isArray(value)) {
    return value.flatMap((item, index) => flattenConfig(item, `${prefix}[${index}]`));
  }

  if (value && typeof value === "object") {
    return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) => {
      const path = prefix ? `${prefix}.${key}` : key;
      return flattenConfig(child, path);
    });
  }

  const lower = prefix.toLowerCase();
  return [
    {
      key: prefix,
      value,
      sensitive: isSensitivePath(lower),
      source: inferSource(lower),
    },
  ];
}

function isSensitivePath(path: string): boolean {
  return ["token", "secret", "password", "api_key", "apikey", "client_secret", "private_key"].some(
    (needle) => path.includes(needle),
  );
}

function inferSource(path: string): string {
  const match = SOURCE_LABELS.find(([needle]) => path.includes(needle));
  return match?.[1] ?? "default → global → project effective merge";
}

function maskValue(value: unknown, reveal: boolean): string {
  if (value === null) return "null";
  if (typeof value === "boolean" || typeof value === "number") return String(value);
  if (typeof value !== "string") return JSON.stringify(value);
  if (reveal) return value;
  if (value.length <= 4) return "••••";
  return `${value.slice(0, 2)}••••${value.slice(-2)}`;
}

function statusVariant(status: string): "default" | "secondary" | "destructive" | "outline" {
  switch (status) {
    case "pass":
    case "healthy":
      return "default";
    case "warn":
    case "degraded":
    case "info":
      return "secondary";
    case "fail":
    case "unhealthy":
      return "destructive";
    default:
      return "outline";
  }
}

function StatusIcon({ status }: { status: string }) {
  if (status === "fail" || status === "unhealthy") {
    return <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />;
  }
  if (status === "warn" || status === "degraded") {
    return <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-500" />;
  }
  if (status === "info") {
    return <Info className="mt-0.5 h-4 w-4 shrink-0 text-blue-500" />;
  }
  return <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-green-600" />;
}

function HealthMatrix({ entries }: { entries: DoctorBackendMatrixEntry[] }) {
  if (!entries.length) {
    return <p className="text-sm text-muted-foreground">No backend matrix available.</p>;
  }

  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
      {entries.map((entry) => (
        <div key={`${entry.subsystem}-${entry.backend}`} className="rounded-lg border p-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <p className="text-sm font-medium">{entry.subsystem}</p>
              <p className="font-mono text-xs text-muted-foreground">{entry.backend}</p>
            </div>
            <Badge variant={statusVariant(entry.status)}>{entry.status}</Badge>
          </div>
          <p className="mt-3 text-sm text-muted-foreground">{entry.capability}</p>
          {entry.fix_hint && (
            <p className="mt-2 text-xs text-muted-foreground">
              <span className="font-medium text-foreground">Fix:</span> {entry.fix_hint}
            </p>
          )}
        </div>
      ))}
    </div>
  );
}

function DoctorChecks({ checks }: { checks: DoctorCheck[] }) {
  if (!checks.length) {
    return <p className="text-sm text-muted-foreground">No doctor checks available.</p>;
  }

  return (
    <div className="space-y-2">
      {checks.map((check) => (
        <div key={check.name} className="flex items-start gap-3 rounded-lg border p-3 text-sm">
          <StatusIcon status={check.status} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="font-medium">{check.name}</span>
              <Badge variant={statusVariant(check.status)} className="text-xs">
                {check.status}
              </Badge>
            </div>
            <p className="mt-1 text-muted-foreground">{check.message}</p>
          </div>
        </div>
      ))}
    </div>
  );
}


function collectSchemaEntries(schema: ConfigSchemaResponse, prefix = ""): JsonEntry[] {
  const entries: JsonEntry[] = [];
  const currentKey = prefix || "(root)";

  if (schema.type || schema.enum || schema.default !== undefined || schema.description || schema.$ref) {
    const parts = [
      schema.type ? `type=${Array.isArray(schema.type) ? schema.type.join(" | ") : schema.type}` : null,
      schema.format ? `format=${schema.format}` : null,
      schema.enum ? `enum=${schema.enum.map((value) => JSON.stringify(value)).join(", ")}` : null,
      schema.default !== undefined ? `default=${JSON.stringify(schema.default)}` : null,
      schema.$ref ? `ref=${schema.$ref}` : null,
      schema.description ?? null,
    ].filter(Boolean);

    entries.push({
      key: currentKey,
      value: parts.join(" • ") || "schema node",
      sensitive: false,
      source: "json-schema",
    });
  }

  if (schema.properties) {
    for (const [key, value] of Object.entries(schema.properties)) {
      const next = prefix ? `${prefix}.${key}` : key;
      entries.push(...collectSchemaEntries(value, next));
    }
  }

  if (schema.definitions) {
    for (const [key, value] of Object.entries(schema.definitions)) {
      entries.push(...collectSchemaEntries(value, `definitions.${key}`));
    }
  }

  if (schema.items) {
    if (Array.isArray(schema.items)) {
      schema.items.forEach((item, index) => {
        entries.push(...collectSchemaEntries(item, `${prefix}[]${index}`));
      });
    } else {
      entries.push(...collectSchemaEntries(schema.items, `${prefix}[]`));
    }
  }

  for (const [label, variants] of [["anyOf", schema.anyOf], ["oneOf", schema.oneOf], ["allOf", schema.allOf]] as const) {
    variants?.forEach((variant, index) => {
      const next = prefix ? `${prefix}.${label}[${index}]` : `${label}[${index}]`;
      entries.push(...collectSchemaEntries(variant, next));
    });
  }

  return entries;
}

function ConfigPage() {
  const { data: config, isLoading } = useConfig();
  const { data: doctor, isLoading: doctorLoading } = useDoctorResults();
  const { data: schema, isLoading: schemaLoading } = useConfigSchema();
  const doctorRun = useDoctorRun();
  const updateConfig = useUpdateConfig();
  const [search, setSearch] = useState("");
  const [editMode, setEditMode] = useState(false);
  const [editJson, setEditJson] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [revealSensitive, setRevealSensitive] = useState(false);

  const configSections = useMemo(() => {
    if (!config) return [];

    return Object.entries(config).filter(([key]) =>
      search ? key.toLowerCase().includes(search.toLowerCase()) : true,
    );
  }, [config, search]);

  const flattenedConfig = useMemo(() => {
    if (!config) return [];
    return flattenConfig(config).filter((entry) =>
      search ? entry.key.toLowerCase().includes(search.toLowerCase()) : true,
    );
  }, [config, search]);

  const schemaEntries = useMemo(() => {
    if (!schema) return [];
    return collectSchemaEntries(schema).filter((entry) =>
      search ? entry.key.toLowerCase().includes(search.toLowerCase()) || String(entry.value).toLowerCase().includes(search.toLowerCase()) : true,
    );
  }, [schema, search]);

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
    <div className="space-y-8">
      <div className="flex items-center justify-between gap-6">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Configuration & Doctor</h1>
          <p className="mt-1 text-muted-foreground">
            Effective config, override provenance, masked secrets, and live doctor health.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant={statusVariant(doctor?.overall ?? "outline")} className="gap-1 capitalize">
            <CheckCircle2 className="h-3.5 w-3.5" />
            {doctor?.overall ?? "live"}
          </Badge>
          <Button
            variant="outline"
            size="sm"
            onClick={() => doctorRun.mutate()}
            disabled={doctorRun.isPending}
          >
            <RefreshCw className={`mr-2 h-4 w-4 ${doctorRun.isPending ? "animate-spin" : ""}`} />
            {doctorRun.isPending ? "Running..." : "Run doctor"}
          </Button>
        </div>
      </div>

      <Tabs defaultValue="viewer">
        <TabsList>
          <TabsTrigger value="viewer">
            <Settings2 className="mr-2 h-4 w-4" />
            Config viewer
          </TabsTrigger>
          <TabsTrigger value="doctor">
            <Stethoscope className="mr-2 h-4 w-4" />
            Doctor results
          </TabsTrigger>
          <TabsTrigger value="schema">
            <Braces className="mr-2 h-4 w-4" />
            Schema
          </TabsTrigger>
          <TabsTrigger value="json">
            <FileJson className="mr-2 h-4 w-4" />
            JSON
          </TabsTrigger>
          <TabsTrigger value="diff">
            <GitCompareArrows className="mr-2 h-4 w-4" />
            Diff
          </TabsTrigger>
        </TabsList>

        <TabsContent value="viewer" className="space-y-6">
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                placeholder="Search config paths or sections..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="pl-10"
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setRevealSensitive((current) => !current)}
            >
              {revealSensitive ? (
                <EyeOff className="mr-2 h-4 w-4" />
              ) : (
                <Eye className="mr-2 h-4 w-4" />
              )}
              {revealSensitive ? "Hide secrets" : "Reveal secrets"}
            </Button>
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
            <>
              <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
                {configSections.map(([section, value]) => (
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
                ))}
              </div>

              <Card>
                <CardHeader className="flex flex-row items-center justify-between gap-3">
                  <div>
                    <CardTitle className="flex items-center gap-2 text-base">
                      <Shield className="h-4 w-4" />
                      Effective values & override sources
                    </CardTitle>
                    <p className="mt-1 text-sm text-muted-foreground">
                      Effective value view with inferred provenance labels for defaults, global config, project overrides, and secrets.
                    </p>
                  </div>
                  <Badge variant="outline">{flattenedConfig.length} entries</Badge>
                </CardHeader>
                <CardContent>
                  <div className="space-y-2">
                    {flattenedConfig.map((entry) => (
                      <div key={entry.key} className="rounded-lg border p-3">
                        <div className="flex flex-col gap-2 lg:flex-row lg:items-start lg:justify-between">
                          <div className="min-w-0">
                            <p className="font-mono text-xs text-muted-foreground">{entry.key}</p>
                            <p className="mt-1 break-all text-sm font-medium">
                              {entry.sensitive ? maskValue(entry.value, revealSensitive) : maskValue(entry.value, true)}
                            </p>
                          </div>
                          <div className="flex flex-wrap items-center gap-2">
                            <Badge variant="outline">{entry.source}</Badge>
                            {entry.sensitive && <Badge variant="secondary">masked</Badge>}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>
            </>
          )}
        </TabsContent>

        <TabsContent value="doctor" className="space-y-6">
          {doctorLoading ? (
            <div className="space-y-4">
              <Skeleton className="h-32 w-full" />
              <Skeleton className="h-48 w-full" />
              <Skeleton className="h-48 w-full" />
            </div>
          ) : !doctor ? (
            <Card>
              <CardContent className="pt-6">
                <p className="text-sm text-muted-foreground">Doctor report unavailable.</p>
              </CardContent>
            </Card>
          ) : (
            <>
              <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Overall status</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <Badge variant={statusVariant(doctor.overall)} className="capitalize">
                      {doctor.overall}
                    </Badge>
                    <p className="mt-3 text-sm text-muted-foreground">Run at {doctor.run_at}</p>
                  </CardContent>
                </Card>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Topology</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-2 text-sm">
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Deployment</span>
                      <span>{doctor.topology?.deployment ?? "—"}</span>
                    </div>
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Database</span>
                      <span>{doctor.topology?.database ?? "—"}</span>
                    </div>
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Models</span>
                      <span>{doctor.topology?.models ?? "—"}</span>
                    </div>
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Search</span>
                      <span>{doctor.topology?.search ?? "—"}</span>
                    </div>
                  </CardContent>
                </Card>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Paths</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-2 text-sm">
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Profile</span>
                      <span>{doctor.paths?.profile ?? "—"}</span>
                    </div>
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Mode</span>
                      <span>{doctor.paths?.mode ?? "—"}</span>
                    </div>
                    <div className="flex justify-between gap-3">
                      <span className="text-muted-foreground">Auto-create</span>
                      <span>{doctor.paths?.auto_create_missing ? "yes" : "no"}</span>
                    </div>
                  </CardContent>
                </Card>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle className="text-base">Provider / channel health matrix</CardTitle>
                </CardHeader>
                <CardContent>
                  <HealthMatrix entries={doctor.backend_matrix} />
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-base">Doctor checks & fix-it suggestions</CardTitle>
                </CardHeader>
                <CardContent>
                  <DoctorChecks checks={doctor.checks} />
                </CardContent>
              </Card>
            </>
          )}
        </TabsContent>


        <TabsContent value="schema" className="space-y-6">
          {schemaLoading ? (
            <div className="space-y-4">
              <Skeleton className="h-32 w-full" />
              <Skeleton className="h-64 w-full" />
            </div>
          ) : !schema ? (
            <Card>
              <CardContent className="pt-6">
                <p className="text-sm text-muted-foreground">Schema unavailable.</p>
              </CardContent>
            </Card>
          ) : (
            <>
              <Card>
                <CardHeader className="flex flex-row items-center justify-between gap-3">
                  <div>
                    <CardTitle className="text-base">Config schema overview</CardTitle>
                    <p className="mt-1 text-sm text-muted-foreground">
                      Runtime JSON Schema for the effective Rune config. Use search to inspect field types, defaults, refs, and enum constraints.
                    </p>
                  </div>
                  <Badge variant="outline">{schemaEntries.length} nodes</Badge>
                </CardHeader>
                <CardContent>
                  <div className="space-y-2">
                    {schemaEntries.map((entry) => (
                      <div key={entry.key} className="rounded-lg border p-3">
                        <p className="font-mono text-xs text-muted-foreground">{entry.key}</p>
                        <p className="mt-1 text-sm">{String(entry.value)}</p>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-base">Raw JSON Schema</CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="overflow-x-auto rounded-md bg-muted p-4 text-sm">
                    {JSON.stringify(schema, null, 2)}
                  </pre>
                </CardContent>
              </Card>
            </>
          )}
        </TabsContent>

        <TabsContent value="diff">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Current vs edited JSON</CardTitle>
              <p className="text-sm text-muted-foreground">
                Quick visual diff between the live effective config and the editor buffer. Enter edit mode to stage changes.
              </p>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
                <div>
                  <p className="mb-2 text-sm font-medium">Current</p>
                  <pre className="min-h-[420px] overflow-x-auto rounded-md bg-muted p-4 text-sm">{rawJson}</pre>
                </div>
                <div>
                  <p className="mb-2 text-sm font-medium">Editor buffer</p>
                  <pre className="min-h-[420px] overflow-x-auto rounded-md bg-muted p-4 text-sm">{editMode ? editJson : rawJson}</pre>
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="json">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-3">
              <CardTitle className="text-base">
                {editMode ? "Edit Configuration" : "Full Configuration"}
              </CardTitle>
              <div className="flex gap-3">
                <Button variant="outline" size="sm" onClick={handleEditToggle}>
                  {editMode ? "Cancel" : "Edit"}
                </Button>
                {editMode && (
                  <Button size="sm" onClick={handleSave} disabled={updateConfig.isPending}>
                    <Save className="mr-2 h-4 w-4" />
                    {updateConfig.isPending ? "Saving..." : "Save"}
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {saveError && <p className="text-sm text-red-600">{saveError}</p>}
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
