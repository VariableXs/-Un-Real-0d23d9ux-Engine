/**
 * AI-07 效率中枢组 · N-17 快捷键中心（单一事实源）：
 * - 全量热键（全局 + 环境内）统一声明 {scope, key, title, when, rebindable}，
 *   运行时从注册表驱动绑定（shortcutsApply 整表重注册管道保持不变）
 * - 冲突检测：内部互冲 + 内置常见三方应用占用表（QQ/IDE/微信…），改键实时标红
 * - Profile 方案档（默认/单手/左手鼠标习惯）`.vkeys` 导入导出
 * - V-50 键位速查表导出：Markdown / 可打印 HTML（A4 双栏、冲突标红、含让位标记）
 *
 * 迁移策略（红线）：旧 SHORTCUT_ACTIONS 为既有单一事实源，本模块以它为地基
 * 增量扩展（AI-07 新动作），不改动既有 id/accel 语义——commands_e2e 契约不变。
 */

import { SHORTCUT_ACTIONS, normalizeAccel, prettyAccel } from "../shortcuts";
import type { Lang } from "../../i18n/dictionaries";

export interface KeyBinding {
  /** 与 shortcuts.ts/winman.rs 既有 action id 一致（迁移零成本）。 */
  action: string;
  /** 词典 key（scAct*）。 */
  labelKey: string;
  accel: string;
  group: "system" | "panel" | "window" | "launch" | "efficiency";
  /** 作用域：global = 系统级 RegisterHotKey；inapp = 环境内（webview 键盘）。 */
  scope: "global" | "inapp";
  /** 是否允许用户改键。 */
  rebindable: boolean;
}

/**
 * AI-07 效率中枢新增热键（NEXT-40 槽位冻结表 + 化境槽位）：
 * - commandPalette: ctrl+k（N-13；应用内优先，全局注册失败如实降级）
 * - purePaste: ctrl+shift+v（V-44 让位检测：目标应用自带时不拦截）
 * - quickNote: ctrl+alt+n（V-46，槽位冻结表指定；notifyCenter 迁移至 ctrl+shift+n）
 * - inlineCalc / timestamp: 面板内触点（V-41/V-49），无全局键
 */
export const EFFICIENCY_BINDINGS: KeyBinding[] = [
  { action: "commandPalette", labelKey: "scActPalette", accel: "ctrl+k", group: "efficiency", scope: "inapp", rebindable: true },
  { action: "purePaste", labelKey: "scActPurePaste", accel: "ctrl+shift+v", group: "efficiency", scope: "global", rebindable: true },
  { action: "quickNote", labelKey: "scActQuickNote", accel: "ctrl+alt+n", group: "efficiency", scope: "global", rebindable: true },
  { action: "snipRegion", labelKey: "scActSnip", accel: "ctrl+alt+s", group: "efficiency", scope: "global", rebindable: true },
];

/** 既有动作（shortcuts.ts）→ KeyBinding（global、可改键）。 */
function legacyBindings(): KeyBinding[] {
  // notifyCenter 迁移：ctrl+alt+n 让位给全局速记（V-46 槽位冻结），
  // 通知中心移至 ctrl+shift+n（N-17 顺序约束内的根级迁移，契约事件名不变）。
  return SHORTCUT_ACTIONS.map((a) => ({
    action: a.id,
    labelKey: a.labelKey,
    accel: a.id === "notifyCenter" ? "ctrl+shift+n" : a.accel,
    group: a.group,
    scope: "global" as const,
    rebindable: true,
  }));
}

/** 整表 = 既有 + AI-07 增量（默认）。 */
export function defaultKeymap(): KeyBinding[] {
  return [...legacyBindings(), ...EFFICIENCY_BINDINGS];
}

/** 应用用户覆盖后的生效表（overrides: action → accel）。 */
export function effectiveKeymap(overrides: Record<string, string> = {}): KeyBinding[] {
  return defaultKeymap().map((b) => {
    const ov = overrides[b.action];
    if (!ov) return b;
    const n = normalizeAccel(ov);
    return n ? { ...b, accel: n } : b; // 非法覆盖忽略，保持默认（如实降级）
  });
}

// ---------- 冲突检测 ----------

/** 内置常见三方应用占用表（N-17：改键实时标红 + 解决建议）。 */
export interface ThirdPartyOccupancy {
  accel: string;
  app: string;
  /** 建议的替代组合。 */
  suggestion: string;
}

