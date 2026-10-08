/**
 * VE-F1401 · H 域开工与边界契约执行（VE-H 域 · 音频引擎域开工条 · 批次 H01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-H 域（F1401~F1600）的**开工条**。它不合成一个采样、不跑一次
 * 卷积、不开一次音频设备，只做四件事，并把四件事写成可机检的契约：
 *   1. 宣告音频引擎域正式开工——官方九主题与十个批次的完整排布；
 *   2. 把 **F1381 接口总账**与**F1397 移交包**携带的四主题处置清单**落 ADR**：
 *      混音 / HRTF / 重采样 / 响度——四张 ADR 逐主题差异化定位（视角差异、
 *      保留面、共享核），**无 ADR 不开工**（册内明文：ADR 落册是 H 域开工的先决）；
 *   3. 抽出**复用声明**：DSP 核（biquad / FFT / 卷积核）两域同核同对拍，
 *      服务层（播放链编排 vs 引擎调度）各自独立——混层即职责污染；
 *   4. 给出组内 20 条分工总览——H01 批次 F1401~F1420 逐条落位。
 *
 * 为什么 H 域的开工条必须先把四主题 ADR 钉死（域级立场，全域根基）：
 *   VE-G 域在 F1381 里**主动揭露了一处计划层重叠**：G07（播放链音频集成）与
 *   VE-H（音频引擎本体）在混音 / HRTF / 重采样 / 响度四个主题上都有条目。
 *   这不是实现层的偶发撞车，而是两份规划各自向下生长时的必然交叠——G07 从
 *   「播放器要有声音」向下长出五层播放总线，VE-H 从「引擎要有本体」向下长出
 *   通用混音图，两者都会长到 biquad、都会长到重采样滤波器。册内对此的处理是
 *   「不静默撞车」：由 G 域给出处置建议，交H 域开工条**消费执行**。
 *   若这四张 ADR 只活在某个人的聊天记录里，那么 H01 二十个条目中的
 *   F1403（混音图）/ F1409（空间音频）/ F1413（重采样）/ F1414（响度归一）
 *   会各自按自己的理解去实现重叠面——到F1420 收口时才发现两套biquad、
 *   两套重采样器，那才是真正的返工：二十个条目的单元测试全部要重写，
 *   而且没人能说清哪一套才是对的。故本条把四主题做成**法理文件**：
 *   每主题一张 ADR，逐段写清「谁保留什么面、共享什么核、差异在哪一层」，
 *   由 `auditBoundaryAdr` 强制四张齐备且段落完整，缺一张即产出
 *   ADR_INCOMPLETE 并阻断开工。这条红线由代码执行，不靠自觉。
 *
 * 四条锚点契约（逐条对应判据）：
 *   1. 域开工 —— 官方九主题（混音图 / 空间音频 / HRTF / 音频令牌 / 淡入淡出 /
 *      事件总线 / 低延迟路径 / 重采样 / 响度归一）逐项登记：主题 id、主题名、
 *      归属批次、职责边界（禁扩面写在句内）、产出契约名。九主题与十批次**不是
 *      一一对应**（H01 同时承载混音图与 HRTF，H07/H08/H10 是跨主题的域治理
 *      批次），本条显式声明这层多对多关系而非假装双射。
 *   2. ADR 四主题落册 —— 混音 / HRTF / 重采样 / 响度四张ADR 逐张登记：
 *      对端条目（G07 侧）、视角差异（集成 vs 本体）、保留面、共享核、
 *      处置结论（g-retain / h-lead / merge-adr）、回归条件。
 *      **处置结论必须落在册内 F1381 允许的三选一之内**，自造第四种处置
 *      产出 DISPOSITION_OUT_OF_VOCABULARY。四张齐备 + 段落完整 = ADR 门禁过。
 *   3. 复用声明与共享核 —— DSP 核（biquad / FFT / 卷积核）登记为**共享核**，
 *      两域同核同对拍（对拍基线为同一份，误差阈值统一）；服务层登记为
 *      **各自独立**（播放链编排 vs 引擎调度）。任何把G07 的播放链编排语义
 *      放进 H 域共享核、或把 H 域调度语义放进 G07 面的行为，产出
 *      LAYER_MIXING_VIOLATION。这是「不重复造轮子也不混层」的双向纪律。
 *   4. 查重对账执行 —— 消费 F1381 总账与 F1397 处置清单：九组两两复核中的
 *      高风险对（G01↔G02 共享算子、G07↔H 域）逐对复核并留结论；官方描述
 *      词表覆盖核对（画中画 = G04 已含、视频墙基础 = G05 多实例）；
 *      产出查重对账报告（含四主题处置的最终态）。
 *
 * 零静默纪律：ADR 缺张 / 段落残缺 / 处置越界词汇表 / 共享核未登记 /
 * 混层 / 对拍基线缺失 / 查重未执行 / 上游未收官 / 移交包缺件 / 义务未核销 /
 * 主题未登记 / 覆盖缺口 / 条目越界 / 组内20 条不齐——全部产出 Diagnostic
 *   （code + message + hint）并由调用方聚合上报。本模块不抛异常、不吞诊断、
 *   无静默分支、不含任何 `as`强转掩盖缺省。
 *
 * 判据：ADR 落册、四主题差异化、复用声明、查重对账、判据。
 * 交接说明：本条是纯契约层，零音频设备调用、零 DSP 计算、零 DOM 依赖、
 *         零全局可变状态——可在任意宿主（浏览器 / Worker / Node 校验脚本）
 *         中原样引入。下游 H01（F1402 服务化架构 起）逐项消费本条的
 *         THEME_REGISTRY、BATCH_REGISTRY、BOUNDARY_ADR_TABLE、
 *         DSP_KERNEL_TABLE 与LAYER_BOUNDARY_TABLE。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 I / L / R 域同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  //── 主题与批次排布 ──
  /** 主题未在九主题注册表中登记（引用了不存在的主题）。 */
  | "THEME_UNREGISTERED"
  /** 主题已登记但字段自相矛盾（批次不存在 / 区间倒置 / 契约名为空）。 */
  | "THEME_SPEC_INCONSISTENT"
  /** 主题覆盖缺口：某个官方主题未被任何实现批次承载。 */
  | "THEME_COVERAGE_GAP"
  /** 批次未登记。 */
  | "BATCH_UNREGISTERED"
  /** 批次与主题的映射自相矛盾（批次主题表与主题归属表互不一致）。 */
  | "BATCH_THEME_MAPPING_INCONSISTENT"
  /** 功能号区间重叠（两个批次抢同一批功能号）。 */
  | "ITEM_RANGE_OVERLAP"
  /** 功能号区间缺口（相邻批次之间存在未分配功能号）。 */
  | "ITEM_RANGE_GAP"

  // ── 边界 ADR（F1401 判据一）──
  /** 边界 ADR 未落册（四主题缺张——无 ADR 不开工的红线直接命中）。 */
  | "ADR_INCOMPLETE"
  /** ADR 段落残缺（视角差异 / 保留面 / 共享核 三段缺一）。 */
  | "ADR_SECTION_MISSING"
  /** 处置结论用了册外词汇（F1381 只允许 g-retain / h-lead / merge-adr 三选一）。 */
  | "DISPOSITION_OUT_OF_VOCABULARY"
  /** ADR 对端条目越界（引用了不在 G 域 F1201-F1400 的条目）。 */
  | "PEER_ITEM_OUT_OF_RANGE"
  /** ADR 缺失回归条件（处置无解除条件 = 永久占用，无法被后续 ADR 推翻）。 */
  | "ADR_REGRESSION_CONDITION_MISSING"
  /** 同一主题被登记了多张 ADR（一个主题一张，多张即法理冲突）。 */
  | "ADR_DUPLICATED"

  // ── 复用声明与共享核（F1401 判据二）──
  /** 共享核未登记（DSP 核必须在共享核表中显式登记，两域才谈得上同核）。 */
  | "SHARED_KERNEL_UNREGISTERED"
  /** 共享核规格自相矛盾（两域入口缺失 / 对拍阈值非正 / 核实现归属为空）。 */
  | "SHARED_KERNEL_SPEC_INCONSISTENT"
  /** 对拍基线缺失或两侧不一致（同核不同基线 = 同核是空话）。 */
  | "PARITY_BASELINE_MISSING"
  /** 服务层边界越界：G07 编排语义被塞进 H 域共享核，或反之。 */
  | "LAYER_MIXING_VIOLATION"
  /** 服务层被误登记为共享核（编排/调度各自独立，不共享）。 */
  | "SERVICE_LAYER_SHARED"

  // ── 查重对账（F1401 判据三）──
  /** 上游 G 域未收官（F1400 宣告未生效），开工前置不成立。 */
  | "UPSTREAM_NOT_CLOSED"
  /** 移交包缺件（F1381 总账件 / F1397 边界清单件 / 教训件 / 归档索引件）。 */
  | "HANDOVER_BUNDLE_INCOMPLETE"
  /** 移交包快照失真（哈希与 G 域宣告值不符）。 */
  | "HANDOVER_SNAPSHOT_MISMATCH"
  /** 开工条件核销义务未逐条确认。 */
  | "HANDOVER_OBLIGATION_UNSETTLED"
  /** 高风险对未复核（G01↔G02 共享算子、G07↔H 域必须逐对留结论）。 */
  | "HIGH_RISK_PAIR_UNREVIEWED"
  /** 查重对账发现域内重复实现（同一能力两个域都声称供给）。 */
  | "DOMAIN_DUPLICATE_FOUND"
  /** 官方描述词表覆盖缺口（词表条目无人认领）。 */
  | "VOCAB_COVERAGE_GAP"
  /** 组内 20 条分工不齐（缺条 / 重复条目号）。 */
  | "WORKBREAKDOWN_INCOMPLETE"
  /** 条目号越出本域区间（映射到了别的域或跳段区）。 */
  | "CAPABILITY_ITEM_OUT_OF_DOMAIN"
  /** 开工被阻断（点名到具体缺口）。 */
  | "KICKOFF_BLOCKED";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办（可操作提示）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  /** 人话描述：面向开发者排障，不含裸异常码、不含「可能」「也许」。 */
  readonly message: string;
  /** 可操作提示：调用方该改哪里、该怎么降级、找哪个域协商。 */
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

/** 成功构造（diagnostics 允许携带非致命告警，例如前向引用提示）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/**
 * 取结果码：成功恒为 `"OK"`，失败恒为诊断码。
 *
 * 为什么需要这个辅助函数（实测踩坑记录，非风格偏好）：`Outcome` 的判别字段是
 * `readonly ok: true | false`。TypeScript 对**真值分支**（`x.ok ? x.value : …`）
 * 能正常收窄，但对**假值分支**（`… : x.code`）无法收窄——尽管
 * `!x.ok` / `x.ok === false` 都能收窄，逐处改写既啰嗦又容易漏。
 * 故把「取码」这件事收敂到一个函数里：它在函数内部用 `o.ok === false` 收敂，
 * 调用方无需窄化即可安全取码，且这段知识只写一遍。
 */
export function outcomeCode<T>(o: Outcome<T>): string {
  return o.ok === false ? o.code : "OK";
}

/** 取结果值的文本投影（失败时给调用方一个可读串，不抛异常）。 */
export function outcomeText<T>(o: Outcome<T>, onOk: (v: T) => string, onFail?: (c: string) => string): string {
  if (o.ok === false) {
    return onFail ? onFail(o.code) : `未过：${o.code}`;
  }
  return onOk(o.value);
}

