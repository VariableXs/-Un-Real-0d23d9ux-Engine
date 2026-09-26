/**
 * PersonaStudio · 个性化全域工作台（E 域 F151-F170 二十项统一界面）。
 *
 * 双形态：
 * - overlay：bus 事件 "ai04:open-feature" detail.feature === "persona-studio"
 *   时 createPortal 挂载（theme-studio 同族惯例）；
 * - embedded：设置中心「个性化」页内嵌（PersonaTab 消费）——同一内容双入口
 *   可达（十二查·双入口通则）。
 *
 * 布局：左导航树（E1-E8 分组）+ 右内容区；信息密度对标专业工具，
 * 层级靠分区卡递进；全键盘可达。
 */
import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { pushOverlay, popOverlay } from "../../state/uiStore";
import { Card, PageHeader, Row, PButton, Notice, useT } from "./ui";
import { baseCharset, generalCharset, scanFont, verdictFor, MISSING_LIST_PREVIEW } from "./fontguard";
import { buildFallbackChain, BatchFontScanner } from "./font-engine";
import { TokensPage, PreviewPage, AutoDarkPage, ExceptionsPage } from "./pages-theme";
import { DailyWallPage, IconPackPage, PointerPage } from "./pages-assets";
import { SoundPage, StartMenuPage, MotionPage } from "./pages-feel";
import { ArchivePage, WidgetsPage, LockPage, BootPage, ImePage } from "./pages-life";
import { CtxMenuPage, TaskbarPage, ShortcutsPage, VerdictPage } from "./pages-misc";

type PageId =
  | "tokens" | "preview" | "autodark" | "exceptions"
  | "wallpaper" | "icons" | "pointer"
  | "sound" | "startmenu" | "font" | "motion"
  | "archive" | "widgets" | "lock" | "boot" | "ime"
  | "ctxmenu" | "taskbar" | "shortcuts" | "verdict";

interface NavItem { id: PageId; zh: string; en: string }
interface NavGroup { title: string; items: NavItem[] }

const NAV: NavGroup[] = [
  {
    title: "E1-E2 主题与预览",
    items: [
      { id: "tokens", zh: "令牌表", en: "Token table" },
      { id: "preview", zh: "实时预览编辑器", en: "Live preview" },
      { id: "autodark", zh: "深浅自动切换", en: "Auto dark" },
      { id: "exceptions", zh: "每应用例外", en: "Exceptions" },
    ],
  },
  {
    title: "E2-E4 壁纸·图标·指针",
    items: [
      { id: "wallpaper", zh: "壁纸每日一换", en: "Daily wallpaper" },
      { id: "icons", zh: "图标包热更换", en: "Icon packs" },
      { id: "pointer", zh: "指针编辑器", en: "Pointer editor" },
    ],
  },
  {
    title: "E5-E8 手感",
    items: [
      { id: "sound", zh: "声音混合器", en: "Sound mixer" },
      { id: "startmenu", zh: "开始菜单预设", en: "Start presets" },
      { id: "font", zh: "字体安全档", en: "Font guard" },
      { id: "motion", zh: "动效强度预演", en: "Motion preview" },
    ],
  },
  {
    title: "档案与生活面",
    items: [
      { id: "archive", zh: "我的档案", en: "My profile" },
      { id: "widgets", zh: "桌面小组件", en: "Widgets" },
      { id: "lock", zh: "锁屏定制", en: "Lock screen" },
      { id: "boot", zh: "开机动画", en: "Boot animation" },
      { id: "ime", zh: "输入法皮肤", en: "IME skin" },
    ],
  },
  {
    title: "系统面",
    items: [
      { id: "ctxmenu", zh: "右键菜单", en: "Context menu" },
      { id: "taskbar", zh: "任务栏", en: "Taskbar" },
      { id: "shortcuts", zh: "快捷键", en: "Shortcuts" },
      { id: "verdict", zh: "域总检", en: "Domain verdict" },
    ],
  },
];

const PAGE_MAP: Record<PageId, () => ReactNode> = {
  tokens: TokensPage,
  preview: PreviewPage,
  autodark: AutoDarkPage,
  exceptions: ExceptionsPage,
  wallpaper: DailyWallPage,
  icons: IconPackPage,
  pointer: PointerPage,
  sound: SoundPage,
  startmenu: StartMenuPage,
  font: FontPage,
  motion: MotionPage,
  archive: ArchivePage,
  widgets: WidgetsPage,
  lock: LockPage,
  boot: BootPage,
  ime: ImePage,
  ctxmenu: CtxMenuPage,
  taskbar: TaskbarPage,
  shortcuts: ShortcutsPage,
  verdict: VerdictPage,
};

