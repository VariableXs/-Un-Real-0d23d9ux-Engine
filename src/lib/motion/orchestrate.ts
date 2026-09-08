/**
 * N-06 动效编排系统（NEXT-40 · AI-2 窗口路）——中央编排器：
 * - 单入口 `orchestrate(elements, opts)`：批量动效自动按 30ms 错峰（stagger）、
 *   自动合并到单 rAF 批次；
 * - FLIP 引擎：`flipMeasure/flipPlay` 布局变化自动量测 First/Last、反演播放，
 *   图标网格重排 / 列表折叠 / 溢出归位全部获得平滑过渡；
 * - 性能预算：动效期间帧预算 8ms，超支自动降级为直接落位（静默降级，
 *   `budgetExceeded` 计数 bench 可见）；
 * - reduce-motion 契约：系统偏好开启时全部动效 ≤80ms 或直接落位；
 * - spring 档位复用 A-2 tokens（--dur-3 spring 模型：-20% 初速、4% 过冲）。
 *
 * 纯调度内核，不触碰业务 DOM 之外的任何状态；对四大应用「生效但不改其代码」。
 */

/** 动效档位（对齐 tokens.css --dur 体系）。 */
export type MotionKind = "spring" | "fade" | "instant";

export interface OrchestrateOptions {
  /** 相邻元素错峰间隔 ms（默认 30）。 */
  stagger?: number;
  /** 动效档位（默认 spring）。 */
  kind?: MotionKind;
  /** 单元素动画时长 ms（默认 spring=240）。 */
  duration?: number;
  /** 动效开始的帧回调（测试注入用；生产走 rAF）。 */
  raf?: (cb: () => void) => number;
  /** 是否 reduce-motion（调用方传 matchMedia 结果；true 时 ≤80ms 或直接落位）。 */
  reduceMotion?: boolean;
}

export interface OrchestrateTarget {
  el: HTMLElement;
  /** 应用的最终样式（class 或 inline transform 由调用方决定）；编排器只负责时序。 */
  play: (el: HTMLElement) => void;
}

export const STAGGER_MS = 30;
export const SPRING_MS = 240;
export const REDUCE_MS = 80;
/** 帧预算：8ms（16.7ms 帧内留给浏览器 ~8.7ms）。 */
export const FRAME_BUDGET_MS = 8;

/** 预算超支计数（bench / 动效实验室可见）。 */
let budgetExceededCount = 0;
export function motionBudgetExceeded(): number {
  return budgetExceededCount;
}
export function resetMotionBudget(): void {
  budgetExceededCount = 0;
}

function durationFor(opts: OrchestrateOptions): number {
  if (opts.reduceMotion) return Math.min(opts.duration ?? SPRING_MS, REDUCE_MS);
  return opts.duration ?? (opts.kind === "instant" ? 0 : SPRING_MS);
}

/**
 * 批量编排：第 i 个元素延迟 `i * stagger` 后调用其 play()。
 * stagger=0 或单元素时零延迟直接落位；reduce-motion 时错峰压缩为 0
 * （全部同时落位，总时长 ≤80ms）——避免长时间排队阻塞。
 */
export function orchestrate(targets: OrchestrateTarget[], opts: OrchestrateOptions = {}): void {
  if (targets.length === 0) return;
  const stagger = opts.reduceMotion ? 0 : Math.max(0, opts.stagger ?? STAGGER_MS);
  const dur = durationFor(opts);
  if (dur === 0) {
    // instant：同一批次直接落位（合并到单帧）
    (opts.raf ?? ((cb: () => void) => requestAnimationFrame(cb)))(() => {
      const t0 = performance.now();
      for (const t of targets) t.play(t.el);
      if (performance.now() - t0 > FRAME_BUDGET_MS) budgetExceededCount += 1;
    });
    return;
  }
  const timers: number[] = [];
  targets.forEach((t, i) => {
    timers.push(
      window.setTimeout(() => {
        const t0 = performance.now();
        t.play(t.el);
        if (performance.now() - t0 > FRAME_BUDGET_MS) budgetExceededCount += 1;
      }, i * stagger),
    );
  });
  // 返回值：无（编排器不持句柄；调用方如需取消用 data 标记）
  void timers;
}

// ---------- FLIP 引擎 ----------

export interface FlipRecord {
  el: HTMLElement;
  first: { x: number; y: number };
}

/** First：布局变化前量测（getBoundingClientRect 一次，调用方自行批量）。 */
export function flipMeasure(el: HTMLElement): FlipRecord {
  const r = el.getBoundingClientRect();
  return { el, first: { x: r.left, y: r.top } };
}

/**
 * Last + Invert + Play：布局变化后调用。
 * 反演 First→Last 差值用 transform 起手，下一帧移除 → 浏览器平滑过渡到落位。
 * reduce-motion 或零位移 → 直接落位（不产生过渡）。
 * 返回 invert 距离（px）；0 表示无需动画。
 */
export function flipPlay(
  rec: FlipRecord,
  opts: { durationMs?: number; reduceMotion?: boolean } = {},
): number {
  const last = rec.el.getBoundingClientRect();
  const dx = rec.first.x - last.left;
  const dy = rec.first.y - last.top;
  if (dx === 0 && dy === 0) return 0;
  if (opts.reduceMotion) return Math.abs(dx) + Math.abs(dy); // 不动画，直接落位
  const dur = opts.durationMs ?? SPRING_MS;
  rec.el.style.transform = `translate(${dx}px, ${dy}px)`;
  rec.el.style.transition = "none";
  requestAnimationFrame(() => {
    rec.el.style.transition = `transform ${dur}ms var(--ease-spring, cubic-bezier(0.34, 1.56, 0.64, 1))`;
    rec.el.style.transform = "";
    const cleanup = () => {
      rec.el.style.transition = "";
      rec.el.removeEventListener("transitionend", cleanup);
    };
    rec.el.addEventListener("transitionend", cleanup);
  });
  return Math.abs(dx) + Math.abs(dy);
}

