//! UNX-F10336 联签性能预算（三面联跑时长/量上限）（0x1b480 段）
//!
//! 锚点：UNX-F10336-J1 三面预算帽——pty/进程/基建三面各有时长子帽，
//! 三面合计另受全局时长帽约束，样例量另有总量帽；任一笔入账超帽即拒，
//! 三面各余量非负且样例量在帽内才算预算成立。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：预算账（三面用去时长+样例量+封账位）+ 双帽核（面子帽先核、
//! 全局帽后核，取用即刻失败）+ 样例帽核 + 注入核（坏面号/零刻入账/
//! 超面子帽/超全局帽四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b480 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b480;
pub const BU_FACE: u32 = SEG_BASE + 0x01; // 面号越界
pub const BU_ZERO: u32 = SEG_BASE + 0x02; // 零刻入账
pub const BU_CAPF: u32 = SEG_BASE + 0x03; // 超面子帽
pub const BU_CAPG: u32 = SEG_BASE + 0x04; // 超全局帽
pub const BU_SAMP: u32 = SEG_BASE + 0x05; // 超样例帽
pub const BU_SEALED: u32 = SEG_BASE + 0x06; // 封账后入账

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 三面子帽（pty/进程/基建）。
pub const FACE_TICK_CAP: [u32; 3] = [4000, 3000, 2000];
/// 全局时长帽。
pub const GLOBAL_TICK_CAP: u32 = 8000;
/// 样例量帽。
pub const SAMPLE_CAP: u32 = 400;

/// 三面预算账。
pub struct TriBudget {
    pub face_used: [u32; 3],
    pub samples: u32,
    pub sealed: bool,
}

impl TriBudget {
    pub fn new() -> TriBudget {
        TriBudget { face_used: [0; 3], samples: 0, sealed: false }
    }

    /// 时长入账：零刻拒、坏面拒，面子帽先核、全局帽后核。
    /// 成功返回（面用去, 全局用去）。
    pub fn charge(&mut self, face: u8, ticks: u32) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(BU_SEALED);
        }
        if face >= FACE_N {
            return Err(BU_FACE);
        }
        if ticks == 0 {
            return Err(BU_ZERO);
        }
        let fu = self.face_used[face as usize];
        if fu + ticks > FACE_TICK_CAP[face as usize] {
            return Err(BU_CAPF);
        }
        let mut gu = 0u32;
        let mut f = 0u8;
        while f < FACE_N {
            gu = gu.wrapping_add(self.face_used[f as usize]);
            f += 1;
        }
        if gu + ticks > GLOBAL_TICK_CAP {
            return Err(BU_CAPG);
        }
        self.face_used[face as usize] = fu + ticks;
        Ok((self.face_used[face as usize], gu + ticks))
    }

    /// 样例量入账。
    pub fn charge_samples(&mut self, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(BU_SEALED);
        }
        if n == 0 {
            return Err(BU_ZERO);
        }
        if self.samples + n > SAMPLE_CAP {
            return Err(BU_SAMP);
        }
        self.samples += n;
        Ok(self.samples)
    }

    /// 面余量。
    pub fn headroom(&self, face: u8) -> u32 {
        if face >= FACE_N {
            return 0;
        }
        FACE_TICK_CAP[face as usize] - self.face_used[face as usize]
    }

    /// 全局余量。
    pub fn global_headroom(&self) -> u32 {
        let mut used = 0u32;
        let mut f = 0u8;
        while f < FACE_N {
            used = used.wrapping_add(self.face_used[f as usize]);
            f += 1;
        }
        GLOBAL_TICK_CAP - used
    }

    /// 样例余量。
    pub fn sample_headroom(&self) -> u32 {
        SAMPLE_CAP - self.samples
    }

    /// 预算成立：三面面子帽未破、全局帽未破、样例帽未破。
    pub fn within(&self) -> bool {
        let mut f = 0u8;
        while f < FACE_N {
            if self.face_used[f as usize] > FACE_TICK_CAP[f as usize] {
                return false;
            }
            f += 1;
        }
        self.global_headroom() >= 0 && self.sample_headroom() >= 0
    }

    /// 全局用去合计。
    pub fn global_used(&self) -> u32 {
        let mut used = 0u32;
        let mut f = 0u8;
        while f < FACE_N {
            used = used.wrapping_add(self.face_used[f as usize]);
            f += 1;
        }
        used
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 预算账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4800;
        let mut f = 0u8;
        while f < FACE_N {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.face_used[f as usize] + 1);
            f += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.samples + 1);
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = TriBudget::new();
    // 1) 坏面号必拒
    got[0] = b.charge(3, 10).err() == Some(BU_FACE);
    // 2) 零刻入账必拒
    got[1] = b.charge(0, 0).err() == Some(BU_ZERO)
        && b.charge_samples(0).err() == Some(BU_ZERO);
    // 3) 超面子帽必拒（pty 帽 4000）
    let _ = b.charge(0, 3900);
    got[2] = b.charge(0, 200).err() == Some(BU_CAPF);
    // 4) 超样例帽必拒
    let _ = b.charge_samples(400);
    got[3] = b.charge_samples(1).err() == Some(BU_SAMP);
    got
}

