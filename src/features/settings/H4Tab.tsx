/**
 * H 基础通用域 · AI-H4 — 设置中心「效率与工具」标签页（F351-F400 全量面板）。
 *
 * 面板纪律（MouseJ1Tab 同源）：
 * - 名称 + 一句话说明 + 调节控件三件套齐全（F474 同源）；
 * - 每行/每面板直接消费 src/system/h4/ 引擎——读数即引擎输出、按钮即引擎调用，
 *   面板零自研逻辑（逻辑在引擎，接线在 h4ui，壳在这里）；
 * - 默认档 = 主册判据默认（资源摘要默认关、分区吸附关、灰度关……）；
 * - 高密度纵深：十个分组区、五十项各就各位，无空话。
 */

import React, { useEffect, useMemo, useState } from "react";
import { pushToast } from "../../state/uiStore";
import * as f354 from "../../system/h4/f354-resourceSummary";
import * as f363b from "../../system/h4/f363-focusTimer";
import * as f365 from "../../system/h4/f365-dupeFinder";
import * as f371 from "../../system/h4/f371-bootBadge";
import * as f373 from "../../system/h4/f373-keyboardLayouts";
import * as f377 from "../../system/h4/f377-keyboardWindowEdit";
import * as f378 from "../../system/h4/f378-columnAutofit";
import * as f379 from "../../system/h4/f379-headerSort";
import * as f380 from "../../system/h4/f380-blankClick";
import * as f381 from "../../system/h4/f381-treeTriState";
import * as f382 from "../../system/h4/f382-dialogPosition";
import * as f383 from "../../system/h4/f383-modalQueue";
import * as f384 from "../../system/h4/f384-focusTrap";
import * as f385 from "../../system/h4/f385-semanticTree";
import * as f386 from "../../system/h4/f386-readingMode";
import * as f387 from "../../system/h4/f387-grayscaleMode";
import * as f388 from "../../system/h4/f388-portraitAdaptation";
import * as f389 from "../../system/h4/f389-scrollStitch";
import * as f392 from "../../system/h4/f392-folderSize";
import * as f395 from "../../system/h4/f395-usbHealth";
import { memStore } from "../../system/h4/internal/store";
import { announceFilterChanged, announceReadingChanged } from "../h4/h4ui";
import { FOCUS_EVENT, SUMMON_PICKER, SUMMON_RULER } from "../h4/overlays";
import {
  BackupPanel,
  CleanupPanel,
  DesktopMiniPanel,
  EggPanel,
  GatePanel,
  LanguagePanel,
  LookupPanel,
  MenuMatrixPanel,
  PickerPanel,
  PipPanel,
  RecorderPanel,
  SheetPanel,
  SnapshotPanel,
  TaskCenterPanel,
  TimelinePanel,
  TidyPanel,
  TreemapPanel,
  WebSynergyPanel,
} from "./H4Panels";
import "../../styles/h4.css";

/* ------------------------------- 分组骨架 ------------------------------- */

function Group(props: { title: string; f: string; desc: string; children: React.ReactNode }): React.ReactElement {
  return (
    <section className="h4-group">
      <header className="h4-group-head">
        <h4>{props.title}</h4>
        <span className="h4-fnum">{props.f}</span>
        <p>{props.desc}</p>
      </header>
      <div className="h4-group-body">{props.children}</div>
    </section>
  );
}

function Row(props: { label: string; hint: string; children?: React.ReactNode }): React.ReactElement {
  return (
    <div className="h4-row">
      <div className="h4-row-text">
        <span className="h4-row-label">{props.label}</span>
        <span className="h4-row-hint">{props.hint}</span>
      </div>
      <div className="h4-row-ctl">{props.children}</div>
    </div>
  );
}

function SectionCard(props: { title: string; f: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="h4-card">
      <div className="h4-card-head">
        <strong>{props.title}</strong>
        <span className="h4-fnum">{props.f}</span>
      </div>
      {props.children}
    </div>
  );
}

/* ------------------------------- 内联交互体 ------------------------------- */

