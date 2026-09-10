/**
 * SINGULARITY-100 · 共享底座：领域模块公用的 DOM 工具、降级账本与上下文类型。
 *
 * 纪律（全景 §0.1）：
 * - 零侵入：所有 DOM 产出物挂载到 #singu-layer（pointer-events:none）或
 *   以只读方式观察既有元素（watchSelector / 事件监听），绝不改写既有组件内部逻辑；
 * - 诚实降级：不可用能力如实记录到降级账本（Q-98 单一真源），不伪造状态；
 * - 一切行为层入口在非 DOM 环境（vitest node）安全 no-op。
 */

import { singuMotionOK } from "./registry";

// ---------------------------------------------------------------------------
// 领域上下文（由 runtime 注入；控制器在事件触发时实时读取开关状态）
// ---------------------------------------------------------------------------

export interface SinguPulseLike {
  cpu_usage: number;
  mem_used_mb: number;
  mem_total_mb: number;
  gpu_usage: number | null;
  cpu_temp_c: number | null;
  temp_estimated: boolean;
  fan_high: boolean;
  battery_percent: number | null;
  battery_ac: boolean;
  disks: Array<{ name: string; mount: string; total_bytes: number; available_bytes: number }>;
  net_up_bps: number;
  net_down_bps: number;
  ts_ms: number;
}

export interface DomainCtx {
  /** 功能开关（触发时实时读取注册表，切换即时生效无需重挂载）。 */
  on: (id: string) => boolean;
  num: (id: string, key: string) => number;
  str: (id: string, key: string) => string;
  bool: (id: string, key: string) => boolean;
  /** 运动允许（reduce-motion / safeMode / static 观感统一判定）。 */
  motionOK: () => boolean;
  /** 共享硬件脉搏（2s 缓存；无 Tauri / 未就绪时 null —— 调用方必须处理）。 */
  pulse: () => SinguPulseLike | null;
  /** toast 反馈通道（uiStore.pushToast 的安全包装）。 */
  toast: (kind: "info" | "success" | "error", msg: string, detail?: string) => void;
}

export interface DomainController {
  domain: string;
  mount(ctx: DomainCtx): void;
  unmount(): void;
}

// ---------------------------------------------------------------------------
// #singu-layer：奇点层宿主（所有浮层/指示器的统一父节点）
// ---------------------------------------------------------------------------

let layerEl: HTMLElement | null = null;

export function singuLayer(): HTMLElement {
  if (layerEl && layerEl.isConnected) return layerEl;
  if (typeof document === "undefined") throw new Error("no document");
  layerEl = document.createElement("div");
  layerEl.id = "singu-layer";
  document.body.appendChild(layerEl);
  return layerEl;
}

/** 在奇点层内创建元素（css 类名 + 可选内联样式）。 */
export function makeEl<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  styles?: Partial<CSSStyleDeclaration>,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (className) el.className = className;
  if (styles) Object.assign(el.style, styles);
  return el;
}

// ---------------------------------------------------------------------------
// DOM 观察 / 事件工具
// ---------------------------------------------------------------------------

export type Unsub = () => void;

/** addEventListener 的可退订包装。 */
export function on<E extends Event>(
  target: EventTarget,
  type: string,
  fn: (e: E) => void,
  opts?: AddEventListenerOptions | boolean,
): Unsub {
  const handler = fn as EventListener;
  target.addEventListener(type, handler, opts);
  return () => target.removeEventListener(type, handler, opts);
}

/** 等待 DOM 就绪（已就绪则同步执行）。 */
export function domReady(fn: () => void): void {
  if (typeof document === "undefined") return;
  if (document.readyState === "loading") {
    const un = on(document, "DOMContentLoaded", () => {
      un();
      fn();
    });
  } else {
    fn();
  }
}

/**
 * 选择器观察：对当前已存在的每个匹配元素调用 onAdd，并对未来出现/移除的
 * 元素持续回调（子树挂载观察，只读不改写）。返回退订。
 */
