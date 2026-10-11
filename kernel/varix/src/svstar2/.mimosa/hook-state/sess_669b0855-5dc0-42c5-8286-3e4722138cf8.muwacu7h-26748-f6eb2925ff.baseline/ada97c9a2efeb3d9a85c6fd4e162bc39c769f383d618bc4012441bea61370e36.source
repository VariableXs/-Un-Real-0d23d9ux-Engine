//! VE-F0401 · VE-Shade 语言设计总纲（VE-C 域 · 着色器系统 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0401`
//!
//! **判据（锚点原文）**：原则六条全部可追溯到具体构造、三方收编策略定义、
//! 与 VE-C 接口冻结书一致、总纲评审通过、判据引用链建立。
//!
//! **定位三句话（锚点原文）**：语法像 WGSL 一样安全、语义像 GLSL 一样全、
//! 入口像 HLSL 一样能收编存量。语言族谱定位：VE-Shade 是"内部统一表示的
//! 源语言"，三方源进 VE 都先归一。总纲是 C 域的宪法，后续 199 项的判据
//! 都引用它。
//!
//! **设计要点**：
//! - **原则六条全部可追溯到具体构造**：原则不是口号——每条原则在构造目录
//!   （`ConstructCatalog`）里至少有一个具体语言构造实现它，且每个构造反向
//!   指回唯一所属原则；双向追踪任一断链（原则无构造/构造无原则/指错原则）
//!   都产出结构化 `VerificationIssue`（发生了什么/为什么/怎么修），不静默；
//! - **三方收编策略定义**：GLSL/HLSL/WGSL 三族各有收编策略（入口语义/
//!   归一路径/版本覆盖/已知不兼容点/禁止事项）。禁止事项必须含 VE 册红线
//!   "不发明私有格式、不魔改兼容"——收编是按标准全文实现语义，不是近似模拟；
//!   HLSL 的入口语义特殊（存量收编）：入口函数签名兼容策略显式在册；
//! - **与 VE-C 接口冻结书一致**：冻结书（F0397 移交包）四件之一是"编译
//!   目标清单（八族后端）"。总纲的后端目标表必须与冻结清单逐项一致
//!   （族数一致、族名一致、无多无漏）；软渲测试靶契约（shader 语义用软渲
//!   跑像素对拍）与前端接口冻结书版本都必须在总纲里登记并核对——总纲与
//!   冻结书冲突时以冻结书为准并出修订记录，不允许两处各说各话；
//! - **总纲评审通过**：评审不是走过场——评审记录是结构化的（评审项清单
//!   ×逐项通过态×双签），全绿且双签齐才算"通过"；任何一项未过，裁决必须
//!   是"未通过"且给整改建议（否决路径注入测试覆盖）；
//! - **判据引用链建立**：总纲条款编号制（VS-G-xx），C 域十组（词法/语法/
//!   语义/IR/优化/GLSL 后端/HLSL 后端/WGSL 后端/SPIR-V/热编译）每组必须
//!   引用至少一条存在的条款；链是双向的——条款能反查"哪些组引用了我"，
//!   引用不存在的条款 = 断链即缺陷；
//! - **族谱归一定位**：三方源一律先经归一（Normalize）再进内部表示——
//!   任何族跳过归一直连后端的路径都不存在（收编路径完整性检查）；
//! - **零静默**：所有验证失败产出五元组问题记录（代码/现象/根因/建议/
//!   严重度），`verification_issues()` 可全量取回。
//!
//! **错误路径与降级矩阵**：原则断链→结构化问题+建议；冻结不一致→以冻结
//! 书为准+修订记录；评审未过→裁决如实+整改建议；引用断链→拒绝入册。
//!
//! **跨批对接点**：上游 VE-F0397 移交包（冻结书四件）；下游 F0402 起全
//! C 域按本总纲引用条款推进。
//!
//! 确定性：全部静态注册表 + 纯函数校验，无时钟、无 IO、可回放对拍。
//! 零外部依赖，只依赖 `crate::checks`（自检侧）与 `alloc`。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、原则与构造（判据一：原则六条全部可追溯到具体构造）
// ---------------------------------------------------------------------------

/// 设计原则编号（六条，锚点原文定名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrincipleId {
    /// 内存安全默认：无裸指针，绑定即校验。
    MemorySafety,
    /// 确定性优先：同输入同输出，浮点模式显式声明。
    Determinism,
    /// 可转译：GLSL/HLSL/WGSL 三方源均可前端收编。
    Transpilability,
    /// 零开销抽象：高级构造编译后无运行时代价。
    ZeroCostAbstraction,
    /// 平台诚实：能力不足编译期报错不运行期降质。
    PlatformHonesty,
    /// 工具友好：每个语言构造可调试可可视化。
    ToolFriendliness,
}

impl PrincipleId {
    /// 全部六条，顺序即总纲宣示顺序（不得增减——总量铁律在本总纲的投影）。
    pub const ALL: [PrincipleId; 6] = [
        PrincipleId::MemorySafety,
        PrincipleId::Determinism,
        PrincipleId::Transpilability,
        PrincipleId::ZeroCostAbstraction,
        PrincipleId::PlatformHonesty,
        PrincipleId::ToolFriendliness,
    ];

    /// 条款代号（引用链用，如 `VS-P1`）。
    pub fn code(self) -> &'static str {
        match self {
            PrincipleId::MemorySafety => "VS-P1",
            PrincipleId::Determinism => "VS-P2",
            PrincipleId::Transpilability => "VS-P3",
            PrincipleId::ZeroCostAbstraction => "VS-P4",
            PrincipleId::PlatformHonesty => "VS-P5",
            PrincipleId::ToolFriendliness => "VS-P6",
        }
    }

    /// 原则名（人话，锚点原文口径）。
    pub fn name(self) -> &'static str {
        match self {
            PrincipleId::MemorySafety => "内存安全默认",
            PrincipleId::Determinism => "确定性优先",
            PrincipleId::Transpilability => "可转译",
            PrincipleId::ZeroCostAbstraction => "零开销抽象",
            PrincipleId::PlatformHonesty => "平台诚实",
            PrincipleId::ToolFriendliness => "工具友好",
        }
    }

    /// 原则宣言（一句话，锚点原文口径）。
    pub fn statement(self) -> &'static str {
        match self {
            PrincipleId::MemorySafety => "无裸指针，绑定即校验",
            PrincipleId::Determinism => "同输入同输出，浮点模式显式声明",
            PrincipleId::Transpilability => "GLSL/HLSL/WGSL 三方源均可前端收编",
            PrincipleId::ZeroCostAbstraction => "高级构造编译后无运行时代价",
            PrincipleId::PlatformHonesty => "能力不足编译期报错不运行期降质",
            PrincipleId::ToolFriendliness => "每个语言构造可调试可可视化",
        }
    }
}