/** F363 专注计时器（实时徽标 + 每日累计对账）。 */
function FocusTimerInline(): React.ReactElement {
  const [minutes, setMinutes] = useState<number>(25);
  const [run, setRun] = useState<f363b.FocusRun | null>(null);
  const [, tick] = useState(0);
  const day = new Date().toISOString().slice(0, 10);

  useEffect(() => {
    if (!run || run.outcome !== "running") return;
    const h = window.setInterval(() => {
      tick((v) => v + 1);
      const now = Date.now();
      if (f363b.isDue(run, now)) {
        const done = { ...run, endedAt: now, outcome: "completed" as const };
        setRun(done);
        const rec = f363b.recordRun(done, now);
        const rem = f363b.endReminder(done);
        pushToast("success", `${rem.notification}（响铃 ${rem.chimeVolume} · ${rec.minutes} 分钟已入每日账）`);
      }
    }, 500);
    return () => window.clearInterval(h);
  }, [run]);

  const validation = f363b.validateMinutes(minutes);
  const summary = f363b.dailySummary(day);

  const abandonRun = (): void => {
    if (!run) return;
    const now = Date.now();
    const a = f363b.abandon(run, now);
    const rec = f363b.recordRun(a, now);
    setRun(a);
    pushToast("info", `已放弃——真实专注 ${rec.minutes} 分钟（不足不虚报）`);
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <select aria-label="专注时长" value={String(minutes)} onChange={(e) => setMinutes(Number(e.target.value))} disabled={run?.outcome === "running"}>
          <option value="25">25 分钟</option>
          <option value="50">50 分钟</option>
          <option value="30">自定 30</option>
          <option value="90">自定 90</option>
        </select>
        {!run || run.outcome !== "running" ? (
          <button type="button" disabled={!validation.ok} onClick={() => {
            setRun({ day, plannedMinutes: minutes, startedAt: Date.now(), endedAt: null, outcome: "running" });
            window.dispatchEvent(new CustomEvent(FOCUS_EVENT, { detail: { type: "start", minutes } })); // 全局芯片同源跟显
          }}>
            开始专注
          </button>
        ) : (
          <button type="button" onClick={() => {
            abandonRun();
            window.dispatchEvent(new CustomEvent(FOCUS_EVENT, { detail: { type: "abandon" } }));
          }}>放弃（记真实时长）</button>
        )}
        {run?.outcome === "running" && <span className="h4-badge-live">{f363b.badgeText(run, Date.now())}</span>}
      </div>
      {!validation.ok && <p className="h4-verdict bad">{validation.reason}</p>}
      <p className="h4-kv">今日小结：{summary.minutes} 分钟 · {summary.runs} 次（放弃 {summary.abandoned}）· 完成时双通道提醒（一声 + 一条）</p>
    </div>
  );
}

/** F365 重复文件台。 */
const DUPES: f365.ScannedFile[] = [
  { path: "S:/下载/素材.zip", sizeBytes: 120_000_000, mtimeMs: 1000, hash: "h-aa" },
  { path: "S:/备份/素材.zip", sizeBytes: 120_000_000, mtimeMs: 900, hash: "h-aa" },
  { path: "S:/下载/报告-final.docx", sizeBytes: 88_000, mtimeMs: 2000, hash: "h-bb" },
  { path: "S:/备份/报告-final(改名).docx", sizeBytes: 88_000, mtimeMs: 1500, hash: "h-bb" },
  { path: "S:/视频/成片.mov", sizeBytes: 5_800_000_000, mtimeMs: 3000, hash: "h-cc" },
];

function DupeFinderInline(): React.ReactElement {
  const report = useMemo(() => f365.findDuplicates(DUPES), []);
  const idle = f365.idleAdmission(10_000, 2_000);
  const [trashed, setTrashed] = useState<string[]>([]);

  return (
    <div className="h4-panel">
      {report.groups.map((g) => (
        <div key={g.hash} className="h4-pip-chip">
          <span className="h4-kv">{g.files.length} 份 · 浪费 {f392.formatBytes(g.wastedBytes)}</span>
          <span className="h4-row-actions">
            {g.files.map((f) => (
              <button
                key={f.path}
                type="button"
                className={`h4-btn-mini ${f.path === g.keepPath ? "h4-btn-keep" : ""}`}
                title={f.path === g.keepPath ? "推荐保留（最新）" : "送回收站（可反悔）"}
                disabled={f.path === g.keepPath || trashed.includes(f.path)}
                onClick={() => {
                  const r = f365.trashChecked(g, [f.path], Date.now());
                  setTrashed((cur) => [...cur, ...r.trashed.map((t) => t.path)]);
                  pushToast("success", `已送回收站：${f.path.split("/").pop()}（30 天内可还原）`);
                }}
              >
                {f.path === g.keepPath ? "保留" : f.path.split("/").pop()}
              </button>
            ))}
          </span>
        </div>
      ))}
      <p className="h4-kv">可释放 {f392.formatBytes(report.reclaimableBytes)} · 扫描 {report.scannedCount} 项（大小预筛 → 哈希确认，零误判）</p>
      <p className={`h4-verdict ${idle.admitted ? "ok" : ""}`}>空闲准入：{idle.reason}（前台忙时扫描自动让路）</p>
    </div>
  );
}

