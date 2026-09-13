/**
 * UNREAL-X AI-01 · 族0004 恢复环境重生（X00076~X00100，Variable 侧）。
 *
 * 快照 / 续作：启动快照登记（含半成品标记）、断点续作、降级守护、
 * 回滚净身。与内核侧 kernel/varix/src/bootchain/recovery.rs 的槽位
 * 语义对齐（4 槽、环形覆盖、generation 单调递增）。
 */

import { findNarrative } from "./bootFailNarrative";

export const SNAPSHOT_CAPACITY = 4;

export type RecoveryPhase = "idle" | "working" | "paused" | "done" | "failed";

export interface RecoverySnapshot {
  id: string;
  label: string;
  stage: number;
  offset: number;
  complete: boolean;
  generation: number;
}

export type RecoveryToolId = "shell" | "scan" | "rollback" | "rebuild" | "wipe";

/** 恢复工具清单（降级档裁剪用）。 */
export const RECOVERY_TOOLS: readonly { id: RecoveryToolId; name: string; tier: number }[] = [
  { id: "shell", name: "应急命令行", tier: 0 },
  { id: "scan", name: "盘体体检", tier: 1 },
  { id: "rollback", name: "快照回滚", tier: 1 },
  { id: "rebuild", name: "重建快照", tier: 2 },
  { id: "wipe", name: "出厂净身", tier: 2 },
];

export type DegradeLevel = 0 | 1 | 2;

/** 恢复环境重生器（Variable 侧状态机）。 */
export class RecoveryReborn {
  snapshots: RecoverySnapshot[] = [];
  phase: RecoveryPhase = "idle";
  generation = 0;
  /** 降级档 0 全量 / 1 精简 / 2 最小。 */
  degrade: DegradeLevel = 0;
  clamped = 0;
  /** 最近一次失败的错误码（叙事映射）。 */
  lastCode: string | null = null;

  /** 登记快照；容量 4，满则挤掉最旧（环形覆盖）。 */
  snapshot(label: string, stage: number, offset: number, complete: boolean): RecoverySnapshot {
    this.generation += 1;
    const snap: RecoverySnapshot = {
      id: `rs-${String(this.generation).padStart(3, "0")}`,
      label: label.slice(0, 24),
      stage: Math.max(0, Math.min(4, Math.round(stage) || 0)),
      offset: Math.max(0, Math.min(0xffffffff, Math.round(offset) || 0)),
      complete,
      generation: this.generation,
    };
    this.snapshots.push(snap);
    while (this.snapshots.length > SNAPSHOT_CAPACITY) this.snapshots.shift();
    return snap;
  }

  /** 最新半成品快照（续作点）。 */
  resumePoint(): RecoverySnapshot | null {
    for (let i = this.snapshots.length - 1; i >= 0; i -= 1) {
      const s = this.snapshots[i];
      if (s && !s.complete) return s;
    }
    return null;
  }

  /** 一键续作：把续作点标记为完成并进入 working。 */
  resume(): boolean {
    const p = this.resumePoint();
    if (!p) {
      this.clamped += 1;
      return false;
    }
    p.complete = true;
    this.phase = "working";
    return true;
  }

  /** 设置降级档：越界钳制到 0~2。 */
  setDegrade(level: number): DegradeLevel {
    const v = Math.max(0, Math.min(2, Math.round(level) || 0)) as DegradeLevel;
    if (v !== level) this.clamped += 1;
    this.degrade = v;
    return v;
  }

  /** 当前降级档可用的工具（最小档只留 tier 0，精简 ≤ tier 1）。 */
  availableTools(): typeof RECOVERY_TOOLS {
    const maxTier = this.degrade === 0 ? 2 : this.degrade === 1 ? 1 : 0;
    return RECOVERY_TOOLS.filter((t) => t.tier <= maxTier);
  }

  /** 执行失败登记：接族0006 叙事。 */
  fail(code: string): string {
    this.lastCode = code;
    this.phase = "failed";
    return findNarrative(code).title;
  }

  /** 回滚净身：不留残档、不残留注册项。 */
  wipe(): void {
    this.snapshots = [];
    this.phase = "idle";
    this.lastCode = null;
  }
}

/** 快照摘要行（UI 直读）。 */
export function snapshotCaption(s: RecoverySnapshot): string {
  return `${s.id} ${s.label || "未命名"} · 阶段${s.stage} · 偏移${s.offset} · ${s.complete ? "完整" : "半成品"}`;
}
