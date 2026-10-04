/**
 * AURORA-10000 · AI-14 视觉一致性组D（领域03 · F01626~F01750 · W2）
 * 族0066 设计令牌扩展｜族0067 暗色与亮度｜族0068 光标与指针
 * 族0069 窗口内容风格｜族0070 视觉动效全局
 *
 * 族0066 的令牌本体已在 src/design/tokens.css 以
 * 「AURORA-10000：AI-11~AI-15 批次，勿删」段静态落库；此处为对应运行时开关
 * 与调试面板（tokenOverrides 经 runtime.ts 写 :root）。
 * 族0067/0071 的主题变体经 [data-w2-theme] 覆写既有语义令牌
 * （--bg-canvas/--bg-surface/--bg-raised/--text-primary/--text-secondary/--accent）。
 */
import { entry, family, type DesignFamily } from "./types";

/* ---------------- 族0066 设计令牌扩展（F01626~F01650 · 25 组令牌） ---------------- */

const f0066 = family("f0066", "0066", "AI-14", "设计令牌扩展", "Design tokens extension", [1626, 1650], [
  entry(1626, "f0066", "switch", "curves", "曲线库 — 动效曲线令牌集", "Easing curve tokens", { params: { tokens: ["--ease-standard", "--ease-emphasized", "--ease-spring"] } }),
  entry(1627, "f0066", "switch", "elev6", "海拔 6 — 第 6 级海拔阴影", "Elevation-6 shadow", { params: { token: "--w2-elev-6" } }),
  entry(1628, "f0066", "switch", "blur", "模糊 — 模糊半径令牌族", "Blur radius tokens", { params: { token: "--w2-blur-1" } }),
  entry(1629, "f0066", "switch", "glass", "玻璃 — 玻璃材质令牌族", "Glass tokens", { params: { token: "--w2-glass" } }),
  entry(1630, "f0066", "switch", "glow", "发光 — 辉光强度令牌", "Glow token", { params: { token: "--w2-glow" } }),
  entry(1631, "f0066", "switch", "stroke", "描边 — 描边宽度令牌", "Stroke width token", { params: { token: "--w2-stroke" } }),
  entry(1632, "f0066", "switch", "divider", "分割线 — 分割线粗细/颜色", "Divider tokens", { params: { token: "--w2-divider" } }),
  entry(1633, "f0066", "switch", "table-density", "表格密度 — 表格行高令牌", "Table row-height token", { params: { token: "--w2-row-h" } }),
  entry(1634, "f0066", "switch", "form-density", "表单密度 — 控件高度令牌", "Control height token", { params: { token: "--w2-control-h" } }),
  entry(1635, "f0066", "switch", "optical", "光学补偿 — 图标光学对齐量", "Optical offset token", { params: { token: "--w2-optical" } }),
  entry(1636, "f0066", "switch", "text-contrast", "文本对比 — 正文对比度令牌", "Body contrast token", { params: { token: "--w2-body-contrast" } }),
  entry(1637, "f0066", "switch", "disabled-alpha", "禁用透明 — 禁用态透明度", "Disabled opacity", { params: { token: "--w2-disabled-alpha" } }),
  entry(1638, "f0066", "switch", "hover-lift", "hover 抬升 — 悬停抬升位移", "Hover lift", { params: { token: "--w2-hover-lift" } }),
  entry(1639, "f0066", "switch", "press-shift", "按压位移 — 按下 1px 位移", "Press shift 1px", { params: { token: "--w2-press-shift" } }),
  entry(1640, "f0066", "switch", "focus-ring", "焦点环 — 焦点环宽度/色", "Focus ring token", { params: { token: "--w2-focus-ring" } }),
  entry(1641, "f0066", "switch", "scrollbar", "滚动条 — 滚动条尺寸色", "Scrollbar tokens", { params: { token: "--w2-scrollbar" } }),
  entry(1642, "f0066", "switch", "tooltip", "提示 — tooltip 令牌", "Tooltip tokens", { params: { token: "--w2-tooltip" } }),
  entry(1643, "f0066", "switch", "menu-shadow", "菜单阴影 — 菜单专用阴影", "Menu shadow", { params: { token: "--w2-menu-shadow" } }),
  entry(1644, "f0066", "switch", "scope", "作用域 — 窗口级作用域规范", "Window scope spec", {}),
  entry(1645, "f0066", "switch", "override-rules", "组件覆写 — 组件级覆写规则", "Component override rules", {}),
  entry(1646, "f0066", "switch", "debug-panel", "调试面板 — 实时令牌调试器", "Live token debugger", {}),
  entry(1647, "f0066", "switch", "export", "导出 — CSS/JSON 双导出", "CSS / JSON export", {}),
  entry(1648, "f0066", "switch", "migration", "迁移 — 版本迁移脚本", "Version migration", {}),
  entry(1649, "f0066", "switch", "audit-ci", "审计加强 — inline style 裸值检测进 CI", "Raw-value audit in CI", {}),
  entry(1650, "f0066", "switch", "docs", "文档站 — 令牌文档页", "Token docs page", {}),
]);

