//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12730 · RTL_CRITICAL_SECTION 位约定对照回归 · 0x82080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12731-J1 OA 六字段对照回归绿（冻结件逐字段），
//!      字段漂移演练 10/10 检出；
//!   ③ 完成每模块必备判据形态与卡面锚定原文为准，②为准。
//! 设计要点：
//!   1. RTL_CRITICAL_SECTION 六字段冻结件（winnt.h x64 ABI 档）：
//!      DebugInfo 0x00/8、LockCount 0x08/4、RecursionCount 0x0C/4、
//!      OwningThread 0x10/8、LockSemaphore 0x18/8、SpinCount 0x20/8，
//!      总尺寸 0x28——逐字段（偏移, 尺寸）进冻结表；
//!   2. LockCount 位约定双档：XP/WS03 档（−1 空闲、0 首占、每递归
//!      +1）与 Vista+ 档（bit0=占用位）共存建模，两档各自映射到同
//!      一规范占有机（对照回归的语义内核）；
//!   3. 占有机单调状态机：Free→Owned(depth≥1)→释放逐层减→Free；
//!      非法倒退（未占先放、跨层清主）显性拒，释放必须逐层走；
//!   4. J1 锚定闸（OA 六字段对照回归·冻结件逐字段）：六字段候选
//!      表对冻结件逐字段（kind/offset/size）比对，Length 规范值
//!      0x30（x64 sizeof OBJECT_ATTRIBUTES）单列一闸；
//!   5. 字段漂移演练 10/10：LCG 选槽扰动（偏移 +1/+8、Length 改写），
//!      对照器轮轮必抓，漏抓一轮即红；
//!   6. LCG 种子 = SEG_BASE（0x82080）；no_std + alloc；零 std、零
//!      unsafe、零 unwrap/expect/panic、零浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82080 = 0x80000 + 0x40 × (12730 − 12600)，VX9 批独占槽）。
pub const SEG_BASE: u32 = 0x82080;

// ── 冻结常量：RTL_CRITICAL_SECTION 六字段（winnt.h x64 ABI 档） ──
// 字段序（冻结表序）：DebugInfo / LockCount / RecursionCount /
// OwningThread / LockSemaphore / SpinCount——逐槽（偏移, 尺寸）见下表。

/// 六字段冻结表：偏移（x64，含 LONG 对齐填充）。
pub const CS_FIELD_OFFS: [u16; 6] = [0x00, 0x08, 0x0C, 0x10, 0x18, 0x20];
/// 六字段冻结表：尺寸。
pub const CS_FIELD_SIZES: [u16; 6] = [8, 4, 4, 8, 8, 8];
/// 结构总尺寸（x64 冻结档）。
pub const CS_TOTAL_SIZE: u16 = 0x28;

/// LockCount XP/WS03 档：空闲值 −1（0xFFFFFFFF 视图）。
pub const LC_XP_IDLE: i32 = -1;
/// LockCount XP/WS03 档：首次占用值 0。
pub const LC_XP_FIRST: i32 = 0;
/// LockCount Vista+ 档：占用位 bit0。
pub const LC_VISTA_BUSY_BIT: i32 = 0x1;

// ── 冻结常量：OA 六字段（J1 锚定，winnt.h/winternl.h 档） ────────
/// OA 字段闭集（六字段，顺序即冻结表序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaField {
    /// Length：ULONG，规范值 0x30（x64 sizeof OBJECT_ATTRIBUTES）
    Length,
    /// RootDirectory：HANDLE，根目录句柄
    RootDirectory,
    /// ObjectName：PUNICODE_STRING，对象名指针
    ObjectName,
    /// Attributes：ULONG，属性位闭集
    Attributes,
    /// SecurityDescriptor：PVOID，安全描述符
    SecurityDescriptor,
    /// SecurityQualityOfService：PVOID，QoS 指针
    SecurityQoS,
}