export const THIRD_PARTY_OCCUPANCY: ThirdPartyOccupancy[] = [
  { accel: "ctrl+shift+v", app: "Chrome/Edge/VS Code（无格式粘贴）", suggestion: "ctrl+alt+v" },
  { accel: "ctrl+k", app: "Chrome/VS Code（焦点地址栏/删除行）", suggestion: "ctrl+alt+k" },
  { accel: "ctrl+alt+tab", app: "Windows（窗口切换）", suggestion: "ctrl+alt+`" },
  { accel: "ctrl+shift+esc", app: "Windows（任务管理器）", suggestion: "ctrl+alt+delete" },
  { accel: "alt+f4", app: "Windows（关闭窗口）", suggestion: "ctrl+shift+w" },
  { accel: "ctrl+shift+n", app: "Chrome（无痕窗口）/ VS Code（新窗口）", suggestion: "ctrl+alt+u" },
];

export interface KeyConflict {
  accel: string;
  /** 内部互冲的 action 列表。 */
  actions: string[];
  /** 三方占用（如有）。 */
  thirdParty?: ThirdPartyOccupancy;
}

/** 冲突检测：同一 accel 多 action（内部互冲）+ 三方占用表命中。 */
export function detectConflicts(keymap: KeyBinding[]): KeyConflict[] {
  const byAccel = new Map<string, string[]>();
  for (const b of keymap) {
    const list = byAccel.get(b.accel) ?? [];
    list.push(b.action);
    byAccel.set(b.accel, list);
  }
  const conflicts: KeyConflict[] = [];
  for (const [accel, actions] of byAccel) {
    if (actions.length > 1) {
      conflicts.push({ accel, actions });
    } else {
      const tp = THIRD_PARTY_OCCUPANCY.find((t) => t.accel === accel);
      if (tp) conflicts.push({ accel, actions, thirdParty: tp });
    }
  }
  return conflicts.sort((a, b) => a.accel.localeCompare(b.accel));
}

// ---------- Profile 方案档（.vkeys） ----------

export interface KeyProfile {
  format: "vkeys";
  version: 1;
  name: string;
  /** action → accel 覆盖。 */
  overrides: Record<string, string>;
}

export const BUILTIN_PROFILES: { id: string; nameKey: string; overrides: Record<string, string> }[] = [
  { id: "default", nameKey: "scProfileDefault", overrides: {} },
  // 单手方案：高频动作用 ctrl+alt+主键区左侧
  {
    id: "onehand",
    nameKey: "scProfileOnehand",
    overrides: {
      snapLeft: "ctrl+alt+q",
      snapRight: "ctrl+alt+e",
      snapUp: "ctrl+alt+w",
      snapDown: "ctrl+alt+s",
      commandPalette: "ctrl+alt+p",
    },
  },
  // 左手鼠标习惯：ctrl+alt+数字区
  {
    id: "mouseleft",
    nameKey: "scProfileMouseleft",
    overrides: {
      snapLeft: "ctrl+alt+insert",
      snapRight: "ctrl+alt+pageup",
      snapUp: "ctrl+alt+home",
      snapDown: "ctrl+alt+end",
    },
  },
];

/** 导出 .vkeys（JSON；非法条目如实剔除）。 */
export function exportProfile(name: string, keymap: KeyBinding[]): KeyProfile {
  const overrides: Record<string, string> = {};
  const defaults = new Map(defaultKeymap().map((b) => [b.action, b.accel]));
  for (const b of keymap) {
    if (!b.rebindable) continue;
    if (defaults.get(b.action) !== b.accel) overrides[b.action] = b.accel;
  }
  return { format: "vkeys", version: 1, name, overrides };
}

/** 导入 .vkeys（版本/格式校验；冲突键如实保留并返回冲突清单）。 */
export function importProfile(raw: string): { profile: KeyProfile; conflicts: KeyConflict[] } {
  const p = JSON.parse(raw) as KeyProfile;
  if (p.format !== "vkeys" || p.version !== 1 || typeof p.overrides !== "object" || p.overrides === null) {
    throw new Error("非法 .vkeys 档案");
  }
  const keymap = effectiveKeymap(p.overrides);
  return { profile: p, conflicts: detectConflicts(keymap) };
}

// ---------- V-50 键位速查表导出 ----------

export interface CheatSheetOptions {
  lang: Lang;
  /** 词典解析（注入避免循环依赖）。 */
  t: (key: string) => string;
  profileName: string;
  now?: Date;
}

