/**
 * FTPE simulation engine (spec ch.9-11). Fully deterministic given a seed:
 *   final prob = base × trait multipliers × random-factor perturbation
 * Trait multipliers sum each (value/100 × effect-weight) per matching tag
 * then clamp — every slider position produces a different outcome (1.3).
 */
import {
  ENDING_ARCHETYPES, PRESET_EVENTS, PRESET_PERSONALITIES,
} from "./dictionaries";
import {
  EVENT_CATEGORIES, type BuffEntry, type EventEntry, type FateNode, type RoleProfile,
  type SimParams, type TagWeight,
} from "./types";

/** mulberry32 — small, fast, fully reproducible (spec 13.1). */
export function makeRng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

/**
 * 模块三：把启用的 buff 列表折叠成“标签 → 概率乘数”函数。
 * 多个 buff 叠乘，整体钳制在 0.05..4，避免一格 buff 把推演掀桌。
 */
export function makeBuffFn(buffs: BuffEntry[]): (tag: string) => number {
  const active = buffs.filter((b) => b.enabled);
  return (tag: string): number => {
    let m = 1;
    for (const b of active) {
      const k = b.boost[tag as keyof BuffEntry["boost"]];
      if (typeof k === "number") m *= k;
    }
    return clamp(m, 0.05, 4);
  };
}

/** All trait vectors that feed the probability model, flattened. */
export interface TraitSource {
  effects: TagWeight[];
  /** -100..100 intensity. */
  value: number;
}

/** Resolve the profile + custom dictionaries into a flat trait list. */
export function collectTraits(
  profile: RoleProfile,
  personalities: { id: string; effects: TagWeight[] }[],
  characters: { id: string; effects: TagWeight[] }[],
  ideologies: { id: string; effects: TagWeight[] }[],
): TraitSource[] {
  const out: TraitSource[] = [];
  for (const p of profile.personalities) {
    const e = personalities.find((x) => x.id === p.id);
    if (e) out.push({ effects: e.effects, value: p.weight * 2 - 30 }); // blend weight 0..100 → -30..+170 effective intensity
  }
  for (const c of profile.characters) {
    const e = characters.find((x) => x.id === c.id);
    if (e) out.push({ effects: e.effects, value: c.value });
  }
  for (const i of profile.ideologies) {
    const e = ideologies.find((x) => x.id === i.id);
    if (e) out.push({ effects: e.effects, value: i.value });
  }
  return out;
}

/**
 * Multiplier applied to events carrying `tag` (spec 1.3 / 9.2).
 * v3.0 1.1: contributions pass through a SIGMOID mapping instead of a linear
 * ramp — middle values stay responsive while extreme values saturate
 * smoothly, so "butterfly effect" growth feels natural instead of runaway.
 */
export function traitMultiplier(traits: TraitSource[], tag: string): number {
  let m = 1;
  for (const t of traits) {
    const s = clamp(t.value / 100, -1.7, 1.7);
    const k = 2 / (1 + Math.exp(-2.6 * s)) - 1; // sigmoid: k(0)=0, k(±100)≈±0.93
    for (const e of t.effects) {
      if (e.tag === tag) m += k * e.weight * 0.6;
    }
  }
  return clamp(m, 0.05, 4);
}

/** Detect inner-struggle ideologies (both sides of a conflict pair high). */
export function ideologyConflicts(
  profile: RoleProfile,
  ideologies: { id: string; name: string; conflict: string[] }[],
): [string, string][] {
  const high = new Map<string, number>();
  for (const i of profile.ideologies) {
    const e = ideologies.find((x) => x.id === i.id);
    if (e && Math.abs(i.value) >= 50) high.set(e.name, i.value);
  }
  const pairs: [string, string][] = [];
  for (const [name] of high) {
    const e = ideologies.find((x) => x.name === name);
    if (!e) continue;
    for (const cf of e.conflict) {
      if (high.has(cf) && name < cf) pairs.push([name, cf]);
    }
  }
  return pairs;
}

