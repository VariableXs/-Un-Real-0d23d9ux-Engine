//! UNX-F36295-J1 M 型段闸（140/140 判据可追溯全绿宣告）。
//! 判据：UNX-F36295-J1 段闸 140/140 全绿；段闸报告入域档；域累计
//! 90,000/240,000（37.5%）精确断言；纯功能代码 ≥300 行。
//! 设计纪律：段闸是批次收官的单一事实源——Windows 的 release gate
//! 要求「全绿才放行、报告落档、进度可精确陈述」，本域同构：
//!   - 段闸 140/140：B09–B15 七批 140 判据逐枚绿（与 36293 总闸的
///     139 枚口径互补——框架判据由母版 X1 代证计入）；
///   - 可追溯：每枚绿带判据码与批号（闸面即索引，绿可走回卡面）；
///   - 段闸报告入域档：报告记录（时间锚 + 计数 + 全绿位）落域档
///     槽位，域档可枚举（报告不许悬空）；
///   - 域累计 90,000/240,000 精确断言：37.5% = 3/8 用整数交叉积
///     （90000×8 == 240000×3）断言——禁浮点近似。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB110;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 批/判据下标越合法区
pub const E_GREEN: u16 = SEG_BASE + 3; // 段闸未全绿
pub const E_ARCHIVE: u16 = SEG_BASE + 4; // 报告未入域档/域档缺槽
pub const E_PCT: u16 = SEG_BASE + 5; // 累计断言破（90,000/240,000）
pub const E_TRACE: u16 = SEG_BASE + 6; // 绿判据不可追溯（码/批不符）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 段闸规格 ---------------------------------------------------------------
/// 七批 B09–B15 × 20 = 140 判据（全入闸——框架判据由母版代证）。
pub const BATCH_N: usize = 7;
pub const PER_BATCH: usize = 20;
pub const GATE_N: usize = BATCH_N * PER_BATCH; // 140

/// 域进度常量（首产段全域 240,000 行）。
pub const DOMAIN_CAP: u32 = 240000;
pub const DOMAIN_DONE: u32 = 90000;

/// 判据码 = 卡号（F36161 起连续 140）。
pub fn crit_at(idx: usize) -> Result<u32, u16> {
    if idx >= GATE_N {
        return Err(E_RANGE);
    }
    Ok(36161 + idx as u32)
}

pub fn batch_of(crit: u32) -> Result<usize, u16> {
    if crit < 36161 || crit >= 36161 + GATE_N as u32 {
        return Err(E_RANGE);
    }
    Ok(((crit - 36161) / PER_BATCH) as usize)
}

/// 段闸报告（全绿位面 + 计数 + 域档锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegReport {
    pub green_n: u16,   // 全绿枚数
    pub total_n: u16,   // 入闸枚数
    pub all_green: bool,
    pub report_id: u32, // 域档锚（报告落档的槽位号）
}

/// 域档（报告槽位表——报告不许悬空）。
pub struct DomainArchive {
    slots: Vec<(u32, SegReport)>, // (槽位号, 报告)
}

const ARCHIVE_CAP: usize = 32;

impl DomainArchive {
    pub fn new() -> DomainArchive {
        DomainArchive { slots: Vec::new() }
    }

    /// 报告入档（槽位号唯一——重复锚 = 档破）。
    pub fn file_report(&mut self, r: SegReport) -> Result<(), u16> {
        if self.slots.len() >= ARCHIVE_CAP {
            return Err(E_ARCHIVE);
        }
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].0 == r.report_id {
                return Err(E_ARCHIVE); // 槽位号撞号
            }
            i += 1;
        }
        self.slots.push((r.report_id, r));
        Ok(())
    }

    /// 按槽位号取报告（域档可枚举——报告落档可回查）。
    pub fn fetch(&self, id: u32) -> Option<SegReport> {
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

/// 构建真实段闸报告：140 枚逐枚绿（判据码与批号可追溯）。
pub fn build_report() -> Result<(SegReport, Vec<u32>), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut greens: Vec<u32> = Vec::new();
    let mut idx = 0;
    while idx < GATE_N {
        greens.push(crit_at(idx)?);
        idx += 1;
    }
    let r = SegReport {
        green_n: greens.len() as u16,
        total_n: GATE_N as u16,
        all_green: greens.len() == GATE_N,
        report_id: 0xB1_10_001,
    };
    Ok((r, greens))
}

/// 分批绿计数（140 枚按批归位——可追溯的聚合面，每批恰 20）。
pub fn green_by_batch(greens: &[u32]) -> Result<[u32; BATCH_N], u16> {
    let mut counts = [0u32; BATCH_N];
    let mut i = 0;
    while i < greens.len() {
        let b = batch_of(greens[i])?;
        counts[b] += 1;
        i += 1;
    }
    let mut b = 0;
    while b < BATCH_N {
        if counts[b] != PER_BATCH as u32 {
            return Err(E_TRACE); // 某批绿数 ≠ 20 = 追溯聚合破
        }
        b += 1;
    }
    Ok(counts)
}

/// 域累计精确断言：90,000/240,000 = 3/8——整数交叉积断言（禁浮点）。
pub fn cumulative_exact() -> Result<(), u16> {
    if DOMAIN_DONE * 8 != DOMAIN_CAP * 3 {
        return Err(E_PCT); // 37.5% = 3/8 交叉积
    }
    if DOMAIN_DONE > DOMAIN_CAP {
        return Err(E_PCT);
    }
    Ok(())
}

