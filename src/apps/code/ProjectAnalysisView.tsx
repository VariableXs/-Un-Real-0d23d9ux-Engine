﻿/**
 * 项目分析空间（PVCCE 独立模块 · 沉浸式工作区）。
 * 布局（三章 3.2，与主软件同一套视觉语言，禁止弹窗表单）：
 *   左侧可折叠文件树（多选/筛选/固定到画布/移除）
 *   中央无限画布（WASD/滚轮，与思维导图同构；结构可远超视口，靠相机漫游）
 *   右侧详情舱（文件解读 / 推理链 / 意图结果 / 字典管理）
 * 背景：一、真实光影极光引擎（AuroraCanvas，WebGL2 Ray-Marching）。
 * 逐行解剖：二、双轨虚拟滚动（L2 结构 → L3 段落 → L4 逐行，可折叠）。
 * 反写：四、意图条会话生效 → 顶栏「写入源文件」二段式落盘（备份+原子+可撤销）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { save as saveDialog, open as fileDialog } from "@tauri-apps/plugin-dialog";
import {
  PenLine, FolderOpen, Save, FileDown, Lightbulb, Maximize2, Link2,
  FolderSearch, Trash2, RefreshCw, PanelLeftClose, PanelLeftOpen, Pin, Copy, ExternalLink,
  Undo2, BookOpen, ArrowLeft, GitBranch,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { uid } from "../../lib/format";
import { pushToast, uiStore } from "../../state/uiStore";
import { loadSettings, saveSetting } from "../../lib/settings";
import { L1_UNIVERSAL, L2_LANGUAGE, L3_FRAMEWORK, L4_DOMAIN, L6_IDIOM, L7_PATTERN } from "./dictionaries";
import { openContextMenu } from "../../components/ContextMenu";
import { xfSet } from "../../lib/xflow";
import { ingestProject } from "./ingest";
import { parseSource } from "./parsers";
import { fileOneLiner } from "./narrate";
import { planIntent } from "./intent";
import { blockRanges, chunkBody, explainLine, structuralBlocks, type LineCard } from "./anatomy";
import { buildReasoning, inferGoal, type ReasoningChains } from "./reasoning";
import { applyIntent, type EditPreview, type IntentResult } from "./transforms";
import type { NarrateStyle } from "./deepDescribe";
import { EditHistory, backupPath } from "./writeEngine";
import { buildDrillDown, routeEdge, CROSS_REF_COLOR, CROSS_REF_NODE, KIND_BORDER, type ProjectModel } from "./generate";
import { AuroraCanvas } from "./AuroraCanvas";
import { ProjectImportOverlay, FileInfoCard } from "./ProjectVizPanels";
import type { FileAnalysis, GenNode, IntentPlan, LangId, ProjectArchive, ProjectDetect } from "./types";
import type { Settings } from "../../lib/settings";
import type { MindNode } from "../../lib/types";

/** Session singleton: survives switching between the two spaces. */
const pvSession: {
  archive: ProjectArchive | null;
  filePath: string | null;
  model: ProjectModel | null;
  external: Map<string, FileAnalysis>;
} = { archive: null, filePath: null, model: null, external: new Map() };

const MIN_Z = 0.08;
const MAX_Z = 4;
const GENERIC_DETECT: ProjectDetect = {
  typeId: "generic", label: "", domain: "", domainKey: "", evidence: [], stack: [],
  entryCandidates: [], primaryLang: "generic",
};

interface Vp { x: number; y: number; z: number }

/** 第一章：逐行解剖的焦点状态（L2 结构级 / L3 段落级 / L4 逐行级）。 */
interface FocusState {
  rel: string;
  root: string;
  level: 2 | 3 | 4;
  symbol?: string;
  content: string;
  lang: LangId;
}

type Pod =
  | { kind: "file"; recordId: string }
  | { kind: "intent"; plan: IntentPlan }
  | { kind: "reasoning"; rel: string; symbol: string; chains: ReasoningChains }
  | { kind: "dict" };

/** 二.3：虚拟化后行数不再截断——逻辑高度按真实行数计算，可一路滚到底。 */
const L4_LINE_HARD_CAP = 20000;

/** 第八章 8.1：冷调语法高亮（先转义再包 span，绝不注入原文）。 */
function highlight(code: string): string {
  const esc1 = code.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return esc1.replace(
    /\b(const|let|var|function|def|fn|class|struct|enum|impl|pub|return|if|else|for|while|loop|match|try|catch|except|import|export|use|await|async|new|throw)\b/g,
    `<span class="pv-kw">$1</span>`,
  ).replace(
    /(&quot;|")([^"]*?)\1/g,
    `<span class="pv-str">"$2"</span>`,
  );
}

