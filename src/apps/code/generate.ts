/**
 * 第五章：思维导图可视化呈现规则。
 * 根节点 + [是什么/有什么/怎么运行] 三支四层结构；文件信息卡数据；
 * 长链条流程可视化；10.2 语义化低饱和配色；确定性横向树布局。
 */

import type {
  FileAnalysis, GenEdge, GenGraph, GenNode, NodeKind,
  ProjectDetect, ProjectScanResult, ScanEntry,
} from "./types";
import {
  dirDescription, entryDescription, fileOneLiner,
  flowStepDescription, purposeSentence, techStackSentence, type Lang,
} from "./narrate";

// 规范 10.2 语义化配色（低饱和冷调，樱花粉仅作点睛）
export const KIND_BORDER: Record<NodeKind, string> = {
  root: "#4f709c",
  branch: "#7f9bd9",
  dir: "#8babc6",
  source: "#a8c8e8",
  config: "#cfe4f8",
  doc: "#dce4ec",
  asset: "#8babc6",
  flow: "#a8c8e8",
  intent: "#f5c6d8",
  info: "#dce4ec",
};
/** 10.1 双向引用连线：淡樱粉色，与其他连线自然区分。 */
export const CROSS_REF_COLOR = "#f5c6d8";
export const CROSS_REF_NODE = "#f8d4e4";
const EDGE_COLOR = "#7f9bd9";

const NODE_GAP = 44;       // 三章：同级兄弟最小垂直安全间距（消除重叠）
const COL_GAP = 130;       // 三章：列间距加宽，留出流光连线的呼吸空间
const W_STD = 240;
const H_STD = 64;
const W_CARD = 300;
const H_CARD = 128;
const MAX_NODES = 240;     // hard cap: canvas must stay navigable
const MAX_CHILDREN = 14;   // per parent; overflow becomes a "…N more" leaf

export interface ProjectModel {
  scan: ProjectScanResult;
  detect: ProjectDetect;
  analyses: Map<string, FileAnalysis>;
  /** relPath → importing relPaths (谁在用它). */
  usedBy: Map<string, string[]>;
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

class GraphBuilder {
  nodes: GenNode[] = [];
  edges: GenEdge[] = [];
  fileKeys = new Map<string, string>();
  private idSeq = 0;

  node(n: Omit<GenNode, "key"> & { key?: string }): GenNode | null {
    if (this.nodes.length >= MAX_NODES) return null;
    const key = n.key ?? `pv${++this.idSeq}`;
    const full: GenNode = { ...n, key };
    this.nodes.push(full);
    return full;
  }

