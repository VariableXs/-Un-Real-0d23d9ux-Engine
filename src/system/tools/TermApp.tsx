import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import { d2Store } from "../../features/desktopxp/d2store";
import {
  Scrollback,
  dragDivider,
  frameAccount,
  layoutTree,
  leafIds,
  lineWidth,
  splitPane,
  FONT_SIZE_STEPS,
  TRUNCATION_NOTICE,
  type PaneTree,
} from "../../features/desktopxp/termcore";
import { Palette, BUILTINS, fillTemplate, templateVars } from "../../features/desktopxp/termpalette";
import "../../styles/desktop-d2.css";

/**
 * F095 终端应用 2.0 / F096 命令面板 · VWM 工具窗口（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据对位：
 * - 10 万行回看：Scrollback 环形双上限 + 头部截断提示行钉头（D12 同源）；
 * - CJK 混排对齐：lineWidth 唯一口径（组合字符零宽/CJK 全宽占 2 列）；
 * - 分屏拖拽实时重排：二叉树布局 + 6px 热区分隔条（面积守恒），每叶独立会话；
 * - 命令面板：内置 12 条全可达 + 收藏 frecency + 模板变量 {host} 回填。
 *
 * 诚实边界：本窗口命令解释器为 Varix 展示壳（help/echo/date/seq/clear/cjk/
 * export）——真实 ConDrv 命令链是 F012（AI-C2 面），此处不冒充系统壳。
 * 面板热键走 React 焦点内 onKeyDown（终端聚焦即生效，零全局注册冲突）。
 */

interface TermTab { id: number; name: string; tree: PaneTree }

let tabSeq = 1;
let sessionSeq = 1;

/** 虚拟滚动窗口行数（只渲染可视——10 万行回看的渲染面判据）。 */
const TERM_VIRT_WIN = 400;

function newTab(name: string): TermTab {
  return { id: tabSeq++, name, tree: { kind: "leaf", id: sessionSeq++, ratio: 1 } };
}

/** 展示壳内建命令（帮助文案 = 命令清单——诚实面）。 */
const SHELL_HELP = [
  "help              显示本清单",
  "echo <文本>       原样输出",
  "date              当前时刻",
  "seq <n>           灌入 n 行测试输出（回看压测）",
  "clear             清屏",
  "cjk               CJK 混排对齐样张（列不错位自检）",
  "export            导出会话文本（含时间戳与退出码）",
];

function runShell(cmd: string, scrollback: Scrollback, nowMs: number): { exit: number; exported?: string; clear?: boolean } {
  const trimmed = cmd.trim();
  scrollback.push(`$ ${trimmed}`, nowMs);
  if (trimmed === "") return { exit: 0 };
  const [head, ...rest] = trimmed.split(/\s+/);
  const arg = rest.join(" ");
  switch (head) {
    case "help":
      for (const l of SHELL_HELP) scrollback.push(`  ${l}`, nowMs);
      return { exit: 0 };
    case "echo":
      scrollback.push(arg, nowMs);
      return { exit: 0 };
    case "date":
      scrollback.push(new Date().toISOString(), nowMs);
      return { exit: 0 };
    case "seq": {
      const n = Math.max(1, Math.min(150_000, Number(arg) || 10));
      for (let i = 1; i <= n; i++) scrollback.push(`line ${i}`, nowMs);
      scrollback.push(`（共 ${n} 行——回看上限 100000 行/80MB 双保险）`, nowMs);
      return { exit: 0 };
    }
    case "clear":
      return { exit: 0, clear: true };
    case "cjk":
      scrollback.push("中文 English 混排 123 aligned", nowMs);
      scrollback.push("中文English混排123aligned", nowMs);
      scrollback.push("宽度口径：CJK=2 列 · ASCII=1 列 · 组合符=0 列", nowMs);
      scrollback.push(`两行显示列宽：${lineWidth("中文 English 混排 123 aligned")} / ${lineWidth("中文English混排123aligned")}`, nowMs);
      return { exit: 0 };
    case "export":
      return { exit: 0, exported: scrollback.export(0) };
    default:
      scrollback.push(`vxshell: 未找到命令「${head}」——输入 help 看内建清单（真实命令链走 F012 ConDrv）`, nowMs);
      return { exit: 127 };
  }
}

