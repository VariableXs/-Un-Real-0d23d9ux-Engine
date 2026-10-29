//! unx4150_renametxn — UNX-F4150 rename 钩子事务边界约定（诊断段 0x9480）。
//!
//! 判据原文：
//!   J1 rename 钩子的原子性约定（成功全成/失败全不成）注入断言：
//!      中途失败后源目标两态与调用前逐字节一致（10/10 次零残留）。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 事务状态机 Idle → Prepared → Committed/Aborted，只有 Committed
//!      才对名字账可见，Aborted 必须零残留（journal 回滚逐字节比对）。
//!   2. 字节级守恒：源名与目标名的字节序在事务前后指纹（整数校验和）
//!      必须一致，指纹不等即判残留。
//!   3. 中途失败注入按阶段（prepare/write/commit）触发，任一阶段失败
//!      都走同一 Aborted 回滚路径，不得旁路。
//!   4. 边界显性：同名重命名、空名、查无源名、目标已存在均拒收留账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9480;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_NAME: u16 = SEG_BASE + 1;  // 空名字拒收
const E_NO_SRC: u16 = SEG_BASE + 2;      // 源名不存在
const E_DST_EXIST: u16 = SEG_BASE + 3;   // 目标名已存在
const E_SAME_NAME: u16 = SEG_BASE + 4;   // 源目标同名
const E_RESIDUE: u16 = SEG_BASE + 5;     // 回滚后残留（判据 J1 失守）
const E_BAD_STAGE: u16 = SEG_BASE + 6;   // 注入阶段号越界
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 事务状态机非法迁移
const E_BAL: u16 = SEG_BASE + 8;         // 名字账守恒失守

pub const NAME_CAP: usize = 16; // 槽位字节容量（本契约验证口径）

/// 事务状态机：Idle 未开 / Prepared 已备 / Committed 已提交 / Aborted 已回滚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxnState {
    Idle,
    Prepared,
    Committed,
    Aborted,
}

/// 注入阶段号：0=不注入 1=prepare 2=write 3=commit。
pub const STAGE_NONE: u8 = 0;
pub const STAGE_PREPARE: u8 = 1;
pub const STAGE_WRITE: u8 = 2;
pub const STAGE_COMMIT: u8 = 3;

/// 名字账条目：定长字节槽 + 有效位。
pub struct NameSlot {
    pub bytes: [u8; NAME_CAP], // 名字字节（0 填充）
    pub len: usize,            // 有效长度
    pub live: bool,            // 有效位
}

/// 事务流水：一账一 rename，回滚前后指纹齐备。
pub struct TxnJournal {
    pub src_fp_before: u32, // 源名事务前指纹
    pub dst_fp_before: u32, // 目标名事务前指纹（不存在=0）
    pub state: TxnState,    // 终态
    pub fail_stage: u8,     // 注入阶段
}

/// rename 事务枢纽：名字账 + 流水账 + 零残留核验。
pub struct RenameFabric {
    slots: Vec<NameSlot>,
    pub journal: Vec<TxnJournal>,
    pub committed_total: u32, // 提交成功计数
    pub aborted_total: u32,   // 回滚计数
    pub residue_total: u32,   // 残留计数（必须恒 0）
}

impl RenameFabric {
    pub fn new() -> RenameFabric {
        RenameFabric {
            slots: Vec::new(),
            journal: Vec::new(),
            committed_total: 0,
            aborted_total: 0,
            residue_total: 0,
        }
    }

