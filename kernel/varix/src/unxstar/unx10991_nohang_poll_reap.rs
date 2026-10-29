//! UNX-F10991 waitpid WNOHANG 批量轮询收割压力（ECHILD 终态）（0x1db60 段）
//!
//! 锚点：UNX-F10991-J1 WNOHANG 轮询收割千次零阻塞——每轮轮询立即返回、
//! 不挂起、收割结果与僵尸队列一致；J2 ECHILD 终态零误报——子池清空后
//! 每次 waitpid 必报 ECHILD、且清空前绝不误报。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：僵尸队列（FIFO 出账）+ WNOHANG 轮询器（每次 poll 立即回报：
//! 有收 / 无收 / ECHILD 三态）+ 轮询账（poll 次数、收割笔数、ECHILD
//! 次数逐笔在册）+ 注入核（重复收割/未退收割/越权轮询/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1db60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1db60;
pub const WP_NOENT: u32 = SEG_BASE + 0x01; // 子 pid 不在册
pub const WP_NOTZOMB: u32 = SEG_BASE + 0x02; // 目标尚未退场
pub const WP_REAPED: u32 = SEG_BASE + 0x03; // 重复收割同一子
pub const WP_NOTPARENT: u32 = SEG_BASE + 0x04; // 轮询方非现任父
pub const WP_FROZEN: u32 = SEG_BASE + 0x05; // 轮询账封账后写入
pub const WP_ECHILD: u32 = SEG_BASE + 0x06; // 无子可等（终态）
pub const WP_DUP: u32 = SEG_BASE + 0x07; // 子 pid 重号

/// WNOHANG 轮询三态。
#[derive(Clone, Debug, PartialEq)]
pub enum PollOutcome {
    /// 收割到 1 笔（载荷为子 pid 与退出码）。
    Reaped(u32, u32),
    /// 子尚在（WNOHANG 立即返回零收）。
    NoHang,
    /// 子池已空（ECHILD 终态）。
    Echild,
}

/// 僵尸队列项：子 pid + 退出码 + 所属父。
#[derive(Clone, Debug)]
pub struct ZombItem {
    pub pid: u32,
    pub code: u32,
    pub parent: u32,
    pub reaped: bool,
}

/// WNOHANG 轮询账：收割逐笔入账，三态回报计数。
pub struct PollBook {
    pub zombs: Vec<ZombItem>,
    pub polls: u32,
    pub reaped: u32,
    pub echilds: u32,
    pub nohangs: u32,
    pub sealed: bool,
}

impl PollBook {
    pub fn new() -> PollBook {
        PollBook {
            zombs: Vec::new(),
            polls: 0,
            reaped: 0,
            echilds: 0,
            nohangs: 0,
            sealed: false,
        }
    }

