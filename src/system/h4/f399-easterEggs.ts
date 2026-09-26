/**
 * F399 彩蛋总谱（H 域 · AI-H4）：
 * 系统的创造性人格：三枚官方彩蛋入册——
 * ① 开机第 100 次特别进入动画（星徽粒子变奏一次，之后不再重复）；
 * ② 设置中心「关于」页点版本号 7 次出现系统谱系星图动画（F199 的浪漫版）；
 * ③ 终端（F095）输入 star 命令全屏星野粒子（Esc 退）。
 * 彩蛋纪律：不藏功能于彩蛋（彩蛋是情感不是门）、不影响性能与判据（动画走 F124 谱）、
 * discoverability 靠口碑不靠提示。
 * 判据（主册 F399）：三枚触发条件与一次性判据；性能零影响（彩蛋动画走 F124 谱）；
 * 不藏功能审计；谱系动画与 F199 数据同源。
 * 依赖锚点：F095 终端 / F124 动画总谱 / F199 版本与谱系页。
 * 存储键：variable:h4:f399（一次性旗标与计数）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 三个彩蛋的触发条件（判据「三枚触发条件」——口径源）。 */
export const EGG_TRIGGERS = {
  boot100: { type: "bootCount", threshold: 100, once: true, name: "星徽粒子变奏" },
  about7taps: { type: "tapCount", threshold: 7, once: false, name: "谱系星图" },
  terminalStar: { type: "command", value: "star", once: false, name: "星野粒子" },
} as const;

export type EggId = keyof typeof EGG_TRIGGERS;

export interface EggState {
  /** boot100 的开机计数（跨会话累计）。 */
  bootCount: number;
  /** boot100 是否已放过（一次性判据）。 */
  boot100Played: boolean;
  /** about 页连点计数（会话内）。 */
  aboutTaps: number;
}

const KEY = h4Key("f399", "state");

function isState(v: unknown): v is Partial<EggState> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export function loadState(store: KvStore = defaultStore()): EggState {
  const s = readJson<Partial<EggState>>(store, KEY, {}, isState);
  return { bootCount: s.bootCount ?? 0, boot100Played: s.boot100Played ?? false, aboutTaps: s.aboutTaps ?? 0 };
}

function persist(state: EggState, store: KvStore): boolean {
  return writeJson(store, KEY, state);
}

/** 开机计数递增（每次真实开机调用一次）。 */
export function countBoot(store: KvStore = defaultStore()): EggState {
  const s = loadState(store);
  const next = { ...s, bootCount: s.bootCount + 1 };
  persist(next, store);
  return next;
}

/**
 * 彩蛋①：开机第 100 次触发，且一生一次（判据「第 100 次特别进入动画……之后不再重复」）。
 */
export function bootEgg(store: KvStore = defaultStore()): { play: boolean; variant: string | null; state: EggState } {
  const s = loadState(store);
  const hit = s.bootCount === EGG_TRIGGERS.boot100.threshold && !s.boot100Played;
  const next: EggState = { ...s, boot100Played: s.boot100Played || hit };
  if (hit) persist(next, store);
  return { play: hit, variant: hit ? "star-emblem-particles" : null, state: next };
}

/** 彩蛋②：版本号连点计数；7 次触发；再点重置循环（非一次性，低频纪律靠口碑）。 */
export function tapVersion(store: KvStore = defaultStore()): { play: boolean; state: EggState } {
  const s = loadState(store);
  const taps = s.aboutTaps + 1;
  const play = taps >= EGG_TRIGGERS.about7taps.threshold;
  const next: EggState = { ...s, aboutTaps: play ? 0 : taps };
  persist(next, store);
  return { play, state: next };
}

/** 彩蛋③：终端 star 命令；Esc 退出（命令路径与退出路径成对）。 */
export function terminalCommand(cmd: string): { play: boolean; exit: boolean; scene: string | null } {
  const c = cmd.trim().toLowerCase();
  if (c === EGG_TRIGGERS.terminalStar.value) return { play: true, exit: false, scene: "starfield-particles" };
  if (c === "star --exit" || c === "esc") return { play: false, exit: true, scene: null };
  return { play: false, exit: false, scene: null };
}

/** 性能零影响审计（判据「动画走 F124 谱」）：彩蛋动画必须登记在总谱表内。 */
export function auditPerfRegistry(scene: string, f124Curves: ReadonlySet<string>): { pass: boolean; registered: boolean } {
  return { pass: f124Curves.has(scene), registered: f124Curves.has(scene) };
}

/** 不藏功能审计（判据「彩蛋是情感不是门」）：彩蛋不得解锁任何功能。 */
export function auditNoFeatureGating(unlockedFeatures: string[]): { pass: boolean; gated: string[] } {
  return { pass: unlockedFeatures.length === 0, gated: unlockedFeatures };
}

/** 谱系动画与 F199 数据同源（判据）：星图数据取谱系页同一数据通道（函数签名即契约）。 */
export function lineageDataForEgg(f199Loader: () => Array<{ version: string; codename: string }>): Array<{ version: string; codename: string }> {
  return f199Loader();
}
