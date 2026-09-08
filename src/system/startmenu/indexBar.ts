import { initialsOf, pinyinOf } from "../../lib/pinyin";

/**
 * 化境 V-11（车道 S）：开始菜单字母索引条的数据逻辑。
 * - 应用按显示名首字分组：A–Z + CJK 拼音首字母（复用 src/lib/pinyin.ts 的
 *   initialsOf——它就是本仓库的「首字母函数」，无需另写 localPinyinInitial）。
 * - 未收录汉字/数字/符号不猜音（pinyin.ts 的诚实降级原则）→ 归入「#」组。
 * - 索引模式启用时列表临时按字母分组渲染（不破坏用户手动 order——
 *   手动排序数据原样保留，关掉索引即还原；取舍已在 StartMenu 注明）。
 */

/** 索引条出现阈值：应用数 >30 才显示（克制，规格 V-11）。 */
export const INDEX_THRESHOLD = 30;

/** 分组键：显示名首个有效字符的首字母；无有效首字母（数字/未收录汉字/符号开头）→ "#"。 */
export function groupKeyOf(label: string): string {
  const ini = initialsOf(label);
  if (!ini) return "#";
  const c = (ini[0] ?? "#").toUpperCase();
  return c >= "A" && c <= "Z" ? c : "#";
}

export interface LetterGroup<T> {
  letter: string;
  items: T[];
}

/**
 * 按字母分组（纯函数）：字母 A→Z、「#」最后；组内按拼音序（pinyinOf 逐字符
 * 拼接，确定性排序不依赖运行环境 ICU），同拼字节按原文稳定。
 */
export function letterGroups<T extends { id: string; label: string }>(items: T[]): LetterGroup<T>[] {
  const map = new Map<string, T[]>();
  for (const it of items) {
    const key = groupKeyOf(it.label);
    const list = map.get(key);
    if (list) list.push(it);
    else map.set(key, [it]);
  }
  const within = (a: T, b: T): number => {
    const pa = pinyinOf(a.label);
    const pb = pinyinOf(b.label);
    return pa < pb ? -1 : pa > pb ? 1 : a.label < b.label ? -1 : a.label > b.label ? 1 : 0;
  };
  return [...map.entries()]
    .map(([letter, list]) => ({ letter, items: list.sort(within) }))
    .sort((a, b) => {
      if (a.letter === "#") return 1;
      if (b.letter === "#") return -1;
      return a.letter < b.letter ? -1 : 1;
    });
}

/** 索引条字母序列（与分组一一对应，# 组也显示，便于回到底部杂项）。 */
export function indexLetters<T>(groups: LetterGroup<T>[]): string[] {
  return groups.map((g) => g.letter);
}
