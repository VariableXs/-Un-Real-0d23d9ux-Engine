/**
 * NOVA-200 · S15 UI 度量路（AI-15）—— 域15 UI 设计与优化（W-176…W-187）。
 *
 * 边界（全景 §15）：
 * - W-176 黄金比实验室管**美学比例**参考（设计网格）；W-180 网格透视管**实现网格**
 *   （工程骨架，CSS Grid 参数 1:1）；二者互补不重叠；
 * - W-177 白空交响管**间距节奏**全局一致性；Z-64 CJK 排版管文字排版，不越界；
 * - W-178 动画成绩单管**自动体检**（清单 N-06 Motion 注册表同源摄入）；
 *   M-75 时长缩放 / Z-67 滚动动效归既有模块；
 * - W-179 控件考古馆是**历史风格**即时换装彩蛋（Win3.1/Win95/Win7 三代 CSS 皮肤）；
 *   Z-02 Fluent 规格复刻管规格一致；
 * - W-181 字体微唱机管**排印微参**（字重/字距/行高三旋钮）；V-76 管字体选择；
 * - W-182 图标配重天平管**视觉配重**均衡体检；Z-01 管网格对齐；
 * - W-183 光标形影一致管五场景**一致性体检**；Z-06 管系统接管、V-62 管方案；
 * - W-184 UI 物理热图管**空间点击分布**（匿名坐标）；Z-62 管功能使用统计；
 * - W-185 动画保释官管**局部无人看**循环动画降频暂停；Z-59 管全局空闲冻结；
 * - W-186 信息密度湿度计管**页面密度度量**（设计工具）；V-74 管用户密度档位；
 * - W-187 对比度哨兵管**全文对比度**全维 WCAG AA 巡检；V-80 管强调色一维。
 *
 * 纪律：
 * - 零侵入：不改任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://design-*`
 *   自定义事件摄入 + `nova.design.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-design-`、事件 `nova://design-*`、localStorage 键 `nova.design.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn）；非 DOM 环境行为层安全 no-op；
 * - 降级：reduce-motion / safeMode / static 三态动效归零（面板滑入、热图呼吸、
 *   换装计时语义保留），语义与数据不变；
 * - 默认档：W-176/177/179/180 与 S0 注册表一致默认关（dev/显式开启），其余默认开。
 *
 * 诚实边界：
 * - W-176/180 参考线层 pointer-events:none 零交互拦截，仅显式开启可见；
 * - W-177 间距采样来自真实 DOM（getBoundingClientRect 相邻差）；跑调=偏离基准
 *   （中位数）30%；无采样如实空态；
 * - W-178 动效清单同源 `nova://design-motion` 摄入（N-06 注册表喂入），无清单
 *   如实空态；同类时长差 > 20% 记问题项；
 * - W-179 换装期间交互保持可用（纯 CSS 皮肤，不改 DOM 结构）；Esc 立即归来，
 *   10s 自动归来；
 * - W-181 压盘保存为 `nova.design.turntable.v1` 预设（≤12 条），应用派发
 *   `nova://design-type-apply` 交 V-76/S17 落地；
 * - W-182 只出体检报告不改图标；配重数据经 `nova://design-icons` 摄入；
 * - W-183 五场景样本经 `nova://design-cursor` 摄入；缺场景如实标注未巡检；
 * - W-184 仅记录视口内匿名坐标 {x,y,ts}（零内容零选择器）；W-184 关闭即停采；
 *   30 天滚动剔除；
 * - W-185 视线判定零摄像头（IntersectionObserver + 悬停代理）；保释/复庭状态
 *   派发 `nova://design-bail-change`（W-147 噪声计可听）；
 * - W-186 三轴（元素密度/留白比/字密度）本地打分；抽检 ≤ 20 页封顶；
 * - W-187 WCAG 相对亮度对比度：正文 ≥ 4.5:1、大字（≥18pt 或 ≥14pt 粗体）≥ 3:1；
 *   装饰性文本白名单降误报；报告可导出 JSON（M-82 CI 由 S17 接）。
 */

import { novaMotionOK, novaOn } from "../registry";

// ---------------------------------------------------------------------------
// 通用工具（本模块自持）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion / safeMode / static 三态统一判定。 */
export function motionOK(): boolean {
  return novaMotionOK();
}

function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function lsSet(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full/blocked → 不持久化 */
  }
}

const NS = "nova.design";
const DAY = 86_400_000;

/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://design-*` 事件（SSR/测试环境安全）。 */
export function designEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://design-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// Hub 注册清单（S0 NovaHub 消费：功能卡 + 降级说明）
// ---------------------------------------------------------------------------

export interface NovaFeatureCard {
  id: string;
  titleZh: string;
  titleEn: string;
  descZh: string;
  defaultOn: boolean;
  /** 独立 overlay 工具窗（ai04:open-feature 的 feature id）。 */
  overlay?: string;
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const DESIGN_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-176",
    titleZh: "黄金比实验室",
    titleEn: "Phi Lab",
    descZh: "Ctrl+Alt+G 叠加黄金分割/三分线/根号矩形三制式美学参考网格，检验布局的美学结构；参考线零交互拦截，仅显式开启可见。",
    defaultOn: false,
    overlay: "nova-philab",
    wiringHint: "dev 模式或 hub 显式开启后 Ctrl+Alt+G 切换；比例即时循环切换",
    degrade: "静态参考线，无呼吸动画；比例与数值完全保留",
  },
  {
    id: "W-177",
    titleZh: "白空交响",
    titleEn: "Space Symphony",
    descZh: "全环境间距节奏五线谱：相邻元素间距=音高可视化，偏离基准（中位数）30% 处标红「跑调」；采样自真实 DOM，报告可导出。",
    defaultOn: false,
    overlay: "nova-symphony",
    wiringHint: "体检目标区域经 nova://design-space {gaps} 喂入，或面板内对当前视口采样",
    degrade: "静态五线谱与跑调标记",
  },
  {
    id: "W-178",
    titleZh: "动画成绩单",
    titleEn: "Motion Score",
    descZh: "注册动效的时长/曲线/位移三元一致性体检：同类操作时长差 ≤ 20% 记过关，出成绩单 + 逐项 diff；清单与 Motion 注册表（N-06）同源。",
    defaultOn: true,
    wiringHint: "Motion 注册表经 nova://design-motion {entries} 喂入 {id,group,ms,curve,px}",
    degrade: "静态成绩单与 diff 清单",
  },
  {
    id: "W-179",
    titleZh: "控件考古馆",
    titleEn: "Control Museum",
    descZh: "当前界面按钮/滚动条/菜单即时换装 Win3.1/Win95/Win7 三代真实 CSS 皮肤，浏览 10 秒自动归来，Esc 立即归来；换装期间交互保持可用。",
    defaultOn: false,
    wiringHint: "换装状态写入 <html data-nova-era>；皮肤 CSS 由本模块注入",
    degrade: "换装照常（纯 CSS 无动效依赖）；10s/Esc 归来语义不变",
  },
  {
    id: "W-180",
    titleZh: "网格透视",
    titleEn: "Grid X-Ray",
    descZh: "Ctrl+Shift+G 叠加当前布局的真实实现网格（CSS Grid 列/槽/边距线框），悬浮节点显示 span/gap 值——实现骨架的 X 光；仅显式开启可见。",
    defaultOn: false,
    overlay: "nova-xray",
    wiringHint: "扫描 display:grid 容器的 computed style，线框与布局参数 1:1",
    degrade: "静态线框与标注",
  },
  {
    id: "W-181",
    titleZh: "字体微唱机",
    titleEn: "Type Turntable",
    descZh: "界面样张（新闻/列表/设置三场景）下字重/字距/行高三旋钮实时旋调，满意后「压成唱片」保存为命名预设（≤12 条），派发应用事件入字体偏好。",
    defaultOn: true,
    overlay: "nova-turntable",
    wiringHint: "压盘预设派发 nova://design-type-apply {weight,tracking,lineHeight,name}，由 V-76/S17 落地",
    degrade: "静态样张与预设清单；三轴实时预览保留（非动效）",
  },
  {
    id: "W-182",
    titleZh: "图标配重天平",
    titleEn: "Icon Balance",
    descZh: "图标包视觉配重体检：每图标前景/背景面积比与重心偏移分析，偏移超阈值标红「左重右轻」；只出报告不改图标，报告可导出给图标包作者。",
    defaultOn: true,
    wiringHint: "图标前景遮罩经 nova://design-icons {icons:[{id,w,h,mask}]} 喂入（mask 为 0/1 权重行）",
    degrade: "静态天平报告与标红清单",
  },
  {
    id: "W-183",
    titleZh: "光标形影一致",
    titleEn: "Cursor Consistency",
    descZh: "光标在桌面/面板/输入/拖拽/等待五场景的形影一致性巡检：阴影角度/大小连续性，跳变处出具体检单 + 修复建议；缺场景如实标注未巡检。",
    defaultOn: true,
    wiringHint: "五场景样本经 nova://design-cursor {scenes} 喂入 {scene,cursor,shadowDeg,shadowPx}",
    degrade: "静态巡检单与建议",
  },
  {
    id: "W-184",
    titleZh: "UI 物理热图",
    titleEn: "UI Heatmap",
    descZh: "本地点击密度热图覆盖层：只记录视口内匿名坐标 {x,y,ts}（零内容零选择器），30 天滚动；关闭即停采；hub 可查「我总点哪里」。",
    defaultOn: true,
    overlay: "nova-heat",
    wiringHint: "点击捕获 pointerdown（仅 W-184 开启时）；覆盖层经 hub overlay 呈现",
    degrade: "静态热图色块，无呼吸动画；数据语义不变",
  },
  {
    id: "W-185",
    titleZh: "动画保释官",
    titleEn: "Motion Bail",
    descZh: "无人注视 >5s 的循环动画自动降频 50%，>10s 保释暂停；区域回到视线（IntersectionObserver/悬停代理，零摄像头）即复庭；状态派发噪声计可听。",
    defaultOn: true,
    wiringHint: "循环动画元素经 nova://design-loop-register {selector} 登记；状态变化派发 nova://design-bail-change",
    degrade: "reduce-motion 下循环动画本就归零，保释仅记录状态不再改频",
  },
  {
    id: "W-186",
    titleZh: "信息密度湿度计",
    titleEn: "Density Meter",
    descZh: "任一界面页的三维密度打分：元素密度/留白比/字密度 → 湿度 0–100，超密页标「潮湿」；三轴算法本地，抽检 ≤ 20 页，报告进设计走查清单。",
    defaultOn: true,
    wiringHint: "页面快照经 nova://design-density {pages:[{id,elements,viewportPx,textChars,blankPx}]} 喂入",
    degrade: "静态湿度计与三轴报告",
  },
  {
    id: "W-187",
    titleZh: "对比度哨兵",
    titleEn: "Contrast Sentinel",
    descZh: "全文 WCAG AA 对比度自动巡检：渲染树文本/背景组合逐对扫描，正文 < 4.5:1（大字 < 3:1）红框哨兵标记；装饰性文本白名单降误报，报告可导出入 CI。",
    defaultOn: true,
    wiringHint: "文本节点经 nova://design-contrast {nodes:[{id,fg,bg,fontSize,bold,decorative}]} 喂入；红框覆盖层 dev 呈现",
    degrade: "静态哨兵报告与红框标记",
  },
];

