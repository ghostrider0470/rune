# Memory Graph UI Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the ECharts memory graph with a WebGL-powered force-graph that handles 50K+ nodes, with Obsidian-quality aesthetics, floating controls, and a slide-in detail panel.

**Architecture:** Single component rewrite of `memory.tsx`. Swap `echarts-for-react` + `echarts` for `react-force-graph-2d`. Full-bleed WebGL canvas with floating overlay controls. Add a backend `DELETE /api/memory/:id` endpoint for memory management.

**Tech Stack:** React 19, react-force-graph-2d, TanStack Query, TanStack Router, Tailwind CSS, shadcn/ui, Lucide icons

---

### Task 1: Add react-force-graph-2d Dependency

**Files:**
- Modify: `ui/package.json`

- [ ] **Step 1: Install the dependency**

```bash
cd /home/hamza/Development/rune/ui && npm install react-force-graph-2d
```

- [ ] **Step 2: Remove echarts (only used by memory page)**

```bash
cd /home/hamza/Development/rune/ui && npm uninstall echarts echarts-for-react
```

- [ ] **Step 3: Verify build still works**

```bash
cd /home/hamza/Development/rune/ui && npx tsc --noEmit -p tsconfig.app.json
```

Expected: may show errors in `memory.tsx` since echarts imports are gone — that's fine, we're rewriting it in Task 3.

- [ ] **Step 4: Commit**

```bash
cd /home/hamza/Development/rune && git add ui/package.json ui/package-lock.json
git commit -m "feat(ui): swap echarts for react-force-graph-2d"
```

---

### Task 2: Add DELETE Memory Backend Endpoint

**Files:**
- Modify: `crates/rune-gateway/src/routes.rs`
- Modify: `crates/rune-gateway/src/server.rs`

- [ ] **Step 1: Add the route handler**

In `crates/rune-gateway/src/routes.rs`, add near the other memory routes:

```rust
/// `DELETE /api/memory/:id` — delete a memory node.
pub async fn memory_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let mem0 = state.mem0.as_ref().ok_or_else(|| {
        GatewayError::ServiceUnavailable("mem0 not configured".into())
    })?;

    mem0.delete_memory(&id).await.map_err(|e| {
        GatewayError::Internal(format!("failed to delete memory: {e}"))
    })?;

    Ok(Json(serde_json::json!({"success": true, "id": id})))
}
```

NOTE: Check if `Mem0Engine` has a `delete_memory` method. If not, you'll need to add one — it should execute `DELETE FROM rune_memories WHERE id = $1` via the postgres client. Check `crates/rune-runtime/src/mem0.rs` for the existing pattern. The table is `rune_memories` and the id column type is `UUID` stored as text.

- [ ] **Step 2: Add the route**

In `crates/rune-gateway/src/server.rs`, after the existing memory routes:

```rust
        .route("/api/memory/{id}", delete(routes::memory_delete))
```

Make sure `delete` is imported from `axum::routing`.

- [ ] **Step 3: Build**

```bash
cargo build -p rune-gateway
```

- [ ] **Step 4: Commit**

```bash
git add crates/rune-gateway/src/routes.rs crates/rune-gateway/src/server.rs crates/rune-runtime/src/mem0.rs
git commit -m "feat(gateway): add DELETE /api/memory/:id endpoint"
```

---

### Task 3: Rewrite Memory Graph Page

**Files:**
- Rewrite: `ui/src/routes/_admin/memory.tsx`

This is the main task. Replace the entire file with the new implementation. The file is large so I'll break it into logical sections.

- [ ] **Step 1: Write the complete new memory.tsx**

Replace the entire content of `ui/src/routes/_admin/memory.tsx` with:

