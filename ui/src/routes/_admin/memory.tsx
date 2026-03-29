import { createFileRoute } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "motion/react";
import Graph from "graphology";
import {
  SigmaContainer,
  useRegisterEvents,
  useSigma,
  useLoadGraph,
  useSetSettings,
} from "@react-sigma/core";
import "@react-sigma/core/lib/style.css";
import forceAtlas2 from "graphology-layout-forceatlas2";
import { api } from "@/lib/api-client";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  BrainCircuit,
  Link2,
  Search,
  X,
  RefreshCw,
  Trash2,
  Zap,
  ChevronRight,
} from "lucide-react";

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

// ── Categories ─────────────────────────────────────────────────────

const CATEGORIES = [
  { name: "preference", color: "#f59e0b" },
  { name: "project", color: "#3b82f6" },
  { name: "identity", color: "#a855f7" },
  { name: "ops", color: "#10b981" },
  { name: "decision", color: "#8b5cf6" },
  { name: "person", color: "#ec4899" },
  { name: "technical", color: "#06b6d4" },
  { name: "workflow", color: "#f97316" },
  { name: "general", color: "#6b7280" },
] as const;

const CATEGORY_COLORS = new Map<string, string>(
  CATEGORIES.map((c) => [c.name, c.color]),
);

function getCategoryColor(cat: string): string {
  return CATEGORY_COLORS.get(cat.toLowerCase()) ?? "#6b7280";
}

// ── Build graphology graph from API data ───────────────────────────

function buildGraph(
  data: GraphResponse,
  enabledCategories: Set<string>,
  searchQuery: string,
): Graph {
  const graph = new Graph();

  const connectionCount = new Map<string, number>();
  for (const e of data.edges) {
    connectionCount.set(e.source, (connectionCount.get(e.source) ?? 0) + 1);
    connectionCount.set(e.target, (connectionCount.get(e.target) ?? 0) + 1);
  }

  const filteredNodes = data.nodes.filter((n) => {
    if (!enabledCategories.has(n.category)) return false;
    if (
      searchQuery &&
      !n.fact.toLowerCase().includes(searchQuery.toLowerCase())
    )
      return false;
    return true;
  });

  const nodeIds = new Set(filteredNodes.map((n) => n.id));

  for (const n of filteredNodes) {
    const connections = connectionCount.get(n.id) ?? 0;
    const size = Math.max(4, Math.min(20, 4 + connections * 3));
    graph.addNode(n.id, {
      label: n.fact.length > 50 ? n.fact.slice(0, 47) + "..." : n.fact,
      size,
      color: getCategoryColor(n.category),
      x: Math.random() * 100,
      y: Math.random() * 100,
      fact: n.fact,
      category: n.category,
      session_id: n.session_id,
      created_at: n.created_at,
      access_count: n.access_count,
    });
  }

  for (const e of data.edges) {
    if (nodeIds.has(e.source) && nodeIds.has(e.target)) {
      const key = `${e.source}--${e.target}`;
      if (!graph.hasEdge(key)) {
        graph.addEdgeWithKey(key, e.source, e.target, {
          size: 0.5 + e.similarity * 2,
          color: "rgba(128,128,128,0.15)",
          similarity: e.similarity,
        });
      }
    }
  }

  if (graph.order > 0) {
    forceAtlas2.assign(graph, {
      iterations: 100,
      settings: {
        gravity: 1,
        scalingRatio: 10,
        barnesHutOptimize: graph.order > 100,
        slowDown: 5,
      },
    });
  }

  return graph;
}

// ── Sigma child: load graph into renderer ──────────────────────────

function GraphLoader({ graphData }: { graphData: Graph }) {
  const loadGraph = useLoadGraph();
  const sigma = useSigma();
  const prevRef = useRef<Graph | null>(null);

  useEffect(() => {
    if (graphData !== prevRef.current) {
      prevRef.current = graphData;
      loadGraph(graphData);
      requestAnimationFrame(() => {
        sigma.getCamera().animatedReset({ duration: 300 });
      });
    }
  }, [graphData, loadGraph, sigma]);

  return null;
}

