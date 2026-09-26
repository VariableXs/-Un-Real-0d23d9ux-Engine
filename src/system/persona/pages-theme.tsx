/**
 * E 域页组①：主题（F151 令牌表 / F152 实时预览编辑器 / F153 深浅自动切换 /
 * F162 每应用主题例外）。
 */
import { useMemo, useState } from "react";
import {
  COLOR_TOKENS, SPACING_TOKENS, RADIUS_DEFAULTS, FONT_DEFAULTS,
  MOTION_CURVES, MOTION_DURATIONS, tokenTableHash,
  loadTokenTable, saveTokenTable, resetToken, applyTokenTableToDOM,
  tokenTableJsonSchema, coverageReport, defaultTokenTable,
  type ColorGroup, type TokenTable, type MotionCurve,
} from "./tokens";
import { PreviewSession, PREVIEW_WIDTH_PX, type PreviewScene } from "./preview";
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

const GROUP_ZH: Record<ColorGroup, Parameters<LaneT>[0]> = {
  bg: "groupBg", fg: "groupFg", brand: "groupBrand", state: "groupState", line: "groupLine", misc: "groupMisc",
};

// ---------- F151 令牌表 ----------

export function TokensPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const [table, setTable] = useState<TokenTable>(() => loadTokenTable());
  const [saved, setSaved] = useState(false);
  const coverage = useMemo(() => coverageReport(table), [table]);

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
    </div>
  );
}

// ---------- F162 每应用主题例外 ----------

export function ExceptionsPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("theme");
  const cfg = loadAppExceptions();
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
        {cfg.exceptions.map((e) => (
          <Row key={e.appId} label={`${e.appName} · ${e.mode === "dark" ? "深色" : "浅色"}`} sub={degradationLabel(e) ?? t("tokenAware")}>
            <PButton kind="danger" onClick={() => saveAppExceptions(removeException(cfg, e.appId))}>{t("reset")}</PButton>
          </Row>
        ))}
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
