/**
 * SINGULARITY-100 · 域2 窗口与空间管理（Q-08…Q-15）行为层。
 *
 * 边界（全景 §2）：存量覆盖吸附/快照/时间机器；本域补活性感知、排列算法、
 * 边缘偷看、监控角与手感物理。全部通过只读 DOM 观察 + vwm 公开几何 API
 * （moveVwmWin/resizeVwmWin）实现，不改写 VWM 内部。
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
  watchSelector,
} from "../shared";
import { singuMotionOK, singuNum } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

export interface WinRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Q-11：四算法排列（grid 等分网格 / spiral 黄金螺旋 / columns 多列 / quadrants 四象限）。 */
export function layoutPositions(
  n: number,
  bounds: WinRect,
  mode: "grid" | "spiral" | "columns" | "quadrants",
): WinRect[] {
  if (n <= 0) return [];
  const out: WinRect[] = [];
  const { x, y, w, h } = bounds;
  if (mode === "grid") {
    const cols = Math.ceil(Math.sqrt(n));
    const rows = Math.ceil(n / cols);
    const cw = w / cols;
    const ch = h / rows;
    for (let i = 0; i < n; i++) {
      const c = i % cols;
      const r = Math.floor(i / cols);
      out.push({ x: x + c * cw, y: y + r * ch, w: cw, h: ch });
    }
    return out;
  }
  if (mode === "columns") {
    const cw = w / n;
    for (let i = 0; i < n; i++) out.push({ x: x + i * cw, y, w: cw, h });
    return out;
  }
  if (mode === "quadrants") {
    const cw = w / 2;
    const ch = h / 2;
    for (let i = 0; i < n; i++) {
      const q = i % 4;
      out.push({ x: x + (q % 2) * cw, y: y + Math.floor(q / 2) * ch, w: cw, h: ch });
    }
    return out;
  }
  // spiral：黄金螺旋 —— 逐窗取当前区域长边分割，剩余区域递归
  let rx = x;
  let ry = y;
  let rw = w;
  let rh = h;
  const phi = 0.618;
  for (let i = 0; i < n; i++) {
    const frac = i === n - 1 ? 1 : phi;
    if (rw >= rh) {
      const cw = rw * frac;
      out.push({ x: rx, y: ry, w: cw, h: rh });
      rx += cw;
      rw -= cw;
    } else {
      const ch = rh * frac;
      out.push({ x: rx, y: ry, w: rw, h: ch });
      ry += ch;
      rh -= ch;
    }
  }
  return out;
}

/** Q-14：速度 → 倾斜角（deg，限幅 ±maxTilt）。 */
export function tiltFromVel(dx: number, dtMs: number, maxTilt: number): number {
  if (dtMs <= 0) return 0;
  const v = Math.abs(dx) / dtMs; // px/ms
  const deg = clamp(v * 1.6, 0, maxTilt);
  return Math.sign(dx) * deg;
}

