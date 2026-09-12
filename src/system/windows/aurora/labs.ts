/**
 * AURORA-10000 · 实验与可达层（AI-08/09）：
 * - 族0040 窗口可达性（F01001~F01025）→ 可达性配置档
 * - 族0042 窗口嗅探与信息（F01051~F01075）→ 信息徽章与配额
 * - 族0045 空间扩展现实（F01126~F01150）→ XR 能力注册与实验开关（预留口径：
 *   接口冻结 + 开关存在 + 默认关 + 无硬件隐藏入口，见实施总步骤图 §15）
 */

// ---------- 族0040 窗口可达性 ----------

export interface ReachConfig {
  /** 全键盘操作。 */
  fullKeyboard: boolean;
  /** 触屏目标最小尺寸（px，≥44）。 */
  minTarget: number;
  /** 悬停触发延时（ms，防误触）。 */
  dwellMs: number;
  /** 慢速键（按键需停留生效）。 */
  slowKeys: boolean;
  /** 粘滞键。 */
  stickyKeys: boolean;
  /** 焦点环常显。 */
  focusRing: boolean;
  /** 声音替代视觉。 */
  soundCue: boolean;
  /** 闪光替代声音。 */
  flashCue: boolean;
  /** CVD 安全语义色。 */
  cvdSafe: boolean;
  /** 光标放大倍率。 */
  cursorScale: number;
  /** 点击涟漪可视化。 */
  clickRipple: boolean;
}

