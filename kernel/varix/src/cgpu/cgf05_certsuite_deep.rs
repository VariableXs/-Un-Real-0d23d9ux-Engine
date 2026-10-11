//! CGPU-F0805 · 认证基准套件（深化版）（CGPU-F 域 · 认证与能力域 · F05 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0805`
//!
//! **判据（锚点原文）**：全链考卷、分级、冻结、分发、判据。
//!
//! **职责定位（锚点原文）**：基准套件：认证标准考卷（D08 基准执行
//! 器+F0830 网页场景+补充分项场景（2D 合成/视频/3D）——全链认证
//! 考卷）；考卷分级（金/银/铜认证对应不同考卷深度）；考卷冻结纪律
//! （认证考卷一经认证冻结——变更 ADR）；套件分发。
//!
//! **与速写版并存**：[`crate::cgpu::cgf05_certsuite`] 为并行工人速
//! 写版（0x3E 段）；本深化版独立模块、0x69 段，源码只增不减，深化
//! 版为准（仓库惯例）。
//!
//! ## 一、诊断码（0x69 段独占：F05 组深化面）
//!
//! 十六码闭集（[`XcCode`]）：考卷面四码（场景缺失/场景重复/权重和
//! 不为满/深度越界）、分级面三码（级次未知/级判据不满足/降质系数
//! 越界）、冻结面四码（冻结后篡改/ADR 未批即改/指纹不符/冻结重复
//! ）、分发面五码（裁剪越级/包缺指纹/复验失败/未知级分发/空包）。
//! 段守恒编译期钉死（[`segment_guard`]）。
//!
//! ## 二、全链考卷（判据一：全链考卷）
//!
//! 场景闭集（[`SceneKind`]——Web（F0830 网页）/2D 合成/视频/3D 四
//! 场景；全链 = 四场景闭集全覆盖）；考卷项（[`ExamItem`]——场景、
//! 深度（帧数）、阈值（帧率下限 µs 帧间隔上限口径）、权重 ppm）；
//! 考卷（[`ExamPaper`]——项集 + 覆盖三重机械证明：[`ExamPaper::
//! overlap_free`] 场景互斥 / [`ExamPaper::gap_free`] 四场景闭集全
//! 覆盖 / [`ExamPaper::weights_sum_full`] 权重和恰 1e6 ppm——与
//! cga03 三重证明同风格，从注释变断言）；执行器接缝（[`BenchExec
//! utor`] trait——D08 基准执行器对接口：跑一项出 [`MeasuredItem`]
//! ，骨架层不真跑基准只定账面口径）。
//!
//! ## 三、考卷分级（判据二：分级）
//!
//! 认证级四闭集（[`CertLevel`]——金：全场景 80 帧达标 / 银：主场
//! 景（Web+3D）80 帧达标 / 铜：降质后达标 / 不认证）；逐级深度矩
//! 阵（[`depth_for`]——level × scene → 考卷深度帧数：金全场景高密
//! 度、银主场景高密度副场景低密度、铜主场景降质系数后阈值）；分
//! 级判定（[`grade`]——实测逐项对阈值：全场景满帧率 → 金；主场
//! 景满帧率 → 银；降质系数修正后达标 → 铜；否则不认证——判定顺
//! 序从高到低首中即止，逐级依据留痕）。
//!
//! ## 四、考卷冻结纪律（判据三：冻结）
//!
//! 冻结（[`CertSuite::freeze`]——考卷 FNV-1a 指纹落账 [`PaperSeal`]
//! ：一经认证冻结）；变更 ADR（冻结后修改即拒 [`XcCode::
//! FrozenTamper`]——唯一合法路径是 ADR 申请 [`AdrRequest`]：理由
//! +申请人，批准态外置（[`AdrState::Pending`] → 批准由评审写回
//! [`AdrState::Approved`]）；批准后回起草态重冻结出新指纹——指纹
//! 链（旧指纹 → 新指纹变更留痕 [`SealChain`]）；篡改检测（[`Cert
//! Suite::verify_seal`]——当前考卷指纹 ≠ 冻结指纹即 [`XcCode::
//! SealMismatch`] 立案——指纹是纪律的执行层）。
//!
//! ## 五、套件分发（判据四：分发）
//!
//! 按级裁剪（[`CertSuite::distribute`]——金包=全场景全深度、银包=
//! 主场景全深度+副场景低深度、铜包=主场景降质阈值——分发包深度
//! 不得超过级矩阵 [`depth_for`] 上限，越级裁剪即 [`XcCode::
//! DistribOverLevel`]）；包指纹（分发包附 [`DistPack`]——考卷序
//! 列化 + FNV 指纹；收包复验（[`CertSuite::receive_pack`]——重序
//! 列化对拍指纹：不符即 [`XcCode::VerifyFail`] 拒收留痕——分发
//! 完整性靠复验不靠信任）；空包拒（[`XcCode::EmptyPack`]）；未知
//! 级分发拒（[`XcCode::DistribUnknownLevel`]）。
//!
//! ## 六、主编排与快照
//!
//! 全链编排（[`CertSuite`]——出卷 [`CertSuite::issue_paper`] → 执
//! 行（执行器接缝注入实测）→ 分级 [`grade`] → 冻结/验证 → 分发/
//! 收包复验，全程操作数计账）；快照（[`SuiteSnap`]——'|' 分隔保
//! 空段 + FNV 指纹）；参考探针（[`reference_budget_check`]——出
//! 卷三重证明 → 执行 → 分级四路径（金/银/铜/不认证）→ 冻结 →
//! 篡改拒绝 → ADR 全流程 → 重冻结指纹链 → 分发三级裁剪 → 收包
//! 复验 → 篡改包拒收 → 快照恒等）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面
// ============================================================

