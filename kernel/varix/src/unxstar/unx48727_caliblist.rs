//! UNX-F48727 C·试产校准清单终版（全部「试产校准定」条目汇总）
//! 判据：UNX-F48727-J1 清单零漏项；判据锚定 §40 诚实登记。
//! 纪律：登记-校准-冻结三段闭环，试产项未校准必抓、冻结后篡改必抓、40 域覆盖守恒。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1680;

const E_DUP_ITEM: u16 = SEG_BASE + 1; // 条目重复
const E_NOT_CALIB: u16 = SEG_BASE + 2; // 试产项未校准
const E_CAL_RANGE: u16 = SEG_BASE + 3; // 校准值超域
const E_DOM_MISS: u16 = SEG_BASE + 4; // 域缺漏
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 冻结后篡改
const E_STATE_BAD: u16 = SEG_BASE + 6; // 状态非法
const E_EXPORT_BREAK: u16 = SEG_BASE + 7; // 导出计数守恒破
const E_NAME_EMPTY: u16 = SEG_BASE + 8; // 项名空白

/// 域数。
pub const DOM_N: u32 = 40;
/// 校准值域上界。
pub const CAL_MAX: u32 = 0xFFFF;

/// 条目状态。
#[derive(Clone, Copy, PartialEq)]
pub enum CalState {
    /// 试产（未定）。
    Trial = 0,
    /// 已校准（校准值就位）。
    Pinned = 1,
    /// 冻结（不可再改）。
    Frozen = 2,
}

/// 校准条目。
#[derive(Clone)]
pub struct CalItem {
    /// 域号（1..=40）。
    pub dom: u32,
    /// 域内序号。
    pub seq: u32,
    /// 项名。
    pub name: String,
    /// 状态。
    pub state: u8,
    /// 校准值（Pinned/Frozen 有效）。
    pub cal: u32,
}

/// 试产校准清单。
pub struct CalibList {
    items: Vec<CalItem>,
    seen: Vec<(u32, u32)>,
    frozen_n: u32,
}

impl CalibList {
    pub fn new() -> CalibList {
        CalibList {
            items: Vec::new(),
            seen: Vec::new(),
            frozen_n: 0,
        }
    }

    /// 登记条目（域合法、项名非空、域内唯一）。
    pub fn put(&mut self, dom: u32, seq: u32, name: &str) -> Result<bool, u16> {
        if dom == 0 || dom > DOM_N {
            return Err(E_DOM_MISS);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        for s in self.seen.iter() {
            if s.0 == dom && s.1 == seq {
                return Err(E_DUP_ITEM);
            }
        }
        self.seen.push((dom, seq));
        self.items.push(CalItem {
            dom,
            seq,
            name: name.to_string(),
            state: CalState::Trial as u8,
            cal: 0,
        });
        Ok(true)
    }

    /// 校准：写入校准值（Trial→Pinned）。
    pub fn calibrate(&mut self, dom: u32, seq: u32, val: u32) -> Result<bool, u16> {
        if val == 0 || val > CAL_MAX {
            return Err(E_CAL_RANGE);
        }
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.dom == dom && it.seq == seq {
                found = true;
                if it.state == CalState::Frozen as u8 {
                    return Err(E_FROZEN_TOUCH);
                }
                it.cal = val;
                it.state = CalState::Pinned as u8;
                break;
            }
        }
        if !found {
            return Err(E_DOM_MISS);
        }
        Ok(true)
    }

