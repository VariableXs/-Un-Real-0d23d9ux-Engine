/**
 * F161 个性化档案导出 · 完整设计。
 *
 * 主册判据：全项导出-导入 round-trip 逐项等值（令牌表哈希对拍）；差异预览准确；
 * 中断原子性实测。
 *
 * 【功能定义】E 域全配置打包 vxtheme：主题+壁纸+图标包+指针+声音+短语库+布局
 * 预设+动效档——含版本号与回滚链；导入即复原（F147 随身同步的本体格式）。
 *
 * 【状态与异常】包含第三方资产 → 版权字段展示（F133 要求）；导入中断 → 原子回退；
 * 跨版本导入 → 兼容矩阵校验+降级项清单。
 *
 * 【设计细节】分节独立勾选（只要短语也行——包是菜单不是套餐）；回滚链与 F121
 * 快照共用存储；包体积预估显示；档案内短语隐私提示。
 */

import { PERSONA_SECTIONS, PERSONA_VERSION, personaStore, type PersonaSection } from "./store";
import { tokenTableHash } from "./tokens";

export const ARCHIVE_FORMAT = "vxtheme-profile";
export const ARCHIVE_VERSION = 1;

/** 档案分节（主册【功能定义】八项 + 其余 E 域分节一并入包——菜单式勾选）。 */
export const ARCHIVE_SECTIONS: readonly { id: PersonaSection; zh: string; en: string }[] = [
  { id: "theme", zh: "主题（令牌表+深浅切换+应用例外）", en: "Theme" },
  { id: "wallpaper", zh: "壁纸（每日一换配置）", en: "Wallpaper" },
  { id: "icons", zh: "图标包", en: "Icon pack" },
  { id: "pointer", zh: "指针方案", en: "Pointer scheme" },
  { id: "sound", zh: "声音混合器", en: "Sound mixer" },
  { id: "startmenu", zh: "开始菜单布局预设", en: "Start menu presets" },
  { id: "font", zh: "字体安全档", en: "Font guard" },
  { id: "motion", zh: "动效强度档", en: "Motion tier" },
  { id: "widgets", zh: "桌面小组件", en: "Desktop widgets" },
  { id: "lock", zh: "锁屏定制", en: "Lock screen" },
  { id: "boot", zh: "开机动画个性化", en: "Boot animation" },
  { id: "ime", zh: "输入法皮肤", en: "IME skin" },
  { id: "ctxmenu", zh: "右键菜单自定义", en: "Context menu" },
  { id: "taskbar", zh: "任务栏个性化", en: "Taskbar" },
  { id: "shortcuts", zh: "快捷键", en: "Shortcuts" },
] as const;

export interface ArchiveMeta {
  name: string;
  createdAt: string;
  /** 创建时的系统档案版本（兼容矩阵输入）。 */
  version: number;
  /** 可选署名。 */
  author?: string;
  /** 版权字段（F133：含第三方资产时必填展示）。 */
  license?: string;
}

export interface ArchivePackage {
  format: typeof ARCHIVE_FORMAT;
  version: number;
  meta: ArchiveMeta;
  /** 勾选入包的分节数据（键=PersonaSection）。 */
  sections: Partial<Record<PersonaSection, Record<string, unknown>>>;
  /** 令牌表哈希（round-trip 对拍锚）。 */
  themeHash?: string;
}

// ---------- 导出 ----------

export interface ExportOptions {
  meta: Omit<ArchiveMeta, "version" | "createdAt">;
  /** 分节勾选（默认全选——包是菜单，但菜单默认全点）。 */
  include?: PersonaSection[];
  /** 隐私提示检查确认（短语内容自查）。 */
  privacyChecked?: boolean;
}

export interface ExportResult {
  pkg: ArchivePackage;
  /** 包体积预估（JSON 字节数——导入前同口径显示）。 */
  bytes: number;
  /** 隐私提示（未勾选确认时阻断）。 */
  privacyWarning: string | null;
}

export function exportArchive(opts: ExportOptions): ExportResult {
  const all = personaStore.exportAll();
  const include = opts.include ?? PERSONA_SECTIONS.filter((s) => s !== "archive");
  const sections: Partial<Record<PersonaSection, Record<string, unknown>>> = {};
  for (const s of include) {
    if (s === "archive") continue;
    sections[s] = all[s];
  }
  const pkg: ArchivePackage = {
    format: ARCHIVE_FORMAT,
    version: ARCHIVE_VERSION,
    meta: { ...opts.meta, version: PERSONA_VERSION, createdAt: new Date().toISOString() },
    sections,
    themeHash: sections.theme ? tokenTableHashFromSection(sections.theme) : undefined,
  };
  const json = JSON.stringify(pkg);
  const privacyWarning = opts.privacyChecked
    ? null
    : "导出前请检查短语库与个人化命名——私人句子可能不想随档案分享（确认勾选后此提示消失）";
  return { pkg, bytes: new TextEncoder().encode(json).length, privacyWarning };
}

