/**
 * F167 使用频率排序深化 · EMA 衰减排名（隐私分组 + 公平 top-k + 导出）。
 *
 * 主册判据延伸：
 * - F167「应用注册项按使用频率自动排序」与 F072 计数同源的排序面——
 *   纯计数会被三个月前的一次疯狂点击永久霸榜：EMA 指数衰减让排名
 *   反映"最近的使用习惯"；
 * - 十六章「日志不记敏感内容」：排序只吃 id 与时刻——文件名/参数
 *   永远不进模型（隐私红线在数据结构层，不靠自觉）；
 * - 「自动排序与 F072 计数一致」：本模型是纯函数（事件流 → 排名），
 *   F072 灌入的事件在两侧产出可对拍的同一排名。
 */

// ---------- 事件与排名 ----------

export interface UsageEvent {
  /** 条目 id（应用/动作——不含任何用户内容）。 */
  itemId: string;
  at: number;
  /** 事件权重（默认 1——特殊动作可加权，如"钉选"记负向）。 */
  weight?: number;
}

export interface Ranking {
  itemId: string;
  /** EMA 分数（半衰期归一后的近期使用度）。 */
  score: number;
  /** 总次数（原始计数——与 F072 计数对拍的面）。 */
  rawCount: number;
  lastUsedAt: number | null;
}

/** 事件流 → EMA 排名（半衰期 halfLifeMs——衰减是数学不是玄学）。 */
export function rankEvents(events: UsageEvent[], now: number, halfLifeMs = 7 * 24 * 3600 * 1000): Ranking[] {
  const byId = new Map<string, { score: number; raw: number; last: number | null }>();
  const decay = (dtMs: number): number => Math.pow(0.5, dtMs / halfLifeMs);
  for (const e of [...events].sort((a, b) => a.at - b.at)) {
    const w = e.weight ?? 1;
    const entry = byId.get(e.itemId) ?? { score: 0, raw: 0, last: null };
    // 分数 = 历史分衰减 + 本次权重（时间正确的 EMA——不是末次快照）。
    const dt = e.at - (entry.last ?? e.at);
    entry.score = entry.score * decay(Math.max(0, dt)) + w;
    entry.raw += w > 0 ? w : 0;
    entry.last = e.at;
    byId.set(e.itemId, entry);
  }
  const out: Ranking[] = [];
  for (const [itemId, e] of byId) {
    // 排名分数对齐到 now（上次使用后经过的时间继续衰减——7 天不用自然下沉）。
    out.push({ itemId, score: Math.round(e.score * decay(now - (e.last ?? now)) * 1000) / 1000, rawCount: e.raw, lastUsedAt: e.last });
  }
  return out.sort((a, b) => b.score - a.score || b.rawCount - a.rawCount);
}

/** 公平 top-k：分数断层处的尾部裁剪（0 分项不占榜——排行榜不是名单）。 */
export function topK(ranked: Ranking[], k: number): Ranking[] {
  return ranked.filter((r) => r.score > 0.001).slice(0, k);
}

/** 钉选语义：钉选项永远排在最前（分数不参与——钉选是承诺）。 */
export function withPinned(ranked: Ranking[], pinned: Set<string>, k: number): Ranking[] {
  const pin = ranked.filter((r) => pinned.has(r.itemId));
  const rest = topK(ranked.filter((r) => !pinned.has(r.itemId)), Math.max(0, k - pin.length));
  return [...pin, ...rest];
}

// ---------- 隐私分组导出（排行可分享——原始事件不出域） ----------

export interface RankedExport {
  format: "vx-usage-ranking";
  version: 1;
  exportedAt: number;
  /** 只含 id/分数/计数——零内容字段（结构即隐私）。 */
  items: Array<{ itemId: string; score: number; rawCount: number }>;
}

export function exportRanking(ranked: Ranking[], now: number): RankedExport {
  return {
    format: "vx-usage-ranking",
    version: 1,
    exportedAt: now,
    items: ranked.slice(0, 50).map((r) => ({ itemId: r.itemId, score: r.score, rawCount: r.rawCount })),
  };
}

/** 导入对拍：外部排名与本域重算的一致性（F072 两源对账的排序侧）。 */
export function reconcileWithExternal(local: Ranking[], external: RankedExport, tolerance = 0.05): { consistent: boolean; mismatches: string[] } {
  const localMap = new Map(local.map((r) => [r.itemId, r]));
  const mismatches: string[] = [];
  for (const item of external.items) {
    const mine = localMap.get(item.itemId);
    if (!mine) {
      mismatches.push(`${item.itemId}: 本域无此条目`);
      continue;
    }
    if (Math.abs(mine.score - item.score) > Math.max(tolerance, item.score * tolerance)) {
      mismatches.push(`${item.itemId}: 分数偏差（本 ${mine.score} vs 外 ${item.score}）`);
    }
  }
  return { consistent: mismatches.length === 0, mismatches };
}
