/**
 * AURORA-10000 · AI-11~AI-15 车道 · 副作用 runner（activate.ts 装载）。
 * - HealthRunner 族0065：真实定时提醒（20-20-20/久坐/饮水），经 uiStore.pushToast
 *   落成桌面可见提醒；触发记录本地 localStorage，数据不出本机（F01623）。
 * - RitualRunner 族0075：仪式到期 → 派发 aurora-w2:ritual 事件（RitualOverlay 接）。
 * - FestivalLayer 族0073：命中节日 → 常驻装饰粒子 canvas。
 * - CursorLayer 族0068：拖尾/点击涟漪/找回闪烁。
 * 全部幂等可停；reduced-motion 下装饰层自动停。
 */
import { pushToast } from "../../state/uiStore";
import { activeEntries, designStore } from "./state";
import { dueHealthRules, festivalOn, inQuietWindow, type FestivalKey, type HealthRuleDef } from "./logic";
import { ParticleEngine, type ParticleKind } from "./particles";
import { fireRitual } from "./ritualBus";

const HEALTH_LOG_KEY = "variable:aurora:w2:health";
const RITUAL_DAY_KEY = "variable:aurora:w2:ritual-day";

type StopFn = () => void;
const stops: StopFn[] = [];

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* ignore */
  }
}

/* ---------------- 族0065 健康提醒 ---------------- */

export function startHealthRunner(): StopFn {
  const timer = window.setInterval(() => {
    const entries = activeEntries(designStore.getState());
    const rules: HealthRuleDef[] = [];
    for (const ent of entries) {
      if (ent.family !== "f0065") continue;
      const every = ent.params?.everyMin;
      if (typeof every === "number") rules.push({ entryId: ent.id, everyMin: every });
    }
    if (rules.length === 0) return;
    const log = readJson<Record<string, number>>(HEALTH_LOG_KEY, {});
    const due = dueHealthRules(rules, log, Date.now());
    for (const d of due) {
      const zh = d.entryId === "F01601" ? "护眼远眺：看看 6 米外 20 秒"
        : d.entryId === "F01603" ? "久坐提醒：起来站一会儿"
        : d.entryId === "F01604" ? "喝水节奏：喝一杯水"
        : "健康提醒";
      const en = d.entryId === "F01601" ? "20-20-20: look 6m away for 20s"
        : d.entryId === "F01603" ? "Stand up and stretch"
        : d.entryId === "F01604" ? "Time for a glass of water"
        : "Health reminder";
      const lang = document.documentElement.lang === "en" ? "en" : "zh";
      pushToast("info", lang === "en" ? "Desktop health" : "桌面健康", lang === "en" ? en : zh);
      log[d.entryId] = Date.now();
    }
    if (due.length > 0) writeJson(HEALTH_LOG_KEY, log);
  }, 30_000);
  return () => window.clearInterval(timer);
}

/* ---------------- 族0075 仪式触发 ---------------- */

interface RitualRule {
  entryId: string;
  kind: "daily-hour" | "weekday-hour" | "quiet-window";
  hour?: number;
  weekday?: number;
  from?: number;
  to?: number;
}