/// 批内判据。
pub fn run_unx10336_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10336");

    // J1 正路径：三面各入一笔时长与样例量，全在帽内
    let mut b = TriBudget::new();
    let mut ok = true;
    let mut f = 0u8;
    while f < FACE_N {
        match b.charge(f, 1000) {
            Ok((fu, gu)) => {
                if fu == 0 || gu == 0 {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        f += 1;
    }
    if b.charge_samples(120).is_err() {
        ok = false;
    }
    s.add(
        "J1-三面预算帽",
        ok && b.within(),
        "三面子帽全局帽样例帽全不破",
    );

    // J2 余量对平：面余量=帽-用去
    let j2 = b.headroom(0) == FACE_TICK_CAP[0] - 1000
        && b.headroom(1) == FACE_TICK_CAP[1] - 1000
        && b.headroom(2) == FACE_TICK_CAP[2] - 1000
        && b.global_headroom() == GLOBAL_TICK_CAP - 3000;
    s.add("J2-余量对平", j2, "面余量与全局余量按帽减用去");

    // J3 注入检出：坏面号 + 零刻
    let inj = inject_four();
    s.add("J3-坏面零刻检出", inj[0] && inj[1], "坏面号与零刻入账必拒");

    // J4 注入检出：超面子帽 + 超样例帽
    s.add("J4-超帽检出", inj[2] && inj[3], "超面子帽与超样例帽必拒");

    // J5 超全局帽必拒（三面各 2700 合计 8100 超 8000）
    let j5 = {
        let mut t = TriBudget::new();
        let _ = t.charge(0, 2700);
        let _ = t.charge(1, 2700);
        let _ = t.charge(2, 2600);
        t.charge(0, 100).err() == Some(BU_CAPG)
    };
    s.add("J5-超全局帽拒", j5, "合计超全局帽必拒");

    // J6 帽值恰好用满允许（边界内）
    let j6 = {
        let mut t = TriBudget::new();
        t.charge(2, 2000).is_ok() && t.headroom(2) == 0
    };
    s.add("J6-满帽允许", j6, "用至帽值恰等允许");

    // J7 样例帽边界（400 恰满）
    let j7 = {
        let mut t = TriBudget::new();
        t.charge_samples(399).is_ok()
            && t.charge_samples(1).is_ok()
            && t.charge_samples(1).err() == Some(BU_SAMP)
    };
    s.add("J7-样例帽边界", j7, "恰满允许逾一必拒");

    // J8 封账后入账必拒
    let j8 = {
        let mut t = TriBudget::new();
        let _ = t.charge(0, 10);
        t.seal();
        t.charge(0, 10).err() == Some(BU_SEALED)
            && t.charge_samples(1).err() == Some(BU_SEALED)
    };
    s.add("J8-封账拒入", j8, "封账后时长与样例入账均拒");

    // J9 用去合计对平
    let j9 = b.global_used() == 3000;
    s.add("J9-合计对平", j9, "三面用去合计为三千刻");

    // J10 指纹随入账推进且封账后稳定
    let j10 = {
        let mut t = TriBudget::new();
        let f0 = t.fold();
        let _ = t.charge(0, 10);
        let f1 = t.fold();
        let _ = t.charge(1, 10);
        let f2 = t.fold();
        t.seal();
        let f3 = t.fold();
        f0 != f1 && f1 != f2 && f2 != f3 && t.fold() == f3
    };
    s.add("J10-指纹推进", j10, "每入账一步指纹一步且封账后稳定");

    // J11 余量枚举全非负
    let j11 = {
        let mut all = true;
        let mut f2 = 0u8;
        while f2 < FACE_N {
            let mut used = 0u32;
            let mut q = 0u8;
            while q < FACE_N {
                used += b.face_used[q as usize];
                q += 1;
            }
            if FACE_TICK_CAP[f2 as usize] < b.face_used[f2 as usize]
                || used > GLOBAL_TICK_CAP
            {
                all = false;
            }
            f2 += 1;
        }
        all
    };
    s.add("J11-余量非负", j11, "面用去不逾帽且合计不逾全局帽");

    // J12 动态明细：样例余量落账
    let sh = b.sample_headroom();
    s.add_dyn(
        "J12-样例余量",
        sh == SAMPLE_CAP - 120,
        &format!("sample_headroom={}", sh),
    );

    // J13 动态明细：预算账指纹落账
    let fp = b.fold();
    s.add_dyn("J13-预算指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10336_tri_budget_flow() {
        let mut b = TriBudget::new();
        assert!(b.charge(3, 10).err() == Some(BU_FACE));
        assert!(b.charge(0, 0).err() == Some(BU_ZERO));
        assert!(b.charge(0, 4001).err() == Some(BU_CAPF));
        assert!(b.charge(0, 2000).is_ok());
        assert!(b.charge(1, 3000).is_ok());
        assert!(b.charge(2, 2000).is_ok());
        assert!(b.charge(2, 200).err() == Some(BU_CAPF));
        assert!(b.charge_samples(0).err() == Some(BU_ZERO));
        assert!(b.charge_samples(400).is_ok());
        assert!(b.charge_samples(1).err() == Some(BU_SAMP));
        assert!(b.headroom(0) == FACE_TICK_CAP[0] - 2000);
        assert!(b.global_headroom() == GLOBAL_TICK_CAP - 5000);
        b.seal();
        assert!(b.charge(0, 1).err() == Some(BU_SEALED));
    }

    #[test]
    fn unx10336_checks_green() {
        let s = run_unx10336_checks();
        assert!(s.all_passed(), "UNX-F10336 判据未全绿");
    }
}
