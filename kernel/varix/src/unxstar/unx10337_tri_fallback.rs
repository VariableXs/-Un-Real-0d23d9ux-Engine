//! UNX-F10337 联签降级预案（缺席挂账）（0x1b4a0 段）
//!
//! 锚点：UNX-F10337-J1 缺席挂账清账——三面各报出席位，缺席面的事项
//! 允许挂账（带缘由），出席面挂账即违例；挂账须逐笔清账，全账清且出
//! 席面零挂账才算降级预案收束。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：出席牌（三面各一位）+ 挂账账（笔=面号+缘由+清账位）+ 挂账核
//! （出席面拒挂、缘由空拒挂、总量有帽）+ 清账核 + 注入核（坏面号/
//! 出席面挂账/空缘由/超帽四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b4a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b4a0;
pub const FB_FACE: u32 = SEG_BASE + 0x01; // 面号越界
pub const FB_PRESENT: u32 = SEG_BASE + 0x02; // 出席面挂账
pub const FB_NOREASON: u32 = SEG_BASE + 0x03; // 缘由空
pub const FB_LIMIT: u32 = SEG_BASE + 0x04; // 挂账超帽
pub const FB_NOHANG: u32 = SEG_BASE + 0x05; // 无账可清
pub const FB_SEALED: u32 = SEG_BASE + 0x06; // 封账后动

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 挂账总量帽。
pub const HANG_LIMIT: usize = 4;

/// 挂账笔。
#[derive(Clone)]
pub struct Hang {
    pub face: u8,
    pub reason: String,
    pub settled: bool,
}

/// 降级预案核。
pub struct FallbackPlan {
    pub presence: [bool; 3],
    pub hangs: Vec<Hang>,
    pub sealed: bool,
}

impl FallbackPlan {
    pub fn new() -> FallbackPlan {
        FallbackPlan {
            presence: [true, true, true],
            hangs: Vec::new(),
            sealed: false,
        }
    }