export const designNovaDomain = {
  id: "S15",
  nameZh: "UI 设计与优化",
  nameEn: "UI Design & Measure",
  route: "AI-15",
  features: DESIGN_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// W-176 黄金比实验室（Phi Lab）—— 三比例美学参考网格（纯逻辑）
// ---------------------------------------------------------------------------

export type PhiMode = "phi" | "thirds" | "root2";

export interface PhiRatioDef {
  key: PhiMode;
  zh: string;
  ratio: number | null; // null = 三分线（非单一比值）
}

/** 三制式：黄金分割 φ≈1.618 / 三分线 1/3 / 根号矩形 √2≈1.414。 */
export const PHI_RATIOS: PhiRatioDef[] = [
  { key: "phi", zh: "黄金分割", ratio: (1 + Math.sqrt(5)) / 2 },
  { key: "thirds", zh: "三分线", ratio: null },
  { key: "root2", zh: "根号矩形", ratio: Math.SQRT2 },
];

/** 比例制式即时循环切换（验收：三种比例即时切换）。 */
export function nextPhiMode(cur: PhiMode): PhiMode {
  const i = PHI_RATIOS.findIndex((r) => r.key === cur);
  return PHI_RATIOS[(i + 1) % PHI_RATIOS.length]!.key;
}

export interface PhiGrid {
  mode: PhiMode;
  vLines: number[];
  hLines: number[];
}

/** 参考网格线（px；v=竖线 x 坐标，h=横线 y 坐标）。 */
export function phiLines(mode: PhiMode, w: number, h: number): PhiGrid {
  if (!(w > 0) || !(h > 0)) return { mode, vLines: [], hLines: [] };
  if (mode === "thirds") {
    return { mode, vLines: [w / 3, (2 * w) / 3], hLines: [h / 3, (2 * h) / 3] };
  }
  const r = mode === "phi" ? (1 + Math.sqrt(5)) / 2 : Math.SQRT2;
  const a = w / r; // 主分割位
  const b = w / (r * r); // 次级分割位
  const ah = h / r;
  const bh = h / (r * r);
  return {
    mode,
    vLines: [a, w - a, b, w - b].filter((x) => x > 1 && x < w - 1),
    hLines: [ah, h - ah, bh, h - bh].filter((y) => y > 1 && y < h - 1),
  };
}

// ---------------------------------------------------------------------------
// W-177 白空交响（Space Symphony）—— 间距五线谱 + 跑调标记（纯逻辑）
// ---------------------------------------------------------------------------

export const OFFKEY_PCT = 0.3; // 跑调阈值：偏离基准（中位数）30%

export interface StaffLine {
  gap: number;
  /** 音高：log2 音阶位置（间距=音高隐喻）。 */
  pitch: number;
  offKey: boolean;
}

export interface SymphonyReport {
  base: number | null;
  lines: StaffLine[];
  total: number;
  offCount: number;
  offRatio: number;
  verdict: "empty" | "harmony" | "offkey";
}

function medianOf(xs: number[]): number | null {
  if (xs.length === 0) return null;
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
}

/** 间距样本 → 五线谱（基准=中位数；跑调=|gap-base|/base > 30%）。 */
export function symphonyOf(gaps: number[]): SymphonyReport {
  const clean = gaps.filter((g) => typeof g === "number" && Number.isFinite(g) && g >= 0);
  const base = medianOf(clean);
  if (base == null || base <= 0) {
    return { base: null, lines: [], total: 0, offCount: 0, offRatio: 0, verdict: "empty" };
  }
  const lines: StaffLine[] = clean.map((gap) => ({
    gap,
    pitch: Math.round(Math.log2(clamp(gap, 1, 4096)) * 12) / 12,
    offKey: Math.abs(gap - base) / base > OFFKEY_PCT,
  }));
  const offCount = lines.filter((l) => l.offKey).length;
  const offRatio = lines.length > 0 ? offCount / lines.length : 0;
  return { base, lines, total: lines.length, offCount, offRatio, verdict: offCount > 0 ? "offkey" : "harmony" };
}

/** 报告导出（JSON 文本；验收：报告可导出）。 */
export function exportSymphony(r: SymphonyReport): string {
  return JSON.stringify({ kind: "nova-design-symphony", v: 1, base: r.base, total: r.total, offCount: r.offCount, lines: r.lines }, null, 2);
}

// ---------------------------------------------------------------------------
// W-178 动画成绩单（Motion Score）—— 时长/曲线/位移三元体检（纯逻辑）
// ---------------------------------------------------------------------------

export interface MotionEntry {
  id: string;
  /** 同类操作分组（如 "drawer-open"/"toast-in"）。 */
  group: string;
  ms: number;
  curve: string;
  px: number;
}

export interface MotionIssue {
  id: string;
  group: string;
  field: "ms" | "curve" | "px";
  detail: string;
}

export interface MotionReport {
  checked: number;
  groups: number;
  issues: MotionIssue[];
  score: number;
  grade: "A" | "B" | "C" | "D";
}

export const MOTION_MS_TOLERANCE = 0.2; // 同类操作时长差 ≤ 20%
export const MOTION_PX_OUTLIER = 3; // 位移 > 组中位数 3 倍记离群

/** 三元体检：时长一致性（≤20%）/ 曲线一致性 / 位移离群。 */
export function motionScore(entries: MotionEntry[]): MotionReport {
  const ok = entries.filter((e) => e.id && e.group && Number.isFinite(e.ms) && e.ms > 0 && Number.isFinite(e.px) && e.px >= 0 && e.curve);
  const byGroup = new Map<string, MotionEntry[]>();
  for (const e of ok) {
    const list = byGroup.get(e.group) ?? [];
    list.push(e);
    byGroup.set(e.group, list);
  }
  const issues: MotionIssue[] = [];
  for (const [group, list] of byGroup) {
    if (list.length < 2) continue;
    const mss = list.map((e) => e.ms);
    const med = medianOf(mss)!;
    const spread = (Math.max(...mss) - Math.min(...mss)) / med;
    if (spread > MOTION_MS_TOLERANCE) {
      const worst = list.reduce((a, b) => (Math.abs(b.ms - med) > Math.abs(a.ms - med) ? b : a));
      issues.push({ id: worst.id, group, field: "ms", detail: `同类时长差 ${Math.round(spread * 100)}% > 20%（${worst.ms}ms vs 中位 ${Math.round(med)}ms）` });
    }
    const curves = new Set(list.map((e) => e.curve));
    if (curves.size > 1) {
      issues.push({ id: list[0]!.id, group, field: "curve", detail: `同类曲线不统一：${[...curves].join(" / ")}` });
    }
    const pxMed = medianOf(list.map((e) => e.px));
    if (pxMed != null && pxMed > 0) {
      for (const e of list) {
        if (e.px > pxMed * MOTION_PX_OUTLIER) {
          issues.push({ id: e.id, group, field: "px", detail: `位移 ${e.px}px > 组中位 ${Math.round(pxMed)}px 的 3 倍` });
          break; // 每组记首个离群即可定位
        }
      }
    }
  }
  const checked = ok.length;
  const score = checked === 0 ? 100 : Math.max(0, Math.round(100 - (issues.length / Math.max(checked, 1)) * 100));
  const grade = score >= 95 ? "A" : score >= 85 ? "B" : score >= 70 ? "C" : "D";
  return { checked, groups: byGroup.size, issues, score, grade };
}

// ---------------------------------------------------------------------------
// W-179 控件考古馆（Control Museum）—— 三代风格即时换装（纯逻辑）
// ---------------------------------------------------------------------------

export type MuseumEra = "win31" | "win95" | "win7";

export interface EraDef {
  key: MuseumEra;
  zh: string;
  year: string;
}

export const MUSEUM_ERAS: EraDef[] = [
  { key: "win31", zh: "Win 3.1", year: "1992" },
  { key: "win95", zh: "Win 95", year: "1995" },
  { key: "win7", zh: "Win 7", year: "2009" },
];

export const MUSEUM_RETURN_MS = 10_000; // 浏览 10 秒自动归来

/** 年代循环切换。 */
export function nextEra(cur: MuseumEra | null): MuseumEra {
  if (cur == null) return MUSEUM_ERAS[0]!.key;
  const i = MUSEUM_ERAS.findIndex((e) => e.key === cur);
  return MUSEUM_ERAS[(i + 1) % MUSEUM_ERAS.length]!.key;
}

/** 换装属性值（null = 归来/卸妆）。 */
export function eraAttr(era: MuseumEra | null): string | null {
  return era == null ? null : `nova-${era}`;
}

// ---------------------------------------------------------------------------
// W-180 网格透视（Grid X-Ray）—— 实现网格 X 光（纯逻辑）
// ---------------------------------------------------------------------------

export interface XrayRect {
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
}

/** 列轨道切分（与 CSS Grid 参数 1:1：总宽/列数/槽宽 → 各列 {x,w}）。 */
export function trackCells(total: number, count: number, gap: number): Array<{ x: number; w: number }> {
  if (!(total > 0) || !(count > 0) || gap < 0) return [];
  const w = (total - gap * (count - 1)) / count;
  if (w <= 0) return [];
  return Array.from({ length: count }, (_, i) => ({ x: i * (w + gap), w }));
}

/** 由网格参数生成 X 光线框（含 span/gap 标注）。 */
export function xrayCells(
  box: { x: number; y: number; w: number; h: number },
  cols: number,
  rows: number,
  gap: number,
): XrayRect[] {
  const colCells = trackCells(box.w, cols, gap);
  const rowCells = trackCells(box.h, rows, gap);
  const out: XrayRect[] = [];
  for (const r of rowCells) {
    for (const c of colCells) {
      out.push({
        x: box.x + c.x,
        y: box.y + r.x, // 行轨道：x=纵向偏移
        w: c.w,
        h: r.w, // 行轨道：w=行高
        label: `span 1/${cols} · gap ${gap}px`,
      });
    }
  }
  return out;
}

/** 节点 span 标注（悬浮显示 span/gap 值；验收）。 */
export function spanLabel(colStart: number, colEnd: number, gap: number): string {
  const span = Math.max(1, colEnd - colStart + 1);
  return `span ${span} · col ${colStart}–${colEnd} · gap ${gap}px`;
}

// ---------------------------------------------------------------------------
// W-181 字体微唱机（Type Turntable）—— 三旋钮排印微参（纯逻辑）
// ---------------------------------------------------------------------------

export interface TypeKnobs {
  /** 字重 100–900（步 100）。 */
  weight: number;
  /** 字距（千分之一 em，-50…200）。 */
  tracking: number;
  /** 行高（1.0–2.0，步 0.05）。 */
  lineHeight: number;
}

export const TYPE_KNOB_DEF = {
  weight: { min: 100, max: 900, step: 100, def: 400 },
  tracking: { min: -50, max: 200, step: 5, def: 0 },
  lineHeight: { min: 1.0, max: 2.0, step: 0.05, def: 1.5 },
} as const;

export const TYPE_PRESET_MAX = 12;
export const SPECIMEN_SCENES = [
  { key: "news", zh: "新闻" },
  { key: "list", zh: "列表" },
  { key: "settings", zh: "设置" },
] as const;

/** 三旋钮取值钳制（超界回界内）。 */
export function normalizeKnobs(k: Partial<TypeKnobs>): TypeKnobs {
  const s = (v: number | undefined, min: number, max: number, step: number, def: number): number => {
    if (typeof v !== "number" || !Number.isFinite(v)) return def;
    const snapped = Math.round(v / step) * step;
    return clamp(Math.round(snapped * 100) / 100, min, max);
  };
  return {
    weight: s(k.weight, TYPE_KNOB_DEF.weight.min, TYPE_KNOB_DEF.weight.max, TYPE_KNOB_DEF.weight.step, TYPE_KNOB_DEF.weight.def),
    tracking: s(k.tracking, TYPE_KNOB_DEF.tracking.min, TYPE_KNOB_DEF.tracking.max, TYPE_KNOB_DEF.tracking.step, TYPE_KNOB_DEF.tracking.def),
    lineHeight: s(k.lineHeight, TYPE_KNOB_DEF.lineHeight.min, TYPE_KNOB_DEF.lineHeight.max, TYPE_KNOB_DEF.lineHeight.step, TYPE_KNOB_DEF.lineHeight.def),
  };
}

/** 旋钮 → CSS 变量片段（实时预览 ≤ 1 帧：单次 style 写入）。 */
export function typographyCss(k: TypeKnobs): { fontWeight: string; letterSpacing: string; lineHeight: string } {
  const n = normalizeKnobs(k);
  return {
    fontWeight: String(n.weight),
    letterSpacing: `${n.tracking / 1000}em`,
    lineHeight: n.lineHeight.toFixed(2),
  };
}

export interface TypePreset {
  name: string;
  knobs: TypeKnobs;
  savedAt: number;
}

/** 压盘：命名保存预设（≤12 条，重名覆盖）。 */
export function savePreset(presets: TypePreset[], p: TypePreset): TypePreset[] {
  const norm: TypePreset = { name: p.name.slice(0, 24) || "未命名", knobs: normalizeKnobs(p.knobs), savedAt: p.savedAt };
  const i = presets.findIndex((x) => x.name === norm.name);
  if (i >= 0) {
    const next = [...presets];
    next[i] = norm;
    return next;
  }
  return [...presets, norm].slice(-TYPE_PRESET_MAX);
}

// ---------------------------------------------------------------------------
// W-182 图标配重天平（Icon Balance）—— 视觉配重体检（纯逻辑）
// ---------------------------------------------------------------------------

export interface IconSample {
  id: string;
  w: number;
  h: number;
  /** 前景权重行（h 行 × w 列，0=背景 1=前景，0–1 权重）。 */
  mask: number[][];
}

export interface IconBalanceResult {
  id: string;
  /** 前景占比 0–1。 */
  fgRatio: number;
  /** 重心（画布归一 0–1）。 */
  cx: number;
  cy: number;
  /** 重心偏移（半幅归一 0–1：0=正中，1=贴边）。 */
  offset: number;
  verdict: "balanced" | "slight" | "off";
}

export const BALANCE_SLIGHT = 0.15; // 轻偏阈值
export const BALANCE_OFF = 0.3; // 失衡阈值

/** 单图标配重：前景占比 + 重心偏移（验收：重心偏移 ≤ 阈值）。 */
export function iconBalance(icon: IconSample): IconBalanceResult {
  const { w, h, mask } = icon;
  if (!(w > 0) || !(h > 0) || mask.length === 0) {
    return { id: icon.id, fgRatio: 0, cx: 0.5, cy: 0.5, offset: 0, verdict: "balanced" };
  }
  let wSum = 0;
  let sx = 0;
  let sy = 0;
  for (let y = 0; y < mask.length; y++) {
    const row = mask[y] ?? [];
    for (let x = 0; x < Math.min(row.length, w); x++) {
      const v = clamp(Number(row[x]) || 0, 0, 1);
      wSum += v;
      sx += v * x;
      sy += v * y;
    }
  }
  if (wSum <= 0) return { id: icon.id, fgRatio: 0, cx: 0.5, cy: 0.5, offset: 0, verdict: "balanced" };
  const cx = sx / wSum / (w - 1 || 1);
  const cy = sy / wSum / (h - 1 || 1);
  const dx = Math.abs(cx - 0.5) * 2; // 半幅归一
  const dy = Math.abs(cy - 0.5) * 2;
  const offset = Math.max(dx, dy);
  const verdict = offset > BALANCE_OFF ? "off" : offset > BALANCE_SLIGHT ? "slight" : "balanced";
  return { id: icon.id, fgRatio: wSum / (w * h), cx, cy, offset, verdict };
}

export interface BalanceReport {
  checked: number;
  flagged: IconBalanceResult[];
  worst: IconBalanceResult | null;
}

/** 图标包天平报告（只体检不改图标；验收）。 */
export function balanceReport(icons: IconSample[]): BalanceReport {
  const results = icons.filter((i) => i && i.id).map(iconBalance);
  const flagged = results.filter((r) => r.verdict !== "balanced");
  const worst = flagged.length > 0 ? flagged.reduce((a, b) => (b.offset > a.offset ? b : a)) : null;
  return { checked: results.length, flagged, worst };
}

// ---------------------------------------------------------------------------
// W-183 光标形影一致（Cursor Consistency）—— 五场景一致性巡检（纯逻辑）
// ---------------------------------------------------------------------------

export const CURSOR_SCENES = ["desktop", "panel", "input", "drag", "wait"] as const;
export type CursorScene = (typeof CURSOR_SCENES)[number];

export interface CursorSceneSample {
  scene: CursorScene | string;
  cursor: string;
  /** 阴影角度（deg）。 */
  shadowDeg: number;
  /** 阴影大小（px）。 */
  shadowPx: number;
}

export interface CursorJump {
  from: string;
  to: string;
  field: "shadowDeg" | "shadowPx";
  delta: number;
  suggest: string;
}

export interface CursorAuditReport {
  inspected: string[];
  missing: string[];
  jumps: CursorJump[];
  verdict: "insufficient" | "consistent" | "jumpy";
}

export const CURSOR_DEG_TOL = 15; // 阴影角度跳变阈值
export const CURSOR_PX_TOL = 2; // 阴影大小跳变阈值

/** 相邻场景形影连续性巡检（缺场景如实标注未巡检）。 */
export function cursorAudit(scenes: CursorSceneSample[]): CursorAuditReport {
  const present = new Map<string, CursorSceneSample>();
  for (const s of scenes) {
    if (s && typeof s.scene === "string" && typeof s.cursor === "string") present.set(s.scene, s);
  }
  const inspected = CURSOR_SCENES.filter((k) => present.has(k)).map(String);
  const missing = CURSOR_SCENES.filter((k) => !present.has(k)).map(String);
  if (inspected.length < 2) {
    return { inspected, missing, jumps: [], verdict: "insufficient" };
  }
  const order = [...CURSOR_SCENES].filter((k) => present.has(k)).map(String);
  const jumps: CursorJump[] = [];
  for (let i = 1; i < order.length; i++) {
    const a = present.get(order[i - 1]!)!;
    const b = present.get(order[i]!)!;
    const dDeg = Math.abs(b.shadowDeg - a.shadowDeg);
    if (dDeg > CURSOR_DEG_TOL) {
      jumps.push({ from: a.scene, to: b.scene, field: "shadowDeg", delta: Math.round(dDeg), suggest: `统一 ${a.scene}→${b.scene} 阴影角度（建议中位 ${Math.round((a.shadowDeg + b.shadowDeg) / 2)}°）` });
    }
    const dPx = Math.abs(b.shadowPx - a.shadowPx);
    if (dPx > CURSOR_PX_TOL) {
      jumps.push({ from: a.scene, to: b.scene, field: "shadowPx", delta: Math.round(dPx * 10) / 10, suggest: `统一 ${a.scene}→${b.scene} 阴影大小（建议中位 ${Math.round(((a.shadowPx + b.shadowPx) / 2) * 10) / 10}px）` });
    }
  }
  return { inspected, missing, jumps, verdict: jumps.length > 0 ? "jumpy" : "consistent" };
}

// ---------------------------------------------------------------------------
// W-184 UI 物理热图（UI Heatmap）—— 匿名点击分布（纯逻辑）
// ---------------------------------------------------------------------------

export const HEAT_BUCKET_PX = 48; // 聚合桶
export const HEAT_KEEP_MS = 30 * DAY; // 30 天滚动
export const HEAT_CAP = 20_000; // 坐标条数封顶

export interface HeatPoint {
  /** 视口内匿名坐标（零内容零选择器）。 */
  x: number;
  y: number;
  ts: number;
}

/** 30 天滚动剔除。 */
export function evictHeat(log: HeatPoint[], now: number): HeatPoint[] {
  return log.filter((p) => now - p.ts < HEAT_KEEP_MS);
}

/** 记录一次匿名点击（封顶滚动；关闭即停采由行为层保证）。 */
export function recordHeat(log: HeatPoint[], x: number, y: number, now: number): HeatPoint[] {
  if (!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0) return log;
  return [...log, { x: Math.round(x), y: Math.round(y), ts: now }].slice(-HEAT_CAP);
}

export interface HeatCell {
  x: number;
  y: number;
  count: number;
  level: 0 | 1 | 2 | 3 | 4;
}

/** 桶聚合 + 5 档热度（0 最冷，4 最热；按热度降序全量返回）。 */
export function heatCells(log: HeatPoint[]): HeatCell[] {
  const buckets = new Map<string, HeatCell>();
  for (const p of log) {
    const bx = Math.floor(p.x / HEAT_BUCKET_PX);
    const by = Math.floor(p.y / HEAT_BUCKET_PX);
    const key = `${bx}:${by}`;
    const cell = buckets.get(key) ?? { x: bx, y: by, count: 0, level: 0 as HeatCell["level"] };
    cell.count += 1;
    buckets.set(key, cell);
  }
  const cells = [...buckets.values()];
  if (cells.length === 0) return [];
  const max = Math.max(...cells.map((c) => c.count));
  for (const c of cells) {
    const t = max > 0 ? c.count / max : 0;
    c.level = (t > 0.75 ? 4 : t > 0.5 ? 3 : t > 0.25 ? 2 : t > 0 ? 1 : 0) as HeatCell["level"];
  }
  return cells.sort((a, b) => b.count - a.count);
}

// ---------------------------------------------------------------------------
// W-185 动画保释官（Motion Bail）—— 无人看循环动画降频暂停（纯逻辑）
// ---------------------------------------------------------------------------

export type BailState = "full" | "half" | "paused";

export const BAIL_HALF_MS = 5_000; // >5s 无人看 → 降频 50%
export const BAIL_PAUSE_MS = 10_000; // >10s → 保释暂停

/** 无人看时长 → 保释状态。 */
export function bailOf(unseenMs: number): BailState {
  if (unseenMs >= BAIL_PAUSE_MS) return "paused";
  if (unseenMs >= BAIL_HALF_MS) return "half";
  return "full";
}

/** 保释状态 → CSS 类（半速=动画时长×2；暂停=animation-play-state）。 */
export function bailClass(state: BailState): string {
  if (state === "half") return "nova-design-bail-half";
  if (state === "paused") return "nova-design-bail-paused";
  return "";
}

// ---------------------------------------------------------------------------
// W-186 信息密度湿度计（Density Meter）—— 三维密度打分（纯逻辑）
// ---------------------------------------------------------------------------

export interface DensityPage {
  id: string;
  /** 可见元素数。 */
  elements: number;
  /** 视口面积（px²）。 */
  viewportPx: number;
  /** 正文字符数。 */
  textChars: number;
  /** 留白面积（px²）。 */
  blankPx: number;
}

export interface DensityResult {
  id: string;
  /** 元素密度分（每万 px² 元素数 → 0–100）。 */
  eScore: number;
  /** 留白比（0–1 → 逆向分）。 */
  airScore: number;
  /** 字密度分（每万 px² 字数 → 0–100）。 */
  cScore: number;
  /** 湿度 0–100（越高越潮）。 */
  humidity: number;
  verdict: "dry" | "mild" | "damp";
}

export const DENSITY_DAMP = 66; // 潮湿线
export const DENSITY_MILD = 33; // 微潮线
export const DENSITY_PAGE_CAP = 20; // 抽检封顶 20 页

function score100(v: number, soft: number): number {
  // 软饱和：v 达 soft 的 1/2 记 50，达 soft 记 100
  return clamp(Math.round((v / soft) * 100), 0, 100);
}

/** 三轴密度打分（元素/留白/字数 → 湿度）。 */
export function densityOf(page: DensityPage): DensityResult {
  const vp = page.viewportPx > 0 ? page.viewportPx : 1;
  const per10k = (n: number): number => (n / vp) * 10_000;
  const eScore = score100(per10k(page.elements), 6);
  const cScore = score100(per10k(page.textChars), 120);
  const blankRatio = clamp(page.blankPx / vp, 0, 1);
  const airScore = Math.round((1 - blankRatio) * 100); // 留白越少分越高（越潮）
  const humidity = clamp(Math.round(eScore * 0.4 + airScore * 0.2 + cScore * 0.4), 0, 100);
  const verdict = humidity > DENSITY_DAMP ? "damp" : humidity > DENSITY_MILD ? "mild" : "dry";
  return { id: page.id, eScore, airScore, cScore, humidity, verdict };
}

export interface DensityAudit {
  pages: DensityResult[];
  damp: string[];
  worst: DensityResult | null;
}

/** 页面集走查（≤20 页封顶，报告进设计走查清单）。 */
export function densityAudit(pages: DensityPage[]): DensityAudit {
  const results = pages
    .filter((p) => p && p.id)
    .slice(0, DENSITY_PAGE_CAP)
    .map(densityOf);
  const damp = results.filter((r) => r.verdict === "damp").map((r) => r.id);
  const worst = results.length > 0 ? results.reduce((a, b) => (b.humidity > a.humidity ? b : a)) : null;
  return { pages: results, damp, worst };
}

// ---------------------------------------------------------------------------
// W-187 对比度哨兵（Contrast Sentinel）—— WCAG AA 全文巡检（纯逻辑）
// ---------------------------------------------------------------------------

export interface ContrastNode {
  id: string;
  /** 前景色 #RRGGBB。 */
  fg: string;
  /** 背景色 #RRGGBB。 */
  bg: string;
  /** 字号 px。 */
  fontSize: number;
  bold?: boolean;
  /** 装饰性文本（白名单，不计失败降误报）。 */
  decorative?: boolean;
}

export interface ContrastFail {
  id: string;
  ratio: number;
  need: number;
  large: boolean;
}

export interface SentinelReport {
  checked: number;
  whitelisted: number;
  fails: ContrastFail[];
  worst: ContrastFail | null;
  verdict: "empty" | "pass" | "fail";
}

/** #RRGGBB / #RGB → [r,g,b]（0–255）；不可解析返回 null（如实跳过不编造）。 */
export function parseHex(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.exec(hex?.trim() ?? "");
  if (!m) return null;
  let h = m[1]!;
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

/** WCAG 相对亮度。 */
export function relLum(rgb: [number, number, number]): number {
  const f = (c: number): number => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]);
}

