import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save as saveDialog, open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  MousePointer2, Hand, Plus, Grid3X3, Magnet, Maximize2, Crosshair,
  Undo2, Redo2, ZoomIn, ZoomOut, Search, Map as MiniMapIcon, Save, FileDown,
  FolderOpen,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { MindEdge, MindNode, Mindmap } from "../../lib/types";
import { clamp, uid } from "../../lib/format";
import { bumpMapList, pushToast, resetGlobalCanvasInteraction, uiStore, useUi } from "../../state/uiStore";
import { askConfirm, askPrompt, askConfirmBubble } from "../../components/Modal";
import { openContextMenu } from "../../components/ContextMenu";
import { sanitizeHtml } from "../../lib/sanitize";
import { parseMindmapFile } from "../../lib/mindmapFile";
import type { Settings } from "../../lib/settings";
import {
  boxIntersectsRect, boxShapeExempt, clampDims, clampInteractive, computeGuides, growDimsForText, MAX_AUTO_H, MAX_TEXT_W, MIN_NODE_H,
  MIN_NODE_W, PREFERRED_TEXT_W, sanitizeDims, shapeCollapsed, vertexDragSigns,
  type GuideLine,
} from "./geometry";
import { EdgeLayer } from "./EdgeLayer";
import { MindNodeView } from "./MindNodeView";
import { NodeMoreMenu } from "./NodeMoreMenu";
import { EdgePopover } from "./EdgePopover";
import { SelectionOpsBar } from "./SelectionOpsBar";
import { Minimap } from "./Minimap";
import { InspectorPanel } from "./InspectorPanel";
import { DockBar } from "./DockBar";
// 项目结构可视化引擎（规范一~八章）
import { ingestProject } from "../projectviz/ingest";
import { buildDrillDown, KIND_BORDER, type ProjectModel } from "../projectviz/generate";
import { ProjectImportOverlay, FileInfoCard, type PvImportState } from "../projectviz/ProjectVizPanels";
import type { FileAnalysis, GenGraph } from "../projectviz/types";

const WORLD_W = 20000;
const WORLD_H = 16000;
const SNAP_GRID = 10;
const MIN_W = MIN_NODE_W;
const MAX_W = 640;
const MIN_H = MIN_NODE_H;

/** Module-4: normalize corrupt node dimensions loaded from the DB or files —
 *  NaN / Infinity / sub-minimum values are reset to a sane standard ratio and
 *  every repair is logged once per load instead of rendering garbage paths. */
function healNodeDims(list: MindNode[]): MindNode[] {
  let fixed = 0;
  const out = list.map((nd) => {
    const { dim, repaired } = sanitizeDims(nd.width, nd.height);
    if (!repaired) return nd;
    fixed++;
    return { ...nd, ...dim };
  });
  if (fixed > 0) {
    console.warn(`[mindmap] healed ${fixed} node(s) with corrupt dimensions`);
    void ipc.log("warn", `healed ${fixed} corrupt node dims on load`).catch(() => {});
  }
  return out;
}
/** Hard zoom limits: never dissolve content, never melt the renderer. */
export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 10;

interface Snapshot {
  nodes: MindNode[];
  edges: MindEdge[];
}

/** Clipboard survives map switches within the session (cross-canvas paste). */
const graphClipboard: { current: Snapshot | null } = { current: null };
const styleClipboard: { current: Partial<MindNode> | null } = { current: null };
/**
 * Open-contract flag (spec 一/三): set when a local file open wants the
 * guaranteed-visible path; consumed by the load effect which then FITS THE
 * VIEWPORT TO THE CONTENT instead of restoring the new map's default (0,0)
 * camera — the #1 cause of "opened successfully but the canvas is blank".
 * Module-level so it survives the write→mindmap mode remount.
 */
let pendingOpenFit = false;

export const NODE_PRESETS = ["", "tech", "modern", "minimal", "handdrawn", "pixel", "cyber"] as const;

