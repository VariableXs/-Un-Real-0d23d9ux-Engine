/**
 * UNREAL-X AI-17 · 族0168 语音输入 2.0 + 族0169 手写输入 2.0 + 族0170 表情符号 2.0
 * （X04176~X04250）。
 *
 * 语音：听写会话状态机/标点口头指令/静音超时/叙事码。
 * 手写：笔画采集/轨迹归一化/候选匹配（纯几何，零 AI）。
 * 表情：面板分组/搜索/最近使用（LRU）/皮肤档。
 */

/* ============================== 族0168 语音输入 2.0 ============================== */

/** 语音输入五档。 */
export const VOICE_PROFILES = [
  { id: "push", name: "按键说话", autoStopMs: 0, punctuate: false },
  { id: "clip", name: "短句", autoStopMs: 1200, punctuate: false },
  { id: "balanced", name: "均衡", autoStopMs: 2000, punctuate: true },
  { id: "dictate", name: "听写", autoStopMs: 4000, punctuate: true },
  { id: "stream", name: "长流", autoStopMs: 0, punctuate: true },
] as const;
export type VoiceProfileId = (typeof VOICE_PROFILES)[number]["id"];
export const DEFAULT_VOICE_ID: VoiceProfileId = "balanced";

export function findVoice(id: string): (typeof VOICE_PROFILES)[number] {
  return VOICE_PROFILES.find((p) => p.id === id) ?? VOICE_PROFILES[2]!;
}

/** 口头标点指令表（中文语境）。 */
export const VOICE_PUNCT_COMMANDS: readonly { say: string; emit: string }[] = [
  { say: "句号", emit: "。" },
  { say: "逗号", emit: "，" },
  { say: "问号", emit: "？" },
  { say: "感叹号", emit: "！" },
  { say: "冒号", emit: "：" },
  { say: "换行", emit: "\n" },
  { say: "逗点", emit: "，" },
];

export type VoicePhase = "idle" | "listening" | "paused" | "done" | "failed";

export const VOICE_NARRATIVES: readonly { code: string; title: string; next: string }[] = [
  { code: "VC-401", title: "麦克风不可用", next: "检查系统声音设置里的输入设备。" },
  { code: "VC-402", title: "静音超时自动收束", next: "继续说即可重新开始听写。" },
  { code: "VC-403", title: "环境噪声过高", next: "换到安静环境，或改用按键说话档。" },
  { code: "VC-404", title: "口头标点未识别", next: "已按原词插入，可在标点表里补充说法。" },
];

export function findVoiceNarrative(code: string): { code: string; title: string; next: string } {
  const up = code.toUpperCase();
  return VOICE_NARRATIVES.find((n) => n.code === up)
    ?? { code: "VC-000", title: "未知语音状况", next: "重跑语音自检或改用键盘输入。" };
}

/** 听写会话：tick 驱动、静音自动收束、口头标点替换。 */
export class VoiceSession {
  profileId: VoiceProfileId;
  phase: VoicePhase = "idle";
  silenceMs = 0;
  text = "";
  codes: string[] = [];
  clamped = 0;

  constructor(profileId: string = DEFAULT_VOICE_ID) {
    const known = VOICE_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findVoice(profileId).id as VoiceProfileId;
  }

  start(): void {
    this.phase = "listening";
    this.silenceMs = 0;
  }

  /** 喂入一帧：有声（text 非空）或静音（静音毫秒数）。 */
  feed(text: string, silenceMs: number): void {
    if (this.phase !== "listening") return;
    if (text) {
      this.silenceMs = 0;
      this.text += this.applyPunct(text);
    } else {
      this.silenceMs += Math.max(0, Math.min(60000, silenceMs));
      const auto = findVoice(this.profileId).autoStopMs;
      if (auto > 0 && this.silenceMs >= auto) {
        this.phase = "done";
        if (!this.codes.includes("VC-402")) this.codes.push("VC-402");
      }
    }
  }

  /** 口头标点替换（VC-404 计数未命中的说法）。 */
  applyPunct(text: string): string {
    if (!findVoice(this.profileId).punctuate) return text;
    let out = text;
    for (const c of VOICE_PUNCT_COMMANDS) {
      if (out.includes(c.say)) out = out.split(c.say).join(c.emit);
    }
    if (VOICE_PUNCT_COMMANDS.some((c) => text.includes(c.say)) && out === text) {
      if (!this.codes.includes("VC-404")) this.codes.push("VC-404");
    }
    return out;
  }

