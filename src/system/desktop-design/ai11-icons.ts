/**
 * AURORA-10000 · AI-11 图标系统组A（领域03 · F01251~F01375 · W2）
 * 族0051 图标风格体系｜族0052 图标动效｜族0053 图标栅格与密度
 * 族0054 图标语义色｜族0055 图标状态机
 *
 * 参数档全部汇入 --w2-icon-* / --w2-sem-* 令牌，由 runtime.ts 写入 :root；
 * 设计中心提供实时预览网格（同一套变量驱动，改档立即可见）。
 */
import { entry, family, type DesignFamily } from "./types";

/* ---------------- 族0051 图标风格体系（F01251~F01275 · 25 款风格包） ---------------- */
/* 每款 = 独立参数档：线重 / 面性 / 双色 / 圆角 / 饱和度 / 发光。 */

const styleVar = (stroke: string, fill: string, duo: string, sat: string, glow: string) => ({
  "--w2-icon-stroke": stroke,
  "--w2-icon-fill": fill,
  "--w2-icon-duo": duo,
  "--w2-icon-sat": sat,
  "--w2-icon-glow": glow,
});

const f0051 = family("f0051", "0051", "AI-11", "图标风格体系", "Icon style system", [1251, 1275], [
  entry(1251, "f0051", "preset", "linear", "线性 — 1.5px 统一线重图标包", "Linear — 1.5px uniform stroke", { vars: styleVar("1.5px", "0", "0", "1", "0") }),
  entry(1252, "f0051", "preset", "filled", "面性 — 实心面填充包", "Filled — solid glyph", { vars: styleVar("1px", "1", "0", "1", "0") }),
  entry(1253, "f0051", "preset", "duotone", "双色 — 主色+辅色双色包", "Duotone — primary + secondary", { vars: styleVar("1.5px", "0.35", "1", "1", "0") }),
  entry(1254, "f0051", "preset", "flat", "扁平 — 纯扁平无阴影包", "Flat — shadowless", { vars: styleVar("1px", "1", "0", "1.1", "0") }),
  entry(1255, "f0051", "preset", "soft-3d", "轻拟物 — 微渐变轻立体包", "Soft 3D — light gradient", { vars: styleVar("1.25px", "0.8", "0", "1", "0.08") }),
  entry(1256, "f0051", "preset", "clay", "粘土 3D — 软糖粘土质感包", "Clay 3D — gummy texture", { vars: styleVar("2px", "1", "0", "1.2", "0.12") }),
  entry(1257, "f0051", "preset", "glass", "玻璃拟态 — 半透玻璃包", "Glassmorphism", { vars: styleVar("1.5px", "0.25", "0", "1", "0.16") }),
  entry(1258, "f0051", "preset", "pixel", "像素 — 16px 复古像素包", "Pixel — retro 16px", { vars: styleVar("2px", "1", "0", "1.15", "0") , params: { pixelated: true } }),
  entry(1259, "f0051", "preset", "sketch", "手绘 — 蜡笔手绘线稿包", "Hand-drawn crayon", { vars: styleVar("1.75px", "0.15", "0", "0.9", "0") , params: { jitter: 0.6 } }),
  entry(1260, "f0051", "preset", "watercolor", "水彩 — 水彩晕染包", "Watercolor bloom", { vars: styleVar("1.25px", "0.45", "0", "0.95", "0.10") , params: { blur: "0.4px" } }),
  entry(1261, "f0051", "preset", "oil", "油画 — 厚涂笔触包", "Oil impasto", { vars: styleVar("2px", "0.9", "0", "1.05", "0.06") , params: { blur: "0.3px" } }),
  entry(1262, "f0051", "preset", "chalk", "粉笔 — 黑板粉笔包", "Chalkboard", { vars: styleVar("1.5px", "0.2", "0", "0.55", "0.04") }),
  entry(1263, "f0051", "preset", "papercut", "剪纸 — 层叠剪纸包", "Layered papercut", { vars: styleVar("1px", "1", "1", "1.1", "0.10") }),
  entry(1264, "f0051", "preset", "relief", "浮雕 — 石雕浮雕包", "Stone relief", { vars: styleVar("1px", "0.7", "0", "0.6", "0.05") , params: { emboss: true } }),
  entry(1265, "f0051", "preset", "stitch", "刺绣 — 十字绣纹理包", "Cross-stitch", { vars: styleVar("2px", "0.6", "0", "1", "0") , params: { pixelated: true } }),
  entry(1266, "f0051", "preset", "woodcut", "木刻 — 版画木刻包", "Woodcut", { vars: styleVar("2.5px", "0.85", "0", "0.7", "0") }),
  entry(1267, "f0051", "preset", "etch", "铜版 — 蚀刻铜版画包", "Copper etching", { vars: styleVar("1.25px", "0.3", "0", "0.5", "0.03") }),
  entry(1268, "f0051", "preset", "neon-flat", "霓虹平涂 — 霓虹色块包", "Neon flat", { vars: styleVar("1px", "1", "0", "1.4", "0.18") }),
  entry(1269, "f0051", "preset", "neon-tube", "霓虹灯管 — 发光灯管描边包", "Neon tube outline", { vars: styleVar("2px", "0", "0", "1.35", "0.30") }),
  entry(1270, "f0051", "preset", "fluoro", "荧光 — 荧光高饱和包", "Fluorescent", { vars: styleVar("1.5px", "0.5", "0", "1.6", "0.14") }),
  entry(1271, "f0051", "preset", "xray", "X 光 — 透视骨骼包", "X-ray", { vars: styleVar("1px", "0.1", "0", "0.35", "0.08") , params: { invert: true } }),
  entry(1272, "f0051", "preset", "blueprint", "蓝图 — 白线蓝图包", "Blueprint white-line", { vars: styleVar("1.25px", "0.12", "0", "0.6", "0.06") , params: { tint: "#3a6ea5" } }),
  entry(1273, "f0051", "preset", "isometric", "等距 — 等距立体包", "Isometric", { vars: styleVar("1.5px", "0.6", "1", "1", "0.05") , params: { tilt: "-6deg" } }),
  entry(1274, "f0051", "preset", "mono-hair", "极简单色 — 单色极细包", "Monochrome hairline", { vars: styleVar("1px", "0", "0", "0", "0") }),
  entry(1275, "f0051", "preset", "double-outline", "极简描边 — 双线描边包", "Double outline", { vars: styleVar("1.25px", "0", "1", "0.9", "0") }),
]);

