import { useEffect, useRef, useState } from "react";
import katex from "katex";
import "katex/dist/katex.min.css";
import { Copy, Link2, MoreHorizontal, Pencil, FileText, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { openContextMenu } from "../../components/ContextMenu";
import type { MindNode } from "../../lib/types";
import { highlightCode } from "../../lib/codehighlight";
import { looksLikeMarkdown, mdToNodeHtml } from "../../lib/nodemarkdown";
import { PREFERRED_TEXT_W, centroidOf, inscribedRect, sanitizeDims, shapePoints } from "./geometry";

interface Props {
  node: MindNode;
  selected: boolean;
  editing: boolean;
  /** Bounding box + handles visible (deep edit or free-transform mode). */
  showBox?: boolean;
  /** Module-0 dismissal: node just lost selection — play the fade-out of its
   *  glow / dashed box / vertex handles before unmounting. */
  ghostFading?: boolean;
  snapEnabled: boolean;
  resizeSensitivity: number;
  dragging?: boolean;
  onCommitText: (html: string) => void;
  onCancelEdit: () => void;
  onToggleCollapse?: () => void;
  onPointerDownNode: (e: React.PointerEvent) => void;
  onDoubleClick: (e: React.MouseEvent) => void;
  onNodeContextMenu?: (e: React.MouseEvent) => void;
  onResizeStart: (e: React.PointerEvent, handle: string) => void;
  /** Polygon vertex handle drag (deep-edit / free-transform only). */
  onVertexResizeStart?: (e: React.PointerEvent, index: number) => void;
  onStartConnect: (e: React.PointerEvent) => void;
  onAction: (action: "edit" | "duplicate" | "connect" | "more" | "delete") => void;
  menuAnchor: boolean;
}

/**
 * One mind-map node: a continuous writing area (no separate title/body split),
 * shape drawn as SVG behind the text layer. Auto-grows with content while
 * editing; hard max height falls back to internal scrolling.
 */
export function MindNodeView(props: Props): React.ReactElement {
  const { t } = useI18n();
  const { node, selected, editing } = props;
  const contentRef = useRef<HTMLDivElement>(null);
  const [focusedViaAction, setFocusedViaAction] = useState(false);

  // Auto-grow: measure content and lift height to parent. Runs in BOTH
  // editing and static mode — the stored height may lag behind the content
  // (e.g. after a commit, a fontSize change or an image load), and anything
  // the frame cannot hold gets clipped / scrolled away.
  useEffect(() => {
    if (node.collapsed || !contentRef.current) return;
    let raf = 0;
    const measure = () => {
      const el = contentRef.current;
      if (!el) return;
      // 先横后纵：先探测文本的自然单行需求宽度（临时 width:max-content，
      // 同帧恢复，不产生中间绘制），封顶 PREFERRED_TEXT_W；高度再由换行后
      // 的 scrollHeight 决定 —— 框架始终完整包住文字，不截断。
      const prev = el.style.width;
      el.style.width = "max-content";
      const naturalW = el.scrollWidth;
      el.style.width = prev;
      const textW = Math.min(naturalW, PREFERRED_TEXT_W);
      window.dispatchEvent(new CustomEvent("variable:mm-autogrow", {
        detail: { id: node.id, textWidth: textW, textHeight: el.scrollHeight },
      }));
    };
    const onInput = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(measure);
    };
    measure();
    if (editing) el_events(contentRef.current, onInput);
    // 静态模式下用 ResizeObserver 捕捉内容尺寸变化（图片加载、重排等）。
    const ro = new ResizeObserver(onInput);
    ro.observe(contentRef.current);
    return () => {
      cancelAnimationFrame(raf);
      if (editing) detach_events(contentRef.current, onInput);
      ro.disconnect();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing, node.id, node.hidden, node.collapsed, node.textHtml, node.fontSize]);

  useEffect(() => {
    if (editing && focusedViaAction && contentRef.current) {
      contentRef.current.focus();
      setFocusedViaAction(false);
    }
  }, [editing, focusedViaAction]);

  // ---------- 静态态富渲染（仅显示层，绝不入库） ----------
  // ``` 代码段做语法高亮；$…$ / $$…$$ 用 KaTeX 排版。两者都是显示态产物：
  // 存储的 node.textHtml 永远保留原始文本，因此再次编辑从源码起步，且
  // sanitize 白名单无需接纳生成标记。
  useEffect(() => {
    if (editing || !contentRef.current) return;
    const el = contentRef.current;
    el.querySelectorAll<HTMLPreElement>("pre.mm-code").forEach((pre) => {
      if (pre.dataset.hl === "1") return;
      pre.dataset.hl = "1";
      const src = pre.textContent ?? "";
      pre.innerHTML = highlightCode(src, pre.getAttribute("data-lang") ?? "");
    });
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => {
        const p = n.parentElement;
        if (!p || p.closest("pre, code, .mm-math")) return NodeFilter.FILTER_REJECT;
        return /\$[^$]+\$/.test(n.nodeValue ?? "") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
      },
    });
    const targets: Text[] = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) targets.push(n as Text);
    for (const t of targets) {
      const raw = t.nodeValue ?? "";
      const frag = document.createDocumentFragment();
      let last = 0;
      const re = /\$\$([^$]+)\$\$|\$([^$\n]+)\$/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(raw)) !== null) {
        if (m.index > last) frag.appendChild(document.createTextNode(raw.slice(last, m.index)));
        const span = document.createElement("span");
        span.className = "mm-math";
        const tex = m[1] ?? m[2] ?? "";
        try {
          katex.render(tex, span, { throwOnError: false, displayMode: m[1] !== undefined });
        } catch {
          span.textContent = m[0]; // 渲染失败时保留原文
        }
        frag.appendChild(span);
        last = m.index + m[0].length;
      }
      if (last < raw.length) frag.appendChild(document.createTextNode(raw.slice(last)));
      t.replaceWith(frag);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing, node.textHtml, node.fontSize]);

  // Seed the editable with existing HTML exactly once per edit session.
  useEffect(() => {
    if (editing && contentRef.current) {
      contentRef.current.innerHTML = node.textHtml;
      contentRef.current.focus();
      const range = document.createRange();
      range.selectNodeContents(contentRef.current);
      range.collapse(false);
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(range);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);

  const isCircle = node.shape === "circle";

  // ---------- 节点输入框剪贴板（复制 / 剪切 / 粘贴） ----------
  // 显式实现标准快捷键与右键菜单：优先走 execCommand 原生路径（同步、保留
  // HTML），失败时回退到异步 Clipboard API，保证任何环境下都可用。
  const pasteFallback = useRef(0); // 原生 paste 事件未到达时的兜底定时器
  useEffect(() => () => {
    if (pasteFallback.current) window.clearTimeout(pasteFallback.current);
  }, []);
  function ensureFocus(): void {
    const el = contentRef.current;
    if (el && document.activeElement !== el) el.focus();
  }
  function selText(): string {
    const sel = window.getSelection();
    return sel && !sel.isCollapsed ? sel.toString() : "";
  }
  function copySel(): void {
    const text = selText();
    if (!text) return;
    ensureFocus();
    if (document.execCommand("copy")) return;
    void navigator.clipboard?.writeText(text).catch(() => {});
  }
  function cutSel(): void {
    const text = selText();
    if (!text) return;
    ensureFocus();
    if (document.execCommand("cut")) return;
    void navigator.clipboard?.writeText(text).catch(() => {});
    document.execCommand("delete");
  }
  /** 菜单/兜底粘贴：优先 text/html（与 onPaste 行为一致），回退纯文本。 */
  async function pasteFromClipboard(): Promise<void> {
    ensureFocus();
    try {
      if (document.execCommand("paste")) return;
      if (navigator.clipboard?.read) {
        const items = await navigator.clipboard.read();
        for (const item of items) {
          if (item.types.includes("text/html")) {
            const html = await (await item.getType("text/html")).text();
            document.execCommand("insertHTML", false, sanitizeLite(html));
            return;
          }
        }
      }
      const text = await navigator.clipboard.readText();
      if (text) document.execCommand("insertText", false, text);
    } catch { /* 剪贴板不可用时静默放弃 */ }
  }

  // ---------- 智能提交与 ``` 代码段（编辑态） ----------
  /** 提交前把代码段规整为纯文本（innerText 把 <br> 折算成 \n）。 */
  function commitHtml(): string {
    const el = contentRef.current;
    if (!el) return "";
    el.querySelectorAll("pre.mm-code").forEach((pre) => {
      const txt = (pre as HTMLPreElement).innerText.replace(/\n+$/, "");
      pre.replaceChildren(document.createTextNode(txt));
    });
    return el.innerHTML;
  }
  /** 光标所在逻辑行、光标之前的文本（null=光标不在文本节点上）。 */
  function caretLineBefore(sel: Selection): string | null {
    if (sel.rangeCount === 0) return null;
    const r = sel.getRangeAt(0);
    if (!r.collapsed || r.startContainer.nodeType !== Node.TEXT_NODE) return null;
    const text = r.startContainer.textContent ?? "";
    const before = text.slice(0, r.startOffset);
    const nl = before.lastIndexOf("\n");
    return nl === -1 ? before : before.slice(nl + 1);
  }
  /** 删除光标所在行从行首到光标的文本。 */
  function deleteLineBefore(sel: Selection): void {
    if (sel.rangeCount === 0) return;
    const r = sel.getRangeAt(0);
    if (r.startContainer.nodeType !== Node.TEXT_NODE) return;
    const text = r.startContainer.textContent ?? "";
    const before = text.slice(0, r.startOffset);
    const nl = before.lastIndexOf("\n");
    const lineStart = nl === -1 ? 0 : nl + 1;
    if (lineStart >= r.startOffset) return;
    const del = document.createRange();
    del.setStart(r.startContainer, lineStart);
    del.setEnd(r.startContainer, r.startOffset);
    del.deleteContents();
  }
  function caretContainerEl(sel: Selection): HTMLElement | null {
    if (sel.rangeCount === 0) return null;
    const n = sel.getRangeAt(0).startContainer;
    return (n.nodeType === Node.TEXT_NODE ? n.parentElement : (n as HTMLElement)) ?? null;
  }
  /**
   * ```lang + Enter：把当前行变成嵌入式代码段并把光标移入；
   * 代码段内 ``` + Enter：关闭代码段，回到普通段落。
   */
  function openCodeFence(sel: Selection, lang: string): void {
    deleteLineBefore(sel);
    const code = document.createElement("pre");
    code.className = "mm-code";
    if (lang) code.setAttribute("data-lang", lang);
    // 只在内容根的内部块（div/p/h*/li/blockquote）上定位；绝不能把
    // closest("div") 匹配到 .mm-content 根自身 —— 那会 replaceWith 掉
    // React 拥有的根节点导致整棵树崩溃。
    const root = contentRef.current;
    let block = caretContainerEl(sel)?.closest("div, p, h1, h2, h3, h4, li, blockquote") ?? null;
    if (!root || !block || block === root || !root.contains(block)) block = null;
    const host = block ?? root;
    if (!host) return;
    if (block) {
      if ((block.textContent ?? "").trim()) block.after(code);
      else block.replaceWith(code);
    } else {
      host.appendChild(code);
    }
    const r = document.createRange();
    r.setStart(code, 0);
    r.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r);
  }
  function closeCodeFence(sel: Selection, pre: Element): void {
    deleteLineBefore(sel);
    const p = document.createElement("div");
    p.innerHTML = "<br>";
    pre.after(p);
    const r = document.createRange();
    r.setStart(p, 0);
    r.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r);
  }

  // ---------- 鼠标拖拽框选文字（编辑态） ----------
  // 用 caretRangeFromPoint 自绘选区：起点为按下位置，拖动过程中实时重设
  // selection，超出输入框边界时钳制到框内（拖到末尾即选到末尾）。松开鼠标
  // 后选区保留，可直接 Delete/Backspace 删除或 Ctrl+C/X 复制剪切。
  function caretPoint(x: number, y: number): { node: Node; offset: number } | null {
    const el = contentRef.current;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cx = Math.min(Math.max(x, r.left + 1), r.right - 1);
    const cy = Math.min(Math.max(y, r.top + 1), r.bottom - 1);
    const range = document.caretRangeFromPoint?.(cx, cy);
    if (!range || !el.contains(range.startContainer)) return null;
    return { node: range.startContainer, offset: range.startOffset };
  }
  function beginDragSelect(e: React.MouseEvent): void {
    if (!editing || e.button !== 0 || !contentRef.current) return;
    const startX = e.clientX;
    const startY = e.clientY;
    const start = caretPoint(startX, startY);
    let active = false;
    const onMove = (ev: MouseEvent): void => {
      if (!active && Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY) < 4) return;
      active = true;
      const cur = caretPoint(ev.clientX, ev.clientY);
      if (!start || !cur) return;
      const range = document.createRange();
      const after = start.node.compareDocumentPosition(cur.node) & Node.DOCUMENT_POSITION_FOLLOWING;
      const sameNodeLater = start.node === cur.node && start.offset <= cur.offset;
      if (after || sameNodeLater) {
        range.setStart(start.node, start.offset);
        range.setEnd(cur.node, cur.offset);
      } else {
        range.setStart(cur.node, cur.offset);
        range.setEnd(start.node, start.offset);
      }
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(range);
    };
    const finish = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", finish);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", finish);
  }

  // Module-0: while the dismissal fade plays, keep the box + handles mounted
  // (fading) even though selection is already gone.
  const ghost = !!props.ghostFading && !selected && !editing;
  const effShowBox = !!props.showBox || ghost;
  // Module-4 render-path sanity check (module 四-1): the frame is NEVER drawn
  // from corrupt dims — NaN / Infinity / sub-minimum values are replaced by a
  // sane standard ratio before any SVG path is computed, and the repair is
  // reported once so the store persists the healed size.
  const sane = sanitizeDims(node.width, node.height);
  useEffect(() => {
    if (!sane.repaired) return;
    window.dispatchEvent(new CustomEvent("variable:mm-repair-node", {
      detail: { id: node.id, width: node.width, height: node.height },
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [node.id, sane.repaired]);
  const dispW = isCircle ? Math.min(sane.dim.width, sane.dim.height) : sane.dim.width;
  const dispH = isCircle ? Math.min(sane.dim.width, sane.dim.height) : sane.dim.height;
  // Readability contract: font size can never be 0/NaN/sub-floor — a corrupt
  // value falls back to the default instead of rendering blank glyphs.
  const fontSize = Math.max(12, Number.isFinite(node.fontSize) ? node.fontSize : 15);
  // Resize-handle hit area from the user's sensitivity setting (clamped).
  const handleSize = Math.max(6, Math.min(24, Math.round(props.resizeSensitivity)));

  const isBoxShape = node.shape === "rect" || node.shape === "rounded";
  const isPolyShape =
    node.shape === "triangle" || node.shape === "diamond" ||
    node.shape === "pentagon" || node.shape === "hexagon" || node.shape === "heptagon";
  // Module-5 control matrix: box shapes keep the classic 8-point frame;
  // circles expose only the four cardinal handles (no square visual);
  // polygons are VERTEX-ONLY — no AABB handles at all.
  const handles: string[] = isBoxShape
    ? ["nw", "n", "ne", "e", "se", "s", "sw", "w"]
    : isCircle
      ? ["n", "e", "s", "w"]
      : [];
  const collapsedH = 36;

  if (node.hidden) return <></>;

  const renderW = node.collapsed ? Math.max(150, Math.min(dispW, 260)) : dispW;
  const renderH = node.collapsed ? collapsedH : dispH;
  const collapseTitle = node.textHtml
    ? stripTagsLocal(node.textHtml).slice(0, 26) || "…"
    : "…";

  // Module-2: non-box shapes anchor their text at the polygon CENTROID and
  // confine it to the largest inscribed rectangle centered there.
  const insc = !isBoxShape && !node.collapsed ? inscribedRect(node.shape, renderW, renderH) : null;
  const centroid = isPolyShape || isCircle ? centroidOf(node.shape, renderW, renderH) : null;

  // Contract-4: the action bar is hidden until the pointer enters the lower
  // half of the frame (or the bar itself); it fades out on leave.
  const [actionsHot, setActionsHot] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  function updateActionsHot(e: React.MouseEvent): void {
    const el = rootRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setActionsHot(e.clientY >= r.top + r.height * 0.55);
  }

  return (
    <div
      ref={rootRef}
      className={[
        "mm-node",
        `shape-${node.shape}`,
        isBoxShape ? "boxed" : "",
        node.preset ? `preset-${node.preset}` : "",
        selected ? "selected" : "",
        ghost ? "sel-fade" : "",
        node.locked ? "locked" : "",
        editing ? "editing" : "",
        node.collapsed ? "collapsed" : "",
        effShowBox ? "show-box" : "",
      ].filter(Boolean).join(" ")}
      style={{
        left: node.x,
        top: node.y,
        width: renderW,
        // 显式高度（而非 min-height）：.shape-bg 是替换元素，父容器高度为
        // auto 时其 height:100% 会被 Chromium 忽略、回退到 SVG 属性高度，
        // 导致内容超高时框体停在旧尺寸、文字向下溢出。父高度显式后百分比
        // 必然解析，框体与内容永远一致；内容瞬时超出时由 mm-content 内部
        // 滚动兜底，绝无外溢。
        height: renderH,
        opacity: node.opacity,
        zIndex: node.zIndex,
        transform: node.rotation ? `rotate(${node.rotation}deg)` : undefined,
        transition: props.dragging ? "none" : "width .18s ease, height .18s ease, box-shadow .2s ease",
      }}
      data-node-id={node.id}
      onPointerDown={props.onPointerDownNode}
      onDoubleClick={props.onDoubleClick}
      onContextMenu={props.onNodeContextMenu}
      onMouseMove={updateActionsHot}
      onMouseLeave={() => setActionsHot(false)}
    >
      <svg className="shape-bg" width={renderW} height={renderH} aria-hidden>
        {isCircle && !node.collapsed ? (
          <circle cx={renderW / 2} cy={renderH / 2} r={Math.min(renderW, renderH) / 2 - 1} fill={node.fillColor} stroke={node.borderColor} strokeWidth={1.4} />
        ) : node.collapsed || node.shape === "rect" || node.shape === "rounded" ? (
          <rect
            x="0.7" y="0.7" width={renderW - 1.4} height={renderH - 1.4}
            rx={node.collapsed ? 10 : node.shape === "rounded" ? node.borderRadius : 0}
            fill={node.fillColor} stroke={node.borderColor} strokeWidth={1.4}
          />
        ) : (
          <polygon points={shapePoints(node.shape, renderW, renderH).map((p) => `${p.x},${p.y}`).join(" ")} fill={node.fillColor} stroke={node.borderColor} strokeWidth={1.4} />
        )}
      </svg>

      {/* collapsed title strip */}
      {node.collapsed ? (
        <div className="mm-collapsed-title" style={{ fontSize }}>
          <button
            type="button"
            className="icon-btn tiny"
            aria-label={t("expand")}
            title={t("expand")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onToggleCollapse?.();
            }}
          >
            ›
          </button>
          <span className="ellipsis">{collapseTitle}</span>
        </div>
      ) : (
      <>
      {/* continuous writing area — key forces a clean remount on edit toggles so
          the DOM always matches the store (cancel never leaves edited residue) */}
      <div
        key={editing ? "editing" : "static"}
        ref={contentRef}
        className="mm-content"
        contentEditable={editing || undefined}
        suppressContentEditableWarning
        data-placeholder={editing ? "" : undefined}
        style={{
          // Readability floor: a node can never render text below 12px,
          // whatever a corrupt/legacy fontSize says (NaN → 15).
          fontSize,
          borderRadius: node.shape === "rounded" ? node.borderRadius : 0,
          // Module-2: polygons/circles pin their text box to the centroid-
          // centered inscribed rectangle — text can never escape a slanted edge.
          ...(insc ? {
            position: "absolute",
            left: insc.x,
            top: insc.y,
            width: insc.w,
            maxWidth: insc.w,
            minWidth: Math.min(72, insc.w),
            maxHeight: insc.h,
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            alignItems: "safe center",
            justifyContent: "safe center",
            textAlign: "center",
            padding: "4px 6px",
          } : {
            // Module-2 anti-vertical-stack guard: the text box can never be
            // narrower than one readable column, whatever the frame shape is.
            minWidth: Math.min(72, renderW),
            // 内容瞬时超出（autogrow 尚未跟上的一帧）时框内滚动，不外溢。
            maxHeight: "100%",
          }),
          overflowWrap: "break-word",
        }}
        onBlur={() => {
          if (editing && contentRef.current) props.onCommitText(commitHtml());
        }}
        onMouseDown={beginDragSelect}
        onKeyDown={(e) => {
          e.stopPropagation(); // canvas shortcuts never steal text input
          const mod = e.ctrlKey || e.metaKey;
          if (mod && (e.key === "c" || e.key === "C")) { e.preventDefault(); copySel(); return; }
          if (mod && (e.key === "x" || e.key === "X")) { e.preventDefault(); cutSel(); return; }
          if (mod && (e.key === "v" || e.key === "V")) {
            // 不 preventDefault：让原生 paste 事件走 onPaste（完整剪贴板数据）。
            // 若事件迟迟未到达（环境屏蔽），由定时器兜底走 Clipboard API。
            if (pasteFallback.current) window.clearTimeout(pasteFallback.current);
            pasteFallback.current = window.setTimeout(() => {
              pasteFallback.current = 0;
              void pasteFromClipboard();
            }, 220);
            return;
          }
          // 智能提交：Enter 自动换行；Shift+Enter / Ctrl+Enter 保存并退出编辑。
          if (e.key === "Enter" && (e.shiftKey || mod)) {
            e.preventDefault();
            props.onCommitText(commitHtml());
            return;
          }
          if (e.key === "Enter" && !e.shiftKey && !mod) {
            const sel = window.getSelection();
            const inCode = sel ? caretContainerEl(sel)?.closest("pre.mm-code") : null;
            if (sel && inCode) {
              e.preventDefault();
              // 代码段内 ``` + Enter → 关闭代码段
              const line = caretLineBefore(sel);
              if (line !== null && line.trim() === "```") {
                closeCodeFence(sel, inCode);
                return;
              }
              // 代码段内 Enter → 字面换行（保持纯文本源码形态）。
              // 必须走 execCommand 原生路径：手写 Range 插入 \n 文本节点时，
              // Chrome 会把光标规范化回上一行尾（pre-wrap 尾部换行陷阱），
              // 后续输入会错误并入前一文本节点；原生路径用 <br> 承载换行，
              // 光标稳定，commitHtml 的 innerText 规整会折算回 \n。
              document.execCommand("insertText", false, "\n");
              return;
            }
            // ```lang + Enter → 开启嵌入式代码段
            const m = (sel ? caretLineBefore(sel) : null)?.match(/^\s*```([A-Za-z0-9+#._-]*)\s*$/) ?? null;
            if (m && sel) {
              e.preventDefault();
              openCodeFence(sel, m[1] ?? "");
              return;
            }
          }
          if (e.key === "Escape") {
            // Discard the in-progress edits; parent clears editingId and the
            // key swap remounts with the stored HTML.
            props.onCancelEdit();
          }
        }}
        onContextMenu={(e) => {
          if (!editing) return; // 非编辑态保持节点操作菜单
          e.preventDefault();
          e.stopPropagation();
          openContextMenu(e.clientX, e.clientY, [
            { label: t("copyNode"), disabled: !selText(), onClick: copySel },
            { label: t("cutNode"), disabled: !selText(), onClick: cutSel },
            { label: t("pasteNode"), onClick: () => void pasteFromClipboard() },
          ]);
        }}
        onPaste={(e) => {
          if (pasteFallback.current) { window.clearTimeout(pasteFallback.current); pasteFallback.current = 0; }
          // sanitize HTML paste; allow plain text and images via files handled globally
          e.preventDefault();
          const html = e.clipboardData.getData("text/html");
          const text = e.clipboardData.getData("text/plain");
          // 代码段内只粘纯文本，保持源码形态。
          const sel = window.getSelection();
          if (sel && caretContainerEl(sel)?.closest("pre.mm-code")) {
            if (text) document.execCommand("insertText", false, text);
            return;
          }
          if (html) {
            const clean = sanitizeLite(html);
            document.execCommand("insertHTML", false, clean);
          } else if (text && looksLikeMarkdown(text)) {
            // Markdown 智能解析：保留标题/加粗/列表/代码块等基本排版。
            document.execCommand("insertHTML", false, sanitizeLite(mdToNodeHtml(text)));
          } else if (text) {
            document.execCommand("insertText", false, text);
          }
        }}
        dangerouslySetInnerHTML={editing ? undefined : { __html: node.textHtml || "" }}
      />
      </>
      )}
      {!editing && !node.collapsed && node.textHtml === "" && (
        <span
          className="mm-empty-hint"
          style={centroid ? { left: centroid.x - 5, top: centroid.y - 9 } : undefined}
        >…</span>
      )}

      {node.recordId && !editing && (() => {
        const isFileAnchor = node.recordId.startsWith("file:");
        const fileName = isFileAnchor
          ? (node.recordId.slice(5).split(/[\\/]/).pop() ?? "file")
          : "";
        return (
          <button
            type="button"
            className="record-badge"
            title={isFileAnchor ? `${t("anchorBadge")}: ${fileName}` : t("openLinkedRecord")}
            onClick={(e) => {
              e.stopPropagation();
              window.dispatchEvent(new CustomEvent("variable:mm-open-record", { detail: node.recordId }));
            }}
          >
            {isFileAnchor ? <Link2 size={10} /> : <FileText size={10} />} {isFileAnchor ? t("anchorBadge") : t("linkedBadge")}
          </button>
        );
      })()}

      {/* Contract-4: bottom quick-action bar. Hidden by default; fades in only
          while the pointer is over the frame's lower half / the bar itself.
          All clicks stop propagation so the canvas never reacts. */}
      {(selected || editing) && !node.locked && (
        <div
          className={`node-actions ${actionsHot ? "show" : ""}`}
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.preventDefault()}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <button type="button" className="icon-btn tiny" data-tip={t("edit")} aria-label={t("edit")} onClick={(e) => { e.stopPropagation(); setFocusedViaAction(true); props.onAction("edit"); }}>
            <Pencil size={12} />
          </button>
          <button type="button" className="icon-btn tiny" data-tip={t("duplicate")} aria-label={t("duplicate")} onClick={(e) => { e.stopPropagation(); props.onAction("duplicate"); }}>
            <Copy size={12} />
          </button>
          <button type="button" className="icon-btn tiny" data-tip={t("connect")} aria-label={t("connect")} onClick={(e) => { e.stopPropagation(); props.onAction("connect"); }}>
            <Link2 size={12} />
          </button>
          <button type="button" className={`icon-btn tiny ${props.menuAnchor ? "active" : ""}`} data-tip={t("more")} aria-label={t("more")} onClick={(e) => { e.stopPropagation(); props.onAction("more"); }}>
            <MoreHorizontal size={12} />
          </button>
          {/* contract-4: explicit delete shortcut inside the sensing zone */}
          <span className="dock-sep" />
          <button type="button" className="icon-btn tiny danger-hover" data-tip={t("deleteNode")} aria-label={t("deleteNode")} onClick={(e) => { e.stopPropagation(); setActionsHot(false); props.onAction("delete"); }}>
            <Trash2 size={12} />
          </button>
        </div>
      )}
      {selected && node.locked && (
        <div className="locked-hint">🔒</div>
      )}

      {/* Contract-3: bounding box + handles ONLY in deep-edit / free-transform
          mode (plus the module-0 fade-out window), with a fade-in. Handles
          stay MOUNTED while the node remains selected (fully transparent) so
          exiting deep-edit fades them out symmetrically; on deselect the
          sel-fade animation carries them away. */}
      {(effShowBox || selected) && !node.locked && !node.collapsed && handles.map((h) => (
        <span
          key={h}
          className={`resize-handle h-${h}`}
          data-handle={h}
          style={{ width: handleSize, height: handleSize }}
          onPointerDown={(e) => {
            e.stopPropagation();
            e.preventDefault();
            props.onResizeStart(e, h);
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        />
      ))}
      {/* True polygon vertex handles (deep-edit / free-transform / fade-out). */}
      {effShowBox && isPolyShape && !node.locked && !node.collapsed &&
        shapePoints(node.shape, renderW, renderH).map((p, i) => (
          <span
            key={`vh-${i}`}
            className="resize-handle vh"
            data-vertex={i}
            style={{ left: p.x, top: p.y, width: handleSize, height: handleSize }}
            onPointerDown={(e) => {
              e.stopPropagation();
              e.preventDefault();
              props.onVertexResizeStart?.(e, i);
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          />
        ))}
    </div>
  );
}

function el_events(el: HTMLElement, fn: () => void): void {
  el.addEventListener("input", fn);
  el.addEventListener("focus", fn);
}
function detach_events(el: HTMLElement | null, fn: () => void): void {
  el?.removeEventListener("input", fn);
  el?.removeEventListener("focus", fn);
}

/** Tiny inline sanitizer for contentEditable paste (full sanitize on save). */
function sanitizeLite(html: string): string {
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  tpl.content.querySelectorAll("script,style,iframe,object,embed,link,meta").forEach((n) => n.remove());
  tpl.content.querySelectorAll("*").forEach((n) => {
    for (const attr of Array.from(n.attributes)) {
      if (attr.name.startsWith("on")) n.removeAttribute(attr.name);
      if (attr.name === "src" && /^(javascript|data:text)/i.test(attr.value)) n.removeAttribute(attr.name);
    }
  });
  return tpl.innerHTML;
}

function stripTagsLocal(html: string): string {
  const d = new DOMParser().parseFromString(html, "text/html");
  return (d.body.textContent ?? "").trim();
}

