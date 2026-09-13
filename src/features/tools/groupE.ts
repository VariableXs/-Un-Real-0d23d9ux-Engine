// AURORA-10000: AI-35 批次（领域07 效率与工具中枢）逻辑核，勿删。
// 族0171 天气与出行 / 族0172 地图与位置 / 族0173 学习工具 / 族0174 家庭模式 / 族0175 工具箱合集。
// 全部纯函数/纯模型；天气/位置数据全部本地模拟表，零网络。

/* ========================= 族0171 天气与出行 ========================= */

export interface WeatherNow {
  city: string;
  tempC: number;
  condition: '晴' | '多云' | '阴' | '小雨' | '大雨' | '雪' | '雾' | '雷暴';
  humidity: number;
  windDir: string;
  windLevel: number;
  aqi: number;
  updatedAt: number;
}

/** 体感温度（简化风寒/湿热模型）。 */
export function feelsLike(tempC: number, humidity: number, windLevel: number): number {
  let t = tempC;
  if (t <= 10) t -= windLevel * 0.6; // 风寒
  if (t >= 26) t += (humidity - 50) / 12; // 湿热
  return Math.round(t * 10) / 10;
}

export interface HourlyForecast {
  hour: number;
  tempC: number;
  precipPct: number;
}

export function hourlyForecast(base: WeatherNow): HourlyForecast[] {
  return Array.from({ length: 24 }, (_, i) => {
    const hour = (new Date(base.updatedAt).getHours() + i) % 24;
    const diurnal = Math.round(4 * Math.sin(((hour - 14) / 24) * 2 * Math.PI) * 10) / 10;
    return { hour, tempC: Math.round((base.tempC + diurnal) * 10) / 10, precipPct: base.condition.includes('雨') ? 40 + (i % 4) * 10 : i % 3 === 0 ? 10 : 0 };
  });
}

export interface DailyForecast {
  date: string;
  hiC: number;
  loC: number;
  condition: WeatherNow['condition'];
}

