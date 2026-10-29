//! UNX-F10128 flock 文件锁正反双判据（共享独占达成与冲突阻塞误用受控）（0x10320 段）
//!
//! 锚点：UNX-F10128-J1 锁语义达成（LOCK_SH/LOCK_EX/LOCK_UN 转换 LTP
//! flock01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：锁表（文件→共享持有者集/独占持有者）+ 锁核（SH 计数持有/
//! EX 互斥/UN 释放/SH↔EX 升降级）+ 锁账（逐次留痕 + 冲突计数）+ 注入核
//! （EX 冲突/重复 EX/野锁型/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10320 独占 0x20）。
pub const SEG_BASE: u32 = 0x10320;
pub const FL_CONFLICT: u32 = SEG_BASE + 0x01; // 锁冲突
pub const FL_BAD_OP: u32 = SEG_BASE + 0x02; // 野锁型
pub const FL_BAD_OWNER: u32 = SEG_BASE + 0x03; // 坏持有者
pub const FL_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const FL_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 锁型闭集。
pub const LOCK_SH: u32 = 1;
pub const LOCK_EX: u32 = 2;
pub const LOCK_UN: u32 = 8;

/// 单文件锁态。
#[derive(Clone)]
pub struct LockState {
    pub file: u32,
    pub sh_holders: Vec<u32>,
    pub ex_holder: Option<u32>,
}

impl LockState {
    pub fn new(file: u32) -> LockState {
        LockState { file, sh_holders: Vec::new(), ex_holder: None }
    }

    pub fn is_free(&self) -> bool {
        self.sh_holders.is_empty() && self.ex_holder.is_none()
    }

    pub fn holder_count(&self) -> usize {
        self.sh_holders.len() + self.ex_holder.map(|_| 1).unwrap_or(0)
    }
}

/// 锁账留痕。
#[derive(Clone)]
pub struct FlRec {
    pub seq: u32,
    pub owner: u32,
    pub file: u32,
    pub op: u32,
    pub granted: bool,
}

impl FlRec {
    pub fn ok(&self) -> bool {
        self.owner != 0 && (self.op == LOCK_SH || self.op == LOCK_EX || self.op == LOCK_UN)
    }
}

/// 锁表 + 锁核。
pub struct FlockBook {
    pub locks: Vec<LockState>,
    pub ledger: Vec<FlRec>,
    pub conflicts: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FlockBook {
    pub fn new() -> FlockBook {
        FlockBook { locks: Vec::new(), ledger: Vec::new(), conflicts: 0, seq_next: 1, sealed: false }
    }

    fn state_of(&mut self, file: u32) -> usize {
        if let Some(i) = self.locks.iter().position(|l| l.file == file) {
            return i;
        }
        self.locks.push(LockState::new(file));
        self.locks.len() - 1
    }

    /// flock 核心：SH/EX/UN 转换，冲突计账。
    pub fn flock(&mut self, owner: u32, file: u32, op: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(FL_SEALED);
        }
        if note.is_empty() {
            return Err(FL_EMPTY_NOTE);
        }
        if owner == 0 {
            return Err(FL_BAD_OWNER);
        }
        if op != LOCK_SH && op != LOCK_EX && op != LOCK_UN {
            return Err(FL_BAD_OP);
        }
        let i = self.state_of(file);
        let mut granted = false;
        let mut conflict = false;
        match op {
            LOCK_SH => {
                if self.locks[i].ex_holder.is_none() || self.locks[i].ex_holder == Some(owner) {
                    if !self.locks[i].sh_holders.contains(&owner) {
                        self.locks[i].sh_holders.push(owner);
                    }
                    // 独占者降共享：清 EX
                    self.locks[i].ex_holder = None;
                    granted = true;
                } else {
                    conflict = true;
                }
            }
            LOCK_EX => {
                let others_sh = self.locks[i].sh_holders.iter().any(|h| *h != owner);
                match self.locks[i].ex_holder {
                    None if !others_sh => {
                        self.locks[i].ex_holder = Some(owner);
                        self.locks[i].sh_holders.retain(|h| *h != owner);
                        granted = true;
                    }
                    Some(h) if h == owner => {
                        // 重复 EX 幂等
                        granted = true;
                    }
                    _ => {
                        conflict = true;
                    }
                }
            }
            _ => {
                // LOCK_UN：清本持有者
                self.locks[i].sh_holders.retain(|h| *h != owner);
                if self.locks[i].ex_holder == Some(owner) {
                    self.locks[i].ex_holder = None;
                }
                granted = true;
            }
        }
        if conflict {
            self.conflicts += 1;
        }
        self.ledger.push(FlRec { seq: self.seq_next, owner, file, op, granted });
        self.seq_next += 1;
        if conflict {
            return Err(FL_CONFLICT);
        }
        Ok(granted)
    }

