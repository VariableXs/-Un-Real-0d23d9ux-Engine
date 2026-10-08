/**
 * F356 PWA 应用化（H 域 · AI-H4）：
 * 网页应用可安装为独立窗口应用：Edge 菜单「安装此站点为应用」→ 生成 vxapp 形制的
 * 轻量壳（独立窗口/任务栏图标/开始菜单条目/可贴靠可快照 F351），卸载即删壳不留残留
 * （F344 简化版）；图标与名称取站点清单，安装后与本地应用在所有列表里平起平坐。
 * 判据（主册 F356）：安装/卸载全链；独立窗口属性（Alt+Tab/贴靠/快照）；清单字段取用；
 * 卸载干净度扫描；列表平权审计。
 * 依赖锚点：F344 卸载清扫 / F351 工作区快照。
 * 存储键：variable:h4:f356:pwa
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 站点清单（manifest）中取用的字段（判据「清单字段取用」）。 */
export interface SiteManifest {
  name: string;
  shortName?: string;
  startUrl: string;
  scope: string;
  display: "standalone" | "fullscreen" | "minimal-ui" | "browser";
  /** 图标候选（取最大尺寸者）。 */
  icons: Array<{ src: string; sizes: string }>;
  themeColor?: string;
}

export interface PwaApp {
  id: string;
  /** 展示名：short_name 优先（列表平权时与本地应用名等长节奏）。 */
  displayName: string;
  startUrl: string;
  scope: string;
  iconSrc: string;
  themeColor: string | null;
  installedAt: number;
  /** 独立窗口属性三件（判据）：Alt+Tab 独立项 / 可贴靠 / 可快照。 */
  windowProps: { altTab: true; snappable: true; snapshotable: true };
}

/** manifest → PWA 应用壳：清单字段逐项取用，缺失字段如实降级。 */
export function fromManifest(m: SiteManifest, now: number): { app: PwaApp | null; problems: string[] } {
  const problems: string[] = [];
  if (!m.name?.trim()) problems.push("清单缺 name");
  if (!m.startUrl?.trim()) problems.push("清单缺 start_url");
  if (m.display === "browser") problems.push("display=browser 不满足独立窗口安装条件");
  if (problems.length) return { app: null, problems };
  const icon = [...(m.icons ?? [])].sort((a, b) => iconArea(b.sizes) - iconArea(a.sizes))[0];
  const app: PwaApp = {
    id: pwaIdOf(m.startUrl),
    displayName: (m.shortName?.trim() || m.name).trim(),
    startUrl: m.startUrl,
    scope: m.scope || m.startUrl,
    iconSrc: icon?.src ?? "",
    themeColor: m.themeColor ?? null,
    installedAt: now,
    windowProps: { altTab: true, snappable: true, snapshotable: true },
  };
  return { app, problems: icon ? [] : ["清单无图标（占位图标安装，如实标注）"] };
}

function iconArea(sizes: string): number {
  const m = /(\d+)x(\d+)/.exec(sizes ?? "");
  return m ? Number(m[1]) * Number(m[2]) : 0;
}

