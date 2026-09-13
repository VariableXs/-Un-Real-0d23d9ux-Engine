/**
 * UNREAL-X AI-02 · 族0020 彩蛋层（X00451 档 · 关机/唤醒仪式彩蛋）。
 *
 * 彩蛋目录 + 确定性触发 + 总开关（可关闭，默认关）；
 * 不损主线体验：触发只影响仪式附加行，不改流程结果。纯逻辑模块。
 */

export interface PowerEgg {
  id: string;
  name: string;
  /** 触发场合。 */
  when: "shutdown" | "wake" | "both";
  /** 触发条件：仪式连续完成次数 ≥ threshold。 */
  threshold: number;
  /** 附加行（仪式收尾追加的一句话）。 */
  line: string;
}

export const POWER_EGGS: readonly PowerEgg[] = [
  { id: "night", name: "晚安", when: "shutdown", threshold: 1, line: "灯已熄，档已存——明天见。" },
  { id: "count", name: "计数者", when: "both", threshold: 5, line: "这是我们的第 5 次告别与重逢。" },
  { id: "dawn", name: "晨光", when: "wake", threshold: 1, line: "屏亮如晨，会话如约归来。" },
  { id: "veteran", name: "老兵", when: "both", threshold: 20, line: "二十次仪式，一次不落。" },
  { id: "eclipse", name: "月食", when: "shutdown", threshold: 10, line: "屏幕暗下去的弧线，像一次月食。" },
] as const;

export const EGG_TRIGGER_MAX = 99;

/** 彩蛋管理器（总开关默认关 = 现状）。 */
export class EggKeeper {
  enabled = false;
  /** 关机/唤醒累计完成次数。 */
  shutdowns = 0;
  wakes = 0;
  clamped = 0;
  /** 用户已看过/屏蔽的单个彩蛋 id。 */
  dismissed = new Set<string>();

  /** 登记一次仪式完成。 */
  celebrate(kind: "shutdown" | "wake"): void {
    const n = kind === "shutdown" ? "shutdowns" : "wakes";
    this[n] = Math.min(EGG_TRIGGER_MAX, this[n] + 1);
  }

  /** 本次仪式应触发的彩蛋（至多 1 条；关闭/已屏蔽/阈值未到 → 空串）。 */
  lineFor(kind: "shutdown" | "wake"): string {
    if (!this.enabled) return "";
    const count = kind === "shutdown" ? this.shutdowns : this.wakes;
    const egg = POWER_EGGS.find((e) => {
      if (e.when !== kind && e.when !== "both") return false;
      if (this.dismissed.has(e.id)) return false;
      return count >= e.threshold;
    });
    return egg ? egg.line : "";
  }

  /** 屏蔽单个彩蛋（可解释、可恢复）。 */
  dismiss(id: string): boolean {
    if (!POWER_EGGS.some((e) => e.id === id)) {
      this.clamped += 1;
      return false;
    }
    this.dismissed.add(id);
    return true;
  }

  /** 恢复被屏蔽的彩蛋。 */
  restore(id: string): boolean {
    if (!this.dismissed.delete(id)) {
      this.clamped += 1;
      return false;
    }
    return true;
  }

  /** 净身：回默认（关闭 + 计数清零 + 屏蔽清空）。 */
  reset(): void {
    this.enabled = false;
    this.shutdowns = 0;
    this.wakes = 0;
    this.dismissed.clear();
    this.clamped = 0;
  }
}