export function startRitualRunner(): StopFn {
  const fire = (): void => {
    const entries = activeEntries(designStore.getState()).filter((e) => e.family === "f0075");
    if (entries.length === 0) return;
    const now = new Date();
    const dayKey = `${now.getFullYear()}-${now.getMonth()}-${now.getDate()}`;
    const fired = readJson<Record<string, string>>(RITUAL_DAY_KEY, {});
    const rules: RitualRule[] = [];
    for (const ent of entries) {
      const p = ent.params ?? {};
      if (typeof p.hour === "number" && typeof p.weekday === "number") {
        rules.push({ entryId: ent.id, kind: "weekday-hour", hour: p.hour, weekday: p.weekday });
      } else if (typeof p.hour === "number") {
        rules.push({ entryId: ent.id, kind: "daily-hour", hour: p.hour });
      } else if (typeof p.from === "number" && typeof p.to === "number" && ent.id !== "F01872") {
        rules.push({ entryId: ent.id, kind: "quiet-window", from: p.from, to: p.to });
      }
    }
    for (const r of rules) {
      if (fired[r.entryId] === dayKey) continue;
      const hit =
        (r.kind === "daily-hour" && now.getHours() === r.hour) ||
        (r.kind === "weekday-hour" && now.getDay() === r.weekday && now.getHours() === r.hour) ||
        (r.kind === "quiet-window" && inQuietWindow(now.getHours(), r.from!, r.to!));
      if (hit) {
        fired[r.entryId] = dayKey;
        fireRitual(r.entryId);
      }
    }
    writeJson(RITUAL_DAY_KEY, fired);
  };
  const timer = window.setInterval(fire, 60_000);
  return () => window.clearInterval(timer);
}

/* ---------------- 族0073 节令装饰层 ---------------- */

const FX_MAP: Record<string, ParticleKind> = {
  lantern: "lantern",
  rain: "rain",
  stars: "stars",
  leaves: "leaves",
  snow: "snow",
  petals: "petals",
  sakura: "sakura",
  meteor: "meteor",
  embers: "embers",
};

/** 由启用的节令行 + 今天日期决定粒子类型。 */
export function festivalFxFor(entries: Array<{ family: string; id: string; params?: Record<string, unknown>; }>, date: Date): { kind: ParticleKind; festival: FestivalKey | null } {
  const hits = festivalOn(date);
  if (hits.length === 0) return { kind: "none", festival: null };
  const enabledFx: string[] = [];
  for (const ent of entries) {
    if (ent.family !== "f0073") continue;
    const fx = ent.params?.fx;
    if (typeof fx === "string") enabledFx.push(fx);
  }
  const priority: FestivalKey[] = ["cny", "chuxi", "lantern-fest", "mid-autumn", "qixi", "duanwu", "qingming", "chongyang", "dongzhi", "laba", "christmas", "halloween", "valentine", "newyear", "sakura", "maple"];
  for (const f of priority) {
    if (!hits.includes(f)) continue;
    const keyMap: Partial<Record<FestivalKey, string>> = {
      cny: "lantern", "lantern-fest": "lantern", midautumn: "stars", qixi: "stars",
      duanwu: "rain", qingming: "rain", chongyang: "leaves", dongzhi: "snow",
      laba: "snow", chuxi: "embers", christmas: "snow", halloween: "embers",
      valentine: "petals", newyear: "confetti", sakura: "sakura", maple: "leaves",
    } as Partial<Record<FestivalKey, string>>;
    const fxKey = keyMap[f];
    if (fxKey && enabledFx.includes(fxKey)) {
      return { kind: FX_MAP[fxKey] ?? "none", festival: f };
    }
  }
  return { kind: "none", festival: hits[0] ?? null };
}

export function startFestivalLayer(reducedMotion: boolean): StopFn {
  if (reducedMotion) return () => undefined;
  const host = document.createElement("canvas");
  host.setAttribute("aria-hidden", "true");
  host.style.cssText = "position:fixed;inset:0;width:100vw;height:100vh;pointer-events:none;z-index:5;";
  host.dataset.auroraW2 = "festival";
  document.body.appendChild(host);
  const engine = new ParticleEngine({ maxFps: 30 });
  engine.attach(host, "none");
  const refresh = (): void => {
    const entries = activeEntries(designStore.getState());
    const { kind } = festivalFxFor(entries, new Date());
    engine.setKind(kind);
    if (kind !== "none") engine.start();
  };
  refresh();
  const unsub = designStore.subscribe(refresh);
  const onVis = (): void => {
    if (document.hidden) engine.stop();
    else engine.start();
  };
  document.addEventListener("visibilitychange", onVis);
  const onResize = (): void => engine.resize();
  window.addEventListener("resize", onResize);
  return () => {
    unsub();
    document.removeEventListener("visibilitychange", onVis);
    window.removeEventListener("resize", onResize);
    engine.destroy();
    host.remove();
  };
}

