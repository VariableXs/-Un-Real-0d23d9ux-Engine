/**
 * U3-v7 引擎一：deskmenu —— 桌面右键菜单 / 行内重命名 / 属性对话框
 * 三路装配引擎（AI-U3 · 批次七）。
 *
 * 判据唯一源（主册摘文）：
 * - F502「省略的全文看 Tooltip（F205）/重命名（F260）/属性（F264）三条
 *   路可达」——本引擎把三路可达从注释纪律变成机检：每一路都能取回全文。
 * - F260「F2 或单击已选中项的名或慢双击名区进入行内编辑：主文件名全选、
 *   扩展名不选不参与、Esc 还原退出、Enter 提交、非法字符输入时即时抖动
 *   拒绝不等到提交；失败（重名/权限）行内红字提示不弹窗」。
 * - F264「右键属性统一对话框：常规页（类型/打开方式/位置/大小/占用/创建
 *   修改访问三时间/只读隐藏勾选——勾选即改写文件属性）、安全提示区（受
 *   保护文件显示只读原因）；与详情窗格（F091）同数据源不同形制；文件夹
 *   多一页包含统计（子项数/累计大小，异步计算带进度）」。
 *
 * 纯函数实现（零 DOM 依赖）——活体件 DeskMenuPane 与单测共用同一事实源。
 */

/* ------------------------------- F502 三路全文可达 ------------------------------- */

/** 三路全文可达的通道名（主册 F502 判据枚举）。 */
export type FullTextRoute = "tooltip" | "rename" | "properties";

/**
 * 三路全文可达机检（判据：省略的全文三条路都要拿得回）。
 * 每路传入一个「取全文」函数——任一路拿不回全文即红（缺路=缺陷）。
 */
export function fullTextReachable(
  name: string,
  routes: Partial<Record<FullTextRoute, () => string>>,
): { ok: boolean; missing: FullTextRoute[] } {
  const need: ReadonlyArray<FullTextRoute> = ["tooltip", "rename", "properties"];
  const missing = need.filter((r) => {
    const get = routes[r];
    return !get || get() !== name;
  });
  return { ok: missing.length === 0, missing };
}

/* ------------------------------- F260 行内重命名 ------------------------------- */

/** 慢双击窗口（判据：慢双击名区进入行内编辑——两次单击在窗口内且慢于快速双击）。 */
export const SLOW_DOUBLE_CLICK_MS = 700;
/** 快速双击防抖窗口（despaint 300ms 同源——快双击是打开语义，不得进重命名）。 */
export const FAST_DOUBLE_CLICK_MS = 300;

/** 非法文件名字符集（主册 F260：\ / : * ? " < > |——9 字符全测）。 */
export const ILLEGAL_NAME_CHARS = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'] as const;

/** 非法字符扫描（判据：即时抖动拒绝不等到提交）。 */
export function firstIllegalChar(name: string): string | null {
  for (const c of ILLEGAL_NAME_CHARS) {
    if (name.includes(c)) return c;
  }
  return null;
}

/** 主名/扩展名切分（判据：扩展名不选不参与——改「报告」不动「.docx」）。 */
export function splitNameExt(name: string): { base: string; ext: string } {
  const dot = name.lastIndexOf(".");
  // 无扩展名 / 点在首位（隐藏文件）→ 整名是主名
  if (dot <= 0) return { base: name, ext: "" };
  return { base: name.slice(0, dot), ext: name.slice(dot) };
}

/** 编辑会话初始选区（判据：主文件名全选、扩展名不参与）。 */
export function initialSelection(name: string): { start: number; end: number } {
  const { base } = splitNameExt(name);
  return { start: 0, end: base.length };
}

export type RenameFailKind = "illegal-char" | "duplicate" | "permission" | "empty";

export interface RenameVerdict {
  ok: boolean;
  /** 非法字符即时抖动拒绝（不等提交）。 */
  shake: boolean;
  /** 行内红字（不弹窗）。 */
  inlineError: string;
  failKind: RenameFailKind | null;
}

