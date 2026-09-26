/**
 * J 鼠标域 · F601 指针速度曲线谱 + F602 慢速微调模式。
 *
 * F601——指针加速不是开关二选一而是曲线族：
 * - linear 线性（1:1，电竞与设计人群）；
 * - classic 经典加速（Windows 增益曲线复刻）；
 * - soft 缓启动（低速精修、高速飞移）；
 * - custom 自定义贝塞尔（两控制点拖拽）。
 * 曲线只作用于增益映射、不改变底层管线：F250 的「关加速」档与 linear 曲线
 * 是同一事实源——开关即曲线选择（同源一致性判据）。
 *
 * F602——按住修饰键（默认 Shift）时指针速度降到常速的 10%（步进式降档而非
 * 平滑减速——精确落点的确定性优先）；与 F250 曲线叠加的正确性：曲线作用于
 * 常速档，微调档恒定增益（不经过曲线）。
 *
 * 判据锚点：
 * - 三曲线增益对拍表（输入-输出位移 20 点采样）→ gainTable20()
 * - 贝塞尔编辑器拖拽即时预览 → evalBezier()（纯函数，面板逐帧重画）
 * - 修饰键冲突审计登记 → SLOW_TUNE_KEY 登记接口（与 F244 同源字段）
 */

import type { J1Section } from "./j1store";

export type CurveId = "linear" | "classic" | "soft" | "custom";

/** 曲线族登记（说明句三件套：名称/一句话说明/调节控件由面板同源渲染）。 */
export const CURVE_LIBRARY: { id: CurveId; name: string; desc: string }[] = [
  { id: "linear", name: "线性 1:1", desc: "零增益零迟疑，电竞与设计人群的原味手感。" },
  { id: "classic", name: "经典加速", desc: "Windows 增益曲线复刻，迁移肌肉记忆零差异。" },
  { id: "soft", name: "缓启动", desc: "低速精修高速飞移，办公党的稳与快兼得。" },
  { id: "custom", name: "自定义曲线", desc: "两控制点贝塞尔自绘，拖拽即时预览。" },
];

export type SlowTuneKey = "shift" | "ctrl" | "alt" | "capslock";

/** F602 修饰键档（冲突审计登记进 F244 的注册行由此生成）。 */
export const SLOW_TUNE_KEYS: { id: SlowTuneKey; name: string }[] = [
  { id: "shift", name: "Shift" },
  { id: "ctrl", name: "Ctrl" },
  { id: "alt", name: "Alt" },
  { id: "capslock", name: "Caps Lock" },
];

/** F602 降速档：5% / 10% / 20%（三档切换即时）。 */
export const SLOW_TUNE_RATIOS = [0.05, 0.1, 0.2] as const;

export interface CurveConfig {
  id: CurveId;
  /** 自定义贝塞尔两控制点（0..1 归一）。 */
  cp1x: number;
  cp1y: number;
  cp2x: number;
  cp2y: number;
  /** 全局灵敏度倍率（对拍表按 sens=1.0 出表）。 */
  sens: number;
}

/** 三曲线 + 自定义增益采样：位移 a(px) → 增益倍率。 */
export function gainAt(curve: CurveId, a: number, cfg?: CurveConfig): number {
  const d = Math.max(0, Math.min(4096, a));
  switch (curve) {
    case "linear":
      return 1;
    case "classic": {
      // Windows 经典增益复刻：≤8px 直通，其后分段缓增（对拍 Windows 采样点）。
      if (d <= 8) return 1;
      if (d <= 32) return 1 + ((d - 8) / 24) * 0.25;
      return 1.25 + Math.min(1, (d - 32) / 128) * 0.75;
    }
    case "soft": {
      // 缓启动：低速 0.6 倍精修，高速 1.6 倍飞移，S 形过渡。
      const t = Math.min(1, d / 96);
      return 0.6 + 1.0 * (t * t * (3 - 2 * t));
    }
    case "custom": {
      const c = cfg ?? { id: "custom", cp1x: 0.35, cp1y: 0.55, cp2x: 0.7, cp2y: 1.0, sens: 1 };
      // 贝塞尔以输入位移归一为参数 t，输出归一为增益 0.5..2.0 区间。
      const t = Math.min(1, d / 128);
      return 0.5 + 1.5 * evalBezier(t, c.cp1x, c.cp1y, c.cp2x, c.cp2y);
    }
    default:
      return 1;
  }
}

