/**
 * AURORA-10000 · AI-11~AI-15 车道 · 纯逻辑内核（全部可单测、不触碰 DOM）。
 * 覆盖：
 * - organizeLayout 族0064（九宫格/磁力/时间线/项目分区/极简/抽屉…）
 * - dueHealthRules 族0065（20-20-20/久坐/饮水/勿扰窗口到期判定）
 * - festivalOn 族0073（2024~2029 农历+节气表；超出范围诚实返回 null）
 * - themePlanValue 族0067（按时段明暗）与 themeVars
 * - craftCssFilter 族0059（滤镜栈 → CSS filter）
 * - libraryOps 族0058（感知查重/低质分/轮换计划）
 * - effectiveRefresh 族0061（可见性智能刷新）
 * - snapToGrid / clutterIndex 族0063/0064
 * - auditSpacing4 / auditTypeScale 族0069（内容风格审计）
 */

/* ================= 族0064 桌面整理布局 ================= */

export interface OrgItem {
  id: string;
  x: number;
  y: number;
  category?: string;
  project?: string;
  openedAt?: number;
  lastUsedAt?: number;
}

export interface OrgRect {
  x: number;
  y: number;
}

export type OrgMode =
  | "festival" | "grid9" | "free" | "magnetic" | "timeline"
  | "projects" | "drawer" | "minimal" | "scatter";

export interface OrganizeOpts {
  cell: number;
  cols: number;
  /** 随机种子（散落/节日布局可复现）。 */
  seed?: number;
  /** 保留的置顶 id（minimal 模式外的白名单不移动）。 */
  keepIds?: string[];
}

