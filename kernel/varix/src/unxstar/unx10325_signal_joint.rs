//! UNX-F10325 跨域信号面联签（signal 族判据互认）（0x1b320 段）
//!
//! 锚点：UNX-F10325-J1 信号处置互认——1..=31 号逐号由甲乙双方独立定
//! 处置（默认/忽略/终止/卸核/暂停五选一），双定且同处置才算该号互认，
//! 三十一号全互认才算信号面联签过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：定容处置账（双甲乙各 [u8; SIG_N] 定容数组，0=未定）+ 处置核
//! （号域 1..=31、处置域 1..=5、已定再定即拒）+ 冻结闸（全互认方可
//! 冻结，冻结后拒定）+ 注入核（坏号/坏处置/重定/未齐冻结四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b320 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b320;
pub const SJ_SIG: u32 = SEG_BASE + 0x01; // 号越界（含 0 与 >31）
pub const SJ_DISP: u32 = SEG_BASE + 0x02; // 处置越界
pub const SJ_RESET: u32 = SEG_BASE + 0x03; // 已定再定
pub const SJ_UNSETTLED: u32 = SEG_BASE + 0x04; // 未齐即冻结
pub const SJ_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后再定

/// 信号号面锚：1..=31。
pub const SIG_MAX: usize = 31;
/// 定容数组宽（下标 0 弃用）。
pub const SIG_N: usize = 32;

/// 处置码。
pub const DISP_DFL: u8 = 1;
pub const DISP_IGN: u8 = 2;
pub const DISP_TERM: u8 = 3;
pub const DISP_CORE: u8 = 4;
pub const DISP_STOP: u8 = 5;
pub const DISP_N: u8 = 6;

/// 信号处置账（双甲乙各一定容数组）。
pub struct SignalBook {
    pub disp_a: [u8; SIG_N],
    pub disp_b: [u8; SIG_N],
    pub frozen: bool,
}

impl SignalBook {
    pub fn new() -> SignalBook {
        SignalBook {
            disp_a: [0; SIG_N],
            disp_b: [0; SIG_N],
            frozen: false,
        }
    }

    fn sig_ok(sig: usize) -> bool {
        sig >= 1 && sig <= SIG_MAX
    }

    fn disp_ok(d: u8) -> bool {
        d >= DISP_DFL && d < DISP_N
    }

    /// 甲方定处置。
    pub fn a_set(&mut self, sig: usize, d: u8) -> Result<(), u32> {
        if !Self::sig_ok(sig) {
            return Err(SJ_SIG);
        }
        if !Self::disp_ok(d) {
            return Err(SJ_DISP);
        }
        if self.frozen {
            return Err(SJ_FROZEN);
        }
        if self.disp_a[sig] != 0 {
            return Err(SJ_RESET);
        }
        self.disp_a[sig] = d;
        Ok(())
    }

    /// 乙方定处置。
    pub fn b_set(&mut self, sig: usize, d: u8) -> Result<(), u32> {
        if !Self::sig_ok(sig) {
            return Err(SJ_SIG);
        }
        if !Self::disp_ok(d) {
            return Err(SJ_DISP);
        }
        if self.frozen {
            return Err(SJ_FROZEN);
        }
        if self.disp_b[sig] != 0 {
            return Err(SJ_RESET);
        }
        self.disp_b[sig] = d;
        Ok(())
    }

    /// 单号互认：双定且同处置。
    pub fn agree(&self, sig: usize) -> bool {
        if !Self::sig_ok(sig) {
            return false;
        }
        self.disp_a[sig] != 0 && self.disp_a[sig] == self.disp_b[sig]
    }