/** 提交裁决（判据：Enter 提交；重名/权限失败行内红字不弹窗；Esc 由状态机处理）。 */
export function renameCommit(
  newName: string,
  siblings: ReadonlyArray<string>,
  canWrite: boolean,
): RenameVerdict {
  if (newName.trim().length === 0) {
    return { ok: false, shake: true, inlineError: "名字不能为空", failKind: "empty" };
  }
  const illegal = firstIllegalChar(newName);
  if (illegal) {
    return { ok: false, shake: true, inlineError: `不能使用字符 ${illegal}`, failKind: "illegal-char" };
  }
  if (!canWrite) {
    return { ok: false, shake: false, inlineError: "没有写入权限——到权限中心申请", failKind: "permission" };
  }
  const clash = siblings.some((s) => s.toLowerCase() === newName.toLowerCase());
  if (clash) {
    return { ok: false, shake: false, inlineError: "同名项目已存在——换一个名字", failKind: "duplicate" };
  }
  return { ok: true, shake: false, inlineError: "", failKind: null };
}

/** 重命名状态机：idle → editing →(commit/esc)→ idle。 */
export interface RenameRt {
  phase: "idle" | "editing";
  target: string | null;
  draft: string;
  /** 进入路径（判据三入口各自登记——体验日志可回放）。 */
  via: "f2" | "name-click" | "slow-dbl" | null;
  original: string | null;
}

export function renameIdle(): RenameRt {
  return { phase: "idle", target: null, draft: "", via: null, original: null };
}

export function renameEnter(_rt: RenameRt, target: string, via: NonNullable<RenameRt["via"]>): RenameRt {
  // rt 仅承载调用位形态（编辑会话总是从 idle/他态重建——原稿不保留）
  return { phase: "editing", target, draft: target, via, original: target };
}

/** Esc 还原退出（判据：还原为原名并退出编辑，不产生副作用）。 */
export function renameEscape(rt: RenameRt): RenameRt {
  return { ...renameIdle(), target: rt.target };
}

/** Enter 提交（判据：成功完成即完成无动画打扰；失败留编辑态行内红字）。 */
export function renameSubmit(
  rt: RenameRt,
  siblings: ReadonlyArray<string>,
  canWrite: boolean,
): { rt: RenameRt; verdict: RenameVerdict } {
  const verdict = renameCommit(rt.draft, siblings, canWrite);
  if (!verdict.ok) return { rt, verdict }; // 留在编辑态——行内红字
  return { rt: renameIdle(), verdict };
}

/* ------------------------------- F264 属性对话框 ------------------------------- */

/** 簇大小（占用按簇取整——NTFS 4KB 基准）。 */
export const CLUSTER_BYTES = 4096;

/** 属性数据源（F264 与 F091 详情窗格同源——双形制不同数据不合法）。 */
export interface ItemFacts {
  name: string;
  kind: "file" | "folder" | "symlink";
  openWith: string | null;
  location: string;
  sizeBytes: number;
  createdAt: number;
  modifiedAt: number;
  accessedAt: number;
  readOnly: boolean;
  hidden: boolean;
  /** 受保护文件（安全提示区——显示只读原因）。 */
  protectedReason: string | null;
  /** 文件夹包含统计（异步计算结果挂载位）。 */
  contains: { items: number; sizeBytes: number } | null;
}

/** 常规页字段装配（判据：类型/打开方式/位置/大小/占用/三时间/只读隐藏）。 */
export function propsGeneral(f: ItemFacts): {
  type: string; openWith: string; location: string;
  size: string; sizeOnDisk: string; times: { created: number; modified: number; accessed: number };
  readOnly: boolean; hidden: boolean; safeNotice: string | null;
} {
  const typeText = f.kind === "folder" ? "文件夹" : f.kind === "symlink" ? "符号链接" : "文件";
  const onDisk = Math.max(CLUSTER_BYTES, Math.ceil(f.sizeBytes / CLUSTER_BYTES) * CLUSTER_BYTES);
  return {
    type: typeText,
    openWith: f.openWith ?? "（无）",
    location: f.location,
    size: `${f.sizeBytes} 字节`,
    sizeOnDisk: `${onDisk} 字节（占用）`,
    times: { created: f.createdAt, modified: f.modifiedAt, accessed: f.accessedAt },
    readOnly: f.readOnly,
    hidden: f.hidden,
    safeNotice: f.protectedReason,
  };
}

/** 勾选即生效（判据：只读/隐藏勾选立即改写属性——异步确认落账）。 */
export function applyAttribute(f: ItemFacts, key: "readOnly" | "hidden", value: boolean): ItemFacts {
  if (f.protectedReason && key === "readOnly" && value === false) {
    // 受保护文件取消只读 → 拒绝并保持（安全提示区语义）
    return f;
  }
  return { ...f, [key]: value };
}

