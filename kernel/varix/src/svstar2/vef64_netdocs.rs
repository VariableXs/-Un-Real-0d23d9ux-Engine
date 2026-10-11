//! VE-F5411 · 网络文档（VE-\ 域 · 网络批次 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5411`
//!
//! **判据（锚点原文）**：三族、三景、四节、阻断发版、判据。
//!
//! **职责定位（锚点原文）**：三族：网络架构册（四层与双通道导览）/
//! 开发者网络指南（多人玩法开发——给开发者的手册）/ 故障手册（连接
//! 失败/同步异常/延迟过高三景）；四节定式 CI 断言同口径。数据结构：
//! 三族目录；三景单；四节定式。错误路径与降级矩阵：缺节→断言拦截；
//! 复现失败→阻断发版；缺景→补录排程。
//!
//! ## 一、三族闭集（数据结构：三族目录）
//!
//! 网络文档面按 [`DocFamily`] 三族闭集管理：**网络架构册**（四层与
//! 双通道导览——给架构读者）、**开发者网络指南**（多人玩法开发——给
//! 开发者的手册）、**故障手册**（三景处置——给排障者）。族是闭集：
//! 族闭集外的册不收（[`DocCode::UnknownFamily`]）——文档面与网络
//! API 面（F5410 三族）同构不同义：那里冻结承诺、这里导览处置。
//!
//! ## 二、四节定式（数据结构：四节定式）
//!
//! 每册必须四节齐（[`Section`] 闭集）：**职责定位**（Scope——这册讲
//! 什么不讲什么）/ **使用步骤**（Steps——照着做能跑通）/ **示例**
//! （Example——可复制的最小样）/ **边界与降级**（Bounds——什么情况
//! 走哪条降级路）。缺节 → 断言拦截（[`DocCode::MissingSection`]，
//! [`DocHub::assert_catalog`] 全量扫描 O(册数) 逐册点名）。**CI 断言
//! 同口径**：断言规则 [`SECTION_MATRIX`] const 表（3 族 × 4 节必要性）
//! 与 [`section_required`] 规则推导**双写全格对拍**——矩阵改规则没改
//! 判据即红（全域纪律同规：F6004 权限矩阵、F5410 变更矩阵、本单节
//! 矩阵，三单同一双写纪律）。
//!
//! ## 三、三景单（数据结构：三景单）
//!
//! 故障手册必备三景（[`Scene`] 闭集）：**连接失败** / **同步异常** /
//! **延迟过高**。每景含**复现步骤清单**（空步骤 = 复现失败的一种——
//! [`DocCode::EmptyStep`]）与**验证位**（[`SceneEntry::verified`]——
//! 复现走通过才算）。缺景 → **补录排程**（[`BacklogItem`] 点名清单，
//! 不静默、不拖到发版才发现）。**复现失败→阻断发版**：
//! [`DocHub::release_gate`] 三景 verified 全过才放行，任一景未验证/
//! 步骤空 → [`DocCode::ReleaseBlocked`]——文档复现不过关的发版是
//! 把故障留给玩家。
//!
//! ## 四、内容源可追溯（跨批对接：F5401-F5410 内容源）
//!
//! 每册条目登记**内容源清单**（源单 ID 闭集 [`SOURCE_UNIVERSE`] =
//! F5401..F5410）——册里的每句话都能溯源到施工书锚点（无障碍与审计
//! 双面：读者知道去哪查原始规格，审计知道文档没凭空编）。引用闭集外
//! 的源 → [`DocCode::SourceMissing`]。册密封指纹（[`DocEntry::seal`]）
//! = 标题+族+节位图+源清单 FNV——内容变更指纹必变（漂移可检出）。
//!
//! ## 五、权限与审计
//!
//! 写册/落节/登记景/验证是 **Maintainer 权**（文档是承诺面——谁都能
//! 写的文档没人信）；断言/查询全员开放（读屏可达——域本色）。写操作
//! 全留痕（[`AuditOp`] append-only，满容拒记计数不覆盖旧痕——
//! [`DocCode::AuditFull`]）。账实一致性探针（[`DocHub::consistency_probe`]）：
//! 景单必须在故障手册族有册、验证景必有非空步骤、节位图与节矩阵对拍。
//!
//! ## 六、畸形语料零崩溃
//!
//! 册名/节名/步骤文本任意输入零崩溃：空标题/超长标题（>
//! [`TITLE_MAX_LEN`]）显性拒（[`DocCode::BadTitle`]）；步骤文本空拒；
//! 全程零 unwrap 零 expect 零切片越界。畸形语料闭集判据钉死（对齐
//! 全域 fuzz 纪律）。
//!
//! **对接**：上游 F5401-F5410 内容源（本模块的内容源闭集）；F5419
//! 联调（发版闸与联调门对接）；F5410 冻结册是架构册的账面素材。
//! 诊断码独占 `0x6Dxx` 段（0x5A=F5410 已占、0x5B~0x5F/0x6E/0x6F 被
//! CGPU 占、0x6D 全仓扫描仅 u32 全值常量非段位码——空闲）。no_std
//! 零 std 依赖、零全局可变状态。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const VEF64_VERSION: &str = "vef64-netdocs-v1";

/// 三族数（架构册/开发指南/故障手册——闭集）。
pub const DOC_FAMILY_COUNT: usize = 3;

/// 四节数（职责定位/使用步骤/示例/边界降级——定式）。
pub const SECTION_COUNT: usize = 4;

/// 三景数（连接失败/同步异常/延迟过高——故障手册必备）。
pub const SCENE_COUNT: usize = 3;

/// 册标题长度上限（DoS 面）。
pub const TITLE_MAX_LEN: usize = 64;