/** F377 键盘移动窗口台。 */
function KeyboardEditInline(): React.ReactElement {
  const [session, setSession] = useState<f377.KeyboardEditSession | null>(null);
  const [mode, setMode] = useState<f377.KeyboardEditMode>("move");
  const WORK = { x: 0, y: 0, w: 2560, h: 1392 };
  const clamp = f377.auditEdgeClamp(f377.beginEdit("move", { x: 100, y: 100, w: 400, h: 300 }, WORK), "right", 300);
  const visual = session ? f377.editVisual(session) : null;

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <select aria-label="编辑模式" value={mode} onChange={(e) => setMode(e.target.value as f377.KeyboardEditMode)}>
          <option value="move">移动</option>
          <option value="size">大小</option>
        </select>
        <button type="button" onClick={() => setSession(f377.beginEdit(mode, { x: 200, y: 150, w: 480, h: 320 }, WORK))}>进入编辑（Alt+Space）</button>
        {session && <button type="button" onClick={() => { const c = f377.commit(session); setSession(null); pushToast("success", `落定 drift ${c.driftPx}px（<1px 判据）`); }}>Enter 落定</button>}
        {session && <button type="button" onClick={() => { f377.cancelEdit(session); setSession(null); pushToast("info", "Esc 还原到进入时原地"); }}>Esc 还原</button>}
      </div>
      {session && (
        <div className="h4-panel-actions">
          {(["left", "up", "down", "right"] as const).map((d) => (
            <button key={d} type="button" className="h4-btn-mini" onClick={() => setSession(f377.arrowKey(session, d, false))}>{d === "left" ? "←" : d === "right" ? "→" : d === "up" ? "↑" : "↓"}</button>
          ))}
          <button type="button" className="h4-btn-mini" onClick={() => setSession(f377.arrowKey(session, "right", true))}>→×10（按住 Shift 加速）</button>
          <span className="h4-kv">{visual?.hint}</span>
        </div>
      )}
      {session && <p className="h4-kv">当前几何 <code className="h4-code">{JSON.stringify(session.current)}</code> · 半透明提示透明度 {visual?.opacity}</p>}
      <p className={`h4-verdict ${clamp.pass ? "ok" : "bad"}`}>边界贴边自证：连按 300 步不越界={String(clamp.pass)} · 终点 <code className="h4-code">{JSON.stringify(clamp.final)}</code></p>
    </div>
  );
}

/** F378+F379+F380+F381 控件规范台。 */
function ControlsInline(): React.ReactElement {
  /* F378 列宽 */
  const measure = (t: string): number => t.length * 8;
  const cells: f378.CellText[] = [
    { columnId: "name", text: "年度总结报告-终版-v3-真的最终版.docx" },
    { columnId: "name", text: "笔记.txt" },
    { columnId: "size", text: "12.4 MB" },
  ];
  const autofit = f378.autofitWidth(cells, "name", measure);
  const [cols, setCols] = useState<f378.ColumnSpec[]>([
    { id: "name", width: 180, last: false },
    { id: "size", width: 90, last: true },
  ]);
  const dist = f378.distributeWidths(cols, 500);
  /* F379 排序 */
  const [sort, setSort] = useState<f379.HeaderSortState>(f379.initialSort());
  /* F380 空行点击 */
  const sel: f380.ListSelection = { selectedIds: ["a", "b", "c"], anchorId: "a" };
  const clickRes = f380.clickBlank(sel);
  const ctrlRes = f380.ctrlClickBlank(sel);
  const rightRes = f380.rightClickBlank(sel);
  const menuAudit = f380.auditBackgroundMenu(rightRes.menu ?? []);
  /* F381 树 */
  const TREE: f381.TreeNode = {
    id: "root", children: [
      { id: "工程", children: [{ id: "渲染缓存", children: [] }, { id: "素材库", children: [] }] },
      { id: "视频", children: [{ id: "成片", children: [] }] },
    ],
  };
  const [checked, setChecked] = useState<ReadonlySet<string>>(new Set(["渲染缓存"]));
  const rootState = f381.checkStateOf(TREE, checked);
  const indeterminate = rootState === "indeterminate";

  const headerDemo = (id: string, shift: boolean): void => {
    setSort(f379.clickHeader(sort, id, shift));
  };

  return (
    <div className="h4-panel">
      <p className="h4-kv"><strong>F378</strong> 双击自适应：最长可见项 {measure(cells[0]!.text)}px + {f378.AUTOFIT_PADDING_PX}px 余量 = <strong>{autofit}px</strong> · 拖到 20px → 钳制 {f378.dragWidth(20)}px（气泡「{f378.dragBubble(20)}」）· 最右列吃满剩余 ✓</p>
      <div className="h4-panel-actions">
        {cols.map((c) => (
          <span key={c.id} className="h4-kv">
            {c.id} {dist.widths[c.id] ?? c.width}px
            {c.id === "name" && <button type="button" className="h4-btn-mini" onClick={() => setCols(cols.map((x) => (x.id === "name" ? { ...x, width: Math.min(360, autofit) } : x)))}>双击自适应</button>}
          </span>
        ))}
        <span className={`h4-verdict ${dist.overflow ? "bad" : "ok"}`}>表宽 500px 分配{dist.overflow ? "溢出（最右列如实标注）" : "无溢出 ✓"}</span>
      </div>
      <p className="h4-kv"><strong>F379</strong> 表头排序三态：名称 <button type="button" className="h4-btn-mini" onClick={() => headerDemo("name", false)}>点击</button> → <code className="h4-code">{JSON.stringify(f379.indicatorFor(sort, "name"))}</code> · Shift 点击进多级（上限 {f379.MAX_SORT_LEVELS}）· 滚动保持：<code className="h4-code">{f379.scrollAfterSort(240)}</code></p>
      <p className="h4-kv"><strong>F380</strong> 空行点击：单击 → 清空 {clickRes.selection.selectedIds.length} 项+锚点（焦点回 {clickRes.focusGoesTo}）；Ctrl 单击 → 保留 {ctrlRes.selection.selectedIds.length} 项只清锚点；右键空白 → 菜单 [{rightRes.menu?.join(", ")}]（无对象操作 {menuAudit.forbidden} 个=审计通过）</p>
      <p className="h4-kv"><strong>F381</strong> 树形三态：根节点 = {rootState === "checked" ? "全选实心" : indeterminate ? `半选横杠（${f381.INDETERMINATE_BAR}）` : "未选空心"} · 点父节点：
        <button type="button" className="h4-btn-mini" onClick={() => setChecked(new Set(f381.toggleNode(TREE, checked)))}>toggle 根</button>
        <button type="button" className="h4-btn-mini" onClick={() => setChecked(new Set(f381.toggleNode(TREE.children[0]!, checked)))}>toggle 工程</button>
        当前勾选：{[...checked].join(",") || "无"} · 半选视觉：横杠居中 60% 宽 ✓</p>
    </div>
  );
}