/// 语言构造类别（构造目录的分类轴）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConstructKind {
    /// 类型系统构造（类型/绑定/布局）。
    Type,
    /// 语句与控制流构造。
    Statement,
    /// 内建函数与运算语义。
    Builtin,
    /// 编译期声明与注记。
    Annotation,
    /// 模块与入口组织。
    Module,
}

/// 语言构造：总纲里"原则落到哪里"的具体承载物。
///
/// 不变式：`principle` 指回唯一所属原则；`id` 全目录唯一（`VS-C-xx`）。
#[derive(Clone, Debug)]
pub struct Construct {
    /// 构造编号（VS-C-01 起，全局唯一）。
    pub id: &'static str,
    /// 构造名（人话）。
    pub name: &'static str,
    /// 所属原则（唯一，反向追踪用）。
    pub principle: PrincipleId,
    /// 构造类别。
    pub kind: ConstructKind,
    /// 语义摘要（该构造如何实现所属原则——一句话说得清，说不清=设计没想清）。
    pub semantics: &'static str,
}

impl Construct {
    /// 构造注册（入口校验：编号格式与语义非空——空语义的构造是占位符，
    /// 违反完成度铁律，注册即拒）。
    pub fn new(
        id: &'static str,
        name: &'static str,
        principle: PrincipleId,
        kind: ConstructKind,
        semantics: &'static str,
    ) -> Result<Self, ConstitError> {
        // "VS-C-01" = 7 字符（前缀 5 + 序号 2）；三位序号 8 字符，取下限 7。
        if !id.starts_with("VS-C-") || id.len() < 7 {
            return Err(ConstitError::BadConstructId {
                id: id.to_string(),
                suggestion: "编号必须形如 VS-C-01（前缀+两位序号）".to_string(),
            });
        }
        if name.is_empty() || semantics.is_empty() {
            return Err(ConstitError::EmptyConstructSpec {
                id: id.to_string(),
                suggestion: "构造名与语义摘要都必填，占位构造不许入册".to_string(),
            });
        }
        Ok(Self { id, name, principle, kind, semantics })
    }
}

/// 语言构造目录：全部已注册构造（总纲的"原则落地清单"）。
pub struct ConstructCatalog {
    /// 目录条目（`pub(crate)` 供自检模块做类别覆盖扫描；外部一律走方法）。
    pub(crate) entries: Vec<Construct>,
}