    /// 报出席位。
    pub fn set_presence(&mut self, face: u8, present: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(FB_SEALED);
        }
        if face >= FACE_N {
            return Err(FB_FACE);
        }
        self.presence[face as usize] = present;
        Ok(())
    }

    /// 缺席面挂账。
    pub fn hang(&mut self, face: u8, reason: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FB_SEALED);
        }
        if face >= FACE_N {
            return Err(FB_FACE);
        }
        if self.presence[face as usize] {
            return Err(FB_PRESENT);
        }
        if reason.is_empty() {
            return Err(FB_NOREASON);
        }
        if self.hangs.len() >= HANG_LIMIT {
            return Err(FB_LIMIT);
        }
        self.hangs.push(Hang { face, reason: reason.to_string(), settled: false });
        Ok(())
    }

    /// 清一笔挂账（该面最早未清笔）。
    pub fn settle(&mut self, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FB_SEALED);
        }
        if face >= FACE_N {
            return Err(FB_FACE);
        }
        let mut k = 0usize;
        while k < self.hangs.len() {
            if self.hangs[k].face == face && !self.hangs[k].settled {
                self.hangs[k].settled = true;
                return Ok(());
            }
            k += 1;
        }
        Err(FB_NOHANG)
    }

    /// 未清笔清单（笔序号）。
    pub fn unsettled(&self) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < self.hangs.len() {
            if !self.hangs[k].settled {
                v.push(k);
            }
            k += 1;
        }
        v
    }

    /// 出席面挂账数。
    pub fn present_hangs(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.hangs.len() {
            if self.presence[self.hangs[k].face as usize] {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 降级收束：全清且出席面零挂账且至少一面出席。
    pub fn clean(&self) -> bool {
        let mut any = false;
        let mut f = 0u8;
        while f < FACE_N {
            if self.presence[f as usize] {
                any = true;
            }
            f += 1;
        }
        any && self.unsettled().is_empty() && self.present_hangs() == 0
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FB_SEALED);
        }
        if !self.clean() {
            return Err(FB_NOHANG);
        }
        self.sealed = true;
        Ok(())
    }

    /// 预案指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4a00;
        let mut f = 0u8;
        while f < FACE_N {
            h = h.wrapping_mul(31).wrapping_add(self.presence[f as usize] as u32 + 1);
            f += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.hangs.len() as u32);
        let mut k = 0usize;
        while k < self.hangs.len() {
            let g = &self.hangs[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(g.face as u32)
                .wrapping_mul(31)
                .wrapping_add(g.settled as u32);
            let rb = g.reason.as_bytes();
            let mut i = 0usize;
            while i < rb.len() {
                h = h.wrapping_mul(31).wrapping_add(rb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut p = FallbackPlan::new();
    // 1) 坏面号必拒
    got[0] = p.set_presence(3, false).err() == Some(FB_FACE)
        && p.hang(9, "x").err() == Some(FB_FACE);
    // 2) 出席面挂账必拒
    got[1] = p.hang(0, "出席挂账").err() == Some(FB_PRESENT);
    // 3) 空缘由必拒
    let _ = p.set_presence(1, false);
    got[2] = p.hang(1, "").err() == Some(FB_NOREASON);
    // 4) 无账可清必拒
    got[3] = p.settle(2).err() == Some(FB_NOHANG);
    got
}

/// 批内判据。
pub fn run_unx10337_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10337");

    // J1 正路径：基建面缺席挂两账逐笔清后收束
    let mut p = FallbackPlan::new();
    let mut ok = true;
    if p.set_presence(2, false).is_err() {
        ok = false;
    }
    if p.hang(2, "基建缺席挂判据").is_err() || p.hang(2, "基建缺席挂跑批").is_err() {
        ok = false;
    }
    if p.settle(2).is_err() || p.settle(2).is_err() {
        ok = false;
    }
    let sealed = p.seal();
    s.add(
        "J1-缺席挂账清账",
        ok && p.clean() && sealed.is_ok() && p.sealed,
        "缺席面挂两账逐笔清后预案收束",
    );

    // J2 清账后全账零未清
    s.add(
        "J2-全账清",
        p.unsettled().is_empty() && p.hangs.len() == 2,
        "两笔挂账全清且账目保留",
    );

    // J3 注入检出：坏面号 + 出席面挂账
    let inj = inject_four();
    s.add("J3-坏面出席检出", inj[0] && inj[1], "坏面号与出席面挂账必拒");

    // J4 注入检出：空缘由 + 无账可清
    s.add("J4-空缘无账检出", inj[2] && inj[3], "空缘由与无账可清必拒");

    // J5 缺席未挂不成收束？——不，缺席面无事项可挂也收束（零挂账）
    let j5 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(0, false);
        t.clean()
    };
    s.add("J5-零挂亦收", j5, "缺席面无挂账亦算收束");

    // J6 出席面挂账破坏收束（先挂后改出席位）
    let j6 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(1, false);
        let _ = t.hang(1, "挂了");
        let _ = t.set_presence(1, true);
        !t.clean() && t.present_hangs() == 1
    };
    s.add("J6-转席挂账", j6, "挂账面转为出席即破坏收束");

    // J7 挂账超帽必拒
    let j7 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(0, false);
        let mut n = 0usize;
        let mut hit = false;
        while n < HANG_LIMIT {
            let rs = "帽样";
            if t.hang(0, rs).is_err() {
                hit = true;
            }
            n += 1;
        }
        !hit && t.hang(0, "超帽").err() == Some(FB_LIMIT)
    };
    s.add("J7-挂账超帽", j7, "超帽必拒且原账不损");

    // J8 封账后动必拒
    let j8 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(2, false);
        let _ = t.hang(2, "封样");
        let _ = t.settle(2);
        let _ = t.seal();
        t.hang(2, "迟").err() == Some(FB_SEALED)
            && t.set_presence(0, false).err() == Some(FB_SEALED)
    };
    s.add("J8-封账拒动", j8, "封账后挂账与报席均拒");

    // J9 未清阻断收束
    let j9 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(2, false);
        let _ = t.hang(2, "未清");
        !t.clean() && t.seal().err() == Some(FB_NOHANG)
    };
    s.add("J9-未清不收", j9, "有未清笔即不得收束");

    // J10 指纹随清账推进且封账后稳定
    let j10 = {
        let mut t = FallbackPlan::new();
        let _ = t.set_presence(2, false);
        let _ = t.hang(2, "指样");
        let f0 = t.fold();
        let _ = t.settle(2);
        let f1 = t.fold();
        let _ = t.seal();
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "清账与封账各改指纹且终态稳定");

    // J11 缘由逐笔可读
    let j11 = {
        let mut all = true;
        let mut k = 0usize;
        while k < p.hangs.len() {
            if p.hangs[k].reason.is_empty() {
                all = false;
            }
            k += 1;
        }
        all && p.hangs.len() > 0
    };
    s.add("J11-缘由在账", j11, "每笔挂账缘由非空");

    // J12 动态明细：未清计数落账
    let un = p.unsettled().len();
    s.add_dyn("J12-未清计数", un == 0, &format!("unsettled={}", un));

    // J13 动态明细：预案指纹落账
    let fp = p.fold();
    s.add_dyn("J13-预案指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10337_fallback_flow() {
        let mut p = FallbackPlan::new();
        assert!(p.set_presence(3, true).err() == Some(FB_FACE));
        assert!(p.hang(0, "出席挂").err() == Some(FB_PRESENT));
        assert!(p.set_presence(1, false).is_ok());
        assert!(p.hang(1, "").err() == Some(FB_NOREASON));
        assert!(p.hang(1, "缺席挂一").is_ok());
        assert!(p.hang(1, "缺席挂二").is_ok());
        assert!(p.settle(2).err() == Some(FB_NOHANG));
        assert!(p.settle(1).is_ok());
        assert!(p.unsettled().len() == 1);
        assert!(p.settle(1).is_ok());
        assert!(p.settle(1).err() == Some(FB_NOHANG));
        assert!(p.clean());
        assert!(p.seal().is_ok());
        assert!(p.hang(1, "封后").err() == Some(FB_SEALED));
    }

    #[test]
    fn unx10337_checks_green() {
        let s = run_unx10337_checks();
        assert!(s.all_passed(), "UNX-F10337 判据未全绿");
    }
}
