/**
 * UNREAL-X AI-01 · 族0003 启动修复工坊（X00051~X00075）。
 *
 * 修复策略执行 + 进度：五种修复策略（档位矩阵 ≥5 档），断点续作、
 * 错误叙事（禁裸报错）、边界钳制、资源降级守护、回滚净身。
 * 纯逻辑模块，React 组件只消费这里的数据。
 */

/** 修复策略目录（档位矩阵 5 档独立可交付）。 */
export const REPAIR_STRATEGIES = [
  { id: "verify-loader", name: "引导器校验", steps: 4, severity: 1 },
  { id: "rebuild-bcd", name: "启动配置重建", steps: 6, severity: 2 },
  { id: "repair-entry", name: "启动项修复", steps: 3, severity: 1 },
  { id: "restore-snapshot", name: "快照回滚", steps: 5, severity: 3 },
  { id: "factory-reset", name: "出厂净身", steps: 7, severity: 4 },
] as const;
export type RepairStrategyId = (typeof REPAIR_STRATEGIES)[number]["id"];

/** 未知/非法策略时的默认档。 */
export const DEFAULT_STRATEGY: RepairStrategyId = "repair-entry";

export function findStrategy(id: string): (typeof REPAIR_STRATEGIES)[number] {
  return REPAIR_STRATEGIES.find((s) => s.id === id) ?? REPAIR_STRATEGIES[2];
}

export interface RepairLogEntry {
  step: number;
  strategy: RepairStrategyId;
  ok: boolean;
  /** 失败时给出错误码（叙事映射在 bootFailNarrative）。 */
  code?: string;
}

export type RepairPhase = "idle" | "running" | "paused" | "done" | "failed";

/** 修复工坊执行器：真实推进逻辑（tick 驱动，可被 UI 按钮操控）。 */
export class RepairWorkshop {
  strategy: RepairStrategyId;
  phase: RepairPhase = "idle";
  step = 0;
  logs: RepairLogEntry[] = [];
  /** 最大重试：钳制 0~5。 */
  maxRetries: number;
  /** 资源降级守护：低配档每 tick 只推 1 步（否则 2 步）。 */
  lowPower: boolean;
  clamped = 0;

  constructor(strategy: string = DEFAULT_STRATEGY, maxRetries = 2, lowPower = false) {
    this.strategy = findStrategy(strategy).id as RepairStrategyId;
    this.maxRetries = Math.min(5, Math.max(0, Math.round(maxRetries) || 0));
    this.lowPower = lowPower;
  }

  private get total(): number {
    return findStrategy(this.strategy).steps;
  }

  start(): void {
    this.phase = "running";
    if (this.step >= this.total) this.step = 0;
  }

  /** 中断续作：保留 step，只把 phase 拉回 running。 */
  resume(): boolean {
    if (this.phase !== "paused" && this.phase !== "failed") return false;
    this.phase = "running";
    return true;
  }

  pause(): void {
    if (this.phase === "running") this.phase = "paused";
  }

  /** 推进一 tick；随机失败由调用方注入（确定性测试：failOnStep >= 0）。 */
  tick(failOnStep = -1): RepairPhase {
    if (this.phase !== "running") return this.phase;
    const per = this.lowPower ? 1 : 2;
    for (let i = 0; i < per && this.step < this.total; i += 1) {
      this.step += 1;
      const failed = failOnStep >= 0 && this.step === failOnStep;
      this.logs.push({ step: this.step, strategy: this.strategy, ok: !failed, code: failed ? "BC-501" : undefined });
      if (failed) {
        this.phase = "failed";
        return this.phase;
      }
    }
    if (this.step >= this.total) this.phase = "done";
    return this.phase;
  }

  progress(): number {
    const t = this.total;
    return t === 0 ? 0 : Math.min(1, this.step / t);
  }

  /** 回滚净身：不留残档。 */
  reset(): void {
    this.phase = "idle";
    this.step = 0;
    this.logs = [];
  }
}

/** 进度百分比（0~100，整数）。 */
export function repairPercent(w: RepairWorkshop): number {
  return Math.round(w.progress() * 100);
}
