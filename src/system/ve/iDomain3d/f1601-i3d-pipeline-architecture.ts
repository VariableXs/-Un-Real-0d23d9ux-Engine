/**
 * VE-F1601 · I 域开工与 3D 管线总架构（I 域 · 3D 管线域开工条 · 批次 I01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-I 域（F1601~F1800）的**开工条**。它不产出任何像素、不加载任何
 * 网格，只做一件事：把「3D 管线域要造什么、按什么次序造、与别的域如何划界、
 * 网格处理能向CGPU 卷借多少算力」四件事写成**可机检的契约**。
 *
 * 为什么开工条必须是代码而不是文档（域级立场）：
 *   3D 管线是全 VE 中横跨面最宽的域——它向下吃设备抽象（A 域）、向上被 2D 合成
 *   （D 域）消费、向侧面要着色器（C 域）、向资产侧要流式加载（Q 域）。
 *   若这些边界只写在文档里，半年后没人说得清「LOD 选择到底归 I09 还是 J 域」，
 *   于是两个域各实现一遍，行为还不一样。故本条把边界写成注册表 + 校验器：
 *   越界调用在开发期就被拒，而不是在集成期靠人吵。
 *
 * 四条锚点契约（逐条对应判据）：
 *   1. 十主题分层 —— 官方十主题（网格格式/顶点流水线/材质系统/PBR 模型/纹理采样/
 *      mipmap/蒙皮/实例化/LOD/剔除）逐项声明：层号、主题名、序号区间、职责边界、
 *      前置主题、产出的数据契约名。未声明的主题不可被引用（注册表封闭集）。
 *   2. 依赖序声明 —— 几何 → 流水线 → 材质 → 实例化/剔除 的推进序是**依赖驱动**的，
 *      不是排期偏好：材质需要顶点属性语义，实例化需要 PSO 描述，剔除需要包围球。
 *      本条提供拓扑排序与环检测，环即开工阻塞（显性失败，不静默按声明序排）。
 *   3. 跨域边界 —— A（设备抽象，本域消费）/C（着色器编译，本域消费 PSO）/D（2D 合成，
 *      本域的 3D 结果进 D 图层，F0838 联动）/Q（资产管线，本域的网格是它的几何子集）。
 *      边界即禁扩面：本域不管设备、不管着色器编译，只管几何数据与着色素材。
 *   4. CGPU 算力声明 —— 网格处理的 SIMD 算力需求以**跨卷标签**形式声明，
 *      交 CGPU 卷（F0258 序列化配套的算力侧）统一排产；本域只声明需要什么档位，
 *      不自己实现 AVX 分派（那是 CGPU 的活，重复实现=两处漂移）。
 *
 * 零静默纪律：注册表缺项、依赖成环、边界越权、算力档位不足，全部产出 Diagnostic
 * （code + message + hint）并由调用方聚合上报；本模块不抛异常、不吞诊断。
 *
 * 判据：十主题、分层、跨域边界、算力声明、判据。
 * 交接说明：本条是纯契约层，零 GPU 调用、零 DOM 依赖、零全局可变状态——
 *         可在任意宿主（浏览器/Worker/Node 校验脚本）中原样引入。
 *         下游 I01（网格容器 F1602）起逐项消费本条的 THEME_REGISTRY 与 BOUNDARY_TABLE。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 D 域同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 主题未在十主题注册表中登记（引用了不存在的层）。 */
  | "THEME_UNREGISTERED"
  /** 主题已登记但字段自相矛盾（如序号区间倒置、层号越界）。 */
  | "THEME_SPEC_INCONSISTENT"
  /** 前置主题缺失（引用了未登记的前置 id）。 */
  | "DEPENDENCY_UNKNOWN"
  /** 依赖图成环：开工阻塞，必须先断环。 */
  | "DEPENDENCY_CYCLE"
  /** 层号倒置：声明了「材质先于几何」这类违反依赖序的关系。 */
  | "LAYER_ORDER_VIOLATION"
  /** 跨域越权：本域调用了不属于自己职责的能力（禁扩面被突破）。 */
  | "BOUNDARY_VIOLATION"
  /** 边界契约缺失：某跨域关系未在边界表中声明即被使用。 */
  | "BOUNDARY_UNDECLARED"
  /** 算力档位不足：声明的 SIMD 需求超出 CGPU 卷提供的上限。 */
  | "SIMD_TIER_INSUFFICIENT"
  /** 算力标签字段缺失或越界（如 lanes 非 2 的幂且不在白名单内）。 */
  | "SIMD_LABEL_MALFORMED"
  /** 序号区间与同层其他主题重叠（两个主题抢同一批功能号）。 */
  | "ITEM_RANGE_OVERLAP"
  /** 数据契约名重复（两个主题声明了同名契约 = 后续必然撞名）。 */
  | "CONTRACT_NAME_DUPLICATE";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办（可操作提示）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  /** 人话描述：面向开发者排障，不含裸异常码、不含"可能""也许"。 */
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

