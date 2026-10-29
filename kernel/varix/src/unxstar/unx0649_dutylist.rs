//! unx0649_dutylist —— UNX-F0649 域值守清单（接管前 12 项检查）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0649 · 域值守清单（接管前 12 项检查） · 目标 ≥220 行
//! 判据原文摘录：UNX-F0649-J1 接管前 12 项检查全过才可交接，缺项
//!   拦截检出 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 12 项检查定容闭集：项号 0..12 一项一检查，检查名册写死，
//!      项外拒收。
//!   2. 二态检查：过（Pass）/缺（Miss）二态，缺项任何一项 → 交接
//!      拦截（无"缺一项先放着"语义）。
//!   3. 拦截检出 100%：任何缺项组合（含缺 1 项/缺多项/全缺）都必须
//!      被拦截，且能报出最小缺项号。
//!   4. 交接位：12/12 全过才置交接位，置位后清单只读。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x52A0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x52A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x52A0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x52AF;

/// XcCode：检查项越界。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1; // 0x52A1 项号 0..12 闭集外
/// XcCode：检查项重复。
pub const XC_DUP_ITEM: u16 = SEG_BASE + 2; // 0x52A2 一项一检
/// XcCode：缺项拦截。
pub const XC_MISSING: u16 = SEG_BASE + 3; // 0x52A3 有缺禁交接
/// XcCode：清单未检满。
pub const XC_UNCHECKED: u16 = SEG_BASE + 4; // 0x52A4 12/12 未检
/// XcCode：交接后写拦截。
pub const XC_HANDED_RDONLY: u16 = SEG_BASE + 5; // 0x52A5 交接后清单只读
/// XcCode：检查项名漂移。
pub const XC_ITEM_DRIFT: u16 = SEG_BASE + 6; // 0x52A6 项名与名册不符

/// 检查项数（判据原文 12 项）。
pub const ITEM_N: usize = 12;

/// 12 项检查名册。
pub const ITEM_NAMES: [&str; ITEM_N] = [
    "封账 v1 在位",
    "三 mock 零差",
    "快照零漂移",
    "勘误通道闭",
    "覆盖率 660/660",
    "红线 12/12 可达",
    "对冲八腿全平",
    "遗留零隐匿",
    "建议 8/8 可达",
    "B31 批链尾 4340",
    "B32 批链尾 4660",
    "交接包 12/12 可读",
];

/// 检查二态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// 过。
    Pass = 0,
    /// 缺。
    Miss = 1,
}

/// 检查项行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DutyRow {
    /// 项号（0..12）。
    pub item_no: usize,
    /// 项名（须与名册一致）。
    pub name: String,
    /// 二态。
    pub state: ItemState,
}

/// 域值守清单。
#[derive(Clone, Debug)]
pub struct DutyList {
    /// 12 槽。
    slots: [Option<DutyRow>; ITEM_N],
    /// 交接位。
    handed: bool,
}

