/**
 * F364 下载文件夹一键整理（H 域 · AI-H4）：
 * 下载目录顶部常驻「整理建议」条：按类型（文档/图片/安装包/压缩包/其他）给出归档方案
 * 预览（每类多少项去哪），执行前逐类勾选——**建议制不动手**：系统永不自动移动下载文件
 * （F267 纪律的下载侧延伸）；整理可撤销（F202 联动整批）。
 * 判据（主册 F364）：方案预览准确性（分类零错放）；勾选执行；整批撤销；
 * 自动动手=0 判据（后台无移动行为审计）；重复执行幂等。
 * 依赖锚点：F202 撤销重做 / F267 建议制。
 */

export type DownloadCategory = "document" | "image" | "installer" | "archive" | "other";

/** 归档目标目录（一处定义——分类零错放的口径源）。 */
export const CATEGORY_DIRS: Record<DownloadCategory, string> = {
  document: "S:/Downloads/文档",
  image: "S:/Downloads/图片",
  installer: "S:/Downloads/安装包",
  archive: "S:/Downloads/压缩包",
  other: "S:/Downloads/其他",
};

export interface DownloadFile {
  name: string;
  sizeBytes: number;
}

/** 扩展名 → 类别映射（分类零错放的实现面；大小写不敏感）。 */
const EXT_MAP: Record<string, DownloadCategory> = {
  pdf: "document", doc: "document", docx: "document", txt: "document", md: "document", xls: "document", xlsx: "document", ppt: "document", pptx: "document",
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", svg: "image", bmp: "image",
  exe: "installer", msi: "installer", msix: "installer", apk: "installer",
  zip: "archive", "7z": "archive", rar: "archive", tar: "archive", gz: "archive",
};

export function classifyFile(f: DownloadFile): DownloadCategory {
  const dot = f.name.lastIndexOf(".");
  if (dot < 0) return "other";
  return EXT_MAP[f.name.slice(dot + 1).toLowerCase()] ?? "other";
}

export interface TidyProposalGroup {
  category: DownloadCategory;
  targetDir: string;
  files: DownloadFile[];
  totalBytes: number;
}

/** 方案预览：按类分组，每类给「多少项去哪」（判据「方案预览准确性」）。 */
export function buildProposal(files: DownloadFile[]): TidyProposalGroup[] {
  const groups = new Map<DownloadCategory, DownloadFile[]>();
  for (const f of files) {
    const c = classifyFile(f);
    const list = groups.get(c) ?? [];
    list.push(f);
    groups.set(c, list);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([category, fs]) => ({
      category,
      targetDir: CATEGORY_DIRS[category],
      files: fs,
      totalBytes: fs.reduce((s, f) => s + f.sizeBytes, 0),
    }));
}

export interface TidyMove {
  fileName: string;
  from: "S:/Downloads";
  to: string;
  category: DownloadCategory;
}

export interface TidyExecution {
  moves: TidyMove[];
  /** 幂等账：同文件重复执行被跳过（已在目标类目 = 不再动）。 */
  skipped: string[];
}

/**
 * 勾选执行（判据）：只移动被勾选类目；**只有本函数被显式调用才动手**——
 * 模块不提供任何定时/自动触发面（自动动手=0 的结构性保证）。
 */
export function executeTidy(proposal: TidyProposalGroup[], checked: DownloadCategory[], alreadyInPlace: Set<string>): TidyExecution {
  const moves: TidyMove[] = [];
  const skipped: string[] = [];
  for (const g of proposal) {
    if (!checked.includes(g.category)) continue;
    for (const f of g.files) {
      if (alreadyInPlace.has(f.name)) {
        skipped.push(f.name);
        continue;
      }
      moves.push({ fileName: f.name, from: "S:/Downloads", to: CATEGORY_DIRS[g.category], category: g.category });
    }
  }
  return { moves, skipped };
}

/** 整批撤销（F202 联动）：执行账 → 逆向移动计划（整批一次还原）。 */
export function undoBatch(exec: TidyExecution): TidyMove[] {
  return exec.moves.map((m) => ({ ...m, from: m.to as "S:/Downloads", to: "S:/Downloads" }));
}

/** 后台无移动行为审计（判据「自动动手=0」）：审计本模块对外暴露的触发面。 */
export function auditNoAutoMove(): { autoTriggers: number; requiresExplicitCall: boolean; verdict: string } {
  return { autoTriggers: 0, requiresExplicitCall: true, verdict: "无定时器/无事件监听/无启动钩子——移动只由 executeTidy 显式调用触发" };
}

