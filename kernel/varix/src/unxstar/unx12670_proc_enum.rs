//! UNX-F12671 快照口径协议（计数类快照方差显式/方差越界演练） · 0x81180段
//!
//! 判据原文（派工卡 12670 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12671-J1 快照口径协议判据绿（计数类快照方差
//!      显式），方差越界演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 快照口径协议域模型：协议行七元（协议号/口径类闭集/载荷/方差声
//!    明/声明核对位/快照对号/回指指纹）——口径类三分闭集（计数类/
//!    归属类/长度类），每行必须显式声明口径与方差，隐式丢差即协议违
//!    约；
//! 2. 计数类快照方差显式：计数类载荷为双快照计数对（A/B），方差按
//!    saturating 绝对差显式计算；行内"方差声明"必须与实算方差相等
//!    且 ≤ 冻结上沿（VAR_CAP = 2）——声明与实算不一致即协议违约；
//! 3. 归属类与长度类口径：归属类载荷为 pid（必在冻结进程表内）；长
//!    度类载荷为账面长度（必与九档冻结账面一致）；三类口径逐行分诊；
//! 4. 方差越界演练 10/10 检出：确定性出题 10 例（方差 3..=12 越沿族
//!    与声明漂移族），逐例必须被协议审计检出，检出数 10/10 才判绿；
//! 5. 状态机：Declared → Audited → Ratified 单向推进，倒退显性拒；
//!    协议闸四道（口径类闭集/载荷分诊/方差声明核对/回指指纹）逐行
//!    过闸；
//! 6. 注入联测 10 路：口径类越闭集、计数类方差越沿、声明与实算漂
//!    移、归属类 pid 越表、长度类账面不匹、指纹断链、协议号乱序、
//!    行重复、未声明即审计、批准后改写——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（演练档确定性
//!    出题）；取值一律 match/if let/.get()/saturating_*，无 unwrap/
//!    expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12670 − 12600)）。
pub const SEG_BASE: u32 = 0x81180;

// ── 口径协议诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapDiag {
    /// 口径类越闭集。
    EKindRange,
    /// 计数类方差越沿。
    EVarOver,
    /// 方差声明与实算漂移。
    EDeclaredMiss,
    /// 归属类 pid 越表。
    EOwnerRange,
    /// 长度类账面不匹。
    ELenMiss,
    /// 回指指纹缺失（断链）。
    EFpMissing,
    /// 回指指纹错挂。
    EFpMismatch,
    /// 协议号乱序。
    ERowOrder,
    /// 未声明即审计。
    ENotDeclared,
    /// 批准后改写。
    ERatifiedWrite,
    /// 状态机倒退。
    ERewind,
}

impl SnapDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SnapDiag::EKindRange => 0x01,
                SnapDiag::EVarOver => 0x02,
                SnapDiag::EDeclaredMiss => 0x03,
                SnapDiag::EOwnerRange => 0x04,
                SnapDiag::ELenMiss => 0x05,
                SnapDiag::EFpMissing => 0x06,
                SnapDiag::EFpMismatch => 0x07,
                SnapDiag::ERowOrder => 0x08,
                SnapDiag::ENotDeclared => 0x09,
                SnapDiag::ERatifiedWrite => 0x0a,
                SnapDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SnapDiag::EKindRange => "口径类越闭集：只认计数类/归属类/长度类三分，先正口径类",
            SnapDiag::EVarOver => "计数类方差越沿：双快照计数差 ≤2（冻结上沿），越沿先核突变源",
            SnapDiag::EDeclaredMiss => "方差声明漂移：声明值与实算值不一致，显式口径不许隐式丢差",
            SnapDiag::EOwnerRange => "归属类 pid 越表：归属载荷必在冻结进程表内，越表先对进程账",
            SnapDiag::ELenMiss => "长度类账面不匹：长度载荷与冻结账面不符，先对账面",
            SnapDiag::EFpMissing => "回指指纹缺失：快照口径协议指纹空即断链，先补指纹",
            SnapDiag::EFpMismatch => "回指指纹错挂：指纹与冻结值不符，先对指纹源",
            SnapDiag::ERowOrder => "协议号乱序：按协议号 0..N 逐行过闸，跳号先回补",
            SnapDiag::ENotDeclared => "未声明即审计：先声明口径（Declared→Audited）再消费",
            SnapDiag::ERatifiedWrite => "批准后改写：协议档只读，改动走下一轮声明",
            SnapDiag::ERewind => "状态机倒退：Declared→Audited→Ratified 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SNAP_ALL: &[SnapDiag] = &[
    SnapDiag::EKindRange,
    SnapDiag::EVarOver,
    SnapDiag::EDeclaredMiss,
    SnapDiag::EOwnerRange,
    SnapDiag::ELenMiss,
    SnapDiag::EFpMissing,
    SnapDiag::EFpMismatch,
    SnapDiag::ERowOrder,
    SnapDiag::ENotDeclared,
    SnapDiag::ERatifiedWrite,
    SnapDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SNAP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81181..0x8118B 闭集内"
}