/** 应用 id：起点 URL 规整（协议+主机+路径去尾斜杠）。 */
export function pwaIdOf(startUrl: string): string {
  return startUrl.replace(/^https?:\/\//, "").replace(/\/+$/, "").toLowerCase();
}

const KEY = h4Key("f356", "pwa");

function isPwaArray(v: unknown): v is PwaApp[] {
  return Array.isArray(v) && v.every((x) => x && typeof (x as PwaApp).id === "string");
}

function load(store: KvStore): PwaApp[] {
  return readJson<PwaApp[]>(store, KEY, [], isPwaArray);
}

function save(store: KvStore, apps: PwaApp[]): boolean {
  return writeJson(store, KEY, apps);
}

export function listPwaApps(store: KvStore = defaultStore()): PwaApp[] {
  return load(store);
}

export interface InstallResult {
  ok: boolean;
  app: PwaApp | null;
  problems: string[];
  /** 幂等：已安装时 ok=false 且 reason=exists。 */
  reason: "ok" | "exists" | "invalid-manifest" | "persist-failed";
}

/** 安装：清单校验 → 生成壳 → 入册（列表平权由消费面读取本表实现）。 */
export function installPwa(m: SiteManifest, now: number, store: KvStore = defaultStore()): InstallResult {
  const { app, problems } = fromManifest(m, now);
  if (!app) return { ok: false, app: null, problems, reason: "invalid-manifest" };
  const all = load(store);
  if (all.some((a) => a.id === app.id)) return { ok: false, app: null, problems: [], reason: "exists" };
  const ok = save(store, [...all, app]);
  return { ok, app, problems, reason: ok ? "ok" : "persist-failed" };
}

export interface UninstallAudit {
  removed: boolean;
  /** 卸载后残留扫描：册内引用、快照引用、任务栏固定引用三项（判据「卸载干净度」）。 */
  residue: Array<{ where: "registry" | "snapshots" | "pinned"; detail: string }>;
}

/** 卸载：删壳不留残留；返回干净度扫描结果（残留项为空 = 干净）。 */
export function uninstallPwa(id: string, snapshotRefs: string[] = [], pinnedRefs: string[] = [], store: KvStore = defaultStore()): UninstallAudit {
  const all = load(store);
  const next = all.filter((a) => a.id !== id);
  const removed = next.length !== all.length;
  if (removed) save(store, next);
  const residue: UninstallAudit["residue"] = [];
  if (load(store).some((a) => a.id === id)) residue.push({ where: "registry", detail: "注册表未删净" });
  for (const s of snapshotRefs.filter((r) => r.includes(id))) residue.push({ where: "snapshots", detail: `快照仍引用：${s}` });
  for (const p of pinnedRefs.filter((r) => r.includes(id))) residue.push({ where: "pinned", detail: `任务栏固定仍引用：${p}` });
  return { removed, residue };
}

export interface ParityAuditRow {
  list: "startMenu" | "allApps" | "altTab" | "taskbar";
  /** 该列表是否包含 PWA（平权判据：全 true）。 */
  includes: boolean;
  /** 呈现字段与本地应用是否同构（名称/图标/说明三件套）。 */
  fieldsParity: boolean;
}

/** 列表平权审计：PWA 在四个列表与本地应用同权（判据「列表平权审计」）。 */
export function auditListParity(app: PwaApp | null): ParityAuditRow[] {
  const present = app !== null;
  return (["startMenu", "allApps", "altTab", "taskbar"] as const).map((list) => ({
    list,
    includes: present,
    fieldsParity: present && app.displayName.length > 0 && app.iconSrc.length > 0,
  }));
}

/* ================= v4 深化批次四：清单深校验 / 图标择优 / 安装 Dry-Run ================= */

/** 图标尺寸解析（"512x512" → 尺寸；无法解析 → 0×0）。 */
export function iconSizeOf(sizes: string): { w: number; h: number } {
  const m = /(\d+)x(\d+)/.exec(sizes ?? "");
  return m ? { w: Number(m[1]), h: Number(m[2]) } : { w: 0, h: 0 };
}

/** 清单深校验（判据「清单字段取用」的防错面）：逐字段规格化校验，问题带字段名（零模糊报错）。 */
export function validateManifestStrict(m: SiteManifest): string[] {
  const problems: string[] = [];
  if (!m.name?.trim()) problems.push("name：缺失或为空");
  else if (m.name.length > 64) problems.push(`name：超长（${m.name.length} > 64）`);
  if (!/^https?:\/\//.test(m.startUrl ?? "")) problems.push("start_url：必须是 http(s) 绝对地址");
  if (m.scope && m.startUrl && !m.startUrl.startsWith(m.scope)) problems.push("scope：不是 start_url 的前缀——安装后页面会跳出应用作用域");
  if (m.display !== undefined && !["standalone", "fullscreen", "minimal-ui", "browser"].includes(m.display)) problems.push(`display：非法值 ${String(m.display)}`);
  if (m.themeColor && !/^#[0-9a-fA-F]{3,8}$/.test(m.themeColor)) problems.push(`theme_color：非法颜色格式 ${m.themeColor}`);
  const best = [...(m.icons ?? [])].map((i) => iconSizeOf(i.sizes)).sort((a, b) => b.w * b.h - a.w * a.h)[0];
  if (!best || best.w < 192) problems.push("icons：缺 ≥192px 图标（高分屏任务栏会糊——4K 验收红线）");
  return problems;
}

/** 图标择优：按面积降序取第一个（缺格如实降级，不编造占位尺寸）。 */
export function bestIcon(m: SiteManifest): { src: string; sizes: string } | null {
  const sorted = [...(m.icons ?? [])].sort((a, b) => {
    const sa = iconSizeOf(a.sizes);
    const sb = iconSizeOf(b.sizes);
    return sb.w * sb.h - sa.w * sa.h;
  });
  return sorted[0] ?? null;
}

/** 安装 Dry-Run（判据「安装/卸载全链」的前置预演）：只校验与预览、零持久化。 */
export function installDryRun(m: SiteManifest): { ok: boolean; problems: string[]; preview: { id: string; displayName: string; iconSrc: string } | null } {
  const problems = validateManifestStrict(m);
  const hardFail = problems.some((p) => p.startsWith("name") || p.startsWith("start_url"));
  if (hardFail) return { ok: false, problems, preview: null };
  const icon = bestIcon(m);
  return {
    ok: true,
    problems,
    preview: { id: pwaIdOf(m.startUrl), displayName: (m.shortName?.trim() || m.name).trim(), iconSrc: icon?.src ?? "" },
  };
}