/** 成功构造（diagnostics 允许携带非致命告警，例如算力降档建议）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备，diagnostics 含本条自身便于统一上报。 */
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
    for (const d of ds) this.items.push(d);
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

  /** 是否存在 error 级（当前全部码皆为 error 级，故等价于非空判定）。 */
  get hasError(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 十主题分层（判据一：十主题 + 分层）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 层号语义（分层即依赖层级，层号小者必须先完成）：
 *   L0 几何数据 —— 网格格式/属性/量化/修复/简化。产出「可被管线消费的几何字节流」。
 *   L1 顶点流水线 —— 变换/PSO 描述/批处理。产出「可提交命令缓冲的绘制批次」。
 *   L2 材质 —— PBR 模型/纹理采样/mipmap。产出「着色素材与材质参数集」。
 *   L3 场景级实例化 —— 蒙皮/实例化/LOD/剔除。产出「按可见性裁剪后的最终提交集」。
 *
 * 为什么材质（L2）排在流水线（L1）之后而不是之前（反直觉处，显式记录）：
 *   PSO（Pipeline State Object）的创建需要材质参与——顶点着色器与像素着色器的
 *   变体是由材质的关键参数（有无贴图、贴图格式数、alpha 模式）决定的。
 *   若材质排在流水线之前，PSO 描述就得二次改写，管线层被迫回头改自己的接口。
 *   故本域声明的依赖序是「几何 → 流水线接口 → 材质 → 实例化」，
 *   流水线的**接口**先于材质，流水线的**PSO 实例化**晚于材质。
 *   这条区分（接口序 vs 实例序）是本域排产不返工的关键，写在layerOrder 而非注释里。
 */
export type LayerId = "L0-geometry" | "L1-pipeline" | "L2-material" | "L3-instancing";

/** 层序号（用于拓扑排序与倒置检测；数值小者先完成）。 */
export const LAYER_RANK: Readonly<Record<LayerId, number>> = {
  "L0-geometry": 0,
  "L1-pipeline": 1,
  "L2-material": 2,
  "L3-instancing": 3,
};

/** 层的人类可读名（自检与文档渲染引用同一事实源）。 */
export const LAYER_LABEL: Readonly<Record<LayerId, string>> = {
  "L0-geometry": "几何数据层",
  "L1-pipeline": "顶点流水线层",
  "L2-material": "材质层",
  "L3-instancing": "场景级实例化层",
};

/**
 * 十个官方主题 id（封闭集：新增主题必须改本类型与THEME_REGISTRY 两处，
 * 只改一处由 selfCheckThemes 拦下——这就是「声明即契约」的机检形态）。
 */
export type ThemeId =
  | "mesh-format"
  | "vertex-pipeline"
  | "material-system"
  | "pbr-model"
  | "texture-sampling"
  | "mipmap"
  | "skinning"
  | "instancing"
  | "lod"
  | "culling";

/** 一个主题的完整规格（十主题逐项声明，无一项靠约定）。 */
export interface ThemeSpec {
  readonly id: ThemeId;
  /** 层归属。 */
  readonly layer: LayerId;
  /** 主题中文名（对外文档与错误提示引用）。 */
  readonly name: string;
  /** 本主题在本域内的功能号区间（含端点）。逐主题不重叠，由自检机检。 */
  readonly itemRange: { readonly lo: number; readonly hi: number };
  /** 职责边界一句话：这一主题负责什么、不负责什么（禁扩面写在句内）。 */
  readonly duty: string;
  /** 前置主题 id 列表（依赖驱动排产的输入；空数组表示无前置）。 */
  readonly requires: readonly ThemeId[];
  /** 本主题产出、供下游消费的数据契约名（契约名全局唯一，由自检机检）。 */
  readonly producesContract: string;
  /** 是否属于「几何数据」范畴（L0 的判据，供跨域边界检查引用）。 */
  readonly isGeometryData: boolean;
  /** 一句话说明：本主题与哪个域/卷交接，交接物是什么。 */
  readonly handoff: string;
}

/**
 * 十主题注册表。判据一要求逐主题声明层/区间/职责/前置/契约，此处即为该声明的
 * 唯一事实源——下游任何代码不得旁路硬编码主题名，一律经 lookupTheme 取规格。
 */
export const THEME_REGISTRY: Readonly<Record<ThemeId, ThemeSpec>> = {
  "mesh-format": {
    id: "mesh-format",
    layer: "L0-geometry",
    name: "网格格式",
    itemRange: { lo: 1601, hi: 1620 },
    duty: "顶点/索引缓冲、子网格、LOD 层级与材质引用的容器格式；不负责几何语义解释",
    requires: [],
    producesContract: "MeshContainer",
    isGeometryData: true,
    handoff: "向 Q 域（资产管线 F3201+）交付可打包的几何块；向 CGPU 卷声明网格遍历算力档位",
  },
  "vertex-pipeline": {
    id: "vertex-pipeline",
    layer: "L1-pipeline",
    name: "顶点流水线",
    itemRange: { lo: 1621, hi: 1640 },
    duty: "顶点变换、管线状态对象描述与批处理编排；不负责着色器源码与编译",
    requires: ["mesh-format"],
    producesContract: "PsoDescriptor",
    isGeometryData: false,
    handoff: "向 A 域（设备抽象）消费命令缓冲与围栏；向 C 域（着色器）消费编译产物",
  },
  "material-system": {
    id: "material-system",
    layer: "L2-material",
    name: "材质系统",
    itemRange: { lo: 1641, hi: 1660 },
    duty: "材质参数集、变体派生与着色素材组织；不负责纹理采样算法本身",
    requires: ["mesh-format"],
    producesContract: "MaterialSet",
    isGeometryData: false,
    handoff: "向 C 域（着色器）声明材质关键参数以派生 PSO 变体；向 D 域供材质预览图",
  },
  "pbr-model": {
    id: "pbr-model",
    layer: "L2-material",
    name: "PBR 模型",
    itemRange: { lo: 1661, hi: 1680 },
    duty: "金属粗糙度工作流与能量守恒的 BRDF 参数化；不负责具体光源（J 域）",
    requires: ["material-system"],
    producesContract: "PbrParams",
    isGeometryData: false,
    handoff: "向 J 域（光照与阴影）交付材质侧反射率与粗糙度，供其做能量分配",
  },
  "texture-sampling": {
    id: "texture-sampling",
    layer: "L2-material",
    name: "纹理采样",
    itemRange: { lo: 1681, hi: 1700 },
    duty: "采样器状态、采样模式与色彩空间解码；不负责纹理内容的mipmap 组织",
    requires: ["material-system"],
    producesContract: "SamplerState",
    isGeometryData: false,
    handoff: "向 A 域消费纹理句柄；向 I06（mipmap）交付已采样层级需求",
  },
  mipmap: {
    id: "mipmap",
    layer: "L2-material",
    name: "mipmap",
    itemRange: { lo: 1701, hi: 1720 },
    duty: "mip 链生成、层级选择与各向异性过滤；不负责纹理压缩格式（F 域）",
    requires: ["texture-sampling"],
    producesContract: "MipChain",
    isGeometryData: false,
    handoff: "向 CGPU 卷借多通道下采样算力；向 A 域申请 mip 层级显存",
  },
  skinning: {
    id: "skinning",
    layer: "L3-instancing",
    name: "蒙皮",
    itemRange: { lo: 1721, hi: 1740 },
    duty: "骨骼权重应用与形变矩阵流；不负责动画曲线本身（M 域）",
    requires: ["vertex-pipeline", "mesh-format"],
    producesContract: "SkinnedVertex",
    isGeometryData: false,
    handoff: "向 M 域（动画系统）消费骨骼层级与逐帧姿态；向 CGPU 卷声明顶点混合算力档位",
  },
  instancing: {
    id: "instancing",
    layer: "L3-instancing",
    name: "实例化",
    itemRange: { lo: 1741, hi: 1760 },
    duty: "同网格多实例的批量提交与实例缓冲编排；不负责内存分块（F1611 归几何格式）",
    requires: ["vertex-pipeline"],
    producesContract: "InstanceBatch",
    isGeometryData: false,
    handoff: "向 A 域消费实例缓冲；向 I09（LOD）交付实例级包围球供分档",
  },
  lod: {
    id: "lod",
    layer: "L3-instancing",
    name: "LOD",
    itemRange: { lo: 1761, hi: 1780 },
    duty: "LOD 链的分档选择与切换策略（含屏幕空间误差判据）；不负责简化算法（F1608）",
    requires: ["mesh-format", "vertex-pipeline"],
    producesContract: "LodSelection",
    isGeometryData: false,
    handoff: "向 I10（剔除）交付分档结果作为剔除粒度；向 U 域（性能治理）报帧预算占用",
  },
  culling: {
    id: "culling",
    layer: "L3-instancing",
    name: "剔除",
    itemRange: { lo: 1781, hi: 1800 },
    duty: "视锥/遮挡/小物件三级剔除与可见集产出；不负责遮挡图渲染（J 域）",
    requires: ["lod", "vertex-pipeline"],
    producesContract: "VisibleSet",
    isGeometryData: false,
    handoff: "向 A 域提交可见集；向 D 域（F0838 联动）交付 3D 渲染结果进 2D 合成图层",
  },
};

/** 注册表键全集（遍历用，避免手写清单与实际注册漂移）。 */
export const THEME_IDS: readonly ThemeId[] = Object.keys(THEME_REGISTRY) as ThemeId[];

/** 查主题规格；未注册返回显性失败——判据要求「十主题封闭集，域外主题不可引用」。 */
export function lookupTheme(id: string): Outcome<ThemeSpec> {
  const table = THEME_REGISTRY as Record<string, ThemeSpec | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "THEME_UNREGISTERED",
      `主题 ${id} 不在 I 域十主题注册表中`,
      `已注册主题：${THEME_IDS.join("、")}；若确为新增主题，须先在 THEME_REGISTRY 与 ThemeId 类型两处同时登记`,
    );
  }
  return ok(spec);
}

/**
 * 依赖序声明（判据二：分层 + 依赖序）。
 *
 * 本函数是「排产序」的唯一推导处：给定已登记主题的集合，按requires 关系做拓扑
 * 排序，返回可开工顺序。成环时返回显性失败并列出环上的主题——
 * 环= 开工阻塞（两个主题互为前置，谁都不能先动），不是可忽略的告警。
 *
 * 排序稳定性：同层同入度者按 itemRange.lo 升序，保证同输入必同输出
 * （排产序可被缓存与复核，序不稳定会让「昨天能开工今天不能」变成玄学）。
 */
export function resolveBuildOrder(themeIds: readonly ThemeId[]): Outcome<readonly ThemeId[]> {
  const bag = new DiagBag();

  // 1) 入参合法性：全部须已登记。未知 id 直接失败——不静默跳过（跳过会让
  //    调用方以为该主题已排进本轮，实际被丢掉，是最难查的一类静默）。
  const indegree = new Map<ThemeId, number>();
  const adjacency = new Map<ThemeId, ThemeId[]>();
  for (const id of themeIds) {
    const spec = lookupTheme(id);
    if (!spec.ok) {
      bag.pushAll(spec.diagnostics);
      return fail<readonly ThemeId[]>(spec.code, spec.message, spec.hint);
    }
    indegree.set(id, 0);
    adjacency.set(id, []);
  }

  // 2) 建边：requires 指向本集合内的主题才计边（指向集合外的主题视为已完工的前置，
  //    因为调用方只请求了本轮子集；若前置不在集合内且也未登记，才是真错误）。
  for (const id of themeIds) {
    const spec = THEME_REGISTRY[id];
    for (const dep of spec.requires) {
      const depSpec = lookupTheme(dep);
      if (!depSpec.ok) {
        bag.pushAll(depSpec.diagnostics);
        return fail<readonly ThemeId[]>(depSpec.code, depSpec.message, depSpec.hint);
      }
      if (!indegree.has(dep)) continue; // 集合外前置：本轮不排，视为已完工
      adjacency.get(dep)?.push(id);
      indegree.set(id, (indegree.get(id) ?? 0) + 1);
    }
  }

  // 3) Kahn 拓扑排序。就绪集按 itemRange.lo 升序出队（稳定序，见函数注释）。
  const ready: ThemeId[] = [];
  for (const id of themeIds) {
    if ((indegree.get(id) ?? 0) === 0) ready.push(id);
  }
  const sortReady = (): void => {
    ready.sort((a, b) => THEME_REGISTRY[a].itemRange.lo - THEME_REGISTRY[b].itemRange.lo);
  };
  sortReady();

  const order: ThemeId[] = [];
  const indeg = new Map(indegree);
  while (ready.length > 0) {
    const cur = ready.shift();
    if (cur === undefined) break;
    order.push(cur);
    for (const next of adjacency.get(cur) ?? []) {
      const left = (indeg.get(next) ?? 0) - 1;
      indeg.set(next, left);
      if (left === 0) {
        ready.push(next);
        sortReady();
      }
    }
  }

  // 4) 出队不完���即成环：列出残余入度 > 0 的主题，它们就是环上的节点。
  if (order.length !== themeIds.length) {
    const stuck = themeIds.filter((id) => (indeg.get(id) ?? 0) > 0);
    const bag2 = new DiagBag();
    bag2.push(
      "DEPENDENCY_CYCLE",
      `主题依赖成环，涉及：${stuck.join(" → ")}；这批主题互为前置，没有任何一个能先开工`,
      `断开环上的至少一条 requires 边（建议改在语义上真正次要的一侧），或把该批主题拆成两轮排产；`
        + `环的存在不是告警级别，是开工阻塞级别`,
    );
    bag2.pushAll(bag.all());
    return {
      ok: false,
      code: "DEPENDENCY_CYCLE",
      message: bag2.all()[0]?.message ?? "主题依赖成环",
      hint: bag2.all()[0]?.hint ?? "请断开环上的一条依赖边",
      diagnostics: bag2.all(),
    };
  }

  // 5) 层号倒置检查：若A 层主题排在 B 层主题之后（A.rank > B.rank），
  //    说明依赖声明与分层矛盾——分层是意图，requires 是实现，二者必须一致。
  for (let i = 0; i < order.length; i += 1) {
    for (let j = i + 1; j < order.length; j += 1) {
      const a = THEME_REGISTRY[order[i] as ThemeId];
      const b = THEME_REGISTRY[order[j] as ThemeId];
      if (LAYER_RANK[a.layer] > LAYER_RANK[b.layer]) {
        bag.push(
          "LAYER_ORDER_VIOLATION",
          `层号倒置：${a.name}（${a.layer}）被排在 ${b.name}（${b.layer}）之后，`
            + `与「${LAYER_LABEL[a.layer]}先于 ${LAYER_LABEL[b.layer]}」的分层声明矛盾`,
          `检查二者之间的 requires 边：若确实存在跨层反向依赖，说明分层声明或依赖声明其一有误；`
            + `修正方向是把依赖关系改为同向（低层依赖高层），而非放宽本检查`,
        );
      }
    }
  }

  return ok(order, bag.all());
}

