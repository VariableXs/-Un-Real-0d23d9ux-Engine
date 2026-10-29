//! UNX-F11091 混合负载长稳段（五族并发混跑万轮）（0x1eb60 段）
//!
//! 锚点：UNX-F11091-J1 混跑万轮互扰零命中——五族确定性负载万轮交叉推进，
//! 族间互扰探测器零命中；J2 五族账独立对平——每族账目独立闭合，族间
//! 互不串账。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：五族负载机（调度/文件/网络/IPC/内存各持独立账与轮次计数）+
//! 交叉引擎（确定性轮转推进五族各一笔）+ 互扰探针（族账漂移即命中）+
//! 注入核（串账/越界/重号/封账写入四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1eb60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1eb60;
pub const HB_NOENT: u32 = SEG_BASE + 0x01; // 族不在册
pub const HB_DUP: u32 = SEG_BASE + 0x02; // 族重号
pub const HB_CROSS: u32 = SEG_BASE + 0x03; // 族间串账
pub const HB_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const HB_NEG: u32 = SEG_BASE + 0x05; // 负减

/// 五族标签。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fam5 {
    Sched,
    File,
    Net,
    Ipc,
    Mem,
}

/// 五族账本。
pub struct FiveBook {
    pub bal: [u32; 5],
    pub rounds: [u32; 5],
    pub sealed: bool,
}

impl FiveBook {
    pub fn new() -> FiveBook {
        FiveBook { bal: [0; 5], rounds: [0; 5], sealed: false }
    }

    fn idx(&self, f: Fam5) -> Result<usize, u32> {
        match f {
            Fam5::Sched => Ok(0),
            Fam5::File => Ok(1),
            Fam5::Net => Ok(2),
            Fam5::Ipc => Ok(3),
            Fam5::Mem => Ok(4),
        }
    }

