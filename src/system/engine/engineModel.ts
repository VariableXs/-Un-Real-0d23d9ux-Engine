/**
 * 阶段 6 · 隐形 Windows 引擎通道 —— 引擎会话纯模型（任务 50/51/54）。
 *
 * 生命周期五态（施工总案 6.0）：
 *   closed → starting（冷启动 20-40s，UI 如实提示）→ ready（常驻近零占用）
 *   → hibernating（内存快照写差分盘）→ ready；任何态可 → crashed → closed。
 *
 * 拉起协议（施工总案 6.7）：点通道=engine 的软件 → 引擎未就绪则登记请求 +
 * 唤醒 + 占位卡 → 就绪自动打开并撤卡。消息带 seq，乱序/重复投递不破坏状态
 * （reducer 幂等、可重放）。
 *
 * 异常三场景（施工总案 6.10 / 任务 54）：拉起中拔盘 / VM 崩溃 / 就绪后拔盘
 * ——各自的中断处理、占位卡撤除、下次插入恢复，全部显式建模。
 */

/** 引擎生命周期状态（五态 + 崩溃终态）。 */
export type EngineLifecycle =
  | "closed"
  | "starting"
  | "ready"
  | "hibernating"
  | "crashed";

/** 冷启动命名阶段（20-40s 拆解，任务 51：每阶段有名称与预估时长，如实公示）。 */
export interface EngineBootStage {
  /** 阶段标识（协议里回传的 stage key）。 */
  key: string;
  /** 用户可读阶段名。 */
  label: string;
  /** 预估时长秒数（用于进度感；实际以事件为准，不虚构完成度）。 */
  estimatedS: number;
}

/**
 * 冷启动阶段表：预估合计 20-40s 区间（下限=热缓存 20s，上限=冷盘 40s）。
 * 进度显示只按「已到达第几阶段」推进，绝不按时间伪造百分比。
 */
export const ENGINE_BOOT_STAGES: readonly EngineBootStage[] = [
  { key: "vhdx-mount", label: "挂载引擎差分盘", estimatedS: 4 },
  { key: "vm-create", label: "准备 Hyper-V 底座", estimatedS: 5 },
  { key: "vm-power", label: "引擎系统上电", estimatedS: 8 },
  { key: "agent-heartbeat", label: "引擎代理心跳（47631）", estimatedS: 8 },
  { key: "ready-handshake", label: "就绪握手", estimatedS: 7 },
];

/** 冷启动预估区间（秒），占位卡「预期等待」如实提示用。 */
export const ENGINE_BOOT_ESTIMATE = {
  minS: ENGINE_BOOT_STAGES.reduce((a, s) => a + s.estimatedS, 0) - 12,
  maxS: ENGINE_BOOT_STAGES.reduce((a, s) => a + s.estimatedS, 0) + 4,
} as const;

/** 后端 → 前端引擎事件（engine://state 载荷；seq 保证可重放）。 */
export interface EngineStateMsg {
  /** 单调递增序号（重复/乱序投递由 reducer 幂等吸收）。 */
  seq: number;
  kind:
    | "boot-stage"      // 冷启动阶段推进
    | "ready"           // 引擎就绪
    | "hibernated"      // 已休眠（快照写盘完成）
    | "awake"           // 休眠恢复完成
    | "crashed"         // VM 崩溃（异常场景二）
    | "usb-removed"     // U 盘被拔（拉起中/就绪后皆走此事件）
    | "closed";         // 引擎正常关闭
  /** boot-stage 携带：当前阶段 key。 */
  stage?: string;
  /** crashed/usb-removed 携带：用户可读原因（如实展示，不粉饰）。 */
  reason?: string;
}

/** 一次「点开 engine 通道软件」的拉起请求。 */
export interface EngineLaunchRequest {
  /** 软件标识（SHARED apps.json 的 key）。 */
  appKey: string;
  /** 请求发起时刻（ms）。 */
  atMs: number;
  /** 用户是否已取消（取消后即使引擎就绪也不自动开窗）。 */
  cancelled: boolean;
}

/** 引擎会话表条目（前端镜像）。 */
export interface EngineSession {
  lifecycle: EngineLifecycle;
  /** 当前/最后到达的冷启动阶段 key（closed/ready 后保留供诊断）。 */
  stage: string | null;
  /** 最后一条已应用消息的 seq（幂等水位）。 */
  lastSeq: number;
  /** crash/拔盘原因（如实展示）。 */
  reason: string | null;
  /** 拉起中挂起的请求（就绪后逐个自动打开）。 */
  pending: EngineLaunchRequest[];
  /** 本会话累计打开过的窗口数（泄漏审计用）。 */
  openedWindows: number;
}

export const EMPTY_ENGINE_SESSION: EngineSession = {
  lifecycle: "closed",
  stage: null,
  lastSeq: 0,
  reason: null,
  pending: [],
  openedWindows: 0,
};

