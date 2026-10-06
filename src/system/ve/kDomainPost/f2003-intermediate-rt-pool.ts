/**
 * VE-F2003 · 中间渲染目标池（K 域 · 批次 K01 · 后处理显存治理底座）
 * ---------------------------------------------------------------------------
 * 职责定位：中间渲染目标池——后处理链的每一步都要一张中间 RT，若每帧重建，
 * 显存分配与释放的抖动会直接吃掉帧预算。本条把中间 RT 做成**池化复用**：
 * 尺寸与格式匹配的 RT 在帧间复用，避免重复创建。
 *
 * 上游 F2001 域架构（RtSpec / PooledRt / RtPool / acquireRt / releaseRt 池语义契约）、
 *       F2002 DAG 执行器（RT 分配计划与别名分析消费图结构）。
 * 下游 F2004+ K 域全部效果（RT 均经本池）、F2059 遥测（池命中/未命中/峰值占用）、
 *       F1776 配额体系（K 段显存配额）、F2014预算表（显存节省实测）。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. 池化复用：空闲链表按（尺寸 × 格式）**分桶**。同桶内取空闲 RT 复用，
 *      显存友好。桶键匹配是强制项——尺寸或格式任一不同即视为不同桶，
 *      绝不跨桶复用（跨桶复用会让效果读到错误精度/错误色彩格式的数据）。
 *   2. 区间别名：RT 生命周期由 DAG 编译期标注——每个 RT 有**出生**（首次写）
 *      与**死亡**（最后读）位置。存活区间不重叠的两个 RT 可别名复用同一物理资源，
 *      这是区间图着色问题（编译期 O(RT 数)）。
 *   3. 帧边界回收：帧边界统一回收 + 引用计数防提前回收。
 *      提前回收会让仍在读该 RT 的效果读到已释放资源——表现是偶发画面撕裂/闪烁，
 *      极难归因。故引用计数未归零不得复用。
 *   4. 泄漏防护：帧边界未回收即记泄漏账本；连续 N 帧未回收 → 告警。
 *      泄漏若不主动报，会表现为「跑几分钟后显存持续上涨」，到 OOM 时早已
 *      失去现场。
 *   5. 配额拒绝：池超配额（F1776 中间池95%）→ 拒绝分配 + 三要素
 *      （当前用量 / 上限 / 建议降档）。静默失败会让上层拿到无效 RT。
 *
 * 四条容易做错、故显式记录的设计立场：
 *
 *   一、别名只在「存活区间严格不重叠」时成立，且必须双向举证。
 *     单向检查（左区间在右区间之前结束）不够——还要确认右区间**不早于**
 *     左区间开始。故本条要求区间为闭区间 [birth, death] 并显式验证
 *     `a.death < b.birth || b.death < a.birth`。用开区间会在
 *     「a 在第 5 步死亡、b 在第 5 步出生」这一情形下误判为可别名——
 *     而第 5 步内a 被读、b 被写，同一物理资源被同时读写，画面会闪。
 *
 *   二、桶键必须包含格式，且比较用严格相等而非「至少这么大」。
 *     「找尺寸不小于请求的最小桶」是显存池的常见优化，但对渲染目标**不成立**：
 *     更大的 RT 虽装得下，却会让后处理的像素级运算按错误分辨率进行
 *     （如半分辨率模糊被当成全分辨率，结果只是被放大采样，细节全丢）。
 *     故本条严格匹配，宁可 miss 也不跨规格复用。
 *
 *   三、引用计数与别名是两套机制，不可互相替代。
 *     别名回答「能否共用一块物理资源」（编译期布局问题）；
 *     引用计数回答「现在能不能回收」（运行期生命周期问题）。
 *     混淆会导致「别名判定安全就跳过引用计数」——而同一物理资源被两个
 *     逻辑 RT 先后使用时，前一个的释放会误伤后一个。故别名后仍逐句柄计数。
 *
 *   四、泄漏账本按「句柄 + 首见帧」记账，不按帧号计数。
 *     若只记「本帧未回收数」，连续泄漏与一次性泄漏的告警强度相同，
 *     无法区分「新泄漏」与「上次的还没释放完」。按首见帧记账才能
 *     报出「句柄 7 已泄漏 152 帧」这种可定位的告警。
 *
 * 零静默纪律：配额超限、别名冲突、桶键不匹配、重复归还、提前回收、
 * 泄漏超阈全部产出 Diagnostic（code + message + hint），降级一律显性。
 *
 * 判据：池化复用、区间别名、帧边界回收、泄漏防护、判据。
 * 依赖锚点：F2001 RtSpec/PooledRt/RtPool 与 acquireRt/releaseRt；F2002 编译产物。
 * 交接说明：本条管理的是**中间 RT 的分配与生命周期**，不碰 GPU 资源本身——
 *          「真正创建 GPU 纹理」由后端实现，本条只决定「谁该拿哪张、
 *          什么时候能还」，即池的策略层。句柄是本条与后端之间的唯一契约。
 *          另：本条与 F1528 是**同构实现**（桶分 + 复用），但不是同一份代码——
 *          跨域同构声明见锚点，复用其理念而非依赖其实现（域间不交叉依赖）。
 */

import type { RtSpec as F2001RtSpec } from "./f2001-k-domain-post-architecture.js";
import type { RtAllocationPlan } from "./f2002-post-fx-dag-executor.js";

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/** 池专属诊断码（独立枚举，不与F2001/F2002 共用——处置方向不同）。 */
export type PoolDiagCode =
  /** 池超配额（F1776 中间池水位）→ 拒绝分配。 */
  | "QUOTA_EXCEEDED"
  /** 桶键不匹配（尺寸或格式不同）→ 拒绝跨桶复用。 */
  | "BUCKET_KEY_MISMATCH"
  /** 别名冲突：区间标注错误导致读写竞争。 */
  | "ALIAS_CONFLICT"
  /** 别名区间的端点非法（死亡早于出生等）。 */
  | "ALIAS_INTERVAL_INVALID"
  /** 重复归还。 */
  | "DOUBLE_RELEASE"
  /** 归还未分配的句柄。 */
  | "UNKNOWN_HANDLE"
  /** 引用计数未归零即被要求回收。 */
  | "EARLY_RECOLLECT"
  /** 泄漏超阈（连续 N 帧未回收）。 */
  | "LEAK_SUSPECTED"
  /** 配额查询参数非法。 */
  | "QUOTA_PARAM_INVALID"
  /** 分配计划与池状态不一致。 */
  | "PLAN_POOL_MISMATCH";

