/**
 * E 域页组①：主题（F151 令牌表 / F152 实时预览编辑器 / F153 深浅自动切换 /
 * F162 每应用主题例外）。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import {
  COLOR_TOKENS, SPACING_TOKENS, RADIUS_DEFAULTS, FONT_DEFAULTS,
  MOTION_CURVES, MOTION_DURATIONS, tokenTableHash,
  loadTokenTable, saveTokenTable, resetToken, applyTokenTableToDOM,
  tokenTableJsonSchema, coverageReport, defaultTokenTable,
  type ColorGroup, type TokenTable, type MotionCurve,
} from "./tokens";
import { PreviewSession, PREVIEW_WIDTH_PX, type PreviewScene } from "./preview";
import {
  auditTableContrast, diffSummary, diffTokenTables, scanHardcodedColors,
  TransitionDriver,
  type HardcodeHit,
} from "./theme-engine";
import { lockedTableForException } from "./archive-engine";
import { compatibilityVerdict, migrateTokenTableV0 } from "./compat-matrix";
import { TOKEN_TABLE_VERSION } from "./tokens";
import { hasSunProvider, injectSunTimes, setSunProvider, sunAccuracyVerdict } from "./integrations";
import {
  loadAutoDarkConfig, saveAutoDarkConfig, parseHHMM, nextSwitchAt,
  sunTimesMinutes, AutoDarkController, type AutoDarkMode,
} from "./autodark";
import {
  loadAppExceptions, saveAppExceptions, addException, removeException,
  degradationLabel, MAX_EXCEPTIONS, type AppThemeException,
} from "./appexcept";
import { Card, PageHeader, Row, Toggle, Slider, Segmented, PButton, Notice, ColorChip, MiniDesktop, useT, usePersonaSection } from "./ui";
import type { LaneT } from "./labels";
import { PaletteLabCard, SchemaLabCard, SolarLabCard } from "./pages-lab";

const GROUP_ZH: Record<ColorGroup, Parameters<LaneT>[0]> = {
  bg: "groupBg", fg: "groupFg", brand: "groupBrand", state: "groupState", line: "groupLine", misc: "groupMisc",
};

// ---------- F151 令牌表 ----------

export function TokensPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const [table, setTable] = useState<TokenTable>(() => loadTokenTable());
  const [saved, setSaved] = useState(false);
  const [scanSrc, setScanSrc] = useState("");
  const coverage = useMemo(() => coverageReport(table), [table]);
  const contrastFixes = useMemo(() => auditTableContrast(table), [table]);
  const scanHits = useMemo<HardcodeHit[]>(
    () => (scanSrc.trim() ? scanHardcodedColors("粘贴片段", scanSrc) : []),
    [scanSrc],
  );

  function commit(next: TokenTable): void {
    setTable(next);
    saveTokenTable(next);
    applyTokenTableToDOM(next);
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1500);
  }

  function exportSchema(): void {
    const blob = new Blob([JSON.stringify(tokenTableJsonSchema(), null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "varix-token-table.schema.json";
    a.click();
    URL.revokeObjectURL(a.href);
  }

  return (
    <div>
      <PageHeader
        title={t("tokensTitle")}
        hint={t("tokensHint")}
        right={<PButton onClick={exportSchema}>{t("exportSchema")}</PButton>}
      />
      {saved ? <Notice tone="ok">{t("saved")}——{t("latencyOk")}</Notice> : null}
      <Card title={t("coverage")}>
        <Row label={`${coverage.covered}/${coverage.total}`} sub={coverage.missing.length > 0 ? coverage.missing.join(" ") : "24/24 · 全覆盖"}>
          <span />
        </Row>
      </Card>
      <Card title="对比度审计（AA 4.5:1 门禁 · F141 公式同源）">
        {contrastFixes.length === 0 ? (
          <Notice tone="ok">全表文本/状态色对画布底全部达标——主题自证清白（B-1104 联动）。</Notice>
        ) : null}
        {contrastFixes.map((f) => (
          <Row
            key={f.tokenKey}
            label={f.tokenKey}
            sub={`当前 ${f.ratioBefore.toFixed(2)}:1 → 建议 ${f.suggestion}（${f.ratioAfter.toFixed(2)}:1 · 保持色相只调明度轴）`}
          >
            <span style={{ ...swatchStyle, background: f.current }} title="当前" />
            <span style={{ ...swatchStyle, background: f.suggestion }} title="建议" />
            <PButton kind="primary" onClick={() => commit({ ...table, colors: { ...table.colors, [f.tokenKey]: f.suggestion } })}>
              采纳建议
            </PButton>
          </Row>
        ))}
      </Card>
      <Card title="硬编码扫描（B-1104 执法 · 语义色字面量审计）">
        <textarea
          value={scanSrc}
          onChange={(e) => setScanSrc(e.target.value)}
          placeholder="粘贴界面源码片段——与令牌默认物理值完全相等的 hex 字面量即违例（门禁要确定性，不做近似猜测）。"
          aria-label="硬编码扫描源码"
          style={{ ...inputStyle, width: "100%", minHeight: 64, fontFamily: "monospace" }}
        />
        {scanSrc.trim() ? (
          scanHits.length === 0 ? (
            <Notice tone="ok">零命中——该片段干净（filesClean 口径）。</Notice>
          ) : (
            scanHits.slice(0, 8).map((h, i) => (
              <Row key={`${h.line}-${i}`} label={`第 ${h.line} 行 · ${h.literal}`} sub={h.snippet}>
                <code style={monoStyle}>{h.suggestion}</code>
              </Row>
            ))
          )
        ) : null}
      </Card>
      <CompatMatrixCard />
      {(Object.keys(GROUP_ZH) as ColorGroup[]).map((g) => (
        <Card key={g} title={t(GROUP_ZH[g])}>
          {COLOR_TOKENS.filter((def) => def.group === g).map((def) => (
            <Row key={def.key} label={`${def.zh} ${def.key}`} sub={def.en}>
              <ColorChip
                value={table.colors[def.key] ?? "#000000"}
                ariaLabel={def.zh}
                onChange={(hex) => commit({ ...table, colors: { ...table.colors, [def.key]: hex } })}
              />
              <PButton onClick={() => commit(resetToken(table, def.key))} ariaLabel={t("restoreThisToken")}>↺</PButton>
            </Row>
          ))}
        </Card>
      ))}
      <Card title={t("groupSpacing")}>
        {SPACING_TOKENS.map((s) => (
          <Row key={s.key} label={`${s.zh} ${s.key}`} sub={`${s.px}px · ${s.en}`}><span /></Row>
        ))}
      </Card>
      <Card title={t("groupRadius")}>
        {(["control", "card", "window"] as const).map((slot) => (
          <Row key={slot} label={`圆角 · ${slot}`} sub={`默认 ${RADIUS_DEFAULTS[slot]}px`}>
            <Slider
              value={table.radius[slot]}
              min={0} max={32} step={1}
              ariaLabel={`圆角 ${slot}`}
              format={(v) => `${v}px`}
              onChange={(v) => commit({ ...table, radius: { ...table.radius, [slot]: v } })}
            />
          </Row>
        ))}
      </Card>
      <Card title={t("groupFont")}>
        {(["caption", "body", "title", "display"] as const).map((slot) => (
          <Row key={slot} label={`字号 · ${slot}`} sub={`默认 ${FONT_DEFAULTS[slot]}px`}>
            <Slider
              value={table.font[slot]}
              min={slot === "caption" ? 10 : slot === "body" ? 12 : slot === "title" ? 14 : 18}
              max={slot === "caption" ? 20 : slot === "body" ? 24 : slot === "title" ? 40 : 96}
              step={1}
              ariaLabel={`字号 ${slot}`}
              format={(v) => `${v}px`}
              onChange={(v) => commit({ ...table, font: { ...table.font, [slot]: v } })}
            />
          </Row>
        ))}
      </Card>
      <Card title={t("groupMotion")}>
        {(Object.keys(MOTION_CURVES) as MotionCurve[]).map((name) => (
          <Row key={name} label={`${MOTION_CURVES[name].zh} · ${name}`} sub={MOTION_CURVES[name].bezier}>
            <select
              value={table.motion[name].duration}
              aria-label={`${name} ${t("duration")}`}
              style={selStyle}
              onChange={(e) => commit({ ...table, motion: { ...table.motion, [name]: { ...table.motion[name], duration: e.target.value as TokenTable["motion"][MotionCurve]["duration"] } } })}
            >
              {Object.entries(MOTION_DURATIONS).map(([k, v]) => (
                <option key={k} value={k}>{v.zh} · {v.ms}ms</option>
              ))}
            </select>
          </Row>
        ))}
      </Card>
      <SchemaLabCard />
      <PaletteLabCard />
    </div>
  );
}

// ---------- F152 实时预览编辑器 ----------

export function PreviewPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const [session] = useState(() => new PreviewSession());
  const [, bump] = useState(0);
  const [scene, setScene] = useState<PreviewScene>("desktop");
  const [started, setStarted] = useState(false);
  const [discardMsg, setDiscardMsg] = useState<string | null>(null);
  const working = loadTokenTable();
  // 换装过渡引擎演示（TransitionDriver 驱动——doSwap 全生命周期恰好一次）。
  const [transDemo, setTransDemo] = useState<{ progress?: number; oldOpacity?: number; newOpacity?: number; swapped?: boolean }>({});
  const [transRunning, setTransRunning] = useState(false);
  const transRafRef = useRef(0);
  useEffect(() => () => window.clearTimeout(transRafRef.current), []);

  function runTransition(): void {
    const driver = new TransitionDriver();
    driver.start();
    setTransRunning(true);
    const t0 = performance.now();
    let swapped = false;
    function step(): void {
      const f = driver.tick(performance.now() - t0);
      if (f.doSwap) swapped = true; // doSwap 全生命周期恰好一次（引擎保证）
      setTransDemo({ progress: f.progress, oldOpacity: f.oldOpacity, newOpacity: f.newOpacity, swapped });
      if (!driver.isDone) {
        transRafRef.current = window.setTimeout(step, 16);
      } else {
        setTransRunning(false);
      }
    }
    step();
  }

  function start(): void {
    session.start(loadTokenTable());
    setStarted(true);
    setDiscardMsg(null);
    bump((n) => n + 1);
  }

  function patchRadius(v: number): void {
    const t0 = performance.now();
    session.patch((draft) => { draft.radius.window = v; });
    session.recordRenderLatency(performance.now() - t0);
    bump((n) => n + 1);
  }

  function apply(): void {
    const { applied } = session.apply();
    saveTokenTable(applied);
    applyTokenTableToDOM(applied);
    setStarted(false);
    bump((n) => n + 1);
  }

  function discard(): void {
    const { restored, hashVerified } = session.discard();
    saveTokenTable(restored);
    applyTokenTableToDOM(restored);
    setStarted(false);
    setDiscardMsg(hashVerified ? `${t("hashVerified")}` : "回退残留——请到域总检回炉");
    bump((n) => n + 1);
  }

  return (
    <div>
      <PageHeader
        title={`${t("navThemePreview")}（F152）`}
        hint={t("dirtyBannerHint")}
        right={<PButton kind="primary" onClick={start} disabled={started}>开启编辑会话</PButton>}
      />
      {started ? (
        <Notice tone="warn"><b>⚠ {t("unappliedChanges")}</b> —— {t("dirtyBannerHint")}　
          <PButton kind="primary" onClick={apply}>{t("apply")}</PButton>{" "}
          <PButton onClick={discard}>{t("discard")}</PButton>
        </Notice>
      ) : null}
      {discardMsg ? <Notice tone="ok">{discardMsg}</Notice> : null}
      {session.active && !session.withinLatencyBudget ? <Notice tone="danger">{t("latencyOver")}（{Math.round(session.session?.lastLatencyMs ?? 0)}ms）</Notice> : null}
      {session.active && session.withinLatencyBudget && session.session?.lastLatencyMs ? <Notice tone="info">{t("latencyOk")}（{Math.round(session.session?.lastLatencyMs ?? 0)}ms）</Notice> : null}
      <Card title={t("previewScene")}>
        <Segmented
          value={scene}
          ariaLabel={t("previewScene")}
          onChange={setScene}
          options={[
            { value: "desktop", label: t("sceneDesktop") },
            { value: "explorer", label: t("sceneExplorer") },
            { value: "settings", label: t("sceneSettings") },
          ]}
        />
      </Card>
      <Card title={`迷你桌面 · ${PREVIEW_WIDTH_PX}px · 0.5x 实时渲染`}>
        <MiniDesktop width={PREVIEW_WIDTH_PX} accent={working.colors["--p-accent"] ?? "#6e7fd4"} scene={scene} />
        <Row label="窗口圆角" sub={`${working.radius.window}px`}>
          <Slider value={working.radius.window} min={0} max={32} step={1} ariaLabel="预览窗口圆角" format={(v) => `${v}px`} onChange={patchRadius} />
        </Row>
        {started ? (
          <Row label="应用前差异预览" sub={diffSummary(diffTokenTables(session.session!.baseline, working))}>
            <span />
          </Row>
        ) : null}
      </Card>
      <Card title="换装过渡引擎（300ms 双层交叉 · 中段原子替换）">
        <Row label="过渡帧演示" sub={`总时长 ${Math.round((transDemo.progress ?? 0) * 100)}% · 旧表 ${Math.round((transDemo.oldOpacity ?? 1) * 100)}% / 新表 ${Math.round((transDemo.newOpacity ?? 0) * 100)}%${transDemo.swapped ? " · 令牌已在中段原子替换" : ""}`}>
          <PButton kind="primary" disabled={transRunning} onClick={runTransition}>预演 300ms 交叉</PButton>
        </Row>
        <div style={{ position: "relative", height: 40, borderRadius: 8, overflow: "hidden", border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))" }}>
          <div style={{ position: "absolute", inset: 0, background: "#5a5a8a", opacity: transDemo.oldOpacity ?? 1 }} />
          <div style={{ position: "absolute", inset: 0, background: "var(--p-accent, #6e7fd4)", opacity: transDemo.newOpacity ?? 0 }} />
          <div style={{ position: "absolute", top: 0, bottom: 0, left: "50%", width: 1, background: "rgba(255,255,255,0.4)" }} title="中段原子替换点 50%" />
        </div>
        <Row label="亮度守恒" sub="旧+新不透明度恒为 100%——双层交叉全程无白屏帧（录屏帧检口径的数学基础）">
          <span />
        </Row>
      </Card>
    </div>
  );
}

// ---------- F153 深浅自动切换 ----------

export function AutoDarkPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const cfg = loadAutoDarkConfig();
  const [now] = useState(() => Date.now());
  const next = nextSwitchAt(cfg, now);

  function update(patch: Partial<typeof cfg>): void {
    saveAutoDarkConfig({ ...cfg, ...patch });
  }

  const dayOfYear = Math.floor((now - new Date(now).setMonth(0, 0)) / 86400000);
  const sun = sunTimesMinutes(39.9, dayOfYear); // 北京纬度兜底（F116 就绪后由其城市数据注入）

  return (
    <div>
      <PageHeader title={`${t("navThemeAuto")}（F153）`} hint="深/浅主题对绑定切换：300ms 交叉淡入跨零点无闪烁；手动切主题 = 自动切换暂停当日（尊重手动——次日恢复）。" />
      <Card>
        <Row label={t("enabled")}>
          <Toggle checked={cfg.enabled} onChange={(v) => update({ enabled: v })} ariaLabel={t("navThemeAuto")} />
        </Row>
        <Row label={t("modeTimer")}>
          <Segmented
            value={cfg.mode}
            ariaLabel="切换模式"
            onChange={(v) => update({ mode: v as AutoDarkMode })}
            options={[{ value: "timer", label: t("modeTimer") }, { value: "sunset", label: t("modeSunset") }, { value: "off", label: t("off") }]}
          />
        </Row>
        {cfg.mode === "timer" ? (
          <>
            <Row label={t("darkAt")} sub="HH:MM">
              <input type="time" value={cfg.darkAt} style={selStyle} aria-label={t("darkAt")} onChange={(e) => { if (parseHHMM(e.target.value) !== null) update({ darkAt: e.target.value }); }} />
            </Row>
            <Row label={t("lightAt")} sub="HH:MM">
              <input type="time" value={cfg.lightAt} style={selStyle} aria-label={t("lightAt")} onChange={(e) => { if (parseHHMM(e.target.value) !== null) update({ lightAt: e.target.value }); }} />
            </Row>
          </>
        ) : (
          <>
            <Row label={t("sunset")} sub={sun ? `${Math.floor((sun.sunset ?? 0) / 60)}:${String((sun.sunset ?? 0) % 60).padStart(2, "0")}（F116 兜底推算）` : "极昼/极夜"}>
              <span style={monoStyle}>{sun?.sunset ?? "—"}</span>
            </Row>
            <Row label={t("sunrise")} sub={sun ? `${Math.floor((sun.sunrise ?? 0) / 60)}:${String((sun.sunrise ?? 0) % 60).padStart(2, "0")}` : "—"}>
              <span style={monoStyle}>{sun?.sunrise ?? "—"}</span>
            </Row>
          </>
        )}
        <Row label={t("pairBinding")} sub="深色主题 × 浅色主题">
          <code style={monoStyle}>{cfg.pair.darkThemeId} × {cfg.pair.lightThemeId}</code>
        </Row>
        <Row label={t("nextSwitch")} sub={next ? new Date(next).toLocaleTimeString() : "—"}>
          <PButton onClick={() => { const c = new AutoDarkController({ applySide: () => {}, showAdvanceToast: () => {}, report: () => {} }); c.pauseToday(Date.now()); }}>{t("pauseToday")}</PButton>
          <PButton onClick={() => { const c = new AutoDarkController({ applySide: () => {}, showAdvanceToast: () => {}, report: () => {} }); c.skipTonight(Date.now()); }}>{t("skipTonight")}</PButton>
        </Row>
      </Card>
      <SolarLabCard />
      <SunProviderCard />
    </div>
  );
}

/** F116 数据注入点状态卡（integrations 接线）：来源/注入/±5min 判定全可见。 */
function SunProviderCard(): React.ReactNode {
  const today = new Date().toISOString().slice(0, 10);
  const injection = injectSunTimes(today);
  const dayOfYear = Math.floor((Date.now() - new Date(Date.now()).setMonth(0, 0)) / 86400000);
  const fallback = sunTimesMinutes(39.9, dayOfYear); // 北京纬度兜底（演示口径）
  const verdict = injection.times && fallback
    ? sunAccuracyVerdict(injection.times, { sunset: fallback.sunset ?? 0, sunrise: fallback.sunrise ?? 0, source: "天文兜底" })
    : null;

  function demoInject(): void {
    setSunProvider((date) => ({ sunset: 18 * 60 + 12, sunrise: 5 * 60 + 58, source: `F116演示库·${date}` }));
  }

  return (
    <Card title="F116 日落数据注入点（Schema 先行——F116 就绪即插即用）">
      <Row label="provider 状态" sub={hasSunProvider() ? "已注册（F116 城市数据源在位）" : "未注册——sunset 模式走天文公式兜底（autodark.sunTimesMinutes）"}>
        <PButton onClick={demoInject}>注册演示 provider</PButton>
      </Row>
      <Row label="今日注入" sub={injection.detail}>
        <span />
      </Row>
      {injection.times ? (
        <Row
          label={`注入值 日落 ${Math.floor(injection.times.sunset / 60)}:${String(injection.times.sunset % 60).padStart(2, "0")} / 日出 ${Math.floor(injection.times.sunrise / 60)}:${String(injection.times.sunrise % 60).padStart(2, "0")}`}
          sub={verdict ? `±${5}min 判定: ${verdict.ok ? "通过" : `偏差 日落 ${verdict.sunsetDeltaMin}min / 日出 ${verdict.sunriseDeltaMin}min`}（F116 判据口径）` : ""}
        >
          <span />
        </Row>
      ) : null}
    </Card>
  );
}

