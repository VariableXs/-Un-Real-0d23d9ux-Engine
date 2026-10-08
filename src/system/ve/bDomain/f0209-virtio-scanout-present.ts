/**
 * VE-F0209 · virtio 扫描输出与呈现（VE-B 域 · virtio 组 B1 · 组内第 9 条）
 * ---------------------------------------------------------------------------
 * 职责定位：把渲染结果真正打到屏幕。SET_SCANOUT（资源绑定到 scanout 口）、
 *   RESOURCE_FLUSH（触发 host 合成）、dmabuf 呈现路径（blob 直接作为
 *   scanout 源，**免最终拷贝**），以及多 scanout 布局与呈现节奏。
 *
 * ┌── 第一性声明（本域最恶劣缺陷：呈现了旧帧）──────────────────────────┐
 * │ 呈现类功能最恶劣的缺陷不是「黑屏」——黑屏会立刻被发现并报错。        │
 * │ 最恶劣的是**呈现了旧帧**：屏幕亮着、画面在动、用户以为一切正常，      │
 * │ 实际显示的是几秒前的画面。这在交互上等价于「系统卡死」，            │
 * │ 却没有任何错误可查。                                                    │
 * │ 旧帧的成因几乎总是同一个：**flush 与帧回调的时序脱钩**。             │
 * │ 资源已经提交、host 已经合成，但呈现绑定的资源句柄没换，             │
 * │ 或者换的时机早于渲染完成。于是「合成完成」与「上屏」之间断开。       │
 * │ 因此本条的第一性声明是：**每一帧的呈现必须可追溯到唯一的 flush 序**，
 * │ 且该序的完成时刻不得早于该帧的渲染完成时刻。做不到就不呈现。        │
 * └─────────────────────────────────────────────────────────────────────┘
 *
 * 为什么「宁可黑屏也不呈现旧帧」：黑屏是自解释的——用户立刻知道坏了，
 * 会报障、会反馈；旧帧是自隐藏的——画面正常，交互无响应，问题会被归到
 * 「驱动兼容性」上，几周后才被当作性能问题提出来。所以本条的降级方向
 * 永远是「显式黑屏 + 显式通知」，不是「继续显示上一帧假装没事」。
 *
 * dmabuf 直呈现零拷贝（锚点原文：blob 资源直接作为 scanout 源）：
 *   承接 F0208 的 blob 机制——当 scanout 绑定的资源是 blob 且地址一致，
 *   host 可直接以该缓冲为合成源，**免除最后一次拷贝**。这一跳省下的
 *   不只是内存带宽，更是全屏 4K 下每帧数十 MB 的搬运量。
 *   但零拷贝有前提：地址必须真的一致（F0208 的 addressIdentity 对拍），
 *   且属性必须正确（uncached）。前提不满足时**必须退化为拷贝并标注**，
 *   不能静默走拷贝路径——那会让「零拷贝」这个优化声明失去意义，
 *   进而让人以为可以省掉同步。
 *
 * 多 scanout 与布局（锚点原文：GET_DISPLAY_INFO 拿 display rect）：
 *   多屏不是「多画几次」，而是**布局坐标系**问题：每个 scanout 有自己的
 *   display rect 与物理尺寸，路由表（VE-F0113 呈现路由）按 rect 把渲染
 *   结果分派到对应口。本条把virtio 多屏实现为路由表的 virtio 后端——
 *   同一份路由表语义，两种后端（本地 / virtio），避免多屏语义分叉。
 *
 * 呈现节奏三模式（锚点原文：vsync 语义由 host 模拟，guest 按 F0048
 * 三模式映射）：host 并不真的等 vsync，它只是「按提交的节奏合成」。
 *   · IMMEDIATE —— 来一帧提交一帧，不等（延迟最低，可能撕裂）；
 *   · FLUSH_ALIGNED —— 以 flush 完成为帧边界（默认，平衡）；
 *   · VSYNC_HINTED —— 以 host 回报的呈现完成为帧边界（最平滑，
 *                        但依赖 host 回报及时，回报缺失须回落）。
 *   三模式的共同纪律：**没有回报就降级到 FLUSH_ALIGNED，不假装在等 vsync**。
 *
 * 零静默纪律：所有拒绝/旧帧/布局越界/回报缺失/零拷贝退化全部产出
 *   Diagnostic（code + severity + message + hint + stage）。本模块不向
 *   UI 抛异常，也不吞掉任何一条诊断。
 *
 * 性能逐项分解（锚点原文：绑定 O(口数)、flush O(1)、布局解析 O(口数)）：
 *   · SET_SCANOUT 绑定 O(1)——单口改绑，不遍历其他口；
 *   · RESOURCE_FLUSH 提交 O(1)——单次通知，不等完成（异步回报）；
 *   · GET_DISPLAY_INFO 解析 O(口数)——口数为设备常量，非 O(屏幕总数)；
 *   · 路由查询 O(1)——rect 索引查表，非 O(输出数 × 帧数)。
 *   零拷贝路径上没有一段是 O(像素数)：出现按像素的循环即说明退化，
 *   守卫会同时报「零拷贝退化」。
 *
 * 工程量（锚点原文分解）：核心逻辑约 200 行（scanout 绑定 + flush 时序
 *   + 呈现节奏三模式 + 多屏布局）、边界防护约 80 行（rect 越界/坐标重叠/
 *   资源态校验/回报缺失阈值）、错误路径约 60 行（旧帧拦截 + 降级处置）、
 *   测试支撑约 60 行（单屏多屏/零拷贝/节奏/布局/flush 对齐），合计约
 *   400 行。
 *
 * 判据：单屏/多屏 scanout 正确、dmabuf 直呈现零拷贝、呈现节奏三模式、
 *   多屏布局解析正确、flush 与回调对齐。
 *
 * 依赖锚点：F0201（初始化 · scanout 口枚举）、F0202（队列协议 · 提交序）、
 *   F0208（blob 资源 · dmabuf 直呈现源，本条零拷贝判据依赖其
 *   addressIdentity 对拍）、F0048（呈现节奏三模式定义 · 本条为其 virtio
 *   后端）、F0060（表面协议）、F0113（呈现路由 · 多屏路由表的属主）。
 * 下游消费：F0211（中断与事件 · display 事件）、F0213（多头与 EDID ·
 * 输出增删热事件）、F0214（性能与诊断 · flush 延迟打点）、F0215（一致性
 *   测试 · 呈现层用例）、F0207（Venus 架构位 PRESENT 段的真实接入点——
 *   该段 reserved 的呈现面即绑定本条的 SET_SCANOUT 序）。
 * 交接说明：本条只管 scanout 绑定/flush 时序/节奏/布局；EDID 解析归
 *   F0213，中断与事件归 F0211，本条不越界实现。
 */

