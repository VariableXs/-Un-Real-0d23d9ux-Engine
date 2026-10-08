/**
 * 十一章 可发现性 · 全域搜索模型 + 十章导航树（页间关系真源）。
 *
 * - 「每个功能回答三个问题：怎么发现/怎么找回/用错怎么带回」——搜索
 *   索引（名称+关键词+拼音首字母）是发现与找回的统一入口；
 * - 最近使用置顶接 usage-model（EMA 排名——搜过的用过的优先）；
 * - 导航树 = 二十页的分组/面包屑/跳转关系真源（孤儿页检测——
 *   没有任何入口指向的页面 = 好功能没有入口 = 没做）。
 */

// ---------- 搜索索引 ----------

export interface SearchEntry {
  id: string;
  page: string;
  title: string;
  keywords: string[];
  /** 拼音首字母（中文检索——"tz" 命中"令牌"）。 */
  pinyinInitials: string;
  /** 专家捷径位（快捷键/直接动作——学习曲线的专家层）。 */
  shortcut?: string;
}

export interface ScoredHit {
  entry: SearchEntry;
  score: number;
  /** 命中方式（标题>关键词>首字母——结果排序的透明依据）。 */
  via: "title" | "keyword" | "initials";
}

/** 索引 → 模糊匹配（子串 + 首字母；分数：标题 30/关键词 20/首字母 10，相同并列按输入序）。 */
export function searchIndex(entries: SearchEntry[], query: string, recentIds: string[] = []): ScoredHit[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const hits: ScoredHit[] = [];
  for (const e of entries) {
    const title = e.title.toLowerCase();
    let hit: ScoredHit | null = null;
    if (title.includes(q)) hit = { entry: e, score: 30, via: "title" };
    else if (e.keywords.some((k) => k.toLowerCase().includes(q))) hit = { entry: e, score: 20, via: "keyword" };
    else if (e.pinyinInitials.toLowerCase().startsWith(q)) hit = { entry: e, score: 10, via: "initials" };
    if (hit) {
      // 最近使用置顶加成（usage-model 接线——用过一次搜索就知道你想要什么）。
      const recentBonus = recentIds.slice(0, 3).indexOf(e.id) >= 0 ? 5 : 0;
      hits.push({ ...hit, score: hit.score + recentBonus });
    }
  }
  return hits.sort((a, b) => b.score - a.score);
}

/** 结果分组（按页面域——结果页不是平铺清单，是带结构的地图）。 */
export function groupHits(hits: ScoredHit[]): Map<string, ScoredHit[]> {
  const m = new Map<string, ScoredHit[]>();
  for (const h of hits) {
    m.set(h.entry.page, [...(m.get(h.entry.page) ?? []), h]);
  }
  return m;
}

// ---------- 导航树（二十页关系真源） ----------

export interface NavGroup {
  id: string;
  title: string;
  pages: string[];
}

export const NAV_GROUPS: NavGroup[] = [
  { id: "look", title: "观感", pages: ["tokens", "preview", "autodark", "exceptions", "font", "motion"] },
  { id: "assets", title: "资产", pages: ["wallpaper", "icons", "pointer", "sound"] },
  { id: "layout", title: "布局", pages: ["startmenu", "widgets", "lock", "boot", "ime"] },
  { id: "system", title: "系统面", pages: ["ctxmenu", "taskbar", "shortcuts"] },
  { id: "governance", title: "治理", pages: ["archive", "verdict"] },
];

export interface NavVerdict {
  groups: NavGroup[];
  /** 二十页全部入组（孤儿页 = 没入口 = 没做）。 */
  orphanPages: string[];
  /** 面包屑：页 → 组路径。 */
  breadcrumbs: Map<string, string>;
}

export function auditNavTree(allPages: readonly string[]): NavVerdict {
  const grouped = new Set(NAV_GROUPS.flatMap((g) => g.pages));
  const orphanPages = allPages.filter((p) => !grouped.has(p));
  const breadcrumbs = new Map<string, string>();
  for (const g of NAV_GROUPS) {
    for (const p of g.pages) breadcrumbs.set(p, `${g.title} / ${p}`);
  }
  return { groups: NAV_GROUPS, orphanPages, breadcrumbs };
}

/** 页间跳转关系（用错带回正路——九章引导的结构面：相关页推荐）。 */
export const CROSS_LINKS: Record<string, string[]> = {
  tokens: ["preview", "autodark", "exceptions"],
  wallpaper: ["autodark", "boot"],
  pointer: ["asset-package", "verdict"],
  shortcuts: ["cheatsheet", "verdict"],
  archive: ["export-formats", "verdict"],
  verdict: ["archive", "perf-budget"],
};