/* ---------------- 族0052 图标动效（F01276~F01300 · 25 个独立动效开关） ---------------- */

const motion = (kind: string, dur: string, ease: string) => ({
  "--w2-icon-motion-kind": kind,
  "--w2-icon-motion-dur": dur,
  "--w2-icon-motion-ease": ease,
});

const f0052 = family("f0052", "0052", "AI-11", "图标动效", "Icon motion", [1276, 1300], [
  entry(1276, "f0052", "switch", "hover-bounce", "悬停微弹 — 悬停 1.05 倍回弹", "Hover micro-bounce 1.05x", { vars: motion("hover-bounce", "160ms", "var(--ease-spring)") }),
  entry(1277, "f0052", "switch", "press-jelly", "点击果冻 — 按压果冻形变", "Press jelly deform", { vars: motion("press-jelly", "200ms", "var(--ease-spring)") }),
  entry(1278, "f0052", "switch", "loading-spin", "加载转环 — 加载态替换转环", "Loading spinner swap", { vars: motion("loading-spin", "900ms", "linear") }),
  entry(1279, "f0052", "switch", "drag-sway", "拖拽摇摆 — 拖起后左右摇摆", "Drag sway", { vars: motion("drag-sway", "320ms", "ease-in-out") }),
  entry(1280, "f0052", "switch", "drag-tilt", "拖拽倾斜 — 拖起 5° 倾斜", "Drag tilt 5°", { vars: motion("drag-tilt", "150ms", "var(--ease-standard)") }),
  entry(1281, "f0052", "switch", "drop-dust", "落尘 — 放置时微粒散落", "Drop dust particles", { params: { particles: 8, durMs: 360 } }),
  entry(1282, "f0052", "switch", "longpress-breath", "长按呼吸 — 长按过程呼吸", "Long-press breathing", { vars: motion("breath", "1200ms", "ease-in-out") }),
  entry(1283, "f0052", "switch", "focus-glow", "焦点发光 — 键盘焦点辉光", "Keyboard focus glow", { vars: { "--w2-icon-glow": "0.22" } }),
  entry(1284, "f0052", "switch", "open-ripple", "激活涟漪 — 打开时水波", "Open ripple", { params: { rippleMs: 420 } }),
  entry(1285, "f0052", "switch", "idle-desat", "休眠灰化 — 不常用渐灰", "Idle desaturate", { vars: { "--w2-icon-sat": "0.45" }, params: { idleDays: 14, fadeMs: 600 } }),
  entry(1286, "f0052", "switch", "wake-sat", "唤醒亮起 — 使用时恢复饱和", "Wake resaturate", { vars: { "--w2-icon-sat": "1" } }),
  entry(1287, "f0052", "switch", "fav-star-flash", "收藏星闪 — 收藏星一次闪光", "Favorite star flash", { params: { flashMs: 260 } }),
  entry(1288, "f0052", "switch", "delete-dissolve", "删除溶解 — 删除时粒子溶解", "Delete dissolve", { params: { dissolveMs: 300 } }),
  entry(1289, "f0052", "switch", "new-pop-in", "新建弹入 — 新图标弹跳入场", "New icon pop-in", { vars: motion("pop-in", "240ms", "var(--ease-spring)") }),
  entry(1290, "f0052", "switch", "rename-shake", "重命名抖动 — 重命名态轻微抖动", "Rename shake", { vars: motion("shake", "180ms", "ease-in-out") }),
  entry(1291, "f0052", "switch", "select-marquee", "选中跑马灯 — 选中描边流动", "Selection marquee border", { vars: motion("marquee", "1200ms", "linear") }),
  entry(1292, "f0052", "switch", "multiselect-gather", "多选聚拢 — 多选时图标靠拢", "Multi-select gather", { params: { gatherPx: 6 } }),
  entry(1293, "f0052", "switch", "badge-bounce", "角标弹跳 — 角标出现小弹跳", "Badge pop bounce", { vars: motion("badge-bounce", "200ms", "var(--ease-spring)") }),
  entry(1294, "f0052", "switch", "badge-roll", "徽标滚动 — 长数字滚动", "Number roll", { params: { rollMs: 260 } }),
  entry(1295, "f0052", "switch", "glitch-flash", "故障闪 — 每 30s 一次故障帧", "Glitch frame / 30s", { params: { periodS: 30, frameMs: 90 } }),
  entry(1296, "f0052", "switch", "magnet-stretch", "磁吸变形 — 靠近目标液态拉长", "Magnet liquid stretch", { params: { stretch: 1.08, snapMs: 140 } }),
  entry(1297, "f0052", "switch", "liquid-merge", "液态融合 — 两图标合并融合", "Liquid merge", { params: { mergeMs: 240 } }),
  entry(1298, "f0052", "switch", "page-flip", "手翻页 — 图标集切换翻页", "Set switch page-flip", { vars: motion("page-flip", "280ms", "var(--ease-emphasized)") }),
  entry(1299, "f0052", "switch", "undo-reverse", "倒转回放 — 撤销时动作倒放", "Undo reverse playback", { params: { reverseMs: 220 } }),
  entry(1300, "f0052", "switch", "celebrate-fireworks", "庆祝烟花 — 批量完成烟花", "Batch-complete fireworks", { params: { burstMs: 900 } }),
]);