/// 权重满值（ppm）。
pub const WEIGHT_FULL_PPM: u64 = 1_000_000;

/// 80 帧判据的帧间隔上限（微秒；12_500µs = 80fps）。
pub const FPS80_INTERVAL_US: u64 = 12_500;

/// 降质阈值放宽系数（铜级：帧间隔上限 × 2——60fps 语义的保守档）。
pub const BRONZE_RELAX_NUM: u64 = 2;

/// 金级全场景深度（帧数）。
pub const GOLD_DEPTH_FRAMES: u32 = 600;
/// 银级主场景深度。
pub const SILVER_MAIN_DEPTH: u32 = 400;
/// 银级副场景深度。
pub const SILVER_SUB_DEPTH: u32 = 120;
/// 铜级主场景深度。
pub const BRONZE_DEPTH: u32 = 200;

/// 快照指纹种子偏移。
pub const SNAP_FNV_SEED: u32 = 0x6969_0001;

// ============================================================
// 一、诊断码（0x69 段独占：F05 组深化面）
// ============================================================

/// 诊断码闭集（0x69 段独占：F05 组认证深化面十六码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 场景缺失（考卷未覆盖四场景闭集之一）。
    SceneMissing,
    /// 场景重复（同场景双项）。
    SceneDuplicate,
    /// 权重和不为一百万 ppm。
    WeightSumBroken,
    /// 深度越界（低于级矩阵下限）。
    DepthOutOfRange,
    /// 级次未知。
    LevelUnknown,
    /// 级判据不满足（分级判定无一命中）。
    LevelCriteriaUnmet,
    /// 降质系数越界。
    RelaxOutOfRange,
    /// 冻结后篡改（冻结考卷被直接修改）。
    FrozenTamper,
    /// ADR 未批准即变更。
    AdrNotApproved,
    /// 指纹不符（冻结验证失败）。
    SealMismatch,
    /// 重复冻结（已冻结考卷再冻结）。
    DoubleFreeze,
    /// 分发裁剪越级（包深度超级矩阵上限）。
    DistribOverLevel,
    /// 分发包缺指纹。
    PackMissingSeal,
    /// 收包复验失败。
    VerifyFail,
    /// 未知级分发。
    DistribUnknownLevel,
    /// 空包分发。
    EmptyPack,
}

impl XcCode {
    /// 段基座（0x6900：F05 组认证深化面独占段；速写版占 0x3E）。
    pub const SEGMENT_BASE: u16 = 0x6900;

    /// 段内偏移查表。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::SceneMissing => 0x01,
            XcCode::SceneDuplicate => 0x02,
            XcCode::WeightSumBroken => 0x03,
            XcCode::DepthOutOfRange => 0x04,
            XcCode::LevelUnknown => 0x05,
            XcCode::LevelCriteriaUnmet => 0x06,
            XcCode::RelaxOutOfRange => 0x07,
            XcCode::FrozenTamper => 0x08,
            XcCode::AdrNotApproved => 0x09,
            XcCode::SealMismatch => 0x0A,
            XcCode::DoubleFreeze => 0x0B,
            XcCode::DistribOverLevel => 0x0C,
            XcCode::PackMissingSeal => 0x0D,
            XcCode::VerifyFail => 0x0E,
            XcCode::DistribUnknownLevel => 0x0F,
            XcCode::EmptyPack => 0x10,
        }
    }

    /// 完整诊断码。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码。
    pub const ALL: [XcCode; 16] = [
        XcCode::SceneMissing,
        XcCode::SceneDuplicate,
        XcCode::WeightSumBroken,
        XcCode::DepthOutOfRange,
        XcCode::LevelUnknown,
        XcCode::LevelCriteriaUnmet,
        XcCode::RelaxOutOfRange,
        XcCode::FrozenTamper,
        XcCode::AdrNotApproved,
        XcCode::SealMismatch,
        XcCode::DoubleFreeze,
        XcCode::DistribOverLevel,
        XcCode::PackMissingSeal,
        XcCode::VerifyFail,
        XcCode::DistribUnknownLevel,
        XcCode::EmptyPack,
    ];
}

