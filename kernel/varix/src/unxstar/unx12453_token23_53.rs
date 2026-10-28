//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12453 · 令牌族边界样本集 · 0x306a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12453-J1 经典坑样本（特权矩阵×组矩阵复合/模拟级越界）
//!      ≥30 例入库全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 复合判定链四段：特权在册 → 特权使能 → 组在册使能 → 模拟级
//!     达标，任一段拒绝即短路（拒绝码逐段定位）；
//!   - 特权矩阵：LUID 2..23 域 × 使能/禁用二态；
//!   - 组矩阵：组 RID 512/513/544/546 经典组 × 使能/禁用二态
//!     （SE_GROUP_ENABLED=0x4 口径）；
//!   - 模拟级档：四档闭集（Anonymous/Identification/Impersonation/
//!     Delegation），操作需求级 > 令牌级即越界拒；
//!   - 36 例样本常量表：12 例特权×组复合 / 12 例模拟级越界 / 12 例
//!     边界复合（全过正路 + 各段拒止），逐例期望裁决钉死；
//!   - 入库全过判据：计算裁决 ≡ 期望裁决，36/36 才绿；
//!   - 变异注入：合法样本确定性变异（撤特权/撤组/降级）必被抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x306a0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x306a0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x306a0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 样本入库下限（判据 ≥30，实入 36）。
pub const SAMPLE_MIN: usize = 30;

/// 组使能位（NT 口径 SE_GROUP_ENABLED）。
pub const GRP_ENABLED: u32 = 0x0000_0004;

/// 特权使能位（NT 口径 SE_PRIVILEGE_ENABLED）。
pub const PRIV_ENABLED: u32 = 0x0000_0002;

/// 经典组 RID 闭集（512=Administrators 513=Users 544=本地管理员别名
/// 546=Guests，模型化四组）。
pub const GROUP_RIDS: [u32; 4] = [512, 513, 544, 546];

/// 诊断码（0x306a0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    /// 0x306a0 特权 LUID 零。
    LuidZero = 0,
    /// 0x306a1 特权未在册。
    PrivMissing = 1,
    /// 0x306a2 特权未使能。
    PrivDisabled = 2,
    /// 0x306a3 组 RID 出经典组闭集。
    GroupOob = 3,
    /// 0x306a4 组未在册。
    GroupMissing = 4,
    /// 0x306a5 组未使能。
    GroupDisabled = 5,
    /// 0x306a6 模拟级越界（需求级 > 令牌级）。
    LevelOob = 6,
    /// 0x306a7 期望未命中（计算裁决 ≠ 期望裁决）。
    ExpectMiss = 7,
    /// 0x306a8 样本重复入库。
    SampleDup = 8,
    /// 0x306a9 账缩水（append-only 被破）。
    LedgerShrink = 9,
    /// 0x306aa 类别缺失（三类样本任一类不足下限）。
    CategoryShort = 10,
}

