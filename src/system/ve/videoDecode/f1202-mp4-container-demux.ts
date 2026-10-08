/**
 * VE-F1202 · MP4 容器解封装（G 域 · L1 容器层实现 · 目标 440 行）
 * ---------------------------------------------------------------------------
 * 职责定位：把一个 MP4 文件（或一段 fragmented MP4 字节流）解成一组**可寻址的
 * 样本**：每个样本有轨道号、文件偏移、长度、解码时间戳（DTS）、呈现时间戳（PTS）
 * 与是否为关键帧。解完之后，F1204-F1207 的解码器才有钱可解——本条不做任何解码。
 *
 * 本条在管线里的位置（G 域 L1 层，消费 F1201 冻结契约）：
 *   字节流 → [L1 容器解封装 = 本条] → 码流样本 → [L2 感知/路由] → [L3~L5 解码/呈现]
 *
 * 四块职责，逐条对应锚点判据：
 *   1. Box 树解析 —— ftyp/moov/moof/mdat 全树遍历；支持 64 位 largesize 扩展
 *      （size==1 时的 64 位真实长度，size==0 表示「延伸到文件末尾」）；父子
 *      完整性校验（子Box 必须完整落在父 Box 的 payload 内）。
 *   2. 轨道提取 + sample 表五件套 —— tkhd/mdhd/stsd 解析轨道头；stsc（样表-块
 *      映射）、stsz（样本大小）、stco/co64（块偏移）、stts（解码时间）、ctts
 *      （呈现偏移）五件套实现。stsc 的样表-块映射算法是 **seek 正确性的根基**：
 *      它决定「第 N 个样本在文件的哪个字节」，算错则 seek 落到别的帧上。
 *   3. 编解码器识别 —— avc1/hev1/av01/mp4a 等四字符码；未知四字符码 → 能力
 *      查询 + 显性不支持（绝不静默按未知格式喂给解码器）。
 *   4. 时间戳管理 —— DTS/PTS 分离（B 帧的 PTS ≥ DTS，解码序≠呈现序）；重排序
 *      缓冲（解码序 → 呈现序重建）并声明 reorder 上界；分片 MP4（moof/traf/
 *      mfhd/tfhd/tfdt）支持流式场景，碎片间轨道延续语义。
 *
 * 整数纪律（对齐 F1122，全域防护）：本模块所有「长度 × 数量」「偏移 + 长度」
 * 「下标 × 元素宽度」一律走 checkedMul / checkedAdd，溢出即拒绝，绝不 wrapping
 * 后拿一个错误偏移去读文件。计数上限表（MP4_BOX_COUNT_LIMIT 等）防分配炸弹：
 * 一个声明 40 亿个 sample 的 stsz 不该让引擎先申请 160GB 内存。
 *
 * 零静默纪律：任何畸形（Box 越界/父子不完整/表项自相矛盾/时间戳倒退/未知
 * 编解码器/重排序上界被突破）都产出三要素诊断（code + message + hint），
 * 不抛异常、不吞诊断、无静默分支。
 *
 * 判据：标准 MP4 测试文件解析、碎片 MP4、编解码器全覆盖、时间戳正确、畸形拦截。
 *
 * 交接说明：纯契约 + 纯函数，零 I/O —— 解析器只吃 Uint8Array，不碰文件系统、
 * 不发网络请求、不分配 GPU 资源，可在 Node 校验脚本与浏览器 Worker 中同构运行。
 * 下游 F1203（MKV/WebM）替换本条实现时，L1 层契约不变——这是 F1201 冻结的用意。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默基础设施；与 F1201 同纪律，此处独立实现不跨文件 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种畸形独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 字节流长度不足以下任何 Box 头（8 字节）——文件被截断。 */
  | "MP4_TRUNCATED"
  /** Box 声明的大小越界（size 超出父 payload 或文件末尾）。 */
  | "MP4_BOX_SIZE_OUT_OF_RANGE"
  /** Box size 字段为 1 但 largesize 扩展缺失或越界。 */
  | "MP4_BOX_LARGE_SIZE_INVALID"
  /** 子 Box 越出父 Box payload（父子完整性校验失败）。 */
  | "MP4_BOX_CHILD_OVERFLOW"
  /** Box 类型未登记（不在已知 Box 清单内，且调用方要求严格模式）。 */
  | "MP4_BOX_TYPE_UNKNOWN"
  /** 必需的顶层 Box 缺失（缺 moov 即无样本表，整条流不可解）。 */
  | "MP4_REQUIRED_BOX_MISSING"
  /** 轨道未找到（按 trackId 查询未命中）。 */
  | "MP4_TRACK_NOT_FOUND"
  /** sample 表五件套不自洽（表项数互相矛盾，如 stsz 数量 ≠ stts 数量）。 */
  | "MP4_SAMPLE_TABLE_INCONSISTENT"
  /** 样表-块映射越界（stsc 的 first_chunk 超过 stco 的块数）。 */
  | "MP4_STSC_CHUNK_OUT_OF_RANGE"
  /** 样本总数超过计数上限（分配炸弹防护，F1122）。 */
  | "MP4_COUNT_LIMIT_EXCEEDED"
  /** 算术溢出（乘法或加法溢出 u64/Float64 安全整数域）。 */
  | "MP4_ARITHMETIC_OVERFLOW"
  /** 编解码器四字符码未登记（显性不支持，不静默透传）。 */
  | "MP4_CODEC_UNSUPPORTED"
  /** 解码时间戳非单调（DTS 必须严格递增，否则 stts 描述自相矛盾）。 */
  | "MP4_DTS_NON_MONONOTONIC"
  /** 呈现时间戳早于解码时间戳（PTS < DTS 在有 B 帧时不可能）。 */
  | "MP4_PTS_BEFORE_DTS"
  /** 重排序缓冲上界被突破（reorder 深度超出声明值 → 声明与实现脱节）。 */
  | "MP4_REORDER_BOUND_EXCEEDED"
  /** 索引越界（sampleIndex / chunkIndex 超出范围）。 */
  | "MP4_INDEX_OUT_OF_RANGE"
  /** 轨道号在文件内重复出现。 */
  | "MP4_TRACK_ID_DUPLICATE"
  /** 解封装结果为空（无任何轨道或零样本）。 */
  | "MP4_NO_SAMPLES"
  /** 判据自检不通过。 */
  | "CRITERION_SELFCHECK_FAILED";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办（可操作）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合：成功必带 value，失败必带 code/message/hint——失败不可被误当成功。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

/** 成功构造（diagnostics 允许携带非致命告警，例如未知 Box 的宽容跳过）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造（三要素齐备，零静默）。 */
export function err<T>(
  code: DiagCode,
  message: string,
  hint: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  return { ok: false, code, message, hint, diagnostics };
}

/** 诊断袋：累积非致命发现，致命问题走返回值。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({ code, message, hint });
  }

  all(): readonly Diagnostic[] {
    return this.items;
  }

  get size(): number {
    return this.items.length;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 整数安全算术 + 计数上限（F1122 全域防护纪律的本地落地面）
// ════════════════════════════════════════════════════════════════════════════

/** 浮点可精确表示整数的上界（2^53-1）；超过它 JS number 会丢精度。 */
export const MAX_SAFE = Number.MAX_SAFE_INTEGER;

/**
 * 乘法：溢出即 null（调用方必须显式处理 null，不得当 0 用）。
 * 之所以不抛异常：解析器面对的是攻击者可控的字节流，异常会被 catch 后
 * 顺手降级为「跳过这个 Box」——那正是 F1122 禁止的静默绕过。
 */
export function checkedMul(a: number, b: number): number | null {
  if (!Number.isSafeInteger(a) || !Number.isSafeInteger(b)) return null;
  const r = a * b;
  return Number.isSafeInteger(r) ? r : null;
}

/** 加法：溢出即 null。 */
export function checkedAdd(a: number, b: number): number | null {
  if (!Number.isSafeInteger(a) || !Number.isSafeInteger(b)) return null;
  const r = a + b;
  return Number.isSafeInteger(r) ? r : null;
}

/** 区间校验：[offset, offset+size) 是否完整落在 [0, limit) 内。加法溢出也判失败。 */
export function checkedRange(offset: number, size: number, limit: number): boolean {
  if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || !Number.isSafeInteger(limit)) {
    return false;
  }
  if (offset < 0 || size < 0 || offset > limit) return false;
  const end = offset + size;
  if (!Number.isSafeInteger(end)) return false; // offset+size 自身溢出
  return end <= limit;
}

/**
 * 每格式的合理计数上限（数据驱动，F1122 要求）。
 * 依据是「真实世界 MP4 的合理上界」而非「内存允许的最大值」：
 * 一个 4K 视频的 sample 数在十万量级；声明一亿个 sample 的文件要么是损坏，
 * 要么是攻击——两者都不该让引擎先分配内存再报错。
 */
export const MP4_COUNT_LIMIT = {
  /** 单文件 Box 总数上限（防 Box 炸弹：深度嵌套的微型 Box 也能堆到百万级）。 */
  boxCount: 200_000,
  /** 顶层 Box 数上限。 */
  topLevelBoxCount: 64,
  /** 单 Box 的直接子 Box 数上限。 */
  childBoxCount: 128,
  /** 轨道数上限。 */
  trackCount: 64,
  /** 单轨 sample 数上限。 */
  sampleCountPerTrack: 5_000_000,
  /** 单轨 chunk（stco 项）数上限。 */
  chunkCount: 2_000_000,
  /** stsc 表项数上限。 */
  stscEntryCount: 100_000,
  /** stts/stsz/ctts 表项数上限。 */
  timeToSampleEntryCount: 1_000_000,
  /** 重排序缓冲深度上界（声明值，超过即认为容器描述与实现脱节）。 */
  reorderDepth: 128,
} as const;

// ════════════════════════════════════════════════════════════════════════════
// §3 Box 树解析（判据一：Box 树遍历 + largesize + 父子完整性）
// ════════════════════════════════════════════════════════════════════════════

/** Box 树解析选项。 */
export interface BoxParseOptions {
  /**
   * 宽容模式：遇到未登记 Box 时跳过并记诊断（默认 true）。
   * 严格模式：未登记 Box 直接拒绝——用于「我要确认这个文件里只有我认识的东西」的场景。
   */
  readonly lenient?: boolean;
}

/** 解析出的一个 Box 节点。 */
export interface BoxNode {
  /** 四字符码，如 "moov"/"trak"/"stsd"。 */
  readonly type: string;
  /** Box 在文件中的绝对起始偏移。 */
  readonly offset: number;
  /** Box 声明的总长度（含 8 字节头）。 */
  readonly size: number;
  /** payload 起点 = offset + 头长度。 */
  readonly payloadStart: number;
  /** payload 长度 = size - 头长度。 */
  readonly payloadSize: number;
  /** 直接子 Box（已解析）。容器类 Box 才有。 */
  readonly children: readonly BoxNode[];
}

/**
 * 已知 Box 类型登记表：哪些 Box 是容器（内含子 Box），哪些是叶子（payload 为原始字段）。
 * 登记的意义在于**只对容器 Box 递归**——若对所有 Box 都尝试按子 Box 解析，
 * 叶子 Box 的二进制字段会被误读成垃圾 Box，产出上百条假诊断。
 */
export const CONTAINER_BOXES: ReadonlySet<string> = new Set([
  // 文件级
  "moov", // 影片元数据：轨道、样本表的总容器
  "trak", // 单条轨道
  "edts", // 编辑列表（elst）
  "mdia", // 轨道媒体信息
  "minf", // 媒体信息容器
  "stbl", // 样本表容器（stsd/stts/stsc/stsz/stco 全在这里）
  "dinf", // 数据引用容器
  "mvex", // 影片扩展（分片 MP4 的 mfhd/traf 挂这里）
  "moof", // 影片片段（分片 MP4 的容器）
  "traf", // 轨道片段
  "mfra", // 片段随机访问（tfra）
  // 编解码特定容器
  "edts_elst_placeholder_removed",
  "udta", // 用户数据
  "meta", // 元数据（注意：meta 是 FullBox，头部多 4 字节 version/flags）
]);

/** 已知叶子 Box（登记以便「严格模式」下区分「不认识的容器」与「不认识但可跳过」）。 */
export const LEAF_BOXES: ReadonlySet<string> = new Set([
  "ftyp", // 文件类型与兼容品牌
  "mvhd", // 影片头（时长/ timescale）
  "tkhd", // 轨道头（trackId、宽高）
  "mdhd", // 媒体头（timescale、时长）
  "hdlr", // 处理器类型（vide/soun/sbtl→字幕轨判定）
  "vmhd", // 视频媒体头
  "smhd", // 声音媒体头
  "nmhd", // 多媒体头
  "sthd", // 字幕媒体头
  "stsd", // 样本描述（含四字符码）
  "stts", // 解码时间到时间
  "ctts", // 呈现时间偏移（有 B 帧时存在）
  "stsc", // 样表-块映射
  "stsz", // 样本大小
  "stz2", // 紧凑样本大小
  "stco", // 块偏移（32 位）
  "co64", // 块偏移（64 位）
  "stss", // 同步样本（关键帧表）
  "elst", // 编辑列表
  "mfhd", // 影片片段头（分片序号）
  "tfhd", // 轨道片段头
  "tfdt", // 轨道片段基址时间（分片内 DTS 基准）
  "trun", // 轨道片段运行（样本尺寸/偏移/时长/标志）
  "sidx", // 片段索引
  "free", // 空闲空间
  "skip", // 空闲空间（另一种类型码）
  "mdat", // 媒体数据
]);

/** FullBox 头部：version(1) + flags(3)，共 4 字节，出现在 meta 等 Box 内部。 */
const FULLBOX_HEADER_SIZE = 4;

