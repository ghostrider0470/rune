import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useState, useRef, useCallback, useMemo } from "react";
import { motion, AnimatePresence } from "motion/react";
import ReactECharts from "echarts-for-react";
import type { EChartsOption } from "echarts";
import { api } from "@/lib/api-client";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import {
  BrainCircuit,
  Link2,
  TrendingUp,
  Search,
  X,
  RotateCcw,
  ZoomIn,
  ZoomOut,
  Maximize2,
} from "lucide-react";
import { Button } from "@/components/ui/button";

export const Route = createFileRoute("/_admin/memory")({
  component: MemoryGraphPage,
});

// ── Types ──────────────────────────────────────────────────────────

interface MemoryNode {
  id: string;
  fact: string;
  category: string;
  session_id: string | null;
  created_at: string;
  access_count: number;
}

interface MemoryEdge {
  source: string;
  target: string;
  similarity: number;
}

interface GraphResponse {
  nodes: MemoryNode[];
  edges: MemoryEdge[];
}

type LayoutMode = "force" | "circular";
type SizeMode = "access" | "connections" | "recency";

// ── Categories ─────────────────────────────────────────────────────

const CATEGORIES = [
  { name: "preference", color: "#f59e0b", glow: "rgba(245,158,11,0.4)" },
  { name: "project", color: "#3b82f6", glow: "rgba(59,130,246,0.4)" },
  { name: "ops", color: "#10b981", glow: "rgba(16,185,129,0.4)" },
  { name: "decision", color: "#8b5cf6", glow: "rgba(139,92,246,0.4)" },
  { name: "person", color: "#ec4899", glow: "rgba(236,72,153,0.4)" },
  { name: "technical", color: "#06b6d4", glow: "rgba(6,182,212,0.4)" },
  { name: "workflow", color: "#f97316", glow: "rgba(249,115,22,0.4)" },
  { name: "general", color: "#6b7280", glow: "rgba(107,114,128,0.4)" },
];

const CATEGORY_INDEX = new Map(CATEGORIES.map((c, i) => [c.name, i]));

function getCategoryIndex(cat: string): number {
  return CATEGORY_INDEX.get(cat.toLowerCase()) ?? CATEGORIES.length - 1;
}

// ── Stats helpers ──────────────────────────────────────────────────

function computeStats(data: GraphResponse) {
  const catCounts: Record<string, number> = {};
  let totalAccess = 0;
  let oldest = Infinity;
  let newest = 0;

  const connectionCount = new Map<string, number>();

  for (const n of data.nodes) {
    catCounts[n.category] = (catCounts[n.category] ?? 0) + 1;
    totalAccess += n.access_count;
    const t = new Date(n.created_at).getTime();
    if (t < oldest) oldest = t;
    if (t > newest) newest = t;
    connectionCount.set(n.id, 0);
  }

  for (const e of data.edges) {
    connectionCount.set(e.source, (connectionCount.get(e.source) ?? 0) + 1);
    connectionCount.set(e.target, (connectionCount.get(e.target) ?? 0) + 1);
  }

  const topAccessed = [...data.nodes]
    .sort((a, b) => b.access_count - a.access_count)
    .slice(0, 5);

  const mostConnected = [...connectionCount.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([id, count]) => ({
      node: data.nodes.find((n) => n.id === id)!,
      count,
    }))
    .filter((x) => x.node);

  const avgSimilarity =
    data.edges.length > 0
      ? data.edges.reduce((s, e) => s + e.similarity, 0) / data.edges.length
      : 0;

  return {
    catCounts,
    totalAccess,
    oldest,
    newest,
    topAccessed,
    mostConnected,
    avgSimilarity,
    connectionCount,
  };
}

// ── Build ECharts option ───────────────────────────────────────────