/** F159 字体安全档页（纯逻辑在 fontguard.ts——本页给预检三档可视与强行应用护栏；回退链与批量扫描走 font-engine）。 */
function FontPage(): ReactNode {
  const t = useT();
  const [input, setInput] = useState("");
  const [monospace, setMonospace] = useState(false);
  const [userFontName, setUserFontName] = useState("我的艺术字体");
  const result = useMemo(() => {
    const covered = new Set<number>();
    for (const ch of input) covered.add(ch.codePointAt(0) ?? 0);
    if (covered.size === 0) return null;
    const r = scanFont({ fontId: "manual-probe", covered, generalChars: generalCharset(), interfaceChars: [...baseCharset()] });
    return { ...r, monospace };
  }, [input, monospace]);
  const verdict = result ? verdictFor(result.interfaceMissingRate) : null;
  // 回退链构建（font-engine）：用户字体 → 中文栈 → 西文栈 → 兜底（永不落空）。
  const chain = useMemo(
    () => buildFallbackChain(userFontName, { cjk: "思源黑体 CJK", latin: "Inter", fallback: "Varix Sans" }),
    [userFontName],
  );
  // 批量扫描协议（font-engine）：分批后台扫 + 部分结论先行（超大字体不冻结界面）。
  const [batchInfo, setBatchInfo] = useState<string | null>(null);
  function runBatchScan(): void {
    const covered = new Set<number>();
    for (const ch of input) covered.add(ch.codePointAt(0) ?? 0);
    if (covered.size === 0) {
      setBatchInfo("先提供字符覆盖样本——批量扫描协议需要输入。");
      return;
    }
    const scanner = new BatchFontScanner({ fontId: "batch-demo", covered, generalChars: generalCharset(), interfaceChars: [...baseCharset()] });
    const snap0 = scanner.snapshot;
    let guard = 0;
    while (scanner.step() && guard++ < 64) { /* 全批跑完（每批 500 字——实机由空闲调度器分帧调用） */ }
    const snap = scanner.snapshot;
    setBatchInfo(`批 ${snap.completedBatches}/${snap.totalBatches} 完成 · 部分结论在第 1 批后即给出（缺字率 ${((snap.partialMissingRate ?? 0) * 100).toFixed(1)}%）· 单批 ${snap.lastBatchMs}ms（初始批 ${snap0.totalBatches} 批计划）`);
  }

  return (
    <div>
      <PageHeader
        title="字体安全档（F159）"
        hint="换字体前自动兼容性预检：双集缺字率扫描（通用 3500 常用字+界面词条）；>5% 黄色警告、>15% 红色劝阻（可强行但显式确认）——个性化不许破坏可用性。"
      />
      <Card title="字符覆盖探针（粘贴字体覆盖字符样本或导入 cmap 后自动扫描）">
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="在此粘贴该字体实际覆盖的字符样本…（生产路径：parseCmapFormat4 直读字体文件）"
          aria-label="字符覆盖样本"
          style={{ ...inputStyle, width: "100%", minHeight: 90, fontFamily: "monospace" }}
        />
        <Row label="等宽检测（终端场景）" sub="等宽数字与字母 advance 一致性">
          <input type="checkbox" checked={monospace} onChange={(e) => setMonospace(e.target.checked)} aria-label="等宽" />
          <span style={{ fontSize: 12, opacity: 0.8 }}>{monospace ? t("monospace") + " ✓" : "非等宽"}</span>
        </Row>
      </Card>
      {result ? (
        <Card title="预检结果">
          <Notice tone={verdict === "danger" ? "danger" : verdict === "warn" ? "warn" : "ok"}>
            {verdict === "danger" ? t("verdictDanger") : verdict === "warn" ? t("verdictWarn") : t("verdictOk")}
            ：界面集缺字率 {(result.interfaceMissingRate * 100).toFixed(1)}%（通用集 {(result.generalMissingRate * 100).toFixed(1)}%）
          </Notice>
          {result.missingChars.length > 0 ? (
            <Row label="缺字清单（前 20 展示口径）" sub={result.missingChars.slice(0, MISSING_LIST_PREVIEW).join(" ")}>
              <span />
            </Row>
          ) : null}
          {verdict !== "ok" ? (
            <Row label={t("forceApply")} sub="强行应用记录入账（显式确认留痕）；回退路径 = personaStore.undoSection('font')">
              <PButton kind="danger" onClick={() => document.dispatchEvent(new CustomEvent("persona:font-force-apply", { detail: { fontId: result.fontId } }))}>{t("forceApply")}</PButton>
            </Row>
          ) : null}
        </Card>
      ) : null}
      <Card title="回退链（缺字不空窗——永不落空的字体栈）">
        <Row label="用户字体名">
          <input value={userFontName} onChange={(e) => setUserFontName(e.target.value)} style={{ ...inputStyle, width: 200 }} aria-label="用户字体名" />
        </Row>
        {chain.stack.map((f, i) => (
          <Row key={`${f}-${i}`} label={`第 ${i + 1} 级 · ${f}`} sub={chain.notes[i] ?? ""}>
            <span />
          </Row>
        ))}
      </Card>
      <Card title="批量扫描协议（超大字体分批后台扫 · 部分结论先行）">
        <Row label="执行" sub="每批 500 字、批间让出 500ms（实机由空闲调度器分帧调用——界面零冻结）">
          <PButton onClick={runBatchScan}>跑批量扫描</PButton>
        </Row>
        {batchInfo ? <Notice tone="info">{batchInfo}</Notice> : null}
      </Card>
    </div>
  );
}

