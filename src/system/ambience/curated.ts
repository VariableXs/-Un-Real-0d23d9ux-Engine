/**
 * AI-18 V-71 本地精选壁纸轮换 — 纯数据 + 纯逻辑。
 *
 * 口径（离线红线）：
 * - 精选集 10 张，全部内置 SVG 场景（零二进制、零版权、零网络）；
 *   （摄影 WebP 资产预算 <15MB，后续可平替入 assets/wallpapers/curated/）
 * - 每张附本地文案（作者署名 + 一句话故事，zh/en）；
 * - 轮换仅换壁纸不动主题（不越权）；轮换点 = 本地日界；
 * - 关闭轮换后停留当前张。
 */

export interface CuratedWallpaper {
  id: string;
  /** 展示名（zh/en）。 */
  zh: string;
  en: string;
  /** 一句话故事（zh/en）。 */
  storyZh: string;
  storyEn: string;
  /** SVG data-URI（抽象场景，OKLCH 渐变）。 */
  svg: string;
}

function svgUri(inner: string, w = 1920, h = 1080): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${inner}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export const CURATED_WALLPAPERS: readonly CuratedWallpaper[] = [
  {
    id: "aurora-drift",
    zh: "极光漂移", en: "Aurora Drift",
    storyZh: "子夜后的天穹，绿与紫在冰原上缓慢换气。", storyEn: "Past midnight, green and violet breathe over the ice.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="oklch(0.32 0.12 165)"/><stop offset=".55" stop-color="oklch(0.22 0.08 280)"/><stop offset="1" stop-color="oklch(0.12 0.03 260)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><ellipse cx="1250" cy="300" rx="720" ry="260" fill="oklch(0.75 0.16 150 / 0.28)"/><ellipse cx="700" cy="420" rx="520" ry="180" fill="oklch(0.68 0.14 300 / 0.20)"/>`),
  },
  {
    id: "dawn-ridge",
    zh: "破晓山脊", en: "Dawn Ridge",
    storyZh: "第一缕光越过山脊之前，世界是安静的。", storyEn: "Before the first light crosses the ridge, the world holds its breath.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="oklch(0.72 0.14 35)"/><stop offset=".5" stop-color="oklch(0.55 0.10 25)"/><stop offset="1" stop-color="oklch(0.28 0.05 260)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><path d="M0 760 L480 560 L820 720 L1240 480 L1920 700 L1920 1080 L0 1080 Z" fill="oklch(0.20 0.04 260 / 0.85)"/><circle cx="1560" cy="240" r="72" fill="oklch(0.88 0.09 85 / 0.9)"/>`),
  },
  {
    id: "ink-lake",
    zh: "墨湖", en: "Ink Lake",
    storyZh: "浓雾落进湖面，山成了淡墨。", storyEn: "Fog settles on the lake; mountains become pale ink.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="oklch(0.62 0.03 250)"/><stop offset="1" stop-color="oklch(0.30 0.04 255)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><path d="M0 620 L360 480 L700 640 L1080 500 L1420 660 L1920 540 L1920 760 L0 760 Z" fill="oklch(0.40 0.04 255 / 0.7)"/><rect y="760" width="1920" height="320" fill="oklch(0.26 0.03 255 / 0.9)"/>`),
  },
  {
    id: "amber-desk",
    zh: "琥珀台灯", en: "Amber Lamp",
    storyZh: "深夜书桌的一盏灯，是所有专注的起点。", storyEn: "A desk lamp at midnight is where focus begins.",
    svg: svgUri(`<defs><radialGradient id="g" cx=".5" cy=".42" r=".75"><stop offset="0" stop-color="oklch(0.80 0.13 75)"/><stop offset=".55" stop-color="oklch(0.42 0.09 60)"/><stop offset="1" stop-color="oklch(0.16 0.04 55)"/></radialGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><rect x="820" y="520" width="280" height="18" rx="9" fill="oklch(0.72 0.11 75 / 0.8)"/>`),
  },
  {
    id: "cyber-grid",
    zh: "霓虹网格", en: "Neon Grid",
    storyZh: "数字城市的地平线，路灯比星星先亮。", storyEn: "In the digital city, streetlights outshine the stars.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="oklch(0.18 0.10 285)"/><stop offset="1" stop-color="oklch(0.10 0.05 260)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><g stroke="oklch(0.75 0.18 320 / 0.35)" stroke-width="2"><line x1="0" y1="700" x2="1920" y2="700"/><line x1="0" y1="820" x2="1920" y2="820"/><line x1="0" y1="980" x2="1920" y2="980"/><line x1="300" y1="700" x2="150" y2="1080"/><line x1="900" y1="700" x2="820" y2="1080"/><line x1="1500" y1="700" x2="1560" y2="1080"/></g><circle cx="1560" cy="220" r="52" fill="oklch(0.85 0.12 320 / 0.8)"/>`),
  },
  {
    id: "moss-valley",
    zh: "苔谷", en: "Moss Valley",
    storyZh: "谷底的苔藓记得每一场雨。", storyEn: "The moss in the valley remembers every rain.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="oklch(0.55 0.10 150)"/><stop offset="1" stop-color="oklch(0.24 0.06 160)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><path d="M0 800 Q480 640 960 780 T1920 720 L1920 1080 L0 1080 Z" fill="oklch(0.30 0.07 155 / 0.9)"/>`),
  },
  {
    id: "paper-fold",
    zh: "折纸晨光", en: "Paper Fold",
    storyZh: "折痕里的光，比平整处更亮。", storyEn: "Light gathers in the folds, brighter than the plains.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="oklch(0.90 0.03 85)"/><stop offset="1" stop-color="oklch(0.72 0.06 60)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><path d="M0 0 L960 540 L0 1080 Z" fill="oklch(0.82 0.05 75 / 0.5)"/><path d="M1920 0 L960 540 L1920 1080 Z" fill="oklch(0.76 0.05 70 / 0.4)"/>`),
  },
  {
    id: "deep-current",
    zh: "深流", en: "Deep Current",
    storyZh: "海面三百米以下，洋流比时间更慢。", storyEn: "Three hundred meters down, currents move slower than time.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="oklch(0.42 0.09 230)"/><stop offset="1" stop-color="oklch(0.12 0.04 250)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><path d="M-100 300 Q600 220 1300 340 T2020 300" stroke="oklch(0.60 0.08 220 / 0.4)" stroke-width="60" fill="none"/><path d="M-100 640 Q700 560 1400 680 T2020 640" stroke="oklch(0.50 0.07 225 / 0.35)" stroke-width="44" fill="none"/>`),
  },
  {
    id: "sakura-dusk",
    zh: "暮樱", en: "Sakura Dusk",
    storyZh: "花瓣落进晚霞的那一秒，春天结束了一半。", storyEn: "The petal that falls into dusk ends half the spring.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="oklch(0.70 0.11 15)"/><stop offset="1" stop-color="oklch(0.38 0.08 340)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><g fill="oklch(0.90 0.06 350 / 0.7)"><circle cx="420" cy="300" r="14"/><circle cx="980" cy="180" r="10"/><circle cx="1500" cy="360" r="12"/><circle cx="1240" cy="520" r="8"/><circle cx="680" cy="560" r="9"/></g>`),
  },
  {
    id: "stone-garden",
    zh: "石庭", en: "Stone Garden",
    storyZh: "枯山水不需要水，留白就是波纹。", storyEn: "A dry garden needs no water; the空白 is the ripple.",
    svg: svgUri(`<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="oklch(0.78 0.03 90)"/><stop offset="1" stop-color="oklch(0.62 0.04 80)"/></linearGradient></defs><rect width="1920" height="1080" fill="url(#g)"/><g stroke="oklch(0.52 0.05 75 / 0.55)" stroke-width="10" fill="none"><path d="M0 780 Q480 720 960 780 T1920 780"/><path d="M0 880 Q480 820 960 880 T1920 880"/><path d="M0 980 Q480 920 960 980 T1920 980"/></g><ellipse cx="620" cy="560" rx="150" ry="90" fill="oklch(0.48 0.05 75)"/><ellipse cx="1340" cy="520" rx="110" ry="70" fill="oklch(0.54 0.05 78)"/>`),
  },
];

/** 自 1970-01-01 的本地天数（稳定轮换种子：每天 +1，环回取模）。 */
export function localDayNumber(d: Date): number {
  const local = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  return Math.floor(local.getTime() / 86_400_000);
}

/** 每日轮换索引（本地时钟精确到日界）。 */
export function dailyIndex(now: Date, total: number): number {
  if (total <= 0) return 0;
  return ((localDayNumber(now) % total) + total) % total;
}

/** 「换一张」：前进一格（环回；不与今日索引冲突则任意）。 */
export function nextIndex(current: number, total: number): number {
  if (total <= 0) return 0;
  return (current + 1) % total;
}
