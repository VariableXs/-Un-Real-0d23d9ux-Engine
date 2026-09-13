/**
 * UNREAL-X AI-02 · 族0011 关机重启仪式 2.0 + 族0012 睡眠唤醒剧场 2.0（X00251~X00300）。
 *
 * 仪式流程逻辑：阶段编排（tick 驱动）、进度、可取消/可暂停续作、
 * 5 档档位矩阵、非法输入钳制、错误叙事码（PW-xxx，禁裸报错）、
 * 快照导出/导入（跨版本携带）。复用 bootPacing 的配速语言。
 * 纯逻辑模块，React 组件只消费这里的数据。
 */

/* ============================== 族0011 关机/重启仪式 2.0 ============================== */

/** 仪式种类。 */
export type CeremonyKind = "shutdown" | "restart" | "signout";

/** 仪式五档档位矩阵（≥5 档独立可交付；默认 = balanced 不改既有手感）。 */
export const CEREMONY_PROFILES = [
  { id: "instant", name: "直通", stages: 2, motionLevel: 0, allowEgg: false },
  { id: "brisk", name: "轻快", stages: 3, motionLevel: 1, allowEgg: false },
  { id: "balanced", name: "均衡", stages: 4, motionLevel: 2, allowEgg: true },
  { id: "cinematic", name: "影院", stages: 6, motionLevel: 3, allowEgg: true },
  { id: "farewell", name: "告别式", stages: 8, motionLevel: 3, allowEgg: true },
] as const;
export type CeremonyProfileId = (typeof CEREMONY_PROFILES)[number]["id"];
export const DEFAULT_CEREMONY_ID: CeremonyProfileId = "balanced";

export function findCeremony(id: string): (typeof CEREMONY_PROFILES)[number] {
  return CEREMONY_PROFILES.find((p) => p.id === id) ?? CEREMONY_PROFILES[2]!;
}

/** 仪式阶段目录（告别式 8 阶段为全集）。 */
export const CEREMONY_STAGES = [
  "收束窗口", "保存会话", "休眠档案", "熄灭氛围",
  "致谢演出", "卸载服务", "落幕", "断电执念",
] as const;

export type CeremonyPhase = "idle" | "running" | "paused" | "done" | "canceled" | "failed";

export interface CeremonyLog {
  stage: number;
  ok: boolean;
  /** 失败叙事码（映射在下方 NARRATIVES）。 */
  code?: string;
}

/** 仪式错误叙事（禁裸报错：每码都有下一步建议）。 */
export const CEREMONY_NARRATIVES: readonly { code: string; title: string; next: string }[] = [
  { code: "PW-401", title: "会话保存未完成", next: "重试保存，或跳过保存直接继续仪式。" },
  { code: "PW-402", title: "有应用拒绝退出", next: "先手工关闭该应用，再一键续作。" },
  { code: "PW-403", title: "电量不足以完成仪式", next: "接通电源后继续，或改用直通档。" },
  { code: "PW-404", title: "仪式中断（异常重启）", next: "下次开机会标记半成品，可一键续作。" },
  { code: "PW-405", title: "休眠档案写入失败", next: "检查存储余量，或关闭休眠档案后重试。" },
];

export function findNarrative(code: string): { code: string; title: string; next: string } {
  const up = code.toUpperCase();
  return CEREMONY_NARRATIVES.find((n) => n.code === up)
    ?? { code: "PW-000", title: "未知仪式状况", next: "在设置里重跑仪式自检，或改用直通档。" };
}

/** 仪式执行器：阶段编排 + 进度 + 可取消 + 断点续作（tick 驱动）。 */
export class CeremonyRunner {
  kind: CeremonyKind;
  profileId: CeremonyProfileId;
  phase: CeremonyPhase = "idle";
  stage = 0;
  logs: CeremonyLog[] = [];
  /** 低电量守护：true 时每 tick 只推 1 阶段（否则 2）。 */
  lowPower: boolean;
  clamped = 0;

  constructor(kind: CeremonyKind = "shutdown", profileId: string = DEFAULT_CEREMONY_ID, lowPower = false) {
    this.kind = kind;
    const p = findCeremony(profileId);
    if (p.id !== profileId) this.clamped += 1;
    this.profileId = p.id as CeremonyProfileId;
    this.lowPower = lowPower;
  }

  private get total(): number {
    return findCeremony(this.profileId).stages;
  }

  start(): void {
    if (this.phase === "done" || this.phase === "canceled") this.stage = 0;
    this.phase = "running";
  }

  pause(): void {
    if (this.phase === "running") this.phase = "paused";
  }

  /** 断点续作：保留 stage，只拉回 running。 */
  resume(): boolean {
    if (this.phase !== "paused" && this.phase !== "failed" && this.phase !== "canceled") return false;
    this.phase = "running";
    return true;
  }

  /** 取消：立即出戏，标记可续作。 */
  cancel(): void {
    if (this.phase === "running" || this.phase === "paused") this.phase = "canceled";
  }

  /** 推进一 tick；失败由调用方注入（failOnStage >= 0），确定性测试。 */
  tick(failOnStage = -1): CeremonyPhase {
    if (this.phase !== "running") return this.phase;
    const per = this.lowPower ? 1 : 2;
    for (let i = 0; i < per && this.stage < this.total; i += 1) {
      this.stage += 1;
      const failed = failOnStage >= 0 && this.stage === failOnStage;
      this.logs.push({ stage: this.stage, ok: !failed, code: failed ? "PW-402" : undefined });
      if (failed) {
        this.phase = "failed";
        return this.phase;
      }
    }
    if (this.stage >= this.total) this.phase = "done";
    return this.phase;
  }

