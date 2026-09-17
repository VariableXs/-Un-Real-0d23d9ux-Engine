/**
 * 任务 36（AI-V）：白名单 / 审计数据模型。
 *
 * 字段命名与内核侧 kernel/varix/src/vfsguard.rs 逐字段对照，并在注释中标注对应
 * 的 Rust 结构与成员。UI 数据模型必须与内核侧逐字段对齐（任务 36 验收口径）。
 *
 * vfsguard.rs 关键常量：
 *   PATH_MAX = 256           目录前缀规范化上限（含根斜杠）
 *   DEPTH_MAX = 16           路径深度上限（组件数）
 *   RULES_MAX = 32           规则上限
 *   AUDIT_PATH_MAX = 80      审计记录内路径截断长度
 */

/** vfsguard.rs: RULES_MAX = 32 —— 内核规则表定容。 */
export const RULES_MAX = 32;

/** vfsguard.rs: AUDIT_PATH_MAX = 80 —— 审计记录内路径截断长度。 */
export const AUDIT_PATH_MAX = 80;

/**
 * 白名单规则。
 *
 * 对齐 vfsguard.rs:
 *   struct Rule { prefix: NormPath, read: bool, write: bool }
 *   struct NormPath { b: [u8;256], len, depth }  // 小写折叠、无 ./..、恒以 / 开头
 *
 * 内核 Rule 无 id，按 RuleSet.rules[i] 数组下标定位；UI 镜像用 `index` 自行维护。
 * 末尾 `/*` 通配在内核 add() 中收敛为纯前缀（vfsguard.rs RuleSet::add），UI 用
 * `wildcard` 标记便于展示与回读。
 */
export interface WhitelistRule {
  /** 目录前缀（已规范化：小写折叠、以 / 开头、无 .. 与反斜杠）。对应 NormPath.b 解码为字符串。 */
  prefix: string;
  /** Rule.read —— 是否授予读位。 */
  read: boolean;
  /** Rule.write —— 是否授予写位。 */
  write: boolean;
  /** 内核规则号（RuleSet.add 返回的下标 0..RULES_MAX）。本地镜像自行维护，内核通道下由真实序号填充。 */
  index: number;
  /** 尾部 /* 通配语义（内核解析时收敛为前缀）。仅展示用途，不进入内核比对。 */
  wildcard?: boolean;
}

/** 规则增删入参（前端 → 垫片命令 / 本地镜像）。 */
export interface WhitelistRuleInput {
  /** 目录前缀原文，可带可选尾部 /* 通配。 */
  prefix: string;
  read: boolean;
  write: boolean;
}

/** 规则增删结果（垫片语义：下发命令 + 状态回读 + 失败原因）。 */
export type RuleOpResult =
  | { ok: true }
  | { ok: false; code: WhitelistError };

/** 内核侧对应 vfsguard.rs GuardError / 业务校验错误。 */
export type WhitelistError =
  | "NOT_ABSOLUTE" // 不以 / 开头（GuardError::NotAbsolute）
  | "BAD_CHAR" // 含 .. / 反斜杠 / 控制字符（GuardError::BadChar）
  | "RULES_FULL" // 规则表满（GuardError::RulesFull）
  | "KERNEL_UNREACHABLE" // 内核通道不可用，已降级本地预览
  | "UNKNOWN";

/**
 * 内核越权审计记录。
 *
 * 逐字段对齐 vfsguard.rs:
 *   struct AuditRecord {
 *     seq: u64,            // journal 全局序号（跨会话单调）
 *     pid: u32,            // 进程号
 *     allow: bool,         // 裁决结论
 *     write: bool,         // 操作是否为写（adjudicate 的 op==Op::Write）
 *     path: [u8;80],       // 路径
 *     path_len: usize,     // 路径有效长度（AUDIT_PATH_MAX 截断）
 *   }
 *
 * 内核审计账本由 AuditJournal（tier3 WAL，断电不丢）承载，记录 allow/deny 全量。
 */
export interface VfsAuditRecord {
  /** seq: u64 —— 审计全局序号（单调）。 */
  seq: number;
  /** pid: u32 —— 发起进程号。 */
  pid: number;
  /** allow —— 裁决是否放行。 */
  allow: boolean;
  /** write —— 是否为写操作（Op::Write）。 */
  write: boolean;
  /** path —— 路径字符串（AUDIT_PATH_MAX=80 截断后的解码）。 */
  path: string;
  /** path_len —— 路径有效长度。 */
  pathLen: number;
}

/**
 * 审计条目（统一列表）。
 *
 * 内核访问记录与「变更审计（管理动作留痕）」同 schema 同列表展示（任务 36 验收）。
 * 二者共享 VfsAuditRecord 的全部字段；ui-change 条目额外填充 source/operator/action。
 *
 * 设计原则：内核记录 source="kernel"，operator/action/summary 为 undefined；
 * 管理变更记录 source="ui-change"，operator 恒为 "local-ui"，action 为 add/remove，
 * summary 为规则摘要（如 "rw /handoff"）。这样两类记录在虚拟列表中混排、按 seq 排序。
 */
export interface AuditEntry extends VfsAuditRecord {
  /** 来源：内核 SHARED 访问裁决 / 本地 UI 管理变更。 */
  source: "kernel" | "ui-change";
  /** ui-change：操作者，恒为 "local-ui"。 */
  operator?: string;
  /** ui-change：变更动作。 */
  action?: "add" | "remove";
  /** ui-change：规则摘要（便于展示与 CSV 导出）。 */
  summary?: string;
}

/** 审计动作显示分类（由 allow/write/action 推导）。 */
export type AuditActionKind = "allow-read" | "allow-write" | "deny-read" | "deny-write" | "add" | "remove";

/** 由一条审计条目推导其展示动作（供过滤/列表展示复用）。 */
export function auditActionKind(e: AuditEntry): AuditActionKind {
  if (e.source === "ui-change") return e.action === "remove" ? "remove" : "add";
  if (e.allow) return e.write ? "allow-write" : "allow-read";
  return e.write ? "deny-write" : "deny-read";
}

/** 内核侧对应 vfsguard.rs AuditRecord 的 TS 字段名（pathLen ↔ 内核 path_len）。 */
export const KERNEL_AUDIT_FIELDS = ["seq", "pid", "allow", "write", "path", "pathLen"] as const;
