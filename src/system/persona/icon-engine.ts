/**
 * F155 图标引擎深化 · 缓存 LRU（键含包版本）+ 失效广播总线 + 深度校验 + 版本迁移。
 *
 * 主册判据延伸：
 * - 【设计细节】「缓存键含包版本（F013 图标缓存联动）」「广播失效（各面监听
 *   重取——订阅制不轮询）」「运行中应用窗口图标 → 下次刷新生效（显式说明）」。
 * - F133 规范子集：SVG 资产安全校验（拒绝脚本注入向量——外部输入全清洗）。
 */

import { ICON_CATEGORIES, type IconPack } from "./iconswap";

// ---------- 缓存（LRU，键 = 包版本:类别:尺寸档） ----------

export interface CacheKey {
  packVersion: string;
  category: string;
  size: "16" | "24" | "32" | "48" | "256";
}

export function cacheKey(k: CacheKey): string {
  return `${k.packVersion}:${k.category}:${k.size}`;
}

export interface IconCacheStats {
  count: number;
  capacity: number;
  hits: number;
  misses: number;
}

export class IconCache {
  private map = new Map<string, string>(); // key → dataURL
  private order: string[] = []; // LRU 序（尾=最新）
  private hits = 0;
  private misses = 0;

  constructor(private capacity = 500) {}

  get(k: CacheKey): string | null {
    const key = cacheKey(k);
    const v = this.map.get(key);
    if (v === undefined) {
      this.misses++;
      return null;
    }
    this.hits++;
    // 触碰 LRU。
    this.order = this.order.filter((x) => x !== key);
    this.order.push(key);
    return v;
  }

  put(k: CacheKey, dataUrl: string): void {
    const key = cacheKey(k);
    if (this.map.has(key)) {
      this.order = this.order.filter((x) => x !== key);
    } else if (this.order.length >= this.capacity) {
      const oldest = this.order.shift();
      if (oldest) this.map.delete(oldest);
    }
    this.map.set(key, dataUrl);
    this.order.push(key);
  }

  /** 整包失效（换包/回退广播的执行面——键前缀 = 包版本）。 */
  invalidatePack(packVersion: string): number {
    const prefix = `${packVersion}:`;
    let removed = 0;
    for (const key of [...this.map.keys()]) {
      if (key.startsWith(prefix)) {
        this.map.delete(key);
        removed++;
      }
    }
    this.order = this.order.filter((x) => !x.startsWith(prefix));
    return removed;
  }

  stats(): IconCacheStats {
    return { count: this.map.size, capacity: this.capacity, hits: this.hits, misses: this.misses };
  }
}

// ---------- 失效广播总线（订阅制不轮询） ----------

export type InvalidationReason = "switch" | "rollback" | "uninstall" | "cache-rebuild";

export interface InvalidationEvent {
  reason: InvalidationReason;
  packId: string | null;
  packVersion: string | null;
  at: number;
}

type InvalidationListener = (e: InvalidationEvent) => void;

export class IconInvalidationBus {
  private listeners = new Set<InvalidationListener>();

  subscribe(l: InvalidationListener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  broadcast(e: InvalidationEvent): void {
    for (const l of this.listeners) {
      try {
        l(e);
      } catch (err) {
        // 监听方异常不炸总线（F175 隔离纪律），但绝不静默。
        console.error("[icon bus] 监听器处理失效事件失败", err);
      }
    }
  }
}

/** 全域唯一总线实例。 */
export const iconInvalidationBus = new IconInvalidationBus();

// ---------- 深度校验（F133 规范子集 · SVG 安全） ----------

export interface DeepValidationIssue {
  category: string;
  level: "error" | "warning";
  message: string;
}

const DANGEROUS_SVG_PATTERNS: readonly { re: RegExp; why: string }[] = [
  { re: /<script[\s>]/i, why: "含 <script>——脚本注入向量" },
  { re: /\son\w+\s*=/i, why: "含内联事件处理器（on*）" },
  { re: /javascript:/i, why: "含 javascript: 伪协议" },
  { re: /<foreignObject/i, why: "含 foreignObject（可嵌 HTML）" },
  { re: /data:text\/html/i, why: "含 text/html data URL" },
];

/** 单个 SVG 资产安全审计（字符串级——确定性规则，不做猜测）。 */
export function auditSvgAsset(svg: string): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  for (const p of DANGEROUS_SVG_PATTERNS) {
    if (p.re.test(svg)) issues.push(p.why);
  }
  return { ok: issues.length === 0, issues };
}

/** 整包深度校验：覆盖表每个资产过 SVG 安全审计 + 必要字段非空。 */
export function deepValidatePack(pack: IconPack, assetLoader: (ref: string) => string | null): DeepValidationIssue[] {
  const issues: DeepValidationIssue[] = [];
  for (const [category, ref] of Object.entries(pack.coverage)) {
    if (!ICON_CATEGORIES.includes(category)) {
      issues.push({ category, level: "error", message: "规范外类别" });
      continue;
    }
    if (!ref || ref.length === 0) {
      issues.push({ category, level: "error", message: "资产引用为空" });
      continue;
    }
    const svg = assetLoader(ref);
    if (svg === null) {
      issues.push({ category, level: "warning", message: `资产 ${ref} 不在包内（运行时回退官方项）` });
      continue;
    }
    const audit = auditSvgAsset(svg);
    if (!audit.ok) {
      issues.push({ category, level: "error", message: `SVG 安全审计失败: ${audit.issues.join("；")}` });
    }
  }
  return issues;
}

// ---------- 版本迁移（v1 → v2 覆盖表键改名映射） ----------

/** 迁移规则表（版本化接口——废弃要走流程的机械面）。 */
export const PACK_KEY_MIGRATIONS: readonly { from: string; to: string; sinceVersion: string }[] = [
  { from: "system.computer", to: "system.this-pc", sinceVersion: "1.1" },
  { from: "type.image-raw", to: "type.image", sinceVersion: "1.1" },
];

/** 把旧包覆盖表迁移到当前规范键；冲突（新旧键并存）保留新键。 */
export function migratePackCoverage(coverage: Record<string, string>): { migrated: Record<string, string>; renamed: { from: string; to: string }[] } {
  const migrated: Record<string, string> = { ...coverage };
  const renamed: { from: string; to: string }[] = [];
  for (const m of PACK_KEY_MIGRATIONS) {
    if (m.from in migrated && !(m.to in migrated)) {
      migrated[m.to] = migrated[m.from] ?? "";
      delete migrated[m.from];
      renamed.push({ from: m.from, to: m.to });
    }
  }
  return { migrated, renamed };
}
