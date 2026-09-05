/**
 * 批次 C（规格 5.7）跨软件数据流协议 — 前端 SDK：
 *
 * 5.7.1 富剪贴板：结构化数据经 localStorage 共享（同源多窗口），同时写
 *       系统剪贴板纯文本兜底。Write 粘贴时优先识别 Variable 富格式：
 *       Mind 节点 → 图片卡片；Code 代码 → 代码块；Fate 角色 → 人物档案。
 * 5.7.2 拖拽协议：HTML5 拖拽无法跨 WebView 窗口，改由 Rust `drag_track`
 *       轮询全局光标广播 `xflow://drag-move|drop|cancel`；目标窗口用
 *       useXDropTarget 判定光标落点并接收负载。负载走 localStorage，
 *       事件只带坐标，避免大 payload 过 Event Bus。
 * 5.7.3 引用协议见 xfRef*（xref.ts）。
 *
 * 零网络：全部本机内存/localStorage/Event Bus，不产生任何外部请求。
 */
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";

export type XfKind = "mind-node" | "code-block" | "fate-character" | "write-record" | "file";

export interface XfClip {
  kind: XfKind;
  /** 源窗口 label（app-write / app-mind / explorer …） */
  from: string;
  ts: number;
  /** 纯文本兜底（已同步写入系统剪贴板） */
  text: string;
  /** 结构化负载（各协议自定义） */
  payload: unknown;
}

const CLIP_KEY = "variable:xclipboard:v1";
const DRAG_KEY = "variable:xdrag:v1";
const CLIP_TTL = 5 * 60_000;

function readJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// 5.7.1 富剪贴板
// ---------------------------------------------------------------------------

/** 写入 Variable 富剪贴板（结构化），并尽力同步纯文本到系统剪贴板。 */
export function xfSet(kind: XfKind, text: string, payload: unknown): void {
  const clip: XfClip = { kind, from: safeLabel(), ts: Date.now(), text, payload };
  try {
    localStorage.setItem(CLIP_KEY, JSON.stringify(clip));
  } catch {
    /* 配额满/被禁 → 富格式丢失，系统剪贴板纯文本仍可用 */
  }
  void navigator.clipboard?.writeText(text).catch(() => {});
}

/** 读取富剪贴板（5 分钟过期；不存在/过期返回 null 走系统剪贴板原生路径）。 */
export function xfGet(): XfClip | null {
  const clip = readJson<XfClip>(CLIP_KEY);
  if (!clip || typeof clip.kind !== "string") return null;
  if (Date.now() - clip.ts > CLIP_TTL) return null;
  return clip;
}

export function xfClear(): void {
  localStorage.removeItem(CLIP_KEY);
}

// ---------------------------------------------------------------------------
// 5.7.2 跨窗口拖拽
// ---------------------------------------------------------------------------

function safeLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "unknown";
  }
}

let dragActive = false;

/**
 * 开始一次跨窗口拖拽（在源窗口的自定义拖拽手势里调用，如 mousedown+移动阈值、
 * 右键菜单"共享到其他软件"）。阻塞至左键释放 / Esc / 超时（60s）。
 * 负载即时写入 localStorage；drop/cancel 事件由 useXDropTarget 消费。
 */
export async function beginXDrag(kind: XfKind, text: string, payload: unknown): Promise<void> {
  if (dragActive) return;
  dragActive = true;
  document.body.classList.add("xf-dragging");
  try {
    const clip: XfClip = { kind, from: safeLabel(), ts: Date.now(), text, payload };
    localStorage.setItem(DRAG_KEY, JSON.stringify(clip));
    await emit("xflow://drag-start", { kind, from: clip.from });
    const res = await invoke<{ x: number; y: number; cancelled: boolean }>("drag_track");
    if (res.cancelled) {
      await emit("xflow://drag-cancel", {});
    } else {
      await emit("xflow://drag-drop", { x: res.x, y: res.y });
    }
  } catch {
    await emit("xflow://drag-cancel", {}).catch(() => {});
  } finally {
    localStorage.removeItem(DRAG_KEY);
    dragActive = false;
    document.body.classList.remove("xf-dragging");
  }
}

interface WinGeo {
  x: number;
  y: number;
  w: number;
  h: number;
  sf: number;
}

async function windowGeo(): Promise<WinGeo | null> {
  try {
    const win = getCurrentWindow();
    const [pos, size, sf] = await Promise.all([win.outerPosition(), win.outerSize(), win.scaleFactor()]);
    return { x: pos.x, y: pos.y, w: size.width, h: size.height, sf };
  } catch {
    return null; // 非 Tauri 运行时（测试/浏览器预览）
  }
}