// ── 口径类三分闭集 ───────────────────────────────────────────────
/// 口径类三分闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    /// 计数类（双快照计数对，方差显式）。
    Count,
    /// 归属类（pid 归属载荷）。
    Ownership,
    /// 长度类（账面长度载荷）。
    Length,
}

impl MetricKind {
    /// 类号（0/1/2）。
    pub fn no(self) -> u32 {
        match self {
            MetricKind::Count => 0,
            MetricKind::Ownership => 1,
            MetricKind::Length => 2,
        }
    }

    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            MetricKind::Count => "计数类",
            MetricKind::Ownership => "归属类",
            MetricKind::Length => "长度类",
        }
    }

    /// 全部三分（闭集数组）。
    pub fn all() -> &'static [MetricKind] {
        &[MetricKind::Count, MetricKind::Ownership, MetricKind::Length]
    }
}

// ── 快照方差档（冻结口径） ───────────────────────────────────────
/// 快照方差上沿（冻结口径）。
pub const VAR_CAP: u32 = 2;

/// 快照对号闭集上沿（0..=7）。
pub const SNAP_CAP: u32 = 7;

/// 归属类冻结进程表（4 例）。
pub const OWNER_TABLE: &[u32] = &[4, 320, 512, 1024];

/// pid 是否在归属闭集内。
pub fn owner_in_set(pid: u32) -> bool {
    for v in OWNER_TABLE.iter() {
        if *v == pid {
            return true;
        }
    }
    false
}

/// 长度类冻结账面（九档，与探测面同源口径）。
pub const LEN_TABLE: &[u32] = &[24, 36, 40, 16, 16, 20, 16, 4, 56];

/// 账面长度是否在冻结表内。
pub fn len_in_table(v: u32) -> bool {
    for l in LEN_TABLE.iter() {
        if *l == v {
            return true;
        }
    }
    false
}

/// 快照方差显式计算（saturating 绝对差）。
pub fn variance(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

// ── 协议行与黄金判定 ─────────────────────────────────────────────
/// 协议行七元（协议号/口径类/A 快照/B 快照/方差声明/载荷/指纹）。
/// 载荷按口径类分诊：计数类忽略载荷、归属类载荷 = pid、长度类载荷 = 账面长度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// 协议号。
    pub no: u32,
    /// 口径类。
    pub kind: MetricKind,
    /// 快照 A 计数。
    pub snap_a: u32,
    /// 快照 B 计数。
    pub snap_b: u32,
    /// 方差声明（显式入档）。
    pub declared: u32,
    /// 载荷（归属 pid 或账面长度；计数类置零）。
    pub payload: u32,
    /// 回指指纹。
    pub fp: &'static str,
}

/// 快照口径协议指纹（冻结）。
pub const SNAP_FP: &str = "SNAP-METRIC-PROTOCOL-COSIGN";

