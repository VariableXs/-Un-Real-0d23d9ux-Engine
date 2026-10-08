/**
 * AURORA-10000 · AI-10 窗口细节组 + 族0028 窗口动效语言 的视觉参数层。
 * 覆盖族：
 * - 族0028 窗口动效语言（F00676~F00700）→ 动效时长/曲线令牌
 * - 族0046 标题栏再造（F01151~F01175）→ 标题栏规格
 * - 族0047 窗口边缘系统（F01176~F01200）→ 边缘参数
 * - 族0048 窗口阴影与光（F01201~F01225）→ 阴影/光源参数
 * - 族0049 窗口玻璃材质（F01226~F01250）→ 材质参数
 * - 族0050 窗口微观手感（F00876~F00900）→ 微交互参数
 * 全部动效与颜色走 design tokens；本层只产出 CSS 变量映射，不做硬编码 UI。
 * 每档均为独立可交付单元（独立参数档）。
 */

/** 动效规格：进入/退出时长（ms）+ 缓动曲线名（须存在于 tokens.css 缓动族）。 */
export interface MotionSpec {
  enter: number;
  exit: number;
  curve: string;
}

/** 族0028：25 种动效档（ID 升序与全景图一致）。 */
export const MOTION_PRESETS: Readonly<Record<string, MotionSpec>> = Object.freeze({
  F00676: { enter: 320, exit: 220, curve: "spring-soft" }, // 开窗弹性
  F00677: { enter: 200, exit: 260, curve: "ease-in-out" }, // 关窗收敛
  F00678: { enter: 260, exit: 240, curve: "ease-out" }, // 最小化入坞
  F00679: { enter: 260, exit: 200, curve: "ease-out" }, // 恢复出坞
  F00680: { enter: 0, exit: 0, curve: "linear" }, // 移动惯性（跟手，无补间）
  F00681: { enter: 120, exit: 120, curve: "ease-out" }, // 缩放阻尼
  F00682: { enter: 280, exit: 180, curve: "spring" }, // 吸附回弹
  F00683: { enter: 240, exit: 200, curve: "ease-out" }, // 贴边滑入
  F00684: { enter: 300, exit: 240, curve: "ease-out" }, // 层叠错峰（30ms 交错由调用方）
  F00685: { enter: 180, exit: 180, curve: "ease" }, // 透明渐变
  F00686: { enter: 220, exit: 220, curve: "ease" }, // 景深模糊
  F00687: { enter: 2400, exit: 2400, curve: "ease-in-out" }, // 阴影呼吸（慢周期）
  F00688: { enter: 200, exit: 200, curve: "ease" }, // 圆角过渡
  F00689: { enter: 700, exit: 400, curve: "linear" }, // 边框流光
  F00690: { enter: 0, exit: 0, curve: "linear" }, // 拖拽残影（跟手）
  F00691: { enter: 360, exit: 240, curve: "ease-out" }, // 放下涟漪
  F00692: { enter: 160, exit: 160, curve: "ease-in-out" }, // 抖动拒绝
  F00693: { enter: 90, exit: 90, curve: "linear" }, // 吸附闪光
  F00694: { enter: 320, exit: 220, curve: "ease-out" }, // 最小化涟漪
  F00695: { enter: 320, exit: 220, curve: "ease-out" }, // 恢复涟漪
  F00696: { enter: 420, exit: 320, curve: "ease-in-out" }, // 切层翻转
  F00697: { enter: 260, exit: 180, curve: "spring" }, // 置顶脉冲
  F00698: { enter: 200, exit: 260, curve: "ease" }, // 后置淡出
  F00699: { enter: 360, exit: 300, curve: "ease-in-out" }, // 多屏飞越
  F00700: { enter: 0, exit: 0, curve: "linear" }, // 降级静态（低端机全静态保帧率）
});

/** 族0046 标题栏规格。 */
export interface TitlebarSpec {
  /** 栏高（px）。 */
  height: number;
  /** 按钮顺序（c=关闭 m=最大化 n=最小化 p=置顶 …=溢出）。 */
  order: string;
  /** 静止隐藏按钮（悬停显钮）。 */
  hoverReveal: boolean;
  /** 内嵌进度条。 */
  progress: boolean;
  /** 双击行为（max=最大化 min=最小化 snap=半屏 none=无）。 */
  dblclick: "max" | "min" | "snap" | "none";
  /** 触屏加大按钮。 */
  touchLarge: boolean;
}

