//! UNX-F10990 孤儿与僵尸场景压力（收养链+收割账）（0x1db40 段）
//!
//! 锚点：UNX-F10990-J1 父先退子收养千次链路对平——父进程先退场时全部在册
//! 子进程即时过继给收养根（INIT=1），收养链逐段留痕且终态对平；J2 僵尸收割
//! 终态归零——所有僵尸态进程经收割后表内无残留、收割账守恒。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：亲缘册（pid 表 + 父指针 + 存活态机 Running/Zombie/Reaped/Adopted）+
//! 收养链（父退场即过继，链号递增留痕）+ 收割账（收割逐笔入账，账目两端
//! 对平：收割笔数 = 僵尸消除数）+ 注入核（重号/孤儿循环亲缘/收割越权/
//! 双亲冲突四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1db40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1db40;
pub const OZ_DUP: u32 = SEG_BASE + 0x01; // pid 重号
pub const OZ_NOENT: u32 = SEG_BASE + 0x02; // 目标进程不在册
pub const OZ_SELF: u32 = SEG_BASE + 0x03; // 自为亲缘（父=子）
pub const OZ_CYCLE: u32 = SEG_BASE + 0x04; // 亲缘环（祖先链回指）
pub const OZ_NOTZOMB: u32 = SEG_BASE + 0x05; // 收割目标非僵尸态
pub const OZ_NOPARENT: u32 = SEG_BASE + 0x06; // 无父可收割（ECHILD 面）
pub const OZ_FROZEN: u32 = SEG_BASE + 0x07; // 收割账封账后写入
pub const OZ_INITMUT: u32 = SEG_BASE + 0x08; // 收养根不可改派

/// 收养根 pid（INIT）。
pub const INIT_PID: u32 = 1;
/// 亲缘册定容。
pub const CAP: usize = 64;
/// 亲缘链域标（fold 种子）。
pub const OZ_TAG: u64 = 0x1db4_0290;
/// 压力轮数（千次）。
pub const ROUNDS: u32 = 1000;

/// 进程存活态机。
#[derive(Clone, Debug, PartialEq)]
pub enum PState {
    Running,
    Zombie,
    Reaped,
}

/// 在册进程项：pid + 父 pid + 态。
#[derive(Clone, Debug)]
pub struct ProcSlot {
    pub pid: u32,
    pub parent: u32,
    pub state: PState,
}

/// 收养链留痕：被收养子 + 原父 + 链号。
#[derive(Clone, Debug)]
pub struct AdoptMark {
    pub child: u32,
    pub old_parent: u32,
    pub chain: u32,
}

/// 收割账一笔：收割方 + 被收僵尸 + 账号。
#[derive(Clone, Debug)]
pub struct ReapMark {
    pub reaper: u32,
    pub zombie: u32,
    pub no: u32,
}

/// 孤儿/僵尸亲缘册：登记→退场过继→僵尸收割→账目对平。
#[derive(Clone, Debug)]
pub struct KinBook {
    pub slots: Vec<ProcSlot>,
    pub adopts: Vec<AdoptMark>,
    pub reaps: Vec<ReapMark>,
    pub sealed: bool,
    pub chain_no: u32,
    pub reap_no: u32,
}

impl KinBook {
    pub fn new() -> KinBook {
        KinBook {
            slots: Vec::new(),
            adopts: Vec::new(),
            reaps: Vec::new(),
            sealed: false,
            chain_no: 0,
            reap_no: 0,
        }
    }

