/**
 * 桌面实绘装配件（AI-U3 v5 · 批次五装配层 · DeskPaint）。
 *
 * 职责：消费 despaint 引擎的纯函数，装配出「桌面图标层可实绘」的活体件：
 * - <DeskIconSurface>：图标面（网格落位 + 橡皮筋 + 键盘导航 + 拖拽幽灵 +
 *   F539 锁定拒绝抖动）——桌面领地可直接嵌入或按同语义重绘（章十四）；
 * - <DeskPaintSection>：U3Lab 演示区（上述件 + 瞥桌面/锁定开关的真实
 *   交互样张——不是截图，是可操作的活体）。
 *
 * 交互纪律（宪章三章逐条自证）：
 * - 浮层出路：拖拽幽灵 Esc 即弃并复原选中；无其他浮层；
 * - 100ms 反馈：点击/框选/键导航全部同步 setState（React 批处理后一帧内）；
 * - 焦点环：面容器 tabIndex=0 可聚焦，键导航前置焦点可见（样式层
 *   u3-desk-surface:focus-visible 出环）。
 */

import React, { useCallback, useMemo, useRef, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  type DragGhost,
  applySelection, gridNeighbor, gridRowEdge, typeAheadMatch, typeAheadPush,
  marqueeVerdict, dragStartVerdict, dragGhost, dragEscRestore, snapDropBatch,
  peekVerdict, paintDragAllowed, clickSequence, type GridNavIndex, type Point,
} from "./engines";
import { u3Store } from "./u3store";

/* ------------------------------ 演示数据 ------------------------------ */

interface DeskIcon { id: string; name: string; x: number; y: number }

const DEMO_ICONS: Array<DeskIcon> = [
  { id: "i1", name: "此机", x: 0, y: 0 },
  { id: "i2", name: "回收站", x: 88, y: 0 },
  { id: "i3", name: "项目一", x: 176, y: 0 },
  { id: "i4", name: "文档", x: 0, y: 96 },
  { id: "i5", name: "下载", x: 88, y: 96 },
  { id: "i6", name: "截图合集", x: 176, y: 96 },
];

const CELL = { colPx: 88, rowPx: 96 };

const NAV_INDEX: GridNavIndex = {
  cols: DEMO_ICONS.map((i) => ({ x: i.x, y: i.y })),
  colW: CELL.colPx,
  rowH: CELL.rowPx,
};

/* ------------------------------ 桌面图标面 ------------------------------ */

export interface DeskIconSurfaceProps {
  icons?: ReadonlyArray<DeskIcon>;
}

/** 图标面：空白/图标点击、橡皮筋、键盘、拖拽全输入路径（章十五走查面）。
 *  布局锁态由 paintDragAllowed() 直读 u3Store（一处一事实——演示区开关与
 *  真实面板改同一份）。 */