/** 按域分组导出 Markdown 速查表（冲突项标红 = ⚠ 前缀；让位中如实标注「系统占用」）。 */
export function exportCheatSheetMarkdown(keymap: KeyBinding[], opts: CheatSheetOptions): string {
  const conflicts = new Map(detectConflicts(keymap).map((c) => [c.accel, c]));
  const groups = new Map<string, KeyBinding[]>();
  for (const b of keymap) {
    const list = groups.get(b.group) ?? [];
    list.push(b);
    groups.set(b.group, list);
  }
  const groupOrder = ["system", "panel", "window", "launch", "efficiency"];
  const lines: string[] = [];
  lines.push(`# ${opts.t("scCheatTitle")} — ${opts.profileName}`);
  lines.push("");
  lines.push(`> ${opts.t("scCheatGenerated")}: ${Math.round((opts.now ?? new Date()).getTime() / 1000)} · ${keymap.length} ${opts.t("scCheatEntries")}`);
  lines.push("");
  for (const g of groupOrder) {
    const list = groups.get(g);
    if (!list?.length) continue;
    lines.push(`## ${opts.t(`scGroup_${g}`)}`);
    lines.push("");
    lines.push(`| ${opts.t("scCheatKey")} | ${opts.t("scCheatAction")} | |`);
    lines.push("|---|---|---|");
    for (const b of list) {
      const conflict = conflicts.get(b.accel);
      const mark = conflict ? ` ⚠️ ${conflict.thirdParty ? opts.t("scCheatOccupied") : opts.t("scCheatConflict")}` : "";
      lines.push(`| \`${prettyAccel(b.accel, opts.lang)}\` | ${opts.t(b.labelKey)} |${mark}|`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

/** 可打印 HTML 速查表（A4 双栏排版，tokens 变量沿用 :root）。 */
export function exportCheatSheetHtml(keymap: KeyBinding[], opts: CheatSheetOptions): string {
  const conflicts = new Map(detectConflicts(keymap).map((c) => [c.accel, c]));
  const groups = new Map<string, KeyBinding[]>();
  for (const b of keymap) {
    const list = groups.get(b.group) ?? [];
    list.push(b);
    groups.set(b.group, list);
  }
  const groupOrder = ["system", "panel", "window", "launch", "efficiency"];
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const sections = groupOrder
    .filter((g) => groups.get(g)?.length)
    .map((g) => {
      const rows = groups
        .get(g)!
        .map((b) => {
          const c = conflicts.get(b.accel);
          const cls = c ? ' class="conflict"' : "";
          const mark = c ? ` <small>${c.thirdParty ? esc(opts.t("scCheatOccupied")) : esc(opts.t("scCheatConflict"))}</small>` : "";
          return `<tr${cls}><td><kbd>${esc(prettyAccel(b.accel, opts.lang))}</kbd></td><td>${esc(opts.t(b.labelKey))}${mark}</td></tr>`;
        })
        .join("");
      return `<section><h2>${esc(opts.t(`scGroup_${g}`))}</h2><table>${rows}</table></section>`;
    })
    .join("");
  return `<!DOCTYPE html>
<html lang="${opts.lang}">
<head>
<meta charset="utf-8">
<title>${esc(opts.t("scCheatTitle"))} — ${esc(opts.profileName)}</title>
<style>
  :root { color-scheme: light dark; }
  body { font: 13px/1.5 system-ui, "Segoe UI", sans-serif; margin: 24px; }
  h1 { font-size: 20px; } h2 { font-size: 15px; margin: 14px 0 6px; }
  .grid { column-count: 2; column-gap: 32px; }
  section { break-inside: avoid; }
  table { border-collapse: collapse; width: 100%; }
  td { padding: 3px 8px 3px 0; border-bottom: 1px solid rgba(128,128,128,.25); }
  kbd { font: 12px ui-monospace, Consolas, monospace; background: rgba(128,128,128,.15); padding: 1px 5px; border-radius: 4px; }
  tr.conflict td { color: #c0392b; font-weight: 600; }
  @media print { body { margin: 8mm; } .grid { column-count: 2; } }
  @page { size: A4; }
</style>
</head>
<body>
<h1>${esc(opts.t("scCheatTitle"))} — ${esc(opts.profileName)}</h1>
<p><small>${esc(opts.t("scCheatGenerated"))}: ${Math.round((opts.now ?? new Date()).getTime() / 1000)} · ${keymap.length} ${esc(opts.t("scCheatEntries"))}</small></p>
<div class="grid">${sections}</div>
</body>
</html>`;
}

// ---------- 学习模式（按键 → 功能说明卡） ----------

/** 学习模式查询：按 accel 反查绑定（供「按下任意键高亮其功能」角卡）。 */
export function findBindingByAccel(keymap: KeyBinding[], accel: string): KeyBinding[] {
  const n = normalizeAccel(accel);
  if (!n) return [];
  return keymap.filter((b) => b.accel === n);
}