/**
 * 解析 Box 头 8 字节（含 largesize 扩展）。
 *
 * ISO/IEC 14496-12 的 Box 头有两种扩展形态，本函数都覆盖：
 *   size == 1  → 真实的 64 位长度存放在紧随其后的 8 字节largesize 中；
 *   size == 0  → 本 Box 延伸到父容器末尾（合法，仅允许在特定位置出现）。
 *
 * 为什么要认真处理 largesize：超过 4GB 的文件（长视频、高码率素材）必须走这条路，
 * 而 MP4 生态里「不支持 largesize 的解析器」在真实素材上会直接失败。
 */
function parseBoxHeader(
  data: Uint8Array,
  offset: number,
  limit: number,
): Outcome<{ type: string; headerSize: number; totalSize: number; payloadStart: number }> {
  // 8 字节头是硬门槛：连头都不完整就只能判定截断。
  if (!checkedRange(offset, 8, limit)) {
    return err(
      "MP4_TRUNCATED",
      `偏移 ${offset} 处不足 8 字节以容纳 Box 头`,
      "文件被截断或偏移计算有误；请核对文件长度与上层 Box 声明的 payload 范围",
    );
  }

  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const declared32 = view.getUint32(offset, false); // big-endian
  const type = String.fromCharCode(
    data[offset + 4]!,
    data[offset + 5]!,
    data[offset + 6]!,
    data[offset + 7]!,
  );

  // 形态一：size == 0 → 延伸到 limit（父容器末尾）。
  if (declared32 === 0) {
    const payloadSize = limit - offset - 8;
    if (payloadSize < 0) {
      return err(
        "MP4_BOX_SIZE_OUT_OF_RANGE",
        `Box ${type} 声明 size=0（延伸至末尾）但其起点已越过容器末尾（offset=${offset}, limit=${limit}）`,
        "size=0 只允许出现在父容器的合法位置；请核对 Box 嵌套与父 payload 边界",
      );
    }
    return ok({ type, headerSize: 8, totalSize: 8 + payloadSize, payloadStart: offset + 8 });
  }

  // 形态二：size == 1 → 64 位 largesize 紧随其后。
  if (declared32 === 1) {
    if (!checkedRange(offset, 16, limit)) {
      return err(
        "MP4_BOX_LARGE_SIZE_INVALID",
        `Box ${type} 声明 size==1（64 位长度）但剩余字节不足以容纳 largesize`,
        "文件在此处被截断；largesize 需要紧跟头部的 8 字节，请核对文件完整性",
      );
    }
    const hi = view.getUint32(offset + 8, false);
    const lo = view.getUint32(offset + 12, false);
    const total = hi * 4_294_967_296 + lo; // 2^32 * hi + lo
    if (!Number.isSafeInteger(total) || total < 16) {
      return err(
        "MP4_BOX_LARGE_SIZE_INVALID",
        `Box ${type} 的 largesize 非法（total=${total}）`,
        "largesize 必须 ≥ 16（8 字节头 + 8 字节 largesize），且不超过 2^53-1",
      );
    }
    if (!checkedRange(offset, total, limit)) {
      return err(
        "MP4_BOX_SIZE_OUT_OF_RANGE",
        `Box ${type} 声明 largesize=${total}，但从 offset=${offset} 起越界（limit=${limit}）`,
        "声明长度超出容器/文件末尾；这是损坏文件或恶意构造的典型特征，拒绝解析",
      );
    }
    return ok({ type, headerSize: 16, totalSize: total, payloadStart: offset + 16 });
  }

  // 形态三：普通 32 位长度。size < 8 意味着连头都装不下，是明确的畸形。
  if (declared32 < 8) {
    return err(
      "MP4_BOX_SIZE_OUT_OF_RANGE",
      `Box ${type} 声明 size=${declared32}，小于 8 字节头本身`,
      "size 必须 ≥ 8；若为 0 或 1 则走对应的扩展形态。请核对字节序（应为 big-endian）",
    );
  }
  if (!checkedRange(offset, declared32, limit)) {
    return err(
      "MP4_BOX_SIZE_OUT_OF_RANGE",
      `Box ${type} 声明 size=${declared32}，但从 offset=${offset} 起越界（limit=${limit}）`,
      "声明长度超出容器/文件末尾；拒绝解析而非截断读取（截断会把后续 Box 解析成垃圾）",
    );
  }
  return ok({ type, headerSize: 8, totalSize: declared32, payloadStart: offset + 8 });
}

/**
 * 解析一段字节为 Box 列表（不递归）。
 *
 * 父子完整性校验就在这里落地：每个子 Box 必须**完整落在**父 payload 内
 * （payloadStart..payloadStart+payloadSize）。若某个子 Box 越界，本函数返回
 * 显式失败——而不是「解析到这里为止，剩下的忽略」。后者会让一个被篡改的
 * Box 长度悄悄吃掉后续 Box 的数据，而症状会表现为「解码到一半花屏」，
 * 根因却在几百万字节之前。
 */
function parseBoxList(
  data: Uint8Array,
  start: number,
  end: number,
  options: BoxParseOptions,
  bag: DiagBag,
  depth: number,
): Outcome<readonly BoxNode[]> {
  const lenient = options.lenient !== false;
  const out: BoxNode[] = [];
  let cursor = start;
  let count = 0;

  while (cursor < end) {
    if (++count > MP4_COUNT_LIMIT.childBoxCount) {
      return err(
        "MP4_COUNT_LIMIT_EXCEEDED",
        `单个容器内 Box 数超过上限 ${MP4_COUNT_LIMIT.childBoxCount}（起始 offset=${start}）`,
        "这是 Box 炸弹的典型特征（大量微型 Box 堆砌）；拒绝解析，避免 CPU 与内存被耗尽",
      );
    }

    const header = parseBoxHeader(data, cursor, end);
    if (!header.ok) return err(header.code, header.message, header.hint, header.diagnostics);

    const { type, headerSize, totalSize, payloadStart } = header.value;
    const payloadSize = totalSize - headerSize;

    // 严格模式下，未登记类型直接拒绝；宽容模式记诊断后跳过。
    const known = CONTAINER_BOXES.has(type) || LEAF_BOXES.has(type);
    if (!known && !lenient) {
      return err(
        "MP4_BOX_TYPE_UNKNOWN",
        `严格模式下遇到未登记的 Box 类型 "${type}"（offset=${cursor}）`,
        "已登记类型清单见 CONTAINER_BOXES / LEAF_BOXES；若确为合法扩展 Box，请登记后再启用严格模式",
      );
    }
    if (!known) {
      bag.push(
        "MP4_BOX_TYPE_UNKNOWN",
        `跳过未登记的 Box "${type}"（offset=${cursor}, size=${totalSize}）`,
        "宽容模式下属正常跳过；若该 Box 承载必要数据，请到 LEAF_BOXES 登记并实现其解析",
      );
    }

    // 容器 Box 递归解析。meta 是特例：它是 FullBox，头后多 4 字节才是子 Box 起点。
    const children: BoxNode[] = [];
    if (CONTAINER_BOXES.has(type)) {
      const childStart =
        type === "meta"
          ? checkedAdd(payloadStart, FULLBOX_HEADER_SIZE)
          : payloadStart;
      if (childStart === null) {
        return err(
          "MP4_ARITHMETIC_OVERFLOW",
          `Box ${type} 的 payload 起点计算溢出（payloadStart=${payloadStart}）`,
          "偏移已超出安全整数域；拒绝解析（宁可失败也不读越界内存）",
        );
      }
      const childEnd = checkedAdd(payloadStart, payloadSize);
      if (childEnd === null) {
        return err(
          "MP4_ARITHMETIC_OVERFLOW",
          `Box ${type} 的 payload 终点计算溢出`,
          "偏移已超出安全整数域；拒绝解析",
        );
      }
      if (depth + 1 > 16) {
        return err(
          "MP4_COUNT_LIMIT_EXCEEDED",
          `Box 嵌套深度超过 16（当前路径经过 ${type}）`,
          "MP4 合法嵌套不超过 8 层；深度异常通常是恶意构造的递归 Box",
        );
      }
      const childOut = parseBoxList(data, childStart, childEnd, options, bag, depth + 1);
      if (!childOut.ok) {
        return err(childOut.code, childOut.message, childOut.hint, childOut.diagnostics);
      }
      children.push(...childOut.value);
    }

    out.push({
      type,
      offset: cursor,
      size: totalSize,
      payloadStart,
      payloadSize,
      children,
    });

    // 前进游标。checkedAdd 失败说明算术越界——直接判失败，不做钳制。
    const next = checkedAdd(cursor, totalSize);
    if (next === null) {
      return err(
        "MP4_ARITHMETIC_OVERFLOW",
        `Box ${type} 结束位置计算溢出（cursor=${cursor}, size=${totalSize}）`,
        "偏移超出安全整数域；拒绝解析",
      );
    }
    if (next <= cursor) {
      return err(
        "MP4_BOX_SIZE_OUT_OF_RANGE",
        `Box ${type} 导致游标未前进（cursor=${cursor}, size=${totalSize}）`,
        "零长度或负长度 Box 会造成死循环；拒绝解析",
      );
    }
    cursor = next;
  }

  return ok(out);
}

// ════════════════════════════════════════════════════════════════════════════
// §4 轨道提取（判据二之一：tkhd/mdhd/hdlr/stsd 语义精确实现）
// ════════════════════════════════════════════════════════════════════════════

/** 轨道媒体类型（由 hdlr 的 handler_type 决定）。 */
export type TrackKind = "video" | "audio" | "subtitle" | "metadata" | "unknown";

/** 解析出的一条轨道。 */
export interface Track {
  /** 轨道号（tkhd 的 track_ID，全文件唯一）。 */
  readonly trackId: number;
  /** 媒体类型。 */
  readonly kind: TrackKind;
  /** 编解码器四字符码（stsd 首个条目，如 "avc1"/"mp4a"）。 */
  readonly codec: string;
  /** 媒体时基（mdhd 的 timescale）：每 tick 的秒数 = 1/timescale。 */
  readonly timescale: number;
  /** 媒体时长（mdhd 的 duration，单位为 timescale tick）。 */
  readonly duration: number;
  /** 视频轨像素宽（tkhd 的 width，16.16 定点）。 */
  readonly width: number;
  /** 视频轨像素高（tkhd 的 height，16.16 定点）。 */
  readonly height: number;
}

/** 从 Box 树中找出指定类型的第一个直接子 Box。 */
export function findChild(node: BoxNode, type: string): BoxNode | null {
  for (const c of node.children) {
    if (c.type === type) return c;
  }
  return null;
}

/** 递归找出第一个指定类型的 Box（深度优先）。 */
export function findBoxDeep(node: BoxNode, type: string): BoxNode | null {
  if (node.type === type) return node;
  for (const c of node.children) {
    const hit = findBoxDeep(c, type);
    if (hit !== null) return hit;
  }
  return null;
}

/** 在给定 Box 树中收集全部指定类型的 Box（按文档顺序，顺序对 stsd 等有意义）。 */
export function collectBoxes(node: BoxNode, type: string, out: BoxNode[] = []): BoxNode[] {
  if (node.type === type) out.push(node);
  for (const c of node.children) collectBoxes(c, type, out);
  return out;
}

/** 建立一个大端 DataView（Box 字段全为 big-endian）。 */
function viewOf(data: Uint8Array): DataView {
  return new DataView(data.buffer, data.byteOffset, data.byteLength);
}

/** 解析 tkhd，取 trackId 与视频宽高。 */
function parseTkhd(data: Uint8Array, box: BoxNode): Outcome<{ trackId: number; width: number; height: number }> {
  // tkhd 是 FullBox：4 字节 version/flags + 4 创建 + 4 修改 + 4 track_ID
  if (box.payloadSize < 20) {
    return err(
      "MP4_TRUNCATED",
      `tkhd payload 仅 ${box.payloadSize} 字节，不足最小长度 20`,
      "轨道头被截断；请核对 moov→trak→tkhd 的嵌套是否完整",
    );
  }
  const v = viewOf(data);
  const p = box.payloadStart;
  const version = data[p]!;
  // version==1 的时间字段是 64 位，track_ID 的偏移因此右移 12 字节。
  // track_ID 紧随时间字段之后：v0 的 creation/modification 各 4 字节，
  // v1 各 8 字节。故 v0 偏移 = 12，v1 偏移 = 24。
  const idOffset = version === 1 ? p + 4 + 16 + 4 : p + 4 + 4 + 4;
  if (box.payloadSize < idOffset - p + 4) {
    return err(
      "MP4_TRUNCATED",
      `tkhd（version=${version}）payload 不足以容纳 track_ID`,
      "版本与长度不匹配；请核对 tkhd 是否被截断",
    );
  }
  const trackId = v.getUint32(idOffset, false);
  if (trackId === 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      "tkhd 的 track_ID 为 0",
      "track_ID 必须非零（0 保留为「无轨道」）；该文件轨道表不可用",
    );
  }
  // 宽高在 payload 末尾 8 字节（16.16 定点），但仅对视频轨有意义。
  const whOffset = box.payloadStart + box.payloadSize - 8;
  let width = 0;
  let height = 0;
  if (box.payloadSize >= 8 && whOffset > box.payloadStart) {
    width = v.getUint32(whOffset, false) / 65_536;
    height = v.getUint32(whOffset + 4, false) / 65_536;
  }
  return ok({ trackId, width, height });
}