/** 失败构造：单条诊断同时进顶层与 diagnostics，两条读法都不会丢信息。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  const d: Diagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器：把散落各处的告警汇成一条时间轴可查的清单。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  /** 追加一条诊断；空 message/hint 被规范化，避免上游写出半截诊断。 */
  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({
      code,
      message: message || "（未提供描述）",
      hint: hint || "（未提供处置建议）",
    });
  }

  /** 追加一条已构造的诊断（用于把子调用的 Outcome.diagnostics 平铺进来）。 */
  pushAll(ds: readonly Diagnostic[]): void {
    for (const d of ds) {
      this.items.push(d);
    }
  }

  /** 当前条数。 */
  get size(): number {
    return this.items.length;
  }

  /** 只读视图（返回副本，调用方改不动内部清单）。 */
  all(): readonly Diagnostic[] {
    return this.items.slice();
  }

  /** 按诊断码筛选——排障时按码聚合的入口。 */
  byCode(code: DiagCode): readonly Diagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  /** 是否存在任何诊断（当前全部码皆为 error 级，故等价于非空判定）。 */
  get hasAny(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 域身份 + 官方九主题 + 十批次排布（判据一：域开工）
// ════════════════════════════════════════════════════════════════════════════

/** 域身份常量（本域所有对外文本引用同一事实源，不各自硬编码）。 */
export const DOMAIN = {
  /** 域编号。 */
  id: "VE-H",
  /** 域名。 */
  name: "音频引擎",
  /** 功能号区间。 */
  itemLo: 1401,
  itemHi: 1600,
  /** 本域功能总数（1600 - 1401 + 1 = 200，与册内「十域 × 200 项」一致）。 */
  itemCount: 200,
  /** 所属波次（VE 卷波次表：W7 = 性能治理与规模，另W4/W5 交叉承载）。 */
  wave: "W7",
  /** 上游域（VE-G 视频引擎 F1201-F1400，收官宣告 F1400）。 */
  upstreamDomain: "VE-G",
  /** 上游收官条目号。 */
  upstreamCloseItem: 1400,
} as const;

/** 官方主题 id（册内第 1 部分域地图 VE-H 行九主题，一字不改）。 */
export type ThemeId =
  | "mix-graph"
  | "spatial-audio"
  | "hrtf"
  | "audio-token"
  | "fade"
  | "event-bus"
  | "low-latency"
  | "resample"
  | "loudness";

/** 主题规格：一条主题能被别人依赖的全部信息。 */
export interface ThemeSpec {
  /** 主题 id。 */
  readonly id: ThemeId;
  /** 中文名（册内原文用词）。 */
  readonly name: string;
  /** 承载该主题的实现批次（多对多，非双射）。 */
  readonly batches: readonly string[];
  /** 职责边界：做什么 + **禁扩面**（不做什么写在句内，防止后续条目往外长）。 */
  readonly boundary: string;
  /** 本主题产出的数据契约名（下游按契约名引用，不按条目号硬编码）。 */
  readonly contract: string;
}

/** 官方九主题注册表。 */
export const THEME_REGISTRY: Readonly<Record<ThemeId, ThemeSpec>> = {
  "mix-graph": {
    id: "mix-graph",
    name: "混音图",
    batches: ["H01", "H02"],
    boundary:
      "通用节点混音图（DAG 拓扑、任意连接、图热更新）。禁扩面：不做媒体解码与容器解析" +
      "（属 G01/G08），不做播放链固定五层编排（属 G07 F1324，本域只提供通用图与" +
      "五层作为预置模板的实例化能力）。",
    contract: "MixGraphContract",
  },
  "spatial-audio": {
    id: "spatial-audio",
    name: "空间音频",
    batches: ["H01", "H02"],
    boundary:
      "世界空间声源（listener / emitter、3D 定位、遮挡、混响区）。禁扩面：不做媒体" +
      "内容的双耳回放（属 G07 F1326，本域只提供世界空间引擎本体），不做场景几何" +
      "求解（几何来自 VE-I/J 3D 域，本域只留查询接口）。",
    contract: "SpatialAudioContract",
  },
  hrtf: {
    id: "hrtf",
    name: "HRTF",
    batches: ["H01", "H02"],
    boundary:
      "HRTF 数据集管理（SOFA加载、双耳渲染、HRIR 核）。禁扩面：不自带个人化测量" +
      "工具，不做医学级听觉建模；数据集来源合法性由音效包清洗（F1417）把关。",
    contract: "HrtfContract",
  },
  "audio-token": {
    id: "audio-token",
    name: "音频令牌",
    batches: ["H01", "H02"],
    boundary:
      "声音设计令牌（语义名 → 资源 + 参数，三级覆盖与主题联动）。禁扩面：不做音频" +
      "资源文件格式解码（F1421 资产层），令牌只声明「要什么」不负责「怎么读」。",
    contract: "AudioTokenContract",
  },
  fade: {
    id: "fade",
    name: "淡入淡出",
    batches: ["H01"],
    boundary:
      "全域fade 服务（曲线库、复合包络、交叉淡化编排、防重叠）。禁扩面：不直接调" +
      "设备音量（经图节点参数面），不做音频硬件淡入淡出控制（属H04 设备组）。",
    contract: "FadeServiceContract",
  },
  "event-bus": {
    id: "event-bus",
    name: "事件总线",
    batches: ["H01", "H02", "H03"],
    boundary:
      "音频事件发布-订阅（事件四要素、节流合并、优先级仲裁）。禁扩面：不承载" +
      "播放数据流（F1332 可视化总线是「正在播的」只读镜像，本总线是「该不该播」" +
      "决策流，两总线方向不同，禁止混用）。",
    contract: "AudioEventBusContract",
  },
  "low-latency": {
    id: "low-latency",
    name: "低延迟路径",
    batches: ["H01", "H03", "H04"],
    boundary:
      "端到端 ≤20ms 低延迟会话（小缓冲、underrun 特化恢复、实测上报）。禁扩面：" +
      "不为媒体回放通道牺牲延迟（高缓冲路径并存且互不拖累），不做 ASIO 驱动直连" +
      "（属 H04 预留位）。",
    contract: "LowLatencyContract",
  },
  resample: {
    id: "resample",
    name: "重采样",
    batches: ["H01", "H05"],
    boundary:
      "引擎级统一重采样服务（offline 批量 + realtime 流式双模式同核、五档质量）。" +
      "禁扩面：不做格式转换与封装（属 H05 工具组），不做媒体链内的隐式重采样" +
      "（播放链侧由 G07 调用本服务，不各自实现滤波器）。",
    contract: "ResampleServiceContract",
  },
  loudness: {
    id: "loudness",
    name: "响度归一",
    batches: ["H01", "H05", "H06"],
    boundary:
      "引擎级响度归一服务（批量 LUFS/True Peak 分析、指纹缓存、增量扫描、预设表）。" +
      "禁扩面：不做播放时对齐测量（G07 F1325 是播放链消费面，算法核在本域），" +
      "不做母带处理（属未来域，本域不冒充）。",
    contract: "LoudnessServiceContract",
  },
};

/** 主题 id 有序列表（按册内域地图顺序，遍历顺序稳定）。 */
export const THEME_IDS: readonly ThemeId[] = [
  "mix-graph",
  "spatial-audio",
  "hrtf",
  "audio-token",
  "fade",
  "event-bus",
  "low-latency",
  "resample",
  "loudness",
];

/** 查主题；未登记即失败（不返回 undefined 让上游自己判）。 */
export function lookupTheme(id: string): Outcome<ThemeSpec> {
  const spec = THEME_REGISTRY[id as ThemeId];
  if (!spec) {
    return fail(
      "THEME_UNREGISTERED",
      `主题 ${id} 不在 VE-H 官方九主题注册表内`,
      `九主题为：${THEME_IDS.join(" / ")}；若确为新主题，先改册内域地图再改本表`,
    );
  }
  return ok(spec);
}

/** 批次规格。 */
export interface BatchSpec {
  /** 批次 id（H01~H10）。 */
  readonly id: string;
  /** 中文名（册内批次标题原文）。 */
  readonly name: string;
  /** 承载的功能号区间（含两端）。 */
  readonly itemLo: number;
  /** 承载的功能号区间（含两端）。 */
  readonly itemHi: number;
  /** 本批次承载的主题（与 THEME_REGISTRY.batches 互为镜像，机检双向一致）。 */
  readonly themes: readonly ThemeId[];
  /** 本批次产出的收口件名（组收口条目统一产移交包）。 */
  readonly closureArtifact: string;
}

/** VE-H 十批次注册表（F1401~F1600，每组 20 项，与册内「域内分十组」一致）。 */
export const BATCH_REGISTRY: readonly BatchSpec[] = [
  {
    id: "H01",
    name: "引擎本体架构与混音图组",
    itemLo: 1401,
    itemHi: 1420,
    themes: ["mix-graph", "spatial-audio", "hrtf", "audio-token", "fade", "event-bus", "low-latency", "resample", "loudness"],
    closureArtifact: "H01KICKOFF_BUNDLE",
  },
  {
    id: "H02",
    name: "音效资产与合成组",
    itemLo: 1421,
    itemHi: 1440,
    themes: ["mix-graph", "spatial-audio", "hrtf", "audio-token", "event-bus"],
    closureArtifact: "H02_ASSET_BUNDLE",
  },
  {
    id: "H03",
    name: "采集通信与播放体验组",
    itemLo: 1441,
    itemHi: 1460,
    themes: ["event-bus", "low-latency"],
    closureArtifact: "H03_CAPTURE_BUNDLE",
  },
  {
    id: "H04",
    name: "系统集成与设备生态组",
    itemLo: 1461,
    itemHi: 1480,
    themes: ["low-latency", "fade"],
    closureArtifact: "H04_DEVICE_BUNDLE",
  },
  {
    id: "H05",
    name: "音频工具服务组",
    itemLo: 1481,
    itemHi: 1500,
    themes: ["resample", "loudness"],
    closureArtifact: "H05_TOOL_BUNDLE",
  },
  {
    id: "H06",
    name: "实时特效与聆听模式组",
    itemLo: 1501,
    itemHi: 1520,
    themes: ["loudness", "mix-graph"],
    closureArtifact: "H06_FX_BUNDLE",
  },
  {
    id: "H07",
    name: "性能与实时安全组",
    itemLo: 1521,
    itemHi: 1540,
    themes: ["mix-graph", "low-latency"],
    closureArtifact: "H07_PERF_BUNDLE",
  },
  {
    id: "H08",
    name: "生态扩展组",
    itemLo: 1541,
    itemHi: 1560,
    themes: ["audio-token", "mix-graph"],
    closureArtifact: "H08_ECO_BUNDLE",
  },
  {
    id: "H09",
    name: "质量与测试组",
    itemLo: 1561,
    itemHi: 1580,
    themes: ["mix-graph", "hrtf", "resample", "loudness"],
    closureArtifact: "H09_QUALITY_BUNDLE",
  },
  {
    id: "H10",
    name: "H 域收口组",
    itemLo: 1581,
    itemHi: 1600,
    themes: ["mix-graph", "spatial-audio", "audio-token", "event-bus", "low-latency", "resample", "loudness"],
    closureArtifact: "H10_CLOSURE_DECLARATION",
  },
];

/** 查批次；未登记即失败。 */
export function lookupBatch(id: string): Outcome<BatchSpec> {
  const spec = BATCH_REGISTRY.find((b) => b.id === id);
  if (!spec) {
    return fail(
      "BATCH_UNREGISTERED",
      `批次 ${id} 不在 VE-H 十批次注册表内`,
      `VE-H 批次为 H01~H10；引用前先确认条目号落在 F1401~F1600 区间内`,
    );
  }
  return ok(spec);
}

/** 条目号是否落在本域区间内（供各机检统一走这一个事实源）。 */
export function inDomainRange(item: number): boolean {
  return Number.isInteger(item) && item >= DOMAIN.itemLo && item <= DOMAIN.itemHi;
}

/**
 * 主题覆盖机检：九主题逐主题查「是否被至少一个批次承载」，并做双向镜像核对。
 * 返回被完整承载的主题 id 列表；任一主题悬空即失败并点名到具体主题。
 */
export function auditThemeCoverage(bag: DiagBag): Outcome<readonly ThemeId[]> {
  // 1) 正向：每个主题都要有承载批次。
  const covered: ThemeId[] = [];
  for (const id of THEME_IDS) {
    const spec = THEME_REGISTRY[id];
    if (spec.batches.length === 0) {
      bag.push(
        "THEME_COVERAGE_GAP",
        `主题 ${spec.name}（${id}）未被任何批次承载`,
        `该主题必须落到至少一个批次；若暂不排期，在册内标注「预留」而不是留空批次数组`,
      );
      continue;
    }
    covered.push(id);
  }

  // 2) 反向：批次声明的主题必须真实存在（防止批次里写错 id 却静默通过）。
  for (const b of BATCH_REGISTRY) {
    for (const t of b.themes) {
      if (!THEME_IDS.includes(t)) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `批次 ${b.id} 声明承载未知主题 ${t}`,
          `九主题 id 拼写以 THEME_REGISTRY 的键为准，修正批次 themes 数组`,
        );
      }
    }
  }

  // 3) 双向镜像：主题说归属某批次，批次也必须说承载该主题。
  for (const id of THEME_IDS) {
    for (const bid of THEME_REGISTRY[id].batches) {
      const b = BATCH_REGISTRY.find((x) => x.id === bid);
      if (!b) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `主题 ${id} 归属批次 ${bid}，但该批次未登记`,
          `先在 BATCH_REGISTRY 补登记 ${bid}，或改主题的 batches 为已登记批次`,
        );
        continue;
      }
      if (!b.themes.includes(id)) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `主题 ${id} 声明归属 ${bid}，但 ${bid} 的 themes 数组未反向登记该主题`,
          `两侧镜像必须一致；只写一侧会让覆盖机检误判为「有承载」`,
        );
      }
    }
  }

  // 4) 字段自相矛盾：主题契约名/边界为空 = 无下游可依。
  for (const id of THEME_IDS) {
    const s = THEME_REGISTRY[id];
    if (!s.contract.trim() || !s.boundary.trim() || !s.name.trim()) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `主题 ${id} 的名称 / 边界 / 契约名存在空串`,
        `边界句必须同时写「做什么」与「禁扩面」，否则下游条目会无约束地往外扩面`,
      );
    }
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "THEME_COVERAGE_GAP",
      message: `主题覆盖机检未过：${bag.size} 条诊断`,
      hint: `按 bag.all() 逐条修；主题表是 H01 二十项的挂载表，缺口会一路传导到 F1420 收口`,
      diagnostics: bag.all(),
    };
  }
  return ok(covered, bag.all());
}

/**
 * 批次区间机检：三件事一起做—— 区间在域内、区间不重叠、区间无缝。
 * 十批次 20 项 × 10 = 200 项，区间和必须恰好等于域区间。
 */