export function sevenDayForecast(base: WeatherNow, start = new Date(base.updatedAt)): DailyForecast[] {
  const conds: WeatherNow['condition'][] = ['晴', '多云', '阴', '小雨', '多云', '晴', '阴'];
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
    return {
      date: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`,
      hiC: base.tempC + 2 + (i % 3),
      loC: base.tempC - 5 + (i % 2),
      condition: i === 0 ? base.condition : conds[i] ?? '多云',
    };
  });
}

export const FIFTEEN_DAY_RESERVED = true;

export function aqiLevel(aqi: number): { label: string; color: string; advice: string } {
  if (aqi <= 50) return { label: '优', color: '#22c55e', advice: '适合户外活动' };
  if (aqi <= 100) return { label: '良', color: '#eab308', advice: '敏感人群减少户外' };
  if (aqi <= 150) return { label: '轻度污染', color: '#f97316', advice: '建议戴口罩' };
  if (aqi <= 200) return { label: '中度污染', color: '#ef4444', advice: '减少外出' };
  return { label: '重度污染', color: '#a21caf', advice: '开启净化器' };
}

/** 降水雷达：栅格化强度。 */
export function radarCells(strengths: number[]): { cell: number; level: 0 | 1 | 2 | 3; label: string }[] {
  return strengths.map((s, cell) => {
    const level: 0 | 1 | 2 | 3 = s <= 0 ? 0 : s < 2 ? 1 : s < 8 ? 2 : 3;
    return { cell, level, label: ['无雨', '小雨', '中雨', '大雨'][level] ?? '无雨' };
  });
}

export function radarAnimationFrames(strengths: number[], frames = 4): number[][] {
  return Array.from({ length: frames }, (_, f) => strengths.map((s, i) => Math.round(Math.max(0, s + Math.sin((f / frames) * 2 * Math.PI + i) * 1.5) * 10) / 10));
}

export interface WeatherAlert {
  kind: '暴雨' | '大风' | '高温' | '寒潮' | '雷电' | '大雾';
  level: '蓝色' | '黄色' | '橙色' | '红色';
  message: string;
}

export function weatherAlerts(w: WeatherNow): WeatherAlert[] {
  const out: WeatherAlert[] = [];
  if (w.condition === '大雨' || w.condition === '雷暴') out.push({ kind: w.condition === '雷暴' ? '雷电' : '暴雨', level: '黄色', message: '请注意出行安全' });
  if (w.windLevel >= 6) out.push({ kind: '大风', level: '蓝色', message: `风力 ${w.windLevel} 级，远离广告牌` });
  if (w.tempC >= 35) out.push({ kind: '高温', level: '橙色', message: '高温橙色预警，注意防暑' });
  if (w.tempC <= 0) out.push({ kind: '寒潮', level: '蓝色', message: '注意保暖防冻' });
  if (w.condition === '雾') out.push({ kind: '大雾', level: '黄色', message: '能见度低，谨慎驾驶' });
  return out;
}

export interface LifeIndex {
  name: string;
  level: 1 | 2 | 3;
  advice: string;
}

export function lifeIndices(w: WeatherNow): LifeIndex[] {
  const fl = feelsLike(w.tempC, w.humidity, w.windLevel);
  return [
    { name: '洗车', level: w.condition.includes('雨') ? 1 : 3, advice: w.condition.includes('雨') ? '不宜洗车' : '适宜洗车' },
    { name: '穿衣', level: fl < 5 ? 1 : fl > 28 ? 3 : 2, advice: fl < 5 ? '建议羽绒服' : fl > 28 ? '短袖即可' : '外套合适' },
    { name: '运动', level: w.aqi <= 100 && !w.condition.includes('雨') ? 3 : 1, advice: w.aqi <= 100 ? '适宜运动' : '建议室内运动' },
    { name: '紫外线', level: w.condition === '晴' ? 3 : 1, advice: w.condition === '晴' ? '注意防晒' : '无需防晒' },
  ];
}

export function humidityLabel(h: number): string {
  return h < 30 ? '干燥' : h < 70 ? '舒适' : '潮湿';
}

const WIND_DIRS = ['北', '东北', '东', '东南', '南', '西南', '西', '西北'];

export function windLabel(dirDeg: number, level: number): string {
  return `${WIND_DIRS[Math.round(dirDeg / 45) % 8]}风 ${level} 级`;
}

export class CityWeatherManager {
  private cities = new Map<string, WeatherNow>();

  add(w: WeatherNow): boolean {
    if (this.cities.has(w.city)) return false;
    this.cities.set(w.city, w);
    return true;
  }

  remove(city: string): boolean {
    return this.cities.delete(city);
  }

  get(city: string): WeatherNow | undefined {
    return this.cities.get(city);
  }

  get names(): string[] {
    return [...this.cities.keys()];
  }

  hottest(): WeatherNow | undefined {
    return [...this.cities.values()].sort((a, b) => b.tempC - a.tempC)[0];
  }
}

export const WEATHER_WIDGET_LINK = { widget: 'weather-2x2', refreshMin: 30 };
export const WEATHER_LOCKSCREEN_LINK = { showTemp: true, showCondition: true };

export function severeWeatherNotify(alerts: WeatherAlert[]): string[] {
  return alerts.filter((a) => a.level === '橙色' || a.level === '红色').map((a) => `【${a.level}预警】${a.kind}：${a.message}`);
}

export const WEATHER_KNOWLEDGE = [
  ' AQI 0~50 为优，户外活动不受限',
  '体感温度综合风与湿度修正',
  '降水概率 70% 指历史同型日子 7 成下雨',
];

const SYNODIC_MONTH = 29.530588;
const KNOWN_NEW_MOON_DAYS = Date.UTC(2000, 0, 6, 18, 14) / 86400_000;

/** F04260 月相：以 2000-01-06 新月为基准的朔望月推算。 */
export function moonPhase(d: Date): string {
  const days = d.getTime() / 86400_000 - KNOWN_NEW_MOON_DAYS;
  const age = ((days % SYNODIC_MONTH) + SYNODIC_MONTH) % SYNODIC_MONTH;
  const idx = Math.floor((age / SYNODIC_MONTH) * 8 + 0.5) % 8;
  return ['新月', '娥眉月', '上弦月', '盈凸月', '满月', '亏凸月', '下弦月', '残月'][idx] ?? '新月';
}

export class WeatherCache {
  private cache = new Map<string, { data: WeatherNow; at: number }>();

  put(key: string, data: WeatherNow, now: number): void {
    this.cache.set(key, { data, at: now });
  }

  get(key: string, now: number, ttlMs = 30 * 60_000): WeatherNow | undefined {
    const c = this.cache.get(key);
    return c && now - c.at <= ttlMs ? c.data : undefined;
  }

  /** 离线：超 TTL 仍返回最后数据（标记 stale）。 */
  offlineGet(key: string): WeatherNow | undefined {
    return this.cache.get(key)?.data;
  }
}

export const WEATHER_SOURCES = ['Varix 本地模型', 'OpenData 镜像（授权后）', '用户自建源'] as const;
export const WEATHER_TIPS = ['预警通知仅限橙红级别打扰', '数据全部本地缓存，可离线查看'];

/* ========================= 族0172 地图与位置 ========================= */

export const OFFLINE_MAP_RESERVED = true;

export interface Place {
  name: string;
  lat: number;
  lon: number;
  kind: 'home' | 'work' | 'favorite' | 'poi';
  note?: string;
}

export class PlaceBook {
  private places: Place[] = [];

  add(p: Omit<Place, 'kind'> & { kind?: Place['kind'] }): Place {
    const place: Place = { ...p, kind: p.kind ?? 'favorite' };
    this.places.push(place);
    return place;
  }

  get list(): readonly Place[] {
    return this.places;
  }

  byKind(kind: Place['kind']): Place[] {
    return this.places.filter((p) => p.kind === kind);
  }

  search(q: string): Place[] {
    const k = q.toLowerCase();
    return this.places.filter((p) => p.name.toLowerCase().includes(k) || (p.note ?? '').includes(k));
  }

  remove(name: string): boolean {
    const i = this.places.findIndex((p) => p.name === name);
    if (i < 0) return false;
    this.places.splice(i, 1);
    return true;
  }
}

/** 简化地理编码表（演示用，正式接离线包）。 */
const GEO_TABLE: { name: string; lat: number; lon: number }[] = [
  { name: '上海 人民广场', lat: 31.2304, lon: 121.4737 },
  { name: '北京 天安门', lat: 39.9087, lon: 116.3975 },
  { name: '广州 塔', lat: 23.1066, lon: 113.3245 },
  { name: '深圳 福田', lat: 22.5431, lon: 114.0579 },
  { name: '杭州 西湖', lat: 30.2411, lon: 120.1494 },
];

export function geocode(address: string): { lat: number; lon: number } | undefined {
  const hit = GEO_TABLE.find((g) => address.includes(g.name.split(' ')[0] ?? ''));
  return hit ? { lat: hit.lat, lon: hit.lon } : undefined;
}

export function reverseGeocode(lat: number, lon: number): string | undefined {
  let best: { name: string; d: number } | undefined;
  for (const g of GEO_TABLE) {
    const d = (g.lat - lat) ** 2 + (g.lon - lon) ** 2;
    if (!best || d < best.d) best = { name: g.name, d };
  }
  return best && best.d < 25 ? best.name : undefined;
}

export function elevationMock(lat: number, lon: number): number {
  return Math.round(50 * Math.sin(lat * 3) + 30 * Math.cos(lon * 2) + 200);
}

export function sunriseMinutes(latDeg: number, dayOfYear: number): number {
  const decl = 23.44 * Math.sin(((2 * Math.PI) / 365) * (dayOfYear - 81));
  const cosH = -Math.tan((latDeg * Math.PI) / 180) * Math.tan((decl * Math.PI) / 180);
  const h = Math.min(12, Math.max(0, (Math.acos(Math.max(-1, Math.min(1, cosH))) * 180) / Math.PI / 15));
  return Math.round(720 - h * 60);
}

export function timezoneOf(lon: number): number {
  return Math.round(lon / 15);
}

/** 球面距离（haversine，返回 km）。 */
export function haversineKm(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const R = 6371;
  const dLat = ((b.lat - a.lat) * Math.PI) / 180;
  const dLon = ((b.lon - a.lon) * Math.PI) / 180;
  const s = Math.sin(dLat / 2) ** 2 + Math.cos((a.lat * Math.PI) / 180) * Math.cos((b.lat * Math.PI) / 180) * Math.sin(dLon / 2) ** 2;
  return Math.round(2 * R * Math.asin(Math.sqrt(s)) * 100) / 100;
}

export const TRIP_LOG_RESERVED = true;
export const LOCATION_SHARE_RESERVED = true;

export const HOMETOWN_WALLPAPER_SPEC = { projection: 'mercator', style: ['线稿', '暗色', '水彩'] };

export function localWeatherLink(place: Place): { lat: number; lon: number; sunriseMin: number } {
  const dayOfYear = Math.floor((Date.now() - new Date(new Date().getFullYear(), 0, 0).getTime()) / 86400_000);
  return { lat: place.lat, lon: place.lon, sunriseMin: sunriseMinutes(place.lat, dayOfYear) };
}

export function mapAnnotationSpec(x: number, y: number, label: string): { x: number; y: number; label: string; pin: 'drop' } {
  return { x, y, label: label.slice(0, 20), pin: 'drop' };
}

export interface GpxPoint {
  lat: number;
  lon: number;
  ele: number;
  time: string;
}

export function gpxParse(gpx: string): GpxPoint[] {
  return [...gpx.matchAll(/<trkpt lat="([-\d.]+)" lon="([-\d.]+)">\s*<ele>([-\d.]+)<\/ele>\s*<time>([^<]+)<\/time>/g)].map((m) => ({
    lat: Number(m[1]),
    lon: Number(m[2]),
    ele: Number(m[3]),
    time: m[4] ?? '',
  }));
}

export function gpxSerialize(points: GpxPoint[]): string {
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<gpx version="1.1" creator="varix">',
    ...points.map((p) => `  <trkpt lat="${p.lat}" lon="${p.lon}"><ele>${p.ele}</ele><time>${p.time}</time></trkpt>`),
    '</gpx>',
  ].join('\n');
}

export interface TripPlanDay {
  day: number;
  city: string;
  items: string[];
}

export function tripPlan(days: number, cities: string[]): TripPlanDay[] {
  return Array.from({ length: days }, (_, i) => ({
    day: i + 1,
    city: cities[Math.min(i, cities.length - 1)] ?? '未定',
    items: i === 0 ? ['出发', '入住'] : i === days - 1 ? ['返程'] : ['游览'],
  }));
}

export function travelChecklist(kind: '短期' | '长途' | '出境'): string[] {
  const base = ['身份证', '充电器', '换洗衣物'];
  if (kind === '长途') return [...base, '常用药', '颈枕'];
  if (kind === '出境') return [...base, '护照', '签证', '转换插头', '外币'];
  return base;
}

export const VISA_REMINDER_RESERVED = true;

export function tripCurrency(amount: number, rate: number): number {
  return Math.round(amount * rate * 100) / 100;
}

const LOCAL_HOLIDAYS: Record<string, string> = {
  '01-01': '元旦',
  '07-04': '独立日（美）',
  '12-25': '圣诞',
  '10-03': '统一日（德）',
};

export function localHoliday(date: string): string | undefined {
  return LOCAL_HOLIDAYS[date.slice(5)];
}

export function localFestivalHint(month: number): string | undefined {
  const table: Record<number, string> = { 1: '新年季', 4: '樱花季', 7: '暑期旺季', 10: '秋叶季', 12: '年末灯季' };
  return table[month];
}

export const LOCATION_PRIVACY = '位置数据仅本地处理，不追踪、不上报';

export class OfflinePackManager {
  private packs = new Map<string, { region: string; sizeMB: number; downloaded: boolean }>();

  download(region: string, sizeMB: number): boolean {
    if (this.packs.has(region)) return false;
    this.packs.set(region, { region, sizeMB, downloaded: true });
    return true;
  }

  remove(region: string): boolean {
    return this.packs.delete(region);
  }

  totalMB(): number {
    return [...this.packs.values()].reduce((s, p) => s + p.sizeMB, 0);
  }

  get list(): readonly string[] {
    return [...this.packs.keys()];
  }
}

export const LOCATION_TIPS = ['离线包按城市下载', 'GPX 轨迹可导入导出', '位置分享默认关闭'];

/* ========================= 族0173 学习工具 ========================= */

export interface Flashcard {
  id: string;
  deck: string;
  front: string;
  back: string;
  // SM-2 状态
  ease: number;
  intervalDays: number;
  dueAt: number;
  reps: number;
  lapses: number;
}

let cardSeq = 0;

export function createCard(deck: string, front: string, back: string, now = Date.now()): Flashcard {
  cardSeq += 1;
  return { id: `fc-${cardSeq}`, deck, front, back, ease: 2.5, intervalDays: 0, dueAt: now, reps: 0, lapses: 0 };
}

/** SM-2 间隔重复：quality 0~5。 */
export function sm2(card: Flashcard, quality: 0 | 1 | 2 | 3 | 4 | 5, now = Date.now()): Flashcard {
  let { ease, intervalDays } = card;
  if (quality < 3) {
    card.lapses += 1;
    intervalDays = 1;
    ease = Math.max(1.3, ease - 0.2);
  } else {
    ease = Math.max(1.3, ease + (0.1 - (5 - quality) * 0.08));
    intervalDays = card.reps === 0 ? 1 : card.reps === 1 ? 6 : Math.round(intervalDays * ease);
  }
  card.reps += 1;
  card.intervalDays = intervalDays;
  card.dueAt = now + intervalDays * 86400_000;
  return card;
}

export class DeckManager {
  private cards: Flashcard[] = [];

  add(card: Flashcard): this {
    this.cards.push(card);
    return this;
  }

  decks(): string[] {
    return [...new Set(this.cards.map((c) => c.deck))];
  }

  byDeck(deck: string): Flashcard[] {
    return this.cards.filter((c) => c.deck === deck);
  }

  due(now = Date.now()): Flashcard[] {
    return this.cards.filter((c) => c.dueAt <= now).sort((a, b) => a.dueAt - b.dueAt);
  }

  /** 错题本：lapses ≥ 2。 */
  mistakeBook(): Flashcard[] {
    return this.cards.filter((c) => c.lapses >= 2);
  }

  stats(): { total: number; mature: number; learning: number; avgEase: number } {
    const mature = this.cards.filter((c) => c.intervalDays >= 21).length;
    const avgEase = this.cards.length ? Math.round((this.cards.reduce((s, c) => s + c.ease, 0) / this.cards.length) * 100) / 100 : 0;
    return { total: this.cards.length, mature, learning: this.cards.length - mature, avgEase };
  }

  exportJson(): string {
    return JSON.stringify(this.cards.map(({ id, deck, front, back, ease, intervalDays }) => ({ id, deck, front, back, ease, intervalDays })));
  }
}

export class StudyStats {
  private minutesByDay = new Map<string, number>();

  add(date: string, minutes: number): this {
    this.minutesByDay.set(date, (this.minutesByDay.get(date) ?? 0) + minutes);
    return this;
  }

  total(): number {
    return [...this.minutesByDay.values()].reduce((s, m) => s + m, 0);
  }

  bestDay(): string | undefined {
    return [...this.minutesByDay.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  }
}

export function reviewReminder(due: Flashcard[], now = Date.now()): string {
  const n = due.filter((c) => c.dueAt <= now).length;
  return n > 0 ? `有 ${n} 张卡片待复习` : '暂无到期卡片';
}

export class VocabularyBook {
  private words = new Map<string, { meaning: string; box: number }>();

  add(word: string, meaning: string): boolean {
    if (this.words.has(word)) return false;
    this.words.set(word, { meaning, box: 1 });
    return true;
  }

  promote(word: string): boolean {
    const w = this.words.get(word);
    if (!w) return false;
    w.box = Math.min(5, w.box + 1);
    return true;
  }

  byBox(box: number): string[] {
    return [...this.words.entries()].filter(([, w]) => w.box === box).map(([k]) => k);
  }

  /** 联动闪卡：生词转卡片。 */
  toCards(deck: string): Flashcard[] {
    return [...this.words.entries()].map(([word, w]) => createCard(deck, word, w.meaning));
  }
}

export function pronunciationPractice(target: string, heard: string): { score: number; verdict: string } {
  const a = [...target];
  const b = [...heard];
  const same = a.filter((c, i) => b[i] === c).length;
  const score = Math.round((same / Math.max(1, a.length)) * 100);
  return { score, verdict: score >= 90 ? '发音标准' : score >= 70 ? '基本清晰' : '再试一次' };
}

export function dictationCheck(expected: string, typed: string): { correct: boolean; diffAt: number } {
  const a = [...expected];
  const b = [...typed];
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) return { correct: false, diffAt: i };
  }
  return { correct: true, diffAt: -1 };
}

export interface StudyPlanItem {
  subject: string;
  minutesPerDay: number;
  days: string[];
}

export function studyPlan(items: StudyPlanItem[]): string {
  return items.map((i) => `${i.subject}：每天 ${i.minutesPerDay} 分钟（${i.days.join('/')}）`).join('\n');
}

export interface ClassPeriod {
  name: string;
  day: number; // 0-6
  startMin: number;
  endMin: number;
}

export function timetableNow(periods: ClassPeriod[], nowMin: number, weekday: number): ClassPeriod | undefined {
  return periods.find((p) => p.day === weekday && nowMin >= p.startMin && nowMin < p.endMin);
}

export function homeworkCountdown(dueAt: number, now: number): string {
  const days = Math.ceil((dueAt - now) / 86400_000);
  return days > 0 ? `距截止还有 ${days} 天` : days === 0 ? '今天截止！' : '已截止';
}

export const FORMULA_LIBRARY = [
  { name: '勾股定理', formula: 'a² + b² = c²' },
  { name: '二次求根', formula: 'x = (-b ± √(b²-4ac)) / 2a' },
  { name: '欧拉公式', formula: 'e^(iπ) + 1 = 0' },
  { name: '圆面积', formula: 'S = πr²' },
  { name: '正态分布', formula: 'f(x) = 1/(σ√(2π)) e^(-(x-μ)²/(2σ²))' },
];

const ELEMENTS: { symbol: string; name: string; z: number; mass: number }[] = [
  { symbol: 'H', name: '氢', z: 1, mass: 1.008 },
  { symbol: 'He', name: '氦', z: 2, mass: 4.003 },
  { symbol: 'C', name: '碳', z: 6, mass: 12.011 },
  { symbol: 'N', name: '氮', z: 7, mass: 14.007 },
  { symbol: 'O', name: '氧', z: 8, mass: 15.999 },
  { symbol: 'Fe', name: '铁', z: 26, mass: 55.845 },
  { symbol: 'Cu', name: '铜', z: 29, mass: 63.546 },
  { symbol: 'Au', name: '金', z: 79, mass: 196.967 },
];

export function elementLookup(symbolOrZ: string | number): { symbol: string; name: string; z: number; mass: number } | undefined {
  if (typeof symbolOrZ === 'number') return ELEMENTS.find((e) => e.z === symbolOrZ);
  const s = symbolOrZ.toLowerCase();
  return ELEMENTS.find((e) => e.symbol.toLowerCase() === s || e.name === symbolOrZ);
}

export const ELEMENT_TABLE = ELEMENTS;
export const UNIT_CHEATSHEET = { 长度: '1 米 = 3 尺', 重量: '1 千克 = 2 斤', 温度: '°C = (°F - 32) × 5/9', 数据: '1 GB = 1024 MB' };
export const CHEAT_SHEETS = ['Git 常用命令', 'Vim 键位', '正则速查', 'SQL 速查'] as const;

export class ReadingNotes {
  private notes: { book: string; text: string; at: number }[] = [];

  add(book: string, text: string): this {
    this.notes.push({ book, text, at: Date.now() });
    return this;
  }

  byBook(book: string): string[] {
    return this.notes.filter((n) => n.book === book).map((n) => n.text);
  }

  /** 定期回顾：距上次 ≥ days 的笔记。 */
  reviewDue(days: number, now = Date.now()): { book: string; text: string }[] {
    return this.notes.filter((n) => now - n.at >= days * 86400_000).map(({ book, text }) => ({ book, text }));
  }
}

export const DOODLE_CANVAS_SPEC = { modes: ['笔', '橡皮', '形状'], pressure: true };

export function studyAchievements(stats: { total: number; streakDays: number; cards: number }): string[] {
  const out: string[] = [];
  if (stats.total >= 600) out.push('学习 10 小时');
  if (stats.streakDays >= 7) out.push('连续打卡 7 天');
  if (stats.cards >= 100) out.push('百卡斩');
  return out.length ? out : ['继续加油，达成首个成就'];
}

export const STUDY_TIPS = ['SM-2 按记忆曲线安排复习', '生词本一键转闪卡', '错题本自动收集两次答错的卡片'];

/* ========================= 族0174 家庭模式 ========================= */

export interface ChildAccount {
  name: string;
  dailyLimitMin: number;
  usedMin: number;
  bedtime: string; // HH:mm
  whitelist: string[];
  ratingLimit: 'all' | 'teen' | 'child';
}

export class FamilyMode {
  private children = new Map<string, ChildAccount>();

  addChild(name: string, opts: { dailyLimitMin?: number; bedtime?: string; ratingLimit?: ChildAccount['ratingLimit'] } = {}): ChildAccount {
    const acc: ChildAccount = {
      name,
      dailyLimitMin: opts.dailyLimitMin ?? 60,
      usedMin: 0,
      bedtime: opts.bedtime ?? '21:30',
      whitelist: [],
      ratingLimit: opts.ratingLimit ?? 'child',
    };
    this.children.set(name, acc);
    return acc;
  }

  get(name: string): ChildAccount | undefined {
    return this.children.get(name);
  }

  get names(): string[] {
    return [...this.children.keys()];
  }

  use(name: string, minutes: number): { allowed: boolean; remainMin: number } {
    const c = this.children.get(name);
    if (!c) return { allowed: false, remainMin: 0 };
    const remain = Math.max(0, c.dailyLimitMin - c.usedMin);
    const allowed = remain >= minutes;
    if (allowed) c.usedMin += minutes;
    return { allowed, remainMin: Math.max(0, c.dailyLimitMin - c.usedMin) };
  }

  setLimit(name: string, minutes: number): boolean {
    const c = this.children.get(name);
    if (!c) return false;
    c.dailyLimitMin = Math.max(0, minutes);
    return true;
  }

  whitelist(name: string, app: string): boolean {
    const c = this.children.get(name);
    if (!c) return false;
    if (!c.whitelist.includes(app)) c.whitelist.push(app);
    return true;
  }

  isAllowed(name: string, app: string): boolean {
    return this.children.get(name)?.whitelist.includes(app) ?? false;
  }

  setRating(name: string, rating: ChildAccount['ratingLimit']): boolean {
    const c = this.children.get(name);
    if (!c) return false;
    c.ratingLimit = rating;
    return true;
  }

  bedtimeLocked(name: string, nowMinutes: number): boolean {
    const c = this.children.get(name);
    if (!c) return false;
    const [hh, mm] = c.bedtime.split(':').map(Number);
    const bed = (hh ?? 21) * 60 + (mm ?? 30);
    return nowMinutes >= bed || nowMinutes < 360; // 就寝后至 6:00 锁定
  }
}

export const BREAK_REMINDER_SPEC = { everyMin: 30, restSec: 120 };
export const EYECARE_FORCE = { warmFilter: true, brightnessLimit: 0.6 };
export const INSTALL_APPROVAL = { requireParent: true, silentBlock: false };

export interface UsageDay {
  date: string;
  minutes: number;
  topApp: string;
}

export function parentReport(child: ChildAccount, days: UsageDay[]): string {
  const total = days.reduce((s, d) => s + d.minutes, 0);
  const avg = days.length ? Math.round(total / days.length) : 0;
  return [`【${child.name}】本周使用报告`, `日均 ${avg} 分钟（限额 ${child.dailyLimitMin} 分钟）`, ...days.map((d) => `· ${d.date} ${d.minutes} 分钟，最多用 ${d.topApp}`)].join('\n');
}

export const FAMILY_LIBRARY_RESERVED = true;

export const ELDER_MODE = { fontScale: 1.4, simplified: true, bigButtons: true };
export const SOS_RESERVED = true;

export function familyNumbers(contacts: string[]): { label: string; dial: string }[] {
  return contacts.slice(0, 4).map((c, i) => ({ label: `亲情号 ${i + 1}`, dial: c }));
}

export class MedicationReminder {
  private meds: { name: string; timesHHMM: string[]; taken: Set<string> }[] = [];

  add(name: string, timesHHMM: string[]): this {
    this.meds.push({ name, timesHHMM, taken: new Set() });
    return this;
  }

  due(nowHHMM: string): string[] {
    return this.meds.filter((m) => m.timesHHMM.includes(nowHHMM) && !m.taken.has(nowHHMM)).map((m) => m.name);
  }

  mark(name: string, time: string): boolean {
    const m = this.meds.find((x) => x.name === name);
    if (!m) return false;
    m.taken.add(time);
    return true;
  }
}

export class FamilyCalendar {
  private events: { date: string; who: string; title: string }[] = [];

  add(date: string, who: string, title: string): boolean {
    this.events.push({ date, who, title });
    return true;
  }

  byDate(date: string): { who: string; title: string }[] {
    return this.events.filter((e) => e.date === date).map(({ who, title }) => ({ who, title }));
  }
}

export const FAMILY_LEDGER_RESERVED = true;

export class FamilyTodos {
  private items: { id: string; text: string; who: string; done: boolean }[] = [];

  add(text: string, who: string): boolean {
    if (!text.trim() || !who) return false;
    this.items.push({ id: `ft-${this.items.length + 1}`, text, who, done: false });
    return true;
  }

  toggle(id: string): boolean {
    const it = this.items.find((x) => x.id === id);
    if (!it) return false;
    it.done = !it.done;
    return true;
  }

  byWho(who: string): { text: string; done: boolean }[] {
    return this.items.filter((i) => i.who === who).map(({ text, done }) => ({ text, done }));
  }
}

export const PHOTO_FRAME_MODE = { sources: ['家庭相册'], intervalSec: 15, kenBurns: true };
export const CHILD_LOCK_SCREEN = { pin: 'parent-only', wallpaper: 'kid-theme' };
export const GAME_TIME_LOCK = { dailyMin: 40, schoolDayMin: 0 };

export function remoteMinutes(child: ChildAccount): { usedMin: number; limitMin: number; pct: number } {
  return { usedMin: child.usedMin, limitMin: child.dailyLimitMin, pct: child.dailyLimitMin ? Math.round((child.usedMin / child.dailyLimitMin) * 100) : 0 };
}

export const EMERGENCY_PAGE = { contacts: 2, medicalNote: true, bigText: true };

export const FAMILY_LOCAL_PROMISE = '家庭数据全部保存在本机，不上传云端';

export const FAMILY_TIPS = ['儿童账户限额到点自动锁定', '安装应用需家长审批', '长辈模式一键放大全部字级'];

/* ========================= 族0175 工具箱合集 ========================= */

export const MAGNIFIER_SPEC = { levels: [1.5, 2, 3, 4], followCursor: true, invertable: true };

export const ONSCREEN_KEYBOARD_ROWS = [
  ['`', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '⌫'],
  ['Tab', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '[', ']', '\\'],
  ['Caps', 'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ';', "'", 'Enter'],
  ['Shift', 'Z', 'X', 'C', 'V', 'B', 'N', 'M', ',', '.', '/', 'Shift'],
  ['Ctrl', 'Win', 'Alt', 'Space', 'Alt', 'Ctrl'],
];

export const NARRATOR_SPEC = { voice: '云扬', rate: 1, pitch: 1, echoKeys: true };

/** 色盲滤镜：通道变换矩阵。 */
export const COLOR_BLIND_FILTERS: Record<'protanopia' | 'deuteranopia' | 'tritanopia', number[]> = {
  protanopia: [0.567, 0.433, 0, 0.558, 0.442, 0, 0, 0.242, 0.758],
  deuteranopia: [0.625, 0.375, 0, 0.7, 0.3, 0, 0, 0.3, 0.7],
  tritanopia: [0.95, 0.05, 0, 0, 0.433, 0.567, 0, 0.475, 0.525],
};

export function applyColorFilter(r: number, g: number, b: number, kind: keyof typeof COLOR_BLIND_FILTERS): [number, number, number] {
  const m = COLOR_BLIND_FILTERS[kind];
  return [
    Math.round(Math.max(0, Math.min(255, (m[0] ?? 0) * r + (m[1] ?? 0) * g + (m[2] ?? 0) * b))),
    Math.round(Math.max(0, Math.min(255, (m[3] ?? 0) * r + (m[4] ?? 0) * g + (m[5] ?? 0) * b))),
    Math.round(Math.max(0, Math.min(255, (m[6] ?? 0) * r + (m[7] ?? 0) * g + (m[8] ?? 0) * b))),
  ];
}

export const HIGH_CONTRAST_THEMES = ['黑底黄字', '白底黑字', '黑底白字', '蓝底黄字'] as const;
export const FOCUS_ASSIST = { dimBackground: true, spotlightCursor: true, autoOnFullscreen: true };

export const TOOLBOX_CLOCK = { modes: ['闹钟', '世界钟', '秒表', '倒计时', '番茄钟'] };
export const TOOLBOX_CALC_LINK = '族0155 计算与换算';

const CHARMAP_RANGES: [number, number][] = [
  [0x4e00, 0x4e0f],
  [0x0041, 0x005a],
  [0x3040, 0x304f],
];

export function charMapPage(page: number, perPage = 16): string[] {
  const start = CHARMAP_RANGES[Math.min(page, CHARMAP_RANGES.length - 1)];
  if (!start) return [];
  return Array.from({ length: perPage }, (_, i) => String.fromCodePoint(Math.min(start[1], start[0] + i)));
}

export const SCREENSHOT_MODES = ['矩形', '窗口', '全屏', '延时', '滚动长图'] as const;
export const SCANNER_RESERVED = true;
export const CAMERA_SPEC = { mirror: true, timer3s: true, grid: true };

export function screenRulerSpec(orientation: 'h' | 'v', lengthPx: number): { orientation: string; lengthPx: number; ticks: number[] } {
  const tickCount = Math.floor(lengthPx / 50);
  return { orientation: orientation === 'h' ? 'horizontal' : 'vertical', lengthPx, ticks: Array.from({ length: tickCount }, (_, i) => (i + 1) * 50) };
}

export function protractorAngle(a: { x: number; y: number }, vertex: { x: number; y: number }, b: { x: number; y: number }): number {
  const v1 = { x: a.x - vertex.x, y: a.y - vertex.y };
  const v2 = { x: b.x - vertex.x, y: b.y - vertex.y };
  const dot = v1.x * v2.x + v1.y * v2.y;
  const m1 = Math.hypot(v1.x, v1.y);
  const m2 = Math.hypot(v2.x, v2.y);
  if (m1 === 0 || m2 === 0) return 0;
  return Math.round((Math.acos(Math.max(-1, Math.min(1, dot / (m1 * m2)))) * 180) / Math.PI);
}

export function spiritLevel(pitchDeg: number, rollDeg: number): { bubbleOk: boolean; pitch: number; roll: number } {
  return { bubbleOk: Math.abs(pitchDeg) < 1 && Math.abs(rollDeg) < 1, pitch: Math.round(pitchDeg * 10) / 10, roll: Math.round(rollDeg * 10) / 10 };
}

export const COLOR_PICKER_SPEC = { sampleRadius: 1, formats: ['HEX', 'RGB', 'HSL'] };
export const COLOR_PALETTES: Record<string, string[]> = {
  Material: ['#F44336', '#E91E63', '#9C27B0', '#673AB7', '#3F51B5', '#2196F3', '#03A9F4', '#00BCD4'],
  中国传统: ['#c02c38', '#f0c239', '#2e59a7', '#5d513c', '#e7dec8', '#76899b'],
  糖果: ['#FF6B6B', '#FFD93D', '#6BCB77', '#4D96FF', '#B983FF'],
};

export class ToolboxStopwatch {
  private startedAt: number | undefined;

  start(now: number): void {
    this.startedAt = now;
  }

  split(now: number): number {
    return this.startedAt === undefined ? 0 : now - this.startedAt;
  }
}

export class ToolboxCountdown {
  private remainSec: number;

  constructor(minutes: number) {
    this.remainSec = minutes * 60;
  }

  tick(sec: number): boolean {
    this.remainSec = Math.max(0, this.remainSec - sec);
    return this.remainSec === 0;
  }

  get display(): string {
    const m = Math.floor(this.remainSec / 60);
    const s = this.remainSec % 60;
    return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  }
}

export function randomInt(min: number, max: number, count = 1): number[] {
  const lo = Math.min(min, max);
  const hi = Math.max(min, max);
  const bytes = new Uint8Array(count * 4);
  globalThis.crypto.getRandomValues(bytes);
  const dv = new DataView(bytes.buffer);
  return Array.from({ length: count }, (_, i) => lo + (dv.getUint32(i * 4) % (hi - lo + 1)));
}

export function rollDice(sides: number, count = 1): number[] {
  return randomInt(1, sides, count);
}

export function compassHeading(magX: number, magY: number): number {
  let deg = (Math.atan2(magY, magX) * 180) / Math.PI;
  if (deg < 0) deg += 360;
  return Math.round(deg);
}

export function compassLabel(deg: number): string {
  const dirs = ['北', '东北', '东', '东南', '南', '西南', '西', '西北'];
  return dirs[Math.round(deg / 45) % 8] ?? '北';
}

export const TOOLBOX_TIPS = ['放大镜跟随光标', '取色器支持 HEX/RGB/HSL', '骰子支持 4~100 面'];