export function DeskIconSurface(props: DeskIconSurfaceProps): React.ReactElement {
  const icons = useMemo(() => props.icons ?? DEMO_ICONS, [props.icons]);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(["i1"]));
  const [anchor, setAnchor] = useState<string | null>("i1");
  const [marquee, setMarquee] = useState<{ from: Point; to: Point } | null>(null);
  const [drag, setDrag] = useState<null | { from: Point; ghost: DragGhost }>(null);
  const [taBuffer, setTaBuffer] = useState("");
  const [lastClick, setLastClick] = useState<{ atMs: number; x: number; y: number } | null>(null);
  const [status, setStatus] = useState("点选/框选/方向键/输入首字母——全输入路径可用");
  const downRef = useRef<Point | null>(null);
  const lastTaAt = useRef(0);

  const order = useMemo(() => icons.map((i) => i.id), [icons]);

  /** 图标内点位命中（拖拽/点击的目标解析）。 */
  const hitTest = useCallback((p: Point): string | null => {
    const hit = icons.find((i) => p.x >= i.x && p.x <= i.x + CELL.colPx - 8 && p.y >= i.y && p.y <= i.y + CELL.rowPx - 8);
    return hit ? hit.id : null;
  }, [icons]);

  const toLocal = useCallback((e: React.PointerEvent): Point => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }, []);

  const onDown = useCallback((e: React.PointerEvent) => {
    const p = toLocal(e);
    downRef.current = p;
    if (!hitTest(p)) setMarquee({ from: p, to: p }); // 空白处按下 → 预备橡皮筋（过阈值才成框）
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }, [hitTest, toLocal]);

  const onMove = useCallback((e: React.PointerEvent) => {
    const down = downRef.current;
    if (!down) return;
    const p = toLocal(e);
    const v = marqueeVerdict(down, p);
    if (marquee) { setMarquee({ ...marquee, to: p }); return; }
    if (v.mode === "marquee") { setMarquee({ from: down, to: p }); return; }
    // 拖拽启动：点中图标 + 过拖拽阈值 + 过布局锁
    const target = hitTest(down);
    if (target && dragStartVerdict(down, p) === "drag") {
      const guard = paintDragAllowed();
      if (!guard.allowed) { setStatus(guard.statusbar); downRef.current = null; return; }
      const ids = selected.has(target) ? [...selected] : [target];
      const grab = { x: p.x - (icons.find((i) => i.id === target)!.x), y: p.y - (icons.find((i) => i.id === target)!.y) };
      setDrag({ from: down, ghost: dragGhost({ primaryId: target, grabOffset: grab }, ids, p) });
    }
  }, [hitTest, icons, marquee, selected, toLocal]);

  const onUp = useCallback((e: React.PointerEvent) => {
    const down = downRef.current;
    downRef.current = null;
    if (!down) return;
    const p = toLocal(e);
    // 拖拽收尾：吸附落位（Esc 路径在 onKeyDown）
    if (drag) {
      const delta = { x: p.x - drag.from.x, y: p.y - drag.from.y };
      const moved = snapDropBatch(icons.map((i) => ({ id: i.id, x: i.x, y: i.y })), delta);
      void moved; // 演示面不真改坐标——桌面领地消费落位结果（诚实边界）
      setStatus(`拖拽落位：${drag.ghost.ids.length} 枚吸附网格（结果由桌面消费）`);
      setDrag(null);
      setMarquee(null);
      return;
    }
    // 框选收尾：中心点命中（图标中心进框才算选中——与资源管理器同语义）
    if (marquee) {
      const v = marqueeVerdict(down, p);
      const inside = v.mode === "marquee"
        ? icons.filter((i) => {
            const cx = i.x + CELL.colPx / 2;
            const cy = i.y + CELL.rowPx / 2;
            return cx >= v.rect.x && cx <= v.rect.x + v.rect.w && cy >= v.rect.y && cy <= v.rect.y + v.rect.h;
          }).map((i) => i.id)
        : [];
      const res = applySelection(inside, selected, anchor, { kind: v.mode === "marquee" ? "marquee" : "blank-click", ctrl: e.ctrlKey }, order);
      setSelected(res.selected); setAnchor(res.anchor);
      setStatus(v.mode === "marquee" ? `框选命中 ${inside.length} 枚` : "空白单击——选中已清");
      setMarquee(null);
      return;
    }
    // 点击收尾：双击防抖（despaint 同源窗口）
    const target = hitTest(down);
    const seq = clickSequence(lastClick, { atMs: e.timeStamp, x: p.x, y: p.y });
    setLastClick({ atMs: e.timeStamp, x: p.x, y: p.y });
    if (target) {
      const res = applySelection([], selected, anchor, { kind: "item-click", target, ctrl: e.ctrlKey, shift: e.shiftKey }, order);
      setSelected(res.selected); setAnchor(res.anchor);
      if (seq.verdict === "double") { setStatus(`双击打开「${icons.find((i) => i.id === target)!.name}」`); }
      else setStatus(`选中「${icons.find((i) => i.id === target)!.name}」`);
    }
  }, [anchor, drag, hitTest, icons, lastClick, marquee, order, selected, toLocal]);

  const onKeyDown = useCallback((e: React.KeyboardEvent) => {
    const focusId = anchor ?? [...selected][0] ?? null;
    if (e.key === "Escape" && drag) {
      const r = dragEscRestore();
      setDrag(null); setMarquee(null);
      setStatus("Esc 放弃拖拽——选中已复原（章三：打断有出口）");
      void r;
      return;
    }
    if (!focusId) return;
    const dir = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" }[e.key] as "up" | "down" | "left" | "right" | undefined;
    if (dir) {
      const next = gridNeighbor(NAV_INDEX, focusId, dir, icons);
      if (next) { setSelected(new Set([next])); setAnchor(next); setStatus("方向键移动选中"); }
      e.preventDefault();
      return;
    }
    if (e.key === "Home" || e.key === "End") {
      const next = gridRowEdge(NAV_INDEX, focusId, e.key === "Home" ? "home" : "end", icons);
      if (next) { setSelected(new Set([next])); setAnchor(next); }
      e.preventDefault();
      return;
    }
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey) {
      const t = typeAheadPush(taBuffer, e.key.toLowerCase(), performance.now(), lastTaAt.current);
      lastTaAt.current = performance.now();
      setTaBuffer(t.buffer);
      const hit = typeAheadMatch(t.buffer, icons.map((i) => ({ id: i.id, name: i.name })));
      if (hit) { setSelected(new Set([hit])); setAnchor(hit); setStatus(`输入即达「${icons.find((i) => i.id === hit)!.name}」`); }
      e.preventDefault();
    }
  }, [anchor, drag, icons, selected, taBuffer]);

  const dragTarget = drag ? icons.find((i) => i.id === drag.ghost.ids[0]) : null;

  return (
    <div
      className="u3-desk-surface"
      tabIndex={0}
      role="listbox"
      aria-label="桌面图标演示面"
      aria-activedescendant={anchor ?? undefined}
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onKeyDown={onKeyDown}
    >
      {icons.map((i) => (
        <div
          key={i.id}
          id={`desk-${i.id}`}
          role="option"
          aria-selected={selected.has(i.id)}
          className={"u3-desk-icon" + (selected.has(i.id) ? " is-selected" : "")}
          style={{ left: i.x + 12, top: i.y + 12, width: CELL.colPx - 24, height: CELL.rowPx - 24 }}
        >
          <span className="u3-desk-glyph" aria-hidden>{i.name.slice(0, 1)}</span>
          <span className="u3-desk-name">{i.name}</span>
        </div>
      ))}
      {marquee && (
        <div
          className="u3-desk-marquee"
          style={{
            left: Math.min(marquee.from.x, marquee.to.x),
            top: Math.min(marquee.from.y, marquee.to.y),
            width: Math.abs(marquee.to.x - marquee.from.x),
            height: Math.abs(marquee.to.y - marquee.from.y),
          }}
        />
      )}
      {drag && dragTarget && (
        <div className="u3-desk-ghost" style={{ left: drag.ghost.x + 12, top: drag.ghost.y + 12, width: CELL.colPx - 24, opacity: drag.ghost.opacity }}>
          <span className="u3-desk-glyph" aria-hidden>{dragTarget.name.slice(0, 1)}</span>
          {drag.ghost.ids.length > 1 && <span className="u3-desk-badge">{drag.ghost.ids.length}</span>}
        </div>
      )}
      <div className="u3-desk-status" role="status">{status}</div>
    </div>
  );
}

