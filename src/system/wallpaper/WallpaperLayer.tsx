import { CosmicBackground, toAssetUrl } from "../../features/background/CosmicBackground";
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

  // 批次E-15b："系统桌面"模式 —— WE 壁纸渲染在系统桌面上，Variable 让位
  // （应用该模式时由调用方自动隐藏到托盘，托盘 V 图标一键返回）。
  // 注：WebView2 透明窗口会让 video 等媒体层停止渲染，"窗口透明透出"不可行，
  // 此处仅渲染纯黑底作为让位前的兜底。
  if (mode === "system") {
    return <div className="wallpaper wallpaper-solid" aria-hidden />;
  }

  if (mode === "solid") {
    return <div className="wallpaper wallpaper-solid" aria-hidden />;
  }

  // 批次E-15：网页壁纸（Wallpaper Engine web 型项目，本地 html 内嵌渲染）
  if (mode === "web") {
    return (
      <div className="wallpaper wallpaper-web" aria-hidden>
        {s.customBg.htmlPath ? (
          <iframe src={toAssetUrl(s.customBg.htmlPath)} title="wallpaper" allow="autoplay" />
        ) : null}
      </div>
    );
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
        // 批次E-13：桌面壁纸 1:1 清晰渲染（无模糊玻璃感）；应用内背景不走这里
        plainMedia
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