export function watchSelector(
  selector: string,
  onAdd: (el: Element) => void,
  onRemove?: (el: Element) => void,
  root?: ParentNode,
): Unsub {
  if (typeof document === "undefined") return () => {};
  const scope = root ?? document.body;
  const seen = new WeakSet<Element>();
  const scan = (): void => {
    for (const el of Array.from(scope.querySelectorAll(selector))) {
      if (!seen.has(el)) {
        seen.add(el);
        onAdd(el);
      }
    }
  };
  scan();
  // 大检查第十四轮：变更去抖。子树挂载可能高频爆发（壁纸层逐帧挂载、
  // 批量图标注册），每批同步 querySelectorAll 全树扫描会形成风暴
  // （designNova 同类问题实测整页冻结）。合并到帧级单次扫描，
  // 语义不变（新元素最迟下一帧纳入）。
  let pending = 0;
  const schedule = (): void => {
    if (pending) return;
    if (typeof requestAnimationFrame === "function") {
      pending = requestAnimationFrame(() => {
        pending = 0;
        scan();
      });
    } else {
      pending = window.setTimeout(() => {
        pending = 0;
        scan();
      }, 50) as unknown as number;
    }
  };
  const mo = new MutationObserver(schedule);
  mo.observe(scope, { childList: true, subtree: true });
  return () => {
    mo.disconnect();
    if (pending) {
      if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(pending);
      else clearTimeout(pending);
      pending = 0;
    }
    if (onRemove) for (const el of Array.from(scope.querySelectorAll(selector))) onRemove(el);
  };
}

/** 事件目标是否正在文本输入（输入缓冲/声呐等功能必须让位）。 */
export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable) return true;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

/** 焦点链上最近的可滚动祖先（键盘惯性/空格流用）。 */
export function scrollableAncestor(start: Node | null): HTMLElement | null {
  let node: Node | null = start;
  while (node instanceof HTMLElement) {
    const style = window.getComputedStyle(node);
    const scrollable = /(auto|scroll|overlay)/.test(style.overflowY + style.overflowX);
    if (scrollable && (node.scrollHeight > node.clientHeight + 4 || node.scrollWidth > node.clientWidth + 4)) {
      return node;
    }
    node = node.parentElement;
  }
  return null;
}

