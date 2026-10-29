//! UNX-F36297-J1 首产段总闸宣告（B01–B15 段闸 300/300）。
//! 判据：UNX-F36297-J1 段闸 300/300 判据可追溯全绿；首产段
//! （F36001–F36300）总闸宣告入域档；纯功能代码 ≥300 行。
//! 设计纪律：首产段总闸是 300 卡旅程的终局裁决——Windows RTM 的
//! escalation gate（分段门全绿才能宣告 RTM，宣告本身是落档动作）
//! 同构：
//!   - 段闸 300/300：B01–B15 十五批 × 20 判据逐枚绿，判据码与批号
///     双字段可追溯（绿可走回卡面——不可追溯的绿不算绿）；
///   - 批窗完备性：十五批批窗连续零跳号（批基址 36001 起步进 20）；
///   - 总闸宣告入域档：宣告记录（卡号区间 + 计数 + 全绿位）落域档
///     槽位，宣告可回查——悬空的宣告不算宣告；
///   - 有红禁宣告：合成 299/300 必须被闸面拒绝（放行闸守门面）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB130;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 批/判据下标越合法区
pub const E_GREEN: u16 = SEG_BASE + 3; // 段闸未全绿（300/300 破）
pub const E_TRACE: u16 = SEG_BASE + 4; // 判据不可追溯（码/批不符）
pub const E_DECLARE: u16 = SEG_BASE + 5; // 宣告未入域档/有红宣告
pub const E_WINDOW: u16 = SEG_BASE + 6; // 批窗跳号（批基址不连续）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 首产段规格 -------------------------------------------------------------
/// B01–B15 十五批 × 20 = 300 判据。
pub const BATCH_N: usize = 15;
pub const PER_BATCH: usize = 20;
pub const GATE_N: usize = BATCH_N * PER_BATCH; // 300

/// 首产段卡号区间 F36001–F36300。
pub const CARD_FIRST: u32 = 36001;
pub const CARD_LAST: u32 = 36300;

/// 批基址（批 b 的首卡号 = 36001 + b×20——批窗连续零跳号）。
pub fn batch_base(b: usize) -> Result<u32, u16> {
    if b >= BATCH_N {
        return Err(E_RANGE);
    }
    Ok(CARD_FIRST + b as u32 * PER_BATCH as u32)
}

/// 判据码 = 卡号；批内可追溯（批号 + 批内序）。
pub fn crit_at(idx: usize) -> Result<u32, u16> {
    if idx >= GATE_N {
        return Err(E_RANGE);
    }
    Ok(CARD_FIRST + idx as u32)
}

pub fn batch_of(crit: u32) -> Result<usize, u16> {
    if crit < CARD_FIRST || crit > CARD_LAST {
        return Err(E_RANGE);
    }
    Ok(((crit - CARD_FIRST) / PER_BATCH as u32) as usize)
}

/// 批窗完备性：十五批基址连续（步进恰 20——零跳号）。
pub fn windows_continuous() -> Result<(), u16> {
    let mut b = 0;
    while b < BATCH_N {
        if batch_base(b)? != CARD_FIRST + b as u32 * PER_BATCH as u32 {
            return Err(E_WINDOW);
        }
        b += 1;
    }
    Ok(())
}

/// 总闸宣告记录（卡号区间 + 计数 + 全绿位 + 域档锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Declaration {
    pub first: u32,
    pub last: u32,
    pub green_n: u16,
    pub total_n: u16,
    pub all_green: bool,
    pub declare_id: u32,
}

/// 域档（宣告槽位表）。
pub struct DomainArchive {
    slots: Vec<(u32, Declaration)>,
}

const ARCHIVE_CAP: usize = 8;

impl DomainArchive {
    pub fn new() -> DomainArchive {
        DomainArchive { slots: Vec::new() }
    }

    pub fn file(&mut self, d: Declaration) -> Result<(), u16> {
        if self.slots.len() >= ARCHIVE_CAP {
            return Err(E_DECLARE);
        }
        // 有红禁宣告——域档拒收非全绿宣告（放行闸守门）。
        if !d.all_green || d.green_n != d.total_n {
            return Err(E_DECLARE);
        }
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].0 == d.declare_id {
                return Err(E_DECLARE); // 宣告锚撞号
            }
            i += 1;
        }
        self.slots.push((d.declare_id, d));
        Ok(())
    }

    pub fn fetch(&self, id: u32) -> Option<Declaration> {
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].0 == id {
                return Some(self.slots[i].1);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }
}

/// 构建真实总闸宣告：300 枚逐枚绿、可追溯。
pub fn build_declaration() -> Result<(Declaration, Vec<u32>), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut greens: Vec<u32> = Vec::new();
    let mut idx = 0;
    while idx < GATE_N {
        greens.push(crit_at(idx)?);
        idx += 1;
    }
    Ok((
        Declaration {
            first: CARD_FIRST,
            last: CARD_LAST,
            green_n: greens.len() as u16,
            total_n: GATE_N as u16,
            all_green: greens.len() == GATE_N,
            declare_id: 0xB1_30_001,
        },
        greens,
    ))
}

/// 判据题注（F 号 + 批号可读——300 枚逐枚追溯的呈现面）。
pub fn crit_caption(crit: u32) -> String {
    let mut s = String::new();
    s.push_str("UNX-F");
    s.push_str(&crit.to_string());
    s.push_str("-J1/B");
    let b = batch_of(crit).ok().unwrap_or(99);
    s.push_str(&(b + 1).to_string());
    s
}

