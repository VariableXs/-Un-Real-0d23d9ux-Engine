/**
 * AURORA-10000 · AI-15 桌面个性组E（领域03 · F01751~F01875 · W2）
 * 族0071 主题系统｜族0072 个性化深度｜族0073 季节与节日
 * 族0074 桌面剧场模式｜族0075 桌面仪式感
 *
 * - 族0071 主题变体经 [data-w2-theme] 覆写语义令牌（与族0067 同一机制）；
 * - 族0073 节日命中为纯函数 festivalOn（logic.ts 内置 2024~2029 农历/节气表）；
 * - 族0074 剧场由 TheaterOverlay.tsx 全屏画布真实渲染（粒子类型 + WebAudio 音景）；
 * - 族0075 仪式由 ritualDue（logic.ts）+ RitualOverlay 落成真实问候/庆祝卡。
 */
import { entry, family, type DesignFamily } from "./types";

/* ---------------- 族0071 主题系统（F01751~F01775 · 25 套主题） ---------------- */

const th = (bg: string, surface: string, raised: string, text: string, text2: string, accent: string) => ({
  vars: {
    "--bg-canvas": bg,
    "--bg-surface": surface,
    "--bg-raised": raised,
    "--text-primary": text,
    "--text-secondary": text2,
    "--accent": accent,
  },
});

const f0071 = family("f0071", "0071", "AI-15", "主题系统", "Theme system", [1751, 1775], [
  entry(1751, "f0071", "preset", "ink", "墨 — 水墨主题", "Ink", { ...th("oklch(0.94 0.005 260)", "oklch(0.97 0.004 260 / 0.92)", "oklch(0.98 0.003 260)", "oklch(0.2 0.01 260)", "oklch(0.45 0.015 260)", "oklch(0.35 0.02 260)") }),
  entry(1752, "f0071", "preset", "mist", "雾 — 朦胧雾灰主题", "Mist", { ...th("oklch(0.8 0.01 260)", "oklch(0.86 0.01 260 / 0.85)", "oklch(0.9 0.008 260)", "oklch(0.28 0.01 260)", "oklch(0.5 0.015 260)", "oklch(0.6 0.05 260)") }),
  entry(1753, "f0071", "preset", "forest", "森 — 森林绿主题", "Forest", { ...th("oklch(0.16 0.03 150)", "oklch(0.2 0.035 150 / 0.88)", "oklch(0.25 0.035 150)", "oklch(0.9 0.03 140)", "oklch(0.62 0.04 150)", "oklch(0.7 0.14 150)") }),
  entry(1754, "f0071", "preset", "tide", "汐 — 海洋蓝主题", "Tide", { ...th("oklch(0.15 0.035 230)", "oklch(0.2 0.04 230 / 0.88)", "oklch(0.25 0.04 230)", "oklch(0.9 0.025 230)", "oklch(0.62 0.04 230)", "oklch(0.72 0.12 220)") }),
  entry(1755, "f0071", "preset", "flame", "焰 — 暖焰橙主题", "Flame", { ...th("oklch(0.16 0.03 40)", "oklch(0.21 0.04 40 / 0.88)", "oklch(0.26 0.04 40)", "oklch(0.9 0.035 50)", "oklch(0.63 0.05 50)", "oklch(0.68 0.16 45)") }),
  entry(1756, "f0071", "preset", "snow", "雪 — 冷雪白主题", "Snow", { ...th("oklch(0.96 0.008 240)", "oklch(0.98 0.006 240 / 0.9)", "oklch(0.99 0.005 240)", "oklch(0.25 0.015 240)", "oklch(0.5 0.02 240)", "oklch(0.58 0.09 250)") }),
  entry(1757, "f0071", "preset", "night-flight", "夜航 — 深夜蓝黑主题", "Night flight", { ...th("oklch(0.1 0.03 265)", "oklch(0.14 0.035 265 / 0.9)", "oklch(0.18 0.035 265)", "oklch(0.88 0.02 265)", "oklch(0.58 0.03 265)", "oklch(0.66 0.12 275)") }),
  entry(1758, "f0071", "preset", "dawn", "晨曦 — 破晓橙粉主题", "Dawn", { ...th("oklch(0.9 0.03 40)", "oklch(0.94 0.03 30 / 0.9)", "oklch(0.96 0.025 30)", "oklch(0.26 0.04 30)", "oklch(0.5 0.06 30)", "oklch(0.6 0.14 20)") }),
  entry(1759, "f0071", "preset", "film", "胶片 — 胶片怀旧主题", "Film", { ...th("oklch(0.18 0.02 80)", "oklch(0.23 0.025 80 / 0.88)", "oklch(0.28 0.025 80)", "oklch(0.87 0.03 85)", "oklch(0.6 0.04 85)", "oklch(0.68 0.1 70)") }),
  entry(1760, "f0071", "preset", "pixel", "像素 — 复古像素主题", "Pixel", { ...th("oklch(0.15 0.05 290)", "oklch(0.2 0.06 290 / 0.9)", "oklch(0.26 0.06 290)", "oklch(0.9 0.06 300)", "oklch(0.62 0.07 300)", "oklch(0.75 0.16 320)"), params: { pixelated: true } }),
  entry(1761, "f0071", "switch", "market", "市场 — 主题市场入口", "Theme market entry", {}),
  entry(1762, "f0071", "switch", "format", "格式规范 — 主题包格式", "Theme pack format", { params: { format: "variable-theme-v1" } }),
  entry(1763, "f0071", "switch", "signature", "签名校验 — 主题包验签", "Pack signature check", {}),
  entry(1764, "f0071", "switch", "preview", "预览 — 应用前预览", "Pre-apply preview", {}),
  entry(1765, "f0071", "switch", "one-click", "一键应用 — 一键换装", "One-click apply", {}),
  entry(1766, "f0071", "switch", "partial-apply", "部分应用 — 只取色板/只取字体", "Partial apply", {}),
  entry(1767, "f0071", "switch", "mix", "混搭 — 跨主题元素混搭", "Cross-theme mixing", {}),
  entry(1768, "f0071", "switch", "daypart", "定时切换 — 按时段换主题", "Daypart switch", { params: { slots: 4 } }),
  entry(1769, "f0071", "switch", "daily-random", "随机轮换 — 每日随机", "Daily random", {}),
  entry(1770, "f0071", "switch", "author", "作者页 — 作者主页与作品集", "Author page", {}),
  entry(1771, "f0071", "switch", "rating", "评分 — 评分与评论", "Rating & reviews", {}),
  entry(1772, "f0071", "switch", "update-hint", "更新提醒 — 主题更新提示", "Update hints", {}),
  entry(1773, "f0071", "switch", "compat", "兼容标注 — 版本兼容标注", "Compat labels", {}),
  entry(1774, "f0071", "switch", "rollback", "回滚备份 — 主题备份回滚", "Backup rollback", {}),
  entry(1775, "f0071", "switch", "share-code", "分享码 — 一串码分享主题", "Share code", {}),
]);