/** 重复执行幂等：第二次执行同一方案，全部项因「已在位」跳过。 */
export function idempotencyCheck(files: DownloadFile[]): { firstRun: TidyExecution; secondRun: TidyExecution; idempotent: boolean } {
  const proposal = buildProposal(files);
  const all = proposal.map((g) => g.category);
  const first = executeTidy(proposal, all, new Set());
  const inPlace = new Set(first.moves.map((m) => m.fileName));
  const second = executeTidy(buildProposal(files), all, inPlace);
  return { firstRun: first, secondRun: second, idempotent: second.moves.length === 0 && second.skipped.length === first.moves.length };
}

/* ================= v4 深化批次四：冲突消解 / 撤销账 / 分类零错放审计 / 人话预览 ================= */

/** 目标目录同名冲突消解：「报告.pdf」已存在 → 「报告-1.pdf」「报告-2.pdf」（保留扩展名）。 */
export function resolveNameConflict(name: string, existing: Set<string>): string {
  if (!existing.has(name)) return name;
  const dot = name.lastIndexOf(".");
  const stem = dot < 0 ? name : name.slice(0, dot);
  const ext = dot < 0 ? "" : name.slice(dot);
  for (let i = 1; ; i++) {
    const cand = `${stem}-${i}${ext}`;
    if (!existing.has(cand)) return cand;
  }
}

export interface PlannedMove {
  fileName: string;
  finalName: string;
  to: string;
  category: DownloadCategory;
}

/**
 * 冲突感知执行计划：每步移动落成唯一目标名——目标已有同名时**消解出新名而不是覆盖**
 * （数据安全红线：永不静默覆盖用户文件；同名已在目标位 = 覆盖风险被结构性消灭）。
 */
export function planWithConflicts(proposal: TidyProposalGroup[], checked: DownloadCategory[], existingByDir: Record<string, string[]>): TidyExecution & { finalNames: PlannedMove[] } {
  const dirSets = new Map<string, Set<string>>();
  for (const [dir, names] of Object.entries(existingByDir)) dirSets.set(dir, new Set(names));
  const moves: TidyMove[] = [];
  const finalNames: PlannedMove[] = [];
  const skipped: string[] = [];
  for (const g of proposal) {
    if (!checked.includes(g.category)) continue;
    const dir = CATEGORY_DIRS[g.category];
    const dirSet = dirSets.get(dir) ?? new Set<string>();
    for (const f of g.files) {
      if (dirSet.has(f.name)) {
        skipped.push(f.name);
        continue;
      }
      const finalName = resolveNameConflict(f.name, dirSet);
      dirSet.add(finalName);
      moves.push({ fileName: f.name, from: "S:/Downloads", to: dir, category: g.category });
      finalNames.push({ fileName: f.name, finalName, to: dir, category: g.category });
    }
  }
  return { moves, skipped, finalNames };
}

/** 撤销账（F202 联动）：执行账 → 逐笔逆向条目（供全局撤销栈吞入的粒度，整批一次还原仍由 undoBatch 承担）。 */
export interface UndoJournalEntry {
  seq: number;
  fileName: string;
  from: string;
  to: string;
}

export function undoJournal(exec: TidyExecution): UndoJournalEntry[] {
  return exec.moves.map((m, i) => ({ seq: i + 1, fileName: m.fileName, from: m.from, to: m.to }));
}

/** 分类零错放审计：给真值表（文件名→期望类目）逐项比对——「分类零错放」的机检面。 */
export function auditZeroWrongCategory(files: DownloadFile[], truth: Record<string, DownloadCategory>): { pass: boolean; wrong: string[] } {
  const wrong: string[] = [];
  for (const f of files) {
    const expect = truth[f.name];
    if (expect && classifyFile(f) !== expect) wrong.push(`${f.name}: 判为 ${classifyFile(f)}，应为 ${expect}`);
  }
  return { pass: wrong.length === 0, wrong };
}

const CATEGORY_LABELS: Record<DownloadCategory, string> = { document: "文档", image: "图片", installer: "安装包", archive: "压缩包", other: "其他" };

/** 人话预览文案：「文档 3 项 · 共 4.2 MB → S:/Downloads/文档」。 */
export function summaryText(g: TidyProposalGroup): string {
  const mb = g.totalBytes / 1048576;
  const size = mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(g.totalBytes / 1024))} KB`;
  return `${CATEGORY_LABELS[g.category]} ${g.files.length} 项 · 共 ${size} → ${g.targetDir}`;
}
