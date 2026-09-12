/**
 * AURORA-10000 领域04 · 族0094 输入法集成（AI-19 批次，勿删）。
 * 状态指示/候选参数/模糊音/自定义短语/词库管理/速度统计（数据模型层）。
 */

export interface ImeState { chinese: boolean; capsLock: boolean; }

/** 候选配置（F02329~F02332）。 */
export interface CandidatePrefs {
  perPage: number;
  fontSize: number;
  theme: "system" | "accent" | "dark" | "light";
  serialSelect: boolean;
}

export function candidatePrefs(perPage: number, fontSize: number): CandidatePrefs {
  return {
    perPage: Math.min(9, Math.max(3, perPage)),
    fontSize: Math.min(24, Math.max(12, fontSize)),
    theme: "system", serialSelect: true,
  };
}

/** 模糊音方案（F02333）：zh=z、ch=c、sh=s、n=l 等映射。 */
export const FUZZY_SCHEMES: Readonly<Record<string, readonly string[]>> = {
  "zh-z": ["zh", "z"], "ch-c": ["ch", "c"], "sh-s": ["sh", "s"], "n-l": ["n", "l"], "f-h": ["f", "h"],
};

export function fuzzyMatch(syllable: string, candidate: string, enabled: readonly string[]): boolean {
  if (syllable === candidate) return true;
  for (const key of enabled) {
    const pair = FUZZY_SCHEMES[key];
    if (pair && pair.includes(syllable) && pair.includes(candidate)) return true;
  }
  return false;
}

/** 自定义短语库（F02334）。 */
const phrases = new Map<string, string>();
export function addPhrase(abbr: string, text: string): void { phrases.set(abbr, text); }
export function lookupPhrase(abbr: string): string | undefined { return phrases.get(abbr); }

/** 词库管理（F02348/49）。 */
export interface LexiconEntry { word: string; freq: number; }
export function mergeLexicon(base: readonly LexiconEntry[], incoming: readonly LexiconEntry[]): LexiconEntry[] {
  const m = new Map<string, LexiconEntry>();
  for (const e of base) m.set(e.word, { ...e });
  for (const e of incoming) {
    const prev = m.get(e.word);
    m.set(e.word, { word: e.word, freq: (prev?.freq ?? 0) + e.freq });
  }
  return [...m.values()].sort((a, b) => b.freq - a.freq);
}
export function exportLexicon(list: readonly LexiconEntry[]): string {
  return list.map((e) => `${e.word}\t${e.freq}`).join("\n");
}
export function importLexicon(text: string): LexiconEntry[] {
  return text.split(/\r?\n/).map((line) => {
    const [word, freq] = line.split("\t");
    return { word: (word ?? "").trim(), freq: parseInt(freq ?? "1", 10) || 1 };
  }).filter((e) => e.word.length > 0);
}

/** 打字速度统计（F02346）：KPM。 */
export function typingSpeed(keystrokes: number, ms: number): number {
  if (ms <= 0) return 0;
  return Math.round((keystrokes / ms) * 60_000);
}