/* ---------------- 族0072 个性化深度（F01776~F01800 · 25 项个性化） ---------------- */

const f0072 = family("f0072", "0072", "AI-15", "个性化深度", "Personalization depth", [1776, 1800], [
  entry(1776, "f0072", "preset", "accent-picker", "任意强调色 — 拾色器任意色", "Any accent color", { params: { token: "--accent" } }),
  entry(1777, "f0072", "preset", "derive", "派生算法 — 强调色派生策略选择", "Accent derivation strategy", { params: { strategies: ["oklch-clamp", "hsl-rotate", "contrast-safe"] } }),
  entry(1778, "f0072", "switch", "window-tint", "窗口着色 — 单窗独立颜色", "Per-window tint", {}),
  entry(1779, "f0072", "switch", "taskbar-tint", "任务栏着色 — 任务栏独立色", "Taskbar tint", {}),
  entry(1780, "f0072", "switch", "startmenu-tint", "开始菜单着色 — 菜单独立色", "Start menu tint", {}),
  entry(1781, "f0072", "switch", "notify-tint", "通知着色 — 通知卡独立色", "Notification tint", {}),
  entry(1782, "f0072", "switch", "selection", "选中色 — 文本选中色", "Selection color", { params: { token: "--w2-selection" } }),
  entry(1783, "f0072", "switch", "link-color", "链接色 — 链接颜色", "Link color", {}),
  entry(1784, "f0072", "switch", "code-link", "代码联动 — 代码高亮联动", "Code highlight link", {}),
  entry(1785, "f0072", "switch", "terminal-link", "终端配色 — 终端主题联动", "Terminal theme link", {}),
  entry(1786, "f0072", "switch", "sound-pack", "声音主题 — 系统音效包", "System sound pack", {}),
  entry(1787, "f0072", "switch", "boot-sound", "开机音 — 自定义开机音", "Custom boot sound", {}),
  entry(1788, "f0072", "preset", "global-font", "全局字体 — 全局字体替换", "Global font", { params: { token: "--w2-font-ui" } }),
  entry(1789, "f0072", "preset", "mono-font", "等宽字体 — 等宽字体选择", "Monospace font", { params: { token: "--w2-font-mono" } }),
  entry(1790, "f0072", "switch", "handwriting", "手写体 — 手写体场景包", "Handwriting pack", {}),
  entry(1791, "f0072", "switch", "type-tuning", "渲染微调 — 字距/对比微调", "Letter-spacing / contrast tuning", { params: { token: "--w2-tracking" } }),
  entry(1792, "f0072", "preset", "radius-tier", "圆角档位 — 圆角全局三档", "Radius 3 tiers", { params: { tiers: [0, 8, 16] } }),
  entry(1793, "f0072", "preset", "border-width", "边框粗细 — 边框宽度档", "Border width tiers", { params: { token: "--w2-border-w" } }),
  entry(1794, "f0072", "preset", "shadow-density", "阴影浓度 — 阴影浓度滑杆", "Shadow density", { params: { token: "--w2-shadow-density" } }),
  entry(1795, "f0072", "preset", "blur-density", "模糊浓度 — 模糊浓度滑杆", "Blur density", { params: { token: "--w2-blur-density" } }),
  entry(1796, "f0072", "preset", "global-alpha", "透明全局 — 全局透明度", "Global opacity", { params: { token: "--w2-surface-alpha" } }),
  entry(1797, "f0072", "preset", "global-sat", "饱和全局 — 全局饱和度", "Global saturation", { params: { token: "--w2-sat" } }),
  entry(1798, "f0072", "preset", "temp-shift", "色温偏移 — 全局色温偏移", "Color-temp shift", { params: { token: "--w2-temp" } }),
  entry(1799, "f0072", "preset", "gamma", "伽马微调 — 伽马曲线微调", "Gamma fine-tune", { params: { token: "--w2-gamma" } }),
  entry(1800, "f0072", "switch", "profile-io", "档案导出 — 个性档案导入导出", "Profile import/export", {}),
]);

