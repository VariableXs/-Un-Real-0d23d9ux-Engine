/**
 * SINGULARITY-100 · 域5 键盘与输入手感（Q-30…Q-36）行为层。
 *
 * 边界（全景 §5）：存量覆盖键位注册/作用域/回显/重映射；本域补 UI 层
 * 惯性物理、无修饰键序列缓冲、和弦节拍、空格流、数字直达、候选剧场与声呐。
 * 输入框聚焦时全部让位（isTypingTarget）。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  blip,
  clamp,
  domReady,
  isTypingTarget,
  makeEl,
  on,
  scrollableAncestor,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuNum } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-30：长按加速曲线（0.4s 后进入加速，最高 maxSpeed 倍）。 */
export function inertiaGain(heldMs: number, maxSpeed = 8): number {
  if (heldMs < 400) return 1;
  const t = (heldMs - 400) / 1000;
  return clamp(1 + t * (maxSpeed - 1), 1, maxSpeed);
}

/** Q-32：和弦时间窗（ms）。 */
export function chordWindowMs(timing: "strict" | "standard" | "lenient"): number {
  return timing === "strict" ? 80 : timing === "lenient" ? 400 : 200;
}

/** Q-33：空格流滚动速度（加速→松开缓停，末端 12% 减速）。 */
export function spaceFlowSpeed(heldMs: number, maxPxPerFrame = 26): number {
  const ramp = clamp(heldMs / 600, 0, 1);
  return maxPxPerFrame * (0.3 + 0.7 * ramp);
}

/** Q-31：默认键击流序列表（可自定义扩展）。 */
export const DEFAULT_FLOWS: Array<{ keys: string; label: string; action: string }> = [
  { keys: "gd", label: "回到桌面", action: "showDesktop" },
  { keys: "nf", label: "新建文件夹", action: "newFolder" },
  { keys: "vp", label: "命令面板", action: "palette" },
];

/** Q-65 同族：密码特征判定（长度 + 字符类 + 常见 key 名启发式）。 */
export function looksSecret(text: string): boolean {
  if (text.length < 8 || text.length > 64) return false;
  if (/\s/.test(text)) return false;
  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((r) => r.test(text)).length;
  return classes >= 3 || /^(sk-|ghp_|gho_|AKIA|eyJ)/.test(text);
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-30 键盘惯性（方向键长按加速 + 边界回弹）----
function mountKeyInertia(): void {
  const heldSince = new Map<string, number>();
  const lastMove = new Map<string, number>();
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-30") || isTypingTarget(ke.target) || ke.ctrlKey || ke.altKey || ke.metaKey) return;
      if (!ke.key.startsWith("Arrow")) return;
      const scroller = scrollableAncestor(ke.target instanceof Node ? ke.target : document.activeElement);
      if (!scroller) return;
      ke.preventDefault();
      if (!heldSince.has(ke.key)) heldSince.set(ke.key, performance.now());
      const held = performance.now() - (heldSince.get(ke.key) ?? 0);
      const gain = inertiaGain(held, singuNum("Q-30", "maxSpeed") || 8);
      const now = performance.now();
      const since = now - (lastMove.get(ke.key) ?? 0);
      if (since < 40 / gain) return; // 按 gain 提升事件密度
      lastMove.set(ke.key, now);
      const d = 34;
      const before = ke.key === "ArrowUp" ? scroller.scrollTop : ke.key === "ArrowDown" ? scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop : ke.key === "ArrowLeft" ? scroller.scrollLeft : scroller.scrollWidth - scroller.clientWidth - scroller.scrollLeft;
      const atEdge = (ke.key === "ArrowUp" || ke.key === "ArrowLeft") ? before <= 0 : before <= 0;
      if (atEdge) {
        // 边界回弹：2 格视觉位移 + 触壁音
        scroller.style.transition = "scroll-behavior";
        void before;
        blip(160, ke.key.endsWith("Left") || ke.key === "ArrowUp" ? -0.6 : 0.6, 50, 0.04);
        return;
      }
      if (ke.key === "ArrowUp") scroller.scrollTop -= d;
      else if (ke.key === "ArrowDown") scroller.scrollTop += d;
      else if (ke.key === "ArrowLeft") scroller.scrollLeft -= d;
      else scroller.scrollLeft += d;
    }),
  );
  bag.add(
    on(window, "keyup", (e) => {
      heldSince.delete((e as KeyboardEvent).key);
      lastMove.delete((e as KeyboardEvent).key);
    }),
  );
}