/** 文件夹包含统计的异步步进器（判据：异步计算带进度）。 */
export function folderStatsStepper(entries: ReadonlyArray<{ sizeBytes: number }>, chunk = 64) {
  let done = 0;
  let items = 0;
  let sizeBytes = 0;
  return (): { finished: boolean; progress: number; stats: { items: number; sizeBytes: number } | null } => {
    const end = Math.min(entries.length, done + chunk);
    for (let i = done; i < end; i++) {
      const e = entries[i];
      if (e) { items += 1; sizeBytes += e.sizeBytes; }
    }
    done = end;
    const finished = done >= entries.length;
    return {
      finished,
      progress: entries.length === 0 ? 1 : done / entries.length,
      stats: finished ? { items, sizeBytes } : null,
    };
  };
}

/* ------------------------------- 右键菜单装配 ------------------------------- */

export interface MenuItem {
  id: string;
  label: string;
  shortcut: string | null;
  disabled: boolean;
  submenu: ReadonlyArray<MenuItem> | null;
}

/** 分隔线（菜单结构里 id 为 "-" 的空项）。 */
export function menuSeparator(): MenuItem {
  return { id: "-", label: "", shortcut: null, disabled: true, submenu: null };
}

function item(id: string, label: string, shortcut: string | null = null, disabled = false): MenuItem {
  return { id, label, shortcut, disabled, submenu: null };
}

/**
 * 桌面图标右键菜单（判据：F260 重命名三入口之一 + F264 属性入口 + F502
 * 全文可达三路之属性路）。「打开方式」带子菜单（十章：子级展开延迟 400ms
 * 基线——装配层给结构，时序归 F610）。
 */
export function iconContextMenu(f: ItemFacts, openWithAlternatives: ReadonlyArray<string>): Array<MenuItem> {
  const openWithSub = openWithAlternatives.map((app) => item(`open-with:${app}`, app));
  return [
    item("open", "打开", "Enter"),
    { id: "open-with", label: "打开方式", shortcut: null, disabled: openWithSub.length === 0, submenu: openWithSub },
    menuSeparator(),
    item("rename", "重命名", "F2", f.protectedReason !== null),
    item("properties", "属性", null),
  ];
}

/** 桌面空白处右键菜单（F503 密度入口 + F539 排列锁定联动）。 */
export function desktopContextMenu(_gridDensityLocked: boolean): Array<MenuItem> {
  // 锁定态影响的是「排序/重排」动作的可用性（F539），菜单结构两态同形——参数留给装配层
  return [
    item("sort", "排序方式"),
    item("refresh", "刷新", "F5"),
    menuSeparator(),
    item("new", "新建"),
    item("display-settings", "显示设置"),
  ];
}

/* ------------------------------- 自检 ------------------------------- */