// ── Sigma child: event handling + hover highlighting ───────────────

function GraphEvents({
  onSelectNode,
  hoveredNode,
  setHoveredNode,
}: {
  onSelectNode: (id: string | null) => void;
  hoveredNode: string | null;
  setHoveredNode: (id: string | null) => void;
}) {
  const sigma = useSigma();
  const registerEvents = useRegisterEvents();
  const setSettings = useSetSettings();

  useEffect(() => {
    registerEvents({
      clickNode: (event) => onSelectNode(event.node),
      clickStage: () => onSelectNode(null),
      enterNode: (event) => setHoveredNode(event.node),
      leaveNode: () => setHoveredNode(null),
    });
  }, [registerEvents, onSelectNode, setHoveredNode]);

  useEffect(() => {
    const container = sigma.getContainer();
    container.style.cursor = hoveredNode ? "pointer" : "default";
  }, [hoveredNode, sigma]);

  useEffect(() => {
    const graph = sigma.getGraph();

    if (!hoveredNode) {
      setSettings({ nodeReducer: undefined, edgeReducer: undefined });
      return;
    }

    const neighbors = new Set(graph.neighbors(hoveredNode));
    neighbors.add(hoveredNode);

    const neighborEdges = new Set<string>();
    graph.forEachEdge(hoveredNode, (edge) => neighborEdges.add(edge));

    setSettings({
      nodeReducer: (node, data) => {
        if (neighbors.has(node)) {
          return {
            ...data,
            zIndex: 1,
            highlighted: node === hoveredNode,
          };
        }
        return { ...data, color: "#2a2a2a", label: "", zIndex: 0 };
      },
      edgeReducer: (edge, data) => {
        if (neighborEdges.has(edge)) {
          const hoverColor = graph.getNodeAttribute(hoveredNode, "color");
          return {
            ...data,
            color: hoverColor + "80",
            size: Math.max(data.size ?? 1, 1.5),
            zIndex: 1,
          };
        }
        return { ...data, color: "rgba(128,128,128,0.03)", zIndex: 0 };
      },
    });
  }, [hoveredNode, sigma, setSettings]);

  return null;
}

// ── Page ───────────────────────────────────────────────────────────

