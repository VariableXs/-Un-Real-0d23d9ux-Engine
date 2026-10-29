//! UNX-F10335 联签面 ktest 集成（0x1b460 段）
//!
//! 锚点：UNX-F10335-J1 三面信道覆盖——联签样例按面信道（pty/进程/基建）
//! 挂入 ktest，逐例执行取裁断，每信道至少两例已执行且零失败、全例无
//! 漏跑，才算联签面 ktest 集成完成。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：样例册（例=例号+信道+裁断 Option）+ 执行核（重跑即拒、未知
//! 例即拒）+ 覆盖核（三信道各达两例）+ 注入核（坏信道/重例/未知例/
//! 重跑四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b460 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b460;
pub const TK_ZERO: u32 = SEG_BASE + 0x01; // 例号零
pub const TK_DUP: u32 = SEG_BASE + 0x02; // 例号重号
pub const TK_NOCASE: u32 = SEG_BASE + 0x03; // 未知例
pub const TK_RERUN: u32 = SEG_BASE + 0x04; // 已跑再跑
pub const TK_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const TK_CH: u32 = SEG_BASE + 0x06; // 信道越界

/// 三面信道。
pub const CH_PTY: u8 = 0;
pub const CH_PROC: u8 = 1;
pub const CH_INFRA: u8 = 2;
pub const CH_N: u8 = 3;

/// 例数锚（六例）。
pub const CASES_NEEDED: usize = 6;
/// 每信道覆盖锚（两例）。
pub const CH_COVER_NEED: usize = 2;
/// 册定容。
pub const CAP: usize = 12;

/// ktest 样例。
#[derive(Clone)]
pub struct KCase {
    pub cid: u32,
    pub ch: u8,
    pub verdict: Option<bool>,
}

/// 联签面 ktest 台。
pub struct TriKtest {
    pub cases: Vec<KCase>,
    pub sealed: bool,
}

impl TriKtest {
    pub fn new() -> TriKtest {
        TriKtest { cases: Vec::new(), sealed: false }
    }

    fn pos(&self, cid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].cid == cid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn ch_count(&self, ch: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].ch == ch {
                n += 1;
            }
            k += 1;
        }
        n
    }

    fn ch_ran(&self, ch: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].ch == ch && self.cases[k].verdict.is_some() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 挂例。
    pub fn register(&mut self, cid: u32, ch: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TK_RERUN);
        }
        if cid == 0 {
            return Err(TK_ZERO);
        }
        if ch >= CH_N {
            return Err(TK_CH);
        }
        if self.pos(cid).is_some() {
            return Err(TK_DUP);
        }
        if self.cases.len() >= CAP {
            return Err(TK_CAP);
        }
        self.cases.push(KCase { cid, ch, verdict: None });
        Ok(())
    }

    /// 执行取裁断。
    pub fn exec(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(TK_RERUN);
        }
        match self.pos(cid) {
            None => Err(TK_NOCASE),
            Some(p) => {
                if self.cases[p].verdict.is_some() {
                    return Err(TK_RERUN);
                }
                self.cases[p].verdict = Some(ok);
                Ok(())
            }
        }
    }

    /// 失败例清单。
    pub fn failures(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].verdict == Some(false) {
                v.push(self.cases[k].cid);
            }
            k += 1;
        }
        v
    }

    /// 漏跑例清单。
    pub fn missing(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].verdict.is_none() {
                v.push(self.cases[k].cid);
            }
            k += 1;
        }
        v
    }

    /// 信道覆盖：该信道例数达锚且全已跑。
    pub fn ch_covered(&self, ch: u8) -> bool {
        if ch >= CH_N {
            return false;
        }
        self.ch_count(ch) >= CH_COVER_NEED && self.ch_ran(ch) >= CH_COVER_NEED
    }

    /// 集成完成：例数达锚、零漏跑、零失败、三信道全覆盖。
    pub fn integrated(&self) -> bool {
        self.cases.len() >= CASES_NEEDED
            && self.missing().is_empty()
            && self.failures().is_empty()
            && self.ch_covered(CH_PTY)
            && self.ch_covered(CH_PROC)
            && self.ch_covered(CH_INFRA)
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TK_RERUN);
        }
        if !self.integrated() {
            return Err(TK_NOCASE);
        }
        self.sealed = true;
        Ok(())
    }

    /// ktest 台指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4600;
        h = h.wrapping_mul(31).wrapping_add(self.cases.len() as u32);
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(c.cid)
                .wrapping_mul(31)
                .wrapping_add(c.ch as u32)
                .wrapping_mul(31)
                .wrapping_add(c.verdict.unwrap_or(false) as u32 + 2);
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut r = TriKtest::new();
    // 1) 坏信道必拒
    got[0] = r.register(1, 3).err() == Some(TK_CH)
        && r.register(1, 9).err() == Some(TK_CH);
    // 2) 重例必拒
    let _ = r.register(1, CH_PTY);
    got[1] = r.register(1, CH_PROC).err() == Some(TK_DUP);
    // 3) 未知例执行必拒
    got[2] = r.exec(99, true).err() == Some(TK_NOCASE);
    // 4) 重跑必拒
    let _ = r.exec(1, true);
    got[3] = r.exec(1, false).err() == Some(TK_RERUN);
    got
}

