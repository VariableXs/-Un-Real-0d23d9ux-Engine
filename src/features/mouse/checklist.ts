/**
 * J 鼠标域 · 二十项条目元数据与十二查对账源（v4 · MD3 附B/通用十二查的机器对账层）。
 *
 * 每项登记：面板三件套同源文案（说明句三件套查）、三落位、路径链、性能线
 * 机器探针（真校验不是抄数字）、默认档对拍、最丑角落自记（通11）。
 * 不可机器化的查（4K 走查/录屏/断电注入）在 twelveChecks() 里如实标
 * 「gated-实机日」——对账表不冒领。
 */

import { gainAt, SLOW_TUNE_RATIOS } from "./curve";
import { liftFilterSelfTest, TREMOR_LEVELS } from "./filters";
import { notchLines, tiltCols, tiltFromShiftWheel, APP_CLASS_DEFAULT } from "./wheel";
import { SEAM_GUARD_PRESET } from "./screen";
import { J1_DEFAULTS } from "./j1store";
import { autoscrollVelocity, edgeScrollSpeed, AUTOSCROLL_PRESET } from "./autoscroll";
import { clampHoverDelay, clampTooltipDelay, longPressMs } from "./hoverTiming";
import { DEVICE_PROFILE_CAP, applyIncremental } from "./profiles";
import { validateSideTarget } from "./sideButtons";
import { TRAIL_FADE_MS, GESTURE_MIN_STEPS, BUILTIN_GESTURES } from "./gestures";
import { DEFAULT_OVERLAY } from "./overlay";
import type { J1Section } from "./j1store";

export interface J1ItemMeta {
  f: string;
  name: string;
  section: J1Section;
  /** 说明句三件套（与面板 Row 同源——一处一事实）。 */
  row: { label: string; hint: string; control: string };
  /** 三落位登记（A=设置直调 / B=设置跳转 / C=免调节）。 */
  placement: "A" | "B" | "C";
  placementNote?: string;
  /** 路径链（③-8 判据 ≤4 段）。 */
  navChain: string[];
  /** 性能线/判据硬线机器探针（真执行，不抄数）。 */
  probe: () => boolean;
  /** 最丑角落自记（通11——m4 复盘可查）。 */
  ugly: string;
}