export function auditBatchRanges(bag: DiagBag): Outcome<number> {
  let declared = 0;
  for (const b of BATCH_REGISTRY) {
    if (b.itemLo > b.itemHi) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `批次 ${b.id} 区间倒置：${b.itemLo} > ${b.itemHi}`,
        `区间必须含两端且小端在前`,
      );
      continue;
    }
    if (!inDomainRange(b.itemLo) || !inDomainRange(b.itemHi)) {
      bag.push(
        "CAPABILITY_ITEM_OUT_OF_DOMAIN",
        `批次 ${b.id} 区间 [${b.itemLo}, ${b.itemHi}] 越出 VE-H 域区间 [${DOMAIN.itemLo}, ${DOMAIN.itemHi}]`,
        `域只决定序号区间；越界即映射到了别的域，先核对册内域地图`,
      );
      continue;
    }
    declared += b.itemHi - b.itemLo + 1;
  }

  // 重叠：两两比对。
  for (let i = 0; i < BATCH_REGISTRY.length; i += 1) {
    for (let j = i + 1; j < BATCH_REGISTRY.length; j += 1) {
      const a = BATCH_REGISTRY[i];
      const c = BATCH_REGISTRY[j];
      if (!a || !c) continue;
      if (a.itemLo <= c.itemHi && c.itemLo <= a.itemHi) {
        bag.push(
          "ITEM_RANGE_OVERLAP",
          `批次 ${a.id} [${a.itemLo}, ${a.itemHi}] 与 ${c.id} [${c.itemLo}, ${c.itemHi}] 区间重叠`,
          `两个批次抢同一批条目号；按册内「每项在全书中出现三次」原则重排`,
        );
      }
    }
  }

  // 无缝：相邻区间必须首尾相接（后一组的lo === 前一组的 hi + 1）。
  for (let i = 1; i < BATCH_REGISTRY.length; i += 1) {
    const prev = BATCH_REGISTRY[i - 1];
    const cur = BATCH_REGISTRY[i];
    if (!prev || !cur) continue;
    if (cur.itemLo !== prev.itemHi + 1) {
      const gap = cur.itemLo - prev.itemHi - 1;
      bag.push(
        gap > 0 ? "ITEM_RANGE_GAP" : "ITEM_RANGE_OVERLAP",
        gap > 0
          ? `批次 ${prev.id} 与 ${cur.id} 之间存在 ${gap} 项未分配条目号`
          : `批次 ${prev.id} 与 ${cur.id} 之间区间倒挂`,
        `VE-H 必须十批次无缝覆盖 F1401~F1600，缺口项要么补批次要么改区间`,
      );
    }
  }

  if (declared !== DOMAIN.itemCount) {
    bag.push(
      "ITEM_RANGE_GAP",
      `批次声明覆盖 ${declared} 项，与域应然 ${DOMAIN.itemCount} 项不符`,
      `以域区间 [${DOMAIN.itemLo}, ${DOMAIN.itemHi}] 为唯一事实源重新分配`,
    );
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "ITEM_RANGE_GAP",
      message: `批次区间机检未过：${bag.size} 条诊断`,
      hint: `区间错位会让 F1420 收口对账时出现「册内有、实现无」的幽灵项`,
      diagnostics: bag.all(),
    };
  }
  return ok(declared, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §3 边界契约 ADR 四主题落册（判据一：ADR 落册 + 四主题差异化 · F1381 规格 120 行）
// ════════════════════════════════════════════════════════════════════════════

/**
 * ADR 处置结论词汇表。
 *
 * 册内 F1381 原文：「每主题三选一（G 保留 / H 主导 / 合并 ADR）」。这三者
 * 不是同义词，混用会让后续争议无法回溯：
 *   - `g-retain`   G 域**完整保留**该主题的实现面（含算法核），H 域只调用不重写。
 *                 适用「本体极小、集成极厚」的主题——重采样、响度即此类。
 *   - `h-lead`     H 域**主导**该主题，G 域退化为消费者（调用 H 服务）。
 *                 适用「本体极厚、需要多消费者复用」的主题——混音、HRTF 即此类。
 *   - `merge-adr`  算法核**合并为共享核**、两侧服务层各自独立。适用于两侧都要
 *                 独立编排、但底层滤波/变换核完全同构的主题。
 *
 * 词汇表封闭：自造第四种处置会让 F1420 收口对账时「三选一」变成「四选一」，
 * 册内承诺失效。故 auditBoundaryAdr 对越界词汇产出 DISPOSITION_OUT_OF_VOCABULARY。
 */
export type Disposition = "g-retain" | "h-lead" | "merge-adr";

/** 处置结论的中文标签（对外文本引用同一事实源）。 */
export const DISPOSITION_LABEL: Readonly<Record<Disposition, string>> = {
  "g-retain": "G 保留",
  "h-lead": "H 主导",
  "merge-adr": "合并 ADR（核共享 / 服务分层）",
};

/** 处置结论的允许集合（有序，便于报错时按册内顺序提示）。 */
export const DISPOSITION_VOCABULARY: readonly Disposition[] = ["g-retain", "h-lead", "merge-adr"];

/** 处置结论是否是合法词汇表成员。 */
export function isDisposition(v: string): v is Disposition {
  return (DISPOSITION_VOCABULARY as readonly string[]).includes(v);
}

/** 视角差异：G07 与 H 域在同主题上的根本差别（ADR 的第一段，必填）。 */
export type Perspective = "playback-integration" | "engine-service";

/** 视角差异的中文标签。 */
export const PERSPECTIVE_LABEL: Readonly<Record<Perspective, string>> = {
  "playback-integration": "播放链音频集成视角（G07）",
  "engine-service": "引擎本体服务视角（VE-H）",
};

/** 四重叠主题 id（与册内 F1381 / F1397 的「四主题」严格同集）。 */
export type OverlapTopic = "mixing" | "hrtf" | "resample" | "loudness";

/** 四主题的有序列表（册内出现顺序：混音/HRTF/重采样/响度）。 */
export const OVERLAP_TOPICS: readonly OverlapTopic[] = ["mixing", "hrtf", "resample", "loudness"];

/** 四主题的中文名。 */
export const OVERLAP_TOPIC_LABEL: Readonly<Record<OverlapTopic, string>> = {
  mixing: "混音",
  hrtf: "HRTF",
  resample: "重采样",
  loudness: "响度",
};

/** 边界 ADR 一张卡的完整结构（三段必填 + 处置 + 回归条件）。 */
export interface BoundaryAdr {
  /** ADR 编号（本域内唯一，形如 ADR-H-01）。 */
  readonly id: string;
  /** 四主题之一。 */
  readonly topic: OverlapTopic;
  /** 主题中文名。 */
  readonly topicLabel: string;
  /** 对端（VE-G G07 组）相关条目号（册内原文条目号，出域即错）。 */
  readonly peerItems: readonly number[];
  /** 对端条目中文摘要（便于不回册也能读懂 ADR）。 */
  readonly peerSummary: string;
  /** 视角差异段：G 域与 H 域分别站在什么角度看这件事。 */
  readonly perspectiveG: string;
  readonly perspectiveH: string;
  /** 保留面段：G 域保留什么（其职责边界内的部分）。 */
  readonly retainG: string;
  /** 主导面段：H 域保留 / 主导什么。 */
  readonly leadH: string;
  /** 共享核段：哪些实现是同一份代码（同核同对拍的对象）。 */
  readonly sharedKernel: string;
  /** 服务层段：哪些编排逻辑明确不共享。 */
  readonly serviceLayerSplit: string;
  /** 处置结论（三选一，封闭词汇）。 */
  readonly disposition: Disposition;
  /** 处置理由（为什么是这三者之一，不是另两者）。 */
  readonly rationale: string;
  /** 回归条件：什么情况下这条 ADR 应当被后续 ADR 推翻重议。 */
  readonly regressionCondition: string;
  /** 本 ADR 冻结的版本号。 */
  readonly version: string;
}

/**
 * 四主题边界 ADR 表（F1401 判据一的主体，逐主题差异化定位入册）。
 *
 * 每张卡的 `rationale` 都写成「为什么不是另外两种处置」，这是 ADR 与
 * 备注的分界线：备注只说结论，ADR 必须说清排除项——否则半年后有人拿着
 * 新方案来改，唯一能挡住他的就是「你当初为什么没选另一条」这段文字。
 */
export const BOUNDARY_ADR_TABLE: readonly BoundaryAdr[] = [
  {
    id: "ADR-H-01",
    topic: "mixing",
    topicLabel: "混音",
    peerItems: [1324],
    peerSummary: "F1324 播放总线（固定五层：解码→重采样→音量→均衡→输出）",
    perspectiveG:
      "G07 从「一个播放器要有声音」出发，五层是**预设管线**：层序固定、层数固定、" +
      "生命周期与播放器一一绑定，目的是让播放器这一个人把声音按确定顺序送出去。",
    perspectiveH:
      "VE-H 从「音频能力要有本体」出发，混音图是**通用 DAG**：节点类型与连接关系" +
      "由使用方定义，目的是让播放链、系统音效、通知、游戏场景四类消费者共用" +
      "同一套引擎而各自编排自己的拓扑。",
    retainG:
      "G07 保留**播放链五层编排面**：五层的固定层序、与播放器实例的绑定、播放" +
      "起停时的总线初始化与释放。它保留的是一个「模板实例化 + 生命周期管理」。",
    leadH:
      "VE-H 主导**通用混音图本体**：四类节点（源 / 处理 / 总线 / 监听）、端口与" +
      "格式协商、建图三查（环 / 采样率 / 布局）、图热更新（等增益过渡不爆音）。" +
      "G07 的五层在 H 域看来只是**预置模板的一种实例化**，不是并列的第二套实现。",
    sharedKernel:
      "共享：biquad 七型（F1329 同一份实现，H 域 F1403 引用不复制）、参数平滑求值器" +
      "（F1329 同核，F1329 预留的自动化接口被 H 域 F1404 自动化挂接消费）、" +
      "音量/增益的线性—对数换算。",
    serviceLayerSplit:
      "不共享：图的构建与生命周期（播放器内部持有 vs 引擎服务持有）、总线初始化" +
      "时机、错误传播路径（播放链的解码失败直接终止播放 vs 引擎侧的节点级错误" +
      "隔离到分支）。混层判定：若 G07 开始自己实现 biquad，或 H 域开始接管播放器" +
      "生命周期，即为 LAYER_MIXING_VIOLATION。",
    disposition: "h-lead",
    rationale:
      "不选 g-retain：五层是**通用图的一个退化实例**，若 G 保留完整实现，H 域的" +
      "混音图就成了第二套拓扑系统，四类消费者无法共用，H02/H03 的多消费者架构" +
      "直接失效。不选 merge-adr：编排语义两侧完全不同（固定层序 vs 任意 DAG），" +
      "不存在可合并的中间形态——可合并的只有底层滤波核，那部分已由 sharedKernel 覆盖。",
    regressionCondition:
      "若 H 域 F1403 的通用图被证明无法表达播放器五层（即模板化实例化不可行，" +
      "或热更新语义与播放器生命周期冲突无法调和），则本 ADR 应被推翻重议为" +
      " merge-adr（核共享、拓扑层两侧独立）。触发证据以 F1420 收口对账为准。",
    version: "v1",
  },
  {
    id: "ADR-H-02",
    topic: "hrtf",
    topicLabel: "HRTF",
    peerItems: [1326],
    peerSummary: "F1326 双耳播放（媒体内容的预烘焙/即时双耳回放路径）",
    perspectiveG:
      "G07 从「媒体内容要听起来像在听者耳边」出发，双耳是**输出端的固定处理**：源" +
      "是媒体流，姿态是听者中心坐标系，目标是让立体声内容有空间感。",
    perspectiveH:
      "VE-H 从「场景里的声源有真实位置」出发，HRTF 是**世界空间引擎的一个环节**：源" +
      "是带 xyz 坐标的 emitter，姿态是 listener 的位置与朝向，目标是让 3D 场景中的" +
      "声源定位正确。",
    retainG:
      "G07 保留**媒体双耳回放面**：媒体源→双耳渲染→立体声输出的固定路径、" +
      "含基础 ITD/ILD 处理与预烘焙双耳资源的调用。它保留的是「一次媒体播放的" +
      "空间化需求」。",
    leadH:
      "VE-H 主导**世界空间引擎本体**：listener / emitter 模型（F1409）、球坐标与" +
      "笛卡尔双输入等价转换、三种距离模型、64 声源并发管理、HRTF 数据集（SOFA）" +
      "管理与双耳渲染深化（F1429/F1430）。G07 的双耳是本引擎的一个退化配置：" +
      "emitter 固定在正前方、listener 固定在原点。",
    sharedKernel:
      "共享：HRIR 卷积核（同一份实现，G 侧 F1326 与 H 侧 F1430 对拍同一基线）、" +
      "biquad（ITD/ILD 分频段处理用到的滤波）、FFT（同核，见 DSP_KERNEL_TABLE）。" +
      "注意：**卷积核共享 ≠ 渲染策略共享**，策略差异见 serviceLayerSplit。",
    serviceLayerSplit:
      "不共享：坐标来源（G 侧来自媒体元数据/预设方位；H 侧来自 3D 场景实时坐标）、" +
      "遮挡判定（G 侧不涉及；H 侧 F1410 走遮挡/半遮挡两档）、数据集选择策略" +
      "（G 侧用内置通用集；H 侧支持 SOFA 自定义集与实验性标注）。混层判定：若" +
      " G07 开始引入场景几何查询，或 H 域开始假设「源只有一个」，即violation。",
    disposition: "h-lead",
    rationale:
      "不选 g-retain：世界空间需要遮挡、距离模型、64 声源并发，这些在播放链集成视角" +
      "里无处安放，硬塞进 G07 会把播放器变成 3D 引擎。不选 merge-adr：两侧的" +
      "**坐标来源与遮挡判定**完全不同（一侧没有几何概念），合并渲染策略等于" +
      "让其中一侧实现对方用不到的分支。可合并的只有卷积核与biquad，已单列。",
    regressionCondition:
      "若实测发现 G07 的媒体双耳路径性能不可接受（卷积开销超出播放预算），需要" +
      "在 G 侧引入预烘焙缓存策略，且该策略与 H 域数据集管理耦合，则需重议为" +
      " merge-adr。触发证据以 H01 组收口（F1420）并发联测的实测延迟数据为准。",
    version: "v1",
  },
  {
    id: "ADR-H-03",
    topic: "resample",
    topicLabel: "重采样",
    peerItems: [1323],
    peerSummary: "F1323 播放链重采样（五档质量档位，媒体链内隐式重采样）",
    perspectiveG:
      "G07 从「媒体采样率与设备采样率不一致时要转换」出发，重采样是**播放链内的" +
      "必要环节**：触发条件明确（媒体源率 ≠ 设备率）、生命周期与播放器绑定、" +
      "目标是让媒体在当前设备上正确播放。",
    perspectiveH:
      "VE-H 从「重采样是一项可被多消费者复用的能力」出发，重采样是**独立服务**：" +
      "offline 批量（文件转采样）与 realtime 流式（播放/采集）双模式同核，" +
      "目标是让媒体、系统音效、通知、游戏场景共享同一滤波器与同一质量档定义。",
    retainG:
      "G07 保留**播放链触发面**：何时需要重采样（采样率不匹配判定）、档位选择策略" +
      "（按媒体类型与设备能力选档）、播放链内的实例化与释放。",
    leadH:
      "VE-H **主导算法核与服务化封装**：五档质量档定义（F1323 档位继承）、" +
      "双模式同核（F1413）、会话隔离与 CPU 计量（G 侧只有单实例，无从隔离）。" +
      "注意此处「主导」的是**核与档定义**，G 侧仍是自己实例的持有者与调用者。",
    sharedKernel:
      "共享：重采样滤波核本身（多相/线性插值实现同一份）、抗混叠低通（biquad " +
      "同核）、档位到参数（通带/阻带截止、相位）的映射表。**两侧必须使用同一份" +
      "档位定义**，否则「同核」只是同名不同参。",
    serviceLayerSplit:
      "不共享：实例生命周期（播放器持有 vs 会话持有）、触发时机、失败处理（播放链" +
      "重采样失败终止播放 vs 服务侧会话级错误隔离）。混层判定：若出现第二份档位" +
      "定义表（哪怕只差一个 dB），即为 LAYER_MIXING_VIOLATION——因为那意味着" +
      "对拍基线不再可比。",
    disposition: "merge-adr",
    rationale:
      "不选 h-lead：G07 是重采样**唯一的规模化消费者**，若核归 H 域独占，G 侧每次" +
      "播放都要跨域调服务，播放链的实时预算会被服务调度污染（F1412 ≤20ms 目标" +
      "与播放高缓冲路径必须互不拖累）。不选 g-retain：档位定义与滤波核若留在 G 域，" +
      "H 域的 offline 批量（F1413）就要复制一份，响度分析与转码工具（F1482 一带）" +
      "会再复制第三份——**两份定义就无法对拍**。故取折中：核与档定义共享，实例与" +
      "生命周期分层各自独立。",
    regressionCondition:
      "若实测显示跨域调用开销使 G 侧播放链延迟超出预算（以 F1420 并发联测实测为" +
      "准），则需把G 侧改为「本地实例 + 共享核二进制」形态，仍属 merge-adr，但" +
      "需补充一份二进制分发约定；若连二进制分发都不可行（如设备侧无共享内存），" +
      "则升级为 h-lead 并接受双份实现的代价。",
    version: "v1",
  },
  {
    id: "ADR-H-04",
    topic: "loudness",
    topicLabel: "响度",
    peerItems: [1325],
    peerSummary: "F1325 播放对齐（播放时的响度对齐/测量）",
    perspectiveG:
      "G07 从「这一条媒体播出来要和库里其他媒体听起来一样大」出发，响度是**播放时" +
      "的单文件对齐**：测一条、播一条、目标是消除条目间的响度差。",
    perspectiveH:
      "VE-H 从「整个媒体库的响度要可管理」出发，响度是**批量分析服务**：全库扫描" +
      "LUFS / True Peak、指纹缓存、增量扫描、预设表、报告导出，目标是让库级响度" +
      "**可查可控**，归一动作本身是可选的下游消费。",
    retainG:
      "G07 保留**播放时对齐面**：单文件测量调用、播放期增益调整、对齐失败时" +
      "「按原样播放并标注未测量」的降级路径。",
    leadH:
      "VE-H 主导**批量分析服务**：调度（F1414）、R128/流媒体/自定义预设表、" +
      "文件指纹→结果缓存（缓存键含分析算法版本，算法升级即失效重扫）、增量扫描、" +
      "报告导出。G07 的单文件测量是本服务的一个调用点，**不是**另一个实现。",
    sharedKernel:
      "共享：K-weighting 滤波链（biquad 组合，同核）、门限与积分算法（EBU R128 " +
      "的绝对门限 + 相对门限，同核）、True Peak 过采样检测（4x 过采样核 + biquad，" +
      "同核）。三者是「同核同对拍」的硬对象：对拍基线为EBU Tech 3342 测试向量，" +
      "两侧输出必须逐样本一致（误差上界见 SHARED_KERNEL_TABLE）。",
    serviceLayerSplit:
      "不共享：调度策略（单条即时 vs 批量排队）、缓存策略（G 侧无缓存，H 侧指纹缓存" +
      "是核心）、降级语义（G 侧降级=按原样播放；H 侧降级=标记未测量并进增量队列）。" +
      "混层判定：若 G 侧自行实现 K-weighting 滤波链，即为 violation。",
    disposition: "h-lead",
    rationale:
      "不选 g-retain：批量扫描与指纹缓存是**库级能力**，放进播放链视角下无处" +
      "承载（播放链一次只播一条，既无扫描时机也无缓存收益），而 F1414 的" +
      "增量扫描恰恰是批量能力独占的价值。不选 merge-adr：两侧连触发粒度都不同" +
      "（单条 vs 全库），调度层与缓存层根本没有共性可合并，能共享的只有算法核" +
      "——那已由 sharedKernel 单列。",
    regressionCondition:
      "若 G 侧播放时对齐被要求「不得有任何跨域阻塞」（例如与解码同线程的硬实时" +
      "约束），则需在 G 侧引入预计算缓存消费路径，并把缓存所有权重议为共享" +
      "（H 侧写、G 侧读），本 ADR 应修订为 merge-adr。触发证据以 F1414 与F1412" +
      "的联测报告为准。",
    version: "v1",
  },
];

/** 查主题对应的 ADR；未落册即失败（这是开工红线的执行点）。 */
export function lookupAdr(topic: string): Outcome<BoundaryAdr> {
  const adr = BOUNDARY_ADR_TABLE.find((a) => a.topic === topic);
  if (!adr) {
    return fail(
      "ADR_INCOMPLETE",
      `四主题 ${topic} 的边界 ADR 未落册——册内明文「无 ADR 不开工」`,
      `四主题为 ${OVERLAP_TOPICS.join(" / ")}；每主题必须恰好一张 ADR，含视角差异、` +
        `保留面、共享核三段与处置结论`,
    );
  }
  return ok(adr);
}

/**
 * 边界 ADR 门禁机检（六查，任一不通过即产出诊断并阻断开工）：
 *   1. 四主题齐备——缺一张即 ADR_INCOMPLETE；
 *   2. 一主题一 ADR——多张即 ADR_DUPLICATED（法理冲突）；
 *   3. 三段完整——视角差异 / 保留面 / 共享核缺一即 ADR_SECTION_MISSING；
 *   4. 处置在词汇表内——越界即 DISPOSITION_OUT_OF_VOCABULARY；
 *   5. 对端条目在 G 域区间内——越界即 PEER_ITEM_OUT_OF_RANGE；
 *   6. 回归条件非空——缺即 ADR_REGRESSION_CONDITION_MISSING。
 */
export function auditBoundaryAdr(bag: DiagBag): Outcome<readonly BoundaryAdr[]> {
  // 1) 四主题齐备。
  for (const t of OVERLAP_TOPICS) {
    const hit = BOUNDARY_ADR_TABLE.filter((a) => a.topic === t);
    if (hit.length === 0) {
      bag.push(
        "ADR_INCOMPLETE",
        `四主题「${OVERLAP_TOPIC_LABEL[t]}」的边界 ADR 未落册`,
        `册内 F1401 判据一为「ADR 落册」且明文「无 ADR 不开工」；` +
          `补齐该主题的 ADR 后重跑本机检`,
      );
    }
  }

  // 2) 一主题一 ADR。
  for (const t of OVERLAP_TOPICS) {
    const hit = BOUNDARY_ADR_TABLE.filter((a) => a.topic === t);
    if (hit.length > 1) {
      bag.push(
        "ADR_DUPLICATED",
        `四主题「${OVERLAP_TOPIC_LABEL[t]}」登记了 ${hit.length} 张 ADR（${hit
          .map((a) => a.id)
          .join(" / ")}）`,
        `一个主题一张法理卡；两张 ADR 对同一主题给不同处置= 后续争议无法回溯，` +
          `请合并为一张并把被否方案写进 rationale 的排除项`,
      );
    }
  }

  // 3~6) 逐张 ADR 的段落与字段。
  for (const adr of BOUNDARY_ADR_TABLE) {
    // 3) 三段完整。
    const sections: readonly (readonly [string, string])[] = [
      ["视角差异（G）", adr.perspectiveG],
      ["视角差异（H）", adr.perspectiveH],
      ["保留面（G）", adr.retainG],
      ["主导面（H）", adr.leadH],
      ["共享核", adr.sharedKernel],
      ["服务层不共享声明", adr.serviceLayerSplit],
    ];
    for (const [label, text] of sections) {
      if (!text || !text.trim()) {
        bag.push(
          "ADR_SECTION_MISSING",
          `${adr.id}（${adr.topicLabel}）的${label}段为空`,
          `ADR 的价值在三段齐备：只写结论不写差异的 ADR 等于备注；` +
            `补齐后 H01 二十项才有一致的边界依据`,
        );
      }
    }

    // 4) 处置在封闭词汇表内。
    if (!isDisposition(adr.disposition)) {
      bag.push(
        "DISPOSITION_OUT_OF_VOCABULARY",
        `${adr.id} 的处置结论「${String(adr.disposition)}」不在册内三选一词汇表内`,
        `F1381 明文只允许 ${DISPOSITION_VOCABULARY.map((d) => DISPOSITION_LABEL[d]).join(" / ")}；` +
          `若确需第四种处置，先改册内 F1381，再改本表`,
      );
    }

    // 5) 对端条目必须在 G 域区间（F1201-F1400）。
    if (adr.peerItems.length === 0) {
      bag.push(
        "PEER_ITEM_OUT_OF_RANGE",
        `${adr.id} 未登记对端条目号`,
        `ADR 必须指名 G07 侧的具体条目（如 1323），否则「保留面」无从校验`,
      );
    }
    for (const it of adr.peerItems) {
      if (!Number.isInteger(it) || it < 1201 || it > 1400) {
        bag.push(
          "PEER_ITEM_OUT_OF_RANGE",
          `${adr.id} 的对端条目 F${it} 不在 VE-G 域区间 [1201, 1400] 内`,
          `对端条目写错会让 ADR 指向不存在的实现；回册内 F1381 / F1397 核对条目号`,
        );
      }
    }

    // 6) 回归条件非空。
    if (!adr.regressionCondition || !adr.regressionCondition.trim()) {
      bag.push(
        "ADR_REGRESSION_CONDITION_MISSING",
        `${adr.id} 缺少回归条件`,
        `无回归条件的 ADR = 永久占用：后续 ADR 无法推翻它，只能叠加冲突结论；` +
          `写清「什么证据出现时应重议」`,
      );
    }

    // 附加：rationale 必须同时覆盖另外两种处置的排除理由。
    if (!adr.rationale || !adr.rationale.trim()) {
      bag.push(
        "ADR_SECTION_MISSING",
        `${adr.id} 缺少处置理由`,
        `理由段要写「为什么不是另两者」，这是 ADR 与备注的分界线`,
      );
    }
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "ADR_INCOMPLETE",
      message: `边界 ADR 门禁未过：${bag.size} 条诊断`,
      hint: `边界 ADR 是 H 域开工的法理前提（册内「无 ADR 不开工」）；` +
        `修完 ADR 再动F1402 及之后的实现条目`,
      diagnostics: bag.all(),
    };
  }
  return ok(BOUNDARY_ADR_TABLE.slice(), bag.all());
}