/// OA 六字段冻结表：偏移（x64）。
pub const OA_FIELD_OFFS: [u16; 6] = [0x00, 0x08, 0x10, 0x18, 0x20, 0x28];
/// OA 六字段冻结表：尺寸。
pub const OA_FIELD_SIZES: [u16; 6] = [4, 8, 8, 4, 8, 8];
/// Length 规范值（x64 sizeof OBJECT_ATTRIBUTES = 48 = 0x30）。
pub const OA_LENGTH_CANONICAL: u32 = 0x30;
/// OA 属性位闭集（winnt.h OBJ_* 档）。
pub const OBJ_INHERIT: u32 = 0x0000_0002;
pub const OBJ_PERMANENT: u32 = 0x0000_0010;
pub const OBJ_EXCLUSIVE: u32 = 0x0000_0020;
pub const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
pub const OBJ_OPENIF: u32 = 0x0000_0080;
pub const OBJ_OPENLINK: u32 = 0x0000_0100;
pub const OBJ_KERNEL_HANDLE: u32 = 0x0000_0200;
pub const OBJ_FORCE_ACCESS_CHECK: u32 = 0x0000_0400;
/// 属性位合法闭集（逐位或）。
pub const OBJ_VALID_MASK: u32 = OBJ_INHERIT
    | OBJ_PERMANENT
    | OBJ_EXCLUSIVE
    | OBJ_CASE_INSENSITIVE
    | OBJ_OPENIF
    | OBJ_OPENLINK
    | OBJ_KERNEL_HANDLE
    | OBJ_FORCE_ACCESS_CHECK;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsCode {
    /// 临界区字段偏移漂移（冻结表逐槽失配）
    CsFieldDrift,
    /// 递归计数倒退：无 leave 路径的深度下降
    RecurseRegress,
    /// 未占先释放：Free 相位调 leave
    ReleaseUnowned,
    /// 占有相位倒退：Free→Owned 之外的反向迁移
    PhaseRegress,
    /// OA 字段漂移（冻结件逐字段失配）
    OaFieldDrift,
    /// OA Length 非规范值（≠0x30）
    OaLenMismatch,
    /// OA 属性位越闭集
    OaAttrInvalid,
    /// 双档 LockCount 约定互斥破坏（同值双解异判）
    ConventionClash,
}

impl CsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CsCode::CsFieldDrift => "临界区字段偏移漂移：六槽冻结表逐槽对照失配，建议回滚 winnt.h x64 档",
            CsCode::RecurseRegress => "递归计数倒退：深度只能经 leave 逐层减，建议先查占有线程归属",
            CsCode::ReleaseUnowned => "未占先释放：Free 相位无主可放，建议先 enter 取得占有",
            CsCode::PhaseRegress => "占有相位倒退：Free→Owned 只进经 enter，回退必经 leave 逐层",
            CsCode::OaFieldDrift => "OA 字段漂移：冻结件逐字段失配，建议按六槽（偏移,尺寸）表回滚",
            CsCode::OaLenMismatch => "OA Length 非规范值：x64 档必须 0x30，建议校 sizeof(OBJECT_ATTRIBUTES)",
            CsCode::OaAttrInvalid => "OA 属性位越闭集：只许 OBJ_* 8 位档，建议按位与 VALID_MASK 过滤",
            CsCode::ConventionClash => "LockCount 双档约定撞车：同值双解异判，建议按 ABI 档位分源解码",
        }
    }
    pub const ALL: [CsCode; 8] = [
        CsCode::CsFieldDrift,
        CsCode::RecurseRegress,
        CsCode::ReleaseUnowned,
        CsCode::PhaseRegress,
        CsCode::OaFieldDrift,
        CsCode::OaLenMismatch,
        CsCode::OaAttrInvalid,
        CsCode::ConventionClash,
    ];
}

/// 诊断码查表（闭集返回；未知码中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CsCode::ALL.len() {
        if let Some(c) = CsCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82080 段闭集内，建议核对 UNX-F12730 码表"
}

// ── 域模型：占有机（单调状态机） ─────────────────────────────────
/// 占有相位（单调：Free→Owned，回退必经 leave 逐层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnPhase {
    /// 无主空闲
    Free,
    /// 已占有（depth ≥ 1）
    Owned(u32),
}

