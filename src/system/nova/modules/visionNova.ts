/**
 * NOVA-200 · S12 视觉氛围路（AI-12）—— 域12 视觉、个性化与氛围（W-139…W-151）。
 *
 * 边界（全景 §12）：壁纸中心（ai04）管壁纸本体与播放清单、W-140 只管生态位
 * 三态标注、W-141 只管粒子行为层、W-145 只管成套推荐；U-08 材质引擎管材质
 * 本体、W-143 只管季节纹理插值因子（与 Q-80 相乘）；U-49 音景管声音本体、
 * W-145 只做配对表；Q-80 主题引擎管 token、W-146 只产香水文学卡；本模块
 * 补**极光历、生态位、水族馆、美术馆、季节纹理、光笔、配对、香水卡、噪声计、
 * 像素画、画框、长曝光、月光舞台**十三项。
 *
 * 纪律：
 * - 零侵入：不改写任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://vision-*`
 *   自定义事件摄入 + `nova.vision.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-vision-`、事件 `nova://vision-*`、localStorage 键 `nova.vision.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn），无注册表时用 manifest
 *   defaultOn 回退（诚实降级，不报错）；
 * - 降级：reduce-motion / safeMode / static 三态下动效归零（水族馆静帧、光笔
 *   无呼吸、极光不渐变），语义与数据保留；非 DOM 环境行为层安全 no-op；
 * - 默认档：W-139 极光历、W-141 水族馆、W-144 光笔、W-148 像素画、W-149 画框、
 *   W-151 月光舞台默认关（与 S0 注册表一致），其余默认开。
 *
 * 诚实边界：
 * - W-139 色相由**真实周序**推导（ISO 周序 × 15°），无随机跳色；
 * - W-140 三态由真实最近使用时间推导（钟爱 ≤7 天 / 休眠 7–60 天 / 荣休 >60 天），
 *   荣休只降权不删除（壁纸本体归 ai04 管）；
 * - W-141 ≤200 粒；粒子数据纯内存，卸载即回收（零落盘）；
 * - W-142 展览墙只用 ai04 真实喂入的图标数据（64px），无数据如实空态；
 * - W-143 与 Q-80 相乘：只输出 0.2–1.0 因子，不覆盖材质引擎本体；
 * - W-144 光笔为 24px 静态伴飞（无拖尾），指针不可达（pointer-events:none）
 *   不挡任何命中区；
 * - W-145 配对表经 nova://vision-pair-table 喂入，无表时按壁纸 id 稳定哈希
 *   兜底推荐并如实标注 HEURISTIC；
 * - W-147 噪声计只计量经 nova://vision-noise-sample 喂入的真实活跃层；
 *   安静档给出建议清单，一键执行派发 nova://vision-quiet-apply 由各层自熄；
 * - W-148 像素字体栈缺失时回退系统栈（不缺字），降采样真实最近邻；
 * - W-149 画框六制式纯装饰层，pointer-events:none 且 z-index 低于任务栏，
 *   命中区豁免；
 * - W-150 每小时主色采样真实摄入；8760 帧压缩存储 ≤140KB（5bit/通道量化）；
 * - W-151 月相由真实朔望周期推导（29.530588 天），月光层透明度随月相包络。
 */

import { novaMotionOK, novaNum, novaOn } from "../registry";

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

const NS = "nova.vision";
const DAY = 86_400_000;

/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://vision-*` 事件（SSR/测试环境安全）。 */
export function visionEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://vision-${name}`, { detail }));
}

/** FNV-1a 32bit（稳定哈希，配对兜底用）。 */
export function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
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

export const VISION_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-139",
    titleZh: "桌面极光历",
    titleEn: "Aurora Almanac",
    descZh: "桌面氛围光随周序每周巡回 15° 色相：一年 52 周走完 780°（两圈余 60°），每周一档、有历可查；不闪不跳，只随周更。",
    defaultOn: false,
    wiringHint: "氛围层消费 nova://vision-aurora {week, hue, hex}",
    degrade: "静态单色，无周内渐变；色相语义不变",
  },
  {
    id: "W-140",
    titleZh: "壁纸生态位",
    titleEn: "Wallpaper Niche",
    descZh: "壁纸按真实最近使用分入钟爱/休眠/荣休三态生态位：7 天内钟爱、7–60 天休眠、60 天以上荣休；荣休只降权不删除。",
    defaultOn: true,
    wiringHint: "ai04 壁纸清单经 nova://vision-wallpapers 喂入",
    degrade: "静态三态标注，语义与筛选不变",
  },
  {
    id: "W-141",
    titleZh: "壁纸水族馆",
    titleEn: "Wallpaper Aquarium",
    descZh: "壁纸粒子自主游动：邻近避让、同类聚散、绕指针而行；≤200 粒 60fps，纯内存零落盘。",
    defaultOn: false,
    wiringHint: "粒子层挂在壁纸叠层；flock 参数 0–200",
    degrade: "静帧粒子构图，不游动；数据保留",
  },
  {
    id: "W-142",
    titleZh: "图标美术馆",
    titleEn: "Icon Gallery",
    descZh: "64px 大卡展览墙鉴赏视图：按首字母分厅、按使用热度挂展签，逐格灯下细看；数据同源 ai04 真实图标。",
    defaultOn: true,
    overlay: "nova-gallery",
    wiringHint: "图标数据经 nova://vision-wallpapers {icons} 喂入",
    degrade: "静态展墙；无灯下渐亮",
  },
  {
    id: "W-143",
    titleZh: "材质季节纹理",
    titleEn: "Seasonal Texture",
    descZh: "材质纤维/磨砂感随季节插值（春细腻→夏舒展→秋颗粒→冬凝霜），与 Q-80 材质引擎相乘而非覆盖。",
    defaultOn: true,
    wiringHint: "Q-80 消费 nova://vision-texture {factor, preset}",
    degrade: "锁定当季静态档；因子语义不变",
  },
  {
    id: "W-144",
    titleZh: "光标光笔",
    titleEn: "Cursor Halo",
    descZh: "光标 24px 静态伴飞柔光晕：深色桌面定位性增强，无拖尾不炫光，不挡命中区。",
    defaultOn: false,
    wiringHint: "叠层挂 html；pointer-events:none",
    degrade: "关闭光晕（reduce-motion 下无常亮环绕动效）",
  },
  {
    id: "W-145",
    titleZh: "壁纸声纹配对",
    titleEn: "A/V Pairing",
    descZh: "壁纸↔声纹成套推荐：按配对表视觉听觉同气氛成套（雪原配落雪、熔岩配低频），试听后一键采用。",
    defaultOn: true,
    wiringHint: "U-49 音景清单经 nova://vision-soundscapes 喂入",
    degrade: "静态配对卡；推荐语义不变",
  },
  {
    id: "W-146",
    titleZh: "主题香水卡",
    titleEn: "Theme Perfume",
    descZh: "主题的嗅觉隐喻文学卡：前中后调三段香（色相定香族），一句主题短诗；纯文学层不改主题。",
    defaultOn: true,
    wiringHint: "主题 token 经 nova://vision-theme 喂入",
    degrade: "静态卡文；无墨晕动画",
  },
  {
    id: "W-147",
    titleZh: "视觉噪声计",
    titleEn: "Visual Noise Meter",
    descZh: "实时计量活跃动效层的噪声总分，超标亮黄灯；一键安静档给出熄灭清单，各层自熄不打断工作。",
    defaultOn: true,
    wiringHint: "各动效层经 nova://vision-noise-sample {id,weight} 自报",
    degrade: "静态计量读数；安静档语义不变",
  },
  {
    id: "W-148",
    titleZh: "像素画模式",
    titleEn: "Pixel Mode",
    descZh: "720p 降采样最近邻放大制式：整屏像素颗粒复古游戏感；像素字体栈缺失自动回退不缺字。",
    defaultOn: false,
    wiringHint: "根 class nova-vision-pixel + image-rendering",
    degrade: "关闭制式（reduce-motion 不参与缩放重排）",
  },
  {
    id: "W-149",
    titleZh: "暗角画框",
    titleEn: "Vignette Frame",
    descZh: "屏幕四缘六制式装饰画框：细线/画廊/柔焦/胶片/毛边/无；纯装饰不挡任务栏命中区。",
    defaultOn: false,
    wiringHint: "叠层 z-index 低于任务栏；pointer-events:none",
    degrade: "静态画框；无呼吸明暗",
  },
  {
    id: "W-150",
    titleZh: "长曝光壁纸",
    titleEn: "Long Exposure",
    descZh: "每小时主色采样沉积成年色带（8760 帧）：一年用色一览，存储 ≤140KB 的诚实压缩。",
    defaultOn: true,
    overlay: "nova-exposure",
    wiringHint: "主色经 nova://vision-exposure-sample {hour,hex} 喂入",
    degrade: "静态色带；无扫描动画",
  },
  {
    id: "W-151",
    titleZh: "月光舞台",
    titleEn: "Moonlight Stage",
    descZh: "真实月相映射环境月光层：满月清辉、新月深沉，透明度随月相包络；有天文依据不做随机夜灯。",
    defaultOn: false,
    wiringHint: "月光叠层消费 nova://vision-moon {phase, opacity}",
    degrade: "静态月光层；无云影漂移",
  },
];