  pause(): void {
    if (this.phase === "listening") this.phase = "paused";
  }

  resume(): boolean {
    if (this.phase !== "paused") return false;
    this.phase = "listening";
    this.silenceMs = 0;
    return true;
  }

  stop(): void {
    if (this.phase === "listening" || this.phase === "paused") this.phase = "done";
  }

  reset(): void {
    this.phase = "idle";
    this.text = "";
    this.codes = [];
    this.silenceMs = 0;
  }
}

/* ============================== 族0169 手写输入 2.0 ============================== */

export type Stroke = { x: number; y: number }[];

/** 手写五档（识别延迟/候选数/笔迹美化）。 */
export const HANDWRITE_PROFILES = [
  { id: "raw", name: "原笔迹", beautify: false, candidates: 3, delayMs: 600 },
  { id: "light", name: "轻量", beautify: false, candidates: 5, delayMs: 450 },
  { id: "balanced", name: "均衡", beautify: true, candidates: 8, delayMs: 350 },
  { id: "smooth", name: "顺滑", beautify: true, candidates: 10, delayMs: 300 },
  { id: "full", name: "全量", beautify: true, candidates: 16, delayMs: 250 },
] as const;
export type HandwriteProfileId = (typeof HANDWRITE_PROFILES)[number]["id"];
export const DEFAULT_HANDWRITE_ID: HandwriteProfileId = "balanced";

export function findHandwrite(id: string): (typeof HANDWRITE_PROFILES)[number] {
  return HANDWRITE_PROFILES.find((p) => p.id === id) ?? HANDWRITE_PROFILES[2]!;
}

/** 轨迹归一化：任意坐标空间 → 0..100 网格（可比较）。 */
export function normalizeStroke(stroke: Stroke): Stroke {
  if (stroke.length === 0) return [];
  const xs = stroke.map((p) => p.x);
  const ys = stroke.map((p) => p.y);
  const minX = Math.min(...xs), maxX = Math.max(...xs);
  const minY = Math.min(...ys), maxY = Math.max(...ys);
  const w = Math.max(1e-6, maxX - minX);
  const h = Math.max(1e-6, maxY - minY);
  return stroke.map((p) => ({ x: Math.round(((p.x - minX) / w) * 100), y: Math.round(((p.y - minY) / h) * 100) }));
}

/** 简化几何特征：归一化后取 9 宫格占用位图（512bit 语义的 9bit 近似）。 */
export function strokeFingerprint(stroke: Stroke): number {
  const n = normalizeStroke(stroke);
  let fp = 0;
  for (const p of n) {
    const gx = Math.min(2, Math.floor(p.x / 34));
    const gy = Math.min(2, Math.floor(p.y / 34));
    fp |= 1 << (gy * 3 + gx);
  }
  return fp;
}

/** 手写候选匹配：与字形库的九宫格指纹汉明距离排序。 */
export function handwritingCandidates(stroke: Stroke, glyphLib: { char: string; fp: number }[], limit = 8): { char: string; score: number }[] {
  const fp = strokeFingerprint(stroke);
  const scored = glyphLib
    .map((g) => {
      let d = 0;
      let x = fp ^ g.fp;
      while (x) { d += x & 1; x >>= 1; }
      return { char: g.char, score: 9 - d };
    })
    .sort((a, b) => b.score - a.score);
  const n = Math.max(1, Math.min(64, limit));
  return scored.slice(0, n);
}