/** Probability of one event under the current profile & random factors. */
export function eventProbability(
  ev: EventEntry,
  traits: TraitSource[],
  chaos: number,
  rng: () => number,
  randomMult = 1,
  struggleBoost = 0,
  buffFn: (tag: string) => number = () => 1,
): number {
  let m = 1;
  for (const t of ev.tags) m *= traitMultiplier(traits, t) * buffFn(t);
  // chaos: broaden the distribution so mid values still surface sometimes
  const jitter = 1 + (rng() * 2 - 1) * (chaos / 100) * 0.6;
  const p = ev.base * clamp(m, 0.05, 4) * jitter * randomMult * (1 + struggleBoost);
  return clamp(p, 0.001, 0.95);
}

const SEASON_ZH = ["春", "夏", "秋", "冬"];
const SEASON_EN = ["Spring", "Summer", "Autumn", "Winter"];

function ageLabel(startAge: number, step: number, rng: () => number, lang: "zh" | "en"): string {
  const age = startAge + step;
  const seasons = lang === "en" ? SEASON_EN : SEASON_ZH;
  const season = step === 0 ? "" : ` · ${seasons[Math.floor(rng() * 4)]}`;
  return lang === "en" ? `Age ${age}${season}` : `${age} 岁${season}`;
}

/** Pick K distinct events for one step via weighted sampling. */
function pickEvents(
  pool: EventEntry[],
  traits: TraitSource[],
  chaos: number,
  k: number,
  rng: () => number,
  randomMult: number,
  struggleBoost: number,
  chainTags: string[],
  buffFn: (tag: string) => number,
): { ev: EventEntry; p: number }[] {
  const scored = pool.map((ev) => {
    let p = eventProbability(ev, traits, chaos, rng, randomMult, struggleBoost, buffFn);
    // chain pressure: tags of the parent event nudge related events
    for (const t of ev.tags) if (chainTags.includes(t)) p *= 1.35;
    return { ev, p };
  });
  const picked: { ev: EventEntry; p: number }[] = [];
  const used = new Set<string>();
  for (let n = 0; n < k; n++) {
    const cands = scored.filter((s) => !used.has(s.ev.id));
    if (cands.length === 0) break;
    const total = cands.reduce((a, s) => a + s.p, 0);
    let r = rng() * total;
    let chosen = cands[cands.length - 1]!;
    for (const c of cands) {
      r -= c.p;
      if (r <= 0) { chosen = c; break; }
    }
    used.add(chosen.ev.id);
    picked.push(chosen);
  }
  return picked.filter((s) => s.p > 0.008);
}

function chooseEnding(tone: number, rng: () => number, lang: "zh" | "en"): { type: string; text: string } {
  // tone ∈ [-1..1] aggregated from the path's event tags
  const pool = ENDING_ARCHETYPES.filter((a) => Math.abs(a.tone - tone) <= 1);
  const a = pool[Math.floor(rng() * pool.length)] ?? ENDING_ARCHETYPES[0]!;
  return lang === "en" ? { type: a.typeEn, text: a.textEn } : { type: a.type, text: a.text };
}

interface SimContext {
  events: EventEntry[];
  traits: TraitSource[];
  randomMult: number;
  struggleBoost: number;
  lang: "zh" | "en";
  buffFn: (tag: string) => number;
}

/** Depth-first tree growth: each step branches into `branching` sampled events. */
function grow(
  parent: FateNode,
  remaining: number,
  ctx: SimContext,
  params: SimParams,
  rng: () => number,
  chainTags: string[],
  idGen: () => string,
  posTone: { v: number; n: number },
): void {
  if (remaining <= 0) {
    const tone = posTone.n === 0 ? 0 : clamp(posTone.v / posTone.n, -1, 1);
    parent.ending = chooseEnding(tone, rng, ctx.lang);
    parent.cat = "ending";
    return;
  }
  const step = parent.step + 1;
  const picked = pickEvents(
    ctx.events, ctx.traits, params.chaos, params.branching, rng,
    ctx.randomMult, ctx.struggleBoost, chainTags, ctx.buffFn,
  );
  if (picked.length === 0) {
    parent.ending = ctx.lang === "en"
      ? { type: "Unfinished ending", text: "The event chain broke here — try looser params or lower chaos." }
      : { type: "未完成结局", text: "事件链在此处中断——可尝试放宽参数或降低随机强度。" };
    parent.cat = "ending";
    return;
  }
  for (const { ev, p } of picked) {
    const node: FateNode = {
      id: idGen(),
      parentId: parent.id,
      step,
      ageLabel: ageLabel(params.startAge, step, rng, ctx.lang),
      name: ev.name,
      cat: ev.cat,
      prob: Math.round(p * 1000) / 1000,
      desc: ev.desc + (ev.requires ? `（前置：${ev.requires}）` : ""),
      children: [],
    };
    for (const t of ev.tags) {
      posTone.v += t === "觉醒" || t === "事业" || t === "感情" || t === "机缘" || t === "羁绊" ? 1 : t === "危险" || t === "冲突" || t === "欲望" ? -1 : 0;
      posTone.n += 1;
    }
    grow(node, remaining - 1, ctx, params, rng, ev.tags, idGen, posTone);
    parent.children.push(node);
  }
}

