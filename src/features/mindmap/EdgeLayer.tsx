import { useMemo } from "react";
import type { MindEdge, MindNode } from "../../lib/types";
import { computeEdge, type Pt } from "./geometry";

interface Props {
  nodes: MindNode[];
  edges: MindEdge[];
  selectedNodes: Set<string>;
  selectedEdges: Set<string>;
  connectingFrom: string | null;
  connectPos: Pt | null;
  animatedAllowed: boolean;
}

interface RenderedEdge {
  edge: MindEdge;
  a: Pt;
  b: Pt;
  d: string;
}

/** Stable per-node usage distribution so multiple edges spread along edges. */
function computeUsage(edges: MindEdge[]): Map<string, { index: number; count: number }> {
  const counts = new Map<string, number>();
  for (const e of edges) {
    counts.set(e.sourceNodeId, (counts.get(e.sourceNodeId) ?? 0) + 1);
    counts.set(e.targetNodeId, (counts.get(e.targetNodeId) ?? 0) + 1);
  }
  const used = new Map<string, number>();
  const map = new Map<string, { index: number; count: number }>();
  for (const e of edges) {
    for (const id of [e.sourceNodeId, e.targetNodeId]) {
      if (!map.has(id)) map.set(id, { index: used.get(id) ?? 0, count: Math.max(1, counts.get(id) ?? 1) });
    }
    // advance index for both endpoints after assigning this edge a slot
    for (const id of [e.sourceNodeId, e.targetNodeId]) {
      const entry = map.get(id);
      if (entry && entry.index === (used.get(id) ?? -1)) {
        used.set(id, (used.get(id) ?? 0) + 1);
        if ((used.get(id) ?? 0) < entry.count) map.set(id, { ...entry, index: used.get(id)! });
      }
    }
  }
  return map;
}

/**
 * SVG edge layer sharing the world coordinate space with the node layer.
 * Edge geometry follows node position/size/shape on every change.
 */
export function EdgeLayer(props: Props): React.ReactElement {
  const byId = useMemo(() => new Map(props.nodes.map((n) => [n.id, n])), [props.nodes]);
  const usage = useMemo(() => computeUsage(props.edges), [props.edges]);

  const rendered = useMemo<RenderedEdge[]>(() => {
    const out: RenderedEdge[] = [];
    for (const e of props.edges) {
      const from = byId.get(e.sourceNodeId);
      const to = byId.get(e.targetNodeId);
      if (!from || !to) continue; // dangling after concurrent delete — skipped safely
      const g = computeEdge({
        from,
        to,
        pathStyle: e.pathStyle,
        fromUsage: usage.get(e.sourceNodeId),
        toUsage: usage.get(e.targetNodeId),
      });
      out.push({ edge: e, a: g.a, b: g.b, d: g.d });
    }
    return out;
  }, [props.edges, byId, usage]);

  const connectingNode = props.connectingFrom ? byId.get(props.connectingFrom) : null;

  return (
    <svg className="edge-layer" aria-hidden>
      <defs>
        {["7f9bd9", "c68fe0", "7fc8a9", "e8c26b", "e07f7f", "bcd0ff"].map((c) => (
          <marker key={c} id={`arr-${c}`} viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
            <path d="M 0 1 L 9 5 L 0 9 z" fill={`#${c}`} />
          </marker>
        ))}
      </defs>
      {rendered.map(({ edge: e, d, a, b }) => {
        const sel = props.selectedEdges.has(e.id);
        const colorKey = sel ? "bcd0ff" : colorSafe(e.color);
        const dash = e.lineStyle === "dashed" ? "8 6" : e.lineStyle === "dotted" ? "2 5" : undefined;
        return (
          <g key={e.id}>
            <path
              className="edge-hit"
              d={d}
              strokeWidth={Math.max(14, e.width + 12)}
              fill="none"
              onPointerDown={(ev) => {
                ev.stopPropagation();
                window.dispatchEvent(new CustomEvent("variable:mm-edge-select", { detail: { id: e.id, additive: ev.shiftKey } }));
              }}
              onContextMenu={(ev) => {
                ev.preventDefault();
                ev.stopPropagation();
                window.dispatchEvent(new CustomEvent("variable:mm-edge-menu", { detail: { id: e.id, x: ev.clientX, y: ev.clientY } }));
              }}
            />
            <path
              d={d}
              stroke={sel ? "#bcd0ff" : e.color}
              strokeWidth={sel ? e.width + 0.8 : e.width}
              strokeDasharray={dash}
              fill="none"
              markerEnd={e.direction !== "none" ? `url(#arr-${colorKey})` : undefined}
              markerStart={e.direction === "both" ? `url(#arr-${colorKey})` : undefined}
              opacity={sel ? 1 : 0.85}
              className={[
                props.animatedAllowed && e.animated ? "edge-flow" : "",
                e.glow && !sel ? "edge-glow" : "",
                e.glow && sel ? "edge-glow strong" : "",
              ].filter(Boolean).join(" ")}
              style={{ pointerEvents: "none", filter: e.glow ? `drop-shadow(0 0 4px ${e.color})` : undefined }}
            />
            {e.label && (
              <g>
                <rect
                  x={(a.x + b.x) / 2 - Math.max(18, e.label.length * 3.4)}
                  y={(a.y + b.y) / 2 - 11}
                  width={Math.max(36, e.label.length * 6.8)}
                  height={20}
                  rx={5}
                  fill="rgba(10,16,32,0.85)"
                  stroke={sel ? "#bcd0ff" : "transparent"}
                  strokeWidth={1}
                  pointerEvents="none"
                />
                <text
                  x={(a.x + b.x) / 2}
                  y={(a.y + b.y) / 2 + 4}
                  textAnchor="middle"
                  fontSize={11}
                  fill="#cdd9ef"
                  pointerEvents="none"
                  style={{ userSelect: "none" }}
                >
                  {e.label}
                </text>
              </g>
            )}
          </g>
        );
      })}
      {connectingNode && props.connectPos && (
        <line
          x1={connectingNode.x + connectingNode.width / 2}
          y1={connectingNode.y + connectingNode.height / 2}
          x2={props.connectPos.x}
          y2={props.connectPos.y}
          stroke="#9db8e8"
          strokeDasharray="4 4"
          strokeWidth={1.5}
          pointerEvents="none"
        />
      )}
    </svg>
  );
}

function colorSafe(c: string): string {
  return c.replace(/[^a-zA-Z0-9]/g, "");
}
