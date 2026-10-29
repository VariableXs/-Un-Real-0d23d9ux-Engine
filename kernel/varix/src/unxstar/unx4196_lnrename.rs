//! unx4196_lnrename — UNX-F4196 符号链接与 rename 竞态锚（诊断段 0x9A00）。
//!
//! 判据原文：J1 链接对象在 rename 窗口内被并发替换时，rename 事务必须以
//!           快照序号裁决——旧目标序号不匹配则整事务回滚且零副作用；
//!           J2 rename 落账后链接名→目标映射恰好一次变更，无双重写；
//!           J3 悬空化竞态：目标在 rename 中途被 unlink 时，事务返回
//!           ENOENT 且链接账与目标账双双守恒；
//!           J4 事件账守恒：attempted = committed + rolled_back + rejected。
//! 设计纪律：
//!   1. Linux VFS 语义：rename 以 dentry 引用钉住源与目标，事务内以
//!      i_rwseq 类序号检测并发变更；本模型以版本号戳（stamp）替代，
//!      任何对链接条目的写操作都推进其 stamp。
//!   2. 事务状态机：Prepared → Validated → Committed / RolledBack，
//!      只允许单向推进；Committed/RolledBack 为终态，禁止二次迁移。
//!   3. 悬空化注入：目标在 Validate 与 Commit 之间被移除时，Commit
//!      阶段重查目标存在性失败 → 整体回滚并返回 ENOENT 语义码。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9A00 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9A00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAMP_RACE: u16 = SEG_BASE + 1;  // 快照序号不匹配（并发替换检出）
const E_GONE: u16 = SEG_BASE + 2;        // 目标在事务中途消失（ENOENT 语义）
const E_DOUBLE_COMMIT: u16 = SEG_BASE + 3; // 终态事务被再次提交
const E_BAD_STATE: u16 = SEG_BASE + 4;   // 状态机非法迁移
const E_BAL: u16 = SEG_BASE + 5;         // 事件账不守恒
const E_NAME_BAD: u16 = SEG_BASE + 6;    // 链接名非法（空/超长）
const E_SELF_LOOP: u16 = SEG_BASE + 7;   // rename 到自身（no-op 语义强制）

/// 链接名长度上限（NAME_MAX=255，POSIX）。
pub const NAME_MAX: usize = 255;

/// 链接条目：名字 → 目标，带版本戳。
pub struct LinkEntry {
    pub name: String,
    pub target: String,
    stamp: u64,
    pub live: bool,
    pub renames: u32,
}

impl LinkEntry {
    fn new(name: &str, target: &str) -> Result<LinkEntry, u16> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        Ok(LinkEntry {
            name: name.to_string(),
            target: target.to_string(),
            stamp: 1,
            live: true,
            renames: 0,
        })
    }
    /// 读版本戳（事务裁决依据）。
    pub fn stamp(&self) -> u64 {
        self.stamp
    }
    /// 推进版本戳（任何写路径必经）。
    fn bump(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
    }
}

/// rename 事务状态机：Prepared → Validated → Committed/RolledBack 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxnState {
    Prepared,
    Validated,
    Committed,
    RolledBack,
}

impl TxnState {
    fn can_advance(self, to: TxnState) -> bool {
        use TxnState::*;
        matches!(
            (self, to),
            (Prepared, Validated) | (Validated, Committed) | (Validated, RolledBack)
        )
    }
}

/// rename 事务：钉住源条目的快照序号，提交前重验。
/// （owned 账面快照，不借用注册表——事务与表解耦，validate/commit 时重验。）
pub struct RenameTxn {
    pub src_idx: usize,
    pub dst_name: String,
    src_stamp: u64,
    state: TxnState,
}

/// 链接注册表 + 竞态裁决枢纽。
pub struct LinkTable {
    entries: Vec<LinkEntry>,
    /// 事务账：attempted = committed + rolled_back + rejected
    pub attempted: u32,
    pub committed: u32,
    pub rolled_back: u32,
    pub rejected: u32,
    /// 竞态检出计数（stamp 不匹配导致的回滚）
    pub races: u32,
    /// 悬空化检出计数（目标中途消失）
    pub vanish: u32,
}

impl LinkTable {
    pub fn new() -> LinkTable {
        LinkTable {
            entries: Vec::new(),
            attempted: 0,
            committed: 0,
            rolled_back: 0,
            rejected: 0,
            races: 0,
            vanish: 0,
        }
    }

