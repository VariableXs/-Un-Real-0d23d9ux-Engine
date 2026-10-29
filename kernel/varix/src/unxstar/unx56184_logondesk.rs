//! UNX-F56184 登录桌面段子段剖析件（0x61700 段）
//!
//! 锚点：UNX-F56184-J1 登录桌面段子段账（四子段闭集——会话管理器
//! 启动/桌面合成器/登录界面就绪/凭据通路——子段 × 起点 × 终点逐子段
//! 落账）；J2 剖析语义（子段连续无缝 + 子段序 + 总时长 = 各子段和）；
//! J3 剖析指纹（子段界折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·登录桌面段子段面）。
//!
//! 结构：四子段闭集 + 子段登记核（无缝闸 + 序核）+ 剖析账 + 剖析指
//! 纹 + 注入核（子段重叠/子段断档/起点倒挂/未知子段/空剖析印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61700 独占 0x20）。
pub const SEG_BASE: u32 = 0x61700;
pub const LD_OVERLAP: u32 = SEG_BASE + 0x01; // 子段区间重叠
pub const LD_GAP: u32 = SEG_BASE + 0x02; // 子段断档
pub const LD_INVERT: u32 = SEG_BASE + 0x03; // 起点倒挂
pub const LD_UNK_SUB: u32 = SEG_BASE + 0x04; // 未知子段
pub const LD_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空剖析印

/// 登录桌面段四子段闭集。
pub const SL_SESMGR: usize = 0; // 会话管理器启动
pub const SL_COMPOS: usize = 1; // 桌面合成器
pub const SL_LOGINGUI: usize = 2; // 登录界面就绪
pub const SL_CRED: usize = 3; // 凭据通路
pub const SUB_COUNT: usize = 4;

pub const SUB_NAMES: [&str; SUB_COUNT] = ["sesmgr", "compos", "logingui", "cred"];

