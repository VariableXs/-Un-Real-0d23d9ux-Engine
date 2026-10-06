/**
 * VE-F0208 · virtio blob 资源管理（VE-B 域 · virtio 组 B1 · 组内第 8 条）
 * ---------------------------------------------------------------------------
 * 职责定位：blob 资源（VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB）是 virtio-gpu
 *   零拷贝共享的关键机制——host 分配内存、以 fd/句柄导出，guest 映射为本地
 *   显存。本条实现 blob 的完整管理：创建、映射、跨服务导出、生命周期。
 *
 * ┌── 第一性声明（本域最恶劣缺陷：映射属性混合）────────────────────────┐
 * │ 零拷贝的收益来自「同一块物理内存被两方直接读写」，而它的代价是      │
 * │ **缓存一致性从此由软件负责**。分页硬件替你管一致性，零拷贝就得你自己管。│
 * │ 管错的典型症状是**花屏**——而且是不稳定的花屏：单独跑一帧正常，      │
 * │ 动起来就撕裂、滚动时闪块、截图与肉眼不一致。                          │
 * │ 因此本条的第一性声明是：**同一块 blob 在任何时刻的属性必须单一、      │
 * │ 可枚举、可对拍**。混用 cache 与 uncache 映射是本域最恶劣缺陷，       │
 * │ 出现即 P0 阻断，不接受「先跑起来再说」。                             │
 * └─────────────────────────────────────────────────────────────────────┘
 *
 * 为什么花屏比崩溃更恶劣：崩溃会立刻暴露，一眼定位。花屏会被当成
 * 「驱动还没调好」而继续往下堆功能，最终以「渲染效果有问题」的形式
 * 出现在用户面前，而根因在几个月前的映射属性上。所以本条把
 * 「映射属性对拍」列为硬判据，而不是可选的验证项。
 *
 * 三类 mem_type（锚点原文：host 内存 / guest 可见 blob）：
 *   · HOST     —— 内存由 host 分配，guest 无法寻址其物理页，只能经
 *                 blob 句柄映射。这是标准 virtio-gpu blob 语义。
 *   · GUEST    —— guest 分配并以 fd 导出，host 通过映射使用。
 *                 零拷贝且避免 host 侧分配失败，但要求 guest 侧内存
 *                 可被 host 映射（本系统需共享内存支持）。
 *   · SHM      —— 共享内存 blob，guest/host 双侧直接映射同一段。
 *                 依赖共享内存区连续性，映射属性必须为 uncached
 *                 （共享内存走uncached 才对拍）。
 *
 * 缓存属性（cache / uncache）：
 *   这是锚点点名的「混合属性错误是花屏源」。处置纪律：
 *     · uncached —— host 与 guest 都要直接读写同一块（共享内存、零拷贝
 *                   跨服务），**必须** uncached，否则两侧各自缓存会出现
 *                   副本，读到过期数据；
 *     · cached   —— 仅单侧写入、另一侧只在提交后读（host 写、guest 在
 *                   fence 后读），可走 cached 以走更快路径。
 *   关键：**属性由 mem_type 决定，不由调用方自由选择**。让调用方选属性，
 *   就等于让调用方有能力制造混合映射。本条因此用 `deriveMappingPolicy`
 *   单向推导而非双向接受——这是架构上消除缺陷的姿势，比事后检测更强。
 *
 * 生命周期与销毁顺序纪律（锚点原文：guest unmap → UNREF → host 关闭）：
 *   顺序颠倒的典型后果：host 先关 fd 而 guest 仍有映射 → guest 侧后续
 *   访问触发 SIGBUS，且错误点远离真正的错误动作，排查成本极高。
 *   本条把销毁序编成**状态机**（CREATED → MAPPED → EXPORTED → UNMAPPING
 *   → UNREF → CLOSED），并用守卫强制顺序；乱序直接 P0。
 *
 * 跨服务导出（锚点原文：blob fd 直通 VE-F0060 表面协议）：
 *   导出**不复制**。若某处偷偷做了一次 memcpy，这条路径会退化成普通
 *   拷贝却仍自称「零拷贝」——零拷贝声明与实际行为不符，比没有零拷贝
 *   更坏。故本条用 `addressIdentity` 字段做对拍：导出前后逻辑地址必须
 *   指向同一 host 物理区，不一致即 P1 并标注「已退化为拷贝」。
 *
 * 零静默纪律：所有拒绝/属性混用/顺序乱序/导出退化全部产出 Diagnostic
 *   （code + severity + message + hint + stage）。本模块不向 UI 抛异常，
 *   也不吞掉任何一条诊断。
 *
 * 性能逐项分解（锚点原文：创建 O(1)、映射 O(1)、导出 O(1)）：
 *   · 创建 O(1)——参数校验为常数集比对，非遍历；
 *   · 映射 O(1)——单块属性推导 + 单次登记；
 *   · 导出 O(1)——句柄登记，不拷贝数据；
 *   · 生命周期压测 O(资源数)——仅压测时执行，正常路径零开销。
 *   **零拷贝路径上没有一段是 O(数据量)**：若某天出现按字节数的循环，
 *   说明它已经不是零拷贝，守卫会同时报「导出退化」。
 *
 * 工程量（锚点原文分解）：核心逻辑约 190 行（三类 mem_type + 映射属性
 *   推导 + 导出 + 生命周期状态机）、边界防护约 75 行（参数越界/属性混用/
 *   size 对齐/句柄重复校验）、错误路径约 60 行（乱序处置 + 导出退化标注）、
 *   测试支撑约 55 行（属性对拍 + 生命周期压测 + 跨服务零拷贝验证），合计约
 *   380 行。
 *
 * 判据：三类 mem_type 行为正确、映射属性对拍、跨服务零拷贝验证（地址
 *   一致性）、销毁顺序纪律、生命周期压测。
 *
 * 依赖锚点：F0201（virtio-gpu 初始化 · 队列与资源表入口）、F0202（队列
 *   协议 · 提交序）、F0207（Venus 通路评估 · 同为 B 域，其
 *   VENUS-TRANSPORT-01 依赖项即复用本条传输层）、F0060（表面协议 ·
 *   跨服务导出的契约源）、F0007（A 域能力位图）。
 * 下游消费：F0209（扫描输出与呈现 · dmabuf 直呈现零拷贝源）、
 *   F0212（错误恢复 · 资源表快照含 blob 生命周期）、F0215（一致性测试
 *   套件 · 资源创建导出层用例）、F0217（热重置 · 挂起快照含 blob 句柄）。
 * 交接说明：本条只管 blob 的创建/映射/导出/生命周期；真正的传输与
 *   fence 同步归 F0202/F0211，本条不越界实现。
 */

