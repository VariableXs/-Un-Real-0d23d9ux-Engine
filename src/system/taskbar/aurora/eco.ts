/**
 * AURORA-10000 领域04 · 族0098 开始菜单应用生态（AI-20 批次，勿删）。
 * 已装应用登记/分类/启动参数/权限/统计/关联与自启管理（数据模型层）。
 */

export interface AppRecord {
  id: string;
  name: string;
  category: string;
  /** 便携登记（F02434）。 */
  portable: boolean;
  /** 启动参数（F02436）。 */
  args: string;
  permissions: readonly string[];
  launchMs: number;
  crashes: number;
  useMinutes: number;
  /** 开机自启（F02449）。 */
  autostart: boolean;
}

/** 分类自动归组（F02432）：关键词规则。 */
const CATEGORY_RULES: ReadonlyArray<{ re: RegExp; category: string }> = [
  { re: /code|editor|dev/i, category: "开发" },
  { re: /explorer|vault|system/i, category: "系统" },
  { re: /write|mind|note/i, category: "效率" },
  { re: /music|player|media/i, category: "影音" },
];

export function autoCategory(name: string): string {
  for (const r of CATEGORY_RULES) if (r.re.test(name)) return r.category;
  return "其他";
}

/** 启动速度排行（F02439）与崩溃榜（F02440）。 */
export function launchRanking(apps: readonly AppRecord[], n = 5): AppRecord[] {
  return [...apps].sort((a, b) => b.launchMs - a.launchMs).slice(0, n);
}
export function crashRanking(apps: readonly AppRecord[], n = 5): AppRecord[] {
  return [...apps].filter((a) => a.crashes > 0).sort((a, b) => b.crashes - a.crashes).slice(0, n);
}

/** 权限总览（F02438）：返回 应用 → 权限 表。 */
export function permissionMatrix(apps: readonly AppRecord[]): Map<string, readonly string[]> {
  return new Map(apps.map((a) => [a.id, a.permissions]));
}

/** 深度卸载清单（F02429）：主体 + 常见残留路径。 */
export function uninstallPlan(app: AppRecord): string[] {
  return [
    `app:${app.id}`,
    `%APPDATA%/${app.id}`,
    `%LOCALAPPDATA%/${app.id}`,
    `%PROGRAMDATA%/${app.id}`,
  ];
}

/** 文件关联（F02446）与协议关联（F02447）。 */
const extAssoc = new Map<string, string>();
export function setExtAssoc(ext: string, appId: string): void { extAssoc.set(ext.replace(/^\./, ""), appId); }
export function getExtAssoc(ext: string): string | undefined { return extAssoc.get(ext.replace(/^\./, "")); }
const protoAssoc = new Map<string, string>();
export function setProtoAssoc(proto: string, appId: string): void { protoAssoc.set(proto, appId); }
export function getProtoAssoc(proto: string): string | undefined { return protoAssoc.get(proto); }

/** 自启管理（F02449）：切换。 */
export function toggleAutostart(apps: readonly AppRecord[], id: string): AppRecord[] {
  return apps.map((a) => (a.id === id ? { ...a, autostart: !a.autostart } : a));
}