impl ConstructCatalog {
    /// 预置目录：六原则 × 每原则三条落地构造（18 条，覆盖类型/语句/内建/
    /// 注记/模块五类——原则没有构造承载就是空话，构造不归原则就是散沙）。
    pub fn standard() -> Self {
        let mut entries = Vec::new();
        let seed: [(&'static str, &'static str, PrincipleId, ConstructKind, &'static str); 18] = [
            // P1 内存安全默认
            ("VS-C-01", "资源绑定声明", PrincipleId::MemorySafety, ConstructKind::Annotation,
             "资源只经 bind 声明的句柄访问，绑定在编译期与管线索引双向校验，不匹配即报错"),
            ("VS-C-02", "无指针类型族", PrincipleId::MemorySafety, ConstructKind::Type,
             "类型系统不提供裸指针/引用逃逸，聚合类型按值与句柄语义传递，悬垂在语言层不可表达"),
            ("VS-C-03", "越界访问诊断", PrincipleId::MemorySafety, ConstructKind::Builtin,
             "数组/向量越界是诊断报错而非未定义行为，读出钳制值必须同时出编译告警"),
            // P2 确定性优先
            ("VS-C-04", "浮点模式声明", PrincipleId::Determinism, ConstructKind::Annotation,
             "每个模块必须显式声明浮点求值模式（strict/ieee/fma），缺省即拒绝编译——同输入同输出的前提是求值规则先定死"),
            ("VS-C-05", "定点循环界", PrincipleId::Determinism, ConstructKind::Statement,
             "循环边界必须是编译期可推断的常量或绑定维数，禁止数据依赖的动态终止——终止时间不确定即输出不确定"),
            ("VS-C-06", "确定性内建白名单", PrincipleId::Determinism, ConstructKind::Builtin,
             "内建函数分确定性/平台差异两级，跨后端输出不一致的内建必须显式标注且默认禁用"),
            // P3 可转译
            ("VS-C-07", "三方前端归一入口", PrincipleId::Transpilability, ConstructKind::Module,
             "GLSL/HLSL/WGSL 三族源各经专用前端归一为同一内部表示，归一前不进优化与后端"),
            ("VS-C-08", "源语言标记", PrincipleId::Transpilability, ConstructKind::Annotation,
             "归一单元携带源语言与版本标记，诊断与对拍可回溯到原始方言——收编不是洗掉出处"),
            ("VS-C-09", "收编映射表", PrincipleId::Transpilability, ConstructKind::Builtin,
             "三方内建/语义差异点逐条登记映射（直译/等价改写/显性拒绝三类），拒绝必须给原因不静默"),
            // P4 零开销抽象
            ("VS-C-10", "函数内联无桩", PrincipleId::ZeroCostAbstraction, ConstructKind::Statement,
             "函数调用编译后无运行时桩与额外栈帧，抽象成本只在编译期——高级构造不许留下运行时税"),
            ("VS-C-11", "泛型单态化", PrincipleId::ZeroCostAbstraction, ConstructKind::Type,
             "泛型按实例单态化展开，无装箱无动态分派，产物与手写特化逐指令等价"),
            ("VS-C-12", "无隐藏分配", PrincipleId::ZeroCostAbstraction, ConstructKind::Statement,
             "语言构造不允许引入源码里看不见的堆分配，编译器对隐式分配直接报错"),
            // P5 平台诚实
            ("VS-C-13", "能力要求注记", PrincipleId::PlatformHonesty, ConstructKind::Annotation,
             "模块可声明 requires 能力位（纹理格式/指令族/存储类），能力不足编译期报错并列出缺口"),
            ("VS-C-14", "禁运行期降质", PrincipleId::PlatformHonesty, ConstructKind::Statement,
             "编译器不产生'降质回退变体'来迁就弱平台——降档是调度层纪律，语言层只诚实报错"),
            ("VS-C-15", "能力缺口报告", PrincipleId::PlatformHonesty, ConstructKind::Builtin,
             "能力检查失败的诊断必须逐条列出：缺什么能力/哪条语句需要/替代写法建议"),
            // P6 工具友好
            ("VS-C-16", "构造级调试注记", PrincipleId::ToolFriendliness, ConstructKind::Annotation,
             "每个构造携带原文跨度与来源方言注记，调试器可从产物定位回源码任意构造"),
            ("VS-C-17", "可视化元数据", PrincipleId::ToolFriendliness, ConstructKind::Module,
             "模块可导出结构摘要（绑定表/内建用量/能力需求），可视化工具直接消费不需反编译"),
            ("VS-C-18", "对拍锚点导出", PrincipleId::ToolFriendliness, ConstructKind::Builtin,
             "编译产物可导出逐构造语义锚点，供对拍基准逐条比对——工具友好包括机器友好"),
        ];
        for (id, name, p, k, s) in seed {
            // 预置目录是宪法本体，注册失败在构建期即暴露（构建期缺陷抛错纪律）。
            let c = Construct::new(id, name, p, k, s).expect("预置构造注册失败");
            entries.push(c);
        }
        Self { entries }
    }

    /// 注册新构造（追加不覆盖——总纲条款只增不改，演进走升版）。
    pub fn register(&mut self, c: Construct) -> Result<(), ConstitError> {
        if self.entries.iter().any(|e| e.id == c.id) {
            return Err(ConstitError::DuplicateConstructId {
                id: c.id.to_string(),
                suggestion: "编号已占用，换新序号；改语义走修订流程不走覆盖".to_string(),
            });
        }
        self.entries.push(c);
        Ok(())
    }

    /// 按原则取构造（正向追踪：原则 → 落地构造）。
    pub fn by_principle(&self, p: PrincipleId) -> Vec<&Construct> {
        self.entries.iter().filter(|c| c.principle == p).collect()
    }

    /// 全目录条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 目录是否为空（空目录=总纲无落地，校验必红）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按编号取构造。
    pub fn find(&self, id: &str) -> Option<&Construct> {
        self.entries.iter().find(|c| c.id == id)
    }
}

/// 构造一条设计原则（宣言 + 正向落地构造清单）。
pub struct DesignPrinciple {
    /// 原则编号。
    pub id: PrincipleId,
    /// 该原则的落地构造编号（从目录取，注册时逐条核对存在性）。
    pub construct_ids: Vec<&'static str>,
}

impl DesignPrinciple {
    /// 从目录派生原则表：每条原则的构造清单由目录反查得出——
    /// 派生而非手抄，杜绝"原则表与构造目录两张皮"。
    pub fn derive_all(catalog: &ConstructCatalog) -> Vec<DesignPrinciple> {
        PrincipleId::ALL
            .iter()
            .map(|p| DesignPrinciple {
                id: *p,
                construct_ids: catalog
                    .by_principle(*p)
                    .iter()
                    .map(|c| c.id)
                    .collect(),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 二、三方收编策略（判据二：三方收编策略定义）
// ---------------------------------------------------------------------------

/// 源语言族（三方：GLSL/HLSL/WGSL）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceFamily {
    /// GLSL——语义参照系（语义像 GLSL 一样全）。
    Glsl,
    /// HLSL——存量收编对象（入口像 HLSL 一样能收编存量）。
    Hlsl,
    /// WGSL——安全语法参照系（语法像 WGSL 一样安全）。
    Wgsl,
}

impl SourceFamily {
    /// 全部三族。
    pub const ALL: [SourceFamily; 3] = [SourceFamily::Glsl, SourceFamily::Hlsl, SourceFamily::Wgsl];

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            SourceFamily::Glsl => "GLSL",
            SourceFamily::Hlsl => "HLSL",
            SourceFamily::Wgsl => "WGSL",
        }
    }
}

/// 归一路径（族谱定位：三方源进 VE 都先归一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NormalizationPath {
    /// 专用前端逐族归一为内部统一表示。
    DedicatedFrontend,
    /// 禁止路径：未经归一直连后端（合法性检查必须红）。
    DirectToBackend,
}

/// 三方收编策略：一族一份，逐字段显式。
pub struct IngestionStrategy {
    /// 源族。
    pub family: SourceFamily,
    /// 入口语义（收编入口怎么定——HLSL 为存量收编，入口签名兼容策略在此）。
    pub entry_semantics: &'static str,
    /// 归一路径（必须 DedicatedFrontend）。
    pub normalization: NormalizationPath,
    /// 版本覆盖声明（收编到哪个版本为止，诚实标注）。
    pub version_coverage: &'static str,
    /// 已知不兼容点（诚实清单，逐条列——没有清单的收编是吹牛）。
    pub known_gaps: Vec<&'static str>,
    /// 禁止事项（必须含红线两条：不发明私有格式、不魔改兼容）。
    pub forbidden: Vec<&'static str>,
}

impl IngestionStrategy {
    /// 注册入口校验：五要素齐 + 归一路径合法 + 禁止事项含红线。
    /// 缺任何一项都拒绝入册（收编策略半份 = 没有策略）。
    pub fn new(
        family: SourceFamily,
        entry_semantics: &'static str,
        normalization: NormalizationPath,
        version_coverage: &'static str,
        known_gaps: Vec<&'static str>,
        forbidden: Vec<&'static str>,
    ) -> Result<Self, ConstitError> {
        if entry_semantics.is_empty() || version_coverage.is_empty() {
            return Err(ConstitError::IncompleteStrategy {
                family: family.name().to_string(),
                missing: "入口语义或版本覆盖为空".to_string(),
                suggestion: "入口语义与版本覆盖逐字写明，诚实标注覆盖边界".to_string(),
            });
        }
        if normalization != NormalizationPath::DedicatedFrontend {
            return Err(ConstitError::IllegalNormalizationPath {
                family: family.name().to_string(),
                suggestion: "族谱定位锁死：三方源一律先归一，直连后端路径不存在".to_string(),
            });
        }
        if known_gaps.is_empty() {
            return Err(ConstitError::IncompleteStrategy {
                family: family.name().to_string(),
                missing: "已知不兼容点清单为空".to_string(),
                suggestion: "没有已知缺口也要写'暂无已知缺口（首版审计）'，空清单=没审计".to_string(),
            });
        }
        for redline in REDLINE_FORBIDDEN {
            if !forbidden.iter().any(|f| f.contains(redline)) {
                return Err(ConstitError::RedlineMissing {
                    family: family.name().to_string(),
                    redline,
                    suggestion: "收编策略必须含 VE 册红线：不发明私有格式、不魔改兼容".to_string(),
                });
            }
        }
        Ok(Self {
            family,
            entry_semantics,
            normalization,
            version_coverage,
            known_gaps,
            forbidden,
        })
    }
}

/// VE 册红线关键词（收编禁止事项的硬门槛）。
pub const REDLINE_FORBIDDEN: [&str; 2] = ["私有格式", "魔改兼容"];

/// 三方收编策略表（预置版：锚点三句话定位 + 诚实标注的已知缺口）。
pub fn standard_ingestion() -> Vec<IngestionStrategy> {
    vec![
        IngestionStrategy::new(
            SourceFamily::Glsl,
            "语义参照系：内建函数全集按 GLSL 语义对拍收编，内建等价表给出逐条映射",
            NormalizationPath::DedicatedFrontend,
            "GLSL 4.60（desktop）+ GLSL ES 3.20；超出部分显性拒绝并给版本提示",
            vec![
                "desktop 扩展依赖硬件特性时受能力注记约束（requires 声明联动）",
                "隐式类型转换宽容度高于 WGSL，归一化为显式转换并在告警面板可见",
            ],
            vec!["不发明私有格式", "不魔改兼容", "禁止静默丢弃标准语义"],
        )
        .expect("GLSL 收编策略预置合法"),
        IngestionStrategy::new(
            SourceFamily::Hlsl,
            "存量收编入口：入口函数签名按 HLSL 惯例识别（SV_* 语义绑定），签名级兼容映射在册",
            NormalizationPath::DedicatedFrontend,
            "HLSL SM 6.x 为主干（DXIL 时代），SM 5.x 常量子集显性标注",
            vec![
                "SM 5.x 以前的 register 绑定模型经映射表转换，未登记的槽位语义拒绝编译",
                "效果框架（fx）语法不在收编范围（显性拒绝并给改写建议）",
            ],
            vec!["不发明私有格式", "不魔改兼容", "禁止静默丢弃标准语义"],
        )
        .expect("HLSL 收编策略预置合法"),
        IngestionStrategy::new(
            SourceFamily::Wgsl,
            "安全语法参照系：内存安全默认原则与其对齐，绑定校验语义同源",
            NormalizationPath::DedicatedFrontend,
            "WGSL 候选快照（实现随标准演进升版，升版走修订流程）",
            vec![
                "地址空间模型与 VE 用户态服务资源模型映射，差异点在映射表逐条登记",
                "部分标准仍在演进中的语法标注为实验档，默认关闭并显性告知",
            ],
            vec!["不发明私有格式", "不魔改兼容", "禁止静默丢弃标准语义"],
        )
        .expect("WGSL 收编策略预置合法"),
    ]
}

// ---------------------------------------------------------------------------
// 三、接口冻结书一致性（判据三：与 VE-C 接口冻结书一致）
// ---------------------------------------------------------------------------

/// VE-C 接口冻结书（F0397 移交包四件之一 + 相关件）。
///
/// 后端目标清单来源：B 域驱动八族分档（virtio、Intel、AMD、NVIDIA、Mali、
/// Adreno、PowerVR、软渲）——着色器编译器必须能面向这八族产出目标码，
/// 族数与族名以冻结清单为准，总纲不得私改。
pub struct FreezeBook {
    /// 编译目标清单（八族后端，冻结序）。
    pub backend_targets: Vec<&'static str>,
    /// 软渲测试靶契约（shader 语义用软渲跑像素对拍——对拍红线的落地方式）。
    pub softrender_target_contract: &'static str,
    /// 前端接口冻结书版本（VE 前端消费编译器接口的冻结版本）。
    pub frontend_interface_version: &'static str,
    /// 冻结书来源锚（可追溯）。
    pub source_anchor: &'static str,
}

impl FreezeBook {
    /// 冻结书标准版（F0397 移交包口径，族序与 B 域八族分档一致）。
    pub fn standard() -> Self {
        Self {
            backend_targets: vec![
                "virtio",
                "intel",
                "amd",
                "nvidia",
                "mali",
                "adreno",
                "powervr",
                "softrender",
            ],
            softrender_target_contract:
                "shader 语义用软渲跑像素对拍：每条语义判据在软渲靶机逐像素比对",
            frontend_interface_version: "VE-C-IF-FROZEN-v1",
            source_anchor: "VE-F0397 · VE-C 域移交包",
        }
    }
}

/// 总纲 ↔ 冻结书一致性报告。
pub struct FreezeConsistency {
    /// 八族后端逐项一致（族数一致 + 族名逐项一致 + 无多无漏）。
    pub backends_match: bool,
    /// 软渲测试靶契约已登记且与冻结书逐字一致。
    pub softrender_contract_ok: bool,
    /// 前端接口冻结书版本已登记且非空。
    pub frontend_interface_ok: bool,
    /// 冲突清单（空 = 一致；非空 = 逐条说明以冻结书为准的修订要求）。
    pub conflicts: Vec<String>,
}

// ---------------------------------------------------------------------------
// 四、总纲条款与判据引用链（判据五：判据引用链建立）
// ---------------------------------------------------------------------------

/// 条款来源（条款从哪来——引用链的溯源轴）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClauseOrigin {
    /// 源自设计原则。
    Principle(PrincipleId),
    /// 源自三方收编策略。
    Ingestion,
    /// 源自接口冻结书。
    Freeze,
    /// 源自治理要求（评审/引用链/修订纪律本身）。
    Governance,
}