/* ---------------- 族0053 图标栅格与密度（F01301~F01325 · 25 档栅格方案） ---------------- */
/* tileW/tileH 为格子几何，与 desktop-icons/density.ts 档位兼容（32/48/64/96 语义一致）。 */

const grid = (w: number, h: number, icon: number, gapX: number, gapY: number) => ({
  "--w2-icon-grid-w": `${w}px`,
  "--w2-icon-grid-h": `${h}px`,
  "--w2-icon-grid-px": `${icon}px`,
  "--w2-icon-grid-gx": `${gapX}px`,
  "--w2-icon-grid-gy": `${gapY}px`,
});

const g = (w: number, h: number, icon: number, gx: number, gy: number) => ({ params: { tileW: w, tileH: h, iconPx: icon, gapX: gx, gapY: gy }, vars: grid(w, h, icon, gx, gy) });

const f0053 = family("f0053", "0053", "AI-11", "图标栅格与密度", "Icon grid & density", [1301, 1325], [
  entry(1301, "f0053", "preset", "base24", "24px 基线 — 全套 24px 网格", "24px baseline grid", { ...g(84, 96, 24, 12, 12) }),
  entry(1302, "f0053", "switch", "keyline-audit", "键线审计 — 键线自动检查", "Keyline audit", { params: { tolerance: 0.5 } }),
  entry(1303, "f0053", "switch", "optical-align", "光学对齐 — 圆形/方形光学补偿", "Optical alignment", { params: { circlePx: -0.5, squarePx: 0 } }),
  entry(1304, "f0053", "switch", "pixel-snap", "像素贴合 — 1x/2x 整像素", "Integer pixel snap", { params: { snap: 1 } }),
  entry(1305, "f0053", "preset", "d4", "密度四档 — 特小/小/中/大", "Density 4 tiers", { ...g(84, 98, 32, 12, 12) }),
  entry(1306, "f0053", "preset", "superdense", "超密模式 — 48px 超密档", "Superdense 48px", { ...g(72, 84, 20, 8, 8) }),
  entry(1307, "f0053", "preset", "wall", "大图模式 — 128px 大图档", "Wall 128px", { ...g(168, 188, 96, 16, 16) }),
  entry(1308, "f0053", "switch", "auto-gap", "自适应间距 — 按数量自动间距", "Auto gap by count", { params: { base: 12, perTen: -1, min: 6 } }),
  entry(1309, "f0053", "preset", "waterfall-wall", "图标墙 — 瀑布式图标墙", "Waterfall icon wall", { ...g(120, 150, 56, 10, 10), params: { tileW: 120, tileH: 150, iconPx: 56, gapX: 10, gapY: 10, mode: "waterfall" } }),
  entry(1310, "f0053", "switch", "row-gap-indep", "行距独立 — 行距单独调节", "Independent row gap", { vars: { "--w2-icon-grid-gy": "20px" } }),
  entry(1311, "f0053", "switch", "col-gap-indep", "列距独立 — 列距单独调节", "Independent column gap", { vars: { "--w2-icon-grid-gx": "20px" } }),
  entry(1312, "f0053", "preset", "ratio", "纵横比 — 方/竖/横三选", "Aspect: square / tall / wide", { params: { ratio: "square" } }),
  entry(1313, "f0053", "preset", "frame", "图标框 — 无/圆/方/玻璃框", "Frame: none/circle/square/glass", { params: { frame: "circle" }, vars: { "--w2-icon-frame": "circle" } }),
  entry(1314, "f0053", "preset", "inset", "留白比例 — 框内留白三档", "Inner inset 3 tiers", { params: { inset: 0.12 } }),
  entry(1315, "f0053", "switch", "plate-alpha", "底板透明 — 底板透明度调节", "Plate opacity", { vars: { "--w2-icon-plate-alpha": "0.4" } }),
  entry(1316, "f0053", "switch", "plate-blur", "底板模糊 — 底板背景模糊", "Plate backdrop blur", { vars: { "--w2-icon-plate-blur": "8px" } }),
  entry(1317, "f0053", "preset", "shadow-depth", "阴影深度 — 三档投影", "Shadow depth 3 tiers", { vars: { "--w2-icon-shadow": "0 2px 8px oklch(0.15 0.02 260 / 0.25)" } }),
  entry(1318, "f0053", "switch", "reflection", "倒影开关 — 图标倒影", "Icon reflection", { params: { reflectAlpha: 0.18 } }),
  entry(1319, "f0053", "preset", "perspective", "透视倾角 — 透视微倾三档", "Perspective tilt 3 tiers", { params: { tilt: "-4deg" } }),
  entry(1320, "f0053", "switch", "handmade-tilt", "手作倾斜 — 随机 2° 手作感", "Handmade random 2° tilt", { params: { maxDeg: 2, seed: 1 } }),
  entry(1321, "f0053", "preset", "scatter", "散落模式 — 自由散落布局", "Scatter mode", { params: { mode: "scatter", jitterPx: 10 } }),
  entry(1322, "f0053", "switch", "magnet-strength", "磁吸强度 — 吸附力度调节", "Magnet strength", { params: { magnetPx: 8 } }),
  entry(1323, "f0053", "switch", "auto-repair", "自动修复 — 栅格一键归正", "Auto re-grid", { params: { } }),
  entry(1324, "f0053", "switch", "grid-visualize", "可视化 — 网格线叠加显示", "Grid overlay", { params: { line: "oklch(0.7 0.1 262 / 0.35)" } }),
  entry(1325, "f0053", "switch", "export-spec", "导出规范 — 栅格规范文档导出", "Export grid spec", { params: { format: "json" } }),
]);

