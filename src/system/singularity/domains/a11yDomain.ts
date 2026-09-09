/**
 * SINGULARITY-100 · 域14 无障碍与本地化（Q-87…Q-92）行为层。
 *
 * 边界（全景 §14）：V-39/V-74/Z-25/U-57 管既有搜索跳转/密度/放大镜/引导；
 * 本域是宽容搜索、文字无级、色觉滤镜、阅读引导、单键模式与手势提示卡。
 * 全部为「叠加辅助层」：不改写既有输入路径，reduce-motion 下克制降级。
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
} from "../shared";
import { singuNum, singuStr } from "../registry";
import { pinyinOf } from "../../../lib/pinyin";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-87：编辑距离（≤2 容错）。 */
export function editDistance(a: string, b: string): number {
  const m = a.length;
  const n = b.length;
  if (Math.abs(m - n) > 2) return 3; // 快速剪枝
  const dp = Array.from({ length: m + 1 }, (_, i) => [i, ...Array(n).fill(0)]);
  for (let j = 0; j <= n; j++) dp[0]![j] = j;
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      dp[i]![j] = Math.min(
        dp[i - 1]![j]! + 1,
        dp[i]![j - 1]! + 1,
        dp[i - 1]![j - 1]! + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
    }
  }
  return dp[m]![n]!;
}

/** Q-87：容错命中（精确匹配恒优先；拼音/错字三级容错）。 */
export type FuzzyLevel = "off" | "low" | "high";
export function fuzzyHit(query: string, target: string, level: FuzzyLevel): boolean {
  if (!query) return false;
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (t.includes(q)) return true; // 精确子串
  if (level === "off") return false;
  const budget = level === "high" ? 2 : 1;
  // 错字容错（按词比较，避免长句全量 DP）
  const tWords = t.split(/\s+/);
  if (tWords.some((w) => Math.abs(w.length - q.length) <= budget && editDistance(q, w) <= budget)) return true;
  // 拼音容错：bianliqi → 变量器
  const py = pinyinOf(target).toLowerCase();
  if (py.includes(q)) return true;
  return Math.abs(py.length - q.length) <= budget && editDistance(q, py) <= budget;
}

/** Q-88：文字缩放（0.85..1.3 ×）。 */
export function textScalePercent(v: number): number {
  return clamp(Math.round(v), 85, 130);
}

