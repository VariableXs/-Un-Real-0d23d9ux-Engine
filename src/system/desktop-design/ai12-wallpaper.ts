/**
 * AURORA-10000 · AI-12 壁纸系统组B（领域03 · F01376~F01500 · W2）
 * 族0056 壁纸引擎｜族0057 壁纸取色联动｜族0058 壁纸管理
 * 族0059 壁纸创作工坊｜族0060 锁屏一体化
 *
 * - 族0056 的引擎档选中后经 `dispatchWpApply`（wallpaper/center/centerCore）
 *   真正驱动既有壁纸引擎，零侵入复用；
 * - 族0057 取色复用 ambience/wallpaperAccent 的 OKLCH 聚类；
 * - 族0058/0059 提供纯函数管理/创作管线（可测），设计中心内做实时预览。
 */
import { entry, family, type DesignFamily } from "./types";

/* ---------------- 族0056 壁纸引擎（F01376~F01400 · 25 档引擎特性） ---------------- */
/* params.engineKind / payload 映射到既有引擎：image | video | shader | generative | web | scene */

const eng = (kind: string, payload: Record<string, unknown>) => ({ params: { engineKind: kind, ...payload } });

const f0056 = family("f0056", "0056", "AI-12", "壁纸引擎", "Wallpaper engine", [1376, 1400], [
  entry(1376, "f0056", "preset", "static", "静态图 — 基础图片壁纸", "Static image", { ...eng("image", {}) }),
  entry(1377, "f0056", "preset", "video", "动态视频 — 视频循环壁纸", "Video loop", { ...eng("video", {}) }),
  entry(1378, "f0056", "preset", "scene", "场景化 — 天气场景自动切换", "Weather-scene auto", { ...eng("scene", { by: "weather" }) }),
  entry(1379, "f0056", "preset", "generative", "程序生成 — 程序化生成壁纸", "Procedural", { ...eng("generative", { preset: "procedural" }) }),
  entry(1380, "f0056", "preset", "particles", "粒子 — 粒子系统壁纸", "Particle system", { ...eng("generative", { preset: "particles" }) }),
  entry(1381, "f0056", "preset", "shader", "着色器 — 自定义着色器壁纸", "Custom shader", { ...eng("shader", {}) }),
  entry(1382, "f0056", "preset", "web", "网页沙箱 — 网页壁纸安全沙箱", "Sandboxed web page", { ...eng("web", { localOnly: true }) }),
  entry(1383, "f0056", "preset", "slideshow", "幻灯片 — 多图定时轮播", "Slideshow", { ...eng("image", { playlist: true }) }),
  entry(1384, "f0056", "preset", "pano360", "全景 360 — 全景图拖动视角", "360° panorama", { ...eng("image", { pano: true }) }),
  entry(1385, "f0056", "preset", "parallax", "视差层 — 多层景深视差", "Multi-layer parallax", { ...eng("generative", { preset: "parallax" }) }),
  entry(1386, "f0056", "preset", "dual-screen", "双屏协同 — 双屏壁纸拼接", "Dual-screen span", { ...eng("image", { span: "dual" }) }),
  entry(1387, "f0056", "preset", "daypart", "时段自动 — 早中晚自动切换", "Daypart auto", { ...eng("scene", { by: "daypart" }) }),
  entry(1388, "f0056", "preset", "solar", "节气 — 节气壁纸联动", "Solar-term link", { ...eng("scene", { by: "solar" }) }),
  entry(1389, "f0056", "preset", "calendar", "日历 — 日历融合壁纸", "Calendar blend", { ...eng("generative", { preset: "calendar" }) }),
  entry(1390, "f0056", "preset", "clock", "时钟 — 巨型时钟壁纸", "Giant clock", { ...eng("generative", { preset: "clock" }) }),
  entry(1391, "f0056", "preset", "lyrics", "歌词 — 正在播放歌词壁纸", "Now-playing lyrics", { ...eng("generative", { preset: "lyrics" }), note: { zh: "歌词源接媒体控制通道，未播放时回退静态帧", en: "Lyrics from media channel; falls back to static" } }),
  entry(1392, "f0056", "preset", "code-rain", "代码雨 — 代码雨壁纸", "Code rain", { ...eng("generative", { preset: "code-rain" }) }),
  entry(1393, "f0056", "preset", "game-of-life", "生命游戏 — Conway 元胞壁纸", "Conway cellular", { ...eng("generative", { preset: "life" }) }),
  entry(1394, "f0056", "preset", "fractal", "分形 — 分形动画壁纸", "Fractal", { ...eng("generative", { preset: "fractal" }) }),
  entry(1395, "f0056", "preset", "fluid", "流体 — 流体模拟壁纸", "Fluid sim", { ...eng("generative", { preset: "fluid" }) }),
  entry(1396, "f0056", "preset", "starry", "星空 — 星空星轨壁纸", "Starry sky / star trails", { ...eng("generative", { preset: "starry" }) }),
  entry(1397, "f0056", "preset", "ocean", "海洋 — 海面波浪壁纸", "Ocean waves", { ...eng("generative", { preset: "ocean" }) }),
  entry(1398, "f0056", "preset", "city", "城市 — 城市天际线昼夜", "City skyline day/night", { ...eng("generative", { preset: "city" }) }),
  entry(1399, "f0056", "preset", "hand-drawn", "手绘动画 — 手绘循环动画", "Hand-drawn loop", { ...eng("video", { loopStyle: "boil" }) }),
  entry(1400, "f0056", "preset", "solid-saver", "纯色省电 — 纯色最低功耗档", "Solid color saver", { ...eng("image", { solid: true }) }),
]);