/* ---------------- 族0067 暗色与亮度（F01651~F01675 · 25 档明暗方案） ---------------- */
/* 主题档写 theme 覆写（logic.ts themeVars），功能档为独立开关；时段/计划判定纯函数。 */

const theme = (bg: string, surface: string, raised: string, text: string, text2: string, accent: string) => ({
  vars: {
    "--bg-canvas": bg,
    "--bg-surface": surface,
    "--bg-raised": raised,
    "--text-primary": text,
    "--text-secondary": text2,
    "--accent": accent,
  },
});

const f0067 = family("f0067", "0067", "AI-14", "暗色与亮度", "Dark & brightness", [1651, 1675], [
  entry(1651, "f0067", "preset", "deep-black", "深空黑 — OLED 纯黑主题", "OLED deep black", { ...theme("oklch(0 0 0)", "oklch(0.08 0.01 262 / 0.9)", "oklch(0.1 0.01 262)", "oklch(0.92 0.01 262)", "oklch(0.62 0.02 262)", "oklch(0.7 0.12 262)") }),
  entry(1652, "f0067", "preset", "charcoal", "炭灰 — 深灰主题", "Charcoal", { ...theme("oklch(0.16 0.01 260)", "oklch(0.2 0.012 260 / 0.85)", "oklch(0.24 0.012 260)", "oklch(0.9 0.01 260)", "oklch(0.64 0.02 260)", "oklch(0.68 0.1 260)") }),
  entry(1653, "f0067", "preset", "warm-night", "暖夜 — 暖色调暗主题", "Warm night", { ...theme("oklch(0.15 0.02 60)", "oklch(0.19 0.025 60 / 0.88)", "oklch(0.23 0.025 60)", "oklch(0.9 0.03 70)", "oklch(0.64 0.04 70)", "oklch(0.7 0.11 60)") }),
  entry(1654, "f0067", "preset", "cool-night", "冷夜 — 冷色调暗主题", "Cool night", { ...theme("oklch(0.14 0.025 240)", "oklch(0.18 0.03 240 / 0.88)", "oklch(0.22 0.03 240)", "oklch(0.9 0.02 240)", "oklch(0.63 0.03 240)", "oklch(0.7 0.12 230)") }),
  entry(1655, "f0067", "preset", "pure-white", "纯白 — 纯白主题", "Pure white", { ...theme("oklch(1 0 0)", "oklch(0.98 0.002 260 / 0.9)", "oklch(0.99 0.001 260)", "oklch(0.22 0.01 260)", "oklch(0.5 0.02 260)", "oklch(0.55 0.1 262)") }),
  entry(1656, "f0067", "preset", "warm-white", "暖白 — 奶油暖白", "Cream warm white", { ...theme("oklch(0.97 0.012 85)", "oklch(0.99 0.01 85 / 0.9)", "oklch(0.995 0.008 85)", "oklch(0.25 0.02 80)", "oklch(0.52 0.03 80)", "oklch(0.58 0.1 70)") }),
  entry(1657, "f0067", "preset", "paper-white", "纸白 — 纸张白", "Paper white", { ...theme("oklch(0.96 0.008 95)", "oklch(0.98 0.006 95 / 0.9)", "oklch(0.99 0.005 95)", "oklch(0.26 0.015 95)", "oklch(0.5 0.02 95)", "oklch(0.56 0.09 100)") }),
  entry(1658, "f0067", "preset", "linen", "米白 — 亚麻米白", "Linen", { ...theme("oklch(0.94 0.014 90)", "oklch(0.97 0.01 90 / 0.9)", "oklch(0.98 0.008 90)", "oklch(0.27 0.02 85)", "oklch(0.52 0.025 85)", "oklch(0.57 0.1 60)") }),
  entry(1659, "f0067", "switch", "daypart", "按时段 — 日夜自动", "Auto day/night", { params: { dayFrom: 7, nightFrom: 19 } }),
  entry(1660, "f0067", "switch", "follow-wallpaper", "随壁纸 — 壁纸明暗联动", "Follow wallpaper luminance", {}),
  entry(1661, "f0067", "switch", "follow-system", "随系统 — 跟随系统设置", "Follow OS setting", {}),
  entry(1662, "f0067", "switch", "transition", "过渡动画 — 切换日出日落过渡", "Sunrise/sunset transition", { params: { ms: 400 } }),
  entry(1663, "f0067", "reserved", "ambient", "环境光 — 光感传感器预留", "Ambient sensor reserved", { note: { zh: "预留位：无光感硬件时隐藏入口（守卫 §15.1）", en: "Reserved: hidden without sensor" } }),
  entry(1664, "f0067", "switch", "low-bright-soft", "低亮护眼 — 低亮度下降低对比", "Low-brightness contrast down", {}),
  entry(1665, "f0067", "switch", "high-bright-boost", "高亮增强 — 强光下增强对比", "High-light contrast boost", {}),
  entry(1666, "f0067", "preset", "contrast-4", "对比四档 — 对比度四档", "Contrast 4 tiers", { params: { tier: 2 } }),
  entry(1667, "f0067", "switch", "text-bold", "文字加粗 — 全局字重+1 选项", "Global weight +1", {}),
  entry(1668, "f0067", "switch", "border-bold", "界面加粗 — 边框加粗渲染", "Bolder borders", {}),
  entry(1669, "f0067", "preset", "night-read", "夜读 — 夜读书模式", "Night reading", { ...theme("oklch(0.11 0.02 50)", "oklch(0.14 0.025 50 / 0.9)", "oklch(0.17 0.025 50)", "oklch(0.85 0.04 60)", "oklch(0.6 0.04 60)", "oklch(0.65 0.1 50)") }),
  entry(1670, "f0067", "preset", "morning-read", "晨读 — 晨间清爽模式", "Morning reading", { ...theme("oklch(0.96 0.015 220)", "oklch(0.98 0.012 220 / 0.9)", "oklch(0.99 0.01 220)", "oklch(0.26 0.02 230)", "oklch(0.5 0.03 230)", "oklch(0.55 0.1 240)") }),
  entry(1671, "f0067", "preset", "darkroom", "暗房 — 摄影暗房模式", "Photographic darkroom", { ...theme("oklch(0.06 0.005 40)", "oklch(0.09 0.005 40 / 0.95)", "oklch(0.11 0.005 40)", "oklch(0.82 0.02 50)", "oklch(0.55 0.02 50)", "oklch(0.62 0.14 30)") }),
  entry(1672, "f0067", "switch", "cinema-lock", "影院恒暗 — 播片锁定暗", "Cinema locked dim", {}),
  entry(1673, "f0067", "switch", "scheduled", "定时切换 — 定时明暗计划", "Scheduled switching", { params: { slots: 4 } }),
  entry(1674, "f0067", "preset", "transition-dur", "过渡时长 — 过渡时长令牌", "Transition duration token", { vars: { "--w2-theme-dur": "240ms" } }),
  entry(1675, "f0067", "switch", "hotkey", "热键 — 明暗一键切换", "Toggle hotkey", {}),
]);