// ---------- F162 每应用主题例外 ----------

export function ExceptionsPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const cfg = loadAppExceptions();
  const workingAccent = loadTokenTable().colors["--p-accent"] as string | undefined;
  const [appId, setAppId] = useState("");
  const [appName, setAppName] = useState("");
  const [mode, setMode] = useState<"dark" | "light">("dark");
  const [msg, setMsg] = useState<string | null>(null);

  function add(): void {
    if (!appId.trim() || !appName.trim()) {
      setMsg("应用 id 与名称都要填");
      return;
    }
    const item: Omit<AppThemeException, "tokenAware"> = { appId: appId.trim(), appName: appName.trim(), mode, accentOverride: null };
    const r = addException(cfg, item);
    saveAppExceptions(r.config);
    setMsg(r.reason);
    if (r.ok) { setAppId(""); setAppName(""); }
  }

  return (
    <div>
      <PageHeader title={t("exceptionsTitle")} hint={t("exceptionsHint")} />
      {msg ? <Notice tone="info">{msg}</Notice> : null}
      <Card title={`${t("addException")}（${cfg.exceptions.length}/${MAX_EXCEPTIONS}）`}>
        <Row label="应用 id"><input value={appId} onChange={(e) => setAppId(e.target.value)} style={inputStyle} aria-label="应用 id" placeholder="app.id" /></Row>
        <Row label="应用名"><input value={appName} onChange={(e) => setAppName(e.target.value)} style={inputStyle} aria-label="应用名" /></Row>
        <Row label={t("exceptionMode")}>
          <Segmented value={mode} ariaLabel={t("exceptionMode")} onChange={setMode} options={[{ value: "dark", label: "深色" }, { value: "light", label: "浅色" }]} />
        </Row>
        <Row label=""><PButton kind="primary" onClick={add}>{t("addException")}</PButton></Row>
      </Card>
      <Card title="例外清单">
        {cfg.exceptions.length === 0 ? <Notice tone="info">尚无例外——全局主题统一生效（这是默认，也是常态）。</Notice> : null}
        {cfg.exceptions.map((e) => {
          // 锁定令牌派生（archive-engine）：全局广播时该应用跳过，实际收到的表由
          // 例外模式从派生引擎生成——此处展示派生结果让「锁定」看得见。
          const globalAccent = workingAccent ?? "#6e7fd4";
          const locked = lockedTableForException(e, e.accentOverride ?? globalAccent);
          return (
            <Row key={e.appId} label={`${e.appName} · ${e.mode === "dark" ? "深色" : "浅色"}`} sub={degradationLabel(e) ?? t("tokenAware")}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11, opacity: 0.85 }}>
                <span style={{ ...swatchStyle, background: locked.colors["--p-bg-canvas"] ?? "#14141c" }} title="锁定画布底" />
                <span style={{ ...swatchStyle, background: locked.colors["--p-accent"] ?? globalAccent }} title="锁定强调色" />
                <span>锁定表已派生</span>
              </span>
              <PButton kind="danger" onClick={() => saveAppExceptions(removeException(cfg, e.appId))}>{t("reset")}</PButton>
            </Row>
          );
        })}
      </Card>
    </div>
  );
}

