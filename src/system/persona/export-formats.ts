/**
 * 十四章 开放性 · 导出格式注册表（schema 化 + 版本承诺 + 废弃流程）。
 *
 * 主册判据延伸：
 * - 「数据开放：用户的数据用开放格式存储、可导出、可迁移，不给用户上锁」
 *   ——E 域全部导出格式的**一处一事实注册表**：格式 id/版本/生产模块/
 *   schema 摘要/向后兼容承诺；
 * - 「接口十年不变：版本化、向后兼容、废弃要走流程」——废弃状态机
 *   （active→deprecated→removed，每态有迁移说明）；
 * - 注册表机检：每个 active 格式必须有生产模块与版本承诺，缺 = 红线。
 */

export type FormatStatus = "active" | "deprecated" | "removed";

export interface ExportFormat {
  /** 格式 id（format 字段的规范值——与各引擎导出的 format 字段对拍）。 */
  id: string;
  version: number;
  /** 生产模块（一处一事实——谁生成谁登记）。 */
  producer: string;
  /** 用途一句话。 */
  purpose: string;
  status: FormatStatus;
  /** schema 关键约束摘要（机器可检的承诺——不是自由文本）。 */
  schemaConstraints: string[];
  /** 版本承诺（向后兼容的具体保证）。 */
  compatibility: string;
  /** 废弃去向（deprecated/removed 必填——不走流程不许废）。 */
  replacedBy?: string;
}

export const EXPORT_FORMATS: ExportFormat[] = [
  {
    id: "vxtheme-profile", version: 1, producer: "store.ts / archive.ts",
    purpose: "个性化全档导出（令牌+配置分节）", status: "active",
    schemaConstraints: ["format 恒为 vxtheme-profile", "sections 分节齐全", "version 整数递增"],
    compatibility: "v1 永久可读；新增字段只加不改名",
  },
  {
    id: "vxtheme-assets", version: 1, producer: "asset-package.ts",
    purpose: "素材包（PNG 字节 + CRC 清单）", status: "active",
    schemaConstraints: ["manifest 与 blobs 等长", "逐条 CRC32", "路径 use 前缀契约"],
    compatibility: "v1 永久可读；条目字段只加不改名",
  },
  {
    id: "vxkeymap", version: 1, producer: "shortcut-engine.ts",
    purpose: "快捷键迁移与备份", status: "active",
    schemaConstraints: ["overrides 键为 actionId", "combo 规范序 Win>Ctrl>Alt>Shift"],
    compatibility: "v1 永久可读；系统保留项导入时跳过并报告",
  },
  {
    id: "vxpointer", version: 1, producer: "pointer.ts",
    purpose: "指针方案（15 枚 + 热点 + 帧）", status: "active",
    schemaConstraints: ["roles 15 枚齐全", "hotspot 1px 级整数", "frames ≤16"],
    compatibility: "v1 永久可读；新增角色走 v2（不进 v1）",
  },
  {
    id: "vx-usage-ranking", version: 1, producer: "usage-model.ts",
    purpose: "使用频率排行分享（隐私分组）", status: "active",
    schemaConstraints: ["items 仅 id/score/rawCount", "零内容字段（结构即隐私）"],
    compatibility: "v1 永久可读",
  },
  {
    id: "vx-interaction-ledger", version: 1, producer: "interaction-ledger.ts",
    purpose: "交互台账导出（体验改进清单）", status: "active",
    schemaConstraints: ["事件七元组无内容位", "挫败信号带人话结论", "stats 含 P95 反馈"],
    compatibility: "v1 永久可读",
  },
  {
    id: "vx-verdict-evidence", version: 1, producer: "verdict-report.ts",
    purpose: "域总检证据包（第三方独立复核）", status: "active",
    schemaConstraints: ["contentHash FNV-1a 全文校验", "items 按 id 排序规范化", "三步证据逐项在案"],
    compatibility: "v1 永久可读；哈希算法变更走 v2",
  },
  {
    id: "vx-ime-skin", version: 1, producer: "imeskin.ts",
    purpose: "输入法皮肤包（JSON Schema 预检）", status: "active",
    schemaConstraints: ["16ms 候选延迟预算字段", "配色跟随主题或独立", "字段越界即拒"],
    compatibility: "v1 永久可读",
  },
  {
    id: "vx-walkcheck-e-checklist", version: 1, producer: "verdict.ts",
    purpose: "域总检清单（脚本工具消费同一份）", status: "active",
    schemaConstraints: ["19 项 id 连续", "每项三步定义"],
    compatibility: "v1 永久可读；新增探针追加不改序",
  },
  {
    id: "vxtheme-profile-v0", version: 0, producer: "（历史）首版令牌导出",
    purpose: "v0 旧档（11 桥迁移）", status: "deprecated",
    schemaConstraints: ["11 个 -- 前缀键", "无 version 戳"],
    compatibility: "导入时自动迁移至 v1（compat-matrix）；导出永远产 v1",
    replacedBy: "vxtheme-profile",
  },
];

