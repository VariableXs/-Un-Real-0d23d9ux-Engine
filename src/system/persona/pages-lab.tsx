/**
 * E 域页组⑥ · 深化实验室（批次四）：十一个引擎实验室面板。
 * 每个面板 = 引擎能力的可操作界面（预览即真话——跑的是引擎本体，非贴图演示）。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice, useT } from "./ui";
import { extractPalette, judgeWallpaperHarmony, deriveTokensFromPalette, nameColor, type PaletteResult } from "./palette-engine";
import { solarTimes, compareWithCooper, polarStrategy, planPreload, preloadFallback, dayOfYear } from "./wallpaper-solar";
import { packAtlas, planSwapBudget, cacheKey, invalidationPrefix, pickIconSize, mipChain, DEFAULT_ATLAS_OPTIONS, type AtlasEntry } from "./icon-atlas";
import { smoothDamp, transformHotspot, ClickMachine, snapToPhysicalPixel, type Vec2 } from "./cursor-physics";
import { synthesizeEvent, defaultSynthSpec, type SynthRender } from "./sound-synthesis";
import { clockFace, normalizeWeather, sanitizeSample, freshness, nextFetchAt, recordFetchFailure, recordFetchSuccess, type WidgetSourcePlan } from "./widget-data";
import { visibleLayers, wakeBudget, blurForWallpaper, layoutDigest, UnlockMachine, greetingFor, GREETING_TEXT, type NotificationGroup } from "./lockscreen-composer";
import { segmentSyllables, generateCandidates, compose, paginateCandidates, compositionFlags, numberKeyToCandidate, type CompositionState } from "./ime-composition";
import { flattenMenu, menuNavigate, auditMenuDepth, layoutTray, trayHitIndex, type MenuItem, type TrayIcon } from "./ctxnav";
import { ChordMatcher, normalizeEvent, auditAllBindings, SEQUENCE_TIMEOUT_MS, type ChordBinding } from "./chord-engine";
import { renderReport, diffRuns, detectFlaky, auditEvidence, buildEvidencePackage, verifyEvidencePackage, regressionMessage, type VerdictRun } from "./verdict-report";
import { generateJsonSchema, toCssVariables, fromCssVariables, scaleFontTable, verifyVariableContract, CSS_VAR_EXPECTED_COUNT } from "./tokens-schema";
import { defaultTokenTable, FONT_KEYS } from "./tokens";
import type { DomainVerdict, ItemVerdict } from "./verdict";

// ---------- F151/F154 · 调色板实验室 ----------

/** 确定性合成壁纸像素（mulberry32——同种子同调色板，预览即真话）。 */
function synthWallpaperPixels(w: number, h: number, seed: number): Uint8ClampedArray {
  let s = seed >>> 0;
  const rand = (): number => {
    s = (s + 0x6d2b79f5) | 0;
    let r = Math.imul(s ^ (s >>> 15), 1 | s);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
  const data = new Uint8ClampedArray(w * h * 4);
  const base = [40 + rand() * 80, 30 + rand() * 70, 60 + rand() * 90];
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const o = (y * w + x) * 4;
      const n = rand() * 40;
      data[o] = Math.min(255, base[0]! + n + x * 0.03);
      data[o + 1] = Math.min(255, base[1]! + n * 0.6);
      data[o + 2] = Math.min(255, base[2]! + n * 0.8);
      data[o + 3] = 255;
    }
  }
  return data;
}

