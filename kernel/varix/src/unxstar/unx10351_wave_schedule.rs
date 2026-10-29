//! UNX-F10351 wave-06 时序表（正演时间表落账）（0x1b660 段）
//!
//! 锚点：UNX-F10351-J1 正演时间表落账——四个时相（准备/干跑/正演/收尾）
//! 逐相落时段（起点必须早于终点），相序必须递增登记，任两相时段不得
//! 交叠，四相齐落零交叠才算时间表落账。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：时段账（槽=相号+起点+终点）+ 时值核（起终倒挂即拒）+ 相序核
//! （相号严格递增登记）+ 交叠核（跨相交叠即拒）+ 注入核（坏相号/倒挂/
//! 重相/交叠四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b660 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b660;
pub const WS_PHASE: u32 = SEG_BASE + 0x01; // 相号越界
pub const WS_ORDER: u32 = SEG_BASE + 0x02; // 起终倒挂
pub const WS_DUP: u32 = SEG_BASE + 0x03; // 相号重落
pub const WS_OVERLAP: u32 = SEG_BASE + 0x04; // 时段交叠
pub const WS_SEQ: u32 = SEG_BASE + 0x05; // 相序倒挂
pub const WS_CAP: u32 = SEG_BASE + 0x06; // 表超容

/// 四时相。
pub const PH_PREP: u8 = 1;
pub const PH_DRY: u8 = 2;
pub const PH_LIVE: u8 = 3;
pub const PH_POST: u8 = 4;
pub const PH_N: u8 = 5;

/// 相数锚（四相）。
pub const PHASES_NEEDED: usize = 4;
/// 表定容。
pub const CAP: usize = 6;

/// 时段。
#[derive(Clone)]
pub struct Slot {
    pub ph: u8,
    pub begin: u32,
    pub end: u32,
}

impl Slot {
    /// 时长。
    pub fn span(&self) -> u32 {
        if self.end > self.begin {
            self.end - self.begin
        } else {
            0
        }
    }

    /// 与它槽交叠。
    pub fn overlaps(&self, o: &Slot) -> bool {
        self.begin < o.end && o.begin < self.end
    }
}

/// 正演时间表。
pub struct WaveSchedule {
    pub slots: Vec<Slot>,
    pub sealed: bool,
}

impl WaveSchedule {
    pub fn new() -> WaveSchedule {
        WaveSchedule { slots: Vec::new(), sealed: false }
    }