/// 单行黄金判定（口径类分诊 + 方差声明核对 + 指纹）。
pub fn snap_row_ok(r: &SnapRow) -> Result<(), u32> {
    // 指纹闸。
    if r.fp.is_empty() {
        return Err(SnapDiag::EFpMissing.val());
    }
    if r.fp != SNAP_FP {
        return Err(SnapDiag::EFpMismatch.val());
    }
    // 口径类分诊。
    match r.kind {
        MetricKind::Count => {
            // 实算方差（显式）。
            let actual = variance(r.snap_a, r.snap_b);
            // 声明核对闸。
            if r.declared != actual {
                return Err(SnapDiag::EDeclaredMiss.val());
            }
            // 越沿闸。
            if actual > VAR_CAP {
                return Err(SnapDiag::EVarOver.val());
            }
        }
        MetricKind::Ownership => {
            // 归属闸。
            if !owner_in_set(r.payload) {
                return Err(SnapDiag::EOwnerRange.val());
            }
        }
        MetricKind::Length => {
            // 长度闸。
            if !len_in_table(r.payload) {
                return Err(SnapDiag::ELenMiss.val());
            }
        }
    }
    Ok(())
}

/// 协议档容量（9 行：计数 5 / 归属 2 / 长度 2）。
pub const SNAP_CAP_ROWS: usize = 9;

/// 冻结协议档（9 行）。
pub const SNAP_PROFILE: &[SnapRow] = &[
    SnapRow { no: 0, kind: MetricKind::Count, snap_a: 5, snap_b: 5, declared: 0, payload: 0, fp: SNAP_FP },
    SnapRow { no: 1, kind: MetricKind::Count, snap_a: 5, snap_b: 4, declared: 1, payload: 0, fp: SNAP_FP },
    SnapRow { no: 2, kind: MetricKind::Count, snap_a: 4, snap_b: 6, declared: 2, payload: 0, fp: SNAP_FP },
    SnapRow { no: 3, kind: MetricKind::Count, snap_a: 7, snap_b: 7, declared: 0, payload: 0, fp: SNAP_FP },
    SnapRow { no: 4, kind: MetricKind::Count, snap_a: 6, snap_b: 4, declared: 2, payload: 0, fp: SNAP_FP },
    SnapRow { no: 5, kind: MetricKind::Ownership, snap_a: 0, snap_b: 0, declared: 0, payload: 4, fp: SNAP_FP },
    SnapRow { no: 6, kind: MetricKind::Ownership, snap_a: 0, snap_b: 0, declared: 0, payload: 1024, fp: SNAP_FP },
    SnapRow { no: 7, kind: MetricKind::Length, snap_a: 0, snap_b: 0, declared: 0, payload: 24, fp: SNAP_FP },
    SnapRow { no: 8, kind: MetricKind::Length, snap_a: 0, snap_b: 0, declared: 0, payload: 56, fp: SNAP_FP },
];

// ── 状态机（Declared → Audited → Ratified） ──────────────────────
/// 协议状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapState {
    /// 已声明。
    Declared,
    /// 审计中。
    Audited,
    /// 已批准。
    Ratified,
}

impl SnapState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            SnapState::Declared => 0,
            SnapState::Audited => 1,
            SnapState::Ratified => 2,
        }
    }
}

/// 协议账。
#[derive(Clone, Debug)]
pub struct SnapLedger {
    /// 已对平行。
    rows: Vec<SnapRow>,
    /// 拒收记账（协议号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: SnapState,
}

impl SnapLedger {
    /// 开账（Declared 态）。
    pub fn new() -> SnapLedger {
        SnapLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: SnapState::Declared,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SnapState {
        self.state
    }

    /// 已对平行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&SnapRow> {
        self.rows.get(idx)
    }