/**
 * 跨窗口拖放目标 hook：监听 `xflow://drag-*` 事件，光标在本窗口内且拖拽
 * kind 被接受时返回 true（组件用于高亮），释放时回调 onDrop。
 * clientX/clientY 为本窗口客户区逻辑坐标（物理光标位置换算）。
 */
export function useXDropTarget(
  accepts: XfKind[],
  onDrop: (clip: XfClip, clientX: number, clientY: number) => void,
): boolean {
  const [hot, setHot] = useState(false);
  const dropRef = useRef(onDrop);
  dropRef.current = onDrop;
  const acceptsRef = useRef(accepts);
  acceptsRef.current = accepts;

  useEffect(() => {
    let disposed = false;
    const unlisten: (() => void)[] = [];
    const geoRef: { current: WinGeo | null } = { current: null };
    let kindOk = false;
    let inside = false;

    const inWin = (p: { x: number; y: number }, g: WinGeo): boolean =>
      p.x >= g.x && p.x < g.x + g.w && p.y >= g.y && p.y < g.y + g.h;

    void (async () => {
      unlisten.push(
        await listen<{ kind: XfKind }>("xflow://drag-start", (ev) => {
          kindOk = acceptsRef.current.includes(ev.payload.kind);
          setHot(false);
          inside = false;
          void windowGeo().then((g) => {
            if (!disposed) geoRef.current = g;
          });
        }),
      );
      unlisten.push(
        await listen<{ x: number; y: number }>("xflow://drag-move", (ev) => {
          const g = geoRef.current;
          if (!g || !kindOk) return;
          const now = inWin(ev.payload, g);
          if (now !== inside) {
            inside = now;
            setHot(now);
          }
        }),
      );
      unlisten.push(
        await listen<{ x: number; y: number }>("xflow://drag-drop", (ev) => {
          setHot(false);
          inside = false;
          const g = geoRef.current;
          if (!g || !kindOk || !inWin(ev.payload, g)) return;
          const clip = readJson<XfClip>(DRAG_KEY);
          if (!clip || !acceptsRef.current.includes(clip.kind)) return;
          const lx = (ev.payload.x - g.x) / g.sf;
          const ly = (ev.payload.y - g.y) / g.sf;
          dropRef.current(clip, lx, ly);
        }),
      );
      unlisten.push(
        await listen("xflow://drag-cancel", () => {
          setHot(false);
          inside = false;
        }),
      );
    })();

    return () => {
      disposed = true;
      for (const u of unlisten.splice(0)) {
        try {
          u();
        } catch {
          /* already torn down */
        }
      }
    };
    // accepts / onDrop 经 ref 透传，无需重新挂载监听
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return hot;
}

// ---------------------------------------------------------------------------
// Mind 节点 → Write 图片卡片（规格 5.7.1：粘贴/拖放 Mind 节点到 Write 变成图片）
// ---------------------------------------------------------------------------

function svgEscape(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** 把纯文本按近似宽度折行（每行 ~22 个全角字符，最多 maxLines 行，末行省略号）。 */
export function foldLines(text: string, maxLines = 6): string[] {
  const flat = text.replace(/\s+/g, " ").trim();
  if (!flat) return [];
  const width = 22;
  const lines: string[] = [];
  let rest = flat;
  while (rest.length > 0 && lines.length < maxLines) {
    if (lines.length === maxLines - 1 && rest.length > width) {
      lines.push(`${rest.slice(0, width - 1)}…`);
      break;
    }
    lines.push(rest.slice(0, width));
    rest = rest.slice(width);
  }
  return lines;
}

/** 生成 Mind 节点卡片 SVG data URL（确定性渲染：同文本同图，零网络）。 */
export function mindCardDataUrl(title: string, body: string): string {
  const W = 420;
  const titleLine = foldLines(title, 1)[0] ?? "";
  const bodyLines = foldLines(body, 5);
  const lineH = 24;
  const padTop = 44;
  const H = padTop + bodyLines.length * lineH + 18;
  const bodySvg = bodyLines
    .map((l, i) => `<text x="24" y="${padTop + i * lineH}" font-size="15" fill="#44506a">${svgEscape(l)}</text>`)
    .join("");
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">` +
    `<rect width="${W}" height="${H}" rx="14" fill="#eef3fb"/>` +
    `<rect x="0.5" y="0.5" width="${W - 1}" height="${H - 1}" rx="13.5" fill="none" stroke="#9db4d8"/>` +
    `<rect x="0" y="0" width="${W}" height="34" rx="14" fill="#d7e2f2"/>` +
    `<rect x="0" y="20" width="${W}" height="14" fill="#d7e2f2"/>` +
    `<circle cx="18" cy="17" r="5" fill="#6f8cc4"/>` +
    `<text x="34" y="22" font-size="14" font-weight="600" fill="#28374f">${svgEscape(titleLine)}</text>` +
    bodySvg +
    `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