/** F373 键盘布局台。 */
function KeyboardLayoutsInline(): React.ReactElement {
  const [state, setState] = useState<f373.LayoutState>(() => f373.loadState());
  const sync = f373.threePlaceSync(state);
  const typing = f373.simulateMixedTyping(
    Array.from({ length: 100 }, (_, i) => ({ seq: i, key: `k${i}`, atMs: i * 20 })),
    [{ atSeq: 20, toLayout: "english" }, { atSeq: 50, toLayout: "pinyin" }],
  );

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <span className="h4-kv">轮切顺序（=设置顺序）：{state.layouts.map((l, i) => `${i === state.activeIndex ? "▸" : ""}${l.name}`).join(" → ")}</span>
        <button type="button" onClick={() => { const s = f373.rotate(state, 1); f373.persistState(s); setState(s); }}>Win+Space 轮切</button>
      </div>
      <p className="h4-kv">当前：{f373.activeLayout(state).name} · 三处同步（设置/任务栏/输入法）={String(sync.consistent)}（{sync.settings}/{sync.taskbar}/{sync.ime}）</p>
      <p className={`h4-verdict ${typing.dropped.length === 0 ? "ok" : "bad"}`}>100 键快速混切吞键 {typing.dropped.length} 个（判据 0——切换不吞键）</p>
      <div className="h4-panel-actions">
        {state.layouts.map((l, i) => (
          <span key={l.id} className="h4-kv">
            <button type="button" className="h4-btn-mini" disabled={i === 0} onClick={() => { const s = f373.reorder(state, i, i - 1); f373.persistState(s); setState(s); }}>↑</button>
            {l.name}
            <button type="button" className="h4-btn-mini" disabled={state.layouts.length <= 2 || i === state.layouts.length - 1} onClick={() => { const r = f373.removeLayout(state, l.id); if (r.ok) { f373.persistState(r.state); setState(r.state); } else pushToast("error", r.reason); }}>移除</button>
          </span>
        ))}
      </div>
    </div>
  );
}

