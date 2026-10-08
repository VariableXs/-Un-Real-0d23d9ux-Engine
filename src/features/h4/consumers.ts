/**
 * H4 消费面接线契约层（v6 · 深化批次六）：
 * F351-F400 五十项与桌面真实表面（任务栏/托盘/窗口框/开始菜单/设置中心/通知中心/
 * 桌面/资源管理器/终端/快速设置/Alt+Tab/任务视图/关于页/恢复环境）的双向接线契约——
 * 每项挂哪些表面、收哪些事件（桌面→功能）、发哪些事件（功能→桌面），一处一事实。
 * 真实桌面壳按本契约逐面接线；本层先于接线落地（Schema 先行纪律），并以审计函数
 * 保证接线完整性：每项至少挂一表面、mountId 唯一、事件命名规范、与登记册逐项对齐。
 * 判据锚定：主册 H 域各项的功能定义（接线即「功能做出来就要让人找得到」的入口面）。
 */

import { H4_REGISTRY } from "../../system/h4/registry";

/** 桌面表面（接线的一级落点——一处一事实的表面词表）。 */
export const SURFACES = [
  "taskbar",
  "tray",
  "window-frame",
  "start-menu",
  "settings",
  "notify-center",
  "desktop",
  "explorer",
  "terminal",
  "quick-settings",
  "alt-tab",
  "task-view",
  "about-page",
  "recovery-env",
] as const;
export type Surface = (typeof SURFACES)[number];

/**
 * 入向事件源词表（入向事件的域注册表——一处一事实）：
 * 表面（SURFACES）之外的设备/系统源（显示器热插拔/浏览器/硬件/热键/用户动作/审计流…）。
 * 入向事件只能挂这两个词表内的域——域外来源必须先进册再发事件。
 */
export const INPUT_SOURCES: ReadonlyArray<string> = [
  ...SURFACES,
  "display", "edge", "video", "pip", "hotkey", "mouse", "record", "focus", "user", "battery", "icon",
  "task", "boot", "audit-event", "titlebar", "menu", "window", "header", "list", "tree", "dialog",
  "layer", "modal", "tab", "dom", "tile", "capture", "text", "context", "dir", "storage", "cleanup",
  "media", "backup", "drill", "closure", "gate",
] as const;

/** 事件命名规范（双向两域）：出向事件挂功能域 f<XXX>.<kebab>；入向事件挂表面域 <surface>.<kebab>——事件属主一眼可辨。 */
const OUT_EVENT_RE = /^f\d{3}\.[a-z0-9-]+$/;

export interface ConsumerBinding {
  item: string;
  /** 挂载表面（≥1——好功能没有入口 = 没做）。 */
  surfaces: Surface[];
  /** 挂载点 id（全局唯一——同一表面同一点位不重复占坑）。 */
  mountId: string;
  /** 入向事件（桌面 → 功能）。 */
  eventsIn: string[];
  /** 出向事件（功能 → 桌面）。 */
  eventsOut: string[];
  /** 接线说明（人话——接线图悬停可读）。 */
  note: string;
}