    /// 在册查询。
    pub fn find(&self, pid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].pid == pid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记：封账/重号/自亲缘/超容必拒；INIT 根不可登记改派。
    pub fn spawn(&mut self, pid: u32, parent: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        if pid == INIT_PID {
            return Err(OZ_INITMUT);
        }
        if pid == parent {
            return Err(OZ_SELF);
        }
        // 亲缘环禁入：沿父链上溯不得回指 pid 自身（先于重号判定，环请求优先归因）。
        let mut cur = parent;
        let mut hops = 0u32;
        while cur != INIT_PID && hops <= CAP as u32 {
            match self.find(cur) {
                None => break,
                Some(i) => {
                    if self.slots[i].pid == pid {
                        return Err(OZ_CYCLE);
                    }
                    cur = self.slots[i].parent;
                }
            }
            hops += 1;
        }
        if self.find(pid).is_some() {
            return Err(OZ_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(OZ_NOPARENT);
        }
        self.slots.push(ProcSlot { pid, parent, state: PState::Running });
        Ok(())
    }

    /// 父退场：其全部 Running 子即时过继给收养根并逐段留痕。
    pub fn exit_parent(&mut self, pid: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        let i = match self.find(pid) {
            Some(i) => i,
            None => return Err(OZ_NOENT),
        };
        self.slots[i].state = PState::Zombie;
        let mut adopted = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].parent == pid && self.slots[k].pid != pid {
                self.chain_no = self.chain_no.wrapping_add(1);
                self.slots[k].parent = INIT_PID;
                self.adopts.push(AdoptMark {
                    child: self.slots[k].pid,
                    old_parent: pid,
                    chain: self.chain_no,
                });
                adopted = adopted.wrapping_add(1);
            }
            k += 1;
        }
        Ok(adopted)
    }

    /// 子退场入僵尸态。
    pub fn exit_child(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        match self.find(pid) {
            None => Err(OZ_NOENT),
            Some(i) => {
                if self.slots[i].state != PState::Running {
                    return Err(OZ_NOTZOMB);
                }
                self.slots[i].state = PState::Zombie;
                Ok(())
            }
        }
    }

    /// 收割：只有现任父可收其僵尸子；收割逐笔入账。
    pub fn reap(&mut self, reaper: u32, zombie: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        let i = match self.find(zombie) {
            Some(i) => i,
            None => return Err(OZ_NOENT),
        };
        if self.slots[i].state != PState::Zombie {
            return Err(OZ_NOTZOMB);
        }
        if self.slots[i].parent != reaper {
            return Err(OZ_NOPARENT);
        }
        self.slots[i].state = PState::Reaped;
        self.reap_no = self.reap_no.wrapping_add(1);
        self.reaps.push(ReapMark { reaper, zombie, no: self.reap_no });
        Ok(self.reap_no)
    }

    /// 某父的未收割子数（零即 ECHILD 面）。
    pub fn children_pending(&self, parent: u32) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].parent == parent && self.slots[k].state == PState::Zombie {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 收养链对平：每笔收养的现任父必须是收养根。
    pub fn adopt_chain_consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.adopts.len() {
            match self.find(self.adopts[k].child) {
                None => return false,
                Some(i) => {
                    if self.slots[i].parent != INIT_PID {
                        return false;
                    }
                }
            }
            if self.adopts[k].chain == 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 收割账两端对平：账笔数 = 账号终值 = Reaped 态总数。
    pub fn reap_ledger_balanced(&self) -> bool {
        if self.reaps.len() as u32 != self.reap_no {
            return false;
        }
        let mut reaped = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].state == PState::Reaped {
                reaped = reaped.wrapping_add(1);
            }
            k += 1;
        }
        reaped == self.reap_no
    }

    /// 僵尸终态归零：表内无残留 Zombie。
    pub fn zombies_left(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].state == PState::Zombie {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 全域对平：收养链一致 + 收割账对平 + 无残留僵尸 + 无 Running 孤儿悬挂于已退场父。
    pub fn all_balanced(&self) -> bool {
        if !self.adopt_chain_consistent() || !self.reap_ledger_balanced() {
            return false;
        }
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].state == PState::Running {
                match self.find(self.slots[k].parent) {
                    // 父在册但已非 Running，说明过继漏做。
                    Some(pi) => {
                        if self.slots[pi].state != PState::Running {
                            return false;
                        }
                    }
                    // 收养根本身不登记在册，作为父悬空属合法终态。
                    None => {
                        if self.slots[k].parent != INIT_PID {
                            return false;
                        }
                    }
                }
            }
            k += 1;
        }
        true
    }

    /// 亲缘链 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = OZ_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let sl = &self.slots[k];
            h = (h ^ sl.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ sl.parent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ match sl.state { PState::Running => 1u64, PState::Zombie => 2, PState::Reaped => 3 })
                .wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ (self.chain_no as u64) << 32).wrapping_mul(0x100000001b3);
        h = (h ^ self.reap_no as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 确定性样本：第 r 轮第 j 子的样本 pid（零碰撞编码）。
pub fn oz_sample_pid(r: u32, j: u32) -> u32 {
    (r.wrapping_mul(97).wrapping_add(j)).wrapping_mul(2).wrapping_add(10)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) pid 重号必拒
    let mut b = KinBook::new();
    let _ = b.spawn(2, INIT_PID);
    got[0] = b.spawn(2, INIT_PID).err() == Some(OZ_DUP);
    // 2) 亲缘环必拒（孙指回祖）
    let mut c = KinBook::new();
    let _ = c.spawn(2, INIT_PID);
    let _ = c.spawn(3, 2);
    got[1] = c.spawn(2, 3).err() == Some(OZ_CYCLE);
    // 3) 收割越权（非现任父）必抓
    let mut d = KinBook::new();
    let _ = d.spawn(2, INIT_PID);
    let _ = d.spawn(5, 2);
    let _ = d.exit_child(5);
    got[2] = d.reap(INIT_PID, 5).err() == Some(OZ_NOPARENT);
    // 4) 非僵尸态收割必抓
    let mut e = KinBook::new();
    let _ = e.spawn(2, INIT_PID);
    got[3] = e.reap(INIT_PID, 2).err() == Some(OZ_NOTZOMB);
    got
}