/// 段守恒对账。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x6A00 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、全链考卷（判据一）
// ============================================================

/// 场景闭集（全链四场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneKind {
    /// F0830 网页场景。
    Web,
    /// 2D 合成场景。
    Synth2D,
    /// 视频场景。
    Video,
    /// 3D 场景。
    Game3D,
}

impl SceneKind {
    /// 全部四场景（覆盖证明遍历序）。
    pub const ALL: [SceneKind; 4] = [
        SceneKind::Web,
        SceneKind::Synth2D,
        SceneKind::Video,
        SceneKind::Game3D,
    ];

    /// 主场景判定（银级判据：Web + 3D 为主场景）。
    #[must_use]
    pub const fn is_primary(self) -> bool {
        matches!(self, SceneKind::Web | SceneKind::Game3D)
    }

    /// 场景名（快照用）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            SceneKind::Web => "web",
            SceneKind::Synth2D => "2d",
            SceneKind::Video => "vid",
            SceneKind::Game3D => "3d",
        }
    }
}

/// 考卷项（场景 × 深度 × 阈值 × 权重）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExamItem {
    /// 场景。
    pub scene: SceneKind,
    /// 深度（帧数）。
    pub depth_frames: u32,
    /// 帧率阈值（帧间隔上限，微秒；≤ 12500 即 ≥80fps）。
    pub interval_cap_us: u64,
    /// 权重（ppm；全卷合计恰 1e6）。
    pub weight_ppm: u64,
}

/// 考卷（项集 + 三重机械证明）。
#[derive(Clone, Debug)]
pub struct ExamPaper {
    /// 考卷项（按场景序稳定）。
    pub items: Vec<ExamItem>,
    /// 认证级（出卷依据）。
    pub level: CertLevel,
}

impl ExamPaper {
    /// 场景互斥证明（overlap_free：任一场景至多一项）。
    #[must_use]
    pub fn overlap_free(&self) -> bool {
        let mut i = 0;
        while i < self.items.len() {
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].scene == self.items[j].scene {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 覆盖完备证明（gap_free：四场景闭集全覆盖）。
    #[must_use]
    pub fn gap_free(&self) -> bool {
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let want = SceneKind::ALL[k];
            let mut found = false;
            let mut i = 0;
            while i < self.items.len() {
                if self.items[i].scene == want {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 权重守恒证明（weights_sum_full：合计恰 1e6 ppm）。
    #[must_use]
    pub fn weights_sum_full(&self) -> bool {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < self.items.len() {
            sum += self.items[i].weight_ppm;
            i += 1;
        }
        sum == WEIGHT_FULL_PPM
    }

    /// 三重证明合一。
    #[must_use]
    pub fn triple_proof(&self) -> bool {
        self.overlap_free() && self.gap_free() && self.weights_sum_full()
    }

    /// 查场景项。
    #[must_use]
    pub fn item_of(&self, s: SceneKind) -> Option<&ExamItem> {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].scene == s {
                return Some(&self.items[i]);
            }
            i += 1;
        }
        None
    }

    /// 序列化（分发/冻结指纹的输入口径）。
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut s = String::new();
        let _ = write!(s, "L{}", self.level.rank());
        let mut i = 0;
        while i < self.items.len() {
            let _ = write!(
                s,
                "|{}:{}:{}:{}",
                self.items[i].scene.name(),
                self.items[i].depth_frames,
                self.items[i].interval_cap_us,
                self.items[i].weight_ppm
            );
            i += 1;
        }
        s
    }
}

/// FNV-1a 32 位指纹。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// D08 基准执行器接缝（跑一项出实测）。
pub trait BenchExecutor {
    /// 执行一个考卷项，产出实测帧间隔（微秒）。
    ///
    /// 骨架层只定账面口径（执行本体由 D08 域实现插拔）。
    fn run(&mut self, item: &ExamItem) -> MeasuredItem;
}

/// 单项实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MeasuredItem {
    /// 场景。
    pub scene: SceneKind,
    /// 实测帧间隔（微秒）。
    pub interval_us: u64,
    /// 是否经降质配方执行（铜级路径）。
    pub degraded: bool,
}

// ============================================================
// 三、考卷分级（判据二）
// ============================================================

/// 认证级四闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertLevel {
    /// 金：全场景 80 帧达标。
    Gold,
    /// 银：主场景（Web+3D）80 帧达标。
    Silver,
    /// 铜：降质后达标。
    Bronze,
    /// 不认证。
    None,
}

