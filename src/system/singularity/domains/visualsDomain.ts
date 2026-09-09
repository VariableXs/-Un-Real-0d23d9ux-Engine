/**
 * SINGULARITY-100 · 域12 视觉、个性化与氛围（Q-77…Q-81）行为层。
 *
 * 边界（全景 §12）：U-46/M-65/N-08/Z-68/U-08 管既有色彩与材质能力；
 * 本域是季节换装、壁纸视差、精品店、材质滑块与过渡仪式。
 * 全部通过 CSS 变量与 ai04 事件驱动，零改写既有组件。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  clamp,
  domReady,
  emitSingu,
  makeEl,
  on,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuNum } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-77：日期 → 季节（3-5 春 / 6-8 夏 / 9-11 秋 / 12-2 冬）。 */
export function seasonOf(date: Date): "spring" | "summer" | "autumn" | "winter" {
  const m = date.getMonth() + 1;
  if (m >= 3 && m <= 5) return "spring";
  if (m >= 6 && m <= 8) return "summer";
  if (m >= 9 && m <= 11) return "autumn";
  return "winter";
}

/** Q-77：季节 → 强调色相（春樱粉 / 夏晴蓝 / 秋枫暖 / 冬霜冷）。 */
export const SEASON_HUE: Record<"spring" | "summer" | "autumn" | "winter", number> = {
  spring: 340,
  summer: 205,
  autumn: 28,
  winter: 195,
};

/** Q-78：鼠标偏移 → 三层视差位移（前景 1× / 本体 0.4× / 远景 0.15×）。 */
export function parallaxShift(nx: number, ny: number, amountPct: number): {
  fg: [number, number];
  base: [number, number];
  far: [number, number];
} {
  const a = amountPct / 100;
  const dx = nx * a;
  const dy = ny * a;
  return {
    fg: [dx, dy],
    base: [dx * 0.4, dy * 0.4],
    far: [dx * 0.15, dy * 0.15],
  };
}