/** 笔迹美化：道格拉斯-普克简化，保留拐点。 */
export function beautifyStroke(stroke: Stroke, epsilon = 3): Stroke {
  if (stroke.length <= 2) return stroke;
  const keep: boolean[] = new Array(stroke.length).fill(false);
  keep[0] = true;
  keep[stroke.length - 1] = true;
  const eps = Math.max(0.1, epsilon);
  const stack: [number, number][] = [[0, stroke.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop()!;
    const ax = stroke[a]!.x, ay = stroke[a]!.y;
    const bx = stroke[b]!.x, by = stroke[b]!.y;
    let maxD = 0;
    let idx = -1;
    for (let i = a + 1; i < b; i++) {
      const px = stroke[i]!.x, py = stroke[i]!.y;
      const dx = bx - ax, dy = by - ay;
      const len2 = dx * dx + dy * dy;
      const d = len2 === 0 ? Math.hypot(px - ax, py - ay) : Math.abs(dy * px - dx * py + bx * ay - by * ax) / Math.sqrt(len2);
      if (d > maxD) { maxD = d; idx = i; }
    }
    if (maxD > eps && idx > 0) {
      keep[idx] = true;
      stack.push([a, idx], [idx, b]);
    }
  }
  return stroke.filter((_, i) => keep[i]);
}

/* ============================== 族0170 表情符号 2.0 ============================== */

export interface EmojiEntry {
  ch: string;
  name: string;
  group: "笑脸" | "手势" | "动物" | "食物" | "物品" | "符号";
  keywords: string[];
}

/** 内置表情目录（可扩展子集；分组语义稳定）。 */
export const EMOJI_CATALOG: readonly EmojiEntry[] = [
  { ch: "😀", name: "开心", group: "笑脸", keywords: ["笑", "开心", "grin"] },
  { ch: "😂", name: "笑哭", group: "笑脸", keywords: ["笑哭", "lol"] },
  { ch: "🥲", name: "含泪笑", group: "笑脸", keywords: ["含泪", "勉强"] },
  { ch: "👍", name: "赞", group: "手势", keywords: ["赞", "好", "thumbs"] },
  { ch: "🙏", name: "合十", group: "手势", keywords: ["谢谢", "拜托"] },
  { ch: "✌️", name: "胜利", group: "手势", keywords: ["耶", "胜利"] },
  { ch: "🐱", name: "猫", group: "动物", keywords: ["猫", "cat"] },
  { ch: "🐕", name: "狗", group: "动物", keywords: ["狗", "dog"] },
  { ch: "🍜", name: "面", group: "食物", keywords: ["面", "拉面"] },
  { ch: "☕", name: "咖啡", group: "食物", keywords: ["咖啡", "coffee"] },
  { ch: "💡", name: "灯泡", group: "物品", keywords: ["想法", "点子"] },
  { ch: "🚀", name: "火箭", group: "物品", keywords: ["发射", "快"] },
  { ch: "✅", name: "完成", group: "符号", keywords: ["完成", "对"] },
  { ch: "⭐", name: "星", group: "符号", keywords: ["星", "收藏"] },
];

/** 表情面板：搜索 + 分组 + 最近使用 LRU（上限钳制）。 */
export class EmojiPanel {
  recents: string[] = [];
  recentLimit = 24;
  clamped = 0;

  constructor(recentLimit = 24) {
    this.recentLimit = Math.max(4, Math.min(96, recentLimit));
  }

  /** 关键词搜索（名称/关键词/字符本身）。 */
  search(q: string, limit = 32): EmojiEntry[] {
    const query = q.trim().toLowerCase();
    if (!query) return [];
    const n = Math.max(1, Math.min(128, limit));
    return EMOJI_CATALOG
      .filter((e) => e.ch === q || e.name.toLowerCase().includes(query) || e.keywords.some((k) => k.toLowerCase().includes(query)))
      .slice(0, n);
  }

  byGroup(group: EmojiEntry["group"]): EmojiEntry[] {
    return EMOJI_CATALOG.filter((e) => e.group === group);
  }

  /** 选用：进最近使用（LRU 去重 + 上限裁剪）。 */
  pick(ch: string): void {
    if (!EMOJI_CATALOG.some((e) => e.ch === ch)) {
      this.clamped += 1;
      return;
    }
    this.recents = [ch, ...this.recents.filter((c) => c !== ch)].slice(0, this.recentLimit);
  }

  /** 皮肤档：给定支持肤色的字符返回档位描述（1~5）。 */
  skinTier(supportsTone: boolean, tier: number): number {
    if (!supportsTone) return 0;
    return Math.max(0, Math.min(5, tier));
  }

  reset(): void {
    this.recents = [];
    this.clamped = 0;
  }
}