export function ProjectAnalysisView(props: { settings: Settings }): React.ReactElement {
  const { lang } = useI18n();
  const [archive, setArchive] = useState<ProjectArchive | null>(pvSession.archive);
  const [filePath, setFilePath] = useState<string | null>(pvSession.filePath);
  const [vp, setVp] = useState<Vp>({ x: 0, y: 0, z: 1 });
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [pod, setPod] = useState<Pod | null>(null);
  const [toolMode, setToolMode] = useState<"select" | "intent">("select");
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [treeOpen, setTreeOpen] = useState(true);
  const [treeFilter, setTreeFilter] = useState("");
  const [treeSel, setTreeSel] = useState<Set<string>>(new Set());
  const [dropHover, setDropHover] = useState(false);
  const [refsOpen, setRefsOpen] = useState(false);
  const [pvImport, setPvImport] = useState<{ root: string; progress?: import("./ingest").IngestProgress; error?: string } | null>(null);
  const [hoverId, setHoverId] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);
  const vpRef = useRef(vp);
  vpRef.current = vp;
  const animRef = useRef(0);
  const archiveRef = useRef(archive);
  archiveRef.current = archive;
  const toolRef = useRef(toolMode);
  toolRef.current = toolMode;
  const keysRef = useRef<Set<string>>(new Set());
  // ---- 第一章：逐行解剖焦点 + 内容缓存 + 编辑历史 + 樱粉闪烁 ----
  const [focus, setFocus] = useState<FocusState | null>(null);
  const focusRef = useRef(focus);
  focusRef.current = focus;
  const contentCacheRef = useRef(new Map<string, string>());
  const editHistoryRef = useRef(new EditHistory());
  const [flashKeys, setFlashKeys] = useState<Set<string>>(new Set());
  const [inlineBar, setInlineBar] = useState<{ mode: "edit" | "continue" | "note"; line: number; text: string } | null>(null);
  const inlineBarRef = useRef(inlineBar);
  inlineBarRef.current = inlineBar;
  /** 四章：已应用到会话、等待「写入源文件」确认的改动。 */
  const [pendingWrite, setPendingWrite] = useState<{
    rel: string; root: string; abs: string; before: string; content: string; note: string; utterance: string; line: number;
  } | null>(null);
  /** 一/三章：L4 虚拟滚动 + 语义块折叠。 */
  const [collapsedSyms, setCollapsedSyms] = useState<Set<string>>(new Set());
  const [vScrollTop, setVScrollTop] = useState(0);
  /** 2.4 拖画布时降级连线动画/阴影，停稳恢复。 */
  const [panActive, setPanActive] = useState(false);

  const markSession = useCallback((a: ProjectArchive | null, path: string | null): void => {
    pvSession.archive = a;
    pvSession.filePath = path;
  }, []);

  function patchArchive(fn: (a: ProjectArchive) => ProjectArchive): void {
    const a = archiveRef.current;
    if (!a) return;
    const next = fn(a);
    archiveRef.current = next;
    setArchive(next);
    markSession(next, filePath);
  }

  // ---------- open pending archive (.project double-click in workspace) ----------
  useEffect(() => {
    const pending = uiStore.getState().pvPendingOpen;
    if (pending) {
      uiStore.setState({ pvPendingOpen: null });
      void loadArchiveFile(pending);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- WASD / arrow pan + Esc / Ctrl+Z（与主软件同一套平移） ----------
  useEffect(() => {
    const navRaf = { current: 0 };
    let last = performance.now();
    const tick = (now: number): void => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const keys = keysRef.current;
      const ae = document.activeElement as HTMLElement | null;
      const typing = !!ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.isContentEditable);
      const speed = 620;
      let ax = 0;
      let ay = 0;
      if (!typing) {
        if (keys.has("w") || keys.has("arrowup")) ay += speed;
        if (keys.has("s") || keys.has("arrowdown")) ay -= speed;
        if (keys.has("a") || keys.has("arrowleft")) ax += speed;
        if (keys.has("d") || keys.has("arrowright")) ax -= speed;
      }
      if (ax !== 0 || ay !== 0) {
        const cur = vpRef.current;
        const nv = { ...cur, x: cur.x + ax * dt, y: cur.y + ay * dt };
        vpRef.current = nv;
        setVp(nv);
      }
      navRaf.current = keys.size > 0 ? requestAnimationFrame(tick) : 0;
    };
    const kd = (e: KeyboardEvent): void => {
      if (uiStore.getState().mode !== "project") return;
      if (e.isComposing) return;
      const k = e.key.toLowerCase();
      if (["w", "a", "s", "d", "arrowup", "arrowdown", "arrowleft", "arrowright"].includes(k)) {
        keysRef.current.add(k);
        if (navRaf.current === 0) {
          last = performance.now();
          navRaf.current = requestAnimationFrame(tick);
        }
      }
      if (k === "z" && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
        const ae2 = document.activeElement as HTMLElement | null;
        const typingNow = !!ae2 && (ae2.tagName === "INPUT" || ae2.tagName === "TEXTAREA" || ae2.isContentEditable);
        if (!typingNow) {
          e.preventDefault();
          undoLastEdit();
        }
        return;
      }
      if (k === "escape") {
        if (inlineBarRef.current) {
          setInlineBar(null);
          return;
        }
        if (focusRef.current) {
          popFocusLevel();
          return;
        }
        setEditingKey(null);
        setPod(null);
        setSelection(new Set());
      }
      // 6.2 面包屑空间导航：Backspace 快捷返回上一层
      if (k === "backspace" && focusRef.current) {
        e.preventDefault();
        popFocusLevel();
      }
      // 二.2 F 键：一键适应全部（Fit to Content）
      if (k === "f" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const ae3 = document.activeElement as HTMLElement | null;
        const typingF = !!ae3 && (ae3.tagName === "INPUT" || ae3.tagName === "TEXTAREA" || ae3.isContentEditable);
        if (!typingF) fitAll();
      }
    };
    const ku = (e: KeyboardEvent): void => {
      keysRef.current.delete(e.key.toLowerCase());
    };
    const blur = (): void => keysRef.current.clear();
    window.addEventListener("keydown", kd);
    window.addEventListener("keyup", ku);
    window.addEventListener("blur", blur);
    return () => {
      cancelAnimationFrame(navRaf.current);
      window.removeEventListener("keydown", kd);
      window.removeEventListener("keyup", ku);
      window.removeEventListener("blur", blur);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- drag & drop: folder → import; files → append; .project → open ----------
  useEffect(() => {
    let disposed = false;
    let un: (() => void) | undefined;
    const p = getCurrentWebview().onDragDropEvent(async (event) => {
      try {
        const paths = "paths" in event.payload ? [...(event.payload.paths ?? [])] : [];
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setDropHover(paths.length > 0);
          return;
        }
        if (event.payload.type === "leave") {
          setDropHover(false);
          return;
        }
        setDropHover(false);
        if (event.payload.type !== "drop" || paths.length === 0) return;
        const first = paths[0]!;
        if (/\.project$/i.test(first)) {
          await loadArchiveFile(first);
          return;
        }
        const kinds = await ipc.checkPaths(paths).catch(() => []);
        const dirs = paths.filter((_, i) => kinds[i]?.kind === "dir");
        if (dirs.length > 0) {
          await importProjectFolder(dirs[0]!);
          return;
        }
        await importLooseFiles(paths);
      } catch { /* non-fatal */ }
    });
    void p.then((u) => {
      if (disposed) u();
      else un = u;
    }).catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- wheel zoom (anchored) ----------
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent): void => {
      e.preventDefault();
      if (dragRef.current) return;
      const r = el.getBoundingClientRect();
      const px = e.clientX - r.left - r.width / 2;
      const py = e.clientY - r.top - r.height / 2;
      const cur = vpRef.current;
      const z2 = Math.min(MAX_Z, Math.max(MIN_Z, cur.z * Math.pow(1.0016, -e.deltaY)));
      const wx = (px - cur.x) / cur.z;
      const wy = (py - cur.y) / cur.z;
      const nv = { z: z2, x: px - wx * z2, y: py - wy * z2 };
      vpRef.current = nv;
      setVp(nv);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  useEffect(() => {
    // 焦点切换时重置虚拟滚动位置
    setVScrollTop(0);
  }, [focus?.rel, focus?.level, focus?.symbol]);

  // 四层视差：把相机位姿广播给极光星空（L0 5% / L1 15% / L2 45% / L3 100%）
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("variable:pv-cam", { detail: { x: vp.x, y: vp.y, z: vp.z } }));
  }, [vp.x, vp.y, vp.z]);

  // ---------- 1.3 自检：空状态导入按钮必须真实可点（命中测试=按钮自身） ----------
  useEffect(() => {
    if (pvSession.archive) return;
    const timer = window.setTimeout(() => {
      const btn = containerRef.current?.querySelector(".pv-empty button") as HTMLElement | null;
      if (!btn) return;
      const r = btn.getBoundingClientRect();
      const inWin = r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 && r.top < window.innerHeight && r.left < window.innerWidth;
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      const hitOk = !!hit && (hit === btn || btn.contains(hit));
      if (!inWin || !hitOk) {
        console.error("[pv] import button hit-test failed", { inWin, hitOk, hit });
        void ipc.log("warn", `pv import button hit-test failed: inWin=${inWin} hitOk=${hitOk}`).catch(() => {});
      }
    }, 350);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function toWorld(clientX: number, clientY: number): { x: number; y: number } {
    const el = containerRef.current;
    const r = el?.getBoundingClientRect();
    const v = vpRef.current;
    return {
      x: (clientX - (r?.left ?? 0) - (r?.width ?? 0) / 2 - v.x) / v.z,
      y: (clientY - (r?.top ?? 0) - (r?.height ?? 0) / 2 - v.y) / v.z,
    };
  }

  /** 1.3 命中测试白名单：点到任何交互元素（按钮/输入/空状态/面板）时，画布既不清空
   *  选中也不起拖——绝不抢走 UI 的 click（修复「导入按钮点不开」的根因）。 */
  const UI_HIT =
    "button, a, input, textarea, select, label, [data-interactive], .pv-empty, .pv-node, .pv-toolbar, .pv-tree, .pv-pod, .pv-status, .pv-progress-strip, .pv-refs, .pv-inline-bar, .pv-crumb-bar, .ctx-menu, .card-pop";

  function onCanvasPointerDown(e: React.PointerEvent): void {
    const el = e.target as HTMLElement | null;
    if (el?.closest(UI_HIT)) return;
    containerRef.current?.focus({ preventScroll: true });
    if (e.button === 0) {
      setSelection(new Set());
      setPod(null);
      // 5.1 意图 = 画布工具模式：空白单击放置轻量意图节点（非模态）。
      if (toolRef.current === "intent") {
        placeIntentNode(toWorld(e.clientX, e.clientY));
        setToolMode("select");
        return;
      }
      setPanActive(true);
    }
    dragRef.current = { sx: e.clientX, sy: e.clientY, ox: vpRef.current.x, oy: vpRef.current.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onCanvasPointerMove(e: React.PointerEvent): void {
    const d = dragRef.current;
    if (!d) return;
    const nv = { ...vpRef.current, x: d.ox + (e.clientX - d.sx), y: d.oy + (e.clientY - d.sy) };
    vpRef.current = nv;
    setVp(nv);
  }

  function onCanvasPointerUp(): void {
    dragRef.current = null;
    setPanActive(false);
  }

  function fitAll(): void {
    const a = archiveRef.current;
    if (!a || a.graph.nodes.length === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of a.graph.nodes) {
      minX = Math.min(minX, n.x); minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x + n.w); maxY = Math.max(maxY, n.y + n.h);
    }
    const el = containerRef.current;
    const pad = 90;
    const zw = ((el?.clientWidth ?? 900) - pad * 2) / Math.max(1, maxX - minX);
    const zh = ((el?.clientHeight ?? 640) - pad * 2) / Math.max(1, maxY - minY);
    const z = Math.min(MAX_Z, Math.max(MIN_Z, Math.min(zw, zh, 1.2)));
    const nv = { z, x: -((minX + maxX) / 2) * z, y: -((minY + maxY) / 2) * z };
    vpRef.current = nv;
    setVp(nv);
  }

  // ---------- import / archive IO ----------
  async function importProjectFolder(root: string): Promise<void> {
    if (pvImport) return;
    setPvImport({ root });
    try {
      const result = await ingestProject(root, { x: 0, y: 0 }, lang, (p) => setPvImport({ root, progress: p }));
      const prev = archiveRef.current;
      const sameRoot = prev && prev.root.replace(/[\\/]+$/, "") === root.replace(/[\\/]+$/, "");
      const next: ProjectArchive = {
        formatVersion: 1,
        root: result.scan.root,
        savedAt: Date.now(),
        detect: result.model.detect,
        graph: result.graph,
        entries: result.scan.entries,
        notes: {},
        refs: sameRoot ? prev!.refs : [],
        external: sameRoot ? prev!.external ?? [] : [],
      };
      pvSession.model = result.model;
      setArchive(next);
      markSession(next, filePath);
      setPvImport(null);
      setSelection(new Set());
      setToolMode("select");
      requestAnimationFrame(() => fitAll());
      pushToast("success", lang !== "en" ? `项目解读完成（${next.graph.nodes.length} 个节点）` : `Project analyzed (${next.graph.nodes.length} nodes)`, next.detect.label);
    } catch (e) {
      setPvImport({ root, error: errText(e) });
    }
  }

  /** 四.1：增量追加单个/多个源文件（可与项目共存，不重开空间）。 */
  async function importLooseFiles(paths: string[]): Promise<void> {
    let added = 0;
    for (const abs of paths.slice(0, 24)) {
      try {
        const norm = abs.replace(/[\\/]+$/, "");
        const sep = norm.includes("\\") ? "\\" : "/";
        const segs = norm.split(sep);
        const name = segs.pop() ?? abs;
        const parent = segs.join(sep);
        const rel = `${parent.replace(/[\\/]/g, "/")}/${name}`;
        if (pvSession.external.has(rel)) continue;
        const content = await ipc.readTextFile(abs);
        const analysis = parseSource(name, content);
        pvSession.external.set(rel, analysis);
        if (!archiveRef.current) {
          const shell: ProjectArchive = {
            formatVersion: 1, root: parent, savedAt: Date.now(),
            detect: { ...GENERIC_DETECT, label: lang !== "en" ? "一组独立文件" : "a set of loose files" },
            graph: { nodes: [], edges: [], rootKey: "", fileKeys: new Map() },
            entries: [],
            notes: {}, refs: [], external: [],
          };
          pvSession.model = { scan: { root: parent, entries: [], sources: [], truncated: false, skipped: 0 }, detect: shell.detect, analyses: new Map(), usedBy: new Map() };
          setArchive(shell);
          markSession(shell, filePath);
        }
        patchArchive((a) => ({
          ...a,
          external: [...(a.external ?? []).filter((x) => x.rel !== rel), { absPath: abs, rel }],
        }));
        await pinRel(rel, parent, true);
        added++;
      } catch (e) {
        pushToast("error", lang !== "en" ? `无法读取 ${abs.split(/[\\/]/).pop()}` : `Cannot read file`, errText(e));
      }
    }
    if (added > 0) {
      pushToast("success", lang !== "en" ? `已加入 ${added} 个文件到分析台` : `${added} file(s) added`);
      requestAnimationFrame(() => fitAll());
    }
  }

  /** 四.3 固定到画布：把文件（项目内或散装）变为画布文件节点。 */
  async function pinRel(rel: string, root: string, placeAtEnd: boolean): Promise<void> {
    const a = archiveRef.current;
    if (!a) return;
    const recordId = `pv:${rel}@${root}`;
    if (a.graph.nodes.some((n) => n.recordId === recordId)) return;
    const analysis = pvSession.model?.analyses.get(rel) ?? pvSession.external.get(rel) ?? null;
    const name = rel.split("/").pop() ?? rel;
    const one = analysis ? fileOneLiner(analysis, lang) : name;
    const role = analysis?.role ?? "source";
    const kind = role === "config" ? "config" : role === "doc" ? "doc" : "source";
    let x = 0;
    let y = 0;
    if (placeAtEnd && a.graph.nodes.length > 0) {
      const last = a.graph.nodes[a.graph.nodes.length - 1]!;
      x = last.x + 340;
      y = last.y;
    } else {
      const c = worldCenter();
      x = c.x - 120;
      y = c.y - 32;
    }
    const node: GenNode = {
      key: `pin-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      html: `<p>📄 <strong>${esc(name)}</strong><br><span style="opacity:.75;font-size:12px">${esc(one)}</span></p>`,
      plain: one,
      kind,
      x: Math.round(x), y: Math.round(y), w: 250, h: 64,
      recordId,
    };
    patchArchive((cur) => ({ ...cur, graph: { ...cur.graph, nodes: [...cur.graph.nodes, node] } }));
  }

  function pinSelected(): void {
    const a = archiveRef.current;
    if (!a) return;
    void (async () => {
      let n = 0;
      for (const rel of treeSel) {
        const ext = (a.external ?? []).find((x) => x.rel === rel);
        const root = ext ? ext.absPath.slice(0, ext.absPath.length - rel.split("/").pop()!.length - 1) : a.root;
        const before = archiveRef.current?.graph.nodes.length ?? 0;
        await pinRel(rel, root, n > 0);
        if ((archiveRef.current?.graph.nodes.length ?? 0) > before) n++;
      }
      if (n > 0) pushToast("success", lang !== "en" ? `已固定 ${n} 个文件到画布` : `${n} file(s) pinned`);
      setTreeSel(new Set());
    })();
  }

  /** 四.3 从分析中移除：仅移出本次会话/画布，不删磁盘文件。 */
  function removeRel(rel: string): void {
    patchArchive((a) => {
      const kept = a.graph.nodes.filter((n) => !n.recordId?.startsWith(`pv:${rel}@`));
      const keptKeys = new Set(kept.map((n) => n.key));
      return {
        ...a,
        external: (a.external ?? []).filter((x) => x.rel !== rel),
        graph: {
          ...a.graph,
          nodes: kept,
          edges: a.graph.edges.filter((e) => keptKeys.has(e.from) && keptKeys.has(e.to)),
        },
      };
    });
    pvSession.external.delete(rel);
    setTreeSel((prev) => {
      const nx = new Set(prev);
      nx.delete(rel);
      return nx;
    });
  }

  // ---------- canvas graph mutation ----------
  function appendNodes(nodes: GenNode[], edges: ProjectArchive["graph"]["edges"]): void {
    patchArchive((a) => ({
      ...a,
      graph: { ...a.graph, nodes: [...a.graph.nodes, ...nodes], edges: [...a.graph.edges, ...edges] },
    }));
  }

  function removeNode(key: string): void {
    const a = archiveRef.current;
    if (!a) return;
    const n = a.graph.nodes.find((x) => x.key === key);
    if (!n) return;
    if (n.recordId?.startsWith("pv:")) {
      const body = n.recordId.slice(3);
      const at = body.lastIndexOf("@");
      removeRel(at > 0 ? body.slice(0, at) : body);
      return;
    }
    patchArchive((cur) => ({
      ...cur,
      graph: {
        ...cur.graph,
        nodes: cur.graph.nodes.filter((x) => x.key !== key),
        edges: cur.graph.edges.filter((e) => e.from !== key && e.to !== key),
      },
    }));
    if (editingKey === key) setEditingKey(null);
  }

  function worldCenter(): { x: number; y: number } {
    const r = containerRef.current?.getBoundingClientRect();
    return toWorld((r?.left ?? 0) + (r?.width ?? 800) / 2, (r?.top ?? 0) + (r?.height ?? 600) / 2);
  }

  /** 5.1 意图节点：体量接近普通导图节点的轻量卡片，双击内联编辑。 */
  function placeIntentNode(at: { x: number; y: number }): void {
    const key = `intent-${Date.now()}`;
    const node: GenNode = {
      key,
      html: `<p>💡 <span style="opacity:.65">${lang !== "en" ? "双击输入：我想做一个…" : "Double-click: I want to…"}</span></p>`,
      plain: "",
      kind: "intent",
      x: Math.round(at.x - 120),
      y: Math.round(at.y - 32),
      w: 250, h: 64,
    };
    appendNodes([node], []);
    setSelection(new Set([key]));
    setEditingKey(key);
  }

  function commitIntentEdit(key: string, text: string): void {
    setEditingKey(null);
    const a = archiveRef.current;
    const node = a?.graph.nodes.find((n) => n.key === key);
    if (!node) return;
    const trimmed = text.trim();
    patchArchive((cur) => ({
      ...cur,
      graph: {
        ...cur.graph,
        nodes: cur.graph.nodes.map((n) => (n.key === key
          ? {
            ...n,
            plain: trimmed,
            html: trimmed
              ? `<p>💡 <strong>${esc(trimmed.slice(0, 60))}</strong></p>`
              : `<p>💡 <span style="opacity:.65">${lang !== "en" ? "双击输入：我想做一个…" : "Double-click: I want to…"}</span></p>`,
          }
          : n)),
      },
    }));
    if (!trimmed) return;
    generateIntent(key, trimmed);
  }

  /** 5.2：通俗步骤长在意图节点下（子分支），代码说明进右侧详情舱。 */
  function generateIntent(key: string, text: string): void {
    const a = archiveRef.current;
    const detect = a?.detect ?? { ...GENERIC_DETECT, label: lang !== "en" ? "未导入项目" : "no project" };
    const plan = planIntent(text, detect, lang);
    const node = a?.graph.nodes.find((n) => n.key === key);
    const bx = node ? node.x : 0;
    const by = node ? node.y + node.h + 30 : 0;
    const stepNodes: GenNode[] = plan.steps.slice(0, 5).map((s, i) => ({
      key: `${key}-step${i}`,
      html: `<p style="font-size:12px">${esc(s)}</p>`,
      plain: s,
      kind: "info",
      x: Math.round(bx + i * 16),
      y: Math.round(by + i * 74),
      w: 260, h: 56,
    }));
    const codeNode: GenNode | null = plan.code
      ? {
        key: `${key}-code`,
        html: `<p>⌨️ <strong>${esc(plan.targetFile.split("/").pop() ?? plan.codeLang)}</strong></p>`,
        plain: plan.targetFile,
        kind: "source",
        x: Math.round(bx + 320), y: Math.round(by),
        w: 220, h: 56,
      }
      : null;
    const edges: ProjectArchive["graph"]["edges"] = stepNodes.map((s) => ({ from: key, to: s.key, animated: false, color: KIND_BORDER.intent }));
    if (codeNode) edges.push({ from: key, to: codeNode.key, animated: true, color: KIND_BORDER.intent });
    const all = codeNode ? [...stepNodes, codeNode] : stepNodes;
    patchArchive((cur) => ({
      ...cur,
      graph: {
        ...cur.graph,
        nodes: [...cur.graph.nodes.filter((n) => !n.key.startsWith(`${key}-step`) && n.key !== `${key}-code`), ...all],
        edges: [...cur.graph.edges.filter((e) => e.from !== key || !e.to.startsWith(`${key}-step`)), ...edges],
      },
    }));
    setPod({ kind: "intent", plan });
  }

  // ---------- 5.4 下钻 / 1.3 双向引用 ----------
  function drillDown(nodeKey: string, analysis: FileAnalysis): void {
    const a = archiveRef.current;
    const parent = a?.graph.nodes.find((n) => n.key === nodeKey);
    if (!a || !parent) return;
    const drillPrefix = `pv-fn:${analysis.relPath}:`;
    if (a.graph.nodes.some((n) => n.key.startsWith(drillPrefix))) {
      pushToast("info", lang !== "en" ? "该文件的函数节点已展开" : "Function nodes already expanded");
      return;
    }
    const { nodes, edges } = buildDrillDown(analysis, { key: parent.key, x: parent.x, y: parent.y, width: parent.w }, lang);
    if (nodes.length === 0) {
      // 一.2 强制契约：万物皆可下钻——没有函数/类（HTML/CSS/JSON/MD…）也必须
      // 能进入逐行解剖，禁止用提示阻断；结构分块由 structuralBlocks 兜底。
      if (parent.recordId?.startsWith("pv:")) {
        const body = parent.recordId.slice(3);
        const at2 = body.lastIndexOf("@");
        void openFocus(at2 > 0 ? body.slice(0, at2) : body, at2 > 0 ? body.slice(at2 + 1) : a.root, 2);
        return;
      }
      pushToast("info", lang !== "en" ? "这个文件里没有解析出函数或类" : "No functions/classes parsed");
      return;
    }
    appendNodes(nodes, edges);
    pushToast("success", lang !== "en" ? `已下钻：展开 ${nodes.length} 个节点` : `Drilled in: ${nodes.length} nodes`);
  }

  function referencedSet(): Set<string> {
    const mapId = uiStore.getState().currentMapId;
    return new Set((archive?.refs ?? []).filter((r) => !mapId || r.mapId === mapId).map((r) => r.relPath));
  }

  async function refToMindmap(relPath: string, name: string, label: string): Promise<void> {
    const a = archiveRef.current;
    const mapId = uiStore.getState().currentMapId;
    if (!a) return;
    if (!mapId) {
      pushToast("error", lang !== "en" ? "请先打开一个思维导图" : "Open a mind map first");
      return;
    }
    if (a.refs.some((r) => r.relPath === relPath && r.mapId === mapId)) {
      pushToast("info", lang !== "en" ? "该文件已引用到当前导图" : "Already referenced");
      return;
    }
    const node: MindNode = {
      id: uid(),
      mindmapId: mapId,
      textHtml: `<p>🔗 <strong>${esc(name)}</strong><br><span style="opacity:.75;font-size:12px">${esc(label || relPath)}</span></p>`,
      textPlain: `项目解读 ${relPath}`,
      x: 0,
      y: 0,
      width: 240,
      height: 72,
      shape: "rounded",
      borderRadius: 14,
      borderColor: CROSS_REF_NODE,
      fillColor: "rgba(13,20,38,0.88)",
      fontSize: 12,
      opacity: 1,
      locked: false,
      zIndex: 1,
      recordId: `pv:${relPath}@${a.root}`,
      rotation: 0,
      groupId: null,
      hidden: false,
      collapsed: false,
      preset: "",
      updatedAt: Date.now(),
    };
    try {
      await ipc.saveNodes([node]);
    } catch (e) {
      pushToast("error", lang !== "en" ? "引用节点创建失败" : "Ref node failed", errText(e));
      return;
    }
    patchArchive((cur) => ({
      ...cur,
      refs: [...cur.refs, { id: uid(), kind: "file", relPath, name, mapId, nodeId: node.id, createdAt: Date.now() }],
    }));
    pushToast("success", lang !== "en" ? "已引用到当前思维导图（双击该节点可跳回解读）" : "Referenced into the mind map");
    if (pvSession.filePath) void writeArchive(pvSession.filePath);
  }

  function jumpToMindmapRef(nodeId: string): void {
    uiStore.setState({ mode: "mindmap" });
    window.setTimeout(() => {
      window.dispatchEvent(new CustomEvent("variable:mm-focus-node", { detail: nodeId }));
    }, 140);
  }

  // ---------- archive IO ----------
  async function loadArchiveFile(path: string): Promise<void> {
    try {
      const raw = await ipc.readTextFile(path);
      const parsed = JSON.parse(raw) as ProjectArchive;
      if (!parsed || parsed.formatVersion !== 1 || !parsed.graph) {
        throw new Error(lang !== "en" ? "不是有效的 .project 档案" : "Not a valid .project archive");
      }
      pvSession.model = null; // lazy re-analysis via rootHint
      pvSession.external = new Map();
      setArchive(parsed);
      markSession(parsed, path);
      setFilePath(path);
      requestAnimationFrame(() => fitAll());
      pushToast("success", lang !== "en" ? "项目档案已打开" : "Project archive opened", parsed.root);
    } catch (e) {
      pushToast("error", lang !== "en" ? "档案打开失败" : "Open archive failed", errText(e));
    }
  }

  async function saveArchiveAs(): Promise<void> {
    const a = archiveRef.current;
    if (!a) return;
    const name = a.root.split(/[\\/]/).pop() ?? "project";
    const p = await saveDialog({ defaultPath: `${name}.project`, filters: [{ name: "Project Archive", extensions: ["project"] }] });
    if (typeof p !== "string") return;
    await writeArchive(p);
  }

  async function saveArchive(): Promise<void> {
    if (filePath) await writeArchive(filePath);
    else await saveArchiveAs();
  }

  /** 代码分析结果 → 思维导图：完整导出为可直接打开的 .mindmap 文件。 */
  async function exportGraphMindmap(): Promise<void> {
    const a = archiveRef.current;
    if (!a) return;
    try {
      const g = a.graph;
      const nodes = g.nodes.map((n) => ({
        id: n.key,
        textHtml: n.html || `<p>${escapePvHtml(n.plain)}</p>`,
        x: n.x, y: n.y, width: n.w, height: n.h,
      }));
      const edges = g.edges.map((e, i) => ({
        id: `${e.from}->${e.to}-${i}`,
        sourceNodeId: e.from,
        targetNodeId: e.to,
      }));
      const name = a.root.split(/[\\/]/).pop() ?? "project";
      const p = await saveDialog({ defaultPath: `${name}.mindmap`, filters: [{ name: "Mindmap", extensions: ["mindmap"] }] });
      if (typeof p !== "string") return;
      await ipc.saveTextFile(p, JSON.stringify({ app: "variable-mindmap", formatVersion: 1, name, nodes, edges }, null, 2), true);
      pushToast("success", lang !== "en" ? "分析结果已导出为思维导图" : "Analysis exported as mindmap", p);
    } catch (e) {
      pushToast("error", lang !== "en" ? "导出失败" : "Export failed", errText(e));
    }
  }

  function escapePvHtml(s: string): string {
    return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
  }

  async function writeArchive(path: string): Promise<void> {
    const a = archiveRef.current;
    if (!a) return;
    try {
      const payload: ProjectArchive = { ...a, savedAt: Date.now() };
      await ipc.saveTextFile(path, JSON.stringify(payload), true);
      setArchive(payload);
      setFilePath(path);
      markSession(payload, path);
      pushToast("success", lang !== "en" ? "档案已保存" : "Archive saved", path);
    } catch (e) {
      pushToast("error", lang !== "en" ? "保存失败" : "Save failed", errText(e));
    }
  }

  // ================= 第一章 · 逐行代码解剖（L2/L3/L4 焦点模式） =================

  /** 进入解剖空间：读取源码（缓存优先），默认 L2 结构级。返回焦点供续钻。 */
  async function openFocus(rel: string, root: string, level: 2 | 3 | 4 = 2, symbol?: string): Promise<FocusState | null> {
    const key = `${rel}@${root}`;
    let content = contentCacheRef.current.get(key) ?? null;
    if (content === null) {
      try {
        const src = await ipc.projectReadFile(root, rel);
        content = src.content;
        contentCacheRef.current.set(key, content);
      } catch (e) {
        pushToast("error", lang !== "en" ? "无法读取源码" : "Cannot read source", errText(e));
        return null;
      }
    }
    const analysis = analysisFor(rel);
    const f: FocusState = { rel, root, level, content, lang: analysis?.lang ?? "generic", ...(symbol ? { symbol } : {}) };
    setFocus(f);
    setSelection(new Set());
    return f;
  }

  function analysisFor(rel: string): FileAnalysis | null {
    return pvSession.model?.analyses.get(rel) ?? pvSession.external.get(rel) ?? null;
  }

  /** Esc 逐级返回：L4→L3→L2→全景。 */
  function popFocusLevel(): void {
    const f = focusRef.current;
    if (!f) return;
    if (f.level === 4) setFocus({ ...f, level: 3 });
    else if (f.level === 3) setFocus({ ...f, level: 2 });
    else setFocus(null);
  }

  function analysisOfFocus(f: FocusState): FileAnalysis | null {
    const base = analysisFor(f.rel);
    if (base) return base;
    const fresh = parseSource(f.rel, f.content);
    pvSession.model?.analyses.set(f.rel, fresh);
    pvSession.external.set(f.rel, fresh);
    return fresh;
  }

  /** 1.2 上下文影响：谁调用了这个函数 / 谁引用了这个文件。 */
  function impactFor(f: FocusState, symbol: string | undefined): string[] {
    const analysis = analysisOfFocus(f);
    if (!analysis) return [];
    const out: string[] = [];
    if (symbol) {
      for (const c of analysis.calls) {
        if (c.to === symbol) out.push(`${f.rel} 内的 ${c.from}() 调用它`);
      }
    }
    const importers = pvSession.model?.usedBy.get(f.rel) ?? [];
    for (const imp of importers.slice(0, 3)) out.push(`${imp} 引用了本文件`);
    return out;
  }

  function flash(key: string): void {
    setFlashKeys((prev) => new Set(prev).add(key));
    window.setTimeout(() => {
      setFlashKeys((prev) => {
        const nx = new Set(prev);
        nx.delete(key);
        return nx;
      });
    }, 2000);
  }

  // ---- 第三/四章：意图条确认 → 会话内生效 → 右上角「写入源文件」二段式落盘 ----
  function runInlineIntent(): void {
    const bar = inlineBarRef.current;
    const f = focusRef.current;
    if (!bar || !f) return;
    const text = bar.text.trim();
    if (!text) return;
    // 9.5 逐行个人注释：存项目档案（不碰源文件，随档案迁移）
    if (bar.mode === "note") {
      patchArchive((a) => ({ ...a, notes: { ...a.notes, [`${f.rel}:${bar.line}`]: text } }));
      flash(`${f.rel}:${bar.line}`);
      pushToast("success", lang !== "en" ? "注释已保存到项目档案" : "Note saved to archive");
      setInlineBar(null);
      return;
    }
    const r: IntentResult = applyIntent(text, { content: f.content, lang: f.lang, line: bar.line, relPath: f.rel });
    if (!r.matched) {
      pushToast("error", r.note || (lang !== "en" ? "没有匹配到可执行的改动" : "No applicable change"), r.clarify ?? undefined);
      return;
    }
    // 四章：先改会话内存，不直接落盘——用户确认后经「写入源文件」按钮提交
    const sep = f.root.includes("\\") || /^[A-Za-z]:/.test(f.root) ? "\\" : "/";
    const abs = `${f.root.replace(/[\\/]+$/, "")}${sep}${f.rel.replace(/\//g, sep)}`;
    setPendingWrite({ rel: f.rel, root: f.root, abs, before: f.content, content: r.content, note: r.note, utterance: text, line: r.previews[0]?.startLine ?? bar.line });
    setFocus({ ...f, content: r.content });
    flash(`${f.rel}:${r.previews[0]?.startLine ?? bar.line}`);
    pushToast("success", lang !== "en" ? "已应用到会话——点右上角「写入源文件」落盘" : "Applied in session — use 'Write to source' to persist", r.note);
    setInlineBar(null);
  }

  /** 3.3 落盘：备份先行 → 原子写入 → 记入撤销栈 → 清除待写。 */
  async function writePendingToFile(): Promise<void> {
    const p = pendingWrite;
    if (!p) return;
    try {
      await ipc.saveTextFile(backupPath(p.abs), p.before, true);
      await ipc.saveTextFile(p.abs, p.content, true);
    } catch (e) {
      pushToast("error", lang !== "en" ? "写入失败（文件未改动）" : "Write failed (file untouched)", errText(e));
      return;
    }
    editHistoryRef.current.push({
      absPath: p.abs, relPath: p.rel, before: p.before, after: p.content,
      backupPath: backupPath(p.abs), at: Date.now(), utterance: p.utterance,
    });
    setPendingWrite(null);
    flash(`${p.rel}:${p.line}`);
    pushToast("success", lang !== "en" ? "已写入源文件并自动备份（Ctrl+Z 可回滚）" : "Written & backed up (Ctrl+Z to undo)", p.note);
  }

  /** 4.6 Ctrl+Z：有待写改动先撤销会话内改动；否则回滚上一次真实写入。 */
  function undoLastEdit(): void {
    const p = pendingWrite;
    if (p && focusRef.current?.rel === p.rel) {
      setFocus({ ...focusRef.current, content: p.before });
      setPendingWrite(null);
      pushToast("success", lang !== "en" ? "已撤销会话内改动（尚未写盘）" : "In-session change reverted");
      return;
    }
    const rec = editHistoryRef.current.pop();
    if (!rec) {
      pushToast("info", lang !== "en" ? "没有可回滚的编辑" : "Nothing to undo");
      return;
    }
    void (async () => {
      try {
        await ipc.saveTextFile(rec.absPath, rec.before, true);
        contentCacheRef.current.set(`${rec.relPath}@${focusRef.current?.root ?? ""}`, rec.before);
        if (focusRef.current?.rel === rec.relPath) setFocus({ ...focusRef.current, content: rec.before });
        pushToast("success", lang !== "en" ? "已回滚上一次编辑" : "Last edit reverted", rec.utterance);
      } catch (e) {
        pushToast("error", lang !== "en" ? "回滚失败" : "Undo failed", errText(e));
      }
    })();
  }

  /** 2.2 推理链舱：三条链一次性生成。 */
  function openReasoning(f: FocusState, symbol: string): void {
    const analysis = analysisOfFocus(f);
    if (!analysis) return;
    const chains = buildReasoning(analysis, symbol, pvSession.model?.usedBy.get(f.rel) ?? []);
    if (!chains) {
      pushToast("info", lang !== "en" ? "没有解析出这个函数" : "Symbol not parsed");
      return;
    }
    setPod({ kind: "reasoning", rel: f.rel, symbol, chains });
  }

  // ---- 解剖层右键菜单 ----
  function symbolMenu(e: React.MouseEvent, name: string): void {
    e.preventDefault();
    e.stopPropagation();
    const f = focusRef.current;
    if (!f) return;
    openContextMenu(e.clientX, e.clientY, [
      { label: lang !== "en" ? "查看推理链（三条）" : "Reasoning chains", onClick: () => openReasoning(f, name) },
      { label: lang !== "en" ? "进入段落级（L3）" : "Paragraphs (L3)", onClick: () => setFocus({ ...f, level: 3, symbol: name }) },
    ]);
  }

  function lineMenu(e: React.MouseEvent, card: LineCard): void {
    e.preventDefault();
    e.stopPropagation();
    const f = focusRef.current;
    if (!f) return;
    openContextMenu(e.clientX, e.clientY, [
      { label: lang !== "en" ? "在此处续写…" : "Continue here…", onClick: () => setInlineBar({ mode: "continue", line: card.line, text: "" }) },
      { label: lang !== "en" ? "用大白话改这一行…" : "Change with plain words…", disabled: !card.editable, onClick: () => setInlineBar({ mode: "edit", line: card.line, text: "" }) },
      { label: lang !== "en" ? (archiveRef.current?.notes[`${f.rel}:${card.line}`] ? "编辑个人注释…" : "添加个人注释…") : (archiveRef.current?.notes[`${f.rel}:${card.line}`] ? "Edit note…" : "Add note…"), onClick: () => setInlineBar({ mode: "note", line: card.line, text: archiveRef.current?.notes[`${f.rel}:${card.line}`] ?? "" }) },
      { label: lang !== "en" ? "复制该行" : "Copy line", onClick: () => { xfSet("code-block", card.code, { code: card.code, lang: "" }); void navigator.clipboard.writeText(card.code).then(() => pushToast("success", lang !== "en" ? "已复制" : "Copied")).catch(() => {}); } },
    ]);
  }

  /** 内联意图条实时预览（4.3：Enter 确认，Esc 取消）。 */
  function inlinePreview(): { preview: EditPreview | null; note: string } {
    const bar = inlineBarRef.current;
    const f = focusRef.current;
    if (!bar || !f || !bar.text.trim()) return { preview: null, note: "" };
    const r = applyIntent(bar.text.trim(), { content: f.content, lang: f.lang, line: bar.line, relPath: f.rel });
    return { preview: r.previews[0] ?? null, note: r.note || r.clarify || "" };
  }

  // ---------- 相机缓动（三章：结构超屏靠飞行/漫游查看） ----------
  function animateVp(to: Vp, dur = 260): void {
    cancelAnimationFrame(animRef.current);
    const from = vpRef.current;
    const t0 = performance.now();
    const tick = (now: number): void => {
      const k = Math.min(1, (now - t0) / dur);
      const e2 = 1 - Math.pow(1 - k, 3);
      const v = { z: from.z + (to.z - from.z) * e2, x: from.x + (to.x - from.x) * e2, y: from.y + (to.y - from.y) * e2 };
      vpRef.current = v;
      setVp(v);
      if (k < 1) animRef.current = requestAnimationFrame(tick);
    };
    animRef.current = requestAnimationFrame(tick);
  }

  function flyToNode(n: GenNode): void {
    const z = vpRef.current.z;
    animateVp({ z, x: -(n.x + n.w / 2) * z, y: -(n.y + n.h / 2) * z });
  }

  // ---------- 右键菜单（第七章 交互细则） ----------
  function canvasMenu(x: number, y: number): void {
    openContextMenu(x, y, [
      { label: lang !== "en" ? "导入项目文件夹…" : "Import project folder…", icon: <FolderOpen size={13} />, onClick: () => void pickFolder() },
      { label: lang !== "en" ? "加入文件到分析台…" : "Add files to session…", onClick: () => void pickFiles() },
      { separator: true },
      { label: lang !== "en" ? "用大白话加功能（单击画布放置意图）" : "Plain-language intent (click canvas)", checked: toolMode === "intent", onClick: () => setToolMode(toolMode === "intent" ? "select" : "intent") },
      { separator: true },
      { label: lang !== "en" ? "适应全部" : "Fit all", onClick: fitAll },
      { label: lang !== "en" ? "保存档案" : "Save archive", disabled: !archive, onClick: () => void saveArchive() },
      { label: lang !== "en" ? "档案另存为…" : "Save archive as…", disabled: !archive, onClick: () => void saveArchiveAs() },
    ]);
  }

  function nodeMenu(e: React.MouseEvent, key: string): void {
    const a = archiveRef.current;
    const n = a?.graph.nodes.find((x) => x.key === key);
    if (!n) return;
    e.preventDefault();
    e.stopPropagation();
    setSelection(new Set([key]));
    const items: import("../../components/ContextMenu").MenuItem[] = [];
    if (n.recordId?.startsWith("pv:")) {
      items.push({ label: lang !== "en" ? "通俗说明" : "Explain", onClick: () => setPod({ kind: "file", recordId: n.recordId! }) });
      items.push({
        label: lang !== "en" ? "逐行解剖（L2 结构级）" : "Line anatomy (L2)",
        onClick: () => {
          const body = n.recordId!.slice(3);
          const at2 = body.lastIndexOf("@");
          void openFocus(at2 > 0 ? body.slice(0, at2) : body, at2 > 0 ? body.slice(at2 + 1) : archiveRef.current?.root ?? "", 2);
        },
      });
      const body = n.recordId.slice(3);
      const at = body.lastIndexOf("@");
      const rel = at > 0 ? body.slice(0, at) : body;
      items.push({ label: lang !== "en" ? "引用到思维导图" : "Ref to mindmap", onClick: () => void refToMindmap(rel, rel.split("/").pop() ?? rel, "") });
      items.push({ label: lang !== "en" ? "从分析中移除" : "Remove from session", danger: true, onClick: () => removeNode(key) });
    } else if (n.kind === "intent") {
      items.push({ label: lang !== "en" ? "编辑大白话" : "Edit intent", onClick: () => setEditingKey(key) });
      if (n.plain) items.push({ label: lang !== "en" ? "重新生成" : "Regenerate", onClick: () => generateIntent(key, n.plain) });
      items.push({ label: lang !== "en" ? "删除意图分支" : "Delete intent", danger: true, onClick: () => removeNode(key) });
    } else {
      items.push({ label: lang !== "en" ? "从画布移除" : "Remove from canvas", danger: true, onClick: () => removeNode(key) });
    }
    openContextMenu(e.clientX, e.clientY, items);
  }

  /** 1.4 双通道之点击：系统对话框。失败必有 Toast，禁止静默。 */
  async function pickFolder(): Promise<void> {
    try {
      const sel = await fileDialog({ directory: true, multiple: false, title: lang !== "en" ? "选择项目文件夹" : "Choose project folder" });
      if (typeof sel === "string" && sel) await importProjectFolder(sel);
    } catch (e) {
      pushToast("error", lang !== "en" ? "打开文件夹选择器失败" : "Folder dialog failed", errText(e));
    }
  }

  async function pickFiles(): Promise<void> {
    try {
      const sel = await fileDialog({ multiple: true, title: lang !== "en" ? "加入文件到分析台" : "Add files" });
      const paths = Array.isArray(sel) ? sel : typeof sel === "string" ? [sel] : [];
      if (paths.length > 0) await importLooseFiles(paths);
    } catch (e) {
      pushToast("error", lang !== "en" ? "打开文件选择器失败" : "File dialog failed", errText(e));
    }
  }

  // ---------- derived ----------
  const nodeByKey = useMemo(() => new Map((archive?.graph.nodes ?? []).map((n) => [n.key, n])), [archive]);
  const treeRows = useMemo(() => {
    const a = archive;
    if (!a) return [] as Array<{ rel: string; isExternal: boolean }>;
    const f = treeFilter.trim().toLowerCase();
    const out: Array<{ rel: string; isExternal: boolean }> = [];
    const entries = a.entries ?? [];
    if (f) {
      for (const e of entries) {
        if (e.kind === "file" && e.path.toLowerCase().includes(f)) out.push({ rel: e.path, isExternal: false });
      }
    } else {
      for (const e of entries) {
        if (e.kind === "file" && e.depth <= 1) out.push({ rel: e.path, isExternal: false });
      }
    }
    for (const x of a.external ?? []) {
      if (!f || x.rel.toLowerCase().includes(f)) out.push({ rel: x.rel, isExternal: true });
    }
    return out.slice(0, 400);
  }, [archive, treeFilter]);
  const rootName = archive ? archive.root.split(/[\\/]/).pop() ?? archive.root : "";
  const pinnedCount = (archive?.graph.nodes.filter((n) => n.recordId?.startsWith("pv:")).length ?? 0);

  // ================= 解剖层渲染（1.1 四级缩放的 L2/L3/L4） =================
  function renderAnatomy(f: FocusState): React.ReactElement {
    const analysis = analysisOfFocus(f);
    const symbols = (analysis?.symbols ?? []).filter((s) => s.kind === "function" || s.kind === "class").slice(0, 40);
    // 一.2 万物皆可下钻：无函数/类时用结构分块（DOM 块/选择器/键值树/标题/段落）
    const blocks = symbols.length === 0 ? structuralBlocks(f.rel, f.content, f.lang).slice(0, 40) : [];
    const sym = f.symbol ? symbols.find((s) => s.name === f.symbol) : undefined;
    const bodyStart = sym ? sym.line - 1 : 0;
    const bodyEnd = sym ? sym.endLine : Math.min(f.content.split("\n").length, L4_LINE_HARD_CAP);
    const bodyLines = f.content.split("\n").slice(bodyStart, bodyEnd);

    return (
      <div className="pv-anatomy" key={`${f.rel}-${f.level}-${f.symbol ?? ""}`}>
        <div className="pv-crumb-bar">
          <button type="button" className="btn tiny ghost" onClick={popFocusLevel}><ArrowLeft size={11} /> {lang !== "en" ? "返回上一层" : "Back"}</button>
          {/* 6.2 可点面包屑：项目根 › 文件 › 函数 › 层级 */}
          <button type="button" className="btn tiny ghost" onClick={() => setFocus(null)}>{rootName}</button>
          <span className="pv-crumb">▸</span>
          <button type="button" className="btn tiny ghost" onClick={() => setFocus({ ...f, level: 2, symbol: undefined })}>{f.rel.split("/").pop()}</button>
          {sym && (
            <>
              <span className="pv-crumb">▸</span>
              <button type="button" className="btn tiny ghost" onClick={() => setFocus({ ...f, level: 3 })}>{sym.name}()</button>
            </>
          )}
          <span className="pv-crumb">▸ {f.level === 2 ? "L2 结构" : f.level === 3 ? "L3 段落" : "L4 逐行"}</span>
        </div>
        <div className="pv-anatomy-col">
          {f.level === 2 && (
            <>
              <p className="pv-anatomy-hint">{lang !== "en" ? "L2 结构级：双击进入下级；右键函数看推理链" : "L2: double-click to drill; right-click functions for chains"}</p>
              {symbols.length === 0 && blocks.length === 0 && (
                <p className="dim pad8">{lang !== "en" ? "文件为空。" : "Empty file."}</p>
              )}
              {symbols.map((s) => (
                <div
                  key={s.name}
                  className="pv-line-node"
                  onDoubleClick={() => setFocus({ ...f, level: 3, symbol: s.name })}
                  onContextMenu={(e) => symbolMenu(e, s.name)}
                >
                  <div className="pv-line-code">
                    <span className="pv-badge">{s.kind === "class" ? "图纸" : "机器"}</span>
                    <strong>{s.name}{s.kind === "function" ? "()" : ""}</strong>
                    <span className="dim small"> L{s.line}</span>
                  </div>
                  <div className="pv-line-explain">{s.params.length > 0 ? `原材料：${s.params.join("、")}` : (lang !== "en" ? "不需要原材料" : "no input")}</div>
                </div>
              ))}
              {blocks.map((b) => (
                <div
                  key={`b${b.line}`}
                  className="pv-line-node"
                  onDoubleClick={() => setFocus({ ...f, level: 4 })}
                  title={b.kindLabel}
                >
                  <div className="pv-line-code">
                    <span className="pv-badge">{lang !== "en" ? "块" : "blk"}</span>
                    <strong>{b.name}</strong>
                    <span className="dim small"> L{b.line}–{b.endLine}</span>
                  </div>
                  <div className="pv-line-explain"><p>{b.kindLabel}</p></div>
                </div>
              ))}
            </>
          )}
          {f.level === 3 && sym && (
            <>
              <p className="pv-anatomy-hint">{lang !== "en" ? `L3 段落级：${sym.name}() 的逻辑分块——双击进入逐行（L4）` : "L3 paragraphs — double-click for line view"}</p>
              {chunkBody(bodyLines).map((ch) => (
                <div
                  key={ch.title}
                  className="pv-line-node"
                  onDoubleClick={() => setFocus({ ...f, level: 4 })}
                  onContextMenu={(e) => symbolMenu(e, sym.name)}
                >
                  <div className="pv-line-code"><span className="pv-badge">{lang !== "en" ? "段落" : "chunk"}</span><strong>{ch.title}</strong> <span className="dim small">L{bodyStart + ch.start}</span></div>
                  <div className="pv-line-explain mono">{ch.lines.slice(0, 3).map((l) => l.trim()).filter(Boolean).join(" · ").slice(0, 120)}</div>
                </div>
              ))}
            </>
          )}
          {(f.level === 4 || (f.level === 3 && !sym)) && (
            <>
              <p className="pv-anatomy-hint">
                {lang !== "en" ? "L4 逐行级：左代码右大白话；双击大白话反写，点击函数行折叠 · " : "L4 dual-track · "}
                <button type="button" className="btn tiny ghost" onClick={() => setCollapsedSyms(new Set(symbols.map((s) => s.name)))}>{lang !== "en" ? "折叠所有函数" : "Fold all"}</button>
                <button type="button" className="btn tiny ghost" onClick={() => setCollapsedSyms(new Set())}>{lang !== "en" ? "展开所有" : "Expand all"}</button>
              </p>
              {renderL4Virtual(f)}
            </>
          )}
        </div>
      </div>
    );
  }

  /** 2.5 通俗风格切换（持久化到本地）。 */
  const [narrateStyle, setNarrateStyle] = useState<NarrateStyle>(() => {
    const v = localStorage.getItem("pvzStyle");
    return v === "story" || v === "engineering" ? v : "metaphor";
  });
  const changeStyle = (s: NarrateStyle): void => {
    setNarrateStyle(s);
    try { localStorage.setItem("pvzStyle", s); } catch { /* ignore */ }
  };
  /** 一.2 七级下钻：函数/逻辑块行模型（含 Level 6 逻辑块折叠 + Level 7 逐行）。 */
  type VRow =
    | { kind: "sym"; name: string; isClass: boolean; line: number; goal: string }
    | { kind: "block"; name: string; line: number; endLine: number }
    | { kind: "line"; card: LineCard }
    | { kind: "more"; text: string };

  function buildL4Rows(f: FocusState): VRow[] {
    const analysis = analysisOfFocus(f);
    const syms = (analysis?.symbols ?? []).filter((s) => s.kind === "function" || s.kind === "class").slice(0, 60);
    const lines = f.content.split("\n");
    const starts = new Map<number, (typeof syms)[number]>();
    for (const s of syms) starts.set(s.line, s);
    const ownerOf = (n: number): (typeof syms)[number] | undefined =>
      syms.find((s) => n > s.line && n <= Math.min(s.endLine, s.line + L4_LINE_HARD_CAP));
    const ranges = blockRanges(lines, f.lang);
    const rows: VRow[] = [];
    const cap = Math.min(lines.length, L4_LINE_HARD_CAP);
    let foldedUntil = 0;
    let n = 1;
    while (n <= cap) {
      const s = starts.get(n);
      if (s) {
        rows.push({ kind: "sym", name: s.name, isClass: s.kind === "class", line: s.line, goal: inferGoal(s.name) });
        n++;
        continue;
      }
      if (n <= foldedUntil) { n++; continue; }
      // Level 6：逻辑块（if/for/while/try…）可折叠，折叠后只留大白话总结
      const bEnd = ranges.get(n);
      if (bEnd !== undefined && bEnd > n) {
        const folded = collapsedSyms.has(`B${n}`);
        rows.push({ kind: "block", name: (lines[n - 1] ?? "").trim().slice(0, 60), line: n, endLine: bEnd });
        n++;
        if (folded) foldedUntil = bEnd;
        continue;
      }
      const owner = ownerOf(n);
      if (owner && collapsedSyms.has(owner.name)) { n++; continue; }
      const card = explainLine(lines[n - 1] ?? "", f.lang, n, { impact: impactFor(f, owner?.name) });
      rows.push({ kind: "line", card });
      n++;
    }
    if (lines.length > cap) {
      rows.push({ kind: "more", text: lang !== "en" ? `仅展开前 ${cap} 行（其余 ${lines.length - cap} 行）` : `First ${cap} lines shown` });
    }
    return rows;
  }

  const ROW_H = 52;
  const V_VISIBLE = 30;

  /** 二章 · 大文件虚拟化视口：只渲染可视区 ~30 行，逻辑高度按真实行数计。 */
  function renderL4Virtual(f: FocusState): React.ReactElement {
    const rows = buildL4Rows(f);
    const start = Math.max(0, Math.floor(vScrollTop / ROW_H) - 4);
    const slice = rows.slice(start, start + V_VISIBLE + 8);
    return (
      <div
        className="pv-vlist"
        onScroll={(e) => setVScrollTop(e.currentTarget.scrollTop)}
      >
        <div style={{ height: rows.length * ROW_H, position: "relative" }}>
          {slice.map((row, i) => {
            const top = (start + i) * ROW_H;
            if (row.kind === "sym" || row.kind === "block") {
              const isSym = row.kind === "sym";
              const foldKey = isSym ? row.name : `B${row.line}`;
              const folded = collapsedSyms.has(foldKey);
              return (
                <div key={`${row.kind}${foldKey}`} className="pv-line-node sym" style={{ top, height: ROW_H - 8 }}
                  onClick={() => {
                    setCollapsedSyms((prev) => {
                      const nx = new Set(prev);
                      if (nx.has(foldKey)) nx.delete(foldKey); else nx.add(foldKey);
                      return nx;
                    });
                  }}
                  onContextMenu={(e) => isSym ? symbolMenu(e, row.name) : undefined}
                >
                  <div className="pv-line-code">
                    <span className="pv-fold">{folded ? "▸" : "▾"}</span>
                    <span className="pv-badge">{isSym ? (row.isClass ? "图纸" : "机器") : "逻辑块"}</span>
                    <strong>{isSym ? `${row.name}${row.isClass ? "" : "()"}` : row.name}</strong>
                    <span className="dim small"> L{row.line}</span>
                  </div>
                  <div className="pv-line-explain" title={lang !== "en" ? "点击折叠/展开" : "click to fold"}>
                    <p>{isSym ? row.goal : (lang !== "en" ? `看情况办事/重复劳动的一段（至 L${row.endLine}），折叠可只留总结` : `conditional/loop block to L${row.endLine}`)}{folded ? (lang !== "en" ? "（已折叠）" : " (folded)") : ""}</p>
                  </div>
                </div>
              );
            }
            if (row.kind === "more") {
              return (
                <div key={`m${start + i}`} className="pv-line-node more" style={{ top, height: ROW_H - 8 }}>{row.text}</div>
              );
            }
            const card = row.card;
            const key = `${f.rel}:${card.line}`;
            return (
              <div key={key} className={`pv-line-node row ${flashKeys.has(key) ? "pv-flash" : ""}`} style={{ top, height: ROW_H - 8 }}
                onContextMenu={(e) => lineMenu(e, card)}
              >
                <div className="pv-line-code mono" title={card.terms.map((t) => `${t.term}：${t.explain}`).join("\n") || undefined}>
                  <span className="pv-ln">{card.line}</span>
                  <span dangerouslySetInnerHTML={{ __html: highlight(card.code) }} />
                </div>
                <div
                  className="pv-line-explain"
                  onDoubleClick={() => setInlineBar({ mode: "edit", line: card.line, text: "" })}
                  title={lang !== "en" ? "双击大白话 → 用自然语言反向修改这一行" : "double-click to edit this line with plain words"}
                >
                  <p>{card.plain || " "}</p>
                  {archiveRef.current?.notes[`${f.rel}:${card.line}`] && (
                    <p className="pv-note-chip">📝 {archiveRef.current.notes[`${f.rel}:${card.line}`]}</p>
                  )}
                  <p className="why">{card.why}</p>
                  {card.impact.length > 0 && <p className="impact">{lang !== "en" ? "影响：" : "Impact: "}{card.impact.join("；")}</p>}
                  {!card.editable && <span className="pv-badge warn">{lang !== "en" ? "慎重" : "caution"}</span>}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="project-view">
      {/* ---------- 一、真实光影与绚烂极光引擎（WebGL2 背景） ---------- */}
      <AuroraCanvas safeMode={props.settings.safeMode} reduceMotion={props.settings.reduceMotion} />
      {/* ---------- 顶栏（复用主软件按钮语言，无第二标题栏） ---------- */}
      <div className="pv-toolbar">
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "返回写作" : "Back to writing"} onClick={() => uiStore.setState({ mode: "write" })}>
          <PenLine size={14} />
        </button>
        <span className="pv-title"><FolderSearch size={14} /> {lang !== "en" ? "项目分析" : "Project"}</span>
        <span className="pv-crumb" title={archive?.root ?? ""}>
          {archive ? `${rootName} ▸ ${lang !== "en" ? "项目根" : "root"}` : (lang !== "en" ? "未导入项目" : "No project")}
        </span>
        {archive && <span className="pv-crumb dim">{lang !== "en" ? `${pinnedCount} 个文件已纳入` : `${pinnedCount} file(s) in session`}</span>}
        <span className="flex-1" />
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "文件树" : "File tree"} onClick={() => setTreeOpen(!treeOpen)}>
          {treeOpen ? <PanelLeftClose size={14} /> : <PanelLeftOpen size={14} />}
        </button>
        <button type="button" className={`icon-btn tiny ${toolMode === "intent" ? "active" : ""}`} data-tip={lang !== "en" ? "用大白话加功能（单击画布放置意图节点）" : "Plain-language intent"}
          onClick={() => setToolMode(toolMode === "intent" ? "select" : "intent")}>
          <Lightbulb size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "导入/重新扫描项目文件夹" : "Import / rescan folder"} onClick={() => void pickFolder()}>
          <FolderOpen size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "加入文件到分析台" : "Add files"} onClick={() => void pickFiles()}>
          <Pin size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "导出为思维导图 (.mindmap)" : "Export as mindmap"} disabled={!archive} onClick={() => void exportGraphMindmap()}>
          <GitBranch size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "保存档案 (.project)" : "Save archive (.project)"} disabled={!archive} onClick={() => void saveArchive()}>
          <Save size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "档案另存为" : "Save archive as"} disabled={!archive} onClick={() => void saveArchiveAs()}>
          <FileDown size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "重新扫描" : "Rescan"} disabled={!archive} onClick={() => void importProjectFolder(archive!.root)}>
          <RefreshCw size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "适应全部" : "Fit all"} disabled={!archive} onClick={fitAll}>
          <Maximize2 size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "撤销上一次代码写入（Ctrl+Z）" : "Undo last write (Ctrl+Z)"} onClick={undoLastEdit}>
          <Undo2 size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang !== "en" ? "字典管理（L1-L5）" : "Dictionary manager"} onClick={() => setPod({ kind: "dict" })}>
          <BookOpen size={14} />
        </button>
        {pendingWrite && (
          <button type="button" className="btn tiny primary pv-save-btn" onClick={() => void writePendingToFile()}>
            <Save size={12} /> {lang !== "en" ? "写入源文件" : "Write to source"}
          </button>
        )}
        <button type="button" className={`icon-btn tiny ${refsOpen ? "active" : ""}`} data-tip={lang === "en" ? "Cross references" : "雙向引用"} disabled={!archive} onClick={() => setRefsOpen(!refsOpen)}>
          <Link2 size={14} />
        </button>
        <span className="zoom-pill">{Math.round(vp.z * 100)}%</span>
        <select
          className="pv-style-select"
          value={narrateStyle}
          aria-label={lang !== "en" ? "通俗风格" : "Narration style"}
          onChange={(e) => changeStyle(e.target.value as NarrateStyle)}
        >
          <option value="metaphor">{lang !== "en" ? "生活比喻" : "Metaphor"}</option>
          <option value="story">{lang !== "en" ? "故事叙事" : "Story"}</option>
          <option value="engineering">{lang !== "en" ? "工程说明" : "Engineering"}</option>
        </select>
      </div>

      {/* ---------- 第八章：顶栏微进度（不挡画布） ---------- */}
      <ProjectImportOverlay state={pvImport} onCancel={() => setPvImport(null)} />

      <div className="pv-body">
        {/* ---------- 左：文件树（可折叠，非弹窗） ---------- */}
        {treeOpen && (
          <div className="pv-tree">
            <div className="pv-tree-head">
              <input
                className="pv-tree-filter"
                value={treeFilter}
                placeholder={lang !== "en" ? "筛选文件…" : "Filter files…"}
                onChange={(e) => setTreeFilter(e.target.value)}
                onKeyDown={(e) => e.stopPropagation()}
              />
            </div>
            <div className="pv-tree-list">
              {treeRows.length === 0 && (
                <p className="dim small pad8">{archive ? (lang !== "en" ? "（无匹配文件）" : "(no match)") : (lang !== "en" ? "导入项目后显示文件树" : "Import a project first")}</p>
              )}
              {treeRows.map((row) => (
                <div
                  key={row.rel}
                  className={`pv-tree-row ${treeSel.has(row.rel) ? "sel" : ""}`}
                  title={row.rel}
                  onClick={(e) => {
                    setTreeSel((prev) => {
                      const nx = new Set(prev);
                      if (e.ctrlKey || e.metaKey) {
                        if (nx.has(row.rel)) nx.delete(row.rel); else nx.add(row.rel);
                      } else {
                        nx.clear();
                        nx.add(row.rel);
                      }
                      return nx;
                    });
                  }}
                  onDoubleClick={() => {
                    // 三章：树↔画布联动——画布已有该文件节点则相机飞向它（不靠裁切一屏）
                    let recordId: string;
                    if (row.isExternal) {
                      const abs = (archive?.external ?? []).find((x) => x.rel === row.rel)?.absPath ?? "";
                      const parent = abs.split(/[\\/]/).slice(0, -1).join("/");
                      recordId = `pv:${row.rel.split("/").pop()}@${parent}`;
                    } else {
                      recordId = `pv:${row.rel}@${archive?.root ?? ""}`;
                    }
                    setPod({ kind: "file", recordId });
                    const node = archiveRef.current?.graph.nodes.find((n) => n.recordId === recordId);
                    if (node) flyToNode(node);
                  }}
                >
                  <span className="pv-tree-name ellipsis">{row.rel.split("/").pop()}</span>
                  {row.isExternal && <span className="pv-tag">{lang !== "en" ? "外" : "ext"}</span>}
                </div>
              ))}
            </div>
            <div className="pv-tree-foot">
              <button type="button" className="btn tiny ghost" disabled={treeSel.size === 0} onClick={pinSelected}>
                <Pin size={11} /> {lang !== "en" ? "固定到画布" : "Pin"}
              </button>
              <span className="dim small">{treeSel.size} {lang !== "en" ? "选中" : "sel"}</span>
            </div>
          </div>
        )}

        {/* ---------- 中：无限画布（世界坐标无限延伸，相机 pan+zoom 漫游） ---------- */}
        <div
          ref={containerRef}
          className={`pv-canvas ${toolMode === "intent" ? "intent-mode" : ""} ${dropHover ? "drop-hover" : ""} ${panActive ? "panning" : ""}`}
          tabIndex={-1}
          onPointerDown={onCanvasPointerDown}
          onPointerMove={onCanvasPointerMove}
          onPointerUp={onCanvasPointerUp}
          onContextMenu={(e) => {
            if ((e.target as HTMLElement).closest(".pv-node, .pv-tree, .pv-pod")) return;
            e.preventDefault();
            canvasMenu(e.clientX, e.clientY);
          }}
        >
          {!archive && !focus && (
            <div className={`pv-empty ${dropHover ? "hot" : ""}`}>
              <p>{lang !== "en" ? "拖入项目文件夹或源文件开始分析" : "Drop a project folder or source files to analyze"}</p>
              <button type="button" data-interactive className="btn ghost" disabled={!!pvImport} onClick={() => void pickFolder()}>
                <FolderOpen size={14} /> {lang !== "en" ? "导入项目或文件" : "Import project or files"}
              </button>
            </div>
          )}
          {focus && renderAnatomy(focus)}
          {!focus && archive && (
            <div className="pv-world" style={{ transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.z})`, transformOrigin: "0 0" }}>
              <svg className="pv-edges" width={1} height={1}>
                {(() => {
                  // 2.4 视口剔除：屏外节点与其连线不绘（相机外 pad 240）
                  const el = containerRef.current;
                  const cw = el?.clientWidth ?? 1200;
                  const ch = el?.clientHeight ?? 800;
                  const wx0 = (-cw / 2 - vp.x) / vp.z - 240;
                  const wy0 = (-ch / 2 - vp.y) / vp.z - 240;
                  const wx1 = (cw / 2 - vp.x) / vp.z + 240;
                  const wy1 = (ch / 2 - vp.y) / vp.z + 240;
                  const visible = (n: GenNode): boolean => n.x + n.w > wx0 && n.x < wx1 && n.y + n.h > wy0 && n.y < wy1;
                  return archive.graph.edges.map((e, i) => {
                    const s = nodeByKey.get(e.from);
                    const t = nodeByKey.get(e.to);
                    if (!s || !t) return null;
                    if (!visible(s) && !visible(t)) return null;
                    const hot = hoverId !== null && (e.from === hoverId || e.to === hoverId);
                    return (
                      <path
                        key={i}
                        className={`pv-edge ${e.animated && !props.settings.safeMode && !panActive ? "pv-edge-anim" : ""} ${hot ? "pv-edge-hot" : ""}`}
                        d={routeEdge(s, t, archive.graph.nodes)}
                        stroke={e.color}
                      />
                    );
                  });
                })()}
              </svg>
              {(() => {
                // 2.4 节点剔除：世界无限延伸，只挂载相机视口附近（pad 240）
                const el = containerRef.current;
                const cw = el?.clientWidth ?? 1200;
                const ch = el?.clientHeight ?? 800;
                const wx0 = (-cw / 2 - vp.x) / vp.z - 240;
                const wy0 = (-ch / 2 - vp.y) / vp.z - 240;
                const wx1 = (cw / 2 - vp.x) / vp.z + 240;
                const wy1 = (ch / 2 - vp.y) / vp.z + 240;
                return archive.graph.nodes
                  .filter((n) => n.x + n.w > wx0 && n.x < wx1 && n.y + n.h > wy0 && n.y < wy1)
                  .map((n) => (
                <div
                  key={n.key}
                  className={`pv-node k-${n.kind} ${selection.has(n.key) ? "sel" : ""} ${editingKey === n.key ? "editing" : ""} ${flashKeys.has(n.key) ? "pv-flash" : ""}`}
                  style={{
                    left: n.x, top: n.y, width: n.w, minHeight: n.h,
                    borderColor: KIND_BORDER[n.kind],
                  }}
                  onMouseEnter={() => setHoverId(n.key)}
                  onMouseLeave={() => setHoverId(null)}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (toolMode === "intent") return;
                    setSelection(new Set([n.key]));
                  }}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    if (n.kind === "intent") { setEditingKey(n.key); return; }
                    // 一.2 七级下钻：函数节点继续深入（L3 结构 + 推理链舱）
                    if (n.recordId?.startsWith("pv-fn:")) {
                      const parts = n.recordId.slice(6).split(":");
                      const rel = parts[0] ?? "";
                      const fnName = parts[1] ?? "";
                      void (async () => {
                        const f2 = await openFocus(rel, archiveRef.current?.root ?? "", 3, fnName);
                        if (f2) openReasoning(f2, fnName);
                      })();
                      return;
                    }
                    if (n.recordId?.startsWith("pv:")) setPod({ kind: "file", recordId: n.recordId });
                  }}
                  onContextMenu={(e) => nodeMenu(e, n.key)}
                >
                  {editingKey === n.key ? (
                    <div
                      className="pv-intent-editor"
                      contentEditable
                      suppressContentEditableWarning
                      autoFocus
                      onBlur={(e) => commitIntentEdit(n.key, e.currentTarget.textContent ?? "")}
                      onKeyDown={(e) => {
                        e.stopPropagation();
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          commitIntentEdit(n.key, e.currentTarget.textContent ?? "");
                        } else if (e.key === "Escape") {
                          e.preventDefault();
                          setEditingKey(null);
                        }
                      }}
                    >{n.plain}</div>
                  ) : (
                    <div dangerouslySetInnerHTML={{ __html: n.html }} />
                  )}
                  {editingKey === n.key && (
                    <div className="pv-intent-hint">{lang !== "en" ? "Enter 生成 · Esc 取消" : "Enter to generate · Esc to cancel"}</div>
                  )}
                </div>
                  ));
              })()}
            </div>
          )}

          {/* ---------- 引用面板 ---------- */}
          {archive && refsOpen && (
            <div className="pv-refs card-pop">
              <div className="pv-info-head">
                <Link2 size={13} />
                <strong>{lang !== "en" ? "双向引用管道" : "Cross references"}</strong>
                <button type="button" className="icon-btn tiny pv-close" aria-label="close" onClick={() => setRefsOpen(false)}>✕</button>
              </div>
              {archive.refs.length === 0 && (
                <p className="dim small pad8">
                  {lang !== "en"
                    ? "在文件节点右键「引用到思维导图」建立樱花粉引用；双击导图节点跳回此处。"
                    : "Right-click a file node → \"Ref to mindmap\"; double-click the ref node in the map to jump back."}
                </p>
              )}
              {archive.refs.map((r) => (
                <div key={r.id} className="pv-ref-row">
                  <span className="pv-ref-dot" style={{ background: CROSS_REF_COLOR }} />
                  <span className="ellipsis" title={r.relPath}>{r.relPath}</span>
                  <button type="button" className="btn tiny ghost" onClick={() => jumpToMindmapRef(r.nodeId)}>
                    {lang !== "en" ? "跳转导图" : "Jump"}
                  </button>
                  <button type="button" className="icon-btn tiny" aria-label="remove" onClick={() => patchArchive((a) => ({ ...a, refs: a.refs.filter((x) => x.id !== r.id) }))}>
                    <Trash2 size={11} />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* ---------- 二.2 右下角小地图：全局缩略 + 视口红框 + 点击跳转 ---------- */}
          {!focus && archive && archive.graph.nodes.length > 0 && (
            <MiniMap
              nodes={archive.graph.nodes}
              vp={vp}
              view={{ w: containerRef.current?.clientWidth ?? 1200, h: containerRef.current?.clientHeight ?? 800 }}
              onJump={(wx, wy) => animateVp({ z: vp.z, x: -wx * vp.z, y: -wy * vp.z })}
            />
          )}

          {/* ---------- 底栏轻提示 ---------- */}
          <div className="pv-status card-pop">
            <span>{focus ? `L${focus.level}` : (archive ? archive.graph.nodes.length : 0)}{focus ? "" : ` ${lang !== "en" ? "节点" : "nodes"}`}</span>
            <span className="sep">·</span>
            <span style={{ color: CROSS_REF_COLOR }}>{archive?.refs.length ?? 0} {lang !== "en" ? "引用" : "refs"}</span>
            <span className="sep">·</span>
            <span>{toolMode === "intent" ? (lang !== "en" ? "意图模式：单击画布放置" : "Intent mode: click canvas") : `${Math.round(vp.z * 100)}%`}</span>
            <span className="sep">·</span>
            <span className="ellipsis">{focus ? focus.rel : (archive?.root ?? "")}</span>
          </div>

          {/* ---------- 4.3 内联意图条（非弹窗，贴附画布底部） ---------- */}
          {inlineBar && (
            <div className="pv-inline-bar card-pop">
              <span className="pv-badge">{inlineBar.mode === "continue" ? (lang !== "en" ? `在第 ${inlineBar.line} 行后续写` : `after L${inlineBar.line}`) : inlineBar.mode === "note" ? (lang !== "en" ? `给第 ${inlineBar.line} 行加个人注释` : `note L${inlineBar.line}`) : (lang !== "en" ? `改第 ${inlineBar.line} 行` : `edit L${inlineBar.line}`)}</span>
              <input
                autoFocus
                value={inlineBar.text}
                placeholder={inlineBar.mode === "note" ? (lang !== "en" ? "写下你对这一行的理解…" : "your note for this line…") : (lang !== "en" ? "大白话，如：加个日志写上“用户尝试登录” / 改成蓝色 / 把 userName 改名叫 userId" : "e.g. add a log / change to blue / rename x to y")}
                onChange={(e) => setInlineBar({ ...inlineBar, text: e.target.value })}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") runInlineIntent();
                  if (e.key === "Escape") setInlineBar(null);
                }}
              />
              {(() => {
                const { preview, note } = inlinePreview();
                if (preview) {
                  return (
                    <span className="pv-inline-preview" title={preview.after}>
                      {lang !== "en" ? "预览：" : "Preview: "}{preview.before ? `第 ${preview.startLine} 行` : `第 ${preview.startLine} 行后`} · {preview.note}
                    </span>
                  );
                }
                return note ? <span className="pv-inline-preview dim">{note}</span> : null;
              })()}
              <button type="button" className="btn tiny primary" onClick={runInlineIntent}>Enter</button>
              <button type="button" className="btn tiny ghost" onClick={() => setInlineBar(null)}>Esc</button>
            </div>
          )}
        </div>

        {/* ---------- 右：详情舱 ---------- */}
        {pod?.kind === "file" && (
          <div className="pv-pod">
            <FileInfoCard
              node={shimNode(pod.recordId)}
              model={pvSession.model}
              onClose={() => setPod(null)}
              onDrill={(analysis) => {
                const a = archiveRef.current;
                const key = a?.graph.nodes.find((n) => n.recordId === pod.recordId)?.key;
                if (key) drillDown(key, analysis);
              }}
              onRefToMindmap={(rel, name, label) => void refToMindmap(rel, name, label)}
              referenced={referencedSet()}
              style={narrateStyle}
              onOpenAnatomy={() => {
                const body = pod.recordId.slice(3);
                const at2 = body.lastIndexOf("@");
                void openFocus(at2 > 0 ? body.slice(0, at2) : body, at2 > 0 ? body.slice(at2 + 1) : archiveRef.current?.root ?? "", 4);
              }}
            />
          </div>
        )}
        {pod?.kind === "intent" && (
          <IntentPod
            plan={pod.plan}
            rootPath={archive?.root ?? null}
            noProjectHint={!archive}
            onImportFirst={() => void pickFolder()}
            onClose={() => setPod(null)}
          />
        )}
        {pod?.kind === "reasoning" && (
          <div className="pv-pod pv-intent-pod">
            <div className="pv-info-head">
              <Lightbulb size={13} />
              <strong>{pod.symbol}() · {lang !== "en" ? "三条推理链" : "reasoning chains"}</strong>
              <button type="button" className="icon-btn tiny pv-close" aria-label="close" onClick={() => setPod(null)}>✕</button>
            </div>
            <div className="pv-info-body">
              <section>
                <h4>{lang !== "en" ? "纵向推理链（从目标到实现）" : "Vertical"}</h4>
                <p>{lang !== "en" ? "目标：" : "Goal: "}{pod.chains.vertical.goal}</p>
                <ol className="pv-list pv-steps">
                  {pod.chains.vertical.steps.map((s, i) => (
                    <li key={i}>{s.title} —— {s.detail}{s.line !== undefined ? `（L${s.line}）` : ""}</li>
                  ))}
                </ol>
              </section>
              <section>
                <h4>{lang !== "en" ? "横向依赖链（与其他函数的关系）" : "Horizontal"}</h4>
                <ul className="pv-list">
                  {pod.chains.horizontal.deps.map((s, i) => <li key={`d${i}`}>→ {s.title}：{s.detail}</li>)}
                  {pod.chains.horizontal.usedBy.map((s, i) => <li key={`u${i}`}>← {s.title}：{s.detail}</li>)}
                  {pod.chains.horizontal.peers.length > 0 && (
                    <li key="p">{lang !== "en" ? "平级协作：" : "Peers: "}{pod.chains.horizontal.peers.join("、")}</li>
                  )}
                </ul>
              </section>
              <section>
                <h4>{lang !== "en" ? "时间因果链（先什么再什么最后什么）" : "Temporal"}</h4>
                <p>{lang !== "en" ? "入口：" : "Entry: "}{pod.chains.temporal.entry}</p>
                <ol className="pv-list pv-steps">
                  {pod.chains.temporal.steps.map((s, i) => (
                    <li key={i}>{s.title}{s.line !== undefined ? `（L${s.line}）` : ""}</li>
                  ))}
                </ol>
                <p>{pod.chains.temporal.result}</p>
              </section>
            </div>
          </div>
        )}
        {pod?.kind === "dict" && (
          <DictPod onClose={() => setPod(null)} />
        )}
      </div>
    </div>
  );
}

// ---------- 意图生成详情舱（右侧，非弹窗） ----------

function IntentPod(props: {
  plan: IntentPlan;
  rootPath: string | null;
  noProjectHint: boolean;
  onImportFirst: () => void;
  onClose: () => void;
}): React.ReactElement {
  const { lang } = useI18n();
  const p = props.plan;
  async function copyCode(): Promise<void> {
    if (!p.code) return;
    try {
      await navigator.clipboard.writeText(p.code);
      pushToast("success", lang !== "en" ? "代码已复制" : "Code copied");
    } catch {
      pushToast("error", lang !== "en" ? "复制失败" : "Copy failed");
    }
  }
  async function writeToFile(): Promise<void> {
    if (!p.code || !props.rootPath || !p.targetFile) return;
    try {
      const sep = props.rootPath.includes("\\") || /^[A-Za-z]:/.test(props.rootPath) ? "\\" : "/";
      const path = `${props.rootPath.replace(/[\\/]+$/, "")}${sep}${p.targetFile.replace(/\//g, sep)}`;
      await ipc.saveTextFile(path, p.code, false);
      pushToast("success", lang !== "en" ? "已创建新文件（不覆盖已有文件）" : "New file created (never overwrites)", path);
    } catch (e) {
      pushToast("error", lang !== "en" ? "写入失败" : "Write failed", errText(e));
    }
  }
  return (
    <div className="pv-pod pv-intent-pod">
      <div className="pv-info-head">
        <Lightbulb size={13} />
        <strong>{p.title}{p.matched ? "" : lang !== "en" ? "（未命中模板）" : " (no template)"}</strong>
        <button type="button" className="icon-btn tiny pv-close" aria-label="close" onClick={props.onClose}>✕</button>
      </div>
      <div className="pv-info-body">
        {props.noProjectHint && (
          <p className="pv-warn-text">
            {lang !== "en" ? "先导入项目，我才能知道该写进哪个文件。" : "Import a project first so I know where to write."}
            <button type="button" className="btn tiny ghost" onClick={props.onImportFirst}>
              <FolderOpen size={11} /> {lang !== "en" ? "导入后再生成" : "Import first"}
            </button>
          </p>
        )}
        <ol className="pv-list pv-steps">
          {p.steps.map((s, i) => <li key={i}>{s}</li>)}
        </ol>
        {p.unknownTerms.length > 0 && (
          <p className="pv-warn-text">{lang !== "en" ? `这些词我还不太懂：${p.unknownTerms.join("、")}` : `Unclear: ${p.unknownTerms.join(", ")}`}</p>
        )}
        {p.code && (
          <>
            <pre className="pv-code"><code>{p.code}</code></pre>
            <p className="pv-detail">{lang !== "en" ? "建议粘贴位置：" : "Paste location: "}<code>{p.targetFile}</code></p>
            <div className="pv-intent-actions">
              <button type="button" className="btn tiny ghost" onClick={() => void copyCode()}><Copy size={12} /> {lang !== "en" ? "复制" : "Copy"}</button>
              <button type="button" className="btn tiny ghost" onClick={() => void writeToFile()}><ExternalLink size={12} /> {lang !== "en" ? "写入新文件" : "Write new file"}</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// ---------- helpers ----------

/** 二.2 小地图：项目结构缩略图 + 当前视口红框 + 点击跳转相机。 */
function MiniMap(props: {
  nodes: GenNode[];
  vp: Vp;
  view: { w: number; h: number };
  onJump: (wx: number, wy: number) => void;
}): React.ReactElement {
  const W = 158;
  const H = 104;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const n of props.nodes) {
    minX = Math.min(minX, n.x); minY = Math.min(minY, n.y);
    maxX = Math.max(maxX, n.x + n.w); maxY = Math.max(maxY, n.y + n.h);
  }
  const bw = Math.max(1, maxX - minX);
  const bh = Math.max(1, maxY - minY);
  const s = Math.min(W / bw, H / bh, 0.25);
  const ox = (W - bw * s) / 2 - minX * s;
  const oy = (H - bh * s) / 2 - minY * s;
  // 当前视口的世界包围盒 → 地图坐标
  const wx0 = (-props.view.w / 2 - props.vp.x) / props.vp.z;
  const wy0 = (-props.view.h / 2 - props.vp.y) / props.vp.z;
  const wx1 = (props.view.w / 2 - props.vp.x) / props.vp.z;
  const wy1 = (props.view.h / 2 - props.vp.y) / props.vp.z;
  const jump = (e: React.MouseEvent): void => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const mx = e.clientX - r.left;
    const my = e.clientY - r.top;
    props.onJump((mx - ox) / s, (my - oy) / s);
  };
  return (
    <div className="pv-minimap" onClick={jump} role="button" aria-label="minimap" data-interactive>
      {props.nodes.map((n) => (
        <div
          key={n.key}
          style={{
            position: "absolute",
            left: ox + n.x * s,
            top: oy + n.y * s,
            width: Math.max(2.5, n.w * s),
            height: Math.max(2, n.h * s),
            background: "rgba(168,200,232,0.45)",
            borderRadius: 1,
          }}
        />
      ))}
      <div
        className="pv-minimap-vp"
        style={{
          left: ox + wx0 * s,
          top: oy + wy0 * s,
          width: Math.max(6, (wx1 - wx0) * s),
          height: Math.max(6, (wy1 - wy0) * s),
        }}
      />
    </div>
  );
}

/** 5.3 字典管理舱：五级字典概览 + 用户词典（8.2）的导入/导出/删除。 */
function DictPod(props: { onClose: () => void }): React.ReactElement {
  const { lang } = useI18n();
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  useEffect(() => {
    void loadSettings().then((s) => setOverrides(s.pvzDictOverrides)).catch(() => {});
  }, []);
  const l2Count = Object.values(L2_LANGUAGE).reduce((n, t) => n + Object.keys(t).length, 0);
  const rows: Array<[string, number]> = [
    ["L1 " + (lang !== "en" ? "通用编程词典" : "Universal"), Object.keys(L1_UNIVERSAL).length],
    ["L2 " + (lang !== "en" ? "语言语法词典" : "Language"), l2Count],
    ["L3 " + (lang !== "en" ? "框架专属词典" : "Framework"), Object.keys(L3_FRAMEWORK).length],
    ["L4 " + (lang !== "en" ? "项目领域词典" : "Domain"), Object.keys(L4_DOMAIN).length],
    ["L5 " + (lang !== "en" ? "意图变换字典" : "Intent transforms"), 8],
    ["L6 " + (lang !== "en" ? "惯用短语词典" : "Idioms"), Object.keys(L6_IDIOM).length],
    ["L7 " + (lang !== "en" ? "项目模式识别字典" : "Patterns"), Object.keys(L7_PATTERN).length],
    ["L1-L7 " + (lang !== "en" ? "用户补充" : "User learned"), Object.keys(overrides).length],
  ];
  async function exportDict(): Promise<void> {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const p = await save({ defaultPath: "my.dict.json", filters: [{ name: "Dict Pack", extensions: ["json"] }] });
    if (typeof p !== "string") return;
    try {
      await ipc.saveTextFile(p, JSON.stringify({ format: "variable-dict", version: 1, terms: overrides }, null, 2), true);
      pushToast("success", lang !== "en" ? "词典包已导出" : "Dict pack exported", p);
    } catch (e) {
      pushToast("error", lang !== "en" ? "导出失败" : "Export failed", errText(e));
    }
  }
  async function importDict(): Promise<void> {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const p = await open({ multiple: false, filters: [{ name: "Dict Pack", extensions: ["json"] }] });
    if (typeof p !== "string") return;
    try {
      const raw = JSON.parse(await ipc.readTextFile(p)) as { terms?: Record<string, string> };
      const terms = raw.terms ?? {};
      const merged = { ...overrides, ...terms };
      setOverrides(merged);
      await saveSetting("pvzDictOverrides", merged);
      pushToast("success", lang !== "en" ? `已导入 ${Object.keys(terms).length} 条词典` : `Imported ${Object.keys(terms).length} terms`);
    } catch (e) {
      pushToast("error", lang !== "en" ? "词典包无效" : "Invalid dict pack", errText(e));
    }
  }
  async function removeTerm(term: string): Promise<void> {
    const merged = { ...overrides };
    delete merged[term];
    setOverrides(merged);
    await saveSetting("pvzDictOverrides", merged).catch(() => {});
  }
  return (
    <div className="pv-pod pv-intent-pod">
      <div className="pv-info-head">
        <BookOpen size={13} />
        <strong>{lang !== "en" ? "字典管理" : "Dictionaries"}</strong>
        <button type="button" className="icon-btn tiny pv-close" aria-label="close" onClick={props.onClose}>✕</button>
      </div>
      <div className="pv-info-body">
        <section>
          <h4>{lang !== "en" ? "已加载字典（五级体系，全部离线）" : "Loaded dictionaries"}</h4>
          <ul className="pv-list">
            {rows.map(([name, n]) => <li key={name}>{name} —— {n} {lang !== "en" ? "条" : "entries"}</li>)}
          </ul>
        </section>
        <section>
          <h4>{lang !== "en" ? "用户补充（8.2 教学式反馈）" : "User-learned terms"}</h4>
          {Object.keys(overrides).length === 0 && <p className="dim">{lang !== "en" ? "还没有补充过词条。" : "No learned terms yet."}</p>}
          <ul className="pv-list">
            {Object.entries(overrides).map(([term, mean]) => (
              <li key={term}>
                {term} —— {mean}
                <button type="button" className="icon-btn tiny" aria-label="remove" onClick={() => void removeTerm(term)}><Trash2 size={10} /></button>
              </li>
            ))}
          </ul>
        </section>
        <div className="pv-intent-actions">
          <button type="button" className="btn tiny ghost" onClick={() => void importDict()}>{lang !== "en" ? "导入 .dict.json" : "Import .dict.json"}</button>
          <button type="button" className="btn tiny ghost" onClick={() => void exportDict()}>{lang !== "en" ? "导出词典包" : "Export pack"}</button>
        </div>
      </div>
    </div>
  );
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function errText(e: unknown): string {
  return errMessage(e).message;
}

/** FileInfoCard expects a MindNode; the project space builds a light shim. */
function shimNode(recordId: string): MindNode {
  return {
    id: "pv-shim",
    mindmapId: "",
    textHtml: "",
    textPlain: "",
    x: 0, y: 0, width: 0, height: 0,
    shape: "rounded",
    borderRadius: 0,
    borderColor: "#000",
    fillColor: "transparent",
    fontSize: 12,
    opacity: 1,
    locked: false,
    zIndex: 0,
    recordId,
    rotation: 0,
    groupId: null,
    hidden: false,
    collapsed: false,
    preset: "",
    updatedAt: 0,
  };
}
