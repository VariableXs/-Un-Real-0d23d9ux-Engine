/**
 * E 域页组⑧ · 深化实验室三（批次六）：八个引擎实验室面板。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice, useT } from "./ui";
import { CURVES, bezierEase, springOvershoot, SPRING_OVERSHOOT_ZETA, advance, reverse, scaledDurationMs, sampleLut, type InterruptibleState, type MotionTier } from "./motion-curve";
import { TooltipMachine, placeTooltip, tabSequence, FocusReturnLedger, trapCycle, TOOLTIP_SHOW_DELAY_MS, type FocusableNode } from "./hover-focus";
import { InteractionLedger } from "./interaction-ledger";
import { makeCatalogSurfacer, auditHiddenSpots, runStartupChain, degradationFor, heartbeatAudit } from "./error-surface";
import { packAssets, unpackAssets, validatePath, spriteEntryPair, type AssetEntry } from "./asset-package";
import { DPI_SCALES, snapZoneFor, snapRect, clampToMinimum, pullBackToScreen, rescaleForDpi, adaptForDpi, type ScreenRect, type DpiTier } from "./window-metrics";
import { PERF_BUDGETS, verdictFor, budgetAlert, regressionCheck, type PerfSample } from "./perf-budget";
import { rasterPointer, encodePng } from "./png-encode";
import { pageMessages } from "./message-catalog";

// ---------- 动效曲线面板 ----------

export function MotionCurveCard(): React.ReactNode {
  const demo = useMemo(() => {
    const lut = sampleLut(CURVES.enter!);
    const spring = springOvershoot({ v0: 0, omega0: 12, zeta: SPRING_OVERSHOOT_ZETA });
    // 打断续跑：走到头反向 → 回到 0（位置/值分离——不发散）。
    let st: InterruptibleState = { pos: 0, value: 0, velocity: 0, direction: 1 };
    for (let i = 0; i < 40; i++) st = advance(st, 16.6667, CURVES.enter!, 300);
    const at60 = st.pos;
    st = reverse(st);
    for (let i = 0; i < 40; i++) st = advance(st, 16.6667, CURVES.enter!, 300);
    return {
      lutMid: lut[Math.round(lut.length / 2)]!,
      enterMid: bezierEase(CURVES.enter!, 0.5),
      springPeak: Math.round(spring.peak * 1000) / 1000,
      at60: Math.round(at60 * 1000) / 1000,
      backTo: Math.round(st.value * 1000) / 1000,
      tiers: (["full", "reduced", "off"] as MotionTier[]).map((tier) => `${tier}:${scaledDurationMs(320, tier)}ms`),
    };
  }, []);
  return (
    <Card title="动效曲线解析（F124/F160 深化）">
      <Row label="cubic-bezier 中点值" sub={`enter(0.5) = ${demo.enterMid} · LUT 64 档中点 = ${Math.round(demo.lutMid * 1000) / 1000}（运行期查表零求解）`}>
        <span />
      </Row>
      <Row label={`spring 过冲峰值（ζ=${SPRING_OVERSHOOT_ZETA} 校准）`} sub={`峰 ${demo.springPeak}（契约 105% 过冲 → ≈1.05 物理解）· 解析首峰时刻可算`}>
        <span />
      </Row>
      <Row label="打断可逆：走到 60% 反向 → 回 0" sub={`60% 处值 ${demo.at60} → 回到 ${demo.backTo}（速度连续、不跳变——大作打断品质）`}>
        <span />
      </Row>
      <Row label="三档时长缩放（320ms 基准）" sub={demo.tiers.join(" · ") + "（60fps 栅格取整——不掉帧的时长语义）"}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 悬停与焦点面板 ----------

export function HoverFocusCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const m = new TooltipMachine();
    const log: string[] = [];
    m.hoverEnter("btn-1");
    log.push(`enter → ${m.state.phase}（${TOOLTIP_SHOW_DELAY_MS}ms 后显示）`);
    m.showDue();
    log.push(`due → ${m.state.phase}`);
    m.hoverLeave();
    log.push(`leave → ${m.state.phase}（200ms 宽限）`);
    m.hoverReenter("btn-1");
    log.push(`reenter → ${m.state.phase}（宽限内取消隐藏）`);
    // 四角翻转：目标在右下角 → tooltip 翻到 top-left。
    const corner = placeTooltip({ x: 380, y: 220, w: 40, h: 20 }, { w: 120, h: 32 }, 6, { width: 400, height: 240 }, "bottom");
    const nodes: FocusableNode[] = [
      { id: "a", order: 0, role: "button", disabled: false },
      { id: "b", order: 1, role: "button", disabled: true },
      { id: "c", order: 2, role: "text-line", disabled: false },
      { id: "d", order: 3, role: "custom", disabled: false },
    ];
    const next = tabSequence(nodes, 0);
    const ledger = new FocusReturnLedger();
    ledger.open("popover-x", "btn-trigger");
    const ret = ledger.close("popover-x");
    const orphan = ledger.close("popover-x");
    return { log, corner, next: nodes[next]!.id, ret: ret?.returnTo ?? null, orphan: orphan === null && ledger.leaks().length === 0, trap: trapCycle(["x", "y", "z"], "z") };
  }, []);
  return (
    <Card title="悬停与焦点（F205/F206 深化）">
      {demo.log.map((line, i) => (
        <Row key={i} label={line} sub="">
          <span />
        </Row>
      ))}
      <Row label="四角翻转几何" sub={`右下角目标 → placement=${demo.corner.placement} @ (${demo.corner.x},${demo.corner.y})（放不下按剩余空间翻转 + 视口夹紧）`}>
        <span />
      </Row>
      <Row label="Tab 序（跳过禁用 + 循环）" sub={`从 a 下一个 → ${demo.next}（b 禁用被跳过）· 陷阱循环尾→首: ${demo.trap}`}>
        <span />
      </Row>
      <Row label="焦点归还配对" sub={`关闭 popover-x → 归还 ${demo.ret} · 重复关闭=显性 null · 泄漏审计 ${demo.orphan ? "干净" : "有泄漏"}`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 交互台账面板 ----------

export function InteractionLedgerCard(): React.ReactNode {
  const t = useT();
  const [tick, setTick] = useState(0);
  const demo = useMemo(() => {
    const ledger = new InteractionLedger();
    const base = Date.now();
    // rage click：1.5s 内同元素 3 点。
    for (let i = 0; i < 3; i++) ledger.record({ page: "icons", elementId: "swap-btn", at: base + i * 300, action: "click", feedbackMs: 400, durationMs: null, verdict: "laggy" });
    // dead click。
    ledger.record({ page: "wallpaper", elementId: "fake-btn", at: base + 2000, action: "click", feedbackMs: null, durationMs: null, verdict: "no-feedback" });
    // 浮层反复开关。
    for (let i = 0; i < 3; i++) ledger.togglePopover("startmenu", "preset-card", base + 3000 + i * 5000);
    ledger.record({ page: "verdict", elementId: "run", at: base + 20000, action: "click", feedbackMs: 40, durationMs: null, verdict: "smooth" });
    ledger.recordErrorWander("font", base + 21000);
    const exported = ledger.exportLedger(base + 30000);
    return { signals: ledger.topFrustrations(5), exported, replay: ledger.replay(base, base + 30000).slice(0, 4) };
  }, [tick]);
  return (
    <Card title={t("usageTitle") + " · 交互台账（十三章）"}>
      <Row label="挫败信号捕获" sub={demo.signals.map((s) => `${s.kind}@${s.elementId}`).join(" · ") || "无"}>
        <span style={{ color: demo.signals.length > 0 ? "var(--p-warn)" : "var(--p-success)" }}>{demo.signals.length} 条</span>
      </Row>
      {demo.signals.slice(0, 3).map((s, i) => (
        <Row key={i} label={s.message} sub={`${s.kind} · ${new Date(s.at).toLocaleTimeString()}（直接可执行——不用人工猜）`}>
          <span />
        </Row>
      ))}
      <Row label="导出（零内容字段）" sub={`events ${demo.exported.events.length} · smooth 占比 ${Math.round(demo.exported.stats.smoothRatio * 100)}% · P95 反馈 ${demo.exported.stats.p95FeedbackMs}ms（100ms 红线对账）`}>
        <PButton onClick={() => setTick((n) => n + 1)}>{t("run")}</PButton>
      </Row>
      <Row label="回放故事线（前 4 条）" sub={demo.replay.join(" ｜ ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 异常显性化面板 ----------

export function ErrorSurfaceCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const surfacer = makeCatalogSurfacer((page) => pageMessages(page, "zh")?.error ?? null);
    const surfaced = surfacer("tokens/save", "error", new Error("quota exceeded"), Date.now());
    const hidden = auditHiddenSpots();
    const chain = runStartupChain(
      () => null,
      () => null,
      () => "motion-monitor 采样器未响应（演示失败环）",
    );
    const degraded = degradationFor(chain);
    const beats = heartbeatAudit([{ source: "widget-refresh", beatAt: Date.now() - 90_000, intervalMs: 30_000 }, { source: "usage-sync", beatAt: Date.now() - 10_000, intervalMs: 60_000 }], Date.now());
    return { surfaced, hidden, chain, degraded, beats };
  }, []);
  return (
    <Card title="异常显性化与启动链（十三章补）">
      <Row label="统一异常包装（三要素 + 技术折叠）" sub={`${demo.surfaced.triad.what}｜${demo.surfaced.triad.next} · 技术详情（默认折叠）: ${demo.surfaced.technical}`}>
        <span style={{ color: "var(--p-danger)" }}>{demo.surfaced.severity}</span>
      </Row>
      <Row label="隐蔽捕获点巡检" sub={demo.hidden.spots.map((s) => `${s.spot}:${s.capture.split("（")[0]}`).join(" · ")}>
        <span style={{ color: demo.hidden.allCovered ? "var(--p-success)" : "var(--p-danger)" }}>{demo.hidden.allCovered ? "六类全覆盖" : "有遗漏"}</span>
      </Row>
      <Row label="启动链探针" sub={demo.chain.outcomes.map((o) => `${o.name}:${o.ok ? "绿" : `红(${o.elapsedMs}ms)`}`).join(" → ")}>
        <span style={{ color: demo.chain.healthy ? "var(--p-success)" : "var(--p-warn)" }}>{demo.chain.healthy ? "全绿" : `首失败环: ${demo.chain.firstFailure?.name}`}</span>
      </Row>
      <Row label="降级路径（诚实降级不白屏）" sub={demo.degraded.message}>
        <span />
      </Row>
      <Row label="心跳看护" sub={demo.beats.map((b) => `${b.source}: ${b.late ? `超期 ${b.overdueMs}ms——${b.advice.split("——")[1]}` : "正常"}`).join(" · ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 素材包面板 ----------

export function AssetPackageCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const make = (size: number): Uint8Array => encodePng(rasterPointer("arrow", { size, color: "#6e7fd4", outline: "#ffffff", aaPx: 1 }));
    const entries: AssetEntry[] = spriteEntryPair("arrow", 24, make);
    const packed = packAssets(entries, Date.now(), 5 * 1024 * 1024);
    const ok = unpackAssets(JSON.parse(JSON.stringify(packed.pkg)));
    const tampered = JSON.parse(JSON.stringify(packed.pkg));
    tampered.blobs[0] = tampered.blobs[1];
    const bad = unpackAssets(tampered);
    return {
      paths: entries.map((e) => e.path),
      pathOk: entries.every((e) => validatePath(e.use, e.path)),
      total: packed.totalBytes,
      within: packed.withinBudget,
      ok: ok.ok,
      bad: bad.ok ? "漏放——必须修" : `${(bad as { reason: string }).reason}（${(bad as { badPaths: string[] }).badPaths.join(",")}）`,
    };
  }, []);
  return (
    <Card title={t("assetTitle") + " · 资产包（F127 联动）"}>
      <Row label="打包（清单 + CRC 逐条）" sub={demo.paths.join(" · ") + ` · 路径契约 ${demo.pathOk ? "符合" : "违约"} · 总 ${(demo.total / 1024).toFixed(1)}KB（${demo.within ? "预算内" : "超预算分批"}` + "）"}>
        <span />
      </Row>
      <Row label="解包校验" sub={`合法包 ${demo.ok ? "通过" : "拒绝"} · 调换字节序的篡改包 ${demo.bad}`}>
        <span style={{ color: demo.ok ? "var(--p-success)" : "var(--p-danger)" }}>{demo.ok ? "双向通过" : "失败"}</span>
      </Row>
      <Notice tone="info">包格式公开（format: vxtheme-assets）——清单即文档，第三方可读写，无锁（十四章数据开放）。</Notice>
    </Card>
  );
}

// ---------- 多屏与窗口几何面板 ----------

export function WindowMetricsCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const screen: ScreenRect = { x: 0, y: 0, w: 1920, h: 1080, workTop: 0, workBottom: 1048 };
    const zones: Array<[number, number]> = [[4, 500], [1916, 500], [960, 4], [4, 4], [1916, 1044], [960, 500]];
    const zoneResults = zones.map(([x, y]) => `${x},${y}→${snapZoneFor(x, y, screen)}`);
    const half = snapRect("left", screen)!;
    const mem = { id: "w1", rect: { x: 1500, y: 200, w: 800, h: 600, workTop: 0, workBottom: 1048 }, dpiTier: "100%" as DpiTier };
    const smaller: ScreenRect = { x: 0, y: 0, w: 1366, h: 768, workTop: 0, workBottom: 736 };
    const pulled = pullBackToScreen(mem, smaller);
    const rescaled = rescaleForDpi(mem, "150%");
    const clamp = clampToMinimum({ w: 300, h: 150 }, { w: 500, h: 300 });
    const dpi = adaptForDpi(48, "150%");
    return {
      zoneResults,
      half: `${half.w}×${half.h}`,
      pulled: pulled.note,
      rescaled: rescaled.note,
      clamp: `${clamp.w}×${clamp.h}（${clamp.degraded}）`,
      dpi: `48px@150% → 物理 ${dpi.physical}px · sprite ${dpi.spriteTier}x（档位表 ${Object.values(DPI_SCALES).join("/")}）`,
    };
  }, []);
  return (
    <Card title="多屏与窗口几何（F168/F237 联动）">
      <Row label="贴边落点判定（8px 触发带 · 八落点）" sub={demo.zoneResults.join(" · ")}>
        <span />
      </Row>
      <Row label="半屏落点（工作区口径——任务栏永不遮）" sub={`left → ${demo.half}`}>
        <span />
      </Row>
      <Row label="显示器热切换拉回" sub={demo.pulled}>
        <span />
      </Row>
      <Row label="DPI 变更重排（WM_DPICHANGED 口径）" sub={demo.rescaled}>
        <span />
      </Row>
      <Row label="最小尺寸钳制（三档降级）" sub={demo.clamp}>
        <span />
      </Row>
      <Row label="DPI 四档适配" sub={demo.dpi}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 性能预算面板 ----------

export function PerfBudgetCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const now = Date.now();
    const samples: PerfSample[] = [];
    for (let i = 0; i < 20; i++) {
      samples.push({ page: "ime", metric: "interaction-p95-ms", value: 12 + (i % 5), at: now });
      samples.push({ page: "sound", metric: "interaction-p95-ms", value: 180 + (i % 40), at: now });
      samples.push({ page: "ctxmenu", metric: "interaction-p95-ms", value: 130 + (i % 20), at: now }); // 超 100ms 预算。
    }
    const { verdicts, unregistered } = verdictFor(samples);
    const alerts = verdicts.filter((v) => !v.within).map(budgetAlert);
    const prev = samples.map((s) => ({ ...s, value: s.value * 0.8 }));
    const regressions = regressionCheck(prev, samples);
    return { verdicts: verdicts.map((v) => `${v.page}:${v.p95}/${v.limit}${v.within ? "✓" : "✗"}`), unregistered, alerts, regressions: regressions.filter((r) => r.regressed), budgetCount: PERF_BUDGETS.length };
  }, []);
  return (
    <Card title="性能预算台账（八章/F041 联动）">
      <Row label={`预算表 ${demo.budgetCount} 条（主册硬线一处一事实）`} sub={demo.verdicts.join(" · ")}>
        <span />
      </Row>
      {demo.alerts.map((a, i) => (
        <Notice key={i} tone={a.tone}>{`${a.what}｜${a.why}｜${a.next}`}</Notice>
      ))}
      <Row label="未登记采样（显性拒绝——预算先于采样）" sub={demo.unregistered.join(", ") || "无"}>
        <span />
      </Row>
      <Row label="回归对比（F061 口径：劣化 >10% 且超线 = 红）" sub={demo.regressions.map((r) => `${r.page}: ${r.prevP95}→${r.currP95}`).join(" · ") || "无回归"}>
        <span style={{ color: demo.regressions.length > 0 ? "var(--p-danger)" : "var(--p-success)" }}>{demo.regressions.length > 0 ? `${demo.regressions.length} 处回归` : "干净"}</span>
      </Row>
    </Card>
  );
}
