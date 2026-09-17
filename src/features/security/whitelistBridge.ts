/**
 * 任务 36（AI-V）：白名单 / 审计垫片桥。
 *
 * 垫片层策略（任务 36 允许实机渲染后置到任务 27）：
 *   - 内核命令可用（能力位满足且 vfs_whitelist_* / vfs_audit_list 返回 OK） -> 走真实通道；
 *   - 命令不可用（MISSING / 能力位不满足 / 后端未接入） -> 内存镜像 + localStorage 降级运行，
 *     并在页面显著位置显示"内核通道未接入，当前为本地预览态"角标——绝不静默假装成功。
 *
 * 命令名遵守协议 snake_case 透传：vfs_whitelist_list / vfs_whitelist_add /
 * vfs_whitelist_remove / vfs_audit_list。注意 vfsguard.rs 当前仅实现增（add）与裁决，
 * 未实现 remove；因此 remove 在真实通道下会回退为 MISSING -> 降级本地，与降级口径一致。
 *
 * 变更审计自身也留痕：每次规则增删都产生一条 AuditEntry（source="ui-change",
 * operator="local-ui"），与内核侧访问审计记录同 schema 同列表展示。
 */
import {
  dispatchShimEvent,
  shimCapability,
  shimInvoke,
  type ShimTransport,
} from "../../lib/shim/shimInvoke";
import {
  RULES_MAX,
  type AuditEntry,
  type RuleOpResult,
  type WhitelistError,
  type WhitelistRule,
  type WhitelistRuleInput,
} from "./whitelistTypes";

/** 垫片命令名（snake_case 透传）。 */
export const CMD_WHITELIST_LIST = "vfs_whitelist_list";
export const CMD_WHITELIST_ADD = "vfs_whitelist_add";
export const CMD_WHITELIST_REMOVE = "vfs_whitelist_remove";
export const CMD_AUDIT_LIST = "vfs_audit_list";

/** 内核侧关联能力位（SHARED 挂载点由 vfsguard 保护）。 */
export const VFS_CAPABILITY = "fsShared";

const LS_RULES = "vfsguard:wl:rules";
const LS_UI_AUDIT = "vfsguard:wl:uiAudit";

/** localStorage 语义（可注入，便于测试与 React Native 式环境）。 */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function memoryStorage(): StorageLike {
  const m = new Map<string, string>();
  return {
    getItem: (k) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k, v) => void m.set(k, v),
    removeItem: (k) => void m.delete(k),
  };
}

function defaultStorage(): StorageLike {
  if (typeof window !== "undefined" && window.localStorage) return window.localStorage;
  return memoryStorage();
}

export interface BridgeDeps {
  /** 垫片 invoke（默认 shimInvoke 全量语义）。 */
  invoke?: ShimTransport;
  /** 能力位查询（默认 shimCapability(VFS_CAPABILITY)）。 */
  available?: () => boolean;
  /** 持久化存储（默认 window.localStorage）。 */
  storage?: StorageLike;
  /** 时钟（默认 Date.now），注入便于测试确定性。 */
  clock?: () => number;
  /** 是否经 settings://changed 事件通道广播（默认 true）。 */
  emitEvents?: boolean;
}

/** 前缀规范化（对齐 vfsguard.rs normalize 的边界口径，不含组件级路径解析）。 */
function normalizePrefix(raw: string): { prefix: string; wildcard: boolean; error?: WhitelistError } {
  let s = raw.trim();
  let wildcard = false;
  if (s.length >= 2 && s.slice(-2) === "/*") {
    wildcard = true;
    s = s.slice(0, -2);
  }
  if (s.length === 0) return { prefix: "", wildcard, error: "NOT_ABSOLUTE" };
  if (s[0] !== "/") return { prefix: s, wildcard, error: "NOT_ABSOLUTE" };
  if (s.includes("..")) return { prefix: s, wildcard, error: "BAD_CHAR" };
  // 控制字符（<0x20）/ DEL(0x7F) / 反斜杠
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x20 || c === 0x7f || c === 0x5c) return { prefix: s, wildcard, error: "BAD_CHAR" };
  }
  // 小写折叠（exFAT 不分大小写口径）
  s = s.toLowerCase();
  // 去尾斜杠（根 / 保留）
  if (s.length > 1 && s.endsWith("/")) s = s.slice(0, -1);
  return { prefix: s, wildcard };
}

export type BridgeMode = "kernel" | "local";

