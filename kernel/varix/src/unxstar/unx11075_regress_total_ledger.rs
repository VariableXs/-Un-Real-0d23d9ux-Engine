//! UNX-F11075 回归结果总账（0x1e960 段）
//!
//! 五族矩阵三态汇总：F/M/E/I/T 五族各自登记 (green, red, open, claimed)，
//! 每个段再按 (seg, family, g, r, o) 落一格。总账要求两本账对平——
//! 族账的三态和等于 claimed，段账按族求和等于族账，且 open 归零后才允许
//! 闭合。三态语义：open>0 未决、red>0 红、否则绿。
//!
//! 不变式：Σ族 claimed == Σ段 total；closed 仅当全族 open==0；冻结后
//! 落族/落段/改写全以 TL_FROZEN 拒止。
//!
//! 结构：FamilyTally（族账行）/ SegmentTally（段账格）/ TotalLedger
//! （五族总账 + 对平 + 闭合 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11075 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e960;

/// 空账：段号为 0、族无段可对
pub const TL_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：族号不在 0..=4、单格计数越界
pub const TL_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：族或段重复落账
pub const TL_DUP: u32 = SEG_BASE + 0x03;
/// 失配：claimed 不平、段求和与族账不符
pub const TL_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：仍有 open 未决、或族/段不足
pub const TL_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：总账冻结后的落账/改写
pub const TL_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x75 标记 11075 号卡
pub const TL_TAG: u64 = 0x1e80_75;

/// 五族：F 地基 / M 机制 / E 错误矩阵 / I 联测 / T 终端
const FAMILIES: usize = 5;
/// 十段
const SEGMENTS: usize = 10;
/// 单族三态计数上限
const FAMILY_CELL_MAX: u16 = 512;
/// 单段三态计数上限
const SEGMENT_CELL_MAX: u8 = 64;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 三态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// 全绿
    Green,
    /// 存在红格
    Red,
    /// 存在未决格
    Open,
}

impl Tone {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            Tone::Green => 0,
            Tone::Red => 1,
            Tone::Open => 2,
        }
    }

    /// 三态名
    pub fn name(self) -> &'static str {
        match self {
            Tone::Green => "green",
            Tone::Red => "red",
            Tone::Open => "open",
        }
    }
}

/// 族账行
#[derive(Clone, Debug)]
pub struct FamilyTally {
    /// 族号 0..=4
    pub family: u8,
    /// 绿格
    pub green: u16,
    /// 红格
    pub red: u16,
    /// 未决格
    pub open: u16,
    /// 落账时声明的总数
    pub claimed: u16,
}

impl FamilyTally {
    /// 新建族账行
    pub fn new(family: u8, green: u16, red: u16, open: u16, claimed: u16) -> FamilyTally {
        FamilyTally { family, green, red, open, claimed }
    }

    /// 三态和
    pub fn total(&self) -> u16 {
        self.green.wrapping_add(self.red).wrapping_add(self.open)
    }

    /// 三态判定：open 优先，其次 red
    pub fn tone(&self) -> Tone {
        if self.open > 0 {
            Tone::Open
        } else if self.red > 0 {
            Tone::Red
        } else {
            Tone::Green
        }
    }

    /// 族账指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_byte(h, self.family);
        h = fnv_u16(h, self.green);
        h = fnv_u16(h, self.red);
        h = fnv_u16(h, self.open);
        fnv_u16(h, self.claimed)
    }
}

/// 段账格
#[derive(Clone, Debug)]
pub struct SegmentTally {
    /// 段号（1..=10）
    pub seg: u16,
    /// 所属族 0..=4
    pub family: u8,
    /// 绿格
    pub green: u8,
    /// 红格
    pub red: u8,
    /// 未决格
    pub open: u8,
}

impl SegmentTally {
    /// 新建段账格
    pub fn new(seg: u16, family: u8, green: u8, red: u8, open: u8) -> SegmentTally {
        SegmentTally { seg, family, green, red, open }
    }

    /// 三态和
    pub fn total(&self) -> u16 {
        (self.green as u16)
            .wrapping_add(self.red as u16)
            .wrapping_add(self.open as u16)
    }

    /// 段账指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.seg);
        h = fnv_byte(h, self.family);
        h = fnv_byte(h, self.green);
        h = fnv_byte(h, self.red);
        fnv_byte(h, self.open)
    }
}