/* ---------------- 族0057 壁纸取色联动（F01401~F01425 · 25 项取色能力） ---------------- */

const f0057 = family("f0057", "0057", "AI-12", "壁纸取色联动", "Wallpaper color link", [1401, 1425], [
  entry(1401, "f0057", "switch", "extract-main", "主色提取 — k-means 主色", "k-means dominant color", { params: { k: 3 } }),
  entry(1402, "f0057", "switch", "derive-accent", "强调色派生 — 主色派生强调色", "Derive accent", { params: { clampL: [0.25, 0.82] } }),
  entry(1403, "f0057", "switch", "text-adapt", "文字自适应 — 明暗文字自动适配", "Text contrast adapt", { params: { minContrast: 4.5 } }),
  entry(1404, "f0057", "switch", "region-pick", "区域选择 — 指定区域取色", "Region picking", { params: { grid: 3 } }),
  entry(1405, "f0057", "switch", "history", "取色历史 — 取色历史列表", "Extraction history", { params: { max: 24 } }),
  entry(1406, "f0057", "switch", "lock", "锁定 — 锁定当前取色", "Lock current palette", {}),
  entry(1407, "f0057", "switch", "manual-hsl", "手动微调 — HSL 手动微调", "HSL fine-tune", { params: { maxHueDeg: 30, maxSat: 0.2 } }),
  entry(1408, "f0057", "switch", "library-unify", "画库统一 — 画库级统一色板", "Library-wide palette", {}),
  entry(1409, "f0057", "switch", "auto-reextract", "定时重取 — 换壁纸自动重取", "Re-extract on change", {}),
  entry(1410, "f0057", "switch", "export", "导出 — 色板导出文件", "Export palette", { params: { format: "json" } }),
  entry(1411, "f0057", "switch", "token-link", "令牌联动 — 写入主题 tokens", "Write to theme tokens", { params: { target: "--accent" } }),
  entry(1412, "f0057", "switch", "contrast-guard", "对比守卫 — 文字对比度守卫", "Contrast guard", { params: { minRatio: 4.5 } }),
  entry(1413, "f0057", "switch", "hue-rotate", "色相旋转 — 色相整体旋转", "Hue rotation", { params: { maxDeg: 180 } }),
  entry(1414, "f0057", "switch", "sat-clamp", "饱和约束 — 过饱和自动压制", "Saturation clamp", { params: { maxC: 0.16 } }),
  entry(1415, "f0057", "switch", "multi-scheme", "多方案 — 多套取色方案保存", "Multiple schemes", { params: { slots: 6 } }),
  entry(1416, "f0057", "switch", "mood-board", "心情板 — 情绪命名色板", "Mood boards", {}),
  entry(1417, "f0057", "switch", "brand-board", "品牌板 — 品牌色固定板", "Brand palette", {}),
  entry(1418, "f0057", "switch", "season-board", "季节板 — 四季预设板", "Season palettes", { params: { seasons: 4 } }),
  entry(1419, "f0057", "switch", "daily-random", "随机实验 — 每日随机新配色", "Daily random palette", {}),
  entry(1420, "f0057", "switch", "preview", "预览 — 应用前预览浮层", "Pre-apply preview", {}),
  entry(1421, "f0057", "switch", "value-show", "色值展示 — HEX/OKLCH 显示", "HEX / OKLCH display", {}),
  entry(1422, "f0057", "switch", "copy", "复制 — 一键复制色值", "Copy value", {}),
  entry(1423, "f0057", "switch", "undo", "撤销 — 取色操作撤销", "Extraction undo", { params: { depth: 10 } }),
  entry(1424, "f0057", "switch", "a11y-warn", "无障碍提示 — 对比不足警示", "Low-contrast warning", { params: { minRatio: 3 } }),
  entry(1425, "f0057", "switch", "guide", "教学 — 取色原理图解", "How-it-works guide", {}),
]);