    /// 挂链接（symlink 语义）。
    pub fn put(&mut self, name: &str, target: &str) -> Result<usize, u16> {
        if name.is_empty() || name.len() > NAME_MAX {
            self.rejected += 1;
            self.attempted += 1;
            return Err(E_NAME_BAD);
        }
        for (i, e) in self.entries.iter().enumerate() {
            if e.live && e.name == name {
                // 已存在同名：拒绝（symlink 不覆盖）
                self.rejected += 1;
                self.attempted += 1;
                return Err(E_NAME_BAD);
            }
            let _ = i;
        }
        let e = LinkEntry::new(name, target)?;
        self.entries.push(e);
        Ok(self.entries.len() - 1)
    }

    /// 取当前 stamp（外部观察窗口）。
    pub fn stamp_of(&self, idx: usize) -> Result<u64, u16> {
        if idx >= self.entries.len() {
            return Err(E_GONE);
        }
        Ok(self.entries[idx].stamp())
    }

    /// 制备 rename 事务：快照源条目 stamp。
    pub fn prepare(&mut self, src_idx: usize, dst_name: &str) -> Result<RenameTxn, u16> {
        self.attempted += 1;
        if dst_name.is_empty() || dst_name.len() > NAME_MAX {
            self.rejected += 1;
            return Err(E_NAME_BAD);
        }
        if src_idx >= self.entries.len() || !self.entries[src_idx].live {
            self.rejected += 1;
            return Err(E_GONE);
        }
        if self.entries[src_idx].name == dst_name {
            self.rejected += 1;
            return Err(E_SELF_LOOP);
        }
        let st = self.entries[src_idx].stamp();
        Ok(RenameTxn {
            src_idx,
            dst_name: dst_name.to_string(),
            src_stamp: st,
            state: TxnState::Prepared,
        })
    }

    /// 事务内推进状态（带非法迁移拦截）。
    pub fn advance(&mut self, txn: &mut RenameTxn, to: TxnState) -> Result<(), u16> {
        if !txn.state.can_advance(to) {
            return Err(E_BAD_STATE);
        }
        txn.state = to;
        Ok(())
    }

    /// 验证阶段：重读 stamp，检查目标名占用与目标条目存在性。
    pub fn validate(&mut self, txn: &mut RenameTxn) -> Result<(), u16> {
        if txn.state != TxnState::Prepared {
            return Err(E_BAD_STATE);
        }
        // stamp 重验：被并发写过的源条目判竞态
        let cur = self.entries[txn.src_idx].stamp();
        if cur != txn.src_stamp {
            self.races += 1;
            txn.state = TxnState::RolledBack;
            self.rolled_back += 1;
            return Err(E_STAMP_RACE);
        }
        // 目标名占用检查（rename 允许覆盖 live 目标，但本锚要求显式声明
        // ——不声明覆盖即拒绝，保证恰好一次变更可证）
        for e in self.entries.iter() {
            if e.live && e.name == txn.dst_name {
                self.rejected += 1;
                txn.state = TxnState::RolledBack;
                self.rolled_back += 1;
                return Err(E_NAME_BAD);
            }
        }
        txn.state = TxnState::Validated;
        Ok(())
    }

    /// 提交阶段：Commit 前重验存在性（悬空化竞态点）。
    pub fn commit(&mut self, txn: &mut RenameTxn) -> Result<(), u16> {
        if txn.state != TxnState::Validated {
            return Err(E_BAD_STATE);
        }
        // 悬空化注入点：源条目若已消失，整体回滚（判据 J3）
        if txn.src_idx >= self.entries.len() || !self.entries[txn.src_idx].live {
            self.vanish += 1;
            txn.state = TxnState::RolledBack;
            self.rolled_back += 1;
            return Err(E_GONE);
        }
        // stamp 终验
        let cur = self.entries[txn.src_idx].stamp();
        if cur != txn.src_stamp {
            self.races += 1;
            txn.state = TxnState::RolledBack;
            self.rolled_back += 1;
            return Err(E_STAMP_RACE);
        }
        // 恰好一次变更：改名 + bump
        let e = &mut self.entries[txn.src_idx];
        e.name = txn.dst_name.clone();
        e.renames += 1;
        e.bump();
        txn.state = TxnState::Committed;
        self.committed += 1;
        Ok(())
    }

    /// 外部并发写（模拟竞态窗口内的替换）：推进 stamp。
    pub fn concurrent_touch(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.entries.len() {
            return Err(E_GONE);
        }
        self.entries[idx].bump();
        Ok(())
    }