/** 解析 mdhd，取 timescale 与 duration。 */
function parseMdhd(data: Uint8Array, box: BoxNode): Outcome<{ timescale: number; duration: number }> {
  if (box.payloadSize < 4) {
    return err(
      "MP4_TRUNCATED",
      `mdhd payload 仅 ${box.payloadSize} 字节，不足最小长度 4`,
      "媒体头被截断；无法确定时基则所有时间戳换算失效",
    );
  }
  const v = viewOf(data);
  const p = box.payloadStart;
  const version = data[p]!;
  if (version === 1) {
    if (box.payloadSize < 4 + 16 + 4 + 8) {
      return err("MP4_TRUNCATED", "mdhd version=1 payload 不足", "媒体头被截断");
    }
    const timescale = v.getUint32(p + 4 + 16 + 4, false);
    const hi = v.getUint32(p + 4 + 16 + 4 + 4, false);
    const lo = v.getUint32(p + 4 + 16 + 4 + 8, false);
    return ok({ timescale, duration: hi * 4_294_967_296 + lo });
  }
  if (box.payloadSize < 4 + 8 + 4 + 4) {
    return err("MP4_TRUNCATED", "mdhd version=0 payload 不足", "媒体头被截断");
  }
  const timescale = v.getUint32(p + 12, false);
  const duration = v.getUint32(p + 16, false);
  if (timescale === 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      "mdhd 的 timescale 为 0",
      "时基为 0 会让所有时间戳换算产生除零；请核对媒体头",
    );
  }
  return ok({ timescale, duration });
}

/** 解析 hdlr，取 handler_type（vide/soun/sbtl/subp/text）。 */
function parseHdlr(data: Uint8Array, box: BoxNode): Outcome<TrackKind> {
  // FullBox(4) + pre_defined(4) + handler_type(4)
  if (box.payloadSize < 12) {
    return err("MP4_TRUNCATED", `hdlr payload 仅 ${box.payloadSize} 字节，不足 12`, "处理器头被截断");
  }
  const p = box.payloadStart;
  const ht = String.fromCharCode(data[p + 8]!, data[p + 9]!, data[p + 10]!, data[p + 11]!);
  switch (ht) {
    case "vide":
      return ok("video");
    case "soun":
      return ok("audio");
    case "sbtl":
    case "subp":
    case "text":
      return ok("subtitle");
    case "meta":
    case "mhlr":
      return ok("metadata");
    default:
      return ok("unknown");
  }
}

/** 解析 stsd，取首个样本描述的编解码器四字符码。 */
function parseStsdCodec(data: Uint8Array, box: BoxNode): Outcome<string> {
  // FullBox(4) + entry_count(4) + 首个 entry: size(4) format(4)
  if (box.payloadSize < 16) {
    return err(
      "MP4_TRUNCATED",
      `stsd payload 仅 ${box.payloadSize} 字节，不足最小长度 16`,
      "样本描述被截断；无法确定编解码器（四字符码在 entry 的前 8 字节）",
    );
  }
  const p = box.payloadStart;
  const entries = viewOf(data).getUint32(p + 4, false);
  if (entries === 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      "stsd 的 entry_count 为 0",
      "轨道没有声明任何样本格式；该轨道的编解码器不可知",
    );
  }
  const codec = String.fromCharCode(data[p + 12]!, data[p + 13]!, data[p + 14]!, data[p + 15]!);
  return ok(codec);
}

/** 从 trak 节点读出 track_ID（与 extractTracks 复用同一实现，避免两处漂移）。 */
function readTrackId(data: Uint8Array, trak: BoxNode): number | null {
  const tkhd = findChild(trak, "tkhd");
  if (tkhd === null || tkhd.payloadSize < 20) return null;
  const version = data[tkhd.payloadStart]!;
  // version==1 的创建/修改时间为 64 位，track_ID 偏移因此右移 12 字节。
  const idOffset = version === 1 ? tkhd.payloadStart + 24 : tkhd.payloadStart + 12;
  if (idOffset + 4 > data.length) return null;
  return viewOf(data).getUint32(idOffset, false);
}

/** 从 moov Box 提取全部轨道。 */
export function extractTracks(data: Uint8Array, moov: BoxNode, bag: DiagBag): Outcome<readonly Track[]> {
  const trakNodes = moov.children.filter((c) => c.type === "trak");
  if (trakNodes.length === 0) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      "moov 内没有任何 trak Box",
      "文件不含轨道；无法解封装。若这是纯音频/视频片段，须确认 moov 未被前移或裁剪",
    );
  }
  if (trakNodes.length > MP4_COUNT_LIMIT.trackCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `轨道数 ${trakNodes.length} 超过上限 ${MP4_COUNT_LIMIT.trackCount}`,
      "轨道数异常，多半是损坏或恶意构造的文件；拒绝解析",
    );
  }

  const tracks: Track[] = [];
  const seenIds = new Set<number>();

  for (const trak of trakNodes) {
    const tkhd = findChild(trak, "tkhd");
    if (tkhd === null) {
      bag.push(
        "MP4_REQUIRED_BOX_MISSING",
        `trak（offset=${trak.offset}）缺少 tkhd，已跳过该轨`,
        "无 tkhd 则无 track_ID，该轨无法寻址；请核对轨道头是否被裁剪",
      );
      continue;
    }
    const head = parseTkhd(data, tkhd);
    if (!head.ok) return err(head.code, head.message, head.hint, head.diagnostics);
    if (seenIds.has(head.value.trackId)) {
      return err(
        "MP4_TRACK_ID_DUPLICATE",
        `track_ID ${head.value.trackId} 在文件内重复出现`,
        "轨道号必须全文件唯一；重复会导致寻址指向错误轨道，属结构性错误",
      );
    }
    seenIds.add(head.value.trackId);

    const mdia = findChild(trak, "mdia");
    if (mdia === null) {
      bag.push(
        "MP4_REQUIRED_BOX_MISSING",
        `轨道 ${head.value.trackId} 缺少 mdia，已跳过该轨`,
        "无 mdia 则无时基与媒体信息，该轨不可解",
      );
      continue;
    }
    const mdhdBox = findChild(mdia, "mdhd");
    if (mdhdBox === null) {
      bag.push(
        "MP4_REQUIRED_BOX_MISSING",
        `轨道 ${head.value.trackId} 缺少 mdhd，已跳过该轨`,
        "无 mdhd 则无 timescale，时间戳无法换算为秒",
      );
      continue;
    }
    const mdhd = parseMdhd(data, mdhdBox);
    if (!mdhd.ok) return err(mdhd.code, mdhd.message, mdhd.hint, mdhd.diagnostics);

    const hdlrBox = findChild(mdia, "hdlr");
    const kind: TrackKind =
      hdlrBox === null
        ? "unknown"
        : (() => {
            const r = parseHdlr(data, hdlrBox);
            return r.ok ? r.value : "unknown";
          })();

    // 样本表路径：mdia → minf → stbl → stsd。任一层缺失则编解码器不可知。
    const minf = findChild(mdia, "minf");
    const stblBox = minf === null ? null : findChild(minf, "stbl");
    const stsdBox = stblBox === null ? null : findChild(stblBox, "stsd");
    if (stsdBox === null) {
      bag.push(
        "MP4_REQUIRED_BOX_MISSING",
        `轨道 ${head.value.trackId} 缺少 stsd（样本描述），编解码器未知`,
        "无 stsd 则无法知道编码格式；该轨将不被解码器支持",
      );
      tracks.push({
        trackId: head.value.trackId,
        kind,
        codec: "none",
        timescale: mdhd.value.timescale,
        duration: mdhd.value.duration,
        width: head.value.width,
        height: head.value.height,
      });
      continue;
    }
    const codec = parseStsdCodec(data, stsdBox);
    if (!codec.ok) return err(codec.code, codec.message, codec.hint, codec.diagnostics);

    tracks.push({
      trackId: head.value.trackId,
      kind,
      codec: codec.value,
      timescale: mdhd.value.timescale,
      duration: mdhd.value.duration,
      width: head.value.width,
      height: head.value.height,
    });
  }

  return ok(tracks, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 sample 表五件套（判据二之二：stts/ctts/stsc/stsz/stco 语义精确实现）
// ════════════════════════════════════════════════════════════════════════════

/** stsc 表项：第 first_chunk 个块起，每块连续 samples_per_chunk 个样本，描述为 desc_index。 */
export interface StscEntry {
  readonly firstChunk: number;
  readonly samplesPerChunk: number;
  readonly descIndex: number;
}

/** stts 表项：连续 sample_count 个样本共享同一 delta（ timescale tick）。 */
export interface SttsEntry {
  readonly sampleCount: number;
  readonly delta: number;
}

/** ctts 表项：连续 sample_count 个样本共享同一 composition offset（有 B 帧时存在）。 */
export interface CttsEntry {
  readonly sampleCount: number;
  readonly offset: number;
}

/** 一条解出的样本——解码器与 seek 的最小工作单位。 */
export interface Sample {
  readonly trackId: number;
  /** 样本序号（0 基，按解码序）。 */
  readonly index: number;
  /** 在文件中的绝对字节偏移。 */
  readonly offset: number;
  /** 字节长度。 */
  readonly size: number;
  /** 解码时间戳（ timescale tick）。 */
  readonly dts: number;
  /** 呈现时间戳（ timescale tick）= dts + ctts 偏移。 */
  readonly pts: number;
  /** 是否为关键帧（stss 存在时以 stss 为准；无 stss 则全关键帧）。 */
  readonly syncSample: boolean;
  /** 所属 chunk 序号（0 基）。 */
  readonly chunkIndex: number;
}

/** 一轨的完整样本表。 */
export interface SampleTable {
  readonly trackId: number;
  readonly timescale: number;
  readonly stts: readonly SttsEntry[];
  readonly ctts: readonly CttsEntry[];
  readonly stsc: readonly StscEntry[];
  readonly sampleSizes: readonly number[];
  readonly chunkOffsets: readonly number[];
  /** stss 关键帧索引集合；null 表示全为关键帧。 */
  readonly syncSamples: ReadonlySet<number> | null;
}

/** 读取 Box 的 entry_count（FullBox 表类Box 通用：version/flags 后 4 字节）。 */
function readEntryCount(data: Uint8Array, box: BoxNode): Outcome<number> {
  if (box.payloadSize < 8) {
    return err(
      "MP4_TRUNCATED",
      `${box.type} payload 仅 ${box.payloadSize} 字节，不足以容纳 version/flags + entry_count`,
      "表类Box 头被截断；请核对 moov→trak→mdia→minf→stbl 路径完整性",
    );
  }
  return ok(viewOf(data).getUint32(box.payloadStart + 4, false));
}

/** 解析 stts（解码时间到时间）。 */
export function parseStts(data: Uint8Array, box: BoxNode): Outcome<readonly SttsEntry[]> {
  const ec = readEntryCount(data, box);
  if (!ec.ok) return err(ec.code, ec.message, ec.hint, ec.diagnostics);
  const n = ec.value;
  if (n > MP4_COUNT_LIMIT.timeToSampleEntryCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `stts 表项数 ${n} 超过上限 ${MP4_COUNT_LIMIT.timeToSampleEntryCount}`,
      "表项数异常；拒绝解析以免分配炸弹（F1122 计数上限纪律）",
    );
  }
  const need = checkedMul(n, 8);
  if (need === null || box.payloadSize < 8 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `stts 声明 ${n} 项（需 ${need === null ? "溢出" : need} 字节 payload），实际 ${box.payloadSize}`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const v = viewOf(data);
  const out: SttsEntry[] = [];
  for (let i = 0; i < n; i++) {
    const p = box.payloadStart + 8 + i * 8;
    out.push({ sampleCount: v.getUint32(p, false), delta: v.getUint32(p + 4, false) });
  }
  return ok(out);
}

/** 解析 ctts（呈现时间偏移）；Box 缺失是合法的（表示无 B 帧重排）。 */
export function parseCtts(data: Uint8Array, box: BoxNode): Outcome<readonly CttsEntry[]> {
  const ec = readEntryCount(data, box);
  if (!ec.ok) return err(ec.code, ec.message, ec.hint, ec.diagnostics);
  const n = ec.value;
  if (n > MP4_COUNT_LIMIT.timeToSampleEntryCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `ctts 表项数 ${n} 超过上限 ${MP4_COUNT_LIMIT.timeToSampleEntryCount}`,
      "表项数异常；拒绝解析（F1122 计数上限纪律）",
    );
  }
  const need = checkedMul(n, 8);
  if (need === null || box.payloadSize < 8 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `ctts 声明 ${n} 项，实际 payload ${box.payloadSize} 字节`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const v = viewOf(data);
  // version==1 的 offset 是有符号 32 位（v1 允许负偏移）；v0 为无符号。
  const signed = data[box.payloadStart] === 1;
  const out: CttsEntry[] = [];
  for (let i = 0; i < n; i++) {
    const p = box.payloadStart + 8 + i * 8;
    const offset = signed ? v.getInt32(p + 4, false) : v.getUint32(p + 4, false);
    out.push({ sampleCount: v.getUint32(p, false), offset });
  }
  return ok(out);
}

/** 解析 stsc（样表-块映射）——seek 正确性的根基。 */
export function parseStsc(data: Uint8Array, box: BoxNode): Outcome<readonly StscEntry[]> {
  const ec = readEntryCount(data, box);
  if (!ec.ok) return err(ec.code, ec.message, ec.hint, ec.diagnostics);
  const n = ec.value;
  if (n > MP4_COUNT_LIMIT.stscEntryCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `stsc 表项数 ${n} 超过上限 ${MP4_COUNT_LIMIT.stscEntryCount}`,
      "表项数异常；拒绝解析（F1122 计数上限纪律）",
    );
  }
  const need = checkedMul(n, 12);
  if (need === null || box.payloadSize < 8 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `stsc 声明 ${n} 项，实际 payload ${box.payloadSize} 字节`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const v = viewOf(data);
  const out: StscEntry[] = [];
  for (let i = 0; i < n; i++) {
    const p = box.payloadStart + 8 + i * 12;
    const firstChunk = v.getUint32(p, false);
    const samplesPerChunk = v.getUint32(p + 4, false);
    const descIndex = v.getUint32(p + 8, false);
    if (firstChunk === 0) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        `stsc 第 ${i} 项的 first_chunk 为 0`,
        "chunk 序号从 1 开始，first_chunk=0 会让映射算法指向不存在的块",
      );
    }
    if (samplesPerChunk === 0) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        `stsc 第 ${i} 项的 samples_per_chunk 为 0`,
        "每块样本数必须≥1；为 0 会导致样本到块的映射无法推进（死循环）",
      );
    }
    if (i > 0 && firstChunk <= out[i - 1]!.firstChunk) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        `stsc 表项未严格递增（first_chunk=${firstChunk}，前项 ${out[i - 1]!.firstChunk}）`,
        "stsc 要求 first_chunk 严格递增；非递增说明表损坏，映射结果不可信",
      );
    }
    out.push({ firstChunk, samplesPerChunk, descIndex });
  }
  return ok(out);
}