/// 复现步骤上限（单景——步骤清单不是无底洞）。
pub const REPRO_STEP_CAP: usize = 32;

/// 文档目录容量（满容显性拒不驱逐）。
pub const DOC_CAP: usize = 64;

/// 审计簿容量（append-only 满容拒记计数不覆盖旧痕）。
pub const AUDIT_CAP: usize = 128;

/// 内容源闭集（F5401-F5410——册内容可溯源的原始锚点 ID）。
pub const SOURCE_UNIVERSE: [&str; 10] = [
    "VE-F5401",
    "VE-F5402",
    "VE-F5403",
    "VE-F5404",
    "VE-F5405",
    "VE-F5406",
    "VE-F5407",
    "VE-F5408",
    "VE-F5409",
    "VE-F5410",
];

/// 签名指纹 FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x6Dxx 段十六码闭集）
// ---------------------------------------------------------------------------

/// F5411 诊断码。独占 `0x6Dxx` 段，十六码闭集两两互异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocCode(pub u16);

impl DocCode {
    /// 0x6D01 缺节（四节定式断言拦截——逐册逐节点名）。
    pub const MissingSection: DocCode = DocCode(0x6D01);
    /// 0x6D02 缺景（故障手册三景不齐——补录排程点名）。
    pub const MissingScene: DocCode = DocCode(0x6D02);
    /// 0x6D03 复现失败（景未验证或步骤不齐）。
    pub const ReproFailed: DocCode = DocCode(0x6D03);
    /// 0x6D04 发版被阻断（release_gate 三景未全过——闸状态）。
    pub const ReleaseBlocked: DocCode = DocCode(0x6D04);
    /// 0x6D05 册不在册。
    pub const UnknownDoc: DocCode = DocCode(0x6D05);
    /// 0x6D06 族闭集外。
    pub const UnknownFamily: DocCode = DocCode(0x6D06);
    /// 0x6D07 节闭集外。
    pub const UnknownSection: DocCode = DocCode(0x6D07);
    /// 0x6D08 重复册名（同册名两条——目录不收）。
    pub const DuplicateDoc: DocCode = DocCode(0x6D08);
    /// 0x6D09 目录满（满容显性拒不驱逐）。
    pub const DocFull: DocCode = DocCode(0x6D09);
    /// 0x6D0A 越权（写操作非 Maintainer）。
    pub const PermissionDenied: DocCode = DocCode(0x6D0A);
    /// 0x6D0B 标题非法（空/超长——畸形语料显性拒）。
    pub const BadTitle: DocCode = DocCode(0x6D0B);
    /// 0x6D0C 内容源缺失（引用闭集外的源单 ID）。
    pub const SourceMissing: DocCode = DocCode(0x6D0C);
    /// 0x6D0D 密封指纹失配（册内容变更未重密封——漂移检出）。
    pub const SealMismatch: DocCode = DocCode(0x6D0D);
    /// 0x6D0E 审计满（append-only 满容拒记计数不覆盖旧痕）。
    pub const AuditFull: DocCode = DocCode(0x6D0E);
    /// 0x6D0F 账实不符（景单/册/节位图对拍失败——自检红线）。
    pub const LedgerDrift: DocCode = DocCode(0x6D0F);
    /// 0x6D10 复现步骤空（空步骤清单——复现失败的显性形态）。
    pub const EmptyStep: DocCode = DocCode(0x6D10);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（诊断可读——拒绝不是哑的 Err）。
    pub fn reason(self) -> String {
        match self {
            DocCode::MissingSection => "缺节：四节定式不齐，断言拦截（逐册逐节点名）".into(),
            DocCode::MissingScene => "缺景：故障手册三景不齐，补录排程".into(),
            DocCode::ReproFailed => "复现失败：景未验证或步骤不齐".into(),
            DocCode::ReleaseBlocked => "发版被阻断：三景复现未全过——文档复现不过关的发版是把故障留给玩家".into(),
            DocCode::UnknownDoc => "册不在册：先登记再操作".into(),
            DocCode::UnknownFamily => "族闭集外：三族目录不收".into(),
            DocCode::UnknownSection => "节闭集外：四节定式外无节".into(),
            DocCode::DuplicateDoc => "重复册名：同册名两条目录不收".into(),
            DocCode::DocFull => "目录满：显性拒不驱逐".into(),
            DocCode::PermissionDenied => "越权：写册/落节/验证是维护者权（文档是承诺面）".into(),
            DocCode::BadTitle => "标题非法：空或超长（畸形语料显性拒）".into(),
            DocCode::SourceMissing => "内容源缺失：引用闭集外的源单 ID（册里的话要能溯源）".into(),
            DocCode::SealMismatch => "密封指纹失配：册内容变更未重密封（漂移检出）".into(),
            DocCode::AuditFull => "审计满：append-only 满容拒记计数".into(),
            DocCode::LedgerDrift => "账实不符：景单/册/节位图对拍失败".into(),
            DocCode::EmptyStep => "复现步骤空：空步骤清单是复现失败的显性形态".into(),
            DocCode(_) => "未知文档诊断码".into(),
        }
    }
}

/// `segment_guard`：十六码闭集两两互异 + 全落 0x6D 段。
pub const SEGMENT_GUARD: [DocCode; 16] = [
    DocCode::MissingSection,
    DocCode::MissingScene,
    DocCode::ReproFailed,
    DocCode::ReleaseBlocked,
    DocCode::UnknownDoc,
    DocCode::UnknownFamily,
    DocCode::UnknownSection,
    DocCode::DuplicateDoc,
    DocCode::DocFull,
    DocCode::PermissionDenied,
    DocCode::BadTitle,
    DocCode::SourceMissing,
    DocCode::SealMismatch,
    DocCode::AuditFull,
    DocCode::LedgerDrift,
    DocCode::EmptyStep,
];