let idCounter = 0;
const nextId = (): string => `fn${(idCounter++).toString(36)}`;

/** Run a full simulation → fate tree root (spec 9.1). */
export function simulate(
  profile: RoleProfile,
  params: SimParams,
  events: EventEntry[],
  traits: TraitSource[],
  randomMult: number,
  struggleBoost: number,
  lang: "zh" | "en" = "zh",
  buffs: BuffEntry[] = [],
): FateNode {
  const rng = makeRng(params.seed);
  idCounter = 0; // 确定性：同种子两次推演产生完全相同的 id 序列（13.1）
  const active = events.filter((e) => e.enabled !== false);
  const root: FateNode = {
    id: nextId(),
    parentId: null,
    step: 0,
    ageLabel: lang === "en" ? `Age ${params.startAge}` : `${params.startAge} 岁`,
    name: profile.name || (lang === "en" ? "The Unnamed" : "无名者"),
    cat: "root",
    prob: 1,
    desc: lang === "en" ? "Origin: base character setup" : "起点：人物基础设定",
    children: [],
  };
  grow(
    root, Math.max(1, Math.min(params.years, 30)),
    { events: active, traits, randomMult, struggleBoost, lang, buffFn: makeBuffFn(buffs) },
    params, rng, [], nextId, { v: 0, n: 0 },
  );
  return root;
}

/**
 * Continue an existing simulation: every terminal ending leaf grows a fresh
 * subtree of `years` more steps (the old ending mark is lifted where growth
 * happens). The existing tree is mutated in place and returned — nothing is
 * cleared, so the user can keep extending until they stop.
 */
export function continueSimulate(
  root: FateNode,
  params: SimParams,
  events: EventEntry[],
  traits: TraitSource[],
  randomMult: number,
  struggleBoost: number,
  years: number,
  lang: "zh" | "en" = "zh",
  buffs: BuffEntry[] = [],
): FateNode {
  const active = events.filter((e) => e.enabled !== false);
  const buffFn = makeBuffFn(buffs);
  const leaves: FateNode[] = [];
  const walk = (n: FateNode): void => {
    if (n.ending && n.children.length === 0) leaves.push(n);
    for (const c of n.children) walk(c);
  };
  walk(root);
  leaves.forEach((leaf, i) => {
    const rng = makeRng((params.seed ^ (leaf.step * 7919) ^ ((i + 1) * 2654435761)) >>> 0);
    leaf.ending = undefined;
    leaf.cat = "L2"; // continuation pivot: the former ending becomes a turning point
    grow(
      leaf, Math.max(1, Math.min(years, 10)),
      { events: active, traits, randomMult, struggleBoost, lang, buffFn },
      params, rng, [], nextId, { v: 0, n: 0 },
    );
  });
  return root;
}

/**
 * Drill-down re-simulation (spec 10.1/10.2): re-derive a node's subtree with
 * finer time granularity. The caller replaces node.children with the result —
 * that IS the reverse feedback onto the trunk.
 */