/** 对比度（1–21）。 */
export function contrastRatio(fg: string, bg: string): number | null {
  const a = parseHex(fg);
  const b = parseHex(bg);
  if (!a || !b) return null;
  const la = relLum(a);
  const lb = relLum(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

/** 大字判定（≥18pt=24px，或 ≥14pt=18.66px 粗体）。 */
export function isLargeText(fontSize: number, bold: boolean): boolean {
  return fontSize >= 24 || (bold && fontSize >= 18.66);
}

/** AA 阈值：正文 4.5:1，大字 3:1。 */
export function aaNeed(fontSize: number, bold: boolean): number {
  return isLargeText(fontSize, bold) ? 3 : 4.5;
}

/** 全文哨兵巡检（装饰白名单降误报；不可解析如实跳过）。 */
export function sentinelReport(nodes: ContrastNode[]): SentinelReport {
  const fails: ContrastFail[] = [];
  let checked = 0;
  let whitelisted = 0;
  for (const n of nodes) {
    if (!n || !n.id) continue;
    if (n.decorative) {
      whitelisted += 1;
      continue;
    }
    const ratio = contrastRatio(n.fg, n.bg);
    if (ratio == null) continue; // 不可解析：如实跳过（不编造）
    checked += 1;
    const need = aaNeed(n.fontSize ?? 16, n.bold === true);
    if (ratio < need) {
      fails.push({ id: n.id, ratio: Math.round(ratio * 100) / 100, need, large: need === 3 });
    }
  }
  const worst = fails.length > 0 ? fails.reduce((a, b) => (b.ratio < a.ratio ? b : a)) : null;
  return { checked, whitelisted, fails, worst, verdict: fails.length > 0 ? "fail" : checked > 0 ? "pass" : "empty" };
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件摄入 + 持久化；非 DOM 环境 no-op）
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById("nova-design-style")) return;
  const st = document.createElement("style");
  st.id = "nova-design-style";
  st.textContent = `
.nova-design-root{position:fixed;inset:auto auto 16px 16px;z-index:2147483000;display:flex;flex-direction:column;gap:8px;font:12px/1.5 Consolas,monospace;color:#e8e8ec;background:rgba(18,18,24,.92);border:1px solid rgba(255,255,255,.14);border-radius:10px;box-shadow:0 8px 28px rgba(0,0,0,.45);padding:10px 12px;max-width:360px}
.nova-design-root[hidden]{display:none}
.nova-design-h{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#9aa0b0;margin:2px 0 6px}
.nova-design-tabs{display:flex;gap:4px;flex-wrap:wrap}
.nova-design-tab{font:11px Consolas,monospace;padding:3px 8px;border-radius:6px;border:1px solid rgba(255,255,255,.16);background:transparent;color:#c8ccd8;cursor:pointer}
.nova-design-tab[aria-selected="true"]{background:#3d5afe;color:#fff;border-color:#3d5afe}
.nova-design-body{max-height:300px;overflow:auto;display:flex;flex-direction:column;gap:6px;min-width:260px}
.nova-design-muted{color:#8b91a3}
.nova-design-mono{font:11px Consolas,monospace;white-space:pre-wrap}
.nova-design-card{border:1px solid rgba(255,255,255,.1);border-radius:8px;padding:6px 8px}
.nova-design-row{display:flex;align-items:center;gap:6px}
.nova-design-bar{flex:1;height:6px;border-radius:3px;background:rgba(255,255,255,.12);overflow:hidden}
.nova-design-fill{height:100%;border-radius:3px;background:#3d5afe}
.nova-design-bad{color:#ff6b6b}
.nova-design-ok{color:#69db7c}
.nova-design-btn{font:11px Consolas,monospace;padding:3px 10px;border-radius:6px;border:1px solid rgba(255,255,255,.2);background:#262a36;color:#e8e8ec;cursor:pointer}
.nova-design-btn:hover{background:#31364a}
.nova-design-knob{width:150px}
.nova-design-ta{width:100%;min-height:44px;background:#15161d;color:#e8e8ec;border:1px solid rgba(255,255,255,.14);border-radius:6px;padding:6px;font:11px Consolas,monospace}
.nova-design-spec{background:#fff;color:#111;border-radius:6px;padding:10px}
/* W-176/180 叠层：零交互拦截 */
.nova-design-overlay{position:fixed;inset:0;z-index:2147482000;pointer-events:none}
.nova-design-phi-line{position:absolute;background:rgba(255,193,7,.55)}
.nova-design-phi-v{width:1px;top:0;bottom:0}
.nova-design-phi-h{height:1px;left:0;right:0}
.nova-design-xray-cell{position:absolute;border:1px dashed rgba(0,229,255,.75);background:rgba(0,229,255,.06)}
.nova-design-xray-label{position:absolute;top:0;left:0;font:10px Consolas,monospace;background:rgba(0,229,255,.9);color:#00222a;padding:1px 4px;border-radius:3px;white-space:nowrap}
/* W-184 热图 */
.nova-design-heat-cell{position:absolute;border-radius:4px;pointer-events:none}
/* W-185 保释 */
.nova-design-bail-half{animation-duration:calc(var(--nova-bail-dur,1s)*2)!important}
.nova-design-bail-paused{animation-play-state:paused!important}
/* W-179 考古换装（三代皮肤：仅 CSS，不改 DOM；交互保持可用） */
[data-nova-era="nova-win31"] button,[data-nova-era="nova-win31"] .nova-era-able{border:2px solid #000!important;background:#d4d0c8!important;color:#000!important;border-radius:0!important;box-shadow:1px 1px 0 #000!important;font-family:"MS Sans Serif",Tahoma,sans-serif!important}
[data-nova-era="nova-win95"] button,[data-nova-era="nova-win95"] .nova-era-able{border:1px outset #fff!important;background:#c0c0c0!important;color:#000!important;border-radius:0!important;font-family:"MS Sans Serif",Tahoma,sans-serif!important}
[data-nova-era="nova-win7"] button,[data-nova-era="nova-win7"] .nova-era-able{border:1px solid #898b8f!important;border-radius:3px!important;background:linear-gradient(#f0f4f9,#d9e4f5)!important;color:#1c2a47!important;box-shadow:inset 0 0 0 1px #fff!important;font-family:Segoe UI,Tahoma,sans-serif!important}
[data-nova-era="nova-win31"] ::-webkit-scrollbar,[data-nova-era="nova-win95"] ::-webkit-scrollbar{width:16px!important;background:#d4d0c8!important}
[data-nova-era="nova-win31"] ::-webkit-scrollbar-thumb,[data-nova-era="nova-win95"] ::-webkit-scrollbar-thumb{background:#808080!important;border:1px solid #000!important}
[data-nova-era="nova-win7"] ::-webkit-scrollbar{width:12px!important}
[data-nova-era="nova-win7"] ::-webkit-scrollbar-thumb{background:#b8c6dc!important;border-radius:6px!important;border:2px solid #f0f4f9!important}
html[data-nova-era="nova-win31"],html[data-nova-era="nova-win95"]{filter:saturate(.75)}
html[data-nova-era="nova-win7"]{filter:saturate(1.05) contrast(.98)}
/* 降级：reduce-motion / safeMode / static → 动效归零 */
[data-reduce-motion="true"] .nova-design-root,[data-safe-mode="true"] .nova-design-root,[data-static-mode="true"] .nova-design-root{transition:none;animation:none}
`;
  document.head.appendChild(st);
  bag.push(() => st.remove());
}

// --- 行为层状态（摄入镜像 + 持久化） ---

type TabId = "score" | "balance" | "cursor" | "heat" | "density" | "sentinel";

let panel: HTMLElement | null = null;
let curTab: TabId = "score";
let motionEntries: MotionEntry[] = [];
let iconSamples: IconSample[] = [];
let cursorSamples: CursorSceneSample[] = [];
let densityPages: DensityPage[] = [];
let contrastNodes: ContrastNode[] = [];
let spaceGaps: number[] = [];

// W-176/180 叠层
let phiMode: PhiMode = "phi";
let phiOverlay: HTMLElement | null = null;
let xrayOverlay: HTMLElement | null = null;

// W-181 唱机
let turntable: HTMLElement | null = null;
let knobs: TypeKnobs = normalizeKnobs({});

// W-184 热图
let heatOverlay: HTMLElement | null = null;

// W-179 考古
let eraTimer = 0;

// W-185 保释
interface BailTrack {
  el: Element;
  seen: boolean;
  unseenSince: number;
  state: BailState;
}
let bailTracks: BailTrack[] = [];
let bailTimer = 0;

function loadState(): void {
  motionEntries = lsGet<MotionEntry[]>(`${NS}.motion.v1`, []);
  iconSamples = lsGet<IconSample[]>(`${NS}.icons.v1`, []);
  cursorSamples = lsGet<CursorSceneSample[]>(`${NS}.cursor.v1`, []);
  densityPages = lsGet<DensityPage[]>(`${NS}.density.v1`, []);
  contrastNodes = lsGet<ContrastNode[]>(`${NS}.contrast.v1`, []);
  spaceGaps = lsGet<number[]>(`${NS}.space.v1`, []);
  knobs = normalizeKnobs(lsGet<Partial<TypeKnobs>>(`${NS}.knobs.v1`, {}));
}

// --- W-176 黄金比叠层（零交互拦截） ---

function renderPhi(): void {
  phiOverlay?.remove();
  phiOverlay = null;
  if (typeof document === "undefined" || !flagOn("W-176")) return;
  const w = window.innerWidth;
  const h = window.innerHeight;
  const grid = phiLines(phiMode, w, h);
  const ov = document.createElement("div");
  ov.className = "nova-design-overlay";
  const zh = PHI_RATIOS.find((r) => r.key === phiMode)?.zh ?? phiMode;
  const lines = [
    ...grid.vLines.map((x) => `<div class="nova-design-phi-line nova-design-phi-v" style="left:${x}px"></div>`),
    ...grid.hLines.map((y) => `<div class="nova-design-phi-line nova-design-phi-h" style="top:${y}px"></div>`),
  ].join("");
  ov.innerHTML = `${lines}<div style="position:absolute;top:8px;left:8px;font:11px Consolas,monospace;background:rgba(18,18,24,.85);color:#ffc107;padding:2px 8px;border-radius:4px">PHI LAB · ${esc(zh)} · ${PHI_RATIOS.find((r) => r.key === phiMode)?.ratio?.toFixed(3) ?? "1/3"} · Ctrl+Alt+G 切换</div>`;
  document.body.appendChild(ov);
  phiOverlay = ov;
}

function togglePhi(force?: boolean): void {
  const show = force ?? phiOverlay == null;
  if (show) renderPhi();
  else {
    phiOverlay?.remove();
    phiOverlay = null;
  }
}

// --- W-177 白空交响（视口采样 + 五线谱渲染在面板） ---

function sampleViewportGaps(): number[] {
  if (typeof document === "undefined") return [];
  const els = [...document.querySelectorAll<HTMLElement>("body *")].filter((el) => {
    if (el.closest(".nova-design-root,.nova-design-overlay")) return false;
    const r = el.getBoundingClientRect();
    return r.width > 8 && r.height > 8 && r.bottom > 0 && r.top < window.innerHeight;
  });
  els.sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top || a.getBoundingClientRect().left - b.getBoundingClientRect().left);
  const gaps: number[] = [];
  for (let i = 1; i < els.length && gaps.length < 240; i++) {
    const a = els[i - 1]!.getBoundingClientRect();
    const b = els[i]!.getBoundingClientRect();
    if (Math.abs(a.left - b.left) < 4) gaps.push(Math.round(Math.abs(b.top - a.bottom)));
  }
  return gaps.filter((g) => g > 0);
}