/**
 * 处置结论投影：把四张 ADR 压成「谁主导哪一块」的一张可查表。
 * F1419（API 冻结）与 F1420（收口）按这张表核对两份 API 的分工，避免互补声明写虚。
 */
/** 一主题在两域的保留面投影（一主题可两侧都有保留面）。 */
export interface TopicRetentionProjection {
  /** 主题 id。 */
  readonly topic: OverlapTopic;
  /** 主题中文名。 */
  readonly topicLabel: string;
  /** 处置结论。 */
  readonly disposition: Disposition;
  /** G 域（VE-G G07）保留的能力（取自查重台账的 g-only / shared 条目）。 */
  readonly retainedByG: readonly string[];
  /** H 域（VE-H）保留的能力（取自查重台账的 h-only / shared 条目）。 */
  readonly retainedByH: readonly string[];
  /** 该主题共享的能力（核级，两域同源）。 */
  readonly sharedKernels: readonly string[];
}

/**
 * 四主题差异化投影（F1419 互补声明与 F1420 收口的引用源）。
 *
 * **语义澄清（初版缺陷，已修）**：本函数早先按「处置结论」划分保留面
 * （g-retain 归G，其余归 H），实测输出 `VE-G=[]`——与四张 ADR 里逐条写明的
 * 「保留面（G）」直接矛盾：即使处置是 h-lead，G 侧仍然保留着播放链五层编排、
 * 媒体双耳回放路径、单文件响度对齐这些**集成面**。处置结论回答的是
 * 「算法核归谁」，保留面回答的是「谁拥有哪一块实现」，两者是正交的两个问题，
 * 不能互相推导。故本函数改为**从查重台账取事实**：台账里每条能力都已判过
 * 三态（shared / h-only / g-only），按主题聚合即得两侧的真实保留面。
 * ADR 的 retainG / leadH 字段是法理表述，台账是逐能力事实，两者交叉核对
 * 才不会让「声明」与「实现」分家。
 */
export function projectTopicRetention(): Outcome<readonly TopicRetentionProjection[]> {
  const bag = new DiagBag();
  const adrOutcome = auditBoundaryAdr(bag);
  if (adrOutcome.ok === false) {
    return {
      ok: false,
      code: adrOutcome.code,
      message: adrOutcome.message,
      hint: adrOutcome.hint,
      diagnostics: adrOutcome.diagnostics,
    };
  }
  const dedupOutcome = auditDedupLedger(bag);
  if (dedupOutcome.ok === false) {
    return {
      ok: false,
      code: dedupOutcome.code,
      message: dedupOutcome.message,
      hint: dedupOutcome.hint,
      diagnostics: dedupOutcome.diagnostics,
    };
  }

  const out: TopicRetentionProjection[] = [];
  for (const adr of BOUNDARY_ADR_TABLE) {
    // 台账中属于本 ADR 主题域的条目：按能力名归属到四主题之一。
    const entries = DEDUP_LEDGER.filter((e) => e.adrId === adr.id);
    const retainedByG = entries
      .filter((e) => e.verdict === "dedup-g-only" || e.verdict === "dedup-shared")
      .map((e) => e.capability);
    const retainedByH = entries
      .filter((e) => e.verdict === "dedup-h-only" || e.verdict === "dedup-shared")
      .map((e) => e.capability);
    const sharedKernels = entries
      .filter((e) => e.verdict === "dedup-shared")
      .map((e) => e.capability);

    // 两侧都空 = 该主题的 ADR 与台账脱节（声明了边界却没有任何能力入账）。
    if (retainedByG.length === 0 && retainedByH.length === 0) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `四主题「${adr.topicLabel}」的 ADR ${adr.id} 在查重台账中无任何对应能力条目`,
        `ADR 声明了边界，台账却没落对应能力——声明与事实分家时，边界等于没写；` +
          `补台账条目（指向本 ADR）`,
      );
    }
    // ADR 写了 G 保留面，台账却一条 g-only/shared 都没有 = 声明落空。
    if (adr.retainG.trim().length > 0 && retainedByG.length === 0) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `四主题「${adr.topicLabel}」的 ADR 声明了 G 保留面，台账中却无 G 侧保留条目`,
        `法理表述与逐能力事实必须一致；要么台账补 g-only/shared 条目，要么 ADR 改写保留面`,
      );
    }

    out.push({
      topic: adr.topic,
      topicLabel: adr.topicLabel,
      disposition: adr.disposition,
      retainedByG,
      retainedByH,
      sharedKernels,
    });
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "DOMAIN_DUPLICATE_FOUND",
      message: `四主题差异化投影未过：${bag.size} 条诊断`,
      hint: `投影是 F1419 互补声明的引用源；投影与 ADR 不一致时互补声明会写虚`,
      diagnostics: bag.all(),
    };
  }
  return ok(out, bag.all());
}

/**
 * 处置结论投影（按 ADR 处置词划分**算法核主导面**，与保留面投影正交）。
 * 注意语义：这里回答的是「核归谁」，不是「谁保留什么实现」——
 * 后者请用 {@link projectTopicRetention}。
 */