/** 五十项全量接线表（v6 起草；桌面壳接线与本表逐项对齐——对账函数在文末）。 */
export const BINDINGS: ConsumerBinding[] = [
  { item: "F351", surfaces: ["task-view", "settings"], mountId: "task-view.workspace-snapshot", eventsIn: ["task-view.save-snapshot", "task-view.restore-snapshot"], eventsOut: ["f351.snapshot-saved", "f351.restore-done", "f351.launch-queue"], note: "任务视图侧栏快照卡 + 设置中心管理页" },
  { item: "F352", surfaces: ["taskbar"], mountId: "taskbar.thumb-actions", eventsIn: ["taskbar.thumb-hover", "taskbar.thumb-click"], eventsOut: ["f352.close-window", "f352.playpause"], note: "悬停缩略图操作集（关闭/迷你键）" },
  { item: "F353", surfaces: ["window-frame"], mountId: "window-frame.display-watch", eventsIn: ["display.removed", "display.added", "window.drag-end"], eventsOut: ["f353.reflow-plan", "f353.reattach", "f353.absence-notice"], note: "显示器热插拔监听 + 拖拽落位记账" },
  { item: "F354", surfaces: ["taskbar", "settings"], mountId: "taskbar.resource-summary", eventsIn: ["taskbar.summary-toggle"], eventsOut: ["f354.sample-tick"], note: "托盘区可选三图摘要（默认关）" },
  { item: "F355", surfaces: ["window-frame", "notify-center"], mountId: "window-frame.edge-pipes", eventsIn: ["edge.download-done", "edge.drag-payload"], eventsOut: ["f355.download-notify", "f355.compat-issue"], note: "Edge 三管子：下载/拖拽/待遇" },
  { item: "F356", surfaces: ["start-menu", "taskbar", "alt-tab"], mountId: "start-menu.pwa-entries", eventsIn: ["edge.install-request"], eventsOut: ["f356.installed", "f356.uninstalled"], note: "PWA 四列表平权（开始/任务栏/Alt+Tab/全部应用）" },
  { item: "F357", surfaces: ["tray", "notify-center"], mountId: "tray.download-progress", eventsIn: ["edge.download-progress"], eventsOut: ["f357.tray-progress", "f357.completion-notice"], note: "托盘微进度 + 完成通知双钮" },
  { item: "F358", surfaces: ["window-frame", "taskbar"], mountId: "window-frame.global-pip", eventsIn: ["video.pip-request", "pip.dock", "pip.exit"], eventsOut: ["f358.pip-entered", "f358.return-to-source"], note: "全局画中画四键 + 四角停靠" },
  { item: "F359", surfaces: ["quick-settings", "desktop", "settings"], mountId: "quick-settings.color-picker", eventsIn: ["hotkey.color-pick", "mouse.pick", "settings.summon-picker"], eventsOut: ["f359.color-picked", "f359.clipboard-write"], note: "系统吸管（放大环 + 双格式复制）" },
  { item: "F360", surfaces: ["quick-settings", "settings"], mountId: "quick-settings.pixel-ruler", eventsIn: ["hotkey.ruler-toggle", "mouse.drag", "settings.summon-ruler"], eventsOut: ["f360.measure-update"], note: "像素标尺 + 8px 网格（点击穿透）" },
  { item: "F361", surfaces: ["quick-settings", "tray"], mountId: "quick-settings.screen-record", eventsIn: ["record.start", "record.pause", "record.stop"], eventsOut: ["f361.recording-frame", "f361.privacy-indicator"], note: "三模式录制 + 呼吸红框 + 隐私指示" },
  { item: "F362", surfaces: ["notify-center", "explorer"], mountId: "notify-center.recording-closing", eventsIn: ["record.finished"], eventsOut: ["f362.closing-bar", "f362.segment-written"], note: "收尾条 + 分节账（断电可恢复）" },
  { item: "F363", surfaces: ["taskbar", "quick-settings", "settings"], mountId: "taskbar.focus-badge", eventsIn: ["focus.start", "focus.abandon", "settings.focus-start", "settings.focus-abandon"], eventsOut: ["f363.badge-text", "f363.focus-done"], note: "任务栏专注徽标（mm:ss）+ 双通道提醒" },
  { item: "F364", surfaces: ["explorer"], mountId: "explorer.downloads-tidy", eventsIn: ["user.tidy-request", "user.tidy-confirm", "user.tidy-undo"], eventsOut: ["f364.tidy-proposal", "f364.tidy-executed", "f364.tidy-undone"], note: "下载目录整理建议条（建议制不动手）" },
  { item: "F365", surfaces: ["explorer", "settings"], mountId: "explorer.dupe-finder", eventsIn: ["user.dupe-scan", "user.dupe-trash"], eventsOut: ["f365.dupe-report", "f365.trash-done"], note: "存储工具重复查找（三级管线）" },
  { item: "F366", surfaces: ["tray", "settings"], mountId: "tray.battery-icon", eventsIn: ["battery.reading"], eventsOut: ["f366.tray-render", "f366.low-battery"], note: "电池三档渲染 + 四段色 + 低电三态" },
  { item: "F367", surfaces: ["desktop"], mountId: "desktop.zone-snap", eventsIn: ["icon.drag", "icon.drop", "icon.arrange-request"], eventsOut: ["f367.snap-guides", "f367.zone-arranged"], note: "图标网格吸附 + 可选四分区" },
  { item: "F368", surfaces: ["tray", "taskbar"], mountId: "tray.minimize-to-tray", eventsIn: ["window.close-click", "tray.icon-click", "tray.quit"], eventsOut: ["f368.to-tray", "f368.restored"], note: "关闭到托盘（声明不骗人）+ 真退出路径" },
  { item: "F369", surfaces: ["task-view", "settings"], mountId: "task-view.task-center", eventsIn: ["task.register", "task.advance", "task.pause", "task.global-pause"], eventsOut: ["f369.badge-count"], note: "后台任务一处可见 + 全局暂停" },
  { item: "F370", surfaces: ["notify-center"], mountId: "notify-center.quiet-failures", eventsIn: ["task.failed"], eventsOut: ["f370.final-notice"], note: "后台失败唯一出口（横幅=0）" },
  { item: "F371", surfaces: ["desktop", "about-page"], mountId: "desktop.boot-badge", eventsIn: ["boot.completed"], eventsOut: ["f371.badge-show"], note: "开机时长徽标（同源换算）+ 历史曲线" },
  { item: "F372", surfaces: ["about-page"], mountId: "about-page.activity-timeline", eventsIn: ["audit-event.appended"], eventsOut: ["f372.timeline-row"], note: "系统大事人话时间线（哈希链只读）" },
  { item: "F373", surfaces: ["taskbar", "settings"], mountId: "taskbar.layout-indicator", eventsIn: ["hotkey.win-space", "settings.layout-change"], eventsOut: ["f373.layout-switched", "f373.osd-show"], note: "语言指示轮切 + 三处同步 + 切换 OSD" },
  { item: "F374", surfaces: ["desktop"], mountId: "desktop.hotkey-sheet", eventsIn: ["hotkey.win-down", "hotkey.win-up"], eventsOut: ["f374.sheet-show", "f374.sheet-hide"], note: "长按 Win 速查浮层（注册表实时同源）" },
  { item: "F375", surfaces: ["settings"], mountId: "settings.h-domain-gate", eventsIn: ["gate.run-request"], eventsOut: ["f375.release-decision"], note: "H 域三走查门禁面板（回炉清单）" },
  { item: "F376", surfaces: ["window-frame"], mountId: "window-frame.system-menu", eventsIn: ["titlebar.context-menu", "menu.key"], eventsOut: ["f376.menu-action"], note: "标题栏系统菜单（四状态×六项矩阵）" },
  { item: "F377", surfaces: ["window-frame"], mountId: "window-frame.keyboard-edit", eventsIn: ["menu.move-size", "window.arrow-key", "window.confirm", "window.cancel"], eventsOut: ["f377.geometry-commit"], note: "键盘移动/大小（Enter 落定 Esc 还原）" },
  { item: "F378", surfaces: ["explorer"], mountId: "explorer.column-autofit", eventsIn: ["header.dblclick", "header.drag"], eventsOut: ["f378.width-changed"], note: "列宽双击自适应 + 手动宽度优先" },
  { item: "F379", surfaces: ["explorer"], mountId: "explorer.header-sort", eventsIn: ["header.click"], eventsOut: ["f379.sort-changed"], note: "表头三态排序 + 多级指示" },
  { item: "F380", surfaces: ["explorer", "desktop"], mountId: "explorer.blank-click", eventsIn: ["list.blank-click", "list.blank-drag"], eventsOut: ["f380.selection-cleared", "f380.background-menu"], note: "空白救命单击 + 背景菜单（无对象操作=0）" },
  { item: "F381", surfaces: ["explorer", "settings"], mountId: "explorer.tree-tristate", eventsIn: ["tree.check", "tree.expand", "tree.key"], eventsOut: ["f381.check-changed"], note: "树三态 + 展开记忆 + 键盘四操作" },
  { item: "F382", surfaces: ["window-frame"], mountId: "window-frame.dialog-position", eventsIn: ["dialog.open", "dialog.move-end"], eventsOut: ["f382.position-applied"], note: "对话框位置记忆（同类归组）" },
  { item: "F383", surfaces: ["window-frame", "notify-center"], mountId: "window-frame.layer-queue", eventsIn: ["layer.enqueue", "layer.close"], eventsOut: ["f383.layer-presented", "f383.banner-overflow"], note: "弹层排队（模态唯一 + 200ms 递补）" },
  { item: "F384", surfaces: ["window-frame"], mountId: "window-frame.focus-trap", eventsIn: ["modal.opened", "modal.closed", "tab"], eventsOut: ["f384.focus-returned"], note: "焦点陷阱（Tab 零逃逸 + 归还唤起者）" },
  { item: "F385", surfaces: ["window-frame", "notify-center"], mountId: "window-frame.semantic-tree", eventsIn: ["dom.mutation"], eventsOut: ["f385.announce"], note: "无障碍语义树 + 弹窗播报" },
  { item: "F386", surfaces: ["window-frame", "settings"], mountId: "window-frame.reading-mode", eventsIn: ["menu.reading-toggle", "settings.reading-toggle"], eventsOut: ["f386.style-applied"], note: "阅读模式（查看菜单统一入口）" },
  { item: "F387", surfaces: ["quick-settings"], mountId: "quick-settings.grayscale", eventsIn: ["tile.toggle", "hotkey.grayscale"], eventsOut: ["f387.filter-changed"], note: "一键灰度（滤镜单点 + 三滤镜互斥）" },
  { item: "F388", surfaces: ["taskbar", "window-frame"], mountId: "taskbar.portrait-adapt", eventsIn: ["display.rotated"], eventsOut: ["f388.layout-readapted"], note: "竖屏换形 + 任务栏侧边 + 批量重适配" },
  { item: "F389", surfaces: ["quick-settings", "explorer"], mountId: "quick-settings.scroll-stitch", eventsIn: ["capture.scroll-capture"], eventsOut: ["f389.stitch-result"], note: "滚动长截图（通配重叠稳健拼接）" },
  { item: "F390", surfaces: ["desktop"], mountId: "desktop.word-lookup", eventsIn: ["text.selection", "context.lookup"], eventsOut: ["f390.card-show"], note: "选中文本查词浮卡（离线 + 不抢焦点）" },
  { item: "F391", surfaces: ["desktop"], mountId: "desktop.text-translate", eventsIn: ["context.translate"], eventsOut: ["f391.card-show"], note: "选中文本翻译（离线诚实提示）" },
  { item: "F392", surfaces: ["explorer"], mountId: "explorer.folder-size", eventsIn: ["dir.expanded", "dir.hovered"], eventsOut: ["f392.size-filled"], note: "文件夹大小列（异步淡入 + 缓存单点）" },
  { item: "F393", surfaces: ["explorer", "settings"], mountId: "explorer.storage-treemap", eventsIn: ["storage.view-request", "storage.drill"], eventsOut: ["f393.treemap-rendered"], note: "存储热点图（squarified + 下钻）" },
  { item: "F394", surfaces: ["settings", "explorer"], mountId: "settings.cleanup-summary", eventsIn: ["cleanup.view-request", "cleanup.execute"], eventsOut: ["f394.cleanup-report"], note: "清理建议收口页（不可逆闸门）" },
  { item: "F395", surfaces: ["settings", "tray"], mountId: "settings.usb-health", eventsIn: ["media.plugged", "media.poll"], eventsOut: ["f395.health-notice"], note: "U 盘健康（读不到不编数）+ 双阈值出路" },
  { item: "F396", surfaces: ["settings"], mountId: "settings.backup-wizard", eventsIn: ["backup.start", "backup.pause", "backup.verify"], eventsOut: ["f396.backup-progress", "f396.verify-result"], note: "备份三步向导（验过能还原）" },
  { item: "F397", surfaces: ["settings", "about-page", "recovery-env"], mountId: "about-page.restore-drill", eventsIn: ["drill.reminder", "drill.run"], eventsOut: ["f397.drill-recorded"], note: "季度还原演练（零副作用沙盒）" },
  { item: "F398", surfaces: ["settings"], mountId: "settings.language-hotswap", eventsIn: ["settings.language-change"], eventsOut: ["f398.reswap-done", "f398.missing-key-report"], note: "界面语言热切（回退标注 + RTL 接口）" },
  { item: "F399", surfaces: ["terminal", "about-page", "desktop"], mountId: "terminal.egg-star", eventsIn: ["boot.count", "about-page.version-tap", "terminal.command"], eventsOut: ["f399.egg-played"], note: "三枚彩蛋（一次性判据 + 不藏功能）" },
  { item: "F400", surfaces: ["settings", "about-page"], mountId: "settings.h-domain-closure", eventsIn: ["closure.run-request"], eventsOut: ["f400.ledger-persisted"], note: "收官登记（一行账 + 三处同源审计）" },
];

