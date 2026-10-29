//! unx0634_advice —— UNX-F0634 下域建议账（A2 接收注记）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0634 · 下域建议账（A2 接收注记） · 目标 ≥200 行
//! 判据原文摘录：建议账齐（A2 接收注记 8 条——消费字段/时点/红线
//!   提示三列），可达断言 8/8。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 八条注记定容账：一条注记三列——消费字段名（非空）/ 消费时点
//!      （枚举：引导期/运行期/关机期）/ 红线提示号（0=无红线，非 0
//!      须指向域红线 0..12）。
//!   2. 可达断言：一条注记"可达" = 字段名非空 + 时点在闭集 + 红线
//!      提示号在闭集（或 0），8/8 全达才账齐。
//!   3. 注记号连续零跳号；重号拒收；封账后只读。
//!   4. 注入红检出：任一列被清空或越闭集，可达断言必红并定位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5100 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5100 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5100;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x510F;

/// XcCode：注记号越界。
pub const XC_BAD_NO: u16 = SEG_BASE + 1; // 0x5101 注记号 0..8 闭集外
/// XcCode：注记重复。
pub const XC_DUP_ADVICE: u16 = SEG_BASE + 2; // 0x5102 一号一注记
/// XcCode：字段名空。
pub const XC_EMPTY_FIELD: u16 = SEG_BASE + 3; // 0x5103 消费字段列空
/// XcCode：时点闭集外。
pub const XC_BAD_TIMING: u16 = SEG_BASE + 4; // 0x5104 三时点之外
/// XcCode：红线提示号越界。
pub const XC_BAD_HINT: u16 = SEG_BASE + 5; // 0x5105 提示号 >12 且 ≠0
/// XcCode：可达断言失败。
pub const XC_REACH_FAIL: u16 = SEG_BASE + 6; // 0x5106 8/8 未全达

/// 注记条数（判据原文 8 条）。
pub const ADVICE_N: usize = 8;

/// 消费时点（三时点闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    /// 引导期消费。
    Boot = 0,
    /// 运行期消费。
    Run = 1,
    /// 关机期消费。
    Shutdown = 2,
}

impl Timing {
    /// 时点名。
    pub fn name(self) -> &'static str {
        match self {
            Timing::Boot => "引导期",
            Timing::Run => "运行期",
            Timing::Shutdown => "关机期",
        }
    }
}

/// 单条 A2 接收注记（三列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdviceRow {
    /// 注记号（0..8）。
    pub adv_no: usize,
    /// 列一：消费字段名。
    pub field: String,
    /// 列二：消费时点。
    pub timing: Timing,
    /// 列三：红线提示号（0 = 无红线关联；1..12 = 域红线号）。
    pub hint: u16,
}

impl AdviceRow {
    /// 可达判式：三列全合法。
    pub fn reachable(&self) -> bool {
        !self.field.is_empty()
            && (self.timing as usize) <= Timing::Shutdown as usize
            && (self.hint == 0 || self.hint <= 12)
    }
}

/// 下域建议账本。
#[derive(Clone, Debug)]
pub struct AdviceBook {
    /// 八条注记槽。
    slots: [Option<AdviceRow>; ADVICE_N],
    /// 封单位。
    sealed: bool,
}

impl AdviceBook {
    /// 空账本。
    pub fn new() -> Self {
        AdviceBook {
            slots: [None, None, None, None, None, None, None, None],
            sealed: false,
        }
    }