/** 域级依赖序声明表（对外文档与排期工具读取的机器可读形式）。 */
export interface DomainLayerPlan {
  readonly layer: LayerId;
  readonly label: string;
  readonly rank: number;
  readonly themes: readonly ThemeId[];
  /** 该层完成后向下一层交付的门禁判据（一句话，可机检即由 CI 断言）。 */
  readonly exitCriterion: string;
}

/** 全域分层推进计划（L0 → L1 → L2 → L3，逐层门禁）。 */
export const LAYER_PLAN: readonly DomainLayerPlan[] = [
  {
    layer: "L0-geometry",
    label: LAYER_LABEL["L0-geometry"],
    rank: 0,
    themes: ["mesh-format"],
    exitCriterion: "网格容器可无损往返（vmesh↔glTF），属性布局校验三查全绿，量化误差有界",
  },
  {
    layer: "L1-pipeline",
    label: LAYER_LABEL["L1-pipeline"],
    rank: 1,
    themes: ["vertex-pipeline"],
    exitCriterion: "PSO 描述可由材质参数确定性派生，批处理不破坏顶点重排的正确性断言",
  },
  {
    layer: "L2-material",
    label: LAYER_LABEL["L2-material"],
    rank: 2,
    themes: ["material-system", "pbr-model", "texture-sampling", "mipmap"],
    exitCriterion: "PBR 参数满足能量守恒约束，mip 链层级选择在缩放 extremes 下无闪烁",
  },
  {
    layer: "L3-instancing",
    label: LAYER_LABEL["L3-instancing"],
    rank: 3,
    themes: ["skinning", "instancing", "lod", "culling"],
    exitCriterion: "三级剔除的可见集与暴力遍历逐实例一致，切换 LOD 无跳变（误差有界声明）",
  },
];

/** 按层取主题（排期与自检用；层不存在返回空数组而非抛错）。 */
export function themesInLayer(layer: LayerId): readonly ThemeId[] {
  return LAYER_PLAN.filter((p) => p.layer === layer).flatMap((p) => p.themes);
}

// ════════════════════════════════════════════════════════════════════════════
// §3 跨域边界（判据三：跨域边界 —— 边界即禁扩面）
// ════════════════════════════════════════════════════════════════════════════

/** 与 I 域有交接关系的域标识。CGPU 是姊妹卷（算力卷），以卷标识参与。 */
export type PeerDomain = "VE-A" | "VE-C" | "VE-D" | "VE-Q" | "CGPU";

/** 一条跨域边界契约：我方职责 / 对方职责 / 交接物 / 越权形态。 */
export interface BoundarySpec {
  readonly peer: PeerDomain;
  /**
   * 对方的人话短名（A / C / D / Q / CGPU）。
   * 单列此字段的原因：handoff 文案写「A 域」「CGPU 卷」，而 peer id 写 VE-A / CGPU，
   * 两者字面不同——覆盖度审计必须按短名匹配，否则会把全部主题误判为「未挂接」。
   */
  readonly shortName: string;
  /** 对方域/卷的职责（一句话，引用其册内锚点）。 */
  readonly peerDuty: string;
  /** 我方从对方消费的东西（消费方向单向声明，禁止双向含糊）。 */
  readonly weConsume: readonly string[];
  /** 我方向对方交付的东西。 */
  readonly weDeliver: readonly string[];
  /** 本域**不**做的事（禁扩面清单——越权即BOUNDARY_VIOLATION）。 */
  readonly weMustNot: readonly string[];
  /** 交接契约名（双方共用的数据结构名，改名=破坏性变更，须走 ADR）。 */
  readonly contract: string;
  /** 联动锚点（册内编号，便于追溯该边界的来历）。 */
  readonly anchor: string;
}

/**
 * 跨域边界表。四条边界逐条声明我方与对方的职责分界，交接物具体到契约名——
 * 「模糊地说个『协同』」不构成边界，只有「谁产出哪个字段、谁消费」才是。
 */
export const BOUNDARY_TABLE: readonly BoundarySpec[] = [
  {
    peer: "VE-A",
    shortName: "A",
    peerDuty: "设备探测与仲裁、上下文生命周期、围栏同步、命令缓冲与显存预算（A 域 F0001-F0200）",
    weConsume: ["设备能力集（纹理格式/顶点格式/MSAA 档位）", "命令缓冲与围栏句柄", "显存预算水位"],
    weDeliver: ["显存占用预测（按网格字节流与 LOD 链估算）"],
    weMustNot: [
      "探测或枚举设备",
      "创建/销毁设备上下文",
      "自行提交命令到硬件队列",
      "绕过围栏做跨队列同步",
    ],
    contract: "DeviceCapabilitySet",
    anchor: "VE-A 域 F0001 设备探测仲裁",
  },
  {
    peer: "VE-C",
    shortName: "C",
    peerDuty: "VE-Shade DSL 与着色器编译全链：词法/语法/语义/IR/优化/后端转译（C 域 F0401-F0600）",
    weConsume: ["编译产物（顶点/像素着色器二进制）", "PSO 变体派生规则", "热编译事件"],
    weDeliver: ["材质关键参数集（决定变体数量）", "顶点属性布局声明（决定输入签名）"],
    weMustNot: ["解析或生成着色器源码", "调用后端编译器", "缓存着色器二进制到自有格式"],
    contract: "ShaderModuleRef",
    anchor: "VE-C 域 F0401 DSL 词法",
  },
  {
    peer: "VE-D",
    shortName: "D",
    peerDuty: "2D 合成引擎：图层树、混合模式 24 种、脏区三档、九宫格与九种滤镜（D 域 F0601-F0800）",
    weConsume: ["合成树节点插入位次与变换", "2D 侧的可见性与裁剪需求"],
    weDeliver: ["3D 渲染结果图层（含深度序与材质引用）", "每帧可见集统计（供 D 侧图层预算）"],
    weMustNot: ["在2D 合成树内部插入私有节点类型", "绕过合成树直接改屏幕", "自建第二套 2D 混合模式"],
    contract: "CompositionLayer3D",
    anchor: "VE-F0838 与 D 域 2D 合成联动",
  },
  {
    peer: "VE-Q",
    shortName: "Q",
    peerDuty: "资产管线：图集、LOD 生成、纹理压缩、流式加载、异步上传、打包格式（Q 域 F3201-F3400）",
    weConsume: ["资产包与块级加载事件", "纹理与几何的磁盘表示", "版本与哈希"],
    weDeliver: ["网格容器 schema 与校验规则", "块级 LOD/可见性元数据的读取约定"],
    weMustNot: ["自行实现磁盘分页与缓存淘汰", "定义与资产包冲突的私有打包格式", "在渲染线程做阻塞式磁盘读"],
    contract: "AssetBlockRef",
    anchor: "VE-F1611 网格流式容器 / Q 域 F3201+",
  },
  {
    peer: "CGPU",
    shortName: "CGPU",
    peerDuty: "CGPU 卷：通用计算管线与 SIMD 算力调度，含图序列化格式（CGPU-F0258）与向量化内核分派",
    weConsume: ["SIMD 算力档位（AVX-512/AVX2/Q15Q31 定点/标量回退）", "网格遍历与顶点混合的内核执行"],
    weDeliver: ["网格处理算力标签（工作量规模/数据局部性/容差要求）", "对拍基准（VE 输出 vs CGPU 输出）"],
    weMustNot: [
      "自行实现 AVX  intrinsics 分派（会与 CGPU 卷产生两份实现且必然漂移）",
      "假设某档位必然可用而不做降级",
      "绕过 CGPU 卷直接操作 SIMD 寄存器",
    ],
    contract: "ComputeWorkloadLabel",
    anchor: "CGPU-F0258 图序列化格式 / 跨卷算力标签",
  },
];

/**
 * 越权检查：给定一个待执行操作描述，判断它是否越过本域边界。
 *
 * 这不是纸面约定而是**开发期闸门**：I 域任一模块在实现某能力前，须先过本函数。
 * 越权不抛异常而是返回显性失败——因为越权在开发期是「设计错误」，
 * 抛异常会让人习惯性 catch 后忽略，返回失败则必须被看见。
 *
 * 参数类型刻意放宽到 string：调用方传进来的对接方标识来自配置/调用现场，
 * 未必受 PeerDomain 联合类型约束，而「传了个没登记的对接方」恰恰是本函数
 * 必须显性拦下的情形——若用窄类型，编译器会替我们把该情形挡在门外，
 * 那道边界闸门就永远不会被触发。
 */