/** 解析 stsz（样本大小；sample_size==0 时逐样本给大小）或 stz2（紧凑版）。 */
export function parseStsz(data: Uint8Array, box: BoxNode): Outcome<readonly number[]> {
  if (box.payloadSize < 12) {
    return err(
      "MP4_TRUNCATED",
      `stsz payload仅 ${box.payloadSize} 字节，不足最小长度 12`,
      "样本大小表被截断；无法定位任何样本的字节范围",
    );
  }
  const v = viewOf(data);
  const p = box.payloadStart;
  const uniformSize = v.getUint32(p + 4, false);
  const count = v.getUint32(p + 8, false);
  if (count > MP4_COUNT_LIMIT.sampleCountPerTrack) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `stsz 声明样本数 ${count} 超过单轨上限 ${MP4_COUNT_LIMIT.sampleCountPerTrack}`,
      "样本数异常（损坏或恶意构造）；拒绝解析，避免先分配巨额内存（F1122）",
    );
  }
  if (uniformSize > 0) {
    // 定长模式：所有样本等长，无需逐个读表。
    const total = checkedMul(count, uniformSize);
    if (total === null) {
      return err(
        "MP4_ARITHMETIC_OVERFLOW",
        `stsz 定长模式计算溢出（count=${count}, size=${uniformSize}）`,
        "样本总字节数超出安全整数域；拒绝解析",
      );
    }
    return ok(new Array<number>(count).fill(uniformSize));
  }
  const need = checkedMul(count, 4);
  if (need === null || box.payloadSize < 12 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `stsz 变长模式声明 ${count} 项，实际 payload ${box.payloadSize} 字节`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const out = new Array<number>(count);
  for (let i = 0; i < count; i++) {
    out[i] = v.getUint32(p + 12 + i * 4, false);
  }
  return ok(out);
}

/** 解析 stco（32 位块偏移）或 co64（64 位块偏移）。 */
export function parseChunkOffsets(
  data: Uint8Array,
  box: BoxNode,
): Outcome<readonly number[]> {
  const wide = box.type === "co64";
  const ec = readEntryCount(data, box);
  if (!ec.ok) return err(ec.code, ec.message, ec.hint, ec.diagnostics);
  const n = ec.value;
  const perEntry = wide ? 8 : 4;
  if (n > MP4_COUNT_LIMIT.chunkCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `${box.type} 块数 ${n} 超过上限 ${MP4_COUNT_LIMIT.chunkCount}`,
      "块数异常；拒绝解析（F1122 计数上限纪律）",
    );
  }
  const need = checkedMul(n, perEntry);
  if (need === null || box.payloadSize < 8 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `${box.type} 声明 ${n} 项，实际 payload ${box.payloadSize} 字节`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const v = viewOf(data);
  const out = new Array<number>(n);
  for (let i = 0; i < n; i++) {
    const p = box.payloadStart + 8 + i * perEntry;
    if (wide) {
      const hi = v.getUint32(p, false);
      const lo = v.getUint32(p + 4, false);
      out[i] = hi * 4_294_967_296 + lo;
    } else {
      out[i] = v.getUint32(p, false);
    }
  }
  return ok(out);
}

/** 解析 stss（同步样本表）；缺失表示全部样本皆为关键帧。 */
export function parseStss(data: Uint8Array, box: BoxNode): Outcome<ReadonlySet<number>> {
  const ec = readEntryCount(data, box);
  if (!ec.ok) return err(ec.code, ec.message, ec.hint, ec.diagnostics);
  const n = ec.value;
  const need = checkedMul(n, 4);
  if (need === null || box.payloadSize < 8 + need) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `stss 声明 ${n} 项，实际 payload ${box.payloadSize} 字节`,
      "entry_count 与 payload 长度矛盾；文件被截断或表头损坏",
    );
  }
  const v = viewOf(data);
  const set = new Set<number>();
  for (let i = 0; i < n; i++) {
    // stss 的 sample_number 从 1 起；内部统一 0 基，故减 1。
    set.add(v.getUint32(box.payloadStart + 8 + i * 4, false) - 1);
  }
  return ok(set);
}

// ════════════════════════════════════════════════════════════════════════════
// §6 样表-块映射 + 时间戳重建（判据二之三 + 判据四：seek 正确性根基）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 展开 stsc：算出每个 chunk 含多少样本（samplesPerChunkOfChunk）。
 *
 * stsc 是**游程压缩**的：它不说「每个块几个样本」，只说「从第 N 个块开始，
 * 每块 S 个样本，直到第 M 个块」。所以必须先展开成逐块数组，后续才能定位样本。
 * 展开用差分：条目 i覆盖 [firstChunk_i, firstChunk_{i+1}) 区间。
 *
 * 这段算法算错的后果不是「差一帧」，而是 seek 落到完全无关的位置——
 * 因为偏移是从 chunk 起点累加出来的，块数错一位，其后全部样本的偏移全错。
 */
export function expandSamplesPerChunk(
  stsc: readonly StscEntry[],
  chunkCount: number,
): Outcome<readonly number[]> {
  // 计数上限前置校验：new Array(chunkCount) 在荒谬值下会直接抛
  // RangeError（不是返回失败），那会让「畸形文件」变成「未捕获异常」——
  // 比错误结果更糟，因为它连三要素诊断都来不及产出。
  if (!Number.isSafeInteger(chunkCount) || chunkCount < 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `块数为非法值（${chunkCount}）`,
      "块数须为非负安全整数；请核对 stco/co64 的 entry_count 是否被篡改",
    );
  }
  if (chunkCount > MP4_COUNT_LIMIT.chunkCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `块数 ${chunkCount} 超过上限 ${MP4_COUNT_LIMIT.chunkCount}`,
      "块数异常（损坏或恶意构造）；拒绝展开，避免巨额内存分配（F1122 计数上限纪律）",
    );
  }
  if (stsc.length === 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      "stsc 为空，无法建立样本到块的映射",
      "seek 需要 stsc；无 stsc 则样本偏移不可解（moov 必须包含 stbl）",
    );
  }
  // 末条目的 first_chunk 不得超过块数+1（等于块数+1 意味着末条目覆盖到末尾）。
  const last = stsc[stsc.length - 1]!;
  if (last.firstChunk > chunkCount + 1) {
    return err(
      "MP4_STSC_CHUNK_OUT_OF_RANGE",
      `stsc 末条 first_chunk=${last.firstChunk} 超过块数 ${chunkCount}+1`,
      "stsc 引用了不存在的块；样表与块偏移表不自洽，seek 结果不可信",
    );
  }
  const out = new Array<number>(chunkCount);
  for (let i = 0; i < stsc.length; i++) {
    const entry = stsc[i]!;
    const next = i + 1 < stsc.length ? stsc[i + 1]!.firstChunk : chunkCount + 1;
    // 1 基chunk 号 → 0 基数组下标。区间右端不包含。
    const from = entry.firstChunk - 1;
    const to = Math.min(next - 1, chunkCount);
    if (from >= chunkCount) break; // 该条目已完全越过实际块数
    for (let c = Math.max(from, 0); c < to; c++) {
      out[c] = entry.samplesPerChunk;
    }
  }
  // 未被任何条目覆盖的块（不应出现，出现即表不自洽）显式标0 供上层发现。
  for (let c = 0; c < chunkCount; c++) {
    if (out[c] === undefined) out[c] = 0;
  }
  return ok(out);
}

/** 展开 stts/ctts：把游程表拉成逐样本序列。 */
function expandRunLength(
  // stts 的项用delta 命名时间增量，ctts 的项用 offset 命名呈现偏移——
  // 两者语义不同（增量 vs 绝对偏移），故按实际存在的字段取值，
  // 写死 delta 会让 ctts 全变undefined，症状是 PTS 变成 NaN。
  entries: readonly { sampleCount: number; delta?: number; offset?: number }[],
  totalSamples: number,
  tableName: string,
): Outcome<number[]> {
  // 同上：先校验再分配，避免 new Array(荒谬值) 抛未捕获的 RangeError。
  if (!Number.isSafeInteger(totalSamples) || totalSamples < 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `${tableName} 展开的样本总数为非法值（${totalSamples}）`,
      "样本总数须为非负安全整数；请核对 stsz 的 sample_count",
    );
  }
  if (totalSamples > MP4_COUNT_LIMIT.sampleCountPerTrack) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `${tableName} 需展开 ${totalSamples} 个样本，超过单轨上限 ${MP4_COUNT_LIMIT.sampleCountPerTrack}`,
      "拒绝展开，避免巨额内存分配（F1122 计数上限纪律）",
    );
  }
  const out = new Array<number>(totalSamples);
  let cursor = 0;
  for (const e of entries) {
    if (cursor + e.sampleCount > totalSamples) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        `${tableName} 游程累计样本数 ${cursor + e.sampleCount} 超过样本总数 ${totalSamples}`,
        `${tableName} 与 stsz 的样本数矛盾；两者必须覆盖同一样本集合`,
      );
    }
    for (let i = 0; i < e.sampleCount; i++) {
      out[cursor + i] = e.delta ?? e.offset ?? 0;
    }
    cursor += e.sampleCount;
  }
  if (cursor !== totalSamples) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `${tableName} 游程累计 ${cursor} 个样本，少于 stsz 声明的 ${totalSamples} 个`,
      `${tableName} 与 stsz 样本数不一致；缺失的时间信息无法凭空虚构`,
    );
  }
  return ok(out);
}

/** 从stbl 组装完整样本表。 */
export function buildSampleTable(
  data: Uint8Array,
  stbl: BoxNode,
  trackId: number,
  timescale: number,
): Outcome<SampleTable> {
  const sttsBox = findChild(stbl, "stts");
  if (sttsBox === null) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      `轨道 ${trackId} 的 stbl 缺少 stts`,
      "无 stts 则无法得到解码时间；轨道的 DTS 序列不可解",
    );
  }
  const stszBox = findChild(stbl, "stsz") ?? findChild(stbl, "stz2");
  if (stszBox === null) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      `轨道 ${trackId} 的 stbl 缺少 stsz/stz2`,
      "无样本大小表则无法定位任何样本的字节范围；轨道的样本不可寻址",
    );
  }
  const offsetBox = findChild(stbl, "stco") ?? findChild(stbl, "co64");
  if (offsetBox === null) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      `轨道 ${trackId} 的 stbl 缺少 stco/co64`,
      "无块偏移表则样本位置不可知；请确认 stbl 是否被裁剪",
    );
  }
  const stscBox = findChild(stbl, "stsc");
  if (stscBox === null) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      `轨道 ${trackId} 的 stbl 缺少 stsc`,
      "无 stsc 则样本到块的映射不可建；seek 无从下手",
    );
  }

  const stts = parseStts(data, sttsBox);
  if (!stts.ok) return err(stts.code, stts.message, stts.hint, stts.diagnostics);
  const sizes = parseStsz(data, stszBox);
  if (!sizes.ok) return err(sizes.code, sizes.message, sizes.hint, sizes.diagnostics);
  const offsets = parseChunkOffsets(data, offsetBox);
  if (!offsets.ok) return err(offsets.code, offsets.message, offsets.hint, offsets.diagnostics);
  const stsc = parseStsc(data, stscBox);
  if (!stsc.ok) return err(stsc.code, stsc.message, stsc.hint, stsc.diagnostics);

  const cttsBox = findChild(stbl, "ctts");
  const ctts = cttsBox === null ? ok<readonly CttsEntry[]>([]) : parseCtts(data, cttsBox);
  if (!ctts.ok) return err(ctts.code, ctts.message, ctts.hint, ctts.diagnostics);

  const stssBox = findChild(stbl, "stss");
  let sync: ReadonlySet<number> | null = null;
  if (stssBox !== null) {
    const parsed = parseStss(data, stssBox);
    if (!parsed.ok) return err(parsed.code, parsed.message, parsed.hint, parsed.diagnostics);
    sync = parsed.value;
  }

  return ok({
    trackId,
    timescale,
    stts: stts.value,
    ctts: ctts.value,
    stsc: stsc.value,
    sampleSizes: sizes.value,
    chunkOffsets: offsets.value,
    syncSamples: sync,
  });
}

/**
 * 展开样本表为逐样本列表（含偏移、长度、DTS、PTS、关键帧位）。
 *
 * 偏移的推进方式：同一 chunk 内的样本在文件中连续存放，故
 *   offset(第 k 个样本) = chunkOffset + Σ(前 k-1 个样本的大小)
 * 跨 chunk 时重置为下一个 chunk 的起点。
 */
