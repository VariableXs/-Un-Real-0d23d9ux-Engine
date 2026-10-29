//! UNX-F48557 · E28 与 B5/J5 持久层边界声明（journal 本体不二写）
//!
//! 判据原文：UNX-F48557-J1 —— 持久层边界冻结：E28 恢复面对 journal
//! 只读重放（读路径唯一），写路径全部委托 B5/J5 持久层（writer 身份
//! 唯一）；恢复器任何写尝试被拒（E28 不二写 journal 本体）；越权写
//! 检出并记账；journal 单写入方审计（append 记录 writer 身份，非持久
//! 层身份全拒）；边界声明常量冻结；同序列重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF4C0;

pub const E_WRITE_DENIED: u16 = SEG_BASE + 1;
pub const E_WRITER_CLASH: u16 = SEG_BASE + 2;
pub const E_RO_VIOLATION: u16 = SEG_BASE + 3;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_BOUND_FROZEN: u16 = SEG_BASE + 6;

/// writer 身份白名单。
pub const W_PERSIST: u32 = 1;
pub const W_RECOVERY: u32 = 2;

/// 边界声明：E28 对 journal 只读。
pub const E28_JOURNAL_RO: bool = true;

/// journal 记录。
#[derive(Clone, PartialEq, Eq)]
pub struct BRec {
    pub key: u32,
    pub val: u64,
    pub writer: u32,
}

/// 受管 journal：append 校验 writer 身份。
pub struct ManagedJournal {
    pub recs: Vec<BRec>,
    pub denied: u64,
}

impl ManagedJournal {
    pub fn new() -> ManagedJournal {
        ManagedJournal { recs: Vec::new(), denied: 0 }
    }

    /// 持久层专用追加。
    pub fn append_by_persist(&mut self, key: u32, val: u64) -> Result<(), u16> {
        self.recs.push(BRec { key, val, writer: W_PERSIST });
        Ok(())
    }

    /// 通用追加：仅持久层身份放行。
    pub fn append(&mut self, writer: u32, key: u32, val: u64) -> Result<(), u16> {
        if writer != W_PERSIST {
            self.denied += 1;
            return Err(E_WRITE_DENIED);
        }
        self.recs.push(BRec { key, val, writer });
        Ok(())
    }

    /// 单写入方审计：全部记录 writer == W_PERSIST。
    pub fn audit_single_writer(&self) -> Result<(), u16> {
        for r in self.recs.iter() {
            if r.writer != W_PERSIST {
                return Err(E_WRITER_CLASH);
            }
        }
        Ok(())
    }
}

/// 恢复面只读句柄：只暴露重放，不暴露写。
pub struct ReadOnlyJournal {
    recs: Vec<BRec>,
}