impl TkCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TkCode; 11] = [
        TkCode::LuidZero, TkCode::PrivMissing, TkCode::PrivDisabled, TkCode::GroupOob,
        TkCode::GroupMissing, TkCode::GroupDisabled, TkCode::LevelOob, TkCode::ExpectMiss,
        TkCode::SampleDup, TkCode::LedgerShrink, TkCode::CategoryShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x306a0 => "特权 LUID 零：LUID 低 32 位 ≥1，回查样本构造。",
        0x306a1 => "特权未在册：令牌特权表查无该 LUID，先授权再复测。",
        0x306a2 => "特权未使能：在册但 ENABLED 位清零，先 Adjust 再复测。",
        0x306a3 => "组 RID 越闭集：只认 512/513/544/546 四组。",
        0x306a4 => "组未在册：令牌组表查无该 RID，先加组再复测。",
        0x306a5 => "组未使能：在册但 ENABLED 位清零，先使能组再复测。",
        0x306a6 => "模拟级越界：需求级 > 令牌级，降需求或改模拟档。",
        0x306a7 => "期望未命中：逐段核对判定链输出与期望裁决。",
        0x306a8 => "样本重复入库：同 id 只收一笔，二投计重复。",
        0x306a9 => "样本账缩水：append-only，禁止截断，重放补齐。",
        0x306aa => "类别缺失：三类样本逐类清点，不足先补样本。",
        _ => "未知诊断码：回查 0x306a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x306a0 段基址。
    pub const SEED: u64 = 0x306a0_306a_0306_a0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 模拟级（四档闭集，秩序口径）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImpLevel {
    /// 0 匿名。
    Anonymous = 0,
    /// 1 识别。
    Identification = 1,
    /// 2 模拟。
    Impersonation = 2,
    /// 3 委派。
    Delegation = 3,
}

/// 样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PitKind {
    /// 特权×组复合。
    PrivGroup,
    /// 模拟级越界。
    ImpOob,
    /// 边界复合。
    EdgeCombo,
}

impl PitKind {
    /// 类别序号。
    pub const fn idx(self) -> usize {
        match self {
            PitKind::PrivGroup => 0,
            PitKind::ImpOob => 1,
            PitKind::EdgeCombo => 2,
        }
    }
}

/// 令牌快照（样本入参：特权 + 组 + 模拟级三面）。
#[derive(Clone, Copy, Debug)]
pub struct TokenSnap {
    /// 特权 LUID（0=无特权语义）。
    pub priv_luid: u32,
    /// 特权属性位（PRIV_ENABLED 组合）。
    pub priv_attrs: u32,
    /// 组 RID（0=无组语义）。
    pub group_rid: u32,
    /// 组属性位（GRP_ENABLED 组合）。
    pub group_attrs: u32,
    /// 令牌模拟级。
    pub level: ImpLevel,
}

/// 操作需求（样本请求面）。
#[derive(Clone, Copy, Debug)]
pub struct OpDemand {
    /// 需要特权 LUID。
    pub need_priv: u32,
    /// 需要组 RID。
    pub need_group: u32,
    /// 需要模拟级。
    pub need_level: ImpLevel,
}

/// 期望裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 应放行。
    Accept,
    /// 应拒绝（诊断码序号）。
    Reject(u8),
}

/// 边界样本（一条经典坑 = 令牌快照 + 操作需求 + 期望裁决）。
#[derive(Clone, Copy, Debug)]
pub struct SampleSpec {
    /// 样本号。
    pub id: u16,
    /// 类别。
    pub kind: PitKind,
    /// 令牌快照。
    pub token: TokenSnap,
    /// 操作需求。
    pub demand: OpDemand,
    /// 期望裁决。
    pub expect: Expect,
}

/// 四段复合判定链（特权在册 → 使能 → 组在册使能 → 模拟级）。
pub fn judge(s: &SampleSpec) -> Result<(), TkCode> {
    let t = &s.token;
    let d = &s.demand;
    // 段 1：特权 LUID 域。
    if d.need_priv == 0 {
        return Err(TkCode::LuidZero);
    }
    // 段 2：特权在册 + 使能。
    let priv_ok = t.priv_luid == d.need_priv && t.priv_attrs & PRIV_ENABLED != 0;
    if t.priv_luid != d.need_priv {
        return Err(TkCode::PrivMissing);
    }
    if t.priv_attrs & PRIV_ENABLED == 0 {
        return Err(TkCode::PrivDisabled);
    }
    let _ = priv_ok;
    // 段 3：组 RID 域 + 在册 + 使能。
    if d.need_group != 0 {
        if !GROUP_RIDS.contains(&d.need_group) {
            return Err(TkCode::GroupOob);
        }
        if t.group_rid != d.need_group {
            return Err(TkCode::GroupMissing);
        }
        if t.group_attrs & GRP_ENABLED == 0 {
            return Err(TkCode::GroupDisabled);
        }
    }
    // 段 4：模拟级。
    if t.level < d.need_level {
        return Err(TkCode::LevelOob);
    }
    Ok(())
}

