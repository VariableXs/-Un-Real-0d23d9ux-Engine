import { CosmicBackground } from "../../features/background/CosmicBackground";
import type { CustomBg, Settings } from "../../lib/settings";

/**
 * 桌面壁纸层（L0 显示层，docs/ARCHITECTURE_V2.md §四）。
 *
 * 5 种模式：
 * - solid   纯黑
 * - gravity 3D 引力场（复用 v1 星空引擎 deep-space 主题，参数原样）
 * - image   图片壁纸（customBg.imagePath）
 * - video   视频壁纸（customBg.videoPath）
 * - hybrid  混合 = 图片/视频之上叠加星空引擎
 *
 * 注意：此组件只服务桌面环境。四款软件（Write/Mind/Code/Fate）内部
 * 背景不经过这里，其光影方案保持原设计不变。
 */
export function WallpaperLayer(props: { settings: Settings }): React.ReactElement {
  const s = props.settings;
  const mode = s.wallpaperMode;

  if (mode === "solid") {
    return <div className="wallpaper wallpaper-solid" aria-hidden />;
  }

  const theme = mode === "gravity" ? "deep-space" : "custom";
  const customBg = effectiveCustomBg(mode, s.customBg);
  return (
    <div className="wallpaper" aria-hidden>
      <CosmicBackground
        theme={theme}
        perfMode={s.perfMode}
        bgTier={s.bgTier}
        reduceMotion={s.reduceMotion}
        safeMode={s.safeMode}
        editing={false}
        customBg={customBg}
        starfieldOverlay={mode === "hybrid"}
      />
    </div>
  );
}

/** 把壁纸模式映射为引擎可渲染的 customBg 类型（不修改用户的 customBg 设置）。 */
function effectiveCustomBg(mode: Settings["wallpaperMode"], cb: CustomBg): CustomBg {
  if (mode === "image") return { ...cb, type: "image" };
  if (mode === "video") return { ...cb, type: "video" };
  if (mode === "hybrid") {
    // 混合：用户已配置图片/视频则叠加星空，否则退化为引力场星空。
    if (cb.type === "image" || cb.type === "video") return cb;
    return { ...cb, type: "image" };
  }
  return cb;
}

/** 壁纸模式是否依赖用户选择的媒体文件（设置页据此显示选择器）。 */
export function wallpaperUsesMedia(mode: Settings["wallpaperMode"]): boolean {
  return mode === "image" || mode === "video" || mode === "hybrid";
}