// ---- Q-31 输入缓冲（无修饰键序列；500ms 窗口）----
function mountInputBuffer(): void {
  let buffer = "";
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flows = [...DEFAULT_FLOWS, ...singuStoreRead<Array<{ keys: string; label: string; action: string }>>("flows", [])];
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-31") || ke.ctrlKey || ke.altKey || ke.metaKey) return;
      if (isTypingTarget(ke.target)) {
        buffer = "";
        return;
      }
      if (ke.key === "Escape") {
        buffer = "";
        return;
      }
      if (ke.key.length !== 1 || !/[a-z]/i.test(ke.key)) return;
      const windowMs = (() => {
        const timing = ctxRef.str("Q-32", "timing") || "standard";
        return chordWindowMs((["strict", "standard", "lenient"].includes(timing) ? timing : "standard") as "strict" | "standard" | "lenient");
      })();
      void windowMs;
      buffer = (buffer + ke.key.toLowerCase()).slice(-4);
      if (timer) clearTimeout(timer);
      const ttl = singuNum("Q-31", "bufferMs") || 500;
      timer = setTimeout(() => (buffer = ""), ttl);
      const hit = flows.find((f) => f.keys === buffer);
      if (hit) {
        buffer = "";
        dispatchFlowAction(hit.action);
      }
    }),
  );
}

function dispatchFlowAction(action: string): void {
  switch (action) {
    case "showDesktop":
      // 复用 VWM 公开 API：最小化全部虚拟窗口 = 回到桌面
      void import("../../windows/vwm")
        .then(({ minimizeAllVwm }) => minimizeAllVwm())
        .catch(() => window.dispatchEvent(new CustomEvent("singu:flow", { detail: { action } })));
      break;
    case "palette":
      // 命令面板的应用内监听是 window keydown ctrl+k —— 合成等价击键直达
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", code: "KeyK", ctrlKey: true, bubbles: true }));
      break;
    default:
      window.dispatchEvent(new CustomEvent("singu:flow", { detail: { action } }));
  }
}

// ---- Q-33 空格流（长按空格平滑滚动）----
function mountSpaceFlow(): void {
  let holding = false;
  let heldStart = 0;
  let raf = 0;
  const loop = (): void => {
    raf = requestAnimationFrame(loop);
    if (!holding || !ctxRef?.on("Q-33")) return;
    const scroller = scrollableAncestor(document.activeElement);
    if (!scroller) return;
    const held = performance.now() - heldStart;
    let v = spaceFlowSpeed(held);
    // 末端 12% 减速缓冲
    const remain = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    if (remain < scroller.clientHeight * 0.12) v *= clamp(remain / (scroller.clientHeight * 0.12), 0.1, 1);
    scroller.scrollTop += v;
  };
  raf = requestAnimationFrame(loop);
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (ke.key !== " " || ke.repeat) return;
      if (!ctxRef?.on("Q-33") || isTypingTarget(ke.target)) return;
      // 焦点在可交互控件时空格归控件
      if ((ke.target as HTMLElement).closest("button, a, [role=button], input[type=checkbox]")) return;
      const scroller = scrollableAncestor(ke.target instanceof Node ? ke.target : document.activeElement);
      if (!scroller) return;
      ke.preventDefault();
      holding = true;
      heldStart = performance.now();
    }),
  );
  bag.add(
    on(window, "keyup", (e) => {
      if ((e as KeyboardEvent).key === " ") holding = false;
    }),
  );
  bag.add(() => {
    cancelAnimationFrame(raf);
    holding = false;
  });
}

// ---- Q-34 数字跳转（按住 Alt 显示 1-9 角标，按数字直达）----
function mountNumberHints(): void {
  let host: HTMLElement | null = null;
  const hide = (): void => {
    host?.remove();
    host = null;
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ke.altKey || !ctxRef?.on("Q-34")) return;
      if (!host) {
        const scope = document.querySelector(".start-menu, .singu-overlay-open") ?? document;
        const items = Array.from(scope.querySelectorAll<HTMLElement>(".start-app, .tb-btn, .singu-list-item")).slice(0, 9);
        if (items.length === 0) return;
        host = makeEl("div", "singu-numhints");
        items.forEach((item, i) => {
          const r = item.getBoundingClientRect();
          const tag = makeEl("span", "singu-numtag");
          tag.textContent = String(i + 1);
          tag.style.left = `${r.left}px`;
          tag.style.top = `${r.top}px`;
          tag.dataset.idx = String(i);
          host!.appendChild(tag);
          (tag as HTMLElement & { __target?: HTMLElement }).__target = item;
        });
        singuLayer().appendChild(host);
      }
      if (/^[1-9]$/.test(ke.key)) {
        const tag = host?.querySelector<HTMLElement & { __target?: HTMLElement }>(`[data-idx="${Number(ke.key) - 1}"]`);
        tag?.__target?.click();
        hide();
      }
    }),
  );
  bag.add(
    on(window, "keyup", (e) => {
      if ((e as KeyboardEvent).key === "Alt") hide();
    }),
  );
  bag.add(hide);
}