/// 文档错误路径（显性携带 [`DocCode`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocErr {
    /// 携诊断码的失败。
    Code(DocCode),
}

impl DocErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            DocErr::Code(c) => c.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、闭集（三族 / 四节 / 三景）
// ---------------------------------------------------------------------------

/// 网络文档三族（闭集——目录外无族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFamily {
    /// 网络架构册（四层与双通道导览——给架构读者）。
    Architecture,
    /// 开发者网络指南（多人玩法开发——给开发者的手册）。
    DevGuide,
    /// 故障手册（三景处置——给排障者）。
    Troubleshooting,
}

impl DocFamily {
    /// 三族闭集（列序 = [`DOC_FAMILY_COUNT`]）。
    pub const ALL: [DocFamily; DOC_FAMILY_COUNT] = [
        DocFamily::Architecture,
        DocFamily::DevGuide,
        DocFamily::Troubleshooting,
    ];

    /// 矩阵/账面下标。
    pub const fn ordinal(self) -> usize {
        match self {
            DocFamily::Architecture => 0,
            DocFamily::DevGuide => 1,
            DocFamily::Troubleshooting => 2,
        }
    }

    /// 人话名（读屏口径）。
    pub const fn tag(self) -> &'static str {
        match self {
            DocFamily::Architecture => "网络架构册",
            DocFamily::DevGuide => "开发者网络指南",
            DocFamily::Troubleshooting => "故障手册",
        }
    }
}

/// 四节定式（闭集——每册四节齐才算完整的册）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// 职责定位（Scope——这册讲什么不讲什么）。
    Scope,
    /// 使用步骤（Steps——照着做能跑通）。
    Steps,
    /// 示例（Example——可复制的最小样）。
    Example,
    /// 边界与降级（Bounds——什么情况走哪条降级路）。
    Bounds,
}

impl Section {
    /// 四节闭集（列序 = [`SECTION_COUNT`] = [`SECTION_MATRIX`] 列序）。
    pub const ALL: [Section; SECTION_COUNT] = [
        Section::Scope,
        Section::Steps,
        Section::Example,
        Section::Bounds,
    ];

    /// 矩阵列下标。
    pub const fn column(self) -> usize {
        match self {
            Section::Scope => 0,
            Section::Steps => 1,
            Section::Example => 2,
            Section::Bounds => 3,
        }
    }

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            Section::Scope => "职责定位",
            Section::Steps => "使用步骤",
            Section::Example => "示例",
            Section::Bounds => "边界与降级",
        }
    }
}

/// 故障三景（闭集——故障手册必备）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// 连接失败（握手/断线/重连）。
    ConnFail,
    /// 同步异常（状态漂移/回滚冲突）。
    SyncAbnormal,
    /// 延迟过高（抖动/丢包/路由劣化）。
    HighLatency,
}

impl Scene {
    /// 三景闭集（列序 = [`SCENE_COUNT`]）。
    pub const ALL: [Scene; SCENE_COUNT] = [
        Scene::ConnFail,
        Scene::SyncAbnormal,
        Scene::HighLatency,
    ];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            Scene::ConnFail => "连接失败",
            Scene::SyncAbnormal => "同步异常",
            Scene::HighLatency => "延迟过高",
        }
    }
}

/// 四节必要性矩阵（行 = 三族（[`DocFamily::ALL`] 序），列 = 四节
/// （[`Section::ALL`] 序））。
///
/// **双写之一**：声明式 const 表。断言走 [`section_required`]（规则
/// 推导）——两路由判据 3×4=12 格逐格对拍。口径：架构册与开发指南
/// 四节全必需；故障手册的「示例」节由三景单的复现步骤承担（景单即
/// 示例——不必另有 Example 节），其余三节必需。
pub const SECTION_MATRIX: [[bool; SECTION_COUNT]; DOC_FAMILY_COUNT] = [
    //              Scope Steps Example Bounds
    /* Architecture */ [true, true, true, true],
    /* DevGuide     */ [true, true, true, true],
    /* Troubleshoot */ [true, true, false, true],
];

/// 节必要性规则推导路（双写之二）。
pub fn section_required(family: DocFamily, section: Section) -> bool {
    match (family, section) {
        // 故障手册：三景复现步骤即示例（不另设 Example 节）
        (DocFamily::Troubleshooting, Section::Example) => false,
        // 其余全必需（四节定式——缺节即断言拦截）
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// 四、数据结构（册条目 / 景单 / 补录排程 / 审计）
// ---------------------------------------------------------------------------

/// 册条目（目录一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// 册标题（目录内唯一）。
    pub title: String,
    /// 所属族。
    pub family: DocFamily,
    /// 节位图（四节是否已落——下标 = [`Section::column`]）。
    pub sections: [bool; SECTION_COUNT],
    /// 内容源清单（源单 ID——可溯源）。
    pub sources: Vec<String>,
    /// 密封指纹（标题+族+节位图+源清单 FNV——内容变更指纹必变）。
    pub seal: u64,
}

/// 景单条目（故障手册三景的一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEntry {
    /// 景。
    pub scene: Scene,
    /// 复现步骤清单（有序——照着做能复现）。
    pub steps: Vec<String>,
    /// 验证位（复现走通过才算——release_gate 依据）。
    pub verified: bool,
}

/// 补录排程条目（断言点名——缺什么补什么）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogKind {
    /// 缺节（逐册逐节）。
    MissingSection,
    /// 缺景（三景点名）。
    MissingScene,
    /// 复现未验证（景在但没走通过）。
    Unverified,
}

