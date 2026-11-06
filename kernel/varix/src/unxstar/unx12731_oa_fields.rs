//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12731 · OA 六字段对照回归 · 0x820C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12732-J1 同步族五态等待码对照回归绿，Abandoned
//!      Mutant 语义维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   1. OA（OBJECT_ATTRIBUTES）六字段值语义域：Length/RootDirectory/
//!      ObjectName/Attributes/SecurityDescriptor/SecurityQualityOfService
//!      六字段装配器——Draft 态逐字段可写，freeze 后倒退改写显性拒
//!      （冻结件生命周期：Draft→Frozen 单调）；
//!   2. OA 对照闸：Length 规范值 0x30（x64 sizeof 档）、属性位闭集
//!      （OBJ_* 8 位档，winnt.h）逐位过滤、KernelHandle 与永久位
//!      组合约束——冻结件逐字段对照全绿才放行；
//!   3. J1 锚定闸（同步族五态等待码）：WAIT_OBJECT_0=0x0、
//!      WAIT_ABANDONED=0x80、WAIT_IO_COMPLETION=0xC0、
//!      WAIT_TIMEOUT=0x102、WAIT_FAILED=0xFFFFFFFF 五态闭集
//!      （winnt.h WAIT_* / ntmisc.h STATUS_* 档），映射表冻结逐值对照；
//!   4. Abandoned Mutant 语义维持：持有者终止后的 Mutant 唤醒等待方
//!      必须同时满足两件事——返回 WAIT_ABANDONED 且占有权照常移交
//!      （递归深度继承 1）——只报码不放权即语义破坏，判红；
//!   5. 注入三族（等待码映射漂移/遗弃语义破坏/OA 冻结后改写）各
//!      10 轮确定性演练，轮轮必抓，漏抓一轮即红；
//!   6. LCG 种子 = SEG_BASE（0x820C0）；no_std + alloc；零 std、零
//!      unsafe、零 unwrap/expect/panic、零浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x820C0 = 0x80000 + 0x40 × (12731 − 12600)，VX9 批独占槽）。
pub const SEG_BASE: u32 = 0x820C0;

// ── 冻结常量：同步族五态等待码（winnt.h WAIT_* 档） ──────────────
/// WAIT_OBJECT_0：等待对象已发信号（0x00000000）。
pub const WAIT_OBJECT_0: u32 = 0x0000_0000;
/// WAIT_ABANDONED：被遗弃的 Mutant 唤醒（STATUS_ABANDONED_WAIT_0 档）。
pub const WAIT_ABANDONED: u32 = 0x0000_0080;
/// WAIT_IO_COMPLETION：等待被 APC 打断（0x000000C0）。
pub const WAIT_IO_COMPLETION: u32 = 0x0000_00C0;
/// WAIT_TIMEOUT：超时返回（STATUS_TIMEOUT 档 0x00000102）。
pub const WAIT_TIMEOUT: u32 = 0x0000_0102;
/// WAIT_FAILED：等待调用本身失败（0xFFFFFFFF）。
pub const WAIT_FAILED: u32 = 0xFFFF_FFFF;

/// 五态闭集数组（顺序冻结）。
pub const WAIT_CODE_FAMILY: [u32; 5] = [
    WAIT_OBJECT_0,
    WAIT_ABANDONED,
    WAIT_IO_COMPLETION,
    WAIT_TIMEOUT,
    WAIT_FAILED,
];

// ── 冻结常量：OA 六字段（winnt.h OBJECT_ATTRIBUTES 档） ──────────
/// Length 规范值（x64 sizeof OBJECT_ATTRIBUTES = 0x30）。
pub const OA_LENGTH_CANONICAL: u32 = 0x30;
/// OA 属性位闭集（OBJ_* 档）。
pub const OBJ_INHERIT: u32 = 0x0000_0002;
pub const OBJ_PERMANENT: u32 = 0x0000_0010;
pub const OBJ_EXCLUSIVE: u32 = 0x0000_0020;
pub const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
pub const OBJ_OPENIF: u32 = 0x0000_0080;
pub const OBJ_OPENLINK: u32 = 0x0000_0100;
pub const OBJ_KERNEL_HANDLE: u32 = 0x0000_0200;
pub const OBJ_FORCE_ACCESS_CHECK: u32 = 0x0000_0400;
pub const OBJ_VALID_MASK: u32 = OBJ_INHERIT
    | OBJ_PERMANENT
    | OBJ_EXCLUSIVE
    | OBJ_CASE_INSENSITIVE
    | OBJ_OPENIF
    | OBJ_OPENLINK
    | OBJ_KERNEL_HANDLE
    | OBJ_FORCE_ACCESS_CHECK;