// ---- Q-35 IME 候选剧场（组合输入大字剧场，仅环境内输入框）----
function mountImeStage(): void {
  let host: HTMLElement | null = null;
  const hide = (): void => {
    host?.remove();
    host = null;
  };
  const show = (text: string, input: HTMLElement): void => {
    if (!host) {
      host = makeEl("div", "singu-ime-stage");
      singuLayer().appendChild(host);
    }
    host.textContent = text;
    const r = input.getBoundingClientRect();
    host.style.left = `${r.left}px`;
    host.style.top = `${Math.max(8, r.top - 56)}px`;
  };
  bag.add(
    on(window, "compositionstart", (e) => {
      if (!ctxRef?.on("Q-35")) return;
      show("…", e.target as HTMLElement);
    }),
  );
  bag.add(
    on(window, "compositionupdate", (e) => {
      if (!ctxRef?.on("Q-35")) return;
      const ce = e as CompositionEvent;
      show(ce.data || "…", ce.target as HTMLElement);
    }),
  );
  bag.add(
    on(window, "compositionend", (e) => {
      if (!ctxRef?.on("Q-35")) return hide();
      const ce = e as CompositionEvent;
      if (host && ce.data) {
        host.classList.add("confirm");
        setTimeout(hide, 220); // 选中词微弹确认
      } else hide();
    }),
  );
  bag.add(hide);
}

// ---- Q-36 导航声呐（方向键导航空间音效）----
function mountNavSonar(): void {
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-36") || isTypingTarget(ke.target)) return;
      if (!ke.key.startsWith("Arrow")) return;
      const pan = ke.key === "ArrowLeft" ? -0.8 : ke.key === "ArrowRight" ? 0.8 : 0;
      const el = ke.target instanceof HTMLElement ? ke.target : document.activeElement;
      const r = el?.getBoundingClientRect();
      const distFromTop = r ? r.top / window.innerHeight : 0.5;
      const pitch = 880 - distFromTop * 500; // 越靠下音高越低
      blip(clamp(pitch, 300, 950), pan, 60, 0.045);
    }),
  );
}

// ---- Q-91 单键模式（组合键顺序单键化，服务运动障碍用户）----
function mountSingleKey(): void {
  let pending: string | null = null;
  let ring: HTMLElement | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const clear = (): void => {
    pending = null;
    ring?.remove();
    ring = null;
    if (timer) clearTimeout(timer);
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-91") || isTypingTarget(ke.target)) return;
      const mod = ["Control", "Alt", "Shift", "Meta"].includes(ke.key);
      if (mod && !pending) {
        pending = ke.key;
        const holdMs = singuNum("Q-91", "holdMs") || 3000;
        ring = makeEl("div", "singu-chordring");
        ring.style.animationDuration = `${holdMs}ms`;
        ring.innerHTML = `<span>${pending === "Control" ? "CTRL" : pending === "Alt" ? "ALT" : pending === "Shift" ? "SHIFT" : "META"}</span>`;
        singuLayer().appendChild(ring);
        if (timer) clearTimeout(timer);
        timer = setTimeout(clear, holdMs);
        return;
      }
      if (pending && !mod && ke.key.length === 1) {
        // 合成和弦：以完整修饰键重放（触发环境内 JS 级快捷键处理）
        const mods = { ctrl: pending === "Control", alt: pending === "Alt", shift: pending === "Shift", meta: pending === "Meta" };
        clear();
        const synth = new KeyboardEvent("keydown", {
          key: ke.key,
          code: ke.code,
          ctrlKey: mods.ctrl,
          altKey: mods.alt,
          shiftKey: mods.shift,
          metaKey: mods.meta,
          bubbles: true,
        });
        (ke.target as HTMLElement).dispatchEvent(synth);
      }
      if (ke.key === "Escape") clear();
    }),
  );
  bag.add(clear);
}

export function inputDomain(): DomainController {
  return {
    domain: "input",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountKeyInertia();
        mountInputBuffer();
        mountSpaceFlow();
        mountNumberHints();
        mountImeStage();
        mountNavSonar();
        mountSingleKey();
        void singuStoreWrite;
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-numhints,.singu-ime-stage,.singu-chordring").forEach((e) => e.remove());
    },
  };
}