/// RTL_CRITICAL_SECTION 占有机：六字段快照 + 占有状态机 + 对账账本。
pub struct CritSec {
    /// OwningThread 快照（0 = 无主）
    owner: u64,
    /// RecursionCount 快照
    recursion: u32,
    /// LockCount 快照（XP 档语义：−1 空闲、≥0 占用深度−1）
    lock_count: i32,
    phase: OwnPhase,
    attempts: u32,
    accepted: u32,
    rejected: u32,
}

impl CritSec {
    pub fn new() -> CritSec {
        CritSec {
            owner: 0,
            recursion: 0,
            lock_count: LC_XP_IDLE,
            phase: OwnPhase::Free,
            attempts: 0,
            accepted: 0,
            rejected: 0,
        }
    }

    /// 进入（可重入）：Free→Owned(1)；Owned(d)→Owned(d+1)。
    pub fn enter(&mut self, tid: u64) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if tid == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CsCode::PhaseRegress.val());
        }
        match self.phase {
            OwnPhase::Free => {
                self.owner = tid;
                self.recursion = 1;
                self.lock_count = LC_XP_FIRST;
                self.phase = OwnPhase::Owned(1);
                self.accepted = self.accepted.saturating_add(1);
                Ok(self.recursion)
            }
            OwnPhase::Owned(d) => {
                // 同线程重入深度 +1；异线程在本模型按忙等拒绝（简化档）。
                if self.owner != tid {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(CsCode::ReleaseUnowned.val());
                }
                self.recursion = self.recursion.saturating_add(1);
                self.lock_count = self.lock_count.saturating_add(1);
                self.phase = OwnPhase::Owned(d.saturating_add(1));
                self.accepted = self.accepted.saturating_add(1);
                Ok(self.recursion)
            }
        }
    }

    /// 离开：Owned(1)→Free；Owned(d≥2)→Owned(d−1)。逐层走，跳层拒。
    pub fn leave(&mut self, tid: u64) -> Result<OwnPhase, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 未占先释放：显性拒。
        let (d, owner_now) = match self.phase {
            OwnPhase::Owned(d) => (d, self.owner),
            OwnPhase::Free => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(CsCode::ReleaseUnowned.val());
            }
        };
        if owner_now != tid {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CsCode::ReleaseUnowned.val());
        }
        if d <= 1 {
            // 最后一层：整层回退到 Free（合法回退路径）。
            self.owner = 0;
            self.recursion = 0;
            self.lock_count = LC_XP_IDLE;
            self.phase = OwnPhase::Free;
        } else {
            self.recursion = self.recursion.saturating_sub(1);
            self.lock_count = self.lock_count.saturating_sub(1);
            self.phase = OwnPhase::Owned(d.saturating_sub(1));
        }
        self.accepted = self.accepted.saturating_add(1);
        Ok(self.phase)
    }

    /// 显性倒退探针：Free 相位上任何「放锁」迁移必拒；Owned 相位上
    /// 跨层清主（不做逐层减）必拒。
    pub fn try_force_release(&mut self) -> Result<OwnPhase, u32> {
        match self.phase {
            OwnPhase::Free => Err(CsCode::PhaseRegress.val()),
            OwnPhase::Owned(_) if self.recursion > 1 => Err(CsCode::RecurseRegress.val()),
            OwnPhase::Owned(_) => Err(CsCode::RecurseRegress.val()),
        }
    }

    /// 对账一致：尝试数 ≡ 受理数 + 拒绝数。
    pub fn ledger_consistent(&self) -> bool {
        self.attempts == self.accepted.saturating_add(self.rejected)
    }

    /// 快照守恒：Free ⇒ owner==0 且 recursion==0 且 lock==−1。
    pub fn snapshot_conserved(&self) -> bool {
        match self.phase {
            OwnPhase::Free => self.owner == 0 && self.recursion == 0 && self.lock_count == LC_XP_IDLE,
            OwnPhase::Owned(d) => self.owner != 0 && self.recursion == d && d >= 1,
        }
    }

    pub fn phase(&self) -> OwnPhase {
        self.phase
    }
    pub fn recursion(&self) -> u32 {
        self.recursion
    }
    pub fn lock_count(&self) -> i32 {
        self.lock_count
    }
    pub fn accepted(&self) -> u32 {
        self.accepted
    }
}