/** 三次贝塞尔（两控制点）：y(t)。x 分量控制参数节奏。 */
export function evalBezier(t: number, x1: number, y1: number, x2: number, y2: number): number {
  const u = 1 - t;
  // 标准三次贝塞尔（端点 (0,0)-(1,1)，两控制点可调）。
  const y = 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t;
  // 控制点 x 参与 easing 重参数化：用 x(t) 反解近似（牛顿两步，精度足够预览）。
  const xx = 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t;
  if (Math.abs(xx - t) < 1e-3 || t <= 0 || t >= 1) return Math.max(0, Math.min(1, y));
  // 以 xx≈t 为目标做两步牛顿迭代找 t'。
  let tt = t;
  for (let i = 0; i < 2; i++) {
    const uu = 1 - tt;
    const cur = 3 * uu * uu * tt * x1 + 3 * uu * tt * tt * x2 + tt * tt * tt;
    const der = 3 * uu * uu * x1 + 6 * uu * tt * (x2 - x1) + 3 * tt * tt * (1 - x2);
    if (Math.abs(der) < 1e-6) break;
    tt -= (cur - t) / der;
    tt = Math.max(0, Math.min(1, tt));
  }
  const uu = 1 - tt;
  const y2v = 3 * uu * uu * tt * y1 + 3 * uu * tt * tt * y2 + tt * tt * tt;
  return Math.max(0, Math.min(1, y2v));
}

/**
 * 增益对拍表：输入-输出位移 20 点采样（F601 判据原文）。
 * 返回 20 行 [输入位移, 输出位移]（sens=1 口径）。
 */
export function gainTable20(curve: CurveId, cfg?: CurveConfig): { inPx: number; outPx: number }[] {
  const rows: { inPx: number; outPx: number }[] = [];
  for (let i = 1; i <= 20; i++) {
    const inPx = i * 8; // 8..160px 采样带覆盖精修/日常/甩动三域
    rows.push({ inPx, outPx: Math.round(inPx * gainAt(curve, inPx, cfg) * 100) / 100 });
  }
  return rows;
}

/** 曲线作用于原始位移：sign 保真 + 增益映射 + 灵敏度。 */
export function applyCurve(dx: number, dy: number, cfg: CurveConfig): { x: number; y: number } {
  const g = gainAt(cfg.id, Math.hypot(dx, dy), cfg) * (cfg.sens || 1);
  return { x: dx * g, y: dy * g };
}

/* ------------------------------- F602 慢速微调 ------------------------------- */

export interface SlowTuneConfig {
  enabled: boolean;
  ratio: number;
  key: SlowTuneKey;
}

/**
 * F602 判定：修饰键按住即恒定增益（不经过曲线——与 F250 曲线叠加正确性判据：
 * 曲线作用于常速档、微调档恒定增益）。
 */
export function slowTuneGain(
  active: boolean,
  cfg: SlowTuneConfig,
): number | null {
  if (!cfg.enabled || !active) return null;
  const r = SLOW_TUNE_RATIOS.includes(cfg.ratio as (typeof SLOW_TUNE_RATIOS)[number]) ? cfg.ratio : 0.1;
  return r;
}

/**
 * F602 修饰键冲突审计登记行（F244 注册表同源字段）：
 * 登记即声明占用——审计表据此裁决与粘滞键等无障碍组合的冲突。
 */
export function slowTuneRegistryRow(cfg: SlowTuneConfig): {
  key: string; scope: string; action: string; source: string; conflictsWith: string[];
} {
  const keyName = SLOW_TUNE_KEYS.find((k) => k.id === cfg.key)?.name ?? "Shift";
  return {
    key: keyName,
    scope: "system.pointer",
    action: "慢速微调（按住降速至 " + Math.round(cfg.ratio * 100) + "%）",
    source: "F602",
    // 已知共占方：粘滞键（五下 Shift）、筛选键（忽略重复击键）——审计表裁决。
    conflictsWith: ["粘滞键(5xShift)", "筛选键"],
  };
}

/** J1 分节名到 F 编号的对照（面板说明句与台账共用）。 */
export const SECTION_F: Record<J1Section, string> = {
  curve: "F601",
  slowTune: "F602",
  liftFilter: "F603",
  autoscroll: "F604",
  wheelNotch: "F605",
  tiltWheel: "F606",
  seamGuard: "F607",
  magnet: "F608",
  dragScroll: "F609",
  hoverTiming: "F610",
  tremor: "F611",
  wheelGain: "F612",
  screenMemory: "F613",
  devices: "F614",
  sideButtons: "F615",
  appProfiles: "F616",
  gestures: "F617",
  passthrough: "F618",
  longPress: "F619",
  overlay: "F620",
};