/// 五族三态总账
#[derive(Clone, Debug)]
pub struct TotalLedger {
    /// 族账
    pub families: Vec<FamilyTally>,
    /// 段账
    pub segments: Vec<SegmentTally>,
    /// 是否已闭合
    pub closed: bool,
    /// 最近一次核对的绿格总数
    pub verified: u16,
    frozen: Option<u64>,
}

impl TotalLedger {
    /// 空总账
    pub fn new() -> TotalLedger {
        TotalLedger { families: Vec::new(), segments: Vec::new(), closed: false, verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(TL_FROZEN);
        }
        Ok(())
    }

    fn family_index(&self, family: u8) -> Option<usize> {
        let mut k = 0usize;
        while k < self.families.len() {
            if self.families[k].family == family {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn segment_index(&self, seg: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.segments.len() {
            if self.segments[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 落族账：族号/重号/计数越界/claimed 不平四道闸
    pub fn post_family(
        &mut self,
        family: u8,
        green: u16,
        red: u16,
        open: u16,
        claimed: u16,
    ) -> Result<(), u32> {
        self.guard()?;
        if family as usize >= FAMILIES {
            return Err(TL_RANGE);
        }
        if self.family_index(family).is_some() {
            return Err(TL_DUP);
        }
        if green > FAMILY_CELL_MAX || red > FAMILY_CELL_MAX || open > FAMILY_CELL_MAX {
            return Err(TL_RANGE);
        }
        let sum = green.wrapping_add(red).wrapping_add(open);
        if sum != claimed {
            return Err(TL_MISMATCH);
        }
        self.families.push(FamilyTally::new(family, green, red, open, claimed));
        Ok(())
    }

    /// 落段账：0 号/重号/未知族/计数越界四道闸
    pub fn post_segment(
        &mut self,
        seg: u16,
        family: u8,
        green: u8,
        red: u8,
        open: u8,
    ) -> Result<(), u32> {
        self.guard()?;
        if seg == 0 {
            return Err(TL_EMPTY);
        }
        if self.segment_index(seg).is_some() {
            return Err(TL_DUP);
        }
        if self.family_index(family).is_none() {
            return Err(TL_MISMATCH);
        }
        if green > SEGMENT_CELL_MAX || red > SEGMENT_CELL_MAX || open > SEGMENT_CELL_MAX {
            return Err(TL_RANGE);
        }
        self.segments.push(SegmentTally::new(seg, family, green, red, open));
        Ok(())
    }

    /// 改写段账（段重跑后回填）
    pub fn revise_segment(
        &mut self,
        seg: u16,
        green: u8,
        red: u8,
        open: u8,
    ) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.segment_index(seg) {
            Some(i) => i,
            None => return Err(TL_EMPTY),
        };
        if green > SEGMENT_CELL_MAX || red > SEGMENT_CELL_MAX || open > SEGMENT_CELL_MAX {
            return Err(TL_RANGE);
        }
        self.segments[idx].green = green;
        self.segments[idx].red = red;
        self.segments[idx].open = open;
        Ok(())
    }

    /// 单族段账求和
    pub fn segment_sum(&self, family: u8) -> Option<(u16, u16, u16)> {
        if self.family_index(family).is_none() {
            return None;
        }
        let mut g = 0u16;
        let mut r = 0u16;
        let mut o = 0u16;
        let mut seen = false;
        let mut k = 0usize;
        while k < self.segments.len() {
            let s = &self.segments[k];
            if s.family == family {
                g = g.wrapping_add(s.green as u16);
                r = r.wrapping_add(s.red as u16);
                o = o.wrapping_add(s.open as u16);
                seen = true;
            }
            k += 1;
        }
        if seen {
            Some((g, r, o))
        } else {
            None
        }
    }

    /// 段账按族求和与族账对平
    pub fn reconcile(&self) -> Result<(), u32> {
        let mut k = 0usize;
        while k < self.families.len() {
            let f = &self.families[k];
            match self.segment_sum(f.family) {
                Some((g, r, o)) => {
                    if g != f.green || r != f.red || o != f.open {
                        return Err(TL_MISMATCH);
                    }
                }
                None => return Err(TL_EMPTY),
            }
            k += 1;
        }
        Ok(())
    }

    /// 守恒：每族三态和等于 claimed，且族 claimed 之和等于段账总和
    pub fn conservation(&self) -> bool {
        let mut claim_sum = 0u16;
        let mut k = 0usize;
        while k < self.families.len() {
            let f = &self.families[k];
            if f.total() != f.claimed {
                return false;
            }
            claim_sum = claim_sum.wrapping_add(f.claimed);
            k += 1;
        }
        let mut seg_total = 0u16;
        k = 0;
        while k < self.segments.len() {
            seg_total = seg_total.wrapping_add(self.segments[k].total());
            k += 1;
        }
        claim_sum == seg_total
    }

    /// 绿格总数
    pub fn green_total(&self) -> u16 {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.families.len() {
            n = n.wrapping_add(self.families[k].green);
            k += 1;
        }
        n
    }

    /// 三态族计数：(green, red, open)
    pub fn tone_counts(&self) -> (u16, u16, u16) {
        let mut g = 0u16;
        let mut r = 0u16;
        let mut o = 0u16;
        let mut k = 0usize;
        while k < self.families.len() {
            match self.families[k].tone() {
                Tone::Green => g = g.wrapping_add(1),
                Tone::Red => r = r.wrapping_add(1),
                Tone::Open => o = o.wrapping_add(1),
            }
            k += 1;
        }
        (g, r, o)
    }

    /// 闭合：先对平，再要求全族 open==0
    pub fn close(&mut self) -> Result<u16, u32> {
        self.guard()?;
        self.reconcile()?;
        let mut k = 0usize;
        while k < self.families.len() {
            if self.families[k].open > 0 {
                return Err(TL_SHORT);
            }
            k += 1;
        }
        let green = self.green_total();
        self.verified = green;
        self.closed = true;
        Ok(green)
    }

    /// 总账核对：五族十段、守恒、对平、已闭合
    pub fn audit(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.families.len() < FAMILIES || self.segments.len() < SEGMENTS {
            return Err(TL_SHORT);
        }
        self.reconcile()?;
        if !self.closed {
            return Err(TL_SHORT);
        }
        if !self.conservation() {
            return Err(TL_MISMATCH);
        }
        self.verified = self.green_total();
        Ok(self.verified)
    }

    /// 总账指纹
    pub fn fold(&self) -> u64 {
        let mut h = TL_TAG;
        h = fnv_byte(h, if self.closed { 1 } else { 0 });
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.families.len() {
            h = self.families[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.segments.len() {
            h = self.segments[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：族越界 / 族重号 / 零段号 / 未决族闭合
pub fn inject_four() -> [bool; 4] {
    let mut l0 = TotalLedger::new();
    let range = matches!(l0.post_family(5, 1, 0, 0, 1), Err(TL_RANGE));
    let mut l1 = TotalLedger::new();
    let first = l1.post_family(0, 1, 0, 0, 1).is_ok();
    let dup = first && matches!(l1.post_family(0, 1, 0, 0, 1), Err(TL_DUP));
    let mut l2 = TotalLedger::new();
    let empty = l2.post_family(0, 1, 0, 0, 1).is_ok()
        && matches!(l2.post_segment(0, 0, 1, 0, 0), Err(TL_EMPTY));
    let mut l3 = TotalLedger::new();
    let seeded = l3.post_family(0, 1, 0, 1, 2).is_ok()
        && l3.post_segment(1, 0, 1, 0, 1).is_ok();
    let short = seeded && matches!(l3.close(), Err(TL_SHORT));
    [range, dup, empty, short]
}

/// 构造五族十段样本（每族两段各 1 绿，闭合）
fn sample_ledger() -> Result<TotalLedger, u32> {
    let mut led = TotalLedger::new();
    let mut f = 0u8;
    while (f as usize) < FAMILIES {
        led.post_family(f, 2, 0, 0, 2)?;
        f = f.wrapping_add(1);
    }
    let mut seg = 1u16;
    let mut fam = 0u8;
    while (fam as usize) < FAMILIES {
        let a = seg;
        let b = seg.wrapping_add(1);
        led.post_segment(a, fam, 1, 0, 0)?;
        led.post_segment(b, fam, 1, 0, 0)?;
        seg = seg.wrapping_add(2);
        fam = fam.wrapping_add(1);
    }
    led.close()?;
    Ok(led)
}

/// UNX-F11075 判据入口
pub fn run_unx11075_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11075");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => TotalLedger::new(),
    };
    let green = led.audit().unwrap_or(0);
    s.add(
        "J1a-三态账逐段对平",
        green == 10 && led.segments.len() == SEGMENTS && led.families.len() == FAMILIES,
        "五族十段必须逐族对平后给出十绿",
    );
    s.add(
        "J1b-总账守恒闭合",
        led.closed && led.conservation() && led.reconcile().is_ok(),
        "闭合位、守恒式、段族对平三者必须同时成立",
    );
    let (tg, tr, to) = led.tone_counts();
    s.add(
        "J2-三态守恒",
        tg == 5 && tr == 0 && to == 0 && led.green_total() == 10,
        "五族全绿、红/未决为零、绿总数十",
    );
    let inj = inject_four();
    s.add("J3a-族越界拒止", inj[0], "族号 5 必须 TL_RANGE");
    s.add("J3b-族重号拒止", inj[1], "同族二次落账必须 TL_DUP");
    s.add("J3c-零段号拒止", inj[2], "seg=0 落段必须 TL_EMPTY");
    s.add("J3d-未决闭合拒", inj[3], "open>0 时闭合必须 TL_SHORT");
    let mut fz = match sample_ledger() {
        Ok(v) => v,
        Err(_) => TotalLedger::new(),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.post_family(0, 1, 0, 0, 1), Err(TL_FROZEN))
        && matches!(fz.post_segment(11, 0, 1, 0, 0), Err(TL_FROZEN))
        && matches!(fz.revise_segment(1, 0, 1, 0), Err(TL_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后落族/落段/改写全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut b = match sample_ledger() {
        Ok(v) => v,
        Err(_) => TotalLedger::new(),
    };
    let sens = b.post_segment(11, 0, 1, 0, 0).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "多落一段必须改变 fold");
    let claim_off = matches!(
        TotalLedger::new().post_family(0, 2, 1, 0, 2),
        Err(TL_MISMATCH)
    );
    let cell_over = matches!(
        TotalLedger::new().post_family(0, 600, 0, 0, 600),
        Err(TL_RANGE)
    );
    let unknown_family = {
        let mut g = TotalLedger::new();
        g.post_family(0, 1, 0, 0, 1).is_ok()
            && matches!(g.post_segment(1, 3, 1, 0, 0), Err(TL_MISMATCH))
    };
    s.add(
        "J6-范围与不平拒止",
        claim_off && cell_over && unknown_family,
        "claimed 不平/单格越界/未知族三路都必须拒",
    );
    let mut fam_order = true;
    let mut k = 1usize;
    while k < led.families.len() {
        if led.families[k].family <= led.families[k - 1].family {
            fam_order = false;
        }
        k += 1;
    }
    let mut seg_order = true;
    k = 1;
    while k < led.segments.len() {
        if led.segments[k].seg <= led.segments[k - 1].seg {
            seg_order = false;
        }
        k += 1;
    }
    s.add(
        "J7-唯一与序不变式",
        fam_order && seg_order && led.segments[0].seg == 1 && led.segments[9].seg == 10,
        "族号 0..=4、段号 1..=10 唯一递增",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => TotalLedger::new(),
    };
    while partial.families.len() > 3 {
        partial.families.pop();
    }
    partial.closed = false;
    let negative = partial.families.len() == 3 && matches!(partial.audit(), Err(TL_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "三族半账必须 TL_SHORT，不得伪装成绿",
    );
    let drift = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => TotalLedger::new(),
        };
        g.revise_segment(1, 2, 0, 0).is_ok() && matches!(g.reconcile(), Err(TL_MISMATCH))
    };
    let reopen = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => TotalLedger::new(),
        };
        g.revise_segment(1, 0, 1, 0).is_ok()
            && matches!(g.reconcile(), Err(TL_MISMATCH))
            && g.families[0].tone() == Tone::Green
    };
    s.add(
        "J10-段漂移可归因",
        drift && reopen,
        "单段改写必须破坏对平且不改族账三态",
    );
    s.add_dyn(
        "J9-动态账",
        green == 10 && led.closed,
        &format!(
            "green={} 族={} 段={} 三态={}/{}/{}",
            green,
            led.families.len(),
            led.segments.len(),
            tg,
            tr,
            to
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), TL_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11075_flow() {
        let mut led = sample_ledger().unwrap_or(TotalLedger::new());
        assert_eq!(led.audit().unwrap_or(0), 10);
        assert!(led.closed);
        assert!(led.conservation());
        assert!(led.reconcile().is_ok());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11075_checks_green() {
        assert!(run_unx11075_checks().all_passed(), "UNX-F11075 判据未全绿");
    }
}
