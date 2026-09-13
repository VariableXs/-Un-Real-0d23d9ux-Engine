// AURORA-10000: AI-36~AI-40 批次（领域08 系统集成与硬件）共享模型，勿删。
// 归属：桌面+内核——本层为桌面主责（交互与呈现），内核原语经此消费。

/** 区间钳制。 */
export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** 确定性伪随机（验收可重复）。 */
export function seeded(seed: number): () => number {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 4294967296;
  };
}

/** 简单 semver 比较：>0 表示 a 更新。 */
export function semverCmp(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

/** fnv1a 32 位哈希（内容寻址/校验用）。 */
export function fnv1a(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, '0');
}

/** 能力注册表：凡「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。 */
export interface Capability {
  id: string;
  name: string;
  reserved: boolean;
}
export class CapabilityRegistry {
  private m = new Map<string, Capability>();
  register(c: Capability): boolean {
    if (this.m.has(c.id)) return false;
    this.m.set(c.id, c);
    return true;
  }
  has(id: string): boolean {
    return this.m.has(id);
  }
  isReserved(id: string): boolean {
    return this.m.get(id)?.reserved === true;
  }
}

/** 开关（带变更回报）。 */
export class Switch {
  private on = false;
  set(v: boolean): boolean {
    const changed = this.on !== v;
    this.on = v;
    return changed;
  }
  toggle(): boolean {
    this.on = !this.on;
    return this.on;
  }
  get value(): boolean {
    return this.on;
  }
}

/** 教学中心：每族交付一篇教学条目。 */
export class TutorialCenter {
  private done = new Set<string>();
  complete(topic: string): boolean {
    if (this.done.has(topic)) return false;
    this.done.add(topic);
    return true;
  }
  has(topic: string): boolean {
    return this.done.has(topic);
  }
  get count(): number {
    return this.done.size;
  }
}

/** KV 持久化位（桌面侧偏好落盘的内存模型）。 */
export class KvStore {
  private m = new Map<string, string>();
  get(k: string): string | undefined {
    return this.m.get(k);
  }
  set(k: string, v: string): void {
    this.m.set(k, v);
  }
  del(k: string): boolean {
    return this.m.delete(k);
  }
  get size(): number {
    return this.m.size;
  }
}