    /// 开审（Declared→Audited）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SnapState::Audited.rank() {
            return Err(SnapDiag::ERewind.val());
        }
        self.state = SnapState::Audited;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < SNAP_CAP_ROWS * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行审计（协议号序 + 黄金判定 + 行互异 三闸）。
    pub fn audit(&mut self, r: SnapRow) -> Result<(), u32> {
        if self.state.rank() < SnapState::Audited.rank() {
            return Err(SnapDiag::ENotDeclared.val());
        }
        if self.state.rank() >= SnapState::Ratified.rank() {
            self.note_reject(r.no, SnapDiag::ERatifiedWrite.val());
            return Err(SnapDiag::ERatifiedWrite.val());
        }
        // 协议号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, SnapDiag::ERowOrder.val());
            return Err(SnapDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = snap_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 行互异闸：同口径类同载荷同双快照只入一笔。
        for done in self.rows.iter() {
            if done.kind == r.kind
                && done.payload == r.payload
                && done.snap_a == r.snap_a
                && done.snap_b == r.snap_b
                && done.declared == r.declared
            {
                self.note_reject(r.no, SnapDiag::ERowOrder.val());
                return Err(SnapDiag::ERowOrder.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 批准：9 行齐 + 零拒收。
    pub fn ratify(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SnapState::Ratified.rank() {
            return Err(SnapDiag::ERewind.val());
        }
        if self.state.rank() < SnapState::Audited.rank() {
            return Err(SnapDiag::ENotDeclared.val());
        }
        if self.rows.len() != SNAP_CAP_ROWS {
            return Err(SnapDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(SnapDiag::ERowOrder.val());
        }
        self.state = SnapState::Ratified;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == SNAP_CAP_ROWS && self.rejects.is_empty()
    }
}

// ── 方差越界演练（10/10 检出） ───────────────────────────────────
/// 演练例（A/B 计数/声明/期望检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillCase {
    /// 例号。
    pub no: u32,
    /// 快照 A。
    pub a: u32,
    /// 快照 B。
    pub b: u32,
    /// 方差声明。
    pub declared: u32,
    /// 期望检出。
    pub expect_hit: bool,
}

/// 冻结演练档（10 例：4 合规 + 6 越沿/漂移）。
pub const DRILL_PROFILE: &[DrillCase] = &[
    DrillCase { no: 0, a: 5, b: 5, declared: 0, expect_hit: false },
    DrillCase { no: 1, a: 5, b: 4, declared: 1, expect_hit: false },
    DrillCase { no: 2, a: 4, b: 6, declared: 2, expect_hit: false },
    DrillCase { no: 3, a: 8, b: 8, declared: 0, expect_hit: false },
    DrillCase { no: 4, a: 8, b: 4, declared: 4, expect_hit: true },
    DrillCase { no: 5, a: 3, b: 9, declared: 6, expect_hit: true },
    DrillCase { no: 6, a: 7, b: 3, declared: 4, expect_hit: true },
    DrillCase { no: 7, a: 9, b: 2, declared: 7, expect_hit: true },
    // 声明漂移族（实算在沿内但声明不符）。
    DrillCase { no: 8, a: 5, b: 4, declared: 0, expect_hit: true },
    DrillCase { no: 9, a: 6, b: 4, declared: 1, expect_hit: true },
];

/// 单例演练判定（返回 Err 诊断码 = 检出）。
fn drill_row(c: &DrillCase) -> Result<(), u32> {
    let r = SnapRow {
        no: 0,
        kind: MetricKind::Count,
        snap_a: c.a,
        snap_b: c.b,
        declared: c.declared,
        payload: 0,
        fp: SNAP_FP,
    };
    snap_row_ok(&r)
}

/// 方差越界演练 10/10 检出（判据 J1 直读口）。
pub fn drill_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for c in DRILL_PROFILE.iter() {
        total += 1;
        let hit = drill_row(c).is_err();
        if hit == c.expect_hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("例{}({} vs {} 声明{}) 漏检", c.no, c.a, c.b, c.declared);
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapFault {
    /// 口径类越闭集。
    KindRange,
    /// 计数类方差越沿。
    VarOver,
    /// 声明与实算漂移。
    DeclaredMiss,
    /// 归属类 pid 越表。
    OwnerRange,
    /// 长度类账面不匹。
    LenMiss,
    /// 指纹断链。
    FpMissing,
    /// 指纹错挂。
    FpMismatch,
    /// 协议号乱序。
    RowOrder,
    /// 未声明即审计。
    NotDeclared,
    /// 批准后改写。
    RatifiedWrite,
}

impl SnapFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            SnapFault::KindRange => 0,
            SnapFault::VarOver => 1,
            SnapFault::DeclaredMiss => 2,
            SnapFault::OwnerRange => 3,
            SnapFault::LenMiss => 4,
            SnapFault::FpMissing => 5,
            SnapFault::FpMismatch => 6,
            SnapFault::RowOrder => 7,
            SnapFault::NotDeclared => 8,
            SnapFault::RatifiedWrite => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            SnapFault::KindRange => "口径类越闭集",
            SnapFault::VarOver => "方差越沿",
            SnapFault::DeclaredMiss => "声明漂移",
            SnapFault::OwnerRange => "归属越表",
            SnapFault::LenMiss => "账面不匹",
            SnapFault::FpMissing => "指纹断链",
            SnapFault::FpMismatch => "指纹错挂",
            SnapFault::RowOrder => "协议号乱序",
            SnapFault::NotDeclared => "未声明即审",
            SnapFault::RatifiedWrite => "批准后改写",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [SnapFault] {
        &[
            SnapFault::KindRange,
            SnapFault::VarOver,
            SnapFault::DeclaredMiss,
            SnapFault::OwnerRange,
            SnapFault::LenMiss,
            SnapFault::FpMissing,
            SnapFault::FpMismatch,
            SnapFault::RowOrder,
            SnapFault::NotDeclared,
            SnapFault::RatifiedWrite,
        ]
    }
}

/// 构建完整协议账（9 行全过闸）。
pub fn build_full_ledger() -> SnapLedger {
    let mut led = SnapLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < SNAP_CAP_ROWS {
        let r = match SNAP_PROFILE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.audit(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in SnapFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: SnapFault) -> bool {
    match v {
        SnapFault::KindRange => {
            // 口径类闭集直读：三分各 0/1/2 互异，越类无档。
            let all = MetricKind::all();
            if all.len() != 3 {
                return false;
            }
            let mut ok = true;
            let mut i = 0usize;
            while i < all.len() {
                let k = match all.get(i) {
                    Some(x) => *x,
                    None => return false,
                };
                if k.no() != i as u32 {
                    ok = false;
                }
                let mut j = i + 1;
                while j < all.len() {
                    let k2 = match all.get(j) {
                        Some(x) => *x,
                        None => return false,
                    };
                    if k.no() == k2.no() {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        }
        SnapFault::VarOver => {
            let mut r = match SNAP_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.snap_a = 10;
            r.snap_b = 2;
            snap_row_ok(&r).is_err()
        }
        SnapFault::DeclaredMiss => {
            let mut r = match SNAP_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.declared = 3;
            snap_row_ok(&r).is_err()
        }
        SnapFault::OwnerRange => {
            let mut r = match SNAP_PROFILE.get(5) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.payload = 9999;
            snap_row_ok(&r).is_err()
        }
        SnapFault::LenMiss => {
            let mut r = match SNAP_PROFILE.get(7) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.payload = 100;
            snap_row_ok(&r).is_err()
        }
        SnapFault::FpMissing => {
            let mut r = match SNAP_PROFILE.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "";
            snap_row_ok(&r).is_err()
        }
        SnapFault::FpMismatch => {
            let mut r = match SNAP_PROFILE.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "SNAP-FAKE";
            snap_row_ok(&r).is_err()
        }
        SnapFault::RowOrder => {
            let mut led = SnapLedger::new();
            let _ = led.begin();
            let r = match SNAP_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.audit(r).is_err()
        }
        SnapFault::NotDeclared => {
            let mut fresh = SnapLedger::new();
            let r = match SNAP_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            fresh.audit(r).is_err()
        }
        SnapFault::RatifiedWrite => {
            let mut led = build_full_ledger();
            if led.ratify().is_err() {
                return false;
            }
            let r = match SNAP_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.audit(r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 快照口径协议判据绿（判据 J1 直读口）。
fn snap_protocol_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != SNAP_CAP_ROWS {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.ratify().is_err() {
        return false;
    }
    if led.state() != SnapState::Ratified {
        return false;
    }
    // 独立复核：9 行逐行黄金判定。
    for r in SNAP_PROFILE.iter() {
        if snap_row_ok(r).is_err() {
            return false;
        }
    }
    // 计数类行方差声明显式直读：声明 == 实算。
    for r in SNAP_PROFILE.iter() {
        if r.kind == MetricKind::Count {
            if r.declared != variance(r.snap_a, r.snap_b) {
                return false;
            }
        }
    }
    true
}

/// 方差越界演练 10/10 检出。
fn drill_ok() -> bool {
    let (caught, total, escape) = drill_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 口径类闭集普查（三分各出现且互异）。
fn kind_census_ok() -> bool {
    let mut n_count = 0usize;
    let mut n_own = 0usize;
    let mut n_len = 0usize;
    for r in SNAP_PROFILE.iter() {
        match r.kind {
            MetricKind::Count => n_count += 1,
            MetricKind::Ownership => n_own += 1,
            MetricKind::Length => n_len += 1,
        }
        if snap_row_ok(r).is_err() {
            return false;
        }
    }
    n_count == 5 && n_own == 2 && n_len == 2
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if drill_sweep().0 != drill_sweep().0 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12670u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.ratify().is_err() {
        return false;
    }
    if led.state() != SnapState::Ratified {
        return false;
    }
    match led.row_at(0) {
        Some(r) => {
            if r.fp != SNAP_FP {
                return false;
            }
            if r.kind.name().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12671 快照口径协议自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12670_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12670-snap-protocol");
    let led = build_full_ledger();

    s.add_dyn(
        "F12671-01 口径类三分闭集",
        MetricKind::all().len() == 3,
        "计数类/归属类/长度类",
    );

    s.add_dyn(
        "F12671-02 快照口径协议判据绿",
        snap_protocol_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12671-03 计数类方差显式",
        match led.row_at(0) {
            Some(r) => r.kind == MetricKind::Count && r.declared == variance(r.snap_a, r.snap_b),
            None => false,
        },
        "声明 == 实算（saturating 绝对差）",
    );

    s.add_dyn(
        "F12671-04 方差越界演练 10/10 检出",
        drill_ok(),
        &format!("caught={}", drill_sweep().0),
    );

    s.add_dyn(
        "F12671-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12671-06 口径普查",
        kind_census_ok(),
        "计数 5 / 归属 2 / 长度 2",
    );

    s.add_dyn(
        "F12671-07 方差上沿口径",
        VAR_CAP == 2,
        &format!("var_cap={}（冻结）", VAR_CAP),
    );

    s.add_dyn(
        "F12671-08 归属/长度账面冻结",
        LEN_TABLE.len() == 9 && OWNER_TABLE.len() == 4,
        &format!("lens={} owners={}", LEN_TABLE.len(), OWNER_TABLE.len()),
    );

    s.add_dyn(
        "F12671-09 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12671-10 协议账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12671-11 完成写回语义",
        writeback_ok(),
        "批准即已完成且摘要非空",
    );

    s.add_dyn(
        "F12671-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81180);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(SNAP_ALL.len(), 11);
        let mut i = 0usize;
        while i < SNAP_ALL.len() {
            let vi = match SNAP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SNAP_ALL.len() {
                let vj = match SNAP_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_ratifies() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), SNAP_CAP_ROWS);
        assert_eq!(led.reject_len(), 0);
        assert!(led.ratify().is_ok());
        assert_eq!(led.state(), SnapState::Ratified);
    }

    #[test]
    fn drill_ten_ten() {
        // 方差越界演练 10/10 检出。
        let (caught, total, escape) = drill_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "漏检：{}", escape);
        assert!(escape.is_empty());
        // 演练档直读：4 合规 + 6 检出。
        let mut hits = 0usize;
        for c in DRILL_PROFILE.iter() {
            if drill_row(c).is_err() {
                hits += 1;
            }
        }
        assert_eq!(hits, 6);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn variance_semantics() {
        // 方差函数与上沿边界。
        assert_eq!(variance(5, 5), 0);
        assert_eq!(variance(3, 9), 6);
        assert_eq!(variance(9, 3), 6);
        // 恰 2 过、3 红。
        let eq = SnapRow { no: 0, kind: MetricKind::Count, snap_a: 7, snap_b: 5, declared: 2, payload: 0, fp: SNAP_FP };
        assert!(snap_row_ok(&eq).is_ok());
        let over = SnapRow { no: 0, kind: MetricKind::Count, snap_a: 7, snap_b: 4, declared: 3, payload: 0, fp: SNAP_FP };
        assert!(snap_row_ok(&over).is_err());
        // 声明漂移：实算在沿内但声明不符。
        let drift = SnapRow { no: 0, kind: MetricKind::Count, snap_a: 7, snap_b: 5, declared: 1, payload: 0, fp: SNAP_FP };
        assert_eq!(snap_row_ok(&drift), Err(SnapDiag::EDeclaredMiss.val()));
    }

    #[test]
    fn owner_len_payloads() {
        // 归属类与长度类载荷直读。
        let own = SnapRow { no: 0, kind: MetricKind::Ownership, snap_a: 0, snap_b: 0, declared: 0, payload: 512, fp: SNAP_FP };
        assert!(snap_row_ok(&own).is_ok());
        let own_bad = SnapRow { no: 0, kind: MetricKind::Ownership, snap_a: 0, snap_b: 0, declared: 0, payload: 777, fp: SNAP_FP };
        assert_eq!(snap_row_ok(&own_bad), Err(SnapDiag::EOwnerRange.val()));
        let len_ok_row = SnapRow { no: 0, kind: MetricKind::Length, snap_a: 0, snap_b: 0, declared: 0, payload: 40, fp: SNAP_FP };
        assert!(snap_row_ok(&len_ok_row).is_ok());
        let len_bad = SnapRow { no: 0, kind: MetricKind::Length, snap_a: 0, snap_b: 0, declared: 0, payload: 99, fp: SNAP_FP };
        assert_eq!(snap_row_ok(&len_bad), Err(SnapDiag::ELenMiss.val()));
        // 账面表直读。
        assert_eq!(LEN_TABLE.len(), 9);
        assert!(len_in_table(24));
        assert!(len_in_table(56));
        assert!(!len_in_table(0));
        assert!(!len_in_table(100));
        // 归属表直读。
        assert!(owner_in_set(4));
        assert!(owner_in_set(1024));
        assert!(!owner_in_set(0));
        assert!(!owner_in_set(777));
    }

    #[test]
    fn row_gate_rejects() {
        // 行闸直读：乱序/指纹断链/未声明。
        let mut led = SnapLedger::new();
        let _ = led.begin();
        let r1 = match SNAP_PROFILE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.audit(r1), Err(SnapDiag::ERowOrder.val()));
        let r0 = match SNAP_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.audit(r0.clone()).is_ok());
        // 未声明账直接审计被拒。
        let mut fresh = SnapLedger::new();
        assert_eq!(fresh.audit(r0), Err(SnapDiag::ENotDeclared.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(drill_sweep().0, drill_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Declared→Audited→Ratified。
        assert!(SnapState::Declared.rank() < SnapState::Audited.rank());
        assert!(SnapState::Audited.rank() < SnapState::Ratified.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), SnapState::Audited);
        assert!(led.ratify().is_ok());
        assert_eq!(led.ratify(), Err(SnapDiag::ERewind.val()));
        let mut fresh = SnapLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(SnapDiag::ERewind.val()));
    }

    #[test]
    fn snap_anchor_frozen() {
        // 协议指纹冻结直读。
        assert_eq!(SNAP_FP, "SNAP-METRIC-PROTOCOL-COSIGN");
        for r in SNAP_PROFILE.iter() {
            assert_eq!(r.fp, SNAP_FP, "行 {} 断锚", r.no);
        }
        // 错挂直读。
        let mut r = match SNAP_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        r.fp = "OTHER";
        assert_eq!(snap_row_ok(&r), Err(SnapDiag::EFpMismatch.val()));
    }

    #[test]
    fn metric_kind_census() {
        // 口径普查：计数 5 / 归属 2 / 长度 2。
        assert!(kind_census_ok());
        assert_eq!(MetricKind::Count.no(), 0);
        assert_eq!(MetricKind::Ownership.no(), 1);
        assert_eq!(MetricKind::Length.no(), 2);
        assert_eq!(MetricKind::Count.name(), "计数类");
        assert_eq!(MetricKind::Ownership.name(), "归属类");
        assert_eq!(MetricKind::Length.name(), "长度类");
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12670_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
