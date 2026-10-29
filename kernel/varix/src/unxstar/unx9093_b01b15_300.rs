//! UNX-F9093 号面冻结清单总纲（B01–B15 铺面 300 号冻结协议）（0x62B60 段）
//!
//! 锚点：UNX-F9093-J1 登记账（四探针闭集全录 + 探针 × 起点 × 终点
//! 逐槽落账 + 渲染导出面）；J2 登记语义（探针连续无缝 + 探针序 + 总时长
//! = 各探针和三性质）；J3 封账印（登记指纹 + 封印后只读 + 注入核）三判
//! 据全绿。
//! 域：CoRun·UNX-C2-B15（LTP 登记面）。
//!
//! 结构：四探针闭集 + 探针登记核（无缝闸 + 序核）+ 登记账 + 登记指纹 +
//! 注入核（探针重叠/探针断档/起点倒挂/未知探针/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62B60 独占 0x20）。
pub const SEG_BASE: u32 = 0x62B60;
pub const FR_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const FR_DUP: u32 = SEG_BASE + 0x02; // 探针重复登记
pub const FR_UNK: u32 = SEG_BASE + 0x03; // 未知探针
pub const FR_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const FR_GAP: u32 = SEG_BASE + 0x05; // 探针断档

/// 四探针闭集。
pub const FR_SLOT_A: usize = 0; // core 探针
pub const FR_SLOT_B: usize = 1; // fs 探针
pub const FR_SLOT_C: usize = 2; // proc 探针
pub const FR_SLOT_D: usize = 3; // aux 探针
pub const FR_SLOT_COUNT: usize = 4;

/// 探针名渲染。
pub const FR_SLOT_NAMES: [&str; FR_SLOT_COUNT] =
    ["core", "fs", "proc", "aux"];

/// 单探针登记账。
#[derive(Clone)]
pub struct FreezeEntry {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl FreezeEntry {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            FR_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 探针序核：start ≤ end。
pub fn fr_order_ok(p: &FreezeEntry) -> bool {
    p.start_ms <= p.end_ms
}

/// 探针连续核：前探针终点 = 后探针起点。
pub fn fr_continuous(ps: &[FreezeEntry]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 探针无重叠核。
pub fn fr_no_overlap(ps: &[FreezeEntry]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// UNX-F9093 登记账本。
pub struct FreezePlan {
    probes: [Option<FreezeEntry>; FR_SLOT_COUNT],
    sealed: bool,
}

impl FreezePlan {
    pub fn new() -> Self {
        FreezePlan {
            probes: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个探针：探针闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(FR_SEALED);
        }
        if slot >= FR_SLOT_COUNT {
            return Err(FR_UNK);
        }
        let p = FreezeEntry { slot, start_ms, end_ms };
        if !fr_order_ok(&p) {
            return Err(FR_INVERT);
        }
        if self.probes[slot].is_some() {
            return Err(FR_DUP);
        }
        self.probes[slot] = Some(p);
        Ok(())
    }

    pub fn probe(&self, slot: usize) -> Option<&FreezeEntry> {
        if slot >= FR_SLOT_COUNT {
            return None;
        }
        self.probes[slot].as_ref()
    }

    /// 登记完整性：四探针全录 + 连续无缝 + 无重叠。
    pub fn suite_ok(&self) -> bool {
        let filled: Vec<FreezeEntry> = self.probes.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == FR_SLOT_COUNT
            && fr_continuous(&filled)
            && fr_no_overlap(&filled)
    }

    /// 登记总时长 = 各探针和。
    pub fn total_ms(&self) -> u64 {
        self.probes.iter().flatten().map(|p| p.duration()).sum()
    }

    /// 最长探针。
    pub fn longest_slot(&self) -> Option<usize> {
        self.probes
            .iter()
            .flatten()
            .max_by_key(|p| p.duration())
            .map(|p| p.slot)
    }

    /// 登记指纹。
    pub fn fr_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in self.probes.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (p.start_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (p.end_ms as u32);
            h ^= (p.end_ms >> 32) as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.suite_ok() {
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
        for p in self.probes.iter().flatten() {
            out.push_str(&p.render_line());
            out.push('\n');
        }
        out.push_str(&format!("FREEZE-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("FREEZE-SEAL\n");
        }
        out
    }
}

impl Default for FreezePlan {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四探针连续登记（0..96/96..240/240..288/288..336）。
fn standard_suite() -> FreezePlan {
    let mut l = FreezePlan::new();
    let _ = l.register(FR_SLOT_A, 0, 96);
    let _ = l.register(FR_SLOT_B, 96, 240);
    let _ = l.register(FR_SLOT_C, 240, 288);
    let _ = l.register(FR_SLOT_D, 288, 336);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = FreezePlan::new();
    if l.register(FR_SLOT_A, 90, 40) == Err(FR_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_suite();
    if l.register(FR_SLOT_B, 96, 240) == Err(FR_DUP) {
        caught += 1;
    }
    // 注 3：未知探针
    let mut l = FreezePlan::new();
    if l.register(7, 0, 10) == Err(FR_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = FreezePlan::new();
    let _ = l.register(FR_SLOT_A, 0, 96);
    let _ = l.register(FR_SLOT_B, 0, 96);
    if !l.suite_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = FreezePlan::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 登记账；J2 登记语义；J3 封账印。
pub fn run_unx9093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9093");
    // J1-1 四探针全录
    let l = standard_suite();
    let ok = (0..FR_SLOT_COUNT).all(|i| l.probe(i).is_some()) && l.suite_ok();
    s.add("9093-J1-四探针全录", ok, if ok { "" } else { "登记账不全" });
    // J1-2 渲染导出齐
    let text = standard_suite().render();
    let ok = text.contains("core|0..96|dur96")
        && text.contains("aux|288..336|dur48")
        && text.contains("FREEZE-TOTAL|336");
    s.add("9093-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（336）
    let ok = l.total_ms() == 336;
    s.add("9093-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 fs 探针时长（144 最长）
    let ok = l
        .probe(FR_SLOT_B)
        .map(|q| q.duration() == 144)
        .unwrap_or(false);
    s.add("9093-J2-fs时长", ok, if ok { "" } else { "探针时长漂移" });
    // J2-3 无缝连续核
    let ps: Vec<FreezeEntry> = standard_suite().probes.iter().flatten().map(|q| q.clone()).collect();
    let ok = fr_continuous(&ps);
    s.add("9093-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = fr_no_overlap(&ps);
    s.add("9093-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = FreezeEntry { slot: FR_SLOT_A, start_ms: 90, end_ms: 40 };
    let ok = !fr_order_ok(&bad);
    s.add("9093-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈探针 = fs
    let ok = l.longest_slot() == Some(FR_SLOT_B);
    s.add("9093-J2-瓶颈探针", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 登记指纹确定
    let ok = standard_suite().fr_fp() == standard_suite().fr_fp();
    s.add("9093-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_suite();
    let fp0 = l2.fr_fp();
    if let Some(q) = l2.probes[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = l2.fr_fp() != fp0;
    s.add("9093-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_suite();
    let ok = l3.seal() && l3.sealed();
    s.add("9093-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(FR_SLOT_A, 0, 1) == Err(FR_SEALED);
    s.add("9093-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9093-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9093_all_checks_green() {
        let set = run_unx9093_checks();
        assert!(set.all_passed(), "UNX-F9093 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9093_suite_stable() {
        assert!(standard_suite().suite_ok());
        assert_eq!(standard_suite().total_ms(), 336);
    }
}
