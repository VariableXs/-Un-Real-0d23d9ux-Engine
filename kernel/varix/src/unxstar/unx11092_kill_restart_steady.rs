//! UNX-F11092 长稳中断注入段（kill/重启恢复账）（0x1eb80 段）
//!
//! 锚点：UNX-F11092-J1 注入千次恢复对平——千次 kill/重启注入后恢复账
//! 逐笔对平；J2 中断后账完整——每次中断注入后重启账面零丢失。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：进程账（pid 册 + 存活态机 Alive/Killed/Restarted）+ 注入引擎
//! （kill 注入 + 重启恢复逐笔留痕）+ 完整性探针（中断前后账面零丢失）+
//! 注入核（kill 不在册/重复 kill/重启未杀/封账写入四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x1eb80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1eb80;
pub const KI_NOENT: u32 = SEG_BASE + 0x01; // pid 不在册
pub const KI_DUP: u32 = SEG_BASE + 0x02; // pid 重号
pub const KI_TWICE: u32 = SEG_BASE + 0x03; // 重复 kill
pub const KI_NOTKILL: u32 = SEG_BASE + 0x04; // 重启未杀对象
pub const KI_FROZEN: u32 = SEG_BASE + 0x05; // 封账后写账
pub const KI_LOST: u32 = SEG_BASE + 0x06; // 重启账面丢失

/// 进程存活态机。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Live {
    Alive,
    Killed,
    Restarted,
}

/// 进程项。
#[derive(Clone, Copy, Debug)]
pub struct ProcItem {
    pub pid: u32,
    pub live: Live,
    pub kills: u32,
    pub boots: u32,
}

/// 中断注入账。
pub struct KillBook {
    pub procs: Vec<ProcItem>,
    pub injected: u32,
    pub recovered: u32,
    pub sealed: bool,
}

impl KillBook {
    pub fn new() -> KillBook {
        KillBook { procs: Vec::new(), injected: 0, recovered: 0, sealed: false }
    }