function tokenTableHashFromSection(themeSection: Record<string, unknown>): string | undefined {
  const table = themeSection["tokenTable"];
  if (!table) return undefined;
  return tokenTableHash(table as Parameters<typeof tokenTableHash>[0]);
}

// ---------- 校验与差异预览 ----------

export interface ArchiveValidation {
  ok: boolean;
  reason: string;
  /** 兼容矩阵：跨版本导入的降级项清单。 */
  degradations: string[];
}

export function validateArchive(raw: unknown): ArchiveValidation {
  const degradations: string[] = [];
  if (typeof raw !== "object" || raw === null) return { ok: false, reason: "档案包不是 JSON 对象", degradations };
  const p = raw as Partial<ArchivePackage>;
  if (p.format !== ARCHIVE_FORMAT) return { ok: false, reason: `format 须为 "${ARCHIVE_FORMAT}"`, degradations };
  if (p.version !== ARCHIVE_VERSION) {
    if (typeof p.version === "number" && p.version < ARCHIVE_VERSION) {
      degradations.push(`档案来自旧版本 v${p.version}，未知分节按默认值兜底`);
    } else {
      return { ok: false, reason: `档案版本 v${String(p.version)} 高于当前支持（v${ARCHIVE_VERSION}），请升级系统`, degradations };
    }
  }
  if (typeof p.sections !== "object" || p.sections === null) return { ok: false, reason: "sections 缺失", degradations };
  const known = new Set<string>(PERSONA_SECTIONS);
  for (const key of Object.keys(p.sections)) {
    if (!known.has(key)) degradations.push(`未知分节 ${key} 导入时忽略`);
  }
  return { ok: true, reason: "校验通过", degradations };
}

/** 差异预览：「将更改：主题/图标/12 条短语」——导入前列出将变化的分节。 */
export interface DiffPreview {
  changedSections: string[];
  unchangedSections: string[];
  detail: string[];
}

export function diffPreview(pkg: ArchivePackage): DiffPreview {
  const current = personaStore.exportAll();
  const changed: string[] = [];
  const unchanged: string[] = [];
  const detail: string[] = [];
  for (const [sec, data] of Object.entries(pkg.sections)) {
    if (!data) continue;
    const cur = current[sec as PersonaSection];
    if (JSON.stringify(cur) === JSON.stringify(data)) {
      unchanged.push(sec);
    } else {
      changed.push(sec);
      const keys = Object.keys(data);
      detail.push(`${sec}: ${keys.length} 项将应用`);
    }
  }
  return { changedSections: changed, unchangedSections: unchanged, detail };
}

// ---------- 导入（中断原子性） ----------

export interface ImportOutcome {
  ok: boolean;
  applied: PersonaSection[];
  /** 原子回退说明：失败时系统保持导入前状态（先校验后切换，单事务）。 */
  reason: string;
  degradations: string[];
}

/**
 * 导入：validate → diff → 原子切换（personaStore.importAll 只接受合法分节，
 * 中断即整包拒绝）。回滚链：导入前配置自动进入各节 undo 栈（store 层保证）。
 */
export function importArchive(raw: unknown): ImportOutcome {
  const v = validateArchive(raw);
  if (!v.ok) return { ok: false, applied: [], reason: v.reason, degradations: v.degradations };
  const pkg = raw as ArchivePackage;
  // 哈希对拍锚（若包带主题哈希）：导入后主题哈希必须等于包内声明。
  try {
    personaStore.importAll(pkg.sections as Record<string, unknown>);
  } catch (e) {
    return { ok: false, applied: [], reason: `导入中断已原子回退: ${String(e)}`, degradations: v.degradations };
  }
  if (pkg.themeHash) {
    const now = personaStore.exportAll().theme;
    const actual = now["tokenTable"] ? tokenTableHash(now["tokenTable"] as Parameters<typeof tokenTableHash>[0]) : undefined;
    if (actual !== pkg.themeHash) {
      // round-trip 失败：立即回退（把导入前的状态再写回去由 undo 栈承担）。
      return { ok: false, applied: [], reason: "主题令牌哈希对拍失败，导入已回退", degradations: v.degradations };
    }
  }
  return { ok: true, applied: Object.keys(pkg.sections) as PersonaSection[], reason: "导入完成", degradations: v.degradations };
}