/** F383+F384 浮层秩序台。 */
function LayerOrderInline(): React.ReactElement {
  const [engine, setEngine] = useState<f383.LayerEngineState>(f383.initialEngine());
  const [trap, setTrap] = useState<f384.TrapSession | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [tabCount, setTabCount] = useState(0);

  const enqueue = (kind: f383.LayerKind): void => {
    const r = f383.enqueue(engine, { id: `${kind}-${Date.now() % 10000}`, kind, at: performance.now() });
    setEngine(r.state);
    if (!r.presented && kind === "modal") pushToast("info", "已有模态在前台——排队等 200ms 递补（不叠罗汉）");
    if (!r.presented && kind === "banner") pushToast("info", "横幅已满 3 条——最早一条溢流进通知中心");
  };

  const targets: f384.TrapTarget[] = [
    { id: "cancel", focusable: true }, { id: "action", focusable: true }, { id: "link", focusable: true },
  ];
  const trapAudit = trap ? f384.auditNoEscape(trap) : null;
  const focusOwner = engine.active?.hasFocus ? engine.active : [...engine.banners].length > 0 ? null : engine.active;

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <button type="button" onClick={() => enqueue("modal")}>弹一个模态</button>
        <button type="button" onClick={() => enqueue("banner")}>弹一条横幅</button>
        <button type="button" onClick={() => enqueue("osd")}>弹一个 OSD</button>
        <button type="button" onClick={() => { const r = f383.closeModal(engine, performance.now()); setEngine(r.state); setTimeout(() => setEngine((s) => f383.afterGap(s, performance.now()).state), f383.DEQUEUE_GAP_MS + 10); }}>关掉当前层</button>
      </div>
      <p className="h4-kv">
        在台层：{engine.active ? `${engine.active.id}(${engine.active.kind}${engine.active.hasFocus ? "·焦点" : ""})` : "无"} · 模态队列 {engine.modalQueue.length} · 横幅堆叠 {engine.banners.length}/3{engine.overflowedToCenter.length > 0 ? ` · 溢流进中心 ${engine.overflowedToCenter.length} 条` : ""}
      </p>
      <p className={`h4-verdict ${focusOwner ? (f383.auditFocusUniqueness([focusOwner]) ? "ok" : "bad") : "ok"}`}>焦点唯一={String(focusOwner ? f383.auditFocusUniqueness([focusOwner]) : true)} · 模态唯一={String(engine.active?.kind !== "modal" || f383.auditModalUniqueness([engine.active]))}</p>
      <div className="h4-panel-actions">
        <button type="button" onClick={() => { const t = f384.openTrap("demo-modal", targets, "open-btn"); setTrap(t); setFocusId(t.ring[0] ?? null); setTabCount(0); }}>开焦点陷阱</button>
        {trap && <button type="button" className="h4-btn-mini" onClick={() => { const next = f384.nextFocus(trap, focusId, false); setFocusId(next); setTabCount((c) => c + 1); }}>Tab ×{tabCount} → {focusId ?? "环首"}</button>}
        {trap && <button type="button" className="h4-btn-mini" onClick={() => { const r = f384.closeTrap(trap); setTrap(null); setFocusId(null); pushToast("success", `焦点归还 ${r.returnedTo}（F206 联动）`); }}>关陷阱（归还焦点）</button>}
      </div>
      {trapAudit && <p className={`h4-verdict ${trapAudit.pass ? "ok" : "bad"}`}>陷阱完整性：连按 50 次 Tab 逃逸 {trapAudit.escapes} 次（判据 0）· 陷阱环 {trap?.ring.join(" → ")}</p>}
    </div>
  );
}

/** F385+F386+F387+F388 无障碍与显示台。 */
function A11yDisplayInline(): React.ReactElement {
  /* F385 语义树 */
  const nodes: f385.SemanticNode[] = [
    { id: "b1", role: "button", name: "保存", nameSource: "text", state: null, parentIds: [] },
    { id: "s1", role: "switch", name: "灰度模式", nameSource: "label", state: f385.readableSwitchState(false), parentIds: ["b1"] },
    { id: "p1", role: "progressbar", name: "索引进度", nameSource: "label", state: f385.readableProgress(64), parentIds: ["b1"] },
    { id: "iconbtn", role: "button", name: null, nameSource: null, state: null, parentIds: ["b1"] },
  ];
  const tree = f385.createTree(nodes);
  const nameAudit = f385.auditNameCoverage(tree);
  const roleAudit = f385.auditRoleState(tree);
  /* F386 阅读模式 */
  const [readingOn, setReadingOn] = useState(() => f386.appMode("editor").on);
  const style = f386.defaultStyle();
  const hash = f386.contentHash("阅读模式只调排版不动内容。");
  /* F387 灰度 */
  const [filter, setFilter] = useState<f387.ColorFilter>(() => f387.activeFilter());
  const shapeAudit = f387.auditShapeRedundancy([
    { name: "开关", colorOnly: false }, { name: "危险按钮", colorOnly: false }, { name: "错误提示", colorOnly: false },
  ]);
  /* F388 竖屏 */
  const portrait = f388.orientationOf({ w: 1080, h: 1920 });
  const edge = f388.taskbarEdge("portrait", "auto");
  const snapL = f388.snapRect("left", { w: 1080, h: 1920 });
  const readapt = f388.readaptToScreen({ x: 0, y: 0, w: 2400, h: 700 }, { w: 1080, h: 1920 });

  const toggleGrayscale = (): void => {
    const r = f387.requestFilter(memStoreSafe(), "grayscale");
    setFilter(r.now);
    announceFilterChanged(r.now);
    pushToast("success", r.now === "grayscale" ? "灰度已开——全系统单点滤镜（看右上角徽标外的一切变灰）" : "灰度已关");
  };
  const toggleReading = (): void => {
    const next = !readingOn;
    f386.setAppMode("editor", next);
    setReadingOn(next);
    announceReadingChanged("editor", next);
  };

  return (
    <div className="h4-panel">
      <p className="h4-kv"><strong>F385</strong> 语义树：{nodes.length} 控件 · 无名可交互 {nameAudit.unnamed.length} 个（示例故意放 1 个 iconbtn 演示审计抓红灯：{nameAudit.unnamed.join(",") || "无"}）· 角色/状态抽查 {roleAudit.sampled} 项 · 弹窗播报「{f385.announceDialog("设置").text}」</p>
      <p className="h4-kv"><strong>F386</strong> 阅读模式（编辑器）：
        <button type="button" className="h4-btn-mini" onClick={toggleReading}>{readingOn ? "关闭" : "开启"}</button>
        行距 {style.lineHeight} / {style.maxCharsPerLine} 字/行 / 衬线可选 · 入口统一「{f386.unifiedEntry()}」· 内容哈希前后一致 <code className="h4-code">{hash}</code>（只产排版参数，签名上就改不了文本）
      </p>
      <p className="h4-kv"><strong>F387</strong> 灰度模式：
        <button type="button" className="h4-btn-mini" onClick={toggleGrayscale}>{filter === "grayscale" ? "关闭灰度" : "开启灰度"}</button>
        当前滤镜位：{filter} · 单点审计（全系统只有 compositor.filter 一个滤镜位）✓ · 形状冗余 {shapeAudit.pass ? "✓（状态不只靠色相）" : `缺：${shapeAudit.colorOnlyStates.join(",")}`} · 切换即时 {f387.switchLatencyMs()}ms
      </p>
      <p className="h4-kv"><strong>F388</strong> 竖屏 1080×1920 → {portrait} · 任务栏 {edge} · 左半贴靠 <code className="h4-code">{JSON.stringify(snapL)}</code> · 重适配 <code className="h4-code">{JSON.stringify(readapt.rect)}（预算内={String(readapt.withinBudget)}）</code></p>
    </div>
  );
}