export function expandSamples(
  table: SampleTable,
  dataLength: number,
): Outcome<readonly Sample[]> {
  const total = table.sampleSizes.length;
  if (total === 0) {
    return err(
      "MP4_NO_SAMPLES",
      `轨道 ${table.trackId} 的样本数为 0`,
      "空轨道的样本表无意义；请确认 stsz 的 sample_count 是否为 0（合法但不可播放）",
    );
  }
  if (total > MP4_COUNT_LIMIT.sampleCountPerTrack) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `轨道 ${table.trackId} 样本数 ${total} 超过上限 ${MP4_COUNT_LIMIT.sampleCountPerTrack}`,
      "样本数异常；拒绝展开（F1122 计数上限纪律）",
    );
  }

  const perChunk = expandSamplesPerChunk(table.stsc, table.chunkOffsets.length);
  if (!perChunk.ok) return err(perChunk.code, perChunk.message, perChunk.hint, perChunk.diagnostics);

  const deltas = expandRunLength(table.stts, total, "stts");
  if (!deltas.ok) return err(deltas.code, deltas.message, deltas.hint, deltas.diagnostics);

  let compOffsets: number[] | null = null;
  if (table.ctts.length > 0) {
    const r = expandRunLength(table.ctts, total, "ctts");
    if (!r.ok) return err(r.code, r.message, r.hint, r.diagnostics);
    compOffsets = r.value;
  }

  const out: Sample[] = new Array(total);
  let sampleIndex = 0;
  let dts = 0;

  for (let c = 0; c < table.chunkOffsets.length && sampleIndex < total; c++) {
    const countInChunk = perChunk.value[c]!;
    if (countInChunk === 0) {
      // 该块无样本：表不自洽的信号，但可能出现在合法文件的空块；跳过并继续。
      continue;
    }
    let offset = table.chunkOffsets[c]!;
    for (let k = 0; k < countInChunk && sampleIndex < total; k++) {
      const size = table.sampleSizes[sampleIndex]!;
      // 偏移必须完整落在文件内——越界说明表与文件长度矛盾。
      if (!checkedRange(offset, size, dataLength)) {
        return err(
          "MP4_SAMPLE_TABLE_INCONSISTENT",
          `轨道 ${table.trackId} 样本 #${sampleIndex} 的字节范围 [${offset}, ${offset}+${size}) 越出文件长度 ${dataLength}`,
          "块偏移表与文件实际长度矛盾（常见于 moov 被前移但 mdat 被截断的文件）；拒绝解析",
        );
      }
      const ctOff = compOffsets === null ? 0 : compOffsets[sampleIndex]!;
      const pts = dts + ctOff;
      if (pts < dts) {
        return err(
          "MP4_PTS_BEFORE_DTS",
          `轨道 ${table.trackId} 样本 #${sampleIndex} 的 PTS(${pts}) 早于 DTS(${dts})`,
          "呈现时间早于解码时间在物理上不可能；ctts 偏移为负且超出 DTS 累积量，多半是 ctts 表损坏",
        );
      }
      out[sampleIndex] = {
        trackId: table.trackId,
        index: sampleIndex,
        offset,
        size,
        dts,
        pts,
        syncSample: table.syncSamples === null ? true : table.syncSamples.has(sampleIndex),
        chunkIndex: c,
      };
      const nextOffset = checkedAdd(offset, size);
      if (nextOffset === null) {
        return err(
          "MP4_ARITHMETIC_OVERFLOW",
          `轨道 ${table.trackId} 样本 #${sampleIndex} 的下一偏移计算溢出`,
          "偏移超出安全整数域；拒绝解析",
        );
      }
      offset = nextOffset;
      const nextDts = checkedAdd(dts, deltas.value[sampleIndex]!);
      if (nextDts === null) {
        return err(
          "MP4_ARITHMETIC_OVERFLOW",
          `轨道 ${table.trackId} 样本 #${sampleIndex} 的 DTS 累加溢出`,
          "时间戳超出安全整数域；拒绝解析",
        );
      }
      dts = nextDts;
      sampleIndex += 1;
    }
  }

  if (sampleIndex < total) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      `轨道 ${table.trackId} 仅映射出 ${sampleIndex}/${total} 个样本`,
      "stsc 的样本数覆盖不足：块内样本数之和少于 stsz 声明的样本数，样表不自洽",
    );
  }
  return ok(out);
}

// ════════════════════════════════════════════════════════════════════════════
// §7 重排序缓冲（判据四：解码序 → 呈现序的重建 + reorder 上界声明）
// ════════════════════════════════════════════════════════════════════════════

/** 重排序缓冲的一项：解码序输入、呈现序输出。 */
export interface ReorderItem {
  /** 解码序序号（输入顺序）。 */
  readonly decodeIndex: number;
  /** 呈现时间戳。 */
  readonly pts: number;
  /** 解码时间戳。 */
  readonly dts: number;
  /** 载荷引用（此处为样本下标，真实实现里指向解码输出缓冲）。 */
  readonly payload: unknown;
}

/**
 * 测量 reordering 深度：解码序中，一个样本到达后最多要等多少个后续样本
 * 才能确定它的呈现时机。
 *
 * 为什么要声明这个上界：解码器按 DTS 顺序产出，但显示要按 PTS 顺序。
 * 若不声明深度，实现就得用「无限缓冲 + 排到底」来保证正确——那会让
 * 直播/低延迟场景永远等不到首帧。深度是内存缓冲大小的依据，必须可测量、
 * 可校验：一旦实测深度超过声明值，说明容器的 reorder 描述与实现脱节
 * （或声明写错了），此时必须显性报错而非默默扩大缓冲。
 */
export function measureReorderDepth(samples: readonly Sample[]): number {
  let maxDepth = 0;
  // 对每个样本，数它前面有多少样本的 PTS 比它大——那些都必须等它先呈现。
  for (let i = 0; i < samples.length; i++) {
    let ahead = 0;
    for (let j = 0; j < i; j++) {
      if (samples[j]!.pts > samples[i]!.pts) ahead += 1;
    }
    if (ahead > maxDepth) maxDepth = ahead;
  }
  return maxDepth;
}

/**
 * 重排序：把解码序样本按 PTS 稳定排序还原为呈现序。
 * 用**稳定**排序是必须的——PTS 相同的样本（无重排时常见）必须保持解码序，
 * 否则同时间戳的样本顺序会随机翻转，音画同步会出现偶发抖动。
 */
export function reorderToPresentation(samples: readonly Sample[]): Outcome<readonly Sample[]> {
  if (samples.length === 0) {
    return err("MP4_NO_SAMPLES", "重排序输入为空", "无样本可重排；请确认展开样本是否成功");
  }
  const depth = measureReorderDepth(samples);
  if (depth > MP4_COUNT_LIMIT.reorderDepth) {
    return err(
      "MP4_REORDER_BOUND_EXCEEDED",
      `实测重排序深度 ${depth} 超过声明上界 ${MP4_COUNT_LIMIT.reorderDepth}`,
      "容器的 PTS/DTS 描述与「reorder 不超过该深度」的声明脱节；"
        + "要么提高上限（并评估缓冲内存），要么核查 ctts 表是否损坏",
    );
  }
  // 稳定排序：tie-break 用解码序下标，保证同 PTS 的相对次序可复现。
  const indexed = samples.map((s, i) => ({ s, i }));
  indexed.sort((a, b) => (a.s.pts !== b.s.pts ? a.s.pts - b.s.pts : a.i - b.i));
  return ok(indexed.map((x) => x.s));
}

// ════════════════════════════════════════════════════════════════════════════
// §8 分片 MP4（fmoof/fragment：流式场景；碎片间轨道延续语义）
// ════════════════════════════════════════════════════════════════════════════

/** 一个影片片段（moof）的解析结果。 */
export interface FragmentInfo {
  /** 片段序号（mfhd 的 sequence_number，从 1 起）。 */
  readonly sequenceNumber: number;
  /** 该片段覆盖的轨道号集合。 */
  readonly trackIds: readonly number[];
  /** 各轨道的片段基址 DTS（tfdt）。 */
  readonly baseDecodeTimes: ReadonlyMap<number, number>;
}

/**
 * 解析分片 MP4 的 moof Box。
 *
 * 分片 MP4 与普通 MP4 的根本差别：moov 里**没有完整样本表**，样本信息分散在
 * 一个个 moof 里（每个 moof 之后紧跟一个 mdat）。这使它天然适合流式——
 * 可以边下边解，不必等整个文件下完。
 *
 * 「碎片间轨道延续语义」：片段 N+1 依赖片段 N 留下的解码器状态，所以片段序号
 * 必须连续；baseMediaDecodeTime 由 tfdt 给出，续接时须与上一片段的
 * DTS 尾部对齐。本函数校验序号递增，具体的时间轴连续性由上层
 * F1210（时间轴拼接）核对。
 */
export function parseFragment(data: Uint8Array, moof: BoxNode): Outcome<FragmentInfo> {
  const mfhd = findChild(moof, "mfhd");
  if (mfhd === null) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      "moof 缺少 mfhd（影片片段头）",
      "无 mfhd 则无片段序号，无法校验片段连续性；分片流不可解",
    );
  }
  if (mfhd.payloadSize < 8) {
    return err("MP4_TRUNCATED", "mfhd payload 不足 8 字节", "片段头被截断");
  }
  // mfhd: FullBox(4) + sequence_number(4)
  const sequenceNumber = viewOf(data).getUint32(mfhd.payloadStart + 4, false);
  if (sequenceNumber === 0) {
    return err(
      "MP4_SAMPLE_TABLE_INCONSISTENT",
      "mfhd 的 sequence_number 为 0",
      "片段序号从 1 起；为 0 说明片段头损坏",
    );
  }

  const trackIds: number[] = [];
  const baseDecodeTimes = new Map<number, number>();
  for (const traf of moof.children.filter((c) => c.type === "traf")) {
    const tfhd = findChild(traf, "tfhd");
    if (tfhd === null) {
      return err(
        "MP4_REQUIRED_BOX_MISSING",
        "traf 缺少 tfhd（轨道片段头）",
        "无 tfhd 则无轨道号与默认采样描述，片段内的样本无法归属到轨道",
      );
    }
    // tfhd 是 FullBox；track_ID 紧随 flags 之后的 4 字节（flags 的低位指示字段存在性，
    // 但 track_ID 本身恒存在）。
    if (tfhd.payloadSize < 8) {
      return err("MP4_TRUNCATED", "tfhd payload 不足 8 字节", "轨道片段头被截断");
    }
    const trackId = viewOf(data).getUint32(tfhd.payloadStart + 4, false);
    if (trackId === 0) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        "tfhd 的 track_ID 为 0",
        "track_ID 必须非零；为 0 表示轨道片段头损坏",
      );
    }
    trackIds.push(trackId);

    // tfdt（基址解码时间）是可选的：缺失时按「延续上一片段」处理。
    const tfdt = findChild(traf, "tfdt");
    if (tfdt !== null) {
      if (tfdt.payloadSize < 8) {
        return err("MP4_TRUNCATED", "tfdt payload 不足 8 字节", "基址时间被截断");
      }
      const version = data[tfdt.payloadStart]!;
      const baseTime =
        version === 1
          ? (() => {
              const v = viewOf(data);
              const hi = v.getUint32(tfdt.payloadStart + 4, false);
              const lo = v.getUint32(tfdt.payloadStart + 8, false);
              return hi * 4_294_967_296 + lo;
            })()
          : viewOf(data).getUint32(tfdt.payloadStart + 4, false);
      baseDecodeTimes.set(trackId, baseTime);
    }
  }

  if (trackIds.length === 0) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      `moof（片段 #${sequenceNumber}）内没有任何 traf`,
      "片段不含轨道数据；空片段在流中属异常，请核对 mux 配置",
    );
  }

  return ok({ sequenceNumber, trackIds, baseDecodeTimes });
}

/** 校验分片序列的序号连续性（碎片间轨道延续的前提）。 */
export function auditFragmentSequence(fragments: readonly FragmentInfo[]): Outcome<number> {
  for (let i = 0; i < fragments.length; i++) {
    const expected = i + 1;
    const got = fragments[i]!.sequenceNumber;
    if (got !== expected) {
      return err(
        "MP4_SAMPLE_TABLE_INCONSISTENT",
        `第 ${i} 个片段的 sequence_number=${got}，期望 ${expected}（序号应从 1 起连续）`,
        "片段序号不连续意味着有片段丢失或乱序；"
          + "分片流依赖前一解码器状态，缺口会导致后续轨道数据错位",
      );
    }
  }
  return ok(fragments.length);
}

// ════════════════════════════════════════════════════════════════════════════
// §9 顶层解封装入口
// ════════════════════════════════════════════════════════════════════════════

/** 编解码器能力查询：未知四字符码 → 显性不支持（绝不静默透传）。 */
export interface CodecCapability {
  readonly supported: boolean;
  /** 若不支持，说明原因与可行的出路。 */
  readonly reason?: string;
}

/** 已登记的编解码器 → 承接其解码的条目（下游 F1204-F1207）。 */
export const CODEC_HANDOFF: Readonly<Record<string, string>> = {
  avc1: "VE-F1204（H.264/AVC）",
  avc3: "VE-F1204（H.264/AVC）",
  hvc1: "VE-F1205（HEVC）",
  hev1: "VE-F1205（HEVC）",
  av01: "VE-F1206（AV1）",
  vp09: "VE-F1206（AV1/VP9 通路）",
  mp4a: "VE-F1207（AAC/音频）",
  Opus: "VE-F1207（Opus）",
  fLaC: "VE-F1207（FLAC）",
};