export function PaletteLabCard(): React.ReactNode {
  const t = useT();
  const [seed, setSeed] = useState(42);
  const [palette, setPalette] = useState<PaletteResult | null>(null);
  const [dark, setDark] = useState(true);

  function extract(): void {
    const px = synthWallpaperPixels(192, 108, seed);
    setPalette(extractPalette(px, 192, 108, 6, 2));
  }
  const derivation = palette ? deriveTokensFromPalette(palette, dark) : null;
  const harmony = palette ? judgeWallpaperHarmony(palette.meanLuminance, dark) : null;

  return (
    <Card title={t("paletteTitle")}>
      <Row label={t("paletteExtract")} sub="确定性种子合成壁纸（同种子同调色板——预览即真话）· 中位切分量化">
        <PButton onClick={extract}>{t("run")}</PButton>
        <PButton kind="ghost" onClick={() => setSeed((s) => s + 1)}>换一张</PButton>
      </Row>
      {palette ? (
        <>
          <Row label={t("paletteSwatches")} sub={`平均亮度 ${(palette.meanLuminance * 100).toFixed(0)}% · 对比跨度 ${palette.contrastSpan.toFixed(1)}× · 提取耗时 ${palette.elapsedMs.toFixed(1)}ms`}>
            <span style={{ display: "flex", gap: 4 }}>
              {palette.swatches.slice(0, 6).map((s) => (
                <span key={s.hex} title={`${nameColor(s.hex)?.label ?? s.hex} · ${(s.population * 100).toFixed(0)}%`} style={{ width: 22, height: 22, borderRadius: 5, background: s.hex, display: "inline-block" }} />
              ))}
            </span>
          </Row>
          {harmony ? <Notice tone={harmony.harmonious ? "ok" : "warn"}>{harmony.message}</Notice> : null}
          <Row label={t("paletteDeriveDark")} sub="主色簇 → 9 键令牌底稿（派生不了的不硬造）">
            <span style={{ display: "flex", gap: 8 }}>
              <PButton onClick={() => setDark(true)}>深色</PButton>
              <PButton onClick={() => setDark(false)}>浅色</PButton>
            </span>
          </Row>
          {derivation ? (
            <Row label={t("paletteDerived")} sub={`源自第 ${derivation.sourceSwatch + 1} 簇 · 未派生 ${derivation.untouched} 键走默认（诚实覆盖度）`}>
              <span style={{ display: "flex", gap: 4 }}>
                {Object.values(derivation.colors).slice(0, 9).map((hex, i) => (
                  <span key={i} style={{ width: 16, height: 16, borderRadius: 4, background: hex ?? "#000", display: "inline-block", border: "1px solid rgba(140,140,160,0.3)" }} />
                ))}
              </span>
            </Row>
          ) : (
            <Notice tone="info">{t("paletteNoAccent")}</Notice>
          )}
        </>
      ) : null}
    </Card>
  );
}

// ---------- F153/F154 · 天文历实验室 ----------

export function SolarLabCard(): React.ReactNode {
  const t = useT();
  const date = useMemo(() => new Date(), []);
  const solar = useMemo(() => solarTimes(date, 39.9, 116.4, 480), [date]);
  const doy = dayOfYear(date);
  // Cooper 兜底对拍（autodark.sunTimesMinutes 同公式内联——对拍面不引依赖）。
  const cooper = useMemo(() => {
    const rad = Math.PI / 180;
    const decl = 23.45 * rad * Math.sin((2 * Math.PI * (284 + doy)) / 365);
    const cosH = -Math.tan(39.9 * rad) * Math.tan(decl);
    if (cosH > 1 || cosH < -1) return null;
    const half = (Math.acos(cosH) / rad / 15) * 60;
    return { sunrise: Math.round(720 - half), sunset: Math.round(720 + half) };
  }, [doy]);
  const cmp = solar && cooper ? compareWithCooper(solar, cooper) : null;
  const polar = polarStrategy(solar, 39.9);
  const plan = useMemo(() => planPreload({ idleWindows: [{ fromMin: 1320, toMin: 360 }], durationMin: 12, changeAtMin: 480, nowMin: 900 }), []);
  const fallback = preloadFallback(plan);

  const fmt = (m: number): string => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
  return (
    <Card title={t("solarTitle")}>
      {solar && cooper ? (
        <>
          <Row label="北京 39.9°N 116.4°E（东八区）" sub={`日出 ${fmt(solar.sunrise)} · 日落 ${fmt(solar.sunset)} · ${t("solarEqTime")} ${solar.eqTimeMin}min · ${t("solarDecl")} ${solar.declinationDeg}°`}>
            <span />
          </Row>
          {cmp ? (
            <Notice tone={cmp.withinFiveMin ? "ok" : "warn"}>
              {t("solarAccuracy")}：日出差 {cmp.sunriseDelta}min · 日落差 {cmp.sunsetDelta}min —— {cmp.withinFiveMin ? "±5 分钟判据裕量内" : "超差（需排查）"}
            </Notice>
          ) : null}
        </>
      ) : (
        <Notice tone="info">{polar ? polar.note : ""}</Notice>
      )}
      <Row label={t("preloadTitle")} sub={`空闲窗 22:00-06:00 ∩ (now, 08:00) · 时长 12min · ${plan ? `落点 ${fmt(plan.atMin)}（重试 ${fmt(plan.retryAtMin)}）` : "无窗"}`}>
        <span />
      </Row>
      <Notice tone={plan ? "ok" : "warn"}>{fallback.message}</Notice>
    </Card>
  );
}

