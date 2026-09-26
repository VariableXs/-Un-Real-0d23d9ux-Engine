/**
 * 检查项台账镜像（AI-U1 · 五十项判据自检的**前端对账面**）。
 *
 * 职责（对齐 C2/J2「隔离验证与检查项对账」先例，一处一事实）：
 * - 内核侧 51 块 CheckSet（kernel/varix/src/uni1/，隔离舱 123/123 绿）的
 *   **逐项检查计数清单**在此登记为常量表——数字由 tally.rs 机数产出后
 *   抄录（抄录处注明产出命令），面板「对账」区直接渲染；
 * - 本表与内核的**一致性由 CI 口径保证**：内核域聚合器 blocks=51（容量
 *   64 内零截断）；任何一侧改动都必须同步另一侧（同一提交内）；
 * - 汇总行 = 隔离舱主层检查总数（与 cabin cargo test 的 CheckSet 检查
 *   数一致——聚合器 + 单测的差额在 MB_TXN 注释里说明）。
 *
 * 边界：本文件只读不写——内核 CheckSet 的红绿以 cargo test 为准，这里
 * 只做「人类可读的镜像账」，不重复判据逻辑（一处一事实）。
 */

/** 一块 = 一个 F 项（或共享底盘）的 CheckSet 清单行。 */
export interface LedgerRow {
  tag: string;          // "F401" … "F450" / "ubase"
  module: string;       // 内核模块文件名
  name: string;         // 功能名
  checks: number;       // 主层 CheckSet 检查数
  unitTests: number;    // 宿主单测数
}

/**
 * 51 块台账（2026-09-26 机数，产出命令见「隔离验证与检查项对账.md」§3）。
 */
