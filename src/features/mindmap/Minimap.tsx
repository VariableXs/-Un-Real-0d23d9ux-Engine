import { useEffect, useRef, useState } from "react";
import type { MindNode } from "../../lib/types";

interface Props {
  nodes: MindNode[];
  selectionIds: Set<string>;
  viewport: { x: number; y: number; zoom: number };
  canvasSize: { w: number; h: number };
  onMoveViewport: (wx: number, wy: number) => void;
}

const W = 190;
const H = 130;
const PAD = 10;

/**
 * Bottom-right minimap: renders node rectangles + the current viewport box.
 * Dragging inside jumps the viewport; hovering enlarges it for a closer look.
 */
export function Minimap(props: Props): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState(false);
  const draggingRef = useRef(false);

  // world bounds = union of nodes and current viewport center region
  const bounds = (): { x0: number; y0: number; x1: number; y1: number } => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const n of props.nodes) {
      x0 = Math.min(x0, n.x); y0 = Math.min(y0, n.y);
      x1 = Math.max(x1, n.x + n.width); y1 = Math.max(y1, n.y + n.height);
    }
    const vw = props.canvasSize.w / props.viewport.zoom;
    const vh = props.canvasSize.h / props.viewport.zoom;
    const cx = -props.viewport.x / props.viewport.zoom;
    const cy = -props.viewport.y / props.viewport.zoom;
    x0 = Math.min(x0, cx - vw / 2); y0 = Math.min(y0, cy - vh / 2);
    x1 = Math.max(x1, cx + vw / 2); y1 = Math.max(y1, cy + vh / 2);
    if (!Number.isFinite(x0)) return { x0: -500, y0: -400, x1: 500, y1: 400 };
    return { x0, y0, x1, y1 };
  };

  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    cv.width = W * dpr;
    cv.height = H * dpr;
    const g = cv.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);

    const b = bounds();
    const bw = Math.max(1, b.x1 - b.x0);
    const bh = Math.max(1, b.y1 - b.y0);
    const s = Math.min((W - PAD * 2) / bw, (H - PAD * 2) / bh);
    const ox = PAD + ((W - PAD * 2) - bw * s) / 2 - b.x0 * s;
    const oy = PAD + ((H - PAD * 2) - bh * s) / 2 - b.y0 * s;

    // node rects
    for (const n of props.nodes) {
      g.fillStyle = props.selectionIds.has(n.id) ? "rgba(140,170,235,.95)" : "rgba(125,150,205,.55)";
      g.fillRect(n.x * s + ox, n.y * s + oy, Math.max(2, n.width * s), Math.max(2, n.height * s));
    }
    // viewport rect
    const vw = props.canvasSize.w / props.viewport.zoom;
    const vh = props.canvasSize.h / props.viewport.zoom;
    const cx = -props.viewport.x / props.viewport.zoom;
    const cy = -props.viewport.y / props.viewport.zoom;
    g.strokeStyle = "rgba(190,210,255,.95)";
    g.lineWidth = 1.4;
    g.strokeRect((cx - vw / 2) * s + ox, (cy - vh / 2) * s + oy, vw * s, vh * s);

    function toWorld(clientX: number, clientY: number): { x: number; y: number } | null {
      const r = wrapRef.current?.getBoundingClientRect();
      if (!r) return null;
      // The wrap may be CSS-scaled (hover zoom); normalize to logical pixels.
      const scaleX = r.width / W;
      const scaleY = r.height / H;
      const px = (clientX - r.left) / scaleX;
      const py = (clientY - r.top) / scaleY;
      const wx = (px - ox) / s;
      const wy = (py - oy) / s;
      void b;
      return { x: wx, y: wy };
    }
    minimapToWorldRef.current = toWorld;
  });

  const minimapToWorldRef = useRef<((x: number, y: number) => { x: number; y: number } | null) | null>(null);

  return (
    <div
      ref={wrapRef}
      className={`minimap ${hover ? "hover" : ""}`}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onPointerDown={(e) => {
        draggingRef.current = true;
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        const w = minimapToWorldRef.current?.(e.clientX, e.clientY);
        if (w) props.onMoveViewport(w.x, w.y);
      }}
      onPointerMove={(e) => {
        if (!draggingRef.current) return;
        const w = minimapToWorldRef.current?.(e.clientX, e.clientY);
        if (w) props.onMoveViewport(w.x, w.y);
      }}
      onPointerUp={() => {
        draggingRef.current = false;
      }}
    >
      <canvas ref={canvasRef} style={{ width: W, height: H }} />
    </div>
  );
}