// ---------- F155 · 图集装箱实验室 ----------

const SAMPLE_ICONS: AtlasEntry[] = Array.from({ length: 40 }, (_, i) => ({ id: `icon-${i + 1}`, w: 48 + (i % 3) * 16, h: 48 + ((i + 1) % 4) * 16 }));

export function AtlasLabCard(): React.ReactNode {
  const t = useT();
  const plan = useMemo(() => packAtlas(SAMPLE_ICONS, DEFAULT_ATLAS_OPTIONS), []);
  const budget = useMemo(() => planSwapBudget({ icons: SAMPLE_ICONS.length, cached: 32, reatlas: SAMPLE_ICONS.length - 32 }), []);
  return (
    <Card title={t("atlasTitle")}>
      <Row label={`${t("atlasPages")} ${plan.stats.pages} · 平均${t("atlasOccupancy")} ${(plan.stats.meanOccupancy * 100).toFixed(0)}%`} sub={`1024×1024 · ${plan.stats.entries}/${SAMPLE_ICONS.length} 入箱${plan.overflow.length > 0 ? ` · ${t("atlasOverflow")}: ${plan.overflow.join(",")}` : ""}`}>
        <span />
      </Row>
      <Row label={t("atlasSwapBudget")} sub={`缓存命中 ${budget.cacheHitMs}ms + 重排 ${budget.reatlasMs}ms + 提交 ${budget.commitMs}ms = ${budget.totalMs}ms`}>
        <span style={{ color: budget.withinBudget ? "var(--p-success)" : "var(--p-danger)" }}>{budget.withinBudget ? t("atlasWithinBudget") : "超预算——建议分批换"}</span>
      </Row>
      <Row label="缓存键契约" sub={`单图标失效 ${invalidationPrefix("pack-a", "icon-1")} → 整包失效 ${invalidationPrefix("pack-a")}`}>
        <code style={{ fontSize: 11 }}>{cacheKey({ packId: "pack-a", iconId: "icon-1", size: 48, dpr: 2 })}</code>
      </Row>
      <Row label="尺寸阶梯选档" sub={`display 20px @2x → ${pickIconSize(20, 2)}px · mip 链 128→16: ${mipChain(128).join("→")}`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F156 · 指针物理实验室 ----------

export function CursorPhysicsCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    // 平滑跟随仿真：目标阶跃 → 30 帧收敛轨迹。
    let pos: Vec2 = { x: 0, y: 0 };
    let vel: Vec2 = { x: 0, y: 0 };
    const target: Vec2 = { x: 200, y: 120 };
    const xs: number[] = [];
    for (let i = 0; i < 30; i++) {
      const r = smoothDamp(pos, target, vel, 0.08, 1 / 60);
      pos = r.pos;
      vel = r.velocity;
      xs.push(Math.round(pos.x));
    }
    return { xs, settled: Math.abs(pos.x - target.x) < 0.1 };
  }, []);
  const machine = useMemo(() => {
    const m = new ClickMachine();
    const t0 = 1000;
    const trace: string[] = [];
    trace.push(`press → ${m.feed({ type: "press", x: 0, y: 0, t: t0 }) ?? "state:pressed"}`);
    trace.push(`move 2px → ${m.feed({ type: "move", x: 2, y: 0, t: t0 + 50 }) ?? "抖动容忍"}`);
    trace.push(`move 10px → ${m.feed({ type: "move", x: 10, y: 0, t: t0 + 100 }) ?? ""}`);
    trace.push(`leave → ${m.feed({ type: "leave" }) ?? ""}`);
    trace.push(`re-press → click? ${m.feed({ type: "press", x: 0, y: 0, t: t0 + 300 }) ?? ""}`);
    const out = m.feed({ type: "release", x: 0, y: 0, t: t0 + 350 });
    trace.push(`release → ${out ?? "（无——移出已取消，不误触）"}`);
    return trace;
  }, []);
  return (
    <Card title={t("cursorPhysicsTitle")}>
      <Row label="SmoothDamp 收敛仿真（200px 阶跃 · 60fps · 30 帧）" sub={`末 5 帧 x = ${demo.xs.slice(-5).join(", ")} · ${demo.settled ? t("cursorSettled") : "未收敛"}`}>
        <span />
      </Row>
      <Row label={t("hotspotTransform")} sub="32px 光标 ×1.5 → 热点 (7,4) → (11,6)（半上取整——1px 级对拍口径）">
        <code style={{ fontSize: 11 }}>→ (${transformHotspot({ x: 7, y: 4 }, 1.5).x}, ${transformHotspot({ x: 7, y: 4 }, 1.5).y})</code>
      </Row>
      <Row label="DPR 吸附" sub="dpr=1.25 时 x=13.37 → " >
        <code style={{ fontSize: 11 }}>x=${snapToPhysicalPixel(13.37, 0, 1.25).x.toFixed(2)}</code>
      </Row>
      <Card title={t("clickMachine")}>
        {machine.map((line, i) => (
          <Row key={i} label={line} sub="">
            <span />
          </Row>
        ))}
      </Card>
    </Card>
  );
}