export function projectDispositionByDomain(): Outcome<Readonly<Record<"VE-G" | "VE-H", readonly OverlapTopic[]>>> {
  const bag = new DiagBag();
  const adrOutcome = auditBoundaryAdr(bag);
  if (adrOutcome.ok === false) {
    return {
      ok: false,
      code: adrOutcome.code,
      message: adrOutcome.message,
      hint: adrOutcome.hint,
      diagnostics: adrOutcome.diagnostics,
    };
  }
  // h-lead 与 merge-adr 的算法核主导面在 H 域；g-retain 在 G 域。
  const gTopics = BOUNDARY_ADR_TABLE.filter((a) => a.disposition === "g-retain").map((a) => a.topic);
  const hTopics = BOUNDARY_ADR_TABLE.filter((a) => a.disposition !== "g-retain").map((a) => a.topic);
  return ok({ "VE-G": gTopics, "VE-H": hTopics }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 复用声明与共享核抽定（判据二：复用声明 · F1401 规格 110 行）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 复用声明的三条硬约束（册内原文：「核心 DSP 核共享实现，服务层各自独立
 * ——不重复造轮子也不混层：复用边界（DSP 核共享库（biquad/FFT/卷积核）共享库
 * （两域同核同对拍）——服务层各自独立」）：
 *
 *   1. **不重复造轮子**：同一算法核在两域各写一份，代价不是磁盘空间，而是
 *      对拍不可能——两份实现的浮点运算序一旦不同（编译器向量化、FMA 收缩、
 *      循环展开），输出会差在末位比特，之后所有「A/B 对比」「回归基线」
 *      全都失去意义。故共享核必须**同一份实现 + 同一份对拍基线**。
 *   2. **不混层**：共享只发生在「无状态、无会话、无生命周期」的算法核上。
 *      任何带生命周期的东西（实例持有、缓冲分配、错误传播、会话隔离）
 *      都属于服务层，两侧各自独立。这条边界是硬的：一旦把服务层语义
 *      塞进共享核，跨域调用就会拖着对方的状态机走。
 *   3. **同核同对拍**：共享核必须登记**对拍基线标识**与**误差上界**。
 *      没有基线的共享核是同名不同参——这正是 ADR-H-03 里「两份档位定义
 *      表」被列为 violation 的原因。
 */

/** 共享核 id。册内点名的三个：biquad / FFT / 卷积核。 */
export type SharedKernelId = "biquad" | "fft" | "convolution";

/** 共享核规格。 */
export interface SharedKernelSpec {
  readonly id: SharedKernelId;
  /** 中文名。 */
  readonly name: string;
  /** 归属的 ADR（该核由哪张边界 ADR 认定为共享）。 */
  readonly adrId: string;
  /** G 域消费入口（条目号 + 契约名）。 */
  readonly gConsumer: { readonly item: number; readonly contract: string };
  /** H 域消费入口（条目号 + 契约名）。 */
  readonly hConsumer: { readonly item: number; readonly contract: string };
  /** 对拍基线标识：两侧对同一输入必须产出同一结果的那份参考。 */
  readonly parityBaseline: string;
  /** 误差上界（绝对值或相对值；共享核必须给出可机检的界）。 */
  readonly tolerance: string;
  /** 对拍执行方式（逐样本 / 统计等价——FFT 类变换允许统计等价）。 */
  readonly parityMode: "sample-exact" | "statistical-equivalent";
  /** 无状态性声明：共享核必须无会话、无生命周期、无可变全局态。 */
  readonly stateless: true;
  /** 明确不共享的部分（把边界写进规格本身，而不是只写在 ADR 里）。 */
  readonly notShared: string;
}

/** DSP 共享核表（biquad / FFT / 卷积核——册内点名的三个）。 */
export const DSP_KERNEL_TABLE: Readonly<Record<SharedKernelId, SharedKernelSpec>> = {
  biquad: {
    id: "biquad",
    name: "双二阶滤波器核",
    adrId: "ADR-H-01",
    gConsumer: { item: 1329, contract: "G07_EQ_BIQUAD" },
    hConsumer: { item: 1403, contract: "MixGraphFilterNode" },
    parityBaseline: "RB-BIQUAD-BASELINE-V1（十类系数 × 阶梯输入的逐样本参考输出）",
    tolerance: "绝对误差 ≤ 1 ULP（双精度）；FMA 收缩开关须在两侧一致",
    parityMode: "sample-exact",
    stateless: true,
    notShared:
      "不共享：节点的参数包络求值（谁持有包络、谁求值）、参数平滑的时间轴" +
      "（G 侧挂在播放器时钟，H 侧挂在引擎时钟 F1415）、EQ 预设的UI 形态。",
  },
  fft: {
    id: "fft",
    name: "快速傅里叶变换核",
    adrId: "ADR-H-02",
    gConsumer: { item: 1326, contract: "G07_DUAL_EARL_FFT" },
    hConsumer: { item: 1430, contract: "HrtfConvolutionEngine" },
    parityBaseline: "RB-FFT-BASELINE-V1（八种长度 × 三类输入谱的参考能量分布）",
    tolerance: "各频点幅度相对误差 ≤ 1e-9；相位差 ≤ 1e-9 rad",
    parityMode: "statistical-equivalent",
    stateless: true,
    notShared:
      "不共享：分帧与重叠策略（FFT 分帧大小由各自的实时预算决定）、窗函数" +
      "的选取（G 侧固定，H 侧随 HRIR 长度自适应）、重叠相加的缓冲管理。",
  },
  convolution: {
    id: "convolution",
    name: "卷积核（HRIR / IR 通用）",
    adrId: "ADR-H-02",
    gConsumer: { item: 1326, contract: "G07_DUAL_EARL_CONV" },
    hConsumer: { item: 1429, contract: "HrtfDatasetRuntime" },
    parityBaseline: "RB-CONV-BASELINE-V1（三组 HRIR × 单/双耳输入的参考脉冲响应输出）",
    tolerance: "绝对误差 ≤ 1e-6（浮点累加序敏感，允许多重累加但须在基线中声明累加序）",
    parityMode: "sample-exact",
    stateless: true,
    notShared:
      "不共享：IR 数据集的装载与校验（H 侧 SOFA 校验）、数据集选择策略、" +
      "缓存策略（H 侧 IR 常驻，G 侧不常驻）、卷积的分区与调度（H 侧分块并行）。",
  },
};

/** 共享核 id 有序列表。 */
export const SHARED_KERNEL_IDS: readonly SharedKernelId[] = ["biquad", "fft", "convolution"];

/**
 * 服务层边界表：这一层**明确不共享**，两侧各自独立。
 * 把不共享的东西也建成表，是因为「不共享」是最容易被无意破坏的边界——
 * 开发时看到两边代码相似，顺手就抽了，比「两边各写一份」更难发现。
 */
export interface LayerBoundarySpec {
  /** 层标识。 */
  readonly id: string;
  /** 中文名。 */
  readonly name: string;
  /** G 域这一层的持有者与语义。 */
  readonly gSide: string;
  /** H 域这一层的持有者与语义。 */
  readonly hSide: string;
  /** 混层判据：出现哪种情况即 LAYER_MIXING_VIOLATION。 */
  readonly mixingViolation: string;
}

/** 服务层边界表（G07 播放链编排 vs VE-H 引擎调度）。 */
export const LAYER_BOUNDARY_TABLE: readonly LayerBoundarySpec[] = [
  {
    id: "graph-lifecycle",
    name: "拓扑与生命周期",
    gSide: "播放器持有五层总线，随播放会话创建与释放",
    hSide: "引擎服务持混音图，会话与图的生命周期分离（图可跨会话复用）",
    mixingViolation: "G 侧开始自建通用 DAG，或 H 侧开始替播放器管理播放会话生命周期",
  },
  {
    id: "instance-ownership",
    name: "实例所有权",
    gSide: "重采样/响度实例由播放器实例持有，单实例",
    hSide: "实例由会话持有，会话间隔离（会话配额 F1416 按会话分账）",
    mixingViolation: "H 侧把会话隔离改成全局单例，或 G 侧要求跨会话共享实例",
  },
  {
    id: "clock",
    name: "时钟域",
    gSide: "播放主时钟 F1328（播放器自己的时间基准）",
    hSide: "引擎时钟 F1415（多消费者共享基准 + 每会话偏移管理）",
    mixingViolation: "两侧各自维护一份「主时钟」却都认为是对方给的",
  },
  {
    id: "error-propagation",
    name: "错误传播",
    gSide: "解码/重采样失败 → 终止播放并报错（面向用户）",
    hSide: "节点级失败 → 隔离到分支，图继续渲染，错误进遥测（F1418）",
    mixingViolation: "H 侧把失败冒泡到消费者，或 G 侧把图级失败当播放失败处理",
  },
  {
    id: "policy",
    name: "策略层",
    gSide: "播放链的档位/对齐策略（按媒体类型选档、单条即时测量）",
    hSide: "服务默认策略（可被消费者覆盖，显性可查）",
    mixingViolation: "策略表被复制成两份（同名不同参 = ADR-H-03 已声明的 violation）",
  },
];

/**
 * 混层检测器：给定一条操作声明（属于哪一层、由哪一侧发起），判断是否越层。
 * 这是「不混层」红线的可执行形态——开发期拦下，而不是 F1420 收官时才发现。
 */
export function checkLayerMixing(
  operation: { readonly layerId: string; readonly actor: "VE-G" | "VE-H"; readonly touchesSharedKernel: boolean },
  bag: DiagBag
): Outcome<LayerBoundarySpec> {
  const spec = LAYER_BOUNDARY_TABLE.find((l) => l.id === operation.layerId);
  if (!spec) {
    return fail(
      "LAYER_MIXING_VIOLATION",
      `操作声明的层 ${operation.layerId} 不在服务层边界表内`,
      `合法层为：${LAYER_BOUNDARY_TABLE.map((l) => l.id).join(" / ")}；` +
        `若确属共享核，请改走 DSP_KERNEL_TABLE 而非服务层`,
    );
  }
  if (operation.touchesSharedKernel) {
    bag.push(
      "LAYER_MIXING_VIOLATION",
      `层 ${spec.name} 被声明为触碰共享核`,
      `共享核只承载无状态算法（见 DSP_KERNEL_TABLE 的 stateless），` +
        `${spec.name} 带生命周期与持有者语义，不属共享核`,
    );
    return {
      ok: false,
      code: "LAYER_MIXING_VIOLATION",
      message: `层 ${spec.name} 混入共享核`,
      hint: `把算法部分下沉到 DSP_KERNEL_TABLE，把持有/生命周期留在服务层`,
      diagnostics: bag.all(),
    };
  }
  return ok(spec, bag.all());
}

/** 共享核引用合法性：引用未登记的核即失败（防「我以为它共享」）。 */
export function resolveSharedKernel(id: string): Outcome<SharedKernelSpec> {
  const spec = DSP_KERNEL_TABLE[id as SharedKernelId];
  if (!spec) {
    return fail(
      "SHARED_KERNEL_UNREGISTERED",
      `共享核 ${id} 不在 DSP_KERNEL_TABLE 内`,
      `册内点名的共享核为 ${SHARED_KERNEL_IDS.join(" / ")}；` +
        `新增共享核必须先在此表登记对拍基线与误差上界`,
    );
  }
  return ok(spec);
}

/**
 * 共享核表机检（四查）：
 *   1. 三核齐备（册内点名biquad / FFT / 卷积核）；
 *   2. 每个核必须有对拍基线与误差上界（无基线的共享核是同名不同参）；
 *   3. 每个核必须声明无状态（stateless=true），否则它其实是服务层；
 *   4. 对端条目必须在册内区间内（G 侧 1201-1400，H 侧 1401-1600）。
 */
export function auditSharedKernels(bag: DiagBag): Outcome<readonly SharedKernelId[]> {
  // 1) 三核齐备。
  for (const id of SHARED_KERNEL_IDS) {
    const spec = DSP_KERNEL_TABLE[id];
    if (!spec) {
      bag.push(
        "SHARED_KERNEL_UNREGISTERED",
        `共享核 ${id} 未在 DSP_KERNEL_TABLE 登记`,
        `册内「DSP 核（biquad/FFT/卷积核）共享库」点名的三个核必须齐备`,
      );
      continue;
    }

    // 2) 对拍基线与误差上界。
    if (!spec.parityBaseline || !spec.parityBaseline.trim()) {
      bag.push(
        "PARITY_BASELINE_MISSING",
        `共享核 ${id} 未登记对拍基线`,
        `同核同对拍的前提是同一份参考输出；没有基线就只能靠肉耳听辨，不构成对拍`,
      );
    }
    if (!spec.tolerance || !spec.tolerance.trim()) {
      bag.push(
        "SHARED_KERNEL_SPEC_INCONSISTENT",
        `共享核 ${id} 未登记误差上界`,
        `对拍必须有可机检的界（绝对误差 / 相对误差），否则「通过」无定义`,
      );
    }
    if (spec.parityMode !== "sample-exact" && spec.parityMode !== "statistical-equivalent") {
      bag.push(
        "SHARED_KERNEL_SPEC_INCONSISTENT",
        `共享核 ${id} 的对拍模式非法：${String(spec.parityMode)}`,
        `对拍模式只有两种：逐样本一致，或统计等价（频点幅度/相位误差界）`,
      );
    }

    // 3) 无状态声明。
    if (spec.stateless !== true) {
      bag.push(
        "SHARED_KERNEL_SPEC_INCONSISTENT",
        `共享核 ${id} 未声明无状态`,
        `带会话/生命周期/可变全局态的实现属于服务层，不进共享核表`,
      );
    }
    if (!spec.notShared || !spec.notShared.trim()) {
      bag.push(
        "SHARED_KERNEL_SPEC_INCONSISTENT",
        `共享核 ${id} 未声明「不共享」的内容`,
        `把不共享的部分写进规格本身，才能挡住开发时顺手抽公共代码的动作`,
      );
    }

    // 4) 对端条目区间。
    const g = spec.gConsumer.item;
    const h = spec.hConsumer.item;
    if (!Number.isInteger(g) || g < 1201 || g > 1400) {
      bag.push(
        "PEER_ITEM_OUT_OF_RANGE",
        `共享核 ${id} 的 G 侧消费条目 F${g} 不在 VE-G 区间 [1201, 1400] 内`,
        `回册内 F1381 核对条目号`,
      );
    }
    if (!Number.isInteger(h) || h < 1401 || h > 1600) {
      bag.push(
        "CAPABILITY_ITEM_OUT_OF_DOMAIN",
        `共享核 ${id} 的 H 侧消费条目 F${h} 不在 VE-H 区间 [1401, 1600] 内`,
        `H 侧消费条目应是本域条目；若指向别的域，说明该核的归属判错了`,
      );
    }
    if (!spec.gConsumer.contract.trim() || !spec.hConsumer.contract.trim()) {
      bag.push(
        "SHARED_KERNEL_SPEC_INCONSISTENT",
        `共享核 ${id} 的消费契约名为空`,
        `两侧各留一个契约名，F1419 冻结时按契约名对齐，不按条目号硬编码`,
      );
    }
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "SHARED_KERNEL_UNREGISTERED",
      message: `共享核表机检未过：${bag.size} 条诊断`,
      hint: `共享核声明是「不重复造轮子」的唯一凭据；机检不过则F1403/F1429/F1430 各自实现的风险已成立`,
      diagnostics: bag.all(),
    };
  }
  return ok(SHARED_KERNEL_IDS.slice(), bag.all());
}

/**
 * 复用声明文本（对外可引用的一句话结论，同时是 F1419 互补声明的引用源）。
 * 返回确定性文本，不含随机、不含时间戳——便于对拍与快照哈希。
 */
export function renderReuseDeclaration(): Outcome<string> {
  const bag = new DiagBag();
  const adr = auditBoundaryAdr(bag);
  const kernels = auditSharedKernels(bag);
  if (adr.ok === false) {
    return {
      ok: false,
      code: adr.code,
      message: adr.message,
      hint: adr.hint,
      diagnostics: adr.diagnostics,
    };
  }
  if (kernels.ok === false) {
    return {
      ok: false,
      code: kernels.code,
      message: kernels.message,
      hint: kernels.hint,
      diagnostics: kernels.diagnostics,
    };
  }
  const lines: string[] = [];
  lines.push(
    `复用声明（VE-F1401 · ${DOMAIN.id} ${DOMAIN.name}）：核心 DSP 核共享实现，服务层各自独立。`
  );
  lines.push(
    `共享核三件（两域同核同对拍）：${SHARED_KERNEL_IDS.map((k) => {
      const s = DSP_KERNEL_TABLE[k];
      const adrOfKernel = BOUNDARY_ADR_TABLE.find((a) => a.id === s.adrId);
      const disp = adrOfKernel ? DISPOSITION_LABEL[adrOfKernel.disposition] : "未绑定 ADR";
      return `${s.name}[${s.id}]（归属 ${s.adrId} · ${disp}）`;
    }).join("、")}。`
  );
  lines.push(
    `共享核的完整清单、对拍基线与误差上界以DSP_KERNEL_TABLE 为唯一事实源；` +
      `服务层的五层不共享边界以 LAYER_BOUNDARY_TABLE 为唯一事实源。`
  );
  lines.push(
    `四主题处置：${BOUNDARY_ADR_TABLE.map((a) => `${a.topicLabel}=${DISPOSITION_LABEL[a.disposition]}`).join("、")}。`
  );
  lines.push(
    `四主题保留面（逐能力事实，取自查重台账而非处置词）：` +
      BOUNDARY_ADR_TABLE.map((a) => {
        const gRows = DEDUP_LEDGER.filter(
          (e) => e.adrId === a.id && (e.verdict === "dedup-g-only" || e.verdict === "dedup-shared")
        ).length;
        const hRows = DEDUP_LEDGER.filter(
          (e) => e.adrId === a.id && (e.verdict === "dedup-h-only" || e.verdict === "dedup-shared")
        ).length;
        const sharedRows = DEDUP_LEDGER.filter((e) => e.adrId === a.id && e.verdict === "dedup-shared").length;
        return `${a.topicLabel}(G侧${gRows}/H侧${hRows}/共享${sharedRows})`;
      }).join("、") +
      `。处置词回答「核归谁」，保留面回答「谁拥有哪一块实现」，两者正交不可互推。`
  );
  return ok(lines.join("\n"), bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 查重对账执行 + 上游移交接收（判据三：查重对账 · F1401 规格 90 行）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 上游移交接收：G 域（F1381 接口总账 / F1397 移交包）→ VE-H（F1401 开工）。
 *
 *册内 F1381 与F1397 把边界契约的**一半**留在了G 域：四主题处置建议、
 *高风险对的识别、官方描述词表的认领。VE-H 的开工条必须**消费执行**这一半，
 * 而不是重新讨论一遍——这正是「处置建议是 H 域开工的直接输入（清单化防口头
 * 交接）」的含义。故本节把移交包的接收做成**逐条核销**：缺件点名到具体件，
 * 快照失真点名到具体哈希，义务未签点名到具体义务。
 */

/** 移交包件名（F1397 四件：冻结清单 / 边界清单 / 教训记录 / 归档索引）。 */
export type HandoverArtifact = "interface-freeze-list" | "audio-boundary-list" | "lesson-ledger" | "evidence-index";

/** 移交包件的中文名。 */
export const HANDOVER_ARTIFACT_LABEL: Readonly<Record<HandoverArtifact, string>> = {
  "interface-freeze-list": "接口冻结清单（F1381 总账 + 边界契约）",
  "audio-boundary-list": "音频边界清单（四主题逐项处置建议）",
  "lesson-ledger": "经验教训记录（九组开发坑与决策依据）",
  "evidence-index": "证据三件套归档索引",
};

/** G 域宣告的移交包快照值（本域引用同一事实源，不各自硬编码哈希）。 */
export const HANDOVER_SNAPSHOT = {
  /** 产出移交包的条目号。 */
  producerItem: 1397,
  /** 接口总账条目号（F1381）。 */
  ledgerItem: 1381,
  /** G 域收官宣告条目号（F1400）。 */
  closeDeclarationItem: 1400,
  /** 快照标识（内容哈希由G 域产出后回填；此处为声明值）。 */
  snapshotId: "SNAPSHOT-G-HANDOVER-V1",
  /** 快照覆盖的四主题。 */
  coversTopics: OVERLAP_TOPICS.slice(),
} as const;

/** 开工前置义务：VE-H 承诺逐条核销的项（未核销即阻断开工）。 */
export interface KickerObligation {
  /** 义务 id。 */
  readonly id: string;
  /** 义务内容（人话）。 */
  readonly text: string;
  /** 对应的可机检依据（本模块提供什么函数来证明已核销）。 */
  readonly evidenceFn: string;
}

/** 开工前置义务表（F1397 边界清单 → H 域的逐条承接）。 */
export const KICKOFF_OBLIGATIONS: readonly KickerObligation[] = [
  {
    id: "OBL-ADR-LANDED",
    text: "四主题逐项落 ADR（混音 / HRTF / 重采样 / 响度），无 ADR 不开工",
    evidenceFn: "auditBoundaryAdr",
  },
  {
    id: "OBL-NO-SECOND-IMPL",
    text: "不在本域重复实现 G 域已保留面的能力（保留面清单逐条对照）",
    evidenceFn: "auditDedupLedger",
  },
  {
    id: "OBL-KERNEL-SHARED",
    text: "共享核两域同核同对拍（对拍基线与误差上界入册）",
    evidenceFn: "auditSharedKernels",
  },
  {
    id: "OBL-NO-LAYER-MIX",
    text: "服务层不混层（编排与调度各自独立，混层即 violation）",
    evidenceFn: "checkLayerMixing",
  },
  {
    id: "OBL-VOCAB-COVERED",
    text: "官方描述词表逐词认领，无人认领的词不得默认归本域",
    evidenceFn: "auditVocabCoverage",
  },
];

/** 移交接收回执。 */
export interface HandoverReceipt {
  /** 接收方（本域恒为 VE-H）。 */
  readonly receiver: string;
  /** 四件移交包齐备标记。 */
  readonly artifactsPresent: boolean;
  /** 快照一致性标记。 */
  readonly snapshotMatched: boolean;
  /** 已核销义务 id 列表。 */
  readonly obligationsSettled: readonly string[];
  /** 声明的 G 域收官状态。 */
  readonly upstreamClosed: boolean;
}

//** 构造移交回执（供调用方注入实际核销结果；本函数不做任何默认值填充）。 */
export function receiveHandover(
  input: {
    readonly artifacts: readonly HandoverArtifact[];
    readonly snapshotId: string;
    readonly settled: readonly string[];
    readonly upstreamClosed: boolean;
  },
  bag: DiagBag
): Outcome<HandoverReceipt> {
  // 1) 四件齐备。
  const missing: HandoverArtifact[] = (
    ["interface-freeze-list", "audio-boundary-list", "lesson-ledger", "evidence-index"] as const
  ).filter((a) => !input.artifacts.includes(a));
  for (const m of missing) {
    bag.push(
      "HANDOVER_BUNDLE_INCOMPLETE",
      `移交包缺件：${HANDOVER_ARTIFACT_LABEL[m]}`,
      `F1397 四件齐备才构成移交；缺件即开工前置不成立（册内「清单化防口头交接」）`,
    );
  }

  // 2) 快照一致。
  const snapshotMatched = input.snapshotId === HANDOVER_SNAPSHOT.snapshotId;
  if (!snapshotMatched) {
    bag.push(
      "HANDOVER_SNAPSHOT_MISMATCH",
      `移交包快照「${input.snapshotId}」与G 域宣告值「${HANDOVER_SNAPSHOT.snapshotId}」不符`,
      `快照失真说明拿到的不是G 域当前基线；回F1397 取当前快照重接`,
    );
  }

  // 3) 上游收官。
  if (!input.upstreamClosed) {
    bag.push(
      "UPSTREAM_NOT_CLOSED",
      `上游 ${DOMAIN.upstreamDomain} 未收官（F${DOMAIN.upstreamCloseItem} 宣告未生效）`,
      `VE-H 的四主题处置清单由 G 域收官时携带；上游未收官时开工= 无边界依据`,
    );
  }

  // 4) 义务逐条核销。
  const settledSet = new Set(input.settled);
  for (const o of KICKOFF_OBLIGATIONS) {
    if (!settledSet.has(o.id)) {
      bag.push(
        "HANDOVER_OBLIGATION_UNSETTLED",
        `开工前置义务未核销：${o.id} —— ${o.text}`,
        `核销凭据是${o.evidenceFn}() 通过；口头承诺不算核销`,
      );
    }
  }

  const receipt: HandoverReceipt = {
    receiver: DOMAIN.id,
    artifactsPresent: missing.length === 0,
    snapshotMatched,
    obligationsSettled: KICKOFF_OBLIGATIONS.filter((o) => settledSet.has(o.id)).map((o) => o.id),
    upstreamClosed: input.upstreamClosed,
  };

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "HANDOVER_OBLIGATION_UNSETTLED",
      message: `移交接收未通过：${bag.size} 条诊断`,
      hint: `移交接收是 H 域开工的硬前置（册内 F1400 断点明示「开工前先读 F1381/F1397 边界契约」）`,
      diagnostics: bag.all(),
    };
  }
  return ok(receipt, bag.all());
}

/**
 * 查重对账台账：把「本域每一处可能与对端重叠的能力」逐条落账。
 * 每条账必须给出对端条目、结论（三态之一）、依据。三态封闭：
 *   - `dedup-shared`    已共享（同一份核），双方条目并存但实现唯一；
 *   - `dedup-h-only`    本域独有（对端无此能力）；
 *   - `dedup-g-only`    对端独有（本域不复刻）。
 * 第四种「双方各自实现」不存在——那正是 ADR 要消灭的东西。
 */
export type DedupVerdict = "dedup-shared" | "dedup-h-only" | "dedup-g-only";

/** 查重台账一条。 */
export interface DedupLedgerEntry {
  /** 能力名。 */
  readonly capability: string;
  /** 对端条目号（VE-G侧；若对端无此能力则填0并注明）。 */
  readonly peerItem: number;
  /** 本域条目号（F1401~F1600）。 */
  readonly localItem: number;
  /** 结论（三态封闭）。 */
  readonly verdict: DedupVerdict;
  /** 关联ADR（若该能力被某张 ADR 覆盖）。 */
  readonly adrId: string;
  /** 依据（人话，说明凭什么这么判）。 */
  readonly basis: string;
}

/** VE-H 查重对账台账（覆盖四主题 + 共享核 + 三条易撞车的相邻能力）。 */
export const DEDUP_LEDGER: readonly DedupLedgerEntry[] = [
  {
    capability: "双二阶滤波（biquad）",
    peerItem: 1329,
    localItem: 1403,
    verdict: "dedup-shared",
    adrId: "ADR-H-01",
    basis: "册内明文「DSP 核共享……两域同核同对拍」；滤波核无状态，双方共用一份实现。",
  },
  {
    capability: "播放链五层固定总线",
    peerItem: 1324,
    localItem: 1403,
    verdict: "dedup-g-only",
    adrId: "ADR-H-01",
    basis: "固定层序 + 播放器生命周期绑定属 G07 保留面；本域只提供通用图与模板实例化能力。",
  },
  {
    capability: "通用混音图（DAG / 四类节点 / 热更新）",
    peerItem: 0,
    localItem: 1403,
    verdict: "dedup-h-only",
    adrId: "ADR-H-01",
    basis: "任意拓扑 + 图校验 + 等增益热更新，G07 无此能力（其五层是本图退化实例）。",
  },
  {
    capability: "双耳卷积核（HRIR）",
    peerItem: 1326,
    localItem: 1430,
    verdict: "dedup-shared",
    adrId: "ADR-H-02",
    basis: "卷积核无状态可共享；渲染策略（坐标来源 / 遮挡判定）两侧不共享。",
  },
  {
    capability: "媒体双耳回放路径",
    peerItem: 1326,
    localItem: 1409,
    verdict: "dedup-g-only",
    adrId: "ADR-H-02",
    basis: "媒体源→双耳输出的固定路径属 G07 保留面；本域的世界空间引擎以它为退化配置。",
  },
  {
    capability: "世界空间音频（listener/emitter/遮挡/混响区）",
    peerItem: 0,
    localItem: 1409,
    verdict: "dedup-h-only",
    adrId: "ADR-H-02",
    basis: "3D 定位 + 遮挡 + 区域混响需要场景几何概念，G07 播放链视角内不存在。",
  },
  {
    capability: "重采样滤波核",
    peerItem: 1323,
    localItem: 1413,
    verdict: "dedup-shared",
    adrId: "ADR-H-03",
    basis: "核与档位定义共享（同名不同参即 violation）；实例生命周期两侧独立。",
  },
  {
    capability: "重采样五档质量定义",
    peerItem: 1323,
    localItem: 1413,
    verdict: "dedup-shared",
    adrId: "ADR-H-03",
    basis: "档位表必须单份：两份定义无法对拍，基线不可比即失去回归意义。",
  },
  {
    capability: "offline 批量重采样",
    peerItem: 0,
    localItem: 1413,
    verdict: "dedup-h-only",
    adrId: "ADR-H-03",
    basis: "批量转采样与实时流式同核但调度不同，播放链无批量需求。",
  },
  {
    capability: "K-weighting 滤波链 / LUFS 积分 / True Peak 检测",
    peerItem: 1325,
    localItem: 1414,
    verdict: "dedup-shared",
    adrId: "ADR-H-04",
    basis: "算法核共享，对拍基线为 EBU Tech 3342 测试向量，逐样本一致。",
  },
  {
    capability: "播放时响度对齐（单文件即时测量）",
    peerItem: 1325,
    localItem: 1414,
    verdict: "dedup-g-only",
    adrId: "ADR-H-04",
    basis: "播放期增益调整与降级语义属 G07 保留面；本域只提供分析结果。",
  },
  {
    capability: "媒体库批量响度分析（指纹缓存 / 增量扫描 / 报告）",
    peerItem: 0,
    localItem: 1414,
    verdict: "dedup-h-only",
    adrId: "ADR-H-04",
    basis: "库级调度与缓存是批量能力独占价值，播放链一次只播一条。",
  },
  {
    capability: "FFT 变换核",
    peerItem: 1326,
    localItem: 1430,
    verdict: "dedup-shared",
    adrId: "ADR-H-02",
    basis: "分帧策略两侧不同但变换核同构；统计等价对拍（频点幅度/相位误差界）。",
  },
  {
    capability: "FFT 分帧与重叠相加策略",
    peerItem: 1326,
    localItem: 1430,
    verdict: "dedup-g-only",
    adrId: "ADR-H-02",
    basis: "分帧大小由各自实时预算决定（G 侧固定，H 侧随 HRIR 长度自适应），无共享价值。",
  },
  {
    capability: "音频解码与容器解析",
    peerItem: 1321,
    localItem: 0,
    verdict: "dedup-g-only",
    adrId: "",
    basis:
      "属 G07（F1321~F1340 音频引擎组）的五层架构总成与解码路由；" +
      "本域零条目——THEME_REGISTRY 各主题边界句已写明禁扩面，此条登记的是「不认领」事实本身。",
  },
  {
    capability: "播放器会话生命周期",
    peerItem: 1324,
    localItem: 1402,
    verdict: "dedup-g-only",
    adrId: "ADR-H-01",
    basis: "F1402 服务化架构的会话模型是引擎会话，与播放器会话并行存在、互不替代。",
  },
];

/** 查重台账机检（四查）：三态封闭 / 条目在域 / ADR 引用有效 / 四主题齐备覆盖。 */
export function auditDedupLedger(bag: DiagBag): Outcome<readonly DedupLedgerEntry[]> {
  const verdicts: readonly DedupVerdict[] = ["dedup-shared", "dedup-h-only", "dedup-g-only"];

  for (const e of DEDUP_LEDGER) {
    // 1) 三态封闭。
    if (!verdicts.includes(e.verdict)) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `台账「${e.capability}」的结论「${String(e.verdict)}」不在三态封闭集合内`,
        `合法结论为 ${verdicts.join(" / ")}；「双方各自实现」不存在——那正是 ADR 要消灭的对象`,
      );
    }
    // 2) 条目在本域区间内（peerItem=0 表示对端无此能力，须配h-only/g-only 结论）。
    if (e.localItem !== 0 && !inDomainRange(e.localItem)) {
      bag.push(
        "CAPABILITY_ITEM_OUT_OF_DOMAIN",
        `台账「${e.capability}」的本域条目 F${e.localItem} 越出 VE-H 区间`,
        `本域条目必须是 F1401~F1600；填 0 表示本域无此能力`,
      );
    }
    if (e.peerItem !== 0 && (e.peerItem < 1201 || e.peerItem > 1400)) {
      bag.push(
        "PEER_ITEM_OUT_OF_RANGE",
        `台账「${e.capability}」的对端条目 F${e.peerItem} 不在 VE-G 区间 [1201, 1400] 内`,
        `回册内 F1381 核对条目号`,
      );
    }
    // 2b) 结论与条目零值必须自洽：两端都是 0 意味着该能力两侧都不实现，
    //     那它就不该出现在本域台账里（登记它等于凭空多一个能力）。
    if (e.localItem === 0 && e.peerItem === 0) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `台账「${e.capability}」两侧条目均为 0（两侧都不实现），却仍入册`,
        `两侧都不实现的能力不是本域台账条目；移除它，或补齐归属域的条目号`,
      );
    }
    // 3) ADR 引用有效性。
    if (e.adrId !== "" && !BOUNDARY_ADR_TABLE.some((a) => a.id === e.adrId)) {
      bag.push(
        "ADR_INCOMPLETE",
        `台账「${e.capability}」引用的 ADR ${e.adrId} 不存在`,
        `引用必须指向 BOUNDARY_ADR_TABLE 中已落册的 ADR`,
      );
    }
    if (!e.basis || !e.basis.trim()) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `台账「${e.capability}」缺少判定依据`,
        `每条台账都要能回答「凭什么这么判」，否则查重只是走过场`,
      );
    }
  }

  // 4) 四主题齐备覆盖：每个四主题至少有一条 shared 结论（证明该主题确实共享了核）。
  for (const t of OVERLAP_TOPICS) {
    const adr = BOUNDARY_ADR_TABLE.find((a) => a.topic === t);
    if (!adr) continue;
    const hasShared = DEDUP_LEDGER.some((e) => e.adrId === adr.id && e.verdict === "dedup-shared");
    if (!hasShared) {
      bag.push(
        "DOMAIN_DUPLICATE_FOUND",
        `四主题「${OVERLAP_TOPIC_LABEL[t]}」在台账中没有任何 shared 结论`,
        `${adr.id} 认定为「不重复造轮子」，台账必须有对应的共享条目把这句话落到具体能力上`,
      );
    }
  }

  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "DOMAIN_DUPLICATE_FOUND",
      message: `查重对账台账机检未过：${bag.size} 条诊断`,
      hint: `台账是「不重复」承诺的凭据；机检不过意味着 H01 二十项开工时无查重依据`,
      diagnostics: bag.all(),
    };
  }
  return ok(DEDUP_LEDGER.slice(), bag.all());
}

