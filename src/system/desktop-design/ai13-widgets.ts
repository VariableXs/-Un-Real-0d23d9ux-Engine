/**
 * AURORA-10000 · AI-13 桌面微件组C（领域03 · F01501~F01625 · W2）
 * 族0061 微件框架｜族0062 内置微件集｜族0063 桌面互动层
 * 族0064 桌面整理哲学｜族0065 桌面健康
 *
 * - 族0061 能力项直接映射既有 N-09 widget SDK（manifest/沙箱/刷新限额）；
 * - 族0062 与 src/system/widgets/widgets 内置组件对齐并补齐目录；
 * - 族0064 整理布局、族0065 健康到期判定为纯函数（logic.ts 实现、单测覆盖）。
 */
import { entry, family, type DesignFamily } from "./types";

/* ---------------- 族0061 微件框架（F01501~F01525 · 25 项框架能力） ---------------- */

const f0061 = family("f0061", "0061", "AI-13", "微件框架", "Widget framework", [1501, 1525], [
  entry(1501, "f0061", "switch", "host", "宿主运行时 — 微件宿主进程", "Host runtime", {}),
  entry(1502, "f0061", "switch", "manifest", "声明清单 — widget.json 规范", "widget.json spec", { params: { format: "widget", version: 1 } }),
  entry(1503, "f0061", "switch", "permissions", "权限模型 — 微件权限声明", "Permission scopes", { params: { scopes: ["refresh", "clock", "todo:read", "net:status"] } }),
  entry(1504, "f0061", "switch", "sandbox", "沙箱 — 微件相互隔离", "iframe sandbox isolation", { params: { allowSameOrigin: false } }),
  entry(1505, "f0061", "switch", "sizes", "尺寸规格 — 2x2/4x2/4x4/自由", "Sizes 2x2/4x2/4x4/free", { params: { sizes: ["2x2", "4x2", "4x4", "free"] } }),
  entry(1506, "f0061", "switch", "auto-layout", "自动布局 — 拖入自动布局", "Auto layout on drop", {}),
  entry(1507, "f0061", "switch", "manual-layout", "手动布局 — 网格自由摆放", "Free grid placement", {}),
  entry(1508, "f0061", "switch", "stack", "堆叠 — 微件堆叠轮播", "Widget stack carousel", { params: { intervalS: 12 } }),
  entry(1509, "f0061", "switch", "store-entry", "商店入口 — 微件市场入口", "Market entry", {}),
  entry(1510, "f0061", "switch", "silent-update", "更新机制 — 微件静默更新", "Silent update", {}),
  entry(1511, "f0061", "switch", "quota", "数据配额 — 数据刷新限额", "Refresh quota", { params: { minSec: 5 } }),
  entry(1512, "f0061", "switch", "smart-refresh", "智能刷新 — 按可见性刷新", "Visibility-aware refresh", {}),
  entry(1513, "f0061", "switch", "offline-cache", "离线缓存 — 数据缓存规范", "Offline cache", {}),
  entry(1514, "f0061", "switch", "theme-api", "主题 API — 主题适配接口", "Theme API", { params: { vars: ["--bg-surface", "--text-primary"] } }),
  entry(1515, "f0061", "switch", "i18n", "多语言 — i18n 框架复用", "i18n reuse", {}),
  entry(1516, "f0061", "switch", "aria", "无障碍 — aria 规范模板", "aria template", {}),
  entry(1517, "f0061", "switch", "debugger", "调试器 — 微件开发调试器", "Widget debugger", {}),
  entry(1518, "f0061", "switch", "frame-budget", "性能预算 — 每微件帧预算", "Per-widget frame budget", { params: { budgetMs: 4 } }),
  entry(1519, "f0061", "switch", "crash-isolation", "崩溃隔离 — 单微件崩溃不连坐", "Crash isolation", { params: { failThreshold: 3 } }),
  entry(1520, "f0061", "switch", "uninstall-clean", "卸载清理 — 卸载残留清理", "Uninstall cleanup", {}),
  entry(1521, "f0061", "switch", "compat-matrix", "兼容矩阵 — 版本兼容标注", "Compat matrix", {}),
  entry(1522, "f0061", "switch", "signature", "签名 — 微件包签名校验", "Package signature check", {}),
  entry(1523, "f0061", "switch", "review", "评分审核 — 商店评分审核流", "Store review flow", {}),
  entry(1524, "f0061", "switch", "recommend", "推荐 — 个性化推荐位", "Personalized picks", {}),
  entry(1525, "f0061", "switch", "tutorial", "教学 — 微件开发教程", "Dev tutorial", {}),
]);

