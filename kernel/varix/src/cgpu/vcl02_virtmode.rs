//! CGPU-F1762 · 虚拟化模式分类（CGPU-L 域 · 虚拟化扩展域 · L02 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1762`
//!
//! **判据（锚点原文）**：四模式、五维矩阵、决策树、共存、损耗诚实、五组、判据。
//!
//! **职责定位（锚点原文）**：模式分类：四模式（①直通（整卡透传——性能
//! 最高共享零②分区（硬件切分（SR-IOV/MIG/GPU-P——硬件隔离③半虚拟化
//! （virtio-gpu 类——API 转译④软件（API remoting/软渲染——最大兼容
//! ——四模式定义表；模式属性矩阵（性能/共享度/兼容/安全/迁移支持——
//! 五维矩阵——矩阵版本化；模式选择（场景×约束→模式推荐——选择决策树
//! 显性；模式共存（混合模式（直通+半虚拟并存——共存语义；诚实（各模式
//! 损耗诚实标注——损耗声明复用家族。
//!
//! ## 一、四模式定义表（判据一）
//!
//! 虚拟化模式闭集（[`VirtMode`]）四元：直通（整卡透传，性能最高、共享
//! 为零、迁移最弱）；分区（SR-IOV/MIG/GPU-P 硬件切分，隔离强、分区数
//! 有限）；半虚拟化（virtio-gpu 类 API 转译，共享好、有转译开销）；软件
//! （API remoting/软渲染，兼容最大、性能最低）。定义表每行带**机制名
//! 字面量**（直通=VFIO/MDEV 禁、分区=SR-IOV|MIG|GPU-P、半虚拟=
//! virtio-gpu|venus、软件=venus-remoting|llvmpipe），机制名是探测面
//! 对接的冻结词表。
//!
//! ## 二、五维属性矩阵（判据二）
//!
//! 每模式五维评分（[`ModeMatrix`]）：性能/共享度/兼容/安全/迁移支持，
//! 0-3 四档整数（不用浮点评分——档位语义清晰可对拍）。矩阵**版本化**
//! （[`MATRIX_VERSION`]）：评分修订走版本号，消费方按版本对账——
//! 无版本矩阵会在演进中悄悄变语义。
//!
//! ## 三、选择决策树（判据三）
//!
//! 场景×约束→模式推荐（[`recommend`]）：约束闭集（性能优先/隔离优先/
//! 兼容优先/迁移频繁），决策树是**显性 if 序**（顺序即优先级，注释写明
//! 每级判据来源）——决策树藏在权重里就不可审计。
//!
//! ## 四、共存语义与损耗诚实（判据四、五）
//!
//! 共存：直通+半虚拟可并存于不同卡（[`coexist`] 给合法性判定——同卡
//! 直通与半虚拟互斥，跨卡合法）；损耗声明（[`LOSS_ANNOUNCE`]）：每模式
//! 的损耗一句话字面量（复用损耗声明家族口径），诚实标注不粉饰——
//! 选型报告必须带损耗原文。
//!
//! **对接**：F1761（L 域开工——本模块是其模式分类面）；F1763+（分区
//! 深化/迁移语义的下游）；F1466（功耗架构——损耗面联动）。零 panic 面、
//! 零 IO、零墙钟、no_std。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const VIRT_MODE_VERSION: &str = "L02-virtmode-v1";

/// 属性矩阵版本（评分修订走版本号；消费方按版本对账）。
pub const MATRIX_VERSION: &str = "matrix-v3";

/// 评分档位数（0-3 四档；整数档位语义）。
pub const SCORE_STEPS: u8 = 3;