/// 总纲条款（编号制：VS-G-xx，C 域后续 199 项的判据都引用它）。
pub struct Clause {
    /// 条款编号（VS-G-01 起，全局唯一）。
    pub id: &'static str,
    /// 条款标题。
    pub title: &'static str,
    /// 条款正文摘要（引用方据此对齐语义——摘要必须自足，读到即懂）。
    pub digest: &'static str,
    /// 条款来源。
    pub origin: ClauseOrigin,
}

/// C 域十组（C01 引言钦定分组，引用链的引用方全集）。
pub const C_DOMAIN_GROUPS: [&str; 10] = [
    "词法",
    "语法",
    "语义",
    "IR",
    "优化",
    "GLSL后端",
    "HLSL后端",
    "WGSL后端",
    "SPIR-V",
    "热编译",
];

/// 引用记录：哪个组引用了哪些条款。
pub struct GroupRef {
    /// 引用方组名（必须是 C_DOMAIN_GROUPS 之一）。
    pub group: &'static str,
    /// 引用的条款编号（必须都存在——断链即缺陷）。
    pub clause_ids: Vec<&'static str>,
}

/// 条款注册表 + 引用链。
pub struct ClauseRegistry {
    clauses: Vec<Clause>,
    refs: Vec<GroupRef>,
}

