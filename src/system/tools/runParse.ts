/**
 * Z-28 运行对话框 —— 纯逻辑核心（可测试，零 DOM / 零 IPC 依赖）：
 * - classifyRunInput：输入分类（uri / path / exec / unknown）
 *   · 引号包裹的含空格路径（"C:\Program Files\app.exe"）
 *   · 环境变量形态（%TEMP%、%TEMP%\x.txt）
 *   · UNC（\\server\share）、盘符（X:\、X:、C:/fwd/slash）
 *   · scheme:// URI（https://…、file:///、mailto:）
 *   · 裸可执行名（calc、notepad、notepad.exe、字母组合）
 *   · 纯数字等无法识别输入 → unknown（如实失败，不瞎猜）
 * - RUN_ALIASES：内部别名 → VWM 工具/动作 id
 * - matchRunCandidates：自动补全候选（别名前缀 + 历史前缀，去重，≤8 条）
 * - 运行历史（≤20，localStorage 键 variable:run:history:v1）
 *
 * 红线：本文件不做任何 IPC 调用；执行路由在 RunDialog.tsx。
 */

export type RunKind = "uri" | "path" | "exec" | "unknown";

export interface RunInput {
  kind: RunKind;
  /** 归一化结果（引号剥离后的可执行目标；unknown 时原样返回）。 */
  normalized: string;
}

/** 内部别名 → VWM 工具/动作 id（大小写不敏感，键统一小写）。 */
export const RUN_ALIASES: Record<string, string> = {
  calc: "calc",
  notes: "notes",
  calendar: "calendar",
  clipboard: "clipboard",
  taskman: "taskman",
  explorer: "explorer",
  settings: "settings",
  snapshot: "snapshot",
  rename: "rename",
  dupe: "dupe",
  space: "space",
  checksum: "checksum",
  clockhub: "clockhub",
  emoji: "emoji",
  magnifier: "magnifier",
  convert: "convert",
  sysinfo: "sysinfo",
  printqueue: "printqueue",
};

// scheme: 长度 ≥2（排除单字母盘符 C:）
const RE_SCHEME = /^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/;
// 环境变量形态：%TEMP% / %ProgramData%\x
const RE_ENV = /^%[A-Za-z0-9_#$]+%([\\/].*)?$/;
// 裸可执行名：字母/数字开头，允许 . _ - 与空格（Windows Run 习惯）
const RE_EXEC = /^[A-Za-z0-9_][A-Za-z0-9_.\- ]*$/;
// 纯数字（如 "123"）：无法判定执行目标
const RE_DIGITS = /^\d+$/;

/** 分类输入：优先级 = 引号剥离 → 环境变量 → URI → 盘符 → UNC → 分隔符路径 → 纯数字 → 可执行名。 */
export function classifyRunInput(raw: string): RunInput {
  const s = raw.trim();
  if (!s) return { kind: "unknown", normalized: "" };
  // 引号包裹（含空格路径）：剥离后按内层分类，normalized 不带引号
  if (s.length >= 2 && s.startsWith('"') && s.endsWith('"')) {
    const inner = s.slice(1, -1).trim();
    if (!inner) return { kind: "unknown", normalized: "" };
    return classifyRunInput(inner);
  }
  // 环境变量形态
  if (RE_ENV.test(s)) return { kind: "path", normalized: s };
  // URI（scheme 长度 ≥2，排除单字母盘符；file:///、https://、mailto: 均命中）
  const m = RE_SCHEME.exec(s);
  if (m && m[1] !== undefined && m[1].length >= 2) return { kind: "uri", normalized: s };
  // 盘符路径（X:\dir、X:、X:/fwd）
  if (/^[A-Za-z]:[\\/]/.test(s) || /^[A-Za-z]:$/.test(s)) return { kind: "path", normalized: s };
  // UNC \\server\share
  if (s.startsWith("\\\\")) return { kind: "path", normalized: s };
  // 含路径分隔符的相对/正斜杠路径
  if (s.includes("\\") || s.includes("/")) return { kind: "path", normalized: s };
  // 纯数字 → unknown
  if (RE_DIGITS.test(s)) return { kind: "unknown", normalized: s };
  // 裸可执行名
  if (RE_EXEC.test(s)) return { kind: "exec", normalized: s };
  return { kind: "unknown", normalized: s };
}

// ---------- 自动补全候选 ----------

export interface RunCandidate {
  text: string;
  kind: "alias" | "history";
}

/** 候选上限。 */
export const CANDIDATE_CAP = 8;

/**
 * 自动补全候选：精确别名 → 其余别名前缀（字母序）→ 历史前缀（新→旧）；
 * 按小写去重；≤8 条；空查询返回空表。
 */
export function matchRunCandidates(q: string, history: string[]): RunCandidate[] {
  const s = q.trim().toLowerCase();
  if (!s) return [];
  const out: RunCandidate[] = [];
  const seen = new Set<string>();
  const add = (text: string, kind: RunCandidate["kind"]): void => {
    if (out.length >= CANDIDATE_CAP) return;
    const key = text.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    out.push({ text, kind });
  };
  if (RUN_ALIASES[s] !== undefined) add(s, "alias");
  for (const a of Object.keys(RUN_ALIASES).sort()) {
    if (a.startsWith(s)) add(a, "alias");
  }
  for (const h of history) {
    if (h.toLowerCase().startsWith(s)) add(h, "history");
  }
  return out.slice(0, CANDIDATE_CAP);
}

// ---------- 运行历史（≤20） ----------

export const RUN_HISTORY_KEY = "variable:run:history:v1";
export const RUN_HISTORY_CAP = 20;
/** 「不记录敏感路径」开关持久化键（同一 variable:run: 命名空间）。 */
export const RUN_NORECORD_KEY = "variable:run:norecord:v1";

/** 解析历史 JSON（坏数据 → 空表，绝不抛错）。 */
export function parseRunHistory(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const v = JSON.parse(raw) as unknown;
    if (!Array.isArray(v)) return [];
    return v.filter((x): x is string => typeof x === "string" && x.trim().length > 0);
  } catch {
    return [];
  }
}

/** 纯函数：新条目置顶；大小写不敏感去重；超过 20 条淘汰最旧（尾部）。 */
export function addRunHistory(list: string[], entry: string): string[] {
  const s = entry.trim();
  if (!s) return list;
  return [s, ...list.filter((h) => h.toLowerCase() !== s.toLowerCase())].slice(0, RUN_HISTORY_CAP);
}

/** 读取历史（localStorage 不可用时返回空表）。 */
export function loadRunHistory(): string[] {
  try {
    return parseRunHistory(localStorage.getItem(RUN_HISTORY_KEY));
  } catch {
    return [];
  }
}

/** 写回历史（存储受限时静默跳过）。 */
export function saveRunHistory(list: string[]): void {
  try {
    localStorage.setItem(RUN_HISTORY_KEY, JSON.stringify(list.slice(0, RUN_HISTORY_CAP)));
  } catch {
    /* 存储满/被禁用 → 本次不持久化 */
  }
}