/// 单子段剖析账。
#[derive(Clone)]
pub struct LSubPhase {
    pub sub: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl LSubPhase {
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

/// 子段序核。
pub fn lsub_order_ok(p: &LSubPhase) -> bool {
    p.start_ms <= p.end_ms
}

/// 子段连续核。
pub fn lprof_continuous(ph: &[LSubPhase]) -> bool {
    ph.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 子段无重叠核。
pub fn lprof_no_overlap(ph: &[LSubPhase]) -> bool {
    ph.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// 登录桌面段子段剖析器。
pub struct LogonDeskProfiler {
    phases: [Option<LSubPhase>; SUB_COUNT],
    sealed: bool,
}

impl LogonDeskProfiler {
    pub fn new() -> Self {
        LogonDeskProfiler {
            phases: [None, None, None, None],
            sealed: false,
        }
    }

    /// 剖析一个子段：子段闭集 + 序核 + 重复拒 + 印后拒。
    pub fn profile(&mut self, sub: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LD_EMPTY_SEAL);
        }
        if sub >= SUB_COUNT {
            return Err(LD_UNK_SUB);
        }
        let p = LSubPhase { sub, start_ms, end_ms };
        if !lsub_order_ok(&p) {
            return Err(LD_INVERT);
        }
        if self.phases[sub].is_some() {
            return Err(LD_OVERLAP);
        }
        self.phases[sub] = Some(p);
        Ok(())
    }

    pub fn phase(&self, sub: usize) -> Option<&LSubPhase> {
        if sub >= SUB_COUNT {
            return None;
        }
        self.phases[sub].as_ref()
    }

    /// 剖析完整性：四子段全录 + 连续无缝 + 无重叠。
    pub fn profile_ok(&self) -> bool {
        let filled: Vec<LSubPhase> = self.phases.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == SUB_COUNT && lprof_continuous(&filled) && lprof_no_overlap(&filled)
    }

    /// 登录桌面段总时长 = 各子段和。
    pub fn total_ms(&self) -> u64 {
        self.phases.iter().flatten().map(|p| p.duration()).sum()
    }

    /// 瓶颈子段。
    pub fn longest_sub(&self) -> Option<usize> {
        self.phases
            .iter()
            .flatten()
            .max_by_key(|p| p.duration())
            .map(|p| p.sub)
    }

    /// 合成器子段时长（独立取值便捷口）。
    pub fn compos_ms(&self) -> u64 {
        self.phases[SL_COMPOS].as_ref().map(|p| p.duration()).unwrap_or(0)
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
        out.push_str(&format!("LDPROF-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("LDPROF-SEAL\n");
        }
        out
    }
}

impl Default for LogonDeskProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四子段连续剖析（2600..3000/3000..4100/4100..4800/4800..5200）。
fn standard_profiler() -> LogonDeskProfiler {
    let mut p = LogonDeskProfiler::new();
    let _ = p.profile(SL_SESMGR, 2600, 3000);
    let _ = p.profile(SL_COMPOS, 3000, 4100);
    let _ = p.profile(SL_LOGINGUI, 4100, 4800);
    let _ = p.profile(SL_CRED, 4800, 5200);
    p
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut p = LogonDeskProfiler::new();
    if p.profile(SL_SESMGR, 2800, 2700) == Err(LD_INVERT) {
        caught += 1;
    }
    // 注 2：重复剖析
    let mut p = standard_profiler();
    if p.profile(SL_COMPOS, 3000, 3200) == Err(LD_OVERLAP) {
        caught += 1;
    }
    // 注 3：未知子段
    let mut p = LogonDeskProfiler::new();
    if p.profile(6, 0, 10) == Err(LD_UNK_SUB) {
        caught += 1;
    }
    // 注 4：断档剖析不过完整性核
    let mut p = LogonDeskProfiler::new();
    let _ = p.profile(SL_SESMGR, 2600, 3000);
    let _ = p.profile(SL_COMPOS, 3100, 4100);
    if !p.profile_ok() {
        caught += 1;
    }
    // 注 5：空剖析印
    let mut p = LogonDeskProfiler::new();
    if !p.seal() && !p.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 子段账；J2 剖析语义；J3 剖析指纹。
pub fn run_unx56184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56184");
    // J1-1 四子段全录
    let p = standard_profiler();
    let ok = (0..SUB_COUNT).all(|i| p.phase(i).is_some()) && p.profile_ok();
    s.add("56184-J1-四子段全录", ok, if ok { "" } else { "子段账不全" });
    // J1-2 渲染导出齐
    let text = standard_profiler().render_prof();
    let ok = text.contains("sesmgr|2600..3000|dur400")
        && text.contains("cred|4800..5200|dur400")
        && text.contains("LDPROF-TOTAL|2600");
    s.add("56184-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 段总时长核（2600）
    let ok = p.total_ms() == 2_600;
    s.add("56184-J2-段总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 合成器子段时长（1100 最长）
    let ok = p.phase(SL_COMPOS).map(|q| q.duration() == 1_100).unwrap_or(false);
    s.add("56184-J2-合成器时长", ok, if ok { "" } else { "子段时长漂移" });
    // J2-3 无缝连续核
    let ph: Vec<LSubPhase> = standard_profiler().phases.iter().flatten().map(|q| q.clone()).collect();
    let ok = lprof_continuous(&ph);
    s.add("56184-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = lprof_no_overlap(&ph);
    s.add("56184-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = LSubPhase { sub: SL_SESMGR, start_ms: 2800, end_ms: 2700 };
    let ok = !lsub_order_ok(&bad);
    s.add("56184-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈子段 = compos
    let ok = p.longest_sub() == Some(SL_COMPOS);
    s.add("56184-J2-瓶颈子段", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J2-7 合成器时长便捷口（1100）
    let ok = p.compos_ms() == 1_100;
    s.add("56184-J2-合成器便捷口", ok, if ok { "" } else { "便捷口漂移" });
    // J3-1 剖析指纹确定
    let ok = standard_profiler().prof_fp() == standard_profiler().prof_fp();
    s.add("56184-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut p2 = standard_profiler();
    let fp0 = p2.prof_fp();
    if let Some(q) = p2.phases[2].as_mut() {
        q.end_ms += 1;
    }
    let ok = p2.prof_fp() != fp0;
    s.add("56184-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封剖析印
    let mut p3 = standard_profiler();
    let ok = p3.seal() && p3.sealed();
    s.add("56184-J3-封剖析印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒剖析
    let ok = p3.profile(SL_SESMGR, 0, 1) == Err(LD_EMPTY_SEAL);
    s.add("56184-J3-印后拒剖析", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56184-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56184_all_checks_green() {
        let set = run_unx56184_checks();
        assert!(set.all_passed(), "UNX-F56184 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56184_prof_stable() {
        assert!(standard_profiler().profile_ok());
    }
}
