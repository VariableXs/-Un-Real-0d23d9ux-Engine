/**
 * F167/F168 导航深化 · 菜单键盘导航模型 + 托盘溢出计算。
 *
 * 主册判据延伸：
 * - F167「键盘全程可达」——右键菜单的键盘模型：方向键在层级间穿行
 *   （右进子菜单/左回父级）、首尾回绕、字母跳转（type-ahead）；
 * - F167「关键项锁定」的导航侧：锁定项跳过但不跳焦——可见可读不可激活；
 * - F168「托盘折叠联动正确」——溢出计算：可见容量 → 折叠顺序（最少
 *   使用优先）→ 展开面板的网格布局，全部几何实算。
 */

// ---------- 菜单树模型 ----------

export interface MenuItem {
  id: string;
  label: string;
  /** 键盘加速器字母（type-ahead 与 Alt 加速共用）。 */
  accelerator?: string;
  disabled?: boolean;
  locked?: boolean;
  children?: MenuItem[];
  /** 危险项（删除类——红显但可用）。 */
  danger?: boolean;
}

export interface FlatItem {
  item: MenuItem;
  /** 层级路径（根=0）。 */
  depth: number;
  /** 兄弟序号。 */
  siblingIndex: number;
  parentPath: string[];
}

/** 菜单树扁平化（导航模型的物理底——深度优先，顺序即视觉顺序）。 */
export function flattenMenu(items: MenuItem[]): FlatItem[] {
  const out: FlatItem[] = [];
  const walk = (list: MenuItem[], depth: number, parentPath: string[]): void => {
    list.forEach((item, i) => {
      out.push({ item, depth, siblingIndex: i, parentPath });
      if (item.children && item.children.length > 0) walk(item.children, depth + 1, [...parentPath, item.id]);
    });
  };
  walk(items, 0, []);
  return out;
}

export type MenuNavAction =
  | { type: "down" }
  | { type: "up" }
  | { type: "enter" }
  | { type: "left" }
  | { type: "right" }
  | { type: "home" }
  | { type: "end" }
  | { type: "type-ahead"; ch: string };

export type MenuNavResult =
  | { type: "focus"; path: string[] }
  | { type: "submenu-open"; path: string[] }
  | { type: "submenu-close"; path: string[] }
  | { type: "activate"; path: string[] }
  | { type: "ignored" }
  | { type: "wrap"; path: string[] };

/**
 * 菜单键盘导航（纯函数状态机——当前焦点路径 + 动作 → 结果）：
 * - 可聚焦 = !disabled（locked 可聚焦但不可激活——锁定项可见可读不可点）；
 * - 首尾回绕（down 在末尾回第一项——循环导航是桌面惯例）；
 * - type-ahead：加速器字母匹配跳转（同字母循环——连按同一字母在命中项间轮转）。
 */
export function menuNavigate(items: MenuItem[], focusPath: string[] | null, action: MenuNavAction): MenuNavResult {
  const flat = flattenMenu(items);
  const focusable = flat.filter((f) => !f.item.disabled);
  if (focusable.length === 0) return { type: "ignored" };

  const findFlat = (path: string[]): FlatItem | undefined => {
    const id = path[path.length - 1];
    return flat.find((f) => f.item.id === id);
  };
  const pathOf = (f: FlatItem): string[] => [...f.parentPath, f.item.id];
  const indexOfFocus = focusPath ? focusable.findIndex((f) => f.item.id === focusPath[focusPath.length - 1]) : -1;

  switch (action.type) {
    case "down":
    case "up": {
      const step = action.type === "down" ? 1 : -1;
      let next = indexOfFocus + step;
      if (next < 0) next = focusable.length - 1; // 回绕。
      if (next >= focusable.length) next = 0;
      const target = focusable[next]!;
      const res: MenuNavResult = { type: next === indexOfFocus ? "ignored" : "focus", path: pathOf(target) };
      return next === indexOfFocus ? { type: "wrap", path: pathOf(target) } : res;
    }
    case "right": {
      if (!focusPath) return { type: "ignored" };
      const cur = findFlat(focusPath);
      if (cur?.item.children && cur.item.children.some((c) => !c.disabled)) {
        const firstChild = cur.item.children.find((c) => !c.disabled)!;
        return { type: "submenu-open", path: [...pathOf(cur), firstChild.id] };
      }
      return { type: "ignored" };
    }
    case "left": {
      if (!focusPath || focusPath.length <= 1) return { type: "ignored" };
      return { type: "submenu-close", path: focusPath.slice(0, -1) };
    }
    case "enter": {
      if (!focusPath) return { type: "ignored" };
      const cur = findFlat(focusPath);
      if (!cur) return { type: "ignored" };
      if (cur.item.locked) return { type: "ignored" }; // 锁定项 Enter 不激活（防自残——锁定语义一致）。
      if (cur.item.disabled) return { type: "ignored" };
      if (cur.item.children && cur.item.children.length > 0) {
        const firstChild = cur.item.children.find((c) => !c.disabled);
        return firstChild ? { type: "submenu-open", path: [...pathOf(cur), firstChild.id] } : { type: "ignored" };
      }
      return { type: "activate", path: pathOf(cur) };
    }
    case "home":
      return { type: "focus", path: pathOf(focusable[0]!) };
    case "end":
      return { type: "focus", path: pathOf(focusable[focusable.length - 1]!) };
    case "type-ahead": {
      const ch = action.ch.toLowerCase();
      // 从焦点下一项开始循环找加速器（同字母循环轮转）。
      const start = indexOfFocus + 1;
      for (let i = 0; i < focusable.length; i++) {
        const f = focusable[(start + i) % focusable.length]!;
        if (f.item.accelerator?.toLowerCase() === ch) return { type: "focus", path: pathOf(f) };
      }
      return { type: "ignored" };
    }
  }
}