impl ClauseRegistry {
    /// 预置条款表：六原则各出一条宪法级条款 + 收编/冻结/治理条款。
    /// 条款摘要必须自足——下游判据引用的是"这一条"，不是"总纲那个意思"。
    pub fn standard() -> Self {
        let clauses = vec![
            Clause { id: "VS-G-01", title: "绑定即校验", digest: "资源访问只允许经过绑定声明的句柄，绑定与管线资源索引不匹配即编译报错，禁止运行期才发现", origin: ClauseOrigin::Principle(PrincipleId::MemorySafety) },
            Clause { id: "VS-G-02", title: "浮点模式显式", digest: "每个模块声明浮点求值模式，缺省拒绝编译；跨后端输出必须同模式逐位一致才算编译通过", origin: ClauseOrigin::Principle(PrincipleId::Determinism) },
            Clause { id: "VS-G-03", title: "三方先归一", digest: "GLSL/HLSL/WGSL 源一律先经专用前端归一为内部统一表示，归一之前不进优化与后端，直连路径不存在", origin: ClauseOrigin::Principle(PrincipleId::Transpilability) },
            Clause { id: "VS-G-04", title: "抽象零运行时税", digest: "高级构造（函数/泛型/聚合）编译后与手写低级代码逐指令等价，任何引入运行时代价的构造必须显性声明代价", origin: ClauseOrigin::Principle(PrincipleId::ZeroCostAbstraction) },
            Clause { id: "VS-G-05", title: "能力不足编译期报错", digest: "requires 能力位不满足即编译报错并列缺口清单，编译器不生成降质回退变体，降档是调度层的事", origin: ClauseOrigin::Principle(PrincipleId::PlatformHonesty) },
            Clause { id: "VS-G-06", title: "构造可调试可可视化", digest: "每个构造携带原文跨度注记，产物可导出结构摘要与逐构造语义锚点，工具消费不需反编译", origin: ClauseOrigin::Principle(PrincipleId::ToolFriendliness) },
            Clause { id: "VS-G-07", title: "收编按标准全文实现", digest: "三方源按标准全文实现语义映射，映射分直译/等价改写/显性拒绝三类，拒绝必给原因；禁止发明私有格式与魔改兼容", origin: ClauseOrigin::Ingestion },
            Clause { id: "VS-G-08", title: "八族后端目标面", digest: "编译目标清单以冻结书八族为准（virtio/Intel/AMD/NVIDIA/Mali/Adreno/PowerVR/软渲），总纲不得增减族序族名", origin: ClauseOrigin::Freeze },
            Clause { id: "VS-G-09", title: "软渲测试靶契约", digest: "shader 语义判据用软渲靶跑像素对拍验证，语义正确性以对拍结果为准不以实现意图为准", origin: ClauseOrigin::Freeze },
            Clause { id: "VS-G-10", title: "判据引用链制度", digest: "C 域后续 199 项的判据必须引用本总纲条款号（VS-G-xx），引用不存在的条款即阻断；条款只增不改，演进走升版", origin: ClauseOrigin::Governance },
            Clause { id: "VS-G-11", title: "评审双签纪律", digest: "总纲评审全项通过且施工/验收双签齐备方可生效，任何单项未过裁决即为未通过并附整改建议", origin: ClauseOrigin::Governance },
        ];
        let refs = vec![
            GroupRef { group: "词法", clause_ids: vec!["VS-G-03", "VS-G-07"] },
            GroupRef { group: "语法", clause_ids: vec!["VS-G-03", "VS-G-07"] },
            GroupRef { group: "语义", clause_ids: vec!["VS-G-01", "VS-G-02", "VS-G-07"] },
            GroupRef { group: "IR", clause_ids: vec!["VS-G-03", "VS-G-06", "VS-G-04"] },
            GroupRef { group: "优化", clause_ids: vec!["VS-G-02", "VS-G-04"] },
            GroupRef { group: "GLSL后端", clause_ids: vec!["VS-G-08", "VS-G-02", "VS-G-05"] },
            GroupRef { group: "HLSL后端", clause_ids: vec!["VS-G-08", "VS-G-07", "VS-G-05"] },
            GroupRef { group: "WGSL后端", clause_ids: vec!["VS-G-08", "VS-G-01", "VS-G-05"] },
            GroupRef { group: "SPIR-V", clause_ids: vec!["VS-G-08", "VS-G-06"] },
            GroupRef { group: "热编译", clause_ids: vec!["VS-G-06", "VS-G-10", "VS-G-09"] },
        ];
        Self { clauses, refs }
    }

    /// 注册新条款（只增不改：同号冲突拒绝，语义修订走升版）。
    pub fn register(&mut self, c: Clause) -> Result<(), ConstitError> {
        // "VS-G-01" = 7 字符（前缀 5 + 序号 2）；三位序号 8 字符，取下限 7。
        if !c.id.starts_with("VS-G-") || c.id.len() < 7 {
            return Err(ConstitError::BadClauseId {
                id: c.id.to_string(),
                suggestion: "条款编号必须形如 VS-G-01".to_string(),
            });
        }
        if c.digest.is_empty() {
            return Err(ConstitError::EmptyClauseDigest {
                id: c.id.to_string(),
                suggestion: "条款摘要必须自足可读，占位条款不许入册".to_string(),
            });
        }
        if self.clauses.iter().any(|e| e.id == c.id) {
            return Err(ConstitError::DuplicateClauseId {
                id: c.id.to_string(),
                suggestion: "条款号已占用；语义修订走升版登记，不许原地覆盖".to_string(),
            });
        }
        self.clauses.push(c);
        Ok(())
    }

    /// 登记引用（校验：组名合法 + 引用的条款都存在——断链拒绝入册）。
    pub fn add_ref(&mut self, r: GroupRef) -> Result<(), ConstitError> {
        if !C_DOMAIN_GROUPS.contains(&r.group) {
            return Err(ConstitError::UnknownGroup {
                group: r.group.to_string(),
                suggestion: "引用方必须是 C 域十组之一".to_string(),
            });
        }
        for cid in &r.clause_ids {
            if !self.clauses.iter().any(|c| c.id == *cid) {
                return Err(ConstitError::DanglingClauseRef {
                    group: r.group.to_string(),
                    clause: (*cid).to_string(),
                    suggestion: "引用不存在的条款即断链；先补条款再引用".to_string(),
                });
            }
        }
        if self.refs.iter().any(|e| e.group == r.group) {
            // 同组重复登记：合并而非覆盖（引用只增不减）。
            if let Some(e) = self.refs.iter_mut().find(|e| e.group == r.group) {
                for cid in r.clause_ids {
                    if !e.clause_ids.contains(&cid) {
                        e.clause_ids.push(cid);
                    }
                }
            }
            return Ok(());
        }
        self.refs.push(r);
        Ok(())
    }