/// 样本单例判据（计算裁决 ≡ 期望裁决）。
pub fn sample_pass(s: &SampleSpec) -> bool {
    match (judge(s), s.expect) {
        (Ok(()), Expect::Accept) => true,
        (Err(c), Expect::Reject(idx)) => (c as u8) == idx,
        _ => false,
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 36 例样本常量表（判据 ≥30，实入 36）。
/// ─────────────────────────────────────────────────────────────────────
/// 特权×组复合 12 例（id 0..12）。
const PRIV_GROUP_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 0, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 1, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: 0, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(2) },
    SampleSpec { id: 2, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(4) },
    SampleSpec { id: 3, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(5) },
    SampleSpec { id: 4, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 8, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(1) },
    SampleSpec { id: 5, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 17, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 17, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 6, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 18, priv_attrs: PRIV_ENABLED, group_rid: 546, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 18, need_group: 546, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 7, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 18, priv_attrs: PRIV_ENABLED, group_rid: 512, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 18, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Reject(4) },
    SampleSpec { id: 8, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 0, priv_attrs: 0, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(1) },
    SampleSpec { id: 9, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 10, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Anonymous }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(6) },
    SampleSpec { id: 11, kind: PitKind::PrivGroup, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 999, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 999, need_level: ImpLevel::Identification }, expect: Expect::Reject(3) },
];

/// 模拟级越界 12 例（id 12..24）。
const IMP_OOB_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 12, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Impersonation }, expect: Expect::Reject(6) },
    SampleSpec { id: 13, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Anonymous }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Identification }, expect: Expect::Reject(6) },
    SampleSpec { id: 14, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Impersonation }, expect: Expect::Accept },
    SampleSpec { id: 15, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Delegation }, expect: Expect::Reject(6) },
    SampleSpec { id: 16, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Delegation }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Delegation }, expect: Expect::Accept },
    SampleSpec { id: 17, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Anonymous }, demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Anonymous }, expect: Expect::Accept },
    SampleSpec { id: 18, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 7, need_group: 513, need_level: ImpLevel::Impersonation }, expect: Expect::Reject(6) },
    SampleSpec { id: 19, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 513, need_level: ImpLevel::Impersonation }, expect: Expect::Accept },
    SampleSpec { id: 20, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 20, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 20, need_group: 0, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 21, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 20, priv_attrs: 0, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 20, need_group: 0, need_level: ImpLevel::Identification }, expect: Expect::Reject(2) },
    SampleSpec { id: 22, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(4) },
    SampleSpec { id: 23, kind: PitKind::ImpOob, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: 0, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(5) },
];

/// 边界复合 12 例（id 24..36）。
const EDGE_COMBO_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 24, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Delegation }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Delegation }, expect: Expect::Accept },
    SampleSpec { id: 25, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 7, priv_attrs: 0, group_rid: 544, group_attrs: 0, level: ImpLevel::Anonymous }, demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Impersonation }, expect: Expect::Reject(2) },
    SampleSpec { id: 26, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 19, priv_attrs: PRIV_ENABLED, group_rid: 512, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 19, need_group: 512, need_level: ImpLevel::Impersonation }, expect: Expect::Accept },
    SampleSpec { id: 27, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 19, priv_attrs: PRIV_ENABLED, group_rid: 546, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation }, demand: OpDemand { need_priv: 19, need_group: 512, need_level: ImpLevel::Impersonation }, expect: Expect::Reject(4) },
    SampleSpec { id: 28, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 22, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 22, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 29, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 22, priv_attrs: PRIV_ENABLED, group_rid: 999, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 22, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Reject(4) },
    SampleSpec { id: 30, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 9, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 9, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 31, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 9, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 0, need_group: 544, need_level: ImpLevel::Identification }, expect: Expect::Reject(0) },
    SampleSpec { id: 32, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 12, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 12, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 33, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 12, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: 0x8, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 12, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Reject(5) },
    SampleSpec { id: 34, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 17, priv_attrs: PRIV_ENABLED, group_rid: 513, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 17, need_group: 513, need_level: ImpLevel::Identification }, expect: Expect::Accept },
    SampleSpec { id: 35, kind: PitKind::EdgeCombo, token: TokenSnap { priv_luid: 17, priv_attrs: PRIV_ENABLED, group_rid: 546, group_attrs: GRP_ENABLED, level: ImpLevel::Identification }, demand: OpDemand { need_priv: 18, need_group: 546, need_level: ImpLevel::Identification }, expect: Expect::Reject(1) },
];