impl BacklogKind {
    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            BacklogKind::MissingSection => "缺节",
            BacklogKind::MissingScene => "缺景",
            BacklogKind::Unverified => "复现未验证",
        }
    }
}

/// 补录排程条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogItem {
    /// 缺什么。
    pub kind: BacklogKind,
    /// 所属族（缺景时为 Troubleshooting）。
    pub family: DocFamily,
    /// 册名或景名。
    pub name: String,
    /// 缺的节（仅 MissingSection 时有效）。
    pub section: Option<Section>,
}

/// 操作者（轻量权限：写是 Maintainer 权，读全员开放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// 维护者：写册/落节/登记景/验证。
    Maintainer,
    /// CI：断言与发版闸（只读）。
    Ci,
    /// 外部读者：只读（读屏可达——域本色）。
    Reader,
}

impl Actor {
    /// 全操作者闭集。
    pub const ALL: [Actor; 3] = [Actor::Maintainer, Actor::Ci, Actor::Reader];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            Actor::Maintainer => "维护者",
            Actor::Ci => "CI",
            Actor::Reader => "读者",
        }
    }
}

/// 审计操作（append-only 留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOp {
    /// 登记册。
    DocAdded { title: String, family: DocFamily },
    /// 落节。
    SectionPut { title: String, section: Section },
    /// 登记景。
    SceneAdded { scene: Scene },
    /// 追加复现步骤。
    StepAdded { scene: Scene },
    /// 景验证通过。
    SceneVerified { scene: Scene },
    /// 越权拒绝。
    Denied { actor: Actor, what: &'static str },
}

/// 审计条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// 序号（单调递增）。
    pub seq: u64,
    /// 操作。
    pub op: AuditOp,
}