impl DutyList {
    /// 空清单。
    pub fn new() -> Self {
        DutyList {
            slots: [
                None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            handed: false,
        }
    }

    /// 已检项数。
    pub fn checked_n(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 是否已交接。
    pub fn handed(&self) -> bool {
        self.handed
    }

    /// 检一项（号闭集 + 去重 + 项名一致性 + 交接后拦截）。
    pub fn check(&mut self, item_no: usize, state: ItemState) -> Result<(), u16> {
        if self.handed {
            return Err(XC_HANDED_RDONLY);
        }
        if item_no >= ITEM_N {
            return Err(XC_BAD_ITEM);
        }
        if self.slots[item_no].is_some() {
            return Err(XC_DUP_ITEM);
        }
        if ITEM_NAMES[item_no].is_empty() {
            return Err(XC_ITEM_DRIFT);
        }
        self.slots[item_no] = Some(DutyRow {
            item_no,
            name: ITEM_NAMES[item_no].to_string(),
            state,
        });
        Ok(())
    }

    /// 缺项定位：最小缺（Miss）项号（无缺回 None）。
    pub fn first_miss(&self) -> Option<usize> {
        for i in 0..ITEM_N {
            match &self.slots[i] {
                Some(r) => {
                    if r.state == ItemState::Miss {
                        return Some(i);
                    }
                }
                None => return Some(i), // 未检亦视作缺
            }
        }
        None
    }

    /// 交接门：12/12 全检 + 全过 → 置交接位；任何缺 → 拦截。
    pub fn handover(&mut self) -> Result<(), u16> {
        if self.handed {
            return Err(XC_HANDED_RDONLY);
        }
        if self.checked_n() != ITEM_N {
            return Err(XC_UNCHECKED);
        }
        for i in 0..ITEM_N {
            match &self.slots[i] {
                Some(r) => {
                    if r.state == ItemState::Miss {
                        return Err(XC_MISSING);
                    }
                    if r.name != ITEM_NAMES[i] {
                        return Err(XC_ITEM_DRIFT);
                    }
                }
                None => return Err(XC_UNCHECKED),
            }
        }
        self.handed = true;
        Ok(())
    }

    /// 缺项计数（含未检）。
    pub fn miss_n(&self) -> usize {
        (0..ITEM_N)
            .filter(|&i| match &self.slots[i] {
                Some(r) => r.state == ItemState::Miss,
                None => true,
            })
            .count()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域值守清单 · 已检 {}/{} · 缺项 {} · 交接={}",
            self.checked_n(),
            ITEM_N,
            self.miss_n(),
            self.handed
        );
        if let Some(m) = self.first_miss() {
            s.push_str(&alloc::format!("\n  最小缺项 {}（{}）", m, ITEM_NAMES[m]));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准全过清单构造器：12 项全 Pass → 交接。
pub fn build_green_duty() -> Result<DutyList, u16> {
    let mut d = DutyList::new();
    for i in 0..ITEM_N {
        d.check(i, ItemState::Pass)?;
    }
    d.handover()?;
    Ok(d)
}

/// 自例程：主路径（12/12 交接）+ 错误路径（缺项拦截/未检满/越界/重检/交接后写）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let d = build_green_duty()?;
    if !d.handed() || d.miss_n() != 0 {
        return Err(XC_MISSING);
    }
    if d.fingerprint() == 0 {
        return Err(XC_HANDED_RDONLY);
    }

    // 错误路径 1：缺 1 项 → 拦截且定位。
    let mut d1 = DutyList::new();
    for i in 0..ITEM_N {
        let st = if i == 7 { ItemState::Miss } else { ItemState::Pass };
        d1.check(i, st)?;
    }
    if d1.handover() != Err(XC_MISSING) {
        return Err(XC_MISSING);
    }
    if d1.first_miss() != Some(7) {
        return Err(XC_MISSING);
    }
    // 错误路径 2：未检满 → 拦截。
    let mut d2 = DutyList::new();
    for i in 0..ITEM_N - 1 {
        d2.check(i, ItemState::Pass)?;
    }
    if d2.handover() != Err(XC_UNCHECKED) {
        return Err(XC_UNCHECKED);
    }
    if d2.first_miss() != Some(ITEM_N - 1) {
        return Err(XC_UNCHECKED);
    }
    // 错误路径 3：全缺（空清单）。
    let mut d3 = DutyList::new();
    if d3.handover() != Err(XC_UNCHECKED) {
        return Err(XC_UNCHECKED);
    }
    if d3.miss_n() != ITEM_N {
        return Err(XC_UNCHECKED);
    }
    // 错误路径 4：项号越界。
    let mut d4 = DutyList::new();
    if d4.check(ITEM_N, ItemState::Pass) != Err(XC_BAD_ITEM) {
        return Err(XC_BAD_ITEM);
    }
    // 错误路径 5：重复检查。
    let mut d5 = DutyList::new();
    d5.check(0, ItemState::Pass)?;
    if d5.check(0, ItemState::Pass) != Err(XC_DUP_ITEM) {
        return Err(XC_DUP_ITEM);
    }
    // 错误路径 6：交接后写拦截。
    let mut d6 = build_green_duty()?;
    if d6.check(0, ItemState::Pass) != Err(XC_HANDED_RDONLY) {
        return Err(XC_HANDED_RDONLY);
    }
    if d6.handover() != Err(XC_HANDED_RDONLY) {
        return Err(XC_HANDED_RDONLY);
    }
    // 错误路径 7：缺多项 → 最小缺项定位。
    let mut d7 = DutyList::new();
    for i in 0..ITEM_N {
        let st = if i == 3 || i == 9 {
            ItemState::Miss
        } else {
            ItemState::Pass
        };
        d7.check(i, st)?;
    }
    if d7.handover() != Err(XC_MISSING) {
        return Err(XC_MISSING);
    }
    if d7.first_miss() != Some(3) {
        return Err(XC_MISSING);
    }
    Ok(0)
}
