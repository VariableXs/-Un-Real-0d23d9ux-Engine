/**
 * 壁纸中心本地词典（车道 W 同款口径：overlay 在 body portal 之外，
 * 不依赖 i18n Provider，navigator.language 本地判定，零依赖）。
 */

export const CENTER_LABELS = {
  zh: {
    title: "壁纸中心",
    tabInstalled: "已安装",
    tabCreate: "创建",
    scanWe: "扫描 Wallpaper Engine",
    addDir: "添加本地目录",
    addImage: "添加图片",
    empty: "没有可用壁纸——扫描 Wallpaper Engine、添加本地目录或图片",
    noPreview: "选择左侧壁纸查看预览",
    applied: "已应用到桌面",
    weScanned: "扫描完成",
    dirAdded: "目录已添加",
    imgAdded: "图片已添加",
    kind_image: "图片",
    kind_video: "视频",
    kind_shader: "着色器",
    kind_web: "网页",
    kind_current: "当前",
    propIntensity: "活化粒子密度",
    propDrift: "漂移幅度",
    propStyle: "粒子风格",
    styleDust: "尘埃",
    styleBokeh: "光斑",
    styleMixed: "混合",
    apply: "应用壁纸",
    videoNote: "视频壁纸：应用后在 设置→外观 可控制播放与通用调节（模糊/亮度等）。",
    playlist: "播放列表",
    playlistAdded: "已加入播放列表",
    playlistDup: "已在播放列表中",
    playlistRemove: "移除",
    playlistClear: "清空",
    interval: "间隔（分）",
    shuffle: "随机",
    nextNow: "立即换一张",
    createHint: "视频 / 着色器 / 生成式 / 网页壁纸在壁纸工坊中创建；静态图片选中后可直接在属性面板调节动态活化效果。",
    openStudio: "打开壁纸工坊",
  },
  en: {
    title: "Wallpaper Center",
    tabInstalled: "Installed",
    tabCreate: "Create",
    scanWe: "Scan Wallpaper Engine",
    addDir: "Add local folder",
    addImage: "Add images",
    empty: "No wallpapers — scan Wallpaper Engine, add a local folder or images",
    noPreview: "Select a wallpaper to preview",
    applied: "Applied to desktop",
    weScanned: "Scan complete",
    dirAdded: "Folder added",
    imgAdded: "Images added",
    kind_image: "Image",
    kind_video: "Video",
    kind_shader: "Shader",
    kind_web: "Web",
    kind_current: "Current",
    propIntensity: "Living particle density",
    propDrift: "Drift amplitude",
    propStyle: "Particle style",
    styleDust: "Dust",
    styleBokeh: "Bokeh",
    styleMixed: "Mixed",
    apply: "Apply wallpaper",
    videoNote: "Video wallpaper: playback and generic adjustments (blur/brightness) live in Settings → Appearance after applying.",
    playlist: "Playlist",
    playlistAdded: "Added to playlist",
    playlistDup: "Already in playlist",
    playlistRemove: "Remove",
    playlistClear: "Clear",
    interval: "Interval (min)",
    shuffle: "Shuffle",
    nextNow: "Next now",
    createHint: "Create video / shader / generative / web wallpapers in Wallpaper Studio; static images get living effects via the properties panel.",
    openStudio: "Open Wallpaper Studio",
  },
} as const;

export type CenterLabelKey = keyof (typeof CENTER_LABELS)["zh"];

/** 简单语言选择：navigator.language 以 zh 开头用中文，否则英文（本地判定，零依赖）。 */
export function cT(lang?: string): (k: CenterLabelKey) => string {
  const l = lang ?? (typeof navigator !== "undefined" ? navigator.language : "zh");
  const dict = CENTER_LABELS[l && l.toLowerCase().startsWith("zh") ? "zh" : "en"];
  return (k) => dict[k] ?? k;
}