/* ---------------- 族0062 内置微件集（F01526~F01550 · 25 个内置微件） ---------------- */
/* exists=true 表示已在 src/system/widgets/widgets 实现可挂载；false 为目录内已声明、随框架接入。 */

const w = (exists: boolean, size: string) => ({ params: { exists, size } });

const f0062 = family("f0062", "0062", "AI-13", "内置微件集", "Built-in widgets", [1526, 1550], [
  entry(1526, "f0062", "switch", "clock", "时钟 — 表盘/数字双模式", "Clock (analog/digital)", { ...w(true, "2x2") }),
  entry(1527, "f0062", "switch", "calendar", "日历 — 月历+日程", "Calendar", { ...w(true, "2x2") }),
  entry(1528, "f0062", "switch", "weather", "天气 — 三日天气卡", "Weather 3-day", { ...w(false, "2x2") }),
  entry(1529, "f0062", "switch", "todos", "待办 — 待办清单", "Todos", { ...w(true, "2x2") }),
  entry(1530, "f0062", "switch", "pomodoro", "番茄钟 — 番茄计时", "Pomodoro", { ...w(true, "2x2") }),
  entry(1531, "f0062", "reserved", "steps", "步数 — 计步预留传感器", "Steps (sensor reserved)", { ...w(false, "2x2"), note: { zh: "预留位：无计步硬件时隐藏（守卫 §15.1）", en: "Reserved: hidden without sensor" } }),
  entry(1532, "f0062", "switch", "memo", "便签 — 桌面便签墙", "Memo wall", { ...w(false, "4x2") }),
  entry(1533, "f0062", "switch", "countdown-day", "倒数日 — 重要日子倒数", "Countdown day", { ...w(false, "2x2") }),
  entry(1534, "f0062", "switch", "fx-rate", "汇率 — 常用汇率卡", "FX rate card", { ...w(false, "2x2") }),
  entry(1535, "f0062", "switch", "unit-convert", "换算 — 单位换算", "Unit convert", { ...w(false, "2x2") }),
  entry(1536, "f0062", "switch", "sysmon", "系统监视 — CPU/内存仪表", "CPU / memory gauge", { ...w(true, "2x2") }),
  entry(1537, "f0062", "switch", "netmon", "网络监视 — 网速曲线", "Net speed curve", { ...w(true, "2x2") }),
  entry(1538, "f0062", "switch", "disks", "磁盘 — 各盘空间条", "Disk bars", { ...w(false, "2x2") }),
  entry(1539, "f0062", "switch", "recycle", "回收站 — 回收站状态", "Recycle bin status", { ...w(false, "2x2") }),
  entry(1540, "f0062", "switch", "clipboard", "剪贴板 — 剪贴历史入口", "Clipboard history entry", { ...w(false, "2x2") }),
  entry(1541, "f0062", "switch", "quick-grid", "快捷格 — 自定义启动格", "Custom launch grid", { ...w(true, "4x2") }),
  entry(1542, "f0062", "switch", "player", "播放器 — 迷你播放器", "Mini player", { ...w(false, "4x2") }),
  entry(1543, "f0062", "switch", "radio", "电台 — 网络电台", "Internet radio", { ...w(false, "2x2") }),
  entry(1544, "f0062", "switch", "white-noise", "白噪音 — 雨声/海浪等", "White noise", { ...w(false, "2x2") }),
  entry(1545, "f0062", "switch", "pomo-stats", "番茄统计 — 专注时长统计", "Focus stats", { ...w(false, "2x2") }),
  entry(1546, "f0062", "switch", "water", "饮水 — 喝水打卡", "Water break", { ...w(false, "2x2") }),
  entry(1547, "f0062", "switch", "stand", "站姿 — 起立提醒", "Stand reminder", { ...w(false, "2x2") }),
  entry(1548, "f0062", "switch", "eye-rest", "护眼 — 20-20-20 提醒", "20-20-20 reminder", { ...w(false, "2x2") }),
  entry(1549, "f0062", "switch", "birthday", "生日 — 生日提醒", "Birthday reminder", { ...w(false, "2x2") }),
  entry(1550, "f0062", "switch", "daily-quote", "每日一句 — 每日一语", "Daily quote", { ...w(true, "4x2") }),
]);