    fn find(&self, pid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.procs.len() {
            if self.procs[k].pid == pid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记 pid。
    pub fn enroll(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(KI_FROZEN);
        }
        if self.find(pid).is_some() {
            return Err(KI_DUP);
        }
        self.procs.push(ProcItem { pid, live: Live::Alive, kills: 0, boots: 0 });
        Ok(())
    }

    /// kill 注入：只有 Alive 可杀。
    pub fn kill(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(KI_FROZEN);
        }
        match self.find(pid) {
            None => Err(KI_NOENT),
            Some(i) => {
                if self.procs[i].live != Live::Alive {
                    return Err(KI_TWICE);
                }
                self.procs[i].live = Live::Killed;
                self.procs[i].kills = self.procs[i].kills.wrapping_add(1);
                self.injected = self.injected.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 重启恢复：只有 Killed 可重启；账面计数零丢失。
    pub fn restart(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(KI_FROZEN);
        }
        match self.find(pid) {
            None => Err(KI_NOENT),
            Some(i) => {
                if self.procs[i].live != Live::Killed {
                    return Err(KI_NOTKILL);
                }
                self.procs[i].live = Live::Restarted;
                self.procs[i].boots = self.procs[i].boots.wrapping_add(1);
                self.recovered = self.recovered.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 完整性探针：kills 计数与注入总数守恒、boots 与恢复总数守恒。
    pub fn intact(&self) -> bool {
        let mut kt = 0u32;
        let mut bt = 0u32;
        let mut k = 0usize;
        while k < self.procs.len() {
            kt = kt.wrapping_add(self.procs[k].kills);
            bt = bt.wrapping_add(self.procs[k].boots);
            k = k.wrapping_add(1);
        }
        kt == self.injected && bt == self.recovered && self.recovered <= self.injected
    }

    /// 恢复对平裁断。
    pub fn balanced(&self, rounds: u32) -> bool {
        self.injected >= rounds && self.recovered == rounds
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1eb8_02a2;
        let mut k = 0usize;
        while k < self.procs.len() {
            let p = &self.procs[k];
            h = (h ^ p.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ (p.kills as u64) << 16).wrapping_mul(0x100000001b3);
            h = (h ^ (p.boots as u64) << 32).wrapping_mul(0x100000001b3);
            h = (h ^ match p.live { Live::Alive => 1u64, Live::Killed => 2, Live::Restarted => 3 })
                .wrapping_mul(0x100000001b3);
            k = k.wrapping_add(1);
        }
        h = (h ^ self.injected as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.recovered as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 千次中断注入压力：Restarted 是终态（见 J9），故逐轮登记新目标走一轮
/// kill/restart，注入与恢复各积千笔。
pub fn stress_kill(rounds: u32) -> (u32, u32, bool) {
    let mut b = KillBook::new();
    let mut alive_all = true;
    let mut r = 0u32;
    while r < rounds {
        let t = r.wrapping_add(1);
        if b.enroll(t).is_err() {
            alive_all = false;
        }
        if b.kill(t).is_err() {
            alive_all = false;
        }
        if b.restart(t).is_err() {
            alive_all = false;
        }
        if !b.intact() {
            alive_all = false;
        }
        r = r.wrapping_add(1);
    }
    (b.injected, b.recovered, alive_all && b.intact())
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) kill 不在册必抓
    let mut b = KillBook::new();
    got[0] = b.kill(9).err() == Some(KI_NOENT);
    // 2) 重复 kill 必抓
    let _ = b.enroll(1);
    let _ = b.kill(1);
    got[1] = b.kill(1).err() == Some(KI_TWICE);
    // 3) 重启未杀对象必抓
    let mut c = KillBook::new();
    let _ = c.enroll(2);
    got[2] = c.restart(2).err() == Some(KI_NOTKILL);
    // 4) 封账写入必抓
    c.seal();
    got[3] = c.kill(2).err() == Some(KI_FROZEN)
        && c.restart(2).err() == Some(KI_FROZEN)
        && c.enroll(3).err() == Some(KI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11092_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11092");

    // J1 注入千次恢复对平
    let (inj, rec, ok) = stress_kill(1000);
    let four = inject_four();
    let j1 = ok && inj == 1000 && rec == 1000 && four[0];
    s.add("J1-千次恢复对平", j1, "千次 kill/重启注入恢复账逐笔对平");

    // J2 中断后账完整
    let mut b = KillBook::new();
    let _ = b.enroll(1);
    let _ = b.enroll(2);
    let _ = b.kill(1);
    let fp_mid = b.fold();
    let _ = b.restart(1);
    let j2 = b.intact() && b.procs[0].live == Live::Restarted
        && b.procs[0].kills == 1 && b.procs[0].boots == 1
        && fp_mid != 0 && four[1];
    s.add("J2-中断账完整", j2, "中断与重启逐笔留痕账面零丢失");

    // J3 kill 不在册必抓
    s.add("J3-未知杀抓", four[0], "kill 不在册 pid 必拒");

    // J4 重复 kill 必抓
    s.add("J4-重复杀抓", four[1], "Killed 态二次 kill 必拒");

    // J5 重启未杀必抓
    s.add("J5-未杀重启抓", four[2], "Alive 态重启必拒");

    // J6 封账写入必抓
    s.add("J6-封账拒写", four[3], "封账后杀/启/登三路均必拒");

    // J7 重号登记必抓
    let mut c = KillBook::new();
    let _ = c.enroll(5);
    let j7 = c.enroll(5).err() == Some(KI_DUP) && c.intact();
    s.add("J7-重号抓", j7, "pid 重号必拒且账不损");

    // J8 完整性探针：计数守恒
    let mut d = KillBook::new();
    // Restarted 是终态，同一目标只能走一轮杀/启：百轮百目标
    let mut r = 0u32;
    while r < 100 {
        let t = r.wrapping_add(1);
        let _ = d.enroll(t);
        let _ = d.kill(t);
        let _ = d.restart(t);
        r = r.wrapping_add(1);
    }
    let j8 = d.intact() && d.injected == 100 && d.recovered == 100
        && d.balanced(100);
    s.add("J8-计数守恒", j8, "kills 总和 = 注入总数且 boots 总和 = 恢复总数");

    // J9 态机单向：Restarted 不可再 kill
    let mut e = KillBook::new();
    let _ = e.enroll(1);
    let _ = e.kill(1);
    let _ = e.restart(1);
    let j9 = e.kill(1).err() == Some(KI_TWICE)
        && e.procs[0].live == Live::Restarted;
    s.add("J9-态机单向", j9, "重启终态不可再杀保证账序单向");

    // J10 fold 指纹与封账根
    let fp0 = d.fold();
    let root = d.seal();
    let j10 = fp0 == root && d.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 千次压力明细落账
    s.add_dyn("J11-压力明细", inj == 1000 && rec == 1000,
        &format!("killed={} restarted={}", inj, rec));

    // J12 指纹落账
    let fp = b.fold();
    s.add_dyn("J12-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11092_kill_flow() {
        let mut b = KillBook::new();
        assert!(b.kill(1).err() == Some(KI_NOENT));
        assert!(b.enroll(1).is_ok());
        assert!(b.enroll(1).err() == Some(KI_DUP));
        assert!(b.restart(1).err() == Some(KI_NOTKILL));
        assert!(b.kill(1).is_ok());
        assert!(b.kill(1).err() == Some(KI_TWICE));
        assert!(b.restart(1).is_ok());
        assert!(b.intact());
        b.seal();
        assert!(b.kill(1).err() == Some(KI_FROZEN));
    }

    #[test]
    fn unx11092_checks_green() {
        let s = run_unx11092_checks();
        assert!(s.all_passed(), "UNX-F11092 判据未全绿");
    }
}