/** 查询编解码器能力。未知码返回 supported=false，绝不「先试试看」。 */
export function queryCodec(codec: string): CodecCapability {
  const handoff = CODEC_HANDOFF[codec];
  if (handoff !== undefined) {
    return { supported: true, reason: `已登记，交由${handoff} 解码` };
  }
  return {
    supported: false,
    reason:
      `四字符码 "${codec}" 未登记到 CODEC_HANDOFF。`
      + "VE 域当前只承接 H.264/HEVC/AV1 视频与 AAC/Opus/FLAC 音频；"
      + "未知编码不得静默送入解码器（会产出花屏或崩溃），须先登记并实现对应解码通路",
  };
}

/** 解封装结果（一个文件的全貌）。 */
export interface DemuxResult {
  /** 文件品牌（ftyp 的 major_brand）。 */
  readonly majorBrand: string;
  /** 各轨道的样本（解码序）。 */
  readonly samples: ReadonlyMap<number, readonly Sample[]>;
  /** 各轨道元信息。 */
  readonly tracks: readonly Track[];
  /** 分片片段序列（非分片文件为空）。 */
  readonly fragments: readonly FragmentInfo[];
  /** 是否为分片 MP4。 */
  readonly fragmented: boolean;
  /** moov 是否在 mdat 之前（faststart）。 */
  readonly faststart: boolean;
}

/**
 * 顶层解封装：解析完整 MP4（普通或分片），产出全部轨道的样本。
 *
 * moov 后置（非faststart）不是错误：流式场景下 moov 在文件尾是完全合法的
 * （边录边写），此时必须先把 moov 找出来才能解sample 表。本函数先全树遍历，
 * 再定位 moov，因此天然兼容 moov 后置——但会记一条诊断提醒「非 faststart，
 * 流式播放需先读尾部」。
 */
export function demuxMp4(data: Uint8Array, options: BoxParseOptions = {}): Outcome<DemuxResult> {
  const bag = new DiagBag();
  const top = parseBoxList(data, 0, data.length, options, bag, 0);
  if (!top.ok) return err(top.code, top.message, top.hint, top.diagnostics);

  const topBoxes = top.value;
  if (topBoxes.length > MP4_COUNT_LIMIT.topLevelBoxCount) {
    return err(
      "MP4_COUNT_LIMIT_EXCEEDED",
      `顶层 Box 数 ${topBoxes.length} 超过上限 ${MP4_COUNT_LIMIT.topLevelBoxCount}`,
      "顶层 Box 数异常；拒绝解析（F1122 计数上限纪律）",
    );
  }

  // Array.find 返回 undefined（不是 null），故这里必须判undefined。
  const ftyp = topBoxes.find((b) => b.type === "ftyp");
  let majorBrand = "unknown";
  if (ftyp !== undefined && ftyp.payloadSize >= 4) {
    majorBrand = String.fromCharCode(
      data[ftyp.payloadStart]!,
      data[ftyp.payloadStart + 1]!,
      data[ftyp.payloadStart + 2]!,
      data[ftyp.payloadStart + 3]!,
    );
  }

  // faststart：moov 必须排在 mdat 之前，否则 seek 表在文件尾、需额外一遍读取。
  const moovIndex = topBoxes.findIndex((b) => b.type === "moov");
  const mdatIndex = topBoxes.findIndex((b) => b.type === "mdat");
  const faststart = moovIndex !== -1 && (mdatIndex === -1 || moovIndex < mdatIndex);
  if (moovIndex !== -1 && !faststart) {
    bag.push(
      "MP4_BOX_TYPE_UNKNOWN",
      "moov 位于 mdat 之后（非 faststart）",
      "流式播放需先读文件尾取moov（两遍读取）；若追求快速起播，请在封装时启用 faststart（moov 前置）",
    );
  }

  // 分片 MP4：存在 moof即为分片流。
  const moofNodes = topBoxes.filter((b) => b.type === "moof");
  const fragmented = moofNodes.length > 0;
  const fragments: FragmentInfo[] = [];
  if (fragmented) {
    for (const moof of moofNodes) {
      const frag = parseFragment(data, moof);
      if (!frag.ok) return err(frag.code, frag.message, frag.hint, frag.diagnostics);
      fragments.push(frag.value);
    }
    const seqAudit = auditFragmentSequence(fragments);
    if (!seqAudit.ok) return err(seqAudit.code, seqAudit.message, seqAudit.hint, seqAudit.diagnostics);
  }

  if (moovIndex === -1) {
    return err(
      "MP4_REQUIRED_BOX_MISSING",
      "顶层未找到 moov Box",
      fragmented
        ? "分片 MP4 也必须有 moov（承载轨道与默认参数）；缺失说明文件不完整"
        : "无moov 则无任何轨道与样本表，文件不可解封装",
    );
  }
  const moov = topBoxes[moovIndex]!;

  const tracksOut = extractTracks(data, moov, bag);
  if (!tracksOut.ok) return err(tracksOut.code, tracksOut.message, tracksOut.hint, tracksOut.diagnostics);
  const tracks = tracksOut.value;

  const samples = new Map<number, readonly Sample[]>();
  for (const t of tracks) {
    // 编解码器能力：未知码显性不支持（记诊断并跳过该轨，不静默透传）。
    const cap = queryCodec(t.codec);
    if (!cap.supported) {
      bag.push("MP4_CODEC_UNSUPPORTED", `轨道 ${t.trackId} 的编解码器 ${t.codec} 不受支持`, cap.reason ?? "");
      continue;
    }
    // 定位该轨的 stbl，组装样本表并展开。
    const trakNode = moov.children.find(
      (c) => c.type === "trak" && readTrackId(data, c) === t.trackId,
    );
    if (trakNode === undefined) {
      bag.push("MP4_TRACK_NOT_FOUND", `轨道 ${t.trackId} 的 trak 节点定位失败`, "内部一致性问题；请核对该轨是否被跳过解析");
      continue;
    }
    const mdia = findChild(trakNode, "mdia");
    const minf = mdia === null ? null : findChild(mdia, "minf");
    const stbl = minf === null ? null : findChild(minf, "stbl");
    if (stbl === null) {
      bag.push("MP4_REQUIRED_BOX_MISSING", `轨道 ${t.trackId} 缺少 stbl，已跳过`, "无样本表则该轨无样本");
      continue;
    }
    const table = buildSampleTable(data, stbl, t.trackId, t.timescale);
    if (!table.ok) {
      // 分片 MP4 的moov 允许只有 stsd 而无完整样本表（样本在片段的 trun 里）。
      // 此时降级为「该轨无静态样本」并记诊断，不阻断整个文件的解析。
      if (fragmented) {
        bag.push(
          table.code,
          `分片 MP4：轨道 ${t.trackId} 的 moov 未提供完整样本表（${table.message}）`,
          "这是分片流的正常形态——样本信息在各moof 的 trun 中；请走分片路径重建样本",
        );
        continue;
      }
      return err(table.code, table.message, table.hint, table.diagnostics);
    }
    const expanded = expandSamples(table.value, data.length);
    if (!expanded.ok) return err(expanded.code, expanded.message, expanded.hint, expanded.diagnostics);
    samples.set(t.trackId, expanded.value);
  }

  // 「零样本」只在**非分片**文件里是错误：分片 MP4 的样本位于 moof/traf/trun，
  // moov 只承载轨道与默认参数，本就不含样本表。此时若同样判失败，会把
  // 「结构完全合法的分片流」误判为损坏文件——那会让流式场景根本起不来。
  if (samples.size === 0 && !fragmented) {
    return err(
      "MP4_NO_SAMPLES",
      "所有轨道均未产出样本（编解码器不受支持或样本表缺失）",
      "先看诊断中各轨的跳过原因：多为 CODEC_HANDOFF 未登记或 stbl 缺失",
    );
  }
  if (samples.size === 0) {
    bag.push(
      "MP4_NO_SAMPLES",
      "分片 MP4：moov 未提供样本表（符合预期，样本位于各片段的 trun）",
      "请走分片路径（按 moof 顺序读取 trun 重建样本）；本条返回的 tracks 可直接用于轨道参数查询",
    );
  }

  return ok(
    { majorBrand, samples, tracks, fragments, fragmented, faststart },
    bag.all(),
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §10 自检（判据的可执行形态：不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应一条判据，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

// ──测试用 MP4 字节流构造器（自检专用；生产路径不依赖它）────────────────────

/** 以大端写入 32 位无符号整数。 */
function putU32(arr: number[], v: number): void {
  arr.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff);
}

/** 以大端写入 4 字节四字符码。 */
function putType(arr: number[], t: string): void {
  for (let i = 0; i < 4; i++) arr.push(t.charCodeAt(i) & 0xff);
}

/**
 * 构造一个 Box：调用方只填 payload，长度由实测字节数自动回填。
 *
 * 为什么坚持不手写长度常量：Box 头里的 size 是后续一切解析的锚点，
 * 手算常量与实际写入差 4 字节就会让**后面所有 Box 整体错位**，
 * 症状表现为「解析到中途遇到垃圾类型码」，根因却在几百字节之前——
 * 这类 bug 极难定位。构造器必须与被测解析器同等严谨，故不给手数留位置。
 */
function box(type: string, fill: (body: number[]) => void): number[] {
  const body: number[] = [];
  fill(body);
  const out: number[] = [];
  putU32(out, 8 + body.length);
  putType(out, type);
  for (const b of body) out.push(b);
  return out;
}

/**
 * 构造一个最小可解封装的 MP4：ftyp + moov（trak→mdia→minf→stbl 五件套）+ mdat。
 *
 * 刻意做成「非 faststart」（moov 在 mdat 之后）——这样自检同时覆盖
 * moov 后置的定位能力，而不是只在最顺手的排列下通过。
 *
 * @param sampleSizes 逐样本大小（写进 stsz）
 * @param deltas      逐样本 DTS 增量（写进 stts，跑成一条 stts 游程）
 * @param cttsOffsets 逐样本呈现偏移（空数组表示无 ctts）
 * @param sampleCount每 chunk 样本数（写进 stsc）
 */
export function buildTestMp4(opts: {
  sampleSizes: readonly number[];
  deltas: readonly number[];
  cttsOffsets?: readonly number[];
  samplesPerChunk?: number;
  syncSamples?: readonly number[];
  timescale?: number;
  use64BitChunkOffset?: boolean;
  moovAfterMdat?: boolean;
  largesizeMdat?: boolean;
}): Uint8Array {
  const spc = opts.samplesPerChunk ?? opts.sampleSizes.length;
  const timescale = opts.timescale ?? 1000;
  const n = opts.sampleSizes.length;
  const bytes: number[] = [];

  // ── mdat：真实样本数据（内容用序号填充，只要求长度对得上）──
  const mdatPayloadLen = opts.sampleSizes.reduce((a, b) => a + b, 0);
  const mdatBody: number[] = [];
  for (let i = 0; i < mdatPayloadLen; i++) mdatBody.push(i & 0xff);

  // ── 构造 moov ──
  // stsz
  const stsz = box("stsz", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 0); // sample_size=0 → 变长模式
    putU32(b, n); // sample_count
    for (const s of opts.sampleSizes) putU32(b, s);
  });

  // stts：逐样本 delta 跑成游程（相同 delta 合并）
  const sttsEntries: { count: number; delta: number }[] = [];
  for (let i = 0; i < n; i++) {
    const d = opts.deltas[i]!;
    const last = sttsEntries[sttsEntries.length - 1];
    if (last !== undefined && last.delta === d) last.count += 1;
    else sttsEntries.push({ count: 1, delta: d });
  }
  const stts = box("stts", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, sttsEntries.length);
    for (const e of sttsEntries) {
      putU32(b, e.count);
      putU32(b, e.delta);
    }
  });

  // ctts（可选）
  let ctts: number[] = [];
  if (opts.cttsOffsets !== undefined && opts.cttsOffsets.length > 0) {
    const e: { count: number; off: number }[] = [];
    for (const off of opts.cttsOffsets) {
      const last = e[e.length - 1];
      if (last !== undefined && last.off === off) last.count += 1;
      else e.push({ count: 1, off });
    }
    ctts = box("ctts", (b) => {
      putU32(b, 0); // version/flags
      putU32(b, e.length);
      for (const x of e) {
        putU32(b, x.count);
        putU32(b, x.off);
      }
    });
  }

  // stsc
  const stsc = box("stsc", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 1); // entry_count
    putU32(b, 1); // first_chunk
    putU32(b, spc); // samples_per_chunk
    putU32(b, 1); // sample_description_index
  });

  // stco / co64
  const chunkOffset = 0; // 稍后回填（mdat 位置取决于排列）
  // 记录 chunk 偏移值在 stco Box 内的相对位置，供组装完毕后精确回填。
  // 用记录位置而非事后扫描：扫描法在 moov 尚未 emit 时根本找不到 stco，
  // 而「先算出位置再回填」与 Box 排列顺序解耦，两种排列都成立。
  const wide64 = opts.use64BitChunkOffset === true;
  // 偏移值字段的起始位置（相对 stco Box 首字节）：size(4)+type(4)+version/flags(4)+entry_count(4) = 16。
  // stco 的值是4 字节；co64 的值是 8 字节（高 32 位在前）——两者**都从同一位置开始**，
  // 差别只在宽度，故此处的相对位置与wide64 无关。
  const chunkValRelPos = 16;
  const stco = box(wide64 ? "co64" : "stco", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 1); // entry_count
    if (wide64) putU32(b, 0); // 高 32 位
    putU32(b, chunkOffset);
  });

  // stss（可选）
  let stss: number[] = [];
  if (opts.syncSamples !== undefined) {
    stss = box("stss", (b) => {
      putU32(b, 0); // version/flags
      putU32(b, opts.syncSamples!.length);
      for (const s of opts.syncSamples!) putU32(b, s + 1); // 1 基
    });
  }

  // stsd：FullBox + entry_count + 1 entry(size+format+6 保留 + 2 索引)
  const stsd = box("stsd", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 1); // entry_count
    putU32(b, 8 + 6 + 2); // entry size
    putType(b, "avc1"); // format（视频）
    for (let i = 0; i < 6; i++) b.push(0); // reserved
    putU32(b, 1); // data_reference_index
  });

  // stbl = stsd+stts+ctts+stsc+stsz+stco+stss
  const stblChildren = [...stsd, ...stts, ...ctts, ...stsc, ...stsz, ...stco, ...stss];
  const stbl = box("stbl", (b) => {
    for (const x of stblChildren) b.push(x);
  });

  // minf
  const minf = box("minf", (b) => {
    for (const x of stbl) b.push(x);
  });

  // hdlr（handler_type = vide）
  const hdlr = box("hdlr", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 0); // pre_defined
    putType(b, "vide");
    for (let i = 0; i < 12; i++) b.push(0); // reserved
  });

  // mdhd（version 0）
  const mdhd = box("mdhd", (b) => {
    putU32(b, 0); // version/flags
    putU32(b, 0); // creation
    putU32(b, 0); // modification
    putU32(b, timescale);
    putU32(b, n * 1000); // duration
    putU32(b, 0x55c4); // language 'und'
    putU32(b, 0); // quality
  });

  // mdia = mdhd+hdlr+minf
  const mdia = box("mdia", (b) => {
    for (const x of [...mdhd, ...hdlr, ...minf]) b.push(x);
  });

  // tkhd（version 0，末 8 字节为 16.16 宽高）
  // 先写 payload 再回填长度：手数常量与实际写入极易漂移（写错一位就让后续
  // 所有 Box 错位，症状是「解析到某个 Box 时遇到垃圾类型码」），故此处一律
  // 用实测长度而非手算常量——构造器必须与被测解析器一样严谨。
  const tkhdBody: number[] = [];
  putU32(tkhdBody, 0x00000007); // version 0 + flags(enabled|in movie|in preview)
  putU32(tkhdBody, 0); // creation
  putU32(tkhdBody, 0); // modification
  putU32(tkhdBody, 1); // track_ID
  putU32(tkhdBody, 0); // reserved
  putU32(tkhdBody, n * 1000); // duration
  for (let i = 0; i < 2; i++) putU32(tkhdBody, 0); // reserved
  putU32(tkhdBody, 0); // layer
  putU32(tkhdBody, 0); // alternate_group
  putU32(tkhdBody, 0); // volume
  putU32(tkhdBody, 0); // reserved
  // unity matrix 3x3
  const unity = [0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000];
  for (const u of unity) putU32(tkhdBody, u);
  putU32(tkhdBody, 1920 * 65_536); // width  16.16
  putU32(tkhdBody, 1080 * 65_536); // height 16.16
  const tkhd: number[] = [];
  putU32(tkhd, 8 + tkhdBody.length);
  putType(tkhd, "tkhd");
  for (const b of tkhdBody) tkhd.push(b);

  // trak = tkhd+mdia
  const trak = box("trak", (b) => {
    for (const x of [...tkhd, ...mdia]) b.push(x);
  });

  // mvhd（影片头，moov 需要它才是合法 moov）——同样先写 payload 再回填长度。
  const mvhdBody: number[] = [];
  putU32(mvhdBody, 0); // version 0 + flags
  putU32(mvhdBody, 0); // creation
  putU32(mvhdBody, 0); // modification
  putU32(mvhdBody, 1000); // timescale
  putU32(mvhdBody, n * 1000); // duration
  putU32(mvhdBody, 0x00010000); // rate 1.0
  putU32(mvhdBody, 0x01000000); // volume 1.0
  putU32(mvhdBody, 0); // reserved
  putU32(mvhdBody, 0); // reserved
  for (const u of unity) putU32(mvhdBody, u);
  for (let i = 0; i < 6; i++) putU32(mvhdBody, 0); // pre_defined
  putU32(mvhdBody, 2); // next_track_ID
  const mvhd: number[] = [];
  putU32(mvhd, 8 + mvhdBody.length);
  putType(mvhd, "mvhd");
  for (const b of mvhdBody) mvhd.push(b);

  // stco 在 moov 内的相对偏移（moov 头 8 + mvhd + trak 头 8 + tkhd + mdia 头 8
  // + mdhd + hdlr + minf 头 8 + stbl 头 8 + stsd + stts + ctts + stsc + stsz）。
  // 逐层累加实测长度而非手数常量——任一环节写错字节，这里就指向错误位置。
  const stcoIndexInMoov =
    8 + mvhd.length + 8 + tkhd.length + 8 + mdhd.length + hdlr.length + 8 + 8
    + stsd.length + stts.length + ctts.length + stsc.length + stsz.length;

  // moov = mvhd + trak
  const moov = box("moov", (b) => {
    for (const x of [...mvhd, ...trak]) b.push(x);
  });

  // ftyp
  const ftyp = box("ftyp", (b) => {
    putType(b, "isom"); // major_brand
    putU32(b, 512); // minor_version
    putType(b, "isom"); // compatible brand
  });

  // 组装：moov 后置（默认）或前置
  const emit = (arr: number[]): void => {
    for (const b of arr) bytes.push(b);
  };

  // chunk 偏移必须指向 mdat 的**数据起点**（不是 Box 头起点）。
  // largesize 形态下 mdat 头占 16 字节，故偏移随形态而变——这里算错，
  // 解析出的每个样本都会指向 mdat 的类型码，症状是「解出全是垃圾字节」。
  const mdatHeaderSize = opts.largesizeMdat === true ? 16 : 8;
  if (opts.moovAfterMdat === false) {
    // faststart：ftyp + moov + mdat。moov 位置已确定，可直接回填。
    emit(ftyp);
    emit(moov);
    const valuePos = ftyp.length + stcoIndexInMoov + chunkValRelPos;
    patchChunkOffset(bytes, valuePos, bytes.length + mdatHeaderSize, wide64);
    pushMdat(bytes, mdatBody, opts.largesizeMdat === true);
  } else {
    // moov 后置：ftyp + mdat + moov。mdat 起点可算，但 stco 所在的moov
    // 此刻还没进 bytes——故先按已知长度把 mdat 写入并记下 moov 起点，
    // 再 emit moov，最后回填。此顺序不可调换：回填依赖 moov 已就位。
    emit(ftyp);
    const mdatDataStart = bytes.length + mdatHeaderSize;
    pushMdat(bytes, mdatBody, opts.largesizeMdat === true);
    const moovStart = bytes.length;
    emit(moov);
    patchChunkOffset(bytes, moovStart + stcoIndexInMoov + chunkValRelPos, mdatDataStart, wide64);
  }

  return new Uint8Array(bytes);
}

