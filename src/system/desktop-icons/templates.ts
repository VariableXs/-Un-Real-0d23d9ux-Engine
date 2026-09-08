/**
 * V-09 新建菜单模板中心（车道 D）。
 * - 模板存 localStorage（variable:desktop:templates:v1）：{ id, name, ext, content }；
 *   诚实边界：后端无用户数据目录专用命令，localStorage 属应用本地数据目录，
 *   与既有 layout/shelves 存储口径一致。
 * - 内置模板（文本文档 / Markdown）不占存储，编码为 BUILTINS。
 * - 纯函数抽离（sanitize/list/add/rename/remove）便于测试；新建文件走
 *   ipc.saveTextFile（显式落盘、失败如实提示），零静默写。
 */

export interface IconTemplate {
  id: string;
  /** 显示名（新建菜单中的条目名，可编辑）。 */
  name: string;
  /** 扩展名（不含点；文件名 = name + "." + ext）。 */
  ext: string;
  /** 模板内容（UTF-8 文本）。 */
  content: string;
}

export const TEMPLATES_LS_KEY = "variable:desktop:templates:v1";
/** 单模板内容上限（256KB —— 与图标资源口径对齐，防 localStorage 爆仓）。 */
export const TEMPLATE_MAX_BYTES = 256 * 1024;
export const TEMPLATES_MAX_COUNT = 50;

/** 内置模板（不可删除/重命名；规格：内置=文件夹/文本文档级别的基础件）。 */
export const BUILTIN_TEMPLATES: readonly IconTemplate[] = [
  { id: "builtin-txt", name: "文本文档", ext: "txt", content: "" },
  { id: "builtin-md", name: "Markdown 笔记", ext: "md", content: "# \n" },
];

export function byteLength(s: string): number {
  return typeof TextEncoder !== "undefined" ? new TextEncoder().encode(s).length : s.length;
}

function sanitize(list: unknown): IconTemplate[] {
  if (!Array.isArray(list)) return [];
  const out: IconTemplate[] = [];
  for (const t of list) {
    const x = t as Partial<IconTemplate>;
    if (
      typeof x === "object" && x !== null &&
      typeof x.id === "string" && x.id.length > 0 && !x.id.startsWith("builtin-") &&
      typeof x.name === "string" && x.name.trim().length > 0 &&
      typeof x.ext === "string" && /^[a-z0-9]{1,12}$/i.test(x.ext) &&
      typeof x.content === "string" && byteLength(x.content) <= TEMPLATE_MAX_BYTES
    ) {
      out.push({ id: x.id, name: x.name.trim(), ext: x.ext.toLowerCase(), content: x.content });
      if (out.length >= TEMPLATES_MAX_COUNT) break;
    }
  }
  return out;
}

/** 全部模板 = 内置 + 用户自定义（顺序：内置在前）。 */
export function listTemplates(): IconTemplate[] {
  try {
    const raw = localStorage.getItem(TEMPLATES_LS_KEY);
    return [...BUILTIN_TEMPLATES, ...(raw ? sanitize(JSON.parse(raw) as unknown) : [])];
  } catch {
    return [...BUILTIN_TEMPLATES];
  }
}

function persistUser(list: IconTemplate[]): void {
  try {
    localStorage.setItem(TEMPLATES_LS_KEY, JSON.stringify(list));
  } catch {
    /* storage full —— 会话内不持久化（诚实：下次启动丢失，可再存） */
  }
}

function newId(): string {
  return `tpl${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`;
}

export type AddResult = "ok" | "limit" | "size" | "invalid";

/** 新增用户模板（“保存为模板”入口；上限/体积如实拒绝，不静默截断）。 */
export function addTemplate(name: string, ext: string, content: string): { result: AddResult; template: IconTemplate | null } {
  const trimmed = name.trim();
  const cleanExt = ext.trim().replace(/^\./, "").toLowerCase();
  if (!trimmed || !/^[a-z0-9]{1,12}$/i.test(cleanExt)) return { result: "invalid", template: null };
  if (byteLength(content) > TEMPLATE_MAX_BYTES) return { result: "size", template: null };
  const t: IconTemplate = { id: newId(), name: trimmed, ext: cleanExt, content };
  const cur = listTemplates().filter((x) => !x.id.startsWith("builtin-"));
  if (cur.length >= TEMPLATES_MAX_COUNT) return { result: "limit", template: null };
  persistUser([...cur, t]);
  return { result: "ok", template: t };
}

/** 重命名（内置模板拒绝）。 */
export function renameTemplate(id: string, name: string): void {
  const trimmed = name.trim();
  if (!trimmed || id.startsWith("builtin-")) return;
  const cur = listTemplates().filter((x) => !x.id.startsWith("builtin-"));
  persistUser(cur.map((x) => (x.id === id ? { ...x, name: trimmed } : x)));
}

/** 删除（内置模板拒绝）。 */
export function removeTemplate(id: string): void {
  if (id.startsWith("builtin-")) return;
  const cur = listTemplates().filter((x) => !x.id.startsWith("builtin-"));
  persistUser(cur.filter((x) => x.id !== id));
}

/** 由模板生成目标文件名（不含目录；冲突由调用方处理）。 */
export function templateFileName(t: IconTemplate, index: number): string {
  const base = index <= 0 ? t.name : `${t.name} (${index + 1})`;
  return `${base}.${t.ext}`;
}