/** 菜单可聚焦审计（F215 层级规范联动——超两层的路径清单）。 */
export function auditMenuDepth(items: MenuItem[], maxDepth = 2): { path: string[]; depth: number }[] {
  const flat = flattenMenu(items);
  return flat.filter((f) => f.depth + 1 > maxDepth).map((f) => ({ path: [...f.parentPath, f.item.id], depth: f.depth + 1 }));
}

// ---------- 托盘溢出计算（F168 折叠联动的几何面） ----------

export interface TrayIcon {
  id: string;
  /** 图标宽 px。 */
  w: number;
  /** 最近使用时间（折叠顺序依据——最少使用先进折叠区）。 */
  lastUsedAt: number;
  /** 用户钉选（钉选永不折叠）。 */
  pinned: boolean;
}

export interface TrayLayout {
  visible: TrayIcon[];
  overflow: TrayIcon[];
  /** 可见区末端 x（溢出展开钮的位置）。 */
  chevronX: number;
  /** 展开面板网格：列数/行数/格子尺寸。 */
  panel: { cols: number; rows: number; cellPx: number };
}

const TRAY_CELL = 32;
const TRAY_GAP = 6;
const CHEVRON_W = 20;
const OVERFLOW_CELL = 40;

/**
 * 托盘布局：容量内按「钉选优先 → 最近使用降序」放可见区；
 * 超容量的（非钉选中）最少使用先进溢出。钉选图标容量不足时
 * 显性报告（不静默折叠钉选——钉选是承诺）。
 */
export function layoutTray(icons: TrayIcon[], visibleWidthPx: number): TrayLayout & { pinnedOverflow: string[] } {
  const capacity = Math.floor((visibleWidthPx - CHEVRON_W + TRAY_GAP) / (TRAY_CELL + TRAY_GAP));
  const pinned = icons.filter((i) => i.pinned).sort((a, b) => b.lastUsedAt - a.lastUsedAt);
  const rest = icons.filter((i) => !i.pinned).sort((a, b) => b.lastUsedAt - a.lastUsedAt);
  const visible: TrayIcon[] = [];
  const overflow: TrayIcon[] = [];
  const pinnedOverflow: string[] = [];
  const slots = Math.max(0, capacity);
  let used = 0;
  for (const p of pinned) {
    if (used < slots) {
      visible.push(p);
      used++;
    } else {
      overflow.push(p);
      pinnedOverflow.push(p.id); // 钉选被挤出 = 缺陷级信号（显性化，不静默）。
    }
  }
  for (const r of rest) {
    if (used < slots) {
      visible.push(r);
      used++;
    } else {
      overflow.push(r);
    }
  }
  // 溢出面板网格：列数随数量自适应（最多 4 列——面板不摊大饼）。
  const cols = Math.min(4, Math.max(1, Math.ceil(Math.sqrt(overflow.length))));
  const rows = Math.ceil(Math.max(1, overflow.length) / cols);
  return {
    visible,
    overflow,
    chevronX: used * (TRAY_CELL + TRAY_GAP),
    panel: { cols, rows, cellPx: OVERFLOW_CELL },
    pinnedOverflow,
  };
}

/** 托盘点击穿透热区（C-6 三件套 <100ms 响应的热区数学）。 */
export function trayHitIndex(x: number, iconCount: number): number | null {
  const idx = Math.floor(x / (TRAY_CELL + TRAY_GAP));
  const inGap = x % (TRAY_CELL + TRAY_GAP) > TRAY_CELL;
  if (idx < 0 || idx >= iconCount || inGap) return null;
  return idx;
}