// ---------- F157 · 合成实验室 ----------

export function SynthLabCard(): React.ReactNode {
  const t = useT();
  const [eventId, setEventId] = useState("chime");
  const [result, setResult] = useState<{ stats: SynthRender; bytes: number } | null>(null);
  function render(): void {
    const r = synthesizeEvent(eventId, 80);
    if (r) setResult({ stats: r.stats, bytes: r.wav.byteLength });
    else setResult(null);
  }
  const spec = defaultSynthSpec(eventId);
  return (
    <Card title={t("synthLabTitle")}>
      <Row label="事件" sub="六事件确定性合成谱（同参数同波形——预览即真话）">
        <span style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
          {["notify", "chime", "error", "click", "plug", "unplug"].map((id) => (
            <PButton key={id} kind={eventId === id ? "primary" : "ghost"} onClick={() => setEventId(id)}>{id}</PButton>
          ))}
        </span>
      </Row>
      {spec ? (
        <Row label="谱面" sub={spec.layers.map((l) => `${l.wave}@${l.freqHz || "-"}Hz pan${l.pan} +${l.offsetMs}ms`).join(" · ")}>
          <PButton onClick={render}>{t("synthRender")}</PButton>
        </Row>
      ) : null}
      {result ? (
        <>
          <Row label={`${t("synthPeak")} ${(result.stats.peak * 100).toFixed(0)}% · ${t("synthRms")} ${(result.stats.rms * 100).toFixed(0)}%`} sub={`${t("synthDuration")} ${result.stats.durationMs.toFixed(0)}ms · ${t("synthClipped")} ${result.stats.clippedSamples}（soft clip 前置——削波即缺陷显性化）`}>
            <span />
          </Row>
          <Row label="WAV 产物" sub={`16-bit PCM stereo · ${(result.bytes / 1024).toFixed(1)} KB（落盘/告警共用格式——数据开放）`}>
            <span />
          </Row>
        </>
      ) : null}
    </Card>
  );
}

// ---------- F163 · 小组件数据引擎 ----------

