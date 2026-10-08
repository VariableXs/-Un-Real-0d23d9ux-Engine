/**
 * AI-11 N-24 预测性启动预热（纯逻辑模块）。
 *
 * 数据面：本地使用洞察（ins_record 由 AI-10 维护）之外，本模块自维护
 * 一份「可执行文件启动计数」（localStorage，仅文件路径与次数、时间戳）。
 * 预热动作 = 后端 predwarm（读前 2MB 页缓存），仅预读、不启动进程。
 *
 * 纪律红线（承分工图）：白名单 = 用户启动史 Top-N；上限 12 条防泛化；
 * 绝不预热非可执行/不存在路径（后端二次校验）。
 */

export interface WarmEntry {
  path: string;
  count: number;
  last: number;
}

const WARM_KEY = "variable:ai11:predwarm";
const WARM_MAX = 12;

export function loadWarmList(): WarmEntry[] {
  try {
    const raw = localStorage.getItem(WARM_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as WarmEntry[];
    if (!Array.isArray(arr)) return [];
    return arr
      .filter((e) => e && typeof e.path === "string" && e.path.length > 0)
      .map((e) => ({ path: e.path, count: Number(e.count) || 0, last: Number(e.last) || 0 }));
  } catch {
    return [];
  }
}

function persist(list: WarmEntry[]): void {
  try {
    localStorage.setItem(WARM_KEY, JSON.stringify(list.slice(0, WARM_MAX)));
  } catch {
    /* storage blocked */
  }
}

/** 记录一次启动（启动器/运行框调用；同路径计数 +1）。 */
export function recordLaunch(path: string, nowMs: number): void {
  const p = path.trim();
  if (!p) return;
  const list = loadWarmList();
  const hit = list.find((e) => e.path === p);
  if (hit) {
    hit.count += 1;
    hit.last = nowMs;
  } else {
    list.push({ path: p, count: 1, last: nowMs });
  }
  list.sort((a, b) => b.count - a.count || b.last - a.last);
  persist(list);
}

/** 候选白名单：Top-N（默认 5）条，仅返回可执行扩展名。 */
export function warmCandidates(topN = 5): string[] {
  const exeRe = /\.(exe|com|bat|cmd|lnk)$/i;
  return loadWarmList()
    .filter((e) => exeRe.test(e.path))
    .slice(0, Math.max(1, topN))
    .map((e) => e.path);
}

/** 移除一条（用户显式管理）。 */
export function removeWarm(path: string): void {
  persist(loadWarmList().filter((e) => e.path !== path));
}