/// 千次收养链压力：亲缘册定容后代际轮换，跨代累计收养/收割账。
pub fn stress_adopt_reap(rounds: u32) -> (u32, u32) {
    let mut book = KinBook::new();
    let mut adopted_total = 0u32;
    let mut reaped_total = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // 定容册每轮占两槽，容不下本轮时换代（旧代账已逐笔落定）。
        if book.slots.len().wrapping_add(2) > CAP {
            book = KinBook::new();
        }
        let parent = oz_sample_pid(r, 0);
        let child = oz_sample_pid(r, 1);
        let _ = book.spawn(parent, INIT_PID);
        let _ = book.spawn(child, parent);
        // 父先退：子被收养。
        if let Ok(a) = book.exit_parent(parent) {
            adopted_total = adopted_total.wrapping_add(a);
        }
        // 子退成僵尸，由收养根收割。
        let _ = book.exit_child(child);
        if book.reap(INIT_PID, child).is_ok() {
            reaped_total = reaped_total.wrapping_add(1);
        }
        // 父僵尸由 INIT 收割。
        let _ = book.reap(INIT_PID, parent);
        r += 1;
    }
    (adopted_total, reaped_total)
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10990_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10990");

    // J1 父先退子收养千次链路对平
    let (adopted, reaped) = stress_adopt_reap(ROUNDS);
    let mut probe = KinBook::new();
    let _ = probe.spawn(2, INIT_PID);
    let _ = probe.spawn(3, 2);
    let a = probe.exit_parent(2);
    let j1 = a == Ok(1) && probe.adopt_chain_consistent() && adopted == ROUNDS && reaped == ROUNDS;
    s.add("J1-收养千次对平", j1, "父先退子收养千次链路逐段留痕且终态对平");

    // J2 僵尸收割终态归零
    let mut zbook = KinBook::new();
    let _ = zbook.spawn(2, INIT_PID);
    let _ = zbook.spawn(3, 2);
    let _ = zbook.spawn(4, 2);
    let _ = zbook.exit_parent(2);
    let mut reaped_all = true;
    let mut k = 0usize;
    while k < zbook.slots.len() {
        if zbook.slots[k].state == PState::Zombie {
            if zbook.reap(INIT_PID, zbook.slots[k].pid).is_err() {
                reaped_all = false;
            }
        }
        k += 1;
    }
    let j2 = reaped_all && zbook.zombies_left() == 0 && zbook.reap_ledger_balanced();
    s.add("J2-收割终态归零", j2, "全部僵尸收割后表内零残留且收割账守恒");

    // J3 收养根不可改派与自亲缘拒
    let mut g = KinBook::new();
    let j3 = g.spawn(INIT_PID, INIT_PID).err() == Some(OZ_INITMUT)
        && g.spawn(5, 5).err() == Some(OZ_SELF);
    s.add("J3-根不可改派", j3, "INIT 不可登记且自为亲缘必拒");

    // J4 亲缘环禁入
    let inj = inject_four();
    s.add("J4-亲缘环拒收", inj[1], "孙指回祖的环必被上溯探测抓出");

    // J5 重号与封账后写账必抓
    let mut d = KinBook::new();
    let _ = d.spawn(2, INIT_PID);
    let dup = d.spawn(2, INIT_PID).err() == Some(OZ_DUP);
    d.seal();
    let j5 = dup && inj[0] && d.spawn(3, INIT_PID).err() == Some(OZ_FROZEN);
    s.add("J5-重号封账拒写", j5, "重号必拒且封账后登记必抓");

    // J6 收割越权与非僵尸态收割必抓
    let j6 = inj[2] && inj[3];
    s.add("J6-越权收割抓", j6, "非现任父收割与非僵尸收割两路必抓");

    // J7 ECHILD 面：无未收割子时 children_pending 归零
    let mut e = KinBook::new();
    let _ = e.spawn(2, INIT_PID);
    let _ = e.spawn(7, 2);
    let _ = e.exit_child(7);
    let pending_before = e.children_pending(2);
    let _ = e.reap(2, 7);
    let j7 = pending_before == 1 && e.children_pending(2) == 0;
    s.add("J7-ECHILD 面", j7, "收割前账 1 收割后归零即 ECHILD 语义");

    // J8 全域对平：悬挂探测（Running 挂于非 Running 父必假）
    let mut h = KinBook::new();
    let _ = h.spawn(2, INIT_PID);
    let _ = h.spawn(9, 2);
    let _ = h.exit_parent(2);
    let mut broken = h.clone();
    // 模拟漏过继：手动把子的父改回已退场父
    let mut k = 0usize;
    while k < broken.slots.len() {
        if broken.slots[k].pid == 9 {
            broken.slots[k].parent = 2;
        }
        k += 1;
    }
    let j8 = h.all_balanced() && !broken.all_balanced();
    s.add("J8-全域对平探测", j8, "漏过继样本必被全域对平抓出");

    // J9 fold 指纹稳定与封账根一致
    let fp0 = h.fold();
    let root = h.seal();
    let j9 = fp0 == root && h.fold() == root && root != 0;
    s.add("J9-指纹冻结", j9, "封账根与 fold 一致且二次 fold 稳定");

    // J10 容量边界：超容必拒且原账不损
    let mut c = KinBook::new();
    let mut filled = true;
    let mut n = 2u32;
    while (c.slots.len() as u32) < CAP as u32 {
        if c.spawn(n, INIT_PID).is_err() {
            filled = false;
        }
        n = n.wrapping_add(1);
    }
    let j10 = filled && c.spawn(9999, INIT_PID).err() == Some(OZ_NOPARENT)
        && c.all_balanced();
    s.add("J10-容量边界", j10, "超定容登记必拒且原账不受损");

    // J11 收养链账号递增且重放一致
    let mut r2 = KinBook::new();
    let _ = r2.spawn(2, INIT_PID);
    let _ = r2.spawn(3, 2);
    let _ = r2.spawn(4, 2);
    let _ = r2.spawn(5, 2);
    let _ = r2.exit_parent(2);
    let chains: Vec<u32> = r2.adopts.iter().map(|m| m.chain).collect();
    let j11 = chains.len() == 3 && chains[0] < chains[1] && chains[1] < chains[2];
    s.add("J11-链号递增", j11, "同轮多子收养链号严格递增可对账");

    // J12 动态明细：千次压力计数落账
    s.add_dyn("J12-压力计数", adopted == ROUNDS && reaped == ROUNDS,
        &format!("adopted={} reaped={}", adopted, reaped));

    // J13 动态明细：亲缘册指纹十六进制落账
    let fp = probe.fold();
    s.add_dyn("J13-册指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10990_adopt_reap_flow() {
        let mut b = KinBook::new();
        assert!(b.spawn(2, INIT_PID).is_ok());
        assert!(b.spawn(2, INIT_PID).err() == Some(OZ_DUP));
        assert!(b.spawn(3, 2).is_ok());
        assert_eq!(b.exit_parent(2), Ok(1));
        assert!(b.adopt_chain_consistent());
        assert!(b.exit_child(3).is_ok());
        assert_eq!(b.reap(2, 3).err(), Some(OZ_NOPARENT));
        assert!(b.reap(INIT_PID, 3).is_ok());
        // 退场父自身亦为僵尸，须由收养根收割后才算终态归零。
        assert!(b.reap(INIT_PID, 2).is_ok());
        assert_eq!(b.zombies_left(), 0);
        assert!(b.all_balanced());
        b.seal();
        assert!(b.spawn(9, INIT_PID).err() == Some(OZ_FROZEN));
    }

    #[test]
    fn unx10990_checks_green() {
        let s = run_unx10990_checks();
        assert!(s.all_passed(), "UNX-F10990 判据未全绿");
    }
}