/** Q-89：色觉滤镜矩阵（保亮度 Daltonization 近似）。 */
export function cvdFilter(kind: "none" | "protan" | "deutan" | "tritan"): string {
  switch (kind) {
    case "protan":
      return "url(#singu-cvd-protan)";
    case "deutan":
      return "url(#singu-cvd-deutan)";
    case "tritan":
      return "url(#singu-cvd-tritan)";
    default:
      return "";
  }
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-87 宽容搜索（singu:fuzzy-query 事件 → 建议条；零侵入叠加）----
function mountFuzzyFind(): void {
  let hintBar: HTMLElement | null = null;
  const suggest = (query: string, candidates: string[]): void => {
    const level = singuStr("Q-87", "fuzzy") as FuzzyLevel;
    if (!ctxRef?.on("Q-87") || level === "off" || !query.trim()) {
      hintBar?.remove();
      hintBar = null;
      return;
    }
    const hits = candidates.filter((c) => fuzzyHit(query, c, level) && !c.toLowerCase().includes(query.toLowerCase()));
    if (hits.length === 0) {
      hintBar?.remove();
      hintBar = null;
      return;
    }
    if (!hintBar) {
      hintBar = makeEl("div", "singu-fuzzy-hint");
      singuLayer().appendChild(hintBar);
    }
    hintBar.innerHTML = hits
      .slice(0, 3)
      .map((h) => `<button class="singu-btn" data-v="${escapeHtml(h)}">${escapeHtml(h)}</button>`)
      .join("");
    hintBar.querySelectorAll("button").forEach((b) =>
      b.addEventListener("click", () => {
        window.dispatchEvent(new CustomEvent("singu:fuzzy-pick", { detail: { value: (b as HTMLElement).dataset.v } }));
        hintBar?.remove();
        hintBar = null;
      }),
    );
    emitSingu("fuzzy-hits", hits.slice(0, 10));
  };
  bag.add(
    on(window, "singu:fuzzy-query", (e: Event) => {
      const d = (e as CustomEvent).detail as { query: string; candidates?: string[] };
      suggest(d.query, d.candidates ?? []);
    }),
  );
  bag.add(() => {
    hintBar?.remove();
    hintBar = null;
  });
}

// ---- Q-88 文字无级（root font-size 缩放；只缩文字不缩布局 via em 系数）----
function mountTextScale(): void {
  const apply = (): void => {
    if (!ctxRef?.on("Q-88")) {
      document.documentElement.style.removeProperty("--singu-text-scale");
      return;
    }
    const pct = textScalePercent(singuNum("Q-88", "scale") || 100);
    document.documentElement.style.setProperty("--singu-text-scale", String(pct / 100));
  };
  bag.add(on(window, "singu:text-scale", apply));
  apply();
  bag.add(() => document.documentElement.style.removeProperty("--singu-text-scale"));
}

// ---- Q-89 色觉滤镜（SVG 滤镜注入 + root filter；截图层豁免）----
function mountCvdFilters(): void {
  let injected = false;
  const apply = (): void => {
    const kind = singuStr("Q-89", "cvd") as "none" | "protan" | "deutan" | "tritan";
    if (!ctxRef?.on("Q-89") || kind === "none") {
      document.documentElement.style.removeProperty("filter");
      document.documentElement.classList.remove("singu-cvd");
      return;
    }
    if (!injected && typeof document !== "undefined") {
      // 三类色觉滤镜矩阵（保亮度：行和为 1 的 Daltonization 近似）
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.id = "singu-cvd-defs";
      svg.setAttribute("aria-hidden", "true");
      svg.style.position = "absolute";
      svg.style.width = "0";
      svg.style.height = "0";
      svg.innerHTML = `
        <filter id="singu-cvd-protan"><feColorMatrix type="matrix" values="0.567 0.433 0 0 0  0.558 0.442 0 0 0  0 0.242 0.758 0 0  0 0 0 1 0"/></filter>
        <filter id="singu-cvd-deutan"><feColorMatrix type="matrix" values="0.625 0.375 0 0 0  0.7 0.3 0 0 0  0 0.3 0.7 0 0  0 0 0 1 0"/></filter>
        <filter id="singu-cvd-tritan"><feColorMatrix type="matrix" values="0.95 0.05 0 0 0  0 0.433 0.567 0 0  0 0.475 0.525 0 0  0 0 0 1 0"/></filter>`;
      document.body.appendChild(svg);
      injected = true;
    }
    document.documentElement.classList.add("singu-cvd");
    document.documentElement.style.setProperty("filter", cvdFilter(kind));
  };
  bag.add(on(window, "singu:cvd", apply));
  apply();
  bag.add(() => {
    document.documentElement.style.removeProperty("filter");
    document.documentElement.classList.remove("singu-cvd");
    document.getElementById("singu-cvd-defs")?.remove();
    injected = false;
  });
}

// ---- Q-90 阅读引导（可移动水平引导条；跨窗口置顶）----
function mountReadingGuide(): void {
  let bar: HTMLElement | null = null;
  const toggle = (open: boolean): void => {
    if (open && !bar) {
      bar = makeEl("div", "singu-reading-guide");
      const grip = makeEl("i", "grip");
      bar.appendChild(grip);
      let dragging = false;
      const move = (y: number): void => {
        if (bar) bar.style.top = `${clamp(y, 8, window.innerHeight - 48)}px`;
      };
      grip.addEventListener("pointerdown", () => {
        dragging = true;
      });
      bag.add(on(window, "pointermove", (e) => dragging && move((e as MouseEvent).clientY)));
      bag.add(on(window, "pointerup", () => (dragging = false)));
      singuLayer().appendChild(bar);
      move(window.innerHeight / 2);
    } else if (!open && bar) {
      bar.remove();
      bar = null;
    }
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (ke.altKey && ke.code === "KeyR" && ctxRef?.on("Q-90")) {
        toggle(!bar);
        ke.preventDefault();
      }
    }),
  );
  bag.add(on(window, "singu:reading-guide", (e: Event) => toggle(Boolean((e as CustomEvent).detail?.open))));
  bag.add(() => {
    bar?.remove();
    bar = null;
  });
}

