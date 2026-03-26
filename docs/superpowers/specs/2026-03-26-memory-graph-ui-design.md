# Memory Graph UI Redesign — Design Spec

**Date:** 2026-03-26
**Goal:** Replace the current ECharts-based memory graph with an Obsidian-quality WebGL knowledge graph using force-graph, supporting 5K-50K nodes with smooth interaction.

## Current Problems

- ECharts force graph: jittery, nodes overlap, doesn't scale past ~500 nodes
- 10-second refetch interval hammering Azure PG on every poll
- Cramped layout, too many controls visible at once
- Slow initial load, laggy interactions
- No way to delete memories or manage the graph

## Design

### Renderer

Replace ECharts with `react-force-graph-2d` (WebGL canvas via three.js). Handles 50K+ nodes natively. Single component replacement — the entire `ui/src/routes/_admin/memory.tsx` is rewritten.

### Layout

- **Full-bleed canvas** — the graph fills the entire page, no card wrapper
- **d3-force** simulation with category-based clustering
- **Warmup:** 300 iterations before first render for instant stable layout (no jitter)
- **Node labels:** Only shown on hover, not rendered for all nodes (performance)

### Nodes

- **Color:** By category (same palette as current: preference=amber, project=blue, ops=green, decision=violet, person=pink, technical=cyan, workflow=orange, general=gray)
- **Size:** Based on selected mode — access count (default), connection count, or recency
- **Glow:** Subtle bloom effect on hover/select, matching category color
- **Highlight on search:** Matching nodes full opacity + white border, non-matching fade to 10% opacity

### Edges

- **Width:** Proportional to similarity score (0.5 + similarity * 2)
- **Opacity:** Low by default (0.05-0.15), highlighted on node hover/select
- **Curvature:** Slight curve to prevent overlap on bidirectional edges

### Floating Controls (top-left overlay)

Translucent backdrop-blur panel, minimal footprint:

- **Search input** — live filter, highlights matching nodes
- **Category filter pills** — toggle categories on/off, colored pills with counts
- **Size mode toggle** — Recalls / Links / Recent (3-button segmented control)
- **Refresh button** — manual data reload
- **Reset view button** — reset zoom/pan to fit all nodes

### Detail Panel (slide-in from right)

Triggered by clicking a node. 320px wide, dark glass panel:

- Category badge with color dot
- Full fact text (no truncation)
- Metadata: access count, connection count, creation date, session ID
- Connected memories list (sorted by similarity, clickable to navigate)
- **Delete button** — calls `DELETE /api/memory/:id` to remove the memory

### Stats (top-right, small overlay)

Four compact stat chips:
- Total memories
- Total connections
- Average similarity
- Total recalls

### Data Loading

- **Single fetch on mount** — no polling, no 10s refetch
- **Manual refresh** via button
- **Loading state:** Skeleton pulse animation while fetching
- **Error state:** Centered error message with retry button
- Backend: existing `GET /api/memory/graph?threshold=0.45&neighbors=5` endpoint

### Performance

- WebGL canvas via force-graph handles 50K nodes natively
- `cooldownTicks={300}` + `warmupTicks={300}` for instant stable layout
- Node text rendering disabled globally (only on hover via `nodeLabel` callback)
- Canvas rendering (not SVG) — O(1) for pan/zoom regardless of node count

### Theming

Reads `document.documentElement.classList.contains("dark")`:
- **Dark:** `#0a0a0f` canvas background, nodes with glow/bloom, edges as translucent white
- **Light:** `#fafafa` canvas background, solid nodes, edges as translucent gray

## Files Changed

| File | Change |
|------|--------|
| `ui/src/routes/_admin/memory.tsx` | Complete rewrite |
| `ui/package.json` | Add `react-force-graph-2d` dependency |

## Dependencies

- `react-force-graph-2d` — WebGL force graph renderer (~150KB with three.js)
- Remove: `echarts-for-react`, `echarts` (only used by memory page)

## Out of Scope

- 3D graph view (can add later as a toggle)
- Memory creation/editing UI (memories are auto-captured by Mem0)
- Graph persistence (layout positions are ephemeral)
- Backend pagination (defer until >50K nodes becomes an issue)

## Success Criteria

1. Graph renders 5K+ nodes at 60fps with smooth pan/zoom
2. No polling — data loads once on mount
3. Search highlights nodes in <100ms
4. Category filtering is instant
5. Node click opens detail panel with full memory info
6. Delete button removes a memory
7. Works in both dark and light mode
8. Page feels spacious — no cramped controls