// --- W-180 网格透视叠层（CSS Grid 参数 1:1） ---

function renderXray(): void {
  xrayOverlay?.remove();
  xrayOverlay = null;
  if (typeof document === "undefined" || !flagOn("W-180")) return;
  const ov = document.createElement("div");
  ov.className = "nova-design-overlay";
  let count = 0;
  for (const el of [...document.querySelectorAll<HTMLElement>("body *")]) {
    if (el.closest(".nova-design-root,.nova-design-overlay") || count >= 40) continue;
    const cs = getComputedStyle(el);
    if (cs.display !== "grid") continue;
    const cols = cs.gridTemplateColumns ? cs.gridTemplateColumns.split(" ").filter((x) => x.includes("px")).length : 0;
    const gap = parseFloat(cs.columnGap || cs.gap || "0") || 0;
    if (cols < 2) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 60 || r.height < 40) continue;
    count += 1;
    for (const cell of xrayCells({ x: r.left, y: r.top, w: r.width, h: r.height }, cols, cols, gap)) {
      const d = document.createElement("div");
      d.className = "nova-design-xray-cell";
      d.style.left = `${cell.x}px`;
      d.style.top = `${cell.y}px`;
      d.style.width = `${cell.w}px`;
      d.style.height = `${cell.h}px`;
      d.innerHTML = `<span class="nova-design-xray-label">${esc(cell.label)}</span>`;
      ov.appendChild(d);
    }
  }
  const tag = document.createElement("div");
  tag.style.cssText = "position:absolute;top:8px;right:8px;font:11px Consolas,monospace;background:rgba(18,18,24,.85);color:#00e5ff;padding:2px 8px;border-radius:4px";
  tag.textContent = `GRID X-RAY · ${count} containers · Ctrl+Shift+G 切换`;
  ov.appendChild(tag);
  document.body.appendChild(ov);
  xrayOverlay = ov;
}