/**
 * 官方描述词表覆盖核对（F1381 规格内的第三查）。
 * 册内点名的两条词表覆盖事实：画中画 = G04 已含；视频墙基础 = G05 多实例，
 * 完整合层归 VE-W。VE-H 侧须逐词认领：与音频相关的词由本域认领，其余显式
 * 指向他域——**不留空白**（空白会被后续域默认认领，是撞车的起点）。
 */
export interface VocabClaim {
  /** 词条。 */
  readonly term: string;
  /** 认领域（VE-H / VE-G / 其他域）。 */
  readonly owner: string;
  /** 认领条目号（owner 非 VE-H 时填对端条目号）。 */
  readonly ownerItem: number;
  /** 认领说明。 */
  readonly note: string;
}

/** 词表认领表（VE-H 关心的全部词逐条认领）。 */
export const VOCAB_CLAIMS: readonly VocabClaim[] = [
  { term: "混音", owner: "VE-H", ownerItem: 1403, note: "通用混音图本体；G07 的五层为退化实例。" },
  { term: "播放总线", owner: "VE-G", ownerItem: 1324, note: "固定五层总线属播放链编排面。" },
  { term: "HRTF", owner: "VE-H", ownerItem: 1429, note: "HRTF 数据集管理与双耳渲染本体。" },
  { term: "双耳播放", owner: "VE-G", ownerItem: 1326, note: "媒体内容的双耳回放路径属播放链。" },
  { term: "重采样", owner: "VE-H", ownerItem: 1413, note: "引擎级统一重采样服务（核共享）。" },
  { term: "响度归一", owner: "VE-H", ownerItem: 1414, note: "批量响度分析服务（核共享）。" },
  { term: "响度对齐", owner: "VE-G", ownerItem: 1325, note: "播放时单文件对齐属播放链。" },
  { term: "空间音频", owner: "VE-H", ownerItem: 1409, note: "世界空间音频本体（listener/emitter）。" },
  { term: "音频令牌", owner: "VE-H", ownerItem: 1406, note: "声音设计令牌与三级覆盖。" },
  { term: "淡入淡出", owner: "VE-H", ownerItem: 1407, note: "全域 fade 服务；G07 的交叉淡化是其特例。" },
  { term: "事件总线", owner: "VE-H", ownerItem: 1408, note: "「该不该播」决策流；可视化总线归G07。" },
  { term: "可视化总线", owner: "VE-G", ownerItem: 1332, note: "「正在播的」只读数据流镜像。" },
  { term: "低延迟路径", owner: "VE-H", ownerItem: 1412, note: "≤20ms 会话；驱动直连归 H04。" },
  { term: "音效包", owner: "VE-H", ownerItem: 1417, note: "包清洗与签名三级；F1337 标准同源。" },
  { term: "音频焦点", owner: "VE-H", ownerItem: 1402, note: "多消费者并存时的仲裁（G07 单消费者无此概念）。" },
  { term: "画中画", owner: "VE-G", ownerItem: 0, note: "册内明示 G04 已含；与音频无关，本域不认领。" },
  { term: "视频墙基础", owner: "VE-G", ownerItem: 0, note: "册内明示 G05 多实例；完整合层归 VE-W。" },
];