export function WidgetDataCard(): React.ReactNode {
  const t = useT();
  const now = Date.now();
  const demo = useMemo(() => {
    const plan: WidgetSourcePlan = { sourceId: "weather", intervalMs: 1800_000, jitterMs: 15_000, failures: 0, lastOkAt: now - 900_000 };
    const jit = (): number => 0.5; // 确定性抖动（对拍口径）。
    const ok = recordFetchSuccess(plan, now);
    const fail = recordFetchFailure({ ...plan, failures: 0 }, now);
    return {
      clock: clockFace(new Date(now)),
      live: freshness(ok, now, (ms) => new Date(ms).toLocaleTimeString()),
      stale: freshness({ ...plan, lastOkAt: now - plan.intervalMs * 5 }, now, (ms) => new Date(ms).toLocaleTimeString()),
      nextOk: nextFetchAt(ok, now, jit),
      nextFail: nextFetchAt(fail, now, jit),
      weather: normalizeWeather({ source: "demo", raw: { temp: 75, unit: "F", feels: null, humidity: 55, wind: 12, condition: "Sunny" } }, now),
      bad: normalizeWeather({ source: "demo", raw: { temp: 999, humidity: 300, wind: "fast", condition: "volcano" } }, now),
      sample: sanitizeSample({ cpuPct: 133.7, memPct: -4, diskPct: 61.2, netKbps: Number.NaN }),
    };
  }, [now]);
  const fmt = (m: number): string => new Date(m).toLocaleTimeString();
  return (
    <Card title={t("widgetDataTitle")}>
      <Row label={`时钟 · ${demo.clock.isoDate} ${String(demo.clock.h).padStart(2, "0")}:${String(demo.clock.m).padStart(2, "0")}:${String(demo.clock.s).padStart(2, "0")}`} sub={`月历：${demo.clock.daysInMonth} 天 · 首行空 ${demo.clock.firstWeekday} 格 · ${demo.clock.meridiem}`}>
        <span />
      </Row>
      <Row label={`${t("freshnessLive")}（${demo.live.label}）· 过期 5×（${demo.stale.label}）`} sub="live <2× 间隔 → stale → frozen >10×（人话标注不装新）">
        <span />
      </Row>
      <Row label={`${t("nextFetch")}（成功后）`} sub={`${fmt(demo.nextOk)} · 失败一次后 ${fmt(demo.nextFail)}（2× 退避 + 抖动——${t("jitterNote")}）`}>
        <span />
      </Row>
      <Row label="天气归一（华氏→摄氏 · 字段清洗）" sub={demo.weather.ok ? `${demo.weather.data.tempC}℃（体感 ${demo.weather.data.feelsC}℃）· 湿度 ${demo.weather.data.humidity}% · ${demo.weather.data.condition}` : "解析失败"}>
        <span />
      </Row>
      <Row label="脏数据拒绝" sub={demo.bad.ok ? "（不应出现）" : demo.bad.reasons.join(" · ")}>
        <span />
      </Row>
      <Row label="系统采样清洗" sub={`cpu 133.7→${demo.sample.clean.cpuPct} · mem -4→${demo.sample.clean.memPct} · net NaN→${demo.sample.clean.netKbps} · 修正 ${demo.sample.fixed.length} 处`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F164 · 锁屏合成实验室 ----------

export function LockComposerCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const budget = wakeBudget();
    const groups: NotificationGroup[] = [{ appId: "mail", icon: "mail", count: 3 }, { appId: "chat", icon: "chat", count: 5 }, { appId: "store", icon: "store", count: 2 }, { appId: "news", icon: "news", count: 1 }, { appId: "extra", icon: "x", count: 7 }];
    const digest = layoutDigest(groups);
    const machine = new UnlockMachine();
    const trace: string[] = [];
    let now = 1000;
    for (let i = 0; i < 5; i++) {
      const r = machine.submit(false, now);
      trace.push(r.type === "rejected" ? `错 ${5 - (r as { remaining: number }).remaining} 次 → 剩 ${r.remaining}` : r.type === "cooldown-started" ? `第 5 错 → 冷却 ${(r.durationMs / 1000) | 0}s` : r.type);
      now += 100;
    }
    const during = machine.submit(true, now); // 冷却期内再试（即使输对也先等冷却——语义一致）。
    return { budget, digest, trace, during: during.type, blur: blurForWallpaper(0.62), layers2s: visibleLayers(2000).length };
  }, []);
  const hr = new Date().getHours();
  const g = GREETING_TEXT[greetingFor(hr)];
  return (
    <Card title={t("lockComposerTitle")}>
      <Row label={`${g.zh} / ${g.en}（${hr} 点）`} sub={`层栈 6 层 · 2s 后可见 ${demo.layers2s}/6 · 第一拍 ${demo.budget.firstBeatMs}ms（${demo.budget.withinBudget ? "≤2s 预算内" : "超预算"}）`}>
        <span />
      </Row>
      <Row label={t("blurSpec")} sub={`亮壁纸（亮度 62%）→ 压暗 ${(100 - demo.blur.brightness * 100).toFixed(0)}% · 模糊 ${demo.blur.radiusPx}px · 饱和 ×${demo.blur.saturate}`}>
        <span />
      </Row>
      <Row label="通知摘要（只计数不显内容）" sub={`前 4 组（${demo.digest.shown.map((g2) => `${g2.icon}×${g2.count}`).join(" ")}）+ 折叠 ${demo.digest.overflowCount} · 总 ${demo.digest.total}`}>
        <span />
      </Row>
      <Card title={t("unlockMachine")}>
        {demo.trace.map((line, i) => (
          <Row key={i} label={line} sub="">
            <span />
          </Row>
        ))}
        <Row label="冷却期内再试" sub={demo.during === "cooldown-active" ? "拒绝并报剩余——不延长冷却（惩罚与提示分离）" : demo.during}>
          <span />
        </Row>
      </Card>
    </Card>
  );
}

// ---------- F166 · 拼音组合实验室 ----------