/* ---------------- 族0068 光标层 ---------------- */

export function startCursorLayer(reducedMotion: boolean): StopFn {
  const trail = document.createElement("div");
  trail.dataset.auroraW2 = "cursor";
  trail.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483000;";
  document.body.appendChild(trail);

  let trailDots = 0;
  let rippleMs = 0;
  let findOn = false;
  const dots: HTMLDivElement[] = [];
  let idx = 0;

  const refresh = (): void => {
    const entries = activeEntries(designStore.getState());
    const trailEntry = entries.find((e) => e.id === "F01680");
    trailDots = reducedMotion ? 0 : typeof trailEntry?.params?.dots === "number" ? trailEntry.params.dots : 0;
    const ripple = entries.find((e) => e.id === "F01681");
    rippleMs = reducedMotion ? 0 : ripple ? 360 : 0;
    findOn = entries.some((e) => e.id === "F01698") && !reducedMotion;
  };
  refresh();
  const unsub = designStore.subscribe(refresh);

  let lastMove = 0;
  const onMove = (e: PointerEvent): void => {
    const now = performance.now();
    if (now - lastMove < 16 || trailDots === 0) return;
    lastMove = now;
    let dot = dots[idx];
    if (!dot) {
      dot = document.createElement("div");
      dot.style.cssText = "position:absolute;width:6px;height:6px;border-radius:50%;background:var(--accent, oklch(0.68 0.09 262));transform:translate(-50%,-50%);transition:opacity .4s linear;";
      trail.appendChild(dot);
      dots[idx] = dot;
    }
    dot.style.left = `${e.clientX}px`;
    dot.style.top = `${e.clientY}px`;
    dot.style.opacity = "0.7";
    const d = dot;
    window.setTimeout(() => { d.style.opacity = "0"; }, 60);
    idx = (idx + 1) % Math.max(1, trailDots);
  };

  const rippleAt = (x: number, y: number): void => {
    if (rippleMs === 0) return;
    const el = document.createElement("div");
    el.style.cssText = `position:absolute;width:28px;height:28px;border-radius:50%;border:2px solid var(--accent, oklch(0.68 0.09 262));transform:translate(-50%,-50%);opacity:0.8;animation:w2-ripple ${rippleMs}ms ease-out forwards;`;
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
    trail.appendChild(el);
    window.setTimeout(() => el.remove(), rippleMs + 50);
  };
  const onDown = (e: PointerEvent): void => rippleAt(e.clientX, e.clientY);
  const flash = (): void => {
    if (!findOn) return;
    rippleAt(window.innerWidth - 60, 60);
    rippleAt(60, 60);
  };

  window.addEventListener("pointermove", onMove, { passive: true });
  window.addEventListener("pointerdown", onDown, { passive: true });
  window.addEventListener("keydown", flash, { passive: true });
  // 涟漪关键帧（本车道自注，不碰全局样式表）
  const style = document.createElement("style");
  style.textContent = "@keyframes w2-ripple{from{width:12px;height:12px;opacity:.8}to{width:56px;height:56px;opacity:0}}";
  document.head.appendChild(style);
  return () => {
    unsub();
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerdown", onDown);
    window.removeEventListener("keydown", flash);
    style.remove();
    trail.remove();
  };
}

/** 装载全部 runner（幂等由调用方保证单次）。 */
export function startAllRunners(): void {
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  stops.push(startHealthRunner());
  stops.push(startRitualRunner());
  stops.push(startFestivalLayer(reduced));
  stops.push(startCursorLayer(reduced));
}

export function stopAllRunners(): void {
  for (const s of stops.splice(0)) s();
}