function MemoryGraphPage() {
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [threshold, setThreshold] = useState(0.4);
  const [neighbors, setNeighbors] = useState(8);
  const [enabledCategories, setEnabledCategories] = useState<Set<string>>(
    () => new Set(CATEGORIES.map((c) => c.name)),
  );
  const [containerHeight, setContainerHeight] = useState(600);
  const measureRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();

  const isDark =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("dark");

  // Measure container height via ResizeObserver
  useEffect(() => {
    const el = measureRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      setContainerHeight(entry.contentRect.height);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // ── Data fetch ───────────────────────────────────────────────

  const {
    data: rawGraph,
    isLoading,
    error,
    refetch,
  } = useQuery({
    queryKey: ["memory-graph", threshold, neighbors],
    queryFn: () =>
      api.get<GraphResponse>(
        `/api/memory/graph?threshold=${threshold}&neighbors=${neighbors}`,
      ),
  });

  // ── Delete mutation ──────────────────────────────────────────

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.delete(`/api/memory/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["memory-graph"] });
      setSelectedNodeId(null);
    },
  });

  // ── Build graphology graph ───────────────────────────────────

  const graphData = useMemo(() => {
    if (!rawGraph) return new Graph();
    return buildGraph(rawGraph, enabledCategories, searchQuery);
  }, [rawGraph, enabledCategories, searchQuery]);

  // ── Selected node data ───────────────────────────────────────

  const selectedNode = useMemo(() => {
    if (!selectedNodeId || !rawGraph) return null;
    return rawGraph.nodes.find((n) => n.id === selectedNodeId) ?? null;
  }, [selectedNodeId, rawGraph]);

  // ── Stats ────────────────────────────────────────────────────

  const stats = useMemo(() => {
    if (!rawGraph) return null;
    const totalAccess = rawGraph.nodes.reduce(
      (s, n) => s + n.access_count,
      0,
    );
    return {
      memories: rawGraph.nodes.length,
      connections: rawGraph.edges.length,
      totalRecalls: totalAccess,
    };
  }, [rawGraph]);

  // ── Category counts ──────────────────────────────────────────

  const categoryCounts = useMemo(() => {
    if (!rawGraph) return new Map<string, number>();
    const counts = new Map<string, number>();
    for (const n of rawGraph.nodes) {
      counts.set(n.category, (counts.get(n.category) ?? 0) + 1);
    }
    return counts;
  }, [rawGraph]);

  // ── Handlers ─────────────────────────────────────────────────

  const onSelectNode = useCallback((id: string | null) => {
    setSelectedNodeId(id);
  }, []);

  const toggleCategory = useCallback((cat: string) => {
    setEnabledCategories((prev) => {
      const next = new Set(prev);
      if (next.has(cat)) {
        if (next.size > 1) next.delete(cat);
      } else {
        next.add(cat);
      }
      return next;
    });
  }, []);

  // ── Sigma settings ───────────────────────────────────────────

  const sigmaSettings = useMemo(
    () => ({
      defaultNodeColor: "#6b7280",
      defaultEdgeColor: "rgba(128,128,128,0.15)",
      labelColor: { color: isDark ? "#cbd5e1" : "#334155" },
      labelFont: "Inter, system-ui, sans-serif",
      labelSize: 12,
      labelWeight: "500",
      labelRenderedSizeThreshold: 8,
      renderEdgeLabels: false,
      enableEdgeEvents: false,
      allowInvalidContainer: true,
      zIndex: true,
      minCameraRatio: 0.05,
      maxCameraRatio: 10,
    }),
    [isDark],
  );

  // ── Render ───────────────────────────────────────────────────

  const bgColor = isDark ? "#0a0a0f" : "#fafafa";

  return (
    <div
      ref={measureRef}
      className="relative flex w-full overflow-hidden"
      style={{ height: "calc(100dvh - 10rem)" }}
    >
      {/* Graph area */}
      <div
        className="relative flex-1"
        style={{ background: bgColor, height: containerHeight }}
      >
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-4">
              <BrainCircuit className="h-16 w-16 animate-pulse text-muted-foreground/30" />
              <p className="text-sm text-muted-foreground animate-pulse">
                Loading knowledge graph...
              </p>
            </div>
          </div>
        ) : error ? (
          <div className="flex h-full flex-col items-center justify-center gap-4">
            <p className="text-sm text-destructive">
              {error instanceof Error ? error.message : "Failed to load graph"}
            </p>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="mr-2 h-3.5 w-3.5" />
              Retry
            </Button>
          </div>
        ) : graphData.order === 0 && !searchQuery ? (
          <div className="flex h-full flex-col items-center justify-center gap-4 text-muted-foreground">
            <BrainCircuit className="h-20 w-20 opacity-15" />
            <p className="text-base">No memories yet</p>
            <p className="text-xs opacity-60">
              Start chatting to build the knowledge graph
            </p>
          </div>
        ) : (
          <SigmaContainer
            style={{
              height: containerHeight,
              width: "100%",
              background: bgColor,
            }}
            settings={sigmaSettings}
          >
            <GraphLoader graphData={graphData} />
            <GraphEvents
              onSelectNode={onSelectNode}
              hoveredNode={hoveredNode}
              setHoveredNode={setHoveredNode}
            />
          </SigmaContainer>
        )}

        {/* ── Floating controls (top-left) ────────────────────── */}
        <div className="absolute left-4 top-4 flex flex-col gap-3 z-10">
          {/* Search */}
          <div className="relative w-56">
            <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search memories..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 pl-8 pr-8 text-xs bg-background/80 backdrop-blur-xl border-border/50"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              >
                <X className="h-3 w-3" />
              </button>
            )}
          </div>

          {/* Category pills */}
          <div className="flex flex-wrap gap-1.5 max-w-xs">
            {CATEGORIES.map((cat) => {
              const count = categoryCounts.get(cat.name) ?? 0;
              if (count === 0) return null;
              const active = enabledCategories.has(cat.name);
              return (
                <button
                  key={cat.name}
                  onClick={() => toggleCategory(cat.name)}
                  className={`flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium transition-all ${
                    active
                      ? "text-white shadow-sm"
                      : "bg-background/60 text-muted-foreground opacity-40 backdrop-blur"
                  }`}
                  style={active ? { backgroundColor: cat.color } : undefined}
                >
                  <span className="capitalize">{cat.name}</span>
                  <span className={active ? "opacity-70" : ""}>{count}</span>
                </button>
              );
            })}
          </div>

          {/* Controls row */}
          <div className="flex items-center gap-1.5">
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 bg-background/80 backdrop-blur-xl border border-border/50"
              onClick={() => void refetch()}
              title="Refresh"
            >
              <RefreshCw className="h-3 w-3" />
            </Button>
          </div>

          {/* Threshold / Neighbors */}
          <div className="flex flex-col gap-1 rounded-lg border border-border/50 bg-background/80 p-2 backdrop-blur-xl w-48">
            <div className="flex items-center justify-between text-[10px] text-muted-foreground">
              <span>Threshold</span>
              <span>{threshold.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min="0.1"
              max="0.9"
              step="0.05"
              value={threshold}
              onChange={(e) => setThreshold(Number(e.target.value))}
              className="w-full h-1"
            />
            <div className="flex items-center justify-between text-[10px] text-muted-foreground">
              <span>Neighbors</span>
              <span>{neighbors}</span>
            </div>
            <input
              type="range"
              min="1"
              max="20"
              step="1"
              value={neighbors}
              onChange={(e) => setNeighbors(Number(e.target.value))}
              className="w-full h-1"
            />
          </div>
        </div>

        {/* ── Stats (top-right of graph area) ────────────────── */}
        {stats && (
          <div className="absolute right-4 top-4 z-10 flex items-center gap-2">
            <StatChip
              icon={BrainCircuit}
              value={stats.memories}
              label="memories"
            />
            <StatChip icon={Link2} value={stats.connections} label="links" />
            <StatChip icon={Zap} value={stats.totalRecalls} label="recalls" />
          </div>
        )}
      </div>

      {/* ── Note panel (right side) ─────────────────────────────── */}
      <AnimatePresence>
        {selectedNode && rawGraph && (
          <NotePanel
            node={selectedNode}
            edges={rawGraph.edges}
            nodes={rawGraph.nodes}
            onClose={() => setSelectedNodeId(null)}
            onNavigate={(id) => setSelectedNodeId(id)}
            onDelete={(id) => deleteMutation.mutate(id)}
            isDeleting={deleteMutation.isPending}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

// ── Stat chip ──────────────────────────────────────────────────────

function StatChip({
  icon: Icon,
  value,
  label,
}: {
  icon: React.ComponentType<{ className?: string }>;
  value: string | number;
  label: string;
}) {
  return (
    <div className="flex items-center gap-1.5 rounded-full border border-border/50 bg-background/80 backdrop-blur-xl px-2.5 py-1">
      <Icon className="h-3 w-3 text-muted-foreground" />
      <span className="text-xs font-semibold">{value}</span>
      <span className="text-[10px] text-muted-foreground">{label}</span>
    </div>
  );
}

// ── Note panel ─────────────────────────────────────────────────────

function NotePanel({
  node,
  edges,
  nodes,
  onClose,
  onNavigate,
  onDelete,
  isDeleting,
}: {
  node: MemoryNode;
  edges: MemoryEdge[];
  nodes: MemoryNode[];
  onClose: () => void;
  onNavigate: (id: string) => void;
  onDelete: (id: string) => void;
  isDeleting: boolean;
}) {
  const connections = useMemo(() => {
    const connected: { node: MemoryNode; similarity: number }[] = [];
    for (const e of edges) {
      if (e.source === node.id) {
        const n = nodes.find((x) => x.id === e.target);
        if (n) connected.push({ node: n, similarity: e.similarity });
      } else if (e.target === node.id) {
        const n = nodes.find((x) => x.id === e.source);
        if (n) connected.push({ node: n, similarity: e.similarity });
      }
    }
    return connected.sort((a, b) => b.similarity - a.similarity);
  }, [node, edges, nodes]);

  const catColor = getCategoryColor(node.category);

  return (
    <motion.div
      initial={{ width: 0, opacity: 0 }}
      animate={{ width: 360, opacity: 1 }}
      exit={{ width: 0, opacity: 0 }}
      transition={{ duration: 0.2, ease: "easeOut" }}
      className="h-full overflow-hidden border-l border-border/50 bg-background"
    >
      <div className="h-full w-[360px] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border/50">
          <div className="flex items-center gap-2.5">
            <span
              className="inline-block h-3 w-3 rounded-full"
              style={{
                backgroundColor: catColor,
                boxShadow: `0 0 10px ${catColor}60`,
              }}
            />
            <span className="text-sm font-semibold capitalize">
              {node.category}
            </span>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          <div className="px-5 py-5 space-y-6">
            {/* Fact */}
            <p className="text-[15px] leading-relaxed text-foreground">
              {node.fact}
            </p>

            {/* Metadata */}
            <div className="flex flex-wrap gap-2">
              <Badge
                variant="secondary"
                className="text-[11px] font-normal gap-1"
              >
                <Zap className="h-3 w-3" />
                {node.access_count}x recalled
              </Badge>
              <Badge
                variant="secondary"
                className="text-[11px] font-normal gap-1"
              >
                <Link2 className="h-3 w-3" />
                {connections.length} linked
              </Badge>
              <Badge variant="outline" className="text-[11px] font-normal">
                {new Date(node.created_at).toLocaleDateString("en-US", {
                  month: "short",
                  day: "numeric",
                })}
              </Badge>
            </div>

            {/* Linked memories */}
            {connections.length > 0 && (
              <div className="space-y-2">
                <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                  Linked memories
                </p>
                <div className="space-y-1">
                  {connections.map((c) => (
                    <button
                      key={c.node.id}
                      onClick={() => onNavigate(c.node.id)}
                      className="w-full flex items-center gap-2.5 rounded-lg px-3 py-2.5 text-left transition-colors hover:bg-accent/50 group"
                    >
                      <span
                        className="inline-block h-2 w-2 shrink-0 rounded-full"
                        style={{
                          backgroundColor: getCategoryColor(c.node.category),
                        }}
                      />
                      <div className="min-w-0 flex-1">
                        <p className="text-[13px] leading-snug line-clamp-2 text-foreground/90 group-hover:text-foreground">
                          {c.node.fact}
                        </p>
                        <p className="text-[11px] text-muted-foreground mt-0.5">
                          {(c.similarity * 100).toFixed(0)}% similar
                        </p>
                      </div>
                      <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground/50 group-hover:text-foreground transition-colors" />
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-border/50">
          <Button
            variant="ghost"
            size="sm"
            className="w-full text-xs text-destructive hover:text-destructive hover:bg-destructive/10"
            onClick={() => onDelete(node.id)}
            disabled={isDeleting}
          >
            <Trash2 className="mr-2 h-3 w-3" />
            {isDeleting ? "Deleting..." : "Delete memory"}
          </Button>
        </div>
      </div>
    </motion.div>
  );
}
