/**
 * J 鼠标域 · F605 滚轮刻度语义 + F606 倾斜滚轮支持 + F612 滚轮自适应增益 + F618 滚轮穿透开关。
 *
 * F605——滚轮「咔哒感」分应用可调：文档/代码类默认逐档（一格 3 行、干脆无惯性
 * 余韵），浏览器/长列表默认平滑连滚（F204 惯性接管）；应用可覆盖全局默认；
 * 全局三档（总是逐档/总是平滑/按应用默认）。
 *
 * F606——带倾斜轮的鼠标左/右倾斜映射为水平滚动：倾斜一次滚 3 列、按住连续滚、
 * 无倾斜轮设备映射到 Shift+滚轮（等效入口）；能力检测自动显隐设置项。
 *
 * F612——低速一格 3 行忠实逐档，高速连滚自动加大每格行数（平滑介入、最高
 * 12 行/格）+ F204 惯性衔接；增益曲线固定默认不开放；逐档应用（F605 覆盖档）豁免。
 *
 * F618——悬停在「不滚动区域」（纯装饰浮层）上时滚轮作用于其下滚动容器
 * （滚动意图穿透）；可关（恢复「滚哪算哪」字面语义）；可滚动弹层豁免。
 *
 * 判据锚点：
 * - 三类应用默认档 / 覆盖优先级（应用>全局）/ 3 行每格对拍 → resolveWheelMode()
 * - 倾斜/按住/连发三事件 → TiltController；Shift+滚轮等效映射 → tiltFromShiftWheel()
 * - 低速/高速两态（3 行 vs 12 行封顶）/ 速度连续性 → WheelGain
 * - 穿透/豁免/关闭三态 / 白名单类型表 → resolveWheelTarget()
 */

import { evalGainCurve } from "./wheelcal";

/* ------------------------------- F605 刻度语义 ------------------------------- */

export type WheelMode = "notch" | "smooth" | "per-app";

export const WHEEL_MODES: { id: WheelMode; name: string; desc: string }[] = [
  { id: "notch", name: "总是逐档", desc: "每一格都是干脆的整档，无惯性余韵。" },
  { id: "smooth", name: "总是平滑", desc: "惯性接管，长列表丝滑连滚。" },
  { id: "per-app", name: "按应用默认", desc: "文档逐档、浏览器平滑，两派各得其所。" },
];

/** 应用类目 → 默认档（终端 F467 独立档先例在此统一归口登记）。 */
export const APP_CLASS_DEFAULT: Record<string, WheelMode> = {
  document: "notch",
  code: "notch",
  terminal: "notch",
  browser: "smooth",
  list: "smooth",
  image: "smooth",
};

export interface WheelNotchConfig {
  mode: WheelMode;
  /** 应用 id → 覆盖档（应用>全局判据）。 */
  overrides: Record<string, WheelMode>;
  linesPerNotch: number;
}

/**
 * 判据：覆盖优先级（应用>全局）。解析顺序：应用覆盖 → 类目默认 → 全局档。
 * appClass 缺省按 "document" 处理（逐档是文档党的多数派）。
 */
export function resolveWheelMode(cfg: WheelNotchConfig, appId: string, appClass?: string): WheelMode {
  const override = cfg.overrides[appId];
  if (override) return override;
  if (cfg.mode === "per-app") return APP_CLASS_DEFAULT[appClass ?? "document"] ?? "notch";
  return cfg.mode;
}

/** 逐档行数：一格 3 行对拍 Windows（1..12 钳制）。 */
export function notchLines(linesPerNotch: number): number {
  return Math.max(1, Math.min(12, Math.round(linesPerNotch) || 3));
}

/* ------------------------------- F606 倾斜滚轮 ------------------------------- */

export interface TiltWheelConfig {
  enabled: boolean;
  colsPerNotch: number;
  repeatDelayMs: number;
  repeatRateMs: number;
  /** 设备能力检测结果（能力检测显隐判据：无硬件时面板隐藏本节）。 */
  hasTilt: boolean;
}