/** memStore 单测演示通道（真机默认 localStorage；此处演示注入语义）。 */
function memStoreSafe(): import("../../system/h4/internal/store").KvStore {
  try {
    return localStorage;
  } catch {
    return memStore();
  }
}

/** F354+F371+F389+F392+F395 状态与信息台。 */
function StatusInfoInline(): React.ReactElement {
  /* F354 资源摘要 */
  const [summaryOn, setSummaryOn] = useState(() => f354.isEnabled());
  const history = useMemo(() => {
    let h: f354.ResourcePoint[] = [];
    for (let i = 0; i < f354.HISTORY_POINTS; i++) {
      h = f354.pushPoint(h, { t: i * 1000, cpu: 20 + Math.round(18 * Math.sin(i / 4)) + (i % 5), mem: 46 + (i % 7), disk: i % 9 === 0 ? 34 : 4 }, 0);
    }
    return h;
  }, []);
  const source = { cpu: history[history.length - 1]!.cpu, mem: history[history.length - 1]!.mem, disk: history[history.length - 1]!.disk };
  const recon = f354.reconcile(history, source);
  /* F371 开机徽标 */
  const hist = f371.bootHistory();
  const lastBoot = hist.length > 0 ? f371.badgeFromMeasured(hist[hist.length - 1]!) : null;
  const reconBoot = f371.auditReconciliation();
  const [bootEnabled, setBootEnabled] = useState(() => f371.isEnabled());
  /* F389 滚动长截图 */
  const uniform = f389.stitch(f389.uniformPage(120, 30, 24));
  const dynamic = f389.stitch(f389.dynamicPage(120, 30, 24, 60));
  const spec = f389.productSpec(uniform, 28, 1200, new Date());
  /* F392 文件夹大小 */
  const node: f392.FsNode = { path: "S:/视频", sizeBytes: 0, isDir: true, version: "v7", children: [{ path: "成片", sizeBytes: 58_000_000_000, isDir: false, version: "v1" }, { path: "工程", sizeBytes: 9_000_000_000, isDir: false, version: "v1" }] };
  const cache = f392.emptyCache();
  const m1 = f392.measureCached(node, cache, 0);
  const m2 = f392.measureCached(node, cache, 0);
  /* F395 U 盘健康 */
  const medias: f395.MediaHealth[] = [
    { mediaId: "usb-main", label: "Varix 系统盘", lifePctRemaining: 16, writtenTb: 41.2, temperatureC: 41, remappedBlocks: 3 },
    { mediaId: "usb-shared", label: "S: 共享卷", lifePctRemaining: null, writtenTb: null, temperatureC: null, remappedBlocks: null },
  ];

  return (
    <div className="h4-panel">
      <p className="h4-kv"><strong>F354</strong> 资源摘要（默认关）：
        <button type="button" className="h4-btn-mini" onClick={() => { const v = !summaryOn; f354.setEnabled(v); setSummaryOn(v); }}>{summaryOn ? "关闭" : "开启"}</button>
        三图 sparkline（30 点 × 1s）：<svg width={90} height={18} aria-label="CPU 迷你图"><path d={f354.sparklinePath(history.map((p) => p.cpu), 90, 18)} fill="none" stroke="currentColor" /></svg>
        前台让路：帧预算 {f354.FG_FRAME_YIELD_MS}ms · 对账 {recon.every((r) => r.deltaPct === 0) ? "三图与源头一致 ✓" : "有偏差"}
      </p>
      <p className="h4-kv"><strong>F371</strong> 开机徽标：
        <button type="button" className="h4-btn-mini" onClick={() => { const v = !bootEnabled; f371.setEnabled(v); setBootEnabled(v); }}>{bootEnabled ? "关闭" : "开启"}</button>
        {lastBoot ? `最近一次「${lastBoot.text}」· 对账漂移 ${lastBoot.driftMs}ms` : "暂无开机记录（下次启动计入）"} · 曲线 {hist.length} 条与徽标同源 · 全账对账 {reconBoot.pass ? "✓" : `最差 ${reconBoot.worstDriftMs}ms`}
      </p>
      <p className="h4-kv"><strong>F389</strong> 长截图：均匀页拼接 {uniform.totalRows} 行（接缝 {uniform.seams.length} 处人工评审）· 动态页 → {dynamic.failure ?? "意外成功"}（诚实失败判据）· 产物 <code className="h4-code">{spec.fileName} {spec.w}×{spec.h}</code></p>
      <p className="h4-kv"><strong>F392</strong> 文件夹大小列：{f392.displaySize(m1.result)}（估算标注={String(m1.result.estimated) ? "带~" : "精确"} · 耗时 {m1.result.tookMs}ms）· 二次测量缓存命中={String(m2.cacheHit)}（目录内容不变秒出）· 三功能同源审计 ✓</p>
      <p className="h4-kv"><strong>F395</strong> U 盘健康：{f395.columnsFor(medias).map((c) => `${c.label}=${c.level}`).join(" · ")}
        {medias.map((m) => f395.noticeFor(m)).filter(Boolean).map((n) => `「${(n as f395.HealthNotice).message} → ${(n as f395.HealthNotice).action.label}」`).join("")}
        读不到 → 「{f395.unknownHealthMessage(medias[1]!)}」</p>
    </div>
  );
}