/** reducer 的输出动作（store 侧据此执行副作用）。 */
export type EngineEffect =
  /** 就绪后自动打开软件（撤占位卡）。 */
  | { type: "open-app"; appKey: string }
  /** 占位卡撤除（引擎失败/取消后无卡可留）。 */
  | { type: "drop-placeholder"; appKey: string }
  /** 请求用户可读 toast。 */
  | { type: "notify"; level: "info" | "warn" | "error"; message: string };

/**
 * 拉起协议 reducer（纯函数；幂等、乱序安全）。
 * 返回新会话 + 需要执行的副作用列表。
 */
export function reduceEngineMsg(
  s: EngineSession,
  msg: EngineStateMsg,
): { next: EngineSession; effects: EngineEffect[] } {
  // 幂等水位：重复投递（seq ≤ lastSeq）直接吸收 —— 乱序旧消息不得回卷状态。
  if (msg.seq <= s.lastSeq) return { next: s, effects: [] };
  const base: EngineSession = { ...s, lastSeq: msg.seq };
  const effects: EngineEffect[] = [];
  switch (msg.kind) {
    case "boot-stage": {
      if (s.lifecycle !== "starting" && s.lifecycle !== "closed") return { next: base, effects };
      const known = ENGINE_BOOT_STAGES.some((st) => st.key === msg.stage);
      base.lifecycle = "starting";
      base.stage = known ? msg.stage! : base.stage;
      return { next: base, effects };
    }
    case "ready": {
      base.lifecycle = "ready";
      base.stage = null;
      base.reason = null;
      // 就绪 → 挂起请求逐个自动打开并撤卡（施工总案 6.7）。
      // 开窗计数归 store（openVwmEngine 实际成功后 markEngineWindowOpened），reducer 纯模型不虚计。
      for (const req of base.pending) {
        if (req.cancelled) {
          effects.push({ type: "drop-placeholder", appKey: req.appKey });
        } else {
          effects.push({ type: "open-app", appKey: req.appKey });
        }
      }
      base.pending = [];
      return { next: base, effects };
    }
    case "hibernated":
      if (s.lifecycle === "ready") base.lifecycle = "hibernating";
      return { next: base, effects };
    case "awake":
      if (s.lifecycle === "hibernating") base.lifecycle = "ready";
      return { next: base, effects };
    case "crashed": {
      // VM 崩溃（异常场景二）：占位卡撤除 + 状态迁移 + 如实原因。
      base.lifecycle = "crashed";
      base.reason = msg.reason ?? "引擎虚拟机异常退出";
      for (const req of base.pending) {
        effects.push({ type: "drop-placeholder", appKey: req.appKey });
        effects.push({ type: "notify", level: "warn", message: `引擎已崩溃，未能打开 ${req.appKey}：${base.reason}` });
      }
      base.pending = [];
      return { next: base, effects };
    }
    case "usb-removed": {
      // 异常场景一（拉起中拔盘）与场景三（就绪后拔盘）共用事件；
      // 场景差异只影响文案与恢复标记，占位卡都必须撤除。
      base.lifecycle = "closed";
      base.reason = msg.reason ?? "U 盘已移除";
      const pulling = base.pending.length > 0 && s.lifecycle === "starting";
      for (const req of base.pending) {
        effects.push({ type: "drop-placeholder", appKey: req.appKey });
        effects.push({
          type: "notify",
          level: "warn",
          message: pulling
            ? `拉起中 U 盘被移除，已取消 ${req.appKey} 的启动；下次插入后重试即可`
            : `U 盘被移除，引擎会话已结束；下次插入自动恢复`,
        });
      }
      base.pending = [];
      return { next: base, effects };
    }
    case "closed":
      base.lifecycle = "closed";
      base.stage = null;
      for (const req of base.pending) {
        effects.push({ type: "drop-placeholder", appKey: req.appKey });
      }
      base.pending = [];
      return { next: base, effects };
  }
}

/** 用户点击 engine 通道软件（未就绪时登记请求并唤醒；返回本请求）。 */
export function requestEngineLaunch(
  s: EngineSession,
  appKey: string,
  atMs: number,
): { next: EngineSession; needWake: boolean } {
  const req: EngineLaunchRequest = { appKey, atMs, cancelled: false };
  const needWake = s.lifecycle === "closed" || s.lifecycle === "starting";
  return {
    next: { ...s, pending: [...s.pending, req] },
    needWake,
  };
}

/** 用户取消等待（就绪前取消路径，施工总案 6.7 验收项）。 */
export function cancelEngineLaunch(s: EngineSession, appKey: string): EngineSession {
  return {
    ...s,
    pending: s.pending.map((r) => (r.appKey === appKey ? { ...r, cancelled: true } : r)),
  };
}

/** 请求已开窗落位（store 侧实际 openVwmEngine 成功后调用，供泄漏审计）。 */
export function markEngineWindowOpened(s: EngineSession): EngineSession {
  return { ...s, openedWindows: s.openedWindows + 1 };
}

// ---------- 任务 53：延迟测量与画质三档（办公/均衡/游戏） ----------

