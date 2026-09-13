/**
 * UNREAL-X-15000 · AI-08 族0071~0080 【V】消费端 UI：
 * 把空间分析引擎（画像/热力/切换成本/混乱度/推荐）输出转为
 * 设置页「空间分析」卡可直接渲染的视图模型。纯函数、零副作用。
 */

export interface ProfileEntry {
  app: string;
  focusMs: number;
  switches: number;
}

export interface HeatCell {
  gx: number;
  gy: number;
  level: 0 | 1 | 2 | 3;
}

export interface LayoutAdvice {
  layout: string;
  reason: string;
}

export interface SpatialReport {
  topApps: ProfileEntry[];
  hotspots: HeatCell[];
  messScore: number;
  messNarrative: string;
  advice: LayoutAdvice | null;
  switchAvgMs: number;
}

/** 画像引擎输出（字符串快照）→ Top-K 视图条目。 */
export function parseProfileSnapshot(text: string, k = 3): ProfileEntry[] {
  const lines = text.split('\n').filter((l) => l.length > 0);
  if (!lines[0]?.startsWith('UX71;')) return [];
  return lines
    .slice(1)
    .map((l) => {
      const [app, ms, sw] = l.split(' ');
      return { app, focusMs: Number(ms) || 0, switches: Number(sw) || 0 };
    })
    .sort((a, b) => b.focusMs - a.focusMs || a.app.localeCompare(b.app))
    .slice(0, Math.max(0, k));
}

/** 热力矩阵数值 → 三档热格视图（0~330 低 / 331~660 中 / 661~1000 热）。 */
export function cellsToView(cells: number[], gridW: number): HeatCell[] {
  const out: HeatCell[] = [];
  for (let i = 0; i < cells.length; i++) {
    const v = cells[i];
    if (v <= 0) continue;
    const level: HeatCell['level'] = v <= 330 ? 0 : v <= 660 ? 1 : 2;
    out.push({ gx: i % gridW, gy: Math.floor(i / gridW), level });
  }
  return out;
}

/** 混乱度分数 → 叙事文案（与引擎五档口径一致）。 */
export function messNarrative(score: number): string {
  if (score < 0) return '无效评分';
  if (score <= 99) return '整洁：无需整理';
  if (score <= 299) return '轻微：可随手归位';
  if (score <= 549) return '中等：建议开启整理助手';
  if (score <= 749) return '混乱：建议一键收纳';
  return '严重：建议重置为推荐布局';
}

/** 引擎推荐结果 → 布局建议卡（超容量时降级为 null）。 */
export function layoutAdvice(layout: string, windowCount: number, reason: string): LayoutAdvice | null {
  const caps: Record<string, number> = {
    cascade: 64,
    'half-split': 2,
    'quarter-grid': 4,
    masonry: 12,
    focus: 1,
  };
  const cap = caps[layout];
  if (cap === undefined || windowCount > cap) return null;
  return { layout, reason };
}

/** 聚合：组装空间分析报告卡。 */
export function buildReport(input: {
  profileSnapshot?: string;
  heatmapCells?: number[];
  gridW?: number;
  messScore?: number;
  recommendedLayout?: string;
  recommendReason?: string;
  windowCount?: number;
  switchAvgMs?: number;
}): SpatialReport {
  const topApps = input.profileSnapshot ? parseProfileSnapshot(input.profileSnapshot) : [];
  const hotspots = input.heatmapCells && input.gridW ? cellsToView(input.heatmapCells, input.gridW) : [];
  const mess = input.messScore ?? 0;
  const advice =
    input.recommendedLayout && input.recommendReason !== undefined && input.windowCount !== undefined
      ? layoutAdvice(input.recommendedLayout, input.windowCount, input.recommendReason)
      : null;
  return {
    topApps,
    hotspots,
    messScore: mess,
    messNarrative: messNarrative(mess),
    advice,
    switchAvgMs: input.switchAvgMs ?? 0,
  };
}