export function checkBoundary(
  peer: PeerDomain | string,
  operation: string,
  bag: DiagBag,
): Outcome<BoundarySpec> {
  const spec = BOUNDARY_TABLE.find((b) => b.peer === peer);
  if (spec === undefined) {
    bag.push(
      "BOUNDARY_UNDECLARED",
      `域/卷 ${peer} 与 I 域之间没有声明任何边界契约，无法判定该操作是否越权`,
      `已知边界对：${BOUNDARY_TABLE.map((b) => b.peer).join("、")}；新增对接方须先在 BOUNDARY_TABLE 登记`,
    );
    return fail("BOUNDARY_UNDECLARED", `域/卷 ${peer} 未声明边界契约`, "请先在 BOUNDARY_TABLE 登记该对接方");
  }

  const op = operation.toLowerCase();
  let hit: string | undefined;
  for (const forbidden of spec.weMustNot) {
    if (matchesForbidden(op, forbidden.toLowerCase())) {
      hit = forbidden;
      break;
    }
  }
  if (hit !== undefined) {
    bag.push(
      "BOUNDARY_VIOLATION",
      `越权：I 域尝试执行「${operation}」，命中 ${peer} 边界的禁扩面条款「${hit}」`,
      `${peer} 域职责是「${spec.peerDuty}」；本域如确需该能力，走${spec.contract} 契约向 ${peer} 申请，`
        + `不要在本域内重做一遍（两处实现必然漂移）`,
    );
    return fail("BOUNDARY_VIOLATION", `操作「${operation}」越过 ${peer} 边界`, `改走${spec.contract} 契约向 ${peer} 申请该能力`);
  }

  return ok(spec, bag.all());
}

/**
 * 禁扩面条款匹配。
 *
 * 为什么分两类 token 而不用统一滑窗（本函数第一版就是这么写的，实测误判率高，
 * 故记录此处的设计理由）：
 *   · 拉丁词（intrinsics、AVX、warp）——**高辨识度**，命中即强证据。只要条款里
 *     存在拉丁词，要求这些词全部出现在操作描述里才判越权（缺一个都不算，
 *     因为条款里的拉丁词往往是同一条禁令的多个必要限定）。
 *   · CJK 词——**低辨识度**，单字/双字组撞车概率高（"分派"、"实现" 到处都有）。
 *     故要求存在一段**连续命中长度 ≥ 3 个双字组**（即约 4 个连续汉字的短语），
 *     单点命中不算。
 * 两类token 都不命中才放行。这个组合让误报与漏报同时压到可接受：
 * 漏报代价（本域偷偷重做一遍别人负责的能力）远高于误报，故阈值偏向宁可多拦。
 */
function matchesForbidden(op: string, forbidden: string): boolean {
  const latin = (forbidden.match(/[a-z][a-z0-9-]*/g) ?? []).filter((t) => t.length >= 3);
  const cjk = forbidden.replace(/^[\x20-\x7e]+$/, "").match(/[^\x00-\x7f]/g) ?? [];

  // 规则一：条款含拉丁词时，必须全部命中。
  if (latin.length > 0 && latin.every((t) => op.includes(t))) return true;

  // 规则二：CJK 连续短语命中 ≥3 个双字组（约 4 汉字连续出现）。
  const grams: string[] = [];
  for (let i = 0; i + 2 <= cjk.length; i += 1) {
    const gram = `${cjk[i] ?? ""}${cjk[i + 1] ?? ""}`;
    if (gram.length === 2) grams.push(gram);
  }
  let run = 0;
  let bestRun = 0;
  for (const g of grams) {
    if (op.includes(g)) {
      run += 1;
      if (run > bestRun) bestRun = run;
    } else {
      run = 0;
    }
  }
  return bestRun >= 3;
}

/** 边界覆盖审计结果（把「挂接了已声明边界」与「交接给了未声明方」分开报告）。 */
export interface BoundaryCoverage {
  /** 主题数。 */
  readonly themeCount: number;
  /** handoff 中点名了已声明边界对（含短名与契约名）的主题 id。 */
  readonly linked: readonly ThemeId[];
  /** handoff 中出现「X 域/X 卷」但该方未在 BOUNDARY_TABLE 登记的主题 id。 */
  readonly toUndeclaredPeer: readonly ThemeId[];
  /** handoff 连一个对手方都没点名的主题 id（真正的孤岛，必为空）。 */
  readonly orphan: readonly ThemeId[];
}

/**
 * 边界对账：逐主题核对其 handoff 指向了谁，并分三类报告。
 *
 * 「交接给了未登记的域」（如 PBR 模型把参数交给 J 域光照）与「孤岛主题」
 * 性质不同：前者是正常的跨域协作尚未在本条登记（真实缺口，需在对应主题开工时
 * 补边界条款），后者是主题不知道自己服务谁（登记遗漏）。故分列，不混为一谈。
 *
 * 匹配用「域短名 + 契约名」双通道：handoff 文案写「A 域」「CGPU 卷」等人话，
 * 而peer id 写 VE-A / CGPU，字面不同——只按 id 匹配会全量误报（第一版的bug）。
 */
