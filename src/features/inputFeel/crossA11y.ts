/**
 * UNREAL-X AI-17 · 族0164 跨窗输入 2.0 + 族0165 输入无障碍 2.0（X04076~X04125）。
 *
 * 跨窗输入：焦点接力/输入路由仲裁/粘贴板跨窗/输入中的窗口归属。
 * 输入无障碍：粘滞键/筛选键/切换键/按键回显/单手模式的纯逻辑模型。
 */

/* ============================== 族0164 跨窗输入 2.0 ============================== */

/** 跨窗输入路由五档（决定按键归属窗口的仲裁策略）。 */
export const CROSS_WINDOW_PROFILES = [
  { id: "strict", name: "严格", reroute: false, hoverType: false, pasteTarget: "focus" },
  { id: "follow", name: "跟随", reroute: true, hoverType: false, pasteTarget: "focus" },
  { id: "balanced", name: "均衡", reroute: true, hoverType: true, pasteTarget: "hover" },
  { id: "broadcast", name: "广播", reroute: true, hoverType: true, pasteTarget: "all" },
  { id: "scripted", name: "编排", reroute: true, hoverType: false, pasteTarget: "script" },
] as const;
export type CrossWindowProfileId = (typeof CROSS_WINDOW_PROFILES)[number]["id"];
export const DEFAULT_CROSS_WINDOW_ID: CrossWindowProfileId = "balanced";

export function findCrossWindow(id: string): (typeof CROSS_WINDOW_PROFILES)[number] {
  return CROSS_WINDOW_PROFILES.find((p) => p.id === id) ?? CROSS_WINDOW_PROFILES[2]!;
}

export type InputRoute = "focus" | "hover" | "dropped";

/** 跨窗输入路由器：按键按档位决定归属（焦点窗 / 悬停窗 / 丢弃）。 */
export class CrossWindowRouter {
  profileId: CrossWindowProfileId;
  focusWindow: string | null = null;
  hoverWindow: string | null = null;
  routed: { atMs: number; target: InputRoute; window: string | null }[] = [];
  clamped = 0;

  constructor(profileId: string = DEFAULT_CROSS_WINDOW_ID) {
    const known = CROSS_WINDOW_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findCrossWindow(profileId).id as CrossWindowProfileId;
  }

  setFocus(w: string | null): void {
    this.focusWindow = w;
  }

  setHover(w: string | null): void {
    this.hoverWindow = w;
  }

  /** 路由一次按键输入。 */
  route(atMs: number): { target: InputRoute; window: string | null } {
    const p = findCrossWindow(this.profileId);
    let target: InputRoute = "focus";
    let win = this.focusWindow;
    if (p.hoverType && this.hoverWindow && this.hoverWindow !== this.focusWindow) {
      target = "hover";
      win = this.hoverWindow;
    }
    if (!win) {
      target = "dropped";
      win = null;
    }
    this.routed.push({ atMs, target, window: win });
    if (this.routed.length > 512) this.routed.shift(); // 环形裁剪
    return { target, window: win };
  }

  /** 粘贴目标解析（档位决定 focus/hover/all/script）。 */
  pasteTargets(): string[] {
    const p = findCrossWindow(this.profileId);
    switch (p.pasteTarget) {
      case "focus": return this.focusWindow ? [this.focusWindow] : [];
      case "hover": return this.hoverWindow ? [this.hoverWindow] : [];
      case "all": {
        const s = new Set<string>();
        if (this.focusWindow) s.add(this.focusWindow);
        if (this.hoverWindow) s.add(this.hoverWindow);
        return [...s];
      }
      case "script": return [];
      default: return [];
    }
  }

  /** 焦点接力：A→B 的切换时延统计（ms），用于跨窗切换成本画像。 */
  focusSwitchCost(): number | null {
    let prev: { atMs: number; window: string | null } | null = null;
    for (const r of this.routed) {
      if (prev && prev.window !== r.window) return r.atMs - prev.atMs;
      prev = r;
    }
    return null;
  }

  reset(): void {
    this.routed = [];
    this.clamped = 0;
  }
}

/* ============================== 族0165 输入无障碍 2.0 ============================== */

