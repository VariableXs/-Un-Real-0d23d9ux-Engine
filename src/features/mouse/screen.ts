/**
 * J 鼠标域 · F607 跨屏接缝手感 + F613 指针跨屏落点记忆。
 *
 * F607——多屏接缝三件套：穿越无卡顿（零跳变由合成器帧同步保障，本模块管
 * 逻辑侧时序）、防勾绊护边（接缝两侧 4px 内停留 200ms 才放行穿越——垂直
 * 对不齐的屏幕不再让指针「卡在看不见的台阶」）、角落直通（屏幕四角 8px 内
 * 不受护边限制——去角落热区（F249）的人永远秒达）；护边开关独立、按屏对记忆。
 *
 * F613——每块屏记住指针最后停留位置：KVM 切换回来/锁屏唤醒/显示器输入源
 * 切回时指针落回该屏记忆点；记忆按显示器 EDID 指纹而非接口顺序（换线不乱）；
 * 只记位置不记其它状态；单屏用户功能自然休眠零存在感。
 *
 * 判据锚点：
 * - 4px/200ms 护边时序 → SeamGuard.feed()
 * - 四角豁免 → cornerExempt()
 * - 双屏记忆与恢复精度（<1px）→ ScreenMemory
 * - EDID 指纹识别（交换接口用例）→ 以 edidFingerprint 为键
 * - 单屏静默 → monitorCount <= 1 时全部直通
 */

export interface MonitorInfo {
  id: string;
  /** 全局桌面坐标（虚拟桌面坐标系）。 */
  x: number;
  y: number;
  width: number;
  height: number;
  /** EDID 指纹（厂商+型号+序列哈希）——记忆与档案都以它为键（换线不乱）。 */
  edidFingerprint: string;
  /** 缩放比（混合 DPI 场景）。 */
  scale: number;
}

export interface SeamGuardConfig {
  enabled: boolean;
  edgePx: number;
  dwellMs: number;
  cornerPx: 8;
}

/** 护边时序参数固定档（判据原文：4px/200ms；角豁免 8px）。 */
export const SEAM_GUARD_PRESET = { edgePx: 4, dwellMs: 200, cornerPx: 8 } as const;

/** 该点是否处于任一显示器的四角 8px 豁免区（角落直通判据）。 */
export function cornerExempt(monitors: MonitorInfo[], gx: number, gy: number, cornerPx = 8): boolean {
  for (const m of monitors) {
    const xs = [m.x, m.x + m.width];
    const ys = [m.y, m.y + m.height];
    for (const cx of xs) {
      for (const cy of ys) {
        if (Math.abs(gx - cx) <= cornerPx && Math.abs(gy - cy) <= cornerPx) return true;
      }
    }
  }
  return false;
}

/** 坐标是否落在指定屏内（含边界）。 */
export function monitorAt(monitors: MonitorInfo[], gx: number, gy: number): MonitorInfo | null {
  return monitors.find((m) => gx >= m.x && gx <= m.x + m.width && gy >= m.y && gy <= m.y + m.height) ?? null;
}

/**
 * 跨屏护边状态机（每个「屏对+边」独立计时——按屏对记忆判据）。
 * feed 每个指针移动事件调用：从 from 屏向 to 屏穿越、且落点在接缝 edgePx
 * 护边带内时，要求停留 dwellMs 才放行；护边等待中指针回到原屏内侧即解除。
 * 角落豁免点、单屏、护边关闭均直通。
 */
export class SeamGuard {
  private key: string | null = null;
  private since = 0;

  constructor(
    private readonly monitors: () => MonitorInfo[],
    private readonly cfg: () => SeamGuardConfig,
    /** 屏对覆盖解析（F607「按屏对记忆」判据的消费端，v4 接线）：返回 undefined=无覆盖走全局。 */
    private readonly pairOverride?: (pairKey: string) => boolean | undefined,
  ) {}

  reset(): void {
    this.key = null;
    this.since = 0;
  }