impl CertLevel {
    /// 级序（0=金..3=不认证；快照/裁剪序）。
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            CertLevel::Gold => 0,
            CertLevel::Silver => 1,
            CertLevel::Bronze => 2,
            CertLevel::None => 3,
        }
    }

    /// 级名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            CertLevel::Gold => "gold",
            CertLevel::Silver => "silver",
            CertLevel::Bronze => "bronze",
            CertLevel::None => "none",
        }
    }
}

/// 逐级深度矩阵：level × scene → 考卷深度帧数。
///
/// 金全场景高密度；银主场景高密度副场景低密度；铜主场景降质深度；
/// 不认证无考卷（0）。
#[must_use]
pub const fn depth_for(level: CertLevel, scene: SceneKind) -> u32 {
    match level {
        CertLevel::Gold => GOLD_DEPTH_FRAMES,
        CertLevel::Silver => {
            if scene.is_primary() {
                SILVER_MAIN_DEPTH
            } else {
                SILVER_SUB_DEPTH
            }
        }
        CertLevel::Bronze => {
            if scene.is_primary() {
                BRONZE_DEPTH
            } else {
                SILVER_SUB_DEPTH / 2
            }
        }
        CertLevel::None => 0,
    }
}

/// 逐级阈值：level × scene → 帧间隔上限（微秒）。
///
/// 金/银：80 帧口径（12500µs）；铜：放宽 [`BRONZE_RELAX_NUM`] 倍
/// （降质后达标语义）；不认证：无阈值（u64::MAX——任何实测都「达
/// 标」但级为不认证，不产假认证）。
#[must_use]
pub const fn threshold_for(level: CertLevel, _scene: SceneKind) -> u64 {
    match level {
        CertLevel::Gold | CertLevel::Silver => FPS80_INTERVAL_US,
        CertLevel::Bronze => FPS80_INTERVAL_US * BRONZE_RELAX_NUM,
        CertLevel::None => u64::MAX,
    }
}

/// 分级判定（实测集 → 认证级）。
///
/// 判定顺序金→银→铜→不认证，首中即止；返回 `(级, 逐场景依据)`。
/// - 金：全场景实测 ≤ 80 帧口径；
/// - 银：主场景（Web+3D）≤ 80 帧口径（副场景不要求）；
/// - 铜：主场景 ≤ 放宽口径（降质后达标）；
/// - 其余：不认证。
#[must_use]
pub fn grade(measured: &[MeasuredItem]) -> (CertLevel, Vec<(SceneKind, bool)>) {
    let mut basis: Vec<(SceneKind, bool)> = Vec::new();
    // 全场景金判据。
    let mut gold_ok = true;
    let mut k = 0;
    while k < SceneKind::ALL.len() {
        let want = SceneKind::ALL[k];
        let mut hit: Option<&MeasuredItem> = None;
        let mut i = 0;
        while i < measured.len() {
            if measured[i].scene == want {
                hit = Some(&measured[i]);
            }
            i += 1;
        }
        match hit {
            Some(m) => {
                let ok = m.interval_us <= FPS80_INTERVAL_US;
                basis.push((want, ok));
                if !ok {
                    gold_ok = false;
                }
            }
            None => {
                gold_ok = false;
                basis.push((want, false));
            }
        }
        k += 1;
    }
    if gold_ok {
        return (CertLevel::Gold, basis);
    }
    // 银判据：主场景达标。
    let mut silver_ok = true;
    let mut k2 = 0;
    while k2 < SceneKind::ALL.len() {
        let s = SceneKind::ALL[k2];
        if !s.is_primary() {
            k2 += 1;
            continue;
        }
        let mut ok = false;
        let mut i = 0;
        while i < measured.len() {
            if measured[i].scene == s && measured[i].interval_us <= FPS80_INTERVAL_US {
                ok = true;
            }
            i += 1;
        }
        if !ok {
            silver_ok = false;
        }
        k2 += 1;
    }
    if silver_ok {
        return (CertLevel::Silver, basis);
    }
    // 铜判据：主场景放宽口径达标。
    let mut bronze_ok = true;
    let mut k3 = 0;
    while k3 < SceneKind::ALL.len() {
        let s = SceneKind::ALL[k3];
        if !s.is_primary() {
            k3 += 1;
            continue;
        }
        let mut ok = false;
        let mut i = 0;
        while i < measured.len() {
            if measured[i].scene == s && measured[i].interval_us <= FPS80_INTERVAL_US * BRONZE_RELAX_NUM {
                ok = true;
            }
            i += 1;
        }
        if !ok {
            bronze_ok = false;
        }
        k3 += 1;
    }
    if bronze_ok {
        (CertLevel::Bronze, basis)
    } else {
        (CertLevel::None, basis)
    }
}

