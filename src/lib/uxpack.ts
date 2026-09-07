/**
 * X-2：`.uxpack.json` Schema v1（蓝图附录 I）与权限模型 —— 前端口径。
 * Rust 侧 shell/extensions.rs 为权威校验；此处镜像用于设置页展示与单测。
 * 权限枚举（精确粒度，storage/net/vault 支持作用域后缀）。
 */

export const UXPACK_VERSION = 1;

export type UxpackType = "web" | "plugin" | "external";

export interface UxpackManifest {
  id: string;
  name: string;
  version: string;
  type: UxpackType;
  permissions: string[];
  entry: string;
  csp?: string | null;
  signature?: string | null;
  description?: string | null;
}

/** 人话解释（安装确认 UI 用，复用 netconsent 确认卡视觉）。 */
export const PERMISSION_HINTS: Record<string, string> = {
  widget: "在桌面创建小组件",
  window: "打开自定义窗口",
  "events:subscribe": "订阅系统事件（启动/通知/壁纸变更等）",
  storage: "读写自己的加密存储",
  vault: "读取金库条目",
  net: "访问指定域名（走白名单代理）",
  notify: "发送桌面通知",
  "layout:control": "查看与应用布局快照",
  "hardware:read": "读取硬件状态摘要（只读）",
  "theme:patch": "修改界面主题局部样式",
  "aihub:invoke": "唤起 AI Hub 工具",
};

/**
 * 权限 root 判定（与 Rust 侧一致）：
 * `root` 只授予 root 本身；`root:action:scope` 授予 `root:action`；`root:*` 授予全部。
 */
export function hasPermission(permissions: string[], need: string): boolean {
  const [nr, ns] = need.split(":");
  if (!ns) return permissions.includes(need);
  return permissions.some((p) => {
    const i = p.indexOf(":");
    if (i < 0) return false;
    const pr = p.slice(0, i);
    const ps = p.slice(i + 1);
    if (pr !== nr || ps === "*none") return false;
    const j = ps.indexOf(":");
    return j >= 0 ? ps.slice(0, j) === ns : ps === "*" || ps === ns;
  });
}

/** Schema v1 校验；返回错误文案数组（空 = 通过）。 */
export function validateManifest(m: Partial<UxpackManifest>): string[] {
  const errs: string[] = [];
  if (!m.id || !/^[a-z0-9-]+$/.test(m.id)) errs.push("id 只允许小写字母/数字/连字符");
  if (!m.name) errs.push("name 不能为空");
  if (!m.version) errs.push("version 不能为空");
  if (m.type !== "web" && m.type !== "plugin" && m.type !== "external") {
    errs.push("type 必须是 web / plugin / external");
  }
  if (!m.entry) errs.push("entry 不能为空");
  const allowedRoots = [
    "widget", "window", "events", "storage", "vault",
    "net", "notify", "layout", "hardware", "theme", "aihub",
  ];
  for (const p of m.permissions ?? []) {
    if (!allowedRoots.includes(p.split(":")[0] ?? "")) errs.push(`未知权限: ${p}`);
  }
  return errs;
}