/** 族0040：25 种可达档（ID 升序；基线档为全键盘 + 44px 目标 + 焦点环）。 */
export const REACH_PRESETS: readonly (ReachConfig & { id: string })[] = Object.freeze([
  { id: "F01001", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01002", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01003", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01004", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01005", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01006", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01007", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01008", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01009", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1.5, clickRipple: false },
  { id: "F01010", fullKeyboard: true, minTarget: 44, dwellMs: 800, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01011", fullKeyboard: true, minTarget: 44, dwellMs: 300, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01012", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: true, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01013", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: true, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01014", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: true, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01015", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: true, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01016", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: true, cursorScale: 1, clickRipple: false },
  { id: "F01017", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01018", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01019", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1.5, clickRipple: false },
  { id: "F01020", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 2, clickRipple: false },
  { id: "F01021", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01022", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: true },
  { id: "F01023", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: true, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01024", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: true, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
  { id: "F01025", fullKeyboard: true, minTarget: 44, dwellMs: 0, slowKeys: false, stickyKeys: false, focusRing: true, soundCue: false, flashCue: false, cvdSafe: false, cursorScale: 1, clickRipple: false },
]);

// ---------- 族0042 窗口嗅探与信息 ----------

export interface InspectConfig {
  /** 悬停信息卡（进程/CPU/GPU）。 */
  hoverCard: boolean;
  /** 功耗徽章。 */
  powerBadge: boolean;
  /** 网络活动角标。 */
  netDot: boolean;
  /** 磁盘活动角标。 */
  diskDot: boolean;
  /** 麦克风/摄像头占用指示。 */
  privacyDots: boolean;
  /** 未响应预警。 */
  hangWarn: boolean;
  /** 内存排行。 */
  memRank: boolean;
  /** 调试线框层。 */
  debugBoxes: boolean;
  /** 每窗资源限额（MB，0=不限）。 */
  memQuotaMb: number;
}

/** 族0042：25 种信息档（ID 升序）。 */
export const INSPECT_PRESETS: readonly (InspectConfig & { id: string })[] = Object.freeze([
  { id: "F01051", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01052", hoverCard: false, powerBadge: true, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01053", hoverCard: false, powerBadge: false, netDot: true, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01054", hoverCard: false, powerBadge: false, netDot: false, diskDot: true, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01055", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01056", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: true, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01057", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: true, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01058", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: true, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01059", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: true, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01060", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01061", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01062", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: true, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01063", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01064", hoverCard: true, powerBadge: true, netDot: true, diskDot: true, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: true, memQuotaMb: 0 },
  { id: "F01065", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01066", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: true, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01067", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01068", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 512 },
  { id: "F01069", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: true, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01070", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: true, memQuotaMb: 0 },
  { id: "F01071", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01072", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01073", hoverCard: true, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01074", hoverCard: false, powerBadge: false, netDot: false, diskDot: false, privacyDots: false, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
  { id: "F01075", hoverCard: false, powerBadge: true, netDot: false, diskDot: false, privacyDots: true, hangWarn: false, memRank: false, debugBoxes: false, memQuotaMb: 0 },
]);

// ---------- 族0045 空间扩展现实（XR · 预留） ----------

/** XR 能力项：接口冻结 + 开关存在 + 默认关（守卫 §15.6/§15.1 口径）。 */
export interface XrCapability {
  id: string;
  /** 能力键。 */
  key: string;
  /** 是否需要外部硬件（无硬件隐藏入口）。 */
  needsHardware: boolean;
  /** 默认状态（一律默认关）。 */
  defaultOn: false;
}

/** 族0045：25 项 XR 能力注册（ID 升序）。 */
export const XR_CAPABILITIES: readonly XrCapability[] = Object.freeze([
  { id: "F01126", key: "xr.place3d", needsHardware: false, defaultOn: false },
  { id: "F01127", key: "xr.parallax", needsHardware: false, defaultOn: false },
  { id: "F01128", key: "xr.gyro", needsHardware: true, defaultOn: false },
  { id: "F01129", key: "xr.hmd.mirror", needsHardware: true, defaultOn: false },
  { id: "F01130", key: "xr.depth.layers", needsHardware: false, defaultOn: false },
  { id: "F01131", key: "xr.audio.spatial", needsHardware: false, defaultOn: false },
  { id: "F01132", key: "xr.audio.window", needsHardware: false, defaultOn: false },
  { id: "F01133", key: "xr.ar.camera", needsHardware: true, defaultOn: false },
  { id: "F01134", key: "xr.virtual.screen", needsHardware: false, defaultOn: false },
  { id: "F01135", key: "xr.eye.interface", needsHardware: true, defaultOn: false },
  { id: "F01136", key: "xr.hand.interface", needsHardware: true, defaultOn: false },
  { id: "F01137", key: "xr.capture", needsHardware: false, defaultOn: false },
  { id: "F01138", key: "xr.record", needsHardware: false, defaultOn: false },
  { id: "F01139", key: "xr.thumb3d", needsHardware: false, defaultOn: false },
  { id: "F01140", key: "xr.depth.guide", needsHardware: false, defaultOn: false },
  { id: "F01141", key: "xr.vestibular.limit", needsHardware: false, defaultOn: false },
  { id: "F01142", key: "xr.perf.tier", needsHardware: false, defaultOn: false },
  { id: "F01143", key: "xr.headsub.keyboard", needsHardware: false, defaultOn: false },
  { id: "F01144", key: "xr.bookmark", needsHardware: false, defaultOn: false },
  { id: "F01145", key: "xr.telemetry.off", needsHardware: false, defaultOn: false },
  { id: "F01146", key: "xr.demo", needsHardware: false, defaultOn: false },
  { id: "F01147", key: "xr.distance.zoom", needsHardware: false, defaultOn: false },
  { id: "F01148", key: "xr.calibrate", needsHardware: true, defaultOn: false },
  { id: "F01149", key: "xr.session.restore", needsHardware: false, defaultOn: false },
  { id: "F01150", key: "xr.lab.switch", needsHardware: false, defaultOn: false },
]);

/** 硬件可用性过滤：无硬件能力从入口隐藏。 */
export function xrAvailable(caps: readonly XrCapability[], hardware: readonly string[]): XrCapability[] {
  return caps.filter((c) => !c.needsHardware || hardware.includes(c.key));
}