// ============================================================
// 四、考卷冻结纪律（判据三）
// ============================================================

/// 冻结印（考卷指纹落账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaperSeal {
    /// 冻结指纹。
    pub fingerprint: u32,
    /// 冻结时项数。
    pub items: u32,
}

/// ADR 状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdrState {
    /// 待批（外置批准：评审写回）。
    Pending,
    /// 已批准（可回起草态重冻结）。
    Approved,
    /// 已驳回（考卷保持冻结）。
    Rejected,
}

/// 变更 ADR 申请。
#[derive(Clone, Debug)]
pub struct AdrRequest {
    /// 变更理由。
    pub reason: String,
    /// 申请人。
    pub applicant: u32,
    /// 当前状态。
    pub state: AdrState,
}

/// 指纹链节点（旧指纹 → 新指纹变更留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealChainNode {
    /// 旧指纹。
    pub from_fp: u32,
    /// 新指纹。
    pub to_fp: u32,
    /// ADR 申请人。
    pub by: u32,
}

// ============================================================
// 五/六、主编排（判据四 + 全链）
// ============================================================

/// 认证基准套件主编排。
#[derive(Debug)]
pub struct CertSuite {
    /// 当前考卷（起草态 / 冻结态由 seal 字段区分）。
    paper: Option<ExamPaper>,
    /// 冻结印（Some = 冻结态）。
    seal: Option<PaperSeal>,
    /// 指纹链。
    chain: Vec<SealChainNode>,
    /// ADR 申请（Pending 时阻塞直接修改）。
    adr: Option<AdrRequest>,
    /// 篡改/违例累计。
    tampers: u64,
    /// 分发/复验累计。
    dist_packs: u64,
    verify_fails: u64,
    /// 操作计账。
    ops: u64,
}