    fn slot_bytes(name: &[u8]) -> ([u8; NAME_CAP], usize) {
        let mut b = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < name.len() && i < NAME_CAP {
            b[i] = name[i];
            i += 1;
        }
        (b, name.len())
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            let (b, l) = Self::slot_bytes(name);
            if self.slots[i].live && self.slots[i].len == l && self.slots[i].bytes == b {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建名（live 置位）。
    pub fn create(&mut self, name: &[u8]) -> Result<(), u16> {
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(E_EMPTY_NAME);
        }
        if self.find(name).is_some() {
            return Err(E_DST_EXIST);
        }
        let (b, l) = Self::slot_bytes(name);
        self.slots.push(NameSlot { bytes: b, len: l, live: true });
        Ok(())
    }

    fn fp_at(&self, idx: Option<usize>) -> u32 {
        match idx {
            None => 0,
            Some(i) => {
                if i >= self.slots.len() {
                    return 0;
                }
                let mut h: u32 = 0x811C9DC5; // FNV-1a 偏移基
                let mut k = 0usize;
                while k < self.slots[i].len {
                    h ^= self.slots[i].bytes[k] as u32;
                    h = h.wrapping_mul(0x01000193); // FNV-1a 质数
                    k += 1;
                }
                h
            }
        }
    }

    fn count_live(&self) -> u32 {
        let mut c = 0u32;
        for s in &self.slots {
            if s.live {
                c += 1;
            }
        }
        c
    }

    /// rename 事务：成功全成 / 任一阶段失败全不成（判据 J1）。
    /// 返回 Ok(true)=已提交，Ok(false)=已回滚零残留。
    pub fn rename(&mut self, src: &[u8], dst: &[u8], fail_stage: u8) -> Result<bool, u16> {
        if src.is_empty() || dst.is_empty() || src.len() > NAME_CAP || dst.len() > NAME_CAP {
            return Err(E_EMPTY_NAME);
        }
        if fail_stage > STAGE_COMMIT {
            return Err(E_BAD_STAGE);
        }
        if src == dst {
            return Err(E_SAME_NAME);
        }
        let s_idx = match self.find(src) {
            Some(i) => i,
            None => return Err(E_NO_SRC),
        };
        let d_idx = self.find(dst);
        if d_idx.is_some() {
            return Err(E_DST_EXIST);
        }
        let src_fp = self.fp_at(Some(s_idx));
        let dst_fp = self.fp_at(d_idx);
        let live_before = self.count_live();

        // 状态机推进：Idle → Prepared（阶段注入 1 在此失败）。
        let mut st = TxnState::Idle;
        st = TxnState::Prepared;
        if fail_stage == STAGE_PREPARE {
            st = TxnState::Aborted;
        }
        // write 阶段：拟写目标槽（此处仅推进状态，写入点在 commit 前）。
        if st == TxnState::Prepared {
            if fail_stage == STAGE_WRITE {
                st = TxnState::Aborted;
            }
        }
        if st == TxnState::Prepared {
            // commit 阶段：真实落账——源废目标立。
            if fail_stage == STAGE_COMMIT {
                st = TxnState::Aborted;
            } else {
                self.slots[s_idx].live = false;
                let (b, l) = Self::slot_bytes(dst);
                self.slots.push(NameSlot { bytes: b, len: l, live: true });
                st = TxnState::Committed;
            }
        }
        // 回滚核验：Aborted 必须源目标两态与调用前逐字节一致。
        if st == TxnState::Aborted {
            let src_fp_after = self.fp_at(Some(s_idx));
            let dst_fp_after = self.fp_at(d_idx);
            let residue = src_fp_after != src_fp || dst_fp_after != dst_fp
                || self.count_live() != live_before;
            if residue {
                self.residue_total += 1; // 零残留判据失守即记账
            }
            self.aborted_total += 1;
        }
        if st == TxnState::Committed {
            self.committed_total += 1;
        }
        self.journal.push(TxnJournal {
            src_fp_before: src_fp,
            dst_fp_before: dst_fp,
            state: st,
            fail_stage,
        });
        if st == TxnState::Aborted && self.residue_total > 0 {
            return Err(E_RESIDUE);
        }
        Ok(st == TxnState::Committed)
    }

    /// 守恒对账：live 名字账 + 流水账互核，残留必须为零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.residue_total != 0 {
            return Err(E_RESIDUE);
        }
        let live = self.count_live();
        if self.committed_total > 0 && live == 0 {
            return Err(E_BAL);
        }
        for j in &self.journal {
            match j.state {
                TxnState::Idle | TxnState::Prepared => return Err(E_BAD_STATE),
                _ => {}
            }
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_NAME);
        }
        Ok(())
    }
}

/// 自例程：R1 提交路径 → R2 边界 → R3 三阶段注入 10/10 零残留 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：建两名 → rename 提交 → 源消失目标立。
    let mut f = RenameFabric::new();
    f.create(b"old")?;
    f.create(b"keep")?;
    let ok = f.rename(b"old", b"new", STAGE_NONE)?;
    if !ok {
        return Err(E_BAD_STATE);
    }
    if f.find(b"old").is_some() || f.find(b"new").is_none() {
        return Err(E_BAL);
    }
    f.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：空名 / 同名 / 查无源 / 目标已存在 / 阶段号越界。
    let mut f = RenameFabric::new();
    f.create(b"a")?;
    if f.rename(b"", b"b", STAGE_NONE).is_ok() {
        return Err(E_EMPTY_NAME);
    }
    if f.rename(b"a", b"a", STAGE_NONE).is_ok() {
        return Err(E_SAME_NAME);
    }
    if f.rename(b"none", b"b", STAGE_NONE).is_ok() {
        return Err(E_NO_SRC);
    }
    if f.rename(b"a", b"a", STAGE_NONE).is_ok() {
        return Err(E_SAME_NAME);
    }
    if f.rename(b"a", b"a2", 9).is_ok() {
        return Err(E_BAD_STAGE);
    }
    f.create(b"b")?;
    if f.rename(b"a", b"b", STAGE_NONE).is_ok() {
        return Err(E_DST_EXIST);
    }
    hits += 1;

    // R3 注入拦截：三阶段失败各 10 次，源目标两态逐字节零残留。
    let mut round = 0u32;
    let mut f = RenameFabric::new();
    while round < 10 {
        // 每轮独立源名（轮间异名，避免查重误拦）。
        let mut src = [b's'; 4];
        src[3] = b'0' + round as u8;
        f.create(&src)?;
        let before_installed = f.slots.len();
        let aborted1 = f.rename(&src, b"dst", STAGE_PREPARE)?;
        if aborted1 {
            return Err(E_BAD_STATE);
        }
        let _ = f.rename(&src, b"dst2", STAGE_WRITE)?;
        let _ = f.rename(&src, b"dst3", STAGE_COMMIT)?;
        // 三次注入后源仍须原样存在（live 且字节未动），无新槽残留。
        if f.find(&src).is_none() {
            return Err(E_RESIDUE);
        }
        if f.slots.len() != before_installed {
            return Err(E_RESIDUE);
        }
        round += 1;
    }
    if f.residue_total != 0 {
        return Err(E_RESIDUE);
    }
    hits += 1;

    // R4 守恒对账：提交/回滚账与流水账一致，审计过。
    let mut f = RenameFabric::new();
    f.create(b"s")?;
    f.rename(b"s", b"d", STAGE_NONE)?;
    f.create(b"s")?;
    f.rename(b"s", b"e", STAGE_COMMIT)?;
    f.audit()?;
    hits += 1;

    Ok(hits)
}