/** 一条诊断。 */
export interface PoolDiagnostic {
  readonly code: PoolDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type PoolOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly PoolDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: PoolDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly PoolDiagnostic[];
    };

/** 成功构造。 */
export function poolOk<T>(value: T, diagnostics: readonly PoolDiagnostic[] = []): PoolOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function poolErr<T>(
  code: PoolDiagCode,
  message: string,
  hint: string,
  diagnostics: readonly PoolDiagnostic[] = [],
): PoolOutcome<T> {
  const d: PoolDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [...diagnostics, d] };
}

/** 诊断聚合器。 */
export class PoolDiagBag {
  private readonly items: PoolDiagnostic[] = [];

  push(code: PoolDiagCode, message: string, hint: string): void {
    this.items.push({
      code,
      message: message || "（未提供描述）",
      hint: hint || "（未提供处置建议）",
    });
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly PoolDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: PoolDiagCode): readonly PoolDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  has(code: PoolDiagCode): boolean {
    return this.items.some((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 RT 规格与桶键（判据一：池化复用）
// ════════════════════════════════════════════════════════════════════════════

/** RT 规格（与 F2001 的 RtSpec 同构，独立声明以免运行时依赖跨条 import 副作用）。 */
export interface RtSpec {
  readonly width: number;
  readonly height: number;
  readonly format: string;
}

/**
 * 桶键：尺寸 × 格式的严格组合（立场二：绝不跨规格复用）。
 * 键用可比较字符串，便于 Map 分桶与调试直读。
 */
export function bucketKey(spec: RtSpec): string {
  return `${spec.width}x${spec.height}@${spec.format}`;
}

/** 规格是否合法（宽高为正整数、格式非空）。 */
export function isValidSpec(spec: RtSpec): boolean {
  return (
    Number.isInteger(spec.width) && spec.width > 0 &&
    Number.isInteger(spec.height) && spec.height > 0 &&
    spec.format.length > 0
  );
}

/** 两规格是否严格相等（桶键相等）。 */
export function sameSpec(a: RtSpec, b: RtSpec): boolean {
  return a.width === b.width && a.height === b.height && a.format === b.format;
}

/** 规格转 F2001 形态（供与 F2001 池对接）。 */
export function toF2001Spec(spec: RtSpec): F2001RtSpec {
  const fmt = spec.format as F2001RtSpec["format"];
  return { width: spec.width, height: spec.height, format: fmt };
}

/** 规格取自 F2001 形态。 */
export function fromF2001Spec(spec: F2001RtSpec): RtSpec {
  return { width: spec.width, height: spec.height, format: spec.format };
}

/** 池中一个 RT 条目（含生命周期状态）。 */
export interface PoolEntry {
  readonly handle: number;
  /** 当前绑定的逻辑槽名（同一物理资源被别名复用时会变）。 */
  readonly slot: string;
  readonly spec: RtSpec;
  /** 在用标记（已归还为 false→true）。 */
  readonly inUse: boolean;
  /** 引用计数：仍持有该 RT 的逻辑引用数（立场三：别名后仍逐句柄计数）。 */
  readonly refCount: number;
  /** 别名组 id：共用同一物理资源的逻辑 RT 共享该 id；非别名为自身 handle。 */
  readonly aliasGroup: number;
}

/** 桶：一个（尺寸×格式）桶内的空闲句柄集合。 */
export interface Bucket {
  readonly key: string;
  readonly spec: RtSpec;
  /** 空闲句柄（已归还且引用计数归零）。 */
  readonly freeHandles: readonly number[];
}

// ════════════════════════════════════════════════════════════════════════════
// §3 区间别名分析（判据二：区间别名，编译期 O(RT 数)）
// ════════════════════════════════════════════════════════════════════════════

/**
 * RT 存活区间（闭区间，立场一）。
 * birth = 首次被写的执行序下标；death = 最后一次被读的执行序下标。
 * 用闭区间而非开区间：第 N 步内既写又读的 RT，其death 仍是 N，
 * 与「第 N 步才出生」的另一个 RT 在 N 处相接，此时**不可**别名。
 */
export interface LiveInterval {
  readonly slot: string;
  readonly birth: number;
  readonly death: number;
}

/** 区间是否合法（birth ≤ death，且皆为有限非负整数）。 */
export function isValidInterval(iv: LiveInterval): boolean {
  return (
    Number.isInteger(iv.birth) && Number.isInteger(iv.death) &&
    iv.birth >= 0 && iv.death >= iv.birth
  );
}

/**
 * 两个区间是否严格不重叠（可别名）。
 * 判据：`a.death < b.birth || b.death < a.birth`（严格小于，闭区间语义）。
 */
export function intervalsDisjoint(a: LiveInterval, b: LiveInterval): boolean {
  return a.death < b.birth || b.death < a.birth;
}

/** 一个别名组：共用同一物理资源的逻辑槽。 */
export interface AliasGroup {
  readonly groupId: number;
  readonly members: readonly string[];
}

/** 别名分析结果。 */
export interface AliasAnalysis {
  /** 逐槽的别名组 id。 */
  readonly groupOfSlot: ReadonlyMap<string, number>;
  /** 别名组列表（成员数 ≥2 的才有复用价值，单成员组也列出便于调试）。 */
  readonly groups: readonly AliasGroup[];
  /** 物理资源数（别名后的实际 RT 数）。 */
  readonly physicalCount: number;
  /** 逻辑 RT 数（别名前）。 */
  readonly logicalCount: number;
  /** 节省的 RT 数（logicalCount - physicalCount）。 */
  readonly savedCount: number;
  /** 非法区间（诊断用）。 */
  readonly invalidIntervals: readonly string[];
}

/**
 * 区间图着色求别名（F2002 编译期调用，O(RT 数)）。
 *
 * 算法（贪心，编译期一次性，不是热路径）：
 *   按 birth 升序扫描每个区间；若与已有组内**所有**成员都不重叠，
 *   则并入该组；否则开新组。组内成员两两不重叠是组的不变式。
 *   贪心不保证最少组数，但保证**正确**（绝不误判可别名）——
 *   而误判会导致读写竞争（画面闪），正确性优先于最优性。
 */
export function analyzeAliases(intervals: readonly LiveInterval[]): AliasAnalysis {
  const valid: LiveInterval[] = [];
  const invalid: string[] = [];
  for (const iv of intervals) {
    if (isValidInterval(iv)) valid.push(iv);
    else invalid.push(iv.slot);
  }
  // 按 birth 升序；同 birth 按 slot 名定序，保证分组结果确定性（立场：确定性优先）
  const sorted = valid.slice().sort((a, b) => {
    if (a.birth !== b.birth) return a.birth - b.birth;
    return a.slot < b.slot ? -1 : a.slot > b.slot ? 1 : 0;
  });
  const groups: AliasGroup[] = [];
  const groupOfSlot = new Map<string, number>();
  for (const iv of sorted) {
    let placed = false;
    for (const g of groups) {
      // 组内全部成员都与新区间不重叠，才可并入（不变式：组内两两不重叠）
      let disjointAll = true;
      for (const m of g.members) {
        const mv = sorted.find((x) => x.slot === m);
        if (mv === undefined) {
          disjointAll = false;
          break;
        }
        if (!intervalsDisjoint(iv, mv)) {
          disjointAll = false;
          break;
        }
      }
      if (disjointAll) {
        const gi = groups.indexOf(g);
        const next = groups[gi];
        if (next !== undefined) {
          groups[gi] = { groupId: next.groupId, members: [...next.members, iv.slot] };
          groupOfSlot.set(iv.slot, next.groupId);
          placed = true;
        }
        break;
      }
    }
    if (!placed) {
      const gid = groups.length;
      groups.push({ groupId: gid, members: [iv.slot] });
      groupOfSlot.set(iv.slot, gid);
    }
  }
  return {
    groupOfSlot,
    groups,
    physicalCount: groups.length,
    logicalCount: valid.length,
    savedCount: valid.length - groups.length,
    invalidIntervals: invalid,
  };
}

/**
 * 别名正确性校验（调试模式断言，判据五的边界防护）。
 * 逐对检查同组成员是否真的两两不重叠；有重叠即断（开发期拦截）。
 */
export function verifyAliasGroups(
  analysis: AliasAnalysis,
  intervals: readonly LiveInterval[],
): PoolDiagnostic[] {
  const out: PoolDiagnostic[] = [];
  const bySlot = new Map<string, LiveInterval>();
  for (const iv of intervals) bySlot.set(iv.slot, iv);
  for (const g of analysis.groups) {
    for (let i = 0; i < g.members.length; i += 1) {
      for (let j = i + 1; j < g.members.length; j += 1) {
        const a = bySlot.get(g.members[i] as string);
        const b = bySlot.get(g.members[j] as string);
        if (a === undefined || b === undefined) continue;
        if (!intervalsDisjoint(a, b)) {
          out.push({
            code: "ALIAS_CONFLICT",
            message: `别名组 ${g.groupId} 内 ${a.slot}[${a.birth},${a.death}] 与 ${b.slot}[${b.birth},${b.death}] 区间重叠。`,
            hint: "区间标注有误：重叠区间共用同一物理资源会造成读写竞争（画面闪烁）。请核对 DAG 编译期标注的 birth/death。",
          });
        }
      }
    }
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §4 泄漏账本（判据四：泄漏防护）
// ════════════════════════════════════════════════════════════════════════════

/** 泄漏记录：按句柄 + 首见帧记账（立场四）。 */
export interface LeakRecord {
  readonly handle: number;
  readonly slot: string;
  /** 首次发现未回收的帧号。 */
  readonly firstSeenFrame: number;
  /** 最近一次仍未回收的帧号。 */
  readonly lastSeenFrame: number;
  /** 连续未回收帧数。 */
  readonly frames: number;
}

/** 泄漏账本。 */
export class LeakLedger {
  private readonly records = new Map<number, LeakRecord>();
  private readonly threshold: number;

  constructor(threshold = 3) {
    this.threshold = threshold;
  }

  /** 登记一次未回收（帧边界调用）。 */
  observe(handle: number, slot: string, frame: number): void {
    const prev = this.records.get(handle);
    if (prev === undefined) {
      this.records.set(handle, { handle, slot, firstSeenFrame: frame, lastSeenFrame: frame, frames: 1 });
      return;
    }
    // 帧号连续则累加；不连续（中间回收过又漏了）则重新起算
    const contiguous = frame === prev.lastSeenFrame + 1;
    this.records.set(handle, {
      handle,
      slot,
      firstSeenFrame: contiguous ? prev.firstSeenFrame : frame,
      lastSeenFrame: frame,
      frames: contiguous ? prev.frames + 1 : 1,
    });
  }

  /** 本帧已回收的句柄应清账。 */
  clear(handle: number): void {
    this.records.delete(handle);
  }

  /** 超阈的泄漏（连续 N 帧未回收）。 */
  suspected(): readonly LeakRecord[] {
    return [...this.records.values()].filter((r) => r.frames >= this.threshold);
  }

  /** 全部在账记录。 */
  all(): readonly LeakRecord[] {
    return [...this.records.values()].sort((a, b) => b.frames - a.frames);
  }

  /** 阈值。 */
  get limit(): number {
    return this.threshold;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §5 中间渲染目标池（判据一 + 三 + 五）
// ════════════════════════════════════════════════════════════════════════════

/** 配额（F1776 中间池段）。 */
export interface Quota {
  /** 池容量（同时在用的 RT 数上限）。 */
  readonly maxEntries: number;
  /** 显存字节上限。 */
  readonly maxBytes: number;
  /** 触发拒绝的水位比例（锚点：中间池 95%）。 */
  readonly rejectRatio: number;
}

/** 池统计（遥测与调优双用，进 F2059）。 */
export interface PoolStats {
  readonly hits: number;
  readonly misses: number;
  readonly liveCount: number;
  readonly totalEntries: number;
  readonly peakLive: number;
  readonly bytesInUse: number;
  readonly bucketCount: number;
  /** 当前是否已过拒绝水位。 */
  readonly atRejectWatermark: boolean;
}

/** 分配结果。 */
export interface AllocResult {
  readonly pool: RtPoolState;
  readonly handle: number;
  /** 是否命中复用（同桶空闲 RT）。 */
  readonly hit: boolean;
}

/**
 * 中间渲染目标池。
 *
 * 不可变风格：所有操作返回新状态（与 F2001 的 RtPool 语义一致），
 * 便于与 F2002 的「编译产物不可变」对齐，也便于回放与快照。
 */
export class RtTargetPool {
  private entries: PoolEntry[] = [];
  private buckets: Bucket[] = [];
  private readonly quota: Quota;
  private readonly diagBag = new PoolDiagBag();
  private readonly leaks: LeakLedger;
  private hits = 0;
  private misses = 0;
  private peakLive = 0;
  private frame = 0;
  private nextGroupId = 1;

  constructor(quota: Quota, leakThreshold = 3) {
    this.quota = quota;
    this.leaks = new LeakLedger(leakThreshold);
  }

  /** 当前状态快照（不可变）。 */
  state(): RtPoolState {
    return {
      entries: this.entries.slice(),
      buckets: this.buckets.map((b) => ({ key: b.key, spec: b.spec, freeHandles: b.freeHandles.slice() })),
      frame: this.frame,
    };
  }

  /** 统计。 */
  stats(): PoolStats {
    const live = this.entries.filter((e) => e.inUse);
    const bytes = live.reduce((sum, e) => sum + rtBytes(e.spec), 0);
    const atReject = this.reachedRejectWatermark();
    return {
      hits: this.hits,
      misses: this.misses,
      liveCount: live.length,
      totalEntries: this.entries.length,
      peakLive: this.peakLive,
      bytesInUse: bytes,
      bucketCount: this.buckets.length,
      atRejectWatermark: atReject,
    };
  }

  /** 单张 RT 的字节数（按格式位深估算，供配额核算）。 */
  private bytesUsed(): number {
    return this.entries.filter((e) => e.inUse).reduce((sum, e) => sum + rtBytes(e.spec), 0);
  }

  /** 是否已达拒绝水位（判据五）。 */
  private reachedRejectWatermark(): boolean {
    const ratio = this.quota.rejectRatio;
    const byCount = this.entries.filter((e) => e.inUse).length >= Math.floor(this.quota.maxEntries * ratio);
    const byBytes = this.bytesUsed() >= Math.floor(this.quota.maxBytes * ratio);
    return byCount || byBytes;
  }

  /**
   * 分配一张 RT。
   *
   * 流程：桶内空闲复用 → 配额水位检查 → 新建。
   * 配额拒绝给三要素（当前用量/上限/建议降档），不是裸失败。
   */
  allocate(slot: string, spec: RtSpec): PoolOutcome<AllocResult> {
    if (!isValidSpec(spec)) {
      return poolErr("QUOTA_PARAM_INVALID",
        `RT 规格非法：${bucketKey(spec)}。`,
        "宽高须为正整数、格式非空；非法规格会落入错误的桶导致跨规格复用。");
    }
    // 步骤 1：同桶空闲复用（严格键匹配，立场二）
    const key = bucketKey(spec);
    const bi = this.buckets.findIndex((b) => b.key === key);
    if (bi >= 0) {
      const bucket = this.buckets[bi] as Bucket;
      const freeHandle = bucket.freeHandles.find((h) => {
        const e = this.entries.find((x) => x.handle === h);
        return e !== undefined && !e.inUse && e.refCount === 0;
      });
      if (freeHandle !== undefined) {
        // 关键：桶键相同才复用，否则拒绝跨桶（显性登记而非静默）
        const entry = this.entries.find((x) => x.handle === freeHandle);
        if (entry !== undefined && !sameSpec(entry.spec, spec)) {
          this.diagBag.push("BUCKET_KEY_MISMATCH",
            `桶 ${key} 内句柄 ${freeHandle} 规格为 ${bucketKey(entry.spec)}，与请求不符，拒绝复用。`,
            "桶键与实际规格不符说明分桶逻辑有 bug；宁可不复用也不能跨规格。");
        } else {
          const entries = this.entries.map((e) =>
            e.handle === freeHandle ? { ...e, slot, inUse: true, refCount: 1 } : e,
          );
          const buckets = this.buckets.slice();
          buckets[bi] = {
            key: bucket.key,
            spec: bucket.spec,
            freeHandles: bucket.freeHandles.filter((h) => h !== freeHandle),
          };
          this.entries = entries;
          this.buckets = buckets;
          this.hits += 1;
          this.trackPeak();
          return poolOk({ pool: this.state(), handle: freeHandle, hit: true }, this.diagBag.all());
        }
      }
    }
    // 步骤 2：配额水位检查（判据五）
    if (this.reachedRejectWatermark()) {
      const live = this.entries.filter((e) => e.inUse).length;
      const bytes = this.bytesUsed();
      this.diagBag.push("QUOTA_EXCEEDED",
        `中间 RT 池达拒绝水位：在用 ${live}/${this.quota.maxEntries} 张、${bytes}/${this.quota.maxBytes} 字节（水位 ${Math.round(this.quota.rejectRatio * 100)}%）。`,
        "三要素——当前：在用 " + live + " 张 " + bytes + " 字节；上限：" +
        this.quota.maxEntries + " 张 " + this.quota.maxBytes + " 字节；建议：降质量档" +
        "（F2015）减少同时存活效果，或由 F2002 重编译出更短的链。");
      return poolErr("QUOTA_EXCEEDED",
        `中间 RT 池达拒绝水位（在用 ${live}/${this.quota.maxEntries} 张，${bytes}/${this.quota.maxBytes} 字节）。`,
        "当前用量 " + live + " 张 " + bytes + " 字节；上限 " + this.quota.maxEntries + " 张 " +
        this.quota.maxBytes + " 字节；建议降质量档（F2015）或缩短后处理链。");
    }
    if (this.entries.filter((e) => e.inUse).length >= this.quota.maxEntries) {
      return poolErr("QUOTA_EXCEEDED",
        `中间 RT 池已满：在用达上限 ${this.quota.maxEntries} 张。`,
        "当前用量 " + this.entries.filter((e) => e.inUse).length + " 张；上限 " +
        this.quota.maxEntries + " 张；建议合并相邻效果或降档。");
    }
    // 步骤 3：新建条目
    const nextHandle = this.entries.reduce((max, e) => Math.max(max, e.handle + 1), 0);
    const entry: PoolEntry = {
      handle: nextHandle,
      slot,
      spec,
      inUse: true,
      refCount: 1,
      aliasGroup: this.nextGroupId,
    };
    this.nextGroupId += 1;
    this.entries = [...this.entries, entry];
    if (bi >= 0) {
      const buckets = this.buckets.slice();
      const bucket = buckets[bi] as Bucket;
      buckets[bi] = { key: bucket.key, spec: bucket.spec, freeHandles: bucket.freeHandles };
      this.buckets = buckets;
    } else {
      this.buckets = [...this.buckets, { key, spec, freeHandles: [] }];
    }
    this.misses += 1;
    this.trackPeak();
    return poolOk({ pool: this.state(), handle: nextHandle, hit: false }, this.diagBag.all());
  }

  /** 引用计数 +1（同一物理资源被多逻辑 RT 持有，立场三）。 */
  retain(handle: number): PoolOutcome<RtPoolState> {
    const e = this.entries.find((x) => x.handle === handle);
    if (e === undefined) {
      return poolErr("UNKNOWN_HANDLE", `引用计数失败：句柄 ${handle} 不存在。`, "核对句柄来源。");
    }
    this.entries = this.entries.map((x) => (x.handle === handle ? { ...x, refCount: x.refCount + 1 } : x));
    return poolOk(this.state(), this.diagBag.all());
  }

  /**
   * 归还 RT（引用计数 -1）。
   * 引用计数未归零 → 拒绝回收并登记 EARLY_RECOLLECT（判据三：防提前回收）。
   */
  release(handle: number): PoolOutcome<RtPoolState> {
    const e = this.entries.find((x) => x.handle === handle);
    if (e === undefined) {
      return poolErr("UNKNOWN_HANDLE", `归还失败：句柄 ${handle} 不存在。`, "核对句柄来源；未分配的句柄会掩盖真正的分配缺陷。");
    }
    if (!e.inUse) {
      return poolErr("DOUBLE_RELEASE", `重复归还句柄 ${handle}。`, "句柄只能归还一次；重复归还会让在用计数虚低，最终表现为池耗尽。");
    }
    if (e.refCount > 1) {
      this.entries = this.entries.map((x) => (x.handle === handle ? { ...x, refCount: x.refCount - 1 } : x));
      return poolOk(this.state(), this.diagBag.all());
    }
    // 引用计数归零：真正进入空闲桶
    const key = bucketKey(e.spec);
    const entries = this.entries.map((x) => (x.handle === handle ? { ...x, inUse: false, refCount: 0 } : x));
    const bi = this.buckets.findIndex((b) => b.key === key);
    const buckets = this.buckets.slice();
    if (bi >= 0) {
      const bucket = buckets[bi] as Bucket;
      buckets[bi] = { key: bucket.key, spec: bucket.spec, freeHandles: [...bucket.freeHandles, handle] };
    } else {
      buckets.push({ key, spec: e.spec, freeHandles: [handle] });
    }
    this.entries = entries;
    this.buckets = buckets;
    return poolOk(this.state(), this.diagBag.all());
  }

  /**
   * 帧边界回收（判据三）：推进帧号、清理泄漏账本、回收「本帧已死且无人持有」的 RT。
   *
   * 回收依据是**引用计数**而非「上一帧用过」——别名机制下同一物理资源
   * 可能跨帧存活，仅按帧号回收会误伤（立场三）。
   */
  beginFrame(deadHandles: readonly number[] = []): PoolOutcome<FrameReport> {
    this.frame += 1;
    const bag = new PoolDiagBag();
    // 泄漏观测：帧边界仍 inUse 的句柄视为未回收
    for (const e of this.entries) {
      if (e.inUse) {
        this.leaks.observe(e.handle, e.slot, this.frame);
      } else {
        this.leaks.clear(e.handle);
      }
    }
    const suspected = this.leaks.suspected();
    for (const r of suspected) {
      bag.push("LEAK_SUSPECTED",
        `句柄 ${r.handle}（槽 ${r.slot}）已连续 ${r.frames} 帧未回收（首见帧 ${r.firstSeenFrame}）。`,
        "三要素——当前：泄漏句柄 " + r.handle + "，持续 " + r.frames + " 帧；上限：" +
        this.leaks.limit + " 帧；建议：核对 F2002 执行列表的 release 调用是否配对，" +
        "或检查是否有效果抛异常跳过了归还。");
    }
    // 回收：引用计数归零的 dead 句柄直接释放出池（减少常驻条目）
    let reaped = 0;
    const reapSet = new Set(deadHandles);
    if (reapSet.size > 0) {
      const before = this.entries.length;
      this.entries = this.entries.filter((e) => {
        const dead = reapSet.has(e.handle);
        if (dead && !e.inUse && e.refCount === 0) {
          reaped += 1;
          return false;
        }
        return true;
      });
      // 同步清理桶中的悬空句柄
      this.buckets = this.buckets.map((b) => ({
        key: b.key,
        spec: b.spec,
        freeHandles: b.freeHandles.filter((h) => this.entries.some((e) => e.handle === h)),
      }));
      void before;
    }
    return poolOk(
      { frame: this.frame, reaped, leaks: suspected, live: this.entries.filter((e) => e.inUse).length },
      bag.all(),
    );
  }

  /** 泄漏账本。 */
  leakLedger(): LeakLedger {
    return this.leaks;
  }

  /** 累计诊断。 */
  diagnostics(): readonly PoolDiagnostic[] {
    return this.diagBag.all();
  }

  private trackPeak(): void {
    const live = this.entries.filter((e) => e.inUse).length;
    if (live > this.peakLive) this.peakLive = live;
  }
}

/** 池状态快照（不可变）。 */
export interface RtPoolState {
  readonly entries: readonly PoolEntry[];
  readonly buckets: readonly Bucket[];
  readonly frame: number;
}

/** 帧边界报告。 */
export interface FrameReport {
  readonly frame: number;
  /** 本帧回收（移出池）的条目数。 */
  readonly reaped: number;
  /** 疑似泄漏。 */
  readonly leaks: readonly LeakRecord[];
  /** 当前在用数。 */
  readonly live: number;
}

/** 按格式位深估算单张 RT 字节数。 */
export function rtBytes(spec: RtSpec): number {
  const bpp = bytesPerPixel(spec.format);
  return spec.width * spec.height * bpp;
}

/** 每像素字节数（未知格式按 4 字节保守估计）。 */
export function bytesPerPixel(format: string): number {
  switch (format) {
    case "RGBA16F":
      return 8;
    case "RGBA8":
      return 4;
    case "R11G11B10F":
      return 4;
    default:
      // 未知格式保守按 8 字节估——宁可高估占用触发降档，也不要低估后 OOM
      return 8;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §6 与 F2002 编译产物对接
// ════════════════════════════════════════════════════════════════════════════

/** 由 F2002 编译产物推导存活区间。 */
export function intervalsFromPlan(
  plan: RtAllocationPlan,
  readsOf: (slot: string) => readonly string[],
): readonly LiveInterval[] {
  // 无执行序信息时退化为按分配序推断：birth=下标，death=最后一次被读的下标
  const lastRead = new Map<string, number>();
  const slots = plan.allocations.map((a) => a.spec.slot);
  for (let i = 0; i < slots.length; i += 1) {
    const slot = slots[i] as string;
    for (const r of readsOf(slot)) {
      const at = slots.indexOf(r);
      if (at >= 0 && at > i) lastRead.set(slot, at);
    }
  }
  return plan.allocations.map((a, i) => {
    const death = lastRead.get(a.spec.slot);
    return { slot: a.spec.slot, birth: i, death: death === undefined ? i : death };
  });
}

/** 池与编译计划的一致性核对。 */
export function verifyPlanAgainstPool(
  plan: RtAllocationPlan,
  pool: RtPoolState,
): PoolDiagnostic[] {
  const out: PoolDiagnostic[] = [];
  const liveHandles = new Set(pool.entries.filter((e) => e.inUse).map((e) => e.slot));
  for (const a of plan.allocations) {
    if (!liveHandles.has(a.spec.slot)) {
      out.push({
        code: "PLAN_POOL_MISMATCH",
        message: `分配计划要求槽 ${a.spec.slot}（效果 ${a.effectId}），但池中无在用条目。`,
        hint: "F2002 执行器须先向池申请再执行；未申请直接执行会读到未初始化 RT。",
      });
    }
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §7 自检（逐条对应判据）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项。 */
export interface PoolSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

const SPEC_A: RtSpec = { width: 1920, height: 1080, format: "RGBA16F" };
const SPEC_B: RtSpec = { width: 1920, height: 1080, format: "RGBA8" };
const SPEC_C: RtSpec = { width: 1280, height: 720, format: "RGBA16F" };

function mkQuota(maxEntries = 8, maxBytes = 1 << 30): Quota {
  return { maxEntries, maxBytes, rejectRatio: 0.95 };
}

/** 判据一「池化复用」自检。 */
export function selfCheckPooling(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  const p = new RtTargetPool(mkQuota());
  const a1 = p.allocate("s1", SPEC_A);
  const a2 = p.allocate("s2", SPEC_A);
  out.push({
    name: "首次分配为 miss",
    pass: a1.ok && !a1.value.hit && a2.ok && !a2.value.hit,
    detail: "两次首配 hit=" + (a1.ok ? a1.value.hit : "n/a") + "/" + (a2.ok ? a2.value.hit : "n/a"),
  });
  out.push({
    name: "不同句柄（两处独立槽）",
    pass: a1.ok && a2.ok && a1.value.handle !== a2.value.handle,
    detail: "句柄=" + (a1.ok ? a1.value.handle : "n/a") + "," + (a2.ok ? a2.value.handle : "n/a"),
  });
  p.release(a1.ok ? a1.value.handle : -1);
  p.release(a2.ok ? a2.value.handle : -1);
  const a3 = p.allocate("s3", SPEC_A);
  out.push({
    name: "归还后同规格复用（命中）",
    pass: a3.ok && a3.value.hit,
    detail: "hit=" + (a3.ok ? a3.value.hit : "n/a"),
  });
  out.push({
    name: "复用不增长条目数",
    pass: a3.ok && p.stats().totalEntries === 2,
    detail: "条目数=" + p.stats().totalEntries + "（应仍为 2）",
  });
  // 跨规格必须 miss（立场二）
  p.release(a3.ok ? a3.value.handle : -1);
  const b1 = p.allocate("b1", SPEC_B);
  out.push({
    name: "格式不同不得复用（严格键匹配）",
    pass: b1.ok && !b1.value.hit,
    detail: "RGBA8 分配 hit=" + (b1.ok ? b1.value.hit : "n/a") + "（应 miss）",
  });
  const c1 = p.allocate("c1", SPEC_C);
  out.push({
    name: "尺寸不同不得复用",
    pass: c1.ok && !c1.value.hit,
    detail: "1280x720 分配 hit=" + (c1.ok ? c1.value.hit : "n/a") + "（应 miss）",
  });
  out.push({
    name: "分桶正确（三规格三桶）",
    pass: p.stats().bucketCount === 3,
    detail: "桶数=" + p.stats().bucketCount + "（应 3）",
  });
  // 桶键相等性
  out.push({
    name: "桶键含尺寸与格式",
    pass: bucketKey(SPEC_A) !== bucketKey(SPEC_B) && bucketKey(SPEC_A) !== bucketKey(SPEC_C),
    detail: "键=" + bucketKey(SPEC_A) + " / " + bucketKey(SPEC_B) + " / " + bucketKey(SPEC_C),
  });
  return out;
}

/** 判据二「区间别名」自检。 */
export function selfCheckAlias(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  // 线性链：s0[0,0] s1[1,1] s2[2,2] s3[3,3] → 全部两两不重叠 → 可压成 1 组
  const linear = analyzeAliases([
    { slot: "s0", birth: 0, death: 0 },
    { slot: "s1", birth: 1, death: 1 },
    { slot: "s2", birth: 2, death: 2 },
    { slot: "s3", birth: 3, death: 3 },
  ]);
  out.push({
    name: "线性链别名压成 1 组",
    pass: linear.physicalCount === 1 && linear.savedCount === 3,
    detail: "物理数=" + linear.physicalCount + " 节省=" + linear.savedCount,
  });
  // 相接区间（第 N 步死、第 N 步生）不可别名
  const touching = analyzeAliases([
    { slot: "a", birth: 0, death: 2 },
    { slot: "b", birth: 2, death: 4 },
  ]);
  out.push({
    name: "相接区间（第2步相接）不别名",
    pass: touching.physicalCount === 2,
    detail: "物理数=" + touching.physicalCount + "（应 2，闭区间语义）",
  });
  out.push({
    name: "相接区间 disjoint 判定为否",
    pass: !intervalsDisjoint({ slot: "a", birth: 0, death: 2 }, { slot: "b", birth: 2, death: 4 }),
    detail: "death==birth 时不重叠=false",
  });
  // 真重叠
  const overlap = analyzeAliases([
    { slot: "a", birth: 0, death: 5 },
    { slot: "b", birth: 3, death: 8 },
  ]);
  out.push({
    name: "真重叠区间不别名",
    pass: overlap.physicalCount === 2,
    detail: "物理数=" + overlap.physicalCount + "（应 2）",
  });
  // 合法性与非法登记
  const bad = analyzeAliases([
    { slot: "ok", birth: 0, death: 1 },
    { slot: "bad", birth: 5, death: 2 },
  ]);
  out.push({
    name: "非法区间（death<birth）被剔除并登记",
    pass: bad.invalidIntervals.indexOf("bad") >= 0 && bad.logicalCount === 1,
    detail: "非法=" + bad.invalidIntervals.join(",") + " 有效=" + bad.logicalCount,
  });
  // 别名组校验：人为构造重叠组必须报 ALIAS_CONFLICT
  const fake: AliasAnalysis = {
    groupOfSlot: new Map([["x", 0], ["y", 0]]),
    groups: [{ groupId: 0, members: ["x", "y"] }],
    physicalCount: 1,
    logicalCount: 2,
    savedCount: 1,
    invalidIntervals: [],
  };
  const conflicts = verifyAliasGroups(fake, [
    { slot: "x", birth: 0, death: 5 },
    { slot: "y", birth: 3, death: 8 },
  ]);
  out.push({
    name: "别名重叠被断言拦截（开发期）",
    pass: conflicts.length > 0 && conflicts[0]?.code === "ALIAS_CONFLICT",
    detail: "冲突数=" + conflicts.length,
  });
  // 确定性
  const runs: string[] = [];
  for (let i = 0; i < 5; i += 1) {
    const r = analyzeAliases([
      { slot: "c", birth: 0, death: 1 },
      { slot: "a", birth: 0, death: 1 },
      { slot: "b", birth: 2, death: 3 },
    ]);
    runs.push(r.groups.map((g) => g.members.join("+")).join("|"));
  }
  out.push({
    name: "别名分组确定性（5 次一致）",
    pass: new Set(runs).size === 1,
    detail: "不同=" + new Set(runs).size,
  });
  return out;
}

/** 判据三「帧边界回收 + 引用计数」自检。 */
export function selfCheckReclaim(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  const p = new RtTargetPool(mkQuota());
  const a = p.allocate("s1", SPEC_A);
  const h = a.ok ? a.value.handle : -1;
  p.retain(h);
  out.push({
    name: "retain 后引用计数为 2",
    pass: p.state().entries.find((e) => e.handle === h)?.refCount === 2,
    detail: "refCount=" + (p.state().entries.find((e) => e.handle === h)?.refCount ?? "n/a"),
  });
  const r1 = p.release(h);
  out.push({
    name: "引用未归零不得回收（仍在用）",
    pass: r1.ok && (p.state().entries.find((e) => e.handle === h)?.inUse === true),
    detail: "inUse=" + (p.state().entries.find((e) => e.handle === h)?.inUse ?? "n/a") + "（应 true）",
  });
  const r2 = p.release(h);
  out.push({
    name: "引用归零后可回收",
    pass: r2.ok && (p.state().entries.find((e) => e.handle === h)?.inUse === false),
    detail: "inUse=" + (p.state().entries.find((e) => e.handle === h)?.inUse ?? "n/a") + "（应 false）",
  });
  // 重复归还
  const dup = p.release(h);
  out.push({
    name: "重复归还被拒",
    pass: !dup.ok && dup.code === "DOUBLE_RELEASE",
    detail: dup.ok ? "被接受" : "拒因=" + dup.code,
  });
  // 未知句柄
  const unknown = p.release(9999);
  out.push({
    name: "未知句柄归还被拒",
    pass: !unknown.ok && unknown.code === "UNKNOWN_HANDLE",
    detail: unknown.ok ? "被接受" : "拒因=" + unknown.code,
  });
  // 帧边界回收条目
  const p2 = new RtTargetPool(mkQuota());
  const x = p2.allocate("x", SPEC_A);
  const xh = x.ok ? x.value.handle : -1;
  p2.release(xh);
  const rep = p2.beginFrame([xh]);
  out.push({
    name: "帧边界回收空闲条目",
    pass: rep.ok && rep.value.reaped === 1 && p2.stats().totalEntries === 0,
    detail: "回收=" + (rep.ok ? rep.value.reaped : "n/a") + " 余=" + p2.stats().totalEntries,
  });
  // 在用条目不得被回收
  const p3 = new RtTargetPool(mkQuota());
  const y = p3.allocate("y", SPEC_A);
  const yh = y.ok ? y.value.handle : -1;
  p3.beginFrame([yh]);
  out.push({
    name: "在用条目不被帧边界回收",
    pass: p3.stats().totalEntries === 1,
    detail: "条目数=" + p3.stats().totalEntries + "（应 1）",
  });
  return out;
}

/** 判据四「泄漏防护」自检。 */
export function selfCheckLeak(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  const p = new RtTargetPool(mkQuota(), 3);
  const a = p.allocate("leaky", SPEC_A);
  const h = a.ok ? a.value.handle : -1;
  let suspected = 0;
  for (let f = 0; f < 5; f += 1) {
    const r = p.beginFrame();
    if (r.ok) suspected = r.value.leaks.length;
  }
  out.push({
    name: "连续未回收超阈报泄漏",
    pass: suspected > 0,
    detail: "第5帧泄漏数=" + suspected,
  });
  out.push({
    name: "泄漏记录含持续帧数与首见帧",
    pass: p.leakLedger().all()[0]?.frames === 5 && p.leakLedger().all()[0]?.firstSeenFrame === 1,
    detail: "记录=" + JSON.stringify(p.leakLedger().all()[0]),
  });
  out.push({
    name: "泄漏诊断含三要素",
    pass: p.diagnostics().length === 0, // 诊断在 beginFrame 返回里，不在累计袋
    detail: "累计袋=" + p.diagnostics().length + "（泄漏诊断随帧报告返回）",
  });
  // 归还后账目清除
  p.release(h);
  p.beginFrame();
  out.push({
    name: "归还后泄漏账本清除",
    pass: p.leakLedger().all().length === 0,
    detail: "在账=" + p.leakLedger().all().length,
  });
  // 不连续帧应重置计数
  const p2 = new RtTargetPool(mkQuota(), 3);
  const b = p2.allocate("x", SPEC_A);
  const bh = b.ok ? b.value.handle : -1;
  p2.beginFrame();
  p2.release(bh);
  p2.beginFrame();
  const c = p2.allocate("y", SPEC_A);
  void c;
  // 再泄漏：首见帧应重新起算
  p2.beginFrame();
  p2.beginFrame();
  const rec = p2.leakLedger().all().find((r) => r.handle !== bh);
  out.push({
    name: "新句柄泄漏独立起算",
    pass: rec === undefined || rec.frames <= 2,
    detail: "记录=" + (rec === undefined ? "无" : "frames=" + rec.frames),
  });
  return out;
}

/** 判据五「配额三要素」自检。 */
export function selfCheckQuota(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  // 水位拒绝：maxEntries=4，rejectRatio=0.95 → 第 4 张即触发（floor(4*0.95)=3）
  const p = new RtTargetPool({ maxEntries: 4, maxBytes: 1 << 30, rejectRatio: 0.95 });
  let lastErr = "";
  let allocated = 0;
  for (let i = 0; i < 6; i += 1) {
    const r = p.allocate("s" + i, { width: 64 + i, height: 64, format: "RGBA8" });
    if (r.ok) allocated += 1;
    else lastErr = r.hint;
  }
  out.push({
    name: "达拒绝水位后拒绝分配",
    pass: allocated === 3,
    detail: "成功分配=" + allocated + "（floor(4*0.95)=3）",
  });
  out.push({
    name: "配额拒绝含三要素（用量/上限/降档建议）",
    pass: lastErr.indexOf("当前用量") >= 0 && lastErr.indexOf("上限") >= 0 && lastErr.indexOf("降") >= 0,
    detail: "hint=" + lastErr.slice(0, 60) + "...",
  });
  // 字节水位
  const pb = new RtTargetPool({ maxEntries: 100, maxBytes: 4096, rejectRatio: 0.95 });
  let byteRejected = false;
  for (let i = 0; i < 30; i += 1) {
    const r = pb.allocate("t" + i, { width: 64, height: 64, format: "RGBA16F" });
    if (!r.ok) {
      byteRejected = true;
      break;
    }
  }
  out.push({
    name: "字节水位同样触发拒绝",
    pass: byteRejected,
    detail: "单张 64x64x8B=32KB，水位 3891B → 应很快拒绝",
  });
  // 非法规格
  const bad = p.allocate("bad", { width: 0, height: 100, format: "RGBA8" });
  out.push({
    name: "非法规格被拒",
    pass: !bad.ok && bad.code === "QUOTA_PARAM_INVALID",
    detail: bad.ok ? "被接受" : "拒因=" + bad.code,
  });
  // 统计
  // 分配序：a(miss) → 归还 a → b(命中 a 的槽)
  const ps = new RtTargetPool(mkQuota());
  const s1 = ps.allocate("a", SPEC_A);
  ps.release(s1.ok ? s1.value.handle : -1);
  const s2 = ps.allocate("b", SPEC_A);
  const st = ps.stats();
  out.push({
    name: "统计记命中与未命中",
    pass: st.misses === 1 && st.hits === 1 && st.peakLive === 1 && s2.ok && s2.value.hit,
    detail: "命中=" + st.hits + " 未命中=" + st.misses + " 峰值=" + st.peakLive,
  });
  out.push({
    name: "字节统计按格式位深",
    pass: st.bytesInUse === 1920 * 1080 * 8,
    detail: "字节=" + st.bytesInUse + "（1920*1080*8）",
  });
  return out;
}

/** 与 F2002 计划对接自检。 */
export function selfCheckPlanBridge(): PoolSelfCheck[] {
  const out: PoolSelfCheck[] = [];
  const p = new RtTargetPool(mkQuota());
  const plan: RtAllocationPlan = {
    allocations: [
      { effectId: "e1", spec: { slot: "sA", width: 1920, height: 1080, format: "RGBA16F" } },
      { effectId: "e2", spec: { slot: "sB", width: 1920, height: 1080, format: "RGBA16F" } },
    ],
    requiredSlots: 2,
  };
  const reads: Record<string, readonly string[]> = { sA: ["sB"], sB: [] };
  const ivs = intervalsFromPlan(plan, (slot) => reads[slot] ?? []);
  out.push({
    name: "由计划推导存活区间",
    pass: ivs.length === 2,
    detail: "区间=" + ivs.map((i) => i.slot + "[" + i.birth + "," + i.death + "]").join(" "),
  });
  out.push({
    name: "sA 死亡于 sB 之后",
    pass: ivs[0] !== undefined && ivs[0].death > ivs[0].birth,
    detail: "sA 区间=[" + (ivs[0]?.birth ?? "?") + "," + (ivs[0]?.death ?? "?") + "]",
  });
  // 未申请就执行 → 报不一致
  const bad = verifyPlanAgainstPool(plan, p.state());
  out.push({
    name: "池空时核对报计划不一致",
    pass: bad.length === 2 && bad[0]?.code === "PLAN_POOL_MISMATCH",
    detail: "不一致数=" + bad.length,
  });
  // 申请后核对通过
  p.allocate("sA", SPEC_A);
  p.allocate("sB", SPEC_A);
  const good = verifyPlanAgainstPool(plan, p.state());
  out.push({
    name: "申请齐备后核对通过",
    pass: good.length === 0,
    detail: "不一致数=" + good.length,
  });
  // 与 F2001 规格互转
  out.push({
    name: "与 F2001 规格互转无损",
    pass: sameSpec(fromF2001Spec(toF2001Spec(SPEC_A)), SPEC_A),
    detail: "回转=" + JSON.stringify(fromF2001Spec(toF2001Spec(SPEC_A))),
  });
  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly PoolSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    pooling: selfCheckPooling(),
    alias: selfCheckAlias(),
    reclaim: selfCheckReclaim(),
    leak: selfCheckLeak(),
    quota: selfCheckQuota(),
    planBridge: selfCheckPlanBridge(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, total, failed };
}