function toggleXray(force?: boolean): void {
  const show = force ?? xrayOverlay == null;
  if (show) renderXray();
  else {
    xrayOverlay?.remove();
    xrayOverlay = null;
  }
}

// --- W-184 热图叠层 ---

function renderHeat(): void {
  heatOverlay?.remove();
  heatOverlay = null;
  if (typeof document === "undefined" || !flagOn("W-184")) return;
  const log = evictHeat(lsGet<HeatPoint[]>(`${NS}.heat.v1`, []), Date.now());
  const cells = heatCells(log);
  if (cells.length === 0) return;
  const ov = document.createElement("div");
  ov.className = "nova-design-overlay";
  const colors = ["rgba(56,189,248,.18)", "rgba(74,222,128,.28)", "rgba(250,204,21,.38)", "rgba(251,146,60,.5)", "rgba(239,68,68,.62)"];
  for (const c of cells) {
    const d = document.createElement("div");
    d.className = "nova-design-heat-cell";
    d.style.left = `${c.x * HEAT_BUCKET_PX}px`;
    d.style.top = `${c.y * HEAT_BUCKET_PX}px`;
    d.style.width = `${HEAT_BUCKET_PX}px`;
    d.style.height = `${HEAT_BUCKET_PX}px`;
    d.style.background = colors[c.level] ?? colors[0]!;
    ov.appendChild(d);
  }
  document.body.appendChild(ov);
  heatOverlay = ov;
}