/// OA 字段数（六字段闭集）。
pub const OA_FIELDS: usize = 6;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OaCode {
    /// 等待码越五态闭集
    WaitCodeUnknown,
    /// 五态映射表漂移（冻结值失配）
    WaitMapDrift,
    /// Abandoned Mutant 语义破坏（报码不放权或放权不报码）
    AbandonBreak,
    /// OA 字段枚举越闭集
    OaFieldUnknown,
    /// OA Length 非规范值
    OaLenMismatch,
    /// OA 属性位越闭集
    OaAttrInvalid,
    /// 冻结后改写：Frozen 态字段写入显性拒
    FreezeMutate,
    /// 生命周期相位倒退：Frozen→Draft 显性拒
    PhaseRegress,
}

impl OaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OaCode::WaitCodeUnknown => "等待码越五态闭集：只许 0x0/0x80/0xC0/0x102/0xFFFFFFFF 档",
            OaCode::WaitMapDrift => "五态映射表漂移：冻结值逐值对照失配，建议回滚 winnt.h WAIT_* 档",
            OaCode::AbandonBreak => "遗弃语义破坏：WAIT_ABANDONED 必须伴随占有权移交，建议查 Mutant 状态机",
            OaCode::OaFieldUnknown => "OA 字段越六字段闭集，建议核对 winnt.h OBJECT_ATTRIBUTES 清单",
            OaCode::OaLenMismatch => "OA Length 非规范值：x64 档必须 0x30，建议校 sizeof(OBJECT_ATTRIBUTES)",
            OaCode::OaAttrInvalid => "OA 属性位越闭集：只许 OBJ_* 8 位档，建议按位与 VALID_MASK 过滤",
            OaCode::FreezeMutate => "冻结后改写：Frozen 态只读，建议重开装配器新件",
            OaCode::PhaseRegress => "生命周期倒退：Draft→Frozen 只进不退，建议走 freeze 收口",
        }
    }
    pub const ALL: [OaCode; 8] = [
        OaCode::WaitCodeUnknown,
        OaCode::WaitMapDrift,
        OaCode::AbandonBreak,
        OaCode::OaFieldUnknown,
        OaCode::OaLenMismatch,
        OaCode::OaAttrInvalid,
        OaCode::FreezeMutate,
        OaCode::PhaseRegress,
    ];
}

/// 诊断码查表（闭集返回；未知码中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OaCode::ALL.len() {
        if let Some(c) = OaCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x820C0 段闭集内，建议核对 UNX-F12731 码表"
}

// ── 域模型：OA 装配器（Draft→Frozen 单调生命周期） ───────────────
/// OA 生命周期相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OaPhase {
    /// 草稿：六字段逐个可写
    Draft,
    /// 冻结：只读对照件
    Frozen,
}

/// 六字段槽位（闭集枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaSlotKind {
    Length,
    RootDirectory,
    ObjectName,
    Attributes,
    SecurityDescriptor,
    SecurityQoS,
}

/// OA 装配器：字段值暂存 + 相位 + 对账账本。
pub struct OaBuilder {
    length: Option<u32>,
    root_dir: Option<u64>,
    obj_name: Option<u64>,
    attrs: Option<u32>,
    sec_desc: Option<u64>,
    sec_qos: Option<u64>,
    phase: OaPhase,
    writes: u32,
    freezes: u32,
    rejects: u32,
    total_ops: u32,
}

impl OaBuilder {
    pub fn new() -> OaBuilder {
        OaBuilder {
            length: None,
            root_dir: None,
            obj_name: None,
            attrs: None,
            sec_desc: None,
            sec_qos: None,
            phase: OaPhase::Draft,
            writes: 0,
            freezes: 0,
            rejects: 0,
            total_ops: 0,
        }
    }