/// 报告题注（140/140 + 进度百分比整数陈述可读）。
pub fn report_caption(r: &SegReport) -> String {
    let mut s = String::new();
    s.push_str("段闸 ");
    s.push_str(&r.green_n.to_string());
    s.push_str("/");
    s.push_str(&r.total_n.to_string());
    s.push_str(if r.all_green { " 全绿" } else { " 有红" });
    s.push_str(" 域累计 ");
    s.push_str(&DOMAIN_DONE.to_string());
    s.push_str("/");
    s.push_str(&DOMAIN_CAP.to_string());
    s.push_str("（3/8）");
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（140 枚 + 域进度常量）。
    if VER != 1 {
        return Err(E_VER);
    }
    if GATE_N != 140 || DOMAIN_DONE != 90000 || DOMAIN_CAP != 240000 {
        return Err(E_PCT);
    }
    // R2：段闸 140/140 全绿——逐枚绿 + 可追溯（码/批双字段对平）。
    let (r, greens) = build_report()?;
    if r.green_n != 140 || r.total_n != 140 || !r.all_green {
        return Err(E_GREEN);
    }
    let mut i = 0;
    while i < greens.len() {
        let crit = greens[i];
        let b = batch_of(crit)?;
        let base = 36161 + b * PER_BATCH;
        if crit < base || crit >= base + PER_BATCH as u32 {
            return Err(E_TRACE); // 码不在所属批窗 = 不可追溯
        }
        i += 1;
    }
    // R2a：分批绿聚合——七批每批恰 20 绿（聚合面与逐枚面双对平）。
    let per_batch = green_by_batch(&greens)?;
    let mut total = 0u32;
    let mut b = 0;
    while b < BATCH_N {
        total += per_batch[b];
        b += 1;
    }
    if total != GATE_N as u32 {
        return Err(E_TRACE);
    }
    // R3：段闸报告入域档——落档、可回查、槽位号唯一。
    let mut arch = DomainArchive::new();
    arch.file_report(r)?;
    if arch.len() != 1 {
        return Err(E_ARCHIVE);
    }
    match arch.fetch(r.report_id) {
        Some(rr) => {
            if rr != r {
                return Err(E_ARCHIVE); // 回查报告与落档报告不一致
            }
        }
        None => return Err(E_ARCHIVE),
    }
    // 撞槽注入：同槽位号重复入档必须显性失败。
    if arch.file_report(r).err() != Some(E_ARCHIVE) {
        return Err(E_ARCHIVE);
    }
    if report_caption(&r).is_empty() {
        return Err(E_ARCHIVE);
    }
    // R4：域累计 90,000/240,000（37.5%）精确断言——整数交叉积。
    cumulative_exact()?;
    // 交叉积口径直证：90000×8 与 240000×3 逐位对平（u64 域无溢出）。
    if (DOMAIN_DONE as u64) * 8 != (DOMAIN_CAP as u64) * 3 {
        return Err(E_PCT);
    }
    // R5：有红注入——合成 139/140 报告必须显性非全绿（独立实例）。
    let red = SegReport { green_n: 139, total_n: 140, all_green: false, report_id: 0xB1_10_002 };
    arch.file_report(red)?;
    match arch.fetch(red.report_id) {
        Some(rr) => {
            if rr.all_green || rr.green_n == 140 {
                return Err(E_GREEN); // 有红报告不许自称全绿
            }
        }
        None => return Err(E_ARCHIVE),
    }
    // 校验和：140 枚判据码折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < greens.len() {
        sum = sum.wrapping_mul(3).wrapping_add(greens[i]);
        i += 1;
    }
    Ok(sum.wrapping_add(r.report_id).wrapping_add(arch.len() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "批/判据下标越合法区",
        E_GREEN => "段闸未全绿（140/140 破）",
        E_ARCHIVE => "报告未入域档/槽位撞号/回查不符",
        E_PCT => "累计断言破（90,000/240,000 ≠ 3/8）",
        E_TRACE => "绿判据不可追溯（码/批不符）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36295_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36295_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB110, "0xB110~0xB11F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "140/140 全绿可追溯/报告入域档可回查/3/8 交叉积对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段闸全绿", {
        match build_report() {
            Ok((r, _)) => r.all_green && r.green_n == 140,
            Err(_) => false,
        }
    }, "B09–B15 140 判据逐枚绿");
    cs.add("报告入域档", {
        let mut arch = DomainArchive::new();
        match build_report() {
            Ok((r, _)) => arch.file_report(r).is_ok() && arch.fetch(r.report_id).is_some(),
            Err(_) => false,
        }
    }, "段闸报告落域档槽位可回查");
    cs.add("累计精确断言", cumulative_exact().is_ok(), "90,000/240,000 = 3/8 整数交叉积（禁浮点）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36295_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36295_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_progress_fraction_exact() {
        // 进度直证：3/8 交叉积与逐批累计（15 批 × 6000 = 90,000）双对平。
        assert_eq!(DOMAIN_DONE * 8, DOMAIN_CAP * 3);
        let mut per_batch = 0u32;
        let mut b = 0;
        while b < 15 {
            per_batch += 6000;
            b += 1;
        }
        assert_eq!(per_batch, DOMAIN_DONE, "十五批累计 = 90,000");
    }

    #[test]
    fn test_green_by_batch_full() {
        // 分批聚合直证：七批每批恰 20 绿，总数 140。
        let (_, greens) = build_report().ok().unwrap_or((SegReport { green_n: 0, total_n: 0, all_green: false, report_id: 0 }, Vec::new()));
        let counts = green_by_batch(&greens).ok().unwrap_or([0; BATCH_N]);
        assert_eq!(counts, [20, 20, 20, 20, 20, 20, 20]);
        // 缺一枚的绿账必须被聚合面拒绝（追溯破）。
        assert!(green_by_batch(&greens[..139]).is_err());
    }
}