/* ---------------- 族0063 桌面互动层（F01551~F01575 · 25 项互动能力） ---------------- */

const f0063 = family("f0063", "0063", "AI-13", "桌面互动层", "Desktop interaction", [1551, 1575], [
  entry(1551, "f0063", "switch", "dblclick-action", "双击空白 — 双击动作自定义", "Double-click blank action", { params: { default: "none" } }),
  entry(1552, "f0063", "switch", "rubber-band", "框选 — 橡皮筋多选", "Rubber-band select", {}),
  entry(1553, "f0063", "switch", "drop-highlight", "放置高亮 — 可放置区高亮", "Drop highlight", {}),
  entry(1554, "f0063", "switch", "wheel-zoom", "滚轮缩放 — 桌面缩放", "Wheel zoom", { params: { min: 0.5, max: 2 } }),
  entry(1555, "f0063", "switch", "mid-pan", "中键平移 — 中键拖动平移", "Middle-button pan", {}),
  entry(1556, "f0063", "switch", "edge-page", "边缘翻页 — 画布翻页", "Edge page flip", {}),
  entry(1557, "f0063", "switch", "doodle", "涂鸦层 — 桌面临时涂鸦", "Temporary doodle", {}),
  entry(1558, "f0063", "switch", "whiteboard", "临时白板 — 一键白板", "One-tap whiteboard", {}),
  entry(1559, "f0063", "switch", "touch-ripple", "指尖涟漪 — 触屏涟漪反馈", "Touch ripple", {}),
  entry(1560, "f0063", "switch", "longpress-menu", "长按菜单 — 长按上下文菜单", "Long-press menu", { params: { ms: 500 } }),
  entry(1561, "f0063", "switch", "drag-ghost", "拖影 — 拖拽残影动效", "Drag ghost trail", {}),
  entry(1562, "f0063", "switch", "snap-drop", "落点吸附 — 拖放自动吸附", "Snap on drop", {}),
  entry(1563, "f0063", "switch", "multi-drag", "多选拖拽 — 批量拖动", "Multi drag", {}),
  entry(1564, "f0063", "switch", "cross-desk-drag", "跨桌拖拽 — 拖到其他桌面", "Cross-desktop drag", {}),
  entry(1565, "f0063", "switch", "cross-screen-drag", "跨屏拖拽 — 拖到其他屏幕", "Cross-screen drag", {}),
  entry(1566, "f0063", "switch", "undo-drop", "放置撤销 — 放错一键还原", "Undo drop", {}),
  entry(1567, "f0063", "switch", "submenu", "二级菜单 — 右键子菜单规范", "Submenu spec", {}),
  entry(1568, "f0063", "switch", "window-dodge", "窗口避让 — 拖拽时窗口让位", "Windows dodge while dragging", {}),
  entry(1569, "f0063", "switch", "drop-predict", "落点预测 — 预测落点预览", "Drop prediction preview", {}),
  entry(1570, "f0063", "switch", "inertia", "惯性滚动 — 桌面惯性", "Desktop inertia", { params: { decay: 0.92 } }),
  entry(1571, "f0063", "switch", "pinch-zoom", "双指缩放 — 触屏缩放", "Two-finger zoom", {}),
  entry(1572, "f0063", "switch", "three-finger-shot", "三指截屏 — 手势截屏", "Three-finger screenshot", {}),
  entry(1573, "f0063", "switch", "pen-pressure", "压感笔 — 压感等级适配", "Pen pressure levels", { params: { levels: 1024 } }),
  entry(1574, "f0063", "reserved", "air-gesture", "隔空手势 — 摄像头手势预留", "Camera gesture reserved", { note: { zh: "预留位：摄像头类默认关 + 首次显式授权（守卫 §15.1）", en: "Reserved: camera off by default" } }),
  entry(1575, "f0063", "switch", "guide", "教学 — 互动手势教学", "Gesture tutorial", {}),
]);