/** 注册表机检：active 必须有 producer/承诺；deprecated 必须有去向；removed 必须绝版。 */
export function auditFormatRegistry(): { ok: boolean; issues: string[]; activeCount: number } {
  const issues: string[] = [];
  for (const f of EXPORT_FORMATS) {
    if (f.status === "active" && (!f.producer || f.producer.startsWith("（"))) issues.push(`${f.id}: active 格式 producer 缺失`);
    if (f.status === "active" && f.schemaConstraints.length === 0) issues.push(`${f.id}: active 格式无 schema 约束（开放性红线）`);
    if (f.status !== "active" && !f.replacedBy) issues.push(`${f.id}: ${f.status} 无替代格式——废弃没走流程`);
    if (f.status === "removed") issues.push(`${f.id}: removed 格式仍在注册表占位（应迁移说明化）`);
  }
  return { ok: issues.length === 0, issues, activeCount: EXPORT_FORMATS.filter((f) => f.status === "active").length };
}

/** 导入面路由：给一坨未知 JSON → 认出格式并给处理建议（开放性的门房）。 */
export function recognizeFormat(raw: unknown): { format: ExportFormat | null; advice: string } {
  if (typeof raw !== "object" || raw === null) return { format: null, advice: "不是 JSON 对象——无法识别" };
  const o = raw as Record<string, unknown>;
  const fmt = typeof o.format === "string" ? String(o.format) : null;
  if (!fmt) return { format: null, advice: "无 format 字段——按 vxtheme-profile-v0 尝试迁移（compat-matrix）" };
  const hit = EXPORT_FORMATS.find((f) => f.id === fmt || (fmt === "vxtheme-profile" && f.id === "vxtheme-profile"));
  if (!hit) return { format: null, advice: `未知格式 ${fmt}——不是 E 域产物，拒绝（不猜）` };
  if (hit.status === "deprecated") return { format: hit, advice: `${hit.id} 已废弃——导入将自动迁移至 ${hit.replacedBy}` };
  return { format: hit, advice: `${hit.id} v${hit.version}——${hit.purpose}` };
}

/** 迁移说明生成（十四章"卸载不留垃圾/升级不破坏旧数据"的文档面）。 */
export function migrationNotes(formatId: string): string[] {
  const f = EXPORT_FORMATS.find((x) => x.id === formatId);
  if (!f) return [`未知格式 ${formatId}`];
  const notes = [`${f.id} v${f.version}（${f.status}）：${f.purpose}`, `兼容承诺：${f.compatibility}`];
  if (f.replacedBy) notes.push(`迁移路径：${f.id} → ${f.replacedBy}（导入自动完成，原始输入保真可回退）`);
  notes.push(`schema 约束：${f.schemaConstraints.join("；")}`);
  return notes;
}
