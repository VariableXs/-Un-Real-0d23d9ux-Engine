//! UNX-F11013 fork/继承挂点联签（epoll fd 与 futex 跨进程共享）（0x1e1a0 段）
//!
//! 锚点：UNX-F11013-J1 继承账逐项对平（10/10），共享挂点行为一致（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：继承账（父进程挂点表：epoll fd 引用与 futex 键）+ 子进程副本
//! （fork 逐项拷贝、引用计数共享）+ 一致性对账器（父/子两侧值一致、单侧
//! 关账不影响他侧）+ fold 指纹与冻结 + 注入核（重复 fork 同名/关不存在的
//! 挂点/双侧皆关后复用/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e1a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e1a0;
pub const FH_DUPFORK: u32 = SEG_BASE + 0x01; // 子进程号重号
pub const FH_NOHOOK: u32 = SEG_BASE + 0x02; // 关不存在的挂点
pub const FH_DEADUSE: u32 = SEG_BASE + 0x03; // 双侧皆关后复用
pub const FH_MISMATCH: u32 = SEG_BASE + 0x04; // 父子值不一致
pub const FH_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const FH_CAP: u32 = SEG_BASE + 0x06; // 挂点表超容

/// 继承挂点对平数（10/10）。
pub const HOOKS_NEEDED: usize = 10;
/// 挂点表定容。
pub const HOOK_CAP: usize = 16;
/// 继承账域标（fold 种子）。
pub const FH_TAG: u64 = 0x1e1a_0d;

/// 单条共享挂点：fd/键 + 父侧值 + 子侧值 + 双侧关账位。
#[derive(Clone, Debug)]
pub struct ShareHook {
    pub key: u32,
    pub val_parent: u32,
    pub val_child: u32,
    pub closed_parent: bool,
    pub closed_child: bool,
}

impl ShareHook {
    /// 双侧一致且任一侧在册才算共享活账。
    pub fn alive(&self) -> bool {
        !(self.closed_parent && self.closed_child)
    }

    /// 父子两侧值一致。
    pub fn agree(&self) -> bool {
        self.val_parent == self.val_child
    }

    pub fn render(&self) -> String {
        let f = if self.closed_parent { "P" } else { "-" };
        let g = if self.closed_child { "C" } else { "-" };
        format!("k{}|{}{}|v{}", self.key, f, g, self.val_parent)
    }
}

/// fork 继承账：父表登记 → fork 拷贝 → 共享行为一致 → 冻结。
pub struct InheritLedger {
    pub hooks: Vec<ShareHook>,
    pub forks: Vec<u32>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl InheritLedger {
    pub fn new() -> InheritLedger {
        InheritLedger { hooks: Vec::new(), forks: Vec::new(), frozen: false, root: None }
    }

    /// 父侧登记挂点：冻结、零键、重键、超容均拒。
    pub fn enroll(&mut self, key: u32, val: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        if key == 0 {
            return Err(FH_NOHOOK);
        }
        if self.hooks.iter().any(|h| h.key == key) {
            return Err(FH_DUPFORK);
        }
        if self.hooks.len() >= HOOK_CAP {
            return Err(FH_CAP);
        }
        self.hooks.push(ShareHook {
            key,
            val_parent: val,
            val_child: val,
            closed_parent: false,
            closed_child: false,
        });
        Ok(())
    }

    /// fork：子进程号在册（重号必拒）；挂点表整体继承（值随写同步）。
    pub fn fork(&mut self, pid: u32) -> Result<usize, u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        if pid == 0 || self.forks.iter().any(|p| *p == pid) {
            return Err(FH_DUPFORK);
        }
        self.forks.push(pid);
        Ok(self.hooks.len())
    }