/* ---------------- 族0068 光标与指针（F01676~F01700 · 25 项光标能力） ---------------- */
/* 光标层（trail/ripple/反色环/找回闪烁）由 runners.ts 的 CursorLayer 真实实现。 */

const f0068 = family("f0068", "0068", "AI-14", "光标与指针", "Cursor & pointer", [1676, 1700], [
  entry(1676, "f0068", "preset", "theme-pack", "主题包 — 五款光标主题", "5 cursor themes", { params: { themes: 5, pick: 1 } }),
  entry(1677, "f0068", "preset", "size", "尺寸 — 五档光标大小", "5 size tiers", { params: { tiers: [1, 1.25, 1.5, 2, 2.5] } }),
  entry(1678, "f0068", "switch", "invert", "反色 — 自动反色光标", "Auto-invert cursor", {}),
  entry(1679, "f0068", "switch", "shadow", "阴影 — 光标投影开关", "Cursor shadow", {}),
  entry(1680, "f0068", "preset", "trail", "拖尾 — 三档拖尾特效", "Trail 3 tiers", { params: { dots: 12, decay: 0.85 } }),
  entry(1681, "f0068", "switch", "click-ripple", "点击涟漪 — 点击涟漪", "Click ripple", { params: { rippleMs: 360 } }),
  entry(1682, "f0068", "switch", "press-color", "按下变色 — 按压变色", "Press color shift", {}),
  entry(1683, "f0068", "switch", "busy", "忙碌 — 忙碌转圈光标", "Busy spinner", {}),
  entry(1684, "f0068", "switch", "ibeam-bold", "I 型加粗 — 文本光标加粗", "Bold I-beam", {}),
  entry(1685, "f0068", "switch", "link-hand", "手型统一 — 链接手型一致", "Unified link hand", {}),
  entry(1686, "f0068", "switch", "copy-cursor", "复制态 — 拖拽复制光标", "Copy cursor", {}),
  entry(1687, "f0068", "switch", "no-cursor", "禁用态 — 禁止光标", "Forbidden cursor", {}),
  entry(1688, "f0068", "switch", "precision", "精准 — 精确定位放大镜", "Precision loupe", {}),
  entry(1689, "f0068", "switch", "crosshair", "十字 — 十字光标", "Crosshair", {}),
  entry(1690, "f0068", "switch", "pen-tip", "触控笔 — 笔尖光标", "Pen tip cursor", {}),
  entry(1691, "f0068", "switch", "touch-hide", "触屏隐藏 — 触屏隐藏光标", "Hide on touch", {}),
  entry(1692, "f0068", "preset", "speed-curve", "速度曲线 — 移动速度曲线", "Speed curve", { params: { gain: 1 } }),
  entry(1693, "f0068", "switch", "accel", "加速开关 — 系统加速开关", "Acceleration toggle", {}),
  entry(1694, "f0068", "switch", "learning", "学习 — 手速自适应", "Speed auto-adapt", {}),
  entry(1695, "f0068", "preset", "dbl-speed", "双击速度 — 双击间隔调节", "Double-click interval", { params: { ms: 500 } }),
  entry(1696, "f0068", "preset", "hover-delay", "悬停延时 — 悬停触发时长", "Hover trigger delay", { params: { ms: 400 } }),
  entry(1697, "f0068", "switch", "cross-screen-anim", "穿边动画 — 跨屏穿边动画", "Screen-cross animation", {}),
  entry(1698, "f0068", "switch", "find", "找回 — 迷失时按键闪烁", "Find-cursor flash", { params: { flashMs: 800 } }),
  entry(1699, "f0068", "switch", "new-win-center", "新窗居中 — 新窗自动移到光标", "New window to cursor", {}),
  entry(1700, "f0068", "switch", "stats", "统计 — 移动距离统计彩蛋", "Travel distance easter egg", {}),
]);