function buildChartOption(
  data: GraphResponse,
  isDark: boolean,
  layout: LayoutMode,
  sizeMode: SizeMode,
  searchQuery: string,
  enabledCategories: Set<string>,
): EChartsOption {
  const stats = computeStats(data);

  // Filter nodes by category and search
  const filteredNodes = data.nodes.filter((n) => {
    if (!enabledCategories.has(n.category)) return false;
    if (searchQuery && !n.fact.toLowerCase().includes(searchQuery.toLowerCase()))
      return false;
    return true;
  });

  const filteredIds = new Set(filteredNodes.map((n) => n.id));

  const filteredEdges = data.edges.filter(
    (e) => filteredIds.has(e.source) && filteredIds.has(e.target),
  );

  // Compute node sizes based on sizeMode
  const now = Date.now();
  function getNodeSize(n: MemoryNode): number {
    switch (sizeMode) {
      case "access":
        return Math.max(10, Math.min(45, 10 + n.access_count * 4));
      case "connections": {
        const conns = stats.connectionCount.get(n.id) ?? 0;
        return Math.max(10, Math.min(45, 10 + conns * 5));
      }
      case "recency": {
        const age = now - new Date(n.created_at).getTime();
        const hours = age / (1000 * 60 * 60);
        // Newer = bigger. Max size within last hour, min after 30 days
        return Math.max(10, Math.min(45, 45 - Math.min(35, hours / 20)));
      }
    }
  }

  const isSearching = searchQuery.length > 0;

  const nodes = filteredNodes.map((n) => {
    const catIdx = getCategoryIndex(n.category);
    const cat = CATEGORIES[catIdx];
    const matchesSearch =
      !isSearching || n.fact.toLowerCase().includes(searchQuery.toLowerCase());

    return {
      id: n.id,
      name: n.fact.length > 60 ? n.fact.slice(0, 57) + "..." : n.fact,
      symbolSize: getNodeSize(n),
      category: catIdx,
      value: n.fact,
      itemStyle: {
        borderWidth: matchesSearch && isSearching ? 3 : 1.5,
        borderColor: matchesSearch && isSearching
          ? "#fff"
          : isDark
            ? "rgba(255,255,255,0.12)"
            : "rgba(0,0,0,0.08)",
        shadowBlur: matchesSearch && isSearching ? 20 : 8,
        shadowColor: matchesSearch && isSearching ? cat.glow : cat.glow.replace("0.4", "0.15"),
        opacity: isSearching && !matchesSearch ? 0.2 : 1,
      },
      label: {
        show: false,
      },
      emphasis: {
        label: {
          show: true,
          fontSize: 11,
          color: isDark ? "#f3f4f6" : "#111827",
          backgroundColor: isDark ? "rgba(0,0,0,0.8)" : "rgba(255,255,255,0.9)",
          borderRadius: 4,
          padding: [4, 8],
          formatter: (p: { name?: string }) => {
            const text = p.name ?? "";
            return text.length > 45
              ? text.replace(/(.{1,45})(\s|$)/g, "$1\n").trim()
              : text;
          },
        },
        itemStyle: {
          shadowBlur: 25,
          shadowColor: cat.glow,
          borderWidth: 2.5,
          borderColor: "#fff",
        },
      },
      _raw: n,
    };
  });

  const edges = filteredEdges.map((e) => ({
    source: e.source,
    target: e.target,
    lineStyle: {
      width: 0.5 + e.similarity * 2.5,
      opacity: isDark
        ? 0.08 + e.similarity * 0.35
        : 0.05 + e.similarity * 0.25,
      color: isDark ? "rgba(148,163,184,0.6)" : "rgba(100,116,139,0.4)",
      curveness: 0.15,
    },
  }));

  const forceConfig =
    layout === "force"
      ? {
          repulsion: Math.max(120, 50 + filteredNodes.length * 3),
          gravity: 0.04,
          edgeLength: [60, 280],
          layoutAnimation: true,
          friction: 0.55,
        }
      : undefined;

  return {
    backgroundColor: "transparent",
    tooltip: {
      trigger: "item",
      confine: true,
      backgroundColor: isDark ? "rgba(15,15,20,0.96)" : "rgba(255,255,255,0.98)",
      borderColor: isDark ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.06)",
      borderWidth: 1,
      padding: [12, 16],
      textStyle: { color: isDark ? "#e5e7eb" : "#1f2937", fontSize: 12 },
      extraCssText: "border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,0.2);backdrop-filter:blur(8px);max-width:360px;",
      formatter: (params: unknown) => {
        const p = params as { data?: { _raw?: MemoryNode } };
        const raw = p.data?._raw;
        if (!raw) return "";
        const date = new Date(raw.created_at).toLocaleDateString("en-US", {
          month: "short",
          day: "numeric",
          year: "numeric",
        });
        const conns = stats.connectionCount.get(raw.id) ?? 0;
        const catColor = CATEGORIES[getCategoryIndex(raw.category)]?.color ?? "#6b7280";
        return `
          <div>
            <div style="display:flex;align-items:center;gap:6px;margin-bottom:8px">
              <span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${catColor}"></span>
              <span style="font-weight:600;text-transform:capitalize;font-size:13px">${raw.category}</span>
            </div>
            <div style="line-height:1.6;margin-bottom:10px;opacity:0.95">${raw.fact}</div>
            <div style="display:flex;gap:12px;opacity:0.5;font-size:11px">
              <span>${raw.access_count}x accessed</span>
              <span>${conns} connections</span>
              <span>${date}</span>
            </div>
          </div>
        `;
      },
    },
    legend: {
      show: false, // We use our own category toggles
    },
    animationDuration: 600,
    animationDurationUpdate: 400,
    animationEasingUpdate: "cubicOut",
    series: [
      {
        type: "graph",
        layout: layout,
        roam: true,
        draggable: true,
        data: nodes,
        links: edges,
        categories: CATEGORIES.map((c) => ({
          name: c.name,
          itemStyle: {
            color: c.color,
          },
        })),
        ...(forceConfig ? { force: forceConfig } : { circular: { rotateLabel: false } }),
        emphasis: {
          focus: "adjacency",
          lineStyle: {
            width: 3,
            opacity: 0.85,
          },
          itemStyle: {
            shadowBlur: 30,
          },
        },
        blur: {
          itemStyle: { opacity: 0.15 },
          lineStyle: { opacity: 0.03 },
        },
        scaleLimit: { min: 0.2, max: 12 },
        lineStyle: { curveness: 0.15 },
        selectedMode: "single",
        select: {
          itemStyle: {
            borderWidth: 3,
            borderColor: "#fff",
            shadowBlur: 30,
          },
        },
      },
    ],
  };
}

