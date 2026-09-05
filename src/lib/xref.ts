/**
 * 批次 C（规格 5.7.3）跨软件引用协议：
 *
 * 引用以约定 URL 编码进普通 <a href>，随文档 HTML 持久化：
 *   xref:<kind>/<id>/<ver>        ver = 创建引用时的源 updatedAt（ms）
 *
 * - Write 可引用 Mind 节点（点击跳转到 Mind 并定位节点）
 * - Code 可引用 Write 的技术文档（引用面板，点击跳转 Write 打开文档）
 * - 引用对象更新后（源 ver > 引用 ver），引用位置/面板显示"内容已更新"
 *
 * 跳转经 Event Bus：目标窗口监听 `xref://focus`。零网络。
 */
import { emit } from "@tauri-apps/api/event";
import { ipc } from "./ipc";
import { openAppWindow, appWindowLabel } from "../system/windows/appWindows";

export type XrefKind = "mind-node" | "write-doc" | "code-file";

export interface Xref {
  kind: XrefKind;
  id: string;
  /** 引用创建时的源版本（updatedAt ms） */
  ver: number;
}

const XREF_PREFIX = "xref:";
const FOCUS_EVENT = "xref://focus";

/** 生成 xref href。id 中的 '/' 会被 URL 编码（code 文件路径含分隔符）。 */
export function xrefHref(kind: XrefKind, id: string, ver: number): string {
  return `${XREF_PREFIX}${kind}/${encodeURIComponent(id)}/${ver}`;
}

/** 解析 xref href；非 xref 链接返回 null。 */
export function parseXrefHref(href: string): Xref | null {
  if (!href.startsWith(XREF_PREFIX)) return null;
  const rest = href.slice(XREF_PREFIX.length);
  const slash1 = rest.indexOf("/");
  if (slash1 < 0) return null;
  const kind = rest.slice(0, slash1) as XrefKind;
  if (!["mind-node", "write-doc", "code-file"].includes(kind)) return null;
  const slash2 = rest.indexOf("/", slash1 + 1);
  if (slash2 < 0) return null;
  const id = decodeURIComponent(rest.slice(slash1 + 1, slash2));
  const ver = Number(rest.slice(slash2 + 1));
  if (!id || !Number.isFinite(ver)) return null;
  return { kind, id, ver };
}

/** 生成引用锚点 HTML（随正文保存；样式由 .xref-link 提供）。 */
export function xrefAnchorHtml(kind: XrefKind, id: string, ver: number, title: string): string {
  const safe = title.replace(/[<>&"]/g, "").trim() || kind;
  return `<a class="xref-link" href="${xrefHref(kind, id, ver)}">↗ ${safe}</a>`;
}

/** 扫描 HTML 中全部 xref 引用（去重：同 kind+id 取最小 ver）。 */
export function collectXrefs(html: string): Xref[] {
  const out = new Map<string, Xref>();
  const re = /href="xref:([^"/]+)\/([^"]+)\/(\d+)"/g;
  for (const m of html.matchAll(re)) {
    const x = parseXrefHref(`xref:${m[1]}/${m[2]}/${m[3]}`);
    if (!x) continue;
    const key = `${x.kind}:${x.id}`;
    const prev = out.get(key);
    if (!prev || x.ver < prev.ver) out.set(key, x);
  }
  return [...out.values()];
}

/** 引用是否过期（源版本 > 引用版本 → "内容已更新"）。 */
export function isStale(x: Xref, sourceVer: number): boolean {
  return sourceVer > x.ver;
}

/** 点击跳转：打开目标软件窗口并发送聚焦事件（由目标窗口的监听方落地）。 */
export async function openXref(x: Xref): Promise<void> {
  if (x.kind === "mind-node") {
    await openAppWindow("mindmap");
    await emit(FOCUS_EVENT, { kind: x.kind, id: x.id }).catch(() => {});
    return;
  }
  if (x.kind === "write-doc") {
    await openAppWindow("write");
    await emit(FOCUS_EVENT, { kind: x.kind, id: x.id }).catch(() => {});
    return;
  }
  // code-file：目标窗口 label 由调用方（Write 端 xref 点击处理）决定 ——
  // 当前唯一 code 引用宿主在 Write，源文件路径暂无独立窗口，忽略。
}

/** 目标窗口 label（供 emitTo 场景）。 */
export function xrefTargetLabel(kind: XrefKind): string | null {
  if (kind === "mind-node") return appWindowLabel("mindmap");
  if (kind === "write-doc") return appWindowLabel("write");
  return null;
}

// ---------------------------------------------------------------------------
// 版本校验（引用对象更新检测）
// ---------------------------------------------------------------------------

export interface StaleXref {
  x: Xref;
  sourceVer: number;
}

/**
 * 批量校验引用版本：
 * - mind-node → Rust nodes_versions（单查询/节点）
 * - write-doc → getDocument（文档已删/回收 → 视为缺失，返回 sourceVer=-1）
 */
export async function checkStaleXrefs(refs: Xref[]): Promise<StaleXref[]> {
  const stale: StaleXref[] = [];
  const mindIds = refs.filter((r) => r.kind === "mind-node").map((r) => r.id);
  const mindVer = new Map<string, number>();
  if (mindIds.length > 0) {
    try {
      const rows = await ipc.nodesVersions(mindIds);
      for (const r of rows) mindVer.set(r.id, r.updated_at);
    } catch {
      return []; // 查询失败时不误报
    }
  }
  for (const x of refs) {
    if (x.kind === "mind-node") {
      const v = mindVer.get(x.id);
      if (v !== undefined && isStale(x, v)) stale.push({ x, sourceVer: v });
    } else if (x.kind === "write-doc") {
      try {
        const d = await ipc.getDocument(x.id);
        if (isStale(x, d.updatedAt)) stale.push({ x, sourceVer: d.updatedAt });
      } catch {
        stale.push({ x, sourceVer: -1 }); // 文档缺失
      }
    }
  }
  return stale;
}

// ---------------------------------------------------------------------------
// 剪贴板引用（复制为引用 / 粘贴引用）
// ---------------------------------------------------------------------------

/** 系统剪贴板中的引用（text/plain 载体：xref: URL，可读可解析）。 */
export async function copyXrefToClipboard(kind: XrefKind, id: string, ver: number, title: string): Promise<void> {
  const text = `${xrefHref(kind, id, ver)} ${title}`.trim();
  const html = xrefAnchorHtml(kind, id, ver, title);
  try {
    const item = new ClipboardItem({
      "text/plain": new Blob([text], { type: "text/plain" }),
      "text/html": new Blob([html], { type: "text/html" }),
    });
    await navigator.clipboard.write([item]);
  } catch {
    await navigator.clipboard.writeText(text).catch(() => {});
  }
}

/** 从剪贴板解析引用（粘贴引用入口）。 */
export async function readXrefFromClipboard(): Promise<Xref & { title: string } | null> {
  try {
    const text = await navigator.clipboard.readText();
    const m = /^xref:([^/\s]+)\/([^\s]+)\/(\d+)(?:\s+(.*))?$/.exec(text.trim());
    if (!m) return null;
    const x = parseXrefHref(`xref:${m[1]}/${m[2]}/${m[3]}`);
    return x ? { ...x, title: (m[4] ?? "").trim() } : null;
  } catch {
    return null;
  }
}