/* ================= 接线完整性审计（对账扩展——接线层自己的检查项） ================= */

export interface WiringAudit {
  pass: boolean;
  problems: string[];
}

/** 事件命名域审计：出向事件必须挂 f<XXX>. 功能域；入向事件必须挂已注册源域——跨域裸奔即违规。 */
export function auditEventNaming(bindings: ConsumerBinding[] = BINDINGS): WiringAudit {
  const problems: string[] = [];
  const sources = new Set<string>(INPUT_SOURCES);
  for (const b of bindings) {
    for (const e of b.eventsOut) {
      if (!OUT_EVENT_RE.test(e)) problems.push(`${b.item}: 出向事件「${e}」不符合 f<XXX>.<event> 功能域命名`);
    }
    for (const e of b.eventsIn) {
      const domain = e.split(".")[0] ?? "";
      if (!sources.has(domain)) problems.push(`${b.item}: 入向事件「${e}」不属于任何已注册源域（先入册再发事件）`);
    }
  }
  return { pass: problems.length === 0, problems };
}

/** 挂载点唯一性：mountId 全域唯一（同点位双挂 = 焦点/事件路由歧义）。 */
export function auditMountUniqueness(bindings: ConsumerBinding[] = BINDINGS): WiringAudit {
  const seen = new Map<string, string>();
  const problems: string[] = [];
  for (const b of bindings) {
    const prev = seen.get(b.mountId);
    if (prev) problems.push(`挂载点「${b.mountId}」被 ${prev} 与 ${b.item} 重复占用`);
    seen.set(b.mountId, b.item);
  }
  return { pass: problems.length === 0, problems };
}