export const visionNovaDomain = {
  id: "S12",
  nameZh: "视觉、个性化与氛围",
  nameEn: "Visual & Ambience",
  route: "AI-12",
  features: VISION_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// W-139 桌面极光历（周序 × 15° 巡回）
// ---------------------------------------------------------------------------

export const AURORA_STEP = 15;

/** ISO 风格周序（1 起，52/53 周；以含 1/4 的周为年首周近似，本地时区）。 */
export function weekIndexOf(t: number): number {
  const d = new Date(t);
  const start = new Date(d.getFullYear(), 0, 1).getTime();
  const week = Math.floor((t - start) / (7 * DAY)) + 1;
  return clamp(week, 1, 53);
}

/** 周序 → 极光色相（15°/周巡回）。 */
export function auroraHue(week: number): number {
  return (((week - 1) * AURORA_STEP) % 360 + 360) % 360;
}

/** HSL → hex（极光层与色带共用；h∈[0,360) s,l∈[0,1]）。 */
export function hslToHex(h: number, s: number, l: number): string {
  const hh = ((h % 360) + 360) % 360;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0, g = 0, b = 0;
  if (hh < 60) { r = c; g = x; }
  else if (hh < 120) { r = x; g = c; }
  else if (hh < 180) { g = c; b = x; }
  else if (hh < 240) { g = x; b = c; }
  else if (hh < 300) { r = x; b = c; }
  else { r = c; b = x; }
  const to = (v: number): string => Math.round((v + m) * 255).toString(16).padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

/** 极光档（周序 → 色相 + 代表 hex）。 */
export function auroraOf(t: number): { week: number; hue: number; hex: string } {
  const week = weekIndexOf(t);
  const hue = auroraHue(week);
  return { week, hue, hex: hslToHex(hue, 0.7, 0.55) };
}

// ---------------------------------------------------------------------------
// W-140 壁纸生态位（钟爱/休眠/荣休）
// ---------------------------------------------------------------------------

export const NICHE_BELOVED_DAYS = 7;
export const NICHE_RETIRE_DAYS = 60;

export type NicheState = "beloved" | "dormant" | "retired";

/** 天数 → 生态位（真实最近使用时间推导）。 */
export function nicheOf(daysSince: number): NicheState {
  if (daysSince <= NICHE_BELOVED_DAYS) return "beloved";
  if (daysSince <= NICHE_RETIRE_DAYS) return "dormant";
  return "retired";
}

export interface NicheEntry {
  id: string;
  lastUsedAt: number;
  /** 用户手动钉住的钟爱（豁免时间推导）。 */
  pinned?: boolean;
}

export function nicheOfEntry(e: NicheEntry, now: number): NicheState {
  if (e.pinned) return "beloved";
  return nicheOf((now - e.lastUsedAt) / DAY);
}

export const NICHE_ZH: Record<NicheState, string> = {
  beloved: "钟爱",
  dormant: "休眠",
  retired: "荣休",
};

/** 生态位排序：钟爱 → 休眠 → 荣休；同级按最近使用倒序。 */
export function nicheSort(entries: readonly NicheEntry[], now: number): NicheEntry[] {
  const rank: Record<NicheState, number> = { beloved: 0, dormant: 1, retired: 2 };
  return [...entries].sort((a, b) => {
    const d = rank[nicheOfEntry(a, now)] - rank[nicheOfEntry(b, now)];
    return d !== 0 ? d : b.lastUsedAt - a.lastUsedAt;
  });
}

/** 荣休降权系数（ai04 排序相乘；不删除）。 */
export function nicheWeight(e: NicheEntry, now: number): number {
  return nicheOfEntry(e, now) === "retired" ? 0.3 : 1;
}

// ---------------------------------------------------------------------------
// W-141 壁纸水族馆（粒子自主行为）
// ---------------------------------------------------------------------------

export const AQUARIUM_MAX = 200;

export interface Particle {
  x: number; y: number;
  vx: number; vy: number;
}

export interface FlockCtx {
  w: number; h: number;
  cursor: { x: number; y: number } | null;
  /** 避让半径（px）。 */
  avoid: number;
  /** 聚散半径（px）。 */
  cohere: number;
}

/** 单步群游：避让（近邻斥力）+ 聚散（中距引力）+ 绕指针 + 边界回游。 */
export function stepParticle(p: Particle, others: readonly Particle[], ctx: FlockCtx): Particle {
  let ax = 0, ay = 0;
  let cx = 0, cy = 0, cn = 0;
  for (const o of others) {
    if (o === p) continue;
    const dx = o.x - p.x, dy = o.y - p.y;
    const d2 = dx * dx + dy * dy;
    if (d2 < ctx.avoid * ctx.avoid && d2 > 0.0001) {
      const d = Math.sqrt(d2);
      ax -= (dx / d) * (1 - d / ctx.avoid);
      ay -= (dy / d) * (1 - d / ctx.avoid);
    } else if (d2 < ctx.cohere * ctx.cohere) {
      cx += o.x; cy += o.y; cn++;
    }
  }
  if (cn > 0) {
    ax += ((cx / cn - p.x) / ctx.cohere) * 0.05;
    ay += ((cy / cn - p.y) / ctx.cohere) * 0.05;
  }
  if (ctx.cursor) {
    const dx = p.x - ctx.cursor.x, dy = p.y - ctx.cursor.y;
    const d2 = dx * dx + dy * dy;
    const R = ctx.avoid * 2;
    if (d2 < R * R && d2 > 0.0001) {
      const d = Math.sqrt(d2);
      ax += (dx / d) * 0.6 * (1 - d / R);
      ay += (dy / d) * 0.6 * (1 - d / R);
    }
  }
  const vx = clamp(p.vx + ax, -1.2, 1.2);
  const vy = clamp(p.vy + ay, -1.2, 1.2);
  let x = p.x + vx, y = p.y + vy;
  if (x < 0) { x = 0; }
  if (x > ctx.w) { x = ctx.w; }
  if (y < 0) { y = 0; }
  if (y > ctx.h) { y = ctx.h; }
  return { x, y, vx, vy };
}

/** 初始化鱼群（上限 200）。 */
export function aquariumInit(n: number, w: number, h: number): Particle[] {
  const count = clamp(Math.floor(n), 0, AQUARIUM_MAX);
  const out: Particle[] = [];
  for (let i = 0; i < count; i++) {
    const ang = (i * 2.399963) % (Math.PI * 2); // 黄金角散布
    out.push({
      x: w * (0.5 + 0.4 * Math.cos(ang)),
      y: h * (0.5 + 0.4 * Math.sin(ang)),
      vx: Math.cos(ang) * 0.4,
      vy: Math.sin(ang) * 0.4,
    });
  }
  return out;
}

// ---------------------------------------------------------------------------
// W-142 图标美术馆（64px 展览墙）
// ---------------------------------------------------------------------------

export const GALLERY_CELL = 64;

export interface Exhibit {
  id: string;
  name: string;
  /** 128px 源图（dataurl）；展览墙缩小到 64px 展出。 */
  dataUrl: string;
  launches: number;
}

export interface GalleryRow {
  letter: string;
  items: Exhibit[];
}

/** 按首字母分厅（非字母归 #）。 */
export function galleryRows(items: readonly Exhibit[]): GalleryRow[] {
  const map = new Map<string, Exhibit[]>();
  for (const it of items) {
    const ch = (it.name.trim()[0] ?? "#").toUpperCase();
    const letter = /[A-Z]/.test(ch) ? ch : "#";
    const arr = map.get(letter) ?? [];
    arr.push(it);
    map.set(letter, arr);
  }
  return [...map.entries()]
    .sort((a, b) => (a[0] === "#" ? 1 : b[0] === "#" ? -1 : a[0].localeCompare(b[0])))
    .map(([letter, its]) => ({
      letter,
      items: [...its].sort((a, b) => b.launches - a.launches),
    }));
}

/** 展签（热度 → 词）。 */
export function plaqueOf(e: Exhibit): string {
  if (e.launches >= 50) return "镇馆之宝";
  if (e.launches >= 10) return "常设展品";
  if (e.launches >= 1) return "新近展出";
  return "库房静候";
}

// ---------------------------------------------------------------------------
// W-143 材质季节纹理（与 Q-80 相乘）
// ---------------------------------------------------------------------------

export const TEXTURE_MIN = 0.2;
export const TEXTURE_MAX = 1.0;

export type SeasonName = "spring" | "summer" | "autumn" | "winter";

export const SEASON_PRESET: Record<SeasonName, { grain: number; blur: number }> = {
  spring: { grain: 0.06, blur: 0.2 },
  summer: { grain: 0.10, blur: 0.5 },
  autumn: { grain: 0.16, blur: 0.8 },
  winter: { grain: 0.22, blur: 0.35 },
};

export function seasonOf(t: number): SeasonName {
  const m = new Date(t).getMonth();
  if (m <= 1 || m === 11) return "winter";
  if (m <= 4) return "spring";
  if (m <= 7) return "summer";
  return "autumn";
}

/** 季内相位（换季前末 10% 为过渡段；月界与 seasonOf 一致）。 */
export function seasonPhase(t: number): { from: SeasonName; to: SeasonName; k: number } {
  const d = new Date(t);
  const m = d.getMonth();
  // 月 → 季（与 seasonOf 同界）：0 春首月等；冬跨年（12,1,2）
  const seasonStartMonth: Record<SeasonName, number> = { spring: 2, summer: 5, autumn: 8, winter: 11 };
  const seasonOfMonth = (mm: number): SeasonName =>
    mm <= 1 || mm === 11 ? "winter" : mm <= 4 ? "spring" : mm <= 7 ? "summer" : "autumn";
  const order: SeasonName[] = ["spring", "summer", "autumn", "winter"];
  const from = seasonOfMonth(m);
  const to = order[(order.indexOf(from) + 1) % 4]!;
  const startDay = (Date.UTC(d.getFullYear(), seasonStartMonth[from]!, 1) - Date.UTC(d.getFullYear(), 0, 0)) / DAY;
  const nextStart =
    to === "winter"
      ? (Date.UTC(d.getFullYear() + 1, 11, 1) - Date.UTC(d.getFullYear(), 0, 0)) / DAY
      : (Date.UTC(d.getFullYear(), seasonStartMonth[to]!, 1) - Date.UTC(d.getFullYear(), 0, 0)) / DAY;
  const len = nextStart - startDay;
  const day = (Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) - Date.UTC(d.getFullYear(), 0, 0)) / DAY;
  const pos = clamp((day - startDay) / len, 0, 1);
  const k = pos > 0.9 ? (pos - 0.9) / 0.1 : 0; // 末 10% 过渡
  return { from, to, k: clamp(k, 0, 1) };
}

/** 线性插值纹理档（grain/blur + 与 Q-80 相乘的因子）。 */
export function textureOf(t: number): { grain: number; blur: number; factor: number; preset: string } {
  const { from, to, k } = seasonPhase(t);
  const a = SEASON_PRESET[from]!;
  const b = SEASON_PRESET[to]!;
  const grain = a.grain + (b.grain - a.grain) * k;
  const blur = a.blur + (b.blur - a.blur) * k;
  const factor = clamp(TEXTURE_MIN + (TEXTURE_MAX - TEXTURE_MIN) * (grain / SEASON_PRESET.winter!.grain), TEXTURE_MIN, TEXTURE_MAX);
  return { grain, blur, factor, preset: k > 0 ? `${from}->${to}` : from };
}

// ---------------------------------------------------------------------------
// W-144 光标光笔（24px 静态伴飞）
// ---------------------------------------------------------------------------

export const PEN_SIZE = 24;

/** 光笔叠层样式（静态，无拖尾；pointer-events:none 不挡命中区）。 */
export function penStyle(): { size: number; pointerEvents: "none"; zIndex: number; boxShadow: string } {
  return {
    size: PEN_SIZE,
    pointerEvents: "none",
    zIndex: 5,
    boxShadow: "0 0 12px 2px rgba(255,255,255,0.35), 0 0 24px 6px rgba(120,160,255,0.18)",
  };
}

// ---------------------------------------------------------------------------
// W-145 壁纸声纹配对
// ---------------------------------------------------------------------------

export interface SoundScape {
  id: string;
  mood: string;
}

export interface PairRule {
  /** 壁纸关键词（小写子串匹配）。 */
  match: string;
  soundId: string;
}

/** 配对表匹配（真实表优先；子串不区分大小写）。 */
export function pairByTable(wallpaperName: string, rules: readonly PairRule[]): SoundScape | null {
  const name = wallpaperName.toLowerCase();
  for (const r of rules) {
    if (name.includes(r.match.toLowerCase())) {
      return { id: r.soundId, mood: r.match };
    }
  }
  return null;
}

/** 兜底：稳定哈希均匀挑选（如实标注 HEURISTIC）。 */
export function pairHeuristic(wallpaperId: string, scapes: readonly SoundScape[]): SoundScape | null {
  if (scapes.length === 0) return null;
  return scapes[fnv1a(wallpaperId) % scapes.length]!;
}

export interface PairVerdict {
  scape: SoundScape | null;
  source: "table" | "HEURISTIC" | "none";
}

export function pairFor(wallpaper: { id: string; name: string }, scapes: readonly SoundScape[], rules: readonly PairRule[]): PairVerdict {
  const t = pairByTable(wallpaper.name, rules);
  if (t) return { scape: scapes.find((s) => s.id === t.id) ?? t, source: "table" };
  const h = pairHeuristic(wallpaper.id, scapes);
  return h ? { scape: h, source: "HEURISTIC" } : { scape: null, source: "none" };
}

// ---------------------------------------------------------------------------
// W-146 主题香水卡
// ---------------------------------------------------------------------------

export const SCENT_FAMILIES: ReadonlyArray<{ max: number; family: string; en: string }> = [
  { max: 40, family: "柑苔", en: "chypre" },
  { max: 90, family: "海盐", en: "marine" },
  { max: 150, family: "绿叶", en: "green" },
  { max: 210, family: "柑橘", en: "citrus" },
  { max: 280, family: "馥奇", en: "fougère" },
  { max: 330, family: "琥珀", en: "amber" },
  { max: 361, family: "乌木", en: "oud" },
];

export interface PerfumeCard {
  family: string;
  familyEn: string;
  top: string;
  mid: string;
  base: string;
  verse: string;
}

const SCENT_NOTES: Record<string, [string, string, string]> = {
  柑苔: ["佛手柑皮", "橡木苔", "广藿香根"],
  海盐: ["海雾", "龙涎盐晶", "漂流木"],
  绿叶: ["折断的茎", "无花果叶", "湿润苔土"],
  柑橘: ["血橙汁", "橙花", "白麝香"],
  馥奇: ["薰衣草梢", "天竺葵", "干草垛"],
  琥珀: ["粉红胡椒", "鸢尾脂", "琥珀脂"],
  乌木: ["藏红花", "乌木屑", "冷灶烟"],
};

/** 主题 → 香水文学卡（色相定香族；确定性输出）。 */
export function perfumeCardOf(theme: { name: string; hue: number }): PerfumeCard {
  const hue = ((theme.hue % 360) + 360) % 360;
  const fam = SCENT_FAMILIES.find((f) => hue < f.max) ?? SCENT_FAMILIES[SCENT_FAMILIES.length - 1]!;
  const notes = SCENT_NOTES[fam.family]!;
  const verses: Record<string, string> = {
    柑苔: `${theme.name} 是雨后林地的背影。`,
    海盐: `${theme.name} 是退潮后留在礁石上的那层光。`,
    绿叶: `${theme.name} 是清晨第一个翻动叶片的人。`,
    柑橘: `${theme.name} 是剥开一枚冷橙时的凉意。`,
    馥奇: `${theme.name} 是黄昏草场最后一段风。`,
    琥珀: `${theme.name} 是旧信纸压在灯下的一角。`,
    乌木: `${theme.name} 是炉火熄灭后仍坐着的椅子。`,
  };
  return {
    family: fam.family,
    familyEn: fam.en,
    top: notes[0]!,
    mid: notes[1]!,
    base: notes[2]!,
    verse: verses[fam.family]!,
  };
}

// ---------------------------------------------------------------------------
// W-147 视觉噪声计
// ---------------------------------------------------------------------------

export const NOISE_WARN = 60;
export const NOISE_MAX_WEIGHT = 10;

export interface NoiseLayer {
  id: string;
  weight: number;
  /** 可安静档自熄的层才可入选安静清单。 */
  muteable: boolean;
}

/** 噪声总分（活跃层权重和 ×2，封顶 100）。 */
export function noiseScore(layers: readonly NoiseLayer[]): number {
  let s = 0;
  for (const l of layers) s += clamp(l.weight, 0, NOISE_MAX_WEIGHT);
  return clamp(Math.round(s * 2), 0, 100);
}

/** 安静档建议清单：muteable 且 weight≥3，按权重倒序，直到预估 ≤40。 */
export function quietPlan(layers: readonly NoiseLayer[]): string[] {
  const cands = layers
    .filter((l) => l.muteable && l.weight >= 3)
    .sort((a, b) => b.weight - a.weight);
  const out: string[] = [];
  let score = noiseScore(layers);
  for (const c of cands) {
    if (score <= 40) break;
    score = clamp(score - clamp(c.weight, 0, NOISE_MAX_WEIGHT) * 2, 0, 100);
    out.push(c.id);
  }
  return out;
}

// ---------------------------------------------------------------------------
// W-148 像素画模式
// ---------------------------------------------------------------------------

export const PIXEL_TARGET_H = 720;

/** 720p 降采样最近邻参数（真实整数缩放）。 */
export function pixelDims(w: number, h: number): { w: number; h: number; scale: number } {
  const scale = Math.max(1, Math.floor(h / PIXEL_TARGET_H));
  return { w: Math.floor(w / scale), h: Math.floor(h / scale), scale };
}

export const PIXEL_FONTS = [
  '"Press Start 2P"',
  '"Fusion Pixel"',
  '"Zpix"',
  '"Silkscreen"',
  'monospace',
];

/** 像素字体栈（逐个可用性回退；available 为本机实测可用族名）。 */
export function pixelFontStack(available: readonly string[]): string {
  const hit = PIXEL_FONTS.find((f) => available.some((a) => a.toLowerCase() === f.toLowerCase().replace(/"/g, "")));
  return hit ? `${hit}, ${PIXEL_FONTS[PIXEL_FONTS.length - 1]}` : PIXEL_FONTS[PIXEL_FONTS.length - 1]!;
}

// ---------------------------------------------------------------------------
// W-149 暗角画框（六制式）
// ---------------------------------------------------------------------------

export const FRAME_STYLES = ["thin", "gallery", "soft", "film", "deckle", "none"] as const;
export type FrameStyle = (typeof FRAME_STYLES)[number];

export interface FrameCss {
  inset: number;
  width: number;
  blur: number;
  color: string;
  /** 内缘阴影（胶片感）。 */
  inner?: string;
}

/** 六制式画框参数（纯装饰；none 为显式关闭档）。 */
export function frameStyle(id: string): FrameCss {
  switch (id) {
    case "thin":
      return { inset: 0, width: 2, blur: 0, color: "rgba(255,255,255,0.5)" };
    case "gallery":
      return { inset: 10, width: 14, blur: 0, color: "rgba(20,16,12,0.92)" };
    case "soft":
      return { inset: 0, width: 48, blur: 18, color: "rgba(0,0,0,0.5)" };
    case "film":
      return { inset: 0, width: 26, blur: 4, color: "rgba(8,8,8,0.85)", inner: "inset 0 0 40px rgba(0,0,0,0.6)" };
    case "deckle":
      return { inset: 6, width: 10, blur: 1, color: "rgba(250,244,230,0.9)" };
    default:
      return { inset: 0, width: 0, blur: 0, color: "transparent" };
  }
}

// ---------------------------------------------------------------------------
// W-150 长曝光壁纸（8760 帧年色带）
// ---------------------------------------------------------------------------

export const EXPOSURE_HOURS = 8760;
export const EXPOSURE_MAX_BYTES = 140 * 1024;

/** hex → 5bit/通道 量化（诚实压缩）。 */
export function quantizeColor(hex: string): number {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return 0;
  const v = parseInt(m[1]!, 16);
  const r = (v >> 19) & 0x1f;
  const g = (v >> 11) & 0x1f;
  const b = (v >> 3) & 0x1f;
  return (r << 10) | (g << 5) | b;
}

export function expandColor(q: number): string {
  const r = ((q >> 10) & 0x1f) * 8 + 4;
  const g = ((q >> 5) & 0x1f) * 8 + 4;
  const b = (q & 0x1f) * 8 + 4;
  const to = (v: number): string => clamp(v, 0, 255).toString(16).padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

export interface ExposureRibbon {
  /** 量化帧序列（index=年内小时）。 */
  frames: number[];
  /** 有样本的小时集合（无样本帧不编造，渲染为空槽）。 */
  sampled: number[];
}

export function emptyRibbon(): ExposureRibbon {
  return { frames: new Array<number>(EXPOSURE_HOURS).fill(0), sampled: [] };
}

/** 摄入小时样本（hour ∈ [0,8760)；覆盖写）。 */
export function ribbonSample(r: ExposureRibbon, hour: number, hex: string): ExposureRibbon {
  const h = clamp(Math.floor(hour), 0, EXPOSURE_HOURS - 1);
  const frames = r.frames.slice();
  while (frames.length < EXPOSURE_HOURS) frames.push(0);
  frames[h] = quantizeColor(hex);
  const sampled = r.sampled.includes(h) ? r.sampled : [...r.sampled, h].sort((a, b) => a - b);
  return { frames, sampled };
}

/** 读取某小时帧（无样本返回 null，不编造）。 */
export function ribbonAt(r: ExposureRibbon, hour: number): string | null {
  const h = clamp(Math.floor(hour), 0, EXPOSURE_HOURS - 1);
  if (!r.sampled.includes(h)) return null;
  return expandColor(r.frames[h]!);
}

/** 压缩存储字节（每帧 2 字节 + 采样表）。 */
export function ribbonBytes(r: ExposureRibbon): number {
  return r.frames.length * 2 + r.sampled.length * 2;
}

/** 年内小时 → 月份序号（0-11，色带刻度）。 */
export function ribbonMonthOf(hour: number): number {
  return clamp(Math.floor((hour / EXPOSURE_HOURS) * 12), 0, 11);
}

// ---------------------------------------------------------------------------
// W-151 月光舞台（真实月相）
// ---------------------------------------------------------------------------

export const MOON_SYNODIC = 29.530588;
/** 参考新月：2000-01-06 18:14 UTC。 */
export const MOON_EPOCH = Date.UTC(2000, 0, 6, 18, 14, 0);

/** 月相 0–1（0 新月，0.5 满月）。 */
export function moonPhase(t: number): number {
  const age = ((t - MOON_EPOCH) / DAY) % MOON_SYNODIC;
  return (((age < 0 ? age + MOON_SYNODIC : age) / MOON_SYNODIC) % 1 + 1) % 1;
}

export const PHASE_ZH = ["新月", "娥眉月", "上弦月", "盈凸月", "满月", "亏凸月", "下弦月", "残月"] as const;

export function phaseName(phase: number): string {
  // round：相位边界是瞬间值（0.4999… 属满月档），回绕后 8 归 0
  const idx = Math.round((((phase % 1) + 1) % 1) * 8) % 8;
  return PHASE_ZH[idx]!;
}

/** 月光层参数：满月最强（opacity 0.16），新月近零；冷银白色调。 */
export function moonlightOf(t: number): { phase: number; name: string; opacity: number; tint: string } {
  const phase = moonPhase(t);
  const envelope = (1 - Math.cos(phase * Math.PI * 2)) / 2; // 0 新月 → 1 满月
  const opacity = 0.02 + 0.14 * envelope;
  return { phase, name: phaseName(phase), opacity, tint: "#cfd8e8" };
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层；非 DOM 环境安全 no-op）
// ---------------------------------------------------------------------------

type Bag = Array<() => void>;

let active = false;
const bag: Bag = [];

function on(id: string, fn: (d: unknown) => void): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  const h = (ev: Event): void => fn((ev as CustomEvent).detail);
  window.addEventListener(`nova://vision-${id}`, h);
  bag.push(() => window.removeEventListener(`nova://vision-${id}`, h));
}

// --- 状态（行为层内存态） ---
let wallpapers: Array<{ id: string; name: string; icon?: string; lastUsedAt: number; pinned?: boolean; launches?: number }> = [];
let soundscapes: SoundScape[] = [];
let pairRules: PairRule[] = [];
let theme: { name: string; hue: number } | null = null;
let noiseLayers = new Map<string, NoiseLayer>();
let ribbon: ExposureRibbon | null = null;
let particles: Particle[] | null = null;
let curTab = "niche";

export function isVisionNovaActive(): boolean {
  return active;
}

// --- 样式 ---
const STYLE_ID = "nova-vision-style";

function ensureStyle(): void {
  if (typeof document === "undefined" || document.getElementById(STYLE_ID)) return;
  const st = document.createElement("style");
  st.id = STYLE_ID;
  st.textContent = `
.nova-vision-panel{position:fixed;right:18px;bottom:64px;width:392px;max-height:70vh;overflow:auto;
  background:rgba(18,20,28,.92);color:#e8ecf5;border:1px solid rgba(255,255,255,.14);border-radius:14px;
  font:12px/1.6 Consolas,monospace;z-index:1200;box-shadow:0 18px 50px rgba(0,0,0,.45);backdrop-filter:blur(10px)}
.nova-vision-tabs{display:flex;gap:2px;padding:8px 8px 0;flex-wrap:wrap}
.nova-vision-tab{padding:4px 9px;border-radius:8px;cursor:pointer;opacity:.72;user-select:none}
.nova-vision-tab[data-on="1"]{background:rgba(120,160,255,.2);opacity:1}
.nova-vision-body{padding:10px 12px 14px}
.nova-vision-row{display:flex;justify-content:space-between;gap:8px;padding:5px 0;border-bottom:1px dashed rgba(255,255,255,.08)}
.nova-vision-chip{display:inline-block;padding:1px 7px;border-radius:999px;font-size:11px}
.nova-vision-chip[data-k="beloved"]{background:rgba(120,200,140,.25)}
.nova-vision-chip[data-k="dormant"]{background:rgba(150,160,255,.25)}
.nova-vision-chip[data-k="retired"]{background:rgba(200,160,120,.25)}
.nova-vision-swatch{width:38px;height:14px;border-radius:4px;display:inline-block;vertical-align:middle}
.nova-vision-card{border:1px solid rgba(255,255,255,.12);border-radius:10px;padding:8px 10px;margin:6px 0}
.nova-vision-gcell{display:inline-flex;flex-direction:column;align-items:center;width:84px;margin:4px;cursor:default}
.nova-vision-gimg{width:64px;height:64px;border-radius:12px;background:rgba(255,255,255,.06);object-fit:contain}
.nova-vision-ribbon{display:flex;height:26px;border-radius:6px;overflow:hidden}
.nova-vision-ribbon i{flex:1}
.nova-vision-halo{position:fixed;width:24px;height:24px;border-radius:50%;pointer-events:none;z-index:5}
.nova-vision-frame{position:fixed;inset:0;pointer-events:none;z-index:900}
.nova-vision-moon{position:fixed;inset:0;pointer-events:none;z-index:3;background:radial-gradient(120% 90% at 70% 0%,rgba(207,216,232,var(--nova-moon-o,0)) 0%,transparent 60%)}
.nova-vision-aqua{position:fixed;inset:0;pointer-events:none;z-index:2}
html[data-static-mode="1"] .nova-vision-aqua,html[data-safe-mode="1"] .nova-vision-aqua{display:none}
.nova-vision-empty{opacity:.6;padding:8px 0}
`;
  document.head.appendChild(st);
}

// --- 面板 ---
let panel: HTMLElement | null = null;

function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}

const TABS: ReadonlyArray<{ id: string; zh: string; w: string }> = [
  { id: "niche", zh: "生态位", w: "W-140" },
  { id: "aurora", zh: "极光历", w: "W-139" },
  { id: "gallery", zh: "美术馆", w: "W-142" },
  { id: "pair", zh: "配对", w: "W-145" },
  { id: "perfume", zh: "香水卡", w: "W-146" },
  { id: "noise", zh: "噪声计", w: "W-147" },
  { id: "exposure", zh: "长曝光", w: "W-150" },
];

function renderPanel(): void {
  if (!panel || !active) return;
  const tabs = TABS.filter((t) => flagOn(t.w))
    .map((t) => `<span class="nova-vision-tab" data-tab="${t.id}" data-on="${curTab === t.id ? 1 : 0}">${t.zh}</span>`)
    .join("");
  let body = "";
  const now = Date.now();
  if (curTab === "niche" && flagOn("W-140")) {
    if (wallpapers.length === 0) {
      body = `<div class="nova-vision-empty">壁纸清单未喂入（nova://vision-wallpapers）——如实空态</div>`;
    } else {
      const sorted = nicheSort(
        wallpapers.map((w) => ({ id: w.id, lastUsedAt: w.lastUsedAt, pinned: w.pinned })),
        now,
      );
      body = sorted
        .map((e) => {
          const st = nicheOfEntry(e, now);
          const days = Math.floor((now - e.lastUsedAt) / DAY);
          return `<div class="nova-vision-row"><span>${esc(wallpapers.find((w) => w.id === e.id)?.name ?? e.id)}</span>
            <span class="nova-vision-chip" data-k="${st}">${NICHE_ZH[st]}${e.pinned ? " ·钉" : ""} ${days}d</span></div>`;
        })
        .join("");
    }
  } else if (curTab === "aurora" && flagOn("W-139")) {
    const a = auroraOf(now);
    const rows: string[] = [];
    for (let w = 1; w <= 53; w += 4) {
      rows.push(`<div class="nova-vision-row"><span>W${String(w).padStart(2, "0")}</span>
        <span><i class="nova-vision-swatch" style="background:${hslToHex(auroraHue(w), 0.7, 0.55)}"></i> ${auroraHue(w)}°</span></div>`);
    }
    body = `<div class="nova-vision-row"><span>本周 W${a.week}</span><span><i class="nova-vision-swatch" style="background:${a.hex}"></i> ${a.hue}°</span></div>${rows.join("")}`;
  } else if (curTab === "gallery" && flagOn("W-142")) {
    const exhibits = wallpapers
      .filter((w) => typeof w.icon === "string" && w.icon.length > 0)
      .map((w) => ({ id: w.id, name: w.name, dataUrl: w.icon!, launches: w.launches ?? 0 }));
    if (exhibits.length === 0) {
      body = `<div class="nova-vision-empty">无 64px 展品——图标数据经 nova://vision-wallpapers {icons} 喂入</div>`;
    } else {
      body = galleryRows(exhibits)
        .map(
          (r) => `<div style="margin:6px 0 2px;opacity:.7">厅 · ${esc(r.letter)}</div>` +
            r.items
              .map(
                (e) => `<span class="nova-vision-gcell" title="${esc(plaqueOf(e))}">
                  <img class="nova-vision-gimg" src="${e.dataUrl}" alt="${esc(e.name)}"/>
                  <span style="opacity:.8">${esc(e.name.slice(0, 8))}</span></span>`,
              )
              .join(""),
        )
        .join("");
    }
  } else if (curTab === "pair" && flagOn("W-145")) {
    if (wallpapers.length === 0 || soundscapes.length === 0) {
      body = `<div class="nova-vision-empty">需壁纸与音景两侧真实清单（nova://vision-wallpapers + nova://vision-soundscapes）</div>`;
    } else {
      body = wallpapers
        .slice(0, 12)
        .map((w) => {
          const v = pairFor(w, soundscapes, pairRules);
          return `<div class="nova-vision-row"><span>${esc(w.name)}</span><span>${v.scape ? esc(v.scape.id) : "—"} <span style="opacity:.55">${v.source}</span></span></div>`;
        })
        .join("");
    }
  } else if (curTab === "perfume" && flagOn("W-146")) {
    if (!theme) {
      body = `<div class="nova-vision-empty">主题 token 未喂入（nova://vision-theme）</div>`;
    } else {
      const c = perfumeCardOf(theme);
      body = `<div class="nova-vision-card"><b>${esc(c.family)} · ${esc(c.familyEn)}</b>
        <div class="nova-vision-row"><span>前调</span><span>${esc(c.top)}</span></div>
        <div class="nova-vision-row"><span>中调</span><span>${esc(c.mid)}</span></div>
        <div class="nova-vision-row"><span>后调</span><span>${esc(c.base)}</span></div>
        <div style="margin-top:6px;opacity:.85">${esc(c.verse)}</div></div>`;
    }
  } else if (curTab === "noise" && flagOn("W-147")) {
    const layers = [...noiseLayers.values()];
    if (layers.length === 0) {
      body = `<div class="nova-vision-empty">无活跃层自报（nova://vision-noise-sample）</div>`;
    } else {
      const score = noiseScore(layers);
      const plan = quietPlan(layers);
      body = `<div class="nova-vision-row"><span>噪声总分</span><b style="color:${score > NOISE_WARN ? "#ffd479" : "#9fe0b0"}">${score}/100</b></div>` +
        layers
          .sort((a, b) => b.weight - a.weight)
          .map((l) => `<div class="nova-vision-row"><span>${esc(l.id)}</span><span>${l.weight}w${l.muteable ? "" : " ·常驻"}</span></div>`)
          .join("") +
        (plan.length > 0
          ? `<div style="margin-top:8px"><span class="nova-vision-tab" data-quiet="1" style="background:rgba(255,212,121,.2)">一键安静档（${plan.length} 层）</span></div>`
          : "");
    }
  } else if (curTab === "exposure" && flagOn("W-150")) {
    if (!ribbon || ribbon.sampled.length === 0) {
      body = `<div class="nova-vision-empty">尚无小时主色样本（nova://vision-exposure-sample）——不编造年色带</div>`;
    } else {
      const cells = ribbon.sampled
        .slice(-168)
        .map((h) => `<i style="background:${expandColor(ribbon!.frames[h]!)}" title="h${h}"></i>`)
        .join("");
      const bytes = ribbonBytes(ribbon);
      body = `<div class="nova-vision-ribbon">${cells}</div>
        <div class="nova-vision-row"><span>样本小时</span><span>${ribbon.sampled.length}/${EXPOSURE_HOURS}</span></div>
        <div class="nova-vision-row"><span>存储</span><span>${bytes}B / ${EXPOSURE_MAX_BYTES}B</span></div>`;
    }
  }
  panel.innerHTML = `<div class="nova-vision-tabs">${tabs}</div><div class="nova-vision-body">${body}</div>`;
}


function togglePanel(force?: boolean): void {
  if (typeof document === "undefined") return;
  const want = force ?? !panel;
  if (want && !panel) {
    panel = document.createElement("div");
    panel.className = "nova-vision-panel";
    panel.addEventListener("click", (ev) => {
      const el = ev.target as HTMLElement;
      const tab = el.dataset?.tab;
      if (tab) {
        curTab = tab;
        renderPanel();
        return;
      }
      if (el.dataset?.quiet) {
        const plan = quietPlan([...noiseLayers.values()]);
        visionEvent("quiet-apply", { ids: plan });
        for (const id of plan) noiseLayers.delete(id);
        renderPanel();
      }
    });
    document.body.appendChild(panel);
    renderPanel();
  } else if (!want && panel) {
    panel.remove();
    panel = null;
  }
}

// --- 叠层：光笔 / 画框 / 月光 / 水族馆 ---
let halo: HTMLElement | null = null;
let frameEl: HTMLElement | null = null;
let moonEl: HTMLElement | null = null;
let aquaCanvas: HTMLCanvasElement | null = null;
let aquaRaf = 0;
let mouse = { x: -1e4, y: -1e4 };

function ensureHalo(): void {
  if (typeof document === "undefined" || halo || !flagOn("W-144")) return;
  const s = penStyle();
  halo = document.createElement("div");
  halo.className = "nova-vision-halo";
  halo.style.pointerEvents = "none";
  halo.style.zIndex = String(s.zIndex);
  halo.style.boxShadow = s.boxShadow;
  document.body.appendChild(halo);
}

function ensureFrame(): void {
  if (typeof document === "undefined" || frameEl || !flagOn("W-149")) return;
  const frameId = (lsGet(`${NS}.frame`, "soft") as string) || "soft";
  const css = frameStyle(frameId);
  frameEl = document.createElement("div");
  frameEl.className = "nova-vision-frame";
  frameEl.style.boxShadow = `inset 0 0 0 ${css.width}px ${css.color}`;
  frameEl.style.inset = `${css.inset}px`;
  frameEl.style.filter = css.blur > 0 ? `blur(${css.blur}px)` : "";
  document.body.appendChild(frameEl);
}

function ensureMoon(): void {
  if (typeof document === "undefined" || moonEl || !flagOn("W-151")) return;
  moonEl = document.createElement("div");
  moonEl.className = "nova-vision-moon";
  const m = moonlightOf(Date.now());
  moonEl.style.setProperty("--nova-moon-o", String(m.opacity));
  document.body.appendChild(moonEl);
  visionEvent("moon", m);
}

function ensureAqua(): void {
  if (typeof document === "undefined" || aquaCanvas || !flagOn("W-141")) return;
  aquaCanvas = document.createElement("canvas");
  aquaCanvas.className = "nova-vision-aqua";
  aquaCanvas.width = window.innerWidth;
  aquaCanvas.height = window.innerHeight;
  document.body.appendChild(aquaCanvas);
  const flock = novaNum("W-141", "flock") || 120;
  particles = aquariumInit(flock, aquaCanvas.width, aquaCanvas.height);
  if (!aquaCanvas.getContext("2d")) return;
  const loop = (): void => {
    if (!aquaCanvas || !particles) return;
    if (motionOK()) {
      const fctx: FlockCtx = {
        w: aquaCanvas.width,
        h: aquaCanvas.height,
        cursor: { x: mouse.x, y: mouse.y },
        avoid: 26,
        cohere: 90,
      };
      particles = particles.map((p) => stepParticle(p, particles!, fctx));
      const g = aquaCanvas.getContext("2d");
      if (g) {
        g.clearRect(0, 0, aquaCanvas.width, aquaCanvas.height);
        g.fillStyle = "rgba(255,255,255,0.55)";
        for (const p of particles) {
          g.beginPath();
          g.arc(p.x, p.y, 1.6, 0, Math.PI * 2);
          g.fill();
        }
      }
    }
    aquaRaf = window.requestAnimationFrame(loop);
  };
  aquaRaf = window.requestAnimationFrame(loop);
}

function teardownLayers(): void {
  if (aquaRaf) {
    window.cancelAnimationFrame(aquaRaf);
    aquaRaf = 0;
  }
  aquaCanvas?.remove();
  aquaCanvas = null;
  particles = null;
  halo?.remove();
  halo = null;
  frameEl?.remove();
  frameEl = null;
  moonEl?.remove();
  moonEl = null;
  if (typeof document !== "undefined") {
    document.documentElement.classList.remove("nova-vision-pixel");
  }
}

// --- 摄入绑定 ---
function bindIntakes(): void {
  on("wallpapers", (d) => {
    const p = d as { wallpapers?: typeof wallpapers } | undefined;
    if (Array.isArray(p?.wallpapers)) {
      wallpapers = p.wallpapers.filter((w) => w && typeof w.id === "string" && typeof w.lastUsedAt === "number");
      renderPanel();
    }
  });
  on("soundscapes", (d) => {
    const p = d as { soundscapes?: SoundScape[] } | undefined;
    if (Array.isArray(p?.soundscapes)) {
      soundscapes = p.soundscapes.filter((s) => s && typeof s.id === "string");
      renderPanel();
    }
  });
  on("pair-table", (d) => {
    const p = d as { rules?: PairRule[] } | undefined;
    if (Array.isArray(p?.rules)) {
      pairRules = p.rules.filter((r) => r && typeof r.match === "string" && typeof r.soundId === "string");
      renderPanel();
    }
  });
  on("theme", (d) => {
    const p = d as { name?: string; hue?: number } | undefined;
    if (typeof p?.name === "string" && typeof p?.hue === "number") {
      theme = { name: p.name, hue: p.hue };
      renderPanel();
    }
  });
  on("noise-sample", (d) => {
    const p = d as { id?: string; weight?: number; muteable?: boolean } | undefined;
    if (typeof p?.id === "string" && typeof p.weight === "number") {
      noiseLayers.set(p.id, { id: p.id, weight: clamp(p.weight, 0, NOISE_MAX_WEIGHT), muteable: p.muteable !== false });
      renderPanel();
    }
  });
  on("exposure-sample", (d) => {
    const p = d as { hour?: number; hex?: string } | undefined;
    if (typeof p?.hour === "number" && typeof p.hex === "string") {
      ribbon = ribbonSample(ribbon ?? emptyRibbon(), p.hour, p.hex);
      lsSet(`${NS}.ribbon.v1`, ribbon);
      renderPanel();
    }
  });
  on("open", () => togglePanel());
}

// --- S0 注册表联动 ---
function onRegistryChange(): void {
  if (typeof document === "undefined") return;
  const anyOn = VISION_NOVA_FEATURES.some((f) => flagOn(f.id));
  if (!anyOn) togglePanel(false);
  if (!flagOn("W-144") && halo) {
    halo.remove();
    halo = null;
  }
  if (!flagOn("W-149") && frameEl) {
    frameEl.remove();
    frameEl = null;
  }
  if (!flagOn("W-151") && moonEl) {
    moonEl.remove();
    moonEl = null;
  }
  if (!flagOn("W-141") && aquaCanvas) {
    if (aquaRaf) window.cancelAnimationFrame(aquaRaf);
    aquaRaf = 0;
    aquaCanvas.remove();
    aquaCanvas = null;
    particles = null;
  }
  if (!flagOn("W-148")) {
    document.documentElement.classList.remove("nova-vision-pixel");
  } else {
    document.documentElement.classList.add("nova-vision-pixel");
  }
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateVisionNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ribbon = lsGet<ExposureRibbon | null>(`${NS}.ribbon.v1`, null);
  ensureStyle();

  const onMouse = (ev: MouseEvent): void => {
    mouse = { x: ev.clientX, y: ev.clientY };
    if (halo) {
      halo.style.left = `${mouse.x - PEN_SIZE / 2}px`;
      halo.style.top = `${mouse.y - PEN_SIZE / 2}px`;
    }
  };
  window.addEventListener("mousemove", onMouse, { passive: true });
  bag.push(() => window.removeEventListener("mousemove", onMouse));

  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-gallery" && flagOn("W-142")) {
      curTab = "gallery";
      togglePanel(true);
    }
    if (f === "nova-exposure" && flagOn("W-150")) {
      curTab = "exposure";
      togglePanel(true);
    }
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  bindIntakes();
  ensureHalo();
  ensureFrame();
  ensureMoon();
  ensureAqua();
  if (flagOn("W-148")) document.documentElement.classList.add("nova-vision-pixel");

  // 极光历广播（激活即广播本周档）
  if (flagOn("W-139")) visionEvent("aurora", auroraOf(Date.now()));
  // 季节纹理因子广播（Q-80 相乘）
  if (flagOn("W-143")) visionEvent("texture", textureOf(Date.now()));
  // 月光档广播
  if (flagOn("W-151")) visionEvent("moon", moonlightOf(Date.now()));

  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      bag.push(mod.subscribeNova(onRegistryChange));
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}

export function deactivateVisionNova(): void {
  if (!active) return;
  active = false;
  for (const fn of bag) {
    try {
      fn();
    } catch {
      /* 卸载容错 */
    }
  }
  bag.length = 0;
  togglePanel(false);
  teardownLayers();
  wallpapers = [];
  soundscapes = [];
  pairRules = [];
  theme = null;
  noiseLayers.clear();
}