    /// 异处置号清单。
    pub fn conflicts(&self) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        let mut sig = 1usize;
        while sig <= SIG_MAX {
            if self.disp_a[sig] != 0
                && self.disp_b[sig] != 0
                && self.disp_a[sig] != self.disp_b[sig]
            {
                v.push(sig);
            }
            sig += 1;
        }
        v
    }

    /// 未齐号清单（任一侧未定）。
    pub fn pending(&self) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        let mut sig = 1usize;
        while sig <= SIG_MAX {
            if self.disp_a[sig] == 0 || self.disp_b[sig] == 0 {
                v.push(sig);
            }
            sig += 1;
        }
        v
    }

    /// 互认号数。
    pub fn agreed_count(&self) -> usize {
        let mut n = 0usize;
        let mut sig = 1usize;
        while sig <= SIG_MAX {
            if self.agree(sig) {
                n += 1;
            }
            sig += 1;
        }
        n
    }

    /// 信号面联签过：三十一号全互认。
    pub fn settled(&self) -> bool {
        self.pending().is_empty() && self.conflicts().is_empty()
    }

    /// 冻结：未齐即拒，冻结后不可再定。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Ok(());
        }
        if !self.settled() {
            return Err(SJ_UNSETTLED);
        }
        self.frozen = true;
        Ok(())
    }

    /// 信号面指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3200;
        let mut sig = 1usize;
        while sig <= SIG_MAX {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.disp_a[sig] as u32)
                .wrapping_mul(31)
                .wrapping_add(self.disp_b[sig] as u32);
            sig += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.frozen as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = SignalBook::new();
    // 1) 坏号（0 与 32）必拒
    got[0] = b.a_set(0, DISP_IGN).err() == Some(SJ_SIG)
        && b.a_set(32, DISP_IGN).err() == Some(SJ_SIG);
    // 2) 坏处置（0 与 6）必拒
    got[1] = b.a_set(1, 0).err() == Some(SJ_DISP)
        && b.a_set(1, DISP_N).err() == Some(SJ_DISP);
    // 3) 重定必拒
    let _ = b.a_set(2, DISP_TERM);
    got[2] = b.a_set(2, DISP_IGN).err() == Some(SJ_RESET)
        && b.b_set(2, DISP_IGN).is_ok()
        && b.b_set(2, DISP_CORE).err() == Some(SJ_RESET);
    // 4) 未齐即冻结必拒
    got[3] = b.freeze().err() == Some(SJ_UNSETTLED);
    got
}

/// 信号面处置样例：1..=31 全部按号取处置（号模五映射）。
pub fn sample_disp(sig: usize) -> u8 {
    let m = sig % 5;
    if m == 0 {
        DISP_STOP
    } else {
        m as u8
    }
}