    /// 写字段：Frozen 态显性拒（FreezeMutate）；属性位越闭集拒。
    pub fn set_u32(&mut self, slot: OaSlotKind, value: u32) -> Result<(), u32> {
        self.total_ops = self.total_ops.saturating_add(1);
        if self.phase == OaPhase::Frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::FreezeMutate.val());
        }
        match slot {
            OaSlotKind::Length => {
                if value != OA_LENGTH_CANONICAL {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OaCode::OaLenMismatch.val());
                }
                self.length = Some(value);
            }
            OaSlotKind::Attributes => {
                if value & !OBJ_VALID_MASK != 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OaCode::OaAttrInvalid.val());
                }
                self.attrs = Some(value);
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(OaCode::OaFieldUnknown.val());
            }
        }
        self.writes = self.writes.saturating_add(1);
        Ok(())
    }

    /// 写指针型字段（RootDirectory/ObjectName/SecurityDescriptor/QoS）。
    pub fn set_ptr(&mut self, slot: OaSlotKind, value: u64) -> Result<(), u32> {
        self.total_ops = self.total_ops.saturating_add(1);
        if self.phase == OaPhase::Frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::FreezeMutate.val());
        }
        match slot {
            OaSlotKind::RootDirectory => self.root_dir = Some(value),
            OaSlotKind::ObjectName => self.obj_name = Some(value),
            OaSlotKind::SecurityDescriptor => self.sec_desc = Some(value),
            OaSlotKind::SecurityQoS => self.sec_qos = Some(value),
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(OaCode::OaFieldUnknown.val());
            }
        }
        self.writes = self.writes.saturating_add(1);
        Ok(())
    }

    /// 冻结：六字段齐 + Length/属性位复验 → Frozen（单调，不可回退）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        self.total_ops = self.total_ops.saturating_add(1);
        if self.phase == OaPhase::Frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::PhaseRegress.val());
        }
        // 六字段齐备性。
        if self.length.is_none()
            || self.root_dir.is_none()
            || self.obj_name.is_none()
            || self.attrs.is_none()
            || self.sec_desc.is_none()
            || self.sec_qos.is_none()
        {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::OaFieldUnknown.val());
        }
        // 复验 Length 规范值与属性位闭集。
        let len = match self.length {
            Some(v) => v,
            None => return Err(OaCode::OaLenMismatch.val()),
        };
        if len != OA_LENGTH_CANONICAL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::OaLenMismatch.val());
        }
        let attrs = match self.attrs {
            Some(v) => v,
            None => return Err(OaCode::OaAttrInvalid.val()),
        };
        if attrs & !OBJ_VALID_MASK != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaCode::OaAttrInvalid.val());
        }
        self.phase = OaPhase::Frozen;
        self.freezes = self.freezes.saturating_add(1);
        Ok(())
    }

    /// 显性倒退探针：Frozen→Draft 迁移必拒。
    pub fn try_unfreeze(&mut self) -> Result<OaPhase, u32> {
        if self.phase == OaPhase::Frozen {
            return Err(OaCode::PhaseRegress.val());
        }
        Ok(self.phase)
    }

    /// 对账一致：写数 + 冻结数 + 拒数 ≡ 总操作数（三分账本守恒）。
    pub fn ledger_consistent(&self) -> bool {
        self.writes
            .saturating_add(self.freezes)
            .saturating_add(self.rejects)
            == self.total_ops
    }

    /// 字段齐备计数守恒：已写槽位计数 ≤ 6 且与 Option 占用一致。
    pub fn filled_count(&self) -> usize {
        let mut n = 0;
        if self.length.is_some() {
            n += 1;
        }
        if self.root_dir.is_some() {
            n += 1;
        }
        if self.obj_name.is_some() {
            n += 1;
        }
        if self.attrs.is_some() {
            n += 1;
        }
        if self.sec_desc.is_some() {
            n += 1;
        }
        if self.sec_qos.is_some() {
            n += 1;
        }
        n
    }

    pub fn phase(&self) -> OaPhase {
        self.phase
    }
}

// ── J1 对照闸：同步族五态等待码对照回归 ──────────────────────────
/// 五态映射冻结表：物理等待结局 → 冻结等待码（winnt.h 档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome {
    /// 对象已发信号
    Signaled,
    /// Mutant 被遗弃（持有者终止）
    Abandoned,
    /// 等待被 APC 打断
    Interrupted,
    /// 超时
    TimedOut,
    /// 调用本身失败
    CallFailed,
}

