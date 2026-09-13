/**
 * UNREAL-X AI-02 · 族0017 性能仪表 2.0（X00376 档 · Variable 侧采集结构）。
 *
 * 电源/性能仪表采样结构：电量、CPU、热度、预估续航四通道；
 * 环形采样窗口、预算越线判定、摘要行渲染。纯逻辑模块。
 */

export const GAUGE_CHANNELS = ["battery", "cpu", "thermal", "runtime"] as const;
export type GaugeChannel = (typeof GAUGE_CHANNELS)[number];

/** 各通道健康区间（0~100 归一；runtime 为小时×10）。 */
export const GAUGE_BUDGETS: Record<GaugeChannel, { warn: number; bad: number; label: string; unit: string }> = {
  battery: { warn: 30, bad: 12, label: "电量", unit: "%" },
  cpu: { warn: 70, bad: 90, label: "处理器", unit: "%" },
  thermal: { warn: 70, bad: 88, label: "热度", unit: "%" },
  runtime: { warn: 40, bad: 15, label: "预估续航", unit: "0.1h" },
};

export const GAUGE_CAPACITY = 16;

export interface GaugeSample {
  stamp: number;
  /** 四通道 0~100 归一值（越界钳制）。 */
  values: Record<GaugeChannel, number>;
}

function clamp100(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(100, Math.max(0, Math.round(v)));
}

/** 性能仪表采样器。 */
export class PowerGauge {
  samples: GaugeSample[] = [];
  clamped = 0;

  /** 采样一次（非法通道值钳制 0~100）。 */
  sample(stamp: number, values: Partial<Record<GaugeChannel, number>>): GaugeSample {
    const s: GaugeSample = {
      stamp: Math.max(0, Math.round(stamp) || 0),
      values: {
        battery: clamp100(Number(values.battery ?? 100)),
        cpu: clamp100(Number(values.cpu ?? 0)),
        thermal: clamp100(Number(values.thermal ?? 0)),
        runtime: clamp100(Number(values.runtime ?? 100)),
      },
    };
    this.samples.unshift(s);
    if (this.samples.length > GAUGE_CAPACITY) this.samples.pop();
    return s;
  }

  /** 通道均值（无样本返回 0）。 */
  avg(ch: GaugeChannel): number {
    if (this.samples.length === 0) return 0;
    const acc = this.samples.reduce((a, s) => a + s.values[ch], 0);
    return Math.round(acc / this.samples.length);
  }

  /** 通道健康度：ok / warn / bad。 */
  health(ch: GaugeChannel): "ok" | "warn" | "bad" {
    const v = this.avg(ch);
    const b = GAUGE_BUDGETS[ch];
    const inverted = ch === "cpu" || ch === "thermal"; // 高 = 差
    if (inverted) return v >= b.bad ? "bad" : v >= b.warn ? "warn" : "ok";
    return v <= b.bad ? "bad" : v <= b.warn ? "warn" : "ok";
  }

  /** 任意通道越线（用于红点提醒）。 */
  degraded(): boolean {
    return GAUGE_CHANNELS.some((ch) => this.health(ch) !== "ok");
  }

  /** 摘要行（仪表卡片直接渲染）。 */
  caption(): string {
    return GAUGE_CHANNELS.map((ch) => {
      const b = GAUGE_BUDGETS[ch];
      const mark = this.health(ch) === "ok" ? "" : this.health(ch) === "warn" ? "!" : "!!";
      return `${b.label} ${this.avg(ch)}${b.unit}${mark}`;
    }).join(" · ");
  }

  /** 净身。 */
  reset(): void {
    this.samples = [];
    this.clamped = 0;
  }
}