export function ImeLabCard(): React.ReactNode {
  const t = useT();
  const [state, setState] = useState<CompositionState>({ raw: "nihao", caret: 5 });
  const [page, setPage] = useState(0);
  const candidates = useMemo(() => generateCandidates(state.raw), [state.raw]);
  const paged = useMemo(() => paginateCandidates(candidates, page), [candidates, page]);
  const segs = useMemo(() => segmentSyllables(state.raw), [state.raw]);
  const flags = compositionFlags(state, candidates);
  return (
    <Card title={t("imeLabTitle")}>
      <Row label={`组合串「${state.raw || "（空）"}」`} sub={`${t("imeSegments")}：${segs.map((s) => s.join("'")).join(" | ")} · ${flags.composing ? t("imeComposing") : "非组合期"}`}>
        <span style={{ display: "flex", gap: 4 }}>
          <PButton onClick={() => setState((s) => compose(s, { type: "input", ch: "a" }))}>+a</PButton>
          <PButton kind="ghost" onClick={() => setState((s) => compose(s, { type: "backspace" }))}>⌫</PButton>
          <PButton kind="ghost" onClick={() => setState((s) => compose(s, { type: "clear" }))}>清空</PButton>
        </span>
      </Row>
      <Row label={t("imeCandidatesPage").replace("{p}", String(paged.page + 1)).replace("{n}", String(paged.pageCount))} sub={paged.items.map((c, i) => `${paged.numberHints[i]}.${c.text}`).join(" ") || "（无候选）"}>
        <span style={{ display: "flex", gap: 4 }}>
          <PButton kind="ghost" onClick={() => setPage((p) => Math.max(0, p - 1))}>‹</PButton>
          <PButton kind="ghost" onClick={() => setPage((p) => Math.min(paged.pageCount - 1, p + 1))}>›</PButton>
        </span>
      </Row>
      <Row label="数字键直选" sub={`2 号候选序号 = ${numberKeyToCandidate(paged.page, 2, candidates.length) ?? "越界（null——显性化）"} · 候选回指切分「${paged.items[0]?.syllables.join("'") ?? "-"}」`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F167/F168 · 导航实验室 ----------

const DEMO_MENU: MenuItem[] = [
  { id: "open", label: "打开", accelerator: "O" },
  { id: "sep-1", label: "-", disabled: true },
  { id: "rename", label: "重命名", accelerator: "R", children: [{ id: "rename-f2", label: "F2 就地改名" }, { id: "rename-adv", label: "高级重命名", disabled: true }] },
  { id: "delete", label: "删除", accelerator: "D", danger: true, locked: true },
];

export function CtxNavCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const flat = flattenMenu(DEMO_MENU);
    const nav: string[] = [];
    let focus: string[] | null = null;
    const step = (a: Parameters<typeof menuNavigate>[2]): void => {
      const r = menuNavigate(DEMO_MENU, focus, a);
      if (r.type === "focus" || r.type === "submenu-open") {
        focus = r.path;
        nav.push(`${a.type} → ${r.path.join(">")}`);
      } else nav.push(`${a.type} → ${r.type}`);
    };
    step({ type: "home" });
    step({ type: "down" });
    step({ type: "down" }); // 跳过禁用分隔线。
    step({ type: "type-ahead", ch: "d" }); // 字母跳转。
    step({ type: "enter" }); // 锁定项 → ignored（防自残）。
    const deep = auditMenuDepth([{ id: "a", label: "A", children: [{ id: "b", label: "B", children: [{ id: "c", label: "C" }] }] }]);
    const icons: TrayIcon[] = Array.from({ length: 9 }, (_, i) => ({ id: `tray-${i}`, w: 32, lastUsedAt: 1000 - i, pinned: i === 0 }));
    const tray = layoutTray(icons, 260);
    return { nav, flatCount: flat.length, deep, tray, hit: trayHitIndex(75, tray.visible.length) };
  }, []);
  return (
    <Card title={t("ctxNavTitle")}>
      {demo.nav.map((line, i) => (
        <Row key={i} label={line} sub="">
          <span />
        </Row>
      ))}
      <Row label={t("ctxTypeAhead")} sub={`锁定项 Enter = 忽略（防自残语义一致）· ${t("trayOverflowTitle")}: 可见 ${demo.tray.visible.length} / 溢出 ${demo.tray.overflow.length}（面板 ${demo.tray.panel.cols}×${demo.tray.panel.rows}）· 热区 75px → 第 ${demo.hit ?? "-"} 图标`}>
        <span />
      </Row>
      {demo.tray.pinnedOverflow.length > 0 ? <Notice tone="danger">{`${t("trayPinnedOverflow")}：${demo.tray.pinnedOverflow.join(",")}`}</Notice> : null}
      {demo.deep.length > 0 ? <Notice tone="warn">{`F215 层级审计：${demo.deep.length} 条超两级路径`}</Notice> : null}
    </Card>
  );
}

