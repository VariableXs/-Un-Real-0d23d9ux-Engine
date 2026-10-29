//! UNX-F9088 LTP IPC 测试集登记（pipe/msg/shm/sem）（0x62AC0 段）
//!
//! 锚点：UNX-F9088-J1 登记账（四探针闭集全录 + 探针 × 起点 × 终点
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

/// 诊断段（0x62AC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x62AC0;
pub const LI_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const LI_DUP: u32 = SEG_BASE + 0x02; // 探针重复登记
pub const LI_UNK: u32 = SEG_BASE + 0x03; // 未知探针
pub const LI_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const LI_GAP: u32 = SEG_BASE + 0x05; // 探针断档

/// 四探针闭集。
pub const LI_SLOT_A: usize = 0; // pipe 探针
pub const LI_SLOT_B: usize = 1; // msg 探针
pub const LI_SLOT_C: usize = 2; // shm 探针
pub const LI_SLOT_D: usize = 3; // sem 探针
pub const LI_SLOT_COUNT: usize = 4;

/// 探针名渲染。
pub const LI_SLOT_NAMES: [&str; LI_SLOT_COUNT] =
    ["pipe", "msg", "shm", "sem"];

/// 单探针登记账。
#[derive(Clone)]
pub struct IpcProbe {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl IpcProbe {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            LI_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 探针序核：start ≤ end。
pub fn li_order_ok(p: &IpcProbe) -> bool {
    p.start_ms <= p.end_ms
}

/// 探针连续核：前探针终点 = 后探针起点。
pub fn li_continuous(ps: &[IpcProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 探针无重叠核。
pub fn li_no_overlap(ps: &[IpcProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// UNX-F9088 登记账本。
pub struct LtpIpcSuite {
    probes: [Option<IpcProbe>; LI_SLOT_COUNT],
    sealed: bool,
}

impl LtpIpcSuite {
    pub fn new() -> Self {
        LtpIpcSuite {
            probes: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个探针：探针闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LI_SEALED);
        }
        if slot >= LI_SLOT_COUNT {
            return Err(LI_UNK);
        }
        let p = IpcProbe { slot, start_ms, end_ms };
        if !li_order_ok(&p) {
            return Err(LI_INVERT);
        }
        if self.probes[slot].is_some() {
            return Err(LI_DUP);
        }
        self.probes[slot] = Some(p);
        Ok(())
    }

    pub fn probe(&self, slot: usize) -> Option<&IpcProbe> {
        if slot >= LI_SLOT_COUNT {
            return None;
        }
        self.probes[slot].as_ref()
    }

    /// 登记完整性：四探针全录 + 连续无缝 + 无重叠。
    pub fn suite_ok(&self) -> bool {
        let filled: Vec<IpcProbe> = self.probes.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == LI_SLOT_COUNT
            && li_continuous(&filled)
            && li_no_overlap(&filled)
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
    pub fn li_fp(&self) -> u32 {
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
        out.push_str(&format!("LTPIPC-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("LTPIPC-SEAL\n");
        }
        out
    }
}

impl Default for LtpIpcSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四探针连续登记（0..70/70..160/160..230/230..310）。
fn standard_suite() -> LtpIpcSuite {
    let mut l = LtpIpcSuite::new();
    let _ = l.register(LI_SLOT_A, 0, 70);
    let _ = l.register(LI_SLOT_B, 70, 160);
    let _ = l.register(LI_SLOT_C, 160, 230);
    let _ = l.register(LI_SLOT_D, 230, 310);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = LtpIpcSuite::new();
    if l.register(LI_SLOT_A, 90, 40) == Err(LI_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_suite();
    if l.register(LI_SLOT_B, 70, 160) == Err(LI_DUP) {
        caught += 1;
    }
    // 注 3：未知探针
    let mut l = LtpIpcSuite::new();
    if l.register(7, 0, 10) == Err(LI_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = LtpIpcSuite::new();
    let _ = l.register(LI_SLOT_A, 0, 70);
    let _ = l.register(LI_SLOT_B, 0, 70);
    if !l.suite_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = LtpIpcSuite::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 登记账；J2 登记语义；J3 封账印。
pub fn run_unx9088_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9088");
    // J1-1 四探针全录
    let l = standard_suite();
    let ok = (0..LI_SLOT_COUNT).all(|i| l.probe(i).is_some()) && l.suite_ok();
    s.add("9088-J1-四探针全录", ok, if ok { "" } else { "登记账不全" });
    // J1-2 渲染导出齐
    let text = standard_suite().render();
    let ok = text.contains("pipe|0..70|dur70")
        && text.contains("sem|230..310|dur80")
        && text.contains("LTPIPC-TOTAL|310");
    s.add("9088-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（310）
    let ok = l.total_ms() == 310;
    s.add("9088-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 msg 探针时长（90 最长）
    let ok = l
        .probe(LI_SLOT_B)
        .map(|q| q.duration() == 90)
        .unwrap_or(false);
    s.add("9088-J2-msg时长", ok, if ok { "" } else { "探针时长漂移" });
    // J2-3 无缝连续核
    let ps: Vec<IpcProbe> = standard_suite().probes.iter().flatten().map(|q| q.clone()).collect();
    let ok = li_continuous(&ps);
    s.add("9088-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = li_no_overlap(&ps);
    s.add("9088-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = IpcProbe { slot: LI_SLOT_A, start_ms: 90, end_ms: 40 };
    let ok = !li_order_ok(&bad);
    s.add("9088-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈探针 = msg
    let ok = l.longest_slot() == Some(LI_SLOT_B);
    s.add("9088-J2-瓶颈探针", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 登记指纹确定
    let ok = standard_suite().li_fp() == standard_suite().li_fp();
    s.add("9088-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_suite();
    let fp0 = l2.li_fp();
    if let Some(q) = l2.probes[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = l2.li_fp() != fp0;
    s.add("9088-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_suite();
    let ok = l3.seal() && l3.sealed();
    s.add("9088-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(LI_SLOT_A, 0, 1) == Err(LI_SEALED);
    s.add("9088-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9088-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9088_all_checks_green() {
        let set = run_unx9088_checks();
        assert!(set.all_passed(), "UNX-F9088 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9088_suite_stable() {
        assert!(standard_suite().suite_ok());
        assert_eq!(standard_suite().total_ms(), 310);
    }
}