/// 批内判据。
pub fn run_unx10335_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10335");

    // J1 正路径：六例三信道各两例全跑全过并封台
    let mut r = TriKtest::new();
    let chans = [CH_PTY, CH_PTY, CH_PROC, CH_PROC, CH_INFRA, CH_INFRA];
    let mut ok = true;
    let mut k = 0usize;
    while k < chans.len() {
        if r.register(k as u32 + 1, chans[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < chans.len() {
        if r.exec(k as u32 + 1, true).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = r.seal();
    s.add(
        "J1-三面信道覆盖",
        ok && r.integrated() && sealed.is_ok(),
        "六例三信道各两例全跑全过并封台",
    );

    // J2 逐信道覆盖裁断
    let j2 = r.ch_covered(CH_PTY) && r.ch_covered(CH_PROC) && r.ch_covered(CH_INFRA);
    s.add("J2-逐信道覆盖", j2, "三信道例数与已跑数均达锚");

    // J3 注入检出：坏信道 + 重例
    let inj = inject_four();
    s.add("J3-信道重例检出", inj[0] && inj[1], "坏信道与重例必拒");

    // J4 注入检出：未知例 + 重跑
    s.add("J4-未知重跑检出", inj[2] && inj[3], "未知例执行与重跑必拒");

    // J5 失败例阻断集成
    let j5 = {
        let mut t = TriKtest::new();
        let cs = [CH_PTY, CH_PTY, CH_PROC, CH_PROC, CH_INFRA, CH_INFRA];
        let mut q = 0usize;
        while q < cs.len() {
            let _ = t.register(q as u32 + 1, cs[q]);
            let _ = t.exec(q as u32 + 1, q != 2);
            q += 1;
        }
        !t.integrated() && t.failures().contains(&3)
    };
    s.add("J5-失败阻断", j5, "任一例失败即不成集成");

    // J6 漏跑阻断集成
    let j6 = {
        let mut t = TriKtest::new();
        let cs = [CH_PTY, CH_PTY, CH_PROC, CH_PROC, CH_INFRA, CH_INFRA];
        let mut q = 0usize;
        while q < cs.len() {
            let _ = t.register(q as u32 + 1, cs[q]);
            if q != 5 {
                let _ = t.exec(q as u32 + 1, true);
            }
            q += 1;
        }
        !t.integrated() && t.missing().contains(&6)
    };
    s.add("J6-漏跑阻断", j6, "漏跑例必须枚举可得");

    // J7 信道只挂一例不成覆盖
    let j7 = {
        let mut t = TriKtest::new();
        let _ = t.register(1, CH_PTY);
        let _ = t.register(2, CH_PTY);
        let _ = t.register(3, CH_PTY);
        let _ = t.register(4, CH_PTY);
        let _ = t.register(5, CH_PTY);
        let _ = t.register(6, CH_PTY);
        !t.ch_covered(CH_PROC) && !t.integrated()
    };
    s.add("J7-信道缺例", j7, "未覆盖信道必不成集成");

    // J8 零例号必拒
    let j8 = TriKtest::new().register(0, CH_PTY).err() == Some(TK_ZERO);
    s.add("J8-零例拒收", j8, "例号零必拒");

    // J9 封台后执行必拒
    let j9 = {
        let mut t = TriKtest::new();
        let cs = [CH_PTY, CH_PTY, CH_PROC, CH_PROC, CH_INFRA, CH_INFRA];
        let mut q = 0usize;
        while q < cs.len() {
            let _ = t.register(q as u32 + 1, cs[q]);
            let _ = t.exec(q as u32 + 1, true);
            q += 1;
        }
        let _ = t.seal();
        t.exec(1, true).err() == Some(TK_RERUN)
            && t.register(9, CH_PTY).err() == Some(TK_RERUN)
    };
    s.add("J9-封台拒动", j9, "封台后执行与挂例均拒");

    // J10 容量边界
    let j10 = {
        let mut t = TriKtest::new();
        let mut q = 1u32;
        let mut filled = true;
        while q <= CAP as u32 {
            let ch = ((q - 1) % 3) as u8;
            if t.register(q, ch).is_err() {
                filled = false;
            }
            q += 1;
        }
        filled && t.register(999, CH_PTY).err() == Some(TK_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随执行推进
    let j11 = {
        let mut t = TriKtest::new();
        let _ = t.register(1, CH_PTY);
        let f0 = t.fold();
        let _ = t.exec(1, true);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J11-指纹推进", j11, "每跑一步指纹一步且终态稳定");

    // J12 动态明细：漏跑计数落账
    let ms = r.missing().len();
    s.add_dyn("J12-漏跑计数", ms == 0, &format!("missing={}", ms));

    // J13 动态明细：ktest 台指纹落账
    let fp = r.fold();
    s.add_dyn("J13-ktest指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10335_tri_ktest_flow() {
        let mut r = TriKtest::new();
        assert!(r.register(1, 3).err() == Some(TK_CH));
        assert!(r.register(0, CH_PTY).err() == Some(TK_ZERO));
        assert!(r.register(1, CH_PTY).is_ok());
        assert!(r.register(1, CH_PROC).err() == Some(TK_DUP));
        assert!(r.exec(99, true).err() == Some(TK_NOCASE));
        assert!(r.exec(1, true).is_ok());
        assert!(r.exec(1, false).err() == Some(TK_RERUN));
        assert!(!r.integrated());
        let cs = [CH_PTY, CH_PROC, CH_INFRA];
        let mut q = 0usize;
        while q < cs.len() {
            let _ = r.register(q as u32 + 2, cs[q]);
            let _ = r.exec(q as u32 + 2, true);
            q += 1;
        }
        assert!(r.integrated());
        assert!(r.seal().is_ok());
        assert!(r.exec(2, true).err() == Some(TK_RERUN));
    }

    #[test]
    fn unx10335_checks_green() {
        let s = run_unx10335_checks();
        assert!(s.all_passed(), "UNX-F10335 判据未全绿");
    }
}
