/**
 * E 域页组⑤：系统面（F167 右键菜单 / F168 任务栏 / F169 快捷键 / F170 域总检）。
 */
import { useEffect, useMemo, useState } from "react";
import {
  SYSTEM_ITEMS, LOCKED_ITEMS, loadCtxMenuConfig, saveCtxMenuConfig, hideItem,
  restoreItem, hiddenRatioWarning, recordUsage, usageCount, autoSort, POPUP_BUDGET_MS,
} from "./ctxmenu";
import {
  loadTaskbarPrefs, saveTaskbarPrefs, ICON_SIZE_PX, HEIGHT_PX, effectiveAutoHide,
  trayShouldFold, shouldReveal,
} from "./taskbarprefs";
import {
  DEFAULT_SHORTCUTS, GROUP_NAMES, loadOverrides, saveOverrides, rebind, undoRebind,
  resetAllToDefault, normalizeCombo, effectiveCombos, searchShortcuts, comboSignature,
  type ShortcutGroup,
} from "./shortcuts";
import { runDomainVerdict, verdictChecklistJson, type DomainVerdict } from "./verdict";
import { assembleContextMenu } from "./ime-menu-engine";
import { taskbarGeometry, autoHideNext, REARRANGE_BUDGET_MS, type AutoHideState } from "./layout-engine";
import { parseComboFromEvent, classifyConflict, exportKeymap, importKeymap, buildEvidenceDoc, evidenceDocComplete, effectiveSnapshot, type ScopedShortcut } from "./shortcut-engine";
import { Card, PageHeader, Row, Toggle, Segmented, PButton, Notice, useT, usePersonaSection } from "./ui";

// ---------- F167 右键菜单 ----------

export function CtxMenuPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("ctxmenu");
  const cfg = loadCtxMenuConfig();
  const warn = hiddenRatioWarning(cfg);
  const visible = Object.values(cfg.items).filter((i) => !i.hidden).map((i) => i.id);
  const sorted = autoSort(cfg, visible);

  return (
    <div>
      <PageHeader title={t("ctxTitle")} hint="定制是修饰不是重构：菜单结构基线对齐 Windows（乙-4 表）；隐藏项在「显示更多选项」二级完整保留；弹出延迟红线 100ms。" />
      {warn ? <Notice tone="warn">{warn}</Notice> : null}
      <Card>
        <Row label={t("autoSort")} sub="仅对「打开方式」组与应用注册组生效（系统组位置不动）">
          <Toggle checked={cfg.autoSort} onChange={(v) => saveCtxMenuConfig({ ...cfg, autoSort: v })} ariaLabel={t("autoSort")} />
        </Row>
        {Object.values(cfg.items).map((item) => {
          const meta = SYSTEM_ITEMS.find((s) => s.id === item.id);
          const locked = LOCKED_ITEMS.has(item.id);
          const count = usageCount(cfg, item.id);
          return (
            <Row
              key={item.id}
              label={meta?.zh ?? item.id}
              sub={locked ? "🔒 " + t("lockedHint") : `${item.appRegistered ? "应用注册组" : "系统组"} · 90 天使用 ${count} 次${item.hidden ? " · 已隐藏（二级保留）" : ""}`}
            >
              {item.hidden ? (
                <PButton onClick={() => saveCtxMenuConfig(restoreItem(cfg, item.id).config)}>{t("restoreDefault")}</PButton>
              ) : (
                <PButton kind={locked ? "ghost" : "danger"} disabled={locked} onClick={() => saveCtxMenuConfig(hideItem(cfg, item.id).config)}>隐藏</PButton>
              )}
            </Row>
          );
        })}
        <Row label="模拟选择打点" sub={`使用计数打点在菜单项选择时刻（零额外开销）——弹出预算 ${POPUP_BUDGET_MS}ms`}>
          <PButton onClick={() => saveCtxMenuConfig(recordUsage(cfg, "open", Date.now()))}>点击「打开」+1</PButton>
        </Row>
        <Row label="排序预览" sub={sorted.join(" → ")}>
          <span />
        </Row>
      </Card>
      <AssembledMenuPreviewCard />
    </div>
  );
}

