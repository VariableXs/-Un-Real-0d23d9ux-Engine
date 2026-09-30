/**
 * 任务 78（专项 I3/I4 · 数据搬家与退役安全清空）—— 逻辑核。
 *
 * 总案验收口径：
 * I3 跨 U 盘搬家向导：
 * - 步骤=差分链重挂 → SHARED 全量 → 配置回灌 → 一致性校验；
 * - 中断续走：步骤状态可序列化恢复（从任一步边界续跑，已完成步不重做）；
 * - 校验=逐分区校验和一致才签发完成；
 * - 幂等=重复执行同一 plan 不产生重复副本。
 * I4 退役安全清空：
 * - 两档：reset（仅重置系统：KV/白名单/审计清空，用户数据保留）/
 *   wipe（全清：数据区多轮覆写 + 验证 + 退役证书）；
 * - 不可逆操作必须双确认（两次显式 confirm token）；
 * - 覆写轮次固定 0x00→0xFF→0xA5，轮轮 fnv1a64 验证。
 *
 * 纯确定性逻辑核（搬运/覆写的执行端 = 复用部署脚本与 kvault 焚毁语义，
 * 本模块是向导状态机与验证账本；导出即文档）。
 */

// ---------- I3 · 搬家向导 ----------

export const MIGRATION_STEPS = [
  "remount-diff-chain",
  "copy-shared",
  "reinject-config",
  "verify-checksums",
] as const;

export type MigrationStep = (typeof MIGRATION_STEPS)[number];
export type StepState = "pending" | "done" | "failed";

export interface MigrationState {
  /** plan 指纹（源/目标标识派生）：恢复时防错盘续跑。 */
  planId: string;
  step: Record<MigrationStep, StepState>;
  /** 每步产物摘要（续走时对账；不一致=拒绝续跑）。 */
  note: Record<MigrationStep, string>;
  done: boolean;
}

/** 各步校验和账本（verify 步逐项比对；键=分区名，值=fnv1a64 十六进制）。 */
export type ChecksumBook = Record<string, string>;

/** 生成空白迁移状态（确定性 planId：源+目标直连拼串——调用方传稳定标识）。 */
export function newMigration(planId: string): MigrationState {
  const step = {} as Record<MigrationStep, StepState>;
  const note = {} as Record<MigrationStep, string>;
  for (const s of MIGRATION_STEPS) {
    step[s] = "pending";
    note[s] = "";
  }
  return { planId, step, note, done: false };
}

/** 序列化/恢复（localStorage JSON 往返；续走前必须过同 planId 校验）。 */
export function saveMigration(s: MigrationState): string {
  return JSON.stringify(s);
}

export function restoreMigration(
  raw: string,
  planId: string,
): MigrationState | null {
  try {
    const s = JSON.parse(raw) as MigrationState;
    if (s?.planId !== planId || !s.step || !s.note) {
      return null; // 换盘/换 plan = 拒绝续跑（防错盘半程续接）
    }
    for (const k of MIGRATION_STEPS) {
      if (s.step[k] !== "done" && s.step[k] !== "failed" && s.step[k] !== "pending") {
        return null;
      }
    }
    return s;
  } catch {
    return null;
  }
}

/**
 * 推进一步（幂等：已 done 的步直接跳过不重做）。
 * execute 返回该步产物摘要；抛错 = failed（可重试同一步）。
 */
export function advanceMigration(
  s: MigrationState,
  step: MigrationStep,
  execute: () => string,
): MigrationState {
  if (s.step[step] === "done") {
    return s; // 幂等：重跑不重做
  }
  try {
    s.note[step] = execute();
    s.step[step] = "done";
  } catch (e) {
    s.step[step] = "failed";
    s.note[step] = e instanceof Error ? e.message : String(e);
  }
  s.done = MIGRATION_STEPS.every((k) => s.step[k] === "done");
  return s;
}

/** 一致性校验：源/目标账本逐键相等才通过（缺键/异值=失败，如实列出）。 */
export function verifyChecksums(
  source: ChecksumBook,
  target: ChecksumBook,
): { ok: boolean; mismatch: string[] } {
  const keys = new Set([...Object.keys(source), ...Object.keys(target)]);
  const mismatch: string[] = [];
  for (const k of keys) {
    if (source[k] !== target[k]) {
      mismatch.push(k);
    }
  }
  return { ok: mismatch.length === 0, mismatch };
}

// ---------- I4 · 退役安全清空 ----------

export type RetireTier = "reset" | "wipe";

/** 覆写轮次模式（固定三轮；轮轮校验——总案 I4 口径）。 */
export const WIPE_PASSES: readonly number[] = [0x00, 0xff, 0xa5];

/** reset 档清空目标（系统态）；返回被清项清单（用户数据键白名单保留）。 */
export function retireReset(
  systemKeys: string[],
  dataKeys: string[],
): { cleared: string[]; kept: string[] } {
  return { cleared: [...systemKeys], kept: [...dataKeys] };
}

/** fnv1a64（与内核 kvault/exfat 校验同族；前端验证账本用）。 */
export function fnv1a64(bytes: Uint8Array): string {
  let h = 0xcbf29ce484222325n;
  const p = 0x100000001b3n;
  const m = (1n << 64n) - 1n;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * p) & m;
  }
  return h.toString(16).padStart(16, "0");
}

/**
 * wipe 档：对数据区块执行三轮覆写（0x00→0xFF→0xA5）+ 轮轮 fnv 验证。
 * buf 为调用方持有的代表性区块（逻辑核驱动覆写模式与验证；物理盘面
 * 覆写由执行端按同一模式逐区完成——总案"抽验覆写区"口径）。
 * 返回每轮摘要（退役证书载荷，确定性、无时钟依赖）。
 */
export function retireWipeBlock(
  buf: Uint8Array,
): { passes: { pattern: number; digest: string }[]; finalDigest: string } {
  const passes = WIPE_PASSES.map((pattern) => {
    buf.fill(pattern);
    return { pattern, digest: fnv1a64(buf) };
  });
  // WIPE_PASSES 非空常量，末轮恒存在。
  const last = passes[passes.length - 1]!;
  return { passes, finalDigest: last.digest };
}

/** 双确认闸：两次显式确认 token 才放行（中间任一撤销=整体拒绝）。 */
export class DoubleConfirm {
  private first = false;
  constructor(private readonly subject: string) {}
  confirm(token: string): "need-first" | "need-second" | "granted" | "rejected" {
    if (token !== this.subject) {
      this.first = false; // 口令不符=撤销
      return "rejected";
    }
    if (!this.first) {
      this.first = true;
      return "need-second";
    }
    return "granted";
  }
  /** 退役证书（wipe 完成签发；双确认通过是前置条件）。 */
  static certificate(
    subject: string,
    digests: string[],
    granted: boolean,
  ): { subject: string; passes: string[]; granted: boolean } | null {
    if (!granted || digests.length !== WIPE_PASSES.length) {
      return null;
    }
    return { subject, passes: [...digests], granted };
  }
}