// ---------------------------------------------------------------------------
// 二、诊断码（0x33 段独占：L02 模式分类面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x33 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 约束闭集外（决策树无此分支）。
    UnknownConstraint,
    /// 同卡直通与半虚拟互斥（共存非法）。
    CoexistIllegal,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::UnknownConstraint => "约束闭集外：决策树无此分支，建议按 Constraint 闭集取用或扩树并记 ADR",
            XcCode::CoexistIllegal => "同卡直通与半虚拟互斥：整卡透传后无转译面，跨卡共存或改分区模式",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、四模式定义表（判据一）
// ---------------------------------------------------------------------------

/// 虚拟化模式闭集（四元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtMode {
    /// ①直通：整卡透传（VFIO），性能最高、共享零、迁移最弱。
    Passthrough,
    /// ②分区：硬件切分（SR-IOV/MIG/GPU-P），隔离强、分区数有限。
    Partitioned,
    /// ③半虚拟化：API 转译（virtio-gpu/venus），共享好、有转译开销。
    Paravirtual,
    /// ④软件：API remoting/软渲染（llvmpipe），兼容最大、性能最低。
    Software,
}

impl VirtMode {
    /// 机制名字面量（探测面冻结词表；判据侧独立重排对拍）。
    pub const fn mechanisms(self) -> &'static str {
        match self {
            VirtMode::Passthrough => "VFIO",
            VirtMode::Partitioned => "SR-IOV|MIG|GPU-P",
            VirtMode::Paravirtual => "virtio-gpu|venus",
            VirtMode::Software => "venus-remoting|llvmpipe",
        }
    }

    /// 模式名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            VirtMode::Passthrough => "直通",
            VirtMode::Partitioned => "分区",
            VirtMode::Paravirtual => "半虚拟化",
            VirtMode::Software => "软件",
        }
    }

    /// 序号（0-3；矩阵行下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            VirtMode::Passthrough => 0,
            VirtMode::Partitioned => 1,
            VirtMode::Paravirtual => 2,
            VirtMode::Software => 3,
        }
    }
}

/// 四模式全集（矩阵行序）。
pub const ALL_MODES: [VirtMode; 4] = [
    VirtMode::Passthrough,
    VirtMode::Partitioned,
    VirtMode::Paravirtual,
    VirtMode::Software,
];

// ---------------------------------------------------------------------------
// 四、五维属性矩阵（判据二；版本化 MATRIX_VERSION）
// ---------------------------------------------------------------------------

/// 属性维度闭集（五维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    /// 性能。
    Perf,
    /// 共享度。
    Sharing,
    /// 兼容。
    Compat,
    /// 安全。
    Security,
    /// 迁移支持。
    Migratable,
}

/// 五维评分行（0-3 档；行值字面量冻结，判据侧独立对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoreRow {
    /// 性能（直通 3 / 分区 2 / 半虚拟 1 / 软件 0）。
    pub perf: u8,
    /// 共享度（直通 0 / 分区 1 / 半虚拟 3 / 软件 3）。
    pub sharing: u8,
    /// 兼容（直通 1 / 分区 1 / 半虚拟 2 / 软件 3）。
    pub compat: u8,
    /// 安全（直通 2 / 分区 3 / 半虚拟 2 / 软件 3）。
    pub security: u8,
    /// 迁移支持（直通 0 / 分区 2 / 半虚拟 3 / 软件 3）。
    pub migratable: u8,
}

/// 五维矩阵（行=ALL_MODES 序；评分依据：直通独占硬件所以性能/共享两极，
/// 分区硬件隔离安全最高但性能被切分摊薄，半虚拟共享与迁移强但转译有损，
/// 软件兼容/安全（纯用户态）最强而性能垫底）。
pub const MODE_MATRIX: [ScoreRow; 4] = [
    ScoreRow { perf: 3, sharing: 0, compat: 1, security: 2, migratable: 0 },
    ScoreRow { perf: 2, sharing: 1, compat: 1, security: 3, migratable: 2 },
    ScoreRow { perf: 1, sharing: 3, compat: 2, security: 2, migratable: 3 },
    ScoreRow { perf: 0, sharing: 3, compat: 3, security: 3, migratable: 3 },
];

