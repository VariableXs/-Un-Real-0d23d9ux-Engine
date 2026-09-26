/**
 * 十三章 文案与语言 · labels 双语机检 + 页组字面量迁移清单。
 *
 * 主册判据延伸：
 * - 「全量文案走查：准确、简洁、有人味，标点不混用，术语前后统一」——
 *   机检三件：zh/en 键集一致性、页面 t() 引用完整性（引用了不存在的键 =
 *   运行期 undefined 上屏）、硬编码字面量迁移清单（漏网之鱼逐条列出）；
 * - 引擎零 fs 依赖：页面源码由调用方喂入（CI 与运行时同消费）。
 */

// ---------- labels 键集机检（数据由 labels.ts 的两个语言表喂入） ----------

export interface LabelsAuditResult {
  /** zh 有 en 没有 / en 有 zh 没有。 */
  zhOnly: string[];
  enOnly: string[];
  /** 空值键（翻译占位 = 未翻）。 */
  emptyValues: string[];
  ok: boolean;
}

export function auditLabels(zh: Record<string, string>, en: Record<string, string>): LabelsAuditResult {
  const zhKeys = new Set(Object.keys(zh));
  const enKeys = new Set(Object.keys(en));
  const zhOnly = [...zhKeys].filter((k) => !enKeys.has(k));
  const enOnly = [...enKeys].filter((k) => !zhKeys.has(k));
  const emptyValues = [...zhKeys].filter((k) => !(zh[k] ?? "").trim() || !(en[k] ?? "").trim());
  return { zhOnly, enOnly, emptyValues, ok: zhOnly.length === 0 && enOnly.length === 0 && emptyValues.length === 0 };
}

// ---------- 页面 t() 引用完整性（引用了不存在的键 = 缺陷） ----------

export interface LabelRef {
  file: string;
  key: string;
  line: number;
}

/** 从页面源码提取 t("key") 引用（正则确定性——ES 模板串 t(`...`) 不支持，按约定禁用）。 */
export function extractLabelRefs(files: Record<string, string>): LabelRef[] {
  const refs: LabelRef[] = [];
  const re = /\bt\("([a-zA-Z0-9_-]+)"/g;
  for (const [file, content] of Object.entries(files)) {
    content.split("\n").forEach((line, i) => {
      for (const m of line.matchAll(re)) {
        refs.push({ file, key: m[1]!, line: i + 1 });
      }
    });
  }
  return refs;
}

export interface RefAuditResult {
  missing: LabelRef[];
  checked: number;
  ok: boolean;
}

/** 引用完整性：每个 t() 引用的键必须在 labels 里（任一语言缺 = 缺陷）。 */
export function auditLabelRefs(refs: LabelRef[], zh: Record<string, string>, en: Record<string, string>): RefAuditResult {
  const missing = refs.filter((r) => !(r.key in zh) || !(r.key in en));
  return { missing, checked: refs.length, ok: missing.length === 0 };
}

// ---------- 硬编码字面量迁移清单（漏网之鱼逐条列出——收尾冲刺的工作单） ----------

export interface HardcodedFinding {
  file: string;
  line: number;
  /** 字面量内容（截断 40 字符）。 */
  snippet: string;
  /** 属性位分类（label= / title= / sub= / 其他）。 */
  slot: string;
}

const HARDCODED_RE = /(label|title|sub|placeholder|ariaLabel)="([^"]*[\u4e00-\u9fff][^"]*)"/g;

/** 扫描页组里硬编码的中文字面量属性（迁移到 labels 的候选工作单——槽位由正则捕获组直接给出）。 */
export function findHardcodedCopy(files: Record<string, string>): { findings: HardcodedFinding[]; total: number } {
  const findings: HardcodedFinding[] = [];
  for (const [file, content] of Object.entries(files)) {
    content.split("\n").forEach((line, i) => {
      for (const m of line.matchAll(HARDCODED_RE)) {
        findings.push({ file, line: i + 1, snippet: m[2]!.slice(0, 40), slot: m[1]! });
      }
    });
  }
  return { findings, total: findings.length };
}

/** 迁移优先级：状态类（label/sub）P1、装饰类（title）P2——收尾冲刺的顺序表。 */
export function migrationPriority(findings: HardcodedFinding[]): Array<{ snippet: string; priority: "P1" | "P2"; count: number }> {
  const bySnippet = new Map<string, number>();
  for (const f of findings) {
    bySnippet.set(f.snippet, (bySnippet.get(f.snippet) ?? 0) + 1);
  }
  return [...bySnippet.entries()]
    .map(([snippet, count]) => {
      const f = findings.find((x) => x.snippet === snippet)!;
      return { snippet, priority: (f.slot === "title" ? "P2" : "P1") as "P1" | "P2", count };
    })
    .sort((a, b) => (a.priority === b.priority ? b.count - a.count : a.priority === "P1" ? -1 : 1));
}