const selStyle: React.CSSProperties = {
  background: "var(--p-bg-raised, rgba(34,34,46,0.9))",
  color: "inherit",
  border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))",
  borderRadius: 6,
  padding: "4px 8px",
  fontSize: 12,
};
const inputStyle: React.CSSProperties = { ...selStyle, width: 180 };
const monoStyle: React.CSSProperties = { fontVariantNumeric: "tabular-nums", fontSize: 12, opacity: 0.85 };
const swatchStyle: React.CSSProperties = { width: 18, height: 18, borderRadius: 4, display: "inline-block", border: "1px solid rgba(255,255,255,0.25)", flexShrink: 0 };

/** 供 PreviewPage 哈希展示（F152 放弃零残留对拍口径）。 */
export function currentTableHash(): string {
  return tokenTableHash(loadTokenTable());
}

/** 恢复全部令牌默认（F151 大回退入口）。 */
export function resetAllTokens(): void {
  const d = defaultTokenTable();
  saveTokenTable(d);
  applyTokenTableToDOM(d);
}

/** 兼容矩阵卡（compat-matrix 接线）：版本戳 + v0 旧主题迁移演示（降级清单可见、可逆）。 */
function CompatMatrixCard(): React.ReactNode {
  const [demoVer, setDemoVer] = useState<number | null>(0);
  const verdict = compatibilityVerdict(demoVer);
  const [migration, setMigration] = useState<ReturnType<typeof migrateTokenTableV0> | null>(null);

  function runMigration(): void {
    setMigration(migrateTokenTableV0({
      version: 0,
      colors: { "--bg-canvas": "#101018", "--accent": "#ff8800", "--bogus": "#123456" },
    }));
  }

  return (
    <Card title={`兼容矩阵（当前版本 v${TOKEN_TABLE_VERSION} · 旧主题不拒之门外——迁移必须可逆）`}>
      <Row label="待检包版本" sub={`判定: ${verdict.level} · ${verdict.reason}`}>
        <Segmented
          value={String(demoVer)} ariaLabel="待检包版本"
          onChange={(v) => { setDemoVer(v === "none" ? null : Number(v)); setMigration(null); }}
          options={[
            { value: "0", label: "v0 旧主题" },
            { value: "1", label: "v1 当前" },
            { value: "none", label: "无版本戳" },
          ]}
        />
      </Row>
      {verdict.degradations.length > 0 ? (
        <Row label="降级清单（迁移前可见）" sub={verdict.degradations.join("；")}>
          <span />
        </Row>
      ) : null}
      <Row label="v0 迁移演示" sub={migration ? `${migration.reason}${migration.ok ? ` · 桥接 ${migration.mapped.length} 项` : ""}` : "11 变量旧主题 → 24 色语义表；非法值逐项指出"}>
        <PButton kind="primary" onClick={runMigration}>跑迁移</PButton>
      </Row>
      {migration?.ok ? (
        <Row label="迁移结果" sub={migration.mapped.map((m) => `${m.from}→${m.to}`).join(" · ")}>
          <PButton kind="danger" onClick={() => {
            // 迁移不是单行道：应用迁移表 = 一次普通令牌热替换（随时可退——undo 走 store 快照）。
            if (migration.table) {
              saveTokenTable(migration.table);
              applyTokenTableToDOM(migration.table);
              setMigration({ ...migration, reason: `${migration.reason} · 已应用（回退走令牌页单项 ↺ 或域总检三步）` });
            }
          }}>应用迁移表</PButton>
        </Row>
      ) : null}
    </Card>
  );
}