/// 查评分行（越界显性拒——防御性：闭集序号恒在界内）。
pub fn score_of(m: VirtMode) -> Result<&'static ScoreRow, XcCode> {
    MODE_MATRIX.get(m.ordinal()).ok_or(XcCode::UnknownConstraint)
}

// ---------------------------------------------------------------------------
// 五、选择决策树（判据三；显性 if 序=优先级）
// ---------------------------------------------------------------------------

/// 选型约束闭集（决策树的输入维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// 性能优先（游戏/创作负载）。
    PerfFirst,
    /// 隔离优先（多租户/不可信负载）。
    IsolationFirst,
    /// 兼容优先（老旧/小众 API）。
    CompatFirst,
    /// 迁移频繁（沙盒快照/负载漂移）。
    MigrateOften,
}

/// 模式推荐（决策树显性 if 序，顺序即优先级：
/// 1) 隔离优先→分区（硬件隔离是唯一不可替代项）；
/// 2) 性能优先→直通（共享需求为空时让独占收益最大化）；
/// 3) 迁移频繁→半虚拟化（虚拟硬件层是热迁移前提）；
/// 4) 兼容优先→软件（API remoting 对客户端最透明）。
pub const fn recommend(c: Constraint) -> VirtMode {
    match c {
        Constraint::IsolationFirst => VirtMode::Partitioned,
        Constraint::PerfFirst => VirtMode::Passthrough,
        Constraint::MigrateOften => VirtMode::Paravirtual,
        Constraint::CompatFirst => VirtMode::Software,
    }
}

// ---------------------------------------------------------------------------
// 六、共存语义（判据四）
// ---------------------------------------------------------------------------