/** 词表覆盖机检：每词必须有认领域，VE-H 认领词必须有域内条目号。 */
export function auditVocabCoverage(bag: DiagBag): Outcome<readonly VocabClaim[]> {
  for (const c of VOCAB_CLAIMS) {
    if (!c.owner || !c.owner.trim()) {
      bag.push(
        "VOCAB_COVERAGE_GAP",
        `词条「${c.term}」无人认领`,
        `空白词会被后续域默认认领（撞车起点）；显式指向他域即可，不留空白`,
      );
      continue;
    }
    if (c.owner === DOMAIN.id && !inDomainRange(c.ownerItem)) {
      bag.push(
        "VOCAB_COVERAGE_GAP",
        `词条「${c.term}」由 VE-H 认领，但条目号 F${c.ownerItem} 越出本域区间`,
        `本域认领的词必须给出 F1401~F1600 的条目号`,
      );
    }
    if (c.owner !== DOMAIN.id && c.ownerItem !== 0 && c.owner !== DOMAIN.id) {
      // 对端条目仅在落在 G 域区间时才校验，其他域条目号不可预知，放行但要求说明。
      if (c.owner === "VE-G" && (c.ownerItem < 1201 || c.ownerItem > 1400)) {
        bag.push(
          "PEER_ITEM_OUT_OF_RANGE",
          `词条「${c.term}」认领给 VE-G，但条目号 F${c.ownerItem} 不在 G 域区间内`,
          `回册内核对；确认 G04 已含 / G05 多实例这类描述无需精确条目号时可填 0 并在note 说明`,
        );
      }
    }
    if (!c.note || !c.note.trim()) {
      bag.push(
        "VOCAB_COVERAGE_GAP",
        `词条「${c.term}」缺少认领说明`,
        `认领要回答「凭什么归它」，一句说明即可`,
      );
    }
  }
  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "VOCAB_COVERAGE_GAP",
      message: `词表覆盖机检未过：${bag.size} 条诊断`,
      hint: `词表覆盖是 F1381 规格内的第三查；缺口会在F1420 收口对账时被翻出来`,
      diagnostics: bag.all(),
    };
  }
  return ok(VOCAB_CLAIMS.slice(), bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §6 组内 20 条分工总览（H01 批次 F1401~F1420 · 判据四）
// ════════════════════════════════════════════════════════════════════════════

/** H01 批次的一条分工。 */
export interface WorkItem {
  /** 条目号（F1401~F1420）。 */
  readonly item: number;
  /** 中文标题（册内条目标题原文）。 */
  readonly title: string;
  /** 职责一句话（做什么）。 */
  readonly duty: string;
  /** 产出契约名。 */
  readonly contract: string;
  /** 判据锚点（册内条目锚点 id）。 */
  readonly anchor: string;
  /** 前置依赖（可为空数组 = 无前置）。 */
  readonly deps: readonly number[];
  /** 目标行数（册内明示）。 */
  readonly targetLines: number;
}

/** H01 批次 F1401~F1420 全20 条分工（逐条落位，与册内锚点一一对应）。 */
export const H01_WORK_ITEMS: readonly WorkItem[] = [
  {
    item: 1401,
    title: "H 域开工与边界契约执行",
    duty: "四主题 ADR 落册、复用声明与共享核抽定、查重对账执行",
    contract: "HDomainKickoffContract",
    anchor: "VE-F1401",
    deps: [],
    targetLines: 400,
  },
  {
    item: 1402,
    title: "音频引擎服务化架构",
    duty: "引擎独立服务与多消费者会话模型（媒体/通信/系统/游戏四类别 + 故障域）",
    contract: "AudioEngineServiceContract",
    anchor: "VE-F1402",
    deps: [1401],
    targetLines: 440,
  },
  {
    item: 1403,
    title: "混音图引擎节点模型",
    duty: "源/处理/总线/监听四类节点、端口协商、建图三查、等增益热更新",
    contract: "MixGraphContract",
    anchor: "VE-F1403",
    deps: [1401, 1402],
    targetLines: 480,
  },
  {
    item: 1404,
    title: "发送插入与侧链",
    duty: "Send 共享效果实例、Insert 串行效果链、侧链调制与尾音保护",
    contract: "SendInsertSidechainContract",
    anchor: "VE-F1404",
    deps: [1403],
    targetLines: 420,
  },
  {
    item: 1405,
    title: "子混音与嵌套图",
    duty: "submix 封装、嵌套八层上限、参数提升、CPU 归属计量、模板库",
    contract: "SubmixNestingContract",
    anchor: "VE-F1405",
    deps: [1403],
    targetLines: 380,
  },
  {
    item: 1406,
    title: "音频令牌系统",
    duty: "语义名→资源+参数映射、三级覆盖、主题联动、缺省链、唯一来源纪律",
    contract: "AudioTokenContract",
    anchor: "VE-F1406",
    deps: [1402],
    targetLines: 400,
  },
  {
    item: 1407,
    title: "淡入淡出引擎",
    duty: "曲线库、复合包络、交叉淡化编排、四场景统一、防重叠状态机",
    contract: "FadeServiceContract",
    anchor: "VE-F1407",
    deps: [1403],
    targetLines: 380,
  },
  {
    item: 1408,
    title: "音频事件总线",
    duty: "发布订阅、事件四要素、节流合并、与可视化总线的边界声明、优先级仲裁",
    contract: "AudioEventBusContract",
    anchor: "VE-F1408",
    deps: [1406],
    targetLines: 380,
  },
  {
    item: 1409,
    title: "空间音频引擎世界空间",
    duty:
      "listener/emitter 模型、球坐标与笛卡尔双输入、三种距离模型、64 声源、坐标契约" +
      "（跨组依赖：HRIR 数据集由 H02 的 F1429 提供，本条目只留消费侧接口）",
    contract: "SpatialAudioContract",
    anchor: "VE-F1409",
    deps: [1403],
    targetLines: 460,
  },
  {
    item: 1410,
    title: "遮挡与穿透",
    duty: "遮挡/半遮挡两档、几何查询接口预留（一期球体代理诚实标注）、材质联动、平滑过渡",
    contract: "OcclusionContract",
    anchor: "VE-F1410",
    deps: [1409],
    targetLines: 360,
  },
  {
    item: 1411,
    title: "混响区与环境声学",
    duty: "分区预设、卷积混响与算法混响双档、IR 资源管理、区域过渡插值、播放链边界重申",
    contract: "ReverbZoneContract",
    anchor: "VE-F1411",
    deps: [1403, 1409],
    targetLines: 400,
  },
  {
    item: 1412,
    title: "低延迟路径",
    duty: "≤20ms 端到端、小缓冲权衡表、underrun 特化恢复、实测上报、安全切换",
    contract: "LowLatencyContract",
    anchor: "VE-F1412",
    deps: [1402],
    targetLines: 400,
  },
  {
    item: 1413,
    title: "引擎级重采样服务",
    duty: "offline/realtime 双模式同核、五档档位继承、并发会话隔离、CPU 账户计量",
    contract: "ResampleServiceContract",
    anchor: "VE-F1413",
    deps: [1401, 1402],
    targetLines: 380,
  },
  {
    item: 1414,
    title: "引擎级响度归一服务",
    duty: "批量 LUFS/True Peak 扫描、预设表、指纹缓存（含算法版本）、增量扫描、报告导出",
    contract: "LoudnessServiceContract",
    anchor: "VE-F1414",
    deps: [1401, 1402],
    targetLines: 380,
  },
  {
    item: 1415,
    title: "引擎时钟与调度服务",
    duty: "播放主钟泛化为引擎时钟、每会话偏移管理、音频精度定时、帧钟联动接口预留",
    contract: "EngineClockContract",
    anchor: "VE-F1415",
    deps: [1402],
    targetLines: 360,
  },
  {
    item: 1416,
    title: "多消费者性能预算",
    duty: "四类别预算分配、过载按类别降载且显性上报、每消费者计量、与 CGPU 预算理念对齐",
    contract: "ConsumerBudgetContract",
    anchor: "VE-F1416",
    deps: [1402],
    targetLines: 360,
  },
  {
    item: 1417,
    title: "音效包安全与隔离",
    duty: "包清洗四上限、三级签名与未知档降权、解码 fuzz 硬化、路径白名单",
    contract: "SoundPackSecurityContract",
    anchor: "VE-F1417",
    deps: [1402],
    targetLines: 340,
  },
  {
    item: 1418,
    title: "引擎遥测与体验日志",
    duty: "五指标采集（voice 峰值/图规模/低延迟命中率/underrun 率/降载次数）、令牌缺失显性、隐私",
    contract: "EngineTelemetryContract",
    anchor: "VE-F1418",
    deps: [1416],
    targetLines: 320,
  },
  {
    item: 1419,
    title: "引擎 API 冻结 v1",
    duty: "五族签名冻结（引擎/图/空间/资产/时钟）、与 G07 API 互补声明、版本化",
    contract: "EngineApiFreezeContract",
    anchor: "VE-F1419",
    deps: [1403, 1406, 1409, 1415],
    targetLines: 320,
  },
  {
    item: 1420,
    title: "H01 组收口与 H02 移交",
    duty: "20 项入树核对、边界 ADR 归档、多消费者并发联测、组双签、向 H02 移交",
    contract: "H01ClosureContract",
    anchor: "VE-F1420",
    deps: [1419],
    targetLines: 320,
  },
];

/** H01 分工条目数（册内：每组 20 项）。 */
export const H01_WORK_ITEM_COUNT = 20;

/** 组内分工机检（五查）：条数齐 / 区间在域 / 无重复 / 契约名非空 / 依赖闭合。 */
export function auditWorkBreakdown(bag: DiagBag): Outcome<number> {
  // 1) 条数齐。
  if (H01_WORK_ITEMS.length !== H01_WORK_ITEM_COUNT) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `H01 分工 ${H01_WORK_ITEMS.length} 条，应为 ${H01_WORK_ITEM_COUNT} 条`,
      `册内「域内分 10 组（每组 20 项）」是硬约束；缺条会让F1420 收口对账出现幽灵项`,
    );
  }
  // 2) 区间 + 连续。
  for (let i = 0; i < H01_WORK_ITEMS.length; i += 1) {
    const w = H01_WORK_ITEMS[i];
    if (!w) continue;
    if (!inDomainRange(w.item)) {
      bag.push(
        "CAPABILITY_ITEM_OUT_OF_DOMAIN",
        `H01 条目 F${w.item} 越出VE-H 区间 [${DOMAIN.itemLo}, ${DOMAIN.itemHi}]`,
        `H01 应承载 F1401~F1420`,
      );
    }
    // 连续性：相邻条目号必须逐一相接。
    if (i > 0) {
      const prev = H01_WORK_ITEMS[i - 1];
      if (prev && w.item !== prev.item + 1) {
        bag.push(
          "WORKBREAKDOWN_INCOMPLETE",
          `H01 条目号不连续：F${prev.item} 之后是 F${w.item}`,
          `条目号须连续；册内「每项在全书中出现三次」，号段跳空即文档缺陷`,
        );
      }
    }
    // 3) 契约名与标题非空。
    if (!w.contract.trim() || !w.title.trim() || !w.duty.trim()) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `H01 条目 F${w.item} 的标题 / 职责 / 契约名存在空串`,
        `下游按契约名引用；空契约名会让 F1419 冻结时无对象可冻`,
      );
    }
    // 目标行数必须为正（册内铁律三：行数明示）。
    if (!Number.isInteger(w.targetLines) || w.targetLines <= 0) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `H01 条目 F${w.item} 的目标行数非法：${String(w.targetLines)}`,
        `册内行数铁律要求每项明示目标行数`,
      );
    }
  }
  // 4) 重复条目号。
  const seen = new Set<number>();
  for (const w of H01_WORK_ITEMS) {
    if (seen.has(w.item)) {
      bag.push("WORKBREAKDOWN_INCOMPLETE", `H01 条目号 F${w.item} 重复`, `一条目号只出现一次`);
    }
    seen.add(w.item);
  }
  // 5) 依赖闭合：前置必须落在本组内（跨组依赖写进注释而非本表）。
  for (const w of H01_WORK_ITEMS) {
    for (const d of w.deps) {
      if (!seen.has(d)) {
        bag.push(
          "WORKBREAKDOWN_INCOMPLETE",
          `H01 条目 F${w.item} 的前置 F${d} 不在本组内`,
          `分组表只登记组内依赖；跨组依赖在条目正文里说明`,
        );
      }
      if (d >= w.item) {
        bag.push(
          "WORKBREAKDOWN_INCOMPLETE",
          `H01 条目 F${w.item} 的前置 F${d} 不早于自身（编号不构成依赖序）`,
          `依赖方向错了会让开工顺序倒置`,
        );
      }
    }
  }
  if (bag.hasAny) {
    return {
      ok: false,
      code: bag.all()[0]?.code ?? "WORKBREAKDOWN_INCOMPLETE",
      message: `H01 分工机检未过：${bag.size} 条诊断`,
      hint: `分工不齐即开工未完成（册内判据：20 条齐备由自检断言）`,
      diagnostics: bag.all(),
    };
  }
  return ok(H01_WORK_ITEMS.length, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §7 开工门禁 + 开工摘要（判据一至四的落地编排）
// ════════════════════════════════════════════════════════════════════════════

/** 开工门禁一条。 */
export interface GateCriterion {
  /** 门禁 id。 */
  readonly id: string;
  /** 中文名。 */
  readonly name: string;
  /** 对应判据。 */
  readonly criterion: string;
  /** 证明函数名（本模块提供的机检）。 */
  readonly proofFn: string;
}

/** 开工门禁表（册内判据：ADR 落册 / 四主题差异化 / 复用声明 / 查重对账 / 判据）。 */
export const KICKOFF_GATES: readonly GateCriterion[] = [
  { id: "G-ADR", name: "边界 ADR 落册", criterion: "ADR 落册", proofFn: "auditBoundaryAdr" },
  { id: "G-DIFF", name: "四主题差异化定位（保留面逐能力入账）", criterion: "四主题差异化", proofFn: "projectTopicRetention" },
  { id: "G-KERNEL", name: "共享核两域同核同对拍", criterion: "复用声明", proofFn: "auditSharedKernels" },
  { id: "G-LAYER", name: "服务层不混层", criterion: "复用声明", proofFn: "checkLayerMixing" },
  { id: "G-DEDUP", name: "查重对账零重复", criterion: "查重对账", proofFn: "auditDedupLedger" },
  { id: "G-VOCAB", name: "词表覆盖无空白", criterion: "查重对账", proofFn: "auditVocabCoverage" },
  { id: "G-THEME", name: "九主题承载齐备", criterion: "域开工", proofFn: "auditThemeCoverage" },
  { id: "G-RANGE", name: "十批次区间无缝", criterion: "域开工", proofFn: "auditBatchRanges" },
  { id: "G-WORK", name: "H01 分工 20 条齐", criterion: "判据", proofFn: "auditWorkBreakdown" },
  { id: "G-HANDOVER", name: "上游移交接收通过", criterion: "判据", proofFn: "receiveHandover" },
];

/** 开工摘要（本条交付的最终聚合结果）。 */
export interface DomainKickoffSummary {
  /** 域身份。 */
  readonly domain: string;
  /** 官方主题数（应然 9）。 */
  readonly themeCount: number;
  /** 批次数（应然 10）。 */
  readonly batchCount: number;
  /** 四主题处置结论。 */
  readonly dispositions: readonly { readonly topic: string; readonly label: string; readonly disposition: string }[];
  /** 共享核 id。 */
  readonly sharedKernels: readonly SharedKernelId[];
  /** 查重台账条数。 */
  readonly dedupEntries: number;
  /** H01 分工条数。 */
  readonly workItems: number;
  /** 已核销义务数。 */
  readonly obligationsSettled: number;
  /** 开工是否放行。 */
  readonly cleared: boolean;
}

/** 全量机检的自动执行顺序（固定顺序，便于对拍复现）。 */
export const AUTO_AUDIT_ORDER: readonly string[] = [
  "auditThemeCoverage",
  "auditBatchRanges",
  "auditBoundaryAdr",
  "auditSharedKernels",
  "auditDedupLedger",
  "auditVocabCoverage",
  "auditWorkBreakdown",
];

/**
 * 开工编排：一次跑完全部门禁并聚合摘要。
 * 任何一门禁未过都产出 KICKOFF_BLOCKED 汇总诊断（逐条列出门禁名与诊断）。
 */
export function buildKickoffSummary(
  receipt?: HandoverReceipt,
  bag: DiagBag = new DiagBag()
): Outcome<DomainKickoffSummary> {
  // 1) 逐个门禁跑；失败的诊断平铺进同一个 bag。
  const themeOutcome = auditThemeCoverage(bag);
  bag.pushAll(themeOutcome.diagnostics);
  const rangeOutcome = auditBatchRanges(bag);
  bag.pushAll(rangeOutcome.diagnostics);
  const adrOutcome = auditBoundaryAdr(bag);
  bag.pushAll(adrOutcome.diagnostics);
  const kernelOutcome = auditSharedKernels(bag);
  bag.pushAll(kernelOutcome.diagnostics);
  const dedupOutcome = auditDedupLedger(bag);
  bag.pushAll(dedupOutcome.diagnostics);
  const vocabOutcome = auditVocabCoverage(bag);
  bag.pushAll(vocabOutcome.diagnostics);
  const workOutcome = auditWorkBreakdown(bag);
  bag.pushAll(workOutcome.diagnostics);

  // 2) 四主题处置投影（依赖 ADR，故在 ADR 之后）。
  const disposition = projectDispositionByDomain();
  if (!disposition.ok) {
    bag.pushAll(disposition.diagnostics);
  }

  // 3) 移交接收：未给回执时视为未核销（不默认通过——这是最容易出事的一处）。
  let settledCount = 0;
  if (receipt && receipt.artifactsPresent && receipt.snapshotMatched && receipt.upstreamClosed) {
    settledCount = receipt.obligationsSettled.length;
    if (settledCount < KICKOFF_OBLIGATIONS.length) {
      bag.push(
        "HANDOVER_OBLIGATION_UNSETTLED",
        `移交回执仅核销 ${settledCount}/${KICKOFF_OBLIGATIONS.length} 条义务`,
        `未核销的义务逐条列在 KICKOFF_OBLIGATIONS，凭据是对应机检函数通过`,
      );
    }
  } else {
    bag.push(
      "HANDOVER_OBLIGATION_UNSETTLED",
      "未提供有效移交回执（缺件 / 快照失真 / 上游未收官）",
      `H 域开工的边界契约由 F1381/F1397 携带；无回执即无边界依据，不默认通过`,
    );
  }

  // 4) 汇总。
  const dispositions = BOUNDARY_ADR_TABLE.map((a) => ({
    topic: a.topic,
    label: a.topicLabel,
    disposition: DISPOSITION_LABEL[a.disposition],
  }));

  const summary: DomainKickoffSummary = {
    domain: `${DOMAIN.id} ${DOMAIN.name}`,
    themeCount: themeOutcome.ok === true ? themeOutcome.value.length : 0,
    batchCount: BATCH_REGISTRY.length,
    dispositions,
    sharedKernels: kernelOutcome.ok === true ? kernelOutcome.value.slice() : [],
    dedupEntries: dedupOutcome.ok === true ? dedupOutcome.value.length : 0,
    workItems: workOutcome.ok ? workOutcome.value : 0,
    obligationsSettled: settledCount,
    cleared: false,
  };

  if (bag.hasAny) {
    bag.push(
      "KICKOFF_BLOCKED",
      `VE-H 开工被阻断：${bag.size - 1} 条前置诊断未清`,
      `按 bag.byCode() 分组修；放行条件是 ${KICKOFF_GATES.length} 道门禁全绿` +
        `（${AUTO_AUDIT_ORDER.join(" → ")}）`,
    );
    return {
      ok: false,
      code: "KICKOFF_BLOCKED",
      message: `VE-H 开工未放行：${summary.dispositions.length} 张 ADR 已登记但门禁未全绿`,
      hint: `门禁清单见 KICKOFF_GATES；修完重跑 buildKickoffSummary()`,
      diagnostics: bag.all(),
    };
  }

  return ok({ ...summary, cleared: true }, bag.all());
}

/**
 * 开工执行：接收移交回执 → 跑全部门禁 → 产出放行结论。
 * 这是 H01 二十项的第一道关口：未放行时，下游条目（F1402 起）不应开工。
 */
export function kickoffDomain(receipt: HandoverReceipt): Outcome<DomainKickoffSummary> {
  return buildKickoffSummary(receipt, new DiagBag());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 自检（判据的可执行形态：不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 一条自检断言的结果。 */
export interface SelfCheck {
  /** 断言名。 */
  readonly name: string;
  /** 期望值（人话）。 */
  readonly expect: string;
  /** 实测值（人话）。 */
  readonly actual: string;
  /** 是否通过。 */
  readonly passed: boolean;
}

/** 构造一份「已核销」的移交回执（自检与联调用；生产路径应注入真实回执）。 */
export function sampleReceipt(): HandoverReceipt {
  return {
    receiver: DOMAIN.id,
    artifactsPresent: true,
    snapshotMatched: true,
    obligationsSettled: KICKOFF_OBLIGATIONS.map((o) => o.id),
    upstreamClosed: true,
  };
}

/** 自检一：域开工（九主题 / 十批次 / 区间无缝）。 */
export function selfCheckDomainKickoff(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();

  const themes = auditThemeCoverage(bag);
  out.push({
    name: "九主题全部被批次承载",
    expect: `${THEME_IDS.length} 个主题零缺口`,
    actual: outcomeText(themes, (v) => `${v.length} 个主题有承载`, () => `未过：${bag.size} 条诊断`),
    passed: themes.ok && themes.value.length === THEME_IDS.length,
  });

  const rangeBag = new DiagBag();
  const ranges = auditBatchRanges(rangeBag);
  out.push({
    name: "十批次区间无缝且覆盖 200 项",
    expect: `${BATCH_REGISTRY.length} 批 / ${DOMAIN.itemCount} 项`,
    actual: outcomeText(ranges, (v) => `${v} 项`, () => `未过：${rangeBag.size} 条诊断`),
    passed: ranges.ok && ranges.value === DOMAIN.itemCount,
  });

  out.push({
    name: "域身份自洽（区间跨度 = 声明项数）",
    expect: `${DOMAIN.itemHi} - ${DOMAIN.itemLo} + 1 = ${DOMAIN.itemCount}`,
    actual: `${DOMAIN.itemHi} - ${DOMAIN.itemLo} + 1 = ${DOMAIN.itemHi - DOMAIN.itemLo + 1}`,
    passed: DOMAIN.itemHi - DOMAIN.itemLo + 1 === DOMAIN.itemCount,
  });

  const overlap = BATCH_REGISTRY.some((b, i) =>
    BATCH_REGISTRY.slice(i + 1).some((c) => b.itemLo <= c.itemHi && c.itemLo <= b.itemHi)
  );
  out.push({
    name: "批次区间零重叠",
    expect: "无重叠",
    actual: overlap ? "存在重叠" : "无重叠",
    passed: !overlap,
  });
  return out;
}

/** 自检二：边界 ADR（四主题齐备 / 处置合法 / 三段完整 / 回归条件齐）。 */
export function selfCheckBoundaryAdr(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();
  const adr = auditBoundaryAdr(bag);
  out.push({
    name: "四主题 ADR 齐备",
    expect: `${OVERLAP_TOPICS.length} 张`,
    actual: outcomeText(adr, (v) => `${v.length} 张`, () => `未过：${bag.size} 条诊断`),
    passed: adr.ok && adr.value.length === OVERLAP_TOPICS.length,
  });

  out.push({
    name: "一主题一 ADR（零重复）",
    expect: "每主题恰好 1 张",
    actual: BOUNDARY_ADR_TABLE.length === OVERLAP_TOPICS.length ? "恰好 1 张" : "数量不符",
    passed: BOUNDARY_ADR_TABLE.length === OVERLAP_TOPICS.length,
  });

  const allLegal = BOUNDARY_ADR_TABLE.every((a) => isDisposition(a.disposition));
  out.push({
    name: "处置结论在封闭词汇表内",
    expect: DISPOSITION_VOCABULARY.join(" / "),
    actual: BOUNDARY_ADR_TABLE.map((a) => a.disposition).join(" / "),
    passed: allLegal,
  });

  const allRegression = BOUNDARY_ADR_TABLE.every(
    (a) => a.regressionCondition.trim().length > 0
  );
  out.push({
    name: "四张 ADR 均声明回归条件",
    expect: "4/4 有回归条件",
    actual: `${BOUNDARY_ADR_TABLE.filter((a) => a.regressionCondition.trim()).length}/4`,
    passed: allRegression,
  });

  const allSections = BOUNDARY_ADR_TABLE.every(
    (a) =>
      a.perspectiveG.trim() && a.perspectiveH.trim() && a.retainG.trim() && a.leadH.trim() && a.sharedKernel.trim() && a.serviceLayerSplit.trim()
  );
  out.push({
    name: "四张 ADR 三段齐备（视角/保留面/共享核 + 服务层不共享）",
    expect: "4/4 六段非空",
    actual: `${BOUNDARY_ADR_TABLE.filter((a) => a.perspectiveG.trim() && a.retainG.trim() && a.sharedKernel.trim()).length}/4`,
    passed: allSections,
  });

  const peersInRange = BOUNDARY_ADR_TABLE.every((a) =>
    a.peerItems.every((it) => it >= 1201 && it <= 1400)
  );
  out.push({
    name: "对端条目均在 VE-G 区间内",
    expect: "全部落在 F1201-F1400",
    actual: peersInRange ? "全部在范围内" : "存在越界条目",
    passed: peersInRange,
  });
  return out;
}

/** 自检三：复用声明（共享核三件/ 对拍基线 / 无状态 / 服务层不混层）。 */
export function selfCheckReuseDeclaration(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();
  const kernels = auditSharedKernels(bag);
  out.push({
    name: "共享核三件齐备（biquad/fft/convolution）",
    expect: `${SHARED_KERNEL_IDS.length} 个`,
    actual: outcomeText(kernels, (v) => `${v.length} 个`, () => `未过：${bag.size} 条诊断`),
    passed: kernels.ok && kernels.value.length === SHARED_KERNEL_IDS.length,
  });

  const baselines = SHARED_KERNEL_IDS.every((k) => DSP_KERNEL_TABLE[k].parityBaseline.trim().length > 0);
  out.push({
    name: "每个共享核均有对拍基线",
    expect: "3/3 有基线",
    actual: `${SHARED_KERNEL_IDS.filter((k) => DSP_KERNEL_TABLE[k].parityBaseline.trim()).length}/3`,
    passed: baselines,
  });

  const tolerances = SHARED_KERNEL_IDS.every((k) => DSP_KERNEL_TABLE[k].tolerance.trim().length > 0);
  out.push({
    name: "每个共享核均有误差上界",
    expect: "3/3 有上界",
    actual: `${SHARED_KERNEL_IDS.filter((k) => DSP_KERNEL_TABLE[k].tolerance.trim()).length}/3`,
    passed: tolerances,
  });

  const stateless = SHARED_KERNEL_IDS.every((k) => DSP_KERNEL_TABLE[k].stateless === true);
  out.push({
    name: "共享核全部声明无状态",
    expect: "3/3 stateless",
    actual: `${SHARED_KERNEL_IDS.filter((k) => DSP_KERNEL_TABLE[k].stateless === true).length}/3`,
    passed: stateless,
  });

  // 混层检测：合法操作放行，触碰共享核的服务层操作必须被拒。
  const goodBag = new DiagBag();
  const good = checkLayerMixing({ layerId: "graph-lifecycle", actor: "VE-H", touchesSharedKernel: false }, goodBag);
  const badBag = new DiagBag();
  const bad = checkLayerMixing({ layerId: "instance-ownership", actor: "VE-G", touchesSharedKernel: true }, badBag);
  out.push({
    name: "混层检测：合法操作放行",
    expect: "ok=true",
    actual: good.ok ? "ok=true" : "ok=false",
    passed: good.ok,
  });
  out.push({
    name: "混层检测：服务层触碰共享核被拒",
    expect: "ok=false 且产出 LAYER_MIXING_VIOLATION",
    actual: outcomeText(bad, () => "ok=true（未拦住）", (c) => `ok=false / ${c}`),
    passed: outcomeCode(bad) === "LAYER_MIXING_VIOLATION",
  });

  const decl = renderReuseDeclaration();
  out.push({
    name: "复用声明可渲染（确定性文本）",
    expect: "含共享核三件 + 四主题处置 + 两个事实源",
    actual: outcomeText(decl, (v) => `${v.split(String.fromCharCode(10)).length} 行`, (c) => `未过：${c}`),
    passed:
      decl.ok === true &&
      decl.value.includes("共享核三件") &&
      decl.value.includes("四主题处置"),
  });

  // 保留面投影：四主题都必须两侧有入账能力（防「声明了边界却没落事实」）。
  const retBag = new DiagBag();
  const ret = projectTopicRetention();
  out.push({
    name: "四主题保留面逐能力入账（ADR 与台账不脱节）",
    expect: `${OVERLAP_TOPICS.length} 主题两侧均非空`,
    actual: outcomeText(ret, (v) => v.map((r) => `${r.topicLabel}(G${r.retainedByG.length}/H${r.retainedByH.length})`).join(" "), (c) => `未过：${retBag.size} 条诊断 / ${c}`),
    passed: ret.ok === true && ret.value.every((r) => r.retainedByG.length > 0 && r.retainedByH.length > 0),
  });

  // 处置词与保留面正交：即使四主题处置全为 h-lead，G 侧仍须有保留能力。
  const allHLead = BOUNDARY_ADR_TABLE.every((a) => a.disposition === "h-lead" || a.disposition === "merge-adr");
  const gHasRetention = ret.ok === true && ret.value.some((r) => r.retainedByG.length > 0);
  out.push({
    name: "处置词与保留面正交（h-lead 不等于 G 侧无保留面）",
    expect: allHLead ? "无 g-retain 时 G 侧仍有保留能力" : "含 g-retain，G 侧必有保留面",
    actual: ret.ok === true ? `G 侧保留条目合计 ${ret.value.reduce((s, r) => s + r.retainedByG.length, 0)}` : "投影失败",
    passed: allHLead ? gHasRetention : gHasRetention,
  });
  return out;
}

/** 自检四：查重对账（台账三态/ 词表覆盖 / 四主题 shared 结论齐）。 */
export function selfCheckDedup(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();
  const dedup = auditDedupLedger(bag);
  out.push({
    name: "查重台账机检通过",
    expect: `${DEDUP_LEDGER.length} 条零诊断`,
    actual: outcomeText(dedup, (v) => `${v.length} 条零诊断`, () => `未过：${bag.size} 条诊断`),
    passed: dedup.ok,
  });

  const sharedCount = DEDUP_LEDGER.filter((e) => e.verdict === "dedup-shared").length;
  out.push({
    name: "台账含共享结论（同核同对拍的能力已入账）",
    expect: "≥4 条 shared（四主题各至少一）",
    actual: `${sharedCount} 条 shared`,
    passed: sharedCount >= 4,
  });

  const hOnly = DEDUP_LEDGER.filter((e) => e.verdict === "dedup-h-only").length;
  const gOnly = DEDUP_LEDGER.filter((e) => e.verdict === "dedup-g-only").length;
  out.push({
    name: "台账三态齐备（本域独有 / 对端独有 / 共享）",
    expect: "三态均非空",
    actual: `h-only=${hOnly} / g-only=${gOnly} / shared=${sharedCount}`,
    passed: hOnly > 0 && gOnly > 0 && sharedCount > 0,
  });

  const vocabBag = new DiagBag();
  const vocab = auditVocabCoverage(vocabBag);
  out.push({
    name: "词表覆盖无空白",
    expect: `${VOCAB_CLAIMS.length} 词全部认领`,
    actual: outcomeText(vocab, (v) => `${v.length} 词已认领`, () => `未过：${vocabBag.size} 条诊断`),
    passed: vocab.ok,
  });

  const bad = auditDedupLedger(new DiagBag());
  out.push({
    name: "查重机检对当前台账为绿（阴性对照）",
    expect: "ok=true",
    actual: outcomeText(bad, () => "ok=true", (c) => `ok=false / ${c}`),
    passed: bad.ok,
  });
  return out;
}

/** 自检五：组内 20 条分工 + 开工门禁整体放行。 */
export function selfCheckWorkBreakdownAndGates(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();
  const work = auditWorkBreakdown(bag);
  out.push({
    name: "H01 分工 20 条齐备",
    expect: `${H01_WORK_ITEM_COUNT} 条`,
    actual: outcomeText(work, (v) => `${v} 条`, () => `未过：${bag.size} 条诊断`),
    passed: work.ok && work.value === H01_WORK_ITEM_COUNT,
  });

  const items = H01_WORK_ITEMS.map((w) => w.item);
  out.push({
    name: "条目号连续覆盖 F1401~F1420",
    expect: `${DOMAIN.itemLo}~${DOMAIN.itemLo + H01_WORK_ITEM_COUNT - 1}`,
    actual: `${items[0]}~${items[items.length - 1]}`,
    passed: items.length === H01_WORK_ITEM_COUNT && items[0] === DOMAIN.itemLo,
  });

  const sumLines = H01_WORK_ITEMS.reduce((s, w) => s + w.targetLines, 0);
  out.push({
    name: "H01 目标行数合计入册",
    expect: "> 0 且逐条明示",
    actual: `${sumLines} 行/ 20 条`,
    passed: sumLines > 0,
  });

  // 门禁放行（用构造回执）。
  const summary = kickoffDomain(sampleReceipt());
  out.push({
    name: "全部门禁放行（以构造回执为输入）",
    expect: "cleared=true",
    actual: outcomeText(summary, (v) => (v.cleared ? "cleared=true" : "cleared=false"), (c) => `未放行：${c}`),
    passed: summary.ok && summary.value.cleared,
  });

  // 阴性对照：无回执时必须阻断（防「默认通过」这一最危险的写法）。
  const noReceipt = kickoffDomain(undefined as unknown as HandoverReceipt);
  out.push({
    name: "阴性对照：缺移交回执时阻断开工",
    expect: "ok=false 且 code=KICKOFF_BLOCKED",
    actual: outcomeText(noReceipt, () => "ok=true（未阻断）", (c) => `ok=false / ${c}`),
    passed: outcomeCode(noReceipt) === "KICKOFF_BLOCKED",
  });

  out.push({
    name: "门禁表齐备（判据四条全覆盖）",
    expect: `${KICKOFF_GATES.length} 道门禁`,
    actual: `${KICKOFF_GATES.length} 道`,
    passed: KICKOFF_GATES.length >= 4,
  });
  return out;
}

/** 自检总入口：跑完全部五组断言并汇总。 */
export function selfCheckAll(): {
  readonly checks: readonly SelfCheck[];
  readonly passed: number;
  readonly failed: number;
  readonly allPassed: boolean;
} {
  const checks: SelfCheck[] = [
    ...selfCheckDomainKickoff(),
    ...selfCheckBoundaryAdr(),
    ...selfCheckReuseDeclaration(),
    ...selfCheckDedup(),
    ...selfCheckWorkBreakdownAndGates(),
  ];
  const passed = checks.filter((c) => c.passed).length;
  const failed = checks.length - passed;
  return { checks, passed, failed, allPassed: failed === 0 };
}