export function auditBoundaryCoverage(bag: DiagBag): Outcome<BoundaryCoverage> {
  const linked: ThemeId[] = [];
  const toUndeclared: ThemeId[] = [];
  const orphan: ThemeId[] = [];

  for (const id of THEME_IDS) {
    const handoff = THEME_REGISTRY[id].handoff;
    const declaredHit = BOUNDARY_TABLE.some(
      (b) => handoff.includes(b.peer) || handoff.includes(b.contract) || handoff.includes(`${b.shortName} 域`) || handoff.includes(`${b.shortName} 卷`),
    );
    if (declaredHit) {
      linked.push(id);
      continue;
    }
    // 未命中已声明边界，则看handoff 是否至少点了某个对手方（X 域/X 卷/CGPU）。
    const namesPeer = /[A-Z]{1,3}\s*(域|卷)/.test(handoff) || handoff.includes("CGPU");
    if (namesPeer) toUndeclared.push(id);
    else orphan.push(id);
  }

  if (orphan.length > 0) {
    bag.push(
      "BOUNDARY_UNDECLARED",
      `以下主题的 handoff 未点名任何对手方（既非已声明边界对，也非未登记的域）：${orphan.join("、")}`,
      `每个主题都须回答「我向谁交付/我消费谁的什么」，否则接入时会出现无人认领的交接面；`
        + `若该主题确为域内闭环，请在 handoff 末尾注明「域内闭环」`,
    );
  }
  if (toUndeclared.length > 0) {
    bag.push(
      "BOUNDARY_UNDECLARED",
      `以下主题交接给了尚未在 BOUNDARY_TABLE 登记的对手方：${toUndeclared.join("、")}`,
      `这不是错误（跨域协作本身合法），但边界表存在缺口：请在对应主题开工时补齐该边界的`
        + `消费方/交付方/禁扩面三要素，否则日后该交接面无人负责`,
    );
  }

  return ok(
    { themeCount: THEME_IDS.length, linked, toUndeclaredPeer: toUndeclared, orphan },
    bag.all(),
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §4 CGPU 算力声明（判据四：算力声明 —— 跨卷算力标签）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 网格处理算力档位（与 CGPU 卷的档位命名保持一致，便于跨卷对账）。
 *
 * 档位语义（为何要有四档而不是「用 AVX-512 或不用」二选一）：
 *   · 标量回退 —— 无SIMD 的兜底路径，必须永远存在，否则老机器直接黑屏。
 *   · Q15Q31 定点 —— 定点数范围可预测、无舍入误差累积，适合需要跨帧位级一致的
 *     计算（如确定性量化的校验值、F1619 的字节级一致断言）。代价是动态范围减半。
 *   · AVX2 —— 通用主力档，256 位四通道浮点，覆盖绝大多数几何运算。
 *   · AVX-512 —— 512 位八通道，仅在几何规模足够大时才有收益（小网格上
 *     寄存器压力反而拖慢），故本条要求调用方同时声明规模下界。
 *
 * ⚠ 档位不是单轴强弱序（本条第一版的建模错误，已修，见下方注释）：
 *   标量回退 / AVX2 / AVX-512 构成一条**向量宽度轴**（1 → 4 → 8 通道）；
 *   Q15Q31 定点则属于另一条**数值域轴**（定点 vs 浮点）——任何宽度的 SIMD 硬件
 *   都能做定点运算，它不是「比 AVX2 弱的一档」。
 *   早先把四档排成一条强弱线（scalar < q15q31 < avx2 < avx512）会导致
 *   「设备支持 AVX2 但不支持定点」被误判为「档位充足」，而实际上定点路径需要
 *   显式声明宿主是否提供该数值域（CGPU 卷的分派器负责此能力）。
 *   现改为二维模型：`VECTOR_WIDTH` 记宽度、`numericDomain` 记数值域，
 *   档位不足的判定必须**两个轴同时比较**。
 *
 * 本域**只声明需要哪一档**，不实现分派。分派归 CGPU 卷（见 BOUNDARY_TABLE 的 CGPU 行）。
 */
export type SimdTier = "scalar-fallback" | "q15q31-fixed" | "avx2-f32x4" | "avx512-f32x8";

/** 数值域：定点还是浮点。与向量宽度正交——这是本条档位建模的核心区分。 */
export type NumericDomain = "fixed-point" | "float" | "scalar";

/** 各档位的向量宽度（通道数）。 */
export const TIER_LANES: Readonly<Record<SimdTier, number>> = {
  "scalar-fallback": 1,
  "q15q31-fixed": 4,
  "avx2-f32x4": 4,
  "avx512-f32x8": 8,
};

/** 各档位的数值域。宽度轴与数值域轴正交，档位不足须两轴同查。 */
export const TIER_DOMAIN: Readonly<Record<SimdTier, NumericDomain>> = {
  "scalar-fallback": "scalar",
  "q15q31-fixed": "fixed-point",
  "avx2-f32x4": "float",
  "avx512-f32x8": "float",
};

/**
 * 档位强度序（**仅表示向量宽度序，不含数值域**）。
 * 保留此表是为了让「档位强弱」在浮点档之间可比；跨数值域比较一律禁止——
 * 需要的判定见 tierSatisfies。
 */
export const TIER_RANK: Readonly<Record<SimdTier, number>> = {
  "scalar-fallback": 0,
  "q15q31-fixed": 2,
  "avx2-f32x4": 2,
  "avx512-f32x8": 3,
};

/** 档位的人话描述（诊断提示引用，避免调用方自己拼）。 */
export const TIER_LABEL: Readonly<Record<SimdTier, string>> = {
  "scalar-fallback": "标量回退（无 SIMD 时的唯一可用路径）",
  "q15q31-fixed": "Q15Q31 定点（要求跨帧位级一致时使用）",
  "avx2-f32x4": "AVX2 四通道浮点（通用主力档）",
  "avx512-f32x8": "AVX-512 八通道浮点（需规模下界支撑）",
};

/**
 * 宿主设备能力（由 A 域设备探测交付，本域只读不写）。
 * 两个字段分别对应两条轴：向量宽度上限 + 可用数值域集合。
 */
export interface HostSimdCapability {
  /** 可用的最高向量宽度档（宽度轴）。 */
  readonly maxWidthTier: SimdTier;
  /** 宿主显式提供的数值域（数值域轴）。定点不在其中则定点路径不可用。 */
  readonly numericDomains: readonly NumericDomain[];
}

/** 一个算力标签：某主题的某类几何运算需要什么算力。 */
export interface ComputeLabel {
  /** 标签 id（跨卷对账的主键）。 */
  readonly labelId: string;
  /** 消费该算力的主题。 */
  readonly theme: ThemeId;
  /** 运算的人话描述。 */
  readonly operation: string;
  /** 需求档位。 */
  readonly requiredTier: SimdTier;
  /**
   * 规模下界（元素数）：低于此值时高��位无收益甚至反噬（寄存器压力 > 并行收益）。
   * 0 表示无规模门槛（如顶点变换这类天然批量极大的运算）。
   */
  readonly minElementsForTier: number;
  /** 是否允许降档运行（false 表示必须原档，否则结果不可信）。 */
  readonly degradable: boolean;
  /** 跨卷对账锚点：本标签对应 CGPU 卷的哪类内核。 */
  readonly cgpuAnchor: string;
}

/**
 * 算力标签表。逐条声明「哪个主题的哪种运算需要哪一档算力」——
 * 这是本域向 CGPU 卷提需求的**唯一接口**，也是对方排产的输入。
 *
 * 声明纪律（反直觉处，写明理由）：
 *   不是每个几何运算都该要最高档。QEM 简化（F1608）、法线加权平均（F1606）这类
 *   带容差判定的运算，位级差异会被判定逻辑吸收，用 AVX2 就够；只有
 *   「量化误差上界声明」「确定性断言」这类需要逐位可复现的运算才值得上定点档。
 *   把所有运算都标AVX-512 会让 CGPU 卷无法排产（预算按最坏情况分配），
 *   也会让本域在低配机上过早触发降档。
 */
export const COMPUTE_LABELS: readonly ComputeLabel[] = [
  {
    labelId: "mesh-format/quantize-position",
    theme: "mesh-format",
    operation: "顶点位置量化（fp32 → 16bit 局部坐标）",
    requiredTier: "q15q31-fixed",
    minElementsForTier: 0,
    degradable: false,
    cgpuAnchor: "CGPU 定点分派（Q15Q31 模式）",
  },
  {
    labelId: "mesh-format/oct-normal-encode",
    theme: "mesh-format",
    operation: "法线八面体编码（法线 → 2D 八面体投影）",
    requiredTier: "avx2-f32x4",
    minElementsForTier: 4096,
    degradable: false,
    cgpuAnchor: "CGPU 几何遍历内核",
  },
  {
    labelId: "mesh-format/qem-simplify",
    theme: "mesh-format",
    operation: "QEM 边收缩的代价矩阵求解（每轮求4x4 二次误差矩阵的最小特征向量）",
    requiredTier: "avx2-f32x4",
    minElementsForTier: 16384,
    degradable: true,
    cgpuAnchor: "CGPU 线性代数内核",
  },
  {
    labelId: "mesh-format/forsyth-reorder",
    theme: "mesh-format",
    operation: "Forsyth 顶点重排的模拟缓存命中率扫描",
    requiredTier: "scalar-fallback",
    minElementsForTier: 0,
    degradable: false,
    cgpuAnchor: "CGPU 标量回退路径",
  },
  {
    labelId: "vertex-pipeline/vertex-transform",
    theme: "vertex-pipeline",
    operation: "顶点变换（模型矩阵 × 顶点，w 除法与透视除）",
    requiredTier: "avx512-f32x8",
    minElementsForTier: 65536,
    degradable: true,
    cgpuAnchor: "CGPU SIMD 三级分派",
  },
  {
    labelId: "mipmap/box-downsample",
    theme: "mipmap",
    operation: "mip 链盒式下采样（2×2 四通道平均）",
    requiredTier: "avx512-f32x8",
    minElementsForTier: 262144,
    degradable: true,
    cgpuAnchor: "CGPU SIMD 三级分派",
  },
  {
    labelId: "skinning/skinning-blend",
    theme: "skinning",
    operation: "四骨骼权重顶点混合（权重乘加链）",
    requiredTier: "avx2-f32x4",
    minElementsForTier: 32768,
    degradable: true,
    cgpuAnchor: "CGPU 顶点混合内核",
  },
  {
    labelId: "culling/frustum-test",
    theme: "culling",
    operation: "视锥剔除的包围球批量判定",
    requiredTier: "avx2-f32x4",
    minElementsForTier: 8192,
    degradable: true,
    cgpuAnchor: "CGPU 几何遍历内核",
  },
  {
    labelId: "culling/occlusion-sample",
    theme: "culling",
    operation: "遮挡查询的层次矩形采样",
    requiredTier: "scalar-fallback",
    minElementsForTier: 0,
    degradable: false,
    cgpuAnchor: "CGPU 标量回退路径",
  },
];

/** 标签 id 全集（对账与去重用，避免手写清单漂移）。 */
export const COMPUTE_LABEL_IDS: readonly string[] = COMPUTE_LABELS.map((l) => l.labelId);

/**
 * 档位满足性判定（两轴同查，见 SimdTier 处的建模说明）。
 *   · 数值域轴：宿主未提供该档所需数值域 → 直接不满足（定点运算不能在纯浮点分派器上跑）。
 *   · 宽度轴：宿主最大宽度 < 需求宽度 → 不满足。
 * 两轴都满足才算「档位充足」，从而原档执行。
 */
export function tierSatisfies(label: ComputeLabel, host: HostSimdCapability): boolean {
  const needDomain = TIER_DOMAIN[label.requiredTier];
  const domainOk =
    needDomain === "scalar" || host.numericDomains.includes(needDomain);
  const widthOk =
    needDomain === "scalar" || TIER_LANES[host.maxWidthTier] >= TIER_LANES[label.requiredTier];
  return domainOk && widthOk;
}

/**
 * 算力需求仲裁：给定宿主实际能力，算出每个标签的最终执行档位。
 *
 * 仲裁四条路径，**顺序即优先级**（本条第一版把规模门控与宽度比较混在一起算，
 * 导致「规模不足」被误报成「宿主能力不足」，自检抓到后重排；顺序不可调换）：
 *   1. 数值域缺失 → 硬阻断。数值域不匹配无法靠降宽度弥补，定点运算走浮点分派器
 *      会破坏 F1619 的字节级一致断言——「能跑但结果不对」比「跑不了」危险得多。
 *   2. 规模门控 → 主动降一档宽度，**不视为能力不足、不阻断**，只发告警说明这是
 *      预期行为。规模不够时升档只增加寄存器压力，净亏损。
 *   3. 宽度充足 → 原档执行（不多占算力，也不损质量）。
 *   4. 宽度不足 → 可降档则降档并给质量影响提示；不可降档则阻断。
 */
export interface TierResolution {
  readonly labelId: string;
  readonly requested: SimdTier;
  readonly resolved: SimdTier;
  /** 相对请求档是否发生降档（含规模门控导致的降档）。 */
  readonly downgraded: boolean;
  /** 不可降档、数值域不匹配时为 true（调用方必须中止该标签的执行）。 */
  readonly blocked: boolean;
  /** 降档或阻塞的显性原因（无则为空串）。 */
  readonly reason: string;
}

export function resolveTier(
  label: ComputeLabel,
  host: HostSimdCapability,
  elementCount: number,
  bag: DiagBag,
): TierResolution {
  const requested = label.requiredTier;
  const needDomain = TIER_DOMAIN[requested];

  // 路径一：数值域缺失 —— 唯一无降档路径可走的硬阻断。
  if (needDomain !== "scalar" && !host.numericDomains.includes(needDomain)) {
    const reason = `宿主未提供 ${needDomain} 数值域（仅提供 ${host.numericDomains.join("、")}）`;
    bag.push(
      "SIMD_TIER_INSUFFICIENT",
      `${label.operation}：需要 ${TIER_LABEL[requested]}，但 ${reason}`,
      `该标签标记为不可降档——数值域不匹配无法靠降宽度档位弥补（定点运算走浮点分派器会破坏`
        + `位级可复现性，令 F1619 的字节级一致断言失效）；请让调用方中止该标签并上报设备能力不足，`
        + `而不是绕过数值域检查改走浮点`,
    );
    return {
      labelId: label.labelId,
      requested,
      resolved: "scalar-fallback",
      downgraded: true,
      blocked: true,
      reason,
    };
  }

  // 路径二：规模门控（规模不足优先于能力比较——它不是宿主的问题）。
  if (elementCount < label.minElementsForTier) {
    const target = stepDownWidth(requested);
    const resolved = TIER_LANES[target] <= TIER_LANES[host.maxWidthTier] ? target : host.maxWidthTier;
    bag.push(
      "SIMD_TIER_INSUFFICIENT",
      `${label.operation}：本次规模 ${elementCount} 低于 ${TIER_LABEL[requested]} 的收益门槛 `
        + `${label.minElementsForTier}，已按 ${TIER_LABEL[resolved]} 执行`,
      `这是预期行为而非能力降级；若该运算在低规模下成为帧时间热点，说明需要改算法（换更省的遍历方式）`
        + `而非升档——规模不够时升档只会增加寄存器压力`,
    );
    return {
      labelId: label.labelId,
      requested,
      resolved,
      downgraded: true,
      blocked: false,
      reason: "规模门控降档",
    };
  }

  // 路径三：宽度充足 → 原档执行。
  if (TIER_LANES[host.maxWidthTier] >= TIER_LANES[requested]) {
    return {
      labelId: label.labelId,
      requested,
      resolved: requested,
      downgraded: false,
      blocked: false,
      reason: "",
    };
  }

  // 路径四a：宽度不足且可降档 → 降档执行 + 质量影响提示。
  if (label.degradable) {
    const reason = `宿主宽度档 ${TIER_LANES[host.maxWidthTier]} 通道低于需求 ${TIER_LANES[requested]} 通道`;
    bag.push(
      "SIMD_TIER_INSUFFICIENT",
      `${label.operation}：${reason}，已由 ${TIER_LABEL[requested]} 降至 ${TIER_LABEL[host.maxWidthTier]}`,
      `质量影响：同数值域内降宽度档只改变吞吐与求和次序，不改变数值域；末位差异若被下游判定逻辑`
        + `（容差/误差上界）吸收则无可见影响，但**确定性断言场景必须改用不可降档标签或标量档**；`
        + `若该运算在大网格上成为瓶颈，应在能力探测阶段把 ${TIER_LABEL[requested]} 列为最低要求`,
    );
    return {
      labelId: label.labelId,
      requested,
      resolved: host.maxWidthTier,
      downgraded: true,
      blocked: false,
      reason,
    };
  }

  // 路径四b：宽度不足且不可降档 → 阻断。
  const reason = `宽度不足（宿主 ${TIER_LANES[host.maxWidthTier]} < 需求 ${TIER_LANES[requested]}）且标签不可降档`;
  bag.push(
    "SIMD_TIER_INSUFFICIENT",
    `${label.operation}：${reason}`,
    `该运算的结果正确性依赖该档位（量化误差上界/字节级一致），用低档执行会产出`
      + `「看起来正常但结果不可复现」的输出；请让调用方中止该标签并上报设备能力不足，`
      + `而不是绕过降档检查`,
  );
  return {
    labelId: label.labelId,
    requested,
    resolved: host.maxWidthTier,
    downgraded: true,
    blocked: true,
    reason,
  };
}

/** 宽度轴下档一级（标量回退是地板）。 */
function stepDownWidth(tier: SimdTier): SimdTier {
  if (tier === "avx512-f32x8") return "avx2-f32x4";
  return "scalar-fallback";
}

/** 算力标签字段校验（判据四的自检前置：标签先自洽，才谈得上向CGPU 提需求）。 */
export function validateComputeLabels(bag: DiagBag): Outcome<readonly ComputeLabel[]> {
  for (const label of COMPUTE_LABELS) {
    const theme = lookupTheme(label.theme);
    if (!theme.ok) {
      bag.pushAll(theme.diagnostics);
      continue;
    }
    if (TIER_LANES[label.requiredTier] < 1 || !Number.isFinite(label.minElementsForTier)) {
      bag.push(
        "SIMD_LABEL_MALFORMED",
        `算力标签 ${label.labelId} 字段非法（档位 ${label.requiredTier} 的 lanes=${TIER_LANES[label.requiredTier]}，`
          + `规模下界 ${label.minElementsForTier}）`,
        "档位须取TIER_LANES 已定义的四档之一，规模下界须为非负有限数（0 表示无门槛）",
      );
    }
    if (label.minElementsForTier < 0) {
      bag.push(
        "SIMD_LABEL_MALFORMED",
        `算力标签 ${label.labelId} 的规模下界为负数（${label.minElementsForTier}）`,
        "规模下界是「低于此值不划算」的阈值，语义上不可能为负；请改为 0（无门槛）或正整数",
      );
    }
    if (label.cgpuAnchor.trim() === "") {
      bag.push(
        "SIMD_LABEL_MALFORMED",
        `算力标签 ${label.labelId} 未声明 CGPU 对账锚点`,
        "跨卷协作要求每个需求都能追到对方的具体内核；请在 cgpuAnchor 写明对应的 CGPU 内核类别",
      );
    }
  }
  return ok(COMPUTE_LABELS, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 开工框架（域级排产与验收契约：本条的落地面）
// ════════════════════════════════════════════════════════════════════════════

/** 域身份常量（本域所有对外文本引用同一事实源，不各自硬编码）。 */
export const DOMAIN = {
  /** 域编号。 */
  id: "VE-I",
  /** 域名。 */
  name: "3D 管线",
  /** 功能号区间。 */
  itemLo: 1601,
  itemHi: 1800,
  /** 本域功能总数（1800 - 1601 + 1 = 200，与册内「32 域 × 200 项」一致）。 */
  itemCount: 200,
  /** 所属波次（VE 卷首丙表：W3 = 3D 管线）。 */
  wave: "W3",
  /** 本域开工条的功能号。 */
  kickoffItemId: 1601,
} as const;

/** 官方十主题的中文名序列（判据一的对外表述，与注册表顺序一致）。 */
export const OFFICIAL_THEMES: readonly string[] = [
  "网格格式",
  "顶点流水线",
  "材质系统",
  "PBR 模型",
  "纹理采样",
  "mipmap",
  "蒙皮",
  "实例化",
  "LOD",
  "剔除",
];

/**
 * 开工门禁（kickoff gate）：本域开工前必须逐条成立的条件。
 *
 * 门禁存在的理由：I 域横跨面最宽，若开工时边界未定清，后面每加一项功能
 * 都要重新讨论「这个该谁做」。故把开工前置条件写成机检项——不靠会议纪要，
 * 靠代码断言。任一条不成立即判开工未完成，缺口写在 detail 里。
 */
export interface GateCriterion {
  readonly id: string;
  readonly criterion: string;
  /** 该门禁对应的判据（映射到五条判据之一）。 */
  readonly mapsTo: "十主题" | "分层" | "跨域边界" | "算力声明" | "判据";
}

/** 开工门禁清单（逐条可机检）。 */
export const KICKOFF_GATES: readonly GateCriterion[] = [
  { id: "G1", criterion: "官方十主题全部登记入注册表且字段自洽（区间不重叠、契约名唯一）", mapsTo: "十主题" },
  { id: "G2", criterion: "四层分层推进计划完整，每层有可机检的出口判据", mapsTo: "分层" },
  { id: "G3", criterion: "依赖序可拓扑排序且无环，无层号倒置", mapsTo: "分层" },
  { id: "G4", criterion: "五条跨域边界（A/C/D/Q/CGPU）逐条声明消费方、交付方与禁扩面", mapsTo: "跨域边界" },
  { id: "G5", criterion: "网格处理算力以跨卷标签形式声明，档位需求与 CGPU 卷可对账", mapsTo: "算力声明" },
  { id: "G6", criterion: "零静默：所有拒绝/越权/降档均产出三要素诊断，无静默分支", mapsTo: "判据" },
];

/** 一个主题的开工就绪度（供排产看板消费；本条只算不算，不派工）。 */
export interface ThemeReadiness {
  readonly theme: ThemeId;
  readonly layer: LayerId;
  /** 依赖是否已全部就绪（requires 全部排在它之前，或已在层内更早收口）。 */
  readonly depsReady: boolean;
  /** 阻塞原因（非空即 depsReady 为 false 的显性依据，不留「未知」态）。 */
  readonly blockedBy: readonly ThemeId[];
  /** 是否允许开工（depsReady 且自身规格自洽）。 */
  readonly canStart: boolean;
  /** 关联的算力标签数（供资源排产参考：算力需求大的主题宜错峰）。 */
  readonly computeLabelCount: number;
}

/**
 * 排产就绪度计算：给定「已收口主题」集合，算出每个主题能否开工。
 *
 * 「已收口」的判定不是本域自说自话——传入集合由上层收口单驱动，本条只做集合运算。
 * 阻塞时返回具体阻塞者（而非仅一个 false），因为排产看板要显示「卡在谁身上」。
 */
export function computeReadiness(closedThemes: readonly ThemeId[]): readonly ThemeReadiness[] {
  const closed = new Set(closedThemes);
  const out: ThemeReadiness[] = [];
  for (const id of THEME_IDS) {
    const spec = THEME_REGISTRY[id];
    const blockedBy = spec.requires.filter((d) => !closed.has(d));
    const specOk = spec.itemRange.lo <= spec.itemRange.hi;
    const labelCount = COMPUTE_LABELS.filter((l) => l.theme === id).length;
    out.push({
      theme: id,
      layer: spec.layer,
      depsReady: blockedBy.length === 0,
      blockedBy,
      canStart: blockedBy.length === 0 && specOk,
      computeLabelCount: labelCount,
    });
  }
  return out;
}

/** 域开工摘要（对外一页纸：进度 + 门禁 + 阻塞，AI 派单与看板共用）。 */
export interface DomainKickoffSummary {
  readonly domain: string;
  readonly layerCount: number;
  readonly themeCount: number;
  readonly itemRange: string;
  readonly boundaryPeers: readonly string[];
  readonly computeLabelCount: number;
  readonly buildOrder: readonly ThemeId[];
  readonly gates: readonly GateCriterion[];
}

/** 生成域开工摘要。构建序失败时返回显性失败（开工单不允许带未知序）。 */
export function buildKickoffSummary(): Outcome<DomainKickoffSummary> {
  const bag = new DiagBag();
  const order = resolveBuildOrder(THEME_IDS);
  if (!order.ok) {
    bag.pushAll(order.diagnostics);
    return fail<DomainKickoffSummary>(order.code, order.message, order.hint);
  }
  const coverage = auditBoundaryCoverage(new DiagBag());
  bag.pushAll(coverage.diagnostics);
  return ok(
    {
      domain: `${DOMAIN.id} ${DOMAIN.name}（${DOMAIN.itemLo}-${DOMAIN.itemHi}，共 ${DOMAIN.itemCount} 项，${DOMAIN.wave} 波）`,
      layerCount: LAYER_PLAN.length,
      themeCount: THEME_IDS.length,
      itemRange: `${DOMAIN.itemLo} ~ ${DOMAIN.itemHi}`,
      boundaryPeers: BOUNDARY_TABLE.map((b) => b.peer),
      computeLabelCount: COMPUTE_LABELS.length,
      buildOrder: order.value,
      gates: KICKOFF_GATES,
    },
    bag.all(),
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §6 自检（五条判据的可执行形态：不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 判据一自检：十主题齐备 + 区间不重叠 + 契约名唯一 + 主题名与官方序列一致。 */
export function selfCheckThemes(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "themes-ten-registered",
    pass: THEME_IDS.length === 10,
    detail: `注册表可枚举主题数 = ${THEME_IDS.length}（期望 10）`,
  });

  const namesMatch = OFFICIAL_THEMES.every((n) => Object.values(THEME_REGISTRY).some((s) => s.name === n));
  out.push({
    name: "themes-match-official-list",
    pass: namesMatch,
    detail: namesMatch
      ? `十主题名与官方序列逐项一致（${OFFICIAL_THEMES.join("/")}）`
      : "注册表主题名与官方十主题序列不一致（漏项或改写）",
  });

  // 区间重叠检测：两两比较 O(n²)，n=10 无需优化。
  let overlap = "";
  const ids = THEME_IDS;
  for (let i = 0; i < ids.length; i += 1) {
    for (let j = i + 1; j < ids.length; j += 1) {
      const a = THEME_REGISTRY[ids[i] as ThemeId].itemRange;
      const b = THEME_REGISTRY[ids[j] as ThemeId].itemRange;
      const hit = a.lo <= b.hi && b.lo <= a.hi;
      if (hit) overlap = `${a.lo}-${a.hi} 与 ${b.lo}-${b.hi}`;
    }
  }
  out.push({
    name: "themes-item-range-disjoint",
    pass: overlap === "",
    detail: overlap === "" ? "十主题功能号区间两两不重叠（200 项无重复认领）" : `区间重叠：${overlap}`,
  });

  const contracts = new Set<string>();
  let dupContract = "";
  for (const id of ids) {
    const c = THEME_REGISTRY[id].producesContract;
    if (contracts.has(c)) dupContract = c;
    contracts.add(c);
  }
  out.push({
    name: "themes-contract-name-unique",
    pass: dupContract === "",
    detail: dupContract === "" ? `十项产出契约名唯一（${contracts.size} 个）` : `契约名重复：${dupContract}（后续必然撞名）`,
  });

  // 区间合计应恰为 200（域功能总数），多退少补都是登记错误。
  let total = 0;
  for (const id of ids) {
    const r = THEME_REGISTRY[id].itemRange;
    total += r.hi - r.lo + 1;
  }
  out.push({
    name: "themes-item-total-matches-domain",
    pass: total === DOMAIN.itemCount,
    detail: `十主题区间合计 ${total} 项（期望 ${DOMAIN.itemCount} 项 = 域功能总数）`,
  });

  // 领域侧几何数据归属：L0 恰有一个主题且标记为几何数据。
  const l0 = themesInLayer("L0-geometry");
  const geoOk = l0.length === 1 && THEME_REGISTRY[l0[0] as ThemeId].isGeometryData;
  out.push({
    name: "themes-l0-is-geometry-data",
    pass: geoOk,
    detail: geoOk ? `L0 几何数据层 = ${l0.join(",")}，标记 isGeometryData 成立` : "L0 层主题数或几何数据标记不符",
  });

  return out;
}

/** 判据二自检：分层完整 + 依赖可拓扑排序 + 无环 + 无层号倒置 + 序稳定。 */
export function selfCheckLayering(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "layers-cover-all-themes",
    pass: LAYER_PLAN.reduce((n, p) => n + p.themes.length, 0) === THEME_IDS.length,
    detail: `分层计划覆盖主题数 = ${LAYER_PLAN.reduce((n, p) => n + p.themes.length, 0)}（期望 ${THEME_IDS.length}）`,
  });

  out.push({
    name: "layers-have-exit-criterion",
    pass: LAYER_PLAN.every((p) => p.exitCriterion.trim().length > 0),
    detail: `四层出口判据齐备数 = ${LAYER_PLAN.filter((p) => p.exitCriterion.trim().length > 0).length}/${LAYER_PLAN.length}`,
  });

  const order = resolveBuildOrder(THEME_IDS);
  out.push({
    name: "build-order-acyclic",
    pass: order.ok,
    detail: order.ok ? `拓扑排序成功，得开工序 ${order.value.join(" → ")}` : `拓扑排序失败：${order.message}`,
  });

  // 排序稳定性：同入参两次调用必得同序（排产序可被缓存复核）。
  const again = resolveBuildOrder(THEME_IDS);
  const stable =
    order.ok && again.ok && order.value.length === again.value.length
    && order.value.every((v, i) => v === again.value[i]);
  out.push({
    name: "build-order-stable",
    pass: stable,
    detail: stable ? "同入参两次拓扑排序逐位一致（序稳定可缓存）" : "两次排序结果不一致（排产序不稳定，禁止缓存）",
  });

  // 无层号倒置：同一批次里高层不得排在低层之前。
  const bag = new DiagBag();
  if (order.ok) bag.pushAll(order.diagnostics);
  const inverted = bag.byCode("LAYER_ORDER_VIOLATION").length;
  out.push({
    name: "no-layer-order-violation",
    pass: inverted === 0,
    detail: inverted === 0 ? "全部主题的 requires 与分层声明同向（无跨层反向依赖）" : `发现 ${inverted} 处层号倒置`,
  });

  // 每个 requires 指向的主题必须存在（引用不存在的 id 是悬空依赖）。
  let dangling = "";
  for (const id of THEME_IDS) {
    for (const dep of THEME_REGISTRY[id].requires) {
      if (THEME_REGISTRY[dep] === undefined) dangling = `${id} → ${dep}`;
    }
  }
  out.push({
    name: "no-dangling-dependency",
    pass: dangling === "",
    detail: dangling === "" ? "全部 requires 指向已登记主题（无悬空依赖）" : `悬空依赖：${dangling}`,
  });

  return out;
}

/** 判据三自检：五条边界齐备 + 越权被拦 + 合法操作不被误拦。 */
export function selfCheckBoundary(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "boundaries-five-peers",
    pass: BOUNDARY_TABLE.length === 5,
    detail: `已声明边界对 = ${BOUNDARY_TABLE.map((b) => b.peer).join("、")}（期望 A/C/D/Q/CGPU 五条）`,
  });

  out.push({
    name: "boundaries-have-consumption-direction",
    pass: BOUNDARY_TABLE.every((b) => b.weConsume.length > 0 && b.weDeliver.length > 0 && b.weMustNot.length > 0),
    detail: `三要素齐备（消费/交付/禁扩面）的边界对 = ${BOUNDARY_TABLE.filter((b) => b.weConsume.length > 0 && b.weDeliver.length > 0 && b.weMustNot.length > 0).length}/${BOUNDARY_TABLE.length}`,
  });

  // 越权必被拦：用 CGPU 边界的真实禁扩面条款构造一次越权操作。
  const bagBad = new DiagBag();
  const violation = checkBoundary("CGPU", "本域自行实现 AVX512 intrinsics 分派", bagBad);
  out.push({
    name: "boundary-violation-blocked",
    pass: !violation.ok && violation.code === "BOUNDARY_VIOLATION",
    detail: violation.ok
      ? "越权操作竟被放行（禁扩面失效，等于无边界）"
      : `越权被拦下：${violation.message}`,
  });

  // 合法操作不被误拦：反向验证——误拦会逼着调用方绕开检查器，边界也就名存实亡。
  const bagGood = new DiagBag();
  const legal = checkBoundary("CGPU", "向CGPU 卷提交网格遍历算力标签", bagGood);
  out.push({
    name: "boundary-legal-op-passes",
    pass: legal.ok,
    detail: legal.ok ? "合法操作未被误拦（检查器不制造假阳性）" : `合法操作被误拦：${legal.message}`,
  });

  // 未声明对接方必须被拦（防止「先用了再说」）。
  const bagUnknown = new DiagBag();
  const unknown = checkBoundary("VE-ZZ", "任意操作", bagUnknown);
  out.push({
    name: "boundary-undeclared-peer-rejected",
    pass: !unknown.ok && unknown.code === "BOUNDARY_UNDECLARED",
    detail: unknown.ok ? "未声明的对接方竟被放行（边界表可被绕过）" : "未声明的对接方被显性拒绝",
  });

  // 边界覆盖率：不得有孤岛主题（handoff 连对手方都没点名）。
  // 「交接给未登记域」不算失败（见 auditBoundaryCoverage 的分类理由），
  // 故只断言 orphan 为空，并单独断言 linked 非空（确有主题挂上了已声明边界）。
  const covBag = new DiagBag();
  const cov = auditBoundaryCoverage(covBag);
  const orphanEmpty = cov.ok && cov.value.orphan.length === 0;
  out.push({
    name: "boundary-no-orphan-theme",
    pass: orphanEmpty,
    detail: orphanEmpty
      ? `无孤岛主题；挂接已声明边界 ${cov.ok ? cov.value.linked.length : 0} 个，`
        + `交接给未登记域 ${cov.ok ? cov.value.toUndeclaredPeer.length : 0} 个（属正常缺口，已出诊断）`
      : `存在孤岛主题：${cov.ok ? cov.value.orphan.join("、") : cov.message}`,
  });

  out.push({
    name: "boundary-at-least-one-linked",
    pass: cov.ok && cov.value.linked.length > 0,
    detail: cov.ok ? `挂接已声明边界的主题 = ${cov.value.linked.join("、")}` : "覆盖审计失败",
  });

  return out;
}