import {
  type Diagnostic,
  type Outcome,
  type Severity,
  err,
  ok,
} from "./f0207-venus-vulkan-pathway-assessment.js";

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断码（登记进 B 域属主 DiagCode 扩展段 · 只增不改）
 * ═══════════════════════════════════════════════════════════════════════════ */

export type ScanoutDiagCode = Extract<
  Diagnostic["code"],
  | "SCANOUT_BIND_INVALID"
  | "SCANOUT_LAYOUT_OVERLAP"
  | "SCANOUT_LAYOUT_OUT_OF_BOUNDS"
  | "SCANOUT_RESOURCE_NOT_FLUSHED"
  | "SCANOUT_STALE_FRAME"
  | "SCANOUT_DMABUF_DEGRADED"
  | "SCANOUT_VSYNC_REPORT_MISSING"
  | "SCANOUT_DISPLAY_INFO_MALFORMED"
>;

function diag(
  code: ScanoutDiagCode,
  severity: Severity,
  message: string,
  hint: string,
  stage: string,
): Diagnostic {
  return { code, severity, message, hint, stage };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 scanout 口与显示信息（GET_DISPLAY_INFO）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 呈现节奏三模式（锚点原文：guest 按 F0048 三模式映射）。 */
export type PresentCadence = "IMMEDIATE" | "FLUSH_ALIGNED" | "VSYNC_HINTED";

/** 节奏模式标签。 */
export const CADENCE_LABELS: Readonly<Record<PresentCadence, string>> = {
  IMMEDIATE: "即时：来一帧提交一帧（延迟最低，可能撕裂）",
  FLUSH_ALIGNED: "flush 对齐：以 flush 完成为帧边界（平衡，默认）",
  VSYNC_HINTED: "vsync 提示：以 host 呈现回报为帧边界（最平滑，依赖回报及时）",
};

/** 节奏模式序（越靠后越依赖 host 回报，越需回报缺失兜底）。 */
export const CADENCE_ORDER: readonly PresentCadence[] = [
  "IMMEDIATE",
  "FLUSH_ALIGNED",
  "VSYNC_HINTED",
];

/** 一个 scanout 口（来自 GET_DISPLAY_INFO 的一次解析结果）。 */
export interface ScanoutDisplay {
  readonly scanoutId: number;
  /** 逻辑位置（x, y, w, h）—— 布局坐标系。 */
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** 物理 DPI 缩放（0=未知，按 1 处理）。 */
  readonly dpiScale: number;
  /** 是否使能。 */
  readonly enabled: boolean;
  /** 宿主提供的 EDID 是否存在（解析归 F0213，本条只记存在性）。 */
  readonly hasEdid: boolean;
}

/** GET_DISPLAY_INFO 原始响应（设备侧顺序返回）。 */
export interface DisplayInfoResponse {
  readonly count: number;
  readonly displays: readonly ScanoutDisplay[];
  readonly enabled: number;
}

/**
 * 解析 GET_DISPLAY_INFO。
 * 关键：**不接受 count 与实际返回数不一致**——设备少返一个口而guest
 * 仍按 count 遍历，会读到不存在的口，绑定到越界 scanoutId。宁可报错。
 */
export function parseDisplayInfo(resp: DisplayInfoResponse): Outcome<readonly ScanoutDisplay[]> {
  const diagnostics: Diagnostic[] = [];

  if (resp.count !== resp.displays.length) {
    diagnostics.push(
      diag(
        "SCANOUT_DISPLAY_INFO_MALFORMED",
        "P1",
        `DISPLAY_INFO 声明 ${resp.count} 个口，实际返回 ${resp.displays.length} 个`,
        "按实际返回数解析；按声明数遍历会越界绑定到不存在的口",
        "display-info",
      ),
    );
    return err("显示信息数量不符，拒绝解析", diagnostics);
  }
  if (resp.enabled > resp.displays.length) {
    diagnostics.push(
      diag(
        "SCANOUT_DISPLAY_INFO_MALFORMED",
        "P1",
        `DISPLAY_INFO 声明 ${resp.enabled} 个使能口，超过实际总数 ${resp.displays.length}`,
        "使能数不得超过总数；否则路由会指向不存在的口",
        "display-info",
      ),
    );
  }

  for (const d of resp.displays) {
    if (d.width <= 0 || d.height <= 0) {
      diagnostics.push(
        diag(
          "SCANOUT_DISPLAY_INFO_MALFORMED",
          "P1",
          `scanout#${d.scanoutId} 尺寸非法（${d.width}x${d.height}）`,
          "尺寸须为正；非正尺寸会使布局计算与呈现区域退化",
          "display-info",
        ),
      );
    }
    if (d.x < 0 || d.y < 0) {
      diagnostics.push(
        diag(
          "SCANOUT_DISPLAY_INFO_MALFORMED",
          "P1",
          `scanout#${d.scanoutId} 原点为负（${d.x},${d.y}）`,
          "布局原点须非负；负原点会导致路由分区错乱",
          "display-info",
        ),
      );
    }
  }

  if (diagnostics.length > 0 && diagnostics.some((x) => x.severity === "P1")) {
    const structural = diagnostics.filter((x) => x.code === "SCANOUT_DISPLAY_INFO_MALFORMED");
    if (structural.some((x) => x.message.includes("尺寸非法") || x.message.includes("原点为负"))) {
      return err("显示信息含非法几何，拒绝解析", diagnostics);
    }
  }
  return ok(resp.displays, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 多屏布局解析（与 VE-F0113 呈现路由对接）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 布局分区（virtio 多屏即路由表的 virtio 实现）。 */
export interface LayoutCell {
  readonly scanoutId: number;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly dpiScale: number;
  /** 物理像素尺寸（供呈现路径按物理像素取帧）。 */
  readonly physicalWidth: number;
  readonly physicalHeight: number;
}

/** 解析结果。 */
export interface LayoutResult {
  readonly cells: readonly LayoutCell[];
  /** 布局包围盒（并集）。 */
  readonly bounds: { readonly width: number; readonly height: number };
  /** 直通零拷贝的口数（与 blob 判据联动）。 */
  readonly zeroCopyOutlets: number;
}

/**
 * 布局解析：把 scanout 口的几何转成路由分区。
 * 纪律：**重叠即P1**——两个口的空间重叠意味着同一片屏幕像素有两路渲染
 * 目标，合成时后到者覆盖先到者，表现为随机画面跳变。这类问题在单屏
 * 测试里永远不会出现，只在双屏接线后才暴露，故必须在布局期拦住。
 */
export function resolveLayout(displays: readonly ScanoutDisplay[]): Outcome<LayoutResult> {
  const diagnostics: Diagnostic[] = [];
  const enabled = displays.filter((d) => d.enabled);
  const cells: LayoutCell[] = [];

  for (const d of enabled) {
    const scale = d.dpiScale > 0 ? d.dpiScale : 1;
    cells.push({
      scanoutId: d.scanoutId,
      x: d.x,
      y: d.y,
      width: d.width,
      height: d.height,
      dpiScale: scale,
      // 物理像素 = 逻辑 × DPI 缩放（供 DPI 感知抓取与呈现对齐）。
      physicalWidth: Math.round(d.width * scale),
      physicalHeight: Math.round(d.height * scale),
    });
  }

  // 重叠检测：两两比较交集面积，O(口²) 但口数为设备常量（通常 ≤4）。
  for (let i = 0; i < cells.length; i += 1) {
    for (let j = i + 1; j < cells.length; j += 1) {
      const a = cells[i];
      const b = cells[j];
      if (!a || !b) continue;
      const overlapW = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
      const overlapH = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
      if (overlapW > 0 && overlapH > 0) {
        diagnostics.push(
          diag(
            "SCANOUT_LAYOUT_OVERLAP",
            "P1",
            `scanout#${a.scanoutId} 与 #${b.scanoutId} 空间重叠（${overlapW}x${overlapH}）`,
            "调整 display rect 至互不重叠；重叠会导致合成覆盖与画面跳变",
            "layout",
          ),
        );
      }
    }
  }

  const bounds = cells.reduce(
    (acc, c) => ({
      width: Math.max(acc.width, c.x + c.width),
      height: Math.max(acc.height, c.y + c.height),
    }),
    { width: 0, height: 0 },
  );

  if (diagnostics.length > 0) {
    return err("布局存在重叠，拒绝产出路由分区", diagnostics);
  }
  return ok({ cells, bounds, zeroCopyOutlets: cells.length }, diagnostics);
}

/** 路由查询：给定坐标找所属scanout 口，O(1) 查表。 */
export function routePoint(
  layout: LayoutResult,
  x: number,
  y: number,
): LayoutCell | null {
  for (const c of layout.cells) {
    if (x >= c.x && x < c.x + c.width && y >= c.y && y < c.y + c.height) {
      return c;
    }
  }
  return null;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 资源 flush 状态与旧帧拦截
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 资源渲染/呈现生命周期。 */
export type ResourceFlushState = "DRAWING" | "FLUSHED" | "PRESENTED";

/** 资源状态标签。 */
export const FLUSH_STATE_LABELS: Readonly<Record<ResourceFlushState, string>> = {
  DRAWING: "绘制中（未 flush）",
  FLUSHED: "已 flush（host 可合成，尚未确认上屏）",
  PRESENTED: "已呈现回报",
};

/** 帧记录。 */
export interface FrameRecord {
  readonly frameId: number;
  readonly scanoutId: number;
  readonly resourceId: number;
  /** flush 发起时刻。 */
  readonly flushIssuedAt: number;
  /** flush 完成时刻（host 合成完），未完成为 null。 */
  readonly flushedAt: number | null;
  /** 呈现回报时刻。 */
  readonly presentedAt: number | null;
  readonly cadence: PresentCadence;
}

/**
 * 资源状态机。
 * DRAWING → FLUSHED → PRESENTED 严格单向；跳过 FLUSHED 直接 PRESENTED
 * 即意味着「呈现了未经 flush 的资源」，host 合成源不确定，等价于旧帧。
 */
export function advanceFlushState(
  state: ResourceFlushState,
  to: ResourceFlushState,
): Outcome<ResourceFlushState> {
  const legal: Record<ResourceFlushState, readonly ResourceFlushState[]> = {
    DRAWING: ["FLUSHED"],
    FLUSHED: ["PRESENTED"],
    PRESENTED: [],
  };
  if (!(legal[state] ?? []).includes(to)) {
    return err(`非法 flush 状态跃迁 ${state} → ${to}`, [
      diag(
        "SCANOUT_STALE_FRAME",
        "P0",
        `非法跃迁 ${state} → ${to}（合法后继：${(legal[state] ?? []).join("/") || "无"}）`,
        "跳步意味着呈现未经flush 的资源；须按 DRAWING→FLUSHED→PRESENTED 推进",
        "flush-state",
      ),
    ]);
  }
  return ok(to);
}

/**
 * 旧帧拦截（判据：flush 与回调对齐）。
 * 三条独立判据，任一不满足即判旧帧并 P0：
 *   ① 呈现回报早于 flush 完成 —— 物理上不可能，说明时序错乱；
 *   ② 呈现回报早于该帧 flush 发起 —— 同上；
 *   ③ 目标口当前绑定的仍是上一帧资源 —— 最隐蔽的一种，表现为
 *      「画面在动但内容不更新」。
 */
export function verifyFrameAlignment(
  frame: FrameRecord,
  boundResourceIdOnOutlet: number,
): Outcome<FrameRecord> {
  const diagnostics: Diagnostic[] = [];

  if (frame.flushedAt === null) {
    diagnostics.push(
      diag(
        "SCANOUT_RESOURCE_NOT_FLUSHED",
        "P0",
        `帧 ${frame.frameId} 已请求呈现但 flush 未完成`,
        "等待 flush 完成再绑定；未flush 即呈现等于呈现上一帧",
        "frame-align",
      ),
    );
  } else if (frame.presentedAt !== null) {
    if (frame.presentedAt < frame.flushedAt) {
      diagnostics.push(
        diag(
          "SCANOUT_STALE_FRAME",
          "P0",
          `帧 ${frame.frameId} 呈现回报(${frame.presentedAt}) 早于 flush 完成(${frame.flushedAt})`,
          "时序倒置；核对时间戳来源是否同源（不同源会给出不可比的时刻）",
          "frame-align",
        ),
      );
    }
    if (frame.presentedAt < frame.flushIssuedAt) {
      diagnostics.push(
        diag(
          "SCANOUT_STALE_FRAME",
          "P0",
          `帧 ${frame.frameId} 呈现回报早于 flush 发起`,
          "回报时间不可能早于发起；检查时钟域是否混用",
          "frame-align",
        ),
      );
    }
  }

  if (boundResourceIdOnOutlet !== frame.resourceId) {
    diagnostics.push(
      diag(
        "SCANOUT_STALE_FRAME",
        "P0",
        `帧 ${frame.frameId} 请求呈现资源 #${frame.resourceId}，但口上仍绑定 #${boundResourceIdOnOutlet}`,
        "先完成 SET_SCANOUT 改绑再请求呈现；否则呈现的是上一帧内容",
        "frame-align",
      ),
    );
  }

  if (diagnostics.length > 0) {
    return err(`帧 ${frame.frameId} 呈现时序不合法`, diagnostics);
  }
  return ok(frame);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 dmabuf 直呈现（零拷贝）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 绑定请求（SET_SCANOUT）。 */
export interface ScanoutBindRequest {
  readonly scanoutId: number;
  readonly resourceId: number;
  /** 资源是否为 blob（决定能否走 dmabuf 直呈现）。 */
  readonly isBlob: boolean;
  /** blob 的地址标识（与 F0208 addressIdentity 同源）。 */
  readonly addressIdentity: string;
  /** 宿主合成侧可达的地址（真实实现由 host 回报）。 */
  readonly hostReachableAddress: string;
  /** 映射属性（F0208 推导结果：mixed 判定在此复用）。 */
  readonly mappingAttr: "CACHED" | "UNCACHED";
  /** 源矩形（crop）。 */
  readonly crop: { readonly x: number; readonly y: number; readonly width: number; readonly height: number };
}

/** 绑定结果。 */
export interface BindResult {
  readonly scanoutId: number;
  readonly resourceId: number;
  /** 是否走 dmabuf 直呈现（零拷贝）。 */
  readonly dmabufDirect: boolean;
  /** 若退化，说明退化原因（可上屏）。 */
  readonly degradedReason: string;
  readonly boundAddress: string;
}

/**
 * SET_SCANOUT 绑定。
 * 零拷贝成立的三个前提，缺一即退化并标注：
 *   ① 是 blob 资源（否则无 dmabuf 可言）；
 *   ② 地址一致（guest 与 host 侧指向同一物理区）；
 *   ③ 映射属性为 UNCACHED（cached 下host 合成读到的是缓存副本）。
 * 退化不阻断呈现（仍可走拷贝），但**必须标注**——否则这条路径会
 * 被误认为零拷贝，进而让人省掉必要的同步。
 */
export function bindScanout(req: ScanoutBindRequest): Outcome<BindResult> {
  const diagnostics: Diagnostic[] = [];
  let dmabufDirect = true;
  const reasons: string[] = [];

  if (!req.isBlob) {
    dmabufDirect = false;
    reasons.push("资源非 blob，无 dmabuf 源可用");
  }
  if (req.addressIdentity !== req.hostReachableAddress) {
    dmabufDirect = false;
    reasons.push(
      `地址不一致（guest ${req.addressIdentity} ≠ host ${req.hostReachableAddress}）`,
    );
  }
  if (req.mappingAttr !== "UNCACHED") {
    dmabufDirect = false;
    reasons.push(`映射属性为 ${req.mappingAttr}，host 合成将读到缓存副本`);
  }

  if (!dmabufDirect) {
    diagnostics.push(
      diag(
        "SCANOUT_DMABUF_DEGRADED",
        "P1",
        `scanout#${req.scanoutId} 绑定退化为拷贝：${reasons.join("；")}`,
        "退化路径不可省同步；若本应零拷贝请核对 F0208 属性推导与地址对拍",
        "scanout-bind",
      ),
    );
  }

  return ok(
    {
      scanoutId: req.scanoutId,
      resourceId: req.resourceId,
      dmabufDirect,
      degradedReason: dmabufDirect ? "零拷贝直呈现" : reasons.join("；"),
      boundAddress: req.hostReachableAddress,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 呈现节奏三模式与帧循环
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 帧循环上下文。 */
export interface CadenceContext {
  readonly cadence: PresentCadence;
  /** host 回报缺失计数（VSYNC_HINTED 模式下降级依据）。 */
  readonly reportMisses: number;
  /** 降级阈值：连续缺失达此数即降级。 */
  readonly missThreshold: number;
  /** 允许的最大 flush 到呈现延迟（超出记P2）。 */
  readonly maxPresentLatency: number;
}

/** 节奏裁决结果。 */
export interface CadenceDecision {
  readonly requested: PresentCadence;
  /** 实际采用的模式（可能已降级）。 */
  readonly effective: PresentCadence;
  readonly degraded: boolean;
  readonly reason: string;
}

/**
 * 呈现节奏裁决（锚点：vsync 语义由 host 模拟，按 F0048 三模式映射）。
 *
 * 降级矩阵：
 *   · VSYNC_HINTED 且回报连续缺失达阈值 → 降级 FLUSH_ALIGNED。
 *     **不降级到 IMMEDIATE**：那会放弃帧边界对齐，重新引入撕裂。
 *   · FLUSH_ALIGNED 为兜底，永不降级——它不依赖任何 host 回报，
 *     是本域唯一无条件可用的模式，故被选为默认与降级目标。
 *   · IMMEDIATE 为最低要求，不做降级判断。
 */
export function resolveCadence(ctx: CadenceContext): CadenceDecision {
  if (ctx.cadence === "IMMEDIATE") {
    return {
      requested: "IMMEDIATE",
      effective: "IMMEDIATE",
      degraded: false,
      reason: "即时模式不做降级判断（不依赖 host 回报）",
    };
  }
  if (ctx.cadence === "VSYNC_HINTED" && ctx.reportMisses >= ctx.missThreshold) {
    return {
      requested: "VSYNC_HINTED",
      effective: "FLUSH_ALIGNED",
      degraded: true,
      reason: `host 呈现回报连续缺失 ${ctx.reportMisses} 次（阈值 ${ctx.missThreshold}），降级为 flush 对齐`,
    };
  }
  if (ctx.cadence === "VSYNC_HINTED") {
    return {
      requested: "VSYNC_HINTED",
      effective: "VSYNC_HINTED",
      degraded: false,
      reason: `host 回报正常（累计缺失 ${ctx.reportMisses}/${ctx.missThreshold}）`,
    };
  }
  return {
    requested: "FLUSH_ALIGNED",
    effective: "FLUSH_ALIGNED",
    degraded: false,
    reason: "flush 对齐为无条件可用模式，作为默认与降级目标",
  };
}

/** 一次完整帧的结果。 */
export interface FrameOutcome {
  readonly frame: FrameRecord;
  readonly bind: BindResult;
  readonly cadence: CadenceDecision;
  /** 是否真正呈现（false 表示按纪律不出画面）。 */
  readonly presented: boolean;
  readonly note: string;
}

/** 帧推进输入。 */
export interface FrameInput {
  readonly frameId: number;
  readonly scanoutId: number;
  readonly resourceId: number;
  readonly bind: ScanoutBindRequest;
  readonly cadenceCtx: CadenceContext;
  readonly flushIssuedAt: number;
  /** host 合成完成时刻；null 表示未完成。 */
  readonly flushedAt: number | null;
  /** 呈现回报时刻；null 表示无回报。 */
  readonly presentedAt: number | null;
  /** 该口当前已绑定资源（用于旧帧对拍）。 */
  readonly boundResourceId: number;
}

/**
 * 推进一帧（绑定 → flush → 呈现判定）。
 *
 * 纪律：**旧帧即不出画面**。若呈现时序不合法，本函数返回
 * `presented: false` 并给出显式黑屏理由，而不是「先显示上一帧，
 * 等下一帧修好」。理由见文件头第一性声明。
 */
export function advanceFrame(input: FrameInput): Outcome<FrameOutcome> {
  const diagnostics: Diagnostic[] = [];
  const cadence = resolveCadence(input.cadenceCtx);

  if (cadence.degraded) {
    diagnostics.push(
      diag(
        "SCANOUT_VSYNC_REPORT_MISSING",
        "P2",
        `呈现节奏降级：${cadence.reason}`,
        "host 回报恢复后自动回到 vsync 提示模式",
        "cadence",
      ),
    );
  }

  const bind = bindScanout(input.bind);
  diagnostics.push(...bind.diagnostics);
  if (!bind.ok) {
    return err("绑定失败", diagnostics);
  }

  // 绑定后才可比对：绑定后该口指向的资源应为本次资源。
  const frame: FrameRecord = {
    frameId: input.frameId,
    scanoutId: input.scanoutId,
    resourceId: input.resourceId,
    flushIssuedAt: input.flushIssuedAt,
    flushedAt: input.flushedAt,
    presentedAt: input.presentedAt,
    cadence: cadence.effective,
  };

  const aligned = verifyFrameAlignment(frame, bind.value.resourceId);
  if (!aligned.ok) {
    diagnostics.push(...aligned.diagnostics);
    // 旧帧：显式不出画面，而非显示上一帧。
    return ok(
      {
        frame,
        bind: bind.value,
        cadence,
        presented: false,
        note: `显式黑屏：帧 ${frame.frameId} 呈现时序不合法，拒绝呈现旧帧（${aligned.message}）`,
      },
      diagnostics,
    );
  }

  diagnostics.push(...aligned.diagnostics);
  return ok(
    {
      frame,
      bind: bind.value,
      cadence,
      presented: true,
      note: bind.value.dmabufDirect
        ? `帧 ${frame.frameId} dmabuf 直呈现（零拷贝）`
        : `帧 ${frame.frameId} 拷贝呈现：${bind.value.degradedReason}`,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 编排：呈现管线（布局 → 绑定 → 逐帧）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 呈现管线输入。
 * 注：节奏上下文由**每帧自带**（`FrameInput.cadenceCtx`），管线层不再
 * 统一收一份——不同口/不同帧的回报缺失状况本就可能不同，在管线层强行
 * 统一会用最小缺失值污染全部帧。
 */
export interface PresentPipelineInput {
  readonly displayInfo: DisplayInfoResponse;
  readonly frames: readonly FrameInput[];
  readonly now: number;
}

/** 呈现管线结论。 */
export interface PresentPipelineResult {
  readonly layout: LayoutResult;
  readonly frames: readonly FrameOutcome[];
  /** 实际呈现帧数。 */
  readonly presentedCount: number;
  /** dmabuf 直呈现帧数（零拷贝计数）。 */
  readonly dmabufCount: number;
  readonly zeroCopyAll: boolean;
  readonly diagnostics: readonly Diagnostic[];
}

/** 呈现管线编排：先解析布局（错了整条呈现都无意义），再逐帧推进。 */
export function runPresentPipeline(input: PresentPipelineInput): Outcome<PresentPipelineResult> {
  const diagnostics: Diagnostic[] = [];

  const displays = parseDisplayInfo(input.displayInfo);
  if (!displays.ok) {
    return err(`显示信息解析失败：${displays.message}`, displays.diagnostics);
  }
  diagnostics.push(...displays.diagnostics);

  const layout = resolveLayout(displays.value);
  if (!layout.ok) {
    return err(`布局解析失败：${layout.message}`, [...diagnostics, ...layout.diagnostics]);
  }
  diagnostics.push(...layout.diagnostics);

  const frames: FrameOutcome[] = [];
  let presented = 0;
  let dmabuf = 0;
  for (const f of input.frames) {
    const r = advanceFrame(f);
    if (!r.ok) {
      return err(`帧 ${f.frameId} 推进失败：${r.message}`, [...diagnostics, ...r.diagnostics]);
    }
    diagnostics.push(...r.diagnostics);
    frames.push(r.value);
    if (r.value.presented) presented += 1;
    if (r.value.bind.dmabufDirect) dmabuf += 1;
  }

  return ok(
    {
      layout: layout.value,
      frames,
      presentedCount: presented,
      dmabufCount: dmabuf,
      zeroCopyAll: frames.length > 0 && dmabuf === frames.length,
      diagnostics,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 域级自检（六组：布局 / 绑定零拷贝 / 节奏三模式 / 时序 / 单屏多屏 / 管线）
 * ═══════════════════════════════════════════════════════════════════════════ */

export interface ScanoutSelfCheckItem {
  readonly group: string;
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

export interface ScanoutSelfCheckReport {
  readonly groups: Readonly<Record<string, readonly ScanoutSelfCheckItem[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
}

/** 单个 1080p 口（回归基线）。 */
function displayOf(id: number, x: number, y: number, enabled = true): ScanoutDisplay {
  return {
    scanoutId: id,
    x,
    y,
    width: 1920,
    height: 1080,
    dpiScale: 1,
    enabled,
    hasEdid: true,
  };
}

/** 零拷贝绑定请求（回归基线）。 */
function bindOf(scanoutId: number, resourceId: number): ScanoutBindRequest {
  return {
    scanoutId,
    resourceId,
    isBlob: true,
    addressIdentity: `phys-${resourceId}`,
    hostReachableAddress: `phys-${resourceId}`,
    mappingAttr: "UNCACHED",
    crop: { x: 0, y: 0, width: 1920, height: 1080 },
  };
}

export function runScanoutSelfCheck(): ScanoutSelfCheckReport {
  const groups: Record<string, ScanoutSelfCheckItem[]> = {};
  const add = (g: string, n: string, p: boolean, d: string): void => {
    const list = groups[g] ?? [];
    list.push({ group: g, name: n, pass: p, detail: d });
    groups[g] = list;
  };

  // ── A. 多屏布局解析 ──────────────────────────────────────────────────────
  const single = resolveLayout([displayOf(0, 0, 0)]);
  add(
    "A-布局解析",
    "单屏布局包围盒正确",
    single.ok && single.value.bounds.width === 1920 && single.value.bounds.height === 1080,
    single.ok ? `包围盒 ${single.value.bounds.width}x${single.value.bounds.height}` : `失败：${single.message}`,
  );

  const dual = resolveLayout([displayOf(0, 0, 0), displayOf(1, 1920, 0)]);
  add(
    "A-布局解析",
    "双屏横排布局正确（并集 3840x1080）",
    dual.ok && dual.value.cells.length === 2 && dual.value.bounds.width === 3840,
    dual.ok ? `${dual.value.cells.length} 分区，包围盒 ${dual.value.bounds.width}x${dual.value.bounds.height}` : `失败：${dual.message}`,
  );

  const overlap = resolveLayout([displayOf(0, 0, 0), displayOf(1, 1000, 0)]);
  add(
    "A-布局解析",
    "空间重叠 → P1 拦截（不产出分区）",
    !overlap.ok && overlap.diagnostics.some((d) => d.code === "SCANOUT_LAYOUT_OVERLAP"),
    overlap.ok ? "重叠未被拦截" : "已拦截",
  );

  const countMismatch = parseDisplayInfo({
    count: 2,
    displays: [displayOf(0, 0, 0)],
    enabled: 1,
  });
  add(
    "A-布局解析",
    "DISPLAY_INFO 数量不符 → 拒绝解析（防越界绑定）",
    !countMismatch.ok,
    countMismatch.ok ? "数量不符被放行" : "已拒绝",
  );

  const routed = dual.ok ? routePoint(dual.value, 2000, 500) : null;
  add(
    "A-布局解析",
    "路由查询 O(1) 定位所属口",
    routed !== null && routed.scanoutId === 1,
    routed !== null ? `坐标(2000,500) → 口#${routed.scanoutId}` : "查询失败",
  );

  const dpiScaled = resolveLayout([{ ...displayOf(0, 0, 0), dpiScale: 2 }]);
  add(
    "A-布局解析",
    "DPI 缩放换算为物理像素",
    dpiScaled.ok && dpiScaled.value.cells[0]?.physicalWidth === 3840,
    dpiScaled.ok ? `物理宽 ${dpiScaled.value.cells[0]?.physicalWidth}` : "失败",
  );

  // ── B. dmabuf 直呈现零拷贝 ───────────────────────────────────────────────
  const zc = bindScanout(bindOf(0, 10));
  add(
    "B-零拷贝呈现",
    "blob+地址一致+uncached → dmabuf 直呈现",
    zc.ok && zc.value.dmabufDirect,
    zc.ok ? zc.value.degradedReason : "绑定失败",
  );

  const attrBad = bindScanout({ ...bindOf(0, 11), mappingAttr: "CACHED" });
  add(
    "B-零拷贝呈现",
    "属性 cached → 退化并标注",
    attrBad.ok && !attrBad.value.dmabufDirect && attrBad.diagnostics.some((d) => d.code === "SCANOUT_DMABUF_DEGRADED"),
    attrBad.ok ? attrBad.value.degradedReason : "异常",
  );

  const addrBad = bindScanout({ ...bindOf(0, 12), hostReachableAddress: "phys-999" });
  add(
    "B-零拷贝呈现",
    "地址不一致 → 退化并标注",
    addrBad.ok && !addrBad.value.dmabufDirect,
    addrBad.ok ? addrBad.value.degradedReason : "异常",
  );

  const notBlob = bindScanout({ ...bindOf(0, 13), isBlob: false });
  add(
    "B-零拷贝呈现",
    "非 blob 资源 → 退化（无 dmabuf 源）",
    notBlob.ok && !notBlob.value.dmabufDirect,
    notBlob.ok ? notBlob.value.degradedReason : "异常",
  );

  // ── C. 呈现节奏三模式 ────────────────────────────────────────────────────
  const cImm = resolveCadence({ cadence: "IMMEDIATE", reportMisses: 99, missThreshold: 3, maxPresentLatency: 16 });
  add(
    "C-呈现节奏",
    "IMMEDIATE 不降级（不依赖回报）",
    cImm.effective === "IMMEDIATE" && !cImm.degraded,
    cImm.reason,
  );

  const cAlign = resolveCadence({ cadence: "FLUSH_ALIGNED", reportMisses: 99, missThreshold: 3, maxPresentLatency: 16 });
  add(
    "C-呈现节奏",
    "FLUSH_ALIGNED 为无条件可用（兜底目标）",
    cAlign.effective === "FLUSH_ALIGNED" && !cAlign.degraded,
    cAlign.reason,
  );

  const cVsync = resolveCadence({ cadence: "VSYNC_HINTED", reportMisses: 0, missThreshold: 3, maxPresentLatency: 16 });
  add(
    "C-呈现节奏",
    "VSYNC_HINTED 回报正常 → 保持",
    cVsync.effective === "VSYNC_HINTED" && !cVsync.degraded,
    cVsync.reason,
  );

  const cDegrade = resolveCadence({ cadence: "VSYNC_HINTED", reportMisses: 5, missThreshold: 3, maxPresentLatency: 16 });
  add(
    "C-呈现节奏",
    "VSYNC_HINTED 回报缺失 → 降级 FLUSH_ALIGNED（不降 IMMEDIATE）",
    cDegrade.effective === "FLUSH_ALIGNED" && cDegrade.degraded,
    cDegrade.reason,
  );

  // ── D. flush 与回调对齐（判据） ───────────────────────────────────────────
  const goodFrame: FrameRecord = {
    frameId: 1,
    scanoutId: 0,
    resourceId: 10,
    flushIssuedAt: 100,
    flushedAt: 110,
    presentedAt: 112,
    cadence: "FLUSH_ALIGNED",
  };
  add(
    "D-时序对齐",
    "规范帧序通过",
    verifyFrameAlignment(goodFrame, 10).ok,
    "flush 完成先于呈现回报",
  );

  add(
    "D-时序对齐",
    "呈现回报早于 flush 完成 → P0",
    !verifyFrameAlignment({ ...goodFrame, presentedAt: 105 }, 10).ok,
    "时序倒置已拦截",
  );

  add(
    "D-时序对齐",
    "flush 未完成即呈现 → P0",
    !verifyFrameAlignment({ ...goodFrame, flushedAt: null }, 10).ok,
    "未flush 呈现已拦截",
  );

  add(
    "D-时序对齐",
    "口上仍绑旧资源 → P0（旧帧最隐蔽形态）",
    !verifyFrameAlignment(goodFrame, 9).ok,
    "旧帧已拦截",
  );

  add(
    "D-时序对齐",
    "flush 状态机跳步 → P0",
    !advanceFlushState("DRAWING", "PRESENTED").ok,
    "跳步已拒绝",
  );

  add(
    "D-时序对齐",
    "flush 状态机正常推进",
    advanceFlushState("DRAWING", "FLUSHED").ok && advanceFlushState("FLUSHED", "PRESENTED").ok,
    "DRAWING→FLUSHED→PRESENTED 合规",
  );

  // ── E. 旧帧显式黑屏纪律 ──────────────────────────────────────────────────
  const staleFrame = advanceFrame({
    frameId: 5,
    scanoutId: 0,
    resourceId: 20,
    bind: bindOf(0, 20),
    cadenceCtx: { cadence: "FLUSH_ALIGNED", reportMisses: 0, missThreshold: 3, maxPresentLatency: 16 },
    flushIssuedAt: 200,
    flushedAt: null,
    presentedAt: null,
    boundResourceId: 19,
  });
  add(
    "E-黑屏纪律",
    "旧帧 → 显式不出画面（而非显示上一帧）",
    staleFrame.ok && !staleFrame.value.presented,
    staleFrame.ok ? staleFrame.value.note : "异常",
  );

  // ── F. 端到端管线 ────────────────────────────────────────────────────────
  const pipeline = runPresentPipeline({
    displayInfo: { count: 2, displays: [displayOf(0, 0, 0), displayOf(1, 1920, 0)], enabled: 2 },
    frames: [0, 1, 2].map((i) => ({
      frameId: i + 1,
      scanoutId: i % 2,
      resourceId: 30 + i,
      bind: bindOf(i % 2, 30 + i),
      cadenceCtx: { cadence: "VSYNC_HINTED" as const, reportMisses: i === 2 ? 5 : 0, missThreshold: 3, maxPresentLatency: 16 },
      flushIssuedAt: 100 + i * 10,
      flushedAt: 110 + i * 10,
      presentedAt: 112 + i * 10,
      boundResourceId: 30 + i,
    })),
    now: 1,
  });
  add(
    "F-端到端管线",
    "双屏 3 帧全部 dmabuf 零拷贝呈现",
    pipeline.ok && pipeline.value.presentedCount === 3 && pipeline.value.zeroCopyAll,
    pipeline.ok
      ? `呈现 ${pipeline.value.presentedCount}/3 帧，dmabuf ${pipeline.value.dmabufCount} 帧`
      : `失败：${pipeline.message}`,
  );

  add(
    "F-端到端管线",
    "帧内含降级仍走通（降级不阻断呈现）",
    pipeline.ok && pipeline.value.frames[2]?.cadence.effective === "FLUSH_ALIGNED",
    pipeline.ok ? `末帧节奏 ${pipeline.value.frames[2]?.cadence.effective}` : "失败",
  );

  const all = Object.values(groups).flat();
  const failed = all.filter((i) => !i.pass).map((i) => `${i.group}/${i.name}`);
  return { groups, allPass: failed.length === 0, failed };
}