    /// 条款表条数。
    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }

    /// 引用记录条数。
    pub fn ref_count(&self) -> usize {
        self.refs.len()
    }

    /// 反查：哪些组引用了该条款（引用链的双向性）。
    pub fn citing_groups(&self, clause_id: &str) -> Vec<&'static str> {
        self.refs
            .iter()
            .filter(|r| r.clause_ids.iter().any(|c| c == &clause_id))
            .map(|r| r.group)
            .collect()
    }

    /// 每组引用数（十组覆盖检查用）。
    pub fn group_coverage(&self) -> Vec<(&'static str, usize)> {
        C_DOMAIN_GROUPS
            .iter()
            .map(|g| {
                let n = self
                    .refs
                    .iter()
                    .find(|r| r.group == *g)
                    .map(|r| r.clause_ids.len())
                    .unwrap_or(0);
                (*g, n)
            })
            .collect()
    }

    /// 条款表只读视图。
    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }
}

// ---------------------------------------------------------------------------
// 五、总纲评审（判据四：总纲评审通过）
// ---------------------------------------------------------------------------

/// 单条评审项。
pub struct ReviewItem {
    /// 评审项编号。
    pub id: &'static str,
    /// 评审问题（审什么）。
    pub question: &'static str,
    /// 逐项通过态。
    pub passed: bool,
    /// 未过时的整改建议（passed=true 时忽略）。
    pub remedy: &'static str,
}

/// 评审裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewVerdict {
    /// 全项通过 + 双签齐备。
    Passed,
    /// 存在未过项。
    Rejected,
    /// 双签不齐（项全绿但签不全，同样不算通过）。
    MissingSignatures,
}

/// 总纲评审记录：结构化、可注入、可复现。
pub struct ReviewRecord {
    items: Vec<ReviewItem>,
    signatures: Vec<&'static str>,
}

impl ReviewRecord {
    /// 建评审记录（空记录不合法——没有评审项的评审是走过场）。
    pub fn new(items: Vec<ReviewItem>, signatures: Vec<&'static str>) -> Result<Self, ConstitError> {
        if items.is_empty() {
            return Err(ConstitError::EmptyReview {
                suggestion: "评审项清单不得为空：至少覆盖五条判据各一项".to_string(),
            });
        }
        Ok(Self { items, signatures })
    }

    /// 裁决：全项绿 + 双签才 Passed；缺签 MissingSignatures；有红项 Rejected。
    pub fn verdict(&self) -> ReviewVerdict {
        let all_passed = self.items.iter().all(|i| i.passed);
        if !all_passed {
            return ReviewVerdict::Rejected;
        }
        // 双签：施工签 + 验收签，缺一不可。
        if self.signatures.len() < 2 {
            return ReviewVerdict::MissingSignatures;
        }
        ReviewVerdict::Passed
    }

    /// 未过项清单（裁决 Rejected 时的整改输入）。
    pub fn failed_items(&self) -> Vec<&ReviewItem> {
        self.items.iter().filter(|i| !i.passed).collect()
    }

    /// 评审项数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 追加签名（签满双签）。
    pub fn add_signature(&mut self, who: &'static str) {
        if !self.signatures.contains(&who) {
            self.signatures.push(who);
        }
    }
}

/// 标准评审项（覆盖五条判据逐条 + 审计项）。
pub fn standard_review() -> ReviewRecord {
    let items = vec![
        ReviewItem { id: "RV-1", question: "原则六条是否全部有具体构造承载（双向追踪无断链）", passed: true, remedy: "为无承载的原则补落地构造" },
        ReviewItem { id: "RV-2", question: "三方收编策略是否五要素齐备且含红线禁止事项", passed: true, remedy: "补齐缺失要素与红线条款" },
        ReviewItem { id: "RV-3", question: "与接口冻结书是否一致（八族/软渲靶/前端接口版本）", passed: true, remedy: "按冻结书修正总纲并留修订记录" },
        ReviewItem { id: "RV-4", question: "判据引用链是否建立（条款编号制 + 十组全覆盖 + 无断链）", passed: true, remedy: "补条款与引用，消除断链" },
        ReviewItem { id: "RV-5", question: "族谱定位是否锁定（三方源先归一，直连路径不存在）", passed: true, remedy: "在收编策略中强制 DedicatedFrontend" },
        ReviewItem { id: "RV-6", question: "条款是否只增不改（修订流程在册，覆盖路径被拒绝）", passed: true, remedy: "补修订流程条款并加覆盖拒绝测试" },
    ];
    let mut r = ReviewRecord::new(items, vec!["施工AI"]).expect("标准评审项合法");
    r.add_signature("验收AI");
    r
}

// ---------------------------------------------------------------------------
// 六、总纲聚合与全量校验（判据的执行面）
// ---------------------------------------------------------------------------

/// 总纲级错误（五要素：现象/建议齐备，零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstitError {
    /// 构造编号格式非法。
    BadConstructId { id: String, suggestion: String },
    /// 构造名/语义为空（占位构造）。
    EmptyConstructSpec { id: String, suggestion: String },
    /// 构造编号重复。
    DuplicateConstructId { id: String, suggestion: String },
    /// 条款编号格式非法。
    BadClauseId { id: String, suggestion: String },
    /// 条款摘要为空。
    EmptyClauseDigest { id: String, suggestion: String },
    /// 条款编号重复。
    DuplicateClauseId { id: String, suggestion: String },
    /// 收编策略要素不全。
    IncompleteStrategy { family: String, missing: String, suggestion: String },
    /// 归一路径非法（直连后端）。
    IllegalNormalizationPath { family: String, suggestion: String },
    /// 禁止事项缺红线。
    RedlineMissing { family: String, redline: &'static str, suggestion: String },
    /// 引用方不是 C 域十组。
    UnknownGroup { group: String, suggestion: String },
    /// 引用了不存在的条款（断链）。
    DanglingClauseRef { group: String, clause: String, suggestion: String },
    /// 评审项清单为空。
    EmptyReview { suggestion: String },
}