/** Q-80：材质四滑块 → CSS 变量补丁。 */
export function materialVars(fog: number, refraction: number, grain: number, gloss: number): Record<string, string> {
  return {
    "--singu-mat-fog": String(clamp(fog, 0, 100) / 100),
    "--singu-mat-refraction": String(clamp(refraction, 0, 100) / 100),
    "--singu-mat-grain": String(clamp(grain, 0, 100) / 100),
    "--singu-mat-gloss": String(clamp(gloss, 0, 100) / 100),
  };
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-77 季节换装（自动按日期；锁定后永不变）----
function mountSeasonal(): void {
  const apply = (season: "spring" | "summer" | "autumn" | "winter" | "auto"): void => {
    const locked = singuStoreRead<string | null>("season-lock", null);
    const s = (locked ?? (season === "auto" ? seasonOf(new Date()) : season)) as
      | "spring"
      | "summer"
      | "autumn"
      | "winter";
    document.documentElement.style.setProperty("--singu-season-hue", String(SEASON_HUE[s]));
    document.documentElement.dataset.singuSeason = s;
    emitSingu("season", s);
  };
  bag.add(
    on(window, "singu:season-lock", (e: Event) => {
      const season = (e as CustomEvent).detail?.season as string | undefined;
      singuStoreWrite("season-lock", season ?? null);
      apply((season ?? "auto") as "spring");
    }),
  );
  apply("auto");
  bag.add(() => {
    document.documentElement.style.removeProperty("--singu-season-hue");
    delete document.documentElement.dataset.singuSeason;
  });
}

// ---- Q-78 壁纸视差（鼠标驱动三层景深；reduce-motion 归零）----
function mountParallax(): void {
  let raf = 0;
  const handler = (e: Event): void => {
    if (!ctxRef?.on("Q-78") || !ctxRef.motionOK()) return;
    const me = e as MouseEvent;
    if (raf) return; // 帧合并
    raf = requestAnimationFrame(() => {
      raf = 0;
      const amount = singuNum("Q-78", "parallax");
      const nx = (me.clientX / window.innerWidth) * 2 - 1;
      const ny = (me.clientY / window.innerHeight) * 2 - 1;
      const { fg, base, far } = parallaxShift(nx, ny, amount);
      const root = document.documentElement.style;
      root.setProperty("--singu-plx-fg-x", `${fg[0].toFixed(2)}px`);
      root.setProperty("--singu-plx-fg-y", `${fg[1].toFixed(2)}px`);
      root.setProperty("--singu-plx-base-x", `${base[0].toFixed(2)}px`);
      root.setProperty("--singu-plx-base-y", `${base[1].toFixed(2)}px`);
      root.setProperty("--singu-plx-far-x", `${far[0].toFixed(2)}px`);
      root.setProperty("--singu-plx-far-y", `${far[1].toFixed(2)}px`);
    });
  };
  bag.add(on(window, "pointermove", handler));
  bag.add(() => {
    if (raf) cancelAnimationFrame(raf);
    const root = document.documentElement.style;
    for (const k of ["--singu-plx-fg-x", "--singu-plx-fg-y", "--singu-plx-base-x", "--singu-plx-base-y", "--singu-plx-far-x", "--singu-plx-far-y"]) {
      root.removeProperty(k);
    }
  });
}

// ---- Q-79 主题精品店（12 套策展主题；悬停试穿/移开还原/点击固化）----
const BOUTIQUE_THEMES: Array<{ id: string; name: string; hue: number; sat: number; light: number }> = [
  { id: "polar-night", name: "极夜观测", hue: 220, sat: 42, light: 8 },
  { id: "warm-paper", name: "暖纸手账", hue: 34, sat: 38, light: 14 },
  { id: "deep-machine", name: "深海机房", hue: 195, sat: 55, light: 7 },
  { id: "sakura-dusk", name: "樱暮", hue: 335, sat: 48, light: 12 },
  { id: "forest-ops", name: "林间运维", hue: 145, sat: 36, light: 9 },
  { id: "ember-lab", name: "余烬实验室", hue: 18, sat: 60, light: 10 },
  { id: "glass-dawn", name: "玻璃黎明", hue: 200, sat: 30, light: 16 },
  { id: "violet-arc", name: "紫弧", hue: 268, sat: 45, light: 11 },
  { id: "desert-relay", name: "荒漠中继", hue: 40, sat: 52, light: 13 },
  { id: "arctic-teal", name: "极地青", hue: 178, sat: 50, light: 9 },
  { id: "cocoa-terminal", name: "可可终端", hue: 22, sat: 30, light: 10 },
  { id: "midnight-mint", name: "午夜薄荷", hue: 160, sat: 40, light: 8 },
];
function mountBoutique(): void {
  const applyTry = (t: { hue: number; sat: number; light: number }): void => {
    const root = document.documentElement.style;
    root.setProperty("--singu-try-hue", String(t.hue));
    root.setProperty("--singu-try-sat", `${t.sat}%`);
    root.setProperty("--singu-try-light", `${t.light}%`);
  };
  const clearTry = (): void => {
    const root = document.documentElement.style;
    root.removeProperty("--singu-try-hue");
    root.removeProperty("--singu-try-sat");
    root.removeProperty("--singu-try-light");
  };
  bag.add(on(window, "singu:boutique-try", (e: Event) => applyTry(BOUTIQUE_THEMES.find((t) => t.id === (e as CustomEvent).detail?.id) ?? BOUTIQUE_THEMES[0]!)));
  bag.add(on(window, "singu:boutique-clear", clearTry));
  bag.add(
    on(window, "singu:boutique-commit", (e: Event) => {
      const t = BOUTIQUE_THEMES.find((x) => x.id === (e as CustomEvent).detail?.id);
      if (!t) return;
      // 固化走既有主题设置（custom 主题的 accent 覆盖；零侵入复用设置通道）
      singuStoreWrite("boutique-committed", t.id);
      window.dispatchEvent(
        new CustomEvent("settings:patch", { detail: { themeId: "custom", accentHue: t.hue } }),
      );
      clearTry();
      ctxRef?.toast("success", "主题已固化", t.name);
    }),
  );
  bag.add(on(window, "singu:boutique-list", () => emitSingu("boutique-list", BOUTIQUE_THEMES)));
  bag.add(clearTry);
}

// ---- Q-80 材质滑块（四滑块实时作用于全部面板材质）----
function mountMaterialSliders(): void {
  const apply = (fog: number, refraction: number, grain: number, gloss: number): void => {
    const vars = materialVars(fog, refraction, grain, gloss);
    for (const [k, v] of Object.entries(vars)) document.documentElement.style.setProperty(k, v);
  };
  bag.add(
    on(window, "singu:material", (e: Event) => {
      const d = (e as CustomEvent).detail as { fog?: number; refraction?: number; grain?: number; gloss?: number };
      apply(d.fog ?? 40, d.refraction ?? 30, d.grain ?? 15, d.gloss ?? 55);
      singuStoreWrite("material", { fog: d.fog ?? 40, refraction: d.refraction ?? 30, grain: d.grain ?? 15, gloss: d.gloss ?? 55 });
    }),
  );
  // 恢复上次调节
  const saved = singuStoreRead<{ fog: number; refraction: number; grain: number; gloss: number }>("material", {
    fog: 40,
    refraction: 30,
    grain: 15,
    gloss: 55,
  });
  if (ctxRef?.on("Q-80")) apply(saved.fog, saved.refraction, saved.grain, saved.gloss);
  bag.add(() => {
    for (const k of ["--singu-mat-fog", "--singu-mat-refraction", "--singu-mat-grain", "--singu-mat-gloss"]) {
      document.documentElement.style.removeProperty(k);
    }
  });
}

// ---- Q-81 过渡仪式（深浅切换三拍换幕：沉没→呼吸→升腾）----
function mountModeRitual(): void {
  let stage: HTMLElement | null = null;
  const run = (toDark: boolean): void => {
    if (!ctxRef?.on("Q-81")) return;
    if (!ctxRef.motionOK()) return; // reduce-motion 直接切换
    if (stage) return; // 上一次仪式未完
    stage = makeEl("div", `singu-ritual ${toDark ? "to-dark" : "to-light"}`);
    singuLayer().appendChild(stage);
    setTimeout(() => {
      stage?.remove();
      stage = null;
    }, 950); // 300 + 150 + 450 + 缓冲
  };
  bag.add(on(window, "theme:mode-changed", (e: Event) => run(Boolean((e as CustomEvent).detail?.dark))));
  bag.add(() => {
    stage?.remove();
    stage = null;
  });
}

export function visualsDomain(): DomainController {
  return {
    domain: "vision",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountSeasonal();
        mountParallax();
        mountBoutique();
        mountMaterialSliders();
        mountModeRitual();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-ritual").forEach((e) => e.remove());
    },
  };
}