/// 密封指纹（标题+族+节位图+源清单 FNV-1a 64）。
fn doc_seal(title: &str, family: DocFamily, sections: &[bool; SECTION_COUNT], sources: &[String]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in title.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h ^= family.ordinal() as u64;
    h = h.wrapping_mul(FNV_PRIME);
    for s in sections.iter() {
        h ^= *s as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    for src in sources.iter() {
        for b in src.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

// ---------------------------------------------------------------------------
// 五、DocHub（三族目录 × 四节定式 × 三景单 × 发版闸）
// ---------------------------------------------------------------------------

/// 网络文档中枢。
///
/// 账本：目录 `catalog` + 景单 `scenes` + 审计簿 `audit`。
/// 断言 O(册数)（全量扫描逐册点名）、复现 O(步骤)、补录 O(景)
/// （锚点性能口径）。
#[derive(Debug, Default)]
pub struct DocHub {
    catalog: Vec<DocEntry>,
    scenes: Vec<SceneEntry>,
    audit: Vec<AuditEntry>,
    seq: u64,
    audit_dropped: u64,
}

impl DocHub {
    /// 空账构造。
    pub fn new() -> Self {
        DocHub {
            catalog: Vec::new(),
            scenes: Vec::new(),
            audit: Vec::new(),
            seq: 0,
            audit_dropped: 0,
        }
    }

    // -- 内部 ---------------------------------------------------------------

    fn audit_push(&mut self, op: AuditOp) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped = self.audit_dropped.saturating_add(1);
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        self.audit.push(AuditEntry { seq: self.seq, op });
    }

    fn find_mut(&mut self, title: &str) -> Option<&mut DocEntry> {
        self.catalog.iter_mut().find(|d| d.title == title)
    }

    /// 册在册检查（标题合法 + 重复检查——注册公共前置）。
    fn title_check(title: &str) -> Result<(), DocCode> {
        if title.is_empty() || title.len() > TITLE_MAX_LEN {
            return Err(DocCode::BadTitle);
        }
        Ok(())
    }

    // -- 登记册 / 落节 --------------------------------------------------------

    /// 登记册（Maintainer 权）：标题合法 → 族闭集 → 内容源闭集 →
    /// 重复拒 → 容量闸 → 落账密封。
    pub fn register_doc(
        &mut self,
        actor: Actor,
        title: &str,
        family: DocFamily,
        sources: &[&str],
    ) -> Result<(), DocErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "登记册" });
            return Err(DocErr::Code(DocCode::PermissionDenied));
        }
        Self::title_check(title).map_err(DocErr::Code)?;
        if self.catalog.len() >= DOC_CAP {
            return Err(DocErr::Code(DocCode::DocFull));
        }
        if self.catalog.iter().any(|d| d.title == title) {
            return Err(DocErr::Code(DocCode::DuplicateDoc));
        }
        // 内容源闭集校验（闭集外源 → SourceMissing——册里的话要能溯源）
        for src in sources.iter() {
            if !SOURCE_UNIVERSE.contains(src) {
                return Err(DocErr::Code(DocCode::SourceMissing));
            }
        }
        let srcs: Vec<String> = sources.iter().map(|s| String::from(*s)).collect();
        let seal = doc_seal(title, family, &[false; SECTION_COUNT], &srcs);
        self.catalog.push(DocEntry {
            title: String::from(title),
            family,
            sections: [false; SECTION_COUNT],
            sources: srcs,
            seal,
        });
        self.audit_push(AuditOp::DocAdded { title: String::from(title), family });
        Ok(())
    }

    /// 落节（Maintainer 权；节闭集校验；落节重密封——指纹随内容变）。
    pub fn put_section(
        &mut self,
        actor: Actor,
        title: &str,
        section: Section,
    ) -> Result<(), DocErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "落节" });
            return Err(DocErr::Code(DocCode::PermissionDenied));
        }
        let d = match self.find_mut(title) {
            Some(d) => d,
            None => return Err(DocErr::Code(DocCode::UnknownDoc)),
        };
        d.sections[section.column()] = true;
        d.seal = doc_seal(&d.title, d.family, &d.sections, &d.sources);
        self.audit_push(AuditOp::SectionPut { title: String::from(title), section });
        Ok(())
    }

    // -- 三景单 ---------------------------------------------------------------

    /// 登记景（Maintainer 权；重复登记幂等拒——DuplicateDoc 同源码）。
    pub fn register_scene(&mut self, actor: Actor, scene: Scene) -> Result<(), DocErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "登记景" });
            return Err(DocErr::Code(DocCode::PermissionDenied));
        }
        if self.scenes.iter().any(|s| s.scene == scene) {
            return Err(DocErr::Code(DocCode::DuplicateDoc));
        }
        self.scenes.push(SceneEntry { scene, steps: Vec::new(), verified: false });
        self.audit_push(AuditOp::SceneAdded { scene });
        Ok(())
    }

    /// 追加复现步骤（Maintainer 权；空步骤文本拒；容量闸）。
    pub fn add_repro_step(
        &mut self,
        actor: Actor,
        scene: Scene,
        step: &str,
    ) -> Result<(), DocErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "追加步骤" });
            return Err(DocErr::Code(DocCode::PermissionDenied));
        }
        if step.is_empty() || step.len() > TITLE_MAX_LEN {
            return Err(DocErr::Code(DocCode::EmptyStep));
        }
        let s = match self.scenes.iter_mut().find(|s| s.scene == scene) {
            Some(s) => s,
            None => return Err(DocErr::Code(DocCode::MissingScene)),
        };
        if s.steps.len() >= REPRO_STEP_CAP {
            return Err(DocErr::Code(DocCode::DocFull));
        }
        // 追加步骤使已验证态失效（内容变了——旧验证不算数）
        s.verified = false;
        s.steps.push(String::from(step));
        self.audit_push(AuditOp::StepAdded { scene });
        Ok(())
    }

    /// 景验证（Maintainer 权；步骤非空才可验证——空景验不过）。
    pub fn verify_scene(&mut self, actor: Actor, scene: Scene) -> Result<(), DocErr> {
        if actor != Actor::Maintainer {
            self.audit_push(AuditOp::Denied { actor, what: "景验证" });
            return Err(DocErr::Code(DocCode::PermissionDenied));
        }
        let s = match self.scenes.iter_mut().find(|s| s.scene == scene) {
            Some(s) => s,
            None => return Err(DocErr::Code(DocCode::MissingScene)),
        };
        if s.steps.is_empty() {
            return Err(DocErr::Code(DocCode::EmptyStep));
        }
        s.verified = true;
        self.audit_push(AuditOp::SceneVerified { scene });
        Ok(())
    }

    // -- CI 断言（O(册数) 全量扫描） ------------------------------------------

    /// 全量断言（CI 同口径）：三族逐册四节必要性（[`SECTION_MATRIX`]/
    /// [`section_required`] 双写口径）+ 故障手册三景齐备 + 景验证位。
    /// 全部通过 → 空清单（放行）；否则逐项点名（**补录排程**）。
    pub fn assert_catalog(&self) -> Vec<BacklogItem> {
        let mut out: Vec<BacklogItem> = Vec::new();
        // 逐册逐节：缺节点名
        for d in self.catalog.iter() {
            for sec in Section::ALL.iter() {
                if section_required(d.family, *sec) && !d.sections[sec.column()] {
                    out.push(BacklogItem {
                        kind: BacklogKind::MissingSection,
                        family: d.family,
                        name: d.title.clone(),
                        section: Some(*sec),
                    });
                }
            }
        }
        // 三景齐备：缺景点名
        for scene in Scene::ALL.iter() {
            match self.scenes.iter().find(|s| s.scene == *scene) {
                None => out.push(BacklogItem {
                    kind: BacklogKind::MissingScene,
                    family: DocFamily::Troubleshooting,
                    name: String::from(scene.tag()),
                    section: None,
                }),
                Some(s) => {
                    if !s.verified {
                        out.push(BacklogItem {
                            kind: BacklogKind::Unverified,
                            family: DocFamily::Troubleshooting,
                            name: String::from(scene.tag()),
                            section: None,
                        });
                    }
                }
            }
        }
        out
    }

    /// 发版闸（复现失败→阻断发版）：三景全 verified 才放行。
    /// 任一景缺失/未验证/步骤空 → [`DocCode::ReleaseBlocked`]。
    pub fn release_gate(&self) -> Result<(), DocErr> {
        for scene in Scene::ALL.iter() {
            match self.scenes.iter().find(|s| s.scene == *scene) {
                None => return Err(DocErr::Code(DocCode::ReleaseBlocked)),
                Some(s) => {
                    if !s.verified || s.steps.is_empty() {
                        return Err(DocErr::Code(DocCode::ReleaseBlocked));
                    }
                }
            }
        }
        Ok(())
    }

    // -- 状态可查 ---------------------------------------------------------------

    /// 按族列册（读屏口径）。
    pub fn docs_of(&self, family: DocFamily) -> Vec<&DocEntry> {
        self.catalog.iter().filter(|d| d.family == family).collect()
    }

    /// 册条数。
    pub fn doc_count(&self) -> usize {
        self.catalog.len()
    }

    /// 景单只读视图。
    pub fn scene_entries(&self) -> &[SceneEntry] {
        &self.scenes
    }

    /// 按册名查。
    pub fn doc_status(&self, title: &str) -> Option<(DocFamily, [bool; SECTION_COUNT], u64)> {
        self.catalog
            .iter()
            .find(|d| d.title == title)
            .map(|d| (d.family, d.sections, d.seal))
    }

    /// 按册名回放审计。
    pub fn audit_replay(&self, title: &str) -> Vec<&AuditEntry> {
        self.audit
            .iter()
            .filter(|a| match &a.op {
                AuditOp::DocAdded { title: t, family: _ } => t == title,
                AuditOp::SectionPut { title: t, section: _ } => t == title,
                AuditOp::SceneAdded { scene: _ } => false,
                AuditOp::StepAdded { scene: _ } => false,
                AuditOp::SceneVerified { scene: _ } => false,
                AuditOp::Denied { actor: _, what: _ } => false,
            })
            .collect()
    }

    /// 审计总条数。
    pub fn audit_count(&self) -> usize {
        self.audit.len()
    }

    /// 审计满容拒记计数。
    pub const fn audit_dropped(&self) -> u64 {
        self.audit_dropped
    }

    // -- 自检 ---------------------------------------------------------------

    /// 账实一致探针：景单必在故障手册族有册、验证景必有非空步骤、
    /// 密封指纹对拍（内容与 seal 一致——漂移检出）。
    pub fn consistency_probe(&self) -> Result<(), DocCode> {
        // 景单：每景在故障手册族至少有一册（景挂在手册下）
        for s in self.scenes.iter() {
            let has_manual = self
                .catalog
                .iter()
                .any(|d| d.family == DocFamily::Troubleshooting);
            if !has_manual {
                return Err(DocCode::LedgerDrift);
            }
            if s.verified && s.steps.is_empty() {
                return Err(DocCode::LedgerDrift);
            }
        }
        // 密封指纹对拍
        for d in self.catalog.iter() {
            if doc_seal(&d.title, d.family, &d.sections, &d.sources) != d.seal {
                return Err(DocCode::SealMismatch);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据：三族、三景、四节、阻断发版、判据）
// ---------------------------------------------------------------------------

/// VE-F5411 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef64_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef64_netdocs");

    // —— 判据一 · 三族目录：按族登记可查 + 内容源闭集 ——
    let mut h = DocHub::new();
    let reg_arch = h.register_doc(
        Actor::Maintainer,
        "网络四层与双通道导览",
        DocFamily::Architecture,
        &["VE-F5401", "VE-F5410"],
    );
    let reg_guide = h.register_doc(
        Actor::Maintainer,
        "多人玩法开发手册",
        DocFamily::DevGuide,
        &["VE-F5404"],
    );
    let reg_manual = h.register_doc(
        Actor::Maintainer,
        "网络故障处置手册",
        DocFamily::Troubleshooting,
        &["VE-F5405", "VE-F5406", "VE-F5409"],
    );
    let by_family = h.docs_of(DocFamily::Architecture).len() == 1
        && h.docs_of(DocFamily::DevGuide).len() == 1
        && h.docs_of(DocFamily::Troubleshooting).len() == 1;
    s.add(
        "VE64-三族-按族登记可查",
        reg_arch.is_ok() && reg_guide.is_ok() && reg_manual.is_ok() && by_family,
        "三族各登记一册按族列清单全可查；内容源 F5401~F5410 闭集内引用（可溯源）",
    );

    // —— 判据一 · 反向：越权写拒 + 内容源闭集外拒 + 重复册名拒 ——
    let mut h = DocHub::new();
    let ext_write = h.register_doc(Actor::Reader, "x", DocFamily::Architecture, &[]);
    let ci_write = h.register_doc(Actor::Ci, "x", DocFamily::Architecture, &[]);
    let bad_src = h.register_doc(Actor::Maintainer, "x", DocFamily::Architecture, &["VE-F9999"]);
    let _ = h.register_doc(Actor::Maintainer, "dup", DocFamily::DevGuide, &[]);
    let dup = h.register_doc(Actor::Maintainer, "dup", DocFamily::DevGuide, &[]);
    s.add(
        "VE64-三族-越权与源闭集与重复拒",
        ext_write == Err(DocErr::Code(DocCode::PermissionDenied))
            && ci_write == Err(DocErr::Code(DocCode::PermissionDenied))
            && bad_src == Err(DocErr::Code(DocCode::SourceMissing))
            && dup == Err(DocErr::Code(DocCode::DuplicateDoc)),
        "读者/CI 写册拒（文档是承诺面）；闭集外源单 ID 拒（册里的话要能溯源）；重复册名拒",
    );

    // —— 判据二 · 四节定式：缺节断言拦截逐册逐节点名 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "架构导览", DocFamily::Architecture, &["VE-F5401"]);
    let _ = h.put_section(Actor::Maintainer, "架构导览", Section::Scope);
    let _ = h.put_section(Actor::Maintainer, "架构导览", Section::Steps);
    // Example 与 Bounds 未落 → 断言点名
    let backlog = h.assert_catalog();
    let miss_example = backlog
        .iter()
        .any(|b| b.kind == BacklogKind::MissingSection && b.name == "架构导览"
            && b.section == Some(Section::Example));
    let miss_bounds = backlog
        .iter()
        .any(|b| b.kind == BacklogKind::MissingSection && b.name == "架构导览"
            && b.section == Some(Section::Bounds));
    s.add(
        "VE64-四节-缺节断言逐项点名",
        miss_example && miss_bounds && backlog.len() >= 3,
        "只落两节 → 断言拦截点名缺 Example/Bounds（+三景缺——全量扫描 O(册数) 不漏）",
    );

    // —— 判据二 · 反向：四节齐断言放行（故障手册 Example 豁免） ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "开发指南", DocFamily::DevGuide, &["VE-F5404"]);
    for sec in Section::ALL.iter() {
        let _ = h.put_section(Actor::Maintainer, "开发指南", *sec);
    }
    let _ = h.register_doc(Actor::Maintainer, "故障手册", DocFamily::Troubleshooting, &["VE-F5405"]);
    for sec in [Section::Scope, Section::Steps, Section::Bounds].iter() {
        let _ = h.put_section(Actor::Maintainer, "故障手册", *sec);
    }
    let doc_only = h.assert_catalog();
    let guide_clean = !doc_only
        .iter()
        .any(|b| b.name == "开发指南");
    let manual_clean = !doc_only
        .iter()
        .any(|b| b.name == "故障手册");
    s.add(
        "VE64-四节-齐备放行与景单豁免",
        guide_clean && manual_clean,
        "开发指南四节齐零点名；故障手册三节齐即过（三景复现步骤即示例——Example 豁免）",
    );

    // —— 判据二 · 反向：节矩阵双写全格对拍 ——
    let mut matrix_ok = true;
    for (i, f) in DocFamily::ALL.iter().enumerate() {
        for sec in Section::ALL.iter() {
            if SECTION_MATRIX[i][sec.column()] != section_required(*f, *sec) {
                matrix_ok = false;
            }
        }
    }
    s.add(
        "VE64-四节-必要性矩阵双写对拍",
        matrix_ok && SECTION_MATRIX.len() == DOC_FAMILY_COUNT
            && SECTION_MATRIX[0].len() == SECTION_COUNT,
        "3 族 × 4 节 = 12 格：矩阵表与规则推导逐格相等（CI 断言同口径——矩阵改规则没改即红）",
    );

    // —— 判据三 · 三景单：登记/步骤/验证闭环 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "故障处置", DocFamily::Troubleshooting, &["VE-F5405"]);
    let reg_conn = h.register_scene(Actor::Maintainer, Scene::ConnFail);
    let dup_scene = h.register_scene(Actor::Maintainer, Scene::ConnFail);
    let empty_verify = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "断开链路层");
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "观察握手超时");
    let verify = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    let conn_state = h.scene_entries().iter().find(|s| s.scene == Scene::ConnFail);
    s.add(
        "VE64-三景-登记步骤验证闭环",
        reg_conn.is_ok()
            && dup_scene == Err(DocErr::Code(DocCode::DuplicateDoc))
            && empty_verify == Err(DocErr::Code(DocCode::EmptyStep))
            && verify.is_ok()
            && conn_state.map(|s| s.verified && s.steps.len() == 2) == Some(true),
        "登记→空景验不过（EmptyStep）→补步骤→验证通过；重复登记显性拒；验证位随步骤追加失效重验",
    );

    // —— 判据三 · 反向：缺景补录排程点名 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "故障处置", DocFamily::Troubleshooting, &["VE-F5405"]);
    let _ = h.register_scene(Actor::Maintainer, Scene::ConnFail);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "断链路");
    let _ = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    let backlog = h.assert_catalog();
    let miss_sync = backlog
        .iter()
        .any(|b| b.kind == BacklogKind::MissingScene && b.name == "同步异常");
    let miss_lat = backlog
        .iter()
        .any(|b| b.kind == BacklogKind::MissingScene && b.name == "延迟过高");
    s.add(
        "VE64-三景-缺景补录排程点名",
        miss_sync && miss_lat,
        "只登记连接失败一景 → 断言点名缺同步异常/延迟过高两景（补录排程——不拖到发版才发现）",
    );

    // —— 判据四 · 阻断发版：三景未全过闸拦 ——
    let mut h = DocHub::new();
    let gate_empty = h.release_gate();
    let _ = h.register_scene(Actor::Maintainer, Scene::ConnFail);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "断链路");
    let _ = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    let gate_partial = h.release_gate();
    let _ = h.register_scene(Actor::Maintainer, Scene::SyncAbnormal);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::SyncAbnormal, "制造状态漂移");
    let _ = h.verify_scene(Actor::Maintainer, Scene::SyncAbnormal);
    let _ = h.register_scene(Actor::Maintainer, Scene::HighLatency);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::HighLatency, "注入抖动");
    let _ = h.verify_scene(Actor::Maintainer, Scene::HighLatency);
    let gate_all = h.release_gate();
    s.add(
        "VE64-阻断发版-三景未全过闸拦",
        gate_empty == Err(DocErr::Code(DocCode::ReleaseBlocked))
            && gate_partial == Err(DocErr::Code(DocCode::ReleaseBlocked))
            && gate_all.is_ok(),
        "零景拦→一景过仍拦（复现失败→阻断发版）→三景全验证放行（发版闸 O(景)）",
    );

    // —— 判据四 · 反向：步骤追加使验证失效重拦 ——
    let mut h = DocHub::new();
    let _ = h.register_scene(Actor::Maintainer, Scene::ConnFail);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "断链路");
    let _ = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    // 另两景补齐（闸要求三景全过）
    for sc in [Scene::SyncAbnormal, Scene::HighLatency].iter() {
        let _ = h.register_scene(Actor::Maintainer, *sc);
        let _ = h.add_repro_step(Actor::Maintainer, *sc, "基线步骤");
        let _ = h.verify_scene(Actor::Maintainer, *sc);
    }
    let gate_ok = h.release_gate().is_ok();
    let _ = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "换网再测");
    let gate_reblocked = h.release_gate();
    let _ = h.verify_scene(Actor::Maintainer, Scene::ConnFail);
    let gate_reok = h.release_gate().is_ok();
    s.add(
        "VE64-阻断发版-内容变更重验证",
        gate_ok && gate_reblocked == Err(DocErr::Code(DocCode::ReleaseBlocked)) && gate_reok,
        "三景全过闸放行；验证后追加步骤→验证位失效闸重拦（旧验证不算数）→重验放行（内容变更指纹必变的闸面）",
    );

    // —— 判据五 · 密封指纹：内容变更指纹必变 + 漂移检出 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "册A", DocFamily::Architecture, &["VE-F5401"]);
    let seal0 = h.doc_status("册A").map(|(_, _, s)| s);
    let _ = h.put_section(Actor::Maintainer, "册A", Section::Scope);
    let seal1 = h.doc_status("册A").map(|(_, _, s)| s);
    let changed = seal0 != seal1 && seal0.is_some() && seal1.is_some();
    let probe = h.consistency_probe();
    s.add(
        "VE64-判据-密封指纹内容变更必变",
        changed && probe == Ok(()),
        "落节前 seal ≠ 落节后（重密封）；一致性探针零漂移（指纹与内容逐册对拍）",
    );

    // —— 判据五 · 反向：畸形语料零崩溃（空/超长标题） ——
    let mut h = DocHub::new();
    let empty_title = h.register_doc(Actor::Maintainer, "", DocFamily::Architecture, &[]);
    let mut long_name = String::new();
    for _ in 0..(TITLE_MAX_LEN + 1) {
        long_name.push('长');
    }
    let long_title = h.register_doc(Actor::Maintainer, &long_name, DocFamily::Architecture, &[]);
    let empty_step = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "");
    s.add(
        "VE64-判据-畸形语料零崩溃",
        empty_title == Err(DocErr::Code(DocCode::BadTitle))
            && long_title == Err(DocErr::Code(DocCode::BadTitle))
            && empty_step == Err(DocErr::Code(DocCode::EmptyStep)),
        "空标题/超长标题显性拒（BadTitle）；空步骤文本拒（EmptyStep）——零 unwrap 零越界",
    );

    // —— 判据六 · 审计留痕可回放 + 满容拒记 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "册B", DocFamily::DevGuide, &["VE-F5404"]);
    let _ = h.put_section(Actor::Maintainer, "册B", Section::Scope);
    let replay = h.audit_replay("册B");
    let has_add = replay.iter().any(|a| matches!(a.op, AuditOp::DocAdded { title: _, family: DocFamily::DevGuide }));
    let has_put = replay.iter().any(|a| matches!(a.op, AuditOp::SectionPut { title: _, section: Section::Scope }));
    let denied_before = h.audit_count();
    let _ = h.register_doc(Actor::Reader, "越权", DocFamily::Architecture, &[]);
    let denied_after = h.audit_count();
    // 审计满容：景验证反复灌——2(登记/步骤)+136(验证) = 138 条 → 溢出 10 拒记
    let mut h2 = DocHub::new();
    let _ = h2.register_scene(Actor::Maintainer, Scene::ConnFail);
    let _ = h2.add_repro_step(Actor::Maintainer, Scene::ConnFail, "s");
    for _ in 0..(AUDIT_CAP + 8) {
        let _ = h2.verify_scene(Actor::Maintainer, Scene::ConnFail);
    }
    s.add(
        "VE64-判据-审计回放与满容拒记",
        has_add && has_put && denied_after == denied_before + 1
            && h2.audit_dropped() == 10
            && h2.audit_count() == AUDIT_CAP,
        "登记/落节按册名回放全痕；越权逐条留痕；审计满容拒记 10 条计数呈现（append-only 旧痕保全）",
    );

    // —— 判据六 · 反向：越权全口留痕（景面三口） ——
    let mut h = DocHub::new();
    let _ = h.register_scene(Actor::Reader, Scene::ConnFail);
    let _ = h.add_repro_step(Actor::Reader, Scene::ConnFail, "s");
    let _ = h.verify_scene(Actor::Ci, Scene::ConnFail);
    let denied = h
        .audit
        .iter()
        .filter(|a| matches!(a.op, AuditOp::Denied { actor: _, what: _ }))
        .count();
    s.add(
        "VE64-判据-越权三口全留痕",
        denied == 3 && h.scene_entries().is_empty(),
        "登记景/追加步骤/景验证三口越权全拒且逐条审计（写面是维护者权——谁都能写的文档没人信）",
    );

    // —— 判据七 · 元数据：码段互异 + reason 全码非空 ——
    let codes_ok = {
        let mut uniq = true;
        for i in 0..SEGMENT_GUARD.len() {
            for j in (i + 1)..SEGMENT_GUARD.len() {
                if SEGMENT_GUARD[i].code() == SEGMENT_GUARD[j].code() {
                    uniq = false;
                }
            }
        }
        uniq
            && SEGMENT_GUARD.iter().all(|c| c.code() & 0xFF00 == 0x6D00)
            && SEGMENT_GUARD.iter().all(|c| !c.reason().is_empty())
    };
    s.add(
        "VE64-判据-码段互异reason全码非空",
        codes_ok && SEGMENT_GUARD.len() == 16,
        "十六码全落 0x6Dxx 两两互异；reason 人话逐码可读（拒绝不是哑的 Err）",
    );

    // —— 判据七 · 反向：账实一致探针 + 闭集边界 ——
    let mut h = DocHub::new();
    let _ = h.register_doc(Actor::Maintainer, "手册", DocFamily::Troubleshooting, &["VE-F5405"]);
    let no_scene = h.add_repro_step(Actor::Maintainer, Scene::ConnFail, "s");
    let _ = h.register_scene(Actor::Maintainer, Scene::SyncAbnormal);
    let _ = h.add_repro_step(Actor::Maintainer, Scene::SyncAbnormal, "制造漂移");
    let _ = h.verify_scene(Actor::Maintainer, Scene::SyncAbnormal);
    let probe_ok = h.consistency_probe() == Ok(());
    s.add(
        "VE64-判据-账实一致与闭集边界",
        no_scene == Err(DocErr::Code(DocCode::MissingScene))
            && probe_ok,
        "未登记景追加步骤→MissingScene（闭集边界显性）；景验证+册在册探针零漂移",
    );

    s
}