/** 判据四自检：标签字段自洽 + 档位仲裁四态各自正确（两轴模型）。 */
export function selfCheckComputeLabels(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const bag = new DiagBag();

  const validated = validateComputeLabels(bag);
  out.push({
    name: "compute-labels-wellformed",
    pass: validated.ok && bag.size === 0,
    detail: bag.size === 0 ? `${COMPUTE_LABELS.length} 条算力标签字段全部自洽` : `非法标签 ${bag.size} 处：${bag.all()[0]?.message ?? ""}`,
  });

  out.push({
    name: "compute-labels-unique-id",
    pass: new Set(COMPUTE_LABEL_IDS).size === COMPUTE_LABEL_IDS.length,
    detail: `标签 id 唯一数 = ${new Set(COMPUTE_LABEL_IDS).size}/${COMPUTE_LABEL_IDS.length}`,
  });

  // 满配宿主：AVX-512 + 浮点 + 定点三域全在 → 所有标签原档执行。
  const full: HostSimdCapability = {
    maxWidthTier: "avx512-f32x8",
    numericDomains: ["float", "fixed-point", "scalar"],
  };
  const quantize = COMPUTE_LABELS.find((l) => l.labelId === "mesh-format/quantize-position") as ComputeLabel;
  const enough = resolveTier(quantize, full, 1_000_000, new DiagBag());
  out.push({
    name: "tier-adequate-no-downgrade",
    pass: enough.resolved === enough.requested && !enough.downgraded && !enough.blocked,
    detail: `满配宿主上定点标签原档执行：请求 ${enough.requested} → 实际 ${enough.resolved}（数值域定点已提供）`,
  });

  // 仲裁态一：数值域缺失 → 硬阻断（这是本条最重要的安全属性）。
  // AVX-512 满宽度但无定点域：宽度够也不能跑量化（跑了就破坏字节级一致）。
  const noFixed: HostSimdCapability = { maxWidthTier: "avx512-f32x8", numericDomains: ["float", "scalar"] };
  const domainBag = new DiagBag();
  const blocked = resolveTier(quantize, noFixed, 1_000_000, domainBag);
  out.push({
    name: "tier-missing-numeric-domain-blocks",
    pass: blocked.blocked && domainBag.byCode("SIMD_TIER_INSUFFICIENT").length === 1,
    detail: `宿主有 AVX-512 但无定点域时，不可降档的量化标签被阻断（${blocked.reason}），诊断 1 条`,
  });

  // 仲裁态二：可降档 + 宽度不足 → 降档执行并给质量影响提示，不阻断。
  // 选 qem-simplify：需求 AVX2 浮点、degradable=true；宿主宽度降到标量（1 通道 < 4 通道）。
  const simplify = COMPUTE_LABELS.find((l) => l.labelId === "mesh-format/qem-simplify") as ComputeLabel;
  const narrow: HostSimdCapability = { maxWidthTier: "scalar-fallback", numericDomains: ["float", "scalar"] };
  const degBag = new DiagBag();
  const degraded = resolveTier(simplify, narrow, 1_000_000, degBag);
  out.push({
    name: "tier-degradable-downgrades-with-hint",
    pass: degraded.downgraded && !degraded.blocked && degraded.reason.includes("低于需求")
      && degBag.byCode("SIMD_TIER_INSUFFICIENT").length === 1,
    detail: `可降档标签在窄宽度宿主上降档执行（${degraded.requested} → ${degraded.resolved}，${degraded.reason}），并产出质量影响提示`,
  });

  // 仲裁态三：不可降档 + 宽度不足 → 阻断（与数值域缺失是两条不同的阻断路径）。
  // 选 oct-normal-encode：需求 AVX2 浮点且 degradable=false，标量宿主上宽度不足。
  const oct = COMPUTE_LABELS.find((l) => l.labelId === "mesh-format/oct-normal-encode") as ComputeLabel;
  const scalarOnly: HostSimdCapability = { maxWidthTier: "scalar-fallback", numericDomains: ["scalar", "float"] };
  const scalarBag = new DiagBag();
  const scalarRes = resolveTier(oct, scalarOnly, 1_000_000, scalarBag);
  out.push({
    name: "tier-narrow-blocks-nondegradable",
    pass: scalarRes.blocked && scalarRes.reason.includes("不可降档"),
    detail: `标量宿主上不可降档的八面体编码标签被阻断（${scalarRes.reason}），与数值域阻断路径独立`,
  });

  // 仲裁态四：规模门控——规模不足时强制降一档宽度并说明是预期行为。
  const xform = COMPUTE_LABELS.find((l) => l.labelId === "vertex-pipeline/vertex-transform") as ComputeLabel;
  const smallBag = new DiagBag();
  const small = resolveTier(xform, full, 16, smallBag);
  out.push({
    name: "tier-scale-gate-downgrades",
    pass: small.downgraded && !small.blocked && small.reason === "规模门控降档"
      && smallBag.byCode("SIMD_TIER_INSUFFICIENT").length === 1,
    detail: `规模 16 低于 AVX-512 收益门槛时强制降至 ${small.resolved}（不阻塞），并说明这是预期行为而非降级`,
  });

  // 档位轴自洽：浮点档位不得被排到定点档之下/之上造成误判（两轴正交性回归）。
  const axesOrthogonal =
    TIER_DOMAIN["q15q31-fixed"] === "fixed-point"
    && TIER_DOMAIN["avx2-f32x4"] === "float"
    && TIER_DOMAIN["avx512-f32x8"] === "float"
    && TIER_RANK["q15q31-fixed"] === TIER_RANK["avx2-f32x4"];
  out.push({
    name: "tier-two-axis-model-intact",
    pass: axesOrthogonal,
    detail: "定点与浮点档同rank 但不同数值域（两轴正交，未退化为单轴强弱序）",
  });

  return out;
}