    /// 移除链接（unlink 语义）：live=false + bump。
    pub fn unlink(&mut self, name: &str) -> Result<(), u16> {
        for e in self.entries.iter_mut() {
            if e.live && e.name == name {
                e.live = false;
                e.bump();
                return Ok(());
            }
        }
        Err(E_GONE)
    }

    /// 事件账守恒（判据 J4）。
    pub fn event_balanced(&self) -> bool {
        self.attempted == self.committed + self.rolled_back + self.rejected
    }

    /// live 条目数。
    pub fn live_count(&self) -> usize {
        self.entries.iter().filter(|e| e.live).count()
    }

    /// 链接账守恒：live 数 ≤ 总数，且 committed 的 renames 计数总和一致。
    pub fn ledger_conserved(&self) -> bool {
        let total_renames: u32 = self.entries.iter().map(|e| e.renames).sum();
        total_renames == self.committed && self.live_count() <= self.entries.len()
    }
}

/// 自例程：R1 正常 rename 闭环；R2 stamp 竞态回滚；R3 悬空化竞态；
/// R4 非法迁移拦截；R5 事件账与链接账双守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：正常 rename——prepare→validate→commit，恰好一次变更。
    let mut t = LinkTable::new();
    let i = t.put("a", "/etc/hosts")?;
    t.put("b", "/tmp/x")?;
    let mut tx = t.prepare(i, "c")?;
    t.advance(&mut tx, TxnState::Validated)?;
    t.validate(&mut tx)?;
    if tx.state != TxnState::Validated {
        return Err(E_BAD_STATE);
    }
    t.advance(&mut tx, TxnState::Committed)?;
    t.commit(&mut tx)?;
    if t.committed != 1 || t.live_count() != 2 || !t.ledger_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：stamp 竞态——prepare 后并发 touch，commit 必须回滚且零改名。
    let j = t.put("d", "/var/log")?;
    let st_before = t.stamp_of(j)?;
    let mut tx2 = t.prepare(j, "e")?;
    t.concurrent_touch(j)?; // 竞态窗口写
    if t.stamp_of(j)? == st_before {
        return Err(E_STAMP_RACE); // touch 未生效 = 自例程失真
    }
    t.advance(&mut tx2, TxnState::Validated)?;
    let r = t.validate(&mut tx2);
    if r != Err(E_STAMP_RACE) || tx2.state != TxnState::RolledBack {
        return Err(E_STAMP_RACE);
    }
    if t.races != 1 || t.rolled_back != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：悬空化——prepare 后 unlink 源条目，commit 必须回滚 ENOENT。
    let k = t.put("f", "/run/pid")?;
    let mut tx3 = t.prepare(k, "g")?;
    t.advance(&mut tx3, TxnState::Validated)?;
    t.validate(&mut tx3)?;
    t.unlink("f")?; // 事务中途目标消失
    t.advance(&mut tx3, TxnState::Committed)?;
    let r3 = t.commit(&mut tx3);
    if r3 != Err(E_GONE) || t.vanish != 1 {
        return Err(E_GONE);
    }
    hits += 1;

    // R4：非法迁移——Committed 终态不得二次 commit；Prepared 不得直接 commit。
    let m = t.put("h", "/dev/null")?;
    let mut tx4 = t.prepare(m, "i")?;
    t.advance(&mut tx4, TxnState::Validated)?;
    t.validate(&mut tx4)?;
    t.advance(&mut tx4, TxnState::Committed)?;
    t.commit(&mut tx4)?;
    if t.commit(&mut tx4) != Err(E_BAD_STATE) {
        return Err(E_DOUBLE_COMMIT);
    }
    let mut tx5 = t.prepare(m, "j")?;
    if t.commit(&mut tx5) != Err(E_BAD_STATE) {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R5：注入拦截 + 双守恒终验。
    if t.put("", "/x").is_ok() {
        return Err(E_NAME_BAD);
    }
    let long = "x".repeat(NAME_MAX + 1);
    if t.put(&long, "/x").is_ok() {
        return Err(E_NAME_BAD);
    }
    // rename 到自身
    let n = t.put("k", "/proc/self")?;
    if t.prepare(n, "k").is_ok() {
        return Err(E_SELF_LOOP);
    }
    if !t.event_balanced() || !t.ledger_conserved() {
        return Err(E_BAL);
    }
    // 账目完全对账：attempted 分解闭合
    let sum = t.committed + t.rolled_back + t.rejected;
    if sum != t.attempted {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