/// 共存合法性判定（同卡直通与半虚拟互斥——整卡透传后无转译面；
/// 直通与分区同卡互斥（VFIO 占卡后 SR-IOV 不可用）；其余组合跨卡合法、
/// 同卡软件模式叠任何模式合法（软渲染不占硬件面））。
pub const fn coexist(a: VirtMode, b: VirtMode, same_card: bool) -> Result<(), XcCode> {
    if !same_card {
        return Ok(());
    }
    let pa = a.ordinal();
    let pb = b.ordinal();
    // (直通,分区)/(分区,直通)/(直通,半虚拟)/(半虚拟,直通) 同卡互斥。
    let conflict = (pa == 0 && pb == 1)
        || (pa == 1 && pb == 0)
        || (pa == 0 && pb == 2)
        || (pa == 2 && pb == 0);
    if conflict {
        return Err(XcCode::CoexistIllegal);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、损耗诚实声明（判据五；复用损耗声明家族口径）
// ---------------------------------------------------------------------------

/// 损耗声明表（每模式一句字面量；选型报告必须带损耗原文——不粉饰）。
pub const LOSS_ANNOUNCE: [&str; 4] = [
    "直通：损耗≈0（独占整卡），代价是宿主与其它客户机完全失去此卡",
    "分区：损耗≈5-15%（切分管线与调度开销），分区数受硬件上限约束",
    "半虚拟化：损耗≈10-30%（API 转译路径）， guest 需专用驱动",
    "软件：损耗最大（逐调用 remoting 或纯 CPU 渲染），仅作兼容兜底",
];

/// 损耗声明读取（越界显性拒——防御性）。
pub fn loss_of(m: VirtMode) -> Result<&'static str, XcCode> {
    LOSS_ANNOUNCE
        .get(m.ordinal())
        .copied()
        .ok_or(XcCode::UnknownConstraint)
}

/// 选型报告（决策树结论 + 损耗原文 + 矩阵版本——审计面三元组）。
pub fn selection_report(c: Constraint) -> String {
    let m = recommend(c);
    let mut s = String::from("推荐=");
    s.push_str(m.zh());
    s.push_str("（机制 ");
    s.push_str(m.mechanisms());
    s.push_str("）；矩阵 ");
    s.push_str(MATRIX_VERSION);
    s.push_str("；损耗：");
    match loss_of(m) {
        Ok(l) => s.push_str(l),
        Err(_) => s.push_str("损耗声明缺失"),
    }
    s
}

// ---------------------------------------------------------------------------
// 八、迁移兼容矩阵（共存判据的迁移面深化）
// ---------------------------------------------------------------------------

/// 迁移合法性矩阵（mode×mode：行=源模式，列=目标模式；true=可迁移。
/// 口径：软件→任何模式可迁（无硬件绑定）；半虚拟↔分区可迁（虚拟
/// 硬件层是迁移前提）；直通进出均不可热迁（整卡绑定）；同模式迁移
/// 合法（同构迁移））。行序=ALL_MODES。
pub const MIGRATION_MATRIX: [[bool; 4]; 4] = [
    // 直通→：直通可(同构)/分区否/半虚拟否/软件否（整卡绑定）。
    [true, false, false, false],
    // 分区→：直通否/分区可/半虚拟可/软件可（硬件切分面重建）。
    [false, true, true, true],
    // 半虚拟→：直通否/分区可/半虚拟可/软件可（虚拟硬件层通用）。
    [false, true, true, true],
    // 软件→：全部可（无硬件绑定——最自由）。
    [true, true, true, true],
];

/// 迁移合法性查询（闭集校验后查表；不合法给原因——显性）。
pub fn migration_ok(from: VirtMode, to: VirtMode) -> Result<bool, XcCode> {
    let f = from.ordinal();
    let t = to.ordinal();
    let row = MIGRATION_MATRIX.get(f).ok_or(XcCode::UnknownConstraint)?;
    let ok = row.get(t).copied().unwrap_or(false);
    Ok(ok)
}

/// 单维评分查询（判据二的维度面：按 Dim 查指定模式的单维分——
/// 消费方按维取分不再拆整行）。
pub const fn score_dim(m: VirtMode, d: Dim) -> Result<u8, XcCode> {
    let oi = m.ordinal();
    if oi >= MODE_MATRIX.len() {
        return Err(XcCode::UnknownConstraint);
    }
    let row = MODE_MATRIX[oi];
    Ok(match d {
        Dim::Perf => row.perf,
        Dim::Sharing => row.sharing,
        Dim::Compat => row.compat,
        Dim::Security => row.security,
        Dim::Migratable => row.migratable,
    })
}

/// 双约束推荐（决策树扩展：双约束交集——按优先级序逐约束跑树取
/// 交集；交集空时按第一约束（优先级高者）落地并标注让步——显性
/// 让步优于隐式猜）。
pub fn recommend_two(first: Constraint, second: Constraint) -> (VirtMode, bool) {
    let a = recommend(first);
    let b = recommend(second);
    if a == b {
        return (a, false);
    }
    // 交集：两模式的共存语义下同卡可用即可落地；直通与半虚拟/分区
    // 同卡互斥——此时按第一约束落地（让步显性标注）。
    match coexist(a, b, true) {
        Ok(()) => (b, true), // 首选 a 承载第一约束，b 可共存叠加（返回 b 为叠加面）
        Err(_) => (a, false),
    }
}

/// 损耗分档表（ppm；与 LOSS_ANNOUNCE 一句话对应的可机检档——
/// 判据侧对拍面：直通 0/分区 100000/半虚拟 200000/软件 900000，
/// 上界口径）。
pub const LOSS_CEIL_PPM: [u64; 4] = [0, 150_000, 300_000, 900_000];

/// 损耗上界查询（越界显性拒——防御性）。
pub fn loss_ceil_ppm(m: VirtMode) -> Result<u64, XcCode> {
    LOSS_CEIL_PPM
        .get(m.ordinal())
        .copied()
        .ok_or(XcCode::UnknownConstraint)
}