/* ---------------- 族0058 壁纸管理（F01426~F01450 · 25 项管理能力） ---------------- */

const f0058 = family("f0058", "0058", "AI-12", "壁纸管理", "Wallpaper library", [1426, 1450], [
  entry(1426, "f0058", "switch", "library", "本地画库 — 本地壁纸库管理", "Local library", {}),
  entry(1427, "f0058", "switch", "favorites", "收藏夹 — 收藏分组", "Favorites group", {}),
  entry(1428, "f0058", "switch", "tags", "标签系统 — 自由打标签", "Free tagging", {}),
  entry(1429, "f0058", "switch", "smart-albums", "智能相册 — 按色/情绪自动集", "Smart albums", { params: { by: ["color", "mood"] } }),
  entry(1430, "f0058", "switch", "dedupe", "重复检测 — 感知哈希查重", "Perceptual dedupe", { params: { hashBits: 64 } }),
  entry(1431, "f0058", "switch", "quality", "低质检测 — 模糊/拉伸检测", "Blur / stretch detect", { params: { minSharp: 0.2, aspectTolerance: 0.05 } }),
  entry(1432, "f0058", "switch", "resolution-info", "分辨率信息 — 逐张分辨率标注", "Per-item resolution", {}),
  entry(1433, "f0058", "switch", "batch-rename", "批量重命名 — 批量重命名规则", "Batch rename", { params: { pattern: "{date}_{seq}" } }),
  entry(1434, "f0058", "switch", "batch-convert", "批量转换 — 批量格式转换", "Batch convert", { params: { formats: ["webp", "png"] } }),
  entry(1435, "f0058", "switch", "lossless-pack", "无损压缩 — PNG→无损 WebP", "PNG → lossless WebP", {}),
  entry(1436, "f0058", "switch", "usage-stats", "占用统计 — 画库占用统计", "Storage stats", {}),
  entry(1437, "f0058", "switch", "cleanup-hints", "清理建议 — 大图/重复清理建议", "Cleanup suggestions", {}),
  entry(1438, "f0058", "switch", "rotate-plan", "定时轮换 — 间隔轮换计划", "Interval rotation", { params: { minMinutes: 10 } }),
  entry(1439, "f0058", "switch", "weather-rotate", "天气轮换 — 按天气换壁纸", "Weather rotation", {}),
  entry(1440, "f0058", "switch", "mood-rotate", "心情轮换 — 按心情档轮换", "Mood rotation", {}),
  entry(1441, "f0058", "switch", "daily-surprise", "随机惊喜 — 每日一换盲盒", "Daily blind pick", {}),
  entry(1442, "f0058", "switch", "exclusion", "排除列表 — 不参与轮换清单", "Rotation exclusion list", {}),
  entry(1443, "f0058", "switch", "import-export", "导入导出 — 画库导入导出", "Library import/export", {}),
  entry(1444, "f0058", "reserved", "cloud-sync", "同步预留 — 云同步接口", "Cloud-sync interface", { note: { zh: "预留位：接口冻结 + 开关存在；默认关、出站需授权（红线 §6）", en: "Reserved: frozen interface + switch; off by default" } }),
  entry(1445, "f0058", "reserved", "multi-device", "多设备 — 设备间一致", "Multi-device consistency", { note: { zh: "预留位：依赖云同步接口（F01444），不单独出网", en: "Reserved: depends on F01444; no network" } }),
  entry(1446, "f0058", "switch", "history", "历史 — 壁纸使用历史", "Usage history", {}),
  entry(1447, "f0058", "switch", "per-item-stats", "使用统计 — 每张使用时长", "Per-wall time", {}),
  entry(1448, "f0058", "switch", "sources", "渠道管理 — 下载来源登记", "Source registry", {}),
  entry(1449, "f0058", "switch", "license-note", "版权标注 — 作者与许可标注", "Author & license note", {}),
  entry(1450, "f0058", "switch", "integrity", "完整性 — 画库自检修复", "Library self-check", {}),
]);