/* ------------------------------ U3Lab 演示区 ------------------------------ */

/** U3Lab v5 桌面实绘区（瞥桌面 + 布局锁真实开关驱动图标面）。 */
export function DeskPaintSection(): React.ReactElement {
  const [peeking, setPeeking] = useState(false);
  const [locked, setLocked] = useState(() => !!u3Store.getWith("layoutLock", "locked", false));
  const peek = peekVerdict(peeking);

  return (
    <SectionCard title="桌面实绘装配区" f="F502/F503/F537/F539·v5">
      <div className="u3-lab-intro">
        橡皮筋/键盘导航/拖拽吸附/双击防抖全输入路径活体——逻辑全部来自 despaint 引擎（与桌面领地同源，非演示特供）。
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F537</span>瞥桌面（Win+逗号）</div>
          <div className="u3-desc">按住 = 窗口透明只读、松手复原；参数与 winkeys.peekStyle 同源（0.15/120ms）</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn"
            onPointerDown={() => setPeeking(true)}
            onPointerUp={() => setPeeking(false)}
            onPointerLeave={() => setPeeking(false)}>
            {peeking ? "瞥视中…松手复原" : "按住瞥桌面"}
          </button>
          <span className="u3-stat">透明度 {peek.style.opacity} · 过渡 {peek.style.transition}</span>
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F539</span>布局锁定</div>
          <div className="u3-desc">锁定后拖拽启动即拒绝并抖动反馈（deskiconLayer.layoutLockGuard 同源）</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <label className="u3-check">
            <input type="checkbox" checked={locked} onChange={(e) => { setLocked(e.target.checked); u3Store.set("layoutLock", { ...u3Store.get("layoutLock"), locked: e.target.checked }); }} />
            锁定桌面布局
          </label>
        </div>
      </div>
      <DeskIconSurface />
    </SectionCard>
  );
}