/* ---------------- 族0054 图标语义色（F01326~F01350 · 25 项语义色规范） ---------------- */

const sem = (work: string, fun: string, dev: string, media: string, sys: string) => ({
  "--w2-sem-work": work,
  "--w2-sem-fun": fun,
  "--w2-sem-dev": dev,
  "--w2-sem-media": media,
  "--w2-sem-sys": sys,
});

const f0054 = family("f0054", "0054", "AI-11", "图标语义色", "Icon semantic colors", [1326, 1350], [
  entry(1326, "f0054", "switch", "extract", "主色提取 — 图标主色自动提取", "Auto icon color extraction", { params: { cluster: 3 } }),
  entry(1327, "f0054", "preset", "category", "分类色板 — 工作/娱乐/开发/媒体/系统五类色", "Category palette (5 classes)", { vars: sem("oklch(0.62 0.12 260)", "oklch(0.68 0.15 330)", "oklch(0.65 0.13 160)", "oklch(0.66 0.14 30)", "oklch(0.6 0.05 262)") }),
  entry(1328, "f0054", "switch", "state-overlay", "状态叠加 — 更新/错误/同步角标规范", "State overlay badges", { params: { update: "amber", error: "red", sync: "blue" } }),
  entry(1329, "f0054", "switch", "unread-badge", "未读徽标 — 未读计数徽标规范", "Unread count badge", { params: { cap: 99 } }),
  entry(1330, "f0054", "switch", "progress-badge", "进度徽标 — 环形进度徽标", "Ring progress badge", { params: { ringMs: 900 } }),
  entry(1331, "f0054", "preset", "danger-red", "危险红 — 删除/危险统一红", "Danger red", { vars: { "--w2-sem-danger": "oklch(0.58 0.2 25)" } }),
  entry(1332, "f0054", "preset", "success-green", "成功绿 — 完成统一绿", "Success green", { vars: { "--w2-sem-success": "oklch(0.66 0.15 150)" } }),
  entry(1333, "f0054", "preset", "warn-amber", "警告黄 — 注意统一黄", "Warning amber", { vars: { "--w2-sem-warn": "oklch(0.78 0.15 85)" } }),
  entry(1334, "f0054", "preset", "info-blue", "信息蓝 — 提示统一蓝", "Info blue", { vars: { "--w2-sem-info": "oklch(0.66 0.12 250)" } }),
  entry(1335, "f0054", "preset", "privacy-purple", "隐私紫 — 权限/隐私统一紫", "Privacy purple", { vars: { "--w2-sem-privacy": "oklch(0.6 0.16 300)" } }),
  entry(1336, "f0054", "preset", "colorful", "彩色模式 — 全彩显示", "Full color mode", { vars: { "--w2-icon-sat": "1" } }),
  entry(1337, "f0054", "preset", "monochrome", "单色模式 — 全图标单色化", "Monochrome mode", { vars: { "--w2-icon-sat": "0" } }),
  entry(1338, "f0054", "preset", "follow-theme", "随主题 — 色板跟随主题", "Follow theme palette", { params: { source: "theme" } }),
  entry(1339, "f0054", "preset", "follow-wallpaper", "随壁纸 — 色板跟随壁纸取色", "Follow wallpaper palette", { params: { source: "wallpaper" } }),
  entry(1340, "f0054", "switch", "cvd-safe", "色弱安全 — CVD 安全重映射", "CVD-safe remap", { params: { mode: "deuteranopia" } }),
  entry(1341, "f0054", "preset", "high-contrast", "高对比 — 高对比色档", "High-contrast tier", { vars: { "--w2-sem-contrast": "boost" } }),
  entry(1342, "f0054", "preset", "dark-variant", "暗色适配 — 暗底专用变体", "Dark variant", { params: { scheme: "dark" } }),
  entry(1343, "f0054", "preset", "light-variant", "亮色适配 — 亮底专用变体", "Light variant", { params: { scheme: "light" } }),
  entry(1344, "f0054", "switch", "temp-adapt", "色温适配 — 护眼模式下校正", "Color-temp adapt", { params: { warmShiftDeg: -10 } }),
  entry(1345, "f0054", "switch", "saturation", "饱和度 — 全局饱和度调节", "Global saturation", { vars: { "--w2-icon-sat": "0.8" } }),
  entry(1346, "f0054", "switch", "brightness", "亮度 — 全局亮度调节", "Global brightness", { vars: { "--w2-icon-bright": "1.1" } }),
  entry(1347, "f0054", "switch", "palette-import", "色板导入 — 外部色板导入", "Palette import", { params: { formats: ["json", "css"] } }),
  entry(1348, "f0054", "switch", "palette-share", "色板分享 — 色板分享文件", "Palette share file", { params: { format: "variable-palette-v1" } }),
  entry(1349, "f0054", "switch", "audit", "审计 — 语义色误用审计", "Semantic misuse audit", { params: { rules: ["danger-on-success", "contrast<3"] } }),
  entry(1350, "f0054", "switch", "docs", "教学 — 语义色规范图解", "Semantics spec guide", { params: { } }),
]);