  progress(): number {
    const t = this.total;
    return t === 0 ? 0 : Math.min(1, this.stage / t);
  }

  /** 当前阶段名（进度字幕）。 */
  stageCaption(): string {
    if (this.phase === "idle") return "待命";
    if (this.stage <= 0) return "准备中";
    return CEREMONY_STAGES[Math.min(this.stage - 1, CEREMONY_STAGES.length - 1)]!;
  }

  /** 快照导出（半成品标记 + 进度，跨版本携带）。 */
  snapshot(): string {
    return JSON.stringify({
      v: 1,
      kind: this.kind,
      profile: this.profileId,
      stage: this.stage,
      phase: this.phase === "done" ? "done" : "half",
    });
  }

  /** 快照导入：坏载荷回默认并记钳制（导入不崩溃）。 */
  restore(snap: string): boolean {
    try {
      const o = JSON.parse(snap) as Record<string, unknown>;
      if (!o || typeof o !== "object" || o.v !== 1) {
        this.clamped += 1;
        return false;
      }
      const p = findCeremony(String(o.profile ?? ""));
      if (p.id !== o.profile) this.clamped += 1;
      this.profileId = p.id as CeremonyProfileId;
      this.kind = o.kind === "restart" || o.kind === "signout" ? (o.kind as CeremonyKind) : "shutdown";
      const st = Number(o.stage);
      this.stage = Number.isFinite(st) ? Math.min(Math.max(0, Math.round(st)), this.total) : 0;
      this.phase = o.phase === "done" ? "done" : "paused";
      return true;
    } catch {
      this.clamped += 1;
      return false;
    }
  }

  /** 净身：卸载不留残档。 */
  reset(): void {
    this.phase = "idle";
    this.stage = 0;
    this.logs = [];
  }
}

export function ceremonyPercent(r: CeremonyRunner): number {
  return Math.round(r.progress() * 100);
}

/* ============================== 族0012 睡眠唤醒剧场 2.0 ============================== */

/** 唤醒五档档位矩阵（默认 = balanced）。 */
export const WAKE_PROFILES = [
  { id: "blink", name: "瞬醒", stages: 1, motionLevel: 0, veil: false },
  { id: "briskWake", name: "轻快", stages: 2, motionLevel: 1, veil: false },
  { id: "balancedWake", name: "均衡", stages: 3, motionLevel: 2, veil: true },
  { id: "dawn", name: "黎明", stages: 4, motionLevel: 3, veil: true },
  { id: "sunrise", name: "日出", stages: 5, motionLevel: 3, veil: true },
] as const;
export type WakeProfileId = (typeof WAKE_PROFILES)[number]["id"];
export const DEFAULT_WAKE_ID: WakeProfileId = "balancedWake";

export function findWake(id: string): (typeof WAKE_PROFILES)[number] {
  return WAKE_PROFILES.find((p) => p.id === id) ?? WAKE_PROFILES[2]!;
}

/** 唤醒阶段目录（sunrise 5 阶段为全集）。 */
export const WAKE_STAGES = ["亮屏", "恢复会话", "揭开面纱", "预热任务", "问候"] as const;

export type WakePhase = "sleeping" | "waking" | "paused" | "awake" | "canceled";

/** 唤醒剧场执行器：从睡眠到 awake 的阶段编排。 */
export class WakeTheater {
  profileId: WakeProfileId;
  phase: WakePhase = "sleeping";
  stage = 0;
  clamped = 0;
  /** 低电量守护：每 tick 只推 1 阶段。 */
  lowPower: boolean;

  constructor(profileId: string = DEFAULT_WAKE_ID, lowPower = false) {
    const p = findWake(profileId);
    if (p.id !== profileId) this.clamped += 1;
    this.profileId = p.id as WakeProfileId;
    this.lowPower = lowPower;
  }

  private get total(): number {
    return findWake(this.profileId).stages;
  }

  /** 入睡：复位舞台。 */
  sleep(): void {
    this.phase = "sleeping";
    this.stage = 0;
  }

  /** 开始唤醒（可被取消 → 回 sleeping）。 */
  wake(): void {
    if (this.phase === "sleeping" || this.phase === "paused") this.phase = "waking";
  }

  pause(): void {
    if (this.phase === "waking") this.phase = "paused";
  }

  /** 断点续作：保留 stage，只拉回 waking。 */
  resume(): boolean {
    if (this.phase !== "paused") return false;
    this.phase = "waking";
    return true;
  }

  cancel(): void {
    if (this.phase === "waking" || this.phase === "paused") {
      this.phase = "sleeping";
      this.stage = 0;
    }
  }

  tick(): WakePhase {
    if (this.phase !== "waking") return this.phase;
    const per = this.lowPower ? 1 : 2;
    for (let i = 0; i < per && this.stage < this.total; i += 1) this.stage += 1;
    if (this.stage >= this.total) this.phase = "awake";
    return this.phase;
  }

  progress(): number {
    const t = this.total;
    return t === 0 ? 1 : Math.min(1, this.stage / t);
  }

  stageCaption(): string {
    if (this.phase === "sleeping") return "睡眠中";
    if (this.stage <= 0) return "唤醒中";
    return WAKE_STAGES[Math.min(this.stage - 1, WAKE_STAGES.length - 1)]!;
  }

  /** 面纱透明度（veil 档才有，0~1，reduce-motion 恒 1）。 */
  veilAlpha(reduceMotion = false): number {
    if (!findWake(this.profileId).veil || reduceMotion) return 1;
    return 0.2 + 0.8 * this.progress();
  }

  reset(): void {
    this.phase = "sleeping";
    this.stage = 0;
    this.clamped = 0;
  }
}

export function wakePercent(w: WakeTheater): number {
  return Math.round(w.progress() * 100);
}