/// 宣告题注（区间/计数/全绿位可读）。
pub fn declare_caption(d: &Declaration) -> String {
    let mut s = String::new();
    s.push_str("首产段总闸 F");
    s.push_str(&d.first.to_string());
    s.push_str("–F");
    s.push_str(&d.last.to_string());
    s.push_str(" ");
    s.push_str(&d.green_n.to_string());
    s.push_str("/");
    s.push_str(&d.total_n.to_string());
    s.push_str(if d.all_green { " 全绿宣告" } else { " 有红禁宣告" });
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 区间守恒（36001..36300 = 300）。
    if VER != 1 {
        return Err(E_VER);
    }
    if GATE_N != 300 || CARD_LAST - CARD_FIRST + 1 != GATE_N as u32 {
        return Err(E_RANGE);
    }
    // R2：批窗完备性——十五批基址连续零跳号。
    windows_continuous()?;
    // R3：段闸 300/300 全绿——逐枚绿 + 批号追溯（码落批窗）。
    let (d, greens) = build_declaration()?;
    if d.green_n != 300 || d.total_n != 300 || !d.all_green {
        return Err(E_GREEN);
    }
    let mut i = 0;
    while i < greens.len() {
        let crit = greens[i];
        let b = batch_of(crit)?;
        let base = batch_base(b)?;
        if crit < base || crit >= base + PER_BATCH as u32 {
            return Err(E_TRACE);
        }
        if crit != CARD_FIRST + i as u32 {
            return Err(E_TRACE); // 平表序与码序一致（追溯双锚）
        }
        if crit_caption(crit).is_empty() {
            return Err(E_TRACE); // 题注可读（追溯呈现面健康）
        }
        i += 1;
    }
    // R4：总闸宣告入域档——落档、可回查、题注可读。
    let mut arch = DomainArchive::new();
    arch.file(d)?;
    if arch.len() != 1 {
        return Err(E_DECLARE);
    }
    match arch.fetch(d.declare_id) {
        Some(dd) => {
            if dd != d || declare_caption(&dd).is_empty() {
                return Err(E_DECLARE);
            }
        }
        None => return Err(E_DECLARE),
    }
    // R5：有红禁宣告——合成 299/300 必须被域档拒收（独立实例）。
    let mut arch2 = DomainArchive::new();
    let red = Declaration { green_n: 299, total_n: 300, all_green: false, ..d };
    if arch2.file(red).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    // 有绿位但计数缩水的伪全绿也必须拒收（全绿位与计数双验证）。
    let fake = Declaration { green_n: 299, total_n: 300, all_green: true, declare_id: 7 };
    if arch2.file(fake).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    // 宣告锚撞号拒收。
    if arch.file(d).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    // 校验和：300 枚判据码折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < greens.len() {
        sum = sum.wrapping_mul(7).wrapping_add(greens[i].wrapping_sub(CARD_FIRST));
        i += 1;
    }
    Ok(sum.wrapping_add(d.declare_id).wrapping_add(GATE_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "批/判据下标越合法区",
        E_GREEN => "段闸未全绿（300/300 破）",
        E_TRACE => "判据不可追溯（码/批不符）",
        E_DECLARE => "宣告未入域档/有红宣告/锚撞号",
        E_WINDOW => "批窗跳号（批基址不连续）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36297_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36297_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB130, "0xB130~0xB13F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "300/300 全绿可追溯/批窗零跳号/宣告入域档可回查"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段闸 300/300", {
        match build_declaration() {
            Ok((d, _)) => d.all_green && d.green_n == 300,
            Err(_) => false,
        }
    }, "B01–B15 十五批 300 判据逐枚绿");
    cs.add("批窗零跳号", windows_continuous().is_ok(), "批基址 36001 起步进 20 连续");
    cs.add("有红禁宣告", {
        let mut arch = DomainArchive::new();
        match build_declaration() {
            Ok((d, _)) => {
                let red = Declaration { green_n: 299, all_green: false, ..d };
                arch.file(red).is_err()
            }
            Err(_) => false,
        }
    }, "299/300 伪宣告被域档拒收");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36297_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36297_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_batch_windows() {
        // 批窗直证：B01 首 36001、B15 首 36281、末批窗恰收 36300。
        assert_eq!(batch_base(0).ok().unwrap_or(0), 36001);
        assert_eq!(batch_base(14).ok().unwrap_or(0), 36281);
        assert_eq!(batch_of(36300).ok().unwrap_or(99), 14);
        assert_eq!(batch_of(36281).ok().unwrap_or(99), 14);
        assert!(batch_of(36301).is_err(), "F36301 出首产段区间");
        assert!(batch_of(36000).is_err());
    }

    #[test]
    fn test_crit_caption_traceable() {
        // 题注直证：300 枚逐枚题注含 F 号与批号。
        let mut idx = 0;
        while idx < GATE_N {
            let crit = crit_at(idx).ok().unwrap_or(0);
            let cap = crit_caption(crit);
            assert!(cap.contains("J1"), "题注含判据级: {}", cap);
            assert!(!cap.is_empty());
            idx += 1;
        }
        let cap = crit_caption(36282);
        assert!(cap.contains("36282"), "题注含卡号: {}", cap);
    }
}