/* ---------------- 族0073 季节与节日（F01801~F01825 · 25 项节令） ---------------- */
/* 装饰粒子由 runners.ts FestivalLayer 真实渲染；命中判定 festivalOn（logic.ts）。 */

const fest = (kind: string) => ({ params: { fx: kind } });

const f0073 = family("f0073", "0073", "AI-15", "季节与节日", "Season & festival", [1801, 1825], [
  entry(1801, "f0073", "switch", "cny", "春节 — 红金主题+灯笼粒子", "CNY lantern particles", { ...fest("lantern") }),
  entry(1802, "f0073", "switch", "lantern-fest", "元宵 — 花灯微件", "Lantern widget", { ...fest("lantern") }),
  entry(1803, "f0073", "switch", "qingming", "清明 — 雨丝氛围", "Qingming rain", { ...fest("rain") }),
  entry(1804, "f0073", "switch", "duanwu", "端午 — 龙舟进度条", "Dragon-boat progress", { ...fest("rain") }),
  entry(1805, "f0073", "switch", "qixi", "七夕 — 星桥动画", "Star bridge", { ...fest("stars") }),
  entry(1806, "f0073", "switch", "mid-autumn", "中秋 — 明月玉兔角标", "Mid-autumn moon", { ...fest("stars") }),
  entry(1807, "f0073", "switch", "chongyang", "重阳 — 秋菊主题", "Chrysanthemum", { ...fest("leaves") }),
  entry(1808, "f0073", "switch", "dongzhi", "冬至 — 饺子挂件", "Dongzhi dumpling", { ...fest("snow") }),
  entry(1809, "f0073", "switch", "laba", "腊八 — 腊八粥计时", "Laba porridge timer", { ...fest("snow") }),
  entry(1810, "f0073", "switch", "chuxi", "除夕 — 倒计时全屏", "CNYE countdown", { ...fest("embers") }),
  entry(1811, "f0073", "switch", "christmas", "圣诞 — 落雪与袜", "Christmas snow", { ...fest("snow") }),
  entry(1812, "f0073", "switch", "halloween", "万圣 — 南瓜灯彩蛋", "Halloween pumpkin", { ...fest("embers") }),
  entry(1813, "f0073", "switch", "valentine", "情人节 — 玫瑰粒子", "Valentine petals", { ...fest("petals") }),
  entry(1814, "f0073", "switch", "school-start", "开学季 — 计划表微件", "School planner widget", {}),
  entry(1815, "f0073", "switch", "graduation", "毕业季 — 纪念册入口", "Graduation album", { ...fest("petals") }),
  entry(1816, "f0073", "switch", "shopping", "购物节 — 清单微件", "Shopping list widget", {}),
  entry(1817, "f0073", "switch", "worldcup", "世界杯 — 比分微件", "World cup scores", {}),
  entry(1818, "f0073", "switch", "olympics", "奥运 — 奖牌计数", "Olympic medals", {}),
  entry(1819, "f0073", "switch", "sakura", "樱花季 — 樱花飘落", "Sakura season", { ...fest("sakura") }),
  entry(1820, "f0073", "switch", "maple", "枫叶季 — 红叶飘落", "Maple season", { ...fest("leaves") }),
  entry(1821, "f0073", "switch", "rainy", "雨季 — 雨声白噪音档", "Rainy season noise", { ...fest("rain") }),
  entry(1822, "f0073", "switch", "typhoon", "台风预警 — 预警横幅", "Typhoon banner", {}),
  entry(1823, "f0073", "switch", "sakura-rain-egg", "樱花雨彩蛋 — 稀有樱花雨", "Rare sakura rain", { ...fest("sakura") }),
  entry(1824, "f0073", "switch", "meteor-egg", "流星雨彩蛋 — 随机流星雨", "Random meteor shower", { ...fest("meteor") }),
  entry(1825, "f0073", "switch", "custom", "自定义节日 — 用户自建节日", "Custom festivals", {}),
]);