/* ---------------- 族0064 桌面整理哲学（F01576~F01600 · 25 项整理哲学） ---------------- */
/* 布局算法在 logic.ts organizeLayout(mode, items, opts)，单测覆盖。 */

const mode = (m: string) => ({ params: { mode: m } });

const f0064 = family("f0064", "0064", "AI-13", "桌面整理哲学", "Desktop organization", [1576, 1600], [
  entry(1576, "f0064", "preset", "festival-tree", "圣诞树 — 节日自动换布局", "Festival auto layout", { ...mode("festival") }),
  entry(1577, "f0064", "preset", "grid9", "九宫格 — 强制九宫格", "Forced 3x3 grid", { ...mode("grid9") }),
  entry(1578, "f0064", "preset", "free", "自由 — 完全自由摆放", "Completely free", { ...mode("free") }),
  entry(1579, "f0064", "preset", "magnetic", "磁力 — 同类自动相吸", "Magnetic clustering", { ...mode("magnetic") }),
  entry(1580, "f0064", "preset", "timeline", "时间线 — 按打开时间横排", "Open-time timeline", { ...mode("timeline") }),
  entry(1581, "f0064", "preset", "project-zones", "项目分区 — 按项目分区摆放", "Project zones", { ...mode("projects") }),
  entry(1582, "f0064", "preset", "drawer", "抽屉 — 全部收进抽屉", "All into drawer", { ...mode("drawer") }),
  entry(1583, "f0064", "preset", "minimal", "极简 — 只留 3 个图标", "Keep only 3 icons", { ...mode("minimal") }),
  entry(1584, "f0064", "switch", "season-remind", "季节提醒 — 换季整理提醒", "Season tidy reminder", { params: { seasons: 4 } }),
  entry(1585, "f0064", "switch", "half-year-archive", "半年归档 — 半年未用归档", "Half-year archive", { params: { days: 182 } }),
  entry(1586, "f0064", "switch", "usage-report", "使用报告 — 桌面使用报告", "Usage report", {}),
  entry(1587, "f0064", "switch", "clutter-index", "杂物指数 — 杂乱度指数", "Clutter index", {}),
  entry(1588, "f0064", "switch", "restore-default", "复原默认 — 一键恢复出厂布局", "Restore factory layout", {}),
  entry(1589, "f0064", "switch", "template-market", "模板市场 — 布局模板市场", "Layout template market", {}),
  entry(1590, "f0064", "switch", "persona", "人格包 — 整理师语气包", "Organizer persona", {}),
  entry(1591, "f0064", "switch", "gamify", "游戏化 — 整理积分系统", "Tidy points", {}),
  entry(1592, "f0064", "switch", "achievements", "成就墙 — 整理成就展示", "Achievement wall", {}),
  entry(1593, "f0064", "switch", "rule-import", "规则导入 — 规则文件导入", "Rule import", {}),
  entry(1594, "f0064", "switch", "conflict", "冲突解决 — 规则冲突提示", "Rule conflict hints", {}),
  entry(1595, "f0064", "switch", "undo-history", "撤销历史 — 整理历史回滚", "Undo history", { params: { depth: 20 } }),
  entry(1596, "f0064", "switch", "rehearsal", "预演 — 整理前动画预演", "Pre-arrange preview", {}),
  entry(1597, "f0064", "switch", "whitelist", "白名单 — 免整理清单", "No-tidy whitelist", {}),
  entry(1598, "f0064", "switch", "quiet-hours", "免打扰 — 时段免打扰", "Quiet hours", {}),
  entry(1599, "f0064", "switch", "calendar", "整理日历 — 约定整理日", "Tidy calendar", {}),
  entry(1600, "f0064", "switch", "guide", "教学 — 整理哲学引导", "Philosophy guide", {}),
]);