/** 倾斜事件 → 水平列数（3 列/档对拍垂直对称）。 */
export function tiltCols(colsPerNotch: number): number {
  return Math.max(1, Math.min(12, Math.round(colsPerNotch) || 3));
}

/**
 * Shift+滚轮 → 倾斜等效映射（键盘党的横向滚动入口——无倾斜轮设备的等效语义）。
 * 返回水平滚动列数与方向。
 */
export function tiltFromShiftWheel(deltaY: number, colsPerNotch: number): { dir: -1 | 1; cols: number } {
  const cols = tiltCols(colsPerNotch);
  // 滚轮向下（deltaY>0）= 向右滚（Windows 语义同款）。
  return { dir: deltaY >= 0 ? 1 : -1, cols };
}

/**
 * 按住倾斜连发控制器：首档立即 → 延迟 repeatDelayMs → 以 repeatRateMs 连发
 * （与垂直连滚加速曲线一致的对称哲学）。
 */
export class TiltController {
  private timer: number | null = null;
  private rateTimer: number | null = null;

  constructor(
    private readonly onStep: (dir: -1 | 1) => void,
    private cfg: () => TiltWheelConfig,
  ) {}

  /** 倾斜按下。 */
  press(dir: -1 | 1): void {
    this.release();
    const cfg = this.cfg();
    if (!cfg.enabled) return;
    this.onStep(dir);
    this.timer = window.setTimeout(() => {
      this.rateTimer = window.setInterval(() => this.onStep(dir), Math.max(16, cfg.repeatRateMs));
    }, Math.max(0, cfg.repeatDelayMs));
  }

  /** 松开倾斜。 */
  release(): void {
    if (this.timer !== null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.rateTimer !== null) {
      window.clearInterval(this.rateTimer);
      this.rateTimer = null;
    }
  }

  dispose(): void {
    this.release();
  }
}

/* ------------------------------- F612 自适应增益 ------------------------------- */

export interface WheelGainConfig {
  enabled: boolean;
  minLines: number;
  maxLines: number;
  /** 增益平滑介入时间常数（ms）——速度连续性（无跳变感）。 */
  accelMs: number;
  /**
   * 标定曲线（v5 滚轮标定向导采纳后写回——lines = k·pace^b 覆盖出厂线性；
   * 不写 = 出厂线性。判据纪律：标定是专家捷径，默认参数不进配置）。
   */
  calib?: { k: number; b: number };
}

/**
 * F612 自适应增益：跟踪近期滚轮节奏（档/秒）平滑抬升每格行数。
 * - 低速（<3 档/秒）忠实 minLines（3 行）；
 * - 高速（≥8 档/秒）趋近 maxLines（12 行）封顶；
 * - 增益值以指数平滑逼近目标（速度连续性判据），不跳变。
 * 与 F605 互斥边界：逐档应用（resolveWheelMode()==="notch"）豁免本增益。
 */
export class WheelGain {
  private ema = 0; // 平滑后的档/秒
  private level = 0; // 平滑后的输出行数（速度连续性的第二重保障）
  private lastAt = -Infinity;
  private lastSign = 0; // 滚动方向（v6：翻转防爬升）

  constructor(private cfg: () => WheelGainConfig) {}

  reset(): void {
    this.ema = 0;
    this.level = 0;
    this.lastAt = -Infinity;
    this.lastSign = 0;
  }