    /// 冻结：Pinned→Frozen（不可逆）。
    pub fn freeze(&mut self, dom: u32, seq: u32) -> Result<bool, u16> {
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.dom == dom && it.seq == seq {
                found = true;
                if it.state != CalState::Pinned as u8 {
                    return Err(E_NOT_CALIB);
                }
                it.state = CalState::Frozen as u8;
                self.frozen_n += 1;
                break;
            }
        }
        if !found {
            return Err(E_DOM_MISS);
        }
        Ok(true)
    }

    /// 清单零漏项：全部条目必须 ≥Pinned（有校准值）。
    pub fn assert_no_leak(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.state == CalState::Trial as u8 {
                return Err(E_NOT_CALIB);
            }
            if it.cal == 0 {
                return Err(E_CAL_RANGE);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 冻结态断言：全部条目 Frozen。
    pub fn assert_all_frozen(&self) -> Result<(), u16> {
        for it in self.items.iter() {
            if it.state != CalState::Frozen as u8 {
                return Err(E_STATE_BAD);
            }
        }
        if self.frozen_n != self.items.len() as u32 {
            return Err(E_EXPORT_BREAK);
        }
        Ok(())
    }

    /// 域覆盖守恒：1..=40 每域至少一条。
    pub fn assert_dom_cover(&self) -> Result<(), u16> {
        let mut d = 1u32;
        while d <= DOM_N {
            let mut hit = false;
            for it in self.items.iter() {
                if it.dom == d {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_DOM_MISS);
            }
            d += 1;
        }
        Ok(())
    }

    /// 终版导出：全部 Frozen 条目快照。
    pub fn export(&self) -> Result<Vec<CalItem>, u16> {
        self.assert_all_frozen()?;
        let mut out: Vec<CalItem> = Vec::new();
        for it in self.items.iter() {
            out.push(it.clone());
        }
        if out.len() as u32 != self.frozen_n {
            return Err(E_EXPORT_BREAK);
        }
        Ok(out)
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 试产校准清单主判据：40 域登记-校准-冻结-零漏项终版。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 域 × 每域 2 条登记
    let mut cl = CalibList::new();
    let mut d = 1u32;
    while d <= DOM_N {
        cl.put(d, 1, "alpha")?;
        cl.put(d, 2, "beta")?;
        d += 1;
    }
    if cl.len() != 80 {
        return Err(E_DUP_ITEM);
    }
    hits += 1;

    // R2 重复条目必拒
    if cl.put(5, 1, "alpha").err() == Some(E_DUP_ITEM) {
        hits += 1;
    } else {
        return Err(E_DUP_ITEM);
    }

    // R3 域越界必拒
    if cl.put(41, 1, "over").err() == Some(E_DOM_MISS) {
        hits += 1;
    } else {
        return Err(E_DOM_MISS);
    }

    // R4 空名必拒
    if cl.put(5, 3, "").err() == Some(E_NAME_EMPTY) {
        hits += 1;
    } else {
        return Err(E_NAME_EMPTY);
    }

    // R5 未校准先冻结必拒
    if cl.freeze(1, 5).err() == Some(E_NOT_CALIB) {
        hits += 1;
    } else {
        return Err(E_NOT_CALIB);
    }

    // R6 校准值零值/超域必拒
    if cl.calibrate(1, 5, 0).err() == Some(E_CAL_RANGE) {
        hits += 1;
    } else {
        return Err(E_CAL_RANGE);
    }
    if cl.calibrate(1, 5, CAL_MAX + 1).err() == Some(E_CAL_RANGE) {
        hits += 1;
    } else {
        return Err(E_CAL_RANGE);
    }

    // R7 全量校准 + 冻结 + 零漏项
    let mut d2 = 1u32;
    while d2 <= DOM_N {
        cl.calibrate(d2, 1, 0x1000 + d2)?;
        cl.calibrate(d2, 2, 0x2000 + d2)?;
        cl.freeze(d2, 1)?;
        cl.freeze(d2, 2)?;
        d2 += 1;
    }
    let n = cl.assert_no_leak()?;
    if n != 80 {
        return Err(E_NOT_CALIB);
    }
    cl.assert_all_frozen()?;
    cl.assert_dom_cover()?;
    hits += 1;

    // R8 冻结后篡改必抓
    if cl.calibrate(1, 5, 0x99).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R9 终版导出计数守恒 + 逐条校准值对拍
    let ex = cl.export()?;
    if ex.len() != 80 {
        return Err(E_EXPORT_BREAK);
    }
    let mut d3 = 1u32;
    while d3 <= DOM_N {
        if ex[(d3 - 1) as usize * 2].cal != 0x1000 + d3 {
            return Err(E_CAL_RANGE);
        }
        if ex[(d3 - 1) as usize * 2 + 1].cal != 0x2000 + d3 {
            return Err(E_CAL_RANGE);
        }
        d3 += 1;
    }
    hits += 1;

    // R10 LCG 随机抽查 ×256：导出表校准值域恒在界内
    let mut rng = Lcg::new(0x48727);
    let mut k = 0u32;
    while k < 256 {
        let pick = rng.below(80) as usize;
        let it = &ex[pick];
        if it.cal == 0 || it.cal > CAL_MAX {
            return Err(E_CAL_RANGE);
        }
        if it.state != CalState::Frozen as u8 {
            return Err(E_STATE_BAD);
        }
        k += 1;
    }
    hits += 1;

    Ok(hits)
}