function toggleHeat(force?: boolean): void {
  const show = force ?? heatOverlay == null;
  if (show) renderHeat();
  else {
    heatOverlay?.remove();
    heatOverlay = null;
  }
}

// --- W-179 控件考古馆（换装 + Esc 归来 + 10s 自动） ---

function setEra(era: MuseumEra | null): void {
  if (typeof document === "undefined") return;
  const html = document.documentElement;
  const val = eraAttr(era);
  if (val) html.setAttribute("data-nova-era", val);
  else html.removeAttribute("data-nova-era");
  if (eraTimer) {
    clearTimeout(eraTimer);
    eraTimer = 0;
  }
  if (era) {
    eraTimer = window.setTimeout(() => setEra(null), MUSEUM_RETURN_MS); // 10s 自动归来
    designEvent("era-change", { era });
  } else {
    designEvent("era-change", { era: null });
  }
}

function toggleMuseum(): void {
  const cur = document.documentElement.getAttribute("data-nova-era");
  const curEra = (MUSEUM_ERAS.find((e) => `nova-${e.key}` === cur)?.key ?? null) as MuseumEra | null;
  setEra(curEra == null ? nextEra(null) : nextEra(curEra));
}

// --- W-185 动画保释官（IntersectionObserver + 悬停代理，零摄像头） ---

function bailApply(t: BailTrack): void {
  const next = bailOf(t.seen ? 0 : Date.now() - t.unseenSince);
  if (next === t.state) return;
  t.state = next;
  const cls = bailClass(next);
  t.el.classList.remove("nova-design-bail-half", "nova-design-bail-paused");
  if (cls) t.el.classList.add(cls);
  designEvent("bail-change", { state: next }); // 噪声计（W-147）可听
}

function bailScan(): void {
  for (const t of bailTracks) bailApply(t);
}

function registerLoops(): void {
  if (typeof window === "undefined" || typeof IntersectionObserver === "undefined") return;
  const io = new IntersectionObserver(
    (entries) => {
      for (const en of entries) {
        const t = bailTracks.find((x) => x.el === en.target);
        if (!t) continue;
        t.seen = en.isIntersecting;
        if (en.isIntersecting) t.unseenSince = Date.now();
        bailApply(t); // 复庭/保释 ≤ 1 帧（同步应用）
      }
    },
    { threshold: 0.05 },
  );
  const attach = (): void => {
    for (const el of document.querySelectorAll("[data-nova-loop], .nova-loop")) {
      if (bailTracks.some((t) => t.el === el)) continue;
      const r = el.getBoundingClientRect();
      const t: BailTrack = { el, seen: r.top < window.innerHeight && r.bottom > 0, unseenSince: Date.now(), state: "full" };
      bailTracks.push(t);
      io.observe(el);
      const onHover = (): void => {
        t.seen = true;
        t.unseenSince = Date.now();
        bailApply(t);
      };
      el.addEventListener("pointerenter", onHover);
      bag.push(() => el.removeEventListener("pointerenter", onHover));
    }
  };
  attach();
  bailTimer = window.setInterval(bailScan, 1000);
  // 变更去抖：body 子树的 childList 变更可能高频爆发（右键菜单/弹层挂载、
  // 壁纸粒子层逐帧节点操作等），全量 querySelectorAll 扫描若随每次变更同步
  // 执行会形成变更→扫描→观察→再变更的风暴（实测整页冻结卡死在此处）。
  // 合并为单次 250ms 低频重扫，语义不变（新循环元素最迟 1/4 秒纳入保释管理）。
  let attachPending = 0;
  const scheduleAttach = (): void => {
    if (attachPending) return;
    attachPending = window.setTimeout(() => {
      attachPending = 0;
      attach();
    }, 250);
  };
  const mo = new MutationObserver(scheduleAttach);
  mo.observe(document.body, { childList: true, subtree: true });
  bag.push(() => {
    io.disconnect();
    mo.disconnect();
    if (attachPending) {
      clearTimeout(attachPending);
      attachPending = 0;
    }
    if (bailTimer) clearInterval(bailTimer);
    bailTimer = 0;
    for (const t of bailTracks) t.el.classList.remove("nova-design-bail-half", "nova-design-bail-paused");
    bailTracks = [];
  });
}

// --- 面板渲染（体检中心：六 tab 全真实数据 + 诚实空态） ---