/* ---------------- 族0069 窗口内容风格（F01701~F01725 · 25 项内容风格） ---------------- */
/* 审计函数在 logic.ts：auditSpacing4 / auditTypeScale，单测覆盖。 */

const f0069 = family("f0069", "0069", "AI-14", "窗口内容风格", "Content style", [1701, 1725], [
  entry(1701, "f0069", "preset", "radius", "圆角体系 — 全局圆角三档统一", "Radius 3 tiers", { params: { tier: "mid" } }),
  entry(1702, "f0069", "switch", "spacing-audit", "内距节奏 — 4 倍数间距审计", "4px spacing audit", {}),
  entry(1703, "f0069", "switch", "type-audit", "字阶审计 — 1.25 字阶使用审计", "1.25 type-scale audit", {}),
  entry(1704, "f0069", "switch", "line-height", "行高 — 行高令牌规范", "Line-height token", { params: { token: "--w2-lh" } }),
  entry(1705, "f0069", "switch", "weight-scale", "字重 — 字重阶梯规范", "Weight ladder", {}),
  entry(1706, "f0069", "switch", "cjk-render", "中文渲染 — 中文渲染优化", "CJK rendering", {}),
  entry(1707, "f0069", "switch", "kerning", "kerning — 西文字距优化", "Latin kerning", {}),
  entry(1708, "f0069", "switch", "tabular-num", "tabular — 数字等宽表格", "Tabular numerals", { params: { feature: "tnum" } }),
  entry(1709, "f0069", "switch", "zebra", "斑马纹 — 表格斑马纹规范", "Table zebra", {}),
  entry(1710, "f0069", "switch", "divider-density", "分割线 — 分割线密度规范", "Divider density", {}),
  entry(1711, "f0069", "switch", "accent-audit", "强调色审计 — 强调色滥用审计", "Accent misuse audit", {}),
  entry(1712, "f0069", "switch", "link-style", "链接统一 — 链接样式统一", "Unified links", {}),
  entry(1713, "f0069", "switch", "button-hierarchy", "按钮层级 — 主/次/幽灵层级", "Button hierarchy", {}),
  entry(1714, "f0069", "switch", "focus-unify", "焦点统一 — 输入焦点统一样式", "Unified input focus", {}),
  entry(1715, "f0069", "switch", "selection-color", "选择态 — 文本选择色统一", "Unified selection color", { params: { token: "--w2-selection" } }),
  entry(1716, "f0069", "switch", "hover-card", "悬浮卡 — 悬浮卡统一规范", "Hover card spec", {}),
  entry(1717, "f0069", "switch", "empty-art", "空态插画 — 空态插画体系统一", "Empty-state art", {}),
  entry(1718, "f0069", "switch", "error-tone", "错误语气 — 错误文案语气规范", "Error copy tone", {}),
  entry(1719, "f0069", "switch", "success-tone", "成功语气 — 成功文案规范", "Success copy tone", {}),
  entry(1720, "f0069", "switch", "skeleton", "骨架屏 — 加载骨架统一", "Unified skeleton", {}),
  entry(1721, "f0069", "switch", "microcopy", "微文案 — 微文案语气手册", "Microcopy handbook", {}),
  entry(1722, "f0069", "switch", "glossary", "术语表 — 术语统一表", "Glossary", {}),
  entry(1723, "f0069", "switch", "mixed-text", "双语混排 — 中英混排规范", "CJK/Latin mixing rules", {}),
  entry(1724, "f0069", "switch", "hanging-punct", "标点悬挂 — 标点悬挂排版", "Hanging punctuation", {}),
  entry(1725, "f0069", "preset", "content-zoom", "内容缩放 — 110/125/150 缩放", "Content zoom 110/125/150", { params: { options: [110, 125, 150] } }),
]);