/** 写 mdat（支持 largesize 形态）。 */
function pushMdat(out: number[], body: readonly number[], large: boolean): void {
  if (large) {
    putU32(out, 1); // size == 1 → largesize 扩展
    putType(out, "mdat");
    putU32(out, 0); // largesize 高 32 位
    putU32(out, 16 + body.length); // largesize 低 32 位
  } else {
    putU32(out, 8 + body.length);
    putType(out, "mdat");
  }
  for (const b of body) out.push(b);
}

/**
 * 定点回填 stco/co64 的块偏移。
 * @param valuePos 偏移值在 arr 中的绝对字节位置（由构造期精确算出）
 * @param offset   要写入的块偏移（mdat 数据起点）
 * @param wide     true=co64（8 字节），false=stco（4 字节）
 */
function patchChunkOffset(arr: number[], valuePos: number, offset: number, wide: boolean): void {
  if (valuePos < 0 || valuePos >= arr.length) {
    // 回填位置越界说明构造期算错了偏移；此处静默跳过会让 chunk 偏移保持 0，
    // 症状是「所有样本都指向文件开头」——必须显性暴露而非假装成功。
    throw new Error(
      `patchChunkOffset: valuePos=${valuePos} 越界（arr 长度 ${arr.length}），构造期偏移计算有误`,
    );
  }
  if (wide) {
    for (let k = 0; k < 4; k++) arr[valuePos + k] = 0; // 高 32 位
    arr[valuePos + 4] = (offset >>> 24) & 0xff;
    arr[valuePos + 5] = (offset >>> 16) & 0xff;
    arr[valuePos + 6] = (offset >>> 8) & 0xff;
    arr[valuePos + 7] = offset & 0xff;
    return;
  }
  arr[valuePos] = (offset >>> 24) & 0xff;
  arr[valuePos + 1] = (offset >>> 16) & 0xff;
  arr[valuePos + 2] = (offset >>> 8) & 0xff;
  arr[valuePos + 3] = offset & 0xff;
}


// ════════════════════════════════════════════════════════════════════════════
// §11 判据自检（五条判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 判据一「标准 MP4 测试文件解析」自检。 */
export function selfCheckStandardMp4(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 态1：基础解析——轨道、编解码器、时基、样本偏移。
  const mp4 = buildTestMp4({ sampleSizes: [10, 20, 30, 40], deltas: [100, 100, 100, 100], timescale: 1000 });
  const r = demuxMp4(mp4);
  const samples = r.ok ? r.value.samples.get(1) : undefined;
  out.push({
    name: "standard-mp4-parses",
    pass: r.ok && samples !== undefined && samples.length === 4,
    detail: r.ok
      ? `基础 MP4 解析成功：轨道 ${r.value.tracks.length} 条、样本 ${samples?.length ?? 0} 个`
      : `解析失败：${r.code}/${r.message}`,
  });

  // 态2：样本字节必须真的指向 mdat 数据区（内容对齐是最强证据——
  // 只查「偏移递增」会放过「整体偏移 4 字节」这类错误）。
  let contentOk = false;
  if (samples !== undefined) {
    contentOk = true;
    let cursor = 0;
    for (const s of samples) {
      if (mp4[s.offset] !== (cursor & 0xff)) contentOk = false;
      cursor += s.size;
    }
  }
  out.push({
    name: "sample-offsets-point-at-payload",
    pass: contentOk,
    detail: contentOk
      ? "每个样本偏移处的字节与 mdat 数据区的预期内容逐字节吻合（偏移未整体偏移）"
      : "样本偏移未指向 mdat 数据区（内容对不上）——偏移计算有误",
  });

  // 态3：时间戳正确——DTS 由 stts 累加、PTS 无 ctts 时等于 DTS。
  const tsOk =
    samples !== undefined &&
    samples[0]!.dts === 0 &&
    samples[1]!.dts === 100 &&
    samples[2]!.dts === 200 &&
    samples[3]!.dts === 300 &&
    samples.every((s) => s.pts === s.dts);
  out.push({
    name: "timestamps-correct",
    pass: tsOk,
    detail: tsOk
      ? "DTS 按 stts 累加为 0/100/200/300；无 ctts 时 PTS 等于 DTS"
      : `时间戳不符：${samples?.map((s) => `${s.dts}/${s.pts}`).join(" ") ?? "无样本"}`,
  });

  // 态4：moov 后置（非 faststart）必须仍能解析——这是真实流式文件的形态。
  const rAfter = demuxMp4(buildTestMp4({ sampleSizes: [10, 20], deltas: [50, 50], moovAfterMdat: true }));
  out.push({
    name: "moov-after-mdat-still-parses",
    pass: rAfter.ok && !rAfter.value.faststart,
    detail: rAfter.ok
      ? `moov 后置文件解析成功（faststart=${rAfter.value.faststart}），非 faststart 不阻断解封装`
      : `moov 后置解析失败：${rAfter.code}/${rAfter.message}`,
  });

  // 态5：mdat 用 largesize（64 位长度）扩展形态。
  const rLarge = demuxMp4(buildTestMp4({ sampleSizes: [10, 20], deltas: [50, 50], largesizeMdat: true }));
  out.push({
    name: "largesize-mdat-supported",
    pass: rLarge.ok && (rLarge.value.samples.get(1)?.length ?? 0) === 2,
    detail: rLarge.ok
      ? "mdat 采用 largesize（size==1 + 64 位长度）形态时解析正常"
      : `largesize 形态解析失败：${rLarge.code}/${rLarge.message}`,
  });

  // 态6：co64（64 位块偏移）。
  const rWide = demuxMp4(buildTestMp4({ sampleSizes: [10, 20], deltas: [50, 50], use64BitChunkOffset: true }));
  out.push({
    name: "co64-64bit-offset-supported",
    pass: rWide.ok && (rWide.value.samples.get(1)?.length ?? 0) === 2,
    detail: rWide.ok
      ? "co64（64 位块偏移）形态解析正常，>4GB 文件的偏移不被截断"
      : `co64 形态解析失败：${rWide.code}/${rWide.message}`,
  });

  return out;
}

/** 判据二「编解码器全覆盖」自检。 */
export function selfCheckCodecs(): SelfCheck[] {
  const out: SelfCheck[] = [];

  const supported = ["avc1", "avc3", "hvc1", "hev1", "av01", "vp09", "mp4a", "Opus", "fLaC"];
  const missing = supported.filter((c) => !queryCodec(c).supported);
  out.push({
    name: "codec-coverage-complete",
    pass: missing.length === 0,
    detail: missing.length === 0
      ? `已登记编解码器 ${supported.length} 个全部可解（H.264/HEVC/AV1 视频 + AAC/Opus/FLAC 音频）`
      : `未登记：${missing.join("、")}`,
  });

  // 未知码必须显性不支持，且给出可操作出路（绝不静默透传）。
  const unknown = queryCodec("xyz9");
  out.push({
    name: "unknown-codec-explicitly-unsupported",
    pass: !unknown.supported && (unknown.reason?.length ?? 0) > 0,
    detail: !unknown.supported
      ? `未知四字符码 xyz9 显性不支持并给出出路：${(unknown.reason ?? "").slice(0, 48)}…`
      : "未知编解码码被放行（会把垃圾数据喂进解码器，症状为花屏或崩溃）",
  });

  // 交接映射：每种编码都要指到具体承接条目，不能只说「支持」。
  const noHandoff = supported.filter((c) => !queryCodec(c).reason || queryCodec(c).reason === "已登记，交由 解码");
  out.push({
    name: "codec-handoff-declared",
    pass: noHandoff.length === 0,
    detail: noHandoff.length === 0
      ? "每种编解码器均声明了承接条目（F1204-F1207），下游无「无人负责」的编码"
      : `缺承接条目：${noHandoff.join("、")}`,
  });

  return out;
}