const inputStyle: CSSProperties = {
  background: "var(--p-bg-raised, rgba(34,34,46,0.9))", color: "inherit",
  border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", borderRadius: 6, padding: "6px 10px", fontSize: 12,
};

export function PersonaStudio(props: { embedded?: boolean; onClose?: () => void }): ReactNode {
  const t = useT();
  const [page, setPage] = useState<PageId>("tokens");
  const Page = PAGE_MAP[page] ?? TokensPage;

  const body = (
    <div style={props.embedded ? styles.embedded : styles.overlay}>
      <aside style={styles.nav}>
        {!props.embedded ? (
          <div style={styles.brand}>
            <b style={styles.brandTitle}>{t("studioTitle")}</b>
            <p style={styles.brandSub}>{t("studioSubtitle")}</p>
          </div>
        ) : (
          <div style={styles.brand}>
            <b style={styles.brandTitle}>{t("studioTitle")}</b>
          </div>
        )}
        {NAV.map((g) => (
          <div key={g.title} style={styles.navGroup}>
            <div style={styles.navGroupTitle}>{g.title}</div>
            {g.items.map((item) => (
              <button
                key={item.id}
                type="button"
                style={{ ...styles.navItem, ...(page === item.id ? styles.navItemOn : {}) }}
                onClick={() => setPage(item.id)}
              >
                {item.zh}
              </button>
            ))}
          </div>
        ))}
      </aside>
      <div style={styles.content}>
        <Page />
      </div>
    </div>
  );

  if (props.embedded) return body;

  return createPortal(
    <div style={styles.backdrop} onClick={(e) => { if (e.target === e.currentTarget) props.onClose?.(); }}>
      <div style={styles.panel} role="dialog" aria-modal="true" aria-label={t("studioTitle")} onKeyDown={(e) => { if (e.key === "Escape") props.onClose?.(); }}>
        <div style={styles.topbar}>
          <b>{t("studioTitle")}</b>
          <button type="button" style={styles.closeBtn} aria-label={t("close")} onClick={props.onClose}><X size={16} aria-hidden /></button>
        </div>
        {body}
      </div>
    </div>,
    document.body,
  );
}

const OVERLAY_ID = "persona-studio";

/** overlay 注册（bus.ts 经 "ai04:open-feature" 懒加载本默认导出）。 */
export default function PersonaStudioOverlay(): ReactNode {
  const [open, setOpen] = useState(true);
  useEffect(() => {
    pushOverlay(OVERLAY_ID);
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      popOverlay(OVERLAY_ID);
    };
  }, []);
  if (!open) return null;
  return <PersonaStudio onClose={() => setOpen(false)} />;
}

const styles: Record<string, CSSProperties> = {
  backdrop: { position: "fixed", inset: 0, background: "rgba(0,0,0,0.5)", backdropFilter: "blur(6px)", zIndex: 1000, display: "grid", placeItems: "center" },
  panel: {
    width: "min(1160px, 94vw)", height: "min(780px, 92vh)", display: "flex", flexDirection: "column",
    background: "var(--p-bg-canvas, var(--bg-canvas, #14141c))", color: "var(--p-fg-primary, var(--text-primary, #e8e8f0))",
    borderRadius: "var(--p-r-window, 16px)", overflow: "hidden",
    border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))",
    boxShadow: "0 24px 64px var(--p-shadow, rgba(0,0,0,0.5))",
  },
  topbar: { display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 16px", borderBottom: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))" },
  closeBtn: { background: "transparent", border: "none", color: "inherit", cursor: "pointer", padding: 6, borderRadius: 6, display: "grid", placeItems: "center" },
  embedded: { display: "flex", gap: 0, minHeight: 520, color: "var(--p-fg-primary, var(--text-primary, #e8e8f0))" },
  overlay: { flex: 1, display: "flex", minHeight: 0 },
  nav: { width: 216, flexShrink: 0, borderRight: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", overflowY: "auto", padding: 10, background: "var(--p-bg-surface, rgba(28,28,38,0.6))" },
  brand: { padding: "4px 6px 12px", borderBottom: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", marginBottom: 8 },
  brandTitle: { fontSize: 14 },
  brandSub: { margin: "6px 0 0", fontSize: 10.5, lineHeight: 1.5, color: "var(--p-fg-secondary, var(--text-secondary, #a0a0b4))" },
  navGroup: { marginBottom: 10 },
  navGroupTitle: { fontSize: 10, textTransform: "uppercase", letterSpacing: 1, opacity: 0.55, padding: "4px 6px" },
  navItem: { display: "block", width: "100%", textAlign: "left", padding: "6px 10px", fontSize: 12.5, background: "transparent", color: "inherit", border: "none", borderRadius: 7, cursor: "pointer", transition: "background 120ms ease-out" },
  navItemOn: { background: "var(--p-accent, var(--accent, #6e7fd4))", color: "var(--p-on-accent, #fff)" },
  content: { flex: 1, overflowY: "auto", padding: 18, minWidth: 0 },
};