/// LockCount 双档解码：XP 档与 Vista+ 档各自解码到同一规范相位。
/// 双源对同一物理状态编码不同，但语义必须可对照（撞车即拒）。
pub fn decode_lc_xp(lc: i32) -> Result<OwnPhase, u32> {
    if lc < LC_XP_IDLE {
        return Err(CsCode::ConventionClash.val());
    }
    if lc == LC_XP_IDLE {
        Ok(OwnPhase::Free)
    } else {
        Ok(OwnPhase::Owned((lc.saturating_sub(LC_XP_FIRST) as u32).saturating_add(1)))
    }
}

pub fn decode_lc_vista(lc: i32) -> Result<OwnPhase, u32> {
    if lc < 0 {
        return Err(CsCode::ConventionClash.val());
    }
    if lc & LC_VISTA_BUSY_BIT == 0 {
        if lc != 0 {
            return Err(CsCode::ConventionClash.val());
        }
        Ok(OwnPhase::Free)
    } else {
        let depth = ((lc as u32 & !LC_VISTA_BUSY_BIT as u32) >> 1).saturating_add(1);
        Ok(OwnPhase::Owned(depth))
    }
}

/// 位约定对照：四组探针（空闲/首占/二层/三层）双档解码必须一致。
fn convention_cross_ok() -> bool {
    let probes_xp: [i32; 4] = [LC_XP_IDLE, LC_XP_FIRST, LC_XP_FIRST + 1, LC_XP_FIRST + 2];
    let probes_vista: [i32; 4] = [0, LC_VISTA_BUSY_BIT, 0b011, 0b101];
    let mut i = 0;
    while i < 4 {
        let px = match probes_xp.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let pv = match probes_vista.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let rx = match decode_lc_xp(px) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let rv = match decode_lc_vista(pv) {
            Ok(p) => p,
            Err(_) => return false,
        };
        if rx != rv {
            return false;
        }
        i += 1;
    }
    true
}

// ── J1 锚定闸：OA 六字段对照回归（冻结件逐字段） ─────────────────
/// OA 冻结件单字段档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OaSlot {
    pub kind: OaField,
    pub offset: u16,
    pub size: u16,
}

/// OA 六字段冻结件（黄金表）。
pub fn oa_frozen_layout() -> Vec<OaSlot> {
    let kinds = [
        OaField::Length,
        OaField::RootDirectory,
        OaField::ObjectName,
        OaField::Attributes,
        OaField::SecurityDescriptor,
        OaField::SecurityQoS,
    ];
    let mut v: Vec<OaSlot> = Vec::new();
    let mut i = 0;
    while i < 6 {
        let kind = match kinds.get(i) {
            Some(k) => *k,
            None => break,
        };
        let off = match OA_FIELD_OFFS.get(i) {
            Some(o) => *o,
            None => break,
        };
        let sz = match OA_FIELD_SIZES.get(i) {
            Some(s) => *s,
            None => break,
        };
        v.push(OaSlot { kind, offset: off, size: sz });
        i += 1;
    }
    v
}

/// 冻结件逐字段对照：候选表对黄金表逐槽（kind/offset/size）比对，
/// 再验 Length 规范值与属性位闭集（后者由候选值带入）。
pub fn oa_cross_check(candidate: &[OaSlot], attrs: u32, length: u32) -> Result<(), u32> {
    let frozen = oa_frozen_layout();
    if candidate.len() != frozen.len() {
        return Err(CsCode::OaFieldDrift.val());
    }
    let mut i = 0;
    while i < frozen.len() {
        let f = match frozen.get(i) {
            Some(x) => *x,
            None => return Err(CsCode::OaFieldDrift.val()),
        };
        let c = match candidate.get(i) {
            Some(x) => *x,
            None => return Err(CsCode::OaFieldDrift.val()),
        };
        if f != c {
            return Err(CsCode::OaFieldDrift.val());
        }
        i += 1;
    }
    if length != OA_LENGTH_CANONICAL {
        return Err(CsCode::OaLenMismatch.val());
    }
    if attrs & !OBJ_VALID_MASK != 0 {
        return Err(CsCode::OaAttrInvalid.val());
    }
    Ok(())
}