export function TermApp(_props: { winId: string }): React.ReactElement {
  const [tabs, setTabs] = useState<TermTab[]>(() => [newTab("主会话")]);
  const [activeTab, setActiveTab] = useState(0);
  const [sessions, setSessions] = useState<Map<number, Scrollback>>(() => new Map());
  const [exits, setExits] = useState<Record<number, number>>({});
  const [inputs, setInputs] = useState<Record<number, string>>({});
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [hitIndex, setHitIndex] = useState(0);
  const paletteRef = useRef<Palette>(new Palette());
  const wrapRef = useRef<HTMLDivElement>(null);
  const outRef = useRef<HTMLDivElement | null>(null);
  const divDrag = useRef<{ dir: "h" | "v"; startX: number; startY: number; base: PaneTree } | null>(null);
  const [focusLeaf, setFocusLeaf] = useState(1);
  const [follow, setFollow] = useState(true);
  /** 虚拟滚动视口顶行（null = 跟随贴底）。 */
  const [viewTop, setViewTop] = useState<number | null>(null);
  const [, force] = useState(0);

  const tab = tabs[activeTab]!;
  const fsIdx = d2Store.getWith("term2", "fontSizeStep", 3);
  const fontSize: number = FONT_SIZE_STEPS[Math.max(0, Math.min(7, fsIdx))] ?? 16;
  /** 虚拟窗口行数与行高（与 CSS line-height 1.45 同源）。 */
  const lineH = Math.max(10, Math.round(fontSize * 1.45));
  const cursorStyle = d2Store.getWith("term2", "cursorStyle", "block");
  const cursorBlink = d2Store.getWith("term2", "cursorBlink", true);
  const paletteEnabled = d2Store.getWith("termpalette", "enabled", true);

  const sessionOf = useCallback((leafId: number): Scrollback => {
    let sb = sessions.get(leafId);
    if (!sb) {
      sb = new Scrollback();
      sb.push("Varix 展示壳——help 看命令清单；seq 150000 压测回看；cjk 看混排对齐。", Date.now());
      sb.push("真实 ConDrv 命令链由 F012（应用兼容域）承接。", Date.now());
      setSessions((cur) => new Map(cur).set(leafId, sb!));
    }
    return sb;
  }, [sessions]);

  useEffect(() => {
    if (!follow || viewTop !== null || !outRef.current) return;
    outRef.current.scrollTop = outRef.current.scrollHeight;
  });

  const exec = useCallback((cmdline: string, leafId: number): void => {
    const nowMs = Date.now();
    const sb = sessionOf(leafId);
    const r = runShell(cmdline, sb, nowMs);
    if (r.clear) {
      setSessions((cur) => new Map(cur).set(leafId, new Scrollback()));
    }
    setExits((cur) => ({ ...cur, [leafId]: r.exit }));
    if (r.exported) {
      void navigator.clipboard?.writeText(r.exported).then(
        () => pushToast("success", "会话已导出到剪贴板", `${sb.length} 行 · 含时间戳与退出码元数据`),
        (e) => pushToast("error", "导出失败", String(e)),
      );
    }
    force((v) => v + 1);
  }, [sessionOf]);

  const isBuiltin = useCallback((cmdline: string): boolean => BUILTINS.some((b) => b.id === cmdline), []);

  const execPalette = useCallback((cmdline: string): void => {
    if (isBuiltin(cmdline)) {
      const leaves = leafIds(tab.tree);
      switch (cmdline) {
        case "clearScreen": exec("clear", focusLeaf); break;
        case "exportSession": exec("export", focusLeaf); break;
        case "splitH": setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: splitPane(t.tree, leaves[leaves.length - 1]!, "h") } : t))); break;
        case "splitV": setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: splitPane(t.tree, leaves[leaves.length - 1]!, "v") } : t))); break;
        case "fontUp": d2Store.set("term2", { fontSizeStep: Math.min(7, fsIdx + 1) }); break;
        case "fontDown": d2Store.set("term2", { fontSizeStep: Math.max(0, fsIdx - 1) }); break;
        case "newTab": {
          const t = newTab(`会话 ${tabs.length + 1}`);
          setTabs((cur) => [...cur, t]);
          setActiveTab(tabs.length);
          break;
        }
        case "closeTab": {
          if (tabs.length > 1) {
            setTabs((cur) => cur.filter((x) => x.id !== tab.id));
            setActiveTab((i) => Math.max(0, i - 1));
          } else pushToast("info", "最后一个标签页不可关", "用窗口标题栏 × 关闭整个终端");
          break;
        }
        case "closeOtherPanes": {
          const first = leaves[0]!;
          setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: { kind: "leaf", id: first, ratio: 1 } } : t)));
          setFocusLeaf(first);
          break;
        }
        case "toggleFollow": setFollow((f) => !f); break;
        default:
          pushToast("info", "复制/搜索模式", "域内语义：选区即复制（浏览器面诚实降级——完整面在内核模型层）");
      }
    } else {
      exec(cmdline, focusLeaf);
    }
    setPaletteOpen(false);
  }, [exec, focusLeaf, fsIdx, isBuiltin, tab.id, tab.tree, tabs.length]);

  const hits = useMemo(
    () => (paletteOpen ? paletteRef.current.search(paletteQuery) : []),
    [paletteOpen, paletteQuery],
  );

  const onPaneKey = (e: React.KeyboardEvent): void => {
    if (paletteOpen) {
      if (e.key === "Escape") { setPaletteOpen(false); e.preventDefault(); return; }
      if (e.key === "ArrowDown") { setHitIndex((i) => Math.min(hits.length - 1, i + 1)); e.preventDefault(); return; }
      if (e.key === "ArrowUp") { setHitIndex((i) => Math.max(0, i - 1)); e.preventDefault(); return; }
      if (e.key === "Enter" && hits[hitIndex]) {
        const h = hits[hitIndex]!;
        const vars = templateVars(h.command.cmdline);
        execPalette(vars.length > 0 ? fillTemplate(h.command.cmdline, vars.map((v) => [v, v === "host" ? "varix" : ""])) : h.command.cmdline);
        e.preventDefault();
      }
      return;
    }
    if (e.ctrlKey && e.shiftKey && (e.key === "P" || e.key === "p")) {
      if (!paletteEnabled) {
        pushToast("info", "命令面板已关闭", "在 设置 → 桌面体验 → 终端与命令面板 可重新开启");
        return;
      }
      setPaletteOpen(true);
      setPaletteQuery("");
      setHitIndex(0);
      e.preventDefault();
    }
  };

  const onDividerMove = (e: React.PointerEvent): void => {
    if (!divDrag.current || !wrapRef.current) return;
    const span = divDrag.current.dir === "h" ? wrapRef.current.clientWidth : wrapRef.current.clientHeight;
    const delta = (divDrag.current.dir === "h" ? e.clientX - divDrag.current.startX : e.clientY - divDrag.current.startY) / span;
    setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: dragDivider(divDrag.current!.base, divDrag.current!.dir, delta, 1) } : t)));
  };

  const rects = layoutTree(tab.tree, 0, 0, wrapRef.current?.clientWidth ?? 800, wrapRef.current?.clientHeight ?? 480);
  const focused = sessionOf(focusLeaf);
  const cellsAccount = frameAccount(
    Math.floor((wrapRef.current?.clientWidth ?? 800) / (fontSize * 0.62)),
    Math.floor(((wrapRef.current?.clientHeight ?? 480) - 72) / (fontSize * 1.45)),
  );

  return (
    <div className="d2app" onKeyDown={onPaneKey} tabIndex={-1}>
      <div className="d2app-toolbar" role="tablist" aria-label="终端标签页">
        {tabs.map((t, i) => (
          <span key={t.id} className={i === activeTab ? "d2-tabbtn d2-tabbtn--on" : "d2-tabbtn"}>
            <button type="button" role="tab" aria-selected={i === activeTab} onClick={() => setActiveTab(i)}>{t.name}</button>
            {tabs.length > 1 && (
              <button type="button" aria-label={`关闭 ${t.name}`} className="d2-tabbtn-x" onClick={() => {
                setTabs((cur) => cur.filter((x) => x.id !== t.id));
                setActiveTab((cur) => Math.max(0, cur > i ? cur - 1 : cur));
              }}>×</button>
            )}
          </span>
        ))}
        <button type="button" onClick={() => { const t = newTab(`会话 ${tabs.length + 1}`); setTabs((cur) => [...cur, t]); setActiveTab(tabs.length); }}>＋ 标签页</button>
        <span className="d2app-sep" />
        <button type="button" onClick={() => setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: splitPane(t.tree, leafIds(t.tree)[leafIds(t.tree).length - 1]!, "h") } : t)))}>左右分屏</button>
        <button type="button" onClick={() => setTabs((cur) => cur.map((t) => (t.id === tab.id ? { ...t, tree: splitPane(t.tree, leafIds(t.tree)[leafIds(t.tree).length - 1]!, "v") } : t)))}>上下分屏</button>
        <span className="d2app-sep" />
        <button type="button" onClick={() => d2Store.set("term2", { fontSizeStep: Math.min(7, fsIdx + 1) })} aria-label="字号增大">A＋</button>
        <button type="button" onClick={() => d2Store.set("term2", { fontSizeStep: Math.max(0, fsIdx - 1) })} aria-label="字号减小">A－</button>
        <button type="button" onClick={() => setFollow((f) => !f)} aria-pressed={follow}>{follow ? "跟随中" : "已暂停跟随"}</button>
        <span className="d2app-title">Ctrl+Shift+P 命令面板</span>
      </div>

      <div
        ref={wrapRef}
        className="d2-term"
        onPointerMove={onDividerMove}
        onPointerUp={() => { divDrag.current = null; }}
        onPointerLeave={() => { divDrag.current = null; }}
      >
        {rects.map((r) => {
          const sb = sessionOf(r.id);
          const isFocus = r.id === focusLeaf;
          const totalRows = sb.length;
          // 虚拟滚动窗口：只渲染可视行（判据本体）——spacer 撑出真实滚动条。
          const startRow = viewTop === null ? Math.max(0, totalRows - TERM_VIRT_WIN) : Math.max(0, Math.min(viewTop - 20, totalRows - 1));
          const winRows = sb.slice(startRow, startRow + TERM_VIRT_WIN + 40);
          return (
            <div
              key={r.id}
              className="d2-term-pane"
              style={{ left: r.x, top: r.y, width: r.w, height: r.h }}
              onPointerDown={() => setFocusLeaf(r.id)}
            >
              <div
                className={`d2-term-out${isFocus ? "" : " d2-term-out--dim"}`}
                ref={isFocus ? outRef : undefined}
                onScroll={isFocus ? (e) => {
                  const el = e.currentTarget;
                  const maxTop = Math.max(0, totalRows * lineH - el.clientHeight);
                  const top = Math.floor(el.scrollTop / lineH);
                  setViewTop(el.scrollTop >= maxTop - lineH * 2 ? null : top);
                } : undefined}
              >
                <div style={{ height: totalRows * lineH + 8, position: "relative" }}>
                  <div style={{ position: "absolute", top: startRow * lineH, left: 0, right: 0 }}>
                    {winRows.map((l, i) => (
                      <div key={startRow + i} className={l.text.startsWith("$") ? "d2-term-line--cmd" : l.text === TRUNCATION_NOTICE ? "d2-term-line--notice" : undefined}>{l.text || " "}</div>
                    ))}
                    <span className={`d2-term-caret d2-term-caret--${isFocus ? cursorStyle : "off"}${cursorBlink && isFocus ? " d2-term-caret--blink" : ""}`} aria-hidden />
                  </div>
                </div>
              </div>
            </div>
          );
        })}
        {tab.tree.kind === "split" && rects.length > 1 && rects.slice(0, -1).map((r, i) => {
          const next = rects[i + 1]!;
          const dir = next.x > r.x + r.w - 1 ? "h" : "v";
          return (
            <div
              key={`div${i}`}
              className={`d2-term-divider d2-term-divider--${dir}`}
              style={dir === "h" ? { left: r.x + r.w - 3, top: 0, height: "100%" } : { top: r.y + r.h - 3, left: 0, width: "100%" }}
              onPointerDown={(e) => {
                divDrag.current = { dir, startX: e.clientX, startY: e.clientY, base: tab.tree };
                (e.target as Element).setPointerCapture?.(e.pointerId);
              }}
              role="separator"
              aria-label="分屏分隔条（拖拽实时重排）"
              aria-orientation={dir === "h" ? "vertical" : "horizontal"}
            />
          );
        })}
        {paletteOpen && (
          <div className="d2-palette-mask" onPointerDown={() => setPaletteOpen(false)}>
            <div className="d2-palette" role="dialog" aria-label="终端命令面板" onPointerDown={(e) => e.stopPropagation()}>
              <input
                className="d2-palette-search"
                autoFocus
                placeholder="搜索命令（子序列 + 拼音首字母，如 qp=清屏）"
                value={paletteQuery}
                onChange={(e) => { setPaletteQuery(e.target.value); setHitIndex(0); }}
                aria-label="面板搜索"
              />
              <div className="d2-palette-list">
                {hits.length === 0 && <div className="d2-palette-empty">无命中——试试「清屏」「qp」「分屏」。</div>}
                {hits.map((h, i) => (
                  <button
                    key={h.command.cmdline + h.command.name}
                    type="button"
                    className={i === hitIndex ? "d2-palette-item d2-palette-item--on" : "d2-palette-item"}
                    onPointerEnter={() => setHitIndex(i)}
                    onClick={() => {
                      const vars = templateVars(h.command.cmdline);
                      execPalette(vars.length > 0 ? fillTemplate(h.command.cmdline, vars.map((v) => [v, v === "host" ? "varix" : ""])) : h.command.cmdline);
                    }}
                  >
                    <span>{h.command.name}</span>
                    <span className="d2-palette-cmd">{h.command.builtin ? `内置 · ${h.command.cmdline}` : h.command.cmdline}</span>
                  </button>
                ))}
              </div>
              <div className="d2app-status" style={{ borderTop: "1px solid rgba(255,255,255,0.1)" }}>
                <span>↑↓ 选择 · Enter 执行 · Esc 关闭</span>
                <span>{hits.length} 命中</span>
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="d2-term-inrow">
        <span className="d2-term-prompt">varix:~$</span>
        <input
          value={inputs[focusLeaf] ?? ""}
          onChange={(e) => setInputs((cur) => ({ ...cur, [focusLeaf]: e.target.value }))}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              exec(inputs[focusLeaf] ?? "", focusLeaf);
              setInputs((cur) => ({ ...cur, [focusLeaf]: "" }));
            }
          }}
          placeholder="输入命令（help / seq 150000 / cjk / export）"
          aria-label="终端输入行"
        />
      </div>

      <div className="d2app-status">
        <span>焦点叶 #{focusLeaf} · exit={exits[focusLeaf] ?? 0}</span>
        <span>回看 {focused.length} 行 / {(focused.bytesUsed / 1024).toFixed(0)} KB（上限 10 万行 · 80MB）</span>
        <span>截断 {focused.truncations} 次（满时丢最早 + 提示行钉头）</span>
        <span className={cellsAccount.withinBudget ? "d2-ok" : "d2-bad"}>帧账 {cellsAccount.cells} 格 ≈ {(cellsAccount.costUs / 1000).toFixed(1)}ms / 12.5ms</span>
        <span>字号 {fontSize}px</span>
        <span>面板 {paletteRef.current.openCount} 开 · 超预算 {paletteRef.current.overBudget}</span>
      </div>
    </div>
  );
}