/** 输入无障碍五档（粘滞/筛选/回显强度的组合档）。 */
export const INPUT_A11Y_PROFILES = [
  { id: "off", name: "标准", sticky: false, bounceMs: 0, echo: false },
  { id: "gentle", name: "温和", sticky: true, bounceMs: 0, echo: false },
  { id: "balanced", name: "均衡", sticky: true, bounceMs: 80, echo: true },
  { id: "strong", name: "强化", sticky: true, bounceMs: 150, echo: true },
  { id: "max", name: "最大", sticky: true, bounceMs: 250, echo: true },
] as const;
export type InputA11yProfileId = (typeof INPUT_A11Y_PROFILES)[number]["id"];
export const DEFAULT_INPUT_A11Y_ID: InputA11yProfileId = "balanced";

export function findInputA11y(id: string): (typeof INPUT_A11Y_PROFILES)[number] {
  return INPUT_A11Y_PROFILES.find((p) => p.id === id) ?? INPUT_A11Y_PROFILES[2]!;
}

/** 粘滞键状态机：修饰键按一次锁定、再按释放（Shift/Ctrl/Alt/Win）。 */
export class StickyKeys {
  enabled: boolean;
  locked = new Set<string>();
  latched = new Set<string>();
  clamped = 0;

  constructor(enabled: boolean) {
    this.enabled = enabled;
  }

  /** 按下修饰键。返回 true 表示事件被粘滞层消化（不透传组合）。 */
  press(mod: string): boolean {
    if (!this.enabled) return false;
    if (!["shift", "ctrl", "alt", "win"].includes(mod)) {
      this.clamped += 1;
      return false;
    }
    if (this.locked.has(mod)) {
      this.locked.delete(mod);
      return true;
    }
    if (this.latched.has(mod)) {
      this.latched.delete(mod);
      this.locked.add(mod);
    } else {
      this.latched.add(mod);
    }
    return true;
  }

  /** 普通键按下：消费所有已锁/已粘修饰并返回组合键描述。 */
  commit(key: string): string {
    const mods = [...this.locked, ...this.latched].sort();
    this.latched.clear();
    return mods.length ? `${mods.join("+")}+${key}` : key;
  }

  /** 连按 5 次 Shift 触发粘滞键询问（系统惯例）。 */
  static shiftAsk(count: number): boolean {
    return count >= 5;
  }
}

/** 筛选键（防抖）：忽略 bounceMs 内的重复按下。 */
export class BounceFilter {
  bounceMs: number;
  lastAt = -Infinity;
  rejected = 0;
  accepted = 0;

  constructor(bounceMs: number) {
    this.bounceMs = Math.max(0, Math.min(1000, bounceMs));
  }

  feed(atMs: number): boolean {
    if (atMs < this.lastAt + this.bounceMs) {
      this.rejected += 1;
      return false;
    }
    this.lastAt = atMs;
    this.accepted += 1;
    return true;
  }
}

/** 按键回显：把按键转成读屏友好文案。 */
export function keyEcho(key: string, mods: string[] = []): string {
  const map: Record<string, string> = {
    shift: "上档", ctrl: "控制", alt: "替换", win: "系统",
    enter: "回车", esc: "退出", tab: "制表", space: "空格",
    up: "上箭头", down: "下箭头", left: "左箭头", right: "右箭头",
  };
  const parts = mods.map((m) => map[m.toLowerCase()] ?? m);
  parts.push(map[key.toLowerCase()] ?? key.toUpperCase());
  return parts.join("，");
}

/** 单手模式：左手/右手镜像映射（按键 → 镜像键）。 */
const HAND_MIRROR: Record<string, string> = {
  // 左手模式：右手区键镜像到左手可达键
  y: "u", u: "y", h: "g", g: "h", n: "b", b: "n", p: "o", o: "p", l: "k", k: "l",
};

export function handMirror(key: string, side: "left" | "right"): string {
  if (side === "left") return HAND_MIRROR[key.toLowerCase()] ?? key;
  const rev = Object.entries(HAND_MIRROR).find(([, v]) => v === key.toLowerCase());
  return rev ? rev[0] : key;
}
