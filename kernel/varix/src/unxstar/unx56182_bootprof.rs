//! UNX-F56182 引导段子段剖析件（0x616C0 段）
//!
//! 锚点：UNX-F56182-J1 引导段子段账（引导段四子段闭集——固件自检/
//! 加载器装载/选单驻留/交接预备——子段 × 起点 × 终点逐子段落账）；
//! J2 剖析语义（子段连续无缝 + 子段序 + 总时长 = 各子段和三性质）；
//! J3 剖析指纹（子段界折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·引导段子段面）。
//!
//! 结构：四子段闭集 + 子段登记核（无缝闸 + 序核）+ 剖析账 + 剖析指
//! 纹 + 注入核（子段重叠/子段断档/起点倒挂/未知子段/空剖析印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x616C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x616C0;
pub const BP_OVERLAP: u32 = SEG_BASE + 0x01; // 子段区间重叠
pub const BP_GAP: u32 = SEG_BASE + 0x02; // 子段断档
pub const BP_INVERT: u32 = SEG_BASE + 0x03; // 起点倒挂
pub const BP_UNK_SUB: u32 = SEG_BASE + 0x04; // 未知子段
pub const BP_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空剖析印

/// 引导段四子段闭集。
pub const SB_FWPOST: usize = 0; // 固件自检
pub const SB_LOAD: usize = 1; // 加载器装载
pub const SB_MENU: usize = 2; // 选单驻留
pub const SB_HANDOFF: usize = 3; // 交接预备
pub const SUB_COUNT: usize = 4;

pub const SUB_NAMES: [&str; SUB_COUNT] = ["fwpost", "load", "menu", "handoff"];

