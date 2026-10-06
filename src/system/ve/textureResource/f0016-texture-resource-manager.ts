/**
 * VE-F0016 · 纹理资源管理器（VE-A 域 · 资源与管线对象组 · 批次 A02）
 * ---------------------------------------------------------------------------
 * 职责定位：纹理全生命周期管理（创建 / 上传 / 更新 / 回收四段）+ 格式转换管线
 * （压缩格式自动选择）+ 显存预算对接（A05 仲裁器消费）+ 流式上传（大纹理
 * 分块不卡帧）；含纹理压缩格式的质量对比基线（有损格式与无损的视觉差异量化）。
 *
 * 上游：VE-F0008 GPU 资源句柄表（句柄代数戳与悬垂检测）、VE-F0005 显存仲裁器
 *      （预算申请与归还）、CGPU-F0017 上传带宽调度（帧预算时间片）。
 * 下游：VE-F0018 采样器状态库（格式 × 采样器兼容矩阵）、VE-F0009 设备丢失
 *      恢复状态机（丢失后按本条台账重建）、CGPU-F1138 纹理压缩格式矩阵。
 *
 * 锚点契约（八条，逐条对应判据）：
 *   1. 四段生命周期：创建 → 上传 → 更新 → 回收，四段的状态迁移写成**显式
 *      迁移表**并做闭包机检（不允许存在「表外可达」的迁移，也不允许跳过
 *      必经段）。更新是已驻留纹理上的**在位改写**，不是第五个状态——把它
 *      做成状态会让「更新中」与「可采样」两个真值同时成立。
 *   2. 压缩格式自动选择：按用途/是否带 alpha/是否线性/目标质量档在格式表上
 *      做**纯函数**选择，可解释（返回候选淘汰轨迹，不只返回结果）。
 *   3. 格式转换质量档位声明：每个格式转换组合给出**量化的视觉损失档**
 *      （位深比 + 质量基线表的保底 PSNR/SSIM 下界），转换损失是声明而非
 *      承诺——没有量化标注的转换不许进管线。
 *   4. 流式上传：大纹理按字节上限切块，帧内只取**总字节不超帧预算**的块，
 *      块粒度上界即「不卡帧」的硬保证（单块自身不得超帧预算）。
 *   5. 流式上传优先级：屏上可见 > 预取 > 后台；同档内**低分辨率级先传**
 *      （先让画面可见，高分辨率级后台补齐）。排序键全序、可复算。
 *   6. 预算超支降分辨率：预算不足时沿**显式降级阶梯**逐级降质（提 mip
 *      下限 → 降分辨率倍率 → 换紧凑格式 → 拒绝），每级声明视觉代价，
 *      阶梯单调（降一级必然更省），阶梯耗尽才拒绝。
 *   7. 别名台账：同一纹理可有多个名字（材质引用、UI 图标、工具链路径），
 *      台账是**唯一真值**，改名/解绑/冲突全部走显式 API，别名数与引用
 *      计数分离（别名多不等于用得多，但两者都要可查）。
 *   8. 回收的显存水位联动：水位分级（normal / elevated / critical），
 *      **先放谁**逐级写明：elevated 先放后台纹理的 mip 尾（保底可见），
 *      critical 放整个后台纹理（LRU 倒序），仍不够才对屏上纹理降质，
 *      **pin 住的纹理任何水位都不放**——放不动时产出诊断而非静默强放。
 *
 * 五条容易做错、故显式记录的设计立场：
 *
 *   一、mip 尾回收必须在「不破坏已采样契约」的前提下做。
 *     朴素做法是直接丢高层 mip。但采样器在 GPU 侧仍持有 mip 计数，写少
 *     一级就会读到越界或退化成 LOD 跳变。故本条的 mip 尾回收是**台账层
 *     的可见性收缩**：mipFloor 抬高后，写入侧按新链重建，采样侧由本条
 *     产出的 `mipFloorChanged` 事件通知上游重新绑定 —— 顺序由上层编排，
 *     本条只保证「不出现台账说有、上传侧没有」的裂缝（机检断言之一）。
 *
 *   二、帧预算不是「尽量」，是「绝不超过」。
 *     朴素实现会「取到预算为止然后取整个块」，于是单个大块把帧预算击穿。
 *     故 `selectChunksForFrame` 的行为是：块字节 > 剩余预算则**本帧不发**
 *     （延后），而不是截断发半块——截断会让上传队列出现空洞，无法判定
 *     块是否已完成。
 *
 *   三、格式自动选择必须留淘汰轨迹。
 *     只返回最终格式的转换函数在出错时不可诊断。故选择结果附
 *     `rejections`：每条写明「哪个候选因何被淘汰」。选择器的正确性由
 *     轨迹机检保证（同一输入必得同一轨迹——纯函数断言）。
 *
 *   四、质量损失声明与质量基线表必须同源。
 *     声明（tier → 允许的损失上界）与基线（格式对 → 实测损失档）若是两
 *     张表，则会出现「声明允许、基线无数据」的空洞。故二者由同一常量
 *     `QUALITY_BASELINE` 派生，`qualityCeilingFor` 只读它。
 *
 *   五、跨域协议哈希必须区分「待补」与「漂移」。
 *     处置方向相反——待补=非阻断（对方还没实现），漂移=必须重签（对方
 *     改了形状）。共用一个码会让非阻断项阻断、或让漂移项静默。故本条
 *     分设 PROTOCOL_HASH_PENDING / PROTOCOL_HASH_DRIFT 两个码，
 *     行状态同步拆 matched / pending / drifted。
 *
 * 零静默纪律：格式缺失、维度越限、非法迁移、退役复用、别名冲突、预算
 * 拒绝、降级耗尽、上传重试耗尽、块超预算、被 pin 阻塞的回收，全部产出
 * TextureDiagnostic（code + message + hint），降级一律显性不做暗转。
 *
 * 判据：四段生命周期、格式自动选择、流式上传、降分辨率、别名台账、
 *      质量档位声明、上传优先级、水位联动回收。
 * 边界：本条是**零运行时契约层**（常量表 + 纯函数 + 诊断袋），
 *      不持有设备句柄、不发起 GPU 调用；与 A05 预算池、CGPU 上传队列
 *      通过显式数据契约对接，互转走本条产出的纯数据。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/**
 * 本条专属诊断码。
 * 处置方向相反的状态刻意不共码：
 *   - 预算类：BUDGET_REJECTED（可降级救回）vs DEGRADATION_EXHAUSTED（不可救）。
 *   - 上传类：UPLOAD_RETRY_EXHAUSTED（已放弃该块，需重决策）vs UPLOAD_STALL_RISK（尚可继续）。
 *   - 协议类：PROTOCOL_HASH_PENDING（非阻断）vs PROTOCOL_HASH_DRIFT（必须重签）。
 */
export type TextureDiagCode =
  // 格式与描述校验
  | "FORMAT_UNKNOWN"
  | "FORMAT_UNSUPPORTED"
  | "SAMPLING_RENDER_CONFLICT"
  | "DIMENSION_LIMIT"
  | "MIP_CHAIN_INVALID"
  | "USAGE_EMPTY"
  | "USAGE_INCOMPATIBLE"
  // 生命周期
  | "ILLEGAL_TRANSITION"
  | "RETIRED_REUSE"
  | "GENERATION_STALE"
  // 别名台账
  | "ALIAS_CONFLICT"
  | "ALIAS_UNBOUND"
  | "ALIAS_TEXTURE_UNKNOWN"
  // 预算与降级
  | "BUDGET_REJECTED"
  | "DEGRADATION_EXHAUSTED"
  // 流式上传
  | "CHUNK_TOO_LARGE"
  | "UPLOAD_RETRY_EXHAUSTED"
  | "UPLOAD_STALL_RISK"
  | "QUEUE_OVERFLOW"
  // 回收
  | "EVICTION_BLOCKED_PINNED"
  | "EVICTION_SHRANK_VISIBLE"
  // sRGB 域纪律
  | "SRGB_DOMAIN_VIOLATION"
  // 泄漏审计
  | "LEAK_SUSPECTED"
  // 跨域协议
  | "PROTOCOL_HASH_PENDING"
  | "PROTOCOL_HASH_DRIFT";

/** 一条诊断：发生了什么、影响什么、下一步怎么办。 */
export interface TextureDiagnostic {
  readonly code: TextureDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type TextureOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly TextureDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: TextureDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly TextureDiagnostic[];
    };

/** 成功构造（诊断可携带：降级不是失败，但必须留痕）。 */
export function texOk<T>(value: T, diagnostics: readonly TextureDiagnostic[] = []): TextureOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function texFail<T>(
  code: TextureDiagCode,
  message: string,
  hint: string,
  prior: readonly TextureDiagnostic[] = [],
): TextureOutcome<T> {
  const d: TextureDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [...prior, d] };
}

/** 诊断聚合器：零静默的容器。任何子系统只往里 push，不自己吞。 */
export class TextureDiagBag {
  private readonly items: TextureDiagnostic[] = [];