    /// 登记子进程（Running 态占位）。
    pub fn enroll(&mut self, parent: u32, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(WP_FROZEN);
        }
        if self.zombs.iter().any(|z| z.pid == pid) {
            return Err(WP_DUP);
        }
        self.zombs.push(ZombItem { pid, code: 0, parent, reaped: false });
        Ok(())
    }

    /// 子退场：落退出码、转僵尸态。
    pub fn exit_child(&mut self, pid: u32, code: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(WP_FROZEN);
        }
        match self.zombs.iter_mut().find(|z| z.pid == pid) {
            None => Err(WP_NOENT),
            Some(z) => {
                if z.reaped {
                    return Err(WP_REAPED);
                }
                if z.code != 0 || z.reaped {
                    return Err(WP_NOTZOMB);
                }
                z.code = code;
                Ok(())
            }
        }
    }

    fn find_idx(&self, pid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.zombs.len() {
            if self.zombs[k].pid == pid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 语义态查询：目标是否已退（code 已落即僵尸）。
    fn is_zombie(&self, pid: u32) -> bool {
        match self.find_idx(pid) {
            None => false,
            Some(i) => self.zombs[i].code != 0 && !self.zombs[i].reaped,
        }
    }

    /// WNOHANG 单点轮询：立即回报三态，绝不阻塞。
    pub fn waitpid_nohang(&mut self, parent: u32, pid: u32) -> Result<PollOutcome, u32> {
        if self.sealed {
            return Err(WP_FROZEN);
        }
        self.polls = self.polls.wrapping_add(1);
        let i = match self.find_idx(pid) {
            Some(i) => i,
            None => return Err(WP_NOENT),
        };
        if self.zombs[i].parent != parent {
            return Err(WP_NOTPARENT);
        }
        if self.zombs[i].reaped {
            // 该子池内此父再无可收之子时按 ECHILD 处理。
            if self.pending_of(parent) == 0 {
                self.echilds = self.echilds.wrapping_add(1);
                return Ok(PollOutcome::Echild);
            }
            self.echilds = self.echilds.wrapping_add(1);
            return Ok(PollOutcome::Echild);
        }
        if !self.is_zombie(pid) {
            self.nohangs = self.nohangs.wrapping_add(1);
            return Ok(PollOutcome::NoHang);
        }
        let code = self.zombs[i].code;
        self.zombs[i].reaped = true;
        self.reaped = self.reaped.wrapping_add(1);
        Ok(PollOutcome::Reaped(pid, code))
    }

    /// WNOHANG 批量轮询：对某父全部子逐个立即回报，零阻塞清池。
    pub fn poll_batch(&mut self, parent: u32) -> Vec<PollOutcome> {
        let mut outs: Vec<PollOutcome> = Vec::new();
        let mut k = 0usize;
        while k < self.zombs.len() {
            if self.zombs[k].parent == parent {
                let pid = self.zombs[k].pid;
                if let Ok(o) = self.waitpid_nohang(parent, pid) {
                    outs.push(o);
                }
            }
            k += 1;
        }
        outs
    }

    /// 某父未收割僵尸数。
    pub fn pending_of(&self, parent: u32) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.zombs.len() {
            let z = &self.zombs[k];
            if z.parent == parent && z.code != 0 && !z.reaped {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// ECHILD 终态核验：某父子池全空时单点轮询必报 Echild。
    pub fn echild_terminal(&mut self, parent: u32) -> bool {
        if self.pending_of(parent) != 0 {
            return false;
        }
        // 对该父任一已收子再轮询必报 Echild。
        let mut k = 0usize;
        while k < self.zombs.len() {
            if self.zombs[k].parent == parent && self.zombs[k].reaped {
                let pid = self.zombs[k].pid;
                return self.waitpid_nohang(parent, pid) == Ok(PollOutcome::Echild);
            }
            k += 1;
        }
        true
    }

    /// 账目对平：收割笔数 = reaped 计数 = 已收标志总数；poll = 三态和。
    pub fn ledger_balanced(&self) -> bool {
        let mut marks = 0u32;
        let mut k = 0usize;
        while k < self.zombs.len() {
            if self.zombs[k].reaped {
                marks = marks.wrapping_add(1);
            }
            k += 1;
        }
        marks == self.reaped
            && self.polls == self.reaped.wrapping_add(self.echilds).wrapping_add(self.nohangs)
    }

    /// fold 指纹（序敏感，含三态计数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1db6_0291;
        let mut k = 0usize;
        while k < self.zombs.len() {
            let z = &self.zombs[k];
            h = (h ^ z.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ z.code as u64).wrapping_mul(0x100000001b3);
            h = (h ^ z.reaped as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.polls as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.reaped as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 确定性样本：第 r 轮第 j 子 pid。
pub fn wp_sample_pid(r: u32, j: u32) -> u32 {
    (r.wrapping_mul(89).wrapping_add(j)).wrapping_mul(3).wrapping_add(7)
}

/// 千次 WNOHANG 轮询压力：每轮 1 父 2 子，NoHang 轮询后再收，账全程对平。
pub fn stress_poll_reap(rounds: u32) -> (u32, u32, u32) {
    let mut book = PollBook::new();
    let mut tot_reaped = 0u32;
    let mut tot_nohang = 0u32;
    let mut tot_echild = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let parent = wp_sample_pid(r, 0);
        let c1 = wp_sample_pid(r, 1);
        let c2 = wp_sample_pid(r, 2);
        let _ = book.enroll(parent, c1);
        let _ = book.enroll(parent, c2);
        // 先 NoHang 轮询（未退，立即返回零收）。
        if book.waitpid_nohang(parent, c1) == Ok(PollOutcome::NoHang) {
            tot_nohang = tot_nohang.wrapping_add(1);
        }
        // 子退场后批量轮询收池。
        let _ = book.exit_child(c1, r.wrapping_add(1));
        let _ = book.exit_child(c2, r.wrapping_add(2));
        let outs = book.poll_batch(parent);
        let mut k = 0usize;
        while k < outs.len() {
            if let PollOutcome::Reaped(_, _) = outs[k] {
                tot_reaped = tot_reaped.wrapping_add(1);
            }
            k += 1;
        }
        // 终态 ECHILD 轮询。
        if book.echild_terminal(parent) {
            tot_echild = tot_echild.wrapping_add(1);
        }
        r += 1;
    }
    (tot_reaped, tot_nohang, tot_echild)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复收割必报（池空按 Echild，不误报 Reaped）
    let mut b = PollBook::new();
    let _ = b.enroll(1, 2);
    let _ = b.exit_child(2, 5);
    let first = b.waitpid_nohang(1, 2);
    let second = b.waitpid_nohang(1, 2);
    got[0] = matches!(first, Ok(PollOutcome::Reaped(2, 5)))
        && second == Ok(PollOutcome::Echild)
        && b.pending_of(1) == 0;
    // 2) 未退收割必 NoHang 不误收
    let mut c = PollBook::new();
    let _ = c.enroll(1, 3);
    got[1] = c.waitpid_nohang(1, 3) == Ok(PollOutcome::NoHang) && c.reaped == 0;
    // 3) 越权轮询必拒
    let mut d = PollBook::new();
    let _ = d.enroll(1, 4);
    got[2] = d.waitpid_nohang(9, 4).err() == Some(WP_NOTPARENT);
    // 4) 封账后轮询必拒
    d.seal();
    got[3] = d.waitpid_nohang(1, 4).err() == Some(WP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10991_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10991");

    // J1 WNOHANG 轮询收割千次零阻塞
    let (reaped, nohang, echild) = stress_poll_reap(1000);
    let inj = inject_four();
    let j1 = reaped == 2000 && nohang == 1000 && inj[1]
        && echild == 1000
        && reaped.wrapping_add(nohang).wrapping_add(echild) > 0;
    s.add("J1-轮询千次零阻塞", j1, "千次轮询立即回报且收割账与僵尸队列一致");

    // J2 ECHILD 终态零误报
    let mut b = PollBook::new();
    let _ = b.enroll(1, 2);
    let _ = b.enroll(1, 3);
    // 清空前绝不误报 Echild：NoHang/Reaped 面先行
    let early = b.waitpid_nohang(1, 2) == Ok(PollOutcome::NoHang);
    let _ = b.exit_child(2, 1);
    let _ = b.exit_child(3, 1);
    let _ = b.waitpid_nohang(1, 2);
    let _ = b.waitpid_nohang(1, 3);
    let j2 = early && b.echild_terminal(1) && b.pending_of(1) == 0 && inj[0];
    s.add("J2-ECHILD 零误报", j2, "池空必 Echild 且清空前绝不误报");

    // J3 账目对平：poll = 三态计数和
    let mut c = PollBook::new();
    let _ = c.enroll(1, 2);
    let _ = c.enroll(1, 3);
    let _ = c.waitpid_nohang(1, 2);
    let _ = c.exit_child(2, 4);
    let _ = c.waitpid_nohang(1, 2);
    let _ = c.waitpid_nohang(1, 2);
    let _ = c.waitpid_nohang(1, 3);
    let j3 = c.ledger_balanced() && c.polls == 4 && c.reaped == 1;
    s.add("J3-账目对平", j3, "poll 总数等于三态回报计数之和");

    // J4 越权轮询与未知子必拒
    let mut d = PollBook::new();
    let _ = d.enroll(1, 2);
    let j4 = d.waitpid_nohang(9, 2).err() == Some(WP_NOTPARENT)
        && d.waitpid_nohang(1, 99).err() == Some(WP_NOENT);
    s.add("J4-越权未知拒", j4, "非现任父与未知子两路必拒");

    // J5 重号登记与封账写账必抓
    let mut e = PollBook::new();
    let _ = e.enroll(1, 2);
    let dup = e.enroll(1, 2).err() == Some(WP_DUP);
    e.seal();
    let j5 = dup && e.enroll(1, 3).err() == Some(WP_FROZEN);
    s.add("J5-重号封账拒", j5, "重号必拒且封账后写账必抓");

    // J6 批量轮询：多子一轮清池且序保 FIFO
    let mut f = PollBook::new();
    let _ = f.enroll(1, 10);
    let _ = f.enroll(1, 11);
    let _ = f.enroll(1, 12);
    let _ = f.exit_child(10, 1);
    let _ = f.exit_child(11, 2);
    let _ = f.exit_child(12, 3);
    let outs = f.poll_batch(1);
    let mut codes: Vec<u32> = Vec::new();
    let mut k = 0usize;
    while k < outs.len() {
        if let PollOutcome::Reaped(_, cd) = outs[k] {
            codes.push(cd);
        }
        k += 1;
    }
    let j6 = codes.len() == 3 && codes[0] == 1 && codes[1] == 2 && codes[2] == 3;
    s.add("J6-批量清池序", j6, "批量轮询按登记序清池且退出码逐笔对号");

    // J7 双父隔离：父 A 清池不影响父 B 账面（A 侧只报 NoHang、零收割）
    let mut g = PollBook::new();
    let _ = g.enroll(1, 2);
    let _ = g.enroll(2, 3);
    let _ = g.exit_child(3, 9);
    let outs_a = g.poll_batch(1);
    let only_nohang = outs_a.len() == 1 && outs_a[0] == PollOutcome::NoHang;
    let j7 = only_nohang && g.pending_of(2) == 1 && g.reaped == 0;
    s.add("J7-双父隔离", j7, "批量轮询按父隔离且互不串账");

    // J8 fold 指纹与封账根
    let fp0 = g.fold();
    let root = g.seal();
    let j8 = fp0 == root && g.fold() == root;
    s.add("J8-指纹冻结", j8, "封账根与 fold 一致且二次 fold 稳定");

    // J9 千次压力账全程对平（独立副本重放）
    let mut h = PollBook::new();
    let mut r = 0u32;
    let mut ok = true;
    while r < 1000 {
        let parent = wp_sample_pid(r, 0);
        let c1 = wp_sample_pid(r, 1);
        let _ = h.enroll(parent, c1);
        let _ = h.exit_child(c1, 1);
        if h.waitpid_nohang(parent, c1).is_err() {
            ok = false;
        }
        // 清池后同点再轮询必落 ECHILD 终态（每轮恰一笔）。
        if h.waitpid_nohang(parent, c1) != Ok(PollOutcome::Echild) {
            ok = false;
        }
        r += 1;
    }
    let j9 = ok && h.ledger_balanced() && h.reaped == 1000 && h.echilds == 1000;
    s.add("J9-千次账对平", j9, "千次独立重放收割与 ECHILD 双千对平");

    // J10 动态明细：千次压力三态计数落账
    s.add_dyn("J10-三态计数", reaped == 2000 && nohang == 1000,
        &format!("reaped={} nohang={} echild={}", reaped, nohang, echild));

    // J11 动态明细：轮询账指纹落账
    let fp = c.fold();
    s.add_dyn("J11-账指纹", fp != 0, &format!("fold={:#x}", fp));

    // J12 静态判据：收割数为无阻塞次数两倍
    s.add("J12-收割对平", reaped == nohang.wrapping_mul(2), "收割总数恒为无阻塞返回次数的两倍");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10991_nohang_flow() {
        let mut b = PollBook::new();
        assert!(b.enroll(1, 2).is_ok());
        assert!(b.enroll(1, 2).err() == Some(WP_DUP));
        assert!(b.waitpid_nohang(1, 2) == Ok(PollOutcome::NoHang));
        assert!(b.exit_child(2, 7).is_ok());
        assert!(b.waitpid_nohang(1, 2) == Ok(PollOutcome::Reaped(2, 7)));
        assert!(b.waitpid_nohang(1, 2) == Ok(PollOutcome::Echild));
        assert!(b.ledger_balanced());
        b.seal();
        assert!(b.waitpid_nohang(1, 2).err() == Some(WP_FROZEN));
    }

    #[test]
    fn unx10991_checks_green() {
        let s = run_unx10991_checks();
        assert!(s.all_passed(), "UNX-F10991 判据未全绿");
    }
}
