/**
 * F166 输入法皮肤深化 · 拼音组合与候选引擎（皮肤演示的真数据源）。
 *
 * 主册判据延伸：
 * - F166「试打：皮肤即改即见」——试打框不能是假输入：组合串编辑
 *   （追加/回删/清空）、音节切分（声母韵母表）、候选生成（词库查询）
 *   全链真实——皮肤参数变化直接反映在候选窗布局（ime-menu-engine 消费）；
 * - B-904「组合期零误触」的组合期状态在这里有完整定义（IME 纪律的
 *   逻辑侧——组合串不是最终文本，快捷键系统必须跳过）；
 * - 零注水：音节切分用贪心最长匹配 + 回溯，候选是真实词库检索。
 */

// ---------- 音节切分 ----------

/** 声母表（含 zh/ch/sh 双字母——切分的边界难点）。 */
const INITIALS = ["zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "r", "z", "c", "s", "y", "w"] as const;

/**
 * 贪心最长匹配 + 回溯的音节切分。
 * 例："nihao" → ["ni","hao"]；"xian" → ["xian"]（不是 xi'an——
 * 歧义串给出两种切分，候选层两种都查——「西安」不能丢）。
 */
export function segmentSyllables(input: string): string[][] {
  const s = input.toLowerCase().replace(/[^a-z]/g, "");
  if (s === "") return [];
  const results: string[][] = [];
  const walk = (pos: number, acc: string[]): void => {
    if (pos >= s.length) {
      results.push([...acc]);
      return;
    }
    if (results.length >= 8) return; // 歧义枚举封顶（组合爆炸保护）。
    for (const init of INITIALS) {
      if (!s.startsWith(init, pos)) continue;
      const rest = s.slice(pos + init.length);
      // 韵母 = 从 rest 取最长合法韵母（简化韵母表：把后续非声母开头段收进来）。
      const finals = matchFinal(rest);
      for (const f of finals) {
        walk(pos + init.length + f.length, [...acc, init + f]);
      }
    }
    // 无声母（a/o/e 开头的零声母音节）。
    const zero = matchFinal(s.slice(pos));
    for (const f of zero) {
      if (f.length > 0) walk(pos + f.length, [...acc, f]);
    }
  };
  walk(0, []);
  return results.length > 0 ? results : [[s]];
}

/** 简化韵母表（常用 24 韵母——切分引擎的词面）。 */
const FINALS = ["iang", "uang", "iong", "uang", "ai", "ei", "ao", "ou", "an", "en", "ang", "eng", "ong", "ia", "ie", "iu", "ian", "in", "iao", "uan", "un", "uo", "ua", "ue", "ui", "er", "a", "o", "e", "i", "u", "v"] as const;

function matchFinal(s: string): string[] {
  // 最长优先枚举所有可作韵母的前缀（歧义串多解——xian/xi'an 都保留）。
  const out: string[] = [];
  for (const f of FINALS) {
    if (s.startsWith(f)) out.push(f);
  }
  if (out.length === 0) out.push(""); // 纯声母（未完成输入——候选层给声母级候选）。
  return out;
}

// ---------- 词库（试打演示的确定性小词库——真实检索非假数据） ----------

export interface ImeCandidate {
  /** 上屏文本。 */
  text: string;
  /** 匹配的音节序列（切分对拍——候选必须可回指切分）。 */
  syllables: string[];
  /** 词频权重（排序依据）。 */
  weight: number;
}

const LEXICON: readonly { text: string; key: string; weight: number }[] = [
  { text: "你好", key: "nihao", weight: 100 },
  { text: "倪豪", key: "nihao", weight: 3 },
  { text: "拟好", key: "nihao", weight: 8 },
  { text: "西安", key: "xian", weight: 60 },
  { text: "先", key: "xian", weight: 90 },
  { text: "线", key: "xian", weight: 40 },
  { text: "现在", key: "xianzai", weight: 95 },
  { text: "西安在", key: "xianzai", weight: 4 },
  { text: " Variable", key: "variable", weight: 50 },
  { text: "个性化", key: "gexinghua", weight: 70 },
  { text: "个人", key: "geren", weight: 85 },
  { text: "个人化", key: "gerenhua", weight: 20 },
  { text: "主题", key: "zhuti", weight: 88 },
  { text: "主体", key: "zhuti", weight: 45 },
  { text: "壁纸", key: "bizhi", weight: 75 },
  { text: "指针", key: "zhizhen", weight: 65 },
  { text: "支持", key: "zhichi", weight: 92 },
  { text: "只吃", key: "zhichi", weight: 5 },
  { text: "深圳", key: "shenzhen", weight: 80 },
  { text: "什么", key: "shenme", weight: 96 },
  { text: "神么", key: "shenme", weight: 2 },
  { text: "时间", key: "shijian", weight: 86 },
  { text: "实践", key: "shijian", weight: 60 },
  { text: "事件", key: "shijian", weight: 70 },
  { text: "桌面", key: "zhuomian", weight: 82 },
  { text: "桌面版", key: "zhuomianban", weight: 30 },
];

/**
 * 候选生成：对每种切分查词库（全匹配 + 前缀匹配未完成输入），
 * 权重降序。前缀匹配给「边打边选」的中间候选（输入法的基本行为）。
 */
export function generateCandidates(input: string, limit = 9): ImeCandidate[] {
  const s = input.toLowerCase().replace(/[^a-z]/g, "");
  if (s === "") return [];
  const segs = segmentSyllables(s);
  const seen = new Map<string, ImeCandidate>();
  for (const seg of segs) {
    const joined = seg.join("");
    for (const e of LEXICON) {
      if (e.key === joined || e.key.startsWith(joined)) {
        const existing = seen.get(e.text);
        if (!existing || existing.weight < e.weight) {
          seen.set(e.text, { text: e.text, syllables: seg, weight: e.weight });
        }
      }
    }
  }
  return [...seen.values()].sort((a, b) => b.weight - a.weight).slice(0, limit);
}

// ---------- 组合串编辑（B-904 组合期的完整操作面） ----------

export interface CompositionState {
  /** 拼音组合串。 */
  raw: string;
  /** 光标在组合串中的位置（编辑中间插入——完整编辑器不是追加器）。 */
  caret: number;
}

export type ComposeAction =
  | { type: "input"; ch: string }
  | { type: "backspace" }
  | { type: "delete" }
  | { type: "caret-move"; to: number }
  | { type: "clear" }
  | { type: "commit"; text: string };

/** 组合串纯编辑器（每步返回新状态——undo 链友好，可测可回放）。 */
export function compose(state: CompositionState, action: ComposeAction): CompositionState {
  const clamp = (v: number) => Math.min(state.raw.length, Math.max(0, v));
  switch (action.type) {
    case "input": {
      const ch = action.ch.toLowerCase();
      if (!/^[a-z]$/.test(ch)) return state; // 组合期只收字母（B-904：其他键不进组合串）。
      const raw = state.raw.slice(0, state.caret) + ch + state.raw.slice(state.caret);
      return { raw, caret: state.caret + 1 };
    }
    case "backspace": {
      if (state.caret === 0) return state;
      return { raw: state.raw.slice(0, state.caret - 1) + state.raw.slice(state.caret), caret: state.caret - 1 };
    }
    case "delete": {
      if (state.caret >= state.raw.length) return state;
      return { raw: state.raw.slice(0, state.caret) + state.raw.slice(state.caret + 1), caret: state.caret };
    }
    case "caret-move":
      return { ...state, caret: clamp(action.to) };
    case "clear":
      return { raw: "", caret: 0 };
    case "commit":
      return { raw: "", caret: 0 };
  }
}

// ---------- 候选窗分页（ime-menu-engine layoutCandidates 的数据面） ----------

export interface CandidatePage {
  items: ImeCandidate[];
  page: number;
  pageCount: number;
  /** 9 档翻页键提示（1-9 数字键选择——F166 判据）。 */
  numberHints: number[];
}

/** 候选分页（每页 9 个——数字键直选的页大小契约）。 */
export function paginateCandidates(all: ImeCandidate[], page: number): CandidatePage {
  const pageSize = 9;
  const pageCount = Math.max(1, Math.ceil(all.length / pageSize));
  const p = Math.min(pageCount - 1, Math.max(0, page));
  return {
    items: all.slice(p * pageSize, (p + 1) * pageSize),
    page: p,
    pageCount,
    numberHints: all.slice(p * pageSize, (p + 1) * pageSize).map((_, i) => i + 1),
  };
}

/** 数字键选择 → 全局候选序号（0..8 → page*9+n；越界返回 null——显性化）。 */
export function numberKeyToCandidate(page: number, key: number, total: number): number | null {
  const idx = page * 9 + (key - 1);
  return key >= 1 && key <= 9 && idx < total ? idx : null;
}

// ---------- 组合期提示（IME 纪律的对外契约——快捷键系统消费） ----------

/** 组合期状态说明（快捷键调度器跳过判定——B-904 的一处一事实）。 */
export interface CompositionFlags {
  /** 组合串非空（快捷键系统必须跳过字母/数字/空格）。 */
  composing: boolean;
  /** 候选窗开着（Esc 关窗不关面板）。 */
  candidateWindow: boolean;
}

export function compositionFlags(state: CompositionState, candidates: ImeCandidate[]): CompositionFlags {
  return { composing: state.raw.length > 0, candidateWindow: state.raw.length > 0 && candidates.length > 0 };
}