  edge(from: string, to: string, animated = false, label?: string): void {
    this.edges.push({ from, to, animated, color: EDGE_COLOR, label });
  }
}

interface Placed {
  node: GenNode;
  bottom: number;
}

/** Stack a list of (already sized) node specs vertically; returns total bottom. */
function stackNodes(
  g: GraphBuilder,
  items: Array<{ html: string; plain: string; kind: NodeKind; recordId?: string; w: number; h: number }>,
  x: number,
  yTop: number,
): Placed[] {
  const placed: Placed[] = [];
  let y = yTop;
  for (const item of items) {
    const n = g.node({ ...item, x, y });
    if (!n) break;
    placed.push({ node: n, bottom: y + item.h });
    y += item.h + NODE_GAP;
  }
  return placed;
}

/** Files of a dir → prioritized leaf specs (spec 5.2 摘要式叶子). */
function fileLeafSpecs(
  model: ProjectModel,
  dirPath: string,
  lang: Lang,
  budget: number,
): Array<{ html: string; plain: string; kind: NodeKind; recordId?: string; w: number; h: number }> {
  const roleRank: Record<FileAnalysis["role"], number> = { entry: 0, config: 1, doc: 2, source: 3, asset: 9 };
  const files = model.scan.entries.filter(
    (e) => e.kind === "file" && e.depth > 0 && (dirPath === "" ? !e.path.includes("/") : e.path.startsWith(`${dirPath}/`) && !e.path.slice(dirPath.length + 1).includes("/")),
  );
  files.sort((a, b) => roleRank[roleOfFile(model, a)] - roleRank[roleOfFile(model, b)] || a.name.localeCompare(b.name));
  const visible = files.filter((f) => roleOfFile(model, f) !== "asset").slice(0, budget);
  const assetCount = files.length - visible.filter((f) => roleOfFile(model, f) !== "asset").length;
  const out: Array<{ html: string; plain: string; kind: NodeKind; recordId?: string; w: number; h: number }> = [];
  for (const f of visible) {
    const analysis = model.analyses.get(f.path);
    const role = roleOfFile(model, f);
    const kind: NodeKind = role === "config" ? "config" : role === "doc" ? "doc" : role === "asset" ? "asset" : "source";
    const one = analysis ? fileOneLiner(analysis, lang) : `${f.name}（${Math.max(1, Math.round(f.size / 1024))} KB）`;
    out.push({
      html: `<p>📄 <strong>${esc(f.name)}</strong><br><span style="opacity:.75;font-size:12px">${esc(one)}</span></p>`,
      plain: one,
      kind,
      recordId: `pv:${f.path}`,
      w: W_STD,
      h: H_STD,
    });
  }
  const hidden = files.length - visible.length;
  if (hidden > 0 || assetCount > 0) {
    const n = hidden + assetCount;
    out.push({
      html: `<p style="opacity:.7">…还有 ${n} 个文件（资源/次要文件已折叠）</p>`,
      plain: `${n} more files`,
      kind: "info",
      w: W_STD,
      h: 44,
    });
  }
  return out;
}

const ASSET_FILE_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "bmp", "webp", "avif", "svg", "ico",
  "mp4", "webm", "mov", "mkv", "mp3", "wav", "woff", "woff2", "ttf", "otf",
  "zip", "gz", "tar", "pdf", "lock",
]);

function roleOfFile(model: ProjectModel, entry: ScanEntry): FileAnalysis["role"] {
  return model.analyses.get(entry.path)?.role
    ?? (/\.md$|readme|license/i.test(entry.path)
      ? "doc"
      : ASSET_FILE_EXTS.has((entry.ext ?? "").toLowerCase())
        ? "asset"
        : entry.ext && ["json", "yaml", "yml", "toml", "xml", "ini", "gradle"].includes(entry.ext)
          ? "config"
          : "source");
}

/** Directory subtree: dir node + its file leaves (spec 5.1 "有什么"). */
function layoutDirSubtree(
  g: GraphBuilder,
  model: ProjectModel,
  entry: ScanEntry,
  x: number,
  yTop: number,
  lang: Lang,
  budget: number,
): Placed | null {
  const desc = dirDescription(entry, lang);
  const dirNode = g.node({
    html: `<p>📁 <strong>${esc(entry.name)}</strong><br><span style="opacity:.75;font-size:12px">${esc(desc)}</span></p>`,
    plain: desc,
    kind: "dir",
    recordId: `pv-dir:${entry.path}`,
    x, y: yTop, w: W_STD, h: H_STD,
  });
  if (!dirNode) return null;
  const leaves = stackNodes(g, fileLeafSpecs(model, entry.path, lang, budget), x + W_STD + COL_GAP, yTop - 40,);
  if (leaves.length > 0) {
    for (const leaf of leaves) g.edge(dirNode.key, leaf.node.key);
  }
  const bottom = Math.max(yTop + H_STD, leaves.length > 0 ? leaves[leaves.length - 1]!.bottom : yTop + H_STD);
  const center = leaves.length > 0 ? (leaves[0]!.node.y + leaves[leaves.length - 1]!.bottom) / 2 - H_STD / 2 : yTop;
  // Move the dir node to the vertical center of its children (tidy tree).
  dirNode.y = Math.round(center);
  return { node: dirNode, bottom };
}