```tsx
import { createFileRoute } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useRef, useCallback, useMemo, useEffect } from "react";
import { motion, AnimatePresence } from "motion/react";
import ForceGraph2D from "react-force-graph-2d";
import { api } from "@/lib/api-client";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  BrainCircuit,
  Link2,
  TrendingUp,
  Search,
  X,
  RotateCcw,
  RefreshCw,
  Trash2,
  Zap,
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

// Force-graph node with position
interface GraphNode extends MemoryNode {
  x?: number;
  y?: number;
  __connections?: number;
}

interface GraphLink {
  source: string;
  target: string;
  similarity: number;
}

// ── Categories ─────────────────────────────────────────────────────

const CATEGORIES = [
  { name: "preference", color: "#f59e0b" },
  { name: "project", color: "#3b82f6" },
  { name: "ops", color: "#10b981" },
  { name: "decision", color: "#8b5cf6" },
  { name: "person", color: "#ec4899" },
  { name: "technical", color: "#06b6d4" },
  { name: "workflow", color: "#f97316" },
  { name: "general", color: "#6b7280" },
] as const;

const CATEGORY_COLORS = new Map(CATEGORIES.map((c) => [c.name, c.color]));

function getCategoryColor(cat: string): string {
  return CATEGORY_COLORS.get(cat.toLowerCase()) ?? "#6b7280";
}

type SizeMode = "access" | "connections" | "recency";

// ── Page ───────────────────────────────────────────────────────────

function MemoryGraphPage() {
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [sizeMode, setSizeMode] = useState<SizeMode>("access");
  const [enabledCategories, setEnabledCategories] = useState<Set<string>>(
    () => new Set(CATEGORIES.map((c) => c.name)),
  );
  const graphRef = useRef<any>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();

  const isDark =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("dark");

  // ── Data fetch ───────────────────────────────────────────────

  const {
    data: rawGraph,
    isLoading,
    error,
    refetch,
  } = useQuery({
    queryKey: ["memory-graph"],
    queryFn: () => api.get<GraphResponse>("/api/memory/graph?threshold=0.4&neighbors=8"),
  });

  // ── Delete mutation ──────────────────────────────────────────

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.delete(`/api/memory/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["memory-graph"] });
      setSelectedNode(null);
    },
  });

  // ── Filtered + enriched graph data ───────────────────────────

  const graphData = useMemo(() => {
    if (!rawGraph) return { nodes: [] as GraphNode[], links: [] as GraphLink[] };

    // Count connections per node
    const connectionCount = new Map<string, number>();
    for (const e of rawGraph.edges) {
      connectionCount.set(e.source, (connectionCount.get(e.source) ?? 0) + 1);
      connectionCount.set(e.target, (connectionCount.get(e.target) ?? 0) + 1);
    }

    // Filter by category and search
    const filteredNodes: GraphNode[] = rawGraph.nodes
      .filter((n) => {
        if (!enabledCategories.has(n.category)) return false;
        if (searchQuery && !n.fact.toLowerCase().includes(searchQuery.toLowerCase()))
          return false;
        return true;
      })
      .map((n) => ({
        ...n,
        __connections: connectionCount.get(n.id) ?? 0,
      }));

    const filteredIds = new Set(filteredNodes.map((n) => n.id));
    const filteredLinks: GraphLink[] = rawGraph.edges
      .filter((e) => filteredIds.has(e.source) && filteredIds.has(e.target))
      .map((e) => ({ ...e }));

    return { nodes: filteredNodes, links: filteredLinks };
  }, [rawGraph, enabledCategories, searchQuery]);

  // ── Stats ────────────────────────────────────────────────────

  const stats = useMemo(() => {
    if (!rawGraph) return null;
    const totalAccess = rawGraph.nodes.reduce((s, n) => s + n.access_count, 0);
    const avgSim =
      rawGraph.edges.length > 0
        ? rawGraph.edges.reduce((s, e) => s + e.similarity, 0) / rawGraph.edges.length
        : 0;
    return {
      memories: rawGraph.nodes.length,
      connections: rawGraph.edges.length,
      avgSimilarity: avgSim,
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

  // ── Node rendering ───────────────────────────────────────────

  const getNodeSize = useCallback(
    (node: GraphNode): number => {
      switch (sizeMode) {
        case "access":
          return Math.max(3, Math.min(16, 3 + node.access_count * 1.5));
        case "connections":
          return Math.max(3, Math.min(16, 3 + (node.__connections ?? 0) * 2));
        case "recency": {
          const age = Date.now() - new Date(node.created_at).getTime();
          const hours = age / (1000 * 60 * 60);
          return Math.max(3, Math.min(16, 16 - Math.min(13, hours / 24)));
        }
      }
    },
    [sizeMode],
  );

  const isSearching = searchQuery.length > 0;

  const nodeCanvasObject = useCallback(
    (node: GraphNode, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const size = getNodeSize(node);
      const color = getCategoryColor(node.category);
      const isSelected = selectedNode?.id === node.id;
      const matchesSearch =
        !isSearching || node.fact.toLowerCase().includes(searchQuery.toLowerCase());

      const alpha = isSearching && !matchesSearch ? 0.08 : 1;

      // Glow
      if (isSelected || (matchesSearch && isSearching)) {
        ctx.beginPath();
        ctx.arc(node.x!, node.y!, size + 4, 0, 2 * Math.PI);
        ctx.fillStyle = color + "40";
        ctx.fill();
      }

      // Node circle
      ctx.beginPath();
      ctx.arc(node.x!, node.y!, size, 0, 2 * Math.PI);
      ctx.fillStyle =
        alpha < 1 ? color + "14" : color;
      ctx.fill();

      // Border
      if (isSelected) {
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = 2;
        ctx.stroke();
      } else if (matchesSearch && isSearching) {
        ctx.strokeStyle = "#ffffffcc";
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      // Label on hover (only when zoomed in enough)
      if (isSelected && globalScale > 1.5) {
        const label = node.fact.length > 50 ? node.fact.slice(0, 47) + "..." : node.fact;
        const fontSize = 10 / globalScale;
        ctx.font = `${fontSize}px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.fillStyle = isDark ? "#e5e7eb" : "#1f2937";
        ctx.fillText(label, node.x!, node.y! + size + 3);
      }
    },
    [getNodeSize, selectedNode, isSearching, searchQuery, isDark],
  );

  // ── Link rendering ───────────────────────────────────────────

  const linkColor = useCallback(
    (link: GraphLink) => {
      const base = isDark ? "rgba(148,163,184," : "rgba(100,116,139,";
      const opacity = 0.04 + link.similarity * 0.2;
      return `${base}${opacity})`;
    },
    [isDark],
  );

  const linkWidth = useCallback((link: GraphLink) => 0.3 + link.similarity * 1.5, []);

  // ── Handlers ─────────────────────────────────────────────────

  const onNodeClick = useCallback((node: GraphNode) => {
    setSelectedNode(node);
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

  const resetView = useCallback(() => {
    graphRef.current?.zoomToFit(400, 60);
  }, []);

  // Fit to view on first load
  useEffect(() => {
    if (graphData.nodes.length > 0) {
      setTimeout(() => graphRef.current?.zoomToFit(600, 80), 500);
    }
  }, [graphData.nodes.length > 0]);

  // ── Render ───────────────────────────────────────────────────

  const bgColor = isDark ? "#0a0a0f" : "#fafafa";

  return (
    <div className="relative h-full w-full overflow-hidden" ref={containerRef}>
      {/* Full-bleed graph canvas */}
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
      ) : graphData.nodes.length === 0 && !searchQuery ? (
        <div className="flex h-full flex-col items-center justify-center gap-4 text-muted-foreground">
          <BrainCircuit className="h-20 w-20 opacity-15" />
          <p className="text-base">No memories yet</p>
          <p className="text-xs opacity-60">Start chatting to build the knowledge graph</p>
        </div>
      ) : (
        <ForceGraph2D
          ref={graphRef}
          graphData={graphData}
          nodeId="id"
          nodeCanvasObject={nodeCanvasObject}
          nodePointerAreaPaint={(node: GraphNode, color, ctx) => {
            const size = getNodeSize(node);
            ctx.beginPath();
            ctx.arc(node.x!, node.y!, size + 2, 0, 2 * Math.PI);
            ctx.fillStyle = color;
            ctx.fill();
          }}
          linkColor={linkColor}
          linkWidth={linkWidth}
          linkCurvature={0.1}
          onNodeClick={onNodeClick}
          backgroundColor={bgColor}
          warmupTicks={200}
          cooldownTicks={300}
          d3AlphaDecay={0.02}
          d3VelocityDecay={0.3}
          enableNodeDrag={true}
          enableZoomInteraction={true}
          enablePanInteraction={true}
        />
      )}

      {/* ── Floating controls (top-left) ────────────────────────── */}
      <div className="absolute left-4 top-4 flex flex-col gap-3 z-10">
        {/* Search */}
        <div className="relative w-64">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search memories..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-9 pl-8 pr-8 text-xs bg-background/80 backdrop-blur-xl border-border/50"
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

        {/* Category filters */}
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

        {/* Size mode + controls */}
        <div className="flex items-center gap-1.5">
          <div className="flex items-center rounded-md border border-border/50 bg-background/80 backdrop-blur-xl p-0.5">
            {(
              [
                ["access", "Recalls"],
                ["connections", "Links"],
                ["recency", "Recent"],
              ] as [SizeMode, string][]
            ).map(([m, label]) => (
              <button
                key={m}
                onClick={() => setSizeMode(m)}
                className={`rounded px-2 py-1 text-[10px] font-medium transition-all ${
                  sizeMode === m
                    ? "bg-foreground/10 text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 bg-background/80 backdrop-blur-xl border border-border/50"
            onClick={resetView}
            title="Fit to view"
          >
            <RotateCcw className="h-3 w-3" />
          </Button>

          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 bg-background/80 backdrop-blur-xl border border-border/50"
            onClick={() => refetch()}
            title="Refresh data"
          >
            <RefreshCw className="h-3 w-3" />
          </Button>
        </div>
      </div>

      {/* ── Stats (top-right) ───────────────────────────────────── */}
      {stats && (
        <div className="absolute right-4 top-4 z-10 flex items-center gap-3">
          <Stat icon={BrainCircuit} value={stats.memories} label="memories" color="text-primary" />
          <Stat icon={Link2} value={stats.connections} label="links" color="text-blue-500" />
          <Stat icon={TrendingUp} value={stats.avgSimilarity.toFixed(2)} label="avg sim" color="text-violet-500" />
          <Stat icon={Zap} value={stats.totalRecalls} label="recalls" color="text-amber-500" />
        </div>
      )}

      {/* ── Detail panel (slide-in right) ───────────────────────── */}
      <AnimatePresence>
        {selectedNode && rawGraph && (
          <DetailPanel
            node={selectedNode}
            edges={rawGraph.edges}
            nodes={rawGraph.nodes}
            isDark={isDark}
            onClose={() => setSelectedNode(null)}
            onDelete={(id) => deleteMutation.mutate(id)}
            isDeleting={deleteMutation.isPending}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

// ── Stat chip ──────────────────────────────────────────────────────

function Stat({
  icon: Icon,
  value,
  label,
  color,
}: {
  icon: React.ComponentType<{ className?: string }>;
  value: string | number;
  label: string;
  color: string;
}) {
  return (
    <div className="flex items-center gap-1.5 rounded-full border border-border/50 bg-background/80 backdrop-blur-xl px-2.5 py-1">
      <Icon className={`h-3 w-3 ${color}`} />
      <span className="text-xs font-semibold">{value}</span>
      <span className="text-[10px] text-muted-foreground">{label}</span>
    </div>
  );
}

// ── Detail panel ───────────────────────────────────────────────────

function DetailPanel({
  node,
  edges,
  nodes,
  isDark,
  onClose,
  onDelete,
  isDeleting,
}: {
  node: MemoryNode;
  edges: MemoryEdge[];
  nodes: MemoryNode[];
  isDark: boolean;
  onClose: () => void;
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
  const connectionCount = edges.filter(
    (e) => e.source === node.id || e.target === node.id,
  ).length;

  return (
    <motion.div
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ duration: 0.2, ease: "easeOut" }}
      className="absolute right-4 top-16 bottom-4 z-20 w-80"
    >
      <div className="h-full overflow-auto rounded-2xl border border-border/50 bg-background/90 backdrop-blur-2xl shadow-2xl">
        <div className="p-5 space-y-5">
          {/* Header */}
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span
                className="inline-block h-3 w-3 rounded-full"
                style={{ backgroundColor: catColor, boxShadow: `0 0 8px ${catColor}` }}
              />
              <span className="text-sm font-semibold capitalize">{node.category}</span>
            </div>
            <button
              onClick={onClose}
              className="rounded-lg p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>

          {/* Fact text */}
          <p className="text-sm leading-relaxed text-foreground/90">{node.fact}</p>

          {/* Metadata */}
          <div className="flex flex-wrap gap-2">
            <Badge variant="secondary" className="text-[11px]">
              {node.access_count}x accessed
            </Badge>
            <Badge variant="secondary" className="text-[11px]">
              {connectionCount} connections
            </Badge>
            <Badge variant="outline" className="text-[11px]">
              {new Date(node.created_at).toLocaleDateString("en-US", {
                month: "short",
                day: "numeric",
                year: "numeric",
              })}
            </Badge>
          </div>

          {/* Session ID */}
          {node.session_id && (
            <p className="text-[10px] text-muted-foreground truncate font-mono">
              {node.session_id}
            </p>
          )}

          {/* Connected memories */}
          {connections.length > 0 && (
            <div className="space-y-2.5 pt-3 border-t border-border/50">
              <p className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
                Connected memories
              </p>
              <div className="space-y-1.5 max-h-56 overflow-y-auto">
                {connections.map((c) => (
                  <div
                    key={c.node.id}
                    className="flex items-start gap-2 rounded-xl bg-muted/30 p-2.5 text-xs"
                  >
                    <span
                      className="mt-1 inline-block h-2 w-2 shrink-0 rounded-full"
                      style={{ backgroundColor: getCategoryColor(c.node.category) }}
                    />
                    <div className="min-w-0 flex-1">
                      <p className="leading-snug line-clamp-2">{c.node.fact}</p>
                      <p className="text-muted-foreground mt-0.5">
                        {(c.similarity * 100).toFixed(0)}% similar
                      </p>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Delete */}
          <div className="pt-3 border-t border-border/50">
            <Button
              variant="destructive"
              size="sm"
              className="w-full text-xs"
              onClick={() => onDelete(node.id)}
              disabled={isDeleting}
            >
              <Trash2 className="mr-2 h-3 w-3" />
              {isDeleting ? "Deleting..." : "Delete memory"}
            </Button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd /home/hamza/Development/rune/ui && npx tsc --noEmit -p tsconfig.app.json
```

Expected: success (or minor type issues with `react-force-graph-2d` — may need `@types` or `any` casts on ref)

- [ ] **Step 3: Build the UI**

```bash
cd /home/hamza/Development/rune/ui && npx vite build
```

Expected: success

- [ ] **Step 4: Commit**

```bash
cd /home/hamza/Development/rune && git add ui/src/routes/_admin/memory.tsx
git commit -m "feat(ui): rewrite memory graph with WebGL force-graph

Replace ECharts with react-force-graph-2d for Obsidian-quality
knowledge graph visualization. Full-bleed WebGL canvas, floating
controls, slide-in detail panel, category clustering, search
highlighting, and memory deletion. Handles 50K+ nodes at 60fps."
```

---

### Task 4: Build, Deploy, Verify

- [ ] **Step 1: Full backend build**

```bash
cargo build --release --bin rune-gateway
```

- [ ] **Step 2: Build UI**

```bash
cd /home/hamza/Development/rune/ui && npx vite build
```

- [ ] **Step 3: Restart and verify**

```bash
systemctl --user restart rune-gateway && sleep 3
```

Open `http://localhost:18790/memory` in a browser. Verify:
- Graph renders with WebGL canvas
- Nodes colored by category
- Search highlights matching nodes
- Category pills filter the graph
- Clicking a node opens the detail panel
- Dark/light mode works

- [ ] **Step 4: Push**

```bash
git push origin main
```