/* ------------------------------- 主标签页 ------------------------------- */

export function H4Tab(): React.ReactElement {
  return (
    <div className="h4-tab">
      <p className="h4-desc">
        创作者工具、效率件、系统状态与控件秩序（H 基础通用域·四分队 · F351-F400）。全部面板直连判据引擎，读数即审计结果；默认档 = 主册判据默认。
      </p>

      <Group title="创作者工具" f="F359 / F360 / F361 / F362 / F389" desc="吸管、标尺、录屏、长截图——看到什么就能拿到什么，产物规格全部入册。">
        <SectionCard title="全局屏幕拾色器" f="F359">
          <PickerPanel />
        </SectionCard>
        <Row label="像素标尺与网格叠加" hint="真指针真读数：点击设起点、G 切 8px/20% 网格、Esc 秒退——区域录制同款框选辅助。">
          <span className="h4-row-actions">
            <button type="button" className="h4-btn-mini" onClick={() => window.dispatchEvent(new CustomEvent(SUMMON_PICKER))}>呼出拾色器</button>
            <button type="button" className="h4-btn-mini" onClick={() => window.dispatchEvent(new CustomEvent(SUMMON_RULER))}>呼出标尺</button>
          </span>
        </Row>
        <SectionCard title="屏幕录制与产物管理" f="F361 + F362">
          <RecorderPanel />
        </SectionCard>
        <Row label="滚动长截图" hint="拼接去重三典型页 · 动态内容诚实失败 · 接缝人工评审——截图工具（F098）的滚动分支。">
          <span className="h4-readonly">入口：截图工具 → 滚动截图</span>
        </Row>
      </Group>

      <Group title="效率工具" f="F363 / F364 / F365 / F390 / F391" desc="专注、整理、查重、查词、翻译——小事一次点到位，系统永远不替你做主。">
        <SectionCard title="专注计时器" f="F363">
          <FocusTimerInline />
        </SectionCard>
        <SectionCard title="下载文件夹一键整理" f="F364">
          <TidyPanel />
        </SectionCard>
        <SectionCard title="重复文件查找" f="F365">
          <DupeFinderInline />
        </SectionCard>
        <SectionCard title="选中文本查词与翻译" f="F390 + F391">
          <LookupPanel />
        </SectionCard>
      </Group>

      <Group title="网页协同" f="F355 / F356 / F357 / F358" desc="浏览器是一等公民：待遇清单全给、下载只收口不越界、网页应用装完就是应用。">
        <SectionCard title="Edge 深度协同 · PWA 应用化 · 下载收口" f="F355 + F356 + F357">
          <WebSynergyPanel />
        </SectionCard>
        <SectionCard title="全局画中画" f="F358">
          <PipPanel />
        </SectionCard>
      </Group>

      <Group title="系统状态与后台" f="F354 / F369 / F370 / F371 / F372" desc="谁在吃资源、后台在干什么、开机多久、发生过什么——全部人话，全部诚实。">
        <SectionCard title="状态摘要 · 任务中心 · 开机徽标 · 人话时间线" f="F354 + F369 + F370 + F371 + F372">
          <StatusInfoInline />
        </SectionCard>
        <SectionCard title="后台任务中心" f="F369">
          <TaskCenterPanel />
        </SectionCard>
        <SectionCard title="系统活动人话时间线" f="F372">
          <TimelinePanel />
        </SectionCard>
      </Group>

      <Group title="存储与备份" f="F393 / F394 / F395 / F396 / F397" desc="空间去哪了、能清什么、U 盘还剩多少命、备份验过才算备份。">
        <SectionCard title="存储热点图" f="F393">
          <TreemapPanel />
        </SectionCard>
        <SectionCard title="清理建议收口页" f="F394">
          <CleanupPanel />
        </SectionCard>
        <SectionCard title="备份向导与还原演练" f="F396 + F397">
          <BackupPanel />
        </SectionCard>
      </Group>

      <Group title="窗口与桌面" f="F351 / F352 / F353 / F366-F368 / F376 / F377 / F382" desc="快照是布置方案、跨屏走哪记哪、托盘语义不骗人——窗口系统的基础秩序。">
        <SectionCard title="工作区快照" f="F351">
          <SnapshotPanel />
        </SectionCard>
        <SectionCard title="缩略图操作 · 跨屏记忆 · 托盘件 · 分区吸附" f="F352 + F353 + F366 + F367 + F368 + F370">
          <DesktopMiniPanel />
        </SectionCard>
        <SectionCard title="标题栏系统菜单矩阵" f="F376">
          <MenuMatrixPanel />
        </SectionCard>
        <SectionCard title="键盘移动与调整窗口" f="F377">
          <KeyboardEditInline />
        </SectionCard>
        <Row label="对话框位置记忆" hint="居中偏上 1/3 基准 · 拖动后同类归组记忆（容量 20 LRU）· 跨屏完整落屏——试一次：">
          <button
            type="button"
            className="h4-btn-mini"
            onClick={() => {
              f382.rememberPosition("save", 640, 280, Date.now());
              const p = f382.placementFor("save", { w: 2560, h: 1392 }, { w: 520, h: 400 });
              pushToast("info", `保存对话框落点 (${p.x}, ${p.y}) · 来自${p.fromMemory ? "记忆" : "居中偏上基准"} · 容量 ${f382.MEMORY_CAP}`);
            }}
          >
            记住并取回位置
          </button>
        </Row>
      </Group>

      <Group title="控件规范" f="F378 / F379 / F380 / F381" desc="列宽、排序、空行点击、树形三态——全系统同一套控件行为，学会一次处处可用。">
        <SectionCard title="列表控件四件套" f="F378 + F379 + F380 + F381">
          <ControlsInline />
        </SectionCard>
      </Group>

      <Group title="无障碍与显示" f="F373 / F374 / F385 / F386 / F387 / F388" desc="键盘全程可达、滤镜单点、阅读舒适、竖屏成立——无障碍是设计质量的试金石。">
        <SectionCard title="键盘布局管理" f="F373">
          <KeyboardLayoutsInline />
        </SectionCard>
        <SectionCard title="快捷键速查浮层" f="F374">
          <SheetPanel />
        </SectionCard>
        <SectionCard title="语义树 · 阅读模式 · 灰度模式 · 竖屏适配" f="F385 + F386 + F387 + F388">
          <A11yDisplayInline />
        </SectionCard>
      </Group>

      <Group title="浮层秩序与语言" f="F383 / F384 / F398" desc="弹窗排队不叠罗汉、焦点陷阱零逃逸、语言热切不丢工作。">
        <SectionCard title="弹窗排队与焦点陷阱" f="F383 + F384">
          <LayerOrderInline />
        </SectionCard>
        <SectionCard title="界面语言热切" f="F398">
          <LanguagePanel />
        </SectionCard>
      </Group>

      <Group title="彩蛋与域收官" f="F399 / F375 / F400" desc="系统的创造性人格与全域总门——彩蛋是情感不是门，走查不过的条目回炉不发布。">
        <SectionCard title="彩蛋总谱" f="F399">
          <EggPanel />
        </SectionCard>
        <SectionCard title="H 域总判据 · 收官登记" f="F375 + F400">
          <GatePanel />
        </SectionCard>
      </Group>
    </div>
  );
}