/**
 * Build the full project mindmap graph anchored at a world position.
 * Deterministic layout: three branches left→right, leaves stacked below.
 */
export function buildGraph(model: ProjectModel, origin: { x: number; y: number }, lang: Lang): GenGraph {
  const g = new GraphBuilder();
  const { detect, scan, analyses } = model;
  const shortLabel = detect.label.split(/[（(]/)[0]?.trim() ?? detect.label;

  // ---- root (spec 5.1) ----
  const rootNode = g.node({
    html: `<p>📦 <strong>${esc(scan.root.split(/[\\/]/).pop() ?? scan.root)}</strong><br><span style="opacity:.85">${esc(shortLabel)}</span></p>`,
    plain: shortLabel,
    kind: "root",
    recordId: `pv-root:${scan.root}`,
    x: origin.x - 130,
    y: origin.y - 55,
    w: 260,
    h: 110,
  });
  if (!rootNode) return { nodes: [], edges: [], rootKey: "", fileKeys: g.fileKeys };
  for (const n of g.nodes) if (n.recordId?.startsWith("pv:")) g.fileKeys.set(n.recordId.slice(3), n.key);

  const branchX = rootNode.x + 260 + COL_GAP + 60;
  const branchSpecs = { plain: "", kind: "branch" as NodeKind, w: 180, h: 54 };

  // ================= branch 1: 是什么 =================
  const whatY = origin.y - 260;
  const what = g.node({ ...branchSpecs, html: `<p>❓ <strong>${lang !== "en" ? "是什么" : "What"}</strong></p>`, plain: lang !== "en" ? "是什么" : "What", x: branchX, y: whatY });
  if (what) {
    g.edge(rootNode.key, what.key);
    const cards: Array<{ html: string; plain: string; kind: NodeKind; recordId?: string; w: number; h: number }> = [
      {
        html: `<p>🎯 <strong>${lang !== "en" ? "一句话概括" : "In one sentence"}</strong></p><p style="font-size:12px;opacity:.85">${esc(purposeSentence(detect, scan, lang))}</p>`,
        plain: purposeSentence(detect, scan, lang),
        kind: "info", w: W_CARD, h: H_CARD,
      },
      {
        html: `<p>🧰 <strong>${lang !== "en" ? "技术栈" : "Tech stack"}</strong></p><p style="font-size:12px;opacity:.85">${esc(techStackSentence(detect, lang))}</p>`,
        plain: techStackSentence(detect, lang),
        kind: "info", w: W_CARD, h: H_CARD,
      },
      {
        html: `<p>🚪 <strong>${lang !== "en" ? "怎么启动" : "How it starts"}</strong></p><p style="font-size:12px;opacity:.85">${esc(entryDescription(detect, lang))}</p>`,
        plain: entryDescription(detect, lang),
        kind: "info", w: W_CARD, h: H_CARD,
      },
    ];
    const placed = stackNodes(g, cards, branchX + 180 + COL_GAP, whatY - 60);
    for (const p of placed) g.edge(what.key, p.node.key);
  }

  // ================= branch 2: 有什么 =================
  const hasY = origin.y - 40;
  const has = g.node({ ...branchSpecs, html: `<p>🗂 <strong>${lang !== "en" ? "有什么" : "Has"}</strong></p>`, plain: lang !== "en" ? "有什么" : "Has", x: branchX, y: hasY });
  if (has) {
    g.edge(rootNode.key, has.key);
    const dirs = scan.entries.filter((e) => e.kind === "dir" && e.depth === 0).slice(0, MAX_CHILDREN - 2);
    const rootFiles = scan.entries.filter(
      (e) => e.kind === "file" && e.depth === 0 && roleOfFile(model, e) !== "asset",
    ).slice(0, 8);
    const remaining = scan.entries.filter((e) => e.kind === "dir" && e.depth === 0).length - dirs.length;
    const budget = Math.max(4, Math.floor((MAX_NODES - g.nodes.length - dirs.length * 2) / Math.max(1, dirs.length)));
    let y = hasY - 120;
    const colX = branchX + 180 + COL_GAP;
    const placedDirs: Placed[] = [];
    for (const d of dirs) {
      const p = layoutDirSubtree(g, model, d, colX, y, lang, budget);
      if (!p) break;
      placedDirs.push(p);
      g.edge(has.key, p.node.key);
      y = p.bottom + NODE_GAP + 30;
    }
    const leafSpecs: Array<{ html: string; plain: string; kind: NodeKind; recordId?: string; w: number; h: number }> = rootFiles.map((f) => {
      const analysis = analyses.get(f.path);
      const role = roleOfFile(model, f);
      const kind: NodeKind = role === "config" ? "config" : role === "doc" ? "doc" : "source";
      const one = analysis ? fileOneLiner(analysis, lang) : `${f.name}`;
      return {
        html: `<p>📄 <strong>${esc(f.name)}</strong><br><span style="opacity:.75;font-size:12px">${esc(one)}</span></p>`,
        plain: one, kind, recordId: `pv:${f.path}`, w: W_STD, h: H_STD,
      };
    });
    if (remaining > 0) {
      leafSpecs.push({
        html: `<p style="opacity:.7">…还有 ${remaining} 个顶级文件夹</p>`,
        plain: `${remaining} more folders`, kind: "info", w: W_STD, h: 44,
      });
    }
    if (leafSpecs.length > 0) {
      const fileX = dirs.length > 0 ? colX : colX;
      const placedFiles = stackNodes(g, leafSpecs, fileX, y, );
      for (const p of placedFiles) g.edge(has.key, p.node.key);
    }
  }

  // ================= branch 3: 怎么运行 =================
  const runY = origin.y + 200;
  const run = g.node({ ...branchSpecs, html: `<p>⚙️ <strong>${lang !== "en" ? "怎么运行" : "Runs"}</strong></p>`, plain: lang !== "en" ? "怎么运行" : "Runs", x: branchX, y: runY });
  if (run) {
    g.edge(rootNode.key, run.key);
    const chain = buildFlowChain(model, lang);
    let y = runY - 30;
    let prevKey: string | null = run.key;
    for (const step of chain) {
      const n = g.node({ ...step.spec, x: branchX + 180 + COL_GAP, y });
      if (!n) break;
      if (prevKey) g.edge(prevKey, n.key, true); // animated circuit line (5.5)
      g.fileKeys.set(`flow:${step.relPath}`, n.key);
      prevKey = n.key;
      y += step.spec.h + NODE_GAP;
    }
  }

  // index all file nodes for info cards
  for (const n of g.nodes) {
    if (n.recordId?.startsWith("pv:")) g.fileKeys.set(n.recordId.slice(3), n.key);
  }
  return { nodes: resolveOverlaps(g.nodes), edges: g.edges, rootKey: rootNode.key, fileKeys: g.fileKeys };
}

/**
 * 三章 · 自动避障：修正任何因人工拖动/固定节点导致的纵向重叠。
 * x 区间相交的节点之间强制 ≥ minGap 的垂直安全间距（向下推让，3 轮收敛）。
 */
export function resolveOverlaps(nodes: GenNode[], minGap = 44): GenNode[] {
  const out = nodes.map((n) => ({ ...n }));
  for (let pass = 0; pass < 3; pass++) {
    let moved = false;
    out.sort((a, b) => a.y - b.y || a.x - b.x);
    for (let i = 0; i < out.length; i++) {
      for (let j = i + 1; j < out.length; j++) {
        const a = out[i]!;
        const b = out[j]!;
        const xOverlap = a.x < b.x + b.w && b.x < a.x + a.w;
        if (!xOverlap) continue;
        const gap = b.y - (a.y + a.h);
        if (gap < minGap) {
          b.y += minGap - gap;
          moved = true;
        }
      }
    }
    if (!moved) break;
  }
  return out;
}

/** 线段与矩形（外扩 pad）是否相交。 */
function segIntersectsRect(
  x1: number, y1: number, x2: number, y2: number,
  rx: number, ry: number, rw: number, rh: number, pad: number,
): boolean {
  const minX = Math.min(x1, x2);
  const maxX = Math.max(x1, x2);
  const minY = Math.min(y1, y2);
  const maxY = Math.max(y1, y2);
  if (maxX < rx - pad || minX > rx + rw + pad || maxY < ry - pad || minY > ry + rh + pad) return false;
  // 粗粒度采样足够：连线仅用于可视化
  const steps = 16;
  for (let i = 1; i < steps; i++) {
    const t = i / steps;
    const px = x1 + (x2 - x1) * t;
    const py = y1 + (y2 - y1) * t;
    if (px > rx - pad && px < rx + rw + pad && py > ry - pad && py < ry + rh + pad) return true;
  }
  return false;
}

/**
 * 三章 · 连线自动避障路由：默认冰蓝贝塞尔；若直连会穿过任何节点文本框，
 * 则改为直角绕行（从阻挡节点上方或下方择近者绕过），逻辑线永不穿透节点。
 */
export function routeEdge(s: GenNode, t: GenNode, nodes: GenNode[]): string {
  const x1 = s.x + s.w;
  const y1 = s.y + s.h / 2;
  const x2 = t.x;
  const y2 = t.y + t.h / 2;
  const blockers = nodes.filter((n) => {
    if (n.key === s.key || n.key === t.key) return false;
    return segIntersectsRect(x1, y1, x2, y2, n.x, n.y, n.w, n.h, 8);
  });
  if (blockers.length === 0) {
    const dx = Math.max(40, Math.abs(x2 - x1) / 2);
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  }
  // 直角绕行：找阻挡簇的上缘/下缘，取离连线中点更近的一侧
  const top = Math.min(...blockers.map((b) => b.y));
  const bottom = Math.max(...blockers.map((b) => b.y + b.h));
  const midY = (y1 + y2) / 2;
  const detourY = Math.abs(top - 28 - midY) <= Math.abs(bottom + 28 - midY) ? top - 28 : bottom + 28;
  const midX = (x1 + x2) / 2;
  return `M ${x1} ${y1} L ${midX} ${y1} L ${midX} ${detourY} L ${x2} ${detourY} L ${x2} ${y2}`;
}

interface FlowStep {
  relPath: string;
  spec: { html: string; plain: string; kind: NodeKind; w: number; h: number };
}

/**
 * spec 5.3 长链条流程可视化：从入口文件沿"文件级 import"广度优先走 6 步，
 * 每步左侧大白话 + 右侧对应源文件位置。
 */
export function buildFlowChain(model: ProjectModel, lang: Lang, maxSteps = 6): FlowStep[] {
  const { detect, analyses } = model;
  const start = detect.entryCandidates.find((e) => analyses.has(e));
  if (!start) return [];
  const steps: FlowStep[] = [];
  const visited = new Set<string>([start]);
  let frontier = [start];
  while (frontier.length > 0 && steps.length < maxSteps) {
    const next: string[] = [];
    for (const rel of frontier) {
      const analysis = analyses.get(rel);
      if (!analysis) continue;
      const isEntry = steps.length === 0;
      const desc = flowStepDescription(analysis, isEntry, lang);
      const firstFn = analysis.symbols.find((s) => s.kind === "function");
      const ref = `${analysis.relPath}:${firstFn?.line ?? 1}`;
      steps.push({
        relPath: rel,
        spec: {
          html: `<p>${isEntry ? "🚀" : "➡️"} <strong>${esc(desc.split("，")[0] ?? desc)}</strong></p><p style="font-size:12px;opacity:.8">${esc(desc)}<br><code style="opacity:.7">${esc(ref)}</code></p>`,
          plain: desc,
          kind: "flow",
          w: W_CARD,
          h: H_STD + 34,
        },
      });
      // follow internal imports
      for (const imp of analysis.imports) {
        const target = resolveImport(model, rel, imp.from);
        if (target && !visited.has(target)) {
          visited.add(target);
          next.push(target);
        }
      }
      if (steps.length >= maxSteps) break;
    }
    frontier = next.slice(0, 2);
  }
  return steps;
}

/** Match an import specifier to a known internal file (suffix heuristics). */
export function resolveImport(model: ProjectModel, fromRel: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  const dir = fromRel.includes("/") ? fromRel.slice(0, fromRel.lastIndexOf("/")) : "";
  const parts = (dir ? dir.split("/") : []).concat(spec.replace(/^\.\//, "").replace(/^\.\.\//, "./").split("/"));
  const resolved: string[] = [];
  for (const p of parts) {
    if (p === "." ) continue;
    else if (p === "..") resolved.pop();
    else resolved.push(p);
  }
  const base = resolved.join("/");
  const candidates = [
    base,
    `${base}.ts`, `${base}.tsx`, `${base}.js`, `${base}.jsx`, `${base}.py`, `${base}.rs`, `${base}.go`,
    `${base}/index.ts`, `${base}/index.js`, `${base}/mod.rs`, `${base}/__init__.py`,
  ];
  for (const c of candidates) {
    if (model.analyses.has(c)) return c;
  }
  // suffix fallback (tsconfig path aliases etc.)
  const tail = resolved[resolved.length - 1];
  if (tail) {
    for (const key of model.analyses.keys()) {
      const k = key.replace(/\.[^.]+$/, "");
      if (k === base || k.endsWith(`/${base}`)) return key;
    }
  }
  return null;
}

/**
 * spec 5.4 下钻进入：为信息卡"下钻"生成函数级节点（围绕父文件节点摆放）。
 * 返回的节点/边由调用方插入画布并持久化。
 */
export function buildDrillDown(
  analysis: FileAnalysis,
  parentNode: { key: string; x: number; y: number; width: number },
  lang: Lang,
): { nodes: GenNode[]; edges: GenEdge[]; anchorKey: string } {
  const nodes: GenNode[] = [];
  const edges: GenEdge[] = [];
  const fns = analysis.symbols.filter((s) => s.kind === "function").slice(0, 10);
  const classes = analysis.symbols.filter((s) => s.kind === "class").slice(0, 4);
  const items = [...classes, ...fns];
  const radius = 260;
  items.forEach((s, i) => {
    const a = (i / Math.max(1, items.length)) * Math.PI * 2 - Math.PI / 2;
    const key = `pv-fn:${analysis.relPath}:${s.name}`;
    const text = s.kind === "class"
      ? `<p>📐 <strong>${esc(s.name)}</strong><br><span style="opacity:.75;font-size:12px">${lang !== "en" ? "设计图纸（类）" : "Blueprint (class)"} · L${s.line}</span></p>`
      : `<p>⚙️ <strong>${esc(s.name)}()</strong><br><span style="opacity:.75;font-size:12px">${esc(s.params.length > 0 ? s.params.join(", ") : (lang !== "en" ? "无参数" : "no params"))} · L${s.line}</span></p>`;
    nodes.push({
      key,
      html: text,
      plain: s.name,
      kind: "info",
      x: Math.round(parentNode.x + parentNode.width + 120 + radius * Math.cos(a) - 110),
      y: Math.round(parentNode.y + radius * Math.sin(a) - 30),
      w: W_STD,
      h: H_STD,
      recordId: `pv-fn:${analysis.relPath}:${s.name}:${s.line}`,
    });
    edges.push({ from: parentNode.key, to: key, animated: false, color: KIND_BORDER.info });
  });
  return { nodes, edges, anchorKey: parentNode.key };
}