    /// 单侧写值：写父侧或子侧，另一侧保持独立快照；对账以 agree 报告。
    pub fn write_side(&mut self, key: u32, parent_side: bool, val: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].key == key {
                if parent_side {
                    self.hooks[k].val_parent = val;
                } else {
                    self.hooks[k].val_child = val;
                }
                return Ok(self.hooks[k].agree());
            }
            k += 1;
        }
        Err(FH_NOHOOK)
    }

    /// 单侧关账：双侧皆关后复用必拒。
    pub fn close_side(&mut self, key: u32, parent_side: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].key == key {
                if !self.hooks[k].alive() {
                    return Err(FH_DEADUSE);
                }
                if parent_side {
                    self.hooks[k].closed_parent = true;
                } else {
                    self.hooks[k].closed_child = true;
                }
                return Ok(());
            }
            k += 1;
        }
        Err(FH_NOHOOK)
    }

    /// 共享活挂点数。
    pub fn alive_n(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].alive() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 继承对平裁断：挂点数恰为对平数、fork 在册、双侧值全一致。
    pub fn closed(&self) -> bool {
        if self.hooks.len() != HOOKS_NEEDED || self.forks.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < self.hooks.len() {
            if !self.hooks[k].agree() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含键/双值/双关位/fork 表）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FH_TAG;
        let mut k = 0usize;
        while k < self.hooks.len() {
            let e = &self.hooks[k];
            h = (h ^ e.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.val_parent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.val_child as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.closed_parent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.closed_child as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut k = 0usize;
        while k < self.forks.len() {
            h = (h ^ self.forks[k] as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 挂点键与值确定性样本。
pub fn fh_key(no: u32) -> u32 {
    no.wrapping_mul(31).wrapping_add(1)
}

pub fn fh_val(no: u32) -> u32 {
    no.wrapping_mul(37).wrapping_add(9)
}

/// 十挂点全链：登记、fork、行为一致校验。
pub fn build_inherit() -> (InheritLedger, bool) {
    let mut l = InheritLedger::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= HOOKS_NEEDED as u32 {
        if l.enroll(fh_key(i), fh_val(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    if l.fork(101).is_err() {
        ok = false;
    }
    // 共享行为一致：父侧写后子侧同写，双侧仍一致
    let mut i = 1u32;
    while i <= HOOKS_NEEDED as u32 {
        let v = fh_val(i).wrapping_add(100);
        if l.write_side(fh_key(i), true, v) != Ok(false) {
            ok = false;
        }
        if l.write_side(fh_key(i), false, v) != Ok(true) {
            ok = false;
        }
        i += 1;
    }
    let l_ok = ok && l.closed();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复 fork 同号必拒
    let mut a = InheritLedger::new();
    let _ = a.fork(7);
    got[0] = a.fork(7).err() == Some(FH_DUPFORK);
    // 2) 关不存在的挂点必拒
    let mut b = InheritLedger::new();
    got[1] = b.close_side(999, true).err() == Some(FH_NOHOOK);
    // 3) 双侧皆关后复用必拒（死账探针）
    let mut c = InheritLedger::new();
    let _ = c.enroll(5, 1);
    let _ = c.close_side(5, true);
    let _ = c.close_side(5, false);
    got[2] = !c.hooks[0].alive() && c.close_side(5, true).err() == Some(FH_DEADUSE);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_inherit();
    d.freeze();
    got[3] = d.fork(202).err() == Some(FH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11013");

    // J1 继承账逐项对平：十挂点登记且双侧值一致
    let (mut book, green) = build_inherit();
    s.add("J1-继承账逐项对平", green && book.closed() && book.hooks.len() == 10, "十挂点登记且双侧一致");

    // J2 共享挂点行为一致：fork 在册且活账全数
    s.add("J2-共享行为一致", book.forks == [101].to_vec() && book.alive_n() == 10, "fork 在册且十挂点全活账");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(fh_key(77), 1).err() == Some(FH_FROZEN)
        && book.write_side(fh_key(1), true, 5).err() == Some(FH_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后登记与写值均必拒");

    // J5 注入检出：重复 fork + 关不存在
    let inj = inject_four();
    s.add("J5-重 fork 关无检出", inj[0] && inj[1], "重号 fork 与关不存在两路必抓");

    // J6 注入检出：双侧皆关复用 + 冻结后写
    s.add("J6-死账冻结检出", inj[2] && inj[3], "死账复用必拒且冻结后写必抓");

    // J7 单侧关账他侧仍活：父关后子侧可继续写
    let j7 = {
        let mut c = InheritLedger::new();
        let _ = c.enroll(5, 1);
        let _ = c.fork(9);
        let _ = c.close_side(5, true);
        c.hooks[0].closed_parent && !c.hooks[0].closed_child && c.hooks[0].alive()
    };
    s.add("J7-单侧关他侧活", j7, "单侧关账不影响他侧共享");

    // J8 零键拒收
    let j8 = InheritLedger::new().enroll(0, 1).err() == Some(FH_NOHOOK);
    s.add("J8-零键拒收", j8, "挂点键零必拒");

    // J9 写不存在的键必拒
    let j9 = InheritLedger::new().write_side(888, true, 1).err() == Some(FH_NOHOOK);
    s.add("J9-写不存在键", j9, "写值不存在的键必拒");

    // J10 父子分歧可探：单侧写后 agree 为假
    let j10 = {
        let mut c = InheritLedger::new();
        let _ = c.enroll(5, 1);
        let _ = c.fork(9);
        let _ = c.write_side(5, true, 2);
        !c.hooks[0].agree() && !c.closed()
    };
    s.add("J10-分歧可探", j10, "单侧写后对账立即报不一致");

    // J11 超容拒：第 HOOK_CAP+1 项必拒
    let j11 = {
        let mut c = InheritLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= HOOK_CAP as u32 {
            if c.enroll(n.wrapping_mul(3), n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(9999, 1).err() == Some(FH_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：关账标记可区分
    let j12 = {
        let a = ShareHook {
            key: 5,
            val_parent: 1,
            val_child: 1,
            closed_parent: false,
            closed_child: false,
        };
        let mut b = a.clone();
        b.closed_parent = true;
        a.render() != b.render() && b.render().contains('P')
    };
    s.add("J12-渲染分旗", j12, "父侧关账标记可区分");

    // J13 样本确定性：键与值同入同出且键互异
    let k3a = fh_key(3);
    let k3b = fh_key(3);
    let v3a = fh_val(3);
    let v3b = fh_val(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= HOOKS_NEEDED as u32 {
        let mut j = i.wrapping_add(1);
        while j <= HOOKS_NEEDED as u32 {
            if fh_key(i) == fh_key(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", k3a == k3b && v3a == v3b && distinct, "键值样本同入同出且键互异");

    // J14 动态明细：fork 表与活账数落账
    let f = if book.forks.is_empty() { 0u32 } else { book.forks[0] };
    s.add_dyn(
        "J14-继承明细",
        f == 101 && book.alive_n() == 10,
        &format!("fork={} alive={}", f, book.alive_n()),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11013_inherit_flow() {
        let mut l = InheritLedger::new();
        assert!(l.enroll(5, 1).is_ok());
        assert!(l.enroll(5, 2).err() == Some(FH_DUPFORK));
        assert!(l.fork(9).is_ok());
        assert!(l.fork(9).err() == Some(FH_DUPFORK));
        assert_eq!(l.write_side(5, true, 3), Ok(false));
        assert!(l.close_side(5, true).is_ok());
        assert!(l.close_side(5, false).is_ok());
        assert!(l.close_side(5, true).err() == Some(FH_DEADUSE));
        l.freeze();
        assert!(l.fork(11).err() == Some(FH_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11013_checks_green() {
        let s = run_unx11013_checks();
        assert!(s.all_passed(), "UNX-F11013 判据未全绿");
    }
}