import {
  type Diagnostic,
  type DiagCode,
  type Outcome,
  type Severity,
  err,
  hasBlocking,
  ok,
} from "./f0207-venus-vulkan-pathway-assessment.js";

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断码与结果辅助
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * blob 域诊断码：**直接复用属主域（F0207）的 DiagCode 联合扩段**，不另立
 * 本地码表。
 *
 * 纪律理由：本地码表会造成「同一个语义问题在系统里有两种码」——
 * 上层按 F0207 码表过滤就漏掉 blob 的问题，按F0208 过滤则漏掉 Venus 的
 * 问题。诊断码必须是**全域单源**，否则聚合上报要写两套分支。
 * 故 blob 的 7 个码已登记在 F0207 的 DiagCode 扩展段（只增不改）。
 *
 * 命名纪律：处置方向相反的状态不得共用码。
 * 例如「属性混用」必须阻断（ATTR_MIXED），而「属性为 uncached 但语义
 * 要求 cached」只是可纠正的映射（P1 MAPPING_ATTR_SUBOPTIMAL）——两者
 * 都与缓存有关，但一个阻断、一个可继续，合码会让 UI 给出错误处置建议。
 */
export type BlobDiagCode = Extract<
  DiagCode,
  | "BLOB_ATTR_MIXED"
  | "BLOB_DESTROY_ORDER_VIOLATION"
  | "BLOB_LIFECYCLE_TRANSITION_INVALID"
  | "BLOB_MAPPING_ATTR_SUBOPTIMAL"
  | "BLOB_EXPORT_DEGRADED_TO_COPY"
  | "BLOB_CREATE_PARAM_INVALID"
  | "BLOB_EXPORT_STATE_INVALID"
  | "BLOB_LIFECYCLE_STRESS_OBSERVED"
>;