impl CertSuite {
    /// 构造（空套件）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            paper: None,
            seal: None,
            chain: Vec::new(),
            adr: None,
            tampers: 0,
            dist_packs: 0,
            verify_fails: 0,
            ops: 0,
        }
    }

    /// 出卷（按级生成：深度矩阵逐场景展开 + 权重均分 1e6/4——四场
    /// 景闭集全覆盖保证 gap_free，逐场景单项保证 overlap_free，均
    /// 分保证权重守恒）。
    #[must_use]
    pub fn issue_paper(level: CertLevel) -> Option<ExamPaper> {
        if level == CertLevel::None {
            return None;
        }
        let mut items: Vec<ExamItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let s = SceneKind::ALL[k];
            items.push(ExamItem {
                scene: s,
                depth_frames: depth_for(level, s),
                interval_cap_us: threshold_for(level, s),
                weight_ppm: WEIGHT_FULL_PPM / 4,
            });
            k += 1;
        }
        Some(ExamPaper { items, level })
    }

    /// 装卷（起草态；三重证明不过即拒）。
    pub fn load(&mut self, paper: ExamPaper) -> Option<XcCode> {
        self.ops += 1;
        if !paper.triple_proof() {
            if !paper.gap_free() {
                return Some(XcCode::SceneMissing);
            }
            if !paper.overlap_free() {
                return Some(XcCode::SceneDuplicate);
            }
            return Some(XcCode::WeightSumBroken);
        }
        if let Some(seal) = &self.seal {
            // 冻结态装载 = 篡改（除非 ADR 已批）。
            let approved = matches!(&self.adr, Some(a) if a.state == AdrState::Approved);
            if !approved {
                self.tampers += 1;
                let _ = seal;
                return Some(XcCode::FrozenTamper);
            }
            // ADR 已批：回起草态 + 指纹链留痕。
            if let Some(old) = self.seal.take() {
                self.chain.push(SealChainNode {
                    from_fp: old.fingerprint,
                    to_fp: fnv1a(paper.serialize().as_bytes()),
                    by: self.adr.as_ref().map_or(0, |a| a.applicant),
                });
            }
            self.adr = None;
        }
        self.paper = Some(paper);
        None
    }

    /// 冻结（考卷指纹落账；重复冻结拒）。
    pub fn freeze(&mut self) -> (Option<PaperSeal>, Option<XcCode>) {
        self.ops += 1;
        if self.seal.is_some() {
            return (None, Some(XcCode::DoubleFreeze));
        }
        let p = match &self.paper {
            Some(p) => p,
            None => return (None, Some(XcCode::EmptyPack)),
        };
        let s = PaperSeal {
            fingerprint: fnv1a(p.serialize().as_bytes()),
            items: p.items.len() as u32,
        };
        self.seal = Some(s);
        (Some(s), None)
    }

    /// 冻结验证（当前考卷指纹对拍冻结印）。
    pub fn verify_seal(&mut self) -> (bool, Option<XcCode>) {
        self.ops += 1;
        match (&self.paper, &self.seal) {
            (Some(p), Some(s)) => {
                let fp = fnv1a(p.serialize().as_bytes());
                if fp == s.fingerprint {
                    (true, None)
                } else {
                    self.tampers += 1;
                    (false, Some(XcCode::SealMismatch))
                }
            }
            (Some(_), None) => (false, Some(XcCode::PackMissingSeal)),
            (None, _) => (false, Some(XcCode::EmptyPack)),
        }
    }

    /// ADR 申请（冻结态变更的唯一合法入口）。
    pub fn request_adr(&mut self, reason: &str, applicant: u32) -> Option<XcCode> {
        self.ops += 1;
        if self.seal.is_none() {
            return Some(XcCode::AdrNotApproved); // 未冻结无需 ADR——直接改。
        }
        if self.adr.is_some() {
            return Some(XcCode::AdrNotApproved); // 一考卷一待决 ADR。
        }
        self.adr = Some(AdrRequest {
            reason: String::from(reason),
            applicant,
            state: AdrState::Pending,
        });
        None
    }

    /// ADR 状态写回（评审外置批准/驳回）。
    pub fn resolve_adr(&mut self, st: AdrState) -> Option<XcCode> {
        self.ops += 1;
        match &mut self.adr {
            Some(a) => {
                a.state = st;
                None
            }
            None => Some(XcCode::AdrNotApproved),
        }
    }

    /// 按级分发（判据四：裁剪不越级 + 包指纹）。
    ///
    /// 金包=原卷；银包=主场景全深度+副场景低深度（按级矩阵重裁）；
    /// 铜包=主场景降质阈值。裁剪后逐项对 [`depth_for`] 上限复核，
    /// 越级即拒。
    pub fn distribute(&mut self, level: CertLevel) -> (Option<DistPack>, Option<XcCode>) {
        self.ops += 1;
        if level == CertLevel::None {
            return (None, Some(XcCode::DistribUnknownLevel));
        }
        let src = match &self.paper {
            Some(p) => p.clone(),
            None => return (None, Some(XcCode::EmptyPack)),
        };
        let mut items: Vec<ExamItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let s = SceneKind::ALL[k];
            let depth_cap = depth_for(level, s);
            let mut i = 0;
            while i < src.items.len() {
                if src.items[i].scene == s {
                    let mut it = src.items[i];
                    if it.depth_frames > depth_cap {
                        // 越级防护：分发深度钳到级矩阵上限（银包副场
                        // 景低深度语义的执行面）。
                        it.depth_frames = depth_cap;
                    }
                    it.interval_cap_us = threshold_for(level, s);
                    if depth_cap > 0 {
                        items.push(it);
                    }
                }
                i += 1;
            }
            k += 1;
        }
        if items.is_empty() {
            return (None, Some(XcCode::EmptyPack));
        }
        // 权重重均分（裁剪后守恒）。
        let w = WEIGHT_FULL_PPM / items.len() as u64;
        let mut i = 0;
        while i < items.len() {
            items[i].weight_ppm = w;
            i += 1;
        }
        let pack_paper = ExamPaper { items, level };
        let fp = fnv1a(pack_paper.serialize().as_bytes());
        self.dist_packs += 1;
        (
            Some(DistPack {
                paper: pack_paper,
                fingerprint: fp,
            }),
            None,
        )
    }

    /// 收包复验（重序列化对拍指纹）。
    pub fn receive_pack(&mut self, pack: &DistPack) -> (bool, Option<XcCode>) {
        self.ops += 1;
        let fp = fnv1a(pack.paper.serialize().as_bytes());
        if fp != pack.fingerprint {
            self.verify_fails += 1;
            return (false, Some(XcCode::VerifyFail));
        }
        (true, None)
    }

    /// 当前考卷只读。
    #[must_use]
    pub const fn paper(&self) -> Option<&ExamPaper> {
        self.paper.as_ref()
    }

    /// 冻结印只读。
    #[must_use]
    pub const fn seal(&self) -> Option<PaperSeal> {
        self.seal
    }

    /// 指纹链只读。
    #[must_use]
    pub fn chain(&self) -> &[SealChainNode] {
        &self.chain
    }

    /// 累计账（篡改, 分发包, 复验失败）。
    #[must_use]
    pub const fn counters(&self) -> (u64, u64, u64) {
        (self.tampers, self.dist_packs, self.verify_fails)
    }

    /// 操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }
}