/* ---------------- 族0055 图标状态机（F01351~F01375 · 25 个状态定义） ---------------- */

const st = (name: string, color: string, overlay: string) => ({
  params: { state: name, color, overlay },
  vars: { "--w2-state-color": color },
});

const f0055 = family("f0055", "0055", "AI-11", "图标状态机", "Icon state machine", [1351, 1375], [
  entry(1351, "f0055", "preset", "default", "默认 — 基础常态", "Default state", { ...st("default", "inherit", "none") }),
  entry(1352, "f0055", "preset", "hover", "悬停 — 悬停态规范", "Hover state", { ...st("hover", "inherit", "lift-1px") }),
  entry(1353, "f0055", "preset", "pressed", "按下 — 按压态规范", "Pressed state", { ...st("pressed", "inherit", "sink-1px") }),
  entry(1354, "f0055", "preset", "disabled", "禁用 — 禁用去饱和 40%", "Disabled (desat 40%)", { ...st("disabled", "inherit", "desat-40") }),
  entry(1355, "f0055", "preset", "loading", "加载 — 加载转圈态", "Loading spinner", { ...st("loading", "var(--accent)", "spinner") }),
  entry(1356, "f0055", "preset", "success", "成功 — 完成对勾态", "Success check", { ...st("success", "var(--w2-sem-success)", "check") }),
  entry(1357, "f0055", "preset", "failed", "失败 — 失败叹号态", "Failed exclaim", { ...st("failed", "var(--w2-sem-danger)", "exclaim") }),
  entry(1358, "f0055", "preset", "update-ready", "更新可用 — 可更新角标态", "Update available badge", { ...st("update-ready", "var(--w2-sem-warn)", "dot") }),
  entry(1359, "f0055", "preset", "updating", "更新中 — 更新进度态", "Updating progress", { ...st("updating", "var(--accent)", "progress") }),
  entry(1360, "f0055", "preset", "syncing", "同步中 — 云同步旋转态", "Syncing spin", { ...st("syncing", "var(--accent)", "spin") }),
  entry(1361, "f0055", "preset", "offline", "离线 — 断网灰态", "Offline gray", { ...st("offline", "inherit", "gray") }),
  entry(1362, "f0055", "preset", "frozen", "休眠 — 后台冻结态", "Frozen", { ...st("frozen", "inherit", "frost") }),
  entry(1363, "f0055", "preset", "locked", "锁定 — 锁形角标态", "Locked badge", { ...st("locked", "var(--w2-sem-privacy)", "lock") }),
  entry(1364, "f0055", "preset", "privacy", "隐私 — 摄像头/麦占用态", "Privacy in-use", { ...st("privacy", "var(--w2-sem-privacy)", "dot-pulse") }),
  entry(1365, "f0055", "preset", "audio-on", "音频开 — 声音输出指示态", "Audio-out indicator", { ...st("audio-on", "var(--accent)", "wave") }),
  entry(1366, "f0055", "preset", "notify-dot", "通知角标 — 未读角标态", "Notify dot", { ...st("notify-dot", "var(--w2-sem-danger)", "dot") }),
  entry(1367, "f0055", "preset", "notify-count", "未读数字 — 数字徽标态", "Notify count", { ...st("notify-count", "var(--w2-sem-danger)", "count") }),
  entry(1368, "f0055", "preset", "multiselect", "多选 — 勾选框态", "Checkbox state", { ...st("multiselect", "var(--accent)", "checkbox") }),
  entry(1369, "f0055", "preset", "drag-source", "拖拽源 — 拖起半透态", "Drag source", { ...st("drag-source", "inherit", "alpha-60") }),
  entry(1370, "f0055", "preset", "drop-target", "放置目标 — 目标高亮态", "Drop target", { ...st("drop-target", "var(--accent)", "ring") }),
  entry(1371, "f0055", "preset", "recent", "最近使用 — 近期高亮描边", "Recent outline", { ...st("recent", "var(--accent)", "outline") }),
  entry(1372, "f0055", "preset", "pinned", "置顶钉 — 图钉角标", "Pinned badge", { ...st("pinned", "var(--accent)", "pin") }),
  entry(1373, "f0055", "preset", "starred", "收藏星 — 星标角标", "Star badge", { ...st("starred", "var(--w2-sem-warn)", "star") }),
  entry(1374, "f0055", "preset", "corrupt", "损坏 — 文件损坏警示态", "Corrupt warning", { ...st("corrupt", "var(--w2-sem-danger)", "exclaim") }),
  entry(1375, "f0055", "preset", "shortcut", "快捷角标 — 快捷方式箭头", "Shortcut arrow", { ...st("shortcut", "inherit", "arrow") }),
]);

export const AI11_FAMILIES: DesignFamily[] = [f0051, f0052, f0053, f0054, f0055];
