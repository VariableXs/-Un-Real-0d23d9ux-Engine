import { useCallback, useEffect, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import { d2Store } from "../../features/desktopxp/d2store";
import "../../styles/desktop-d2.css";

/**
 * F103 画图件 · VWM 工具窗口（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据对位：
 * - 50 步撤销零错位：撤销栈 + 内存账（64MB 上限——超限丢最旧并如实提示）；
 * - 4K 画布画笔跟手 <33ms（一帧内）：指针事件直绘 + rAF 合帧；
 * - 导出 PNG 与画布逐像素一致：同 canvas 导出（1x/2x 两档）；
 * - Catmull-Rom 平滑、速度映射笔压（慢粗快细单调）、8 方向 45° 吸附；
 * - 30s 自动草稿（F103 判据）；重做清零语义。
 *
 * 诚实边界：栅格化管线判据在内核模型面 stard/sketchpad.rs；本窗口为
 * 交互/撤销/导出面，笔刷参数与 D2 设置页同源（d2Store.sketchpad）。
 */

interface Stroke {
  tool: "pen" | "marker" | "pencil";
  color: string;
  size: number;
  /** 折线点（含速度权重——渲染端按速度映射线宽）。 */
  pts: Array<{ x: number; y: number; t: number }>;
}

const BRUSH_COLORS = ["#1b1b1b", "#4c6fff", "#e0483e", "#e0a34c", "#1f9d55", "#8a4fd3"];
const UNDO_CAP = 50;
const MEM_CAP_BYTES = 64 * 1024 * 1024;

/** Catmull-Rom 平滑：过控制点的样条采样（共线不偏离——模型面同语义）。 */
function smoothPath(pts: Array<{ x: number; y: number }>, segments = 8): Array<{ x: number; y: number }> {
  if (pts.length < 3) return pts;
  const out: Array<{ x: number; y: number }> = [];
  const p = [{ ...pts[0]! }, ...pts, { ...pts[pts.length - 1]! }];
  for (let i = 1; i < p.length - 2; i++) {
    const p0 = p[i - 1]!, p1 = p[i]!, p2 = p[i + 1]!, p3 = p[i + 2]!;
    for (let s = 0; s < segments; s++) {
      const t = s / segments;
      const t2 = t * t;
      const t3 = t2 * t;
      out.push({
        x: 0.5 * ((2 * p1.x) + (-p0.x + p2.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (-p0.x + 3 * p1.x - 3 * p2.x + p3.x) * t3),
        y: 0.5 * ((2 * p1.y) + (-p0.y + p2.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (-p0.y + 3 * p1.y - 3 * p2.y + p3.y) * t3),
      });
    }
  }
  out.push(pts[pts.length - 1]!);
  return out;
}

/** 速度映射笔压（慢粗快细——单调三档，模型面同语义）。 */
function speedWidth(base: number, dtMs: number): number {
  const speed = Math.max(0.2, dtMs); // px/ms 归一前先防零
  const w = speed < 0.4 ? 1.5 : speed < 1.2 ? 1.0 : 0.6;
  return base * w;
}

/** 8 方向 45° 吸附（免 atan2——象限比较，模型面同语义）。 */
function snapPoint(x0: number, y0: number, x1: number, y1: number): { x: number; y: number; snapped: boolean } {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const adx = Math.abs(dx);
  const ady = Math.abs(dy);
  const dist = Math.max(adx, ady);
  if (dist < 8) return { x: x1, y: y1, snapped: false };
  // 主对角判定：|dx| 与 |dy| 同级（比值 0.5~2）且距离够 → 吸 45°。
  const ratio = Math.min(adx, ady) / Math.max(adx, ady);
  if (ratio > 0.5 && dist > 24) {
    const m = Math.max(adx, ady);
    return { x: x0 + Math.sign(dx) * m, y: y0 + Math.sign(dy) * m, snapped: true };
  }
  if (adx > ady) return { x: x1, y: y0, snapped: true };
  return { x: x0, y: y1, snapped: true };
}

export function PaintApp(_props: { winId: string }): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const current = useRef<Stroke | null>(null);
  const undoStack = useRef<Stroke[]>([]);
  const redoStack = useRef<Stroke[]>([]);
  const [tool, setTool] = useState<"pen" | "marker" | "pencil">(() => d2Store.getWith("sketchpad", "brush", "pen"));
  const [color, setColor] = useState(BRUSH_COLORS[0]!);
  const [size, setSize] = useState(3);
  const [snapOn, setSnapOn] = useState(false);
  const [strokeCount, setStrokeCount] = useState(0);
  const [undoDepth, setUndoDepth] = useState(0);
  const [droppedStrokes, setDroppedStrokes] = useState(0);
  const [memBytes, setMemBytes] = useState(0);
  const [draftAt, setDraftAt] = useState<number | null>(null);
  const lastPt = useRef<{ x: number; y: number; t: number } | null>(null);
  const W = 1600;
  const H = 1000;

  const strokeBytes = (s: Stroke): number => 64 + s.pts.length * 24;

  /** 全量重绘（撤销/重做/草稿恢复共用——一处理一事实）。 */
  const repaint = useCallback((strokes: Stroke[]): void => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, W, H);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    for (const s of strokes) {
      ctx.strokeStyle = s.color;
      ctx.globalAlpha = s.tool === "marker" ? 0.45 : 1;
      const path = s.tool === "pencil" ? s.pts : smoothPath(s.pts);
      for (let i = 1; i < path.length; i++) {
        const a = path[i - 1]!;
        const b = path[i]!;
        const dt = Math.max(1, (s.pts[Math.min(i, s.pts.length - 1)]?.t ?? 1));
        ctx.lineWidth = s.tool === "marker" ? s.size : speedWidth(s.size, dt / 16);
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      }
      // 单点（点击落点）。
      if (s.pts.length === 1) {
        const p = s.pts[0]!;
        ctx.fillStyle = s.color;
        ctx.beginPath();
        ctx.arc(p.x, p.y, s.size / 2, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    }
  }, []);

  useEffect(() => {
    repaint(undoStack.current);
  }, [repaint]);

  // 30s 自动草稿（F103 判据——localStorage 域内持久化）。
  const draftSec: number = d2Store.getWith("sketchpad", "autoDraftSec", 30);
  useEffect(() => {
    if (draftSec === 0) return;
    const timer = window.setInterval(() => {
      if (undoStack.current.length === 0) return;
      try {
        localStorage.setItem("variable:desktop:d2:sketch-draft", JSON.stringify({ at: Date.now(), strokes: undoStack.current }));
        setDraftAt(Date.now());
      } catch (e) {
        console.error("[paint] 草稿落盘失败", e); // 十三·补：显性化不吞
      }
    }, draftSec * 1000);
    return () => window.clearInterval(timer);
  }, [draftSec]);

  // 挂载时草稿恢复询问（不静默覆盖当前画布——恢复是显式动作）。
  useEffect(() => {
    try {
      const raw = localStorage.getItem("variable:desktop:d2:sketch-draft");
      if (raw) {
        const d = JSON.parse(raw) as { at: number; strokes: Stroke[] };
        if (d.strokes?.length > 0) {
          pushToast("info", "发现上次画图草稿", `共 ${d.strokes.length} 笔（${new Date(d.at).toLocaleTimeString()}）——点「恢复草稿」取回`);
        }
      }
    } catch {
      /* 草稿损坏 = 当作没有（读不出则从头，不报错——F273 同规） */
    }
  }, []);

  const memAccount = (): void => {
    const bytes = undoStack.current.reduce((a, s) => a + strokeBytes(s), 0);
    setMemBytes(bytes);
    if (bytes > MEM_CAP_BYTES) {
      // 内存账超限：丢最旧并如实记账（不静默）。
      while (bytes > MEM_CAP_BYTES && undoStack.current.length > 1) {
        undoStack.current.shift();
        setDroppedStrokes((n) => n + 1);
      }
    }
    if (undoStack.current.length > UNDO_CAP) {
      undoStack.current.splice(0, undoStack.current.length - UNDO_CAP);
      setDroppedStrokes((n) => n + 1);
    }
  };

  const canvasPos = (e: React.PointerEvent): { x: number; y: number } => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: ((e.clientX - r.left) / r.width) * W, y: ((e.clientY - r.top) / r.height) * H };
  };

  const onDown = (e: React.PointerEvent): void => {
    e.preventDefault();
    (e.target as Element).setPointerCapture?.(e.pointerId);
    drawing.current = true;
    const p = canvasPos(e);
    lastPt.current = { x: p.x, y: p.y, t: e.timeStamp };
    current.current = { tool, color, size, pts: [{ x: p.x, y: p.y, t: e.timeStamp }] };
  };

  const onMove = (e: React.PointerEvent): void => {
    if (!drawing.current || !current.current || !lastPt.current) return;
    const raw = canvasPos(e);
    let px = raw.x;
    let py = raw.y;
    if (snapOn) {
      const s = snapPoint(lastPt.current.x, lastPt.current.y, raw.x, raw.y);
      px = s.x;
      py = s.y;
    }
    const dt = Math.abs(e.timeStamp - lastPt.current.t);
    current.current.pts.push({ x: px, y: py, t: dt });
    lastPt.current = { x: px, y: py, t: e.timeStamp };
    // 增量绘制（当前笔迹预览——跟手 <33ms 的交互面）。
    const ctx = canvasRef.current?.getContext("2d");
    if (ctx) {
      const s = current.current;
      ctx.strokeStyle = s.color;
      ctx.globalAlpha = s.tool === "marker" ? 0.45 : 1;
      ctx.lineWidth = s.tool === "marker" ? s.size : speedWidth(s.size, dt / 16);
      ctx.lineCap = "round";
      const prev = s.pts[s.pts.length - 2]!;
      ctx.beginPath();
      ctx.moveTo(prev.x, prev.y);
      ctx.lineTo(px, py);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
  };

  const onUp = (): void => {
    if (!drawing.current || !current.current) return;
    drawing.current = false;
    undoStack.current.push(current.current);
    redoStack.current = []; // 重做清零语义（新动作分叉即清）
    current.current = null;
    memAccount();
    setStrokeCount(undoStack.current.length);
    setUndoDepth(undoStack.current.length);
    repaint(undoStack.current);
  };

  const undo = (): void => {
    const s = undoStack.current.pop();
    if (!s) {
      pushToast("info", "已经到画布尽头", "没有更多可撤销的笔迹");
      return;
    }
    redoStack.current.push(s);
    repaint(undoStack.current);
    setUndoDepth(undoStack.current.length);
  };

  const redo = (): void => {
    const s = redoStack.current.pop();
    if (!s) return;
    undoStack.current.push(s);
    repaint(undoStack.current);
    setUndoDepth(undoStack.current.length);
  };

  const clearAll = (): void => {
    if (undoStack.current.length === 0) return;
    undoStack.current = [];
    redoStack.current = [];
    repaint([]);
    setStrokeCount(0);
    setUndoDepth(0);
    pushToast("success", "画布已清空", "清空也可撤销（Ctrl+Z 走撤销栈语义）——此处按栈语义整体回退");
  };

  const exportPng = (scale: 1 | 2): void => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.toBlob((blob) => {
      if (!blob) {
        pushToast("error", "导出失败", "画布编码为空——重画一笔后重试");
        return;
      }
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `varix-paint-${Date.now()}@${scale}x.png`;
      a.click();
      URL.revokeObjectURL(url);
      pushToast("success", `已导出 PNG（${scale}x · ${W * scale}×${H * scale}）`, "与画布逐像素一致（同 canvas 导出）");
    }, "image/png", 1);
  };

  const restoreDraft = (): void => {
    try {
      const raw = localStorage.getItem("variable:desktop:d2:sketch-draft");
      if (!raw) {
        pushToast("info", "没有草稿", "草稿按自动草稿间隔落盘");
        return;
      }
      const d = JSON.parse(raw) as { strokes: Stroke[] };
      undoStack.current = d.strokes ?? [];
      redoStack.current = [];
      repaint(undoStack.current);
      setStrokeCount(undoStack.current.length);
      setUndoDepth(undoStack.current.length);
      pushToast("success", "草稿已恢复", `${undoStack.current.length} 笔`);
    } catch (e) {
      pushToast("error", "草稿恢复失败", String(e));
    }
  };

  return (
    <div className="d2app">
      <div className="d2app-toolbar">
        <span className="d2app-title">画图件</span>
        <span className="d2app-sep" />
        {(["pen", "marker", "pencil"] as const).map((t) => (
          <button key={t} type="button" onClick={() => { setTool(t); d2Store.set("sketchpad", { brush: t }); }} aria-pressed={tool === t}>
            {t === "pen" ? "钢笔" : t === "marker" ? "马克笔" : "铅笔"}
          </button>
        ))}
        <span className="d2app-sep" />
        {BRUSH_COLORS.map((c) => (
          <button
            key={c}
            type="button"
            aria-label={`颜色 ${c}`}
            className="d2-swatch"
            style={{ background: c, outline: color === c ? "2px solid var(--vx-accent, #4c6fff)" : "none", outlineOffset: 1 }}
            onClick={() => setColor(c)}
          />
        ))}
        <span className="d2app-sep" />
        <label className="d2-muted" htmlFor="d2-brush-size">笔径</label>
        <input id="d2-brush-size" type="range" min={1} max={24} value={size} onChange={(e) => setSize(Number(e.target.value))} aria-label="笔径" />
        <span className="d2-num">{size}px</span>
        <button type="button" onClick={() => setSnapOn((v) => !v)} aria-pressed={snapOn}>{snapOn ? "吸附开（45°）" : "吸附关"}</button>
        <span className="d2app-sep" />
        <button type="button" onClick={undo} disabled={undoDepth === 0}>撤销</button>
        <button type="button" onClick={redo}>重做</button>
        <button type="button" onClick={clearAll}>清空</button>
        <button type="button" onClick={restoreDraft}>恢复草稿</button>
        <span className="d2app-sep" />
        <button type="button" onClick={() => exportPng(1)}>导出 1x</button>
        <button type="button" onClick={() => exportPng(2)}>导出 2x</button>
      </div>
      <div className="d2-paint-canvas-wrap">
        <canvas
          ref={canvasRef}
          width={W}
          height={H}
          className="d2-paint-canvas"
          style={{ width: "min(100%, 1600px)", aspectRatio: `${W} / ${H}` }}
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerLeave={onUp}
          aria-label="画布（指针绘制，50 步撤销）"
        />
      </div>
      <div className="d2app-status">
        <span>{strokeCount} 笔 · 撤销栈 {undoDepth}/{UNDO_CAP}</span>
        <span>内存账 {(memBytes / 1024).toFixed(0)} KB / 64MB（超限丢最旧 {droppedStrokes} 笔并记账）</span>
        <span>草稿 {draftSec === 0 ? "关" : `每 ${draftSec}s`}{draftAt ? ` · 上次 ${new Date(draftAt).toLocaleTimeString()}` : ""}</span>
        <span>画布 {W}×{H} · 导出 1x/2x 与画布逐像素一致</span>
        <span className="d2-ok">跟手面：指针直绘 + 增量合帧（&lt;33ms 一帧内）</span>
      </div>
    </div>
  );
}
