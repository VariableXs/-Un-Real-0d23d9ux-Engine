/**
 * Fate Tree Prediction Engine (FTPE) — shared types (spec v2.0).
 * Fully offline, template-free: every dictionary entry is user-editable and
 * extensible; no real-person templates exist anywhere in this module.
 */

/** Event semantic tags shared by dictionaries and the engine. */
export const FATE_TAGS = [
  "冒险", "社交", "事业", "感情", "健康", "意外",
  "觉醒", "冲突", "守成", "金钱", "名望", "内心",
  "危险", "家庭", "学识",
  // 模块三·第四轮：更多 buff 维度标签
  "机缘", "羁绊", "传承", "欲望",
] as const;
export type FateTag = (typeof FATE_TAGS)[number];

/** Event library layers (spec ch.7). */
export const EVENT_CATEGORIES = [
  { id: "L1", name: "日常琐事", nameEn: "Daily life", color: "#8babc6", base: 0.8 },
  { id: "L2", name: "关键转折", nameEn: "Turning point", color: "#7fa8d6", base: 0.35 },
  { id: "L3", name: "命运级", nameEn: "Fate-level", color: "#4f709c", base: 0.15 },
  { id: "L4", name: "黑天鹅", nameEn: "Black swan", color: "#f8d4e4", base: 0.05 },
  { id: "L5", name: "内心觉醒", nameEn: "Inner awakening", color: "#f0f6ff", base: 0.25 },
  { id: "L6", name: "人际关系", nameEn: "Relationships", color: "#9db8d9", base: 0.5 },
  { id: "L7", name: "事业成长", nameEn: "Career growth", color: "#a9c4e8", base: 0.45 },
] as const;
export type EventCategoryId = (typeof EVENT_CATEGORIES)[number]["id"];

/** Weight of a trait on one semantic tag. Range -1..1 (e.g. 冒险 +0.6). */
export type TagWeight = { tag: FateTag; weight: number };

/** A dictionary entry trait slider value, -100..+100 (no-step, spec 1.3). */
export type TraitValue = number;

interface DictBase {
  id: string;
  name: string;
  /** Plain-language portrait, user editable. */
  desc: string;
  /** Effects on event tags: the precise influence table (spec 1.3). */
  effects: TagWeight[];
  /** User-created / modified entries survive in overrides + .fatetree files. */
  custom?: boolean;
  enabled?: boolean;
}

export interface PersonalityEntry extends DictBase {
  /** Compat / conflict network (names of other personalities). */
  compat: string[];
  conflict: string[];
}

export interface CharacterEntry extends DictBase {
  /** Name of the opposite pole shown at the slider's negative end. */
  opposite: string;
}

export interface IdeologyEntry extends DictBase {
  /** Names of inherently conflicting ideologies (spec 5.5). */
  conflict: string[];
}

export interface RandomFactor {
  id: string;
  name: string;
  desc: string;
  /** 0..1 base occurrence weight per time step. */
  base: number;
  /** Tags it perturbs. */
  tags: FateTag[];
  enabled: boolean;
  /** -100..100 user strength dial. */
  strength: number;
  custom?: boolean;
}

/** 模块三：增益（buff）——启用后影响整个推演的倾向修正。 */
export interface BuffEntry {
  id: string;
  name: string;
  desc: string;
  /** 对携带这些标签的事件做概率乘法（1 = 无影响）。 */
  boost: Partial<Record<FateTag, number>>;
  enabled: boolean;
}

export interface EventEntry {
  id: string;
  name: string;
  cat: EventCategoryId;
  /** Base trigger probability 0..1 per qualifying time step. */
  base: number;
  desc: string;
  tags: FateTag[];
  /** Free-text precondition (checked narratively, shown to the user). */
  requires?: string;
  custom?: boolean;
  enabled?: boolean;
}

/** Full role portrait assembled from the three dimensions (spec 1.2/3.5). */
export interface RoleProfile {
  name: string;
  /** 1-3 dominant personalities with blend weights (0..100). */
  personalities: { id: string; weight: number }[];
  /** Every character trait the user touched, value -100..100. */
  characters: { id: string; value: number }[];
  /** Chosen ideologies with intensity -100..100. */
  ideologies: { id: string; value: number }[];
}

export interface SimParams {
  /** Time horizon in steps (years). */
  years: number;
  /** Max branches per time step (1..4). */
  branching: number;
  /** Randomness strength 0..100. */
  chaos: number;
  /** Deterministic seed (spec 13.1: reproducible). */
  seed: number;
  /** Age at the simulation start. */
  startAge: number;
  /** 模块三：启用的增益（buff）id 列表——修改事件概率的持久状态。 */
  buffs?: string[];
}

/** One node of the fate tree. */
export interface FateNode {
  id: string;
  parentId: string | null;
  /** Absolute step index (0 = start). */
  step: number;
  /** Age label, e.g. "27 岁 · 春". */
  ageLabel: string;
  name: string;
  cat: EventCategoryId | "root" | "ending";
  prob: number;
  desc: string;
  children: FateNode[];
  ending?: { type: string; text: string };
  /** Set when this subtree came from a committed drill-down. */
  drilled?: boolean;
}

export interface FateVersion {
  id: string;
  name: string;
  createdAt: number;
  root: FateNode;
}

/** The complete .fatetree document (spec ch.13). */
export interface FateDoc {
  app: "variable-fatetree";
  formatVersion: number;
  name: string;
  savedAt: number;
  profile: RoleProfile;
  params: SimParams;
  root: FateNode;
  versions: FateVersion[];
  /** Custom/edited dictionary entries travelling with the file. */
  customDict: {
    personalities: PersonalityEntry[];
    characters: CharacterEntry[];
    ideologies: IdeologyEntry[];
    events: EventEntry[];
    randoms: RandomFactor[];
  };
  /** User-added annotations. */
  notes: string;
  /** Last canvas viewport (v3.0 1.3: reopen restores the exact view). */
  viewport?: { x: number; y: number; z: number };
}
