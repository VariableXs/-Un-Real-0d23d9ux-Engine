/**
 * E 域页组⑤：系统面（F167 右键菜单 / F168 任务栏 / F169 快捷键 / F170 域总检）。
 */
import { useEffect, useState } from "react";
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
    </div>
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
    </div>
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

  // 重录捕获层独立于输入系统：录时不触发功能（keydown preventDefault + 只读键面）。
  useEffect(() => {
    if (!recording) return;
    function onKey(e: KeyboardEvent): void {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      if (!recording) return;
      const entry = DEFAULT_SHORTCUTS.find((x) => x.id === recording);
      if (!entry) return;
      const combo = {
        win: e.metaKey,
        ctrl: e.ctrlKey,
        alt: e.altKey,
        shift: e.shiftKey,
        key: e.key.length === 1 ? e.key.toUpperCase() : e.key,
      };
      const r = rebind(overrides, recording, combo);
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
    </div>
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