/* ---------------- 族0074 桌面剧场模式（F01826~F01850 · 25 个场景） ---------------- */
/* 场景 = 粒子系统 + 色板 + 音景（TheaterOverlay.tsx 真实渲染与合成音景）。 */

const scene = (fx: string, sound: string, palette: [string, string]) => ({
  params: { fx, sound, palette },
  vars: { "--w2-scene-bg": palette[0], "--w2-scene-fg": palette[1] },
});

const f0074 = family("f0074", "0074", "AI-15", "桌面剧场模式", "Desktop theater", [1826, 1850], [
  entry(1826, "f0074", "preset", "showcase", "展示 — 隐私保护轮播", "Privacy showcase carousel", { ...scene("stars", "none", ["oklch(0.08 0.02 262)", "oklch(0.8 0.05 262)"]) }),
  entry(1827, "f0074", "preset", "frame", "相框 — 全屏数字相框", "Fullscreen photo frame", { ...scene("none", "none", ["oklch(0.1 0.01 60)", "oklch(0.9 0.01 60)"]) }),
  entry(1828, "f0074", "preset", "zen", "禅 — 只留一句话", "Zen one-liner", { ...scene("none", "wind", ["oklch(0.12 0.01 120)", "oklch(0.85 0.03 120)"]) }),
  entry(1829, "f0074", "preset", "gallery", "画廊 — 壁纸策展轮播", "Curated gallery", { ...scene("none", "none", ["oklch(0.14 0.01 260)", "oklch(0.85 0.02 260)"]) }),
  entry(1830, "f0074", "preset", "meditate", "冥想 — 呼吸引导", "Breathing guide", { ...scene("breath", "wind", ["oklch(0.1 0.02 180)", "oklch(0.75 0.08 180)"]) }),
  entry(1831, "f0074", "preset", "stargaze", "观星 — 星图全屏", "Star chart", { ...scene("stars", "none", ["oklch(0.06 0.02 265)", "oklch(0.9 0.03 80)"]) }),
  entry(1832, "f0074", "preset", "campfire", "篝火 — 篝火音画", "Campfire", { ...scene("embers", "fire", ["oklch(0.08 0.02 40)", "oklch(0.7 0.16 50)"]) }),
  entry(1833, "f0074", "preset", "rainy-window", "雨天 — 雨窗音画", "Rainy window", { ...scene("rain", "rain", ["oklch(0.1 0.02 240)", "oklch(0.7 0.05 240)"]) }),
  entry(1834, "f0074", "preset", "library", "图书馆 — 书页音画", "Library pages", { ...scene("dust", "wind", ["oklch(0.2 0.03 80)", "oklch(0.85 0.04 80)"]) }),
  entry(1835, "f0074", "preset", "cafe", "咖啡馆 — 环境音场景", "Café ambience", { ...scene("dust", "cafe", ["oklch(0.16 0.02 50)", "oklch(0.8 0.05 60)"]) }),
  entry(1836, "f0074", "preset", "zen-garden", "禅园 — 枯山水互动", "Zen rock garden", { ...scene("none", "wind", ["oklch(0.85 0.01 90)", "oklch(0.4 0.02 90)"]) }),
  entry(1837, "f0074", "preset", "aquarium", "水族馆 — 鱼群游动", "Aquarium", { ...scene("fish", "waves", ["oklch(0.1 0.04 230)", "oklch(0.75 0.1 200)"]) }),
  entry(1838, "f0074", "preset", "butterfly", "蝴蝶谷 — 蝶群飞舞", "Butterfly valley", { ...scene("butterfly", "wind", ["oklch(0.14 0.03 140)", "oklch(0.8 0.12 330)"]) }),
  entry(1839, "f0074", "preset", "fireflies", "萤火虫夜 — 萤光点点", "Fireflies night", { ...scene("fireflies", "crickets", ["oklch(0.06 0.02 150)", "oklch(0.85 0.15 130)"]) }),
  entry(1840, "f0074", "preset", "aurora", "极光夜 — 极光天幕", "Aurora night", { ...scene("aurora", "wind", ["oklch(0.05 0.02 270)", "oklch(0.8 0.15 160)"]) }),
  entry(1841, "f0074", "preset", "goodnight", "晚安篝火 — 入睡引导", "Goodnight campfire", { ...scene("embers", "fire", ["oklch(0.05 0.015 40)", "oklch(0.6 0.12 45)"]) }),
  entry(1842, "f0074", "preset", "storytime", "炉边故事 — 讲故事模式", "Hearth storytelling", { ...scene("embers", "fire", ["oklch(0.1 0.02 40)", "oklch(0.78 0.1 60)"]) }),
  entry(1843, "f0074", "preset", "cat-nest", "猫窝 — 桌面猫彩蛋", "Desktop cat", { ...scene("cat", "none", ["oklch(0.9 0.02 80)", "oklch(0.5 0.05 60)"]) }),
  entry(1844, "f0074", "preset", "goldfish", "金鱼缸 — 金鱼游动", "Goldfish bowl", { ...scene("fish", "waves", ["oklch(0.12 0.04 220)", "oklch(0.8 0.14 40)"]) }),
  entry(1845, "f0074", "preset", "rainforest", "雨林 — 雨林晨雾", "Rainforest mist", { ...scene("mist", "rain", ["oklch(0.12 0.03 150)", "oklch(0.7 0.06 150)"]) }),
  entry(1846, "f0074", "preset", "snow-cabin", "雪夜小屋 — 温暖小屋", "Snowy cabin", { ...scene("snow", "wind", ["oklch(0.08 0.015 260)", "oklch(0.85 0.05 60)"]) }),
  entry(1847, "f0074", "preset", "city-dusk", "城市黄昏 — 都市黄昏", "City dusk", { ...scene("stars", "city", ["oklch(0.12 0.04 300)", "oklch(0.8 0.12 60)"]) }),
  entry(1848, "f0074", "preset", "lighthouse", "灯塔守望 — 灯塔光束", "Lighthouse beam", { ...scene("lighthouse", "waves", ["oklch(0.07 0.02 250)", "oklch(0.9 0.1 90)"]) }),
  entry(1849, "f0074", "preset", "onsen", "温泉旅馆 — 汤雾袅袅", "Onsen steam", { ...scene("mist", "waves", ["oklch(0.14 0.02 20)", "oklch(0.82 0.03 40)"]) }),
  entry(1850, "f0074", "preset", "custom", "自定义 — 自组合场景编辑器", "Custom scene editor", { ...scene("stars", "none", ["oklch(0.1 0.01 262)", "oklch(0.8 0.05 262)"]) }),
]);