export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** 命名空间事件派发（singu:* 总线，供 hub/overlay/测试观察）。 */
export function emitSingu(name: string, detail?: unknown): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(`singu:${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// 本地持久化（奇点层数据统一键空间 variable.singu.*）
// ---------------------------------------------------------------------------

export function singuStoreRead<T>(key: string, fallback: T): T {
  if (typeof localStorage === "undefined") return fallback;
  try {
    const raw = localStorage.getItem(`variable.singu.${key}`);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function singuStoreWrite(key: string, value: unknown): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(`variable.singu.${key}`, JSON.stringify(value));
  } catch {
    /* 配额/隐私模式：如实跳过 */
  }
}

/** 环形数组（定长，最新在前）。 */
export function ringPush<T>(arr: readonly T[], item: T, max: number): T[] {
  return [item, ...arr].slice(0, max);
}

// ---------------------------------------------------------------------------
// 降级账本（Q-98 单一真源）：谁被降级、为什么、如何恢复
// ---------------------------------------------------------------------------

export interface DegradeEntry {
  key: string;
  zh: string;
  active: boolean;
  reason: string;
  recover: string;
  ts: number;
}

const degradeListeners = new Set<() => void>();
let degradeState: Record<string, DegradeEntry> = {};

/** 记录/更新一条降级态（key 稳定；active=false 时保留记录便于回看恢复条件）。 */
export function recordDegrade(key: string, zh: string, active: boolean, reason: string, recover: string): void {
  const cur = degradeState[key];
  if (cur && cur.active === active) return;
  degradeState = { ...degradeState, [key]: { key, zh, active, reason, recover, ts: Date.now() } };
  for (const fn of degradeListeners) fn();
}

export function getDegradations(): DegradeEntry[] {
  return Object.values(degradeState).sort((a, b) => Number(b.active) - Number(a.active) || a.key.localeCompare(b.key));
}

export function subscribeDegrade(fn: () => void): Unsub {
  degradeListeners.add(fn);
  return () => degradeListeners.delete(fn);
}

// ---------------------------------------------------------------------------
// WebAudio 微件（Q-36 声呐 / Q-48 轮盘 / Q-85 优先铃声共用；懒初始化）
// ---------------------------------------------------------------------------

let audioCtx: AudioContext | null = null;

function ctx(): AudioContext | null {
  if (typeof window === "undefined") return null;
  try {
    if (!audioCtx) {
      const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return null;
      audioCtx = new AC();
    }
    if (audioCtx.state === "suspended") void audioCtx.resume();
    return audioCtx;
  } catch {
    return null;
  }
}

/** 单音脉冲（声呐/确认音）。pan ∈ -1(左)..1(右)，freq 越高音高越高。 */
export function blip(freq: number, pan = 0, durMs = 70, gain = 0.06): void {
  const ac = ctx();
  if (!ac) return;
  try {
    const osc = ac.createOscillator();
    const g = ac.createGain();
    const p = ac.createStereoPanner();
    osc.type = "sine";
    osc.frequency.value = freq;
    p.pan.value = clamp(pan, -1, 1);
    g.gain.setValueAtTime(0, ac.currentTime);
    g.gain.linearRampToValueAtTime(gain, ac.currentTime + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, ac.currentTime + durMs / 1000);
    osc.connect(g).connect(p).connect(ac.destination);
    osc.start();
    osc.stop(ac.currentTime + durMs / 1000 + 0.02);
  } catch {
    /* 音频设备不可用：如实静默 */
  }
}

/** 双音铃声（优先通知用，与普通提示音可区分）。 */
export function chime(): void {
  blip(880, 0, 120, 0.05);
  setTimeout(() => blip(1174.66, 0, 160, 0.05), 110);
}

/** 白噪雨声（Q-44 番茄休息期，本地合成零素材）。返回停止函数。 */
export function rainNoise(): () => void {
  const ac = ctx();
  if (!ac) return () => {};
  try {
    const len = ac.sampleRate * 2;
    const buf = ac.createBuffer(1, len, ac.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < len; i++) data[i] = (Math.random() * 2 - 1) * 0.12;
    const src = ac.createBufferSource();
    src.buffer = buf;
    src.loop = true;
    const filter = ac.createBiquadFilter();
    filter.type = "lowpass";
    filter.frequency.value = 900;
    const g = ac.createGain();
    g.gain.value = 0.0;
    g.gain.linearRampToValueAtTime(0.5, ac.currentTime + 0.8);
    src.connect(filter).connect(g).connect(ac.destination);
    src.start();
    let stopped = false;
    return () => {
      if (stopped) return;
      stopped = true;
      try {
        g.gain.linearRampToValueAtTime(0, ac.currentTime + 0.6);
        setTimeout(() => src.stop(), 700);
      } catch {
        /* 已停止 */
      }
    };
  } catch {
    return () => {};
  }
}

// ---------------------------------------------------------------------------
// 挂载辅助：一次性安装一组退订（控制器卸载时统一回收）
// ---------------------------------------------------------------------------

export class UnsubBag {
  private list: Unsub[] = [];
  add(u: Unsub): void {
    this.list.push(u);
  }
  run(): void {
    for (const u of this.list.splice(0)) {
      try {
        u();
      } catch {
        /* 卸载尽力而为 */
      }
    }
  }
}

/** 统一运动判定（reduce-motion + safeMode/static 观感，运行时注入缓存）。 */
export function motionAllowed(safeModeLike: boolean): boolean {
  return singuMotionOK() && !safeModeLike;
}