export function MindmapView(props: { settings: Settings }): React.ReactElement {
  const { t, lang } = useI18n();
  const currentMapId = useUi((s) => s.currentMapId);
  const [map, setMap] = useState<Mindmap | null>(null);
  const [mapsList, setMapsList] = useState<{ id: string; name: string }[]>([]);
  const [chooserOpen, setChooserOpen] = useState(false);
  const [nodes, setNodes] = useState<MindNode[]>([]);
  const [edges, setEdges] = useState<MindEdge[]>([]);
  const [vp, setVp] = useState({ x: 0, y: 0, zoom: 1 });
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [selectedEdges, setSelectedEdges] = useState<Set<string>>(new Set());
  const [editingId, setEditingId] = useState<string | null>(null);
  const [tool, setTool] = useState<"pan" | "select">("pan");
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [connectingFrom, setConnectingFrom] = useState<string | null>(null);
  const [connectPos, setConnectPos] = useState<{ x: number; y: number } | null>(null);
  const [menuAnchor, setMenuAnchor] = useState<{ id: string } | null>(null);
  const [edgePop, setEdgePop] = useState<{ id: string } | null>(null);
  const [quickFind, setQuickFind] = useState(false);
  const [quickQuery, setQuickQuery] = useState("");
  const [restoredNote, setRestoredNote] = useState(false);
  const [guides, setGuides] = useState<GuideLine[] | null>(null);
  const [minimapOpen, setMinimapOpen] = useState(true);
  const [draggingNode, setDraggingNode] = useState(false);
  const [freeTransform, setFreeTransform] = useState<Set<string>>(new Set());
  // 自适应回缩的豁免名单镜像：window 级监听器（一次性注册）需要最新值。
  const freeTransformRef = useRef<Set<string>>(new Set());
  useEffect(() => { freeTransformRef.current = freeTransform; }, [freeTransform]);
  const [cursorWorld, setCursorWorld] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  /** Nodes whose selection glow/handles are playing the forced fade-out
   *  animation after a blank-canvas dismissal (module-0 protocol). */
  const [fadeGhosts, setFadeGhosts] = useState<Set<string>>(new Set());
  const ghostTimer = useRef(0);
  /** Session map of mapId → linked local .mindmap/.json file (save target). */
  const [linkedPaths, setLinkedPaths] = useState<Record<string, string>>(() => {
    try {
      const raw = localStorage.getItem("mm.fileLink.v1");
      if (raw) return JSON.parse(raw) as Record<string, string>;
    } catch { /* corrupt → empty */ }
    return {};
  });
  const linkedFilesRef = useRef<Record<string, string>>(linkedPaths);
  function setLinkedFile(mapId: string, path: string): void {
    linkedFilesRef.current = { ...linkedFilesRef.current, [mapId]: path };
    setLinkedPaths(linkedFilesRef.current);
    try { localStorage.setItem("mm.fileLink.v1", JSON.stringify(linkedFilesRef.current)); } catch { /* quota */ }
  }
  /** Live settings mirror for single-mount event listeners. */
  const settingsRef = useRef(props.settings);
  settingsRef.current = props.settings;

  // ---- project visualization engine state (spec chapters 2-7) ----
  const [pvImport, setPvImport] = useState<PvImportState | null>(null);
  const [pvInfoId, setPvInfoId] = useState<string | null>(null);
  const pvModelRef = useRef<ProjectModel | null>(null);

  // Starfield engine feed: the background's depth parallax and zoom bokeh
  // track the canvas viewport (chapter 3, L1/L3/L7).
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("variable:mm-viewport", { detail: vp }));
  }, [vp]);

  // L10 gravity field feed: sparse branch centroids pushed on a slow clock.
  useEffect(() => {
    const iv = window.setInterval(() => {
      if (uiStore.getState().mode !== "mindmap") return;
      const list = nodesRef.current;
      if (list.length === 0) return;
      const step = Math.max(1, Math.ceil(list.length / 16));
      const centers: Array<{ x: number; y: number }> = [];
      for (let i = 0; i < list.length && centers.length < 16; i += step) {
        const n = list[i]!;
        centers.push({ x: n.x + n.width / 2, y: n.y + n.height / 2 });
      }
      window.dispatchEvent(new CustomEvent("variable:mm-topology", { detail: centers }));
    }, 5000);
    return () => window.clearInterval(iv);
  }, []);

  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<
    | { kind: "pan"; sx: number; sy: number; ox: number; oy: number }
    | { kind: "move"; sx: number; sy: number; origins: Map<string, { x: number; y: number }> }
    | { kind: "resize"; id: string; handle: string; sx: number; sy: number; orig: { x: number; y: number; w: number; h: number }; vs?: { sx: number; sy: number } }
    | { kind: "marquee"; sx: number; sy: number }
    | null
  >(null);
  const spaceRef = useRef(false);
  const pendingSaveIds = useRef<Set<string>>(new Set());
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const metaTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const vpRef = useRef(vp);
  vpRef.current = vp;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const edgesRef = useRef(edges);
  edgesRef.current = edges;
  const mapRef = useRef(map);
  mapRef.current = map;
  const history = useRef<{ past: Snapshot[]; future: Snapshot[] }>({ past: [], future: [] });
  const clickTracker = useRef({ time: 0, id: "", count: 0 });
  const marqueeRef = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const pendingFocusRef = useRef<string | null>(null);
  const keysRef = useRef<Set<string>>(new Set());
  const velRef = useRef({ vx: 0, vy: 0 });
  /** Live editing-session mirror so the rAF nav loop can gate without re-render deps. */
  const editingIdRef = useRef<string | null>(null);
  editingIdRef.current = editingId;
  const selectionRef = useRef<Set<string>>(selection);
  selectionRef.current = selection;
  /** Window focus mirror: blur must freeze navigation instantly. */
  const winFocusRef = useRef(true);
  const navRafRef = useRef(0);
  const metaDirtyRef = useRef(false);
  const animRef = useRef<{ from: { x: number; y: number; zoom: number }; to: { x: number; y: number; zoom: number }; t0: number; dur: number } | null>(null);
  const lastCursorTick = useRef(0);

  // ---------- module-0: forced-destroy dismissal protocol ----------
  /** Play the selection fade-out for the given nodes, then drop the ghosts. */
  function fadeClearSelection(ids: Set<string>): void {
    if (ids.size === 0) return;
    window.clearTimeout(ghostTimer.current);
    setFadeGhosts(new Set(ids));
    ghostTimer.current = window.setTimeout(() => setFadeGhosts(new Set()), 220);
  }

  /** Full reset of every local overlay/activation state, with fade-outs.
   *  Invoked from BOTH the CanvasRoot blank-pointer-down handler and the
   *  global `variable:mm-dismiss-all` broadcast of the state machine. */
  const dismissAllLocal = useCallback((): void => {
    if (editingIdRef.current) setEditingId(null);
    const sel = selectionRef.current;
    if (sel.size > 0) {
      setSelection(new Set());
      fadeClearSelection(sel);
    }
    setSelectedEdges((prev) => (prev.size > 0 ? new Set<string>() : prev));
    setFreeTransform((prev) => (prev.size > 0 ? new Set<string>() : prev));
    setConnectingFrom((prev) => {
      if (prev !== null) setConnectPos(null);
      return prev === null ? prev : null;
    });
    setQuickFind(false);
    setGuides(null);
    // menuAnchor / edgePop intentionally NOT cleared here: NodeMoreMenu and
    // EdgePopover listen to this same broadcast and run their own 150ms
    // fade-out before calling onClose (which clears the anchor).
  }, []);

  useEffect(() => {
    const onDismissAll = (): void => dismissAllLocal();
    window.addEventListener("variable:mm-dismiss-all", onDismissAll);
    return () => window.removeEventListener("variable:mm-dismiss-all", onDismissAll);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dismissAllLocal]);

  // Mirror the interaction state into the global state machine so external
  // surfaces (and future callers of resetGlobalCanvasInteraction) always see
  // an accurate picture of what is active on the canvas.
  useEffect(() => {
    uiStore.setState({ selectedFrameId: Array.from(selection)[0] ?? null });
  }, [selection]);
  useEffect(() => {
    uiStore.setState({ editingId });
  }, [editingId]);


  // ---------- persistence ----------
  const flushNodesSave = useCallback(async (): Promise<void> => {
    if (saveTimer.current) {
      clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    const idsToSave = Array.from(pendingSaveIds.current);
    pendingSaveIds.current.clear();
    if (idsToSave.length === 0) return;
    const latest = nodesRef.current.filter((n) => idsToSave.includes(n.id));
    if (latest.length === 0) return;
    try {
      await ipc.saveNodes(latest);
    } catch (e) {
      pushToast("error",
        lang === "zh" ? "节点保存失败（内容保留在内存中）" : "Node save failed (kept in memory)",
        errMessage(e).message);
    }
  }, [lang]);

  const scheduleNodesSave = useCallback((ids: string[]) => {
    ids.forEach((id) => pendingSaveIds.current.add(id));
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => void flushNodesSave(), 550);
  }, [flushNodesSave]);

  const scheduleMetaSave = useCallback(() => {
    metaDirtyRef.current = true;
    if (metaTimer.current) clearTimeout(metaTimer.current);
    metaTimer.current = setTimeout(() => {
      metaDirtyRef.current = false;
      const m = mapRef.current;
      if (!m) return;
      void ipc.updateMindmap({
        id: m.id,
        viewportX: Math.round(vpRef.current.x),
        viewportY: Math.round(vpRef.current.y),
        zoom: Math.round(vpRef.current.zoom * 1000) / 1000,
      }).catch(() => {});
    }, 900);
  }, []);

  // ---------- animated viewport (eased zoom / centering) ----------
  const stopAnim = useCallback(() => {
    animRef.current = null;
    if (navRafRef.current) {
      cancelAnimationFrame(navRafRef.current);
      navRafRef.current = 0;
    }
  }, []);

  // ---------- WASD instant navigation (no accel / no inertia) ----------
  // (Declared before animateVpTo so a finishing animation can hand control
  //  back to keys that are still physically held down.)
  const ensureNavLoop = useCallback((): void => {
    if (navRafRef.current) return; // already running
    stopAnim(); // manual navigation cancels eased animations
    let last = performance.now();
    const tick = (now: number): void => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const keys = keysRef.current;
      const base = props.settings.mindDefaults.wasdSpeed;
      const boost = keys.has("shift") ? 3 : keys.has("ctrl") ? 0.25 : 1;
      const speed = base * boost;
      let ax = 0, ay = 0;
      if (keys.has("w")) ay += speed;
      if (keys.has("s")) ay -= speed;
      if (keys.has("a")) ax += speed;
      if (keys.has("d")) ax -= speed;
      // Per-frame intent gate (module-1): navigation is suppressed while the
      // user is typing, deep-editing a frame, or the window lost focus. Keys
      // stay tracked, so the instant the gate reopens motion resumes without
      // another keypress. Blocked frames decay existing inertia only.
      const ae = document.activeElement as HTMLElement | null;
      const typing = !!ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.isContentEditable);
      if (uiStore.getState().mode !== "mindmap" || editingIdRef.current || typing || !winFocusRef.current) {
        ax = 0; ay = 0;
      }
      const vel = velRef.current;
      // 即时跟手平移：速度直接等于按键意图，无加速曲线、无惯性滑行，
      // 松键立即停止（被门控拦截时速度为零）。
      vel.vx = ax;
      vel.vy = ay;
      if (vel.vx !== 0 || vel.vy !== 0) {
        animRef.current = null; // manual input cancels eased animations
        const cur = vpRef.current;
        const nv = { ...cur, x: cur.x + vel.vx * dt, y: cur.y + vel.vy * dt };
        vpRef.current = nv;
        setVp(nv);
        metaDirtyRef.current = true;
        navRafRef.current = requestAnimationFrame(tick);
        return;
      }
      if (metaDirtyRef.current) scheduleMetaSave();
      navRafRef.current = keys.size > 0 ? requestAnimationFrame(tick) : 0;
    };
    navRafRef.current = requestAnimationFrame(tick);
  }, [props.settings.mindDefaults.wasdSpeed, scheduleMetaSave, stopAnim]);

  /**
   * Eased viewport transition. When `anchor` is provided (wheel zoom), the
   * caller-supplied world point stays under the pointer during EVERY frame —
   * zoom eases, x/y are derived from it, never lerped.
   * Contract: `anchor.world` MUST be computed against the LIVE viewport at the
   * moment of the call (single basis; see zoomAtAnimated).
   */
  const animateVpTo = useCallback(
    (
      to: { x: number; y: number; zoom: number },
      dur = 260,
      anchor?: { screen: { x: number; y: number }; world: { x: number; y: number } },
    ): void => {
      velRef.current = { vx: 0, vy: 0 };
      if (navRafRef.current) {
        cancelAnimationFrame(navRafRef.current);
        navRafRef.current = 0;
      }
      const from = vpRef.current;
      const w = anchor ? anchor.world : null;
      animRef.current = { from, to, t0: performance.now(), dur };
      const tick = (now: number): void => {
        const a = animRef.current;
        if (!a) return;
        const k = Math.min(1, (now - a.t0) / a.dur);
        const e = 1 - Math.pow(1 - k, 3); // easeOutCubic
        let v: { x: number; y: number; zoom: number };
        if (w && anchor) {
          const z = a.from.zoom + (a.to.zoom - a.from.zoom) * e;
          v = { zoom: z, x: anchor.screen.x - w.x * z, y: anchor.screen.y - w.y * z };
        } else {
          v = {
            zoom: a.from.zoom + (a.to.zoom - a.from.zoom) * e,
            x: a.from.x + (a.to.x - a.from.x) * e,
            y: a.from.y + (a.to.y - a.from.y) * e,
          };
        }
        vpRef.current = v;
        setVp(v);
        if (k < 1) {
          navRafRef.current = requestAnimationFrame(tick);
        } else {
          animRef.current = null;
          navRafRef.current = 0;
          scheduleMetaSave();
          // Hand control back to keys still physically held down.
          if (keysRef.current.size > 0) ensureNavLoop();
        }
      };
      navRafRef.current = requestAnimationFrame(tick);
    },
    [scheduleMetaSave, ensureNavLoop],
  );

  // ---------- capture-phase key rail (module-1) ----------
  // Physical key tracking runs in the CAPTURE phase, before any bubbling
  // stopPropagation anywhere in the tree can swallow the event. Tracking is
  // inert bookkeeping; motion is decided per-frame by the nav loop's gate.
  // Idempotent under StrictMode double-mounting.
  useEffect(() => {
    const MOVES = new Set(["w", "a", "s", "d"]);
    const ALIAS: Record<string, string> = { ArrowUp: "w", ArrowDown: "s", ArrowLeft: "a", ArrowRight: "d" };
    const trackDown = (e: KeyboardEvent): void => {
      const k = e.key.toLowerCase();
      if (e.key === "Shift") { keysRef.current.add("shift"); return; }
      if (e.key === "Control") { keysRef.current.add("ctrl"); return; }
      if (MOVES.has(k)) {
        keysRef.current.add(k);
        if (!e.repeat) ensureNavLoop(); // kick the physics loop on fresh press
        return;
      }
      const alias = ALIAS[e.key];
      if (alias) {
        keysRef.current.add(alias);
        if (!e.repeat) ensureNavLoop();
      }
    };
    const trackUp = (e: KeyboardEvent): void => {
      if (e.key === "Shift") { keysRef.current.delete("shift"); return; }
      if (e.key === "Control") { keysRef.current.delete("ctrl"); return; }
      const k = e.key.toLowerCase();
      if (MOVES.has(k)) keysRef.current.delete(k);
      const alias = ALIAS[e.key];
      if (alias) keysRef.current.delete(alias);
    };
    const mm = (e: MouseEvent): void => {
      lastMousePos.current = { x: e.clientX, y: e.clientY };
    };
    const onBlur = (): void => {
      winFocusRef.current = false;
      keysRef.current.clear();
      spaceRef.current = false;
      velRef.current = { vx: 0, vy: 0 };
    };
    const onFocus = (): void => { winFocusRef.current = true; };
    window.addEventListener("keydown", trackDown, true);
    window.addEventListener("keyup", trackUp, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    window.addEventListener("mousemove", mm, { passive: true });
    return () => {
      window.removeEventListener("keydown", trackDown, true);
      window.removeEventListener("keyup", trackUp, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("mousemove", mm);
      keysRef.current.clear();
      velRef.current = { vx: 0, vy: 0 };
    };
  }, [ensureNavLoop]);

  // ---------- loading: prefer last opened map + last viewport ----------
  useEffect(() => {
    let cancelled = false;
    async function load(): Promise<void> {
      try {
        let id = currentMapId;
        if (!id) {
          const raw = await ipc.getSettings();
          id = raw["lastMindmapId"] ?? null;
          if (!id) id = (await ipc.listMindmaps())[0]?.id ?? null;
        }
        if (!id) {
          const maps = await ipc.listMindmaps();
          if (cancelled) return;
          setMapsList(maps.map((m) => ({ id: m.id, name: m.name })));
          setChooserOpen(true);
          setMap(null);
          setNodes([]);
          setEdges([]);
          return;
        }
        const data = await ipc.getMindmap(id);
        if (cancelled) return;
        setMap(data.mindmap);
        // Module-4: normalize corrupt dims from the DB before anything renders.
        const loadedNodes = healNodeDims(data.nodes);
        setNodes(loadedNodes);
        setEdges(data.edges);
        history.current = { past: [], future: [] };
        setSelection(new Set());
        setSelectedEdges(new Set());
        setEditingId(null);
        setFadeGhosts(new Set());
        setChooserOpen(false);
        setMenuAnchor(null);
        setEdgePop(null);
        setQuickFind(false);
        setConnectingFrom(null);
        setConnectPos(null);
        // Clamp restored zoom: a corrupt DB value must never break rendering.
        const rawZ = data.mindmap.zoom;
        setVp({
          x: data.mindmap.viewportX,
          y: data.mindmap.viewportY,
          zoom: Number.isFinite(rawZ) ? clamp(rawZ, MIN_ZOOM, MAX_ZOOM) : 1,
        });
        // Open-contract "加载成功三连": a freshly opened FILE always lands the
        // camera on its content — the new map's stored viewport is the default
        // (0,0) while nodes keep their original (possibly far) coordinates,
        // which rendered as a guaranteed-blank canvas before this fix.
        if (pendingOpenFit) {
          pendingOpenFit = false;
          if (loadedNodes.length > 0) {
            let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
            for (const nd of loadedNodes) {
              minX = Math.min(minX, nd.x);
              minY = Math.min(minY, nd.y);
              maxX = Math.max(maxX, nd.x + nd.width);
              maxY = Math.max(maxY, nd.y + nd.height);
            }
            const el = containerRef.current;
            const pad = 80;
            const zw = ((el?.clientWidth ?? 900) - pad * 2) / Math.max(1, maxX - minX);
            const zh = ((el?.clientHeight ?? 640) - pad * 2) / Math.max(1, maxY - minY);
            const z = clamp(Math.min(zw, zh, 1.6), MIN_ZOOM, MAX_ZOOM);
            setVp({ x: -((minX + maxX) / 2) * z, y: -((minY + maxY) / 2) * z, zoom: z });
          }
        }
        // Mounted self-check (spec 七): data rows must exist in the DOM next
        // frame, or the store→render link is broken — log it loudly. Runs on
        // EVERY load (not only file opens) so a DB-load mount failure can
        // never pass silently either.
        requestAnimationFrame(() => {
          const rendered = containerRef.current?.querySelectorAll(".mm-node").length ?? 0;
          const visible = loadedNodes.filter((n) => !n.hidden).length;
          if (visible > 0 && rendered === 0) {
            console.error(`[mindmap] mount check FAILED: ${visible} nodes in store, 0 in DOM`);
            void ipc.log("error", `open self-check: ${visible} nodes, 0 rendered`).catch(() => {});
          }
        });
        if (data.nodes.length > 0 || data.mindmap.viewportX !== 0 || data.mindmap.viewportY !== 0 || data.mindmap.zoom !== 1) {
          setRestoredNote(true);
          setTimeout(() => setRestoredNote(false), 2400);
        }
        await ipc.setSettings({ lastMindmapId: id }).catch(() => {});
        // A search hit may have requested focus before the map finished loading.
        const pending = pendingFocusRef.current;
        pendingFocusRef.current = null;
        if (pending) attemptFocusNode(pending, data.nodes);
      } catch (e) {
        if (!cancelled) pushToast("error", lang === "zh" ? "导图加载失败" : "Failed to load map", errMessage(e).message);
      }
    }
    void load();
    return () => {
      cancelled = true;
      if (saveTimer.current) clearTimeout(saveTimer.current);
      const ids = Array.from(pendingSaveIds.current);
      pendingSaveIds.current.clear();
      if (ids.length > 0) {
        const latest = nodesRef.current.filter((n) => ids.includes(n.id));
        if (latest.length > 0) void ipc.saveNodes(latest).catch(() => {});
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentMapId]);

  // ---------- history (undo/redo of the whole graph) ----------
  const pushHistory = useCallback((): void => {
    history.current.past.push({ nodes: structuredClone(nodesRef.current), edges: structuredClone(edgesRef.current) });
    if (history.current.past.length > 100) history.current.past.shift();
    history.current.future = [];
  }, []);

  const undo = useCallback((): void => {
    const prev = history.current.past.pop();
    if (!prev) return;
    history.current.future.push({ nodes: structuredClone(nodesRef.current), edges: structuredClone(edgesRef.current) });
    void applySnapshot(prev);
  }, []);

  const redo = useCallback((): void => {
    const next = history.current.future.pop();
    if (!next) return;
    history.current.past.push({ nodes: structuredClone(nodesRef.current), edges: structuredClone(edgesRef.current) });
    void applySnapshot(next);
  }, []);

  /**
   * Replace the whole graph with a snapshot AND reconcile the database so a
   * reload never resurrects ghost nodes/edges after undo/redo.
   */
  async function applySnapshot(snap: Snapshot): Promise<void> {
    const curNodes = nodesRef.current;
    const curEdges = edgesRef.current;
    setNodes(snap.nodes);
    setEdges(snap.edges);
    setSelection(new Set());
    setSelectedEdges(new Set());
    setEditingId(null);
    closeOverlays();
    try {
      const delNodes = curNodes.filter((n) => !snap.nodes.some((p) => p.id === n.id)).map((n) => n.id);
      const delEdges = curEdges.filter((e) => !snap.edges.some((p) => p.id === e.id)).map((e) => e.id);
      if (delNodes.length > 0) await ipc.deleteNodes(delNodes);
      if (delEdges.length > 0) await ipc.deleteEdges(delEdges);
      if (snap.nodes.length > 0) await ipc.saveNodes(snap.nodes);
      for (const e of snap.edges) await ipc.saveEdge(e);
    } catch (err) {
      pushToast("error", lang === "zh" ? "撤销同步失败（内存状态正确）" : "Undo sync failed (in-memory state is correct)", errMessage(err).message);
    }
  }

  function closeOverlays(): void {
    setMenuAnchor(null);
    setEdgePop(null);
  }

  // ---------- coordinates & view ops ----------
  // Mapping contract (exact at any zoom):
  //   screen = canvasCenter + v + world * zoom
  // The .mm-origin element sits at the canvas center (left:50% top:50%, 0×0),
  // and .mm-world applies translate(v) scale(z) with transform-origin 0 0.
  const toWorld = useCallback((clientX: number, clientY: number): { x: number; y: number } => {
    const el = containerRef.current;
    if (!el) return { x: 0, y: 0 };
    const r = el.getBoundingClientRect();
    const v = vpRef.current;
    return {
      x: (clientX - r.left - r.width / 2 - v.x) / v.zoom,
      y: (clientY - r.top - r.height / 2 - v.y) / v.zoom,
    };
  }, []);

  /** Screen point for a world coordinate (used to center nodes precisely). */
  function centerOnWorld(wx: number, wy: number, zoom?: number): void {
    const z = zoom ?? vpRef.current.zoom;
    animateVpTo({ zoom: z, x: -wx * z, y: -wy * z }, 280);
  }

  /** 即时跟手的指针锚定缩放（滚轮/按钮/快捷键）：不做任何缓动插值，
   *  每次事件都从实时视口出发一步到位，光标下的世界坐标严格保持不动。 */
  const zoomAt = useCallback((factor: number, cx?: number, cy?: number): void => {
    if (dragRef.current) return; // no re-mapping while something is being dragged
    const el = containerRef.current;
    if (!el) return;
    stopAnim();
    velRef.current = { vx: 0, vy: 0 };
    const r = el.getBoundingClientRect();
    // No explicit point → zoom around the geometric center of the viewport.
    const px = (cx ?? r.width / 2) - r.width / 2;
    const py = (cy ?? r.height / 2) - r.height / 2;
    const cur = vpRef.current;
    const z2 = clamp(cur.zoom * factor, MIN_ZOOM, MAX_ZOOM);
    const wx = (px - cur.x) / cur.zoom;
    const wy = (py - cur.y) / cur.zoom;
    const nv = { zoom: z2, x: px - wx * z2, y: py - wy * z2 };
    vpRef.current = nv;
    setVp(nv);
    scheduleMetaSave();
  }, [scheduleMetaSave, stopAnim]);

  const fitAll = useCallback((onlySelection = false): void => {
    const src = onlySelection && selection.size > 0 ? nodes.filter((n) => selection.has(n.id)) : nodes;
    const el = containerRef.current;
    if (!el || src.length === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of src) {
      minX = Math.min(minX, n.x); minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x + n.width); maxY = Math.max(maxY, n.y + n.height);
    }
    const pad = 80;
    const zw = (el.clientWidth - pad * 2) / Math.max(1, maxX - minX);
    const zh = (el.clientHeight - pad * 2) / Math.max(1, maxY - minY);
    const zoom = clamp(Math.min(zw, zh, 1.6), MIN_ZOOM, MAX_ZOOM);
    centerOnWorld((minX + maxX) / 2, (minY + maxY) / 2, zoom);
  }, [nodes, selection]);

  const homeCenter = useCallback((): void => {
    animateVpTo({ ...vpRef.current, x: 0, y: 0 }, 260);
  }, [animateVpTo]);

  /** Reset viewport to the default zoom & centered origin (Numpad 0). */
  const resetViewport = useCallback((): void => {
    animateVpTo({ x: 0, y: 0, zoom: 1 }, 300);
  }, [animateVpTo]);

  const snapVal = useCallback((v: number): number =>
    (mapRef.current?.snapEnabled ?? props.settings.mindDefaults.snapEnabled)
      ? Math.round(v / SNAP_GRID) * SNAP_GRID
      : Math.round(v), [props.settings.mindDefaults.snapEnabled]);

  function nextZ(): number {
    return nodes.reduce((m, n) => Math.max(m, n.zIndex), 0) + 1;
  }

  function makeNode(x: number, y: number, html: string): MindNode {
    return {
      id: uid(),
      mindmapId: map?.id ?? "",
      textHtml: html,
      textPlain: "",
      x: snapVal(x),
      y: snapVal(y),
      // Standard frame ratio (module-4): always above the hard minimums.
      width: 240,
      height: 88,
      shape: props.settings.mindDefaults.defaultShape,
      borderRadius: 14,
      borderColor: "#5b7bd0",
      fillColor: "rgba(13,20,38,0.88)",
      fontSize: 15,
      opacity: 1,
      locked: false,
      zIndex: nextZ(),
      recordId: null,
      rotation: 0,
      groupId: null,
      hidden: false,
      collapsed: false,
      preset: "",
      updatedAt: Date.now(),
    };
  }

  function createNodeAt(wx: number, wy: number, html = ""): MindNode {
    pushHistory();
    const n = makeNode(wx, wy, html);
    setNodes((prev) => [...prev, n]);
    setSelection(new Set([n.id]));
    setSelectedEdges(new Set());
    setEditingId(n.id);
    void ipc.saveNodes([n]).catch((e) =>
      pushToast("error", lang === "zh" ? "创建失败" : "Create failed", errMessage(e).message));
    return n;
  }

  function patchNode(id: string, patch: Partial<MindNode>): void {
    setNodes((prev) => prev.map((n) => (n.id === id ? { ...n, ...patch, updatedAt: Date.now() } : n)));
    scheduleNodesSave([id]);
  }

  function patchMany(ids: Set<string>, patch: Partial<MindNode>): void {
    pushHistory();
    setNodes((prev) => prev.map((n) => (ids.has(n.id) ? { ...n, ...patch, updatedAt: Date.now() } : n)));
    scheduleNodesSave(Array.from(ids));
  }

  async function deleteSelection(): Promise<void> {
    if (selection.size === 0) return;
    const ok = await askConfirm({ title: t("deleteNode"), body: t("deleteNodesConfirm", { n: selection.size }), danger: true });
    if (!ok) return;
    pushHistory();
    const doomedEdges = edges.filter((e) => selection.has(e.sourceNodeId) || selection.has(e.targetNodeId)).map((e) => e.id);
    const doomedNodes = Array.from(selection);
    setEdges((prev) => prev.filter((e) => !doomedEdges.includes(e.id)));
    setNodes((prev) => prev.filter((n) => !selection.has(n.id)));
    setSelection(new Set());
    setEditingId(null);
    setMenuAnchor(null);
    setFreeTransform(new Set());
    try {
      if (doomedEdges.length > 0) await ipc.deleteEdges(doomedEdges);
      await ipc.deleteNodes(doomedNodes);
    } catch (e) {
      pushToast("error", lang === "zh" ? "删除失败" : "Delete failed", errMessage(e).message);
    }
  }

  /** Non-modal delete confirmation bubble anchored at the cursor (spec 2.2). */
  async function deleteSelectionBubble(x?: number, y?: number): Promise<void> {
    if (selection.size === 0) return;
    const ok = await askConfirmBubble({
      x: x ?? lastMousePos.current.x,
      y: y ?? lastMousePos.current.y,
      message: t("deleteNodesConfirm", { n: selection.size }),
    });
    if (!ok) return;
    void deleteSelectionConfirmed();
  }

  async function deleteSelectionConfirmed(): Promise<void> {
    pushHistory();
    const sel = new Set(selection);
    const doomedEdges = edges.filter((e) => sel.has(e.sourceNodeId) || sel.has(e.targetNodeId)).map((e) => e.id);
    const doomedNodes = Array.from(sel);
    setEdges((prev) => prev.filter((e) => !doomedEdges.includes(e.id)));
    setNodes((prev) => prev.filter((n) => !sel.has(n.id)));
    setSelection(new Set());
    setEditingId(null);
    setMenuAnchor(null);
    setFreeTransform(new Set());
    try {
      if (doomedEdges.length > 0) await ipc.deleteEdges(doomedEdges);
      await ipc.deleteNodes(doomedNodes);
    } catch (e) {
      pushToast("error", lang === "zh" ? "删除失败" : "Delete failed", errMessage(e).message);
    }
  }

  /** Nudge selection with arrow keys; grouped frames move together. */
  function moveSelectionBy(dx: number, dy: number): void {
    if (selection.size === 0) return;
    const ids = expandWithGroups(selection);
    pushHistory();
    setNodes((prev) => prev.map((n) => (ids.has(n.id) ? { ...n, x: n.x + dx, y: n.y + dy, updatedAt: Date.now() } : n)));
    scheduleNodesSave(Array.from(ids));
  }

  /** Expand a selection so grouped frames travel as one unit. */
  function expandWithGroups(sel: Set<string>): Set<string> {
    const out = new Set(sel);
    const groupIds = new Set<string>();
    for (const n of nodesRef.current) {
      if (out.has(n.id) && n.groupId) groupIds.add(n.groupId);
    }
    if (groupIds.size > 0) {
      for (const n of nodesRef.current) {
        if (n.groupId && groupIds.has(n.groupId)) out.add(n.id);
      }
    }
    return out;
  }

  /** World-space AABB of the visible viewport (+margin). */
  function viewRectWorld(margin = 200): { x0: number; y0: number; x1: number; y1: number } {
    const el = containerRef.current;
    const r = el?.getBoundingClientRect();
    const tl = toWorld(r?.left ?? 0, r?.top ?? 0);
    const br = toWorld(r?.right ?? 1000, r?.bottom ?? 800);
    return { x0: tl.x - margin, y0: tl.y - margin, x1: br.x + margin, y1: br.y + margin };
  }

  const lastMousePos = useRef({ x: 200, y: 200 });

  // ---------- clipboard ----------
  function copySelection(cut = false): void {
    if (selection.size === 0) return;
    const ns = nodes.filter((n) => selection.has(n.id));
    const es = edges.filter((e) => selection.has(e.sourceNodeId) && selection.has(e.targetNodeId));
    graphClipboard.current = { nodes: structuredClone(ns), edges: structuredClone(es) };
    if (cut) void deleteSelection();
    else pushToast("info", t("copied"), `${ns.length}`);
  }

  function pasteClipboard(atWx?: number, atWy?: number): void {
    const clip = graphClipboard.current;
    const firstClip = clip?.nodes[0];
    if (!clip || !firstClip || clip.nodes.length === 0) return;
    pushHistory();
    const idMap = new Map<string, string>();
    const baseX = atWx ?? firstClip.x + 28;
    const baseY = atWy ?? firstClip.y + 28;
    const offX = baseX - firstClip.x;
    const offY = baseY - firstClip.y;
    const newNodes = clip.nodes.map((n) => {
      const nid = uid();
      idMap.set(n.id, nid);
      return { ...structuredClone(n), id: nid, mindmapId: map?.id ?? n.mindmapId, groupId: null, x: snapVal(n.x + offX), y: snapVal(n.y + offY), zIndex: nextZ(), updatedAt: Date.now() };
    });
    const newEdges = clip.edges.map((e) => ({
      ...structuredClone(e),
      id: uid(),
      mindmapId: map?.id ?? e.mindmapId,
      sourceNodeId: idMap.get(e.sourceNodeId) ?? "",
      targetNodeId: idMap.get(e.targetNodeId) ?? "",
      createdAt: Date.now(),
    }));
    setNodes((prev) => [...prev, ...newNodes]);
    setEdges((prev) => [...prev, ...newEdges]);
    setSelection(new Set(newNodes.map((n) => n.id)));
    void ipc.saveNodes(newNodes).catch((e) => pushToast("error", lang === "zh" ? "粘贴失败" : "Paste failed", errMessage(e).message));
    for (const e of newEdges) void ipc.saveEdge(e).catch(() => {});
  }

  // ---------- edges ----------
  async function createEdge(sourceId: string, targetId: string): Promise<void> {
    if (sourceId === targetId) return;
    if (edges.some((e) => e.sourceNodeId === sourceId && e.targetNodeId === targetId)) {
      pushToast("info", t("connect"), lang === "zh" ? "连接已存在" : "Connection already exists");
      return;
    }
    pushHistory();
    const edge: MindEdge = {
      id: uid(),
      mindmapId: map?.id ?? "",
      sourceNodeId: sourceId,
      targetNodeId: targetId,
      direction: "forward",
      lineStyle: props.settings.mindDefaults.edgeStyle,
      pathStyle: "curve",
      color: "#7f9bd9",
      width: 1.6,
      label: "",
      animated: props.settings.safeMode ? false : props.settings.mindDefaults.edgeAnim,
      glow: false,
      createdAt: Date.now(),
    };
    setEdges((prev) => [...prev, edge]);
    setSelectedEdges(new Set([edge.id]));
    try {
      await ipc.saveEdge(edge);
    } catch (e) {
      pushToast("error", lang === "zh" ? "连接保存失败" : "Edge save failed", errMessage(e).message);
    }
  }

  async function patchEdge(id: string, patch: Partial<MindEdge>): Promise<void> {
    pushHistory();
    const updated = edgesRef.current.find((e) => e.id === id);
    setEdges((prev) => prev.map((e) => (e.id === id ? { ...e, ...patch } : e)));
    if (updated) await ipc.saveEdge({ ...updated, ...patch }).catch((e) =>
      pushToast("error", lang === "zh" ? "连接保存失败" : "Edge save failed", errMessage(e).message));
  }

  async function deleteSelectedEdges(): Promise<void> {
    if (selectedEdges.size === 0) return;
    pushHistory();
    const ids = Array.from(selectedEdges);
    setEdges((prev) => prev.filter((e) => !selectedEdges.has(e.id)));
    setSelectedEdges(new Set());
    setEdgePop(null);
    await ipc.deleteEdges(ids).catch((e) => pushToast("error", lang === "zh" ? "删除失败" : "Delete failed", errMessage(e).message));
  }

  // ---------- pointer interactions ----------
  /** Interactive overlays living inside the canvas element. Pointer events on
   *  them must never start canvas drags or get preventDefault-ed (which would
   *  swallow clicks/focus). */
  const OVERLAY_SELECTOR = [
    ".mm-toolbar", ".card-pop", ".inspector", ".minimap", ".quick-find",
    ".dock-wrap", ".node-menu", ".edge-pop", ".confirm-bubble", ".node-actions",
    ".mm-status", ".mm-map-name",
  ].join(",");

  /** Module-0: CanvasRoot-level global pointer-down (spec: 全局空白点击监听).
   *  A LEFT click whose target is the blank canvas background — anything that
   *  is not a text frame, a context/popup menu or an inspector panel — must
   *  instantly force-destroy the global activation state. Menus and frames
   *  isolate themselves via stopPropagation, so this handler never needs to
   *  intercept events aimed at them; everything else passes through freely. */
  function onCanvasRootPointerDown(e: React.PointerEvent): void {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement | null;
    if (!el) return;
    if (el.closest(OVERLAY_SELECTOR)) return; // toolbars/panels own their state
    if (el.closest(".ctx-menu")) return;      // right-click menu (defence in depth)
    if (el.closest(".mm-node")) return;       // text boxes keep their session
    resetGlobalCanvasInteraction();
  }

  function onCanvasPointerDown(e: React.PointerEvent): void {
    if ((e.target as HTMLElement).closest(OVERLAY_SELECTOR)) return;
    // NOTE: stale menu/popover anchors are intentionally NOT force-cleared
    // here anymore — NodeMoreMenu / EdgePopover / ContextMenuHost each listen
    // to the dismissal broadcast below and run their own fade-out protocol
    // before unmounting (module-4), including when a different frame is hit.
    containerRef.current?.focus({ preventScroll: true });
    if ((e.target as HTMLElement).closest(".mm-node")) return; // nodes handle their own

    // Contract-2 (module-0): ANY blank-canvas left click instantly resets the
    // global interaction state machine — activeContextMenu / selectedFrameId /
    // editingId are nulled and every overlay plays its fade-out protocol.
    // Independent of the active tool, so neither pan mode nor marquee mode can
    // deadlock the deselect path. The broadcast is synchronous: local overlay
    // state is already cleared by the time the drag setup below runs.
    if (e.button === 0) {
      resetGlobalCanvasInteraction();
    }

    const isMiddle = e.button === 1;
    const wantPan = e.button === 0 && (spaceRef.current || tool === "pan");
    if (isMiddle || wantPan) {
      dragRef.current = { kind: "pan", sx: e.clientX, sy: e.clientY, ox: vpRef.current.x, oy: vpRef.current.y };
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      e.preventDefault();
      return;
    }
    if (e.button === 0 && tool === "select") {
      dragRef.current = { kind: "marquee", sx: e.clientX, sy: e.clientY };
      const w = toWorld(e.clientX, e.clientY);
      const mq0 = { x0: w.x, y0: w.y, x1: w.x, y1: w.y };
      marqueeRef.current = mq0;
      setMarquee(mq0);
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      if (!e.shiftKey) {
        setSelection(new Set());
        setSelectedEdges(new Set());
      }
    }
  }

  /** Edge auto-scroll: slow pan when dragging near the canvas boundary.
   *  Returns screen-space compensation applied to the viewport so drags stay
   *  glued to the pointer (we shift the drag reference by the same amount). */
  function autoScrollPan(e: { clientX: number; clientY: number }): { ax: number; ay: number } {
    const el = containerRef.current;
    if (!el) return { ax: 0, ay: 0 };
    const r = el.getBoundingClientRect();
    const M = 28;
    let ax = 0, ay = 0;
    if (e.clientX < r.left + M) ax = -Math.min(10, (r.left + M - e.clientX) / 3);
    else if (e.clientX > r.right - M) ax = Math.min(10, (e.clientX - (r.right - M)) / 3);
    if (e.clientY < r.top + M) ay = -Math.min(10, (r.top + M - e.clientY) / 3);
    else if (e.clientY > r.bottom - M) ay = Math.min(10, (e.clientY - (r.bottom - M)) / 3);
    if (ax !== 0 || ay !== 0) {
      stopAnim();
      const nv = { ...vpRef.current, x: vpRef.current.x + ax, y: vpRef.current.y + ay };
      vpRef.current = nv;
      setVp(nv);
    }
    return { ax, ay };
  }

  function onCanvasPointerMove(e: React.PointerEvent): void {
    // Throttle cursor readout: full re-render per mousemove would jank the UI.
    const nowMs = performance.now();
    if (nowMs - lastCursorTick.current > 60) {
      lastCursorTick.current = nowMs;
      setCursorWorld(toWorld(e.clientX, e.clientY));
    }
    if (connectingFrom) setConnectPos(toWorld(e.clientX, e.clientY));
    const d = dragRef.current;
    if (!d) return;

    if (d.kind === "pan") {
      stopAnim();
      const nv = {
        ...vpRef.current,
        x: d.ox + (e.clientX - d.sx),
        y: d.oy + (e.clientY - d.sy),
      };
      vpRef.current = nv;
      setVp(nv);
      return;
    }
    // Boundary auto-scroll for content drags; compensate the drag reference so
    // the dragged content does not jump by the pan amount.
    const { ax, ay } = autoScrollPan(e);
    if (ax !== 0 || ay !== 0) {
      d.sx += ax;
      d.sy += ay;
    }

    if (d.kind === "marquee") {
      const w = toWorld(e.clientX, e.clientY);
      const mq = marqueeRef.current ?? { x0: w.x, y0: w.y, x1: w.x, y1: w.y };
      const nm = { ...mq, x1: w.x, y1: w.y };
      marqueeRef.current = nm;
      setMarquee(nm);
    } else if (d.kind === "move") {
      let dx = (e.clientX - d.sx) / vpRef.current.zoom;
      let dy = (e.clientY - d.sy) / vpRef.current.zoom;
      setNodes((prev) =>
        prev.map((n) => {
          const o = d.origins.get(n.id);
          if (!o) return n;
          return { ...n, x: snapVal(o.x + dx), y: snapVal(o.y + dy) };
        }),
      );
      // Alignment guides + soft snap against other frames (single-frame drag).
      if (props.settings.mindDefaults.guidesEnabled && d.origins.size === 1) {
        const firstId = Array.from(d.origins.keys())[0]!;
        const origin = d.origins.get(firstId)!;
        const rawX = origin.x + dx;
        const rawY = origin.y + dy;
        const moved = nodesRef.current.find((n) => n.id === firstId);
        if (moved) {
          const g = computeGuides(
            { id: moved.id, x: rawX, y: rawY, width: moved.width, height: moved.height },
            nodesRef.current.filter((n) => !n.hidden),
            6 / vpRef.current.zoom,
          );
          // Soft snap: pull the dragged frame exactly onto the guide line.
          const vLine = g.find((x) => x.axis === "v");
          const hLine = g.find((x) => x.axis === "h");
          if (vLine || hLine) {
            let adjX = dx, adjY = dy;
            if (vLine) {
              // align whichever moving edge/center matched: approximate by centering correction
              const targets = [origin.x, origin.x + moved.width / 2, origin.x + moved.width];
              let bestT = targets[0]!, bestD = Infinity;
              for (const tv of targets) { const dd = Math.abs(tv - vLine.at); if (dd < bestD) { bestD = dd; bestT = tv; } }
              adjX += vLine.at - bestT;
            }
            if (hLine) {
              const targets = [origin.y, origin.y + moved.height / 2, origin.y + moved.height];
              let bestT = targets[0]!, bestD = Infinity;
              for (const tv of targets) { const dd = Math.abs(tv - hLine.at); if (dd < bestD) { bestD = dd; bestT = tv; } }
              adjY += hLine.at - bestT;
            }
            setNodes((prev) => prev.map((n) => {
              const o2 = d.origins.get(n.id);
              if (!o2) return n;
              return { ...n, x: snapVal(o2.x + adjX), y: snapVal(o2.y + adjY) };
            }));
          }
          setGuides(g.length > 0 ? g : null);
        } else if (guides) {
          setGuides(null);
        }
      } else if (guides) {
        setGuides(null);
      }
    } else if (d.kind === "resize") {
      const sdx = (e.clientX - d.sx) / vpRef.current.zoom;
      const sdy = (e.clientY - d.sy) / vpRef.current.zoom;
      const o = d.orig;
      let x = o.x, y = o.y, w = o.w, h = o.h;
      const hnd = d.handle;
      const target = nodesRef.current.find((n) => n.id === d.id);
      // Box frames (rect/rounded) are free-form writing containers: the manual
      // height cap is lifted to the same ceiling the auto-grow path uses, so a
      // long-text column can also be shaped by hand. Polygons/circles keep the
      // tight 1200 cap (elongation breaks their geometry). Caps are floored at
      // the drag-start size so a manually resized frame never SNAPS BACK below
      // what auto-grow (or the user) already gave it.
      const maxDragH = Math.max(target && boxShapeExempt(target.shape) ? 20000 : 1200, o.h);
      const maxDragW = Math.max(MAX_W, o.w);
      // Convert screen-space deltas into the node's local axes when rotated.
      const rot = target?.rotation ?? 0;
      let dx = sdx, dy = sdy;
      if (rot) {
        const rad = (-rot * Math.PI) / 180;
        dx = sdx * Math.cos(rad) - sdy * Math.sin(rad);
        dy = sdx * Math.sin(rad) + sdy * Math.cos(rad);
      }
      if (d.vs) {
        // Polygon vertex drag: each non-zero axis sign acts like the matching
        // edge handle (e/w/n/s) derived from that vertex's side of the center.
        const { sx, sy } = d.vs;
        if (sx !== 0) {
          w = clamp(o.w + sx * dx, MIN_W, maxDragW);
          if (sx < 0) x = o.x + (o.w - w);
        }
        if (sy !== 0) {
          h = clamp(o.h + sy * dy, MIN_H, maxDragH);
          if (sy < 0) y = o.y + (o.h - h);
        }
        // Module-1 vertex safety lock: reject any pending geometry that has
        // degenerated into a sliver (collision damping keeps the last good
        // frame instead of collapsing the polygon into a line).
        if (target && shapeCollapsed(target.shape, w, h)) {
          w = o.w; h = o.h; x = o.x; y = o.y;
        }
      } else {
        if (hnd.includes("e")) w = clamp(o.w + dx, MIN_W, maxDragW);
        if (hnd.includes("s")) h = clamp(o.h + dy, MIN_H, maxDragH);
        if (hnd.includes("w")) { w = clamp(o.w - dx, MIN_W, maxDragW); x = o.x + (o.w - w); }
        if (hnd.includes("n")) { h = clamp(o.h - dy, MIN_H, maxDragH); y = o.y + (o.h - h); }
      }
      // Module-1 aspect-ratio guard: pull the long side back to ≤3× the short
      // one (equivalent to force-expanding the short side). Anchor
      // compensation re-glues a grabbed west/north edge to the cursor.
      // MANUAL DRAGS ONLY — box frames are exempt; text-driven autogrow never
      // passes through here (stored dims are rendered as-is).
      const cd = clampInteractive(w, h, target?.shape);
      if (cd.width !== w && (hnd.includes("w") || (d.vs?.sx ?? 0) < 0)) x = o.x + (o.w - cd.width);
      if (cd.height !== h && (hnd.includes("n") || (d.vs?.sy ?? 0) < 0)) y = o.y + (o.h - cd.height);
      w = cd.width; h = cd.height;
      if (target?.shape === "circle") {
        // circle stays a circle — only the diameter changes
        const s = Math.min(w, h);
        w = s; h = s;
        if (hnd.includes("w")) x = o.x + (o.w - s);
        if (hnd.includes("n")) y = o.y + (o.h - s);
      }
      setNodes((prev) => prev.map((n) => (n.id === d.id ? { ...n, x: snapVal(x), y: snapVal(y), width: Math.round(w), height: Math.round(h) } : n)));
    }
  }

  function onCanvasPointerUp(): void {
    const d = dragRef.current;
    dragRef.current = null;
    setDraggingNode(false);
    if (guides) setGuides(null);
    if (!d) return;
    if (d.kind === "pan") scheduleMetaSave();
    if (d.kind === "marquee") {
      const mq = marqueeRef.current;
      setMarquee(null);
      marqueeRef.current = null;
      if (mq) finishMarquee(mq);
    }
    if (d.kind === "move") scheduleNodesSave(Array.from(d.origins.keys()));
    if (d.kind === "resize") scheduleNodesSave([d.id]);
  }

  function finishMarquee(mq: { x0: number; y0: number; x1: number; y1: number }): void {
    const rx0 = Math.min(mq.x0, mq.x1), ry0 = Math.min(mq.y0, mq.y1);
    const rx1 = Math.max(mq.x0, mq.x1), ry1 = Math.max(mq.y0, mq.y1);
    if (Math.abs(rx1 - rx0) < 4 && Math.abs(ry1 - ry0) < 4) return;
    const hit = new Set<string>();
    for (const n of nodesRef.current) {
      if (n.x < rx1 && n.x + n.width > rx0 && n.y < ry1 && n.y + n.height > ry0) hit.add(n.id);
    }
    setSelection(hit);
  }

  function onNodePointerDown(e: React.PointerEvent, node: MindNode): void {
    // Click-state machine runs FIRST so triple-click can cancel an active
    // editing session (the old early-return made cancel unreachable).
    const now = Date.now();
    const tr = clickTracker.current;
    tr.count = tr.id === node.id && now - tr.time < 450 ? tr.count + 1 : 1;
    tr.time = now;
    tr.id = node.id;

    if (editingId === node.id) {
      // 编辑态的所有点击都不冒泡到画布：画布 pointerdown 会把焦点抢给
      // containerRef，从而摧毁输入框选区 —— 拖拽框选 / 复制粘贴全部失效。
      // Clicks 1-2 are caret interactions; the third exits editing without
      // touching content and never starts a drag here.
      e.stopPropagation();
      if (tr.count >= 3) {
        tr.count = 0;
        e.preventDefault();
        setEditingId(null);
        setSelection(new Set([node.id]));
        containerRef.current?.focus({ preventScroll: true });
      }
      return;
    }

    e.stopPropagation();
    containerRef.current?.focus({ preventScroll: true });

    if (tr.count >= 3) {
      // triple-click outside editing → plain deselect, never destructive
      tr.count = 0;
      setSelection(new Set());
      return;
    }

    if (connectingFrom) {
      void createEdge(connectingFrom, node.id);
      setConnectingFrom(null);
      setConnectPos(null);
      return;
    }
    if (node.locked) {
      setSelection(new Set([node.id]));
      setSelectedEdges(new Set());
      return;
    }

    // Ctrl + drag duplicates the selection and drags the copies (spec 2.2).
    if (e.ctrlKey && !e.shiftKey && !connectingFrom) {
      const srcNodes = nodesRef.current.filter((n) => (selection.has(n.id) ? n : n.id === node.id));
      if (srcNodes.length > 0) {
        pushHistory();
        const clones = srcNodes.map((n) => ({
          ...structuredClone(n),
          id: uid(),
          groupId: null,
          zIndex: nextZ(),
          updatedAt: Date.now(),
        }));
        setNodes((prev) => [...prev, ...clones]);
        void ipc.saveNodes(clones).catch(() => {});
        const cloneIds = new Set(clones.map((c) => c.id));
        setSelection(cloneIds);
        setSelectedEdges(new Set());
        const origins = new Map<string, { x: number; y: number }>();
        for (const c of clones) origins.set(c.id, { x: c.x, y: c.y });
        dragRef.current = { kind: "move", sx: e.clientX, sy: e.clientY, origins };
        setDraggingNode(true);
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        return;
      }
    }

    // Shift toggles membership; plain click selects.
    const additive = e.shiftKey;
    const nextSel = new Set<string>();
    if (additive) {
      for (const v of selection) nextSel.add(v);
      if (nextSel.has(node.id)) nextSel.delete(node.id);
      else nextSel.add(node.id);
    } else {
      nextSel.add(node.id);
    }
    setSelection(nextSel);
    setSelectedEdges(new Set());

    // Grouped frames travel together.
    const ids = expandWithGroups(nextSel);
    const origins = new Map<string, { x: number; y: number }>();
    for (const n of nodesRef.current) {
      if (ids.has(n.id) && !n.locked) origins.set(n.id, { x: n.x, y: n.y });
    }
    dragRef.current = { kind: "move", sx: e.clientX, sy: e.clientY, origins };
    setDraggingNode(true);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onNodeDoubleClick(node: MindNode): void {
    // Sub-map anchor: double-click jumps to the linked local file.
    if (node.recordId?.startsWith("file:")) {
      void openLocalMindmapFile(node.recordId.slice(5));
      return;
    }
    // 12.2：双击 .project 档案引用节点 → 切入项目分析空间并加载档案。
    if (node.recordId?.startsWith("pv-archive:")) {
      uiStore.setState({ mode: "project", pvPendingOpen: node.recordId.slice(11) });
      return;
    }
    // 项目可视化（规范 5.2）：双击 pv 节点打开通俗解读信息卡。
    if (node.recordId?.startsWith("pv:")) {
      setSelection(new Set([node.id]));
      setPvInfoId(node.id);
      return;
    }
    if (!node.locked) setEditingId(node.id);
  }

  function onResizeStart(e: React.PointerEvent, handle: string, node: MindNode): void {
    setDraggingNode(true);
    dragRef.current = {
      kind: "resize", id: node.id, handle,
      sx: e.clientX, sy: e.clientY,
      orig: { x: node.x, y: node.y, w: node.width, h: node.height },
    };
  }

  /** Deep-edit vertex handle for polygon shapes: dragging a vertex scales the
   *  frame along that vertex's axis signs (captured once at drag start). */
  function onVertexResizeStart(e: React.PointerEvent, node: MindNode, index: number): void {
    setDraggingNode(true);
    dragRef.current = {
      kind: "resize", id: node.id, handle: `v${index}`,
      sx: e.clientX, sy: e.clientY,
      orig: { x: node.x, y: node.y, w: node.width, h: node.height },
      vs: vertexDragSigns(node.shape, node.width, node.height, index),
    };
  }

  // ---------- wheel: anchored zoom with eased animation, trackpad pan ----------
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    // Normalize deltas across devices: pixel (WebView2/Chrome), line
    // (Firefox ≈ 1/16 of a notch each) and page modes all become pixels.
    const norm = (d: number, mode: number): number => {
      if (mode === 1) return d * 16;
      if (mode === 2) return d * (el.clientHeight || 600);
      return d;
    };
    const onWheel = (e: WheelEvent): void => {
      e.preventDefault();
      if (dragRef.current) return; // dragging takes priority; no viewport re-mapping
      const dx = norm(e.deltaX, e.deltaMode);
      const dy = norm(e.deltaY, e.deltaMode);
      if (e.ctrlKey) {
        zoomAt(Math.pow(1.0018, -dy), e.clientX, e.clientY);
      } else if (Math.abs(dx) > Math.abs(dy)) {
        stopAnim();
        const nv = { ...vpRef.current, x: vpRef.current.x - dx };
        vpRef.current = nv; setVp(nv);
        scheduleMetaSave();
      } else if (e.shiftKey) {
        stopAnim();
        const nv = { ...vpRef.current, x: vpRef.current.x - dy };
        vpRef.current = nv; setVp(nv);
        scheduleMetaSave();
      } else {
        zoomAt(Math.pow(1.0016, -dy), e.clientX, e.clientY);
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomAt, scheduleMetaSave, stopAnim]);

  // ---------- keyboard (WASD gated: never while typing) ----------
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent): void => {
      if (uiStore.getState().mode !== "mindmap") return;
      const ae = document.activeElement as HTMLElement | null;
      const typing = !!ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.isContentEditable);
      if (typing) return;
      // IME composition (CJK input) must never trigger canvas shortcuts.
      if (e.isComposing) return;
      const k = e.key.toLowerCase();

      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && k === "z") { e.preventDefault(); undo(); return; }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && k === "s") { e.preventDefault(); void saveToFile(); return; }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && k === "s") { e.preventDefault(); void saveMapAs(); return; }
      if ((e.ctrlKey || e.metaKey) && (k === "y" || (e.shiftKey && k === "z"))) { e.preventDefault(); redo(); return; }
      if ((e.ctrlKey || e.metaKey) && k === "c") { copySelection(); return; }
      if ((e.ctrlKey || e.metaKey) && k === "x") { copySelection(true); return; }
      if ((e.ctrlKey || e.metaKey) && k === "v") { pasteClipboard(); return; }
      if ((e.ctrlKey || e.metaKey) && k === "a") {
        // Select-all limited to the visible viewport (spec 2.3).
        e.preventDefault();
        const vr = viewRectWorld();
        setSelection(new Set(nodes.filter((n) => !n.hidden && boxIntersectsRect(n, vr)).map((n) => n.id)));
        return;
      }
      if ((e.ctrlKey || e.metaKey) && k === "f") { e.preventDefault(); setQuickFind(true); return; }
      if ((e.ctrlKey || e.metaKey) && k === "d") { e.preventDefault(); pasteClipboard(); return; }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && k === "g" && selection.size > 1) {
        e.preventDefault();
        const gid = uid();
        patchMany(selection, { groupId: gid });
        pushToast("info", lang === "zh" ? `已编组（${selection.size}）` : `Grouped (${selection.size})`);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && k === "g" && selection.size > 0) {
        e.preventDefault();
        patchMany(selection, { groupId: null });
        pushToast("info", lang === "zh" ? "已解组" : "Ungrouped");
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && k === "c") {
        const firstSel = selectedArr[0];
        if (firstSel) {
          styleClipboard.current = {
            shape: firstSel.shape, borderRadius: firstSel.borderRadius,
            borderColor: firstSel.borderColor, fillColor: firstSel.fillColor,
            fontSize: firstSel.fontSize, preset: firstSel.preset, opacity: firstSel.opacity,
          };
          pushToast("info", t("styleCopied"));
        }
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && k === "v") {
        const st = styleClipboard.current;
        if (st && selection.size > 0) { patchMany(selection, st); pushToast("success", t("styleApplied")); }
        return;
      }

      if (k === " ") { e.preventDefault(); spaceRef.current = true; return; }
      if (k === "delete" || k === "backspace") {
        if (selectedEdges.size > 0) { e.preventDefault(); void deleteSelectedEdges(); return; }
        if (selection.size > 0) { e.preventDefault(); void deleteSelectionBubble(); }
        return;
      }
      if (k === `escape`) {
        if (freeTransform.size > 0) { setFreeTransform(new Set()); return; }
        if (quickFind) { setQuickFind(false); return; }
        if (menuAnchor) { setMenuAnchor(null); return; }
        if (edgePop) { setEdgePop(null); return; }
        if (connectingFrom) { setConnectingFrom(null); setConnectPos(null); return; }
        if (editingId) { cancelTextEdit(); return; }
        setSelection(new Set());
        setSelectedEdges(new Set());
        return;
      }
      if (k === "enter" && selection.size > 0 && !editingId) {
        const firstSel = Array.from(selection)[0];
        if (firstSel !== undefined) { const nn = nodes.find((x) => x.id === firstSel); if (nn && !nn.locked) { e.preventDefault(); setEditingId(firstSel); } }
        return;
      }
      if (k === "tab" && nodes.length > 0) {
        // Cycle focus through nodes in creation (load) order.
        e.preventDefault();
        const idx = nodes.findIndex((n) => selection.has(n.id));
        const nextIdx = e.shiftKey
          ? (idx <= 0 ? nodes.length - 1 : idx - 1)
          : (idx === -1 || idx === nodes.length - 1 ? 0 : idx + 1);
        const target = nodes[nextIdx];
        if (target) setSelection(new Set([target.id]));
        return;
      }
      if (k === "[" || k === "]") {
        if (selection.size > 0) {
          e.preventDefault();
          const delta = k === "]" ? 1 : -1;
          for (const id of selection) {
            const n = nodeById.get(id);
            if (n) patchNode(id, { zIndex: n.zIndex + delta });
          }
        }
        return;
      }
      if (k === "n" && !editingId) {
        e.preventDefault();
        const r = containerRef.current?.getBoundingClientRect();
        const c = toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 600) / 2);
        createNodeAt(c.x - 115, c.y - 36);
        return;
      }
      if (k === "f") { fitAll(e.shiftKey); return; }
      if (k === "home") { homeCenter(); return; }
      if (e.code === "Numpad0") { e.preventDefault(); resetViewport(); return; }
      if (k === "g" && !e.ctrlKey && !e.metaKey) { toggleGrid(); return; }
      if (k === "=" || k === "+") { zoomAt(1.15); return; }
      if (k === "-") { zoomAt(1 / 1.15); return; }

      // Arrow keys nudge the SELECTION when present (Shift = big step);
      // otherwise they pan the canvas like WASD.
      const arrowDx = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
      const arrowDy = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
      if (arrowDx !== 0 || arrowDy !== 0) {
        e.preventDefault();
        if (selection.size > 0 && !editingId) {
          const step = e.shiftKey ? SNAP_GRID * 5 : SNAP_GRID;
          moveSelectionBy(arrowDx * step, arrowDy * step);
        } else {
          // Key itself already tracked by the capture rail; just start physics.
          ensureNavLoop();
        }
        return;
      }
      if (!editingId && ["w", "a", "s", "d"].includes(k)) {
        ensureNavLoop(); // capture rail owns key state
        return;
      }
    };
    const onKeyUp = (e: KeyboardEvent): void => {
      if (e.key === " ") spaceRef.current = false;
      // Movement keys/modifiers are cleared exclusively by the capture rail.
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [undo, redo, selection, selectedEdges, editingId, nodes, edges, quickFind, menuAnchor, edgePop, connectingFrom, props.settings]);

  // Leaving mindmap mode (or unmounting) must drop every held navigation key,
  // otherwise stale WASD state resurrects an unstoppable camera drift on return.
  useEffect(() => {
    const resetNav = (): void => {
      if (uiStore.getState().mode !== "mindmap") {
        keysRef.current.clear();
        velRef.current = { vx: 0, vy: 0 };
        spaceRef.current = false;
      }
    };
    resetNav();
    const unsub = uiStore.subscribe(resetNav);
    return (): void => {
      unsub();
      keysRef.current.clear();
      velRef.current = { vx: 0, vy: 0 };
      spaceRef.current = false;
    };
  }, []);

  function toggleGrid(): void {
    const m = mapRef.current;
    if (!m) return;
    const next = !m.gridEnabled;
    setMap({ ...m, gridEnabled: next });
    void ipc.updateMindmap({ id: m.id, gridEnabled: next }).catch(() => {});
  }

  function toggleSnap(): void {
    const m = mapRef.current;
    if (!m) return;
    const next = !m.snapEnabled;
    setMap({ ...m, snapEnabled: next });
    void ipc.updateMindmap({ id: m.id, snapEnabled: next }).catch(() => {});
  }

  // ---------- cross-component events & text commit ----------
  function commitTextEdit(id: string, rawHtml: string): void {
    const html = sanitizeHtml(rawHtml.trim());
    const n = nodesRef.current.find((x) => x.id === id);
    if (!n) return;
    if (html !== n.textHtml) {
      pushHistory();
      patchNode(id, { textHtml: html });
    }
    setEditingId(null);
    // Hand keyboard focus back to the canvas so WASD resumes instantly.
    containerRef.current?.focus({ preventScroll: true });
  }

  function cancelTextEdit(): void {
    setEditingId(null);
    containerRef.current?.focus({ preventScroll: true });
  }

  useEffect(() => {
    const onEdgeSelect = (ev: Event): void => {
      const d = (ev as CustomEvent<{ id: string; additive: boolean }>).detail;
      setSelectedEdges((prev) => {
        const next = new Set(d.additive ? prev : []);
        if (d.additive && next.has(d.id)) next.delete(d.id);
        else next.add(d.id);
        return next;
      });
      setSelection(new Set());
      setEditingId(null);
    };
    const onEdgeMenu = (ev: Event): void => {
      const d = (ev as CustomEvent<{ id: string; x: number; y: number }>).detail;
      setSelectedEdges(new Set([d.id]));
      // Pinned to the client-area top-left origin (contract-5); the event's
      // cursor coordinates are intentionally ignored.
      setEdgePop({ id: d.id });
    };
    const onCreateFromDoc = (ev: Event): void => {
      const d = (ev as CustomEvent<{ docId: string; title: string }>).detail;
      if (!mapRef.current) return;
      const r = containerRef.current?.getBoundingClientRect();
      const c = toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 500) / 2);
      const n = createNodeAt(c.x - 115, c.y - 30, `<strong>${escapeHtml(d.title)}</strong>`);
      patchNode(n.id, { recordId: d.docId });
    };
    const onOpenRecord = (ev: Event): void => {
      const detail = (ev as CustomEvent<string>).detail;
      if (detail.startsWith("file:")) {
        void openLocalMindmapFile(detail.slice(5));
        return;
      }
      uiStore.setState({ currentDocId: detail, mode: "write" });
    };
    // ---- local workspace file integration (spec II-3) ----
    const onImportNodes = (ev: Event): void => {
      const d = (ev as CustomEvent<{ nodes: unknown[]; edges: unknown[] }>).detail;
      importNodesIntoCanvas(d);
    };
    const onImportAnchor = (ev: Event): void => {
      const d = (ev as CustomEvent<{ path: string; clientX?: number; clientY?: number }>).detail;
      createAnchorNode(d.path, d.clientX, d.clientY);
    };
    const onOpenFile = (ev: Event): void => {
      const d = (ev as CustomEvent<{ path: string }>).detail;
      // .fatetree 档案属于命运推演空间：路由过去再完整载入（不是导图文件）。
      if (/\.fatetree$/i.test(d.path)) {
        uiStore.setState({ mode: "fate" });
        window.setTimeout(() => window.dispatchEvent(new CustomEvent("variable:fate-open-file", { detail: d })), 80);
        return;
      }
      void openLocalMindmapFile(d.path);
    };
    const onAutogrow = (ev: Event): void => {
      const d = (ev as CustomEvent<{ id: string; textWidth: number; textHeight: number }>).detail;
      const n = nodesRef.current.find((x) => x.id === d.id);
      if (!n) return;
      // Module-2 text-first sizing + module-3 reverse dilation: polygons grow
      // proportionally until their centroid-inscribed rectangle fits the
      // measured text plus a TEXT_PAD margin on every side — text can never
      // touch a slanted edge. allowShrink: 稳定实测远超内容需要时回缩贴合
      // （自由变形/锁定的节点除外 —— 它们的尺寸是用户明确意志）。
      const mayShrink = !n.locked && !n.collapsed && !freeTransformRef.current.has(n.id);
      let { width, height } = growDimsForText(n.shape, n.width, n.height, d.textWidth, d.textHeight, mayShrink);
      const isBox = n.shape === "rect" || n.shape === "rounded" || n.shape === "circle";
      if (isBox) {
        // 增长式自适应（编辑/静态态统一）：“先横后纵” —— 宽度向文本自然需求
        // 扩展（首选 280 列宽），高度随换行行数增长。超长文（面积在 280 列宽
        // 下超过 MAX_AUTO_H）按面积守恒反推加宽列宽（至 MAX_TEXT_W），让
        // ≈5 万字整体容纳在 ≤20000px 框内；超出绝对上限的部分由框内右侧
        // 滚动条兜底。宽度只增不减（手动加宽是用户意志）；高度在远超内容
        // 需要（>1.5×，滞回）时一步回缩贴合，消除过渡期测量的永久性推过头。
        const colWNow = Math.max(60, n.width - 30); // 当前文本列宽（去内边距）
        const area = colWNow * d.textHeight;
        const fitColW = d.textHeight > 0 ? Math.ceil(area / (MAX_AUTO_H * 0.97)) : 0;
        const adaptiveW = Math.min(Math.max(fitColW, PREFERRED_TEXT_W), MAX_TEXT_W);
        const estH = adaptiveW > 0 ? area / adaptiveW : d.textHeight;
        // d.textWidth 已被钳在首选列宽 —— 超长文时以加宽后的列宽为准
        const targetW = fitColW > PREFERRED_TEXT_W
          ? adaptiveW + 30
          : Math.min(d.textWidth + 30, PREFERRED_TEXT_W + 30);
        width = clamp(Math.max(n.width, targetW), MIN_W, MAX_TEXT_W + 30);
        const fitH = Math.min(estH + 26, MAX_AUTO_H);
        height = mayShrink && n.height > fitH * 1.5
          ? clamp(fitH * 1.18, MIN_H, 20000)
          : clamp(Math.max(n.height, fitH), MIN_H, 20000);
        if (n.shape === "circle") {
          const dia = Math.max(width, height);
          width = dia; height = dia;
        }
      }
      if (Math.abs(n.height - height) > 1 || Math.abs(n.width - width) > 1) {
        setNodes((prev) => prev.map((x) =>
          x.id === d.id ? { ...x, width: Math.round(width), height: Math.round(height) } : x));
        scheduleNodesSave([d.id]);
      }
    };
    const onRepairNode = (ev: Event): void => {
      const d = (ev as CustomEvent<{ id: string; width: number; height: number }>).detail;
      const cd = clampDims(d.width, d.height);
      patchNode(d.id, cd);
      console.warn(`[mindmap] repaired corrupt dims of node ${d.id} → ${cd.width}x${cd.height}`);
      void ipc.log("warn", `repaired node dims ${d.id} -> ${cd.width}x${cd.height}`).catch(() => {});
    };
    const onCommitTextEv = (ev: Event): void => {
      const d = (ev as CustomEvent<{ id: string; html: string }>).detail;
      commitTextEdit(d.id, d.html);
    };
    // Global search → locate & center a node (works even if the map is still loading).
    const onFocusNode = (ev: Event): void => {
      const id = (ev as CustomEvent<string>).detail;
      if (!attemptFocusNode(id, nodesRef.current)) pendingFocusRef.current = id;
    };
    window.addEventListener("variable:mm-edge-select", onEdgeSelect);
    window.addEventListener("variable:mm-edge-menu", onEdgeMenu);
    window.addEventListener("variable:create-node-from-doc", onCreateFromDoc);
    window.addEventListener("variable:mm-open-record", onOpenRecord);
    window.addEventListener("variable:mm-autogrow", onAutogrow);
    window.addEventListener("variable:mm-repair-node", onRepairNode);
    window.addEventListener("variable:mm-commit-text", onCommitTextEv);
    window.addEventListener("variable:mm-focus-node", onFocusNode);
    window.addEventListener("variable:mm-import-nodes", onImportNodes);
    window.addEventListener("variable:mm-import-anchor", onImportAnchor);
    window.addEventListener("variable:mm-open-file", onOpenFile);
    return () => {
      window.removeEventListener("variable:mm-edge-select", onEdgeSelect);
      window.removeEventListener("variable:mm-edge-menu", onEdgeMenu);
      window.removeEventListener("variable:create-node-from-doc", onCreateFromDoc);
      window.removeEventListener("variable:mm-open-record", onOpenRecord);
      window.removeEventListener("variable:mm-autogrow", onAutogrow);
      window.removeEventListener("variable:mm-repair-node", onRepairNode);
      window.removeEventListener("variable:mm-commit-text", onCommitTextEv);
      window.removeEventListener("variable:mm-focus-node", onFocusNode);
      window.removeEventListener("variable:mm-import-nodes", onImportNodes);
      window.removeEventListener("variable:mm-import-anchor", onImportAnchor);
      window.removeEventListener("variable:mm-open-file", onOpenFile);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Select a node and center the viewport on it. Returns false when not found yet. */
  function attemptFocusNode(id: string, list: MindNode[]): boolean {
    const n = list.find((x) => x.id === id);
    if (!n) return false;
    setSelection(new Set([id]));
    setSelectedEdges(new Set());
    setEditingId(null);
    centerOnWorld(n.x + n.width / 2, n.y + n.height / 2);
    return true;
  }

  /** Convert #rrggbb + alpha to rgba() so the opacity slider works. */
  function gridColorCss(): string {
    const hex = props.settings.mindDefaults.gridColor;
    const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return hex;
    const int = parseInt(m[1]!, 16);
    const a = clamp(props.settings.mindDefaults.gridOpacity, 0, 1);
    return `rgba(${(int >> 16) & 255}, ${(int >> 8) & 255}, ${int & 255}, ${a})`;
  }

  /** World-space grid cell that keeps ~40px on-screen spacing at any zoom. */
  function gridSizeWorld(): number {
    const z = vp.zoom;
    const steps = [5, 10, 20, 40, 80, 160, 320, 640];
    for (const s of steps) {
      if (s * z >= 26) return s;
    }
    return steps[steps.length - 1]!;
  }

  function openNodeContextMenu(e: React.MouseEvent, node: MindNode): void {
    e.preventDefault();
    e.stopPropagation();
    setSelection(new Set([node.id]));
    setSelectedEdges(new Set());
    lastMousePos.current = { x: e.clientX, y: e.clientY };
    openContextMenu(e.clientX, e.clientY, nodeMenuItems(node));
  }

  function nodeMenuItems(node: MindNode): import("../../components/ContextMenu").MenuItem[] {
    return [
      { label: t("edit"), disabled: node.locked, onClick: () => setEditingId(node.id) },
      ...(node.recordId?.startsWith("pv:")
        ? [{ label: lang === "zh" ? "通俗解读此文件" : "Explain this file", onClick: () => setPvInfoId(node.id) }]
        : []),
      { label: t("copyNode"), onClick: () => { graphClipboard.current = { nodes: [structuredClone(node)], edges: [] }; pushToast("info", t("copied")); } },
      { label: lang === "zh" ? "复制样式" : "Copy style", onClick: () => { styleClipboard.current = { shape: node.shape, borderRadius: node.borderRadius, borderColor: node.borderColor, fillColor: node.fillColor, fontSize: node.fontSize, preset: node.preset, opacity: node.opacity }; pushToast("info", t("styleCopied")); } },
      {
        label: lang === "zh" ? "粘贴样式" : "Paste style",
        disabled: !styleClipboard.current,
        onClick: () => { if (styleClipboard.current) patchNode(node.id, styleClipboard.current); },
      },
      { separator: true },
      { label: node.locked ? t("unlock") : t("lock"), onClick: () => patchNode(node.id, { locked: !node.locked }) },
      { label: node.hidden ? (lang === "zh" ? "取消隐藏" : "Unhide") : (lang === "zh" ? "隐藏" : "Hide"), onClick: () => patchNode(node.id, { hidden: !node.hidden }) },
      { label: node.collapsed ? t("expand") : t("collapse"), onClick: () => patchNode(node.id, { collapsed: !node.collapsed }) },
      {
        label: lang === "zh" ? "自由变形" : "Free transform",
        checked: freeTransform.has(node.id),
        onClick: () =>
          setFreeTransform((prev) => {
            const nx = new Set(prev);
            if (nx.has(node.id)) nx.delete(node.id);
            else nx.add(node.id);
            return nx;
          }),
      },
      { separator: true },
      { label: lang === "zh" ? "置顶" : "Bring to front", onClick: () => patchNode(node.id, { zIndex: nodes.reduce((m, n) => Math.max(m, n.zIndex), 0) + 1 }) },
      { label: lang === "zh" ? "置底" : "Send to back", onClick: () => patchNode(node.id, { zIndex: nodes.reduce((m, n) => Math.min(m, n.zIndex), 0) - 1 }) },
      { separator: true },
      { label: t("connect"), onClick: () => { setConnectingFrom(node.id); setSelection(new Set([node.id])); } },
      ...(styleClipboard.current
        ? []
        : []),
      { label: lang === "zh" ? "风格面板…" : "Style panel…", onClick: () => setMenuAnchor({ id: node.id }) },
      { separator: true },
      { label: t("deleteNode"), danger: true, onClick: () => void deleteSelectionBubble(lastMousePos.current.x, lastMousePos.current.y) },
    ];
  }


  // ---------- drag & drop media onto canvas ----------
  useEffect(() => {
    let disposed = false;
    const p = getCurrentWebview().onDragDropEvent(async (event) => {
      try {
        if (event.payload.type !== "drop") return;
        if (uiStore.getState().mode !== "mindmap" || !mapRef.current) return;
        const paths: string[] = [...(event.payload.paths ?? [])];
        if (paths.length === 0) return;
        const scale = await getCurrentWindow().scaleFactor().catch(() => 1);
        const cx = event.payload.position.x / scale;
        const cy = event.payload.position.y / scale;
        const r = containerRef.current?.getBoundingClientRect();
        if (!r || cx < r.left || cx > r.right || cy < r.top || cy > r.bottom) return;
        const w = toWorld(cx, cy);
        // 项目导入（规范 2.1）：拖入的是文件夹 → 走项目可视化引擎。
        const kinds = await ipc.checkPaths(paths).catch(() => []);
        const dirSet = new Set<string>();
        kinds.forEach((k, i) => {
          if (k && k.kind === "dir") dirSet.add(paths[i] ?? "");
        });
        const dirs = paths.filter((p) => dirSet.has(p));
        if (dirs.length > 0) {
          void importProjectFolder(dirs[0]!, { x: w.x, y: w.y });
          if (dirs.length === paths.length) return;
        }
        const nonDir = paths.filter((p) => !dirSet.has(p));
        let offsetY = 0;
        // .project 档案（12.2）：拖到导图画布 → 樱花粉档案引用节点。
        const projectFiles = nonDir.filter((p) => /\.project$/i.test(p));
        for (const pf of projectFiles.slice(0, 4)) {
          createAnchorNode(pf, cx, cy + offsetY);
          offsetY += 90;
        }
        if (projectFiles.length > 0) {
          pushToast("success", lang === "zh" ? "已创建项目档案引用节点" : "Project archive refs created");
        }
        // Spec II-3 consistency: a local map file dropped straight onto the
        // canvas becomes a jump-anchor node (same as dragging it from the
        // workspace panel) — never a useless paperclip attachment.
        const mapFiles = nonDir.filter((p) => /\.(mindmap|json)$/i.test(p));
        for (const path of mapFiles.slice(0, 6)) {
          createAnchorNode(path, cx, cy + offsetY);
          offsetY += 90;
        }
        const mediaPaths = nonDir.filter((p) => !/\.(mindmap|json)$/i.test(p));
        for (const path of mediaPaths.slice(0, 6)) {
          const lower = path.toLowerCase();
          const isImg = /\.(png|jpe?g|webp|gif|bmp|avif)$/.test(lower);
          const isVid = /\.(mp4|webm|ogv|mov|m4v|mkv)$/.test(lower);
          const atts = await ipc.importMedia({ paths: [path], mode: "copy" });
          const att = atts[0];
          if (!att) continue;
          const url = convertFileSrc(att.absPath);
          const html = isImg ? `<img src="${url}" />` : isVid ? `<video src="${url}" controls></video>` : `<p>📎 ${escapeHtml(att.displayName)}</p>`;
          pushHistory();
          const node = makeNode(w.x - 110, w.y - 30 + offsetY, html);
          setNodes((prev) => [...prev, node]);
          void ipc.saveNodes([node]).catch(() => {});
          offsetY += 90;
        }
        if (mapFiles.length > 0) {
          pushToast("success", lang === "zh" ? "已创建子导图锚点" : "Sub-map anchor created");
        }
        if (mediaPaths.length > 0) {
          pushToast("success", lang === "zh" ? "已插入媒体" : "Media inserted");
        }
      } catch (e) {
        pushToast("error", lang === "zh" ? "插入媒体失败" : "Insert media failed", errMessage(e).message);
      }
    });
    void p.then((u) => {
      if (disposed) u();
      else un = u;
    }).catch(() => {});
    let un: (() => void) | undefined;
    return () => {
      disposed = true;
      un?.();
    };
  }, [toWorld]);

  function escapeHtml(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // ---------- batch selection ops ----------
  const nodeById = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const selectedArr = useMemo(() => Array.from(selection).map((id) => nodeById.get(id)).filter((n): n is MindNode => !!n), [selection, nodeById]);

  function alignSel(dir: "l" | "cx" | "r" | "t" | "cy" | "b"): void {
    if (selectedArr.length < 2) return;
    pushHistory();
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const n of selectedArr) {
      minX = Math.min(minX, n.x); maxX = Math.max(maxX, n.x + n.width);
      minY = Math.min(minY, n.y); maxY = Math.max(maxY, n.y + n.height);
    }
    setNodes((prev) => prev.map((n) => {
      if (!selection.has(n.id)) return n;
      switch (dir) {
        case "l": return { ...n, x: minX };
        case "r": return { ...n, x: maxX - n.width };
        case "t": return { ...n, y: minY };
        case "b": return { ...n, y: maxY - n.height };
        case "cx": return { ...n, x: (minX + maxX - n.width) / 2 };
        case "cy": return { ...n, y: (minY + maxY - n.height) / 2 };
      }
    }));
    scheduleNodesSave(Array.from(selection));
  }

  function distributeSel(axis: "h" | "v"): void {
    if (selectedArr.length < 3) return;
    pushHistory();
    const arr = [...selectedArr].sort((a, b) => (axis === "h" ? a.x - b.x : a.y - b.y));
    const first = arr[0]!;
    const last = arr[arr.length - 1]!;
    const spanStart = axis === "h" ? first.x : first.y;
    const spanEnd = axis === "h" ? last.x : last.y;
    const totalSize = arr.reduce((s, n) => s + (axis === "h" ? n.width : n.height), 0);
    const gap = (spanEnd - spanStart - totalSize) / (arr.length - 1);
    let cur = spanStart;
    const posMap = new Map<string, number>();
    for (const n of arr) {
      posMap.set(n.id, cur);
      cur += (axis === "h" ? n.width : n.height) + gap;
    }
    setNodes((prev) => prev.map((n) => {
      const p = posMap.get(n.id);
      if (p === undefined) return n;
      return axis === "h" ? { ...n, x: snapVal(p) } : { ...n, y: snapVal(p) };
    }));
    scheduleNodesSave(arr.map((n) => n.id));
  }

  function autoLayout(mode: "tree" | "grid" | "circle" | "force" = "tree"): void {
    const visible = nodes.filter((n) => !n.hidden);
    if (visible.length === 0) return;
    pushHistory();
    const updated = new Set<string>();
    const applyPos = (fn: (n: MindNode, i: number) => { x: number; y: number }): void => {
      setNodes((prev) => prev.map((n) => {
        if (n.hidden) return n;
        const i = visible.findIndex((v) => v.id === n.id);
        const p = fn(n, i);
        updated.add(n.id);
        return { ...n, x: snapVal(p.x), y: snapVal(p.y) };
      }));
      scheduleNodesSave(Array.from(updated));
    };

    if (mode === "tree") {
      // BFS from roots (no incoming edges) over undirected adjacency.
      const incoming = new Map<string, number>();
      const adj = new Map<string, string[]>();
      for (const n of visible) { incoming.set(n.id, 0); adj.set(n.id, []); }
      for (const e of edges) {
        if (!incoming.has(e.targetNodeId)) continue;
        incoming.set(e.targetNodeId, (incoming.get(e.targetNodeId) ?? 0) + 1);
        adj.get(e.sourceNodeId)?.push(e.targetNodeId);
        adj.get(e.targetNodeId)?.push(e.sourceNodeId);
      }
      const roots = visible.filter((n) => (incoming.get(n.id) ?? 0) === 0).map((n) => n.id);
      const fallbackRoot = visible[0]?.id;
      const queue = roots.length > 0 ? [...roots] : fallbackRoot !== undefined ? [fallbackRoot] : [];
      const level = new Map<string, number>(queue.map((q) => [q, 0]));
      const visited = new Set<string>(queue);
      while (queue.length > 0) {
        const id = queue.shift();
        for (const nb of adj.get(id ?? "") ?? []) {
          if (!visited.has(nb)) {
            visited.add(nb);
            level.set(nb, (level.get(id ?? "") ?? 0) + 1);
            queue.push(nb);
          }
        }
      }
      const byLevel = new Map<number, string[]>();
      for (const n of visible) {
        const lv = level.get(n.id) ?? 0;
        byLevel.get(lv)?.push(n.id) ?? byLevel.set(lv, [n.id]);
      }
      applyPos((n) => {
        const lv = level.get(n.id) ?? 0;
        const row = byLevel.get(lv) ?? [];
        const idxInRow = Math.max(0, row.indexOf(n.id));
        const rowCount = row.length;
        return { x: -(rowCount * 280) / 2 + idxInRow * 280, y: lv * 170 - 100 };
      });
      return;
    }

    if (mode === "grid") {
      const cols = Math.ceil(Math.sqrt(visible.length));
      applyPos((_n, i) => ({
        x: ((i % cols) - (cols - 1) / 2) * 300,
        y: (Math.floor(i / cols) - Math.floor((visible.length - 1) / cols) / 2) * 180,
      }));
      return;
    }

    if (mode === "circle") {
      const R = Math.max(220, (visible.length * 90) / (2 * Math.PI));
      applyPos((_n, i) => {
        const a = (i / visible.length) * Math.PI * 2 - Math.PI / 2;
        return { x: R * Math.cos(a), y: R * Math.sin(a) };
      });
      return;
    }

    // force-directed: simple repulsion + edge attraction iterations
    const pos = new Map<string, { x: number; y: number }>(
      visible.map((n, i) => [n.id, { x: n.x || Math.cos(i) * 260, y: n.y || Math.sin(i) * 260 }]),
    );
    const ids = visible.map((n) => n.id);
    for (let iter = 0; iter < 60; iter++) {
      for (let i = 0; i < ids.length; i++) {
        for (let j = i + 1; j < ids.length; j++) {
          const a = pos.get(ids[i]!)!;
          const b = pos.get(ids[j]!)!;
          let dx = a.x - b.x, dy = a.y - b.y;
          let d2 = dx * dx + dy * dy;
          if (d2 < 1) { dx = (Math.random() - 0.5); dy = (Math.random() - 0.5); d2 = 1; }
          const f = 52000 / d2;
          const d = Math.sqrt(d2);
          a.x += (dx / d) * f; a.y += (dy / d) * f;
          b.x -= (dx / d) * f; b.y -= (dy / d) * f;
        }
      }
      for (const e of edges) {
        const a = pos.get(e.sourceNodeId);
        const b = pos.get(e.targetNodeId);
        if (!a || !b) continue;
        const dx = b.x - a.x, dy = b.y - a.y;
        const d = Math.max(1, Math.hypot(dx, dy));
        const f = (d - 240) * 0.02;
        a.x += (dx / d) * f; a.y += (dy / d) * f;
        b.x -= (dx / d) * f; b.y -= (dy / d) * f;
      }
    }
    applyPos((n) => ({ x: pos.get(n.id)?.x ?? n.x, y: pos.get(n.id)?.y ?? n.y }));
  }

  /** Re-route every visible edge as orthogonal circuit-style wiring. */
  function tidyEdges(): void {
    pushHistory();
    setEdges((prev) => prev.map((e) => (e.pathStyle === "ortho" ? e : { ...e, pathStyle: "ortho" })));
    for (const e of edgesRef.current) {
      if (e.pathStyle !== "ortho") void ipc.saveEdge({ ...e, pathStyle: "ortho" }).catch(() => {});
    }
  }

  /** Built-in starter templates placed near a world anchor. */
  function insertTemplate(kind: "mindmap" | "flow" | "swot" | "kanban", at: { x: number; y: number }): void {
    pushHistory();
    const created: MindNode[] = [];
    const mk = (dx: number, dy: number, html: string, preset = "", shape: MindNode["shape"] = props.settings.mindDefaults.defaultShape): MindNode => {
      const n = makeNode(at.x + dx, at.y + dy, html);
      n.preset = preset;
      n.shape = shape;
      created.push(n);
      return n;
    };
    const H = (s: string): string => `<strong>${escapeHtml(s)}</strong>`;
    const links: [MindNode, MindNode][] = [];
    if (kind === "mindmap") {
      const c = mk(-110, -30, H(lang === "zh" ? "中心主题" : "Central topic"));
      for (let i = 0; i < 4; i++) {
        const a = (i / 4) * Math.PI * 2;
        const child = mk(Math.cos(a) * 320 - 110, Math.sin(a) * 190 - 30, lang === "zh" ? `分支 ${i + 1}` : `Branch ${i + 1}`);
        links.push([c, child]);
      }
    } else if (kind === "flow") {
      let prev: MindNode | null = null;
      const steps = lang === "zh"
        ? ["开始", "处理 A", "处理 B", "评审", "完成"]
        : ["Start", "Process A", "Process B", "Review", "Done"];
      steps.forEach((s, i) => {
        const n = mk(i * 280 - 110, -30, H(s), "", i === 0 || i === steps.length - 1 ? "rounded" : "rect");
        if (prev) links.push([prev, n]);
        prev = n;
      });
    } else if (kind === "swot") {
      const cells = lang === "zh"
        ? [["优势 S", "劣势 W"], ["机会 O", "威胁 T"]]
        : [["Strengths", "Weaknesses"], ["Opportunities", "Threats"]];
      cells.forEach((row, ry) =>
        row.forEach((label, rx) => {
          mk(rx * 300 - 150 + 40, ry * 200 - 100 + 40, `<h3>${H(label)}</h3><p></p>`, "modern");
        }),
      );
    } else {
      const cols = lang === "zh" ? ["待办", "进行中", "已完成"] : ["To do", "Doing", "Done"];
      cols.forEach((c, i) => {
        mk(i * 300 - 300, -140, H(c), "modern");
        mk(i * 300 - 300, -50, lang === "zh" ? "卡片…" : "Card…", "modern");
      });
    }
    setNodes((prev) => [...prev, ...created]);
    void ipc.saveNodes(created).catch((e) => pushToast("error", lang === "zh" ? "模板插入失败" : "Insert template failed", errMessage(e).message));
    for (const [a, b] of links) void createEdge(a.id, b.id);
  }

  function gridModeLabel(): string {
    const m = props.settings.mindDefaults.gridMode;
    return m === "dot" ? (lang === "zh" ? "点阵" : "Dots")
      : m === "iso" ? (lang === "zh" ? "等距" : "Isometric")
      : m === "none" ? (lang === "zh" ? "无" : "None")
      : (lang === "zh" ? "方格" : "Grid");
  }

  function cycleGridMode(): void {
    const order: import("../../lib/settings").GridMode[] = ["grid", "dot", "iso", "none"];
    const cur = props.settings.mindDefaults.gridMode;
    const nextM = order[(order.indexOf(cur) + 1) % order.length];
    patchSettingsMind({ gridMode: nextM });
  }

  function patchSettingsMind(patch: Partial<Settings["mindDefaults"]>): void {
    window.dispatchEvent(new CustomEvent("variable:mind-defaults-patch", { detail: patch }));
  }


  function uniform(what: "size" | "shape" | "color" | "font" | "border"): void {
    const first = selectedArr[0];
    if (!first || selectedArr.length < 2) return;
    switch (what) {
      case "size": patchMany(selection, { width: first.width, height: first.shape === "circle" ? first.width : first.height }); break;
      case "shape": {
        if (first.shape === "circle") {
          const d = Math.min(first.width, first.height);
          patchMany(selection, { shape: "circle", width: d, height: d });
        } else {
          patchMany(selection, { shape: first.shape });
        }
        break;
      }
      case "color": patchMany(selection, { fillColor: first.fillColor }); break;
      case "font": patchMany(selection, { fontSize: first.fontSize }); break;
      case "border": patchMany(selection, { borderColor: first.borderColor, borderRadius: first.borderRadius }); break;
    }
  }

  function chainConnect(): void {
    if (selectedArr.length < 2) return;
    const sorted = [...selectedArr].sort((a, b) => a.x + a.y - (b.x + b.y));
    (async () => {
      for (let i = 0; i + 1 < sorted.length; i++) {
        const a = sorted[i]!;
        const b = sorted[i + 1]!;
        await createEdge(a.id, b.id);
      }
    })();
  }

  async function exportSelected(): Promise<void> {
    if (selectedArr.length === 0) return;
    const path = await saveDialog({
      defaultPath: "variable-selection.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof path !== "string") return;
    const ids = new Set(selectedArr.map((n) => n.id));
    const payload = {
      app: "variable-mindmap-selection",
      formatVersion: 1,
      exportedAt: Date.now(),
      nodes: selectedArr,
      edges: edges.filter((e) => ids.has(e.sourceNodeId) && ids.has(e.targetNodeId)),
    };
    try {
      await ipc.saveTextFile(path, JSON.stringify(payload, null, 2), true);
      pushToast("success", t("exportedOk"), path);
    } catch (e) {
      pushToast("error", lang === "zh" ? "导出失败" : "Export failed", errMessage(e).message);
    }
  }

  async function createRecordFromNode(nodeId: string): Promise<void> {
    const n = nodeById.get(nodeId);
    if (!n || n.recordId) return;
    try {
      const doc = await ipc.createDocument(null, stripTags(n.textHtml).slice(0, 60) || undefined);
      patchNode(nodeId, { recordId: doc.id });
      pushToast("success", t("createRecordFromNode"));
    } catch (e) {
      pushToast("error", "Create record failed", errMessage(e).message);
    }
  }

  function stripTags(html: string): string {
    const d = new DOMParser().parseFromString(html, "text/html");
    return (d.body.textContent ?? "").trim();
  }

  // ---------- local file save / open (.mindmap / .json, spec II-1) ----------
  function buildLocalPayload(): string {
    const m = mapRef.current;
    return JSON.stringify({
      app: "variable-mindmap",
      formatVersion: 1,
      name: m?.name ?? "untitled",
      exportedAt: Date.now(),
      viewport: { x: vpRef.current.x, y: vpRef.current.y, zoom: vpRef.current.zoom },
      nodes: nodesRef.current,
      edges: edgesRef.current,
    }, null, 2);
  }

  function safeFileName(name: string): string {
    return (name || "untitled").replace(/[\\/:*?"<>|]/g, "_").slice(0, 60);
  }

  /** Save to the linked file (falls back to Save-As when there is none). */
  async function saveToFile(): Promise<void> {
    const mid = mapRef.current?.id;
    const cur = mid ? linkedFilesRef.current[mid] : undefined;
    if (!cur) { await saveMapAs(); return; }
    try {
      await ipc.saveTextFile(cur, buildLocalPayload(), true);
      pushToast("success", lang === "zh" ? "已保存到文件" : "Saved to file", cur);
    } catch (e) {
      pushToast("error", lang === "zh" ? "保存失败" : "Save failed", errMessage(e).message);
    }
  }

  /** Native Save-As dialog: user picks path / name / format (.mindmap|.json). */
  async function saveMapAs(): Promise<void> {
    const m = mapRef.current;
    if (!m) return;
    const p = await saveDialog({
      defaultPath: `${safeFileName(m.name)}.mindmap`,
      filters: [
        { name: lang === "zh" ? "思维导图" : "Mindmap", extensions: ["mindmap"] },
        { name: "JSON", extensions: ["json"] },
      ],
    });
    if (typeof p !== "string" || !p) return;
    try {
      await ipc.saveTextFile(p, buildLocalPayload(), true);
      setLinkedFile(m.id, p);
      pushToast("success", lang === "zh" ? "已另存为" : "Saved as", p);
    } catch (e) {
      pushToast("error", lang === "zh" ? "另存为失败" : "Save as failed", errMessage(e).message);
    }
  }

  // ---------- import from local files (spec II-3) ----------
  function numOr(v: unknown, fallback: number): number {
    return typeof v === "number" && Number.isFinite(v) ? v : fallback;
  }

  function remapImportedNode(raw: unknown, idMap: Map<string, string>, mapId: string, dx: number, dy: number): MindNode {
    const n = (raw ?? {}) as Partial<MindNode>;
    const nid = uid();
    if (typeof n.id === "string") idMap.set(n.id, nid);
    const html = typeof n.textHtml === "string" ? sanitizeHtml(n.textHtml) : "";
    const base = makeNode(numOr(n.x, 0) + dx, numOr(n.y, 0) + dy, html);
    return {
      ...base,
      // Module-1: imports run through the same hard ABSOLUTE dimension clamps
      // as every persistence path (mins/maxes, poison guard) — aspect is not
      // enforced here so legitimately tall text frames survive a round-trip.
      ...((n.width !== undefined || n.height !== undefined)
        ? clampDims(numOr(n.width, base.width), numOr(n.height, base.height))
        : {}),
      ...(n.shape !== undefined ? { shape: n.shape } : {}),
      ...(n.borderRadius !== undefined ? { borderRadius: n.borderRadius } : {}),
      ...(n.borderColor !== undefined ? { borderColor: n.borderColor } : {}),
      ...(n.fillColor !== undefined ? { fillColor: n.fillColor } : {}),
      ...(n.fontSize !== undefined ? { fontSize: clamp(numOr(n.fontSize, base.fontSize), 12, 34) } : {}),
      // 可读性契约：导入文件里损坏的 0/负 opacity 至少恢复到 UI 允许的最低值
      // （属性面板滑杆下限 20%），绝不渲染近乎隐形的节点。
      ...(n.opacity !== undefined ? { opacity: clamp(numOr(n.opacity, 1), 0.2, 1) } : {}),
      ...(n.preset !== undefined ? { preset: n.preset } : {}),
      ...(n.rotation !== undefined ? { rotation: n.rotation } : {}),
      ...(n.collapsed !== undefined ? { collapsed: n.collapsed } : {}),
      ...(n.hidden !== undefined ? { hidden: n.hidden } : {}),
      id: nid,
      mindmapId: mapId,
      updatedAt: Date.now(),
    };
  }

  function remapImportedEdge(raw: unknown, idMap: Map<string, string>, mapId: string): MindEdge | null {
    const ed = (raw ?? {}) as Partial<MindEdge>;
    const s = typeof ed.sourceNodeId === "string" ? idMap.get(ed.sourceNodeId) : undefined;
    const tg = typeof ed.targetNodeId === "string" ? idMap.get(ed.targetNodeId) : undefined;
    if (!s || !tg) return null;
    const md = settingsRef.current.mindDefaults;
    return {
      id: uid(),
      mindmapId: mapId,
      sourceNodeId: s,
      targetNodeId: tg,
      direction: ed.direction ?? "forward",
      lineStyle: ed.lineStyle ?? md.edgeStyle,
      pathStyle: ed.pathStyle ?? "curve",
      color: ed.color ?? "#7f9bd9",
      width: numOr(ed.width, 1.6),
      label: typeof ed.label === "string" ? ed.label : "",
      animated: !!ed.animated && !settingsRef.current.safeMode,
      glow: !!ed.glow,
      createdAt: Date.now(),
    };
  }

  /** Open a local .mindmap/.json file as its OWN new mind map and jump in. */
  async function openLocalMindmapFile(path: string): Promise<void> {
    try {
      // 定位去重：这个文件之前打开过 → 直接回到它对应的导图上下文。
      const norm = path.replace(/\\/g, "/");
      for (const [mapId, linked] of Object.entries(linkedFilesRef.current)) {
        if (linked.replace(/\\/g, "/") === norm) {
          try {
            const data = await ipc.getMindmap(mapId);
            if (data?.mindmap) {
              pendingOpenFit = true;
              uiStore.setState({ currentMapId: mapId, mode: "mindmap" });
              pushToast("info", lang === "zh" ? "已定位到对应导图" : "Located the linked map", norm);
              return;
            }
          } catch { /* stale link → fall through to reopen */ }
        }
      }
      const text = await ipc.wsReadText(path);
      const parsed = parseMindmapFile(text);
      if (!parsed) {
        pushToast("error", t("invalidFile"));
        return;
      }
      // 空文档 ≠ 解析失败：一个合法但没有节点的文件必须给出明确文案，
      // 不能伪装成"文件损坏"，也不能假装打开成功。
      if (parsed.nodes.length === 0) {
        pushToast("info", lang === "zh" ? "该文件是空文档（不含任何节点）" : "This file is an empty document (no nodes)", path);
        return;
      }
      const stem = (path.split(/[\\/]/).pop() ?? "").replace(/\.(mindmap|json)$/i, "") || parsed.name || "imported";
      const m = await ipc.createMindmap(safeFileName(stem));
      try {
        const idMap = new Map<string, string>();
        const newNodes = parsed.nodes.map((raw) => remapImportedNode(raw, idMap, m.id, 0, 0));
        const newEdges = parsed.edges
          .map((raw) => remapImportedEdge(raw, idMap, m.id))
          .filter((e): e is MindEdge => e !== null);
        // 打开契约（七-防空载）：节点与边必须全部落库后才能切换 currentMapId。
        // 旧实现 saveEdge 是 fire-and-forget，加载_effect 的 getMindmap 可能
        // 跑在边落库之前 → 打开后偶发"有框无连线/内容不完整"的竞态。
        await ipc.saveNodes(newNodes);
        await Promise.all(newEdges.map((e) => ipc.saveEdge(e)));
        setLinkedFile(m.id, path);
        bumpMapList();
        pendingOpenFit = true; // open-contract: land the camera on the content
        uiStore.setState({ currentMapId: m.id, mode: "mindmap" });
        pushToast("success", lang === "zh" ? `已打开「${stem}」` : `Opened "${stem}"`, `${newNodes.length} nodes`);
      } catch (inner) {
        // 半写入的孤儿地图必须回收，避免库列表里出现一个误导性的空文档。
        void ipc.trashMindmap(m.id).catch(() => {});
        throw inner;
      }
    } catch (e) {
      pushToast("error", t("invalidFile"), errMessage(e).message);
    }
  }

  /** Merge a parsed file's nodes/edges into the CURRENT canvas. */
  function importNodesIntoCanvas(parsed: { nodes: unknown[]; edges: unknown[] }): void {
    const m = mapRef.current;
    if (!m || parsed.nodes.length === 0) return;
    pushHistory();
    const idMap = new Map<string, string>();
    let minX = Infinity;
    let minY = Infinity;
    for (const raw of parsed.nodes) {
      const n = (raw ?? {}) as Partial<MindNode>;
      minX = Math.min(minX, numOr(n.x, 0));
      minY = Math.min(minY, numOr(n.y, 0));
    }
    const r = containerRef.current?.getBoundingClientRect();
    const c = toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 500) / 2);
    const offX = c.x - (Number.isFinite(minX) ? minX : 0);
    const offY = c.y - (Number.isFinite(minY) ? minY : 0);
    const newNodes = parsed.nodes.map((raw) => remapImportedNode(raw, idMap, m.id, offX, offY));
    const newEdges = parsed.edges
      .map((raw) => remapImportedEdge(raw, idMap, m.id))
      .filter((e): e is MindEdge => e !== null);
    setNodes((prev) => [...prev, ...newNodes]);
    setSelection(new Set(newNodes.map((x) => x.id)));
    void ipc.saveNodes(newNodes).catch((e) => pushToast("error", lang === "zh" ? "导入失败" : "Import failed", errMessage(e).message));
    for (const e of newEdges) void ipc.saveEdge(e).catch(() => {});
    pushToast("success", t("importedOk", { n: newNodes.length }));
  }

  /** Drop a workspace file onto the canvas → sub-map anchor node that jumps.
   *  `.project` 档案例外（12.2）：生成樱花粉档案引用节点，双击进入项目分析空间。 */
  function createAnchorNode(path: string, clientX?: number, clientY?: number): void {
    const fileName = path.split(/[\\/]/).pop() ?? path;
    const isProject = /\.project$/i.test(fileName);
    const stem = isProject ? fileName.replace(/\.project$/i, "") : fileName.replace(/\.(mindmap|json)$/i, "");
    let wx = 0;
    let wy = 0;
    if (clientX !== undefined && clientY !== undefined) {
      const w = toWorld(clientX, clientY);
      wx = w.x;
      wy = w.y;
    } else {
      const r = containerRef.current?.getBoundingClientRect();
      const c = toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 600) / 2);
      wx = c.x;
      wy = c.y;
    }
    const html = isProject
      ? `<p>📂 <strong>${escapeHtml(stem)}</strong><br><span style="color:#f5c6d8">${lang === "zh" ? "项目档案 · 双击进入分析" : "Project archive · double-click to analyze"}</span></p>`
      : `<p>🧩 <strong>${escapeHtml(stem)}</strong><br><span style="color:#8fb0ff">${escapeHtml(fileName)}</span></p>`;
    pushHistory();
    const node: MindNode = {
      ...makeNode(wx - 100, wy - 30, html),
      mindmapId: mapRef.current?.id ?? "",
      recordId: isProject ? `pv-archive:${path}` : `file:${path}`,
      ...(isProject ? { borderColor: "#f8d4e4" } : {}),
    };
    setNodes((prev) => [...prev, node]);
    void ipc.saveNodes([node]).catch(() => {});
    pushToast("success", t("anchorCreated"), fileName);
  }


  // ---------- project visualization engine (spec chapters 2-7) ----------
  function worldCenter(): { x: number; y: number } {
    const r = containerRef.current?.getBoundingClientRect();
    return toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 600) / 2);
  }

  /** Convert a generated graph into real MindNodes/MindEdges, persist and fit. */
  function insertGraph(graph: GenGraph): void {
    if (!mapRef.current || graph.nodes.length === 0) return;
    pushHistory();
    const created: MindNode[] = graph.nodes.map((gn) => {
      const n = makeNode(gn.x, gn.y, gn.html);
      n.textPlain = gn.plain;
      n.width = gn.w;
      n.height = gn.h;
      n.shape = gn.kind === "root" ? "hexagon" : "rounded";
      n.borderColor = KIND_BORDER[gn.kind];
      n.fontSize = gn.kind === "info" || gn.kind === "source" ? 12 : 13;
      n.recordId = gn.recordId ?? null;
      return n;
    });
    setNodes((prev) => [...prev, ...created]);
    void ipc.saveNodes(created).catch((e) =>
      pushToast("error", lang === "zh" ? "节点保存失败" : "Node save failed", errMessage(e).message));
    const keyToId = new Map<string, string>();
    graph.nodes.forEach((gn, i) => keyToId.set(gn.key, created[i]!.id));
    for (const ge of graph.edges) {
      const s = keyToId.get(ge.from);
      const t = keyToId.get(ge.to);
      if (!s || !t || s === t) continue;
      const edge: MindEdge = {
        id: uid(),
        mindmapId: mapRef.current.id,
        sourceNodeId: s,
        targetNodeId: t,
        direction: "forward",
        lineStyle: props.settings.mindDefaults.edgeStyle,
        pathStyle: "curve",
        color: ge.color,
        width: 1.6,
        label: ge.label ?? "",
        animated: ge.animated && !props.settings.safeMode && !props.settings.reduceMotion && props.settings.mindDefaults.edgeAnim,
        glow: ge.animated,
        createdAt: Date.now(),
      };
      setEdges((prev) => [...prev, edge]);
      void ipc.saveEdge(edge).catch(() => {});
    }
    // Glide-fit the viewport to the freshly imported graph.
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of created) {
      minX = Math.min(minX, n.x); minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x + n.width); maxY = Math.max(maxY, n.y + n.height);
    }
    const pad = 100;
    const zw = ((containerRef.current?.clientWidth ?? 900) - pad * 2) / Math.max(1, maxX - minX);
    const zh = ((containerRef.current?.clientHeight ?? 640) - pad * 2) / Math.max(1, maxY - minY);
    const z = clamp(Math.min(zw, zh, 1), MIN_ZOOM, MAX_ZOOM);
    animateVpTo({ zoom: z, x: -((minX + maxX) / 2) * z, y: -((minY + maxY) / 2) * z }, 320);
  }

  /** 7.1 导入项目流程：扫描 → 解析 → 生成导图（带进度浮层）。 */
  async function importProjectFolder(root: string, at?: { x: number; y: number }): Promise<void> {
    if (!mapRef.current || pvImport) return;
    setPvImport({ root });
    try {
      const origin = at ?? worldCenter();
      const result = await ingestProject(root, origin, lang, (p) => setPvImport({ root, progress: p }));
      pvModelRef.current = result.model;
      insertGraph(result.graph);
      setPvImport(null);
      pushToast("success",
        lang === "zh" ? `项目解读完成（${result.graph.nodes.length} 个节点）` : `Project analyzed (${result.graph.nodes.length} nodes)`,
        result.model.detect.label);
    } catch (e) {
      setPvImport({ root, error: errMessage(e).message });
    }
  }

  /** 5.4 下钻进入：在文件节点周围展开函数/类节点。 */
  function drillDown(nodeId: string, analysis: FileAnalysis): void {
    const parent = nodeById.get(nodeId);
    if (!parent || !mapRef.current) return;
    // Guard: avoid duplicating a drill-down for the same file.
    const drillPrefix = `pv-fn:${analysis.relPath}:`;
    if (nodesRef.current.some((n) => n.recordId?.startsWith(drillPrefix))) {
      pushToast("info", lang === "zh" ? "该文件的函数节点已展开" : "Function nodes already expanded");
      return;
    }
    const { nodes: fnNodes, edges: fnEdges } = buildDrillDown(
      analysis,
      { key: parent.id, x: parent.x, y: parent.y, width: parent.width },
      lang,
    );
    if (fnNodes.length === 0) {
      pushToast("info", lang === "zh" ? "这个文件里没有解析出函数或类" : "No functions/classes parsed in this file");
      return;
    }
    pushHistory();
    const created = fnNodes.map((gn) => {
      const n = makeNode(gn.x, gn.y, gn.html);
      n.textPlain = gn.plain;
      n.width = gn.w;
      n.height = gn.h;
      n.borderColor = KIND_BORDER[gn.kind];
      n.fontSize = 12;
      n.recordId = gn.recordId ?? null;
      return n;
    });
    setNodes((prev) => [...prev, ...created]);
    void ipc.saveNodes(created).catch(() => {});
    const keyToId = new Map<string, string>([[parent.id, parent.id]]);
    created.forEach((n, i) => keyToId.set(fnNodes[i]!.key, n.id));
    for (const fe of fnEdges) {
      const s = keyToId.get(fe.from);
      const t = keyToId.get(fe.to);
      if (!s || !t || s === t) continue;
      const edge: MindEdge = {
        id: uid(),
        mindmapId: mapRef.current.id,
        sourceNodeId: s,
        targetNodeId: t,
        direction: "forward",
        lineStyle: "dashed",
        pathStyle: "curve",
        color: fe.color,
        width: 1.2,
        label: "",
        animated: false,
        glow: false,
        createdAt: Date.now(),
      };
      setEdges((prev) => [...prev, edge]);
      void ipc.saveEdge(edge).catch(() => {});
    }
    pushToast("success", lang === "zh" ? `已下钻：展开 ${created.length} 个节点` : `Drilled in: ${created.length} nodes`);
  }

  // ---------- render ----------
  if (chooserOpen) {
    return (
      <div className="mindmap-view chooser">
        <div className="chooser-card">
          <h3>{t("mapPicker")}</h3>
          {mapsList.length === 0 && <p className="dim">{lang === "zh" ? "还没有思维导图。" : "No mind maps yet."}</p>}
          {mapsList.map((m) => (
            <button key={m.id} type="button" className="btn ghost wide" onClick={() => uiStore.setState({ currentMapId: m.id })}>
              {m.name}
            </button>
          ))}
          <button
            type="button"
            className="btn primary wide"
            onClick={async () => {
              const v = await askPrompt({ title: t("newMapName"), initial: t("untitledMap") });
              if (!v) return;
              try {
                const m = await ipc.createMindmap(v.trim());
                uiStore.setState({ currentMapId: m.id });
                setChooserOpen(false);
              } catch (e) {
                pushToast("error", lang === "zh" ? "创建失败" : "Create failed", errMessage(e).message);
              }
            }}
          >
            + {t("newMapName")}
          </button>
        </div>
      </div>
    );
  }

  const menuNode = menuAnchor ? nodeById.get(menuAnchor.id) : null;
  const popEdge = edgePop ? edges.find((e) => e.id === edgePop.id) : null;
  const linkedNow = !!(map && linkedPaths[map.id]);

  return (
    <div className={`mindmap-view${editingId ? " editing-focus" : ""}`} tabIndex={0} onPointerDown={onCanvasRootPointerDown}>
      <div className="mm-toolbar">
        <div className="seg">
          <button type="button" className={tool === "pan" ? "on" : ""} data-tip={lang === "zh" ? "拖拽平移画布" : "Drag to pan"} aria-label="pan tool" onClick={() => setTool("pan")}><Hand size={14} /></button>
          <button type="button" className={tool === "select" ? "on" : ""} data-tip={t("marqueeHint")} aria-label="select tool" onClick={() => setTool("select")}><MousePointer2 size={14} /></button>
        </div>
        <span className="tb-sep" />
        <button type="button" className="icon-btn tiny" data-tip={t("newTextbox")} aria-label={t("newTextbox")}
          onClick={() => {
            const r = containerRef.current?.getBoundingClientRect();
            const c = toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 600) / 2);
            createNodeAt(c.x - 115, c.y - 36);
          }}
        >
          <Plus size={14} />
        </button>
        <span className="tb-sep" />
        <button type="button" className={`icon-btn tiny ${map?.gridEnabled ? "active" : ""}`} data-tip={t("gridToggle")} aria-label={t("gridToggle")} onClick={toggleGrid}><Grid3X3 size={14} /></button>
        <button type="button" className={`icon-btn tiny ${map?.snapEnabled ? "active" : ""}`} data-tip={t("snapToggle")} aria-label={t("snapToggle")} onClick={toggleSnap}><Magnet size={14} /></button>
        <span className="tb-sep" />
        <button type="button" className="icon-btn tiny" data-tip={t("mmUndo")} aria-label={t("undo")} disabled={history.current.past.length === 0} onClick={undo}><Undo2 size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("mmRedo")} aria-label={t("redo")} disabled={history.current.future.length === 0} onClick={redo}><Redo2 size={14} /></button>
        <span className="tb-sep" />
        <button type="button" className={`icon-btn tiny ${linkedNow ? "active" : ""}`} data-tip={t("mmSaveFile")} aria-label={t("mmSaveFile")} onClick={() => void saveToFile()}><Save size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("mmSaveAs")} aria-label={t("mmSaveAs")} onClick={() => void saveMapAs()}><FileDown size={14} /></button>
        <span className="flex-1" />
        <span className="zoom-pill">{Math.round(vp.zoom * 100)}%</span>
        <button type="button" className="icon-btn tiny" data-tip={t("zoomOut")} aria-label={t("zoomOut")} onClick={() => zoomAt(1 / 1.15)}><ZoomOut size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("zoomIn")} aria-label={t("zoomIn")} onClick={() => zoomAt(1.15)}><ZoomIn size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("fitAll")} aria-label={t("fitAll")} onClick={() => fitAll(false)}><Maximize2 size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("home")} aria-label={t("home")} onClick={homeCenter}><Crosshair size={14} /></button>
        <button type="button" className="icon-btn tiny" data-tip={t("quickFind")} aria-label={t("quickFind")} onClick={() => setQuickFind(true)}><Search size={14} /></button>
      </div>

      {map && (
        <div className="mm-map-name" data-tip={t("rename")}
          onDoubleClick={() =>
            void askPrompt({ title: t("rename"), initial: map.name }).then(async (v) => {
              if (!v) return;
              await ipc.renameMindmap(map.id, v.trim()).then(() => setMap({ ...map, name: v.trim() })).catch((e) => pushToast("error", t("rename"), errMessage(e).message));
            })
          }
        >
          {map.name}
        </div>
      )}

      <div
        ref={containerRef}
        className={`mm-canvas ${tool}`}
        // Focusable programmatically (not via Tab) so canvas shortcuts and
        // blur-clearing work; the root-level keyboard blackout is gone, so
        // window listeners now receive every key regardless of focus point.
        tabIndex={-1}
        onPointerDown={onCanvasPointerDown}
        onPointerMove={onCanvasPointerMove}
        onPointerUp={onCanvasPointerUp}
        onDoubleClick={(e) => {
          // Double-click empty canvas: glide-center the viewport there.
          if ((e.target as HTMLElement).closest(".mm-node")) return;
          if ((e.target as HTMLElement).closest(OVERLAY_SELECTOR)) return;
          const w = toWorld(e.clientX, e.clientY);
          animateVpTo({ ...vpRef.current, x: -w.x * vpRef.current.zoom, y: -w.y * vpRef.current.zoom }, 280);
        }}
        onContextMenu={(e) => {
          if ((e.target as HTMLElement).closest(OVERLAY_SELECTOR)) return;
          if ((e.target as HTMLElement).closest(".mm-node")) return;
          e.preventDefault();
          openContextMenu(e.clientX, e.clientY, [
            {
              label: t("newTextbox"),
              icon: <Plus size={13} />,
              onClick: () => createNodeAt(toWorld(e.clientX, e.clientY).x - 110, toWorld(e.clientX, e.clientY).y - 30),
            },
            { label: t("pasteNode"), disabled: !graphClipboard.current, onClick: () => pasteClipboard(...(() => { const w = toWorld(e.clientX, e.clientY); return [w.x - 110, w.y - 30] as const; })()) },
            { separator: true },
            {
              label: t("templates"),
              onClick: () =>
                openContextMenu(e.clientX, e.clientY, [
                  { label: t("tplMindmap"), onClick: () => insertTemplate("mindmap", toWorld(e.clientX, e.clientY)) },
                  { label: t("tplFlow"), onClick: () => insertTemplate("flow", toWorld(e.clientX, e.clientY)) },
                  { label: t("tplSwot"), onClick: () => insertTemplate("swot", toWorld(e.clientX, e.clientY)) },
                  { label: t("tplKanban"), onClick: () => insertTemplate("kanban", toWorld(e.clientX, e.clientY)) },
                ]),
            },
            {
              label: lang === "zh" ? "自动布局" : "Auto layout",
              onClick: () =>
                openContextMenu(e.clientX, e.clientY, [
                  { label: lang === "zh" ? "树状排列" : "Tree", onClick: () => autoLayout("tree") },
                  { label: lang === "zh" ? "网格排列" : "Grid", onClick: () => autoLayout("grid") },
                  { label: lang === "zh" ? "圆形排列" : "Circle", onClick: () => autoLayout("circle") },
                  { label: lang === "zh" ? "力导向排列" : "Force", onClick: () => autoLayout("force") },
                ]),
            },
            {
              label: lang === "zh" ? "整理连接线（正交）" : "Tidy edges (orthogonal)",
              onClick: tidyEdges,
            },
            { separator: true },
            {
              label: lang === "zh" ? "导入项目文件夹…" : "Import project folder…",
              icon: <FolderOpen size={13} />,
              onClick: async () => {
                const sel = await openFileDialog({ directory: true, multiple: false, title: lang === "zh" ? "选择项目文件夹" : "Choose project folder" });
                if (typeof sel === "string" && sel) void importProjectFolder(sel);
              },
            },
            {
              label: lang === "zh" ? "显示全部隐藏框架" : "Show all hidden frames",
              disabled: !nodes.some((n) => n.hidden),
              onClick: () => {
                pushHistory();
                setNodes((prev) => prev.map((n) => (n.hidden ? { ...n, hidden: false } : n)));
                scheduleNodesSave(nodes.filter((n) => n.hidden).map((n) => n.id));
              },
            },
            { separator: true },
            { label: t("selectAll"), onClick: () => setSelection(new Set(nodes.filter((n) => !n.hidden).map((n) => n.id))) },
            { label: t("fitAll"), onClick: () => fitAll(false) },
            { label: `${t("gridToggle")} (${map?.gridEnabled ? "✓" : ""})`, onClick: toggleGrid },
            { label: `${t("snapToggle")} (${map?.snapEnabled ? "✓" : ""})`, onClick: toggleSnap },
            {
              label: `${t("gridMode")}: ${gridModeLabel()}`,
              onClick: cycleGridMode,
            },
            { separator: true },
            { label: t("mmSaveFile"), icon: <Save size={13} />, onClick: () => void saveToFile() },
            { label: t("mmSaveAs"), icon: <FileDown size={13} />, onClick: () => void saveMapAs() },
            {
              label: t("export"),
              onClick: async () => {
                if (!map) return;
                const p = await saveDialog({ defaultPath: `${map.name}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
                if (typeof p !== "string") return;
                try {
                  await ipc.exportMindmapJson(map.id, p);
                  pushToast("success", t("exportedOk"), p);
                } catch (er) {
                  pushToast("error", lang === "zh" ? "导出失败" : "Export failed", errMessage(er).message);
                }
              },
            },
          ]);
        }}
      >
        <div className="mm-origin">
          <div
            className={`mm-world ${map?.gridEnabled ? `grid grid-${props.settings.mindDefaults.gridMode}` : "grid-none"}`}
            style={{
              width: WORLD_W,
              height: WORLD_H,
              transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.zoom})`,
              transformOrigin: "0 0",
              ["--grid-color" as string]: gridColorCss(),
              ["--grid-opacity" as string]: String(props.settings.mindDefaults.gridOpacity),
              ["--grid-size" as string]: `${gridSizeWorld()}px`,
            }}
          >
          {guides?.map((g, i) => (
            <div key={`guide-${i}`} className={`align-guide guide-${g.axis}`}
              style={g.axis === "v"
                ? { left: g.at, top: g.from, height: Math.max(1, g.to - g.from), display: "block" }
                : { top: g.at, left: g.from, width: Math.max(1, g.to - g.from), display: "block" }}>
              <span className="guide-gap">{g.gap !== undefined ? `${g.gap}px` : ""}</span>
            </div>
          ))}
          <EdgeLayer
            nodes={nodes}
            edges={edges.filter((e) => {
              const s = nodeById.get(e.sourceNodeId);
              const t = nodeById.get(e.targetNodeId);
              return !!s && !!t && !s.hidden && !t.hidden;
            })}
            selectedNodes={selection}
            selectedEdges={selectedEdges}
            connectingFrom={connectingFrom}
            connectPos={connectPos}
            animatedAllowed={!props.settings.safeMode && !props.settings.reduceMotion}
          />
          {(() => {
            const vr = viewRectWorld();
            // Keep selected/being-edited nodes mounted even when briefly
            // culled, so an active editor can never be unmounted mid-typing.
            // Fade-ghosts stay mounted for the dismissal animation window.
            const visibleNodes = nodes.filter((n) =>
              !n.hidden && (boxIntersectsRect(n, vr) || selection.has(n.id) || editingId === n.id || fadeGhosts.has(n.id)));
            return visibleNodes.map((n) => (
            <MindNodeView
              key={n.id}
              node={n}
              selected={selection.has(n.id)}
              ghostFading={fadeGhosts.has(n.id) && !selection.has(n.id)}
              editing={editingId === n.id}
              snapEnabled={!!map?.snapEnabled}
              resizeSensitivity={props.settings.mindDefaults.resizeSensitivity}
              dragging={draggingNode && selection.has(n.id)}
              showBox={editingId === n.id || freeTransform.has(n.id)}
              onCommitText={(html) => commitTextEdit(n.id, html)}
              onCancelEdit={cancelTextEdit}
              onToggleCollapse={() => patchNode(n.id, { collapsed: !n.collapsed })}
              onPointerDownNode={(e) => onNodePointerDown(e, n)}
              onDoubleClick={() => onNodeDoubleClick(n)}
              onNodeContextMenu={(e) => openNodeContextMenu(e, n)}
              onResizeStart={(e, h) => onResizeStart(e, h, n)}
              onVertexResizeStart={(e, i) => onVertexResizeStart(e, n, i)}
              onStartConnect={(e) => {
                setConnectingFrom(n.id);
                const w = toWorld(e.clientX, e.clientY);
                setConnectPos(w);
              }}
              onAction={(action) => {
                switch (action) {
                  case "edit": setEditingId(n.id); break;
                  case "duplicate": {
                    graphClipboard.current = { nodes: [structuredClone(n)], edges: [] };
                    pasteClipboard();
                    break;
                  }
                  case "connect": {
                    setConnectingFrom(n.id);
                    setSelection(new Set([n.id]));
                    break;
                  }
                  case "more": {
                    setMenuAnchor({ id: n.id });
                    break;
                  }
                  case "delete": void deleteSelectionBubble(); break;
                }
              }}
              menuAnchor={menuAnchor?.id === n.id}
            />
            ));
          })()}
          {marquee && (
            <div className="marquee" style={{
              left: Math.min(marquee.x0, marquee.x1),
              top: Math.min(marquee.y0, marquee.y1),
              width: Math.abs(marquee.x1 - marquee.x0),
              height: Math.abs(marquee.y1 - marquee.y0),
            }} />
          )}
          </div>
        </div>

        {restoredNote && (
          <div className="mm-note card-pop">{t("openLastViewport")}</div>
        )}

        {/* viewport status strip (zoom / coords / selection / tool) */}
        <div className="mm-status card-pop">
          <span>{Math.round(vp.zoom * 100)}%</span>
          <span className="sep">·</span>
          <span>{Math.round(cursorWorld.x)}, {Math.round(cursorWorld.y)}</span>
          <span className="sep">·</span>
          <span>{lang === "zh" ? "选中" : "sel"} {selection.size}</span>
          <span className="sep">·</span>
          <span>{tool === "pan" ? (lang === "zh" ? "抓手" : "Pan") : lang === "zh" ? "框选" : "Select"}</span>
          <button type="button" className={`icon-btn tiny ${minimapOpen ? "active" : ""}`}
            data-tip={t("minimap")} aria-label={t("minimap")}
            onClick={() => setMinimapOpen(!minimapOpen)}>
            <MiniMapIcon size={13} />
          </button>
        </div>

        <SelectionOpsBar
          count={selection.size}
          ops={{
            align: alignSel,
            distribute: distributeSel,
            autoLayout: () => autoLayout("tree"),
            uniform,
            uniformPreset: () => {
              const first = selectedArr[0];
              if (first && selectedArr.length >= 2) patchMany(selection, { preset: first.preset });
            },
            chainConnect,
            exportSelected: () => void exportSelected(),
            copy: () => copySelection(),
            deleteSel: () => void deleteSelectionBubble(),
          }}
        />

        {/* hidden bottom dock (spec 5.1) */}
        <DockBar
          hasSel={selection.size > 0}
          count={selection.size}
          locked={selectedArr.length === 1 && selectedArr[0] ? !!selectedArr[0].locked : undefined}
          onEdit={() => {
            const id = Array.from(selection)[0];
            const n = id ? nodeById.get(id) : null;
            if (n && !n.locked && id) setEditingId(id);
          }}
          onCopy={() => copySelection()}
          onDelete={() => void deleteSelectionBubble(window.innerWidth / 2 - 90, window.innerHeight - 110)}
          onLock={() => {
            if (selection.size === 0) return;
            const anyUnlocked = selectedArr.some((n) => !n.locked);
            patchMany(selection, { locked: anyUnlocked });
          }}
          onStyle={() => {
            if (selection.size !== 1) return;
            const id = Array.from(selection)[0]!;
            setMenuAnchor({ id });
          }}
          onMore={() => {
            if (selection.size !== 1) return;
            openContextMenu(window.innerWidth / 2 - 120, window.innerHeight - 150, nodeMenuItems(nodeById.get(Array.from(selection)[0]!)!));
          }}
        />

        {quickFind && (
          <div className="quick-find card-pop">
            <Search size={14} />
            <input
              autoFocus
              value={quickQuery}
              placeholder={t("quickFind")}
              onChange={(e) => setQuickQuery(e.target.value)}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Escape") setQuickFind(false);
                if (e.key === "Enter") {
                  const q = quickQuery.toLowerCase();
                  const hit = nodes.find((n) => stripTags(n.textHtml).toLowerCase().includes(q));
                  if (hit) {
                    setSelection(new Set([hit.id]));
                    centerOnWorld(hit.x + hit.width / 2, hit.y + hit.height / 2);
                  } else {
                    pushToast("info", t("noMatch"));
                  }
                }
              }}
            />
            <button type="button" className="icon-btn tiny" onClick={() => setQuickFind(false)}>✕</button>
          </div>
        )}

        {menuNode && menuAnchor && (
          <NodeMoreMenu
            node={menuNode}
            onClose={() => setMenuAnchor(null)}
            onPatch={(patch) => patchNode(menuNode.id, patch)}
            onCreateRecordFromNode={() => void createRecordFromNode(menuNode.id)}
            onOpenLinkedRecord={
              menuNode.recordId
                ? () => uiStore.setState({ currentDocId: menuNode.recordId!, mode: "write" })
                : null
            }
            onDelete={() => void deleteSelection()}
          />
        )}

        {popEdge && edgePop && (
          <EdgePopover
            edge={popEdge}
            onClose={() => setEdgePop(null)}
            onPatch={(patch) => void patchEdge(popEdge.id, patch)}
            onDelete={() => void deleteSelectedEdges()}
          />
        )}

        {minimapOpen && map && (
          <Minimap
            nodes={nodes.filter((n) => !n.hidden)}
            selectionIds={selection}
            viewport={{ x: vp.x, y: vp.y, zoom: vp.zoom }}
            canvasSize={{ w: containerRef.current?.clientWidth ?? 1200, h: containerRef.current?.clientHeight ?? 800 }}
            onMoveViewport={(wx, wy) => centerOnWorld(wx, wy)}
          />
        )}

        {!menuAnchor && selection.size === 1 && nodeById.get(Array.from(selection)[0]!) !== undefined && (() => {
          const selNode = nodeById.get(Array.from(selection)[0]!)!;
          return (
            <InspectorPanel
              key={selNode.id}
              node={selNode}
              onPatch={(patch) => patchNode(selNode.id, patch)}
              onZOrder={(dir) => patchNode(selNode.id, { zIndex: selNode.zIndex + dir })}
              onClose={() => setSelection(new Set())}
            />
          );
        })()}

        {/* ---- project visualization engine panels (spec chapters 5-7) ---- */}
        <ProjectImportOverlay state={pvImport} onCancel={() => setPvImport(null)} />
        {pvInfoId !== null && (() => {
          const infoNode = nodeById.get(pvInfoId);
          if (!infoNode || !infoNode.recordId?.startsWith("pv:")) return null;
          return (
            <FileInfoCard
              node={infoNode}
              model={pvModelRef.current}
              onClose={() => setPvInfoId(null)}
              onDrill={(analysis) => drillDown(infoNode.id, analysis)}
            />
          );
        })()}
      </div>
    </div>
  );
}






