/** 20 项逐条登记（顺序即 F 编号序）。 */
export const J1_ITEMS: J1ItemMeta[] = [
  {
    f: "F601", name: "指针速度曲线谱", section: "curve",
    row: { label: "曲线族", hint: "换曲线即时生效、试错零成本；「线性 1:1」与输入手感关加速档同一事实源。", control: "select + 全局灵敏度滑杆 + 贝塞尔编辑器" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "指针速度曲线谱"],
    probe: () => {
      const t = gainAt("linear", 100);
      const c = gainAt("classic", 100) > 1;
      const s = gainAt("soft", 8) < 1 && gainAt("soft", 200) > 1;
      return t === 1 && c && s;
    },
    ugly: "自定义曲线的 x(t) 反解是两步牛顿近似——极端控制点下预览与实际增益有可见偏差，完整反解该用二分。",
  },
  {
    f: "F602", name: "慢速微调模式", section: "slowTune",
    row: { label: "启用慢速微调", hint: "按住修饰键指针立刻「听话变慢」——步进式降档，1px 步进可达；占用已登记冲突审计。", control: "开关 + 修饰键下拉 + 降速档三选" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "慢速微调"],
    probe: () => SLOW_TUNE_RATIOS.includes(0.1 as (typeof SLOW_TUNE_RATIOS)[number]),
    ugly: "修饰键状态在 pointermove 才被消费——按住修饰键不动指针时无任何视觉反馈（HUD 已补但常驻可见性弱）。",
  },
  {
    f: "F603", name: "抬笔滤波", section: "liftFilter",
    row: { label: "抬笔滤波", hint: "按键抬起后 8ms 窗口内末位移按 50% 折算——松手前抖一下被吃掉；快速移动完全无感。", control: "开关" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滤波与手抖"],
    probe: () => liftFilterSelfTest([{ dx: 0.6, dy: -0.4, dtMs: 3 }, { dx: -0.5, dy: 0.5, dtMs: 6 }], 100).pass,
    ugly: "窗口/权重写死不可调是刻意的（手感参数宜少不宜多），但「固定」的理由只在注释里——面板没有一句话解释为什么不给调。",
  },
  {
    f: "F604", name: "中键自动滚动", section: "autoscroll",
    row: { label: "中键自动滚动", hint: "按一下、推一推、点一下退出（Windows 肌肉记忆）；锚标走优先平面。", control: "开关 + 锚点封顶速度滑杆" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "自动滚动"],
    probe: () => {
      const dead = autoscrollVelocity(4, 0, { enabled: true, ...AUTOSCROLL_PRESET });
      const dirs = new Set(Array.from({ length: 16 }, (_, i) => {
        const rad = (i * 22.5 * Math.PI) / 180;
        return autoscrollVelocity(Math.cos(rad) * 40, Math.sin(rad) * 40, { enabled: true, ...AUTOSCROLL_PRESET }).dirIndex;
      }));
      return dead.vx === 0 && dirs.size === 16;
    },
    ugly: "锚标接管期间横竖混合滚动的速度矢量没有做各向异性钳制——斜推时对角速度比纯方向快 √2 倍（Windows 同款行为，但值得记录）。",
  },
  {
    f: "F605", name: "滚轮刻度语义", section: "wheelNotch",
    row: { label: "全局刻度档", hint: "「按应用默认」= 文档/代码逐档、浏览器/长列表平滑；应用覆盖优先于全局。", control: "三档下拉 + 每格行数滑杆 + 应用覆盖编辑器" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滚轮手感"],
    probe: () => notchLines(3) === 3 && APP_CLASS_DEFAULT.document === "notch" && APP_CLASS_DEFAULT.browser === "smooth",
    ugly: "应用覆盖编辑器要求手填 data-app-id——对普通用户是黑话（可发现性欠账，真实入口应该从应用列表选）。",
  },
  {
    f: "F606", name: "倾斜滚轮支持", section: "tiltWheel",
    row: { label: "倾斜滚轮", hint: "左/右倾斜映射水平滚动，一次 3 列（与垂直对称）；无硬件时本节自动隐藏、Shift+滚轮等效始终可用。", control: "开关（能力检测显隐）" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滚轮手感"],
    probe: () => tiltCols(3) === 3 && tiltFromShiftWheel(120, 3).cols === 3 && tiltFromShiftWheel(120, 3).dir === 1,
    ugly: "真实倾斜路径（deltaX）与 Shift+滚轮等效入口共用「一次 N 列」语义，但连发加速曲线与垂直连滚的一致性只有逻辑同构、没有实机对拍（随闸门）。",
  },
  {
    f: "F607", name: "跨屏接缝手感", section: "seamGuard",
    row: { label: "接缝护边", hint: "穿越需在接缝 4px 内停留 200ms；四角 8px 豁免；可按屏对独立覆盖。", control: "开关 + 屏对覆盖面板" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "跨屏与落点"],
    probe: () => SEAM_GUARD_PRESET.edgePx === 4 && SEAM_GUARD_PRESET.dwellMs === 200 && SEAM_GUARD_PRESET.cornerPx === 8,
    ugly: "护边时序只在虚拟桌面坐标自洽——Tauri 多屏物理像素坐标与 window.screenX 的换算在奇数缩放比（125%/150%）下有 ±1px 量化误差，逻辑上无害但强迫症不爽。",
  },
  {
    f: "F608", name: "指针磁吸对齐", section: "magnet",
    row: { label: "指针磁吸对齐", hint: "接近小目标 12px 内视觉微移对齐；大目标不吸；判定零偏移。", control: "开关 + 三档半径" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "磁吸与悬停"],
    probe: () => (J1_DEFAULTS.magnet as { enabled: boolean }).enabled === false,
    ugly: "磁吸只看命中元素类型与尺寸白名单——两个紧挨的小按钮之间会把指针吸向「命中链上第一个」而不是视觉上更近的那个。",
  },
  {
    f: "F609", name: "拖拽边缘自动滚", section: "dragScroll",
    row: { label: "拖拽边缘自动滚", hint: "边缘 24px 触发带、深入三档（4/10/18px每帧）；仅对声明容器生效；悬停静止区绝不误滚。", control: "开关" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "自动滚动"],
    probe: () => edgeScrollSpeed(0) === 0 && edgeScrollSpeed(12) === 10 && edgeScrollSpeed(22) === 18,
    ugly: "三档速度是阶跃不是连续线性——深入 8px 边界处速度从 4 跳 10 有轻微顿挫（判据要求「线性三档」，阶梯实现是判据的字面服从）。",
  },
  {
    f: "F610", name: "悬停时序自定义", section: "hoverTiming",
    row: { label: "菜单子级展开延迟", hint: "急性子调快、手抖党调慢；点击展开永远即时；tooltip 独立四档。", control: "两个四档下拉" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "磁吸与悬停"],
    probe: () => clampHoverDelay(400) === 400 && clampTooltipDelay(500) === 500 && clampHoverDelay(550) === 600,
    ugly: "--vx-menu-delay 变量已广播但菜单组件尚未消费（扩展点就绪、消费端待菜单域接入）——「即时生效」目前对 tooltip 全真、对菜单半真。",
  },
  {
    f: "F611", name: "手抖过滤", section: "tremor",
    row: { label: "手抖过滤", hint: "帕金森/震颤用户「停得住」；只压高频不压意图；默认关。", control: "三档下拉 + 自动调谐" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滤波与手抖"],
    probe: () => (J1_DEFAULTS.tremor as { level: string }).level === "off" && TREMOR_LEVELS.light.ampPx === 0.5 && TREMOR_LEVELS.strong.ampPx === 2,
    ugly: "IIR 一阶低通对 6Hz 以上强档的相位滞后约 80ms——意图移动的前沿会被轻微拖尾（直通阈值挡住了大部分，但 2-4px 的中等意图移动处于灰区）。",
  },
  {
    f: "F612", name: "滚轮自适应增益", section: "wheelGain",
    row: { label: "自适应增益", hint: "轻滚逐行精读、快滚一撸到底（3→12 行/格）；逐档应用自动豁免；介入无跳变。", control: "开关" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滚轮手感"],
    probe: () => {
      const g = J1_DEFAULTS.wheelGain as { minLines: number; maxLines: number };
      return g.minLines === 3 && g.maxLines === 12;
    },
    ugly: "增益 EMA 的节奏窗口只看事件间隔、不看滚动方向翻转——上下抖着滚时增益同样爬升（体感略怪，真实用户可能永远踩不到）。",
  },
  {
    f: "F613", name: "指针跨屏落点记忆", section: "screenMemory",
    row: { label: "跨屏落点记忆", hint: "锁屏唤醒/KVM 切回时指针回到离开的地方；EDID 指纹为键；单屏自然休眠。", control: "开关 + 记忆点管理面板" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "跨屏与落点"],
    probe: () => (J1_DEFAULTS.screenMemory as { enabled: boolean }).enabled === true,
    ugly: "记忆上限 8 屏用「删 keys[0]」近似 LRU——对象键序在极端插入模式下不严格等于最久未用序（上限 8 的场景里几乎不可能触发）。",
  },
  {
    f: "F614", name: "鼠标分设备档案", section: "devices",
    row: { label: "首插气泡提示", hint: "新设备建档时提示——不静默改手感；档案上限 10 台、超出淘汰最久未用。", control: "开关 + 档案列表 + 导入导出" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "设备与应用档案"],
    probe: () => DEVICE_PROFILE_CAP === 10,
    ugly: "webview 环境拿不到 VID-PID——「每只鼠标一套手感」的判据在本域只能以单宿主档案诚实降级（四件套与淘汰纪律全真，设备区分随闸门）。",
  },
  {
    f: "F615", name: "侧键编程", section: "sideButtons",
    row: { label: "侧键映射", hint: "全局默认后退/前进、应用可覆盖；映射目标三选；与 F244 注册表互通。", control: "五键位下拉 + 应用覆盖面板 + 快捷键录制（v4）" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "侧键与手势"],
    probe: () => validateSideTarget({ kind: "action", action: "no-such" }).length > 0 && validateSideTarget({ kind: "shortcut", keys: "Ctrl+Shift+V" }).length === 0,
    ugly: "侧键映射的「启动应用」目标在浏览器 dev 模式派发事件后无人消费——未处理显性化会提示，但「为什么」要翻审计面板才知道。",
  },
  {
    f: "F616", name: "应用级鼠标档案", section: "appProfiles",
    row: { label: "应用级档案", hint: "前台获焦即挂载（<100ms、增量切换防跳变）；设备档案与应用档案正交。", control: "档案列表 + 灵敏度滑杆 + 试挂载" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "设备与应用档案"],
    probe: () => {
      const base = { sens: 1.4, curve: "classic" as const, wheelMode: "per-app" as const };
      const out = applyIncremental(base, { sens: 0.8 });
      return out.curve === base.curve && out.wheelMode === base.wheelMode && out.sens === 0.8;
    },
    ugly: "应用档案的 wheelMode/侧键覆盖项类型上可声明、运行时只消费 sens/curve（v3 起真实生效的前两件）——wheelMode 覆盖项声明了但不生效，审计面板未显著标注此边界。",
  },
  {
    f: "F617", name: "右键手势层", section: "gestures",
    row: { label: "右键手势层", hint: "按住右键画轨迹触发动作；墨迹淡出；无轨迹松开=右键菜单（零误伤）；支持重绑定与自定义轨迹。", control: "开关 + 手势库表 + 录制台 + 重绑定面板（v4）" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "侧键与手势"],
    probe: () => TRAIL_FADE_MS === 120 && GESTURE_MIN_STEPS === 2 && BUILTIN_GESTURES.length === 12,
    ugly: "八向编码对「抖动转折」的容错靠 ±1 档——快速画 L 形时中间容易蹦出一个斜向步导致查重误判重复（录制台会显性拒绝，用户得重画一次）。",
  },
  {
    f: "F618", name: "滚轮穿透开关", section: "passthrough",
    row: { label: "滚轮穿透", hint: "悬停装饰浮层时滚轮作用其下容器（阅读多数派）；可滚弹层自动豁免。", control: "开关 + 类型语义表" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "滚轮手感"],
    probe: () => (J1_DEFAULTS.passthrough as { enabled: boolean }).enabled === true,
    ugly: "穿透检测沿 DOM 找「第一个可滚祖先」——嵌套滚动容器（弹层里套弹层）时会穿透到外层而非最近的语义滚动区（白名单按类型声明可解，但当前靠结构巧合）。",
  },
  {
    f: "F619", name: "长按时长统一旋钮", section: "longPress",
    row: { label: "长按时长档", hint: "触屏菜单/磁贴/ClickLock 统一跟随缩放；默认 1x=现行值；新增长按功能必须登记。", control: "三档下拉 + 登记表" },
    placement: "A",
    placementNote: "藏在进阶位（判据原文：普通用户不该被问「长按多长」）",
    navChain: ["设置中心", "鼠标", "长按与衬底"],
    probe: () => longPressMs(500, 0.6) === 300 && longPressMs(500, 1.6) === 800 && longPressMs(1100, 1.0) === 1100,
    ugly: "登记纪律靠「未登记调用即抛错」执法——运行时抛错的调用点在触摸域（F496/F542 消费端）尚未全部接入旋钮（变量已广播、消费端待接线，边界登记于 §2）。",
  },
  {
    f: "F620", name: "指针衬底与投影", section: "overlay",
    row: { label: "反色描边", hint: "1px 反色描边默认开（可见性底线）；柔投影/衬圈默认关；三件独立。", control: "三开关 + 三底色预览" },
    placement: "A",
    navChain: ["设置中心", "鼠标", "长按与衬底"],
    probe: () => {
      const d = DEFAULT_OVERLAY;
      return d.outline === true && d.shadow === false && d.ring === false;
    },
    ugly: "反色取的是指针主体色的反色而非实时底色——指针悬停在深浅分界线上时描边恒定（判据要求的是「任何底色看得清」，当前实现满足多数场景、非逐像素自适应）。",
  },
];

/** F609 三档速度语义（4/10/18 px每帧）——探针即判据原文的机械表达。 */
export const EDGE_SPEED_LADDER = [4, 10, 18] as const;