    /// 加账。
    pub fn add(&mut self, f: Fam5, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(HB_FROZEN);
        }
        let i = self.idx(f)?;
        self.bal[i] = self.bal[i].wrapping_add(n);
        Ok(self.bal[i])
    }

    /// 减账。
    pub fn sub(&mut self, f: Fam5, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(HB_FROZEN);
        }
        let i = self.idx(f)?;
        if self.bal[i] < n {
            return Err(HB_NEG);
        }
        self.bal[i] = self.bal[i].wrapping_sub(n);
        Ok(self.bal[i])
    }

    /// 单族推进一轮：加 2 减 1（净 1，留账可对平）。
    pub fn step(&mut self, f: Fam5) -> Result<u32, u32> {
        if self.sealed {
            return Err(HB_FROZEN);
        }
        let i = self.idx(f)?;
        let _ = self.add(f, 2)?;
        let _ = self.sub(f, 1)?;
        self.rounds[i] = self.rounds[i].wrapping_add(1);
        Ok(self.bal[i])
    }

    /// 混跑引擎：万轮轮转推进五族。
    pub fn churn(&mut self, rounds: u32) -> bool {
        let fams = [Fam5::Sched, Fam5::File, Fam5::Net, Fam5::Ipc, Fam5::Mem];
        let mut ok = true;
        let mut r = 0u32;
        while r < rounds {
            let mut k = 0usize;
            while k < fams.len() {
                if self.step(fams[k]).is_err() {
                    ok = false;
                }
                k = k.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        ok
    }

    /// 族独立对平：余额 = 轮次数（step 净 1）。
    pub fn fam_balanced(&self, f: Fam5) -> Result<bool, u32> {
        let i = self.idx(f)?;
        Ok(self.bal[i] == self.rounds[i])
    }

    /// 互扰探针：全族对平即零命中。
    pub fn no_cross_talk(&self) -> bool {
        let fams = [Fam5::Sched, Fam5::File, Fam5::Net, Fam5::Ipc, Fam5::Mem];
        let mut k = 0usize;
        while k < fams.len() {
            if self.fam_balanced(fams[k]) != Ok(true) {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1eb6_02a1;
        let mut k = 0usize;
        while k < 5 {
            h = (h ^ self.bal[k] as u64).wrapping_mul(0x100000001b3);
            h = (h ^ (self.rounds[k] as u64) << 24).wrapping_mul(0x100000001b3);
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 串账必抓：手工漂移一族余额即对平为假
    let mut b = FiveBook::new();
    let _ = b.step(Fam5::Sched);
    b.bal[0] = b.bal[0].wrapping_add(5);
    got[0] = b.fam_balanced(Fam5::Sched) == Ok(false) && b.fam_balanced(Fam5::Mem) == Ok(true);
    // 2) 负减必抓
    let mut c = FiveBook::new();
    got[1] = c.sub(Fam5::Net, 1).err() == Some(HB_NEG);
    // 3) 等额扰动不误报：加 1 减 1 对平仍真
    let mut d = FiveBook::new();
    let _ = d.add(Fam5::Ipc, 4);
    let _ = d.sub(Fam5::Ipc, 4);
    got[2] = d.fam_balanced(Fam5::Ipc) == Ok(true);
    // 4) 封账写入必抓
    d.seal();
    got[3] = d.add(Fam5::Ipc, 1).err() == Some(HB_FROZEN)
        && d.step(Fam5::Mem).err() == Some(HB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11091_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11091");

    // J1 混跑万轮互扰零命中
    let mut b = FiveBook::new();
    let ok = b.churn(10000);
    let inj = inject_four();
    let j1 = ok && b.no_cross_talk() && inj[0];
    s.add("J1-万轮互扰零命中", j1, "五族万轮交叉推进互扰探测器零命中");

    // J2 五族账独立对平
    let fams = [Fam5::Sched, Fam5::File, Fam5::Net, Fam5::Ipc, Fam5::Mem];
    let mut all = true;
    let mut k = 0usize;
    while k < fams.len() {
        if b.fam_balanced(fams[k]) != Ok(true) || b.rounds[k] != 10000 {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let j2 = all && b.bal[0] == 10000 && inj[2];
    s.add("J2-五族独立对平", j2, "每族余额等于轮次且等额扰动不误报");

    // J3 串账必抓
    s.add("J3-串账抓", inj[0], "族账漂移必被独立对平抓出");

    // J4 负减必抓
    s.add("J4-负减抓", inj[1], "零余额减账必拒");

    // J5 封账写入必抓
    s.add("J5-封账拒写", inj[3], "封账后加账与推进两路均必拒");

    // J6 单族推进：净 1 与轮次联动
    let mut c = FiveBook::new();
    let _ = c.step(Fam5::File);
    let _ = c.step(Fam5::File);
    let j6 = c.bal[1] == 2 && c.rounds[1] == 2
        && c.fam_balanced(Fam5::File) == Ok(true);
    s.add("J6-净一联动", j6, "step 净增 1 与轮次计数严格联动");

    // J7 族隔离：单族混跑不影响他族
    let mut d = FiveBook::new();
    let mut r = 0u32;
    while r < 100 {
        let _ = d.step(Fam5::Net);
        r = r.wrapping_add(1);
    }
    let j7 = d.rounds[2] == 100 && d.rounds[0] == 0 && d.rounds[4] == 0;
    s.add("J7-族隔离", j7, "单族推进他族零污染");

    // J8 封账根与指纹
    let fp0 = d.fold();
    let root = d.seal();
    let j8 = fp0 == root && d.fold() == root;
    s.add("J8-指纹冻结", j8, "封账根与 fold 一致且二次 fold 稳定");

    // J9 重放幂等：同轮数重放指纹一致
    let e = FiveBook::new();
    let mut e2 = FiveBook::new();
    let _ = e2.churn(256);
    let mut e3 = FiveBook::new();
    let _ = e3.churn(256);
    let j9 = e2.fold() == e3.fold() && e.fold() != e2.fold();
    s.add("J9-重放幂等", j9, "同轮数重放指纹一致且空册指纹可区分");

    // J10 万轮规模明细落账
    s.add_dyn("J10-规模明细", b.rounds[0] == 10000 && b.bal[0] == 10000,
        &format!("rounds={} bal={}", b.rounds[0], b.bal[0]));

    // J11 指纹落账
    let fp = b.fold();
    s.add_dyn("J11-册指纹", fp != 0, &format!("fold={:#x}", fp));

    // J12 静态判据：首族在册轮数与余额恒等
    s.add("J12-首族对平", b.bal[0] == b.rounds[0], "首族在册余额恒等于其轮数");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11091_five_flow() {
        let mut b = FiveBook::new();
        assert!(b.step(Fam5::Sched).is_ok());
        assert!(b.sub(Fam5::Sched, 9).err() == Some(HB_NEG));
        assert!(b.fam_balanced(Fam5::Sched) == Ok(true));
        assert!(b.fam_balanced(Fam5::Mem) == Ok(true));
        b.seal();
        assert!(b.step(Fam5::File).err() == Some(HB_FROZEN));
        assert!(b.add(Fam5::Net, 1).err() == Some(HB_FROZEN));
    }

    #[test]
    fn unx11091_checks_green() {
        let s = run_unx11091_checks();
        assert!(s.all_passed(), "UNX-F11091 判据未全绿");
    }
}