/* ---------------- 族0075 桌面仪式感（F01851~F01875 · 25 项仪式） ---------------- */
/* 触发判定 ritualDue（logic.ts 纯函数）；RitualOverlay 渲染真实问候/庆祝卡。 */

const f0075 = family("f0075", "0075", "AI-15", "桌面仪式感", "Desktop rituals", [1851, 1875], [
  entry(1851, "f0075", "switch", "greeting", "开机问候 — 时段+天气+日程问候卡", "Boot greeting card", { params: { by: "daypart" } }),
  entry(1852, "f0075", "switch", "friday-bye", "周五告别 — 周五下班告别动画", "Friday farewell", { params: { weekday: 5, hour: 18 } }),
  entry(1853, "f0075", "switch", "birthday-boot", "生日开机 — 生日当天专属开机", "Birthday boot", {}),
  entry(1854, "f0075", "switch", "hundred-days", "百日纪念 — 连续使用 100 天纪念", "100-day milestone", { params: { days: 100 } }),
  entry(1855, "f0075", "switch", "deep-night-line", "深夜文案 — 深夜时段专属提示语", "Late-night line", { params: { from: 23, to: 5 } }),
  entry(1856, "f0075", "switch", "morning-water", "晨间饮水 — 清晨第一杯水提醒卡", "First glass of water", { params: { hour: 8 } }),
  entry(1857, "f0075", "switch", "annual-leave", "年假倒数 — 年假倒计时卡", "Annual-leave countdown", {}),
  entry(1858, "f0075", "switch", "project-dday", "项目 D-Day — 里程碑倒计时卡", "Project D-Day", {}),
  entry(1859, "f0075", "switch", "anniversary", "纪念日 — 纪念日提醒卡", "Anniversary card", {}),
  entry(1860, "f0075", "switch", "year-goal", "目标进度 — 年度目标进度桌面卡", "Year goal progress", {}),
  entry(1861, "f0075", "switch", "achieve-celebrate", "成就庆祝 — 成就弹窗庆祝动效", "Achievement confetti", { params: { fx: "confetti" } }),
  entry(1862, "f0075", "switch", "weekly-review", "周报仪式 — 每周回顾生成仪式", "Weekly review ritual", { params: { weekday: 5 } }),
  entry(1863, "f0075", "switch", "monthly-look", "月度回顾 — 月度使用回顾卡", "Monthly lookback", {}),
  entry(1864, "f0075", "switch", "year-book", "年度总结 — 年度总结电子册", "Yearbook", {}),
  entry(1865, "f0075", "switch", "newyear-count", "跨年倒数 — 跨年全屏倒计时", "New Year countdown", { params: { date: "12-31" } }),
  entry(1866, "f0075", "switch", "newyear-bell", "新年钟声 — 跨年钟声音效", "New Year bell", {}),
  entry(1867, "f0075", "switch", "back-to-work", "开工第一天 — 假后开工仪式", "Back-to-work ritual", {}),
  entry(1868, "f0075", "switch", "ribbon", "完工彩带 — 项目完成彩带动效", "Project ribbon", { params: { fx: "confetti" } }),
  entry(1869, "f0075", "switch", "closing-time", "打烊模式 — 下班关机仪式", "Closing-time ritual", { params: { hour: 18 } }),
  entry(1870, "f0075", "switch", "coffee-time", "咖啡时间 — 咖啡休息提醒卡", "Coffee time", { params: { hour: 15 } }),
  entry(1871, "f0075", "switch", "tea-break", "茶歇模式 — 下午茶放松档", "Tea break mode", {}),
  entry(1872, "f0075", "switch", "lunch-break", "午休模式 — 午休勿扰+闹钟", "Lunch break DND", { params: { from: 12, to: 13 } }),
  entry(1873, "f0075", "switch", "goodnight", "晚安模式 — 睡前提醒渐暗", "Goodnight dim", { params: { hour: 22 } }),
  entry(1874, "f0075", "switch", "morning-read", "晨读模式 — 晨间专注档", "Morning focus mode", { params: { hour: 7 } }),
  entry(1875, "f0075", "switch", "custom-editor", "自定义编辑器 — 自建仪式时间轴", "Custom ritual timeline", {}),
]);

export const AI15_FAMILIES: DesignFamily[] = [f0071, f0072, f0073, f0074, f0075];