function renderTabBody(): string {
  if (curTab === "score") {
    const r = motionScore(motionEntries);
    if (r.checked === 0) return `<div class="nova-design-muted">空态成绩单：暂无动效清单（等待 nova://design-motion 摄入，N-06 同源）。</div>`;
    const issues = r.issues.map((i) => `<div class="nova-design-card"><span class="nova-design-bad">✗ ${esc(i.id)}</span> <span class="nova-design-muted">[${esc(i.field)}]</span> ${esc(i.detail)}</div>`).join("");
    return `<div class="nova-design-h">动画成绩单 · ${r.grade}（${r.score} 分 / ${r.groups} 组 / ${r.checked} 条）</div>${issues || '<div class="nova-design-ok">✓ 同类时长差 ≤ 20%，曲线统一</div>'}`;
  }
  if (curTab === "balance") {
    const r = balanceReport(iconSamples);
    if (r.checked === 0) return `<div class="nova-design-muted">空态天平：暂无图标遮罩（等待 nova://design-icons 摄入）。只体检不改图标。</div>`;
    const rows = r.flagged.map((f) => `<div class="nova-design-card"><span class="nova-design-bad">✗ ${esc(f.id)}</span> 重心偏移 ${(f.offset * 100).toFixed(1)}%（fg ${(f.fgRatio * 100).toFixed(0)}%）</div>`).join("");
    return `<div class="nova-design-h">图标配重天平 · ${r.checked} 检 / ${r.flagged.length} 标红</div>${rows || '<div class="nova-design-ok">✓ 全部配重均衡</div>'}`;
  }
  if (curTab === "cursor") {
    const r = cursorAudit(cursorSamples);
    if (r.inspected.length === 0) return `<div class="nova-design-muted">空态巡检：暂无场景样本（等待 nova://design-cursor 摄入）。</div>`;
    const miss = r.missing.length > 0 ? `<div class="nova-design-muted">未巡检：${r.missing.map(esc).join(" / ")}</div>` : "";
    const jumps = r.jumps.map((j) => `<div class="nova-design-card"><span class="nova-design-bad">✗ ${esc(j.from)}→${esc(j.to)} ${esc(j.field)} Δ${j.delta}</span><br><span class="nova-design-muted">${esc(j.suggest)}</span></div>`).join("");
    return `<div class="nova-design-h">光标形影 · ${r.verdict}</div>${miss}${jumps || '<div class="nova-design-ok">✓ 五场景形影一致</div>'}`;
  }
  if (curTab === "heat") {
    const log = evictHeat(lsGet<HeatPoint[]>(`${NS}.heat.v1`, []), Date.now());
    if (log.length === 0) return `<div class="nova-design-muted">空态热图：暂无匿名点击坐标（关闭即停采；30 天滚动）。</div>`;
    const cells = heatCells(log);
    const top = cells.slice(0, 5).map((c) => `<div class="nova-design-mono">(${c.x * HEAT_BUCKET_PX},${c.y * HEAT_BUCKET_PX}) ×${c.count} L${c.level}</div>`).join("");
    return `<div class="nova-design-h">UI 物理热图 · ${log.length} 点 / ${cells.length} 桶</div>${top}<div class="nova-design-muted">仅匿名坐标 {x,y,ts}，零内容记录</div>`;
  }
  if (curTab === "density") {
    const r = densityAudit(densityPages);
    if (r.pages.length === 0) return `<div class="nova-design-muted">空态湿度计：暂无页面快照（等待 nova://design-density 摄入）。</div>`;
    const rows = r.pages.map((p) => `<div class="nova-design-card">${esc(p.id)} · 湿度 <span class="${p.verdict === "damp" ? "nova-design-bad" : "nova-design-ok"}">${p.humidity}</span>（E${p.eScore}/A${p.airScore}/C${p.cScore}）</div>`).join("");
    return `<div class="nova-design-h">密度湿度计 · ${r.pages.length} 页 / 潮湿 ${r.damp.length}</div>${rows}`;
  }
  const r = sentinelReport(contrastNodes);
  if (r.checked === 0 && r.whitelisted === 0) return `<div class="nova-design-muted">空态哨兵：暂无文本节点（等待 nova://design-contrast 摄入）。</div>`;
  const fails = r.fails.slice(0, 8).map((f) => `<div class="nova-design-card"><span class="nova-design-bad">✗ ${esc(f.id)}</span> ${f.ratio}:1 &lt; ${f.need}:1${f.large ? "（大字）" : ""}</div>`).join("");
  return `<div class="nova-design-h">对比度哨兵 · ${r.checked} 检 / ${r.fails.length} 违规 / 白名单 ${r.whitelisted}</div>${fails || '<div class="nova-design-ok">✓ 全文达 WCAG AA</div>'}`;
}

function renderPanel(): void {
  if (!panel) return;
  const tabs: Array<{ id: TabId; zh: string; wid: string }> = [
    { id: "score", zh: "动画", wid: "W-178" },
    { id: "balance", zh: "天平", wid: "W-182" },
    { id: "cursor", zh: "光标", wid: "W-183" },
    { id: "heat", zh: "热图", wid: "W-184" },
    { id: "density", zh: "密度", wid: "W-186" },
    { id: "sentinel", zh: "哨兵", wid: "W-187" },
  ];
  panel.innerHTML = `
<div class="nova-design-h">UI 度量体检中心 · AI-15</div>
<div class="nova-design-tabs">${tabs.map((t) => `<button class="nova-design-tab" role="tab" aria-selected="${curTab === t.id}" data-tab="${t.id}">${esc(t.zh)} ${t.wid}</button>`).join("")}</div>
<div class="nova-design-body">${renderTabBody()}</div>
<div class="nova-design-row" style="justify-content:space-between">
<button class="nova-design-btn" id="nova-design-export">导出报告</button>
<button class="nova-design-btn" id="nova-design-symphony">白空采样</button>
<button class="nova-design-btn" id="nova-design-museum">考古换装</button>
<button class="nova-design-btn" id="nova-design-close">收起</button>
</div>`;
  panel.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((btn) => {
    btn.addEventListener("click", () => {
      curTab = (btn.dataset.tab as TabId) ?? "score";
      renderPanel();
    });
  });
  panel.querySelector("#nova-design-close")?.addEventListener("click", () => togglePanel(false));
  panel.querySelector("#nova-design-export")?.addEventListener("click", exportAll);
  panel.querySelector("#nova-design-symphony")?.addEventListener("click", () => {
    spaceGaps = sampleViewportGaps();
    lsSet(`${NS}.space.v1`, spaceGaps);
    designEvent("space-sampled", { count: spaceGaps.length });
  });
  panel.querySelector("#nova-design-museum")?.addEventListener("click", () => {
    if (flagOn("W-179")) toggleMuseum(); // 三代换装彩蛋（10s 自动归来 / Esc 立即归来）
  });
}

function exportAll(): void {
  const report = {
    kind: "nova-design-report",
    v: 1,
    motion: motionScore(motionEntries),
    balance: balanceReport(iconSamples),
    cursor: cursorAudit(cursorSamples),
    density: densityAudit(densityPages),
    sentinel: sentinelReport(contrastNodes),
    symphony: symphonyOf(spaceGaps),
  };
  designEvent("report-export", report);
  if (typeof console !== "undefined") console.info("[nova-design] 报告导出（M-82 CI 由 S17 接线）", JSON.stringify(report));
}

function togglePanel(force?: boolean): void {
  if (typeof document === "undefined") return;
  const show = force ?? panel == null;
  if (show && !panel) {
    panel = document.createElement("div");
    panel.className = "nova-design-root";
    document.body.appendChild(panel);
  }
  if (panel) panel.hidden = !show;
  if (show) renderPanel();
}

// --- W-181 字体微唱机面板 ---

function renderTurntable(): void {
  if (!turntable) return;
  const css = typographyCss(knobs);
  const presets = lsGet<TypePreset[]>(`${NS}.turntable.v1`, []);
  const specimens: Record<string, string> = {
    news: "新星计划 NOVA-200 —— 十六路并行，两百项功能，一座桌面生态的度量衡。",
    list: "W-176 黄金比实验室\nW-177 白空交响\nW-181 字体微唱机\nW-187 对比度哨兵",
    settings: "字重 / 字距 / 行高三旋钮实时旋调；满意后压成唱片，保存为命名预设。",
  };
  turntable.innerHTML = `
<div class="nova-design-h">字体微唱机 · W-181（V-76 字体选择之外的三轴微参）</div>
<div class="nova-design-spec" id="nova-design-spec" style="font-weight:${css.fontWeight};letter-spacing:${css.letterSpacing};line-height:${css.lineHeight}">${esc(specimens.news ?? "")}</div>
<div class="nova-design-row"><label style="width:44px" class="nova-design-muted">场景</label>${SPECIMEN_SCENES.map((s) => `<button class="nova-design-tab" data-scene="${s.key}" aria-selected="${s.key === "news"}">${esc(s.zh)}</button>`).join("")}</div>
<div class="nova-design-row"><span style="width:64px" class="nova-design-muted">字重 ${knobs.weight}</span><input class="nova-design-knob" type="range" min="${TYPE_KNOB_DEF.weight.min}" max="${TYPE_KNOB_DEF.weight.max}" step="${TYPE_KNOB_DEF.weight.step}" value="${knobs.weight}" data-knob="weight"></div>
<div class="nova-design-row"><span style="width:64px" class="nova-design-muted">字距 ${knobs.tracking}</span><input class="nova-design-knob" type="range" min="${TYPE_KNOB_DEF.tracking.min}" max="${TYPE_KNOB_DEF.tracking.max}" step="${TYPE_KNOB_DEF.tracking.step}" value="${knobs.tracking}" data-knob="tracking"></div>
<div class="nova-design-row"><span style="width:64px" class="nova-design-muted">行高 ${knobs.lineHeight.toFixed(2)}</span><input class="nova-design-knob" type="range" min="${TYPE_KNOB_DEF.lineHeight.min}" max="${TYPE_KNOB_DEF.lineHeight.max}" step="${TYPE_KNOB_DEF.lineHeight.step}" value="${knobs.lineHeight}" data-knob="lineHeight"></div>
<div class="nova-design-row" style="gap:4px"><input class="nova-design-ta" id="nova-design-preset-name" placeholder="预设名（≤24 字）"><button class="nova-design-btn" id="nova-design-press">压盘</button></div>
<div class="nova-design-body">${presets.map((p) => `<div class="nova-design-row" style="justify-content:space-between"><span class="nova-design-mono">${esc(p.name)} · ${p.knobs.weight}/${p.knobs.tracking}/${p.knobs.lineHeight}</span><button class="nova-design-btn" data-apply="${esc(p.name)}">应用</button></div>`).join("") || '<span class="nova-design-muted">暂无唱片预设（≤12 条）</span>'}</div>
<button class="nova-design-btn" id="nova-design-tt-close" style="align-self:flex-start">收起</button>`;
  const spec = turntable.querySelector<HTMLElement>("#nova-design-spec");
  turntable.querySelectorAll<HTMLButtonElement>("[data-scene]").forEach((btn) => {
    btn.addEventListener("click", () => {
      if (spec) spec.textContent = specimens[btn.dataset.scene ?? "news"] ?? "";
      turntable?.querySelectorAll("[data-scene]").forEach((b) => b.setAttribute("aria-selected", String(b === btn)));
    });
  });
  turntable.querySelectorAll<HTMLInputElement>("[data-knob]").forEach((input) => {
    input.addEventListener("input", () => {
      // 实时预览 ≤ 1 帧：input 即写 style，不经 React/重排
      knobs = normalizeKnobs({ ...knobs, [input.dataset.knob as keyof TypeKnobs]: Number(input.value) });
      lsSet(`${NS}.knobs.v1`, knobs);
      const c = typographyCss(knobs);
      if (spec) {
        spec.style.fontWeight = c.fontWeight;
        spec.style.letterSpacing = c.letterSpacing;
        spec.style.lineHeight = c.lineHeight;
      }
      const label = input.previousElementSibling;
      if (label) {
        const key = input.dataset.knob as keyof TypeKnobs;
        label.textContent = `${key === "weight" ? "字重" : key === "tracking" ? "字距" : "行高"} ${key === "lineHeight" ? knobs.lineHeight.toFixed(2) : knobs[key]}`;
      }
    });
  });
  turntable.querySelector("#nova-design-press")?.addEventListener("click", () => {
    const name = (turntable?.querySelector<HTMLInputElement>("#nova-design-preset-name")?.value ?? "").trim();
    if (!name) return;
    const next = savePreset(lsGet<TypePreset[]>(`${NS}.turntable.v1`, []), { name, knobs, savedAt: Date.now() });
    lsSet(`${NS}.turntable.v1`, next);
    designEvent("type-apply", { ...knobs, name }); // 压盘 → 派发应用（V-76/S17 落地）
    renderTurntable();
  });
  turntable.querySelectorAll<HTMLButtonElement>("[data-apply]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const p = lsGet<TypePreset[]>(`${NS}.turntable.v1`, []).find((x) => x.name === btn.dataset.apply);
      if (!p) return;
      knobs = normalizeKnobs(p.knobs);
      lsSet(`${NS}.knobs.v1`, knobs);
      designEvent("type-apply", { ...knobs, name: p.name });
      renderTurntable();
    });
  });
  turntable.querySelector("#nova-design-tt-close")?.addEventListener("click", () => toggleTurntable(false));
}