  /**
   * @returns "pass"=直接放行（跨屏）|"hold"=护边等待中（本次留在原屏）|"stay"=未跨屏
   */
  feed(gx: number, gy: number, atMs: number): "pass" | "hold" | "stay" {
    const cfg = this.cfg();
    const monitors = this.monitors();
    if (monitors.length <= 1 || !cfg.enabled) return "pass";
    const cur = monitorAt(monitors, gx, gy);
    if (!cur) return "pass";
    const corner = cornerExempt(monitors, gx, gy, cfg.cornerPx);
    // 找接缝：检查四方向是否存在相邻屏（间隙 ≤ 护边带宽视为接缝）。
    for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]] as const) {
      const nx = gx + dx * cfg.edgePx;
      const ny = gy + dy * cfg.edgePx;
      const next = monitorAt(monitors, nx, ny);
      if (!next || next.id === cur.id) continue;
      if (corner) return "pass"; // 四角豁免：秒达
      // F607 屏对覆盖：该屏对显式关护边 = 直通（覆盖 > 全局，与 F605 同构）。
      const pairOn = this.pairOverride?.(seamPairKey(cur.edidFingerprint, next.edidFingerprint)) ?? cfg.enabled;
      if (!pairOn) return "pass";
      const key = `${cur.id}>${next.id}@${dx},${dy}`;
      if (this.key !== key) {
        this.key = key;
        this.since = atMs;
        return "hold";
      }
      if (atMs - this.since >= cfg.dwellMs) {
        this.reset();
        return "pass";
      }
      return "hold";
    }
    this.reset();
    return "stay";
  }
}

/* ------------------------------- F613 跨屏落点记忆 ------------------------------- */

export interface ScreenMemoryConfig {
  enabled: boolean;
  /** EDID 指纹 → {x,y}（只记位置不记其它状态）。 */
  points: Record<string, { x: number; y: number }>;
}

const SCREEN_MEMORY_LIMIT = 8;

/**
 * 跨屏落点记忆（按 EDID 指纹为键）：
 * - remember()：指针每秒与跨屏瞬间记录（runtime 节流调用）；
 * - restore()：屏幕重新出现（KVM/锁屏唤醒/输入源切回）时取回记忆点，
 *   并钳制在该屏范围内（<1px 精度：存取不经过任何换算）。
 * 上限 8 块屏（超出淘汰最久——LRU 纪律）。
 */
export class ScreenMemory {
  constructor(private readonly cfg: () => ScreenMemoryConfig) {}

  remember(edid: string, gx: number, gy: number, monitors: MonitorInfo[]): void {
    if (!this.cfg().enabled || monitors.length <= 1) return; // 单屏静默
    const points = { ...this.cfg().points };
    points[edid] = { x: Math.round(gx), y: Math.round(gy) };
    const keys = Object.keys(points);
    if (keys.length > SCREEN_MEMORY_LIMIT) delete points[keys[0]!];
    j1Set("screenMemory", { points });
  }

  /** 恢复：返回钳制在屏内的记忆点；无记忆/超出则返回 null（调用方走 F513 涟漪兜底）。 */
  restore(edid: string, monitors: MonitorInfo[]): { x: number; y: number } | null {
    if (!this.cfg().enabled || monitors.length <= 1) return null;
    const p = this.cfg().points[edid];
    if (!p) return null;
    const m = monitors.find((x) => x.edidFingerprint === edid);
    if (!m) return null;
    return {
      x: Math.max(m.x, Math.min(m.x + m.width, p.x)),
      y: Math.max(m.y, Math.min(m.y + m.height, p.y)),
    };
  }

  /** 清空某块屏的记忆（面板管理入口——用户能看见能控制）。 */
  forget(edid: string): void {
    const points = { ...this.cfg().points };
    delete points[edid];
    j1Set("screenMemory", { points });
  }

  clearAll(): void {
    j1Set("screenMemory", { points: {} });
  }
}

/* ------------------------------- F607 分屏对护边配置 ------------------------------- */

export interface SeamPairConfig {
  /** 屏对键（按 EDID 有序拼接）→ 护边开关覆盖（「按屏对记忆」判据）。 */
  overrides: Record<string, { enabled: boolean }>;
}

/** 屏对键：两块屏的 EDID 指纹字典序拼接（A|B 与 B|A 同键——方向无关）。 */
export function seamPairKey(edidA: string, edidB: string): string {
  return [edidA, edidB].sort().join("|");
}

/** 该屏对的护边是否启用：屏对覆盖 > 全局开关。 */
export function seamGuardForPair(globalEnabled: boolean, overrides: SeamPairConfig["overrides"], edidA: string, edidB: string): boolean {
  const o = overrides[seamPairKey(edidA, edidB)];
  return o ? o.enabled : globalEnabled;
}

/** 设置屏对覆盖（面板入口）。 */
export function setSeamPairOverride(overrides: SeamPairConfig["overrides"], edidA: string, edidB: string, enabled: boolean): SeamPairConfig["overrides"] {
  return { ...overrides, [seamPairKey(edidA, edidB)]: { enabled } };
}

// 避免与 j1store 产生模块环：延迟动态取 set（store 侧不 import 本文件）。
import { j1Store } from "./j1store";
function j1Set(section: Parameters<typeof j1Store.set>[0], patch: Record<string, unknown>): void {
  j1Store.set(section, patch);
}
