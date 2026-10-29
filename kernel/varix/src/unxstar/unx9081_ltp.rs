//! UNX-F9081 LTP 登记总纲（"可跑"三档定义）（0x629E0 段）
//!
//! 锚点：UNX-F9081-J1 总纲账（四登记槽闭集全录 + 槽 × 起点 × 终点逐槽
//! 落账 + 渲染导出面）；J2 总纲语义（登记连续无缝 + 登记序 + 总时长 =
//! 各槽和三性质）；J3 封账印（总纲指纹 + 封印后只读 + 注入核）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B15（LTP 登记面）。
//!
//! 结构：四登记槽闭集 + 登记核（无缝闸 + 序核）+ 总纲账 + 总纲指纹 +
//! 注入核（槽位重叠/槽位断档/起点倒挂/未知槽位/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x629E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x629E0;
pub const LP_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const LP_DUP: u32 = SEG_BASE + 0x02; // 槽位重复登记
pub const LP_UNK: u32 = SEG_BASE + 0x03; // 未知槽位
pub const LP_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const LP_GAP: u32 = SEG_BASE + 0x05; // 槽位断档

/// LTP 登记总纲四槽闭集。
pub const LP_SLOT_TIER1: usize = 0; // 可跑·全绿档
pub const LP_SLOT_TIER2: usize = 1; // 可跑·有红档
pub const LP_SLOT_TIER3: usize = 2; // 可跑·跳过档
pub const LP_SLOT_PROBE: usize = 3; // 附加探针档
pub const LP_SLOT_COUNT: usize = 4;

/// 槽名渲染。
pub const LP_SLOT_NAMES: [&str; LP_SLOT_COUNT] = ["tier1", "tier2", "tier3", "probe"];

/// 单槽登记账。
#[derive(Clone)]
pub struct LtpPlanEntry {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl LtpPlanEntry {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            LP_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 槽序核：start ≤ end。
pub fn lp_order_ok(e: &LtpPlanEntry) -> bool {
    e.start_ms <= e.end_ms
}

/// 槽连续核：前槽终点 = 后槽起点。
pub fn lp_continuous(es: &[LtpPlanEntry]) -> bool {
    es.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 槽无重叠核。
pub fn lp_no_overlap(es: &[LtpPlanEntry]) -> bool {
    es.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// LTP 登记总纲账本。
pub struct LtpPlan {
    entries: [Option<LtpPlanEntry>; LP_SLOT_COUNT],
    sealed: bool,
}

impl LtpPlan {
    pub fn new() -> Self {
        LtpPlan {
            entries: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个槽：槽闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LP_SEALED);
        }
        if slot >= LP_SLOT_COUNT {
            return Err(LP_UNK);
        }
        let e = LtpPlanEntry { slot, start_ms, end_ms };
        if !lp_order_ok(&e) {
            return Err(LP_INVERT);
        }
        if self.entries[slot].is_some() {
            return Err(LP_DUP);
        }
        self.entries[slot] = Some(e);
        Ok(())
    }

    pub fn entry(&self, slot: usize) -> Option<&LtpPlanEntry> {
        if slot >= LP_SLOT_COUNT {
            return None;
        }
        self.entries[slot].as_ref()
    }

    /// 总纲完整性：四槽全录 + 连续无缝 + 无重叠。
    pub fn plan_ok(&self) -> bool {
        let filled: Vec<LtpPlanEntry> = self.entries.iter().flatten().map(|e| e.clone()).collect();
        filled.len() == LP_SLOT_COUNT && lp_continuous(&filled) && lp_no_overlap(&filled)
    }

    /// 总纲总时长 = 各槽和。
    pub fn total_ms(&self) -> u64 {
        self.entries.iter().flatten().map(|e| e.duration()).sum()
    }

    /// 最长槽（瓶颈候选）。
    pub fn longest_slot(&self) -> Option<usize> {
        self.entries
            .iter()
            .flatten()
            .max_by_key(|e| e.duration())
            .map(|e| e.slot)
    }

    /// 总纲指纹。
    pub fn lp_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (e.start_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (e.end_ms as u32);
            h ^= (e.end_ms >> 32) as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.plan_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!("LTPPLAN-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("LTPPLAN-SEAL\n");
        }
        out
    }
}

impl Default for LtpPlan {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四槽连续登记（0..150/150..520/520..760/760..800）。
fn standard_plan() -> LtpPlan {
    let mut l = LtpPlan::new();
    let _ = l.register(LP_SLOT_TIER1, 0, 150);
    let _ = l.register(LP_SLOT_TIER2, 150, 520);
    let _ = l.register(LP_SLOT_TIER3, 520, 760);
    let _ = l.register(LP_SLOT_PROBE, 760, 800);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = LtpPlan::new();
    if l.register(LP_SLOT_TIER1, 90, 40) == Err(LP_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_plan();
    if l.register(LP_SLOT_TIER2, 150, 200) == Err(LP_DUP) {
        caught += 1;
    }
    // 注 3：未知槽位
    let mut l = LtpPlan::new();
    if l.register(7, 0, 10) == Err(LP_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = LtpPlan::new();
    let _ = l.register(LP_SLOT_TIER1, 0, 150);
    let _ = l.register(LP_SLOT_TIER2, 200, 520);
    if !l.plan_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = LtpPlan::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 总纲账；J2 总纲语义；J3 封账印。
pub fn run_unx9081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9081");
    // J1-1 四槽全录
    let l = standard_plan();
    let ok = (0..LP_SLOT_COUNT).all(|i| l.entry(i).is_some()) && l.plan_ok();
    s.add("9081-J1-四槽全录", ok, if ok { "" } else { "总纲账不全" });
    // J1-2 渲染导出齐
    let text = standard_plan().render();
    let ok = text.contains("tier1|0..150|dur150")
        && text.contains("probe|760..800|dur40")
        && text.contains("LTPPLAN-TOTAL|800");
    s.add("9081-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（800）
    let ok = l.total_ms() == 800;
    s.add("9081-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 tier2 槽时长（370 最长）
    let ok = l.entry(LP_SLOT_TIER2).map(|q| q.duration() == 370).unwrap_or(false);
    s.add("9081-J2-tier2时长", ok, if ok { "" } else { "槽时长漂移" });
    // J2-3 无缝连续核
    let es: Vec<LtpPlanEntry> = standard_plan().entries.iter().flatten().map(|q| q.clone()).collect();
    let ok = lp_continuous(&es);
    s.add("9081-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = lp_no_overlap(&es);
    s.add("9081-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = LtpPlanEntry { slot: LP_SLOT_TIER1, start_ms: 90, end_ms: 40 };
    let ok = !lp_order_ok(&bad);
    s.add("9081-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈槽 = tier2
    let ok = l.longest_slot() == Some(LP_SLOT_TIER2);
    s.add("9081-J2-瓶颈槽", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 总纲指纹确定
    let ok = standard_plan().lp_fp() == standard_plan().lp_fp();
    s.add("9081-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_plan();
    let fp0 = l2.lp_fp();
    if let Some(q) = l2.entries[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = l2.lp_fp() != fp0;
    s.add("9081-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_plan();
    let ok = l3.seal() && l3.sealed();
    s.add("9081-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(LP_SLOT_TIER1, 0, 1) == Err(LP_SEALED);
    s.add("9081-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9081-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9081_all_checks_green() {
        let set = run_unx9081_checks();
        assert!(set.all_passed(), "UNX-F9081 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9081_plan_stable() {
        assert!(standard_plan().plan_ok());
        assert_eq!(standard_plan().total_ms(), 800);
    }
}