// ---- Q-91 单键模式（组合键顺序单键化：Ctrl 按下浮出待续环，3s 内按 C 生效）----
function mountSingleKey(): void {
  let pendingMod: string | null = null;
  let ring: HTMLElement | null = null;
  let ringTimer: ReturnType<typeof setTimeout> | null = null;
  const showRing = (mod: string): void => {
    if (!ring) {
      ring = makeEl("div", "singu-chord-ring");
      singuLayer().appendChild(ring);
    }
    ring.textContent = `${mod.toUpperCase()} …`;
    if (ringTimer) clearTimeout(ringTimer);
    ringTimer = setTimeout(() => {
      ring?.remove();
      ring = null;
      pendingMod = null;
    }, 3000);
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-91")) return;
      if (ke.key === "Control" && !ke.repeat) {
        pendingMod = "ctrl";
        showRing("ctrl");
        ke.preventDefault();
      } else if (ke.key === "Alt" && !ke.repeat) {
        pendingMod = "alt";
        showRing("alt");
        ke.preventDefault();
      } else if (ke.key === "Shift" && !ke.repeat) {
        pendingMod = "shift";
        showRing("shift");
      } else if (pendingMod) {
        // 待续环内按键 → 组合语义派发（重放完整组合键）
        const mod = pendingMod;
        pendingMod = null;
        if (ringTimer) clearTimeout(ringTimer);
        ring?.remove();
        ring = null;
        if (ke.key.length === 1) {
          window.dispatchEvent(
            new KeyboardEvent("keydown", {
              key: ke.key,
              code: ke.code,
              ctrlKey: mod === "ctrl",
              altKey: mod === "alt",
              shiftKey: mod === "shift",
              bubbles: true,
            }),
          );
          ke.preventDefault();
        }
      }
    }, true),
  );
  bag.add(() => {
    if (ringTimer) clearTimeout(ringTimer);
    ring?.remove();
    ring = null;
  });
}

// ---- Q-92 手势提示卡（鼠标操作后 → 键盘等价提示，每操作每会话一次）----
const GESTURE_HINTS: Record<string, string> = {
  "snap-left": "刚才的左半屏吸附 = Win + ←",
  "snap-right": "刚才的右半屏吸附 = Win + →",
  "snap-max": "刚才的最大化 = Win + ↑",
  "minimize": "刚才的最小化 = Win + ↓",
  "close": "刚才的关闭 = Ctrl + W",
  "switch-app": "刚才的应用切换 = Alt + Tab",
  "show-desktop": "刚才的显示桌面 = Win + D",
  lasso: "刚才的框选 = Ctrl + A（全选语境）",
};
function mountGestureHints(): void {
  const shown = new Set<string>();
  let card: HTMLElement | null = null;
  let cardTimer: ReturnType<typeof setTimeout> | null = null;
  bag.add(
    on(window, "singu:gesture", (e: Event) => {
      if (!ctxRef?.on("Q-92")) return;
      const gesture = String((e as CustomEvent).detail?.kind ?? "");
      const hint = GESTURE_HINTS[gesture];
      if (!hint || shown.has(gesture)) return;
      shown.add(gesture);
      card?.remove();
      card = makeEl("div", "singu-gesture-hint");
      card.textContent = hint;
      singuLayer().appendChild(card);
      if (cardTimer) clearTimeout(cardTimer);
      cardTimer = setTimeout(() => {
        card?.remove();
        card = null;
      }, 5000);
    }),
  );
  bag.add(() => {
    if (cardTimer) clearTimeout(cardTimer);
    card?.remove();
    card = null;
  });
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function a11yDomain(): DomainController {
  return {
    domain: "a11y",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountFuzzyFind();
        mountTextScale();
        mountCvdFilters();
        mountReadingGuide();
        mountSingleKey();
        mountGestureHints();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-fuzzy-hint,.singu-reading-guide,.singu-chord-ring,.singu-gesture-hint").forEach((e) => e.remove());
      document.documentElement.style.removeProperty("filter");
      document.documentElement.style.removeProperty("--singu-text-scale");
      document.documentElement.classList.remove("singu-cvd");
      document.getElementById("singu-cvd-defs")?.remove();
    },
  };
}