/** 表面覆盖审计：每项 ≥1 表面；全部词表表面至少被一项使用（空表面 = 词表虚胖）。 */
export function auditSurfaceCoverage(bindings: ConsumerBinding[] = BINDINGS): WiringAudit {
  const problems: string[] = [];
  for (const b of bindings) {
    if (b.surfaces.length === 0) problems.push(`${b.item}: 零表面挂载——功能没有入口（好功能没有入口 = 没做）`);
    for (const s of b.surfaces) if (!SURFACES.includes(s)) problems.push(`${b.item}: 未知表面「${s}」`);
  }
  const used = new Set(bindings.flatMap((b) => b.surfaces));
  for (const s of SURFACES) {
    if (!used.has(s)) problems.push(`表面「${s}」无任何功能挂载——词表虚胖或接线缺失`);
  }
  return { pass: problems.length === 0, problems };
}

/** 登记册对齐：绑定表与 H4_REGISTRY 逐项一一对应（接线层不漏项不越项）。 */
export function auditRegistryAlignment(bindings: ConsumerBinding[] = BINDINGS): WiringAudit {
  const regItems = H4_REGISTRY.map((e) => e.item).sort();
  const bindItems = bindings.map((b) => b.item).sort();
  const problems: string[] = [];
  if (regItems.length !== bindItems.length) problems.push(`绑定表 ${bindItems.length} 项 ≠ 登记册 ${regItems.length} 项`);
  for (let i = 0; i < Math.min(regItems.length, bindItems.length); i++) {
    if (regItems[i] !== bindItems[i]) problems.push(`对齐断裂：登记册 ${regItems[i]} vs 绑定表 ${bindItems[i]}`);
  }
  return { pass: problems.length === 0, problems };
}