function toggleTurntable(force?: boolean): void {
  if (typeof document === "undefined") return;
  const show = force ?? turntable == null;
  if (show && !turntable) {
    turntable = document.createElement("div");
    turntable.className = "nova-design-root";
    turntable.style.left = "16px";
    turntable.style.right = "auto";
    turntable.style.bottom = "auto";
    turntable.style.top = "16px";
    document.body.appendChild(turntable);
  }
  if (turntable) turntable.hidden = !show;
  if (show) renderTurntable();
}

// --- 事件摄入（S17 喂数入口） ---

function bindIntakes(): void {
  const on = (name: string, fn: (d: unknown) => void): void => {
    const h = (e: Event): void => fn((e as CustomEvent).detail);
    window.addEventListener(`nova://design-${name}`, h);
    bag.push(() => window.removeEventListener(`nova://design-${name}`, h));
  };

  on("motion", (d) => {
    const entries = (d as { entries?: MotionEntry[] } | undefined)?.entries;
    if (Array.isArray(entries)) {
      motionEntries = entries.filter((e) => e && e.id && e.group);
      lsSet(`${NS}.motion.v1`, motionEntries);
      renderPanel();
    }
  });
  on("icons", (d) => {
    const icons = (d as { icons?: IconSample[] } | undefined)?.icons;
    if (Array.isArray(icons)) {
      iconSamples = icons.filter((i) => i && i.id && i.mask);
      lsSet(`${NS}.icons.v1`, iconSamples);
      renderPanel();
    }
  });
  on("cursor", (d) => {
    const scenes = (d as { scenes?: CursorSceneSample[] } | undefined)?.scenes;
    if (Array.isArray(scenes)) {
      cursorSamples = scenes.filter((s) => s && typeof s.scene === "string");
      lsSet(`${NS}.cursor.v1`, cursorSamples);
      renderPanel();
    }
  });
  on("density", (d) => {
    const pages = (d as { pages?: DensityPage[] } | undefined)?.pages;
    if (Array.isArray(pages)) {
      densityPages = pages.filter((p) => p && p.id);
      lsSet(`${NS}.density.v1`, densityPages);
      renderPanel();
    }
  });
  on("contrast", (d) => {
    const nodes = (d as { nodes?: ContrastNode[] } | undefined)?.nodes;
    if (Array.isArray(nodes)) {
      contrastNodes = nodes.filter((n) => n && n.id);
      lsSet(`${NS}.contrast.v1`, contrastNodes);
      renderPanel();
    }
  });
  on("space", (d) => {
    const gaps = (d as { gaps?: number[] } | undefined)?.gaps;
    if (Array.isArray(gaps)) {
      spaceGaps = gaps.filter((g) => typeof g === "number" && Number.isFinite(g));
      lsSet(`${NS}.space.v1`, spaceGaps);
      renderPanel();
    }
  });
  on("loop-register", () => {
    // 循环动画登记（保释官管辖范围扩展；零摄像头）
    bailScan();
  });
  on("museum", (d) => {
    const era = (d as { era?: MuseumEra | null } | undefined)?.era;
    if (era === null) setEra(null);
    else if (era && MUSEUM_ERAS.some((e) => e.key === era)) setEra(era);
  });
}

// --- S0 注册表联动 ---

function onRegistryChange(): void {
  if (typeof document === "undefined") return;
  const anyOn = DESIGN_NOVA_FEATURES.some((f) => flagOn(f.id));
  if (!anyOn) {
    togglePanel(false);
    togglePhi(false);
    toggleXray(false);
    toggleHeat(false);
    setEra(null);
  }
  if (!flagOn("W-176")) togglePhi(false);
  if (!flagOn("W-180")) toggleXray(false);
  if (!flagOn("W-184")) {
    toggleHeat(false);
  }
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateDesignNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  loadState();
  ensureStyle();

  const onOpen = (): void => togglePanel();
  window.addEventListener("nova://design-open", onOpen);
  bag.push(() => window.removeEventListener("nova://design-open", onOpen));

  // Hub overlay 直达（ai04 协议）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-philab" && flagOn("W-176")) togglePhi(true);
    if (f === "nova-symphony" && flagOn("W-177")) {
      spaceGaps = sampleViewportGaps();
      lsSet(`${NS}.space.v1`, spaceGaps);
      curTab = "sentinel";
      togglePanel(true);
    }
    if (f === "nova-xray" && flagOn("W-180")) toggleXray(true);
    if (f === "nova-turntable" && flagOn("W-181")) toggleTurntable(true);
    if (f === "nova-heat" && flagOn("W-184")) toggleHeat(true);
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // 热键：Ctrl+Alt+G 黄金比（W-176）/ Ctrl+Shift+G 网格透视（W-180）/ Esc 考古归来（W-179）
  const onKey = (e: KeyboardEvent): void => {
    if (e.ctrlKey && e.altKey && (e.key === "g" || e.key === "G") && flagOn("W-176")) {
      e.preventDefault();
      if (phiOverlay == null) renderPhi();
      else {
        phiMode = nextPhiMode(phiMode); // 三比例即时切换
        renderPhi();
      }
    } else if (e.ctrlKey && e.shiftKey && (e.key === "g" || e.key === "G") && flagOn("W-180")) {
      e.preventDefault();
      toggleXray();
    } else if (e.key === "Escape" && document.documentElement.hasAttribute("data-nova-era")) {
      setEra(null); // Esc 立即归来
    }
  };
  window.addEventListener("keydown", onKey);
  bag.push(() => window.removeEventListener("keydown", onKey));

  // W-184 匿名点击捕获（仅 W-184 开启时；关闭即停采）
  const onPointer = (e: PointerEvent): void => {
    if (!flagOn("W-184")) return;
    const log = recordHeat(evictHeat(lsGet<HeatPoint[]>(`${NS}.heat.v1`, []), Date.now()), e.clientX, e.clientY, Date.now());
    lsSet(`${NS}.heat.v1`, log);
  };
  window.addEventListener("pointerdown", onPointer);
  bag.push(() => window.removeEventListener("pointerdown", onPointer));

  // W-185 循环动画保释（零摄像头：IntersectionObserver + 悬停代理）
  if (flagOn("W-185")) registerLoops();

  bindIntakes();

  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      bag.push(mod.subscribeNova(onRegistryChange));
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}

export function deactivateDesignNova(): void {
  if (!active) return;
  active = false;
  for (const fn of bag) {
    try {
      fn();
    } catch {
      /* 卸载容错 */
    }
  }
  bag = [];
  panel?.remove();
  panel = null;
  turntable?.remove();
  turntable = null;
  phiOverlay?.remove();
  phiOverlay = null;
  xrayOverlay?.remove();
  xrayOverlay = null;
  heatOverlay?.remove();
  heatOverlay = null;
  if (eraTimer) {
    clearTimeout(eraTimer);
    eraTimer = 0;
  }
  document.documentElement.removeAttribute("data-nova-era");
  document.querySelectorAll(".nova-design-root,.nova-design-overlay").forEach((n) => n.remove());
}

export function isDesignNovaActive(): boolean {
  return active;
}