export function drillDown(
  node: FateNode,
  params: SimParams,
  events: EventEntry[],
  traits: TraitSource[],
  randomMult: number,
  struggleBoost: number,
  seedSalt: number,
  subYears: number,
  lang: "zh" | "en" = "zh",
  buffs: BuffEntry[] = [],
): FateNode {
  const rng = makeRng((params.seed ^ (node.id.length << 7) ^ seedSalt) >>> 0);
  const sub: FateNode = {
    ...node,
    id: nextId(),
    step: node.step,
    children: [],
    drilled: true,
  };
  const holder: FateNode = { ...sub, children: [] };
  grow(
    holder, Math.max(1, Math.min(subYears, 8)),
    { events: events.filter((e) => e.enabled !== false), traits, randomMult, struggleBoost, lang, buffFn: makeBuffFn(buffs) },
    { ...params, years: subYears, branching: Math.max(2, params.branching) },
    rng, [], nextId, { v: 0, n: 0 },
  );
  sub.children = holder.children;
  return sub;
}

/** Aggregate a subtree: node count / max depth / endings (结局全览). */
export function treeStats(root: FateNode): { count: number; endings: FateNode[]; maxDepth: number } {
  let count = 0;
  let maxDepth = 0;
  const endings: FateNode[] = [];
  const walk = (n: FateNode, d: number): void => {
    count++;
    maxDepth = Math.max(maxDepth, d);
    if (n.ending) endings.push(n);
    for (const c of n.children) walk(c, d + 1);
  };
  walk(root, 0);
  return { count, endings, maxDepth };
}

/**
 * Tidy horizontal layout (模块二): the tree is forced onto a strict
 * left → right logic axis — x is exactly the depth (step) of the node,
 * y is distributed evenly over in-order leaf slots, and parents always
 * sit vertically centered between their children. GAP_Y exceeds the
 * largest node (ending seal ≈130px) so nothing ever overlaps.
 */
export function layoutTree(root: FateNode): Map<string, { x: number; y: number }> {
  const pos = new Map<string, { x: number; y: number }>();
  let leafY = 0;
  let maxStep = 0;
  const GAP_X = 290; // 横向步距：大于节点宽度 190px，层与层绝不粘连
  const GAP_Y = 150; // 纵向叶距：大于结局印章 130px，兄弟分支绝不重叠
  const walk = (n: FateNode, depth: number): { minY: number; maxY: number } => {
    maxStep = Math.max(maxStep, depth);
    pos.set(n.id, { x: depth * GAP_X, y: 0 });
    if (n.children.length === 0) {
      const y = leafY++ * GAP_Y;
      pos.get(n.id)!.y = y;
      return { minY: y, maxY: y };
    }
    let minY = Infinity;
    let maxY = -Infinity;
    for (const c of n.children) {
      const r = walk(c, depth + 1);
      minY = Math.min(minY, r.minY);
      maxY = Math.max(maxY, r.maxY);
    }
    pos.get(n.id)!.y = (minY + maxY) / 2;
    return { minY, maxY };
  };
  walk(root, 0);
  void maxStep;
  return pos;
}

export const catColor = (cat: string): string =>
  cat === "root" ? "#dff0ff"
    : cat === "ending" ? "#f8d4e4"
      : EVENT_CATEGORIES.find((c) => c.id === cat)?.color ?? "#8babc6";

export const catName = (cat: string, lang: "zh" | "en" = "zh"): string => {
  if (cat === "root") return lang === "en" ? "Origin" : "起点";
  if (cat === "ending") return lang === "en" ? "Ending" : "结局";
  const c = EVENT_CATEGORIES.find((x) => x.id === cat);
  if (!c) return cat;
  return lang === "en" ? (c as { nameEn?: string }).nameEn ?? c.name : c.name;
};

/** Default profile when the space is first opened. */
export function emptyProfile(): RoleProfile {
  return { name: "", personalities: [], characters: [], ideologies: [] };
}

export function defaultParams(): SimParams {
  return { years: 8, branching: 2, chaos: 35, seed: (Date.now() & 0xffffff) >>> 0, startAge: 22 };
}

/** Guard: a profile must have at least one trait before simulating. */
export function profileReady(p: RoleProfile): boolean {
  return p.personalities.length > 0 || p.characters.length > 0 || p.ideologies.length > 0;
}

export { PRESET_EVENTS, PRESET_PERSONALITIES };