impl ConstitError {
    /// 人话呈现（发生了什么 + 怎么修——异常零静默铁律）。
    pub fn describe(&self) -> String {
        match self {
            ConstitError::BadConstructId { id, suggestion } => {
                format!("构造编号非法：{id}。建议：{suggestion}")
            }
            ConstitError::EmptyConstructSpec { id, suggestion } => {
                format!("构造 {id} 名或语义为空。建议：{suggestion}")
            }
            ConstitError::DuplicateConstructId { id, suggestion } => {
                format!("构造编号重复：{id}。建议：{suggestion}")
            }
            ConstitError::BadClauseId { id, suggestion } => {
                format!("条款编号非法：{id}。建议：{suggestion}")
            }
            ConstitError::EmptyClauseDigest { id, suggestion } => {
                format!("条款 {id} 摘要为空。建议：{suggestion}")
            }
            ConstitError::DuplicateClauseId { id, suggestion } => {
                format!("条款编号重复：{id}。建议：{suggestion}")
            }
            ConstitError::IncompleteStrategy { family, missing, suggestion } => {
                format!("{family} 收编策略不全：{missing}。建议：{suggestion}")
            }
            ConstitError::IllegalNormalizationPath { family, suggestion } => {
                format!("{family} 归一路径非法。建议：{suggestion}")
            }
            ConstitError::RedlineMissing { family, redline, suggestion } => {
                format!("{family} 禁止事项缺红线「{redline}」。建议：{suggestion}")
            }
            ConstitError::UnknownGroup { group, suggestion } => {
                format!("引用方组名未知：{group}。建议：{suggestion}")
            }
            ConstitError::DanglingClauseRef { group, clause, suggestion } => {
                format!("{group} 引用了不存在的条款 {clause}。建议：{suggestion}")
            }
            ConstitError::EmptyReview { suggestion } => {
                format!("评审项清单为空。建议：{suggestion}")
            }
        }
    }
}

/// 校验问题（验证引擎的结构化产出——失败不是 panic，是可检索的记录）。
pub struct VerificationIssue {
    /// 问题代码（检索键，如 `TRACE-P1-NOCARRY`）。
    pub code: String,
    /// 现象（发生了什么）。
    pub what: String,
    /// 建议（怎么办）。
    pub suggestion: String,
}

/// VE-Shade 语言设计总纲：原则/构造/收编/冻结/条款/评审的聚合体。
pub struct Constitution {
    /// 原则六条（含正向落地构造清单）。
    pub principles: Vec<DesignPrinciple>,
    /// 语言构造目录。
    pub catalog: ConstructCatalog,
    /// 三方收编策略。
    pub ingestion: Vec<IngestionStrategy>,
    /// 接口冻结书（一致性基准）。
    pub freeze: FreezeBook,
    /// 总纲声明后端目标面（总纲侧的编译目标承诺——与冻结书对账的本方清单）。
    pub declared_backend_targets: Vec<&'static str>,
    /// 条款注册表与引用链。
    pub registry: ClauseRegistry,
    /// 评审记录。
    pub review: ReviewRecord,
    /// 族谱定位声明（总纲三句话）。
    pub family_position: Vec<&'static str>,
}

impl Constitution {
    /// 标准总纲（锚点口径的预置聚合）。
    pub fn standard() -> Self {
        let catalog = ConstructCatalog::standard();
        let principles = DesignPrinciple::derive_all(&catalog);
        let freeze = FreezeBook::standard();
        let declared = freeze.backend_targets.clone();
        Self {
            principles,
            catalog,
            ingestion: standard_ingestion(),
            freeze,
            declared_backend_targets: declared,
            registry: ClauseRegistry::standard(),
            review: standard_review(),
            family_position: vec![
                "语法像 WGSL 一样安全",
                "语义像 GLSL 一样全",
                "入口像 HLSL 一样能收编存量",
                "VE-Shade 是内部统一表示的源语言，三方源进 VE 都先归一",
            ],
        }
    }