    /// 已落注记数。
    pub fn filled(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 落一条注记（号闭集 + 去重 + 封后拒写 + 列面即时校验）。
    pub fn put(&mut self, adv_no: usize, field: &str, timing: Timing, hint: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_REACH_FAIL);
        }
        if adv_no >= ADVICE_N {
            return Err(XC_BAD_NO);
        }
        if self.slots[adv_no].is_some() {
            return Err(XC_DUP_ADVICE);
        }
        if field.is_empty() {
            return Err(XC_EMPTY_FIELD);
        }
        if timing as usize > Timing::Shutdown as usize {
            return Err(XC_BAD_TIMING);
        }
        if hint > 12 {
            return Err(XC_BAD_HINT);
        }
        self.slots[adv_no] = Some(AdviceRow {
            adv_no,
            field: field.to_string(),
            timing,
            hint,
        });
        Ok(())
    }

    /// 按号取注记。
    pub fn at(&self, adv_no: usize) -> Option<&AdviceRow> {
        if adv_no >= ADVICE_N {
            return None;
        }
        self.slots[adv_no].as_ref()
    }

    /// 可达断言：8/8 全落且逐条可达。
    pub fn all_reachable(&self) -> Result<usize, u16> {
        if self.filled() != ADVICE_N {
            return Err(XC_REACH_FAIL);
        }
        for i in 0..ADVICE_N {
            match &self.slots[i] {
                Some(r) => {
                    if !r.reachable() {
                        return Err(XC_REACH_FAIL);
                    }
                }
                None => return Err(XC_REACH_FAIL),
            }
        }
        Ok(ADVICE_N)
    }

    /// 封账：全达才可封；封后只读。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_REACH_FAIL);
        }
        let n = self.all_reachable()?;
        self.sealed = true;
        Ok(n)
    }

    /// 时点分布（三时点计数）。
    pub fn timing_hist(&self) -> [usize; 3] {
        let mut h = [0usize; 3];
        for r in self.slots.iter().flatten() {
            h[r.timing as usize] += 1;
        }
        h
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let h = self.timing_hist();
        let mut s = alloc::format!(
            "下域建议账（A2 接收注记）· 落注记 {}/{} · 封={} · 时点 [{},{},{}]",
            self.filled(),
            ADVICE_N,
            self.sealed,
            h[0],
            h[1],
            h[2]
        );
        for i in 0..ADVICE_N {
            match &self.slots[i] {
                Some(r) => s.push_str(&alloc::format!(
                    "\n  注记{} {} @{} 红线提示{}",
                    r.adv_no, r.field, r.timing.name(), r.hint
                )),
                None => s.push_str(&alloc::format!("\n  注记{} 缺", i)),
            }
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

/// 标准建议账构造器：八条注记（字段名/时点/提示号确定性名册）。
pub fn build_standard() -> Result<AdviceBook, u16> {
    let mut b = AdviceBook::new();
    let fields = [
        "tsc_freq_khz",
        "e820_rows",
        "irq_routes",
        "acpi_csum",
        "pci_csum",
        "fb_mode",
        "log_ring_cap",
        "initrd_pkg_n",
    ];
    let timings = [
        Timing::Boot,
        Timing::Boot,
        Timing::Boot,
        Timing::Boot,
        Timing::Run,
        Timing::Run,
        Timing::Run,
        Timing::Boot,
    ];
    let hints = [1, 2, 3, 4, 5, 6, 9, 11];
    for i in 0..ADVICE_N {
        b.put(i, fields[i], timings[i], hints[i])?;
    }
    b.seal()?;
    Ok(b)
}

/// 自例程：主路径（8/8 可达→封账）+ 错误路径（越界/重号/空字段/坏提示/封后写）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let b = build_standard()?;
    if b.all_reachable()? != ADVICE_N {
        return Err(XC_REACH_FAIL);
    }
    let h = b.timing_hist();
    if h != [5, 3, 0] {
        return Err(XC_BAD_TIMING);
    }
    if b.fingerprint() == 0 {
        return Err(XC_REACH_FAIL);
    }

    // 错误路径 1：注记号越界。
    let mut b1 = AdviceBook::new();
    if b1.put(ADVICE_N, "f", Timing::Boot, 0) != Err(XC_BAD_NO) {
        return Err(XC_BAD_NO);
    }
    // 错误路径 2：空字段。
    let mut b2 = AdviceBook::new();
    if b2.put(0, "", Timing::Boot, 0) != Err(XC_EMPTY_FIELD) {
        return Err(XC_EMPTY_FIELD);
    }
    // 错误路径 3：红线提示号越界。
    let mut b3 = AdviceBook::new();
    if b3.put(0, "f", Timing::Boot, 13) != Err(XC_BAD_HINT) {
        return Err(XC_BAD_HINT);
    }
    // 错误路径 4：重号。
    let mut b4 = AdviceBook::new();
    b4.put(0, "f1", Timing::Boot, 0)?;
    if b4.put(0, "f2", Timing::Run, 1) != Err(XC_DUP_ADVICE) {
        return Err(XC_DUP_ADVICE);
    }
    // 错误路径 5：缺注记禁封。
    let mut b5 = AdviceBook::new();
    b5.put(0, "f", Timing::Boot, 0)?;
    if b5.seal() != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    // 错误路径 6：封后写拦截。
    let mut b6 = build_standard()?;
    if b6.put(0, "再写", Timing::Run, 0) != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    if b6.seal() != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    // 错误路径 7：注记被清字段 → 可达红。
    let mut b7 = build_standard()?;
    b7.sealed = false;
    if let Some(r) = &mut b7.slots[2] {
        r.field = String::new();
    }
    if b7.all_reachable() != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    Ok(0)
}