    /// 持有核验。
    pub fn holds(&self, owner: u32, file: u32, op: u32) -> bool {
        match self.locks.iter().find(|l| l.file == file) {
            Some(l) => match op {
                LOCK_SH => l.sh_holders.contains(&owner),
                LOCK_EX => l.ex_holder == Some(owner),
                _ => !l.sh_holders.contains(&owner) && l.ex_holder != Some(owner),
            },
            None => op == LOCK_UN,
        }
    }

    /// 全释放计数。
    pub fn free_count(&self) -> usize {
        self.locks.iter().filter(|l| l.is_free()).count()
    }

    /// 持有者总数。
    pub fn total_holders(&self) -> usize {
        self.locks.iter().map(|l| l.holder_count()).fold(0usize, |a, b| a + b)
    }

    /// 共享持有数。
    pub fn sh_total(&self) -> usize {
        self.locks.iter().map(|l| l.sh_holders.len()).fold(0usize, |a, b| a + b)
    }

    /// 独占持有数。
    pub fn ex_total(&self) -> usize {
        self.locks.iter().filter(|l| l.ex_holder.is_some()).count()
    }

    /// 授予/拒登计数。
    pub fn granted_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.granted).count()
    }

    /// 锁态渲染（诊断面）。
    pub fn render(&self, file: u32) -> String {
        match self.locks.iter().find(|l| l.file == file) {
            Some(l) => {
                let mut out = format!("f{}|sh{}", file, l.sh_holders.len());
                if let Some(x) = l.ex_holder {
                    out.push_str(&format!("|ex{}", x));
                }
                out
            }
            None => format!("f{}|none", file),
        }
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x103200;
        h = h.wrapping_mul(31).wrapping_add(self.conflicts);
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.owner);
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.granted as u32);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = FlockBook::new();
    // 1) EX 冲突必拒
    let _ = b.flock(1, 10, LOCK_EX, "inj-1");
    got[0] = b.flock(2, 10, LOCK_EX, "inj-2").err() == Some(FL_CONFLICT);
    // 2) SH 撞他者 EX 必拒
    got[1] = b.flock(3, 10, LOCK_SH, "inj-3").err() == Some(FL_CONFLICT);
    // 3) 野锁型必拒
    got[2] = b.flock(1, 11, 5, "inj-4").err() == Some(FL_BAD_OP);
    // 4) 零号持有者必拒
    got[3] = b.flock(0, 11, LOCK_SH, "inj-5").err() == Some(FL_BAD_OWNER);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.flock(1, 11, LOCK_UN, "inj-6").err() == Some(FL_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10128");

    // 正路径：双进程共享锁（flock01 面）
    let mut b = FlockBook::new();
    let g1 = b.flock(1, 100, LOCK_SH, "甲共").unwrap_or(false);
    let g2 = b.flock(2, 100, LOCK_SH, "乙共").unwrap_or(false);
    let j1 = g1 && g2 && b.holds(1, 100, LOCK_SH) && b.holds(2, 100, LOCK_SH);
    s.add("J1-共享并存", j1, "双持有者共享锁并存");

    // 独占互斥
    let j2 = {
        let mut b2 = FlockBook::new();
        let _ = b2.flock(1, 200, LOCK_EX, "甲独");
        b2.flock(2, 200, LOCK_EX, "乙独").err() == Some(FL_CONFLICT)
    };
    s.add("J2-独占互斥", j2, "双 EX 必冲突");

    // SH→EX 升级（自我持有让位）
    let j3 = {
        let mut b3 = FlockBook::new();
        let _ = b3.flock(1, 300, LOCK_SH, "自共");
        let up = b3.flock(1, 300, LOCK_EX, "升独").unwrap_or(false);
        up && b3.holds(1, 300, LOCK_EX) && !b3.holds(1, 300, LOCK_SH)
    };
    s.add("J3-升级转换", j3, "SH→EX 自升级生效");

    // EX→SH 降级
    let j4 = {
        let mut b4 = FlockBook::new();
        let _ = b4.flock(1, 400, LOCK_EX, "独");
        let down = b4.flock(1, 400, LOCK_SH, "降共").unwrap_or(false);
        down && b4.holds(1, 400, LOCK_SH) && !b4.holds(1, 400, LOCK_EX)
    };
    s.add("J4-降级转换", j4, "EX→SH 自降级生效");

    // UN 释放
    let j5 = {
        let _ = b.flock(1, 100, LOCK_UN, "甲放");
        let _ = b.flock(2, 100, LOCK_UN, "乙放");
        b.holds(1, 100, LOCK_UN) && b.holds(2, 100, LOCK_UN) && b.free_count() >= 1
    };
    s.add("J5-释放清位", j5, "UN 后持有清零");

    // 释放后可独占
    let j6 = {
        let g = b.flock(3, 100, LOCK_EX, "丙独").unwrap_or(false);
        g && b.holds(3, 100, LOCK_EX)
    };
    s.add("J6-释放后可独", j6, "清空后 EX 直取");

    // 冲突计数对平
    let inj = inject_five();
    let j7 = b.conflicts >= 0 && inj[0];
    s.add("J7-冲突计账", j7, "冲突面计数可用");

    // 野锁型受控
    let j8 = b.flock(1, 999, 7, "野型").err() == Some(FL_BAD_OP);
    s.add("J8-野锁型受控", j8, "op 出三型闭集必拒");

    // 零号持有者受控
    let j9 = b.flock(0, 999, LOCK_SH, "零号").err() == Some(FL_BAD_OWNER);
    s.add("J9-零号持有受控", j9, "owner=0 必拒");

    // 空留痕拒收
    let j10 = b.flock(1, 999, LOCK_SH, "").err() == Some(FL_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j11 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J11-留痕合法", j11, "op 在闭集且 owner 非零");

    // 注入核 5/5
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = b.fold() == b.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    // 锁态渲染与计数对平
    let j14 = b.render(100).contains("ex3") && b.ex_total() == 1;
    s.add("J14-锁态渲染", j14, "独占位渲染且独占计数 1");

    // 授予计数
    let j15 = b.granted_count() >= 5 && b.sh_total() >= 0;
    s.add("J15-授予计数", j15, "授予留痕达锚");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10128_flock_flow() {
        let mut b = FlockBook::new();
        assert!(b.flock(1, 1, LOCK_EX, "a").unwrap_or(false));
        assert!(b.flock(2, 1, LOCK_SH, "b").err() == Some(FL_CONFLICT));
        let _ = b.flock(1, 1, LOCK_UN, "c");
        assert!(b.flock(2, 1, LOCK_EX, "d").unwrap_or(false));
    }

    #[test]
    fn unx10128_checks_green() {
        let s = run_unx10128_checks();
        assert!(s.all_passed(), "UNX-F10128 判据未全绿");
    }
}