/* ---------------- 族0059 壁纸创作工坊（F01451~F01475 · 25 项创作能力） ---------------- */

const f0059 = family("f0059", "0059", "AI-12", "壁纸创作工坊", "Wallpaper workshop", [1451, 1475], [
  entry(1451, "f0059", "switch", "crop-rules", "裁剪构图 — 裁剪+三分/黄金参考线", "Crop guides (thirds/golden)", {}),
  entry(1452, "f0059", "switch", "filter-stack", "滤镜栈 — 可叠加滤镜链", "Stackable filter chain", { params: { maxLayers: 8 } }),
  entry(1453, "f0059", "switch", "typography", "文字排版 — 标题文字排版工具", "Title typography", {}),
  entry(1454, "f0059", "switch", "signature", "签名水印 — 签名/水印添加", "Signature watermark", {}),
  entry(1455, "f0059", "switch", "collage", "拼贴模板 — 多图拼贴模板", "Collage templates", { params: { templates: 6 } }),
  entry(1456, "f0059", "switch", "gradient", "渐变器 — 多点渐变生成", "Multi-stop gradient", { params: { maxStops: 6 } }),
  entry(1457, "f0059", "switch", "noise", "噪点纹理 — 噪点/纸纹生成", "Noise / paper texture", { params: { grain: 0.03 } }),
  entry(1458, "f0059", "switch", "geometry", "几何图案 — 几何图案生成器", "Geometry generator", {}),
  entry(1459, "f0059", "reserved", "local-gen", "本地生成 — 本地模型生图接口", "Local model image API", { note: { zh: "预留位：接口冻结；模型默认不内置，不自动下载", en: "Reserved: frozen API; no bundled model" } }),
  entry(1460, "f0059", "reserved", "style-transfer", "风格迁移 — 风格迁移预留", "Style transfer reserved", { note: { zh: "预留位：依赖 F01459 本地模型接口", en: "Reserved: depends on F01459" } }),
  entry(1461, "f0059", "switch", "layer-blend", "图层混合 — 图层混合模式", "Layer blend modes", { params: { modes: ["normal", "multiply", "screen", "overlay"] } }),
  entry(1462, "f0059", "switch", "mask", "蒙版 — 蒙版笔刷编辑", "Mask brush", {}),
  entry(1463, "f0059", "switch", "grading", "色彩分级 — 曲线/分级工具", "Curves / grading", {}),
  entry(1464, "f0059", "switch", "vignette", "晕影 — 暗角晕影", "Vignette", { params: { strength: 0.35 } }),
  entry(1465, "f0059", "switch", "bokeh", "光斑 — bokeh 光斑叠加", "Bokeh overlay", { params: { count: 12 } }),
  entry(1466, "f0059", "switch", "rain-snow", "雨雪叠加 — 动态雨雪层", "Rain/snow layer", {}),
  entry(1467, "f0059", "switch", "particles", "粒子叠加 — 粒子层叠加", "Particle layer", {}),
  entry(1468, "f0059", "switch", "timeslice", "时间切片 — 日转夜时间切片", "Day-to-night timeslice", { params: { slices: 4 } }),
  entry(1469, "f0059", "switch", "pano-stitch", "宽幅拼接 — 全景拼接", "Panorama stitch", {}),
  entry(1470, "f0059", "switch", "hdr-merge", "HDR 合成 — 多曝光合成", "Multi-exposure merge", { params: { exposures: 3 } }),
  entry(1471, "f0059", "switch", "upscale", "超分 — 本地超分锐化", "Local upscale sharpen", { params: { max2x: true } }),
  entry(1472, "f0059", "switch", "inpaint", "内容感知 — 修复/去除杂物", "Content-aware heal", {}),
  entry(1473, "f0059", "switch", "batch-template", "批套模板 — 批量套用模板", "Batch template apply", {}),
  entry(1474, "f0059", "switch", "tutorial", "教程 — 创作分步教程", "Step tutorial", {}),
  entry(1475, "f0059", "switch", "submit", "投稿 — 社区投稿通道", "Community submission", {}),
]);