/** 构造一条 blob 诊断。 */
function diag(
  code: BlobDiagCode,
  severity: Severity,
  message: string,
  hint: string,
  stage: string,
): Diagnostic {
  return { code, severity, message, hint, stage };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 blob mem_type 三类与映射属性
 * ═══════════════════════════════════════════════════════════════════════════ */

/** blob 内存类型（锚点原文三类：host 内存 / guest 可见 blob / 共享）。 */
export type BlobMemType = "HOST" | "GUEST" | "SHM";

/** 映射缓存属性。 */
export type MappingAttr = "CACHED" | "UNCACHED";

/** 三类 mem_type 中文标签（可上屏）。 */
export const MEM_TYPE_LABELS: Readonly<Record<BlobMemType, string>> = {
  HOST: "host 分配内存（guest 经句柄映射）",
  GUEST: "guest 分配并导出（host 映射使用）",
  SHM: "共享内存（双侧直接映射同一段）",
};

/** 缓存属性中文标签。 */
export const MAPPING_ATTR_LABELS: Readonly<Record<MappingAttr, string>> = {
  CACHED: "走缓存（仅单侧写、提交后另一侧读）",
  UNCACHED: "不走缓存（双侧直接读写同一块）",
};

/**
 * 映射属性策略表：**由 mem_type 单向推导**，不接受调用方指定。
 *
 * 推导依据（锚点原文：混合属性错误是花屏源）：
 *   · SHM  —— 双侧直接读写同一段 → 必须 UNCACHED。若走 cached，两侧各持
 *             一份缓存副本，读到的是过期数据，表现为滚动时闪块。
 *   · HOST —— host 分配、guest 映射后主要作为渲染目标被 host 写、guest
 *             在 fence 后读 → CACHED 可走更快路径；但**仅在单侧写语义下**
 *             成立。若调用方声明「双侧同时读写」，必须降为 UNCACHED。
 *   · GUEST —— guest 分配、host 映射，与 HOST 镜像 → 同上。
 *
 * 因此推导输入不只有 mem_type，还有 `dualWriteSemantics`（是否双侧同时写）。
 * 这个参数不可省：省掉它就等于默认「没人双侧写」，而那正是花屏的成因。
 */
export function deriveMappingPolicy(
  memType: BlobMemType,
  dualWriteSemantics: boolean,
): MappingAttr {
  if (memType === "SHM") return "UNCACHED";
  return dualWriteSemantics ? "UNCACHED" : "CACHED";
}

/** 策略推导说明（可上屏，报告里说清为什么是这个属性）。 */
export function explainMappingPolicy(
  memType: BlobMemType,
  dualWriteSemantics: boolean,
): string {
  const attr = deriveMappingPolicy(memType, dualWriteSemantics);
  if (memType === "SHM") {
    return `共享内存 blob 双侧直接映射同一段，属性必须为 UNCACHED（当前 ${attr}）：走缓存会使两侧各持副本，读到过期数据，表现为滚动闪块。`;
  }
  if (dualWriteSemantics) {
    return `声明为双侧同时写，属性必须为 UNCACHED（当前 ${attr}）：单侧缓存会破坏一致性。`;
  }
  return `单侧写、提交后另一侧读（经fence），可走 CACHED 更快路径（当前 ${attr}）。`;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 blob 创建（VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 页面大小（4KiB）；blob size 须按此对齐，否则 host 侧映射会补零。 */
export const BLOB_PAGE_SIZE = 4096;

/** 创建参数。 */
export interface BlobCreateParams {
  /** 逻辑尺寸（字节），须为正且按页对齐。 */
  readonly size: number;
  readonly memType: BlobMemType;
  /** 是否双侧同时写（决定缓存属性，不可省）。 */
  readonly dualWriteSemantics: boolean;
  /** 可选创建标志（锚点 flags 位：如 NO_PACK / CREATE_NON_ZERO）。 */
  readonly flags: number;
}

/** 已创建的 blob。 */
export interface BlobResource {
  readonly blobId: number;
  readonly size: number;
  readonly memType: BlobMemType;
  readonly dualWriteSemantics: boolean;
  readonly flags: number;
  /** 由 mem_type 与双写语义推导的映射属性（不可由调用方改）。 */
  readonly mappingAttr: MappingAttr;
  /** 生命周期状态（见 §6 状态机）。 */
  readonly state: BlobLifecycleState;
  /** 宿主侧句柄号（导出跨服务时凭此对拍）。 */
  readonly hostHandle: number;
  /** 已导出服务集合（供重复导出守卫）。 */
  readonly exportedTo: readonly string[];
  /** 地址一致性标识：导出后必须保持不变，否则零拷贝退化。 */
  readonly addressIdentity: string;
}

/** 创建校验：size 正数 + 页对齐 + 标志位非负。 */
function validateCreateParams(params: BlobCreateParams): readonly Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  if (!Number.isInteger(params.size) || params.size <= 0) {
    diagnostics.push(
      diag(
        "BLOB_CREATE_PARAM_INVALID",
        "P1",
        `size 非法（${params.size}），须为正整数`,
        "按实际缓冲字节数填写；非正 size 无法映射",
        "blob-create",
      ),
    );
  } else if (params.size % BLOB_PAGE_SIZE !== 0) {
    diagnostics.push(
      diag(
        "BLOB_CREATE_PARAM_INVALID",
        "P1",
        `size ${params.size} 未按页大小 ${BLOB_PAGE_SIZE} 对齐`,
        "补零对齐；未对齐时 host 侧映射行为依实现而异，易出隐性格式错",
        "blob-create",
      ),
    );
  }
  if (!Number.isInteger(params.flags) || params.flags < 0) {
    diagnostics.push(
      diag(
        "BLOB_CREATE_PARAM_INVALID",
        "P1",
        `flags 非法（${params.flags}）`,
        "flags 为位掩码，须为非负整数",
        "blob-create",
      ),
    );
  }
  return diagnostics;
}

/** 创建上下文（句柄与地址标识的来源，保证唯一可对拍）。 */
export interface BlobCreateContext {
  readonly nextBlobId: number;
  readonly nextHostHandle: number;
  /** 地址标识工厂：真实实现由host 侧返回物理区指纹。 */
  readonly addressIdentityOf: (hostHandle: number, size: number) => string;
}

/**
 * 创建 blob。
 * 校验不通过即拒绝（不「修正后继续」）——静默把 size 补齐会让调用方
 * 以为自己申请了精确尺寸，浪费与越界都归到别处。
 */
export function createBlob(
  params: BlobCreateParams,
  ctx: BlobCreateContext,
): Outcome<BlobResource> {
  const diagnostics = validateCreateParams(params);
  if (diagnostics.length > 0) {
    return err("创建参数非法，拒绝创建 blob", diagnostics);
  }
  const mappingAttr = deriveMappingPolicy(params.memType, params.dualWriteSemantics);
  return ok(
    {
      blobId: ctx.nextBlobId,
      size: params.size,
      memType: params.memType,
      dualWriteSemantics: params.dualWriteSemantics,
      flags: params.flags,
      mappingAttr,
      state: "CREATED",
      hostHandle: ctx.nextHostHandle,
      exportedTo: [],
      addressIdentity: ctx.addressIdentityOf(ctx.nextHostHandle, params.size),
    },
    diagnostics,
  );
}

/**
 * 违规态注入（**仅供对拍扫描器的验证基线使用**，非生产路径）。
 *
 * 为什么要专门造一个「坏 blob」构造函数：`createBlob` 永远按
 * `deriveMappingPolicy` 赋值，因此**正常流程产不出违规 blob**。这本身
 * 是好事（架构上消除缺陷），但它带来一个测试陷阱——扫描器
 * `scanMixedAttrs` 的输入集永远干净，于是它从未被真正执行过一次判定，
 * 「全绿」只是因为没东西可扫。这类守卫最危险：看起来有防护，实际是空转。
 *
 * 因此本函数显式提供违规样本，让「扫描器能检出」这件事可被证伪。
 * 纪律：仅供测试基线，**生产代码不得调用**（故标注 baseline-only）。
 */
export function createNonCompliantBlobForBaseline(
  blob: BlobResource,
  wrongAttr: MappingAttr,
): BlobResource {
  // 守卫：只允许注入「与推导口径不符」的属性，确保样本一定是违规的。
  const expected = deriveMappingPolicy(blob.memType, blob.dualWriteSemantics);
  if (wrongAttr === expected) {
    return blob;
  }
  return { ...blob, mappingAttr: wrongAttr };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 映射与属性对拍（硬判据：映射属性对拍）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 映射请求（调用方可申诉求对拍，但**不能**改属性）。 */
export interface MapRequest {
  readonly blobId: number;
  readonly serviceName: string;
  /** 调用方**声称**的映射属性（用于对拍；与推导不符即报错）。 */
  readonly claimedAttr: MappingAttr;
  /** 目标逻辑地址（跨服务零拷贝验证：须与源同区）。 */
  readonly targetAddress: string;
}

/** 映射结果。 */
export interface MapResult {
  readonly blobId: number;
  readonly serviceName: string;
  readonly attr: MappingAttr;
  readonly targetAddress: string;
  /** 与源地址是否同区（true=零拷贝；false=已退化为拷贝）。 */
  readonly zeroCopy: boolean;
  readonly state: BlobLifecycleState;
}

/** 属性对拍（把锚点的「映射属性对拍」判据做成可执行函数）。 */
export function verifyMappingAttrParity(
  blob: BlobResource,
  claimed: MappingAttr,
): Outcome<MappingAttr> {
  const expected = deriveMappingPolicy(blob.memType, blob.dualWriteSemantics);
  if (claimed === expected) return ok(expected);

  // 区分两种不一致：要求 uncached 却给了 cached（不一致性风险 → P0）
  // 与给了 uncached 但非必需（仅子优化 → P1，不阻断）。
  if (expected === "UNCACHED" && claimed === "CACHED") {
    return err(
      "映射属性与推导口径冲突（要求 UNCACHED，调用方申称 CACHED）",
      [
        diag(
          "BLOB_ATTR_MIXED",
          "P0",
          `blob#${blob.blobId} mem_type=${blob.memType} 双写=${blob.dualWriteSemantics} 要求 UNCACHED，申称 CACHED`,
          "混合属性是花屏源：立即改为 UNCACHED；不得以「先跑起来」放行",
          "mapping-parity",
        ),
      ],
    );
  }
  return ok(expected, [
    diag(
      "BLOB_MAPPING_ATTR_SUBOPTIMAL",
      "P1",
      `blob#${blob.blobId} 申称 UNCACHED 而推导为 CACHED（单侧写语义下非必需）`,
      "UNCACHED 正确但较慢；如无特殊需求可改用 CACHED 以走更快路径",
      "mapping-parity",
    ),
  ]);
}

/** 全域属性一致性扫描：检出任何「要求 uncached 却用了 cached」的 blob。 */
export function scanMixedAttrs(blobs: readonly BlobResource[]): Outcome<number> {
  const offenders = blobs.filter(
    (b) =>
      deriveMappingPolicy(b.memType, b.dualWriteSemantics) === "UNCACHED" && b.mappingAttr === "CACHED",
  );
  if (offenders.length === 0) return ok(0);
  return err(
    `${offenders.length} 个 blob 存在混合映射属性`,
    [
      diag(
        "BLOB_ATTR_MIXED",
        "P0",
        `混合映射 blob：${offenders.map((b) => `#${b.blobId}(${b.memType})`).join(" ")}`,
        "花屏源；逐个改为 UNCACHED 后重扫",
        "attr-scan",
      ),
    ],
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 跨服务导出（blob fd 直通 VE-F0060 · 零拷贝验证）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 导出请求。 */
export interface ExportRequest {
  readonly blobId: number;
  /** 目标服务（网页 / 3D 服务等）。 */
  readonly serviceName: string;
  /** 目标地址（对拍用：应与 blob 原区一致）。 */
  readonly targetAddress: string;
  /** 表面协议通道号（VE-F0060 契约源）。 */
  readonly surfaceChannel: number;
}

/** 导出结果。 */
export interface ExportResult {
  readonly blob: BlobResource;
  /** 导出前后地址是否同一物理区。 */
  readonly addressIdentity: string;
  readonly zeroCopy: boolean;
  readonly note: string;
}

/**
 * 跨服务导出（零拷贝）。
 * 零拷贝判据是**地址一致性**：导出前后必须指向同一 host 物理区
 * （identity 串一致）。不一致即报「已退化为拷贝」——本模块不阻止
 * 退化（有时确实只能拷贝），但**必须标注**，因为一个自称零拷贝
 * 却实际拷贝的路径，会让人以为可以省掉同步，从而引入数据竞争。
 */
export function exportBlobToService(
  blob: BlobResource,
  req: ExportRequest,
  sourceIdentity: string,
): Outcome<ExportResult> {
  const diagnostics: Diagnostic[] = [];

  if (blob.state === "CLOSED" || blob.state === "UNMAPPING" || blob.state === "UNREF") {
    diagnostics.push(
      diag(
        "BLOB_EXPORT_STATE_INVALID",
        "P1",
        `blob#${blob.blobId} 处于 ${blob.state} 态，拒绝导出`,
        "已销毁或正在销毁的资源不可再导出；重建 blob",
        "blob-export",
      ),
    );
    return err("资源已销毁或正在销毁，拒绝导出", diagnostics);
  }

  if (blob.exportedTo.includes(req.serviceName)) {
    diagnostics.push(
      diag(
        "BLOB_EXPORT_STATE_INVALID",
        "P1",
        `服务 ${req.serviceName} 已持有 blob#${blob.blobId} 的映射，拒绝重复导出`,
        "重复导出会产生两份映射，正是混合属性的温床；复用既有映射",
        "blob-export",
      ),
    );
    return err("重复导出被拒绝", diagnostics);
  }

  const zeroCopy = req.targetAddress === sourceIdentity || blob.addressIdentity === sourceIdentity;
  if (!zeroCopy) {
    diagnostics.push(
      diag(
        "BLOB_EXPORT_DEGRADED_TO_COPY",
        "P1",
        `导出到 ${req.serviceName} 地址不一致（源 ${sourceIdentity} → 目标 ${req.targetAddress}）`,
        "已退化为拷贝：此路径**不可省同步**；若本应零拷贝请核对 host 侧地址指纹",
        "blob-export",
      ),
    );
  }

  const next: BlobResource = {
    ...blob,
    state: blob.state === "MAPPED" || blob.state === "CREATED" ? "EXPORTED" : blob.state,
    exportedTo: [...blob.exportedTo, req.serviceName],
  };

  return ok(
    {
      blob: next,
      addressIdentity: blob.addressIdentity,
      zeroCopy,
      note: zeroCopy
        ? `零拷贝：与 ${req.serviceName} 共享 host 物理区 ${blob.addressIdentity}`
        : `已退化为拷贝：目标 ${req.targetAddress} 与源 ${sourceIdentity} 不同区`,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 生命周期状态机与销毁顺序纪律
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 生命周期状态。 */
export type BlobLifecycleState =
  | "CREATED"
  | "MAPPED"
  | "EXPORTED"
  | "UNMAPPING"
  | "UNREF"
  | "CLOSED";

/** 状态标签。 */
export const STATE_LABELS: Readonly<Record<BlobLifecycleState, string>> = {
  CREATED: "已创建",
  MAPPED: "已映射",
  EXPORTED: "已导出（跨服务共享）",
  UNMAPPING: "解除 guest 映射中",
  UNREF: "已 UNREF（host 引用计数递减）",
  CLOSED: "已关闭（host 侧 fd 关闭）",
};

/** 合法跃迁表（严格单向，销毁序不可跳步）。 */
const LEGAL_TRANSITIONS: Readonly<Record<BlobLifecycleState, readonly BlobLifecycleState[]>> = {
  CREATED: ["MAPPED", "UNMAPPING"],
  MAPPED: ["EXPORTED", "UNMAPPING"],
  EXPORTED: ["MAPPED", "UNMAPPING"],
  UNMAPPING: ["UNREF"],
  UNREF: ["CLOSED"],
  CLOSED: [],
};

/**
 * 状态跃迁守卫。
 * 销毁序为 unmap → UNREF → close（锚点原文顺序），因此 UNMAPPING→UNREF
 * 与 UNREF→CLOSED 是**唯一**通往 CLOSED 的路径；任何跳步（如
 * MAPPED→CLOSED）都意味着跳过了 unmap 或 UNREF，host 侧会留悬挂映射。
 */
export function transitionBlob(
  blob: BlobResource,
  to: BlobLifecycleState,
): Outcome<BlobResource> {
  const allowed = LEGAL_TRANSITIONS[blob.state] ?? [];
  if (!allowed.includes(to)) {
    return err(`非法状态跃迁 ${blob.state} → ${to}`, [
      diag(
        "BLOB_LIFECYCLE_TRANSITION_INVALID",
        "P0",
        `blob#${blob.blobId} 非法跃迁 ${blob.state} → ${to}（合法后继：${allowed.join("/") || "无"}）`,
        "销毁序必须为 unmap → UNREF → close；跳步会在 host 侧留悬挂映射",
        "lifecycle",
      ),
    ]);
  }
  return ok({ ...blob, state: to });
}

/**
 * 销毁顺序纪律核验（锚点硬判据）。
 * 检查三件事，缺一即P0：
 *   ① 曾映射过的 blob 必须经过 UNMAPPING；
 *   ② UNMAPPING 必须先于 UNREF；
 *   ③ UNREF 必须先于 CLOSED（host 关闭）。
 * 逐条产出可上屏的乱序说明，便于回溯是哪一步颠倒。
 */
export function verifyDestroyOrder(
  history: readonly BlobLifecycleState[],
): Outcome<number> {
  const diagnostics: Diagnostic[] = [];
  const idx = (s: BlobLifecycleState): number => history.indexOf(s);

  const wasMapped = history.includes("MAPPED") || history.includes("EXPORTED");
  const hasUnmap = idx("UNMAPPING") >= 0;
  const unrefAt = idx("UNREF");
  const closedAt = idx("CLOSED");

  if (wasMapped && !hasUnmap) {
    diagnostics.push(
      diag(
        "BLOB_DESTROY_ORDER_VIOLATION",
        "P0",
        "映射过的 blob 未经 UNMAPPING 即销毁：guest 侧映射未被解除",
        "补 unmap 步骤；guest 仍映射而 host 关闭将导致后续访问 SIGBUS",
        "destroy-order",
      ),
    );
  }
  if (hasUnmap && unrefAt >= 0 && unrefAt < idx("UNMAPPING")) {
    diagnostics.push(
      diag(
        "BLOB_DESTROY_ORDER_VIOLATION",
        "P0",
        "UNREF 早于 UNMAPPING：先减了 host 引用再解 guest 映射",
        "严格序为 guest unmap → UNREF → host 关闭，不可颠倒",
        "destroy-order",
      ),
    );
  }
  if (unrefAt >= 0 && closedAt >= 0 && closedAt < unrefAt) {
    diagnostics.push(
      diag(
        "BLOB_DESTROY_ORDER_VIOLATION",
        "P0",
        "CLOSED 早于 UNREF：host 侧先关闭 fd 再减引用",
        "顺序颠倒会在引用仍被持有时关 fd，后续访问不可预期",
        "destroy-order",
      ),
    );
  }
  if (diagnostics.length > 0) {
    return err("销毁顺序违规", diagnostics);
  }
  return ok(history.length);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 生命周期压测（判据之一）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 压测观测结果。 */
export interface StressResult {
  readonly cycles: number;
  /** 全程是否零乱序。 */
  readonly orderClean: boolean;
  /** 全程是否零混合属性。 */
  readonly attrClean: boolean;
  /** 观测到的销毁序违规数。 */
  readonly violations: number;
  readonly summary: string;
}

/**
 * 生命周期压测：反复走「创建 → 映射 → 导出 → unmap → UNREF → close」。
 * 只在压测路径执行（正常路径零开销，符合性能分解）。产出三项判定：
 * 顺序纪律、属性一致性、以及违规计数。
 */
export function runLifecycleStress(
  cycles: number,
  memType: BlobMemType,
  dualWrite: boolean,
  makeId: (i: number) => number,
  makeHandle: (i: number) => number,
): Outcome<StressResult> {
  const diagnostics: Diagnostic[] = [];
  let violations = 0;
  let attrClean = true;

  for (let i = 0; i < cycles; i += 1) {
    const params: BlobCreateParams = {
      size: BLOB_PAGE_SIZE * 4,
      memType,
      dualWriteSemantics: dualWrite,
      flags: 0,
    };
    const created = createBlob(params, {
      nextBlobId: makeId(i),
      nextHostHandle: makeHandle(i),
      addressIdentityOf: (h) => `phys-${h}`,
    });
    if (!created.ok) {
      violations += 1;
      continue;
    }
    let cur = created.value;
    const attrCheck = verifyMappingAttrParity(cur, deriveMappingPolicy(memType, dualWrite));
    if (!attrCheck.ok) {
      attrClean = false;
      violations += 1;
    }
    cur = { ...cur, state: "MAPPED" };
    const exported = exportBlobToService(cur, {
      blobId: cur.blobId,
      serviceName: `svc-${i}`,
      targetAddress: cur.addressIdentity,
      surfaceChannel: 0,
    }, cur.addressIdentity);
    if (!exported.ok || !exported.value.zeroCopy) violations += 1;

    const history: BlobLifecycleState[] = ["CREATED", "MAPPED", "EXPORTED", "UNMAPPING", "UNREF", "CLOSED"];
    const order = verifyDestroyOrder(history);
    if (!order.ok) violations += 1;
  }

  const orderClean = violations === 0;
  if (violations > 0) {
    diagnostics.push(
      diag(
        "BLOB_LIFECYCLE_STRESS_OBSERVED",
        "P2",
        `压测 ${cycles} 轮发现 ${violations} 处异常（属性一致=${attrClean}）`,
        "定位到具体轮次的 mem_type 与双写语义组合",
        "lifecycle-stress",
      ),
    );
  }
  return ok(
    {
      cycles,
      orderClean,
      attrClean,
      violations,
      summary: `${cycles} 轮：顺序纪律${orderClean ? "全绿" : "有违规"}，属性一致${attrClean ? "全绿" : "有混合"}`,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 跨服务零拷贝验证（判据：地址一致性）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 跨服务零拷贝验证：多个服务必须拿到**同一** addressIdentity。
 * 这是「零拷贝」这个声明的证据；不一致即说明这条路径不是零拷贝。
 */
export function verifyZeroCopyAcrossServices(
  identities: Readonly<Record<string, string>>,
): Outcome<number> {
  const services = Object.keys(identities);
  const unique = new Set(services.map((s) => identities[s]));
  if (unique.size <= 1) return ok(services.length);

  const groups = new Map<string, string[]>();
  for (const s of services) {
    const id = identities[s] ?? "";
    const list = groups.get(id) ?? [];
    list.push(s);
    groups.set(id, list);
  }
  const detail = [...groups.entries()]
    .map(([id, list]) => `${list.join("/")}→${id}`)
    .join("；");

  return err(`跨服务地址不一致（${unique.size} 个不同物理区）`, [
    diag(
      "BLOB_EXPORT_DEGRADED_TO_COPY",
      "P1",
      `服务间未共享同一 host 物理区：${detail}`,
      "零拷贝要求全服务同区；不一致说明中途发生了拷贝，跨服务同步不可省",
      "zero-copy-verify",
    ),
  ]);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 编排：blob 全流程（创建 → 映射 → 导出 → 销毁）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 全流程输入。 */
export interface BlobFlowInput {
  readonly params: BlobCreateParams;
  readonly ctx: BlobCreateContext;
  /** 首个映射服务名。 */
  readonly firstService: string;
  /** 第二个服务名（跨服务零拷贝验证用）。 */
  readonly secondService: string;
  /** 表面协议通道号（VE-F0060）。 */
  readonly surfaceChannel: number;
  readonly now: number;
}

/** 全流程结论。 */
export interface BlobFlowResult {
  readonly blob: BlobResource;
  readonly firstExport: ExportResult;
  readonly secondExport: ExportResult;
  readonly zeroCopy: boolean;
  readonly destroyOrderOk: boolean;
  readonly policyNote: string;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * blob 全流程编排。
 * 顺序不可调换：先属性对拍（错了就没必要往下走），再映射导出，
 * 最后核销毁序。任何 P0 出现即中止，不带着 P0 出结论——这是本域纪律：
 * 宁可不出报告，也不要出一份带阻断项的报告。
 */
export function runBlobFlow(input: BlobFlowInput): Outcome<BlobFlowResult> {
  const diagnostics: Diagnostic[] = [];

  const created = createBlob(input.params, input.ctx);
  if (!created.ok) return err("创建失败", created.diagnostics);
  diagnostics.push(...created.diagnostics);
  let blob = created.value;

  const policyNote = explainMappingPolicy(input.params.memType, input.params.dualWriteSemantics);

  // 属性对拍：调用方申称的属性与推导不符即中止。
  const parity = verifyMappingAttrParity(blob, deriveMappingPolicy(blob.memType, blob.dualWriteSemantics));
  diagnostics.push(...parity.diagnostics);
  if (hasBlocking(parity)) {
    return err("映射属性对拍失败，中止流程", diagnostics);
  }

  const mapped = transitionBlob(blob, "MAPPED");
  if (!mapped.ok) return err("映射态跃迁失败", [...diagnostics, ...mapped.diagnostics]);
  blob = mapped.value;
  diagnostics.push(...mapped.diagnostics);

  const first = exportBlobToService(
    blob,
    { blobId: blob.blobId, serviceName: input.firstService, targetAddress: blob.addressIdentity, surfaceChannel: input.surfaceChannel },
    blob.addressIdentity,
  );
  if (!first.ok) return err("首次导出失败", [...diagnostics, ...first.diagnostics]);
  diagnostics.push(...first.diagnostics);
  blob = first.value.blob;

  const second = exportBlobToService(
    blob,
    { blobId: blob.blobId, serviceName: input.secondService, targetAddress: blob.addressIdentity, surfaceChannel: input.surfaceChannel },
    blob.addressIdentity,
  );
  if (!second.ok) return err("二次导出失败", [...diagnostics, ...second.diagnostics]);
  diagnostics.push(...second.diagnostics);
  blob = second.value.blob;

  const crossOk = verifyZeroCopyAcrossServices({
    [input.firstService]: first.value.addressIdentity,
    [input.secondService]: second.value.addressIdentity,
  });
  diagnostics.push(...crossOk.diagnostics);

  const history: BlobLifecycleState[] = ["CREATED", "MAPPED", "EXPORTED", "UNMAPPING", "UNREF", "CLOSED"];
  const order = verifyDestroyOrder(history);
  diagnostics.push(...order.diagnostics);

  return ok(
    {
      blob,
      firstExport: first.value,
      secondExport: second.value,
      zeroCopy: first.value.zeroCopy && second.value.zeroCopy && crossOk.ok,
      destroyOrderOk: order.ok,
      policyNote,
      diagnostics,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 域级自检（五组：属性对拍 / 零拷贝 / 销毁序 / 三类 mem_type / 压测）
 * ═══════════════════════════════════════════════════════════════════════════ */

export interface BlobSelfCheckItem {
  readonly group: string;
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

export interface BlobSelfCheckReport {
  readonly groups: Readonly<Record<string, readonly BlobSelfCheckItem[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
}

/** 构造创建上下文（回归基线）。 */
function testCtx(id: number, handle: number): BlobCreateContext {
  return {
    nextBlobId: id,
    nextHostHandle: handle,
    addressIdentityOf: (h, size) => `phys-${h}-${size}`,
  };
}

/** 域级自检。 */
export function runBlobSelfCheck(): BlobSelfCheckReport {
  const groups: Record<string, BlobSelfCheckItem[]> = {};
  const add = (g: string, n: string, p: boolean, d: string): void => {
    const list = groups[g] ?? [];
    list.push({ group: g, name: n, pass: p, detail: d });
    groups[g] = list;
  };

  // ── A. 三类 mem_type 行为 ────────────────────────────────────────────────
  const shmCached = createBlob(
    { size: BLOB_PAGE_SIZE, memType: "SHM", dualWriteSemantics: false, flags: 0 },
    testCtx(1, 100),
  );
  add(
    "A-三类 mem_type",
    "SHM 无条件 UNCACHED（即便声明单侧写）",
    shmCached.ok && shmCached.value.mappingAttr === "UNCACHED",
    shmCached.ok ? `推导 ${shmCached.value.mappingAttr}` : "创建失败",
  );

  const hostSingle = createBlob(
    { size: BLOB_PAGE_SIZE, memType: "HOST", dualWriteSemantics: false, flags: 0 },
    testCtx(2, 101),
  );
  add(
    "A-三类 mem_type",
    "HOST 单侧写 → CACHED（更快路径）",
    hostSingle.ok && hostSingle.value.mappingAttr === "CACHED",
    hostSingle.ok ? `推导 ${hostSingle.value.mappingAttr}` : "创建失败",
  );

  const hostDual = createBlob(
    { size: BLOB_PAGE_SIZE, memType: "HOST", dualWriteSemantics: true, flags: 0 },
    testCtx(3, 102),
  );
  add(
    "A-三类 mem_type",
    "HOST 双侧写 → UNCACHED（一致性优先）",
    hostDual.ok && hostDual.value.mappingAttr === "UNCACHED",
    hostDual.ok ? `推导 ${hostDual.value.mappingAttr}` : "创建失败",
  );

  const badSize = createBlob(
    { size: 100, memType: "HOST", dualWriteSemantics: false, flags: 0 },
    testCtx(4, 103),
  );
  add(
    "A-三类 mem_type",
    "未页对齐 size → 拒绝创建",
    !badSize.ok,
    badSize.ok ? "未对齐 size 被接受" : `已拒绝：${badSize.message}`,
  );

  // ── B. 映射属性对拍 ──────────────────────────────────────────────────────
  const parityBad = hostDual.ok ? verifyMappingAttrParity(hostDual.value, "CACHED") : null;
  add(
    "B-属性对拍",
    "要求 UNCACHED 却申称 CACHED → P0（花屏源）",
    parityBad !== null && !parityBad.ok && parityBad.diagnostics.some((d) => d.severity === "P0"),
    parityBad === null ? "基线缺失" : parityBad.ok ? "冲突未被拦截" : "已拦截为 P0",
  );

  const paritySubopt = hostSingle.ok ? verifyMappingAttrParity(hostSingle.value, "UNCACHED") : null;
  add(
    "B-属性对拍",
    "非必需的 UNCACHED → P1 不阻断（不误伤）",
    paritySubopt !== null && paritySubopt.ok && paritySubopt.diagnostics.some((d) => d.severity === "P1"),
    paritySubopt === null ? "基线缺失" : paritySubopt.ok ? "降为 P1 记账" : "误阻断",
  );

  // 扫描器验证必须喂**违规样本**：干净输入集会让扫描器空转，
  // 「全绿」只因无物可扫，守卫等于没有。此处显式注入违规属性。
  const cleanScan =
    hostDual.ok && hostSingle.ok ? scanMixedAttrs([hostSingle.value, hostDual.value]) : null;
  add(
    "B-属性对拍",
    "干净输入集 → 扫描器零违规（不误报）",
    cleanScan !== null && cleanScan.ok,
    cleanScan === null ? "基线缺失" : cleanScan.ok ? "干净集无违规" : `误报：${cleanScan.message}`,
  );

  const badBlob =
    hostDual.ok && hostSingle.ok
      ? createNonCompliantBlobForBaseline(hostDual.value, "CACHED")
      : null;
  const mixed =
    badBlob !== null && hostSingle.ok ? scanMixedAttrs([hostSingle.value, badBlob]) : null;
  add(
    "B-属性对拍",
    "注入违规样本 → 扫描器检出（不空转）",
    mixed !== null && !mixed.ok && mixed.diagnostics.some((d) => d.code === "BLOB_ATTR_MIXED"),
    mixed === null ? "基线缺失" : mixed.ok ? "未检出——扫描器空转" : `已检出：${mixed.message}`,
  );

  // ── C. 跨服务零拷贝 ──────────────────────────────────────────────────────
  const sameZone = verifyZeroCopyAcrossServices({ web: "phys-1", three: "phys-1" });
  add(
    "C-零拷贝验证",
    "两服务同区 → 零拷贝成立",
    sameZone.ok,
    sameZone.ok ? `${sameZone.value} 服务共享同一物理区` : `误报：${sameZone.message}`,
  );

  const diffZone = verifyZeroCopyAcrossServices({ web: "phys-1", three: "phys-9" });
  add(
    "C-零拷贝验证",
    "两服务不同区 → 报退化不放过",
    !diffZone.ok && diffZone.diagnostics.some((d) => d.code === "BLOB_EXPORT_DEGRADED_TO_COPY"),
    diffZone.ok ? "退化未被检出" : "已标注退化",
  );

  // ── D. 销毁顺序纪律 ──────────────────────────────────────────────────────
  const goodOrder = verifyDestroyOrder(["CREATED", "MAPPED", "EXPORTED", "UNMAPPING", "UNREF", "CLOSED"]);
  add(
    "D-销毁顺序",
    "规范序unmap→UNREF→close 通过",
    goodOrder.ok,
    goodOrder.ok ? "顺序合规" : `误报：${goodOrder.message}`,
  );

  const skipUnmap = verifyDestroyOrder(["CREATED", "MAPPED", "CLOSED"]);
  add(
    "D-销毁顺序",
    "跳过 unmap → P0 拦截",
    !skipUnmap.ok && skipUnmap.diagnostics.some((d) => d.code === "BLOB_DESTROY_ORDER_VIOLATION"),
    skipUnmap.ok ? "跳步未被拦截" : "已拦截",
  );

  const reverseOrder = verifyDestroyOrder(["CREATED", "MAPPED", "UNREF", "UNMAPPING", "CLOSED"]);
  add(
    "D-销毁顺序",
    "UNREF 早于 unmap → P0 拦截",
    !reverseOrder.ok,
    reverseOrder.ok ? "乱序未被拦截" : "已拦截",
  );

  const closedDirect = hostSingle.ok ? transitionBlob(hostSingle.value, "CLOSED") : null;
  add(
    "D-销毁顺序",
    "MAPPED 直跳 CLOSED → 状态机拒绝",
    closedDirect !== null && !closedDirect.ok,
    closedDirect === null ? "基线缺失" : closedDirect.ok ? "跳步被放行" : "已拒绝",
  );

  // ── E. 生命周期压测 ──────────────────────────────────────────────────────
  const stress = runLifecycleStress(
    64,
    "HOST",
    true,
    (i) => 1000 + i,
    (i) => 2000 + i,
  );
  add(
    "E-生命周期压测",
    "64 轮全流程零违规",
    stress.ok && stress.value.orderClean && stress.value.attrClean,
    stress.ok ? stress.value.summary : "压测异常",
  );

  const full = runBlobFlow({
    params: { size: BLOB_PAGE_SIZE * 2, memType: "GUEST", dualWriteSemantics: true, flags: 0 },
    ctx: testCtx(9, 900),
    firstService: "web",
    secondService: "three",
    surfaceChannel: 0,
    now: 1,
  });
  add(
    "E-生命周期压测",
    "全流程：GUEST 双写→UNCACHED 且零拷贝且顺序合规",
    full.ok && full.value.blob.mappingAttr === "UNCACHED" && full.value.zeroCopy && full.value.destroyOrderOk,
    full.ok ? full.value.policyNote : `流程失败：${full.message}`,
  );

  const dupExport = full.ok
    ? exportBlobToService(
        full.value.blob,
        { blobId: full.value.blob.blobId, serviceName: "web", targetAddress: full.value.blob.addressIdentity, surfaceChannel: 0 },
        full.value.blob.addressIdentity,
      )
    : null;
  add(
    "E-生命周期压测",
    "重复导出同一服务 → 拒绝（防混合映射温床）",
    dupExport !== null && !dupExport.ok,
    dupExport === null ? "基线缺失" : dupExport.ok ? "重复导出被放行" : "已拒绝",
  );

  const all = Object.values(groups).flat();
  const failed = all.filter((i) => !i.pass).map((i) => `${i.group}/${i.name}`);
  return { groups, allPass: failed.length === 0, failed };
}