/** 接线报告（面板数据源）：每表面挂载项数 + 事件总数 + 审计汇总。 */
export function wiringReport(bindings: ConsumerBinding[] = BINDINGS): {
  totalItems: number;
  perSurface: Array<{ surface: Surface; count: number }>;
  totalEventsIn: number;
  totalEventsOut: number;
  audits: WiringAudit[];
  allGreen: boolean;
} {
  const perSurface = SURFACES.map((surface) => ({ surface, count: bindings.filter((b) => b.surfaces.includes(surface)).length }));
  const audits = [auditEventNaming(bindings), auditMountUniqueness(bindings), auditSurfaceCoverage(bindings), auditRegistryAlignment(bindings)];
  return {
    totalItems: bindings.length,
    perSurface,
    totalEventsIn: bindings.reduce((s, b) => s + b.eventsIn.length, 0),
    totalEventsOut: bindings.reduce((s, b) => s + b.eventsOut.length, 0),
    audits,
    allGreen: audits.every((a) => a.pass),
  };
}

/** 单项接线查询（桌面壳按 item 取契约——一处查询处处一致）。 */
export function bindingFor(item: string, bindings: ConsumerBinding[] = BINDINGS): ConsumerBinding | null {
  return bindings.find((b) => b.item === item) ?? null;
}

/** 表面视角查询：某表面上挂了哪些功能（接线矩阵的列视图）。 */
export function bindingsOn(surface: Surface, bindings: ConsumerBinding[] = BINDINGS): ConsumerBinding[] {
  return bindings.filter((b) => b.surfaces.includes(surface));
}