/** 判据五自检：开工门禁齐备 + 就绪度可算 + 摘要可生成。 */
export function selfCheckKickoff(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "gates-cover-five-criteria",
    pass: new Set(KICKOFF_GATES.map((g) => g.mapsTo)).size === 5,
    detail: `门禁覆盖判据数 = ${new Set(KICKOFF_GATES.map((g) => g.mapsTo)).size}（期望 5：十主题/分层/跨域边界/算力声明/判据）`,
  });

  out.push({
    name: "domain-identity-consistent",
    pass: DOMAIN.itemHi - DOMAIN.itemLo + 1 === DOMAIN.itemCount && DOMAIN.kickoffItemId === DOMAIN.itemLo,
    detail: `域身份自洽：${DOMAIN.itemLo}-${DOMAIN.itemHi} = ${DOMAIN.itemCount} 项，开工条 = ${DOMAIN.kickoffItemId}`,
  });

  // 就绪度：空收口集时，仅无前置主题可开工。
  const fresh = computeReadiness([]);
  const starters = fresh.filter((r) => r.canStart);
  out.push({
    name: "readiness-blocks-on-unclosed-deps",
    pass: starters.length > 0 && starters.every((r) => r.blockedBy.length === 0),
    detail: `零收口时可开工主题 = ${starters.map((r) => r.theme).join("、")}（应为唯一无前置的网格格式）`,
  });

  // 全收口后：十主题全部可开工，且阻塞集合全空。
  const allClosed = computeReadiness(THEME_IDS);
  out.push({
    name: "readiness-all-open-after-close",
    pass: allClosed.every((r) => r.canStart && r.blockedBy.length === 0),
    detail: `全收口后可开工主题 = ${allClosed.filter((r) => r.canStart).length}/${allClosed.length}`,
  });

  const summary = buildKickoffSummary();
  out.push({
    name: "kickoff-summary-buildable",
    pass: summary.ok && summary.value.buildOrder.length === THEME_IDS.length,
    detail: summary.ok
      ? `开工摘要可生成：${summary.value.layerCount} 层 / ${summary.value.themeCount} 主题 / ${summary.value.computeLabelCount} 算力标签`
      : `开工摘要生成失败：${summary.message}`,
  });

  return out;
}

/** 全量自检入口：一次跑完五组判据对应的全部检查项，返回逐项结果（不聚合为单一布尔）。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
} {
  const groups = {
    themes: selfCheckThemes(),
    layering: selfCheckLayering(),
    boundary: selfCheckBoundary(),
    compute: selfCheckComputeLabels(),
    kickoff: selfCheckKickoff(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}