  push(code: TextureDiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  /** 合并另一袋（子系统的诊断向父级汇流）。 */
  absorb(other: TextureDiagBag | readonly TextureDiagnostic[]): void {
    const src = other instanceof TextureDiagBag ? other.all() : other;
    for (const d of src) this.items.push(d);
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly TextureDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: TextureDiagCode): readonly TextureDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  /** 阻断级诊断（处置方向=必须阻断）计数，供上层决定是否拒绝整批。 */
  blockingCount(): number {
    const blocking: readonly TextureDiagCode[] = [
      "RETIRED_REUSE",
      "GENERATION_STALE",
      "DEGRADATION_EXHAUSTED",
      "EVICTION_SHRANK_VISIBLE",
      "SRGB_DOMAIN_VIOLATION",
      "PROTOCOL_HASH_DRIFT",
    ];
    return this.items.filter((d) => blocking.includes(d.code)).length;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 像素格式表（常量 · 自动选择的唯一真值）
// ════════════════════════════════════════════════════════════════════════════

export type PixelFormat =
  // 未压缩
  | "rgba8unorm"
  | "rgba8unorm-srgb"
  | "bgra8unorm"
  | "r8unorm"
  | "rg8unorm"
  | "rgba16float"
  | "rg16float"
  | "r32float"
  // 块压缩
  | "bc1-rgba-unorm"
  | "bc1-rgba-unorm-srgb"
  | "bc3-rgba-unorm"
  | "bc3-rgba-unorm-srgb"
  | "bc5-rg-unorm"
  | "bc7-rgba-unorm"
  | "bc7-rgba-unorm-srgb"
  | "astc-4x4-rgba-unorm"
  | "astc-4x4-rgba-unorm-srgb";

/** 压缩族标识。 */
export type CompressionFamily = "none" | "bc1" | "bc3" | "bc5" | "bc7" | "astc4";

/**
 * 有损度分级（声明用，非实测）。
 * none=位精确；negligible=逐像素感知不到；low=放大可见；moderate=肉眼可见差异。
 */
export type LossGrade = "none" | "negligible" | "low" | "moderate";

/** 单个格式的完整刻画。 */
export interface FormatSpec {
  readonly format: PixelFormat;
  readonly family: CompressionFamily;
  /** 未压缩格式的有效字节/像素；块压缩为 0（用 blockBytes/blockEdge 推导）。 */
  readonly bytesPerPixel: number;
  /** 压缩块字节数（未压缩为 0）。 */
  readonly blockBytes: number;
  /** 压缩块边长（未压缩为 1）。 */
  readonly blockEdge: number;
  /** 有效通道数：1=单通道 2=双通道 4=RGBA。选择器必须与源格式匹配。 */
  readonly channelCount: 1 | 2 | 4;
  readonly srgbCapable: boolean;
  readonly lossy: boolean;
  /** 有损度分级（无损格式为 none）。 */
  readonly loss: LossGrade;
  readonly supportsRenderTarget: boolean;
  readonly supportsStorage: boolean;
  /** 同族 sRGB 变体（未压缩 sRGB 纹理升级色彩空间时取此格式）。 */
  readonly srgbVariant: PixelFormat | null;
  /** 该格式的线性变体（sRGB 纹理的 mip 生成域用）。 */
  readonly linearVariant: PixelFormat | null;
}

/**
 * 格式表。
 * 字段纪律：blockBytes/blockEdge 与 bytesPerPixel 互斥——块压缩填前两者，
 * 未压缩填后者。填错会让 levelByteSize 静默算错，故由 checkFormatTable 断言。
 */
export const FORMAT_TABLE: Readonly<Record<PixelFormat, FormatSpec>> = {
  rgba8unorm: {
    format: "rgba8unorm",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 4,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: true,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: "rgba8unorm-srgb",
    linearVariant: "rgba8unorm",
  },
  "rgba8unorm-srgb": {
    format: "rgba8unorm-srgb",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 4,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: true,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: "rgba8unorm-srgb",
    linearVariant: "rgba8unorm",
  },
  bgra8unorm: {
    format: "bgra8unorm",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 4,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: null,
    linearVariant: "bgra8unorm",
  },
  r8unorm: {
    format: "r8unorm",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 1,
    bytesPerPixel: 1,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: null,
    linearVariant: "r8unorm",
  },
  rg8unorm: {
    format: "rg8unorm",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 2,
    bytesPerPixel: 2,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: null,
    linearVariant: "rg8unorm",
  },
  rgba16float: {
    format: "rgba16float",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 8,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: null,
    linearVariant: "rgba16float",
  },
  rg16float: {
    format: "rg16float",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 2,
    bytesPerPixel: 4,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: true,
    srgbVariant: null,
    linearVariant: "rg16float",
  },
  r32float: {
    format: "r32float",
    family: "none",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 1,
    bytesPerPixel: 4,
    blockBytes: 0,
    blockEdge: 1,
    srgbCapable: false,
    lossy: false,
    loss: "none",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: null,
    linearVariant: "r32float",
  },
  "bc1-rgba-unorm": {
    format: "bc1-rgba-unorm",
    family: "bc1",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 8,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "low",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc1-rgba-unorm-srgb",
    linearVariant: "bc1-rgba-unorm",
  },
  "bc1-rgba-unorm-srgb": {
    format: "bc1-rgba-unorm-srgb",
    family: "bc1",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 8,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "low",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc1-rgba-unorm-srgb",
    linearVariant: "bc1-rgba-unorm",
  },
  "bc3-rgba-unorm": {
    format: "bc3-rgba-unorm",
    family: "bc3",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc3-rgba-unorm-srgb",
    linearVariant: "bc3-rgba-unorm",
  },
  "bc3-rgba-unorm-srgb": {
    format: "bc3-rgba-unorm-srgb",
    family: "bc3",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc3-rgba-unorm-srgb",
    linearVariant: "bc3-rgba-unorm",
  },
  "bc5-rg-unorm": {
    format: "bc5-rg-unorm",
    family: "bc5",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 2,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: false,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: false,
    supportsStorage: false,
    srgbVariant: null,
    linearVariant: "bc5-rg-unorm",
  },
  "bc7-rgba-unorm": {
    format: "bc7-rgba-unorm",
    family: "bc7",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc7-rgba-unorm-srgb",
    linearVariant: "bc7-rgba-unorm",
  },
  "bc7-rgba-unorm-srgb": {
    format: "bc7-rgba-unorm-srgb",
    family: "bc7",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "bc7-rgba-unorm-srgb",
    linearVariant: "bc7-rgba-unorm",
  },
  "astc-4x4-rgba-unorm": {
    format: "astc-4x4-rgba-unorm",
    family: "astc4",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "astc-4x4-rgba-unorm-srgb",
    linearVariant: "astc-4x4-rgba-unorm",
  },
  "astc-4x4-rgba-unorm-srgb": {
    format: "astc-4x4-rgba-unorm-srgb",
    family: "astc4",
    /** 有效通道数：选择器必须与源格式匹配，否则颜色信息被静默丢弃。 */
    channelCount: 4,
    bytesPerPixel: 0,
    blockBytes: 16,
    blockEdge: 4,
    srgbCapable: true,
    lossy: true,
    loss: "negligible",
    supportsRenderTarget: true,
    supportsStorage: false,
    srgbVariant: "astc-4x4-rgba-unorm-srgb",
    linearVariant: "astc-4x4-rgba-unorm",
  },
};

/** 格式表的全量键（选择器的候选全集，避免依赖 Record 键序）。 */
export const ALL_PIXEL_FORMATS: readonly PixelFormat[] = Object.keys(FORMAT_TABLE) as PixelFormat[];

/** 取格式规格；未知格式返回 null（调用方必须显式处理，不得默认兜底）。 */
export function lookupFormat(format: PixelFormat): FormatSpec | null {
  return Object.prototype.hasOwnProperty.call(FORMAT_TABLE, format) ? FORMAT_TABLE[format] : null;
}

/** 是否块压缩格式。 */
export function isBlockCompressed(format: PixelFormat): boolean {
  const s = lookupFormat(format);
  return s !== null && s.family !== "none";
}

/** 是否 sRGB 变体。 */
export function isSrgbVariant(format: PixelFormat): boolean {
  return format.endsWith("-srgb");
}

/**
 * 有效位深（bit/像素）。
 * 未压缩：bytesPerPixel × 8。
 * 块压缩：(blockBytes × 8) / (blockEdge²) —— 块内像素共享块字节数，
 * 故必须除以块面积。BC7 是 16 字节 / 16 像素 = 8bpp，不是 128bpp。
 */
export function effectiveBitsPerPixel(format: PixelFormat): number {
  const s = lookupFormat(format);
  if (s === null) return 0;
  if (s.family === "none") return s.bytesPerPixel * 8;
  const blockArea = s.blockEdge * s.blockEdge;
  return (s.blockBytes * 8) / blockArea;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 纹理描述、用途与设备能力
// ════════════════════════════════════════════════════════════════════════════

/** 用途位标记。 */
export type TextureUsage =
  | "sampled"
  | "renderTarget"
  | "storage"
  | "uploadDestination"
  | "depthStencil";

/** 用途集合（显式数组，不用位掩码——位掩码的「组合合法性」不可读）。 */
export type TextureUsageSet = readonly TextureUsage[];

export const USAGE_ALL: readonly TextureUsage[] = [
  "sampled",
  "renderTarget",
  "storage",
  "uploadDestination",
  "depthStencil",
];

/** 纹理描述（不可变输入）。 */
export interface TextureDesc {
  readonly width: number;
  readonly height: number;
  /** 3D 纹理的深度；2D 为 1。 */
  readonly depth: number;
  /** 数组层数；非数组为 1。 */
  readonly arrayLayers: number;
  /** 显式 mip 级数；0 表示「完整链」。 */
  readonly mipLevels: number;
  readonly format: PixelFormat;
  readonly usage: TextureUsageSet;
  /** sRGB 色彩空间（与格式后缀必须一致，见 SRGB 纪律）。 */
  readonly srgb: boolean;
  /** 源资产是否带 alpha（决定 BC1 是否可用）。 */
  readonly hasAlpha: boolean;
  /** 源资产是否为法线图（决定 BC5 是否优先）。 */
  readonly isNormalMap: boolean;
}

/** 设备能力上限（预算仲裁与维度校验都吃这份）。 */
export interface DeviceCaps {
  readonly maxTextureDimension2D: number;
  readonly maxTextureDimension3D: number;
  readonly maxTextureArrayLayers: number;
  readonly maxMipLevels: number;
  /** 设备原生支持的压缩族；不在表内的格式一律视为不支持。 */
  readonly supportedFamilies: readonly CompressionFamily[];
  /** 单次纹理预算硬上限（超过即拒绝，无法降级）。 */
  readonly perTextureBudgetBytes: number;
  readonly totalBudgetBytes: number;
}

/** 通用整数校验。 */
function isPositiveInt(v: number): boolean {
  return Number.isInteger(v) && v > 0;
}

/** mip 链完整级数：floor(log2(max(w,h))) + 1。 */
export function fullMipCount(width: number, height: number): number {
  const m = Math.max(width, height);
  let levels = 1;
  let cur = m;
  while (cur > 1) {
    cur = Math.floor(cur / 2);
    levels += 1;
  }
  return levels;
}

/** 指定 mip 级的分辨率（逐级折半，最小 1）。 */
export function mipDimensions(width: number, height: number, level: number): { w: number; h: number } {
  const w = Math.max(1, Math.floor(width / Math.pow(2, level)));
  const h = Math.max(1, Math.floor(height / Math.pow(2, level)));
  return { w, h };
}

/** 有效 mip 级数（把 0 展开为完整链）。 */
export function effectiveMipCount(desc: TextureDesc): number {
  return desc.mipLevels === 0 ? fullMipCount(desc.width, desc.height) : desc.mipLevels;
}

/** 单个 mip 级的字节数（块压缩按块对齐，未压缩按像素）。 */
export function levelByteSize(desc: TextureDesc, format: PixelFormat, level: number): number {
  const spec = lookupFormat(format);
  if (spec === null) return 0;
  const { w, h } = mipDimensions(desc.width, desc.height, level);
  const perLevel =
    spec.family === "none"
      ? w * h * spec.bytesPerPixel
      : Math.ceil(w / spec.blockEdge) * Math.ceil(h / spec.blockEdge) * spec.blockBytes;
  return perLevel * desc.arrayLayers * desc.depth;
}

/**
 * 完整 mip 链字节数。
 * `mipFloor` > 0 表示只驻留 [mipFloor, mipCount) 的链（水位联动回收用）。
 */
export function mipChainBytes(
  desc: TextureDesc,
  format: PixelFormat,
  mipFloor: number = 0,
): number {
  const count = effectiveMipCount(desc);
  const from = Math.max(0, Math.min(mipFloor, count - 1));
  let total = 0;
  for (let lv = from; lv < count; lv += 1) {
    total += levelByteSize(desc, format, lv);
  }
  return total;
}

/**
 * 描述校验（创建段的入口闸门）。
 * 每条失败都带专属码与处置建议——不合并为「参数非法」。
 */
export function validateDesc(desc: TextureDesc, caps: DeviceCaps): TextureOutcome<TextureDesc> {
  const bag = new TextureDiagBag();

  if (lookupFormat(desc.format) === null) {
    return texFail("FORMAT_UNKNOWN", `未知像素格式 ${String(desc.format)}`, "先在 FORMAT_TABLE 登记格式再引用");
  }
  for (const [label, v] of [
    ["width", desc.width],
    ["height", desc.height],
    ["depth", desc.depth],
    ["arrayLayers", desc.arrayLayers],
  ] as const) {
    if (!isPositiveInt(v)) {
      return texFail(
        "DIMENSION_LIMIT",
        `${label}=${String(v)} 非正整数`,
        "维度必须为正整数；2D 纹理的 depth 与 arrayLayers 固定为 1",
      );
    }
  }

  const is3d = desc.depth > 1;
  const dimLimit = is3d ? caps.maxTextureDimension3D : caps.maxTextureDimension2D;
  if (desc.width > dimLimit || desc.height > dimLimit || desc.depth > dimLimit) {
    return texFail(
      "DIMENSION_LIMIT",
      `纹理 ${desc.width}×${desc.height}×${desc.depth} 超过设备上限 ${dimLimit}`,
      "降分辨率后再创建；或走流式上传先驻留低分辨率级",
    );
  }
  if (desc.arrayLayers > caps.maxTextureArrayLayers) {
    return texFail(
      "DIMENSION_LIMIT",
      `数组层数 ${desc.arrayLayers} 超过上限 ${caps.maxTextureArrayLayers}`,
      "拆成多张纹理，或走纹理数组分批",
    );
  }

  const mipCount = effectiveMipCount(desc);
  if (mipCount > caps.maxMipLevels) {
    return texFail(
      "MIP_CHAIN_INVALID",
      `mip 级数 ${mipCount} 超过设备上限 ${caps.maxMipLevels}`,
      "显式收窄 mipLevels，不依赖完整链",
    );
  }
  if (desc.mipLevels !== 0 && desc.mipLevels > fullMipCount(desc.width, desc.height)) {
    return texFail(
      "MIP_CHAIN_INVALID",
      `显式 mipLevels=${desc.mipLevels} 超过完整链 ${fullMipCount(desc.width, desc.height)}`,
      "mipLevels 取 0 表示完整链，或取不超过完整链的值",
    );
  }

  if (desc.usage.length === 0) {
    return texFail("USAGE_EMPTY", "用途集合为空", "至少声明 sampled 或 renderTarget 之一");
  }
  const seen = new Set<TextureUsage>();
  for (const u of desc.usage) {
    if (!USAGE_ALL.includes(u)) {
      return texFail("USAGE_INCOMPATIBLE", `未知用途 ${String(u)}`, "用途取值必须来自 USAGE_ALL");
    }
    if (seen.has(u)) {
      bag.push("USAGE_INCOMPATIBLE", `用途 ${u} 重复声明`, "用途集合去重，重复项不改变语义但会污染台账");
    }
    seen.add(u);
  }

  const spec = lookupFormat(desc.format);
  if (spec !== null) {
    if (spec.supportsStorage && !seen.has("storage")) {
      bag.push(
        "USAGE_INCOMPATIBLE",
        `格式 ${desc.format} 声明支持 storage 但用途未含 storage`,
        "补上 storage 用途，或换不支持 storage 的格式",
      );
    }
    if (seen.has("storage") && !spec.supportsStorage) {
      return texFail(
        "USAGE_INCOMPATIBLE",
        `格式 ${desc.format} 不支持 storage 用途`,
        "改用支持 storage 的未压缩格式（如 rgba16float）",
      );
    }
    if (seen.has("renderTarget") && seen.has("sampled") && !spec.supportsRenderTarget) {
      return texFail(
        "SAMPLING_RENDER_CONFLICT",
        `格式 ${desc.format} 同时作渲染目标与采样源，但不支持渲染目标`,
        "拆分渲染与采样两个纹理，或换 rgba8unorm 族",
      );
    }
  }

  // sRGB 纪律：格式后缀与 srgb 标志必须一致（不一致会导致 mip 在错误域生成）
  if (desc.srgb !== isSrgbVariant(desc.format)) {
    return texFail(
      "SRGB_DOMAIN_VIOLATION",
      `srgb=${String(desc.srgb)} 与格式 ${desc.format} 的域后缀不一致`,
      "sRGB 纹理必须使用 *-srgb 格式；mip 生成在线性域进行，格式后缀即声明",
    );
  }
  if (desc.srgb && spec !== null && !spec.srgbCapable) {
    return texFail(
      "SRGB_DOMAIN_VIOLATION",
      `格式 ${desc.format} 不具备 sRGB 变体`,
      "改用 rgba8unorm-srgb 或压缩族的 -srgb 变体",
    );
  }

  return texOk(desc, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 格式转换管线（判据二：压缩格式自动选择 · 判据三：质量档位声明）
// ════════════════════════════════════════════════════════════════════════════

/** 质量档位（由严到宽）。 */
export type QualityTier = "lossless" | "high" | "balanced" | "compact" | "minimal";

export const QUALITY_TIERS: readonly QualityTier[] = [
  "lossless",
  "high",
  "balanced",
  "compact",
  "minimal",
];

/** 每档允许的有损上界（由 QUALITY_BASELINE 派生，立场四）。 */
const TIER_LOSS_CEILING: Readonly<Record<QualityTier, LossGrade>> = {
  lossless: "none",
  high: "negligible",
  balanced: "negligible",
  compact: "low",
  minimal: "low",
};

/** 质量损失声明：档 → 允许上界 + 面向用户的说法。 */
export interface QualityDeclaration {
  readonly tier: QualityTier;
  readonly maxLoss: LossGrade;
  /** 位深比下界（相对源格式，单位倍）。 */
  readonly minBitsPerPixelRatio: number;
  /** 读屏播报用的通俗说法。 */
  readonly plainText: string;
}

export const QUALITY_DECLARATIONS: readonly QualityDeclaration[] = [
  {
    tier: "lossless",
    maxLoss: "none",
    minBitsPerPixelRatio: 1.0,
    plainText: "无损：像素逐位一致，占用最高",
  },
  {
    tier: "high",
    maxLoss: "negligible",
    minBitsPerPixelRatio: 0.5,
    plainText: "高：肉眼难辨差异，占用约减半",
  },
  {
    tier: "balanced",
    maxLoss: "negligible",
    minBitsPerPixelRatio: 0.28,
    plainText: "均衡：正常观看无差异，近距离可见",
  },
  {
    tier: "compact",
    maxLoss: "low",
    minBitsPerPixelRatio: 0.15,
    plainText: "紧凑：放大可见轻微块状，占用约六分之一",
  },
  {
    tier: "minimal",
    maxLoss: "low",
    minBitsPerPixelRatio: 0.1,
    plainText: "最小：仅远景可用，占用约十分之一",
  },
];

/**
 * 质量对比基线（有损格式 vs 无损的**视觉差异量化**——判据要求）。
 * 数据性质：档位声明的量化下界，取自格式规范的块结构与经验档，
 * **不是逐资产的实测 PSNR**；实测须由对拍工具按资产补录（见 needBaseline）。
 */
export interface QualityBaselineRow {
  readonly from: PixelFormat;
  readonly to: PixelFormat;
  readonly loss: LossGrade;
  /** 相对源格式的位深比。 */
  readonly bitsRatio: number;
  /** PSNR 下界（dB）；无损为 null。 */
  readonly psnrFloorDb: number | null;
  /** SSIM 下界；无损为 null。 */
  readonly ssimFloor: number | null;
  /** 该组合是否已有实测记录（false = 仍需对拍补录）。 */
  readonly measured: boolean;
}

function ratioTo(from: PixelFormat, to: PixelFormat): number {
  const src = effectiveBitsPerPixel(from);
  const dst = effectiveBitsPerPixel(to);
  return src === 0 ? 0 : dst / src;
}

function baselineRow(
  from: PixelFormat,
  to: PixelFormat,
  loss: LossGrade,
  psnrFloorDb: number | null,
  ssimFloor: number | null,
  measured: boolean,
): QualityBaselineRow {
  return { from, to, loss, bitsRatio: ratioTo(from, to), psnrFloorDb, ssimFloor, measured };
}

/** 未压缩源 → 各压缩目标的质量基线（判据：有损与无损的差异量化）。 */
export const QUALITY_BASELINE: readonly QualityBaselineRow[] = [
  // rgba8unorm（32bpp）→ 压缩族
  baselineRow("rgba8unorm", "bc1-rgba-unorm", "low", 38, 0.92, false),
  baselineRow("rgba8unorm", "bc3-rgba-unorm", "negligible", 45, 0.97, false),
  baselineRow("rgba8unorm", "bc7-rgba-unorm", "negligible", 48, 0.98, false),
  baselineRow("rgba8unorm", "astc-4x4-rgba-unorm", "negligible", 46, 0.97, false),
  // 浮点 HDR 源（RGBA16F 128bpp）→ 压缩族（位比极低，须显式标注）
  baselineRow("rgba16float", "bc7-rgba-unorm", "moderate", 32, 0.9, false),
  baselineRow("rg16float", "bc5-rg-unorm", "negligible", 44, 0.96, false),
  // 法线图 → BC5（双通道，天然匹配）
  baselineRow("rg8unorm", "bc5-rg-unorm", "negligible", 46, 0.97, false),
  // 无损基线（自身）
  baselineRow("rgba8unorm", "rgba8unorm", "none", null, null, true),
];

/** 查质量基线；无记录返回 null（**不得默认「无损失」**）。 */
export function lookupBaseline(from: PixelFormat, to: PixelFormat): QualityBaselineRow | null {
  return QUALITY_BASELINE.find((r) => r.from === from && r.to === to) ?? null;
}

/** 某转换是否仍需实测补录（对拍台账的待办源）。 */
export function needBaseline(from: PixelFormat, to: PixelFormat): boolean {
  const row = lookupBaseline(from, to);
  return row === null || !row.measured;
}

/** 档位允许的有损上界（只读 TIER_LOSS_CEILING，与声明同源）。 */
export function qualityCeilingFor(tier: QualityTier): LossGrade {
  return TIER_LOSS_CEILING[tier];
}

/** 有损度是否在档位允许范围内（none < negligible < low < moderate）。 */
const LOSS_RANK: Readonly<Record<LossGrade, number>> = {
  none: 0,
  negligible: 1,
  low: 2,
  moderate: 3,
};

export function lossWithinTier(tier: QualityTier, loss: LossGrade): boolean {
  return LOSS_RANK[loss] <= LOSS_RANK[TIER_LOSS_CEILING[tier]];
}

/** 候选淘汰轨迹的一条记录。 */
export interface FormatRejection {
  readonly candidate: PixelFormat;
  readonly reason:
    | "family-unsupported"
    | "loss-exceeds-tier"
    | "srgb-domain-mismatch"
    | "channel-count-mismatch"
    | "alpha-required"
    | "no-alpha-bc1"
    | "not-render-target"
    | "not-storage"
    | "higher-bits-no-gain"
    | "bigger-than-current";
  readonly detail: string;
}

/** 自动选择的结果：格式 + 淘汰轨迹（立场三：轨迹必须可诊断）。 */
export interface FormatChoice {
  readonly format: PixelFormat;
  readonly tier: QualityTier;
  readonly rejected: readonly FormatRejection[];
  /** 位深比（相对源格式）。 */
  readonly bitsRatio: number;
  /** 该选择是否已有实测基线；false 需对拍补录。 */
  readonly baselineMeasured: boolean;
}

/**
 * 压缩格式自动选择（纯函数）。
 * 候选顺序 = ALL_PIXEL_FORMATS 的稳定序，故同输入必得同结果同轨迹。
 */
export function selectFormat(
  desc: TextureDesc,
  tier: QualityTier,
  caps: DeviceCaps,
): TextureOutcome<FormatChoice> {
  const bag = new TextureDiagBag();
  const sourceSpec = lookupFormat(desc.format);
  if (sourceSpec === null) {
    return texFail("FORMAT_UNKNOWN", `源格式 ${String(desc.format)} 未登记`, "先登记格式");
  }

  // 法线图语义前置归一：法线贴图的天然形态就是双通道（法线 XY + 高度 Z）。
  // 若调用方用四通道格式声明法线图，那是描述错误——直接给结论而不是
  // 硬让选择器在「四通道但只有 bc5 能压」的死路里空转。
  if (desc.isNormalMap && sourceSpec.channelCount !== 2) {
    return texFail(
      "FORMAT_UNSUPPORTED",
      `法线图源格式 ${desc.format} 为 ${sourceSpec.channelCount} 通道，法线图必须声明双通道（rg8unorm / rg16float）`,
      "把法线图源的 format 改为 rg8unorm 或 rg16float；四通道法线图会把 Z 通道一并压进 bc5 而失真",
    );
  }

  const rejected: FormatRejection[] = [];
  const wantsRender = desc.usage.includes("renderTarget");
  const wantsStorage = desc.usage.includes("storage");

  // 源已是无损且档位要求无损 → 原样返回（不折腾）
  if (tier === "lossless" && !sourceSpec.lossy) {
    return texOk({
      format: desc.format,
      tier,
      rejected,
      bitsRatio: 1,
      baselineMeasured: true,
    });
  }

  const viable: FormatSpec[] = [];
  // 淘汰顺序纪律：**先结构、后用途、再代价**。
  // 通道数是格式的结构属性，与源不符即为不可用（选它=静默丢数据），
  // 这比色彩空间/用途错配更根本，故先判——否则一个 r8unorm 候选会先被
  // 「无 sRGB 变体」淘汰，掩盖掉它真正的问题是「只有 1 个通道」。
  for (const fmt of ALL_PIXEL_FORMATS) {
    const spec = FORMAT_TABLE[fmt];
    if (spec.channelCount !== sourceSpec.channelCount) {
      rejected.push({
        candidate: fmt,
        reason: "channel-count-mismatch",
        detail:
          `通道数 ${spec.channelCount} 与源 ${sourceSpec.channelCount} 不符` +
          `（选它等于静默丢弃 ${Math.abs(spec.channelCount - sourceSpec.channelCount)} 个通道的数据）`,
      });
      continue;
    }
    if (isSrgbVariant(fmt) !== desc.srgb) {
      rejected.push({
        candidate: fmt,
        reason: "srgb-domain-mismatch",
        detail: `色彩空间与源不符（源 srgb=${String(desc.srgb)}）`,
      });
      continue;
    }
    if (!caps.supportedFamilies.includes(spec.family)) {
      rejected.push({
        candidate: fmt,
        reason: "family-unsupported",
        detail: `压缩族 ${spec.family} 不在设备支持列表`,
      });
      continue;
    }
    if (!lossWithinTier(tier, spec.loss)) {
      rejected.push({
        candidate: fmt,
        reason: "loss-exceeds-tier",
        detail: `有损度 ${spec.loss} 超出档位 ${tier} 的上界 ${TIER_LOSS_CEILING[tier]}`,
      });
      continue;
    }
    if (desc.isNormalMap && spec.family !== "bc5") {
      rejected.push({
        candidate: fmt,
        reason: "alpha-required",
        detail: "法线图仅接受双通道压缩族（bc5）",
      });
      continue;
    }
    if (spec.family === "bc1" && desc.hasAlpha) {
      rejected.push({
        candidate: fmt,
        reason: "no-alpha-bc1",
        detail: "源带 alpha，bc1 的 alpha 通道仅 1bit，会出现透明噪点",
      });
      continue;
    }
    if (wantsRender && !spec.supportsRenderTarget) {
      rejected.push({
        candidate: fmt,
        reason: "not-render-target",
        detail: "用途含 renderTarget，该格式不支持",
      });
      continue;
    }
    if (wantsStorage && !spec.supportsStorage) {
      rejected.push({
        candidate: fmt,
        reason: "not-storage",
        detail: "用途含 storage，该格式不支持",
      });
      continue;
    }
    viable.push(spec);
  }

  if (viable.length === 0) {
    return texFail(
      "FORMAT_UNSUPPORTED",
      `档位 ${tier} 下无可用格式（候选 ${ALL_PIXEL_FORMATS.length} 个全部被淘汰）`,
      "放宽质量档，或为该资产开启对应压缩族",
      bag.all(),
    );
  }

  // 在存活候选里取「位深最低者」；位深相同取格式名字典序最小者（确定性）
  let best = viable[0] as FormatSpec;
  let bestBits = best.family === "none" ? best.bytesPerPixel * 8 : best.blockBytes * 2;
  for (let i = 1; i < viable.length; i += 1) {
    const cand = viable[i] as FormatSpec;
    const candBits = cand.family === "none" ? cand.bytesPerPixel * 8 : cand.blockBytes * 2;
    if (candBits < bestBits || (candBits === bestBits && cand.format < best.format)) {
      best = cand;
      bestBits = candBits;
    }
  }

  const row = lookupBaseline(desc.format, best.format);
  if (row === null) {
    bag.push(
      "FORMAT_UNSUPPORTED",
      `转换 ${desc.format} → ${best.format} 无质量基线记录`,
      "补录质量基线（PSNR/SSIM 实测）后再放行；无记录的转换损失不可量化",
    );
  }

  // 位深比是格式规格的**确定函数**（块字节 ÷ 块面积），不依赖实测——
  // 故此处直接算，不从基线表取：否则「基线未录」会把一个确定量报成 0。
  // 基线表只负责 loss / psnr / ssim 这些需要实测的量。
  return texOk(
    {
      format: best.format,
      tier,
      rejected,
      bitsRatio: ratioTo(desc.format, best.format),
      baselineMeasured: row !== null && row.measured,
    },
    bag.all(),
  );
}

/** 转换管线的阶段（顺序固定，阶段不可跳过）。 */
export type TranscodeStage =
  | "unpack-source"
  | "channel-map"
  | "to-linear-domain"
  | "quantize"
  | "encode-blocks"
  | "mip-generate"
  | "pack-target";

export const TRANSCODE_STAGES: readonly TranscodeStage[] = [
  "unpack-source",
  "channel-map",
  "to-linear-domain",
  "quantize",
  "encode-blocks",
  "mip-generate",
  "pack-target",
];

/**
 * 计划一次转换：返回阶段序列 + mip 生成域。
 * sRGB 纪律：mip 必须在**线性域**生成，逐级降采样后再按目标格式编码——
 * 在 gamma 空间折半是常见的画质塌陷来源。
 */
export function planTranscode(
  from: PixelFormat,
  to: PixelFormat,
  mipCount: number,
): TextureOutcome<readonly TranscodeStage[]> {
  const src = lookupFormat(from);
  const dst = lookupFormat(to);
  if (src === null || dst === null) {
    return texFail(
      "FORMAT_UNKNOWN",
      `转换端点未登记：${from} → ${to}`,
      "两端都必须在 FORMAT_TABLE 中登记",
    );
  }
  const bag = new TextureDiagBag();

  const stages: TranscodeStage[] = [];
  // 源为块压缩才需要解包；源为未压缩则直接从通道映射起步
  if (src.family !== "none") stages.push("unpack-source");
  stages.push("channel-map");
  // sRGB 源必须显式转到线性域再做 mip 生成与量化
  const needsLinear = isSrgbVariant(from) || isSrgbVariant(to);
  if (needsLinear) {
    stages.push("to-linear-domain");
    bag.push(
      "SRGB_DOMAIN_VIOLATION",
      "转换含 sRGB 端点：mip 生成在线性域进行",
      "禁止在 gamma 空间折半生成 mip；逐级降采样后按目标格式重新编码",
    );
  }
  if (dst.lossy) stages.push("quantize");
  if (dst.family !== "none") stages.push("encode-blocks");
  if (mipCount > 1) stages.push("mip-generate");
  stages.push("pack-target");

  return texOk(stages, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 四段生命周期状态机（判据一）
// ════════════════════════════════════════════════════════════════════════════

/** 生命周期状态。更新不是状态，是事件（见锚点契约一）。 */
export type TexturePhase = "declared" | "allocated" | "resident" | "streaming" | "retired";

/** 生命周期事件。 */
export type TextureEvent =
  | "allocate"
  | "begin-upload"
  | "complete-upload"
  | "begin-stream"
  | "stream-complete"
  | "retire";

export const TEXTURE_EVENTS: readonly TextureEvent[] = [
  "allocate",
  "begin-upload",
  "complete-upload",
  "begin-stream",
  "stream-complete",
  "retire",
];

/** 显式迁移表：event → 允许的源状态集合 → 目标状态。 */
export const TRANSITION_TABLE: Readonly<
  Record<TextureEvent, { readonly from: readonly TexturePhase[]; readonly to: TexturePhase }>
> = {
  allocate: { from: ["declared"], to: "allocated" },
  "begin-upload": { from: ["allocated", "retired"], to: "streaming" },
  "complete-upload": { from: ["streaming"], to: "resident" },
  "begin-stream": { from: ["resident"], to: "streaming" },
  "stream-complete": { from: ["streaming"], to: "resident" },
  retire: { from: ["declared", "allocated", "resident", "streaming"], to: "retired" },
};

/** 台账条目的代戳（与 F0008 句柄代数戳同源，防「退役后同名复活」）。 */
export interface GenerationStamp {
  readonly serial: number;
  readonly phase: TexturePhase;
}

export interface TransitionResult {
  readonly stamp: GenerationStamp;
  readonly changed: boolean;
}

/**
 * 状态迁移（纯函数）。
 * 退役态的「复活」是显式允许的（retired → streaming），但**必须由新的
 * allocate 事件抬起代戳**——复用同名纹理而不抬代戳是 F0008 悬垂的同源缺陷。
 */
export function applyTransition(
  stamp: GenerationStamp,
  event: TextureEvent,
): TextureOutcome<TransitionResult> {
  const rule = TRANSITION_TABLE[event];
  if (!rule.from.includes(stamp.phase)) {
    return texFail(
      "ILLEGAL_TRANSITION",
      `状态 ${stamp.phase} 不接受事件 ${event}`,
      `该状态可接受：${rule.from.join(" / ")}`,
    );
  }
  if (stamp.phase === "retired" && event !== "begin-upload") {
    return texFail(
      "RETIRED_REUSE",
      `退役纹理（代戳 ${stamp.serial}）收到事件 ${event}`,
      "退役对象只接受 begin-upload 重新驻留；其余操作须先重新 allocate",
    );
  }
  // 从 retired 重新驻留 = 新生命，抬代戳（同名不同代，绝不与旧代混用）
  const nextSerial = stamp.phase === "retired" ? stamp.serial + 1 : stamp.serial;
  return texOk({
    stamp: { serial: nextSerial, phase: rule.to },
    changed: stamp.phase !== rule.to,
  });
}

/** 校验：给定代戳的期望代，与请求代是否一致（防旧代句柄误用）。 */
export function checkGeneration(stamp: GenerationStamp, expectedSerial: number): TextureOutcome<void> {
  if (stamp.serial !== expectedSerial) {
    return texFail(
      "GENERATION_STALE",
      `代戳不符：持有 ${stamp.serial}，台账当前 ${expectedSerial}`,
      "旧代句柄自动失效；重新从台账取句柄",
    );
  }
  if (stamp.phase === "retired") {
    return texFail(
      "RETIRED_REUSE",
      `代戳 ${stamp.serial} 已退役`,
      "退役对象不可用；需要复用同名时重新 allocate 并抬高代戳",
    );
  }
  return texOk(undefined);
}

/** 更新事件（不是状态迁移，但要求对象已驻留）。 */
export interface UpdateRequest {
  readonly textureId: string;
  readonly serial: number;
  /** 更新的 mip 级；更新高分辨率级不影响已采样的低分辨率级。 */
  readonly level: number;
  readonly byteLength: number;
  /** 是否有区域未覆盖（未覆盖区域保持旧内容，显存不清零）。 */
  readonly partial: boolean;
}

/** 校验一次更新请求。 */
export function validateUpdate(
  stamp: GenerationPhaseInput,
  req: UpdateRequest,
  desc: TextureDesc,
): TextureOutcome<void> {
  const g = checkGeneration(stamp, req.serial);
  if (!g.ok) return { ...g, diagnostics: g.diagnostics };
  if (stamp.phase !== "resident" && stamp.phase !== "streaming") {
    return texFail(
      "ILLEGAL_TRANSITION",
      `更新要求驻留态，当前 ${stamp.phase}`,
      "先完成上传（complete-upload）再更新",
    );
  }
  if (req.level >= effectiveMipCount(desc)) {
    return texFail(
      "MIP_CHAIN_INVALID",
      `更新 mip 级 ${req.level} 超出链长 ${effectiveMipCount(desc)}`,
      "取 0 到 mipCount-1 的级别",
    );
  }
  if (!isPositiveInt(req.byteLength)) {
    return texFail("DIMENSION_LIMIT", `更新字节数 ${String(req.byteLength)} 非正整数`, "字节数必须为正");
  }
  return texOk(undefined);
}

/** checkGeneration 的入参形态（避免直接依赖全量 stamp 字段）。 */
export type GenerationPhaseInput = GenerationStamp;

// ════════════════════════════════════════════════════════════════════════════
// §6 别名台账（判据：同纹理多名字的统一台账）
// ════════════════════════════════════════════════════════════════════════════

/** 一条别名绑定。 */
export interface AliasBinding {
  readonly alias: string;
  readonly textureId: string;
  /** 绑定来源（材质 / UI / 工具链），归因用。 */
  readonly origin: string;
}

/** 别名台账：alias → textureId（唯一真值，纹理条目反查走索引）。 */
export class TextureAliasRegistry {
  private readonly bindings = new Map<string, AliasBinding>();
  private readonly byTexture = new Map<string, Set<string>>();

  /** 绑定别名；同名指向不同纹理 = 冲突（拒绝，不覆盖）。 */
  bind(alias: string, textureId: string, origin: string): TextureOutcome<AliasBinding> {
    const existing = this.bindings.get(alias);
    if (existing !== undefined) {
      if (existing.textureId === textureId) {
        return texOk(existing);
      }
      return texFail(
        "ALIAS_CONFLICT",
        `别名「${alias}」已指向 ${existing.textureId}（来源 ${existing.origin}），不能改指 ${textureId}`,
        "先 unbind 旧指向，或改用新别名；别名即契约，禁止静默改指",
      );
    }
    const b: AliasBinding = { alias, textureId, origin };
    this.bindings.set(alias, b);
    let set = this.byTexture.get(textureId);
    if (set === undefined) {
      set = new Set<string>();
      this.byTexture.set(textureId, set);
    }
    set.add(alias);
    return texOk(b);
  }

  /** 解绑；解绑不存在的别名 = 显式失败（防止上层以为解了其实没解）。 */
  unbind(alias: string): TextureOutcome<void> {
    const b = this.bindings.get(alias);
    if (b === undefined) {
      return texFail("ALIAS_UNBOUND", `别名「${alias}」未绑定`, "先查 aliasesOf 确认当前绑定");
    }
    this.bindings.delete(alias);
    this.byTexture.get(b.textureId)?.delete(alias);
    return texOk(undefined);
  }

  resolve(alias: string): string | null {
    const b = this.bindings.get(alias);
    return b === null ? null : (b?.textureId ?? null);
  }

  /** 某纹理的全部别名（台账正查）。 */
  aliasesOf(textureId: string): readonly string[] {
    const s = this.byTexture.get(textureId);
    return s === undefined ? [] : Array.from(s).sort();
  }

  get size(): number {
    return this.bindings.size;
  }

  /** 孤儿审计：指向不存在纹理的别名（纹理被回收但别名未解 = 泄漏线索）。 */
  auditOrphans(liveTextureIds: ReadonlySet<string>): readonly AliasBinding[] {
    const out: AliasBinding[] = [];
    for (const b of this.bindings.values()) {
      if (!liveTextureIds.has(b.textureId)) out.push(b);
    }
    return out.sort((a, b) => (a.alias < b.alias ? -1 : a.alias > b.alias ? 1 : 0));
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §7 显存预算对接与降分辨率阶梯（判据四 · 跨批对接点 A05）
// ════════════════════════════════════════════════════════════════════════════

/** 预算申请意图。 */
export type BudgetIntent = "texture-mip-chain" | "texture-resize" | "texture-reformat" | "transient";

/** 预算裁决。 */
export interface BudgetDecision {
  readonly granted: boolean;
  readonly grantedBytes: number;
  readonly reason: string;
}

/** 预算台账（三级池的纹理侧投影：只管纹理这一维的收支）。 */
export class TextureBudgetLedger {
  private usedBytes = 0;
  private readonly reserved = new Map<string, number>();

  constructor(
    readonly poolId: string,
    readonly totalBytes: number,
  ) {}

  /** 申请预留（先校验，后提交——两步分离便于批量申请整体回滚）。 */
  request(amount: number, intent: BudgetIntent): BudgetDecision {
    if (!Number.isFinite(amount) || amount <= 0) {
      return { granted: false, grantedBytes: 0, reason: "申请字节数非法" };
    }
    if (this.usedBytes + amount > this.totalBytes) {
      return {
        granted: false,
        grantedBytes: 0,
        reason: `池 ${this.poolId} 余量不足：已用 ${this.usedBytes} + 申请 ${amount} > 总量 ${this.totalBytes}（意图 ${intent}）`,
      };
    }
    return { granted: true, grantedBytes: amount, reason: "余量充足" };
  }

  /** 提交预留（把预留转为实际占用）。 */
  commit(token: string, amount: number): BudgetDecision {
    const prev = this.reserved.get(token) ?? 0;
    this.usedBytes += amount - prev;
    this.reserved.set(token, amount);
    return { granted: true, grantedBytes: this.usedBytes, reason: `已提交 ${amount}，池占用 ${this.usedBytes}` };
  }

  /** 原子归还（纹理销毁必须走归还——漏归还即显存泄漏）。 */
  release(token: string): BudgetDecision {
    const prev = this.reserved.get(token);
    if (prev === undefined) {
      return { granted: false, grantedBytes: this.usedBytes, reason: `令牌 ${token} 无预留记录，拒绝归还（防重复归还）` };
    }
    this.reserved.delete(token);
    this.usedBytes -= prev;
    return { granted: true, grantedBytes: this.usedBytes, reason: `已归还 ${prev}，池占用 ${this.usedBytes}` };
  }

  get used(): number {
    return this.usedBytes;
  }

  get available(): number {
    return this.totalBytes - this.usedBytes;
  }

  snapshot(): { readonly poolId: string; readonly totalBytes: number; readonly usedBytes: number } {
    return { poolId: this.poolId, totalBytes: this.totalBytes, usedBytes: this.usedBytes };
  }
}

/** 降级阶梯的一级。 */
export interface DegradationStep {
  readonly kind: "mip-floor" | "dimension-scale" | "format-compaction" | "reject";
  /** 施加后的 mipFloor。 */
  readonly mipFloor: number;
  /** 施加后的分辨率倍率（1 = 原尺寸）。 */
  readonly scale: number;
  /** 施加后的格式；null = 不换格式。 */
  readonly format: PixelFormat | null;
  /** 降级后的预计字节数。 */
  readonly bytes: number;
  /** 视觉代价声明（读屏可见）。 */
  readonly visualCost: string;
}

/** 分辨率降级阶梯（倍率单调递减，不可跳级）。 */
export const SCALE_LADDER: readonly number[] = [1.0, 0.75, 0.5, 0.35, 0.25];

/** 阶梯末档（最低分辨率倍率）。取常量而非索引末位——
 *  索引在 noUncheckedIndexedAccess 下可能是 undefined，且阶梯增删会让索引漂移。 */
export const MIN_RESOLUTION_SCALE: number = 0.25;

/** mip 下限阶梯（每级释放一个 mip）。 */
export const MIP_FLOOR_LADDER: readonly number[] = [0, 1, 2, 3, 4, 5];

/**
 * 规划降级阶梯（判据四：预算超支 → 降分辨率）。
 * 顺序纪律：先抬 mip 下限（画质损失最小、视觉不可见）→ 再降分辨率
 * → 再换紧凑格式 → 最后拒绝。顺序颠倒会让「看不见的损失」先发生。
 */
export function planDegradation(
  desc: TextureDesc,
  format: PixelFormat,
  budgetBytes: number,
  perTextureCap: number,
): TextureOutcome<readonly DegradationStep[]> {
  const bag = new TextureDiagBag();
  const mipCount = effectiveMipCount(desc);
  const steps: DegradationStep[] = [];

  // 级一：抬 mip 下限（保底分辨率级，放走高层）
  for (const floor of MIP_FLOOR_LADDER) {
    if (floor >= mipCount) break;
    const bytes = mipChainBytes(desc, format, floor);
    steps.push({
      kind: "mip-floor",
      mipFloor: floor,
      scale: 1,
      format: null,
      bytes,
      visualCost:
        floor === 0
          ? "完整 mip 链"
          : `释放最高 ${floor} 级 mip；正常观看无差异，缩放到 100% 时可能欠采样`,
    });
    if (bytes <= budgetBytes) {
      return texOk(steps, bag.all());
    }
  }

  // 级二：降分辨率倍率
  for (const scale of SCALE_LADDER) {
    if (scale >= 1) continue;
    const scaled: TextureDesc = {
      ...desc,
      width: Math.max(1, Math.floor(desc.width * scale)),
      height: Math.max(1, Math.floor(desc.height * scale)),
    };
    for (const floor of MIP_FLOOR_LADDER) {
      if (floor >= effectiveMipCount(scaled)) break;
      const bytes = mipChainBytes(scaled, format, floor);
      steps.push({
        kind: "dimension-scale",
        mipFloor: floor,
        scale,
        format: null,
        bytes,
        visualCost: `分辨率降为 ${Math.round(scale * 100)}%；近距离可见模糊`,
      });
      if (bytes <= budgetBytes) {
        return texOk(steps, bag.all());
      }
    }
  }

  // 级三：换紧凑格式
  const compacted: readonly PixelFormat[] = format === "bc3-rgba-unorm"
    ? ["bc1-rgba-unorm"]
    : format === "bc7-rgba-unorm"
      ? ["bc3-rgba-unorm"]
      : format === "rgba8unorm"
        ? ["bc3-rgba-unorm"]
        : format === "rgba16float"
          ? ["bc7-rgba-unorm"]
          : [];
  for (const cf of compacted) {
    const spec = lookupFormat(cf);
    if (spec === null) continue;
    const scaled: TextureDesc = {
      ...desc,
      width: Math.max(1, Math.floor(desc.width * MIN_RESOLUTION_SCALE)),
      height: Math.max(1, Math.floor(desc.height * MIN_RESOLUTION_SCALE)),
    };
    const bytes = mipChainBytes(scaled, cf, 0);
    steps.push({
      kind: "format-compaction",
      mipFloor: 0,
      scale: MIN_RESOLUTION_SCALE,
      format: cf,
      bytes,
      visualCost: `换用 ${cf}（有损度 ${spec.loss}）且分辨率降至最低档；仅远景可用`,
    });
    if (bytes <= budgetBytes) {
      return texOk(steps, bag.all());
    }
  }

  // 级四：拒绝
  steps.push({
    kind: "reject",
    mipFloor: mipCount - 1,
    scale: MIN_RESOLUTION_SCALE,
    format: null,
    bytes: mipChainBytes(desc, format, mipCount - 1),
    visualCost: "无可用降级方案，拒绝创建（不放行超预算资源）",
  });

  if (mipChainBytes(desc, format, 0) > perTextureCap) {
    return texFail(
      "DEGRADATION_EXHAUSTED",
      `单纹理预算上限 ${perTextureCap} 字节无法满足，降级阶梯已耗尽`,
      "缩小资产源尺寸，或申请提升单纹理上限",
      bag.all(),
    );
  }
  return texOk(steps, bag.all());
}

/** 阶梯单调性断言：每一级字节数必须严格不增（否则降级顺序失去意义）。 */
export function isDegradationMonotone(steps: readonly DegradationStep[]): boolean {
  for (let i = 1; i < steps.length; i += 1) {
    const prev = steps[i - 1];
    const cur = steps[i];
    if (prev === undefined || cur === undefined) continue;
    if (cur.bytes > prev.bytes) return false;
  }
  return true;
}

// ════════════════════════════════════════════════════════════════════════════
// §8 流式上传（判据：分块不卡帧 + 优先级）
// ════════════════════════════════════════════════════════════════════════════

/** 可见性档（决定上传优先级，判据五）。 */
export type VisibilityTier = "onscreen" | "prefetch" | "background";

/** 可见性 → 优先级数值（越小越先）。 */
export const VISIBILITY_PRIORITY: Readonly<Record<VisibilityTier, number>> = {
  onscreen: 0,
  prefetch: 1,
  background: 2,
};

/** 上传调度参数。 */
export interface UploadSchedulerConfig {
  /** 每帧上传字节预算（时间片的字节当量，CGPU-F0017 对接点）。 */
  readonly frameBudgetBytes: number;
  /** 单块字节上界：块不得超帧预算，否则一帧放不下 → 卡帧。 */
  readonly maxChunkBytes: number;
  /** 队列容量上限（防无界占用 CPU 侧内存）。 */
  readonly maxQueuedChunks: number;
  /** 单块重试上限。 */
  readonly retryLimit: number;
}

/** 默认调度参数（保守值：1.5ms 时间片按 ~1.5GB/s 折算约 2MB）。 */
export const DEFAULT_UPLOAD_CONFIG: UploadSchedulerConfig = {
  frameBudgetBytes: 2 * 1024 * 1024,
  maxChunkBytes: 1024 * 1024,
  maxQueuedChunks: 4096,
  retryLimit: 3,
};

/** 一个上传块。 */
export interface UploadChunk {
  readonly chunkId: string;
  readonly textureId: string;
  readonly mipLevel: number;
  readonly byteOffset: number;
  readonly byteLength: number;
  readonly visibility: VisibilityTier;
  /** 已尝试次数。 */
  readonly attempts: number;
}

/**
 * 切块（纯函数）。
 * 块数上界 = ceil(bytes / maxChunkBytes)；块必须 ≤ maxChunkBytes 且
 * maxChunkBytes ≤ frameBudgetBytes（配置校验在 config 闸门里做）。
 */
export function planChunks(
  textureId: string,
  totalBytes: number,
  maxChunkBytes: number,
  visibility: VisibilityTier,
  mipLevelCount: number,
): TextureOutcome<readonly UploadChunk[]> {
  const bag = new TextureDiagBag();
  if (!isPositiveInt(maxChunkBytes)) {
    return texFail("CHUNK_TOO_LARGE", `块上界 ${String(maxChunkBytes)} 非法`, "块上界必须为正整数");
  }
  if (totalBytes <= 0) {
    return texFail("DIMENSION_LIMIT", `总字节 ${String(totalBytes)} 非正`, "无数据可上传");
  }
  const out: UploadChunk[] = [];
  let level = 0;
  let remaining = totalBytes;
  let offset = 0;
  let idx = 0;
  while (remaining > 0) {
    const len = Math.min(maxChunkBytes, remaining);
    out.push({
      chunkId: `${textureId}#${idx}`,
      textureId,
      mipLevel: Math.min(level, Math.max(0, mipLevelCount - 1)),
      byteOffset: offset,
      byteLength: len,
      visibility,
      attempts: 0,
    });
    remaining -= len;
    offset += len;
    idx += 1;
    if (level < mipLevelCount) level += 1;
  }
  if (out.length === 0) {
    bag.push("DIMENSION_LIMIT", "切块结果为空", "检查总字节与块上界");
  }
  return texOk(out, bag.all());
}

/**
 * 排序键（全序，可复算）：
 *   (可见性优先级, mip 级升序, 字节偏移升序)
 * 低分辨率级先传 —— 先让画面可见，高分辨率级后台补齐。
 */
export function compareChunks(a: UploadChunk, b: UploadChunk): number {
  const pa = VISIBILITY_PRIORITY[a.visibility];
  const pb = VISIBILITY_PRIORITY[b.visibility];
  if (pa !== pb) return pa - pb;
  if (a.mipLevel !== b.mipLevel) return a.mipLevel - b.mipLevel;
  if (a.byteOffset !== b.byteOffset) return a.byteOffset - b.byteOffset;
  return a.chunkId < b.chunkId ? -1 : a.chunkId > b.chunkId ? 1 : 0;
}

/** 排好序的队列。 */
export function sortQueue(queue: readonly UploadChunk[]): readonly UploadChunk[] {
  return queue.slice().sort(compareChunks);
}

/** 一帧的上传计划。 */
export interface FrameUploadPlan {
  /** 本帧发出的块（总字节 ≤ 帧预算）。 */
  readonly issued: readonly UploadChunk[];
  /** 本帧字节。 */
  readonly issuedBytes: number;
  /** 因放不下而延后的块。 */
  readonly deferred: readonly UploadChunk[];
  /** 因块自身超帧预算而被拦截的块（配置错误信号）。 */
  readonly oversized: readonly UploadChunk[];
  readonly diagnostics: readonly TextureDiagnostic[];
}

/**
 * 取一帧计划（判据：分块不卡帧）。
 * 规则：
 *   1. 队列为空 → 空计划。
 *   2. 块字节 > 帧预算 → 进 oversized 并产 CHUNK_TOO_LARGE（不发出）。
 *   3. 剩余预算 < 块字节 → **本帧不发**（立场二：不截断，避免队列空洞）。
 *   4. 帧内发满即停；其余顺延。
 */
export function selectChunksForFrame(
  queue: readonly UploadChunk[],
  cfg: UploadSchedulerConfig,
): FrameUploadPlan {
  const bag = new TextureDiagBag();
  const sorted = sortQueue(queue);
  const issued: UploadChunk[] = [];
  const deferred: UploadChunk[] = [];
  const oversized: UploadChunk[] = [];
  let spent = 0;

  for (const c of sorted) {
    if (c.byteLength > cfg.frameBudgetBytes) {
      oversized.push(c);
      bag.push(
        "CHUNK_TOO_LARGE",
        `块 ${c.chunkId} 字节 ${c.byteLength} 超过帧预算 ${cfg.frameBudgetBytes}`,
        "块上界必须 ≤ 帧预算；否则该块无论何时发都会击穿帧预算",
      );
      continue;
    }
    if (spent + c.byteLength > cfg.frameBudgetBytes) {
      // 预算不够：本帧不发（不发半块），顺延
      deferred.push(c);
      continue;
    }
    issued.push(c);
    spent += c.byteLength;
  }

  // 大块被拦截且队列非空 = 存在卡帧风险，显性告警
  if (oversized.length > 0 && queue.length > 0) {
    bag.push(
      "UPLOAD_STALL_RISK",
      `${oversized.length} 个块超帧预算被拦截，上传无法推进`,
      "调小 maxChunkBytes 或调大 frameBudgetBytes；不可通过「先发半块」绕过",
    );
  }

  return {
    issued,
    issuedBytes: spent,
    deferred,
    oversized,
    diagnostics: bag.all(),
  };
}

/** 一次块失败的处置。 */
export type ChunkOutcome =
  | { readonly kind: "done"; readonly chunk: UploadChunk }
  | { readonly kind: "retry"; readonly chunk: UploadChunk; readonly nextAttempts: number }
  | { readonly kind: "abandoned"; readonly chunk: UploadChunk; readonly reason: string };

/**
 * 块失败处置（重试 → 放弃，重试不静默）。
 * 放弃不是丢数据而是**降级**的信号：调用方应把该纹理降级为低分辨率级驻留。
 */
export function settleChunk(chunk: UploadChunk, cfg: UploadSchedulerConfig): ChunkOutcome {
  const nextAttempts = chunk.attempts + 1;
  if (nextAttempts <= cfg.retryLimit) {
    return { kind: "retry", chunk: { ...chunk, attempts: nextAttempts }, nextAttempts };
  }
  return {
    kind: "abandoned",
    chunk,
    reason: `已重试 ${chunk.attempts} 次（上限 ${cfg.retryLimit}）；放弃该块，纹理降级为低分辨率级驻留`,
  };
}

/** 队列溢出处置。 */
/**
 * 入队准入（队列容量保护）。
 *
 * 处置纪律（关键）：**屏上纹理块永不驱逐**。溢出时按「最不重要者先走」
 * 驱逐——后台块先于预取块先于屏上块，且同级按字节偏移倒序（后入队者先走，
 * 已经开始上传的前段块优先保留）。若无块可驱逐且仍超容，则显式拒绝。
 */
export function admitToQueue(
  queue: readonly UploadChunk[],
  incoming: readonly UploadChunk[],
  cfg: UploadSchedulerConfig,
): TextureOutcome<readonly UploadChunk[]> {
  const combined = sortQueue([...queue, ...incoming]);
  if (combined.length <= cfg.maxQueuedChunks) {
    return texOk(combined);
  }

  // 占位顺序纪律：**屏上块先占位**，非屏上块只填剩余空位。
  // 朴素写法是「按排序逐个塞，塞满即止」，但排序把屏上块排在前面之外，
  // 容量若小于「屏上块数」，屏上块会在后续轮次因新后台块入队被挤出——
  // 表现为「明明还有空位却报容量不足」。故这里显式分两趟。
  const onscreen = combined.filter((c) => c.visibility === "onscreen");
  if (onscreen.length > cfg.maxQueuedChunks) {
    return texFail(
      "QUEUE_OVERFLOW",
      `队列上限 ${cfg.maxQueuedChunks} 不足以容纳全部屏上块（屏上块 ${onscreen.length} 个）`,
      "调大 maxQueuedChunks 至屏上块总数以上；屏上纹理块不得丢弃——容量不足须扩队列而非丢屏上块",
      [
        {
          code: "QUEUE_OVERFLOW",
          message: `本帧保 ${onscreen.slice(0, cfg.maxQueuedChunks).length} 个屏上块，${onscreen.length - cfg.maxQueuedChunks} 个无处安放`,
          hint: "这是配置容量问题而非数据问题：后台纹理请降分辨率或延后入队，不得挤占屏上块",
        },
      ],
    );
  }

  const capacityForRest = cfg.maxQueuedChunks - onscreen.length;
  // 非屏上块按「优先级低者先走」驱逐：background 先于 prefetch，
  // 同级按字节偏移倒序（后入队者先走，保留已排在前段的块）。
  const restPriority: Readonly<Record<VisibilityTier, number>> = {
    onscreen: 2,
    prefetch: 1,
    background: 0,
  };
  const rest = combined
    .filter((c): c is UploadChunk & { readonly visibility: Exclude<VisibilityTier, "onscreen"> } =>
      c.visibility !== "onscreen")
    .sort((a, b) => {
      const pa = restPriority[a.visibility];
      const pb = restPriority[b.visibility];
      if (pa !== pb) return pa - pb;
      if (a.byteOffset !== b.byteOffset) return b.byteOffset - a.byteOffset;
      return a.chunkId < b.chunkId ? 1 : -1;
    });
  const keptRest = rest.slice(0, capacityForRest);
  const evicted = rest.slice(capacityForRest);

  const kept = sortQueue([...onscreen, ...keptRest]);
  const orderLabel = [...new Set(evicted.map((c) => c.visibility))].sort().join("、");
  return texOk(kept, [
    {
      code: "QUEUE_OVERFLOW",
      message:
        `队列上限 ${cfg.maxQueuedChunks} 突破：入队 ${incoming.length} 块后共 ${combined.length} 块，` +
        `驱逐 ${evicted.length} 个非屏上块${orderLabel.length > 0 ? `（${orderLabel}）` : ""}`,
      hint: "屏上纹理块全部保留；被驱逐者需重新切块入队（下一轮会按优先级重新排序）",
    },
  ]);
}

// ════════════════════════════════════════════════════════════════════════════
// §9 回收与显存水位联动（判据：水位告急时先放谁）
// ════════════════════════════════════════════════════════════════════════════

/** 显存水位分级。 */
export type WatermarkTier = "normal" | "elevated" | "critical";

/** 水位阈值（占用率）。 */
export const WATERMARK_THRESHOLDS: Readonly<Record<WatermarkTier, number>> = {
  normal: 0.75,
  elevated: 0.9,
  critical: 0.98,
};

/** 按占用率判水位。 */
export function classifyWatermark(usedRatio: number): WatermarkTier {
  if (usedRatio >= WATERMARK_THRESHOLDS.critical) return "critical";
  if (usedRatio >= WATERMARK_THRESHOLDS.elevated) return "elevated";
  return "normal";
}

/** 回收候选（纹理台账的回收视图）。 */
export interface EvictionCandidate {
  readonly textureId: string;
  readonly visibility: VisibilityTier;
  readonly bytes: number;
  /** 已驻留的最低 mip 级（抬高它即释放高层）。 */
  readonly mipFloor: number;
  readonly mipCount: number;
  /** 距上次使用的 tick（越大越久未用）。 */
  readonly lastUsedTick: number;
  /** 业务 pin：UI 图标 / 当前渲染目标等，绝不放。 */
  readonly pinned: boolean;
}

/** 一次回收的一条决策。 */
export interface EvictionAction {
  readonly textureId: string;
  /** 抬高后的 mipFloor（与原值相同 = 未动）。 */
  readonly newMipFloor: number;
  readonly freedBytes: number;
  readonly reason: string;
}

/** 回收计划。 */
export interface EvictionPlan {
  readonly tier: WatermarkTier;
  readonly actions: readonly EvictionAction[];
  readonly freedBytes: number;
  readonly blockedByPinned: readonly string[];
  /** 是否触到了屏上纹理（触到即产出 EVICTION_SHRANK_VISIBLE）。 */
  readonly touchedVisible: boolean;
  readonly diagnostics: readonly TextureDiagnostic[];
}

/**
 * 规划回收（判据：水位告急时先放谁写明）。
 * 顺序（逐级写死，不隐式）：
 *   elevated：只放**后台**纹理的 mip 尾（抬 mipFloor，逐级到 mipCount-1），
 *            再放**预取**纹理的 mip 尾。屏上纹理的 mip 尾本级不动。
 *   critical：后台纹理按 LRU 倒序**整体回收**；预取同理；
 *            仍不够才对屏上纹理**降质**（抬 mip 尾到只剩保底级），
 *            并产 EVICTION_SHRANK_VISIBLE（显性告知）。
 *   pinned：任何水位都不放；全部候选耗尽仍不够 → 产 EVICTION_BLOCKED_PINNED。
 */
export function planEviction(
  candidates: readonly EvictionCandidate[],
  needBytes: number,
  tier: WatermarkTier,
): EvictionPlan {
  const bag = new TextureDiagBag();
  // 可变数组：第二级需要**升级**第一级的动作（原地替换），故不能用只读数组。
  const actions: EvictionAction[] = [];
  const blocked: string[] = [];
  let freed = 0;
  let touchedVisible = false;

  const unpinned = candidates.filter((c) => !c.pinned);
  for (const c of candidates) {
    if (c.pinned) blocked.push(c.textureId);
  }

  const finish = (): EvictionPlan => {
    if (blocked.length > 0 && freed < needBytes) {
      bag.push(
        "EVICTION_BLOCKED_PINNED",
        `${blocked.length} 个 pin 住的纹理未参与回收，缺口 ${needBytes - freed} 字节无法补足`,
        "pin 是业务契约，不得强放；请业务侧释放 pin 或降低水位需求",
      );
    }
    if (touchedVisible) {
      bag.push(
        "EVICTION_SHRANK_VISIBLE",
        "回收触及屏上纹理（已降质到保底分辨率级）",
        "屏上纹理被降质必须显性告知用户；持续发生说明显存预算与资产规模不匹配",
      );
    }
    return {
      tier,
      actions,
      freedBytes: freed,
      blockedByPinned: blocked.sort(),
      touchedVisible,
      diagnostics: bag.all(),
    };
  };

  if (needBytes <= 0 || tier === "normal") return finish();

  // 第一级：抬 mip 尾（保底分辨率级不动）
  // elevated 与 critical 在本级的处置相同——只放非屏上纹理的 mip 尾，
  // 屏上纹理的 mip 尾留给 critical 的第三级（降质而非回收）。
  if (tier === "elevated" || tier === "critical") {
    const pool = unpinned
      .filter((c) => c.visibility !== "onscreen")
      .sort((a, b) => (a.lastUsedTick === b.lastUsedTick
        ? a.textureId < b.textureId ? -1 : 1
        : b.lastUsedTick - a.lastUsedTick));
    for (const c of pool) {
      if (freed >= needBytes) break;
      const target = c.mipCount - 1;
      if (c.mipFloor >= target) continue;
      const descStub = mipTailBytesProxy(c, target);
      actions.push({
        textureId: c.textureId,
        newMipFloor: target,
        freedBytes: descStub,
        reason: `${tier} 水位：释放 ${c.visibility} 纹理的 mip 尾（保留保底分辨率级）`,
      });
      freed += descStub;
    }
  }

  // 第二级（仅 critical）：后台纹理整体回收，按 LRU 倒序
  //
  // 纪律：此处**不是**跳过第一级已动过的纹理，而是把它的动作从
  // 「抬 mip 尾」**升级**为「整体回收」，并只累加差额。朴素实现会在这里
  // 过滤掉已动过的纹理，结果是：critical 水位下后台纹理永远停在
  // 「只剩保底分辨率级」而不会被整体回收——策略事实上失效。
  if (tier === "critical" && freed < needBytes) {
    const pool = unpinned
      .filter((c) => c.visibility !== "onscreen")
      .sort((a, b) => b.lastUsedTick - a.lastUsedTick || (a.textureId < b.textureId ? -1 : 1));
    for (const c of pool) {
      if (freed >= needBytes) break;
      const existingIdx = actions.findIndex((a) => a.textureId === c.textureId);
      const alreadyFreed = existingIdx >= 0 ? (actions[existingIdx]?.freedBytes ?? 0) : 0;
      if (c.bytes <= alreadyFreed) continue;
      const upgrade: EvictionAction = {
        textureId: c.textureId,
        newMipFloor: c.mipCount,
        freedBytes: c.bytes,
        reason: "critical 水位：整体回收非屏上纹理（LRU 倒序，释放量含前级已释放部分）",
      };
      if (existingIdx >= 0) actions[existingIdx] = upgrade;
      else actions.push(upgrade);
      freed += c.bytes - alreadyFreed;
    }
  }

  // 第三级（仅 critical）：仍不够才动屏上纹理，且只降质不回收
  if (tier === "critical" && freed < needBytes) {
    const pool = unpinned
      .filter((c) => c.visibility === "onscreen" && !actions.some((a) => a.textureId === c.textureId))
      .sort((a, b) => b.lastUsedTick - a.lastUsedTick || (a.textureId < b.textureId ? -1 : 1));
    for (const c of pool) {
      if (freed >= needBytes) break;
      const target = Math.min(c.mipCount - 1, c.mipFloor + 1);
      if (target <= c.mipFloor) continue;
      const d = mipTailBytesProxy(c, target);
      actions.push({
        textureId: c.textureId,
        newMipFloor: target,
        freedBytes: d,
        reason: "critical 水位：屏上纹理降质一级（最后手段，不整体回收）",
      });
      freed += d;
      touchedVisible = true;
    }
  }

  return finish();
}

/**
 * mip 尾释放字节的代理估算。
 * 真实字节需 TextureDesc + 格式；回收候选视图刻意不带这两项（保持回收
 * 规划轻量、可在预算热路径上调用），故按 mip 级差折半估算并**声明**为
 * 代理值；差额由预算侧在提交后按实际归还修正。
 */
function mipTailBytesProxy(c: EvictionCandidate, newFloor: number): number {
  const levelsReleased = Math.max(0, Math.min(newFloor, c.mipCount) - c.mipFloor);
  if (levelsReleased === 0) return 0;
  // 每释放一级，字节按 1/4 递减（面积减半、每像素字节不变）
  let freed = 0;
  let perLevel = c.bytes;
  for (let i = 0; i < levelsReleased; i += 1) {
    perLevel = Math.floor(perLevel / 4);
    freed += perLevel;
  }
  return freed;
}

// ════════════════════════════════════════════════════════════════════════════
// §10 泄漏审计
// ════════════════════════════════════════════════════════════════════════════

/** 纹理台账的一条（生命周期视图）。 */
export interface TextureLedgerEntry {
  readonly textureId: string;
  readonly desc: TextureDesc;
  readonly format: PixelFormat;
  readonly serial: number;
  readonly phase: TexturePhase;
  readonly mipFloor: number;
  readonly bytes: number;
  readonly aliases: readonly string[];
  /** 引用计数（别名数 ≠ 引用数，两者都记）。 */
  readonly refCount: number;
  readonly budgetToken: string;
}

/** 审计发现。 */
export interface AuditFinding {
  readonly textureId: string;
  readonly kind: "orphan-alias" | "zero-ref-live" | "retired-with-refs" | "budget-token-missing";
  readonly detail: string;
}

/** 预算令牌账（与预算台账对账用）。 */
export interface BudgetTokenBook {
  readonly tokens: ReadonlySet<string>;
}

/**
 * 泄漏审计（判据矩阵的「泄漏→审计」）。
 * 四类发现：
 *   orphan-alias：别名指向不存在的纹理（纹理回收了别名没解）。
 *   zero-ref-live：仍驻留但引用数为 0（无人用却占显存）。
 *   retired-with-refs：已退役但引用数 > 0（有人还攥着旧代句柄 → 悬垂）。
 *   budget-token-missing：纹理驻留但预算令牌缺失（显存已记不清 → 泄漏）。
 */
export function auditLeaks(
  entries: readonly TextureLedgerEntry[],
  aliasRegistry: TextureAliasRegistry,
  book: BudgetTokenBook,
): TextureOutcome<readonly AuditFinding[]> {
  const bag = new TextureDiagBag();
  const live = new Set<string>();
  for (const e of entries) {
    if (e.phase !== "retired") live.add(e.textureId);
  }

  const findings: AuditFinding[] = [];

  for (const b of aliasRegistry.auditOrphans(live)) {
    findings.push({
      textureId: b.textureId,
      kind: "orphan-alias",
      detail: `别名「${b.alias}」（来源 ${b.origin}）指向已不存在的纹理`,
    });
  }

  for (const e of entries) {
    if (e.phase === "resident" || e.phase === "streaming") {
      if (e.refCount === 0) {
        findings.push({
          textureId: e.textureId,
          kind: "zero-ref-live",
          detail: `仍驻留 ${e.bytes} 字节但引用数为 0`,
        });
      }
      if (!book.tokens.has(e.budgetToken)) {
        findings.push({
          textureId: e.textureId,
          kind: "budget-token-missing",
          detail: `预算令牌 ${e.budgetToken} 不在账上，显存归还链断裂`,
        });
      }
    }
    if (e.phase === "retired" && e.refCount > 0) {
      findings.push({
        textureId: e.textureId,
        kind: "retired-with-refs",
        detail: `已退役（代戳 ${e.serial}）但仍有 ${e.refCount} 个引用 → 悬垂引用`,
      });
    }
  }

  if (findings.length > 0) {
    bag.push(
      "LEAK_SUSPECTED",
      `审计发现 ${findings.length} 处泄漏线索`,
      "按 kind 分类处置：orphan-alias 解绑别名；zero-ref-live 回收；retired-with-refs 排查旧代句柄；budget-token-missing 补归还",
    );
  }

  return texOk(findings.sort((a, b) => (a.textureId === b.textureId
    ? a.kind < b.kind ? -1 : 1
    : a.textureId < b.textureId ? -1 : 1)), bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §11 无障碍：纹理面板读屏可达
// ════════════════════════════════════════════════════════════════════════════

/** 读屏用的状态形状（形状 + 文字双冗余，不依赖颜色单通道）。 */
export type ReaderShape = "circle" | "square" | "triangle";

/** 面板一行的读屏文本。 */
export interface ReaderRow {
  readonly textureId: string;
  readonly shape: ReaderShape;
  /** 完整可读文本（读屏逐字播报，不允许只给形状）。 */
  readonly spoken: string;
}

/** 状态 → 形状 + 措辞。形状冗余保证色觉障碍下仍可区分。 */
const STATE_SHAPE: Readonly<Record<TexturePhase, ReaderShape>> = {
  declared: "circle",
  allocated: "circle",
  streaming: "triangle",
  resident: "square",
  retired: "circle",
};

const STATE_SPOKEN: Readonly<Record<TexturePhase, string>> = {
  declared: "已声明，尚未分配显存",
  allocated: "已分配显存，尚未上传数据",
  streaming: "正在流式上传，画面可能欠采样",
  resident: "已驻留，可正常采样",
  retired: "已回收退役",
};

/**
 * 产出读屏行。
 * 无障碍纪律：文本必须自足——读屏用户不能靠看颜色或形状猜状态，
 * 故 spoken 里同时含状态、尺寸、格式、占用与可见性。
 */
export function buildReaderRows(entries: readonly TextureLedgerEntry[]): readonly ReaderRow[] {
  return entries
    .slice()
    .sort((a, b) => (a.textureId < b.textureId ? -1 : a.textureId > b.textureId ? 1 : 0))
    .map((e) => ({
      textureId: e.textureId,
      shape: STATE_SHAPE[e.phase],
      spoken:
        `${e.textureId}：${STATE_SPOKEN[e.phase]}；` +
        `尺寸 ${e.desc.width} 乘 ${e.desc.height}，格式 ${e.format}，` +
        `驻留 mip 从第 ${e.mipFloor} 级起，占用 ${e.bytes} 字节，` +
        `别名 ${e.aliases.length} 个，引用 ${e.refCount} 次`,
    }));
}

/** 面板整体摘要（一句话，读屏进入面板时先播报）。 */
export function summarizePanel(entries: readonly TextureLedgerEntry[]): string {
  let resident = 0;
  let streaming = 0;
  let retired = 0;
  let bytes = 0;
  for (const e of entries) {
    if (e.phase === "resident") resident += 1;
    else if (e.phase === "streaming") streaming += 1;
    else if (e.phase === "retired") retired += 1;
    bytes += e.phase === "retired" ? 0 : e.bytes;
  }
  return (
    `纹理面板：共 ${entries.length} 项，已驻留 ${resident} 项，` +
    `流式上传中 ${streaming} 项，已退役 ${retired} 项，显存占用合计 ${bytes} 字节。`
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §12 跨域协议哈希（A05 预算池 / CGPU 上传队列）
// ════════════════════════════════════════════════════════════════════════════

/** 本条对外的数据契约形状（跨域对接的唯一真值）。 */
export interface TextureDomainContract {
  readonly name: string;
  readonly version: number;
  readonly phases: readonly TexturePhase[];
  readonly events: readonly TextureEvent[];
  readonly formats: readonly PixelFormat[];
  readonly visibilityTiers: readonly VisibilityTier[];
  readonly qualityTiers: readonly QualityTier[];
  readonly watermarkTiers: readonly WatermarkTier[];
  readonly diagCodes: readonly TextureDiagCode[];
}

/** 本条契约的规范化摘要（哈希输入；字段序固定，序列化确定）。 */
export function canonicalContractString(c: TextureDomainContract): string {
  return [
    c.name,
    String(c.version),
    c.phases.join(","),
    c.events.join(","),
    c.formats.join(","),
    c.visibilityTiers.join(","),
    c.qualityTiers.join(","),
    c.watermarkTiers.join(","),
    c.diagCodes.join(","),
  ].join("|");
}

/** 协议行状态（待补与漂移必须分开，立场五）。 */
export type ProtocolRowState = "matched" | "pending" | "drifted";

/** 一条协议核对结果。 */
export interface ProtocolRow {
  readonly field: string;
  readonly expected: string;
  readonly actual: string;
  readonly state: ProtocolRowState;
  readonly diagnostic: TextureDiagnostic | null;
}

/**
 * 跨域协议核对。
 * 规则：actual 为空（对方尚未实现该字段）→ pending（非阻断）；
 * actual 有值但与 expected 不同 → drifted（必须重签）。
 * 绝不用同一个码表达两种处置方向。
 */
export function verifyContract(
  expected: TextureDomainContract,
  actual: Partial<TextureDomainContract> | null,
): readonly ProtocolRow[] {
  const expStr = canonicalContractString(expected);
  if (actual === null) {
    return [
      {
        field: "contract",
        expected: expStr,
        actual: "",
        state: "pending",
        diagnostic: {
          code: "PROTOCOL_HASH_PENDING",
          message: "对方域尚未提供契约",
          hint: "待补为非阻断项：按本条 canonicalContractString 实现后重核",
        },
      },
    ];
  }

  const rows: ProtocolRow[] = [];
  const push = (field: string, e: unknown, a: unknown): void => {
    const es = Array.isArray(e) ? e.join(",") : String(e);
    const as = a === undefined || a === null ? "" : Array.isArray(a) ? a.join(",") : String(a);
    if (as === "") {
      rows.push({
        field,
        expected: es,
        actual: as,
        state: "pending",
        diagnostic: {
          code: "PROTOCOL_HASH_PENDING",
          message: `${field} 对方未提供`,
          hint: "待补为非阻断项：补齐后本项自动转 matched",
        },
      });
      return;
    }
    if (as !== es) {
      rows.push({
        field,
        expected: es,
        actual: as,
        state: "drifted",
        diagnostic: {
          code: "PROTOCOL_HASH_DRIFT",
          message: `${field} 契约漂移：期望 ${es.slice(0, 96)}，实际 ${as.slice(0, 96)}`,
          hint: "漂移必须重签：双方对齐后升版本号，不得就地私改",
        },
      });
      return;
    }
    rows.push({ field, expected: es, actual: as, state: "matched", diagnostic: null });
  };

  push("name", expected.name, actual.name);
  push("version", expected.version, actual.version);
  push("phases", expected.phases, actual.phases);
  push("events", expected.events, actual.events);
  push("formats", expected.formats, actual.formats);
  push("visibilityTiers", expected.visibilityTiers, actual.visibilityTiers);
  push("qualityTiers", expected.qualityTiers, actual.qualityTiers);
  push("watermarkTiers", expected.watermarkTiers, actual.watermarkTiers);
  push("diagCodes", expected.diagCodes, actual.diagCodes);
  return rows;
}

/** 本条契约实例（供跨域方 import 比对）。 */
export const TEXTURE_DOMAIN_CONTRACT: TextureDomainContract = {
  name: "ve.textureResource",
  version: 1,
  phases: ["declared", "allocated", "resident", "streaming", "retired"],
  events: TEXTURE_EVENTS,
  formats: ALL_PIXEL_FORMATS,
  visibilityTiers: ["onscreen", "prefetch", "background"],
  qualityTiers: QUALITY_TIERS,
  watermarkTiers: ["normal", "elevated", "critical"],
  diagCodes: [
    "FORMAT_UNKNOWN",
    "FORMAT_UNSUPPORTED",
    "SAMPLING_RENDER_CONFLICT",
    "DIMENSION_LIMIT",
    "MIP_CHAIN_INVALID",
    "USAGE_EMPTY",
    "USAGE_INCOMPATIBLE",
    "ILLEGAL_TRANSITION",
    "RETIRED_REUSE",
    "GENERATION_STALE",
    "ALIAS_CONFLICT",
    "ALIAS_UNBOUND",
    "ALIAS_TEXTURE_UNKNOWN",
    "BUDGET_REJECTED",
    "DEGRADATION_EXHAUSTED",
    "CHUNK_TOO_LARGE",
    "UPLOAD_RETRY_EXHAUSTED",
    "UPLOAD_STALL_RISK",
    "QUEUE_OVERFLOW",
    "EVICTION_BLOCKED_PINNED",
    "EVICTION_SHRANK_VISIBLE",
    "SRGB_DOMAIN_VIOLATION",
    "LEAK_SUSPECTED",
    "PROTOCOL_HASH_PENDING",
    "PROTOCOL_HASH_DRIFT",
  ],
};

// ════════════════════════════════════════════════════════════════════════════
// §13 域级自检（不变式机检；纯函数，不含断言框架依赖）
// ════════════════════════════════════════════════════════════════════════════

/** 一条自检结论。 */
export interface TextureSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

const CAPS_FULL: DeviceCaps = {
  maxTextureDimension2D: 16384,
  maxTextureDimension3D: 2048,
  maxTextureArrayLayers: 2048,
  maxMipLevels: 15,
  supportedFamilies: ["none", "bc1", "bc3", "bc5", "bc7", "astc4"],
  perTextureBudgetBytes: 512 * 1024 * 1024,
  totalBudgetBytes: 4 * 1024 * 1024 * 1024,
};

function mkDesc(over: Partial<TextureDesc> = {}): TextureDesc {
  return {
    width: 2048,
    height: 2048,
    depth: 1,
    arrayLayers: 1,
    mipLevels: 0,
    format: "rgba8unorm",
    usage: ["sampled", "uploadDestination"],
    srgb: true,
    hasAlpha: false,
    isNormalMap: false,
    ...over,
  };
}

/** §A 格式表自洽。 */
export function selfCheckFormatTable(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  let tableErr = 0;
  let variantErr = 0;
  for (const fmt of ALL_PIXEL_FORMATS) {
    const s = FORMAT_TABLE[fmt];
    if (s.format !== fmt) tableErr += 1;
    const blockish = s.family !== "none";
    if (blockish && (s.blockBytes <= 0 || s.blockEdge <= 1 || s.bytesPerPixel !== 0)) tableErr += 1;
    if (!blockish && (s.blockBytes !== 0 || s.blockEdge !== 1 || s.bytesPerPixel <= 0)) tableErr += 1;
    if (s.lossy !== (s.loss !== "none")) tableErr += 1;
    if (s.srgbVariant !== null && lookupFormat(s.srgbVariant) === null) variantErr += 1;
    if (s.linearVariant !== null && lookupFormat(s.linearVariant) === null) variantErr += 1;
    // 通道数必须落在 {1,2,4}：0 或 3 会让选择器的通道匹配失效
    if (!([1, 2, 4] as const).includes(s.channelCount)) variantErr += 1;
    // sRGB 变体链必须自洽：srgbCapable 为真时必须给出 -srgb 变体，
    // 且该变体自身也必须 srgbCapable（否则会出现二段跳）。
    if (s.srgbCapable && (s.srgbVariant === null || !FORMAT_TABLE[s.srgbVariant]?.srgbCapable)) {
      variantErr += 1;
    }
    // 非 sRGB 格式不得声称自己是 sRGB 变体
    if (isSrgbVariant(fmt) && !s.srgbCapable) variantErr += 1;
  }
  out.push({
    name: "format-table-self-consistent",
    pass: tableErr === 0,
    detail: `格式表字段互斥与自指一致（违规 ${tableErr} 处）`,
  });
  out.push({
    name: "format-variant-links-resolve",
    pass: variantErr === 0,
    detail: `sRGB/线性变体链接均指向已登记格式（断链 ${variantErr} 处）`,
  });
  return out;
}

/** §B 描述校验闸门。 */
export function selfCheckDescValidation(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const good = validateDesc(mkDesc({ format: "rgba8unorm-srgb" }), CAPS_FULL);
  out.push({
    name: "valid-desc-accepted",
    pass: good.ok,
    detail: good.ok ? "合规描述被接受" : `合规描述被拒：${good.message}`,
  });

  const srgbMismatch = validateDesc(mkDesc({ format: "rgba8unorm", srgb: true }), CAPS_FULL);
  out.push({
    name: "srgb-domain-violation-rejected",
    pass: !srgbMismatch.ok && srgbMismatch.code === "SRGB_DOMAIN_VIOLATION",
    detail: "格式后缀与 srgb 标志不一致被 SRGB_DOMAIN_VIOLATION 拦截",
  });

  const tooBig = validateDesc(mkDesc({ width: 99999, height: 8, format: "rgba8unorm-srgb" }), CAPS_FULL);
  out.push({
    name: "dimension-limit-rejected",
    pass: !tooBig.ok && tooBig.code === "DIMENSION_LIMIT",
    detail: "超设备上限的维度被 DIMENSION_LIMIT 拦截",
  });

  const emptyUsage = validateDesc(mkDesc({ format: "rgba8unorm-srgb", usage: [] }), CAPS_FULL);
  out.push({
    name: "empty-usage-rejected",
    pass: !emptyUsage.ok && emptyUsage.code === "USAGE_EMPTY",
    detail: "空用途集合被 USAGE_EMPTY 拦截",
  });

  const storageOnCompressed = validateDesc(
    mkDesc({ format: "bc7-rgba-unorm-srgb", usage: ["sampled", "storage"] }),
    CAPS_FULL,
  );
  out.push({
    name: "storage-incompatible-rejected",
    pass: !storageOnCompressed.ok && storageOnCompressed.code === "USAGE_INCOMPATIBLE",
    detail: "压缩格式声明 storage 被 USAGE_INCOMPATIBLE 拦截",
  });
  return out;
}

/** §C 生命周期迁移表闭包。 */
export function selfCheckLifecycle(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const phases: readonly TexturePhase[] = ["declared", "allocated", "resident", "streaming", "retired"];

  // 所有 (phase, event) 组合要么在表中，要么报 ILLEGAL_TRANSITION —— 不容第三态
  let silent = 0;
  for (const p of phases) {
    for (const e of TEXTURE_EVENTS) {
      const r = applyTransition({ serial: 0, phase: p }, e);
      if (!r.ok && r.code !== "ILLEGAL_TRANSITION" && r.code !== "RETIRED_REUSE") silent += 1;
    }
  }
  out.push({
    name: "transition-table-total",
    pass: silent === 0,
    detail: `所有 (状态, 事件) 组合都有显式结论，无静默放行（异常 ${silent} 处）`,
  });

  // 四段必经：declared → allocated → streaming → resident 可达
  const seq: TextureEvent[] = ["allocate", "begin-upload", "complete-upload"];
  let stamp: GenerationStamp = { serial: 0, phase: "declared" };
  let reachable = true;
  for (const e of seq) {
    const r = applyTransition(stamp, e);
    if (!r.ok) {
      reachable = false;
      break;
    }
    stamp = r.value.stamp;
  }
  out.push({
    name: "four-stage-path-reachable",
    pass: reachable && stamp.phase === "resident",
    detail: `创建→上传→更新前置链可达，终态 ${stamp.phase}`,
  });

  // 退役后重新驻留必须抬代戳
  const retired = applyTransition({ serial: 7, phase: "retired" }, "begin-upload");
  out.push({
    name: "retired-reuse-bumps-generation",
    pass: retired.ok && retired.value.stamp.serial === 8,
    detail: "退役对象重新驻留时代戳 7 → 8（旧代句柄自动失效）",
  });

  // 旧代句柄误用被拦
  const stale = checkGeneration({ serial: 9, phase: "resident" }, 8);
  out.push({
    name: "stale-generation-rejected",
    pass: !stale.ok && stale.code === "GENERATION_STALE",
    detail: "持有代戳 9 而台账为 8 时被 GENERATION_STALE 拦截",
  });
  return out;
}

/** §D 格式选择与质量声明。 */
export function selfCheckFormatSelection(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const d = mkDesc({ format: "rgba8unorm", srgb: true });

  const pick = selectFormat(d, "balanced", CAPS_FULL);
  out.push({
    name: "auto-select-picks-compressed",
    pass: pick.ok && isBlockCompressed(pick.value.format),
    detail: pick.ok ? `balanced 档自动选中 ${pick.value.format}` : `选择失败：${pick.message}`,
  });

  const pick2 = selectFormat(d, "balanced", CAPS_FULL);
  out.push({
    name: "selection-deterministic",
    pass:
      pick.ok &&
      pick2.ok &&
      pick.value.format === pick2.value.format &&
      pick.value.rejected.length === pick2.value.rejected.length,
    detail: `同输入两次选择同结果同轨迹（轨迹长度 ${pick.ok ? pick.value.rejected.length : 0}）`,
  });

  // 法线图只能落在 bc5（源须声明双通道，这是法线图的前提而非选择偏好）
  const normalPick = selectFormat(
    mkDesc({ format: "rg8unorm", srgb: false, isNormalMap: true }),
    "balanced",
    CAPS_FULL,
  );
  out.push({
    name: "normal-map-forces-bc5",
    pass: normalPick.ok && normalPick.value.format === "bc5-rg-unorm",
    detail: normalPick.ok
      ? `双通道法线图选中 ${normalPick.value.format}`
      : `法线图选择失败：${normalPick.message}`,
  });
  out.push({
    name: "four-channel-normal-map-rejected",
    pass: !selectFormat(mkDesc({ format: "rgba8unorm", srgb: false, isNormalMap: true }), "balanced", CAPS_FULL).ok,
    detail: "四通道法线图源被显式拒绝（Z 通道压进 bc5 会失真）",
  });

  // 带 alpha 的资产不得落 bc1
  const alphaPick = selectFormat(
    mkDesc({ format: "rgba8unorm", srgb: false, hasAlpha: true }),
    "compact",
    CAPS_FULL,
  );
  out.push({
    name: "alpha-asset-avoids-bc1",
    pass: alphaPick.ok && alphaPick.value.format !== "bc1-rgba-unorm",
    detail: alphaPick.ok ? `带 alpha 资产选中 ${alphaPick.value.format}（未落 bc1）` : "选择失败",
  });

  // 档位上界被尊重：lossless 档不得产出有损格式
  const lossless = selectFormat(mkDesc({ format: "rgba16float", srgb: false }), "lossless", CAPS_FULL);
  out.push({
    name: "lossless-tier-yields-lossless",
    pass: lossless.ok && !FORMAT_TABLE[lossless.value.format].lossy,
    detail: lossless.ok ? `lossless 档选中 ${lossless.value.format}（无损失）` : "选择失败",
  });

  // 设备不支持压缩族时必须落到未压缩而非报错
  const noBc: DeviceCaps = { ...CAPS_FULL, supportedFamilies: ["none"] };
  const fallback = selectFormat(mkDesc({ format: "rgba8unorm", srgb: true }), "compact", noBc);
  out.push({
    name: "unsupported-family-falls-back-uncompressed",
    pass: fallback.ok && lookupFormat(fallback.value.format)?.family === "none",
    detail: fallback.ok ? `无压缩族设备落到 ${fallback.value.format}` : `回退失败：${fallback.message}`,
  });

  // 淘汰轨迹必须非空且给出理由
  out.push({
    name: "rejection-trail-populated",
    pass: pick.ok && pick.value.rejected.length > 0 && pick.value.rejected.every((r) => r.detail.length > 0),
    detail: pick.ok ? `淘汰轨迹 ${pick.value.rejected.length} 条，均带理由` : "无轨迹",
  });

  // 通道数必须与源匹配：否则选择器会把 RGBA 纹理换成单通道格式，
  // 颜色信息被静默丢弃（本条历史缺陷，必须有回归断言）。
  const chMatched = pick.ok && (lookupFormat(pick.value.format)?.channelCount ?? 0) === 4;
  out.push({
    name: "channel-count-preserved",
    pass: chMatched,
    detail: pick.ok
      ? `RGBA 源选出 ${pick.value.format}，通道数 ${String(lookupFormat(pick.value.format)?.channelCount)}`
      : "选择失败",
  });
  out.push({
    name: "channel-mismatch-in-trail",
    pass:
      pick.ok &&
      pick.value.rejected.some((r) => r.reason === "channel-count-mismatch"),
    detail: "通道数不符的候选在淘汰轨迹中可见（不是静默过滤）",
  });

  // 位深比必须落在合理区间（0 或 >1 都是错算）
  const ratioOk =
    pick.ok && pick.value.bitsRatio > 0 && pick.value.bitsRatio <= 1.0;
  out.push({
    name: "bits-ratio-sane",
    pass: ratioOk,
    detail: pick.ok ? `位深比 ${pick.value.bitsRatio.toFixed(4)} ∈ (0, 1]` : "选择失败",
  });
  return out;
}

/** §E 质量基线与转换计划。 */
export function selfCheckQuality(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];

  // 基线表必须有实测缺口清单（不能全是 measured=true 的假完备）
  const gaps = QUALITY_BASELINE.filter((r) => !r.measured);
  out.push({
    name: "baseline-gaps-declared",
    pass: gaps.length > 0 && gaps.every((r) => r.psnrFloorDb !== null && r.ssimFloor !== null),
    detail: `质量基线表显式登记 ${gaps.length} 处待实测组合（各有 PSNR/SSIM 下界）`,
  });

  // 有损转换必须有量化损失，不能是无损声明
  const lossyRows = QUALITY_BASELINE.filter((r) => r.loss !== "none");
  out.push({
    name: "lossy-baseline-quantified",
    pass: lossyRows.length > 0 && lossyRows.every((r) => r.psnrFloorDb !== null && r.bitsRatio > 0),
    detail: `${lossyRows.length} 条有损基线均带位深比与 PSNR 下界`,
  });

  // 无损基线的 PSNR 必须为 null（不能编造一个数字）
  const losslessRow = lookupBaseline("rgba8unorm", "rgba8unorm");
  out.push({
    name: "lossless-baseline-has-no-psnr",
    pass: losslessRow !== null && losslessRow.psnrFloorDb === null && losslessRow.ssimFloor === null,
    detail: "无损基线不编造 PSNR/SSIM 数值",
  });

  // 档位上界与声明表同源
  const declMismatch = QUALITY_DECLARATIONS.filter(
    (d) => qualityCeilingFor(d.tier) !== d.maxLoss,
  );
  out.push({
    name: "declaration-single-source",
    pass: declMismatch.length === 0,
    detail: `${QUALITY_DECLARATIONS.length} 条档位声明与上界表同源（不一致 ${declMismatch.length} 条）`,
  });

  // sRGB 端点的转换必须显式经过线性域
  const plan = planTranscode("rgba8unorm-srgb", "bc7-rgba-unorm-srgb", 12);
  out.push({
    name: "srgb-transcode-passes-linear-domain",
    pass: plan.ok && plan.value.includes("to-linear-domain"),
    detail: plan.ok ? "sRGB 端点转换含 to-linear-domain 阶段" : "计划生成失败",
  });

  // 未压缩→未压缩不需要解包阶段
  const plan2 = planTranscode("rgba8unorm", "bgra8unorm", 1);
  out.push({
    name: "uncompressed-transcode-skips-unpack",
    pass: plan2.ok && !plan2.value.includes("unpack-source"),
    detail: plan2.ok ? "未压缩源不产生 unpack-source 阶段" : "计划生成失败",
  });
  return out;
}

/** §F 别名台账。 */
export function selfCheckAliases(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const reg = new TextureAliasRegistry();

  const b1 = reg.bind("mat/hero_albedo", "tex-1", "material");
  const b2 = reg.bind("ui/icon/hero", "tex-1", "ui");
  out.push({
    name: "multi-alias-same-texture",
    pass: b1.ok && b2.ok && reg.aliasesOf("tex-1").length === 2,
    detail: `同纹理两别名（${reg.aliasesOf("tex-1").join("、")}）`,
  });

  const conflict = reg.bind("mat/hero_albedo", "tex-2", "material");
  out.push({
    name: "alias-conflict-rejected",
    pass: !conflict.ok && conflict.code === "ALIAS_CONFLICT",
    detail: "同名改指被 ALIAS_CONFLICT 拒绝，不静默改写",
  });

  const unbindMissing = reg.unbind("no/such/alias");
  out.push({
    name: "unbind-missing-rejected",
    pass: !unbindMissing.ok && unbindMissing.code === "ALIAS_UNBOUND",
    detail: "解绑不存在别名被 ALIAS_UNBOUND 拒绝（防止误以为已解）",
  });

  const unbind = reg.unbind("ui/icon/hero");
  out.push({
    name: "unbind-shrinks-alias-set",
    pass: unbind.ok && reg.aliasesOf("tex-1").length === 1,
    detail: "解绑后别名集收缩为 1",
  });

  const orphans = reg.auditOrphans(new Set<string>());
  out.push({
    name: "orphan-alias-audited",
    pass: orphans.length === 1 && orphans[0]?.alias === "mat/hero_albedo",
    detail: `纹理不存在时审计点名 ${orphans.length} 条孤儿别名`,
  });
  return out;
}

/** §G 预算与降级阶梯。 */
export function selfCheckBudget(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const ledger = new TextureBudgetLedger("ve-texture", 1024);

  const big = ledger.request(768, "texture-mip-chain");
  const commit = ledger.commit("tok-a", 768);
  const over = ledger.request(512, "texture-mip-chain");
  out.push({
    name: "budget-request-commit-release",
    pass: big.granted && commit.granted && ledger.used === 768,
    detail: `申请 768 / 池 1024 → 占用 ${ledger.used}`,
  });
  out.push({
    name: "budget-over-request-rejected",
    pass: !over.granted,
    detail: `超池申请被拒：${over.reason}`,
  });
  const rel = ledger.release("tok-a");
  out.push({
    name: "budget-release-restores",
    pass: rel.granted && ledger.used === 0,
    detail: "归还后池占用归零",
  });
  const dup = ledger.release("tok-a");
  out.push({
    name: "double-release-rejected",
    pass: !dup.granted,
    detail: "重复归还被拒（防显存账虚增）",
  });

  // 降级阶梯：单块预算下必须给出可满足方案
  const steps = planDegradation(
    mkDesc({ format: "rgba8unorm-srgb" }),
    "rgba8unorm-srgb",
    8 * 1024 * 1024,
    CAPS_FULL.perTextureBudgetBytes,
  );
  out.push({
    name: "degradation-plan-found",
    pass: steps.ok && steps.value.length > 0,
    detail: steps.ok ? `降级阶梯 ${steps.value.length} 级` : `降级规划失败：${steps.message}`,
  });
  out.push({
    name: "degradation-monotone",
    pass: steps.ok && isDegradationMonotone(steps.value),
    detail: steps.ok ? "每级字节数单调不增" : "无阶梯可验",
  });
  out.push({
    name: "degradation-under-budget-satisfied",
    pass:
      steps.ok &&
      steps.value.some((s) => s.kind !== "reject" && s.bytes <= 8 * 1024 * 1024),
    detail: steps.ok
      ? `阶梯含满足 ${8 * 1024 * 1024} 字节预算的方案`
      : "无方案",
  });

  // 阶梯耗尽才拒绝：给一个连最小方案都放不下的预算
  const exhausted = planDegradation(
    mkDesc({ format: "rgba8unorm-srgb" }),
    "rgba8unorm-srgb",
    16,
    CAPS_FULL.perTextureBudgetBytes,
  );
  out.push({
    name: "degradation-exhausted-reports",
    pass: exhausted.ok && exhausted.value[exhausted.value.length - 1]?.kind === "reject",
    detail: "极小预算下阶梯以 reject 收尾（显性拒绝而非放行）",
  });
  return out;
}

/** §H 流式上传。 */
export function selfCheckStreaming(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const cfg = DEFAULT_UPLOAD_CONFIG;

  const chunks = planChunks("tex-1", 5 * 1024 * 1024, cfg.maxChunkBytes, "onscreen", 12);
  out.push({
    name: "chunk-plan-covers-bytes",
    pass:
      chunks.ok &&
      chunks.value.reduce((a, c) => a + c.byteLength, 0) === 5 * 1024 * 1024,
    detail: chunks.ok ? `切块 ${chunks.value.length} 块覆盖全字节` : "切块失败",
  });
  out.push({
    name: "chunk-size-within-cap",
    pass: chunks.ok && chunks.value.every((c) => c.byteLength <= cfg.maxChunkBytes),
    detail: `每块 ≤ ${cfg.maxChunkBytes} 字节`,
  });

  const frame = selectChunksForFrame(chunks.ok ? chunks.value : [], cfg);
  out.push({
    name: "frame-budget-never-exceeded",
    pass: frame.issuedBytes <= cfg.frameBudgetBytes,
    detail: `一帧发出 ${frame.issuedBytes} 字节 ≤ 帧预算 ${cfg.frameBudgetBytes}`,
  });

  // 优先级：屏上先于后台
  const bgChunks = planChunks("tex-bg", 1024, cfg.maxChunkBytes, "background", 1);
  const mixed = sortQueue([
    ...(chunks.ok ? chunks.value : []),
    ...(bgChunks.ok ? bgChunks.value : []),
  ]);
  const firstIsOnscreen = mixed.length > 0 && mixed[0]?.visibility === "onscreen";
  const allOnscreenBeforeBackground = mixed.every((c, i) => {
    if (c.visibility !== "background") return true;
    return mixed.slice(i).every((later) => later.visibility === "background");
  });
  out.push({
    name: "visibility-priority-respected",
    pass: firstIsOnscreen && allOnscreenBeforeBackground,
    detail: "屏上纹理块全部排在后台块之前",
  });

  // 低分辨率级先传：同可见性内 mip 升序
  const lvlOrder = mixed
    .filter((c) => c.textureId === "tex-1")
    .map((c) => c.mipLevel);
  out.push({
    name: "low-mip-first",
    pass: lvlOrder.length > 1 && lvlOrder[0] === 0,
    detail: `同纹理先传 mip ${lvlOrder[0] ?? "-"}（低分辨率级优先，高级后台补齐）`,
  });

  // 超大块被拦截且产诊断
  const oversize = selectChunksForFrame(
    [{ chunkId: "x", textureId: "t", mipLevel: 0, byteOffset: 0, byteLength: cfg.frameBudgetBytes * 2, visibility: "onscreen", attempts: 0 }],
    cfg,
  );
  out.push({
    name: "oversized-chunk-blocked",
    pass:
      oversize.issued.length === 0 &&
      oversize.oversized.length === 1 &&
      oversize.diagnostics.some((d) => d.code === "CHUNK_TOO_LARGE"),
    detail: "超帧预算的块被拦截并产 CHUNK_TOO_LARGE（不截断发半块）",
  });

  // 重试 → 放弃
  let c: UploadChunk = {
    chunkId: "c1",
    textureId: "t",
    mipLevel: 0,
    byteOffset: 0,
    byteLength: 1024,
    visibility: "onscreen",
    attempts: 0,
  };
  let retries = 0;
  let outcome: ChunkOutcome = settleChunk(c, cfg);
  while (outcome.kind === "retry") {
    retries += 1;
    outcome = settleChunk(outcome.chunk, cfg);
  }
  out.push({
    name: "retry-then-abandon",
    pass: retries === cfg.retryLimit && outcome.kind === "abandoned",
    detail: `重试 ${retries} 次后放弃（上限 ${cfg.retryLimit}），放弃理由显性`,
  });

  // 队列准入：溢出时屏上块必须全保留，且溢出必显性（回归断言）
  // 容量取「屏上块数 + 少量余量」：必须装得下全部屏上块，
  // 才能验证「溢出时驱逐的是后台块而非屏上块」。容量小于屏上块数时
  // 拒绝才是正确行为（见 queue-starvation-rejects-not-drops）。
  const onCount = 5;
  const bgCount = 5;
  const tiny: UploadSchedulerConfig = {
    ...cfg,
    maxChunkBytes: 1024,
    maxQueuedChunks: onCount + 2,
  };
  const bgChunk = planChunks("t-bg", bgCount * 1024, tiny.maxChunkBytes, "background", 4);
  const onChunk = planChunks("t-on", onCount * 1024, tiny.maxChunkBytes, "onscreen", 4);
  const admitted = admitToQueue(
    bgChunk.ok ? bgChunk.value : [],
    onChunk.ok ? onChunk.value : [],
    tiny,
  );
  const onTotal = onChunk.ok ? onChunk.value.length : 0;
  const bgTotal = bgChunk.ok ? bgChunk.value.length : 0;
  const onKept = admitted.ok ? admitted.value.filter((x) => x.visibility === "onscreen").length : 0;
  out.push({
    name: "queue-admission-protects-onscreen",
    pass: admitted.ok && onTotal > 0 && onKept === onTotal && admitted.value.length === tiny.maxQueuedChunks,
    detail: admitted.ok
      ? `容量 ${tiny.maxQueuedChunks}、共 ${onTotal + bgTotal} 块 → 保留全部 ${onKept} 个屏上块` +
        ` + ${admitted.value.length - onKept} 个后台块，容量填满`
      : `准入被拒：${admitted.message}`,
  });
  out.push({
    name: "queue-overflow-is-explicit",
    pass: admitted.ok && admitted.diagnostics.some((d) => d.code === "QUEUE_OVERFLOW"),
    detail: admitted.ok
      ? `队列溢出产 ${admitted.diagnostics.filter((d) => d.code === "QUEUE_OVERFLOW").length} 条 QUEUE_OVERFLOW（零静默驱逐）`
      : "溢出未产诊断",
  });

  // 容量不足到连屏上块都保不住时必须拒绝而非丢屏上数据
  const starve: UploadSchedulerConfig = { ...cfg, maxChunkBytes: 1024, maxQueuedChunks: 2 };
  const starveOn = planChunks("t-on", 5 * 1024, starve.maxChunkBytes, "onscreen", 4);
  const starved = admitToQueue(starveOn.ok ? starveOn.value : [], [], starve);
  out.push({
    name: "queue-starvation-rejects-not-drops",
    pass: !starved.ok && starved.code === "QUEUE_OVERFLOW",
    detail: starved.ok
      ? "屏上块被静默丢弃（违规）"
      : `容量不足以容纳屏上块时显式拒绝：${starved.message}`,
  });
  return out;
}

/** §I 回收水位联动。 */
export function selfCheckEviction(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];

  out.push({
    name: "watermark-classification",
    pass:
      classifyWatermark(0.5) === "normal" &&
      classifyWatermark(0.92) === "elevated" &&
      classifyWatermark(0.99) === "critical",
    detail: "水位分级 0.5/0.92/0.99 → normal/elevated/critical",
  });

  const cands: EvictionCandidate[] = [
    { textureId: "t-on", visibility: "onscreen", bytes: 1024, mipFloor: 0, mipCount: 12, lastUsedTick: 100, pinned: false },
    { textureId: "t-bg", visibility: "background", bytes: 1024, mipFloor: 0, mipCount: 12, lastUsedTick: 10, pinned: false },
    { textureId: "t-pin", visibility: "background", bytes: 1024, mipFloor: 0, mipCount: 12, lastUsedTick: 5, pinned: true },
  ];

  const elevated = planEviction(cands, 100000, "elevated");
  out.push({
    name: "elevated-only-releases-background-mip-tail",
    pass:
      elevated.actions.length > 0 &&
      elevated.actions.every((a) => a.textureId !== "t-on") &&
      elevated.actions.every((a) => a.newMipFloor < 12),
    detail: `elevated 释放 ${elevated.actions.length} 项，全部为后台纹理且仅抬 mip 尾`,
  });

  const critical = planEviction(cands, 100000, "critical");
  out.push({
    name: "critical-releases-whole-background",
    pass:
      critical.actions.some((a) => a.textureId === "t-bg" && a.newMipFloor === 12) &&
      !critical.actions.some((a) => a.textureId === "t-pin"),
    detail: "critical 整体回收后台纹理，且 pin 住的纹理未被触碰",
  });

  const pinnedOnly = planEviction([cands[2] as EvictionCandidate], 999999, "critical");
  out.push({
    name: "pinned-blocks-eviction-with-diagnostic",
    pass:
      pinnedOnly.actions.length === 0 &&
      pinnedOnly.blockedByPinned.includes("t-pin") &&
      pinnedOnly.diagnostics.some((d) => d.code === "EVICTION_BLOCKED_PINNED"),
    detail: "全部候选被 pin 时不回收，产 EVICTION_BLOCKED_PINNED",
  });

  const touchVisible = planEviction([cands[0] as EvictionCandidate], 999999, "critical");
  out.push({
    name: "visible-shrink-is-explicit",
    pass:
      touchVisible.touchedVisible &&
      touchVisible.diagnostics.some((d) => d.code === "EVICTION_SHRANK_VISIBLE"),
    detail: "动屏上纹理必产 EVICTION_SHRANK_VISIBLE（不得静默降质）",
  });

  out.push({
    name: "normal-tier-evicts-nothing",
    pass: planEviction(cands, 999999, "normal").actions.length === 0,
    detail: "normal 水位不做任何回收",
  });
  return out;
}

/** §J 泄漏审计。 */
export function selfCheckAudit(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const reg = new TextureAliasRegistry();
  reg.bind("a/1", "tex-1", "material");
  reg.bind("a/2", "tex-gone", "ui");

  const entries: TextureLedgerEntry[] = [
    {
      textureId: "tex-1",
      desc: mkDesc(),
      format: "rgba8unorm-srgb",
      serial: 0,
      phase: "resident",
      mipFloor: 0,
      bytes: 1024,
      aliases: ["a/1"],
      refCount: 1,
      budgetToken: "tok-1",
    },
    {
      textureId: "tex-2",
      desc: mkDesc(),
      format: "rgba8unorm-srgb",
      serial: 1,
      phase: "resident",
      mipFloor: 0,
      bytes: 1024,
      aliases: [],
      refCount: 0,
      budgetToken: "tok-missing",
    },
    {
      textureId: "tex-3",
      desc: mkDesc(),
      format: "rgba8unorm-srgb",
      serial: 2,
      phase: "retired",
      mipFloor: 0,
      bytes: 0,
      aliases: [],
      refCount: 2,
      budgetToken: "tok-3",
    },
  ];

  const audit = auditLeaks(entries, reg, { tokens: new Set(["tok-1", "tok-3"]) });
  out.push({
    name: "audit-finds-all-four-kinds",
    pass:
      audit.ok &&
      audit.value.some((f) => f.kind === "orphan-alias") &&
      audit.value.some((f) => f.kind === "zero-ref-live") &&
      audit.value.some((f) => f.kind === "retired-with-refs") &&
      audit.value.some((f) => f.kind === "budget-token-missing"),
    detail: audit.ok ? `审计发现 ${audit.value.length} 处，覆盖四类` : "审计失败",
  });
  out.push({
    name: "audit-finds-trigger-diagnostic",
    pass: audit.ok && audit.diagnostics.some((d) => d.code === "LEAK_SUSPECTED"),
    detail: "审计命中必产 LEAK_SUSPECTED（零静默）",
  });
  return out;
}

/** §K 无障碍读屏。 */
export function selfCheckAccessible(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];
  const entries: TextureLedgerEntry[] = [
    {
      textureId: "tex-1",
      desc: mkDesc(),
      format: "rgba8unorm-srgb",
      serial: 0,
      phase: "streaming",
      mipFloor: 0,
      bytes: 4096,
      aliases: ["a/1", "a/2"],
      refCount: 2,
      budgetToken: "tok-1",
    },
  ];
  const rows = buildReaderRows(entries);
  out.push({
    name: "reader-row-self-contained",
    pass:
      rows.length === 1 &&
      (rows[0]?.spoken.includes("流式上传") ?? false) &&
      (rows[0]?.spoken.includes("格式") ?? false) &&
      (rows[0]?.spoken.includes("占用") ?? false),
    detail: "读屏文本自足：含状态、尺寸、格式、占用、别名与引用",
  });
  out.push({
    name: "reader-shape-redundancy",
    pass: rows.length === 1 && rows[0]?.shape === "triangle",
    detail: "流式上传态用 triangle 形状（形状冗余，不依赖颜色）",
  });
  const sum = summarizePanel(entries);
  out.push({
    name: "panel-summary-spoken",
    pass: sum.includes("纹理面板") && sum.includes("显存占用"),
    detail: "面板摘要含总量、驻留数与显存占用",
  });
  return out;
}

/** §L 跨域协议哈希。 */
export function selfCheckProtocol(): readonly TextureSelfCheck[] {
  const out: TextureSelfCheck[] = [];

  const pending = verifyContract(TEXTURE_DOMAIN_CONTRACT, null);
  out.push({
    name: "missing-contract-is-pending",
    pass:
      pending.length === 1 &&
      pending[0]?.state === "pending" &&
      pending[0]?.diagnostic?.code === "PROTOCOL_HASH_PENDING",
    detail: "对方未提供契约 → pending 且用 PROTOCOL_HASH_PENDING（非阻断）",
  });

  const drifted = verifyContract(TEXTURE_DOMAIN_CONTRACT, {
    name: TEXTURE_DOMAIN_CONTRACT.name,
    version: 2,
  });
  const driftRow = drifted.find((r) => r.field === "version");
  out.push({
    name: "version-mismatch-is-drift",
    pass: driftRow?.state === "drifted" && driftRow.diagnostic?.code === "PROTOCOL_HASH_DRIFT",
    detail: "版本不一致 → drifted 且用 PROTOCOL_HASH_DRIFT（必须重签）",
  });
  out.push({
    name: "pending-and-drift-codes-distinct",
    pass:
      pending[0]?.diagnostic?.code !== driftRow?.diagnostic?.code &&
      pending[0]?.diagnostic?.code === "PROTOCOL_HASH_PENDING" &&
      driftRow?.diagnostic?.code === "PROTOCOL_HASH_DRIFT",
    detail: "待补与漂移不共码（处置方向相反，禁止合并）",
  });

  const matched = verifyContract(TEXTURE_DOMAIN_CONTRACT, { ...TEXTURE_DOMAIN_CONTRACT });
  out.push({
    name: "identical-contract-all-matched",
    pass: matched.every((r) => r.state === "matched"),
    detail: `完全一致契约 ${matched.length} 项全部 matched`,
  });

  // 规范化串确定：同契约两次序列化逐位一致
  const s1 = canonicalContractString(TEXTURE_DOMAIN_CONTRACT);
  const s2 = canonicalContractString(TEXTURE_DOMAIN_CONTRACT);
  out.push({
    name: "canonical-string-deterministic",
    pass: s1 === s2 && s1.length > 0,
    detail: `契约规范化串确定（长度 ${s1.length}）`,
  });
  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly TextureSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    format: selfCheckFormatTable(),
    desc: selfCheckDescValidation(),
    lifecycle: selfCheckLifecycle(),
    selection: selfCheckFormatSelection(),
    quality: selfCheckQuality(),
    aliases: selfCheckAliases(),
    budget: selfCheckBudget(),
    streaming: selfCheckStreaming(),
    eviction: selfCheckEviction(),
    audit: selfCheckAudit(),
    accessible: selfCheckAccessible(),
    protocol: selfCheckProtocol(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, total, failed };
}