/// 单子段剖析账。
#[derive(Clone)]
pub struct SubPhase {
    pub sub: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl SubPhase {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            SUB_NAMES[self.sub],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 子段序核：start ≤ end。
pub fn sub_order_ok(p: &SubPhase) -> bool {
    p.start_ms <= p.end_ms
}

/// 子段连续核：前子段终点 = 后子段起点。
pub fn prof_continuous(ph: &[SubPhase]) -> bool {
    ph.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 子段无重叠核。
pub fn prof_no_overlap(ph: &[SubPhase]) -> bool {
    ph.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// 引导段子段剖析器。
pub struct BootProfiler {
    phases: [Option<SubPhase>; SUB_COUNT],
    sealed: bool,
}

impl BootProfiler {
    pub fn new() -> Self {
        BootProfiler {
            phases: [None, None, None, None],
            sealed: false,
        }
    }

    /// 剖析一个子段：子段闭集 + 序核 + 重复拒 + 印后拒。
    pub fn profile(&mut self, sub: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_EMPTY_SEAL);
        }
        if sub >= SUB_COUNT {
            return Err(BP_UNK_SUB);
        }
        let p = SubPhase { sub, start_ms, end_ms };
        if !sub_order_ok(&p) {
            return Err(BP_INVERT);
        }
        if self.phases[sub].is_some() {
            return Err(BP_OVERLAP);
        }
        self.phases[sub] = Some(p);
        Ok(())
    }

    pub fn phase(&self, sub: usize) -> Option<&SubPhase> {
        if sub >= SUB_COUNT {
            return None;
        }
        self.phases[sub].as_ref()
    }

    /// 剖析完整性：四子段全录 + 连续无缝 + 无重叠。
    pub fn profile_ok(&self) -> bool {
        let filled: Vec<SubPhase> = self.phases.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == SUB_COUNT && prof_continuous(&filled) && prof_no_overlap(&filled)
    }

    /// 引导段总时长 = 各子段和。
    pub fn total_ms(&self) -> u64 {
        self.phases.iter().flatten().map(|p| p.duration()).sum()
    }

    /// 最长子段（瓶颈候选）。
    pub fn longest_sub(&self) -> Option<usize> {
        self.phases
            .iter()
            .flatten()
            .max_by_key(|p| p.duration())
            .map(|p| p.sub)
    }

    /// 剖析指纹。
    pub fn prof_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in self.phases.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (p.start_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (p.end_ms as u32);
            h ^= (p.end_ms >> 32) as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封剖析印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.profile_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_prof(&self) -> String {
        let mut out = String::new();
        for p in self.phases.iter().flatten() {
            out.push_str(&p.render_line());
            out.push('\n');
        }
        out.push_str(&format!("BOOTPROF-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("BOOTPROF-SEAL\n");
        }
        out
    }
}

impl Default for BootProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四子段连续剖析（0..150/150..520/520..760/760..800）。
fn standard_profiler() -> BootProfiler {
    let mut p = BootProfiler::new();
    let _ = p.profile(SB_FWPOST, 0, 150);
    let _ = p.profile(SB_LOAD, 150, 520);
    let _ = p.profile(SB_MENU, 520, 760);
    let _ = p.profile(SB_HANDOFF, 760, 800);
    p
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut p = BootProfiler::new();
    if p.profile(SB_FWPOST, 90, 40) == Err(BP_INVERT) {
        caught += 1;
    }
    // 注 2：重复剖析
    let mut p = standard_profiler();
    if p.profile(SB_LOAD, 150, 200) == Err(BP_OVERLAP) {
        caught += 1;
    }
    // 注 3：未知子段
    let mut p = BootProfiler::new();
    if p.profile(7, 0, 10) == Err(BP_UNK_SUB) {
        caught += 1;
    }
    // 注 4：断档剖析不过完整性核
    let mut p = BootProfiler::new();
    let _ = p.profile(SB_FWPOST, 0, 150);
    let _ = p.profile(SB_LOAD, 200, 520);
    if !p.profile_ok() {
        caught += 1;
    }
    // 注 5：空剖析印
    let mut p = BootProfiler::new();
    if !p.seal() && !p.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 子段账；J2 剖析语义；J3 剖析指纹。
pub fn run_unx56182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56182");
    // J1-1 四子段全录
    let p = standard_profiler();
    let ok = (0..SUB_COUNT).all(|i| p.phase(i).is_some()) && p.profile_ok();
    s.add("56182-J1-四子段全录", ok, if ok { "" } else { "子段账不全" });
    // J1-2 渲染导出齐
    let text = standard_profiler().render_prof();
    let ok = text.contains("fwpost|0..150|dur150")
        && text.contains("handoff|760..800|dur40")
        && text.contains("BOOTPROF-TOTAL|800");
    s.add("56182-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 引导总时长核（800）
    let ok = p.total_ms() == 800;
    s.add("56182-J2-引导总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 加载子段时长（370 最长）
    let ok = p.phase(SB_LOAD).map(|q| q.duration() == 370).unwrap_or(false);
    s.add("56182-J2-加载时长", ok, if ok { "" } else { "子段时长漂移" });
    // J2-3 无缝连续核
    let ph: Vec<SubPhase> = standard_profiler().phases.iter().flatten().map(|q| q.clone()).collect();
    let ok = prof_continuous(&ph);
    s.add("56182-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = prof_no_overlap(&ph);
    s.add("56182-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = SubPhase { sub: SB_FWPOST, start_ms: 90, end_ms: 40 };
    let ok = !sub_order_ok(&bad);
    s.add("56182-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈子段 = load
    let ok = p.longest_sub() == Some(SB_LOAD);
    s.add("56182-J2-瓶颈子段", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 剖析指纹确定
    let ok = standard_profiler().prof_fp() == standard_profiler().prof_fp();
    s.add("56182-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut p2 = standard_profiler();
    let fp0 = p2.prof_fp();
    if let Some(q) = p2.phases[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = p2.prof_fp() != fp0;
    s.add("56182-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封剖析印
    let mut p3 = standard_profiler();
    let ok = p3.seal() && p3.sealed();
    s.add("56182-J3-封剖析印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒剖析
    let ok = p3.profile(SB_FWPOST, 0, 1) == Err(BP_EMPTY_SEAL);
    s.add("56182-J3-印后拒剖析", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56182-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56182_all_checks_green() {
        let set = run_unx56182_checks();
        assert!(set.all_passed(), "UNX-F56182 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56182_prof_stable() {
        assert!(standard_profiler().profile_ok());
    }
}