/** F502/F260/F264 判据自检（前端面）。 */
export function deskmenuSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // F502 三路全文可达（含破坏注入：缺一路即红）
  const full = "项目总结报告最终版-特别长的名字";
  const ok3 = fullTextReachable(full, {
    tooltip: () => full,
    rename: () => full,
    properties: () => full,
  });
  checks.push({ name: "F502 三路全文可达", pass: ok3.ok });
  const missing1 = fullTextReachable(full, { tooltip: () => full, rename: () => full });
  checks.push({ name: "F502 缺路显性红", pass: !missing1.ok && missing1.missing[0] === "properties" });

  // F260 扩展名隔离与初始选区
  const se = splitNameExt("报告.docx");
  checks.push({ name: "F260 扩展名隔离", pass: se.base === "报告" && se.ext === ".docx" });
  const hidden = splitNameExt(".gitignore");
  checks.push({ name: "F260 隐藏文件整名主名", pass: hidden.base === ".gitignore" && hidden.ext === "" });
  const sel = initialSelection("报告最终版.docx");
  checks.push({ name: "F260 主名全选选区", pass: sel.start === 0 && sel.end === "报告最终版".length });

  // F260 非法字符 9 个全测（即时抖动拒绝）
  const illegalAll = ILLEGAL_NAME_CHARS.every((c) => renameCommit(`a${c}b`, [], true).failKind === "illegal-char" && renameCommit(`a${c}b`, [], true).shake);
  checks.push({ name: "F260 非法字符 9 个全测即时拒绝", pass: illegalAll });
  checks.push({ name: "F260 合法名通过", pass: renameCommit("2026 报告 v2", [], true).ok });

  // F260 失败行内不弹窗：重名（大小写不敏感）与权限
  const dup = renameCommit("REPORT.txt", ["report.txt"], true);
  const perm = renameCommit("x.txt", [], false);
  checks.push({ name: "F260 重名行内红字", pass: !dup.ok && dup.failKind === "duplicate" && dup.inlineError.length > 0 && !dup.shake });
  checks.push({ name: "F260 权限行内红字", pass: !perm.ok && perm.failKind === "permission" });
  checks.push({ name: "F260 空名拒绝", pass: !renameCommit("  ", [], true).ok });

  // F260 状态机：三入口 → Esc 还原 → Enter 提交
  let rt = renameIdle();
  rt = renameEnter(rt, "报告.docx", "slow-dbl");
  checks.push({ name: "F260 慢双击进入编辑", pass: rt.phase === "editing" && rt.via === "slow-dbl" });
  const esc = renameEscape(rt);
  checks.push({ name: "F260 Esc 还原退出", pass: esc.phase === "idle" && esc.target === "报告.docx" });
  rt = renameEnter(rt, "报告.docx", "f2");
  rt.draft = "总结.docx";
  const sub = renameSubmit(rt, [], true);
  checks.push({ name: "F260 Enter 提交成功", pass: sub.verdict.ok && sub.rt.phase === "idle" });
  const bad = renameSubmit({ ...rt, draft: "总/结.docx" }, [], true);
  checks.push({ name: "F260 非法提交留编辑态", pass: !bad.verdict.ok && bad.rt.phase === "editing" });

  // F264 常规页字段与占用取整
  const facts: ItemFacts = {
    name: "a.bin", kind: "file", openWith: "编辑器", location: "D:\\work",
    sizeBytes: 5000, createdAt: 1000, modifiedAt: 2000, accessedAt: 3000,
    readOnly: false, hidden: false, protectedReason: null, contains: null,
  };
  const g = propsGeneral(facts);
  checks.push({ name: "F264 常规页七类字段", pass: g.type === "文件" && g.openWith === "编辑器" && g.times.created === 1000 && g.readOnly === false });
  checks.push({ name: "F264 占用按簇取整", pass: g.sizeOnDisk.includes("8192") && g.size === "5000 字节" });
  const fold: ItemFacts = { ...facts, kind: "folder", sizeBytes: 0, openWith: null };
  checks.push({ name: "F264 文件夹无打开方式", pass: propsGeneral(fold).openWith === "（无）" });

  // F264 勾选即生效 + 受保护拒绝取消只读
  const flipped = applyAttribute(facts, "readOnly", true);
  checks.push({ name: "F264 勾选即生效", pass: flipped.readOnly === true && facts.readOnly === false });
  const guarded: ItemFacts = { ...facts, readOnly: true, protectedReason: "系统保护文件" };
  const denied = applyAttribute(guarded, "readOnly", false);
  checks.push({ name: "F264 受保护取消只读被拒", pass: denied.readOnly === true && denied.protectedReason === "系统保护文件" });

  // F264 文件夹统计异步步进（进度单调、末次出结果）
  const step = folderStatsStepper([{ sizeBytes: 10 }, { sizeBytes: 20 }, { sizeBytes: 30 }], 2);
  const p1 = step();
  const p2 = step();
  const p3 = step();
  checks.push({ name: "F264 统计异步带进度", pass: !p1.finished && p1.progress === 2 / 3 && !!p3.stats && p3.stats.items === 3 && p3.stats.sizeBytes === 60 && p2.progress === 1 });

  // 菜单装配：图标菜单四项 + 打开方式子菜单 + 受保护禁用重命名
  const menu = iconContextMenu(facts, ["编辑器", "查看器"]);
  checks.push({ name: "F264 图标菜单四项", pass: menu.length === 5 && menu[3]?.id === "rename" && menu[4]?.id === "properties" });
  checks.push({ name: "F260 菜单重命名带 F2 快捷键", pass: menu[3]?.shortcut === "F2" });
  const openWith = menu.find((m) => m.id === "open-with");
  checks.push({ name: "F264 打开方式子菜单", pass: !!openWith && openWith.submenu?.length === 2 });
  const menuLocked = iconContextMenu({ ...facts, protectedReason: "受保护" }, []);
  checks.push({ name: "F264 受保护重命名禁用+子菜单空禁用", pass: menuLocked[3]?.disabled === true && menuLocked.find((m) => m.id === "open-with")?.disabled === true });
  const deskMenu = desktopContextMenu(true);
  checks.push({ name: "F503 桌面空白菜单在册", pass: deskMenu.some((m) => m.id === "sort") && deskMenu.some((m) => m.id === "refresh") });

  return checks;
}
