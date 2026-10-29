//! UNX-F9086 LTP 信号族测试集登记（sigaction/kill）（0x62A80 段）
//!
//! 锚点：UNX-F9086-J1 登记账（四探针闭集全录 + 探针 × 起点 × 终点
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

/// 诊断段（0x62A80 独占 0x20）。
pub const SEG_BASE: u32 = 0x62A80;
pub const LS_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const LS_DUP: u32 = SEG_BASE + 0x02; // 探针重复登记
pub const LS_UNK: u32 = SEG_BASE + 0x03; // 未知探针
pub const LS_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const LS_GAP: u32 = SEG_BASE + 0x05; // 探针断档

/// 四探针闭集。
pub const LS_SLOT_A: usize = 0; // sigaction 探针
pub const LS_SLOT_B: usize = 1; // kill 探针
pub const LS_SLOT_C: usize = 2; // sigmask 探针
pub const LS_SLOT_D: usize = 3; // suspend 探针
pub const LS_SLOT_COUNT: usize = 4;

/// 探针名渲染。
pub const LS_SLOT_NAMES: [&str; LS_SLOT_COUNT] =
    ["sigaction", "kill", "sigmask", "suspend"];

/// 单探针登记账。
#[derive(Clone)]
pub struct SigProbe {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl SigProbe {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            LS_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 探针序核：start ≤ end。
pub fn ls_order_ok(p: &SigProbe) -> bool {
    p.start_ms <= p.end_ms
}

/// 探针连续核：前探针终点 = 后探针起点。
pub fn ls_continuous(ps: &[SigProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 探针无重叠核。
pub fn ls_no_overlap(ps: &[SigProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// UNX-F9086 登记账本。
pub struct LtpSigSuite {
    probes: [Option<SigProbe>; LS_SLOT_COUNT],
    sealed: bool,
}

impl LtpSigSuite {
    pub fn new() -> Self {
        LtpSigSuite {
            probes: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个探针：探针闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_SEALED);
        }
        if slot >= LS_SLOT_COUNT {
            return Err(LS_UNK);
        }
        let p = SigProbe { slot, start_ms, end_ms };
        if !ls_order_ok(&p) {
            return Err(LS_INVERT);
        }
        if self.probes[slot].is_some() {
            return Err(LS_DUP);
        }
        self.probes[slot] = Some(p);
        Ok(())
    }

    pub fn probe(&self, slot: usize) -> Option<&SigProbe> {
        if slot >= LS_SLOT_COUNT {
            return None;
        }
        self.probes[slot].as_ref()
    }

    /// 登记完整性：四探针全录 + 连续无缝 + 无重叠。
    pub fn suite_ok(&self) -> bool {
        let filled: Vec<SigProbe> = self.probes.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == LS_SLOT_COUNT
            && ls_continuous(&filled)
            && ls_no_overlap(&filled)
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
    pub fn ls_fp(&self) -> u32 {
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
        out.push_str(&format!("LTPSIG-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("LTPSIG-SEAL\n");
        }
        out
    }
}

impl Default for LtpSigSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四探针连续登记（0..40/40..130/130..210/210..260）。
fn standard_suite() -> LtpSigSuite {
    let mut l = LtpSigSuite::new();
    let _ = l.register(LS_SLOT_A, 0, 40);
    let _ = l.register(LS_SLOT_B, 40, 130);
    let _ = l.register(LS_SLOT_C, 130, 210);
    let _ = l.register(LS_SLOT_D, 210, 260);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = LtpSigSuite::new();
    if l.register(LS_SLOT_A, 90, 40) == Err(LS_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_suite();
    if l.register(LS_SLOT_B, 40, 130) == Err(LS_DUP) {
        caught += 1;
    }
    // 注 3：未知探针
    let mut l = LtpSigSuite::new();
    if l.register(7, 0, 10) == Err(LS_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = LtpSigSuite::new();
    let _ = l.register(LS_SLOT_A, 0, 40);
    let _ = l.register(LS_SLOT_B, 0, 40);
    if !l.suite_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = LtpSigSuite::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 登记账；J2 登记语义；J3 封账印。
pub fn run_unx9086_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9086");
    // J1-1 四探针全录
    let l = standard_suite();
    let ok = (0..LS_SLOT_COUNT).all(|i| l.probe(i).is_some()) && l.suite_ok();
    s.add("9086-J1-四探针全录", ok, if ok { "" } else { "登记账不全" });
    // J1-2 渲染导出齐
    let text = standard_suite().render();
    let ok = text.contains("sigaction|0..40|dur40")
        && text.contains("suspend|210..260|dur50")
        && text.contains("LTPSIG-TOTAL|260");
    s.add("9086-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（260）
    let ok = l.total_ms() == 260;
    s.add("9086-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 kill 探针时长（90 最长）
    let ok = l
        .probe(LS_SLOT_B)
        .map(|q| q.duration() == 90)
        .unwrap_or(false);
    s.add("9086-J2-kill时长", ok, if ok { "" } else { "探针时长漂移" });
    // J2-3 无缝连续核
    let ps: Vec<SigProbe> = standard_suite().probes.iter().flatten().map(|q| q.clone()).collect();
    let ok = ls_continuous(&ps);
    s.add("9086-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = ls_no_overlap(&ps);
    s.add("9086-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = SigProbe { slot: LS_SLOT_A, start_ms: 90, end_ms: 40 };
    let ok = !ls_order_ok(&bad);
    s.add("9086-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈探针 = kill
    let ok = l.longest_slot() == Some(LS_SLOT_B);
    s.add("9086-J2-瓶颈探针", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 登记指纹确定
    let ok = standard_suite().ls_fp() == standard_suite().ls_fp();
    s.add("9086-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_suite();
    let fp0 = l2.ls_fp();
    if let Some(q) = l2.probes[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = l2.ls_fp() != fp0;
    s.add("9086-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_suite();
    let ok = l3.seal() && l3.sealed();
    s.add("9086-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(LS_SLOT_A, 0, 1) == Err(LS_SEALED);
    s.add("9086-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9086-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9086_all_checks_green() {
        let set = run_unx9086_checks();
        assert!(set.all_passed(), "UNX-F9086 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9086_suite_stable() {
        assert!(standard_suite().suite_ok());
        assert_eq!(standard_suite().total_ms(), 260);
    }
}
