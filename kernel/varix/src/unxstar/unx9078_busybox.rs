//! UNX-F9078 消费核回归套件（静态链接 busybox 探针登记）（0x62980 段）
//!
//! 锚点：UNX-F9078-J1 探针账（busybox 四探针槽闭集全录 + 探针 × 起点 ×
//! 终点逐槽落账 + 渲染导出面）；J2 探针语义（探针连续无缝 + 探针序 +
//! 总时长 = 各探针和三性质）；J3 封账印（探针指纹 + 封印后只读 + 注入
//! 核）三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：四探针槽闭集 + 探针登记核（无缝闸 + 序核）+ 探针账 + 探针指纹
//! + 注入核（探针重叠/探针断档/起点倒挂/未知探针/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62980 独占 0x20）。
pub const SEG_BASE: u32 = 0x62980;
pub const BB_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const BB_DUP: u32 = SEG_BASE + 0x02; // 探针重复登记
pub const BB_UNK: u32 = SEG_BASE + 0x03; // 未知探针槽
pub const BB_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const BB_GAP: u32 = SEG_BASE + 0x05; // 探针断档（覆盖不连续）

/// busybox 四探针槽闭集。
pub const BB_SLOT_ECHO: usize = 0; // echo 探针（write 路径）
pub const BB_SLOT_LS: usize = 1; // ls 探针（getdents64 路径）
pub const BB_SLOT_SH: usize = 2; // sh 探针（fork/exec 路径）
pub const BB_SLOT_DATE: usize = 3; // date 探针（clock 路径）
pub const BB_SLOT_COUNT: usize = 4;

/// 探针名渲染。
pub const BB_SLOT_NAMES: [&str; BB_SLOT_COUNT] = ["echo", "ls", "sh", "date"];

/// 单探针登记账。
#[derive(Clone)]
pub struct BusyProbe {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl BusyProbe {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            BB_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 探针序核：start ≤ end。
pub fn bb_order_ok(p: &BusyProbe) -> bool {
    p.start_ms <= p.end_ms
}

/// 探针连续核：前探针终点 = 后探针起点。
pub fn bb_continuous(ps: &[BusyProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 探针无重叠核。
pub fn bb_no_overlap(ps: &[BusyProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// busybox 探针登记账本。
pub struct BusyboxSuite {
    probes: [Option<BusyProbe>; BB_SLOT_COUNT],
    sealed: bool,
}

impl BusyboxSuite {
    pub fn new() -> Self {
        BusyboxSuite {
            probes: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个探针：探针闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BB_SEALED);
        }
        if slot >= BB_SLOT_COUNT {
            return Err(BB_UNK);
        }
        let p = BusyProbe { slot, start_ms, end_ms };
        if !bb_order_ok(&p) {
            return Err(BB_INVERT);
        }
        if self.probes[slot].is_some() {
            return Err(BB_DUP);
        }
        self.probes[slot] = Some(p);
        Ok(())
    }

    pub fn probe(&self, slot: usize) -> Option<&BusyProbe> {
        if slot >= BB_SLOT_COUNT {
            return None;
        }
        self.probes[slot].as_ref()
    }

    /// 探针完整性：四探针全录 + 连续无缝 + 无重叠。
    pub fn suite_ok(&self) -> bool {
        let filled: Vec<BusyProbe> = self.probes.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == BB_SLOT_COUNT && bb_continuous(&filled) && bb_no_overlap(&filled)
    }

    /// 探针总时长 = 各探针和。
    pub fn total_ms(&self) -> u64 {
        self.probes.iter().flatten().map(|p| p.duration()).sum()
    }

    /// 最长探针（瓶颈候选）。
    pub fn longest_slot(&self) -> Option<usize> {
        self.probes
            .iter()
            .flatten()
            .max_by_key(|p| p.duration())
            .map(|p| p.slot)
    }

    /// 探针指纹。
    pub fn bb_fp(&self) -> u32 {
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
        out.push_str(&format!("BUSYBOX-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("BUSYBOX-SEAL\n");
        }
        out
    }
}

impl Default for BusyboxSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四探针连续登记（0..120/120..300/300..420/420..500）。
fn standard_suite() -> BusyboxSuite {
    let mut b = BusyboxSuite::new();
    let _ = b.register(BB_SLOT_ECHO, 0, 120);
    let _ = b.register(BB_SLOT_LS, 120, 300);
    let _ = b.register(BB_SLOT_SH, 300, 420);
    let _ = b.register(BB_SLOT_DATE, 420, 500);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut b = BusyboxSuite::new();
    if b.register(BB_SLOT_ECHO, 90, 40) == Err(BB_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut b = standard_suite();
    if b.register(BB_SLOT_LS, 120, 200) == Err(BB_DUP) {
        caught += 1;
    }
    // 注 3：未知探针槽
    let mut b = BusyboxSuite::new();
    if b.register(7, 0, 10) == Err(BB_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut b = BusyboxSuite::new();
    let _ = b.register(BB_SLOT_ECHO, 0, 120);
    let _ = b.register(BB_SLOT_LS, 200, 300);
    if !b.suite_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = BusyboxSuite::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 探针账；J2 探针语义；J3 封账印。
pub fn run_unx9078_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9078");
    // J1-1 四探针全录
    let b = standard_suite();
    let ok = (0..BB_SLOT_COUNT).all(|i| b.probe(i).is_some()) && b.suite_ok();
    s.add("9078-J1-四探针全录", ok, if ok { "" } else { "探针账不全" });
    // J1-2 渲染导出齐
    let text = standard_suite().render();
    let ok = text.contains("echo|0..120|dur120")
        && text.contains("date|420..500|dur80")
        && text.contains("BUSYBOX-TOTAL|500");
    s.add("9078-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（500）
    let ok = b.total_ms() == 500;
    s.add("9078-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 ls 探针时长（180 最长）
    let ok = b.probe(BB_SLOT_LS).map(|q| q.duration() == 180).unwrap_or(false);
    s.add("9078-J2-ls时长", ok, if ok { "" } else { "探针时长漂移" });
    // J2-3 无缝连续核
    let ps: Vec<BusyProbe> = standard_suite().probes.iter().flatten().map(|q| q.clone()).collect();
    let ok = bb_continuous(&ps);
    s.add("9078-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = bb_no_overlap(&ps);
    s.add("9078-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = BusyProbe { slot: BB_SLOT_ECHO, start_ms: 90, end_ms: 40 };
    let ok = !bb_order_ok(&bad);
    s.add("9078-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈探针 = ls
    let ok = b.longest_slot() == Some(BB_SLOT_LS);
    s.add("9078-J2-瓶颈探针", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 探针指纹确定
    let ok = standard_suite().bb_fp() == standard_suite().bb_fp();
    s.add("9078-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut b2 = standard_suite();
    let fp0 = b2.bb_fp();
    if let Some(q) = b2.probes[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = b2.bb_fp() != fp0;
    s.add("9078-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut b3 = standard_suite();
    let ok = b3.seal() && b3.sealed();
    s.add("9078-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = b3.register(BB_SLOT_ECHO, 0, 1) == Err(BB_SEALED);
    s.add("9078-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9078-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9078_all_checks_green() {
        let set = run_unx9078_checks();
        assert!(set.all_passed(), "UNX-F9078 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9078_suite_stable() {
        assert!(standard_suite().suite_ok());
        assert_eq!(standard_suite().total_ms(), 500);
    }
}