// ---------- F169 · 序列匹配实验室 ----------

const DEMO_BINDINGS: ChordBinding[] = [
  { actionId: "copy-line-down", sequence: ["Ctrl+K", "Ctrl+C"], enabled: true },
  { actionId: "comment", sequence: ["Ctrl+K", "Ctrl+U"], enabled: true },
  { actionId: "save", sequence: ["Ctrl+S"], enabled: true },
  { actionId: "save-all", sequence: ["Ctrl+S"], enabled: true }, // 故意冲突——对拍要抓到。
  { actionId: "quit", sequence: ["Ctrl+Q"], enabled: false },
];

export function ChordLabCard(): React.ReactNode {
  const t = useT();
  const [log, setLog] = useState<string[]>([]);
  const [matcher] = useState(() => new ChordMatcher(DEMO_BINDINGS));
  function send(key: string, ctrl: boolean): void {
    const ev = { key, ctrl, alt: false, shift: false, meta: false };
    const r = matcher.feed(ev, Date.now());
    setLog((l) => [`${normalizeEvent(ev)} → ${r.type}${r.type === "match" ? `: ${r.actionId}` : r.type === "pending" ? `（${t("chordPending")}）` : r.type === "conflict" ? `（冲突 ${r.actionIds.join(",")}）` : ""}`, ...l].slice(0, 6));
  }
  const audit = useMemo(() => auditAllBindings(DEMO_BINDINGS), []);
  return (
    <Card title={t("chordLabTitle")}>
      <Row label="两段序列（VSCode 式）" sub={`超时 ${SEQUENCE_TIMEOUT_MS}ms 作废 · Esc 清缓冲 · IME 组合期不匹配`}>
        <span style={{ display: "flex", gap: 4 }}>
          <PButton onClick={() => send("k", true)}>Ctrl+K</PButton>
          <PButton onClick={() => send("c", true)}>Ctrl+C</PButton>
          <PButton onClick={() => send("u", true)}>Ctrl+U</PButton>
          <PButton kind="ghost" onClick={() => send("s", true)}>Ctrl+S</PButton>
        </span>
      </Row>
      {log.map((line, i) => (
        <Row key={i} label={line} sub="">
          <span />
        </Row>
      ))}
      <Row label={t("chordConsistency")} sub={`表 ${audit.total} 条（启用 ${audit.checks.length}）· 一致 ${audit.consistent} · 冲突抓捕：${audit.checks.filter((c) => !c.consistent).map((c) => c.reason).join("；") || "无"}`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F170 · 报告与证据包实验室 ----------

function fakeRun(runId: string, failIds: string[], start: number): VerdictRun {
  const items: ItemVerdict[] = Array.from({ length: 19 }, (_, i) => {
    const id = `F${151 + i}`;
    const pass = !failIds.includes(id);
    return {
      id,
      name: `项 ${id}`,
      probe: "三步验收",
      steps: [
        { step: "mutate", ok: pass, detail: pass ? "写入新值成功" : "写入后值未变化", evidence: "hash" },
        { step: "take-effect", ok: pass, detail: pass ? "生效确认" : "生效观测失败", evidence: "hash" },
        { step: "rollback", ok: pass, detail: pass ? "逐令牌回滚零残留" : "回退哈希不一致", evidence: "hash" },
      ],
      pass,
    };
  });
  const verdict: DomainVerdict = { items, passed: items.filter((i) => i.pass).length, total: items.length, allGreen: failIds.length === 0, elapsedMinutes: 0.1, withinBudget: true, evidenceComplete: true };
  return { runId, startedAt: start, finishedAt: start + 6000, verdict };
}

export function ReportLabCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const now = Date.now();
    const runA = fakeRun("run-a", ["F154", "F165"], now - 7 * 24 * 3600 * 1000);
    const runB = fakeRun("run-b", ["F154"], now);
    const diff = diffRuns(runA, runB);
    const flaky = detectFlaky([runA, runB, fakeRun("run-c", ["F165"], now)], "F165");
    const staleRun = fakeRun("run-old", [], now - 9 * 24 * 3600 * 1000);
    const audit = auditEvidence(staleRun, now);
    const pkg = buildEvidencePackage(runB);
    const verified = verifyEvidencePackage(pkg);
    const tampered = verifyEvidencePackage({ ...pkg, items: [{ ...pkg.items[0]!, pass: false }] });
    const report = renderReport(runB);
    const contract = verifyVariableContract(defaultTokenTable());
    return { diff, flaky, audit, verified, tampered, report, contract };
  }, []);
  return (
    <Card title={t("reportLabTitle")}>
      <Row label={t("reportRegression")} sub={regressionMessage(demo.diff)}>
        <span style={{ color: demo.diff.regressed.length > 0 ? "var(--p-danger)" : "var(--p-success)" }}>{`修复 ${demo.diff.fixed.length} · 回归 ${demo.diff.regressed.length} · 持续红 ${demo.diff.persistentlyFailing.length}`}</span>
      </Row>
      <Row label={t("reportFlaky")} sub={demo.flaky.message}>
        <span style={{ color: demo.flaky.flaky ? "var(--p-warn)" : "var(--p-success)" }}>{demo.flaky.flaky ? "flaky" : "稳定"}</span>
      </Row>
      <Row label={t("reportEvidenceAudit")} sub={demo.audit.complete ? "证据包完整" : demo.audit.gaps.map((g) => g.human).join("；")}>
        <span style={{ color: demo.audit.complete ? "var(--p-success)" : "var(--p-warn)" }}>{demo.audit.complete ? "100%" : "有缺口"}</span>
      </Row>
      <Row label="证据包验真" sub={`合法包 ${demo.verified.ok ? "通过" : "拒绝"} · 篡改包 ${demo.tampered.ok ? "通过（不应发生！）" : `拒绝：${demo.tampered.reason}`}`}>
        <span />
      </Row>
      <Row label="Markdown 报告" sub={`${demo.report.markdown.length} 字符 · ${demo.report.passCount}/${demo.report.passCount + demo.report.failCount} 绿 · 用时 ${(demo.report.durationMs / 1000).toFixed(1)}s`}>
        <PButton onClick={() => navigator.clipboard?.writeText(demo.report.markdown)}>{t("copy")}</PButton>
      </Row>
    </Card>
  );
}