/**
 * 菜单总装预览（ime-menu-engine 接线）：基线+应用注册+用户定制 → 最终渲染模型。
 * 隐藏项收进「显示更多选项」二级（功能不丢只收纳）；系统组位置不动。
 */
function AssembledMenuPreviewCard(): React.ReactNode {
  const cfg = loadCtxMenuConfig();
  const [context, setContext] = useState<"file" | "desktop">("file");
  const appItems = [
    { id: "app.archiver", label: "压缩", shortcutHint: "" },
    { id: "app.editor", label: "编辑", shortcutHint: "" },
    { id: "app.scan", label: "扫描", shortcutHint: "" },
  ];
  const assembled = assembleContextMenu(cfg, appItems, { target: context });
  const tier1 = assembled.items.filter((i) => i.tier === 1);
  const tier2 = assembled.items.filter((i) => i.tier === 2);

  return (
    <Card title="菜单总装预览（真实渲染模型 · 乙-4 基线对齐）">
      <Row label="右键目标" sub="文件目标含剪切/复制/重命名；桌面背景自动过滤文件类项">
        <Segmented
          value={context} ariaLabel="右键目标" onChange={setContext}
          options={[{ value: "file", label: "文件" }, { value: "desktop", label: "桌面背景" }]}
        />
      </Row>
      <div style={{ border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", borderRadius: 8, padding: 8, maxWidth: 280, background: "var(--p-bg-surface, rgba(28,28,38,0.6))" }}>
        {tier1.map((i) => (
          <div key={i.id} style={{ display: "flex", justifyContent: "space-between", gap: 12, padding: "3px 6px", fontSize: 12, opacity: i.locked ? 0.75 : 1 }}>
            <span>{i.label}{i.locked ? " 🔒" : ""}</span>
            <span style={{ opacity: 0.55, fontSize: 10 }}>{i.group === "app" ? "应用" : i.shortcutHint}</span>
          </div>
        ))}
        {tier2.length > 0 ? (
          <div style={{ borderTop: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", marginTop: 4, paddingTop: 4 }}>
            <div style={{ padding: "3px 6px", fontSize: 12, fontWeight: 600 }}>显示更多选项（{assembled.moreCount}）</div>
            {tier2.map((i) => (
              <div key={i.id} style={{ padding: "2px 6px 2px 16px", fontSize: 11, opacity: 0.7 }}>{i.label}{i.locked ? " 🔒" : ""}</div>
            ))}
          </div>
        ) : null}
      </div>
      <Row label="弹出预算对账" sub={`总装耗时 ${assembled.assembledInMs ?? 0}ms ≤ ${POPUP_BUDGET_MS}ms 红线（定制不增负——性能回归项）`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F168 任务栏 ----------

export function TaskbarPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("taskbar");
  const prefs = loadTaskbarPrefs();
  const fullscreen = false;
  const hide = effectiveAutoHide(prefs, fullscreen);

  return (
    <div>
      <PageHeader title={t("taskbarTitle")} hint="三选项独立生效互不干扰：大图标档任务栏 48→56px；自动隐藏光标贴底 200ms 唤出（热区 6px 防误唤）；全屏态强制显示。" />
      <Card title="迷你任务栏预览（即改即见）">
        <div style={{ display: "flex", justifyContent: prefs.align === "center" ? "center" : "flex-start", alignItems: "center", gap: 10, height: HEIGHT_PX[prefs.iconSize] / 2, background: "var(--p-bg-surface, rgba(28,28,38,0.8))", borderRadius: 10, padding: "0 12px", border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))" }}>
          {[0, 1, 2].map((i) => (
            <span key={i} style={{ width: ICON_SIZE_PX[prefs.iconSize] / 2, height: ICON_SIZE_PX[prefs.iconSize] / 2, borderRadius: 6, background: i === 0 ? "var(--p-accent, #6e7fd4)" : "var(--p-border-regular, rgba(140,140,160,0.4))", transition: "width 200ms, height 200ms" }} />
          ))}
        </div>
      </Card>
      <Card>
        <Row label={t("iconSize")} sub={`标准 ${ICON_SIZE_PX.standard}px / 大 ${ICON_SIZE_PX.large}px · 高 ${HEIGHT_PX[prefs.iconSize]}px`}>
          <Segmented
            value={prefs.iconSize} ariaLabel={t("iconSize")} onChange={(v) => saveTaskbarPrefs({ ...prefs, iconSize: v })}
            options={[{ value: "standard", label: "标准 24" }, { value: "large", label: "大 32" }]}
          />
        </Row>
        <Row label={t("align")}>
          <Segmented
            value={prefs.align} ariaLabel={t("align")} onChange={(v) => saveTaskbarPrefs({ ...prefs, align: v })}
            options={[{ value: "center", label: t("alignCenter") }, { value: "left", label: t("alignLeft") }]}
          />
        </Row>
        <Row label={t("autoHide")} sub="全屏应用冲突 → 强制显示（可交互性优先）；隐藏态 Win 键仍弹开始菜单（可连性兜底）">
          <Toggle checked={prefs.autoHide} onChange={(v) => saveTaskbarPrefs({ ...prefs, autoHide: v })} ariaLabel={t("autoHide")} />
        </Row>
        <Row label="托盘折叠联动" sub={`当前档折叠判定（12 图标）: ${trayShouldFold(prefs, 12) ? "折叠为「^」展开器" : "平铺"} · 唤出判定（贴底 3px 停留 250ms）: ${shouldReveal(3, 250) ? "唤出" : "否"}`}>
          <span />
        </Row>
        <Row label="隐藏态预览" sub={hide ? "任务栏已隐藏——贴底唤出" : "任务栏常显"}>
          <span />
        </Row>
      </Card>
      <TaskbarEngineCard />
    </div>
  );
}

/**
 * 任务栏引擎面板（layout-engine 接线）：布局几何实算（图标位/折叠线/托盘起点）
 * + 自动隐藏五态状态机（光标位置 × 驻留 × 焦点 × 全屏——迟滞去抖）。
 */
function TaskbarEngineCard(): React.ReactNode {
  const prefs = loadTaskbarPrefs();
  const screenW = typeof window !== "undefined" ? window.innerWidth : 1920;
  const items = [
    { id: "start", pinned: true },
    { id: "explorer", pinned: true },
    { id: "editor", pinned: false },
    { id: "term", pinned: false },
    { id: "media", pinned: false },
  ];
  const geo = taskbarGeometry(prefs, items, screenW, 12);
  const [state, setState] = useState<AutoHideState>("shown");

  function step(s: { y: number; dwell: number; focus: boolean; fullscreen: boolean }): void {
    const next = autoHideNext({
      state,
      cursorYFromBottom: s.y,
      dwellMs: s.dwell,
      focusPinned: s.focus,
      fullscreenActive: s.fullscreen,
      autoHidePref: prefs.autoHide,
      sinceTransitionMs: 500,
    });
    setState(next);
  }

  return (
    <Card title="布局几何与状态机（layout-engine 实算 · 重排预算 200ms）">
      <Row
        label={`几何 · 高 ${geo.heightPx}px · 图标 ${geo.iconPx}px · 开始钮 ${geo.startButtonPx}px`}
        sub={`图标区起点 x=${geo.iconsOriginX} · 托盘起点 x=${geo.trayOriginX}（从右缘向左排）· 折叠 ${geo.foldedIds.length} 项${geo.foldedIds.length > 0 ? `: ${geo.foldedIds.join(" ")}` : "（固定项永不折叠）"}`}
      >
        <span />
      </Row>
      <Row label="重排预算" sub={`三选项任一变更 → 布局重排 ${REARRANGE_BUDGET_MS}ms 内完成（F124 大面板档）`}>
        <span />
      </Row>
      <Row label="自动隐藏状态机" sub={`当前态: ${state}（shown 显示 / hiding 隐藏中 / hidden 已隐藏 / revealing 唤出中 / pinned-by-focus 焦点钉住）`}>
        <span style={{ display: "inline-flex", gap: 6, flexWrap: "wrap" }}>
          <PButton onClick={() => step({ y: 200, dwell: 0, focus: false, fullscreen: false })}>光标离开</PButton>
          <PButton onClick={() => step({ y: 3, dwell: 250, focus: false, fullscreen: false })}>贴底 250ms</PButton>
          <PButton onClick={() => step({ y: 3, dwell: 250, focus: true, fullscreen: false })}>托盘弹层开着</PButton>
          <PButton onClick={() => step({ y: 3, dwell: 250, focus: false, fullscreen: true })}>全屏应用</PButton>
        </span>
      </Row>
    </Card>
  );
}

// ---------- F169 快捷键查看器 ----------

export function ShortcutsPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("shortcuts");
  const overrides = loadOverrides();
  const [q, setQ] = useState("");
  const [recording, setRecording] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const combos = effectiveCombos(overrides);
  const groups = Object.keys(GROUP_NAMES) as ShortcutGroup[];
  const filtered = q ? searchShortcuts(q, "zh") : DEFAULT_SHORTCUTS;
  // 冲突审计（shortcut-engine 接线）：对全表逐条分级判定——系统系统冲突即时显出。
  const conflicts = useMemo(() => {
    const scoped: ScopedShortcut[] = DEFAULT_SHORTCUTS.map((s) => ({ id: s.id, scope: "system" as const }));
    const found: { id: string; message: string }[] = [];
    for (const e of DEFAULT_SHORTCUTS) {
      const c = combos.get(e.id) ?? e.default;
      const v = classifyConflict(scoped, overrides, e.id, c);
      if (v.kind !== "none" && v.message) found.push({ id: e.id, message: `${e.zh}: ${v.message}` });
    }
    return found;
  }, [overrides, combos]);

  // 重录捕获层独立于输入系统：录时不触发功能（keydown preventDefault + 只读键面）。
  // 事件语法解析走 shortcut-engine（parseComboFromEvent）——Esc 取消、单键拒绝、
  // 规范序归一全在解析器内，页面不自带第二套规则。
  useEffect(() => {
    if (!recording) return;
    function onKey(e: KeyboardEvent): void {
      e.preventDefault();
      e.stopPropagation();
      if (!recording) return;
      const parsed = parseComboFromEvent(e);
      if (!parsed.ok) {
        if (parsed.reason === "Esc 取消" || e.key === "Escape") {
          setRecording(null);
          setMsg("已取消重录（原键位不变）");
          return;
        }
        setMsg(`拒绝：${parsed.reason}`);
        return;
      }
      const entry = DEFAULT_SHORTCUTS.find((x) => x.id === recording);
      if (!entry) return;
      const r = rebind(overrides, recording, parsed.combo!);
      setMsg(r.reason);
      if (r.ok) saveOverrides(r.overrides);
      setRecording(null);
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, overrides]);

  return (
    <div>
      <PageHeader
        title={t("shortcutsTitle")}
        hint="快捷键是契约，契约要可查可改：规范序 Win>Ctrl>Alt>Shift>键；冲突即录即查（O(1)）；录时不触发功能（捕获层独立于输入系统）。"
        right={
          <span style={{ display: "flex", gap: 8 }}>
            <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("searchPlaceholder")} style={inputStyle} aria-label={t("searchPlaceholder")} />
            <PButton kind="danger" onClick={() => { saveOverrides(resetAllToDefault()); setMsg(t("resetAll")); }}>{t("resetAll")}</PButton>
          </span>
        }
      />
      {msg ? <Notice tone="info">{msg}</Notice> : null}
      {recording ? <Notice tone="warn">{t("recording")}</Notice> : null}
      {conflicts.length > 0 ? (
        <Notice tone="warn">冲突审计：{conflicts.length} 条冲突——{conflicts.slice(0, 3).map((c) => c.message).join("；")}</Notice>
      ) : (
        <Notice tone="ok">冲突审计：全表零冲突（即录即查 O(1) 查表的静态对账）。</Notice>
      )}
      {groups.map((g) => (
        <Card key={g} title={GROUP_NAMES[g].zh}>
          {filtered.filter((e) => e.group === g).map((e) => {
            const combo = combos.get(e.id) ?? e.default;
            const isCustom = overrides.combos[e.id] !== undefined && comboSignature(combo) !== comboSignature(e.default);
            return (
              <Row key={e.id} label={`${e.zh}${isCustom ? "（已改）" : ""}`} sub={e.en}>
                <code style={kbdStyle}>{normalizeCombo(combo)}</code>
                {e.rebindingAllowed ? (
                  <PButton onClick={() => setRecording(e.id)}>{t("rebind")}</PButton>
                ) : (
                  <span style={{ fontSize: 11, opacity: 0.6 }}>系统保留</span>
                )}
              </Row>
            );
          })}
        </Card>
      ))}
      <Row label="撤销最近一次重录" sub={`撤销栈 ${overrides.undo.length} 条`}>
        <PButton onClick={() => { saveOverrides(undoRebind(overrides)); setMsg("已改回"); }}>撤销</PButton>
      </Row>
      <KeymapExchangeRow overrides={overrides} onMsg={setMsg} />
    </div>
  );
}

/** 键位导出/导入（shortcut-engine 接线）：vxkeymap v1 格式——迁移与备份的契约格式。 */
function KeymapExchangeRow(props: { overrides: ReturnType<typeof loadOverrides>; onMsg: (s: string) => void }): React.ReactNode {
  function doExport(): void {
    const km = exportKeymap(props.overrides);
    const blob = new Blob([JSON.stringify(km, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "varix-keymap.vxkeymap.json";
    a.click();
    URL.revokeObjectURL(a.href);
    props.onMsg(`已导出 ${Object.keys(km.overrides).length} 条自定义键位（vxkeymap v1）`);
  }

  function doImport(file: File): void {
    file.text().then((text) => {
      try {
        const r = importKeymap(JSON.parse(text), props.overrides);
        if (r.ok) saveOverrides(r.overrides);
        props.onMsg(r.reason);
      } catch {
        props.onMsg("不是合法 JSON");
      }
    });
  }

  return (
    <Row label="键位迁移" sub="导出 vxkeymap v1 / 导入同格式（换机迁移与备份的契约格式；非法格式拒绝并说明）">
      <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
        <PButton onClick={doExport}>导出键位</PButton>
        <label style={{ fontSize: 11, cursor: "pointer", padding: "4px 10px", borderRadius: 6, border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))" }}>
          导入键位
          <input
            type="file" accept=".json,application/json" style={{ display: "none" }}
            aria-label="导入键位文件"
            onChange={(e) => { const f = e.target.files?.[0]; if (f) doImport(f); e.target.value = ""; }}
          />
        </label>
      </span>
    </Row>
  );
}

// ---------- F170 域总检 ----------

export function VerdictPage(): React.ReactNode {
  const t = useT();
  const [result, setResult] = useState<DomainVerdict | null>(null);
  const [running, setRunning] = useState(false);

  function run(): void {
    setRunning(true);
    // 三步引擎是同步纯逻辑——setTimeout 让「执法中…」态先渲染（反馈 <100ms 纪律）。
    window.setTimeout(() => {
      setResult(runDomainVerdict());
      setRunning(false);
    }, 30);
  }

  function downloadChecklist(): void {
    const blob = new Blob([verdictChecklistJson()], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "vx-walkcheck-e-checklist.json";
    a.click();
    URL.revokeObjectURL(a.href);
  }

  /** 证据归档（shortcut-engine 接线）：三步验收记录落成 docs/acceptance 惯例 JSON。 */
  function downloadEvidence(): void {
    if (!result) return;
    const notes = [
      `域总检 UI 内执行 · ${new Date().toLocaleString()}`,
      `通过 ${result.passed}/${result.total} · 耗时 ${result.elapsedMinutes.toFixed(3)} 分钟`,
      "逐项配置哈希前后果见 verdict.items[].evidence（探针 mutate 前快照逐位等值）",
    ];
    const doc = buildEvidenceDoc(result, notes);
    const blob = new Blob([JSON.stringify(doc, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "vx-walkcheck-e-evidence.json";
    a.click();
    URL.revokeObjectURL(a.href);
  }

  return (
    <div>
      <PageHeader
        title={t("verdictTitle")}
        hint="E 域宪法执法：随时可改 / 随时可退 / 预设+微调——对 F151-F169 逐项三步打勾；任何一项「改完要重启」即整项不达标。"
        right={<PButton kind="primary" onClick={run} disabled={running}>{running ? t("verdictRunning") : t("verdictRun")}</PButton>}
      />
      {result ? (
        <>
          <Notice tone={result.allGreen ? "ok" : "danger"}>
            {result.allGreen ? t("verdictPass") : t("verdictFail")} — {result.passed}/{result.total} · 耗时 {result.elapsedMinutes.toFixed(3)} 分钟（{result.withinBudget ? t("withinBudget") : "超预算"}） · {t("evidenceChain")} {result.evidenceComplete ? "100%" : "不足"}
          </Notice>
          <Card title="逐项三步">
            {result.items.map((item) => (
              <Row key={item.id} label={`${item.id} ${item.name}`} sub={item.probe}>
                <span style={{ display: "inline-flex", gap: 4 }}>
                  {item.steps.map((s) => (
                    <span key={s.step} title={`${s.step}: ${s.detail} · 证据 ${s.evidence}`} style={{ ...stepChip, background: s.ok ? "var(--p-success-soft, rgba(95,191,138,0.2))" : "var(--p-danger-soft, rgba(212,104,95,0.2))" }}>
                      {s.step === "mutate" ? t("stepMutate") : s.step === "take-effect" ? t("stepEffect") : t("stepRollback")}{s.ok ? "✓" : "✗"}
                    </span>
                  ))}
                </span>
              </Row>
            ))}
          </Card>
        </>
      ) : (
        <Notice tone="info">点右上「执行」开始执法——脚本工具版 tools/vx-walkcheck-e.py 消费同一 checklist（门禁前移纪律：新个性化功能合入前先扩三步脚本再合）。</Notice>
      )}
      <Row label="checklist 导出" sub="vx-walkcheck-e v1（走查材料族惯例）">
        <PButton onClick={downloadChecklist}>导出 JSON</PButton>
        <PButton disabled={!result} onClick={downloadEvidence}>{result ? (evidenceDocComplete(buildEvidenceDoc(result, ["执行于域总检页"])) ? "导出证据包" : "证据链不足——先修再导") : "导出证据包"}</PButton>
      </Row>
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  background: "var(--p-bg-raised, rgba(34,34,46,0.9))", color: "inherit",
  border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", borderRadius: 6, padding: "4px 8px", fontSize: 12,
};
const kbdStyle: React.CSSProperties = {
  fontSize: 11, padding: "2px 8px", borderRadius: 5,
  background: "var(--p-bg-disabled, rgba(44,44,56,0.7))", border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))",
  fontVariantNumeric: "tabular-nums",
};
const stepChip: React.CSSProperties = { fontSize: 10, padding: "2px 6px", borderRadius: 5, cursor: "help" };