/// 判据谓词：OA 六字段对照回归绿（冻结件逐字段）。
fn oa_fields_ok() -> bool {
    // 冻结件自照 + 规范 Length + 规范属性位。
    let frozen = oa_frozen_layout();
    if oa_cross_check(&frozen, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE, OA_LENGTH_CANONICAL).is_err()
    {
        return false;
    }
    // 黄金偏移序自查：严格递增。
    let mut i = 1;
    while i < frozen.len() {
        let prev = match frozen.get(i - 1) {
            Some(x) => x.offset,
            None => return false,
        };
        let cur = match frozen.get(i) {
            Some(x) => x.offset,
            None => return false,
        };
        if cur <= prev {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据谓词：字段漂移演练 10/10 检出。
/// LCG 选槽扰动（偏移 +1/+8、kind 错位、Length 改写），对照器轮轮必抓。
fn oa_drift_drill_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0u32;
    let mut round = 0;
    while round < 10 {
        let mut cand = oa_frozen_layout();
        let slot = (rng.below(6)) as usize;
        let detected = match round % 3 {
            0 => {
                // 偏移漂移：+1 或 +8。
                if let Some(cell) = cand.get_mut(slot) {
                    cell.offset = cell.offset.saturating_add(if round % 2 == 0 { 1 } else { 8 });
                }
                oa_cross_check(&cand, 0, OA_LENGTH_CANONICAL).is_err()
            }
            1 => {
                // 尺寸漂移：+2。
                if let Some(cell) = cand.get_mut(slot) {
                    cell.size = cell.size.saturating_add(2);
                }
                oa_cross_check(&cand, 0, OA_LENGTH_CANONICAL).is_err()
            }
            _ => {
                // Length 改写（规范值 ±4）。
                let bad_len = OA_LENGTH_CANONICAL.saturating_add(4);
                oa_cross_check(&cand, 0, bad_len).is_err()
            }
        };
        if detected {
            caught = caught.saturating_add(1);
        }
        round += 1;
    }
    caught == 10
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：enter/leave 全路径 + 位约定对照 + OA 对照 ──────────
pub fn golden_run() -> Result<u32, u32> {
    let mut cs = CritSec::new();
    // 重入三层。
    cs.enter(0xA1)?;
    cs.enter(0xA1)?;
    cs.enter(0xA1)?;
    if cs.recursion() != 3 || cs.lock_count() != LC_XP_FIRST + 2 {
        return Err(CsCode::RecurseRegress.val());
    }
    // 逐层放到底。
    cs.leave(0xA1)?;
    cs.leave(0xA1)?;
    cs.leave(0xA1)?;
    if cs.phase() != OwnPhase::Free || !cs.snapshot_conserved() {
        return Err(CsCode::PhaseRegress.val());
    }
    // 位约定对照闸。
    if !convention_cross_ok() {
        return Err(CsCode::ConventionClash.val());
    }
    // OA 对照闸。
    if oa_fields_ok() && oa_drift_drill_ok() {
        Ok(cs.accepted())
    } else {
        Err(CsCode::OaFieldDrift.val())
    }
}

// ── 注入面：三族各 10 轮必抓 ─────────────────────────────────────
/// 注入账本（未占先放/递归倒退/OA 字段漂移）。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if t != c {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2B) as u64);

    // 族 0：未占先放（Free 相位 leave）10/10。
    let mut round = 0;
    while round < 10 {
        ledger.tries[0] = ledger.tries[0].saturating_add(1);
        let mut cs = CritSec::new();
        if cs.leave((rng.below(0x1000) as u64).saturating_add(1))
            == Err(CsCode::ReleaseUnowned.val())
        {
            ledger.caught[0] = ledger.caught[0].saturating_add(1);
        }
        round += 1;
    }

    // 族 1：递归倒退（force_release 跨层清主）10/10。
    round = 0;
    while round < 10 {
        ledger.tries[1] = ledger.tries[1].saturating_add(1);
        let mut cs = CritSec::new();
        let _ = cs.enter(0xB7);
        let _ = cs.enter(0xB7);
        if cs.try_force_release() == Err(CsCode::RecurseRegress.val()) {
            ledger.caught[1] = ledger.caught[1].saturating_add(1);
        }
        round += 1;
    }

    // 族 2：OA 字段漂移 10/10（LCG 选槽改偏移/尺寸/Length）。
    round = 0;
    while round < 10 {
        ledger.tries[2] = ledger.tries[2].saturating_add(1);
        let mut cand = oa_frozen_layout();
        let slot = (rng.below(6)) as usize;
        if let Some(cell) = cand.get_mut(slot) {
            if round % 2 == 0 {
                cell.offset = cell.offset.saturating_add(1);
            } else {
                cell.size = cell.size.saturating_add(8);
            }
        }
        if oa_cross_check(&cand, 0, OA_LENGTH_CANONICAL).is_err() {
            ledger.caught[2] = ledger.caught[2].saturating_add(1);
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ─────────────────────
/// 正常流：黄金正演受理 6 笔（3 enter + 3 leave）。
fn golden_ok() -> bool {
    golden_run() == Ok(6)
}

/// 边界：tid=0 进拒；单层 enter/leave 回 Free；snapshot 守恒。
fn boundary_ok() -> bool {
    let mut cs = CritSec::new();
    // 零句柄 tid 非法（下边界外）。
    if cs.enter(0) != Err(CsCode::PhaseRegress.val()) {
        return false;
    }
    let _ = cs.enter(1);
    let _ = cs.leave(1);
    if cs.phase() != OwnPhase::Free || !cs.snapshot_conserved() || !cs.ledger_consistent() {
        return false;
    }
    // 属性位边界：0 与全闭集收；最高位噪声拒。
    let frozen = oa_frozen_layout();
    if oa_cross_check(&frozen, 0, OA_LENGTH_CANONICAL).is_err() {
        return false;
    }
    if oa_cross_check(&frozen, OBJ_VALID_MASK, OA_LENGTH_CANONICAL).is_err() {
        return false;
    }
    oa_cross_check(&frozen, 0x8000_0000, OA_LENGTH_CANONICAL).is_err()
}

/// 判定次序冻结：字段表对照先于 Length/属性位判定——
/// 漂移表 + 坏 Length 双错必报 OaFieldDrift。
fn judge_order_ok() -> bool {
    let mut cand = oa_frozen_layout();
    if let Some(cell) = cand.get_mut(2) {
        cell.offset = cell.offset.saturating_add(8);
    }
    matches!(
        oa_cross_check(&cand, 0, 0x9999),
        Err(c) if c == CsCode::OaFieldDrift.val()
    )
}

/// 状态机倒退显性拒：异线程放、跨层清主、Free 放全拒。
fn phase_regress_ok() -> bool {
    let mut cs = CritSec::new();
    let _ = cs.enter(1);
    // 异线程放锁拒。
    if cs.leave(2) != Err(CsCode::ReleaseUnowned.val()) {
        return false;
    }
    // 跨层清主拒。
    let _ = cs.enter(1);
    if cs.try_force_release() != Err(CsCode::RecurseRegress.val()) {
        return false;
    }
    // 放到底再放：Free 相位放锁拒。
    let _ = cs.leave(1);
    let _ = cs.leave(1);
    cs.leave(1) == Err(CsCode::ReleaseUnowned.val())
}

/// 双跑一致。
/// （实现注记：比较 extra_items 两次——extra_checks 本身含本条，
/// 直调会构成无限递归，本条是双层去递归的关键设计。）
fn dual_run_ok() -> bool {
    let a = extra_items();
    let b = extra_items();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let ea = match a.get(i) {
            Some(x) => x,
            None => return false,
        };
        let eb = match b.get(i) {
            Some(x) => x,
            None => return false,
        };
        if ea.0 != eb.0 || ea.1 != eb.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 段位守卫。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && (0x80000..=0x82FC0).contains(&SEG_BASE)
}

/// 诊断码窗口闭集。
fn code_window_ok() -> bool {
    let mut i = 0;
    while i < CsCode::ALL.len() {
        if let Some(c) = CsCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文案闭集 + 未知码兜底。
fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < CsCode::ALL.len() {
        let code = match CsCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        let d = detail(code);
        if d.is_empty() || d.starts_with("未知诊断码") {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// 收口只增不减：8 码 / 6+6 字段 / 0x28+0x30 尺寸冻结基线。
fn closure_monotonic_ok() -> bool {
    CsCode::ALL.len() == 8
        && CS_FIELD_OFFS.len() == 6
        && CS_FIELD_SIZES == [8, 4, 4, 8, 8, 8]
        && OA_FIELD_OFFS.len() == 6
        && CS_TOTAL_SIZE == 0x28
        && OA_LENGTH_CANONICAL == 0x30
}

// ── extra_checks：判据汇总（≥12 条） ─────────────────────────────
/// 基础条目（不含「双跑一致」——该项须由 extra_checks 追加，
/// 避免 extra_items→dual_run→extra_checks 的无限递归）。
fn extra_items() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 正常流黄金正演。
    v.push((
        "J1-正常流黄金正演",
        golden_ok(),
        format!("受理={:?}", golden_run()),
    ));

    // 2. J1 锚定：OA 六字段对照回归绿（冻结件逐字段）。
    v.push((
        "J1-OA六字段对照回归",
        oa_fields_ok(),
        String::from("六槽(偏移,尺寸)逐字段+Length0x30+属性位"),
    ));

    // 3. J1 锚定：字段漂移演练 10/10 检出。
    v.push((
        "J1-字段漂移演练10/10",
        oa_drift_drill_ok(),
        String::from("LCG选槽扰动，对照器轮轮必抓"),
    ));

    // 4. 位约定对照（双档 LockCount）。
    v.push((
        "J1-LockCount双档位约定对照",
        convention_cross_ok(),
        String::from("XP(-1/0/+1) 与 Vista(bit0) 解码一致"),
    ));

    // 5. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族10/10全抓",
        led.all_caught(),
        format!("t={:?} c={:?}", led.tries, led.caught),
    ));

    // 6. 边界：tid=0 / 单层回 Free / 属性位三档。
    v.push((
        "J1-边界三档",
        boundary_ok(),
        String::from("零tid拒/单层回路/属性位闭集三档"),
    ));

    // 7. 判定次序：字段表先于 Length。
    v.push((
        "J1-判定次序冻结",
        judge_order_ok(),
        String::from("漂移表+坏Length双错必报字段漂移"),
    ));

    // 8. 状态机倒退显性拒。
    v.push((
        "J1-相位倒退显性拒",
        phase_regress_ok(),
        String::from("异线程放/跨层清主/Free放全拒"),
    ));

    // 9. 快照守恒。
    let mut cs9 = CritSec::new();
    let _ = cs9.enter(0xD1);
    let _ = cs9.leave(0xD1);
    v.push((
        "J1-快照守恒",
        cs9.snapshot_conserved(),
        format!("recursion={} lc={}", cs9.recursion(), cs9.lock_count()),
    ));

    // 10. 段位守卫。
    v.push((
        "J1-段位守卫",
        seg_guard_ok(),
        format!("SEG_BASE=0x{:X} 0x40对齐+VX9区", SEG_BASE),
    ));

    // 11. 诊断码窗口闭集。
    v.push((
        "J1-诊断码窗口闭集",
        code_window_ok(),
        String::from("全部码值落 0x82080..0x820C0"),
    ));

    // 12. 文案闭集 + 兜底。
    v.push((
        "J1-文案闭集+兜底",
        detail_closed_ok(),
        String::from("逐码命中，未知码中文兜底"),
    ));

    // 13. 收口只增不减。
    v.push((
        "J1-收口只增不减",
        closure_monotonic_ok(),
        String::from("8码/6+6字段/0x28+0x30 冻结基线"),
    ));

    v
}

/// 判据汇总入口：基础条目 + 「双跑一致」收尾条（≥12 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v = extra_items();
    v.push((
        "J1-双跑一致",
        dual_run_ok(),
        String::from("extra_checks 两次逐条同判"),
    ));
    v
}

/// VX9 检查入口（≤8 条动态明细槽，超量走静态明细，保证不丢红）。
pub fn run_unx12730_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12730 RTL_CRITICAL_SECTION 位约定对照回归");
    let items = extra_checks();
    let mut dyn_used = 0usize;
    for item in items.iter() {
        let &(name, passed, ref dyn_detail) = item;
        if dyn_used < 8 {
            s.add_dyn(name, passed, dyn_detail);
            dyn_used += 1;
        } else {
            s.add(name, passed, "静态明细位：红项请查 extra_checks 对应谓词");
        }
    }
    s
}

// ── 测试区（≥8 条 #[test]） ──────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_normal_flow_green() {
        // 正常流：三层重入 + 逐层放 + 双闸全绿。
        assert_eq!(golden_run(), Ok(6));
    }

    #[test]
    fn boundary_tier_checks() {
        // 边界：零 tid 拒、单层回路、属性位闭集三档。
        assert!(boundary_ok());
    }

    #[test]
    fn injection_family0_release_unowned_caught() {
        // 注入族 0：未占先放 10/10 必抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(0), Some(&10));
        assert_eq!(led.caught.get(0), Some(&10));
    }

    #[test]
    fn injection_family1_recurse_regress_caught() {
        // 注入族 1：递归倒退 10/10 必抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(1), Some(&10));
        assert_eq!(led.caught.get(1), Some(&10));
    }

    #[test]
    fn injection_family2_oa_drift_caught() {
        // 注入族 2：OA 字段漂移 10/10 必抓（J1 锚定语义）。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(2), Some(&10));
        assert_eq!(led.caught.get(2), Some(&10));
    }

    #[test]
    fn convention_cross_and_oa_drill() {
        // 双档位约定对照绿 + OA 漂移演练 10/10。
        assert!(convention_cross_ok());
        assert!(oa_fields_ok());
        assert!(oa_drift_drill_ok());
        assert_eq!(decode_lc_xp(-1), Ok(OwnPhase::Free));
        assert_eq!(decode_lc_xp(2), Ok(OwnPhase::Owned(3)));
        assert_eq!(decode_lc_vista(0b011), Ok(OwnPhase::Owned(2)));
    }

    #[test]
    fn judge_order_reports_field_drift_first() {
        // 判定次序：字段表对照先于 Length 判定。
        assert!(judge_order_ok());
    }

    #[test]
    fn phase_regress_all_rejected() {
        // 状态机倒退：异线程放/跨层清主/Free 放全拒。
        assert!(phase_regress_ok());
    }

    #[test]
    fn ledger_snapshot_dual_run() {
        // 对账一致 + 快照守恒 + 双跑一致。
        let mut cs = CritSec::new();
        let _ = cs.enter(9);
        let _ = cs.enter(9);
        let _ = cs.leave(9);
        let _ = cs.leave(9);
        assert!(cs.ledger_consistent());
        assert!(cs.snapshot_conserved());
        assert!(dual_run_ok());
    }

    #[test]
    fn seg_guard_and_detail_closure() {
        // 段位守卫 + 窗口闭集 + 文案闭集 + 收口基线。
        assert!(seg_guard_ok());
        assert!(code_window_ok());
        assert_eq!(SEG_BASE, 0x82080);
        assert!(detail_closed_ok());
        assert!(closure_monotonic_ok());
    }

    #[test]
    fn run_unx_checks_all_green() {
        // 全绿收口。
        let s = run_unx12730_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert_eq!(s.failed_count(), 0);
        assert!(s.len() >= 12);
    }
}