/* ---------------- 族0065 桌面健康（F01601~F01625 · 25 项健康能力） ---------------- */
/* 到期判定在 logic.ts dueHealthRules；运行时经 uiStore.pushToast 落成真实提醒。 */

const f0065 = family("f0065", "0065", "AI-13", "桌面健康", "Desktop health", [1601, 1625], [
  entry(1601, "f0065", "switch", "eye-202020", "20-20-20 — 护眼远眺提醒", "20-20-20 eye rest", { params: { everyMin: 20 } }),
  entry(1602, "f0065", "switch", "blue-plan", "蓝光计划 — 分时段蓝光策略", "Blue-light schedule", { params: { nightFrom: 19 } }),
  entry(1603, "f0065", "switch", "sedentary", "久坐 — 起立提醒", "Sedentary stand-up", { params: { everyMin: 45 } }),
  entry(1604, "f0065", "switch", "water", "饮水 — 喝水节奏提醒", "Water cadence", { params: { everyMin: 60 } }),
  entry(1605, "f0065", "reserved", "posture", "坐姿 — 摄像头坐姿可选（默认关闭 · 纯本地推理 · 首次显式授权）", "Posture camera (off by default)", { note: { zh: "守卫 §15.1：默认关、纯本地、首次显式授权；无摄像头隐藏入口", en: "Guard: off by default, local-only" } }),
  entry(1606, "f0065", "switch", "distance", "距离 — 屏幕距离提示", "Screen distance hint", {}),
  entry(1607, "f0065", "switch", "dark-room", "暗环境 — 光线过暗提醒", "Too-dark hint", {}),
  entry(1608, "f0065", "reserved", "ambient-light", "亮度自适应 — 环境光预留接入", "Ambient light reserved", { note: { zh: "预留位：无光感硬件时隐藏入口", en: "Reserved: hidden without sensor" } }),
  entry(1609, "f0065", "switch", "temp-auto", "色温自动 — 时段色温", "Auto color temp", { params: { nightDeg: 3400 } }),
  entry(1610, "f0065", "switch", "pomo-stats", "番茄统计 — 每日番茄数", "Daily pomodoros", {}),
  entry(1611, "f0065", "switch", "workout", "休息操 — 工间操引导", "Desk stretches", {}),
  entry(1612, "f0065", "switch", "noise", "白噪音 — 专注白噪音", "Focus white noise", {}),
  entry(1613, "f0065", "switch", "mindful", "正念 — 一分钟正念引导", "1-min mindfulness", { params: { sec: 60 } }),
  entry(1614, "f0065", "switch", "screen-time", "时长报告 — 今日屏幕时长", "Today screen time", {}),
  entry(1615, "f0065", "switch", "app-mix", "应用分布 — 使用分布图", "App distribution", {}),
  entry(1616, "f0065", "switch", "interrupts", "打扰分析 — 打断源分析", "Interrupt analysis", {}),
  entry(1617, "f0065", "switch", "focus-week", "专注周报 — 每周专注报告", "Weekly focus report", {}),
  entry(1618, "f0065", "switch", "detox", "戒断 — 数字戒断模式", "Digital detox", {}),
  entry(1619, "f0065", "switch", "kids", "儿童模式 — 家长控制档", "Kids mode", {}),
  entry(1620, "f0065", "switch", "elders", "长辈模式 — 大字简化档", "Elders mode", {}),
  entry(1621, "f0065", "switch", "night-dnd", "夜间勿扰 — 定时勿扰", "Night DND", { params: { from: 23, to: 7 } }),
  entry(1622, "f0065", "switch", "meeting-mute", "会议联动 — 开会自动静音", "Meeting auto-mute", {}),
  entry(1623, "f0065", "switch", "local-only", "本地承诺 — 健康数据不出本机", "Health data stays local", {}),
  entry(1624, "f0065", "switch", "export", "数据导出 — 健康记录导出", "Export health log", { params: { format: "json" } }),
  entry(1625, "f0065", "switch", "guide", "教学 — 健康功能导览", "Health guide", {}),
]);

export const AI13_FAMILIES: DesignFamily[] = [f0061, f0062, f0063, f0064, f0065];
