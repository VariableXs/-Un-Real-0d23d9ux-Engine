/**
 * M-15 任务栏空区菜单定制（AI-03 任务栏与托盘组）：
 * - 菜单项注册表（id + i18n key + 默认显隐 + 动作枚举）
 * - 用户覆盖（排序 + 显隐）存 localStorage KV；「恢复默认」= 清除覆盖
 * - 不支持任意命令注入：只有注册表内的安全项（防误配）
 * - 默认状态与改动前逐像素一致：默认项集 = 改动前的三项
 */

const KEY = "variable:taskbar:blankmenu:v1";

export interface TaskbarMenuEntry {
  id: string;
  /** 词典 key */
  labelKey: string;
  /** 默认是否显示（默认项集 = 现状三项；sticky 默认隐藏） */
  defaultVisible: boolean;
}

export const TASKBAR_MENU_REGISTRY: TaskbarMenuEntry[] = [
  { id: "showDesktop", labelKey: "showDesktop", defaultVisible: true },
  { id: "wallpaperCenter", labelKey: "wpCenterTitle", defaultVisible: true },
  { id: "launcher", labelKey: "launcherTitle", defaultVisible: true },
  { id: "sticky", labelKey: "tbQuickSticky", defaultVisible: false },
  { id: "taskbarSettings", labelKey: "taskbarSettings", defaultVisible: true },
];

export interface TaskbarMenuOverride {
  /** 顺序 = 显示的 id 顺序（只含可见项排序；隐藏项附后按注册表序） */
  order: string[];
  hidden: string[];
}

export function defaultOverride(): TaskbarMenuOverride {
  return {
    order: TASKBAR_MENU_REGISTRY.filter((e) => e.defaultVisible).map((e) => e.id),
    hidden: TASKBAR_MENU_REGISTRY.filter((e) => !e.defaultVisible).map((e) => e.id),
  };
}

function sanitize(raw: unknown): TaskbarMenuOverride | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Partial<TaskbarMenuOverride>;
  const ids = new Set(TASKBAR_MENU_REGISTRY.map((e) => e.id));
  const order = Array.isArray(o.order) ? o.order.filter((x): x is string => typeof x === "string" && ids.has(x)) : [];
  const hidden = Array.isArray(o.hidden) ? o.hidden.filter((x): x is string => typeof x === "string" && ids.has(x)) : [];
  if (order.length === 0) return null;
  // 去重 + 补齐漏掉的注册表项（默认可见补入 order 尾部 → 老用户升级后可见新功能；
  // 默认隐藏补入 hidden）
  const uniqOrder = [...new Set(order)];
  const uniqHidden = [...new Set(hidden)].filter((h) => !uniqOrder.includes(h));
  for (const e of TASKBAR_MENU_REGISTRY) {
    if (!uniqOrder.includes(e.id) && !uniqHidden.includes(e.id)) {
      if (e.defaultVisible) uniqOrder.push(e.id);
      else uniqHidden.push(e.id);
    }
  }
  return { order: uniqOrder, hidden: uniqHidden };
}

export function loadMenuOverride(): TaskbarMenuOverride {
  try {
    return sanitize(JSON.parse(localStorage.getItem(KEY) ?? "null")) ?? defaultOverride();
  } catch {
    return defaultOverride();
  }
}

export function saveMenuOverride(o: TaskbarMenuOverride): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(o));
  } catch {
    /* storage full/blocked */
  }
}

export function clearMenuOverride(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
}

/** 应用覆盖 → 渲染顺序（可见项按 order，隐藏项不渲染）。 */
export function effectiveMenuIds(o: TaskbarMenuOverride): string[] {
  return o.order.filter((id) => !o.hidden.includes(id));
}