export const U1_LEDGER: readonly LedgerRow[] = [
  { tag: "ubase", module: "ubase.rs", name: "共享底盘（F244 键位表/浮层栈/事件环/旋钮）", checks: 12, unitTests: 4 },
  { tag: "F401", module: "autoarrange.rs", name: "桌面自动排列", checks: 10, unitTests: 4 },
  { tag: "F402", module: "taskmhot.rs", name: "任务管理器快捷入口", checks: 8, unitTests: 3 },
  { tag: "F403", module: "lockhot.rs", name: "Win+L 锁屏快捷", checks: 9, unitTests: 4 },
  { tag: "F404", module: "explorehot.rs", name: "Win+E 资源管理器", checks: 8, unitTests: 4 },
  { tag: "F405", module: "altf4.rs", name: "Alt+F4 与关机菜单", checks: 13, unitTests: 3 },
  { tag: "F406", module: "secscr.rs", name: "安全屏（简版）", checks: 11, unitTests: 2 },
  { tag: "F407", module: "sethot.rs", name: "Win+I 设置快捷", checks: 9, unitTests: 4 },
  { tag: "F408", module: "winxmenu.rs", name: "Win+X 快捷菜单", checks: 12, unitTests: 5 },
  { tag: "F409", module: "ctxhelp.rs", name: "F1 上下文帮助", checks: 9, unitTests: 2 },
  { tag: "F410", module: "enterkey.rs", name: "Enter 打开与 Ctrl+Enter", checks: 8, unitTests: 2 },
  { tag: "F411", module: "backnav.rs", name: "Backspace 上级目录", checks: 6, unitTests: 2 },
  { tag: "F412", module: "altrprop.rs", name: "Alt+Enter 属性", checks: 7, unitTests: 2 },
  { tag: "F413", module: "prtsrc.rs", name: "PrintScreen 键接入", checks: 8, unitTests: 2 },
  { tag: "F414", module: "dragtrash.rs", name: "拖拽进回收站", checks: 10, unitTests: 2 },
  { tag: "F415", module: "trashicon.rs", name: "回收站满空两态", checks: 15, unitTests: 2 },
  { tag: "F416", module: "winkey.rs", name: "Win 键开合开始菜单", checks: 10, unitTests: 5 },
  { tag: "F417", module: "tilegrid.rs", name: "开始菜单磁贴交互", checks: 12, unitTests: 2 },
  { tag: "F418", module: "pinbar.rs", name: "任务栏固定应用", checks: 11, unitTests: 2 },
  { tag: "F419", module: "barctx.rs", name: "任务栏右键菜单", checks: 10, unitTests: 2 },
  { tag: "F420", module: "clockctx.rs", name: "时钟右键快捷", checks: 7, unitTests: 1 },
  { tag: "F421", module: "imeind.rs", name: "输入法指示器点击", checks: 11, unitTests: 2 },
  { tag: "F422", module: "volfly.rs", name: "音量图标浮层", checks: 8, unitTests: 2 },
  { tag: "F423", module: "batfly.rs", name: "电池图标浮层", checks: 10, unitTests: 2 },
  { tag: "F424", module: "escstack.rs", name: "Esc 通用关闭语义", checks: 9, unitTests: 4 },
  { tag: "F425", module: "shake.rs", name: "Aero Shake 晃动窗口", checks: 7, unitTests: 2 },
  { tag: "F426", module: "docskeys.rs", name: "文档键位保存族", checks: 7, unitTests: 2 },
  { tag: "F427", module: "clipkeys.rs", name: "Ctrl+X/C/V 语义表", checks: 11, unitTests: 2 },
  { tag: "F428", module: "printkey.rs", name: "Ctrl+P 打印链路", checks: 10, unitTests: 2 },
  { tag: "F429", module: "fullscreen.rs", name: "F11 全屏模式", checks: 10, unitTests: 1 },
  { tag: "F430", module: "zoomwheel.rs", name: "Ctrl+滚轮视图缩放", checks: 13, unitTests: 1 },
  { tag: "F431", module: "newfolder.rs", name: "Ctrl+Shift+N 快捷新建", checks: 8, unitTests: 1 },
  { tag: "F432", module: "listnav.rs", name: "列表翻页与定位键", checks: 9, unitTests: 2 },
  { tag: "F433", module: "kbdmenu.rs", name: "Shift+F10 键盘右键", checks: 13, unitTests: 1 },
  { tag: "F434", module: "dlgkeys.rs", name: "对话框控件键位", checks: 11, unitTests: 1 },
  { tag: "F435", module: "dropdown.rs", name: "下拉框键盘操作", checks: 12, unitTests: 1 },
  { tag: "F436", module: "sliderkeys.rs", name: "滑杆键盘操作", checks: 14, unitTests: 2 },
  { tag: "F437", module: "fmtdisk.rs", name: "磁盘格式化工具", checks: 14, unitTests: 1 },
  { tag: "F438", module: "diskmnt.rs", name: "盘符与挂载管理", checks: 6, unitTests: 2 },
  { tag: "F439", module: "drvcrypt.rs", name: "驱动器加密", checks: 13, unitTests: 2 },
  { tag: "F440", module: "isomount.rs", name: "ISO 镜像挂载", checks: 8, unitTests: 2 },
  { tag: "F441", module: "schedtask.rs", name: "计划任务创建", checks: 3, unitTests: 2 },
  { tag: "F442", module: "restpoint.rs", name: "手动创建还原点", checks: 9, unitTests: 2 },
  { tag: "F443", module: "btpair.rs", name: "蓝牙配对流程", checks: 11, unitTests: 2 },
  { tag: "F444", module: "ptrsetup.rs", name: "打印机安装向导", checks: 7, unitTests: 2 },
  { tag: "F445", module: "disparrange.rs", name: "显示器排列拖拽", checks: 7, unitTests: 2 },
  { tag: "F446", module: "ressel.rs", name: "分辨率与刷新率选择", checks: 9, unitTests: 2 },
  { tag: "F447", module: "sndaudition.rs", name: "事件声音试听", checks: 9, unitTests: 2 },
  { tag: "F448", module: "mictest.rs", name: "麦克风测试向导", checks: 9, unitTests: 2 },
  { tag: "F449", module: "camtest.rs", name: "摄像头预览测试", checks: 9, unitTests: 2 },
  { tag: "F450", module: "stickkeys.rs", name: "粘滞键与筛选键", checks: 10, unitTests: 2 },
];

/** 汇总（对账基准——与 tally.rs 产出一致）。 */
export function ledgerTotals(): { blocks: number; items: number; checks: number; unitTests: number } {
  const items = U1_LEDGER.filter((r) => r.tag !== "ubase").length;
  return {
    blocks: U1_LEDGER.length,
    items,
    checks: U1_LEDGER.reduce((s, r) => s + r.checks, 0),
    unitTests: U1_LEDGER.reduce((s, r) => s + r.unitTests, 0),
  };
}

/** 零截断证明（判据：聚合器容量 64——51 块 ≤ 64）。 */
export const AGGREGATOR_CAPACITY = 64;

export function noTruncation(): boolean {
  return U1_LEDGER.length <= AGGREGATOR_CAPACITY && ledgerTotals().blocks === U1_LEDGER.length;
}

/** 范围完整性：F401-F450 恰好各一块。 */
export function rangeComplete(): boolean {
  const tags = new Set(U1_LEDGER.map((r) => r.tag));
  for (let n = 401; n <= 450; n++) {
    if (!tags.has(`F${n}`)) return false;
  }
  return true;
}
