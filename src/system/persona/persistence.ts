/**
 * F161/F151 持久化深化 · 版本化 KV 存储模型（原子写 + 校验和 + 迁移链 + 配额）。
 *
 * 主册判据延伸：
 * - F161「导出-导入 round-trip 逐项等值」「中断原子性实测」——原子性的
 *   存储侧模型：写临时槽 → 校验 → 提交指针翻转（断电只会落在旧值或新值，
 *   永不半空）；
 * - 十二章「永远不丢确定性：升级不破坏旧数据」——版本化信封 + 迁移链
 *   注册表（逐版本小步迁移，任何一版可回放）；
 * - 十三章补「日志系统启动之前的早期失败」——读路径自带校验和与损坏
 *   重建路径（损坏显性化 + 从默认值重建，不静默吃掉）。
 * 纯逻辑模型：物理 IO 由壳层适配（store.ts 的 localStorage 即一个适配器）。
 */

// ---------- 信封（版本 + 校验和 + 时间戳） ----------

export interface Envelope<T> {
  /** 槽位名（如 "persona/tokens"）。 */
  slot: string;
  version: number;
  payload: T;
  /** CRC32 校验和（对 version+payload 序列化计算——损坏必检出）。 */
  checksum: number;
  savedAt: number;
}

function stableStringify(v: unknown): string {
  if (v === null || typeof v !== "object") return JSON.stringify(v) ?? "null";
  if (Array.isArray(v)) return `[${v.map(stableStringify).join(",")}]`;
  const entries = Object.entries(v as Record<string, unknown>).sort(([a], [b]) => a.localeCompare(b));
  return `{${entries.map(([k, val]) => `${JSON.stringify(k)}:${stableStringify(val)}`).join(",")}}`;
}

/** CRC-32 与 png-encode 同表（一处一事实——导入复用避免第二份实现）。 */
import { crc32 } from "./png-encode";

export function sealEnvelope<T>(slot: string, version: number, payload: T, now: number): Envelope<T> {
  const checksum = crc32(new TextEncoder().encode(stableStringify({ slot, version, payload })));
  return { slot, version, payload, checksum, savedAt: now };
}

export type UnsealResult<T> = { ok: true; envelope: Envelope<T> } | { ok: false; reason: string };

/** 拆信验证：槽位/版本/校验和三重对——任何不符都显性拒绝（零静默）。 */
export function unsealEnvelope<T>(raw: unknown): UnsealResult<T> {
  if (typeof raw !== "object" || raw === null) return { ok: false, reason: "信封不是对象——数据损坏或非本域产物" };
  const e = raw as Partial<Envelope<T>>;
  if (typeof e.slot !== "string" || typeof e.version !== "number" || typeof e.checksum !== "number") {
    return { ok: false, reason: "信封字段缺失（slot/version/checksum）" };
  }
  const recomputed = crc32(new TextEncoder().encode(stableStringify({ slot: e.slot, version: e.version, payload: e.payload })));
  if (recomputed !== e.checksum) return { ok: false, reason: `校验和不符（存 ${e.checksum} ≠ 算 ${recomputed}）——数据已损坏` };
  if (typeof e.savedAt !== "number") return { ok: false, reason: "savedAt 缺失——信封不完整" };
  return { ok: true, envelope: e as Envelope<T> };
}

// ---------- 原子写两阶段（模型层——适配器照此执行） ----------

export type AtomicPhase = "write-tmp" | "verify" | "commit" | "done";

export interface AtomicPlan<T> {
  slot: string;
  phases: Array<{ phase: AtomicPhase; action: string }>;
  /** 断电恢复表：每个阶段中断后下一步该做什么（永不半空的执行细则）。 */
  recovery: Array<{ crashedAt: AtomicPhase; do: "keep-old" | "promote-tmp" | "delete-tmp" }>;
  payload: T;
}

export function planAtomicWrite<T>(slot: string, version: number, payload: T, now: number): AtomicPlan<T> {
  const envelope = sealEnvelope(slot, version, payload, now);
  return {
    slot,
    phases: [
      { phase: "write-tmp", action: `写临时槽 ${slot}.tmp = ${JSON.stringify(envelope).length}B 信封` },
      { phase: "verify", action: "回读临时槽并 unsealEnvelope 校验" },
      { phase: "commit", action: `提交指针翻转：${slot} → ${slot}.tmp 内容，删除 tmp` },
      { phase: "done", action: "完成（旧值/新值二选一，永不半空）" },
    ],
    recovery: [
      { crashedAt: "write-tmp", do: "keep-old" },
      { crashedAt: "verify", do: "delete-tmp" },
      { crashedAt: "commit", do: "promote-tmp" },
      { crashedAt: "done", do: "keep-old" },
    ],
    payload,
  };
}

// ---------- 迁移链（逐版本小步——任何一版可回放可测试） ----------

export type Migration<T> = (old: unknown) => T;

export interface MigrationEntry<T> {
  from: number;
  to: number;
  migrate: Migration<T>;
  /** 迁移说明（降级清单前置可见——F161 同源）。 */
  note: string;
}

export class MigrationChain<T> {
  private links: MigrationEntry<T>[] = [];

  register(from: number, to: number, migrate: Migration<T>, note: string): this {
    if (to !== from + 1) throw new Error(`迁移链必须逐版本小步：${from}→${to} 跳版`);
    this.links.push({ from, to, migrate, note });
    return this;
  }

  /** 从任意旧版本迁到目标版本；无路可走显性失败（带走了哪些版本）。 */
  migrate(raw: unknown, fromVersion: number, targetVersion: number): { ok: true; data: T; applied: string[] } | { ok: false; reason: string } {
    let data = raw;
    const applied: string[] = [];
    let v = fromVersion;
    while (v < targetVersion) {
      const link = this.links.find((l) => l.from === v);
      if (!link) return { ok: false, reason: `无 ${v}→${v + 1} 迁移链——版本过旧，请先在原环境升级` };
      try {
        data = link.migrate(data);
        applied.push(`${v}→${v + 1}: ${link.note}`);
        v = link.to;
      } catch (err) {
        return { ok: false, reason: `迁移 ${v}→${v + 1} 异常：${String(err)}（原始输入未丢弃）` };
      }
    }
    return { ok: true, data: data as T, applied };
  }
}

// ---------- 配额账（十二章「诚实透明：占用了什么资源」） ----------

export interface SlotUsage {
  slot: string;
  bytes: number;
  updatedAt: number;
}

export interface QuotaVerdict {
  usedBytes: number;
  limitBytes: number;
  within: boolean;
  /** 最大的三个槽（清理建议的数据源）。 */
  top3: SlotUsage[];
}

export function quotaVerdict(usages: SlotUsage[], limitBytes: number): QuotaVerdict {
  const usedBytes = usages.reduce((s, u) => s + u.bytes, 0);
  const top3 = [...usages].sort((a, b) => b.bytes - a.bytes).slice(0, 3);
  return { usedBytes, limitBytes, within: usedBytes <= limitBytes, top3 };
}

/** 老槽清理候选：超过 maxAgeMs 且不在钉选名单（可清 ≠ 自动清——显性列表）。 */
export function cleanupCandidates(usages: SlotUsage[], now: number, maxAgeMs: number, pinned: Set<string>): SlotUsage[] {
  return usages.filter((u) => now - u.updatedAt > maxAgeMs && !pinned.has(u.slot));
}