/** 族0046：25 种标题栏档。 */
export const TITLEBAR_PRESETS: Readonly<Record<string, TitlebarSpec>> = Object.freeze({
  F01151: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 按钮排序（自定义序）
  F01152: { height: 32, order: "c", hoverReveal: true, progress: false, dblclick: "max", touchLarge: false }, // 悬浮展开
  F01153: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 智能标题（文件名+面包屑）
  F01154: { height: 38, order: "n m c", hoverReveal: false, progress: true, dblclick: "max", touchLarge: false }, // 进度条
  F01155: { height: 38, order: "n m c", hoverReveal: false, progress: true, dblclick: "max", touchLarge: false }, // 音频波形
  F01156: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 通知点
  F01157: { height: 36, order: "t n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 工具位
  F01158: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "snap", touchLarge: false }, // 双击行为=半屏
  F01159: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 右键菜单
  F01160: { height: 44, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: true }, // 拖区加宽
  F01161: { height: 36, order: "b n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 历史后退
  F01162: { height: 40, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 搜索
  F01163: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 活动指示
  F01164: { height: 28, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 紧凑模式
  F01165: { height: 0, order: "c", hoverReveal: true, progress: false, dblclick: "max", touchLarge: false }, // 无栏模式
  F01166: { height: 32, order: "n m c", hoverReveal: true, progress: false, dblclick: "max", touchLarge: false }, // 悬停显钮
  F01167: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 随色
  F01168: { height: 38, order: "n m c", hoverReveal: false, progress: true, dblclick: "max", touchLarge: false }, // 番茄钟
  F01169: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 时钟
  F01170: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 录屏状态
  F01171: { height: 36, order: "p n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 置顶图钉
  F01172: { height: 40, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 透明滑杆
  F01173: { height: 36, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: false }, // 省略策略
  F01174: { height: 40, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: true }, // 无障碍命名
  F01175: { height: 48, order: "n m c", hoverReveal: false, progress: false, dblclick: "max", touchLarge: true }, // 触屏加大
});

/** 族0047 边缘参数。 */
export interface EdgeSpec {
  /** resize 热区外扩（px）。 */
  hitExpand: number;
  /** 呼吸灯/流光强度 0~1（0=关）。 */
  glow: number;
  /** 圆角风格（squircle=连续圆角）。 */
  corner: "normal" | "squircle";
  /** 碰撞音/触觉反馈。 */
  haptic: boolean;
  /** 最小宽高保护。 */
  minGuard: boolean;
}

/** 族0047：25 种边缘档。 */
export const EDGE_PRESETS: Readonly<Record<string, EdgeSpec>> = Object.freeze({
  F01176: { hitExpand: 4, glow: 0.6, corner: "normal", haptic: false, minGuard: true }, // 呼吸灯
  F01177: { hitExpand: 10, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 热区放大
  F01178: { hitExpand: 6, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 八向柄
  F01179: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 键盘调尺
  F01180: { hitExpand: 4, glow: 0.8, corner: "normal", haptic: false, minGuard: true }, // 吸附高亮
  F01181: { hitExpand: 4, glow: 0, corner: "normal", haptic: true, minGuard: true }, // 碰撞音
  F01182: { hitExpand: 4, glow: 0, corner: "normal", haptic: true, minGuard: true }, // 阻力
  F01183: { hitExpand: 4, glow: 0.9, corner: "normal", haptic: false, minGuard: true }, // 方向光
  F01184: { hitExpand: 4, glow: 0, corner: "squircle", haptic: false, minGuard: true }, // 连续圆角
  F01185: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 光源阴影
  F01186: { hitExpand: 4, glow: 0.7, corner: "normal", haptic: false, minGuard: true }, // 故障描边
  F01187: { hitExpand: 4, glow: 0.5, corner: "normal", haptic: false, minGuard: true }, // 流沙
  F01188: { hitExpand: 4, glow: 0.4, corner: "normal", haptic: false, minGuard: true }, // 半透提示
  F01189: { hitExpand: 8, glow: 0.3, corner: "normal", haptic: false, minGuard: true }, // 快捷扇
  F01190: { hitExpand: 4, glow: 0.2, corner: "normal", haptic: false, minGuard: true }, // 磁轨显示
  F01191: { hitExpand: 6, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 中缝柄
  F01192: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 拼接焊缝
  F01193: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 最小保护
  F01194: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 性能优化
  F01195: { hitExpand: 12, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 放大镜
  F01196: { hitExpand: 4, glow: 0, corner: "normal", haptic: true, minGuard: true }, // 触觉强度
  F01197: { hitExpand: 4, glow: 0.5, corner: "normal", haptic: false, minGuard: true }, // 颜色语义
  F01198: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 动画降级
  F01199: { hitExpand: 4, glow: 0, corner: "normal", haptic: false, minGuard: true }, // 悬停提示
  F01200: { hitExpand: 8, glow: 0.8, corner: "normal", haptic: false, minGuard: true }, // 教学高亮
});

/** 族0048 阴影与光参数。 */
export interface ShadowSpec {
  /** 光源方位角（度，0=正上）。 */
  angle: number;
  /** 阴影距离（px）。 */
  dist: number;
  /** 模糊半径（px）。 */
  blur: number;
  /** 不透明度 0~1。 */
  opacity: number;
  /** 硬/软档。 */
  softness: "hard" | "soft";
  /** 接触阴影（贴地）。 */
  contact: boolean;
}

const shadow = (angle: number, dist: number, blur: number, opacity: number, softness: ShadowSpec["softness"], contact = false): ShadowSpec => ({
  angle,
  dist,
  blur,
  opacity,
  softness,
  contact,
});

/** 族0048：25 种光影档。 */
export const SHADOW_PRESETS: Readonly<Record<string, ShadowSpec>> = Object.freeze({
  F01201: shadow(0, 8, 24, 0.35, "soft"), // 单光源
  F01202: shadow(45, 10, 28, 0.35, "soft"), // 方向设置（NE）
  F01203: shadow(0, 8, 20, 0.4, "hard"), // 软硬档（硬）
  F01204: shadow(0, 2, 6, 0.3, "hard", true), // 接触阴影
  F01205: shadow(0, 6, 18, 0.3, "soft", true), // 高度阶梯
  F01206: shadow(0, 14, 36, 0.45, "soft"), // 升起加深
  F01207: shadow(0, 4, 12, 0.5, "hard", true), // AO 近似
  F01208: shadow(30, 18, 40, 0.3, "soft"), // 投影壁纸
  F01209: shadow(0, 12, 32, 0.25, "soft"), // 水面反射
  F01210: shadow(0, 10, 30, 0.4, "soft"), // 光晕呼吸
  F01211: shadow(0, 0, 8, 0.6, "hard"), // 边缘光
  F01212: shadow(10, 6, 14, 0.35, "soft"), // 烛光摇曳
  F01213: shadow(0, 3, 6, 0.55, "hard"), // 正午硬影
  F01214: shadow(180, 8, 24, 0.28, "soft"), // 月光冷影
  F01215: shadow(0, 16, 44, 0.5, "soft"), // 舞台追光
  F01216: shadow(0, 0, 0, 0, "soft"), // 性能档（关阴影）
  F01217: shadow(0, 8, 24, 0.35, "soft"), // DPI 无关
  F01218: shadow(0, 12, 36, 0.55, "soft"), // HDR 层次
  F01219: shadow(35, 12, 30, 0.4, "soft"), // 艺术包
  F01220: shadow(0, 8, 24, 0.3, "soft"), // 暗部保护
  F01221: shadow(0, 8, 24, 0.35, "soft"), // 图解教学
  F01222: shadow(0, 8, 24, 0.35, "soft"), // 主题联动
  F01223: shadow(0, 8, 24, 0.35, "soft"), // 调试视图
  F01224: shadow(0, 6, 16, 0.3, "soft"), // 性能预算
  F01225: shadow(0, 4, 10, 0.3, "hard", true), // 栅格回退
});

/** 族0049 材质参数。 */
export interface MaterialSpec {
  /** 背景模糊（px）。 */
  blur: number;
  /** 饱和度补偿（1=原样）。 */
  saturate: number;
  /** 底色透明度 0~1。 */
  tintAlpha: number;
  /** 纹理名（tokens 材质库键；none=纯色）。 */
  texture: string;
  /** 边框不透明度 0~1。 */
  borderAlpha: number;
}

const mat = (blur: number, saturate: number, tintAlpha: number, texture: string, borderAlpha = 0.12): MaterialSpec => ({
  blur,
  saturate,
  tintAlpha,
  texture,
  borderAlpha,
});

/** 族0049：25 种材质档。 */
export const MATERIAL_PRESETS: Readonly<Record<string, MaterialSpec>> = Object.freeze({
  F01226: mat(24, 1.2, 0.55, "none"), // 亚克力
  F01227: mat(10, 1, 0.4, "none"), // 薄雾
  F01228: mat(6, 1, 0.5, "noise"), // 磨砂
  F01229: mat(20, 1.3, 0.45, "liquid"), // 液态
  F01230: mat(8, 1, 0.5, "ice"), // 冰面
  F01231: mat(14, 1.1, 0.5, "dew"), // 凝露
  F01232: mat(4, 1, 0.6, "etch"), // 蚀刻
  F01233: mat(2, 1.4, 0.7, "stained"), // 教堂彩玻
  F01234: mat(28, 0.9, 0.75, "none"), // 烟熏
  F01235: mat(0, 1.1, 0.35, "coat"), // 镀膜
  F01236: mat(0, 1.3, 0.3, "iridescent"), // 虹彩
  F01237: mat(0, 1, 0.55, "ribbed"), // 棱纹
  F01238: mat(0, 1, 0.6, "honeycomb"), // 蜂窝
  F01239: mat(0, 1, 0.6, "weave"), // 编织
  F01240: mat(0, 1, 0.65, "brushed"), // 拉丝金属
  F01241: mat(0, 1.1, 0.6, "anodized"), // 阳极氧化
  F01242: mat(0, 1, 0.9, "ceramic"), // 陶瓷白
  F01243: mat(0, 1, 0.95, "piano"), // 釉黑
  F01244: mat(0, 1, 0.8, "wood"), // 木纹
  F01245: mat(0, 1, 0.75, "carbon"), // 碳纤维
  F01246: mat(0, 1, 0.85, "paper"), // 纸质
  F01247: mat(0, 1, 0.8, "canvas"), // 帆布
  F01248: mat(0, 1, 0.8, "xuan"), // 宣纸
  F01249: mat(0, 1, 0.8, "leather"), // 皮革
  F01250: mat(0, 1, 0.92, "none"), // 哑光
});

/** 族0050 微交互参数。 */
export interface MicrofeelSpec {
  /** 按下下沉（px）。 */
  pressDepth: number;
  /** 悬停提亮（0~1）。 */
  hoverLift: number;
  /** 交互时长（ms）。 */
  dur: number;
  /** 微音效（悬停/点击/滑动/碰撞/吸附/拨动）。 */
  sounds: boolean;
  /** 拖拽质量滞后。 */
  inertia: boolean;
}

/** 族0050：25 种微手感档（注意编号区为 F00876~F00900）。 */
export const MICROFEEL_PRESETS: Readonly<Record<string, MicrofeelSpec>> = Object.freeze({
  F00876: { pressDepth: 1, hoverLift: 0, dur: 120, sounds: false, inertia: false }, // 按下沉降
  F00877: { pressDepth: 0, hoverLift: 0, dur: 220, sounds: false, inertia: false }, // 释放回弹
  F00878: { pressDepth: 0, hoverLift: 0.04, dur: 140, sounds: false, inertia: false }, // 悬停亮起
  F00879: { pressDepth: 0, hoverLift: 0, dur: 160, sounds: false, inertia: false }, // 滚动磁吸
  F00880: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: false, inertia: true }, // 拖拽滞后
  F00881: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: false, inertia: true }, // 抛掷惯性
  F00882: { pressDepth: 0, hoverLift: 0, dur: 280, sounds: false, inertia: false }, // 双击缓动
  F00883: { pressDepth: 0, hoverLift: 0, dur: 120, sounds: false, inertia: false }, // 确认半径
  F00884: { pressDepth: 0, hoverLift: 0, dur: 200, sounds: false, inertia: false }, // 长按快捷扇
  F00885: { pressDepth: 0, hoverLift: 0, dur: 180, sounds: false, inertia: false }, // 拖动让位
  F00886: { pressDepth: 0, hoverLift: 0, dur: 900, sounds: false, inertia: false }, // 放置呼吸
  F00887: { pressDepth: 0, hoverLift: 0, dur: 200, sounds: false, inertia: false }, // 越界橡皮筋
  F00888: { pressDepth: 0, hoverLift: 0, dur: 260, sounds: false, inertia: false }, // 松手回滑
  F00889: { pressDepth: 0, hoverLift: 0, dur: 160, sounds: false, inertia: false }, // 边缘预拉
  F00890: { pressDepth: 0, hoverLift: 0, dur: 200, sounds: false, inertia: false }, // 曲线库
  F00891: { pressDepth: 0, hoverLift: 0, dur: 220, sounds: false, inertia: false }, // 错误抖动
  F00892: { pressDepth: 0, hoverLift: 0, dur: 320, sounds: false, inertia: false }, // 成功描画
  F00893: { pressDepth: 0, hoverLift: 0, dur: 200, sounds: false, inertia: false }, // 禁用去饱和
  F00894: { pressDepth: 0, hoverLift: 0, dur: 100, sounds: true, inertia: false }, // 悬停微音
  F00895: { pressDepth: 0, hoverLift: 0, dur: 100, sounds: true, inertia: false }, // 点击微音
  F00896: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: true, inertia: false }, // 滑动摩擦
  F00897: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: true, inertia: false }, // 碰撞闷响
  F00898: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: true, inertia: false }, // 吸附咔哒
  F00899: { pressDepth: 0, hoverLift: 0, dur: 0, sounds: true, inertia: false }, // 拨动音
  F00900: { pressDepth: 0, hoverLift: 0, dur: 200, sounds: false, inertia: false }, // 令牌化（基准档）
});

/**
 * 统一应用入口：族号 + 条目 ID → CSS 变量映射（键名带 --aurora- 前缀）。
 * 未知组合返回空对象（调用方回退默认）。
 */
export function applyVisual(fam: number, itemId: string): Record<string, string> {
  switch (fam) {
    case 28: {
      const m = MOTION_PRESETS[itemId];
      if (!m) return {};
      return {
        "--aurora-ws-enter": `${m.enter}ms`,
        "--aurora-ws-exit": `${m.exit}ms`,
        "--aurora-ws-curve": `var(--ease-${m.curve}, ${m.curve})`,
      };
    }
    case 46: {
      const t = TITLEBAR_PRESETS[itemId];
      if (!t) return {};
      return {
        "--aurora-titlebar-h": `${t.height}px`,
        "--aurora-titlebar-order": t.order,
        "--aurora-titlebar-hover-reveal": t.hoverReveal ? "1" : "0",
        "--aurora-titlebar-progress": t.progress ? "1" : "0",
      };
    }
    case 47: {
      const e = EDGE_PRESETS[itemId];
      if (!e) return {};
      return {
        "--aurora-edge-hit": `${e.hitExpand}px`,
        "--aurora-edge-glow": String(e.glow),
        "--aurora-edge-corner": e.corner === "squircle" ? "1" : "0",
      };
    }
    case 48: {
      const s = SHADOW_PRESETS[itemId];
      if (!s) return {};
      const rad = (s.angle * Math.PI) / 180;
      const dx = Math.round(Math.sin(rad) * s.dist);
      const dy = -Math.round(Math.cos(rad) * s.dist);
      return {
        "--aurora-shadow-x": `${dx}px`,
        "--aurora-shadow-y": `${dy}px`,
        "--aurora-shadow-blur": `${s.blur}px`,
        "--aurora-shadow-alpha": String(s.opacity),
      };
    }
    case 49: {
      const m = MATERIAL_PRESETS[itemId];
      if (!m) return {};
      return {
        "--aurora-material-blur": `${m.blur}px`,
        "--aurora-material-saturate": String(m.saturate),
        "--aurora-material-tint": String(m.tintAlpha),
        "--aurora-material-texture": m.texture,
        "--aurora-material-border": String(m.borderAlpha),
      };
    }
    case 50: {
      const f = MICROFEEL_PRESETS[itemId];
      if (!f) return {};
      return {
        "--aurora-press-depth": `${f.pressDepth}px`,
        "--aurora-hover-lift": String(f.hoverLift),
        "--aurora-micro-dur": `${f.dur}ms`,
        "--aurora-micro-sound": f.sounds ? "1" : "0",
        "--aurora-micro-inertia": f.inertia ? "1" : "0",
      };
    }
    default:
      return {};
  }
}

/** 视觉族号集合（28/46/47/48/49/50）。 */
export const VISUAL_FAMS: readonly number[] = [28, 46, 47, 48, 49, 50];