// ---------- F151 · Schema 与变量桥实验室 ----------

export function SchemaLabCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const { schema } = generateJsonSchema();
    const base = defaultTokenTable();
    const vars = toCssVariables(base);
    const back = fromCssVariables({ ...vars, "--p-radius-card": "bad", "--p-unknown-x": "1" });
    const scaled = scaleFontTable(base, 1.25);
    return {
      schemaBytes: JSON.stringify(schema).length,
      varCount: Object.keys(vars).length,
      expected: CSS_VAR_EXPECTED_COUNT,
      roundTripKeys: Object.keys(back.patch.colors ?? {}).length,
      rejected: back.rejected.map((r) => `${r.key}: ${r.reason}`),
      scaled: FONT_KEYS.map((f) => `${f.slot} ${base.font[f.slot]}→${scaled.scaled[f.slot]}`),
      clamped: scaled.clamped.length,
      contract: verifyVariableContract(base),
    };
  }, []);
  return (
    <Card title="令牌 Schema 与变量桥">
      <Row label={`JSON Schema 生成（${demo.schemaBytes} 字节 · Draft-07）`} sub="第三方作者可用任意校验器预检主题包——错误前置到提交前（开放性十四章）">
        <span />
      </Row>
      <Row label={`CSS 变量桥：${demo.varCount}/${demo.expected} 变量（契约${demo.contract.ok ? "达成" : "违约"}）`} sub={`往返回收 ${demo.roundTripKeys} 色 · 非法/未知键 ${demo.rejected.length} 条逐条拒绝：${demo.rejected.join("；") || "无"}`}>
        <span />
      </Row>
      <Row label={`无障碍字号缩放 1.25×：${demo.scaled.join(" · ")}`} sub={demo.clamped > 0 ? `${demo.clamped} 档触底钳制（≥9px 可读性底线）` : "无钳制——层级比例完整保留"}>
        <span />
      </Row>
      <Notice tone="info">{t("tokensHint")}</Notice>
    </Card>
  );
}