/* ---------------- 族0070 视觉动效全局（F01726~F01750 · 25 项全局动效） ---------------- */

const f0070 = family("f0070", "0070", "AI-14", "视觉动效全局", "Global motion", [1726, 1750], [
  entry(1726, "f0070", "preset", "intensity", "强度滑杆 — 全局 0~100%", "Intensity 0-100%", { params: { percent: 100 } }),
  entry(1727, "f0070", "switch", "master-off", "总开关 — 一键关闭全部动效", "Motion master off", { vars: { "--dur-scale": "0" } }),
  entry(1728, "f0070", "switch", "reduce-motion", "减少动态 — 前庭安全档", "Vestibular-safe reduce", { params: { maxMs: 80 } }),
  entry(1729, "f0070", "switch", "unify-transition", "转场统一 — 转场时长统一令牌", "Unified transition token", { params: { token: "--dur-3" } }),
  entry(1730, "f0070", "preset", "micro-dur", "微时长 — 100/150/200 三档", "Micro durations", { params: { options: [100, 150, 200] } }),
  entry(1731, "f0070", "preset", "macro-dur", "大时长 — 300/400 两档", "Macro durations", { params: { options: [300, 400] } }),
  entry(1732, "f0070", "preset", "spring", "弹性系 — spring 系数体系", "Spring system", { params: { overshoot: 0.04, v0: -0.2 } }),
  entry(1733, "f0070", "preset", "damping", "阻尼系 — 阻尼参数体系", "Damping system", { params: { zeta: 1 } }),
  entry(1734, "f0070", "preset", "stagger", "stagger — 列表交错步长令牌", "Stagger step token", { params: { ms: 30 } }),
  entry(1735, "f0070", "preset", "enter-origin", "入场方向 — 从光标/边缘入场", "Enter origin", { params: { origin: "cursor" } }),
  entry(1736, "f0070", "preset", "exit-direction", "退场方向 — 退场方向规范", "Exit direction", { params: { dir: "scale-down" } }),
  entry(1737, "f0070", "switch", "scroll-parallax", "滚动视差 — 列表视差开关", "Scroll parallax", {}),
  entry(1738, "f0070", "preset", "parallax-strength", "视差强度 — 视差幅度三档", "Parallax 3 tiers", { params: { tiers: [0.2, 0.5, 1] } }),
  entry(1739, "f0070", "preset", "scroll-damping", "滚动阻尼 — 滚动阻尼手感", "Scroll damping", { params: { damping: 0.3 } }),
  entry(1740, "f0070", "switch", "overscroll-bounce", "滚动回弹 — 端点回弹开关", "Overscroll bounce", {}),
  entry(1741, "f0070", "preset", "inertia-curve", "惯性曲线 — 惯性衰减曲线", "Inertia decay curve", { params: { decay: 0.95 } }),
  entry(1742, "f0070", "switch", "list-stagger", "列表交错 — 列表入场 stagger", "List enter stagger", {}),
  entry(1743, "f0070", "preset", "modal-curve", "模态曲线 — 弹窗升起曲线", "Modal rise curve", { params: { ease: "var(--ease-emphasized)" } }),
  entry(1744, "f0070", "preset", "drawer-curve", "抽屉曲线 — 抽屉专用曲线", "Drawer curve", { params: { ease: "var(--ease-standard)" } }),
  entry(1745, "f0070", "switch", "ripple-master", "涟漪开关 — 涟漪总开关", "Ripple master", {}),
  entry(1746, "f0070", "switch", "particles-master", "粒子总开关 — 彩蛋粒子开关", "Particles master", {}),
  entry(1747, "f0070", "switch", "fps-budget", "FPS 预算 — 动效帧预算", "Motion frame budget", { params: { budgetMs: 8 } }),
  entry(1748, "f0070", "switch", "degrade-chain", "降级链 — 分级降级策略", "Degrade ladder", {}),
  entry(1749, "f0070", "switch", "ab-fav", "A/B 收藏 — 动效方案收藏切换", "A/B scheme favorite", {}),
  entry(1750, "f0070", "switch", "guide", "教学 — 动效体系图解", "Motion system guide", {}),
]);

export const AI14_FAMILIES: DesignFamily[] = [f0066, f0067, f0068, f0069, f0070];