/** 画质档位（任务表口径：办公/均衡/游戏；总案口径 流畅/均衡/高清 同义映射）。 */
export type StreamQualityTier = "office" | "balanced" | "gaming";

/** 档位参数（码率参数化；档位切换即时生效不重连——参数全部运行时可改）。 */
export interface StreamQualityParams {
  /** 目标帧率上限。 */
  fps: number;
  /** 目标码率 Kbps。 */
  bitrateKbps: number;
  /** 编码预设（协议枚举，可替换流实现）。 */
  codecPreset: "speed" | "balanced" | "quality";
  /** 采集步进 ms（0=全帧率）。 */
  captureIntervalMs: number;
}

/** 三档预设（开放性：档位参数用户可自定义覆盖——见 settings.resProfileCustom）。 */
export const STREAM_QUALITY_TIERS: Record<StreamQualityTier, StreamQualityParams> = {
  office: { fps: 24, bitrateKbps: 4000, codecPreset: "speed", captureIntervalMs: 0 },
  balanced: { fps: 30, bitrateKbps: 8000, codecPreset: "balanced", captureIntervalMs: 0 },
  gaming: { fps: 60, bitrateKbps: 20000, codecPreset: "quality", captureIntervalMs: 0 },
};

/** 按档位取参数；custom 非空字段逐项覆盖（开放性验收：档位用户可自定义参数）。 */
export function qualityParamsFor(
  tier: StreamQualityTier,
  custom?: Partial<StreamQualityParams> | null,
): StreamQualityParams {
  const base = STREAM_QUALITY_TIERS[tier] ?? STREAM_QUALITY_TIERS.balanced;
  if (!custom) return { ...base };
  return {
    fps: clampInt(custom.fps ?? base.fps, 10, 120),
    bitrateKbps: clampInt(custom.bitrateKbps ?? base.bitrateKbps, 500, 80000),
    codecPreset: custom.codecPreset ?? base.codecPreset,
    captureIntervalMs: clampInt(custom.captureIntervalMs ?? base.captureIntervalMs, 0, 100),
  };
}

function clampInt(v: number, lo: number, hi: number): number {
  const n = Math.round(Number.isFinite(v) ? v : lo);
  return Math.min(hi, Math.max(lo, n));
}

/** 端到端延迟五段拆解（施工总案 6.9：采集/编码/传输/解码/合成）。 */
export interface LatencySegments {
  captureMs: number;
  encodeMs: number;
  transmitMs: number;
  decodeMs: number;
  composeMs: number;
}

/** 五段合计。 */
export function latencyTotal(l: LatencySegments): number {
  return l.captureMs + l.encodeMs + l.transmitMs + l.decodeMs + l.composeMs;
}

/** P95（线性插值法；样本 <1 返回 0）。 */
export function latencyP95(samples: number[]): number {
  if (samples.length === 0) return 0;
  const xs = [...samples].sort((a, b) => a - b);
  const idx = (xs.length - 1) * 0.95;
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return xs[lo]!;
  return xs[lo]! + (xs[hi]! - xs[lo]!) * (idx - lo);
}

/**
 * 同机三次测量方差校验（任务 53 验收：同机三次方差 ≤5%）。
 * 以三次均值分子；极差/均值 ≤0.05 视为稳定。
 */
export function varianceWithinFivePct(runs: number[]): boolean {
  if (runs.length < 3) return false;
  const mean = runs.reduce((a, b) => a + b, 0) / runs.length;
  if (mean <= 0) return false;
  const spread = Math.max(...runs) - Math.min(...runs);
  return spread / mean <= 0.05;
}

// ---------- 任务 50 完善性：与 embed 收编窗口的能力对照表（如实列） ----------

/** VWM 特性在引擎窗口上的支持情况（诚实文化：不支持就写不支持）。 */
export const ENGINE_VWM_SUPPORT: ReadonlyArray<{ feature: string; supported: boolean; note: string }> = [
  { feature: "拖拽移动 / 缩放", supported: true, note: "几何经 embed://native-geo 同语义回传引擎侧" },
  { feature: "边缘贴靠分屏", supported: true, note: "贴靠后流分辨率双向同步 ×20 验证" },
  { feature: "最小化到任务栏", supported: true, note: "复用 embed://native-min 同步" },
  { feature: "最大化 / 还原", supported: true, note: "复用 embed://native-max（DWM 边界 + DPR 换算）" },
  { feature: "几何持久化", supported: true, note: "复用 vwm.ts GEOM_KEY 同一存储" },
  { feature: "Z 序 / 聚焦", supported: true, note: "vwmStore 原生参与" },
  { feature: "卷帘（M-02）", supported: false, note: "流画面裁剪语义不成立——与 tp: 窗口一致关闭" },
  { feature: "标签组（W-5）", supported: false, note: "流会话与窗口一一对应，不支持合并" },
  { feature: "不透明度（Z-36）", supported: false, note: "流合成在引擎侧，前端透明度不作用于远端内容" },
  { feature: "剪贴板直通", supported: true, note: "走引擎代理（与 Wine 通道同一数据总线）" },
];