// ── Stat cards ─────────────────────────────────────────────────────

function StatsBar({ data }: { data: GraphResponse }) {
  const stats = useMemo(() => computeStats(data), [data]);

  return (
    <div className="grid grid-cols-4 gap-6">
      <Card className="bg-card/60 backdrop-blur">
        <CardContent className="flex items-center gap-3 p-3">
          <div className="rounded-lg bg-primary/10 p-2">
            <BrainCircuit className="h-4 w-4 text-primary" />
          </div>
          <div>
            <p className="text-2xl font-bold leading-none">{data.nodes.length}</p>
            <p className="text-[11px] text-muted-foreground">memories</p>
          </div>
        </CardContent>
      </Card>
      <Card className="bg-card/60 backdrop-blur">
        <CardContent className="flex items-center gap-3 p-3">
          <div className="rounded-lg bg-blue-500/10 p-2">
            <Link2 className="h-4 w-4 text-blue-500" />
          </div>
          <div>
            <p className="text-2xl font-bold leading-none">{data.edges.length}</p>
            <p className="text-[11px] text-muted-foreground">connections</p>
          </div>
        </CardContent>
      </Card>
      <Card className="bg-card/60 backdrop-blur">
        <CardContent className="flex items-center gap-3 p-3">
          <div className="rounded-lg bg-violet-500/10 p-2">
            <TrendingUp className="h-4 w-4 text-violet-500" />
          </div>
          <div>
            <p className="text-2xl font-bold leading-none">{stats.avgSimilarity.toFixed(2)}</p>
            <p className="text-[11px] text-muted-foreground">avg similarity</p>
          </div>
        </CardContent>
      </Card>
      <Card className="bg-card/60 backdrop-blur">
        <CardContent className="flex items-center gap-3 p-3">
          <div className="rounded-lg bg-amber-500/10 p-2">
            <TrendingUp className="h-4 w-4 text-amber-500" />
          </div>
          <div>
            <p className="text-2xl font-bold leading-none">{stats.totalAccess}</p>
            <p className="text-[11px] text-muted-foreground">total recalls</p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

// ── Category distribution mini bar ─────────────────────────────────

function CategoryBar({
  data,
  enabled,
  onToggle,
}: {
  data: GraphResponse;
  enabled: Set<string>;
  onToggle: (cat: string) => void;
}) {
  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const n of data.nodes) c[n.category] = (c[n.category] ?? 0) + 1;
    return c;
  }, [data]);

  const total = data.nodes.length || 1;

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {CATEGORIES.map((cat) => {
        const count = counts[cat.name] ?? 0;
        if (count === 0) return null;
        const active = enabled.has(cat.name);
        const pct = ((count / total) * 100).toFixed(0);
        return (
          <button
            key={cat.name}
            onClick={() => onToggle(cat.name)}
            className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium transition-all ${
              active
                ? "border-transparent text-white"
                : "border-border bg-background/50 text-muted-foreground opacity-40"
            }`}
            style={active ? { backgroundColor: cat.color } : undefined}
          >
            <span className="capitalize">{cat.name}</span>
            <span className={active ? "opacity-70" : ""}>{count} ({pct}%)</span>
          </button>
        );
      })}
    </div>
  );
}

// ── Detail panel ───────────────────────────────────────────────────

function DetailPanel({
  node,
  data,
  onClose,
}: {
  node: MemoryNode;
  data: GraphResponse;
  onClose: () => void;
}) {
  const stats = useMemo(() => computeStats(data), [data]);
  const connections = useMemo(() => {
    const connected: { node: MemoryNode; similarity: number }[] = [];
    for (const e of data.edges) {
      if (e.source === node.id) {
        const n = data.nodes.find((x) => x.id === e.target);
        if (n) connected.push({ node: n, similarity: e.similarity });
      } else if (e.target === node.id) {
        const n = data.nodes.find((x) => x.id === e.source);
        if (n) connected.push({ node: n, similarity: e.similarity });
      }
    }
    return connected.sort((a, b) => b.similarity - a.similarity);
  }, [node, data]);

  const catColor = CATEGORIES[getCategoryIndex(node.category)]?.color ?? "#6b7280";
  const connCount = stats.connectionCount.get(node.id) ?? 0;

  return (
    <motion.div
      key={node.id}
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ duration: 0.25, ease: "easeOut" }}
      className="w-80 shrink-0"
    >
      <Card className="h-full overflow-auto border-none bg-card/80 backdrop-blur-xl shadow-xl">
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm flex items-center gap-2 capitalize">
              <span
                className="inline-block h-3 w-3 rounded-full shadow-lg"
                style={{ backgroundColor: catColor, boxShadow: `0 0 8px ${catColor}` }}
              />
              {node.category}
            </CardTitle>
            <button
              onClick={onClose}
              className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </CardHeader>
        <CardContent className="space-y-4 text-sm">
          <p className="leading-relaxed text-foreground/90">{node.fact}</p>

          <div className="flex flex-wrap gap-2">
            <Badge variant="secondary" className="text-[11px]">
              {node.access_count}x accessed
            </Badge>
            <Badge variant="secondary" className="text-[11px]">
              {connCount} connections
            </Badge>
            <Badge variant="outline" className="text-[11px]">
              {new Date(node.created_at).toLocaleDateString("en-US", {
                month: "short",
                day: "numeric",
                year: "numeric",
              })}
            </Badge>
          </div>

          {node.session_id && (
            <p className="text-xs text-muted-foreground truncate font-mono">
              {node.session_id}
            </p>
          )}

          {connections.length > 0 && (
            <div className="space-y-2 pt-2 border-t">
              <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                Connected memories
              </p>
              <div className="space-y-1.5 max-h-48 overflow-y-auto">
                {connections.map((c) => (
                  <div
                    key={c.node.id}
                    className="flex items-start gap-2 rounded-lg bg-muted/40 p-2 text-xs"
                  >
                    <span
                      className="mt-1 inline-block h-2 w-2 shrink-0 rounded-full"
                      style={{
                        backgroundColor: CATEGORIES[getCategoryIndex(c.node.category)]?.color,
                      }}
                    />
                    <div className="min-w-0 flex-1">
                      <p className="truncate leading-snug">{c.node.fact}</p>
                      <p className="text-muted-foreground mt-0.5">
                        {(c.similarity * 100).toFixed(0)}% similar
                      </p>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}

// ── Page component ─────────────────────────────────────────────────

function MemoryGraphPage() {
  const [threshold, setThreshold] = useState(0.45);
  const [neighbors, setNeighbors] = useState(5);
  const [selectedNode, setSelectedNode] = useState<MemoryNode | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [layout, setLayout] = useState<LayoutMode>("force");
  const [sizeMode, setSizeMode] = useState<SizeMode>("access");
  const [enabledCategories, setEnabledCategories] = useState<Set<string>>(
    () => new Set(CATEGORIES.map((c) => c.name)),
  );
  const chartRef = useRef<ReactECharts | null>(null);

  const isDark =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("dark");

  const {
    data: graph,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["memory-graph", threshold, neighbors],
    queryFn: () =>
      api.get<GraphResponse>(
        `/api/memory/graph?threshold=${threshold}&neighbors=${neighbors}`,
      ),
    refetchInterval: 10000,
    placeholderData: (prev) => prev,
  });

  const onChartClick = useCallback(
    (params: { data?: { _raw?: MemoryNode } }) => {
      const raw = params.data?._raw;
      if (raw) setSelectedNode(raw);
    },
    [],
  );

  const toggleCategory = useCallback((cat: string) => {
    setEnabledCategories((prev) => {
      const next = new Set(prev);
      if (next.has(cat)) {
        // Don't allow disabling all
        if (next.size > 1) next.delete(cat);
      } else {
        next.add(cat);
      }
      return next;
    });
  }, []);

  const resetView = useCallback(() => {
    chartRef.current?.getEchartsInstance()?.dispatchAction({ type: "restore" });
  }, []);

  const zoomIn = useCallback(() => {
    const instance = chartRef.current?.getEchartsInstance();
    if (instance) {
      const zoom = (instance.getOption() as { series: { zoom?: number }[] })?.series?.[0]?.zoom ?? 1;
      instance.dispatchAction({ type: "graphRoam", zoom: zoom * 1.3 });
    }
  }, []);

  const zoomOut = useCallback(() => {
    const instance = chartRef.current?.getEchartsInstance();
    if (instance) {
      const zoom = (instance.getOption() as { series: { zoom?: number }[] })?.series?.[0]?.zoom ?? 1;
      instance.dispatchAction({ type: "graphRoam", zoom: zoom * 0.7 });
    }
  }, []);

  const chartOption = useMemo(() => {
    if (!graph || graph.nodes.length === 0) return null;
    return buildChartOption(graph, isDark, layout, sizeMode, searchQuery, enabledCategories);
  }, [graph, isDark, layout, sizeMode, searchQuery, enabledCategories]);

  return (
    <div className="flex h-full flex-col gap-4 overflow-hidden">
      {/* Stats bar */}
      {graph && graph.nodes.length > 0 && <StatsBar data={graph} />}

      {/* Toolbar */}
      <div className="flex items-center gap-3 flex-wrap">
        {/* Search */}
        <div className="relative flex-1 min-w-[200px] max-w-xs">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search memories..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-8 pl-8 pr-8 text-xs"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery("")}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              <X className="h-3 w-3" />
            </button>
          )}
        </div>

        {/* Similarity */}
        <div className="flex items-center gap-2">
          <Label className="text-[11px] text-muted-foreground whitespace-nowrap">
            Similarity
          </Label>
          <input
            type="range"
            value={threshold}
            min={0.2}
            max={0.8}
            step={0.05}
            onChange={(e) => setThreshold(parseFloat(e.target.value))}
            className="w-20 accent-primary"
          />
          <span className="text-[11px] text-muted-foreground w-8">{threshold.toFixed(2)}</span>
        </div>

        {/* Neighbors */}
        <div className="flex items-center gap-2">
          <Label className="text-[11px] text-muted-foreground whitespace-nowrap">
            K
          </Label>
          <input
            type="range"
            value={neighbors}
            min={1}
            max={15}
            step={1}
            onChange={(e) => setNeighbors(parseInt(e.target.value))}
            className="w-16 accent-primary"
          />
          <span className="text-[11px] text-muted-foreground w-4">{neighbors}</span>
        </div>

        {/* Layout toggle */}
        <div className="flex items-center rounded-md border bg-muted/30 p-0.5">
          {(["force", "circular"] as LayoutMode[]).map((l) => (
            <button
              key={l}
              onClick={() => setLayout(l)}
              className={`rounded px-2.5 py-1 text-[11px] font-medium transition-all ${
                layout === l
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {l === "force" ? "Force" : "Circular"}
            </button>
          ))}
        </div>

        {/* Size mode */}
        <div className="flex items-center rounded-md border bg-muted/30 p-0.5">
          {([
            ["access", "Recalls"],
            ["connections", "Links"],
            ["recency", "Recent"],
          ] as [SizeMode, string][]).map(([m, label]) => (
            <button
              key={m}
              onClick={() => setSizeMode(m)}
              className={`rounded px-2.5 py-1 text-[11px] font-medium transition-all ${
                sizeMode === m
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {/* Zoom controls */}
        <div className="flex items-center gap-0.5 ml-auto">
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={zoomIn}>
            <ZoomIn className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={zoomOut}>
            <ZoomOut className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={resetView}>
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={() => {
              setSearchQuery("");
              setEnabledCategories(new Set(CATEGORIES.map((c) => c.name)));
              setLayout("force");
              setSizeMode("access");
              setThreshold(0.45);
              setNeighbors(5);
              setSelectedNode(null);
            }}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Category filters */}
      {graph && graph.nodes.length > 0 && (
        <CategoryBar
          data={graph}
          enabled={enabledCategories}
          onToggle={toggleCategory}
        />
      )}

      {/* Graph + Detail panel */}
      <div className="flex min-h-0 flex-1 gap-4">
        <div className="min-h-0 flex-1 rounded-xl border bg-gradient-to-br from-card/80 to-card/40 backdrop-blur overflow-hidden">
          {isLoading && !graph ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center gap-3">
                <Skeleton className="h-48 w-48 rounded-full" />
                <p className="text-sm text-muted-foreground animate-pulse">
                  Loading knowledge graph...
                </p>
              </div>
            </div>
          ) : error ? (
            <div className="flex h-full items-center justify-center text-destructive">
              {error instanceof Error ? error.message : "Failed to load graph"}
            </div>
          ) : chartOption ? (
            <ReactECharts
              ref={chartRef}
              option={chartOption}
              style={{ width: "100%", height: "100%" }}
              onEvents={{ click: onChartClick }}
              notMerge={true}
              lazyUpdate={true}
              theme={isDark ? "dark" : undefined}
            />
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground">
              <BrainCircuit className="h-16 w-16 opacity-20" />
              <p>No memories yet. Start chatting to build the knowledge graph.</p>
            </div>
          )}
        </div>

        {/* Detail panel */}
        <AnimatePresence>
          {selectedNode && graph && (
            <DetailPanel
              node={selectedNode}
              data={graph}
              onClose={() => setSelectedNode(null)}
            />
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