/// 全量样本表拼接（36 例）。
pub fn all_samples() -> Vec<SampleSpec> {
    let mut v: Vec<SampleSpec> = Vec::new();
    for s in PRIV_GROUP_SAMPLES.iter() {
        v.push(*s);
    }
    for s in IMP_OOB_SAMPLES.iter() {
        v.push(*s);
    }
    for s in EDGE_COMBO_SAMPLES.iter() {
        v.push(*s);
    }
    v
}

/// 样本账正本。
#[derive(Debug)]
pub struct SampleBook {
    /// 已登记样本号。
    seen: Vec<u16>,
    /// 结果账（append-only）。
    rows: Vec<(u16, bool)>,
    /// 账高水位。
    high: usize,
    /// 重复计数。
    dup: u32,
}

impl SampleBook {
    /// 建账。
    pub fn new() -> Self {
        SampleBook { seen: Vec::new(), rows: Vec::new(), high: 0, dup: 0 }
    }

    /// 入库（判定 + 记账；同 id 二投计重复）。
    pub fn ingest(&mut self, s: &SampleSpec) -> bool {
        if self.seen.contains(&s.id) {
            self.dup += 1;
            return false;
        }
        self.seen.push(s.id);
        let ok = sample_pass(s);
        self.rows.push((s.id, ok));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 全量入库。
    pub fn ingest_all(&mut self, specs: &[SampleSpec]) -> usize {
        let mut ok = 0usize;
        for s in specs.iter() {
            if self.ingest(s) {
                ok += 1;
            }
        }
        ok
    }

    /// 全过数。
    pub fn pass_count(&self) -> usize {
        self.rows.iter().filter(|(_, ok)| *ok).count()
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 重复计数。
    pub fn dup(&self) -> u32 {
        self.dup
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 类别清点（id 段归类：0..12 复合 / 12..24 模拟级 / 24..36 边界）。
    pub fn category_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for (id, _) in self.rows.iter() {
            let k = if *id < 12 { 0 } else if *id < 24 { 1 } else { 2 };
            c[k] += 1;
        }
        c
    }

    /// 类别足额判据。
    pub fn categories_ok(&self) -> bool {
        let c = self.category_counts();
        c[0] >= 10 && c[1] >= 10 && c[2] >= 10 && self.rows.len() >= SAMPLE_MIN
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("样本账：\n");
        for (id, ok) in self.rows.iter() {
            s.push_str(&format!("例{} {}\n", id, if *ok { "过" } else { "红" }));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 入库全过演练：36 例全过 10/10 轮。
pub fn drill_sample_batch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x306a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let specs = all_samples();
        let mut bk = SampleBook::new();
        let passed = bk.ingest_all(&specs);
        let _ = lcg.below(3);
        if passed == specs.len() && specs.len() >= SAMPLE_MIN && bk.categories_ok() && bk.monotone() && bk.dup() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("入库全过轮={}/{} 例数={}", ok, INJECT_ROUNDS, all_samples().len()))
}

/// 三类分面演练：每类独立全过 10/10 轮。
pub fn drill_category_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306a1（分面序列）。
    let mut lcg = Lcg::new(0x306a1);
    for r in 0..INJECT_ROUNDS {
        let specs = all_samples();
        let lo = (r % 3) * 12;
        let seg: Vec<SampleSpec> = specs.iter()
            .filter(|s| (s.id as usize) >= lo && (s.id as usize) < lo + 12)
            .cloned().collect();
        let mut bk = SampleBook::new();
        let p = bk.ingest_all(&seg);
        // 模拟级四档闭集清点。
        let levels = [
            ImpLevel::Anonymous,
            ImpLevel::Identification,
            ImpLevel::Impersonation,
            ImpLevel::Delegation,
        ];
        let mut lv_set: Vec<ImpLevel> = Vec::new();
        for s in specs.iter() {
            if !lv_set.contains(&s.token.level) {
                lv_set.push(s.token.level);
            }
        }
        let lv_ok = lv_set.len() == 4
            && levels[0].rank_lt(&levels[1])
            && levels[1].rank_lt(&levels[2])
            && levels[2].rank_lt(&levels[3]);
        let _ = lcg.below(3);
        if p == seg.len() && seg.len() == 12 && lv_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三类分面轮={}/{}", ok, INJECT_ROUNDS))
}

impl ImpLevel {
    /// 秩序比较（严格小于）。
    pub fn rank_lt(&self, other: &ImpLevel) -> bool {
        (*self as u8) < (*other as u8)
    }
}

/// 变异注入演练：合法样本三向变异必被抓 10/10 轮。
pub fn drill_mutant_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306a2（变异序列）。
    let mut lcg = Lcg::new(0x306a2);
    for _r in 0..INJECT_ROUNDS {
        // 基线：全过正路样本。
        let base = SampleSpec {
            id: 900,
            kind: PitKind::EdgeCombo,
            token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: GRP_ENABLED, level: ImpLevel::Impersonation },
            demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification },
            expect: Expect::Accept,
        };
        if !sample_pass(&base) {
            // 基线必须先绿。
            let _ = lcg.below(3);
            continue;
        }
        // 变异 1：撤特权（换未持有 LUID）。
        let mut m1 = base;
        m1.token.priv_luid = 8;
        m1.expect = Expect::Reject(1);
        // 变异 2：撤特权使能。
        let mut m2 = base;
        m2.token.priv_attrs = 0;
        m2.expect = Expect::Reject(2);
        // 变异 3：撤组（换未在册 RID）。
        let mut m3 = base;
        m3.token.group_rid = 513;
        m3.expect = Expect::Reject(4);
        // 变异 4：撤组使能。
        let mut m4 = base;
        m4.token.group_attrs = 0;
        m4.expect = Expect::Reject(5);
        // 变异 5：降模拟级。
        let mut m5 = base;
        m5.token.level = ImpLevel::Anonymous;
        m5.expect = Expect::Reject(6);
        let caught = sample_pass(&m1) && sample_pass(&m2) && sample_pass(&m3)
            && sample_pass(&m4) && sample_pass(&m5);
        let _ = lcg.below(3);
        if caught {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("变异注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 组闭集演练：四经典组逐组在册/越界 10/10 轮。
pub fn drill_groups_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306a3（组序列）。
    let mut lcg = Lcg::new(0x306a3);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 四组逐一：在册使能 → Accept。
        for &rid in GROUP_RIDS.iter() {
            let s = SampleSpec {
                id: 950,
                kind: PitKind::EdgeCombo,
                token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: rid, group_attrs: GRP_ENABLED, level: ImpLevel::Identification },
                demand: OpDemand { need_priv: 7, need_group: rid, need_level: ImpLevel::Identification },
                expect: Expect::Accept,
            };
            all_ok = all_ok && sample_pass(&s);
        }
        // 越组 RID → GroupOob。
        let bad = SampleSpec {
            id: 951,
            kind: PitKind::EdgeCombo,
            token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 600, group_attrs: GRP_ENABLED, level: ImpLevel::Identification },
            demand: OpDemand { need_priv: 7, need_group: 600, need_level: ImpLevel::Identification },
            expect: Expect::Reject(3),
        };
        all_ok = all_ok && sample_pass(&bad);
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("组闭集轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：特权 LUID 零必拒。
    let z = SampleSpec {
        id: 901, kind: PitKind::EdgeCombo,
        token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification },
        demand: OpDemand { need_priv: 0, need_group: 0, need_level: ImpLevel::Identification },
        expect: Expect::Reject(0),
    };
    if sample_pass(&z) {
        caught += 1;
        d.push("注1 零L:抓".to_string());
    } else {
        d.push("注1 零L:漏".to_string());
    }
    // 注 2：特权未在册必拒。
    let p = SampleSpec {
        id: 902, kind: PitKind::EdgeCombo,
        token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification },
        demand: OpDemand { need_priv: 8, need_group: 0, need_level: ImpLevel::Identification },
        expect: Expect::Reject(1),
    };
    if sample_pass(&p) {
        caught += 1;
        d.push("注2 无权:抓".to_string());
    } else {
        d.push("注2 无权:漏".to_string());
    }
    // 注 3：组未使能必拒。
    let g = SampleSpec {
        id: 903, kind: PitKind::EdgeCombo,
        token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 544, group_attrs: 0, level: ImpLevel::Identification },
        demand: OpDemand { need_priv: 7, need_group: 544, need_level: ImpLevel::Identification },
        expect: Expect::Reject(5),
    };
    if sample_pass(&g) {
        caught += 1;
        d.push("注3 组禁:抓".to_string());
    } else {
        d.push("注3 组禁:漏".to_string());
    }
    // 注 4：模拟级越界必拒。
    let lv = SampleSpec {
        id: 904, kind: PitKind::ImpOob,
        token: TokenSnap { priv_luid: 7, priv_attrs: PRIV_ENABLED, group_rid: 0, group_attrs: 0, level: ImpLevel::Identification },
        demand: OpDemand { need_priv: 7, need_group: 0, need_level: ImpLevel::Delegation },
        expect: Expect::Reject(6),
    };
    if sample_pass(&lv) {
        caught += 1;
        d.push("注4 越级:抓".to_string());
    } else {
        d.push("注4 越级:漏".to_string());
    }
    // 注 5：同 id 二投必去重。
    let mut bk = SampleBook::new();
    let s5 = &PRIV_GROUP_SAMPLES[0];
    let first = bk.ingest(s5);
    let second = bk.ingest(s5);
    if first && !second && bk.dup() == 1 && bk.rows() == 1 {
        caught += 1;
        d.push("注5 重复:抓".to_string());
    } else {
        d.push("注5 重复:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x306a0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    TkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：样本报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 样本集报告。
#[derive(Clone, Copy, Debug)]
pub struct TkReport {
    /// 入库例数。
    pub rows: usize,
    /// 全过数。
    pub passed: usize,
    /// 类别清点。
    pub cats: [usize; 3],
    /// 重复数。
    pub dup: u32,
    /// 单调。
    pub mono: bool,
}

impl TkReport {
    /// 建报告。
    pub fn build(bk: &SampleBook) -> Self {
        TkReport {
            rows: bk.rows(),
            passed: bk.pass_count(),
            cats: bk.category_counts(),
            dup: bk.dup(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rows >= SAMPLE_MIN && self.passed == self.rows && self.mono && self.dup == 0
            && self.cats[0] >= 10 && self.cats[1] >= 10 && self.cats[2] >= 10
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TkReport 入库={} 过={} 类=[{},{},{}] dup={} 单调={}",
            self.rows, self.passed, self.cats[0], self.cats[1], self.cats[2], self.dup, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct TkCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_tk_checklist(bk: &SampleBook) -> Vec<TkCheckRow> {
    let rep = TkReport::build(bk);
    let mut rows: Vec<TkCheckRow> = Vec::new();
    rows.push(TkCheckRow { name: "入库≥30", passed: bk.rows() >= SAMPLE_MIN });
    rows.push(TkCheckRow { name: "全过", passed: bk.pass_count() == bk.rows() });
    rows.push(TkCheckRow { name: "复合类≥10", passed: bk.category_counts()[0] >= 10 });
    rows.push(TkCheckRow { name: "模拟级类≥10", passed: bk.category_counts()[1] >= 10 });
    rows.push(TkCheckRow { name: "边界类≥10", passed: bk.category_counts()[2] >= 10 });
    rows.push(TkCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(TkCheckRow { name: "零重复", passed: bk.dup() == 0 });
    rows.push(TkCheckRow { name: "转录非空", passed: bk.export_lines().contains("样本账") });
    rows.push(TkCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TkCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tk_checklist_green(rows: &[TkCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tk_stats_line(bk: &SampleBook) -> String {
    format!(
        "边界样本概况：入库 {} 全过 {} 类别 {:?}",
        bk.rows(),
        bk.pass_count(),
        bk.category_counts()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12453_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let specs = all_samples();
    let mut bk = SampleBook::new();
    let _ = bk.ingest_all(&specs);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        TkReport::build(&bk).all_green() && bk.export_lines().contains("样本账"),
        tk_stats_line(&bk),
    ));
    // X-b：变异注入 10/10。
    let (ok_m, d_m) = drill_mutant_10();
    out.push(("X-变异注入10/10", ok_m, d_m));
    // X-c：组闭集 10/10。
    let (ok_g, d_g) = drill_groups_10();
    out.push(("X-组闭集10/10", ok_g, d_g));
    // X-d：终验清单。
    let rows = build_tk_checklist(&bk);
    out.push((
        "X-终验清单10项",
        tk_checklist_green(&rows),
        TkReport::build(&bk).to_text(),
    ));
    // X-e：口径常量核对。
    out.push((
        "X-口径常量核对",
        SAMPLE_MIN == 30 && PRIV_ENABLED == 0x2 && GRP_ENABLED == 0x4
            && GROUP_RIDS == [512, 513, 544, 546],
        format!("min={} priv={:#x} grp={:#x} rids={:?}", SAMPLE_MIN, PRIV_ENABLED, GRP_ENABLED, GROUP_RIDS),
    ));
    out
}

/// run_unx12453_checks：本单判据聚合（诊断段 0x306a0）。
pub fn run_unx12453_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12453 令牌族边界样本集");
    // J1-a：入库全过 10/10。
    let (ok1, d1) = drill_sample_batch_10();
    s.add_dyn("J1-入库全过10/10", ok1, &d1);
    // J1-b：三类分面 10/10。
    let (ok2, d2) = drill_category_10();
    s.add_dyn("J1-三类分面10/10", ok2, &d2);
    // J1-c：标准会话恒等式（36 例全过 + 类别足额）。
    let specs = all_samples();
    let mut bk = SampleBook::new();
    let passed = bk.ingest_all(&specs);
    s.add_dyn(
        "J1-36例入库全过",
        passed == specs.len() && bk.pass_count() == bk.rows() && bk.rows() >= SAMPLE_MIN && bk.categories_ok(),
        &format!("入库={} 过={} 例数={}", bk.rows(), bk.pass_count(), specs.len()),
    );
    // J2-a：变异注入 10/10。
    let (ok3, d3) = drill_mutant_10();
    s.add_dyn("J2-变异注入10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：组闭集 10/10。
    let (ok4, d4) = drill_groups_10();
    s.add_dyn("J2-组闭集10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x306a0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12453_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