/** 线性同余伪随机（同种子同布局，可测）。 */
function rng(seed: number): () => number {
  let s = seed >>> 0 || 1;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/**
 * 整理布局：返回每个 item 的新坐标（栅格步进坐标，cell 为步长）。
 * keepIds 中的项保持原位（白名单语义，族0109/01597 同口径）。
 */
export function organizeLayout(
  mode: OrgMode,
  items: OrgItem[],
  opts: OrganizeOpts,
): Map<string, OrgRect> {
  const out = new Map<string, OrgRect>();
  const keep = new Set(opts.keepIds ?? []);
  const movable = items.filter((it) => !keep.has(it.id));
  const placed = (it: OrgItem, col: number, row: number): void => {
    out.set(it.id, { x: col * opts.cell, y: row * opts.cell });
  };
  switch (mode) {
    case "free": {
      for (const it of items) out.set(it.id, { x: it.x, y: it.y });
      return out;
    }
    case "grid9": {
      movable.forEach((it, i) => {
        const slot = i % 9;
        placed(it, slot % 3, Math.floor(slot / 3) + Math.floor(i / 9) * 4);
      });
      break;
    }
    case "magnetic": {
      // 同类相吸：按类分带（每类占一竖带），带内列优先排布
      const order: string[] = [];
      const groupIndex = new Map<string, number>();
      for (const it of movable) {
        const c = it.category ?? "_";
        if (!groupIndex.has(c)) {
          groupIndex.set(c, order.length);
          order.push(c);
        }
      }
      const inner = new Map<string, number>();
      const perCol = Math.max(1, Math.ceil(Math.sqrt(Math.max(1, movable.length))));
      for (const it of movable) {
        const c = it.category ?? "_";
        const band = groupIndex.get(c)!;
        const n = inner.get(c) ?? 0;
        inner.set(c, n + 1);
        placed(it, band * 2 + Math.floor(n / perCol), n % perCol);
      }
      break;
    }
    case "timeline": {
      const sorted = [...movable].sort((a, b) => (a.openedAt ?? 0) - (b.openedAt ?? 0));
      sorted.forEach((it, i) => placed(it, i % opts.cols, Math.floor(i / opts.cols)));
      break;
    }
    case "projects": {
      // 先定分区顺序，再把每区排进自己的区块（列优先）
      const zoneOrder: string[] = [];
      const zoneOf = new Map<string, number>();
      for (const it of movable) {
        const p = it.project ?? "_";
        if (!zoneOf.has(p)) {
          zoneOf.set(p, zoneOrder.length);
          zoneOrder.push(p);
        }
      }
      const zoneCols = Math.max(1, Math.ceil(opts.cols / Math.max(1, zoneOrder.length)));
      const inner = new Map<string, number>();
      for (const it of movable) {
        const p = it.project ?? "_";
        const z = zoneOf.get(p)!;
        const n = inner.get(p) ?? 0;
        inner.set(p, n + 1);
        const zoneRowBase = Math.floor(z / Math.floor(opts.cols / zoneCols)) * Math.max(2, zoneCols);
        placed(it, (z % Math.max(1, Math.floor(opts.cols / zoneCols))) * zoneCols + (n % zoneCols), zoneRowBase + Math.floor(n / zoneCols));
      }
      break;
    }
    case "drawer": {
      for (const it of movable) out.set(it.id, { x: -1, y: -1 }); // -1 = 收入抽屉
      break;
    }
    case "minimal": {
      const keepTop = [...movable]
        .sort((a, b) => (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0))
        .slice(0, 3);
      keepTop.forEach((it, i) => placed(it, 0, i));
      for (const it of movable) if (!keepTop.includes(it)) out.set(it.id, { x: -1, y: -1 });
      break;
    }
    case "scatter": {
      const r = rng(opts.seed ?? 7);
      movable.forEach((it, i) => {
        const col = i % opts.cols;
        const row = Math.floor(i / opts.cols);
        placed(it, col, row);
        const p = out.get(it.id)!;
        p.x = Math.round((p.x + (r() - 0.5) * 0.6) * opts.cell);
        p.y = Math.round((p.y + (r() - 0.5) * 0.6) * opts.cell);
      });
      break;
    }
    case "festival": {
      const r = rng(opts.seed ?? 42);
      movable.forEach((it, i) => {
        const col = i % opts.cols;
        const row = Math.floor(i / opts.cols);
        placed(it, col, row);
        const p = out.get(it.id)!;
        p.y = Math.round(p.y + Math.sin(i) * 0.3 * opts.cell * (0.5 + r() * 0.5));
      });
      break;
    }
  }
  for (const it of items) if (keep.has(it.id) && !out.has(it.id)) out.set(it.id, { x: it.x, y: it.y });
  return out;
}

/** 杂乱度指数（F01587）：位置熵 + 网格偏离度归一 0~100。 */
export function clutterIndex(items: OrgItem[], cell: number): number {
  if (items.length === 0) return 0;
  let off = 0;
  for (const it of items) {
    const dx = Math.abs(it.x % cell);
    const dy = Math.abs(it.y % cell);
    off += Math.min(dx, cell - dx) + Math.min(dy, cell - dy);
  }
  const offScore = off / (items.length * cell); // 0~1
  const countScore = Math.min(1, Math.max(0, items.length - 12) / 48);
  return Math.round(Math.min(100, (offScore * 0.6 + countScore * 0.4) * 100));
}

/* ================= 族0063 互动落点吸附 ================= */

export function snapToGrid(x: number, y: number, cell: number, magnetPx = 0): { x: number; y: number; snapped: boolean } {
  const gx = Math.round(x / cell) * cell;
  const gy = Math.round(y / cell) * cell;
  const d = Math.hypot(gx - x, gy - y);
  if (d <= magnetPx) return { x: gx, y: gy, snapped: true };
  return { x, y, snapped: false };
}

/* ================= 族0065 健康到期判定 ================= */

export interface HealthRuleDef {
  entryId: string;
  everyMin: number;
}

export interface HealthLog {
  [entryId: string]: number; // 上次触发的 epoch ms
}

export interface HealthDue {
  entryId: string;
  overdueMin: number;
}

/** 判定哪些健康规则到期（nowMs 可注入便于测试）。 */
export function dueHealthRules(rules: HealthRuleDef[], log: HealthLog, nowMs: number): HealthDue[] {
  const out: HealthDue[] = [];
  for (const r of rules) {
    const last = log[r.entryId] ?? 0;
    const diffMin = (nowMs - last) / 60000;
    if (diffMin >= r.everyMin) out.push({ entryId: r.entryId, overdueMin: Math.floor(diffMin) });
  }
  return out.sort((a, b) => b.overdueMin - a.overdueMin);
}

/** 勿扰窗口（F01621/F01598）：支持跨午夜 [from,to)，如 23→7。 */
export function inQuietWindow(hour: number, from: number, to: number): boolean {
  if (from === to) return false;
  if (from < to) return hour >= from && hour < to;
  return hour >= from || hour < to;
}

/* ================= 族0073 节日（2024~2029 农历 + 节气表） ================= */

/** 农历节日表（公历 ISO 日期）。数据为公开发布的万年历；2029 后诚实返回 null。 */
const LUNAR_TABLE: Record<string, Record<string, string>> = {
  cny: { "2024": "02-10", "2025": "01-29", "2026": "02-17", "2027": "02-06", "2028": "01-26", "2029": "02-13" },
  yuanxiao: { "2024": "02-24", "2025": "02-12", "2026": "03-03", "2027": "02-20", "2028": "02-09", "2029": "02-27" },
  duanwu: { "2024": "06-10", "2025": "05-31", "2026": "06-19", "2027": "06-09", "2028": "05-28", "2029": "06-16" },
  qixi: { "2024": "08-10", "2025": "08-29", "2026": "08-19", "2027": "08-08", "2028": "08-26", "2029": "08-16" },
  zhongqiu: { "2024": "09-17", "2025": "10-06", "2026": "09-25", "2027": "09-15", "2028": "10-03", "2029": "09-22" },
  chongyang: { "2024": "10-11", "2025": "10-29", "2026": "10-18", "2027": "10-08", "2028": "10-26", "2029": "10-16" },
  laba: { "2024": "01-18", "2025": "01-07", "2026": "01-26", "2027": "01-15", "2028": "01-05", "2029": "01-22" },
  qingming: { "2024": "04-04", "2025": "04-04", "2026": "04-05", "2027": "04-05", "2028": "04-04", "2029": "04-04" },
  dongzhi: { "2024": "12-21", "2025": "12-21", "2026": "12-22", "2027": "12-22", "2028": "12-21", "2029": "12-21" },
};

/** 公历固定节日（月-日）。 */
const GREG_TABLE: Record<string, string> = {
  valentine: "02-14", halloween: "10-31", christmas: "12-25", newyear: "01-01",
};

export type FestivalKey = keyof typeof LUNAR_TABLE | keyof typeof GREG_TABLE | "chuxi" | "sakura" | "maple";

/** 除夕 = 春节前一天，由表推得。 */
function chuxiOf(year: string): string | null {
  const cny = LUNAR_TABLE.cny?.[year];
  if (!cny) return null;
  const [m, d] = cny.split("-").map(Number) as [number, number];
  const dt = new Date(Date.UTC(Number(year), m - 1, d));
  dt.setUTCDate(dt.getUTCDate() - 1);
  const mm = String(dt.getUTCMonth() + 1).padStart(2, "0");
  const dd = String(dt.getUTCDate()).padStart(2, "0");
  return `${mm}-${dd}`;
}

/** 该日期命中的节日 key 列表；农历/节气表未覆盖年份返回空（诚实边界，UI 注明）。 */
export function festivalOn(date: Date): FestivalKey[] {
  const year = String(date.getFullYear());
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  const md = `${mm}-${dd}`;
  const out: FestivalKey[] = [];
  for (const [key, years] of Object.entries(LUNAR_TABLE)) {
    if (years[year] === md) out.push(key as FestivalKey);
  }
  const cx = chuxiOf(year);
  if (cx && cx === md) out.push("chuxi");
  for (const [key, val] of Object.entries(GREG_TABLE)) {
    if (val === md) out.push(key as FestivalKey);
  }
  // 季节档（约数：3-4 月樱花、10-11 月枫叶）
  if (md >= "03-15" && md <= "04-15") out.push("sakura");
  if (md >= "10-15" && md <= "11-30") out.push("maple");
  return out;
}

/** 季节档（F01584 换季提醒）：0春 1夏 2秋 3冬。 */
export function seasonOf(date: Date): 0 | 1 | 2 | 3 {
  const m = date.getMonth() + 1;
  if (m >= 3 && m <= 5) return 0;
  if (m >= 6 && m <= 8) return 1;
  if (m >= 9 && m <= 11) return 2;
  return 3;
}

/* ================= 族0067 明暗计划 ================= */

export interface ThemePlan {
  /** 日间 7:00 起 / 夜间 19:00 起（entryId F01659）。 */
  dayFrom: number;
  nightFrom: number;
}

export type ThemePhase = "day" | "night";

export function themePhase(hour: number, plan: ThemePlan): ThemePhase {
  return hour >= plan.nightFrom || hour < plan.dayFrom ? "night" : "day";
}

/* ================= 族0059 滤镜栈 → CSS filter ================= */

export interface CraftOp {
  kind: "saturate" | "brightness" | "contrast" | "blur" | "sepia" | "grayscale" | "vignette" | "hue-rotate" | "noise";
  value: number;
}

/** 滤镜栈编译为 CSS filter 字符串（vignette/noise 属叠加层，此处只产可用子集）。 */
export function craftCssFilter(stack: CraftOp[]): string {
  const parts: string[] = [];
  for (const op of stack) {
    switch (op.kind) {
      case "saturate": parts.push(`saturate(${clamp100(op.value)}%)`); break;
      case "brightness": parts.push(`brightness(${clamp100(op.value)}%)`); break;
      case "contrast": parts.push(`contrast(${clamp100(op.value)}%)`); break;
      case "grayscale": parts.push(`grayscale(${clamp100(op.value)}%)`); break;
      case "sepia": parts.push(`sepia(${clamp100(op.value)}%)`); break;
      case "blur": parts.push(`blur(${Math.max(0, op.value)}px)`); break;
      case "hue-rotate": parts.push(`hue-rotate(${op.value}deg)`); break;
      case "vignette":
      case "noise":
        break; // 叠加层：不在 filter 内表达（诚实边界）
    }
  }
  return parts.join(" ") || "none";
}

function clamp100(v: number): number {
  return Math.min(300, Math.max(0, Math.round(v)));
}

/* ================= 族0058 画库操作 ================= */

export interface WallRecord {
  id: string;
  /** 感知哈希（64bit 十六进制；由工坊落库时计算）。 */
  phash?: string;
  /** 0~1 清晰度评分。 */
  sharp?: number;
  w?: number;
  h?: number;
  tags?: string[];
  favorite?: boolean;
}

/** 汉明距离（等长 hex 串）。 */
export function hammingHex(a: string, b: string): number {
  if (a.length !== b.length) return 64;
  let d = 0;
  for (let i = 0; i < a.length; i++) {
    const x = parseInt(a[i]!, 16) ^ parseInt(b[i]!, 16);
    d += (x & 1) + ((x >> 1) & 1) + ((x >> 2) & 1) + ((x >> 3) & 1);
  }
  return d;
}

/** 查重：返回 [保留 id, 重复 id 列表]（汉明距离 ≤ 8 视为重复，收藏项优先保留）。 */
export function dedupeWallpapers(items: WallRecord[], threshold = 8): Array<{ keep: string; dupes: string[] }> {
  const pool = items.filter((i) => i.phash);
  const used = new Set<string>();
  const out: Array<{ keep: string; dupes: string[] }> = [];
  for (const a of pool) {
    if (used.has(a.id)) continue;
    const dupes: string[] = [];
    for (const b of pool) {
      if (b.id === a.id || used.has(b.id)) continue;
      if (hammingHex(a.phash!, b.phash!) <= threshold) {
        // 收藏项优先保留
        if (!a.favorite && b.favorite) {
          dupes.push(a.id);
          used.add(a.id);
          out.push({ keep: b.id, dupes: [a.id] });
          used.add(b.id);
          break;
        }
        dupes.push(b.id);
        used.add(b.id);
      }
    }
    if (!used.has(a.id)) {
      out.push({ keep: a.id, dupes });
      used.add(a.id);
    }
  }
  return out;
}

/** 低质检测：模糊（sharp 低）或拉伸（宽高比与目标屏偏离大）。 */
export function qualityFlag(item: WallRecord, screenAspect: number): "ok" | "blurry" | "stretched" {
  if ((item.sharp ?? 1) < 0.2) return "blurry";
  if (item.w && item.h) {
    const aspect = item.w / item.h;
    if (Math.abs(aspect - screenAspect) / screenAspect > 0.05) return "stretched";
  }
  return "ok";
}

/** 轮换计划：按间隔从可用池顺次取（排除清单生效）。 */
export function rotatePlan(pool: string[], excluded: string[], index: number): string | null {
  const usable = pool.filter((p) => !excluded.includes(p));
  if (usable.length === 0) return null;
  return usable[((index % usable.length) + usable.length) % usable.length] ?? null;
}

/* ================= 族0061 智能刷新 ================= */

/** 可见性感知刷新：不可见时刷新间隔放大 8 倍（省电，F01512）。 */
export function effectiveRefresh(baseSec: number, visible: boolean): number {
  const base = Math.max(5, baseSec);
  return visible ? base : base * 8;
}

/* ================= 族0069 内容风格审计 ================= */

/** 4 倍数间距审计：返回不合规值列表。 */
export function auditSpacing4(values: number[]): number[] {
  return values.filter((v) => v % 4 !== 0);
}

/** 1.25 字阶审计：以 12 为基，检查是否落在 {12,15,19,24,30,37,46…} 阶上。 */
export function auditTypeScale(values: number[], base = 12, ratio = 1.25, tolerance = 0.6): number[] {
  const steps: number[] = [];
  let v = base;
  while (v < 200) {
    steps.push(v);
    v = v * ratio;
  }
  return values.filter((n) => !steps.some((s) => Math.abs(s - n) <= tolerance));
}

/* ================= 族0075 仪式触发 ================= */

export interface RitualDef {
  entryId: string;
  kind: "daily-hour" | "weekday-hour" | "quiet-window" | "date";
  hour?: number;
  weekday?: number;
  from?: number;
  to?: number;
  /** "MM-DD" */
  date?: string;
}

export interface RitualFire {
  entryId: string;
}

/** 判定给定时刻应触发的仪式（小时粒度；每天每仪式至多一次由调用方记录）。 */
export function ritualDue(rules: RitualDef[], date: Date): RitualFire[] {
  const h = date.getHours();
  const wd = date.getDay();
  const md = `${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  const out: RitualFire[] = [];
  for (const r of rules) {
    switch (r.kind) {
      case "daily-hour":
        if (r.hour === h) out.push({ entryId: r.entryId });
        break;
      case "weekday-hour":
        if (r.weekday === wd && r.hour === h) out.push({ entryId: r.entryId });
        break;
      case "quiet-window":
        if (r.from !== undefined && r.to !== undefined && inQuietWindow(h, r.from, r.to)) out.push({ entryId: r.entryId });
        break;
      case "date":
        if (r.date === md && h === (r.hour ?? 0)) out.push({ entryId: r.entryId });
        break;
    }
  }
  return out;
}