impl ReadOnlyJournal {
    pub fn from(j: &ManagedJournal) -> ReadOnlyJournal {
        let mut v = Vec::new();
        for r in j.recs.iter() {
            v.push(r.clone());
        }
        ReadOnlyJournal { recs: v }
    }
    pub fn replay_count(&self) -> usize {
        self.recs.len()
    }
    pub fn state_fp(&self) -> u64 {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= self.recs.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for r in self.recs.iter() {
            h ^= r.key as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= r.val;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 恢复器：尝试写 journal 必被拒（边界执法点）。
pub struct RecoveryAgent {
    pub replayed: u32,
    pub write_attempts: u64,
    pub denied_writes: u64,
}

impl RecoveryAgent {
    pub fn new() -> RecoveryAgent {
        RecoveryAgent { replayed: 0, write_attempts: 0, denied_writes: 0 }
    }

    /// 重放：只读消费。
    pub fn replay(&mut self, ro: &ReadOnlyJournal) -> u32 {
        self.replayed = ro.replay_count() as u32;
        self.replayed
    }

    /// 恢复器写尝试：必然被拒（不二写执法）。
    pub fn attempt_write(&mut self, j: &mut ManagedJournal, key: u32, val: u64) -> Result<(), u16> {
        self.write_attempts += 1;
        match j.append(W_RECOVERY, key, val) {
            Err(E_WRITE_DENIED) => {
                self.denied_writes += 1;
                Err(E_RO_VIOLATION)
            }
            Err(e) => Err(e),
            Ok(()) => Err(E_RO_VIOLATION),
        }
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 边界声明冻结：E28_JOURNAL_RO == true。
pub fn r1_boundary_frozen() -> Result<(), u16> {
    if !E28_JOURNAL_RO {
        return Err(E_BOUND_FROZEN);
    }
    Ok(())
}

/// R2 持久层写入正常：append_by_persist 全部成功。
pub fn r2_persist_write_ok() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    for i in 0..10u32 {
        j.append_by_persist(i % 4, i as u64 * 3)?;
    }
    if j.recs.len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    j.audit_single_writer()
}

/// R3 恢复器写被拒：attempt_write 三连全拒。
pub fn r3_recovery_write_denied() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    j.append_by_persist(1, 1)?;
    let mut a = RecoveryAgent::new();
    for i in 0..3u32 {
        match a.attempt_write(&mut j, i, i as u64) {
            Err(E_RO_VIOLATION) => {}
            _ => return Err(E_RO_VIOLATION),
        }
    }
    if a.denied_writes != 3 || a.write_attempts != 3 {
        return Err(E_LEDGER_BAD);
    }
    // journal 尺寸未变（写未穿透）。
    if j.recs.len() != 1 {
        return Err(E_RO_VIOLATION);
    }
    if j.denied != 3 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R4 非持久层身份全拒：W_RECOVERY 与未知身份都拒。
pub fn r4_writer_gate() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    match j.append(W_RECOVERY, 1, 1) {
        Err(E_WRITE_DENIED) => {}
        _ => return Err(E_WRITE_DENIED),
    }
    match j.append(99, 1, 1) {
        Err(E_WRITE_DENIED) => {}
        _ => return Err(E_WRITE_DENIED),
    }
    match j.append(0, 1, 1) {
        Err(E_WRITE_DENIED) => {}
        _ => return Err(E_WRITE_DENIED),
    }
    if j.recs.len() != 0 {
        return Err(E_WRITER_CLASH);
    }
    Ok(())
}

/// R5 只读句柄零写接口：from 后重放数与原 journal 一致、状态指纹等。
pub fn r5_readonly_handle() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    for i in 0..8u32 {
        j.append_by_persist(i % 3, i as u64)?;
    }
    let fp_before = {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= j.recs.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for r in j.recs.iter() {
            h ^= r.key as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= r.val;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let ro = ReadOnlyJournal::from(&j);
    let mut a = RecoveryAgent::new();
    let n = a.replay(&ro);
    if n != 8 {
        return Err(E_LEDGER_BAD);
    }
    if ro.state_fp() != fp_before {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R6 拒写不影响重放：拒写后重放仍全量。
pub fn r6_denied_no_effect() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    for i in 0..6u32 {
        j.append_by_persist(i, i as u64)?;
    }
    let mut a = RecoveryAgent::new();
    let _ = a.attempt_write(&mut j, 9, 9);
    let ro = ReadOnlyJournal::from(&j);
    let n = a.replay(&ro);
    if n != 6 {
        return Err(E_RO_VIOLATION);
    }
    Ok(())
}

/// R7 写入方身份记账：全记录 writer == W_PERSIST（越权未穿透）。
pub fn r7_writer_ledger() -> Result<(), u16> {
    let mut g = Lcg::new(0x48557_0001);
    let mut j = ManagedJournal::new();
    for _ in 0..20u64 {
        let key = (g.next() % 5) as u32;
        j.append_by_persist(key, g.next())?;
        // 尝试越权穿插。
        let _ = j.append(W_RECOVERY, key, g.next());
    }
    j.audit_single_writer()?;
    if j.recs.len() != 20 || j.denied != 20 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R8 fuzz：随机写身份 50 轮，仅持久层穿透。
pub fn r8_fuzz_writer() -> Result<(), u16> {
    let mut g = Lcg::new(0x48557_0002);
    let mut j = ManagedJournal::new();
    let mut accepted = 0;
    for _ in 0..50u64 {
        let w = if g.below(2) == 0 { W_PERSIST } else { W_RECOVERY };
        match j.append(w, (g.next() % 7) as u32, g.next()) {
            Ok(()) => accepted += 1,
            Err(E_WRITE_DENIED) => {}
            Err(e) => return Err(e),
        }
    }
    if accepted != 25 {
        return Err(E_LEDGER_BAD);
    }
    j.audit_single_writer()
}

/// R9 重放确定性：同 journal 两轮重放计数与指纹全等。
pub fn r9_replay_deterministic() -> Result<(), u16> {
    let mut j = ManagedJournal::new();
    for i in 0..15u32 {
        j.append_by_persist(i % 5, i as u64 * 7)?;
    }
    let ro = ReadOnlyJournal::from(&j);
    let mut a = RecoveryAgent::new();
    let mut b = RecoveryAgent::new();
    let na = a.replay(&ro);
    let nb = b.replay(&ro);
    if na != nb || ro.state_fp() == 0 {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 边界三常量冻结：W_PERSIST/W_RECOVERY/E28_JOURNAL_RO。
pub fn r10_constants_frozen() -> Result<(), u16> {
    if W_PERSIST != 1 || W_RECOVERY != 2 {
        return Err(E_BOUND_FROZEN);
    }
    if W_PERSIST == W_RECOVERY {
        return Err(E_BOUND_FROZEN);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_boundary_frozen()?;
    hits += 1;
    r2_persist_write_ok()?;
    hits += 1;
    r3_recovery_write_denied()?;
    hits += 1;
    r4_writer_gate()?;
    hits += 1;
    r5_readonly_handle()?;
    hits += 1;
    r6_denied_no_effect()?;
    hits += 1;
    r7_writer_ledger()?;
    hits += 1;
    r8_fuzz_writer()?;
    hits += 1;
    r9_replay_deterministic()?;
    hits += 1;
    r10_constants_frozen()?;
    hits += 1;
    Ok(hits)
}