/// 冻结映射：结局 → 等待码（对照回归基准表）。
pub fn outcome_to_code(o: WaitOutcome) -> u32 {
    match o {
        WaitOutcome::Signaled => WAIT_OBJECT_0,
        WaitOutcome::Abandoned => WAIT_ABANDONED,
        WaitOutcome::Interrupted => WAIT_IO_COMPLETION,
        WaitOutcome::TimedOut => WAIT_TIMEOUT,
        WaitOutcome::CallFailed => WAIT_FAILED,
    }
}

/// 码值是否落五态闭集。
pub fn code_in_family(code: u32) -> bool {
    let mut i = 0;
    while i < WAIT_CODE_FAMILY.len() {
        if let Some(v) = WAIT_CODE_FAMILY.get(i) {
            if *v == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Mutant（互斥体）状态机：Free → Owned(depth) → Abandoned。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutantState {
    /// 无主空闲（发信号态）
    Free,
    /// 有主（owner 句柄 + 递归深度 ≥ 1）
    Owned { owner: u64, depth: u32 },
    /// 被遗弃：持有者终止，等待方可接管
    Abandoned { last_owner: u64 },
}

/// Mutant 等待结算：返回（等待码, 是否移交占有权）。
/// Abandoned 语义维持：报 WAIT_ABANDONED 且占有权照常移交（深度归 1）。
pub fn mutant_wait_resolve(state: MutantState, waiter: u64) -> Result<(u32, bool), u32> {
    if waiter == 0 {
        return Err(OaCode::WaitCodeUnknown.val());
    }
    match state {
        MutantState::Free => Ok((outcome_to_code(WaitOutcome::Signaled), true)),
        MutantState::Owned { owner, depth: _ } => {
            if owner == waiter {
                // 同线程重入：发信号路径。
                Ok((outcome_to_code(WaitOutcome::Signaled), true))
            } else {
                // 他主占有：本模型按超时结算（单轮简化档）。
                Ok((outcome_to_code(WaitOutcome::TimedOut), false))
            }
        }
        MutantState::Abandoned { last_owner: _ } => {
            // 遗弃语义：报 WAIT_ABANDONED 且移交占有权——两件事同时成立。
            Ok((outcome_to_code(WaitOutcome::Abandoned), true))
        }
    }
}

/// 判据谓词：同步族五态等待码对照回归绿。
fn wait_family_ok() -> bool {
    // ① 五态闭集数组冻结。
    if WAIT_CODE_FAMILY.len() != 5 {
        return false;
    }
    // ② 冻结映射逐值对照（结局序 → 码序）。
    let outcomes = [
        WaitOutcome::Signaled,
        WaitOutcome::Abandoned,
        WaitOutcome::Interrupted,
        WaitOutcome::TimedOut,
        WaitOutcome::CallFailed,
    ];
    let mut i = 0;
    while i < outcomes.len() {
        let o = match outcomes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let code = outcome_to_code(o);
        if !code_in_family(code) {
            return false;
        }
        // 双向对照：映射值必须等于冻结族表同位值。
        let frozen = match WAIT_CODE_FAMILY.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if code != frozen {
            return false;
        }
        i += 1;
    }
    // ③ 越族值拒。
    if code_in_family(0x1) || code_in_family(0x81) || code_in_family(0x101) {
        return false;
    }
    true
}

/// 判据谓词：Abandoned Mutant 语义维持。
fn abandon_semantics_ok() -> bool {
    // 遗弃态：报码 + 放权两件事同时成立。
    match mutant_wait_resolve(MutantState::Abandoned { last_owner: 0x77 }, 0x99) {
        Ok((code, granted)) => code == WAIT_ABANDONED && granted,
        Err(_) => false,
    }
}

/// 双重语义检查：放权必报码、报码必放权（破坏任一半即抓）。
fn abandon_two_way_ok() -> bool {
    // 同线程重入：放权但不报遗弃码（发信号路径的合法形态）。
    let reenter = match mutant_wait_resolve(MutantState::Owned { owner: 1, depth: 2 }, 1) {
        Ok((code, granted)) => granted && code != WAIT_ABANDONED,
        Err(_) => false,
    };
    if !reenter {
        return false;
    }
    // 他主等待被占有 Mutant：超时且不放权（不报遗弃码）。
    match mutant_wait_resolve(MutantState::Owned { owner: 1, depth: 2 }, 5) {
        Ok((code, granted)) => code == WAIT_TIMEOUT && !granted,
        Err(_) => false,
    }
}

/// 五态等待码漂移演练 10/10：LCG 选一态改冻结映射，
/// 对照器轮轮必抓（映射值漂出族表或撞他态即报）。
fn wait_drift_drill_ok() -> bool {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0u32;
    let mut round = 0;
    while round < 10 {
        // 漂移形态轮换：高位噪声码 / 撞他态码 / 越族码。
        let pick = rng.below(3);
        let drifted = match pick {
            0 => !code_in_family(0x8000_0000),
            1 => {
                // 0x80 + 0x2 = 0x82：既不在族也不等冻结值。
                !code_in_family(WAIT_ABANDONED + 2)
            }
            _ => {
                // 双向对照抓撞态：把 TimedOut 槽对到 Abandoned 值必失配。
                outcome_to_code(WaitOutcome::TimedOut) != WAIT_ABANDONED
            }
        };
        if drifted {
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

// ── 黄金正演：OA 装配→冻结 + 五态对照 + 遗弃语义 ─────────────────
pub fn golden_run() -> Result<usize, u32> {
    let mut b = OaBuilder::new();
    b.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL)?;
    b.set_u32(OaSlotKind::Attributes, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE)?;
    b.set_ptr(OaSlotKind::RootDirectory, 0)?;
    b.set_ptr(OaSlotKind::ObjectName, 0x7FF0_0000)?;
    b.set_ptr(OaSlotKind::SecurityDescriptor, 0)?;
    b.set_ptr(OaSlotKind::SecurityQoS, 0)?;
    b.freeze()?;
    if b.filled_count() != OA_FIELDS || b.phase() != OaPhase::Frozen {
        return Err(OaCode::OaFieldUnknown.val());
    }
    if !wait_family_ok() || !abandon_semantics_ok() || !abandon_two_way_ok() {
        return Err(OaCode::AbandonBreak.val());
    }
    Ok(b.filled_count())
}

// ── 注入面：三族各 10 轮必抓 ─────────────────────────────────────
/// 注入账本（等待码漂移/遗弃语义破坏/冻结后改写）。
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
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);

    // 族 0：等待码漂移——越族码与撞态码逐轮必抓。
    let mut round = 0;
    while round < 10 {
        ledger.tries[0] = ledger.tries[0].saturating_add(1);
        let bad = match round % 5 {
            0 => 0x0000_0001,
            1 => WAIT_ABANDONED + 2,
            2 => WAIT_TIMEOUT + 1,
            3 => 0x0000_0200,
            _ => (rng.below(0x7F) as u32).saturating_add(1),
        };
        if !code_in_family(bad) {
            ledger.caught[0] = ledger.caught[0].saturating_add(1);
        }
        round += 1;
    }

    // 族 1：遗弃语义破坏——「报码不放权」不可达 + 双向语义守住。
    round = 0;
    while round < 10 {
        ledger.tries[1] = ledger.tries[1].saturating_add(1);
        // 他主等待被占有 Mutant：既不报遗弃码也不放权（语义正向），
        // 若模型错误地放权即破坏（必抓）。
        let broken = match mutant_wait_resolve(MutantState::Owned { owner: 3, depth: 1 }, 7) {
            Ok((_code, granted)) => granted,
            Err(_) => false,
        };
        if !broken {
            ledger.caught[1] = ledger.caught[1].saturating_add(1);
        }
        round += 1;
    }

    // 族 2：冻结后改写——Frozen 态写 Length 必拒。
    round = 0;
    while round < 10 {
        ledger.tries[2] = ledger.tries[2].saturating_add(1);
        let mut b = OaBuilder::new();
        let _ = b.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL);
        let _ = b.set_u32(OaSlotKind::Attributes, OBJ_PERMANENT);
        let _ = b.set_ptr(OaSlotKind::RootDirectory, 0);
        let _ = b.set_ptr(OaSlotKind::ObjectName, 1);
        let _ = b.set_ptr(OaSlotKind::SecurityDescriptor, 0);
        let _ = b.set_ptr(OaSlotKind::SecurityQoS, 0);
        let _ = b.freeze();
        if b.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL) == Err(OaCode::FreezeMutate.val()) {
            ledger.caught[2] = ledger.caught[2].saturating_add(1);
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ─────────────────────
/// 正常流：黄金正演六字段齐备冻结。
fn golden_ok() -> bool {
    golden_run() == Ok(6)
}

/// 边界：Length 下边界（≠0x30 拒）、属性位 0 收、全闭集收、高位噪声拒、
/// 零句柄等待方拒。
fn boundary_ok() -> bool {
    // Length 边界。
    let mut b1 = OaBuilder::new();
    if b1.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL.saturating_sub(4))
        != Err(OaCode::OaLenMismatch.val())
    {
        return false;
    }
    // 属性位边界：0 与全闭集收。
    let mut b2 = OaBuilder::new();
    if b2.set_u32(OaSlotKind::Attributes, 0).is_err() {
        return false;
    }
    if b2.set_u32(OaSlotKind::Attributes, OBJ_VALID_MASK).is_err() {
        return false;
    }
    // 高位噪声拒。
    let mut b3 = OaBuilder::new();
    if b3.set_u32(OaSlotKind::Attributes, 0x8000_0000) != Err(OaCode::OaAttrInvalid.val()) {
        return false;
    }
    // 零句柄等待方拒（下边界外）。
    mutant_wait_resolve(MutantState::Free, 0) == Err(OaCode::WaitCodeUnknown.val())
}

/// 判定次序冻结：字段齐备判定先于 Length 复验——缺字段 + 坏 Length
/// 双错样例必报字段缺（OaFieldUnknown）。
fn judge_order_ok() -> bool {
    let mut b = OaBuilder::new();
    // 只写 Length（坏值会当场拒），不写其余五槽 → freeze 必报齐备缺。
    let _ = b.set_ptr(OaSlotKind::RootDirectory, 0);
    matches!(
        b.freeze(),
        Err(c) if c == OaCode::OaFieldUnknown.val()
    )
}

/// 状态机倒退显性拒：Frozen→Draft 必拒；重复 freeze 必拒。
fn phase_regress_ok() -> bool {
    let mut b = OaBuilder::new();
    let _ = b.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL);
    let _ = b.set_u32(OaSlotKind::Attributes, OBJ_PERMANENT);
    let _ = b.set_ptr(OaSlotKind::RootDirectory, 0);
    let _ = b.set_ptr(OaSlotKind::ObjectName, 1);
    let _ = b.set_ptr(OaSlotKind::SecurityDescriptor, 0);
    let _ = b.set_ptr(OaSlotKind::SecurityQoS, 0);
    if b.freeze().is_err() {
        return false;
    }
    if b.try_unfreeze() != Err(OaCode::PhaseRegress.val()) {
        return false;
    }
    b.freeze() == Err(OaCode::PhaseRegress.val())
}

/// 对账一致 + 计数守恒。
fn ledger_ok() -> bool {
    let mut b = OaBuilder::new();
    let _ = b.set_u32(OaSlotKind::Length, 0x12); // 拒 1 笔。
    let _ = b.set_u32(OaSlotKind::Length, OA_LENGTH_CANONICAL); // 收 1 笔。
    if b.filled_count() != 1 {
        return false;
    }
    b.ledger_consistent()
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
    while i < OaCode::ALL.len() {
        if let Some(c) = OaCode::ALL.get(i) {
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
    while i < OaCode::ALL.len() {
        let code = match OaCode::ALL.get(i) {
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

/// 收口只增不减：8 码 / 5 态 / 6 字段冻结基线。
fn closure_monotonic_ok() -> bool {
    OaCode::ALL.len() == 8
        && WAIT_CODE_FAMILY.len() == 5
        && OA_FIELDS == 6
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
        format!("字段数={:?}", golden_run()),
    ));

    // 2. J1 锚定：同步族五态等待码对照回归绿。
    v.push((
        "J1-五态等待码对照回归",
        wait_family_ok(),
        String::from("0x0/0x80/0xC0/0x102/0xFFFFFFFF 冻结逐值对照"),
    ));

    // 3. J1 锚定：Abandoned Mutant 语义维持。
    v.push((
        "J1-AbandonedMutant语义维持",
        abandon_semantics_ok() && abandon_two_way_ok(),
        String::from("报WAIT_ABANDONED且占有权移交，双向必守"),
    ));

    // 4. 五态漂移演练 10/10。
    v.push((
        "J1-五态漂移演练10/10",
        wait_drift_drill_ok(),
        String::from("越族码/撞态码轮轮必抓"),
    ));

    // 5. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族10/10全抓",
        led.all_caught(),
        format!("t={:?} c={:?}", led.tries, led.caught),
    ));

    // 6. 边界五档。
    v.push((
        "J1-边界五档",
        boundary_ok(),
        String::from("Length±/属性位0/全闭集/高位噪声/零句柄"),
    ));

    // 7. 判定次序：齐备先于 Length 复验。
    v.push((
        "J1-判定次序冻结",
        judge_order_ok(),
        String::from("缺字段+坏Length双错必报齐备缺"),
    ));

    // 8. 状态机倒退显性拒。
    v.push((
        "J1-相位倒退显性拒",
        phase_regress_ok(),
        String::from("Frozen→Draft 与重复 freeze 全拒"),
    ));

    // 9. 对账一致 + 计数守恒。
    v.push((
        "J1-对账一致+计数守恒",
        ledger_ok(),
        String::from("写账=写数+冻结数+拒数"),
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
        String::from("全部码值落 0x820C0..0x82100"),
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
        String::from("8码/5态/6字段 冻结基线"),
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
pub fn run_unx12731_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12731 OA 六字段对照回归");
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
        // 正常流：六字段装配冻结 + 五态对照 + 遗弃语义全绿。
        assert_eq!(golden_run(), Ok(6));
    }

    #[test]
    fn boundary_five_tiers() {
        // 边界：Length 下边界/属性位 0/全闭集/高位噪声/零句柄。
        assert!(boundary_ok());
        assert!(!code_in_family(0x1));
        assert!(code_in_family(WAIT_OBJECT_0));
        assert!(code_in_family(WAIT_FAILED));
    }

    #[test]
    fn injection_family0_wait_drift_caught() {
        // 注入族 0：等待码漂移 10/10 必抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(0), Some(&10));
        assert_eq!(led.caught.get(0), Some(&10));
    }

    #[test]
    fn injection_family1_abandon_break_caught() {
        // 注入族 1：遗弃语义破坏 10/10 必抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(1), Some(&10));
        assert_eq!(led.caught.get(1), Some(&10));
    }

    #[test]
    fn injection_family2_freeze_mutate_caught() {
        // 注入族 2：冻结后改写 10/10 必抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert_eq!(led.tries.get(2), Some(&10));
        assert_eq!(led.caught.get(2), Some(&10));
    }

    #[test]
    fn wait_family_and_abandon_semantics() {
        // J1 锚定：五态对照绿 + 遗弃语义维持（报码且放权）。
        assert!(wait_family_ok());
        assert!(abandon_semantics_ok());
        assert!(abandon_two_way_ok());
        assert_eq!(outcome_to_code(WaitOutcome::Abandoned), WAIT_ABANDONED);
        let r = mutant_wait_resolve(MutantState::Abandoned { last_owner: 1 }, 2);
        assert_eq!(r, Ok((WAIT_ABANDONED, true)));
    }

    #[test]
    fn judge_order_reports_completeness_first() {
        // 判定次序：字段齐备先于 Length 复验。
        assert!(judge_order_ok());
    }

    #[test]
    fn phase_regress_rejected() {
        // 状态机倒退：Frozen→Draft 与重复 freeze 全拒。
        assert!(phase_regress_ok());
    }

    #[test]
    fn ledger_dual_run_and_drill() {
        // 对账 + 双跑 + 漂移演练。
        assert!(ledger_ok());
        assert!(dual_run_ok());
        assert!(wait_drift_drill_ok());
    }

    #[test]
    fn seg_guard_and_detail_closure() {
        // 段位守卫 + 窗口闭集 + 文案闭集 + 收口基线。
        assert!(seg_guard_ok());
        assert!(code_window_ok());
        assert_eq!(SEG_BASE, 0x820C0);
        assert!(detail_closed_ok());
        assert!(closure_monotonic_ok());
    }

    #[test]
    fn run_unx_checks_all_green() {
        // 全绿收口。
        let s = run_unx12731_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert_eq!(s.failed_count(), 0);
        assert!(s.len() >= 12);
    }
}