/* ---------------- 族0060 锁屏一体化（F01476~F01500 · 25 项锁屏能力） ---------------- */

const f0060 = family("f0060", "0060", "AI-12", "锁屏一体化", "Lockscreen integration", [1476, 1500], [
  entry(1476, "f0060", "switch", "wallpaper-link", "桌面联动 — 锁屏壁纸与桌面配套", "Follow desktop wallpaper", {}),
  entry(1477, "f0060", "preset", "clock-layout", "时钟版式 — 六款时钟版式", "Six clock layouts", { params: { layouts: 6, pick: 1 } }),
  entry(1478, "f0060", "switch", "notify-aggregate", "通知聚合 — 锁屏通知列表", "Notification list", {}),
  entry(1479, "f0060", "switch", "media", "媒体控制 — 锁屏音乐控制", "Media controls", {}),
  entry(1480, "f0060", "switch", "quick-toggles", "快捷开关 — 手电/勿扰等快捷", "Quick toggles", {}),
  entry(1481, "f0060", "switch", "weather-card", "天气 — 锁屏天气卡", "Weather card", {}),
  entry(1482, "f0060", "switch", "agenda-card", "日程 — 今日日程卡", "Agenda card", {}),
  entry(1483, "f0060", "switch", "memo", "便签 — 锁屏快速便签", "Quick memo", {}),
  entry(1484, "f0060", "switch", "countdown", "倒计时 — 事件倒计时", "Event countdown", {}),
  entry(1485, "f0060", "switch", "photo-story", "照片故事 — 每日照片+故事", "Daily photo story", {}),
  entry(1486, "f0060", "switch", "carousel", "轮播 — 精选轮播", "Curated carousel", {}),
  entry(1487, "f0060", "switch", "password-fallback", "密码入口 — 密码备用解锁", "Password fallback", {}),
  entry(1488, "f0060", "reserved", "biometric", "生物识别 — 指纹/人脸预留", "Biometrics reserved", { note: { zh: "预留位：硬件不在位时隐藏入口（守卫 §15.1）", en: "Reserved: hidden without hardware" } }),
  entry(1489, "f0060", "switch", "hide-content", "防窥 — 锁屏内容默认隐藏", "Hide content by default", {}),
  entry(1490, "f0060", "switch", "perf-tier", "性能档 — 锁屏动效降档", "Perf tier", { params: { tier: "low" } }),
  entry(1491, "f0060", "switch", "low-light", "低光适配 — 夜间自动降低亮度", "Night dim", {}),
  entry(1492, "f0060", "switch", "hdr", "HDR — HDR 锁屏渲染", "HDR rendering", {}),
  entry(1493, "f0060", "preset", "font", "字体 — 锁屏字体可选", "Clock font options", { params: { fonts: 5 } }),
  entry(1494, "f0060", "switch", "wallpaper-tint", "随壁纸色 — 锁屏配色随壁纸", "Wallpaper tint", {}),
  entry(1495, "f0060", "preset", "motion-dur", "动效时长 — 锁屏动画时长档", "Motion duration tiers", { vars: { "--w2-lock-dur": "180ms" } }),
  entry(1496, "f0060", "switch", "screen-reader", "朗读 — 读屏锁屏支持", "Screen-reader support", {}),
  entry(1497, "f0060", "switch", "per-screen", "多屏独立 — 多屏锁屏独立", "Per-screen lockscreen", {}),
  entry(1498, "f0060", "switch", "watermark", "水印 — 设备标识水印", "Device watermark", {}),
  entry(1499, "f0060", "switch", "recovery-flow", "恢复流程 — 忘记密码恢复", "Forgot-password recovery", {}),
  entry(1500, "f0060", "switch", "guide", "教学 — 锁屏功能导览", "Lockscreen guide", {}),
]);

export const AI12_FAMILIES: DesignFamily[] = [f0056, f0057, f0058, f0059, f0060];