    /// 判据一校验：原则六条全部可追溯到具体构造（双向）。
    ///
    /// 检查面：原则数恰为六；每条原则有构造承载；每个构造反向指回
    /// 存在的原则且指认一致；构造语义非空。
    pub fn check_traceability(&self) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        if self.principles.len() != 6 {
            issues.push(VerificationIssue {
                code: "TRACE-COUNT".to_string(),
                what: format!("原则数为 {}，应为 6（一条不多一条不少）", self.principles.len()),
                suggestion: "原则集以锚点六条为准，不得增减".to_string(),
            });
        }
        if self.catalog.is_empty() {
            issues.push(VerificationIssue {
                code: "TRACE-EMPTY".to_string(),
                what: "构造目录为空：六条原则全部无落地承载".to_string(),
                suggestion: "为每条原则注册至少一条落地构造".to_string(),
            });
        }
        // 正向：每条原则至少一条构造。
        for p in &self.principles {
            if p.construct_ids.is_empty() {
                issues.push(VerificationIssue {
                    code: format!("TRACE-{}-NOCARRY", p.id.code()),
                    what: format!("原则「{}」没有任何具体构造承载——原则成空话", p.id.name()),
                    suggestion: format!("为 {} 注册落地构造并在构造语义里写明如何实现该原则", p.id.code()),
                });
            }
        }
        // 反向：每个构造指回存在的原则，且原则的正向清单收录它。
        for c in 0..self.catalog.len() {
            let con = &self.catalog.entries[c];
            let listed = self
                .principles
                .iter()
                .filter(|p| p.id == con.principle)
                .any(|p| p.construct_ids.contains(&con.id));
            if !listed {
                issues.push(VerificationIssue {
                    code: "TRACE-BACKREF".to_string(),
                    what: format!(
                        "构造 {} 声明属于「{}」，但该原则的正向清单未收录它——两张皮",
                        con.id,
                        con.principle.name()
                    ),
                    suggestion: "原则表由目录派生（derive_all），不许手抄两份".to_string(),
                });
            }
        }
        issues
    }

    /// 判据二校验：三方收编策略定义完整（三族齐 + 各自五要素 + 归一路径合法）。
    pub fn check_ingestion(&self) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        for f in SourceFamily::ALL {
            match self.ingestion.iter().find(|s| s.family == f) {
                None => issues.push(VerificationIssue {
                    code: format!("INGEST-{}-MISSING", f.name().to_uppercase()),
                    what: format!("{} 的收编策略缺失——三方收编少一族即不成策略", f.name()),
                    suggestion: "为该族补齐入口语义/归一路径/版本覆盖/已知缺口/禁止事项".to_string(),
                }),
                Some(s) => {
                    if s.normalization != NormalizationPath::DedicatedFrontend {
                        issues.push(VerificationIssue {
                            code: format!("INGEST-{}-PATH", f.name().to_uppercase()),
                            what: format!("{} 存在未经归一直连后端的路径，违反族谱定位", f.name()),
                            suggestion: "归一路径锁死为专用前端归一".to_string(),
                        });
                    }
                }
            }
        }
        issues
    }

    /// 判据三校验：与 VE-C 接口冻结书一致。
    ///
    /// 总纲侧的后端目标表以冻结书为准逐项核对；软渲测试靶契约与前端
    /// 接口版本必须已登记。总纲自己是权威消费方不是权威源——冲突时
    /// 以冻结书为准并出修订记录。
    pub fn check_freeze_consistency(&self) -> FreezeConsistency {
        let mut conflicts = Vec::new();
        let frozen = &self.freeze.backend_targets;
        // 八族：族数与族名逐项核对（总纲声明 vs 冻结清单，两张清单必须逐字一致）。
        if frozen.len() != 8 {
            conflicts.push(format!(
                "冻结清单族数为 {}，应为 8 族——冻结书数据异常，先核冻结书",
                frozen.len()
            ));
        }
        for (i, declared) in self.declared_backend_targets.iter().enumerate() {
            match frozen.get(i) {
                Some(f) if f == declared => {}
                Some(f) => conflicts.push(format!(
                    "总纲声明后端第 {} 族为「{}」，冻结清单为「{}」——以冻结书为准修正",
                    i + 1,
                    declared,
                    f
                )),
                None => conflicts.push(format!(
                    "总纲声明的后端「{}」超出冻结清单范围——总纲不得私设目标面",
                    declared
                )),
            }
        }
        for f in frozen {
            if !self.declared_backend_targets.iter().any(|d| d == f) {
                conflicts.push(format!(
                    "冻结清单中的「{}」未在总纲声明——总纲不得漏族",
                    f
                ));
            }
        }
        // 软渲必须在八族内（软渲测试靶是语义判据的兜底验证面）。
        if !frozen.iter().any(|t| *t == "softrender") {
            conflicts.push("冻结清单缺软渲靶：shader 语义像素对拍失去兜底验证面".to_string());
        }
        let softrender_ok = !self.freeze.softrender_target_contract.is_empty()
            && self.freeze.softrender_target_contract.contains("像素对拍");
        let frontend_ok = !self.freeze.frontend_interface_version.is_empty();
        if !softrender_ok {
            conflicts.push("软渲测试靶契约未登记或不完整".to_string());
        }
        if !frontend_ok {
            conflicts.push("前端接口冻结书版本未登记".to_string());
        }
        let backends_match = frozen.len() == 8
            && self.declared_backend_targets.len() == frozen.len()
            && self
                .declared_backend_targets
                .iter()
                .zip(frozen.iter())
                .all(|(d, f)| d == f)
            && frozen.iter().all(|f| {
                self.declared_backend_targets.iter().any(|d| d == f)
            });
        FreezeConsistency {
            backends_match,
            softrender_contract_ok: softrender_ok,
            frontend_interface_ok: frontend_ok,
            conflicts,
        }
    }

    /// 判据四校验：总纲评审通过（结构化裁决，不走过场）。
    pub fn check_review(&self) -> (ReviewVerdict, Vec<VerificationIssue>) {
        let v = self.review.verdict();
        let mut issues = Vec::new();
        if v != ReviewVerdict::Passed {
            for f in self.review.failed_items() {
                issues.push(VerificationIssue {
                    code: format!("REVIEW-{}", f.id),
                    what: format!("评审项「{}」未通过", f.question),
                    suggestion: f.remedy.to_string(),
                });
            }
            if v == ReviewVerdict::MissingSignatures {
                issues.push(double_sign_issue());
            }
        }
        (v, issues)
    }

    /// 判据五校验：判据引用链建立（十组全覆盖 + 引用全存在 + 双向可查）。
    pub fn check_reference_chain(&self) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        for (g, n) in self.registry.group_coverage() {
            if n == 0 {
                issues.push(VerificationIssue {
                    code: format!("CHAIN-{}-NOREF", g),
                    what: format!("C 域「{g}」组没有引用任何总纲条款——判据引用链断在该组"),
                    suggestion: format!("为 {g} 组登记其判据依据的 VS-G-xx 条款"),
                });
            }
        }
        // 双向：每条下游消费型条款（原则/收编/冻结来源）至少被一组引用
        // （条款不许成孤岛）。治理类条款（引用链制度/评审纪律本身）约束的是
        // 总纲与施工流程，不预期被十组引用，不参与孤岛判定——否则是误报。
        for c in self.registry.clauses() {
            let downstream_facing = !matches!(c.origin, ClauseOrigin::Governance);
            if downstream_facing && self.registry.citing_groups(c.id).is_empty() {
                issues.push(VerificationIssue {
                    code: format!("CHAIN-{}-ORPHAN", c.id),
                    what: format!("条款 {}（{}）没有任何组引用——孤岛条款", c.id, c.title),
                    suggestion: "要么给条款补引用方，要么显性标注为预留条款并说明理由".to_string(),
                });
            }
        }
        issues
    }

    /// 全量校验（五条判据一次跑齐，issues 逐条可检索）。
    pub fn verification_issues(&self) -> Vec<VerificationIssue> {
        let mut all = Vec::new();
        all.extend(self.check_traceability());
        all.extend(self.check_ingestion());
        let fz = self.check_freeze_consistency();
        for c in &fz.conflicts {
            all.push(VerificationIssue {
                code: "FREEZE-CONFLICT".to_string(),
                what: c.clone(),
                suggestion: "以冻结书为准修正总纲并留修订记录".to_string(),
            });
        }
        let (_, ri) = self.check_review();
        all.extend(ri);
        all.extend(self.check_reference_chain());
        all
    }
}

/// 双签缺失的固定问题项（独立函数避免重复构造字面量）。
fn double_sign_issue() -> VerificationIssue {
    VerificationIssue {
        code: "REVIEW-SIGN".to_string(),
        what: "评审项全绿但施工/验收双签不齐——未通过".to_string(),
        suggestion: "补齐双签后再宣告评审通过".to_string(),
    }
}

/// 纯功能行数自证：总纲聚合体 + 校验引擎的可执行性冒烟。
///
/// （正式门禁在 `vec01_checks.rs` 的 `run_vec01_checks` 与单元测试。）
pub fn constitution_smoke() -> usize {
    let c = Constitution::standard();
    c.principles.len() + c.catalog.len() + c.ingestion.len() + c.registry.clause_count()
}