/// 批内判据。
pub fn run_unx10325_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10325");

    // J1 正路径：三十一号双定同处置并冻结
    let mut b = SignalBook::new();
    let mut ok = true;
    let mut sig = 1usize;
    while sig <= SIG_MAX {
        let d = sample_disp(sig);
        if b.a_set(sig, d).is_err() || b.b_set(sig, d).is_err() {
            ok = false;
        }
        sig += 1;
    }
    let fr = b.freeze();
    s.add(
        "J1-信号处置互认",
        ok && b.settled() && fr.is_ok() && b.frozen,
        "1..=31 号双定同处置全互认并冻结",
    );

    // J2 互认号数满三十一
    s.add("J2-互认满号", b.agreed_count() == SIG_MAX, "互认号数恰为 31");

    // J3 注入检出：坏号 + 坏处置
    let inj = inject_four();
    s.add("J3-号处置检出", inj[0] && inj[1], "坏号与坏处置必拒");

    // J4 注入检出：重定 + 未齐冻结
    s.add("J4-重定冻检出", inj[2] && inj[3], "重定必拒且未齐冻结必拒");

    // J5 异处置阻断互认
    let j5 = {
        let mut t = SignalBook::new();
        let _ = t.a_set(9, DISP_IGN);
        let _ = t.b_set(9, DISP_TERM);
        !t.agree(9) && !t.settled() && t.conflicts().contains(&9)
    };
    s.add("J5-异处置阻断", j5, "同号异处置入冲突单且不成互认");

    // J6 缺号不成联签（缺 31 号）
    let j6 = {
        let mut t = SignalBook::new();
        let mut sg = 1usize;
        while sg < SIG_MAX {
            let d = sample_disp(sg);
            let _ = t.a_set(sg, d);
            let _ = t.b_set(sg, d);
            sg += 1;
        }
        !t.settled() && t.freeze().err() == Some(SJ_UNSETTLED)
    };
    s.add("J6-缺号不签", j6, "缺任一号即不成信号面联签");

    // J7 冻结后再定必拒
    let j7 = {
        let mut t = SignalBook::new();
        let mut sg = 1usize;
        while sg <= SIG_MAX {
            let d = sample_disp(sg);
            let _ = t.a_set(sg, d);
            let _ = t.b_set(sg, d);
            sg += 1;
        }
        let _ = t.freeze();
        t.a_set(1, DISP_IGN).err() == Some(SJ_FROZEN)
            && t.b_set(3, DISP_TERM).err() == Some(SJ_FROZEN)
    };
    s.add("J7-冻结拒定", j7, "冻结后甲乙再定均拒");

    // J8 单侧未定不成互认
    let j8 = {
        let mut t = SignalBook::new();
        let _ = t.a_set(5, DISP_CORE);
        !t.agree(5) && t.pending().contains(&5)
    };
    s.add("J8-单定不成", j8, "单侧未定入未齐单且不成互认");

    // J9 处置五码合法且互异
    let j9 = DISP_DFL < DISP_IGN
        && DISP_IGN < DISP_TERM
        && DISP_TERM < DISP_CORE
        && DISP_CORE < DISP_STOP;
    s.add("J9-处置码域", j9, "五处置码 1..=5 严格递增");

    // J10 指纹随定值推进且冻结后稳定
    let j10 = {
        let mut t = SignalBook::new();
        let f0 = t.fold();
        let _ = t.a_set(1, DISP_IGN);
        let f1 = t.fold();
        let _ = t.b_set(1, DISP_IGN);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每定一步指纹一步且终态稳定");

    // J11 互认处置分布覆盖五码（样例映射覆盖 1..=5）
    let j11 = {
        let mut seen = [false; 8];
        let mut sg = 1usize;
        while sg <= SIG_MAX {
            let d = sample_disp(sg) as usize;
            if d < 8 {
                seen[d] = true;
            }
            sg += 1;
        }
        seen[DISP_DFL as usize]
            && seen[DISP_IGN as usize]
            && seen[DISP_TERM as usize]
            && seen[DISP_CORE as usize]
            && seen[DISP_STOP as usize]
    };
    s.add("J11-五码覆盖", j11, "样例处置映射覆盖全部五码");

    // J12 动态明细：互认号数落账
    let ac = b.agreed_count();
    s.add_dyn(
        "J12-互认号数",
        ac == SIG_MAX,
        &format!("agreed={}/{}", ac, SIG_MAX),
    );

    // J13 动态明细：信号面指纹落账
    let fp = b.fold();
    s.add_dyn("J13-信号指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10325_signal_flow() {
        let mut b = SignalBook::new();
        assert!(b.a_set(0, DISP_IGN).err() == Some(SJ_SIG));
        assert!(b.a_set(32, DISP_IGN).err() == Some(SJ_SIG));
        assert!(b.a_set(1, 0).err() == Some(SJ_DISP));
        assert!(b.a_set(1, DISP_N).err() == Some(SJ_DISP));
        assert!(b.a_set(1, DISP_IGN).is_ok());
        assert!(b.a_set(1, DISP_TERM).err() == Some(SJ_RESET));
        assert!(!b.agree(1));
        assert!(b.b_set(1, DISP_TERM).is_ok());
        assert!(!b.agree(1));
        assert!(b.conflicts().contains(&1));
        let _ = b.b_set(1, DISP_IGN);
        assert!(b.b_set(1, DISP_IGN).err() == Some(SJ_RESET));
        assert!(b.agree(1));
        assert!(b.freeze().err() == Some(SJ_UNSETTLED));
        let mut sg = 2usize;
        while sg <= SIG_MAX {
            let d = sample_disp(sg);
            let _ = b.a_set(sg, d);
            let _ = b.b_set(sg, d);
            sg += 1;
        }
        assert!(b.settled());
        assert!(b.freeze().is_ok());
        assert!(b.a_set(2, DISP_IGN).err() == Some(SJ_FROZEN));
    }

    #[test]
    fn unx10325_checks_green() {
        let s = run_unx10325_checks();
        assert!(s.all_passed(), "UNX-F10325 判据未全绿");
    }
}