/** Q-08：活力环参数（速度与亮度映射活性 0..1）。 */
export function vitalsOf(cpu: number, focused: boolean): { spinSec: number; opacity: number } {
  if (!focused) return { spinSec: 0, opacity: 0.22 };
  return { spinSec: cpu > 60 ? 1.1 : cpu > 20 ? 2.2 : 4, opacity: 0.45 + clamp(cpu / 100, 0, 0.5) };
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

function frames(): HTMLElement[] {
  if (typeof document === "undefined") return [];
  return Array.from(document.querySelectorAll<HTMLElement>(".vwm-window"));
}

// ---- Q-08 生命体征环 ----
function mountVitalsRing(): void {
  const style = makeEl("style");
  style.textContent = "";
  let timer: ReturnType<typeof setInterval> | null = null;
  const tick = (): void => {
    if (!ctxRef?.on("Q-08")) {
      document.querySelectorAll(".singu-vitals").forEach((e) => e.remove());
      return;
    }
    const pulse = ctxRef.pulse();
    const cpu = pulse?.cpu_usage ?? 0;
    for (const f of frames()) {
      let ring = f.querySelector<HTMLElement>(".singu-vitals");
      if (!ring) {
        ring = makeEl("span", "singu-vitals");
        const bar = f.querySelector(".vwm-titlebar");
        (bar ?? f).appendChild(ring);
      }
      const v = vitalsOf(cpu, f.classList.contains("focused"));
      ring.style.opacity = String(v.opacity);
      ring.style.animationDuration = v.spinSec > 0 ? `${v.spinSec}s` : "0s";
      ring.title = pulse ? `CPU ${cpu.toFixed(0)}%` : "";
      ring.classList.toggle("static", !singuMotionOK() || v.spinSec === 0);
    }
  };
  timer = setInterval(tick, 2000);
  bag.add(() => {
    if (timer) clearInterval(timer);
    document.querySelectorAll(".singu-vitals").forEach((e) => e.remove());
    style.remove();
  });
}

// ---- Q-09 焦点轨迹残影 ----
function mountFocusTrail(): void {
  let lastRect: DOMRect | null = null;
  const trails: SVGSVGElement[] = [];
  bag.add(
    on(window, "focusin", (e) => {
      if (!ctxRef?.on("Q-09") || !singuMotionOK()) return;
      const frame = (e.target as HTMLElement).closest<HTMLElement>(".vwm-window");
      if (!frame) return;
      const nowRect = frame.getBoundingClientRect();
      if (lastRect && (lastRect.left !== nowRect.left || lastRect.top !== nowRect.top)) {
        const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        svg.setAttribute("class", "singu-trail");
        const dur = singuNum("Q-09", "trailMs") || 350;
        const x1 = lastRect.left + lastRect.width / 2;
        const y1 = lastRect.top + lastRect.height / 2;
        const x2 = nowRect.left + nowRect.width / 2;
        const y2 = nowRect.top + nowRect.height / 2;
        svg.innerHTML = `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" />`;
        svg.style.animationDuration = `${dur}ms`;
        singuLayer().appendChild(svg);
        trails.push(svg);
        while (trails.length > 3) trails.shift()?.remove(); // 光轨最多 3 道
        setTimeout(() => {
          svg.remove();
          const i = trails.indexOf(svg);
          if (i >= 0) trails.splice(i, 1);
        }, dur + 80);
      }
      lastRect = nowRect;
    }),
  );
}

// ---- Q-10 窗口年龄衰减 ----
function mountAgeFade(): void {
  const lastTouch = new WeakMap<HTMLElement, number>();
  let timer: ReturnType<typeof setInterval> | null = null;
  const mark = (f: HTMLElement): void => {
    lastTouch.set(f, Date.now());
  };
  bag.add(
    on(window, "pointerover", (e) => {
      const f = (e.target as HTMLElement).closest<HTMLElement>(".vwm-window");
      if (f) mark(f);
    }),
  );
  bag.add(
    on(window, "focusin", (e) => {
      const f = (e.target as HTMLElement).closest<HTMLElement>(".vwm-window");
      if (f) mark(f);
    }),
  );
  timer = setInterval(() => {
    if (!ctxRef?.on("Q-10")) {
      frames().forEach((f) => f.classList.remove("singu-aged"));
      return;
    }
    const ageMin = singuNum("Q-10", "ageMin") || 30;
    const cutoff = Date.now() - ageMin * 60_000;
    for (const f of frames()) {
      const t = lastTouch.get(f);
      const aged = t !== undefined && t < cutoff && !f.classList.contains("focused");
      f.classList.toggle("singu-aged", aged);
    }
  }, 30_000);
  bag.add(() => {
    if (timer) clearInterval(timer);
    frames().forEach((f) => f.classList.remove("singu-aged"));
  });
}

// ---- Q-11 排列画廊（hub / 命令面板触发；singu:arrange 总线）----
function mountLayoutGallery(): void {
  bag.add(
    on(window, "singu:arrange", (e: Event) => {
      if (!ctxRef?.on("Q-11")) return;
      const mode = ((e as CustomEvent).detail?.mode ?? "grid") as "grid" | "spiral" | "columns" | "quadrants";
      void (async () => {
        const { moveVwmWin, resizeVwmWin, settleVwmWin } = await import("../../windows/vwm");
        const bounds: WinRect = {
          x: 24,
          y: 56,
          w: window.innerWidth - 48,
          h: window.innerHeight - 140,
        };
        const wins = frames().filter((f) => !f.classList.contains("minimized"));
        const rects = layoutPositions(wins.length, bounds, mode);
        const gap = 8;
        wins.forEach((f, i) => {
          const r = rects[i];
          const id = f.dataset.winid;
          if (!r || !id) return;
          const x = Math.round(r.x + gap);
          const y = Math.round(r.y + gap);
          const w = Math.round(r.w - gap * 2);
          const h = Math.round(r.h - gap * 2);
          f.style.transition = "left .35s cubic-bezier(.2,.8,.2,1), top .35s cubic-bezier(.2,.8,.2,1), width .35s, height .35s";
          moveVwmWin(id, x, y);
          resizeVwmWin(id, { x, y, w, h });
          setTimeout(() => {
            f.style.transition = "";
            settleVwmWin(id);
          }, 420);
        });
        emitSingu("arrange-applied", mode);
      })().catch(() => {
        /* vwm 模块不可用：如实放弃 */
      });
    }),
  );
}

// ---- Q-12 边缘窗口偷看 ----
function mountEdgePeek(): void {
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let peekEl: HTMLElement | null = null;
  const clear = (): void => {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = null;
    peekEl?.remove();
    peekEl = null;
  };
  bag.add(
    on(window, "pointermove", (e) => {
      if (!ctxRef?.on("Q-12")) return clear();
      const m = e as PointerEvent;
      const edge = m.clientX <= 4 ? "left" : m.clientX >= window.innerWidth - 4 ? "right" : null;
      if (!edge) return clear();
      if (hoverTimer || peekEl) return;
      hoverTimer = setTimeout(() => {
        hoverTimer = null;
        const ms = singuNum("Q-12", "peekMs") || 400;
        void ms;
        peekEl = makeEl("div", `singu-peek ${edge}`);
        for (const f of frames()) {
          if (f.classList.contains("focused") || f.classList.contains("minimized")) continue;
          const r = f.getBoundingClientRect();
          if (edge === "left" ? r.left > 60 : r.left < window.innerWidth - 60) continue; // 该侧被压住的
          const ghost = makeEl("div", "singu-peek-ghost");
          ghost.style.left = `${r.left}px`;
          ghost.style.top = `${r.top}px`;
          ghost.style.width = `${r.width}px`;
          ghost.style.height = `${r.height}px`;
          const title = f.querySelector(".vwm-title")?.textContent ?? "";
          ghost.title = title;
          peekEl!.appendChild(ghost);
        }
        singuLayer().appendChild(peekEl);
      }, 400);
    }),
  );
  bag.add(clear);
}

// ---- Q-13 镜像监控角 ----
function mountMirrorCorner(): void {
  let host: HTMLElement | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let idx = 0;
  const tick = (): void => {
    if (!ctxRef?.on("Q-13")) {
      host?.remove();
      host = null;
      return;
    }
    const candidates = frames().filter(
      (f) => !f.classList.contains("focused") && !f.classList.contains("minimized"),
    );
    if (candidates.length === 0) {
      if (host) {
        host.remove();
        host = null;
      }
      return;
    }
    if (!host) {
      host = makeEl("div", "singu-mirror");
      host.addEventListener("click", () => {
        const cur = candidates[idx % candidates.length]!;
        const id = cur.dataset.winid;
        if (id) void import("../../windows/vwm").then(({ focusVwmWin }) => focusVwmWin(id));
      });
      singuLayer().appendChild(host);
    }
    idx = (idx + 1) % candidates.length;
    const f = candidates[idx]!;
    const title = f.querySelector(".vwm-title")?.textContent ?? "—";
    const r = f.getBoundingClientRect();
    host.innerHTML = `<div class="singu-mirror-title">${escapeHtml(title)}</div>
      <div class="singu-mirror-meta">${Math.round(r.width)}×${Math.round(r.height)} · BACKGROUND</div>`;
  };
  timer = setInterval(tick, 10_000);
  tick();
  bag.add(() => {
    if (timer) clearInterval(timer);
    host?.remove();
  });
}

// ---- Q-14 动量拖拽 ----
function mountMomentumDrag(): void {
  interface DragState {
    frame: HTMLElement;
    lastX: number;
    lastT: number;
  }
  let drag: DragState | null = null;
  bag.add(
    on(window, "pointerdown", (e) => {
      const pe = e as PointerEvent;
      const bar = (pe.target as HTMLElement).closest<HTMLElement>(".vwm-titlebar");
      drag = bar ? { frame: bar.closest<HTMLElement>(".vwm-window")!, lastX: pe.clientX, lastT: performance.now() } : null;
    }),
  );
  bag.add(
    on(window, "pointermove", (e) => {
      if (!drag || !ctxRef?.on("Q-14") || !singuMotionOK()) return;
      const pe = e as PointerEvent;
      const now = performance.now();
      const tilt = tiltFromVel(pe.clientX - drag.lastX, now - drag.lastT, singuNum("Q-14", "maxTilt") || 2);
      drag.frame.style.setProperty("--singu-tilt", `${tilt.toFixed(2)}deg`);
      drag.lastX = pe.clientX;
      drag.lastT = now;
    }),
  );
  const release = (): void => {
    if (drag) drag.frame.style.setProperty("--singu-tilt", "0deg");
    drag = null;
  };
  bag.add(on(window, "pointerup", release));
  bag.add(on(window, "pointercancel", release));
}

// ---- Q-15 尺寸预设菜单（Alt+R 直达面板；作用于焦点窗口）----
function mountResizePresets(): void {
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ke.altKey || ke.key.toLowerCase() !== "r" || !ctxRef?.on("Q-15")) return;
      const focused = document.querySelector<HTMLElement>(".vwm-window.focused");
      if (!focused) return;
      let panel = document.querySelector<HTMLElement>("#singu-resize-presets");
      if (panel) {
        panel.remove();
        return;
      }
      panel = makeEl("div", "singu-panel singu-resize-presets");
      panel.id = "singu-resize-presets";
      const r = focused.getBoundingClientRect();
      panel.style.left = `${clamp(r.left + 40, 8, window.innerWidth - 240)}px`;
      panel.style.top = `${clamp(r.top + 40, 8, window.innerHeight - 220)}px`;
      const presets: Array<[string, number, number]> = [
        ["1280 × 720", 1280, 720],
        ["1600 × 900", 1600, 900],
        ["1920 × 1080", 1920, 1080],
        ["50% 桌面", Math.round(window.innerWidth * 0.5), Math.round(window.innerHeight * 0.5)],
        ["66% 桌面", Math.round(window.innerWidth * 0.66), Math.round(window.innerHeight * 0.66)],
        ["33% × 全高", Math.round(window.innerWidth * 0.33), window.innerHeight - 120],
      ];
      for (const [label, w, h] of presets) {
        const btn = makeEl("button", "singu-btn");
        btn.textContent = label;
        btn.addEventListener("click", () => {
          const id = focused.dataset.winid;
          if (id) {
            void import("../../windows/vwm").then(({ resizeVwmWin, settleVwmWin }) => {
              resizeVwmWin(id, {
                x: Math.max(8, Math.round(r.left)),
                y: Math.max(8, Math.round(r.top)),
                w: Math.min(w, window.innerWidth - 16),
                h: Math.min(h, window.innerHeight - 80),
              });
              settleVwmWin(id);
            });
          }
          panel!.remove();
        });
        panel.appendChild(btn);
      }
      singuLayer().appendChild(panel);
      const close = (ev: Event): void => {
        if (panel && !panel.contains(ev.target as Node)) {
          panel.remove();
          window.removeEventListener("pointerdown", close);
        }
      };
      setTimeout(() => window.addEventListener("pointerdown", close), 50);
    }),
  );
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function windowsDomain(): DomainController {
  return {
    domain: "windows",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountVitalsRing();
        mountFocusTrail();
        mountAgeFade();
        mountLayoutGallery();
        mountEdgePeek();
        mountMirrorCorner();
        mountMomentumDrag();
        mountResizePresets();
      });
      void watchSelector; // VWM 挂载后帧集合以实时查询为准，无需额外观察
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-vitals,.singu-trail,.singu-aged,.singu-peek,.singu-mirror,.singu-resize-presets").forEach((e) => e.remove());
    },
  };
}