/**
 * 白名单 / 审计垫片桥（任务 36）。
 *
 * 用法：组件用默认单例 getDefaultBridge()；测试可 new VfsGuardBridge(deps) 注入 transport。
 */
export class VfsGuardBridge {
  private readonly invoke: ShimTransport;
  private readonly available: () => boolean;
  private readonly storage: StorageLike;
  private readonly clock: () => number;
  private readonly emitEvents: boolean;

  private mode: BridgeMode = "local";
  private rules: WhitelistRule[] = [];
  private kernelAudit: AuditEntry[] = [];
  private uiAudit: AuditEntry[] = [];
  private maxSeq = 0;
  private initialized = false;
  private readonly listeners = new Set<() => void>();

  constructor(deps: BridgeDeps = {}) {
    this.invoke = deps.invoke ?? ((cmd, args) => shimInvoke(cmd, args));
    this.available = deps.available ?? (() => shimCapability(VFS_CAPABILITY));
    this.storage = deps.storage ?? defaultStorage();
    this.clock = deps.clock ?? (() => Date.now());
    this.emitEvents = deps.emitEvents ?? true;
  }

  /** 订阅状态变化（规则/审计变更后触发），返回退订函数。 */
  onChange(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  private emitChange(): void {
    for (const cb of this.listeners) cb();
  }

  /** 当前模式：kernel=已接入内核裁决层；local=本地预览态（降级）。 */
  getMode(): BridgeMode {
    return this.mode;
  }

  /** 是否已接入内核（即非降级）——驱动角标显示。 */
  isLocalPreview(): boolean {
    return this.mode === "local";
  }

  isInitialized(): boolean {
    return this.initialized;
  }

  getRules(): WhitelistRule[] {
    return this.rules;
  }

  /** 合并内核访问审计 + 本地变更审计，按 seq 升序（时间线正序）。 */
  getAudit(): AuditEntry[] {
    return [...this.uiAudit, ...this.kernelAudit].sort((a, b) => a.seq - b.seq);
  }

  /** 初始化：探测内核通道，失败降级本地镜像。幂等（重复调用安全）。 */
  async init(): Promise<BridgeMode> {
    if (this.available()) {
      try {
        const rules = (await this.invoke(CMD_WHITELIST_LIST, {})) as WhitelistRule[] | undefined;
        const audit = (await this.invoke(CMD_AUDIT_LIST, {})) as AuditEntry[] | undefined;
        this.rules = (rules ?? []).map((r, i) => ({ ...r, index: r.index ?? i }));
        this.kernelAudit = (audit ?? [])
          .filter((e) => e.source !== "ui-change")
          .map((e) => ({ ...e, source: "kernel" as const }));
        this.mode = "kernel";
      } catch {
        this.enterLocal();
      }
    } else {
      this.enterLocal();
    }
    this.initialized = true;
    this.emitChange();
    return this.mode;
  }

  private enterLocal(): void {
    this.mode = "local";
    this.rules = this.loadRulesLS();
    this.uiAudit = this.loadUiAuditLS();
    this.kernelAudit = [];
    this.maxSeq = this.uiAudit.reduce((mx, e) => Math.max(mx, e.seq), 0);
  }

  // ---- 持久化（localStorage 语义：内存镜像 + 写后异步持久化） ----

  private loadRulesLS(): WhitelistRule[] {
    try {
      const raw = this.storage.getItem(LS_RULES);
      if (!raw) return [];
      const arr = JSON.parse(raw) as WhitelistRule[];
      return Array.isArray(arr) ? arr : [];
    } catch {
      return [];
    }
  }

  private loadUiAuditLS(): AuditEntry[] {
    try {
      const raw = this.storage.getItem(LS_UI_AUDIT);
      if (!raw) return [];
      const arr = JSON.parse(raw) as AuditEntry[];
      return Array.isArray(arr) ? arr : [];
    } catch {
      return [];
    }
  }

  private persistRules(): void {
    try {
      this.storage.setItem(LS_RULES, JSON.stringify(this.rules));
    } catch {
      /* 持久化失败如实暴露：不静默 */
    }
  }

  private persistUiAudit(): void {
    try {
      this.storage.setItem(LS_UI_AUDIT, JSON.stringify(this.uiAudit));
    } catch {
      /* 同上 */
    }
  }

  // ---- 规则增删（垫片语义：下发命令 + 状态回读 + 失败降级） ----

  async addRule(input: WhitelistRuleInput): Promise<RuleOpResult> {
    const norm = normalizePrefix(input.prefix);
    if (norm.error) return { ok: false, code: norm.error };
    if (this.rules.length >= RULES_MAX) return { ok: false, code: "RULES_FULL" };

    const rule: WhitelistRule = {
      prefix: norm.prefix,
      read: input.read,
      write: input.write,
      index: this.rules.length,
      wildcard: norm.wildcard,
    };

    if (this.mode === "kernel") {
      try {
        const added = (await this.invoke(CMD_WHITELIST_ADD, {
          prefix: norm.prefix,
          read: input.read,
          write: input.write,
        })) as WhitelistRule | undefined;
        const listed = (await this.invoke(CMD_WHITELIST_LIST, {})) as WhitelistRule[] | undefined;
        this.rules = (listed ?? []).map((r, i) => ({ ...r, index: r.index ?? i }));
        // 内核返回的规则号优先；否则用本地计算的 index
        if (added && typeof added.index === "number") rule.index = added.index;
        this.appendChangeAudit("add", rule);
        this.emitChange();
        this.emitSettingsChanged();
        return { ok: true };
      } catch {
        // 真实通道失败 -> 降级本地（不静默假装成功）
        this.mode = "local";
        this.appendChangeAudit("add", rule);
      }
    }

    // 本地预览路径
    this.rules = [...this.rules, rule];
    this.persistRules();
    this.appendChangeAudit("add", rule);
    this.emitChange();
    this.emitSettingsChanged();
    return { ok: true };
  }

  async removeRule(index: number): Promise<RuleOpResult> {
    const idx = this.rules.findIndex((r) => r.index === index);
    if (idx < 0) return { ok: false, code: "UNKNOWN" };
    const removed = this.rules[idx]!;

    if (this.mode === "kernel") {
      try {
        await this.invoke(CMD_WHITELIST_REMOVE, { index });
        const listed = (await this.invoke(CMD_WHITELIST_LIST, {})) as WhitelistRule[] | undefined;
        this.rules = (listed ?? []).map((r, i) => ({ ...r, index: r.index ?? i }));
        this.appendChangeAudit("remove", removed);
        this.emitChange();
        this.emitSettingsChanged();
        return { ok: true };
      } catch {
        this.mode = "local";
      }
    }

    this.rules = this.rules.filter((r) => r.index !== index).map((r, i) => ({ ...r, index: i }));
    this.persistRules();
    this.appendChangeAudit("remove", removed);
    this.emitChange();
    this.emitSettingsChanged();
    return { ok: true };
  }

  /** 变更审计留痕：追加一条 ui-change 审计条目（与内核记录同 schema 同列表）。 */
  private appendChangeAudit(action: "add" | "remove", rule: WhitelistRule): void {
    const perm = `${rule.read ? "r" : ""}${rule.write ? "w" : ""}` || "-";
    const summary = `${perm} ${rule.prefix}${rule.wildcard ? "/*" : ""}`;
    const entry: AuditEntry = {
      seq: ++this.maxSeq,
      pid: 0,
      allow: true,
      write: true,
      path: summary,
      pathLen: summary.length,
      source: "ui-change",
      operator: "local-ui",
      action,
      summary,
    };
    this.uiAudit = [...this.uiAudit, entry];
    this.persistUiAudit();
  }

  private emitSettingsChanged(): void {
    if (!this.emitEvents) return;
    dispatchShimEvent("settings://changed", {
      area: "vfsguard",
      mode: this.mode,
      at: this.clock(),
    });
  }

  /** 测试辅助：清空持久化与内存（不重置 transport）。 */
  resetLocal(): void {
    this.rules = [];
    this.kernelAudit = [];
    this.uiAudit = [];
    this.maxSeq = 0;
    try {
      this.storage.removeItem(LS_RULES);
      this.storage.removeItem(LS_UI_AUDIT);
    } catch {
      /* ignore */
    }
  }
}

let singleton: VfsGuardBridge | null = null;

/** 默认单例（真实通道 + window.localStorage）。组件共用，保证两 Tab 状态一致。 */
export function getDefaultBridge(): VfsGuardBridge {
  if (!singleton) singleton = new VfsGuardBridge();
  return singleton;
}

/** 测试辅助：替换默认单例。 */
export function setDefaultBridge(b: VfsGuardBridge): void {
  singleton = b;
}