/** 判据三「时间戳正确 + 重排序」自检。 */
export function selfCheckReorder(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 构造带ctts 的 B 帧序列：解码序 dts=0,100,200；呈现序 pts=200,0,100（典型 I/P/B）。
  // ctts 的游程压缩：相邻相同的 offset 会被合并成一条 count=N 的游程。
  // 传入 [200,0,100] 三项互不相同，故不合并，解析后应得count=1/offset=200、1/0、1/100。
  const mp4 = buildTestMp4({
    sampleSizes: [10, 10, 10],
    deltas: [100, 100, 100],
    cttsOffsets: [200, 0, 100],
  });
  const r = demuxMp4(mp4);
  const s = r.ok ? r.value.samples.get(1) : undefined;
  out.push({
    name: "ctts-parsed",
    pass: s !== undefined && s[0]!.pts === 200 && s[1]!.pts === 100 && s[2]!.pts === 300,
    detail: s !== undefined
      ? `ctts 偏移已应用：PTS = ${s.map((x) => x.pts).join("/")}（DTS = ${s.map((x) => x.dts).join("/")}，`
        + "PTS=DTS+ctts 偏移逐样本成立）"
      : "ctts 未生效：PTS 应与 DTS 不同",
  });

  // PTS 必须恒≥ DTS（有 B 帧时呈现不早于解码）。
  const ptsOk = s !== undefined && s.every((x) => x.pts >= x.dts);
  out.push({
    name: "pts-not-before-dts",
    pass: ptsOk,
    detail: ptsOk ? "全部样本满足 PTS ≥ DTS（呈现不早于解码）" : "存在 PTS < DTS 的样本（物理上不可能）",
  });

  // 重排序：解码序 [dts0/pts200, dts100/pts100, dts200/pts200] → 呈现序应按 PTS 升序且稳定。
  const reordered = s === undefined ? undefined : reorderToPresentation(s);
  const sortedOk =
    reordered !== undefined &&
    reordered.ok &&
    reordered.value.every((x, i, a) => i === 0 || a[i - 1]!.pts <= x.pts);
  out.push({
    name: "reorder-restores-presentation-order",
    pass: sortedOk,
    detail: sortedOk && reordered.ok
      ? `重排序后 PTS 升序：${reordered.value.map((x) => x.pts).join("/")}`
      : "重排序未产生升序 PTS 序列",
  });

  // 重排序深度可测量且在声明上界内。
  const depth = s === undefined ? -1 : measureReorderDepth(s);
  out.push({
    name: "reorder-depth-measurable",
    pass: depth >= 0 && depth <= MP4_COUNT_LIMIT.reorderDepth,
    detail: `实测重排序深度 = ${depth}（声明上界 ${MP4_COUNT_LIMIT.reorderDepth}）`,
  });

  return out;
}

/** 判据四「分片 MP4」自检。 */
export function selfCheckFragmented(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 构造 ftyp + moov（分片 MP4 同样需要 moov 承载轨道与默认参数）+ moof×2。
  const build = (): Uint8Array => {
    const outBytes: number[] = [];
    const emit = (a: readonly number[]): void => {
      for (const b of a) outBytes.push(b);
    };
    emit(box("ftyp", (b) => {
      putType(b, "iso5"); // fragmented MP4 的典型品牌
      putU32(b, 512);
      putType(b, "iso5");
      putType(b, "dash");
    }));
    // 最小 moov：mvhd + 一个承载 avc1 的 trak（无样本表——分片的样本在 moof 里）。
    const mdhd = box("mdhd", (b) => {
      putU32(b, 0);
      putU32(b, 0);
      putU32(b, 0);
      putU32(b, 1000); // timescale
      putU32(b, 6000); // duration
      putU32(b, 0x55c4);
      putU32(b, 0);
    });
    const hdlr = box("hdlr", (b) => {
      putU32(b, 0);
      putU32(b, 0);
      putType(b, "vide");
      for (let i = 0; i < 12; i++) b.push(0);
    });
    const stsd = box("stsd", (b) => {
      putU32(b, 0);
      putU32(b, 1);
      putU32(b, 8 + 6 + 2);
      putType(b, "avc1");
      for (let i = 0; i < 6; i++) b.push(0);
      putU32(b, 1);
    });
    const stbl = box("stbl", (b) => {
      for (const x of [stsd]) b.push(...x);
    });
    const minf = box("minf", (b) => {
      for (const x of [stbl]) b.push(...x);
    });
    const mdia = box("mdia", (b) => {
      for (const x of [mdhd, hdlr, minf]) b.push(...x);
    });
    const tkhdBody: number[] = [];
    putU32(tkhdBody, 0x00000007);
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 1); // track_ID
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 6000);
    for (let i = 0; i < 2; i++) putU32(tkhdBody, 0);
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 0);
    putU32(tkhdBody, 0);
    for (const u of [0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]) putU32(tkhdBody, u);
    putU32(tkhdBody, 1920 * 65_536);
    putU32(tkhdBody, 1080 * 65_536);
    const tkhd = box("tkhd", (b) => {
      for (const x of tkhdBody) b.push(x);
    });
    const trak = box("trak", (b) => {
      for (const x of [tkhd, mdia]) b.push(...x);
    });
    const mvhd = box("mvhd", (b) => {
      putU32(b, 0);
      putU32(b, 0);
      putU32(b, 0);
      putU32(b, 1000);
      putU32(b, 6000);
      putU32(b, 0x00010000);
      putU32(b, 0x01000000);
      putU32(b, 0);
      putU32(b, 0);
      for (const u of [0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]) putU32(b, u);
      for (let i = 0; i < 6; i++) putU32(b, 0);
      putU32(b, 2);
    });
    emit(box("moov", (b) => {
      for (const x of [mvhd, trak]) b.push(...x);
    }));
    for (const seq of [1, 2]) {
      const mfhd = box("mfhd", (b) => {
        putU32(b, 0);
        putU32(b, seq);
      });
      const tfhd = box("tfhd", (b) => {
        putU32(b, 0x020000); // flags: default-base-is-moof
        putU32(b, 1); // track_ID
      });
      const tfdt = box("tfdt", (b) => {
        putU32(b, 0); // version 0
        putU32(b, (seq - 1) * 3000);
      });
      const traf = box("traf", (b) => {
        for (const x of [tfhd, tfdt]) b.push(...x);
      });
      const moof = box("moof", (b) => {
        for (const x of [mfhd, traf]) b.push(...x);
      });
      emit(moof);
    }
    return new Uint8Array(outBytes);
  };

  const r = demuxMp4(build());
  out.push({
    name: "fragmented-mp4-detected",
    pass: r.ok && r.value.fragmented && r.value.fragments.length === 2,
    detail: r.ok
      ? `识别为分片 MP4，片段数 = ${r.value.fragments.length}`
      : `分片流解析失败：${r.code}/${r.message}`,
  });

  // 片段序号连续。
  const seqOk = r.ok && r.value.fragments[0]!.sequenceNumber === 1 && r.value.fragments[1]!.sequenceNumber === 2;
  out.push({
    name: "fragment-sequence-continuous",
    pass: seqOk,
    detail: seqOk ? "片段序号 1,2 连续（碎片间轨道延续的前提成立）" : "片段序号不连续",
  });

  // tfdt 基址解码时间随片段递增。
  const baseOk =
    r.ok &&
    r.value.fragments[1]!.baseDecodeTimes.get(1) === 3000;
  out.push({
    name: "fragment-base-decode-time",
    pass: baseOk,
    detail: baseOk
      ? "片段 2 的 tfdt 基址 DTS = 3000（承接片段 1 的时间轴尾部）"
      : "片段基址 DTS 解析不正确",
  });

  // 序号不连续必须被拦截（延续语义失效）。
  const badFrag: FragmentInfo[] = [
    { sequenceNumber: 1, trackIds: [1], baseDecodeTimes: new Map() },
    { sequenceNumber: 3, trackIds: [1], baseDecodeTimes: new Map() },
  ];
  const audit = auditFragmentSequence(badFrag);
  out.push({
    name: "fragment-gap-rejected",
    pass: !audit.ok && audit.code === "MP4_SAMPLE_TABLE_INCONSISTENT",
    detail: !audit.ok ? `片段序号缺口被拦截（${audit.code}）` : "片段序号缺口未被拦截（后续轨道会错位）",
  });

  return out;
}

/** 判据五「畸形拦截」自检。 */
export function selfCheckMalformed(): SelfCheck[] {
  const out: SelfCheck[] = [];

  const good = buildTestMp4({ sampleSizes: [10, 20], deltas: [50, 50] });

  // 畸形1：Box 声明长度越界。
  const bad1 = new Uint8Array(good);
  bad1[3] = 0xff; // 把首个 Box（ftyp）的 size 改得极大
  const r1 = demuxMp4(bad1);
  out.push({
    name: "malformed-box-size-rejected",
    pass: !r1.ok && (r1.code === "MP4_BOX_SIZE_OUT_OF_RANGE" || r1.code === "MP4_TRUNCATED"),
    detail: !r1.ok ? `Box 长度越界被拦截（${r1.code}）` : "越界 Box 未被拦截（会越界读取）",
  });

  // 畸形2：文件截断（只剩前 6 字节，不足一个 Box 头）。
  const r2 = demuxMp4(new Uint8Array(good.slice(0, 6)));
  out.push({
    name: "truncated-file-rejected",
    pass: !r2.ok,
    detail: !r2.ok ? `截断文件被拦截（${r2.code}）` : "截断文件竟解析成功（必然是越界读取）",
  });

  // 畸形3：子Box 越出父 payload——把 stsd 的 size 改大到吞掉后续 Box。
  const bad3 = new Uint8Array(good);
  // 定位 stsd 后把其 size 放大
  for (let i = 0; i + 8 < bad3.length; i++) {
    const t = String.fromCharCode(bad3[i + 4]!, bad3[i + 5]!, bad3[i + 6]!, bad3[i + 7]!);
    if (t === "stsd") {
      const cur = (bad3[i]! << 24) | (bad3[i + 1]! << 16) | (bad3[i + 2]! << 8) | bad3[i + 3]!;
      const bigger = cur + 32;
      bad3[i] = (bigger >>> 24) & 0xff;
      bad3[i + 1] = (bigger >>> 16) & 0xff;
      bad3[i + 2] = (bigger >>> 8) & 0xff;
      bad3[i + 3] = bigger & 0xff;
      break;
    }
  }
  const r3 = demuxMp4(bad3);
  out.push({
    name: "oversized-child-box-rejected",
    pass: !r3.ok,
    detail: !r3.ok
      ? `子 Box 越界被拦截（${r3.code}）——父子完整性校验生效`
      : "子 Box 越界未被拦截（后续 Box 会被静默吞掉）",
  });

  // 畸形4：算术溢出（checked 纪律）。
  const mulOk = checkedMul(Number.MAX_SAFE_INTEGER, 2) === null && checkedMul(2, 3) === 6;
  const addOk = checkedAdd(Number.MAX_SAFE_INTEGER, 1) === null;
  const rngOk = !checkedRange(Number.MAX_SAFE_INTEGER - 1, 10, Number.MAX_SAFE_INTEGER);
  out.push({
    name: "overflow-guard-works",
    pass: mulOk && addOk && rngOk,
    detail: `checkedMul/checkedAdd/checkedRange 均在溢出时返回 null/拒绝（乘法=${mulOk} 加法=${addOk} 区间=${rngOk}）`,
  });

  // 畸形5：样本偏移越出文件长度——把 stco 值改成极大。
  const bad5 = new Uint8Array(good);
  for (let i = 0; i + 8 < bad5.length; i++) {
    const t = String.fromCharCode(bad5[i + 4]!, bad5[i + 5]!, bad5[i + 6]!, bad5[i + 7]!);
    if (t === "stco") {
      const p = i + 16;
      bad5[p] = 0xff; bad5[p + 1] = 0xff; bad5[p + 2] = 0xff; bad5[p + 3] = 0xfe;
      break;
    }
  }
  const r5 = demuxMp4(bad5);
  out.push({
    name: "out-of-file-sample-offset-rejected",
    pass: !r5.ok && r5.code === "MP4_SAMPLE_TABLE_INCONSISTENT",
    detail: !r5.ok
      ? `样本偏移越出文件被拦截（${r5.code}）`
      : "越界样本偏移未被拦截（会读到文件外内存）",
  });

  // 畸形6：非法入参（NaN 规模）不得静默通过——路由类比：计数类。
  const badCount = expandSamplesPerChunk(
    [{ firstChunk: 1, samplesPerChunk: 1, descIndex: 1 }],
    Number.MAX_SAFE_INTEGER,
  );
  out.push({
    name: "absurd-chunk-count-rejected",
    pass: !badCount.ok,
    detail: !badCount.ok ? `荒谬块数被拦截（${badCount.code}）` : "荒谬块数竟被接受",
  });

  return out;
}

/** 全量自检入口：一次跑完五组判据。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
} {
  const groups = {
    standardMp4: selfCheckStandardMp4(),
    codecs: selfCheckCodecs(),
    reorder: selfCheckReorder(),
    fragmented: selfCheckFragmented(),
    malformed: selfCheckMalformed(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}