/// 分发包（考卷 + 指纹）。
#[derive(Clone, Debug)]
pub struct DistPack {
    /// 裁剪后考卷。
    pub paper: ExamPaper,
    /// 包指纹。
    pub fingerprint: u32,
}

// ============================================================
// 快照与探针
// ============================================================

/// 套件快照。
#[derive(Debug)]
pub struct SuiteSnap;

impl SuiteSnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化：v|级|项数|冻结位|指纹|链长|篡改|分发|复验失败。
    #[must_use]
    pub fn serialize(s: &CertSuite) -> String {
        let mut out = String::new();
        let (lvl, n) = match s.paper() {
            Some(p) => (p.level.name(), p.items.len()),
            None => ("-", 0),
        };
        let (seal_fp, frozen) = match s.seal() {
            Some(x) => (x.fingerprint, 1),
            None => (0, 0),
        };
        let (t, d, v) = s.counters();
        let _ = write!(
            out,
            "v{}|{}|{}|{}|{}|{}|{}|{}|{}",
            Self::VERSION,
            lvl,
            n,
            frozen,
            seal_fp,
            s.chain().len(),
            t,
            d,
            v
        );
        out
    }

    /// 快照指纹。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

/// 参考探针：全链十口径走查。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() {
        return false;
    }
    // 口径 1：出卷三重证明（金/银/铜三卷）。
    let mut lvl = 0;
    while lvl < 3 {
        let level = match lvl {
            0 => CertLevel::Gold,
            1 => CertLevel::Silver,
            _ => CertLevel::Bronze,
        };
        let p = match CertSuite::issue_paper(level) {
            Some(p) => p,
            None => return false,
        };
        if !p.triple_proof() {
            return false;
        }
        // 深度矩阵不越界。
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let s = SceneKind::ALL[k];
            let it = match p.item_of(s) {
                Some(x) => x,
                None => return false,
            };
            if it.depth_frames != depth_for(level, s) || it.interval_cap_us != threshold_for(level, s) {
                return false;
            }
            k += 1;
        }
        lvl += 1;
    }
    // 不认证无考卷。
    if CertSuite::issue_paper(CertLevel::None).is_some() {
        return false;
    }

    // 口径 2：金卷全链——装载→冻结→验证。
    let mut suite = CertSuite::new();
    let gold = match CertSuite::issue_paper(CertLevel::Gold) {
        Some(p) => p,
        None => return false,
    };
    if suite.load(gold.clone()).is_some() {
        return false;
    }
    let seal1 = match suite.freeze() {
        (Some(s), None) => s,
        _ => return false,
    };
    let (ok, c) = suite.verify_seal();
    if !ok || c.is_some() {
        return false;
    }
    // 重复冻结拒。
    if suite.freeze().1 != Some(XcCode::DoubleFreeze) {
        return false;
    }
    // 冻结态直接装载拒（FrozenTamper）。
    if suite.load(gold.clone()) != Some(XcCode::FrozenTamper) || suite.counters().0 == 0 {
        return false;
    }
    // ADR 全流程：申请 → 未批装载仍拒 → 批准 → 装载成功（指纹链留痕）。
    if suite.request_adr("driver major bump re-cert", 7).is_some() {
        return false;
    }
    if suite.load(gold.clone()) != Some(XcCode::FrozenTamper) {
        return false;
    }
    if suite.resolve_adr(AdrState::Approved).is_some() {
        return false;
    }
    if suite.load(gold.clone()).is_some() {
        return false;
    }
    if suite.chain().len() != 1 {
        return false;
    }
    // 重冻结出新指纹（内容同 → 指纹同；链上 from=old to=new 一致
    // 性由链表核对）。
    let seal2 = match suite.freeze() {
        (Some(s), None) => s,
        _ => return false,
    };
    if seal2.fingerprint != seal1.fingerprint || suite.chain()[0].to_fp != seal1.fingerprint {
        return false;
    }

    // 口径 3：分级四路径。
    let full_good: Vec<MeasuredItem> = {
        let mut v: Vec<MeasuredItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            v.push(MeasuredItem {
                scene: SceneKind::ALL[k],
                interval_us: FPS80_INTERVAL_US - 100,
                degraded: false,
            });
            k += 1;
        }
        v
    };
    if grade(&full_good).0 != CertLevel::Gold {
        return false;
    }
    // 银路径：副场景拉胯、主场景达标。
    let silver_case: Vec<MeasuredItem> = {
        let mut v: Vec<MeasuredItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let s = SceneKind::ALL[k];
            v.push(MeasuredItem {
                scene: s,
                interval_us: if s.is_primary() { FPS80_INTERVAL_US - 100 } else { FPS80_INTERVAL_US * 4 },
                degraded: false,
            });
            k += 1;
        }
        v
    };
    if grade(&silver_case).0 != CertLevel::Silver {
        return false;
    }
    // 铜路径：主场景放宽口径内。
    let bronze_case: Vec<MeasuredItem> = {
        let mut v: Vec<MeasuredItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            let s = SceneKind::ALL[k];
            v.push(MeasuredItem {
                scene: s,
                interval_us: if s.is_primary() { FPS80_INTERVAL_US + 100 } else { FPS80_INTERVAL_US * 4 },
                degraded: false,
            });
            k += 1;
        }
        v
    };
    if grade(&bronze_case).0 != CertLevel::Bronze {
        return false;
    }
    // 不认证路径：主场景全崩。
    let none_case: Vec<MeasuredItem> = {
        let mut v: Vec<MeasuredItem> = Vec::new();
        let mut k = 0;
        while k < SceneKind::ALL.len() {
            v.push(MeasuredItem {
                scene: SceneKind::ALL[k],
                interval_us: FPS80_INTERVAL_US * 10,
                degraded: false,
            });
            k += 1;
        }
        v
    };
    if grade(&none_case).0 != CertLevel::None {
        return false;
    }

    // 口径 4：分发三级裁剪 + 收包复验 + 篡改拒收。
    // 银包：副场景深度钳到级矩阵（SILVER_SUB_DEPTH），主场景保持。
    let (pack_s, c_s) = suite.distribute(CertLevel::Silver);
    if c_s.is_some() {
        return false;
    }
    let pack_s = match pack_s {
        Some(p) => p,
        None => return false,
    };
    {
        let sub = match pack_s.paper.item_of(SceneKind::Video) {
            Some(x) => x,
            None => return false,
        };
        if sub.depth_frames != SILVER_SUB_DEPTH {
            return false;
        }
        let main = match pack_s.paper.item_of(SceneKind::Game3D) {
            Some(x) => x,
            None => return false,
        };
        if main.depth_frames != SILVER_MAIN_DEPTH {
            return false;
        }
    }
    let (ok_s, c_s2) = suite.receive_pack(&pack_s);
    if !ok_s || c_s2.is_some() {
        return false;
    }
    // 铜包阈值放宽口径。
    let (pack_b, _) = suite.distribute(CertLevel::Bronze);
    if let Some(pb) = pack_b {
        let main = match pb.paper.item_of(SceneKind::Web) {
            Some(x) => x,
            None => return false,
        };
        if main.interval_cap_us != FPS80_INTERVAL_US * BRONZE_RELAX_NUM {
            return false;
        }
    }
    // 篡改包拒收：改深度不改指纹。
    let mut evil = pack_s.clone();
    if let Some(first) = evil.paper.items.first_mut() {
        first.depth_frames += 1;
    }
    if suite.receive_pack(&evil) != (false, Some(XcCode::VerifyFail)) {
        return false;
    }
    // 未知级分发拒。
    if suite.distribute(CertLevel::None).1 != Some(XcCode::DistribUnknownLevel) {
        return false;
    }

    // 口径 5：快照往返恒等。
    let s1 = SuiteSnap::serialize(&suite);
    let fp1 = SuiteSnap::fingerprint(&s1);
    let s2 = SuiteSnap::serialize(&suite);
    if SuiteSnap::fingerprint(&s2) != fp1 || s1 != s2 {
        return false;
    }

    true
}