    fn pos(&self, ph: u8) -> Option<usize> {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].ph == ph {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 落时段：相序递增、起终有序、跨相不交叠。
    pub fn book(&mut self, ph: u8, begin: u32, end: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(WS_CAP);
        }
        if ph == 0 || ph >= PH_N {
            return Err(WS_PHASE);
        }
        if self.pos(ph).is_some() {
            return Err(WS_DUP);
        }
        if end <= begin {
            return Err(WS_ORDER);
        }
        // 相序核：新相号必须大于账内全部相号
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].ph >= ph {
                return Err(WS_SEQ);
            }
            k += 1;
        }
        // 交叠核
        let cand = Slot { ph, begin, end };
        let mut k = 0usize;
        while k < self.slots.len() {
            if cand.overlaps(&self.slots[k]) {
                return Err(WS_OVERLAP);
            }
            k += 1;
        }
        if self.slots.len() >= CAP {
            return Err(WS_CAP);
        }
        self.slots.push(cand);
        Ok(())
    }

    /// 未落相清单。
    pub fn missing(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut p = 1u8;
        while p < PH_N {
            if self.pos(p).is_none() {
                v.push(p);
            }
            p += 1;
        }
        v
    }

    /// 全长（首槽起点至末槽终点）。
    pub fn horizon(&self) -> u32 {
        if self.slots.is_empty() {
            return 0;
        }
        let b = self.slots[0].begin;
        let n = self.slots.len();
        let e = self.slots[n - 1].end;
        if e > b {
            e - b
        } else {
            0
        }
    }

    /// 相间间隙合计。
    pub fn gaps(&self) -> u32 {
        let mut g = 0u32;
        let mut k = 1usize;
        while k < self.slots.len() {
            let prev_end = self.slots[k - 1].end;
            let cur_begin = self.slots[k].begin;
            if cur_begin > prev_end {
                g += cur_begin - prev_end;
            }
            k += 1;
        }
        g
    }

    /// 落账毕：四相齐且账内两两不交叠。
    pub fn booked(&self) -> bool {
        self.missing().is_empty()
            && self.slots.len() >= PHASES_NEEDED
            && {
                let mut i = 0usize;
                let mut clean = true;
                while i < self.slots.len() {
                    let mut j = i + 1;
                    while j < self.slots.len() {
                        if self.slots[i].overlaps(&self.slots[j]) {
                            clean = false;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                clean
            }
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(WS_CAP);
        }
        if !self.booked() {
            return Err(WS_OVERLAP);
        }
        self.sealed = true;
        Ok(())
    }

    /// 时序表指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6600;
        h = h.wrapping_mul(31).wrapping_add(self.slots.len() as u32);
        let mut k = 0usize;
        while k < self.slots.len() {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.slots[k].ph as u32)
                .wrapping_mul(31)
                .wrapping_add(self.slots[k].begin)
                .wrapping_mul(31)
                .wrapping_add(self.slots[k].end);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut w = WaveSchedule::new();
    // 1) 坏相号必拒
    got[0] = w.book(0, 1, 2).err() == Some(WS_PHASE)
        && w.book(5, 1, 2).err() == Some(WS_PHASE);
    // 2) 起终倒挂必拒
    got[1] = w.book(PH_PREP, 10, 10).err() == Some(WS_ORDER)
        && w.book(PH_PREP, 12, 5).err() == Some(WS_ORDER);
    // 3) 重相必拒
    let _ = w.book(PH_PREP, 1, 5);
    got[2] = w.book(PH_PREP, 6, 9).err() == Some(WS_DUP);
    // 4) 相序倒挂必拒
    got[3] = w.book(PH_POST, 20, 30).err() == Some(WS_SEQ);
    got
}

/// 批内判据。
pub fn run_unx10351_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10351");

    // J1 正路径：四相按时序落账零交叠并封表
    let mut w = WaveSchedule::new();
    let plan = [(PH_PREP, 0u32, 10u32), (PH_DRY, 10, 20), (PH_LIVE, 20, 40), (PH_POST, 40, 50)];
    let mut ok = true;
    let mut k = 0usize;
    while k < plan.len() {
        if w.book(plan[k].0, plan[k].1, plan[k].2).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = w.seal();
    s.add(
        "J1-正演时间表",
        ok && w.booked() && sealed.is_ok() && w.sealed,
        "四相按序落账零交叠并封表",
    );

    // J2 全长与零间隙
    let j2 = w.horizon() == 50 && w.gaps() == 0;
    s.add("J2-全长间隙", j2, "全长五十刻且相间零间隙");

    // J3 注入检出：坏相号 + 倒挂
    let inj = inject_four();
    s.add("J3-相号倒挂检出", inj[0] && inj[1], "坏相号与起终倒挂必拒");

    // J4 注入检出：重相 + 相序倒挂
    s.add("J4-重相序挂检出", inj[2] && inj[3], "重相与相序倒挂必拒");

    // J5 交叠必拒
    let j5 = {
        let mut t = WaveSchedule::new();
        let _ = t.book(PH_PREP, 0, 10);
        let _ = t.book(PH_DRY, 10, 20);
        t.book(PH_LIVE, 15, 30).err() == Some(WS_OVERLAP)
    };
    s.add("J5-交叠拒落", j5, "跨相时段交叠必拒");

    // J6 未落全不成账
    let j6 = {
        let mut t = WaveSchedule::new();
        let _ = t.book(PH_PREP, 0, 10);
        let _ = t.book(PH_DRY, 10, 20);
        !t.booked() && t.missing().contains(&PH_LIVE) && t.missing().contains(&PH_POST)
    };
    s.add("J6-缺相不成", j6, "缺相可枚举且不成账");

    // J7 相时长逐相核
    let j7 = w.slots[0].span() == 10
        && w.slots[1].span() == 10
        && w.slots[2].span() == 20
        && w.slots[3].span() == 10;
    s.add("J7-相时长核", j7, "四相时长 10/10/20/10");

    // J8 封表后落槽必拒
    let j8 = {
        let mut t = WaveSchedule::new();
        let plan2 = [(PH_PREP, 0u32, 5u32), (PH_DRY, 5, 10), (PH_LIVE, 10, 15), (PH_POST, 15, 20)];
        let mut q = 0usize;
        while q < plan2.len() {
            let _ = t.book(plan2[q].0, plan2[q].1, plan2[q].2);
            q += 1;
        }
        let _ = t.seal();
        t.book(PH_POST, 20, 25).err() == Some(WS_CAP)
    };
    s.add("J8-封表拒落", j8, "封表后落槽必拒");

    // J9 容量边界
    let j9 = {
        let mut t = WaveSchedule::new();
        let mut q = 0u32;
        let mut filled = true;
        while (q as usize) < CAP {
            let ph = ((q % 4) + 1) as u8;
            // 同相重落即拒，故只在前四相可落；此处按相序落四相再验帽
            if q < 4 {
                let b = q * 10;
                if t.book(ph, b, b + 5).is_err() {
                    filled = false;
                }
            }
            q += 1;
        }
        filled && t.book(PH_N, 999, 1000).err() == Some(WS_PHASE)
    };
    s.add("J9-容量边界", j9, "相域封闭且越界必拒");

    // J10 指纹随落槽推进且封表后稳定
    let j10 = {
        let mut t = WaveSchedule::new();
        let f0 = t.fold();
        let _ = t.book(PH_PREP, 0, 5);
        let f1 = t.fold();
        let _ = t.book(PH_DRY, 5, 9);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每落一槽指纹一变且终态稳定");

    // J11 相号递增核（账内相号严格递增）
    let j11 = {
        let mut all = true;
        let mut k = 1usize;
        while k < w.slots.len() {
            if w.slots[k].ph <= w.slots[k - 1].ph {
                all = false;
            }
            k += 1;
        }
        all
    };
    s.add("J11-相号递增", j11, "账内相号严格递增");

    // J12 动态明细：缺相计数落账
    let ms = w.missing().len();
    s.add_dyn("J12-缺相计数", ms == 0, &format!("missing={}", ms));

    // J13 动态明细：时序表指纹落账
    let fp = w.fold();
    s.add_dyn("J13-时序指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10351_wave_schedule_flow() {
        let mut w = WaveSchedule::new();
        assert!(w.book(0, 1, 2).err() == Some(WS_PHASE));
        assert!(w.book(PH_PREP, 5, 5).err() == Some(WS_ORDER));
        assert!(w.book(PH_PREP, 5, 3).err() == Some(WS_ORDER));
        assert!(w.book(PH_PREP, 0, 10).is_ok());
        assert!(w.book(PH_PREP, 11, 12).err() == Some(WS_DUP));
        assert!(w.book(PH_POST, 20, 30).err() == Some(WS_SEQ));
        assert!(w.book(PH_DRY, 5, 15).err() == Some(WS_OVERLAP));
        assert!(w.book(PH_DRY, 10, 20).is_ok());
        assert!(!w.booked());
        assert!(w.book(PH_LIVE, 20, 40).is_ok());
        assert!(w.book(PH_POST, 40, 50).is_ok());
        assert!(w.booked());
        assert!(w.gaps() == 0);
        assert!(w.seal().is_ok());
        assert!(w.book(PH_POST, 50, 60).err() == Some(WS_CAP));
    }

    #[test]
    fn unx10351_checks_green() {
        let s = run_unx10351_checks();
        assert!(s.all_passed(), "UNX-F10351 判据未全绿");
    }
}
