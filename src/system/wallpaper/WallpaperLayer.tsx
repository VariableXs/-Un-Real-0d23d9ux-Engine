import { CosmicBackground, toAssetUrl } from "../../features/background/CosmicBackground";
import { SceneShaderWallpaper } from "./SceneShaderWallpaper";
import { LivingWallpaper } from "./LivingWallpaper";
import type { CustomBg, Settings } from "../../lib/settings";

/**
 * 桌面壁纸层（L0 显示层，docs/ARCHITECTURE_V2.md §四）。
 *
 * 6 种模式：
 * - solid   纯黑
 * - gravity 3D 引力场（复用 v1 星空引擎 deep-space 主题，参数原样）
 * - image   图片壁纸（customBg.imagePath）
 * - living  活化图片（实机反馈：Windows 动态壁纸在 Variable 里变静态 —— 图片 +
 *           粒子活化层 + Ken Burns 缓动，任何静态图都有呼吸感）
 * - video   视频壁纸（customBg.videoPath）
 * - hybrid  混合 = 图片/视频之上叠加星空引擎
 *
 * 注意：此组件只服务桌面环境。四款软件（Write/Mind/Code/Fate）内部
 * 背景不经过这里，其光影方案保持原设计不变。
 */
export function WallpaperLayer(props: { settings: Settings }): React.ReactElement {
  const s = props.settings;
  const mode = s.wallpaperMode;

  // "系统桌面（Wallpaper Engine）"模式已下线：此前该模式渲染纯黑底并让位隐藏，
  // WE 未实际接管时整屏黑屏（实机反馈）。历史设置里残留的 system 值
  // 直接落入下方媒体/星空分支按本地壁纸渲染，不再黑屏。
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

  // 实机反馈：scene 着色器壁纸 —— WebGL 本地渲染（WE 全局变量兼容），
  // 编译失败自动回退「活化图片」（粒子 + 缓动，不再是死静态），绝不黑屏
  if (mode === "shader") {
    return (
      <div className="wallpaper wallpaper-web" aria-hidden>
        <SceneShaderWallpaper
          shaderPath={s.customBg.shaderPath}
          fallbackImage={s.customBg.imagePath || undefined}
          reduceMotion={s.reduceMotion}
          safeMode={s.safeMode}
          perfMode={s.perfMode}
        />
      </div>
    );
  }

  // 实机反馈：Windows 动态壁纸在 Variable 里变静态 —— living 活化模式
  // （图片 + 粒子层 + Ken Burns；reduce-motion/static 档诚实降级静态）。
  if (mode === "living") {
    return (
      <LivingWallpaper
        imagePath={s.customBg.imagePath}
        reduceMotion={s.reduceMotion}
        safeMode={s.safeMode}
        perfMode={s.perfMode}
      />
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
  return mode === "image" || mode === "living" || mode === "video" || mode === "hybrid" || mode === "shader";
}