  /**
   * 一次滚轮事件 → 本次应滚行数。
   * @param atMs 事件时刻
   * @param notchMode 当前应用的解析档（notch 档豁免增益——F605 互斥边界）
   * @param dirSign 滚动方向符号（v6：上下抖着滚时节奏 EMA 不该爬升——
   *        方向翻转即把节奏 EMA 腰斩，「抖着滚增益反而变大」的体感缺陷闭合）
   */
  feed(atMs: number, notchMode: boolean, dirSign = 0): number {
    const cfg = this.cfg();
    const min = Math.max(1, cfg.minLines);
    const max = Math.max(min, cfg.maxLines);
    if (!cfg.enabled || notchMode) {
      this.reset();
      return min;
    }
    if (this.lastAt !== -Infinity) {
      const dt = Math.max(1, atMs - this.lastAt);
      const rate = 1000 / dt; // 档/秒
      if (dirSign !== 0 && this.lastSign !== 0 && dirSign !== this.lastSign) {
        this.ema *= 0.5; // 方向翻转：节奏清半（抖滚不爬升）
      }
      this.ema = this.ema === 0 ? rate : this.ema + (rate - this.ema) * 0.35;
    }
    if (dirSign !== 0) this.lastSign = dirSign;
    this.lastAt = atMs;
    // 目标行数：标定曲线（幂律）优先，无标定走出厂线性 3..8 档/秒。
    // 输出行数再经 EMA 平滑——即使节奏突变，行数也连续逼近（增益介入无跳变感判据）。
    const calib = cfg.calib;
    const target = calib
      ? Math.max(min, Math.min(max, evalGainCurve(calib.k, calib.b, this.ema)))
      : min + (max - min) * Math.max(0, Math.min(1, (this.ema - 3) / 5));
    this.level = this.level === 0 ? min : this.level + (target - this.level) * 0.35;
    return Math.round(this.level * 100) / 100;
  }
}

/* ------------------------------- F618 滚轮穿透 ------------------------------- */

export interface PassthroughConfig {
  enabled: boolean;
  /** 穿透白名单按控件类型登记（数据属性 data-wheel="..."）： */
  exemptTypes: string[];
}

/** 类型语义表（审计口径）：以下类型默认豁免穿透——该滚的还是滚它自己。 */
export const PASSTHROUGH_TYPE_SEMANTICS: { type: string; meaning: string }[] = [
  { type: "scrollable-layer", meaning: "可滚动弹层：自己能滚就滚自己（豁免穿透）。" },
  { type: "select", meaning: "下拉选项列表：长下拉自动豁免。" },
  { type: "menu", meaning: "菜单/列表浮层：滚轮用于换选不用于穿透。" },
  { type: "decor", meaning: "纯装饰浮层：穿透到下层滚动容器。" },
];

/**
 * 穿透判定三态（F618 判据原文：穿透/豁免/关闭三态用例）：
 * - 关闭：滚哪算哪（字面语义），返回命中元素本身；
 * - 豁免：命中元素或其祖先声明了 exemptTypes 中的类型 → 滚它自己；
 * - 穿透：命中纯装饰层（data-wheel="decor"）→ 沿 DOM 找第一个可滚祖先。
 * 返回 { target, passthrough }；找不到可滚祖先时退回命中元素（零死胡同）。
 */
export function resolveWheelTarget(
  hit: Element | null,
  cfg: PassthroughConfig,
): { target: Element; passthrough: boolean } {
  if (!hit) throw new Error("[mouse-j1:F618] resolveWheelTarget 需要命中元素");
  if (!cfg.enabled) return { target: hit, passthrough: false };
  // 豁免检测：命中链上任何一层声明了豁免类型。
  let cur: Element | null = hit;
  while (cur) {
    const t = cur.getAttribute?.("data-wheel");
    if (t && cfg.exemptTypes.includes(t)) return { target: cur, passthrough: false };
    cur = cur.parentElement;
  }
  // 穿透检测：命中链上是装饰层 → 找第一个可滚祖先。
  let deco: Element | null = null;
  cur = hit;
  while (cur) {
    if (cur.getAttribute?.("data-wheel") === "decor") {
      deco = cur;
      break;
    }
    cur = cur.parentElement;
  }
  if (!deco) return { target: hit, passthrough: false };
  let node: HTMLElement | null = deco.parentElement;
  while (node) {
    const style = node.ownerDocument.defaultView?.getComputedStyle(node);
    if (style && (style.overflowY === "auto" || style.overflowY === "scroll")) {
      return { target: node, passthrough: true };
    }
    node = node.parentElement;
  }
  return { target: hit, passthrough: false };
}
