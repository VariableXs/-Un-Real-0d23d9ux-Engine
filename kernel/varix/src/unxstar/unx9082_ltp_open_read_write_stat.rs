//! UNX-F9082 LTP 文件族测试集登记（open/read/write/stat）（0x62A00 段）
//!
//! 锚点：UNX-F9082-J1 登记账（文件族四探针闭集全录 + 探针 × 起点 × 终点
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

/// 诊断段（0x62A00 独占 0x20）。
pub const SEG_BASE: u32 = 0x62A00;
pub const LF_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const LF_DUP: u32 = SEG_BASE + 0x02; // 探针重复登记
pub const LF_UNK: u32 = SEG_BASE + 0x03; // 未知探针
pub const LF_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const LF_GAP: u32 = SEG_BASE + 0x05; // 探针断档

/// 文件族四探针闭集。
pub const LF_SLOT_OPEN: usize = 0; // open/read/write 主探针
pub const LF_SLOT_STAT: usize = 1; // stat 族探针
pub const LF_SLOT_DIR: usize = 2; // 目录族探针
pub const LF_SLOT_LINK: usize = 3; // 链接族探针
pub const LF_SLOT_COUNT: usize = 4;

/// 探针名渲染。
pub const LF_SLOT_NAMES: [&str; LF_SLOT_COUNT] = ["open", "stat", "dir", "link"];

/// 单探针登记账。
#[derive(Clone)]
pub struct FsProbe {
    pub slot: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl FsProbe {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            LF_SLOT_NAMES[self.slot],
            self.start_ms,
            self.end_ms,
            self.duration()
        )
    }
}

/// 探针序核：start ≤ end。
pub fn lf_order_ok(p: &FsProbe) -> bool {
    p.start_ms <= p.end_ms
}

/// 探针连续核：前探针终点 = 后探针起点。
pub fn lf_continuous(ps: &[FsProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 探针无重叠核。
pub fn lf_no_overlap(ps: &[FsProbe]) -> bool {
    ps.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// LTP 文件族登记账本。
pub struct LtpFileSuite {
    probes: [Option<FsProbe>; LF_SLOT_COUNT],
    sealed: bool,
}

impl LtpFileSuite {
    pub fn new() -> Self {
        LtpFileSuite {
            probes: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个探针：探针闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LF_SEALED);
        }
        if slot >= LF_SLOT_COUNT {
            return Err(LF_UNK);
        }
        let p = FsProbe { slot, start_ms, end_ms };
        if !lf_order_ok(&p) {
            return Err(LF_INVERT);
        }
        if self.probes[slot].is_some() {
            return Err(LF_DUP);
        }
        self.probes[slot] = Some(p);
        Ok(())
    }

    pub fn probe(&self, slot: usize) -> Option<&FsProbe> {
        if slot >= LF_SLOT_COUNT {
            return None;
        }
        self.probes[slot].as_ref()
    }

    /// 登记完整性：四探针全录 + 连续无缝 + 无重叠。
    pub fn suite_ok(&self) -> bool {
        let filled: Vec<FsProbe> = self.probes.iter().flatten().map(|p| p.clone()).collect();
        filled.len() == LF_SLOT_COUNT && lf_continuous(&filled) && lf_no_overlap(&filled)
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
    pub fn lf_fp(&self) -> u32 {
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
        out.push_str(&format!("LTPFS-TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("LTPFS-SEAL\n");
        }
        out
    }
}

impl Default for LtpFileSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四探针连续登记（0..64/64..192/192..240/240..336）。
fn standard_suite() -> LtpFileSuite {
    let mut l = LtpFileSuite::new();
    let _ = l.register(LF_SLOT_OPEN, 0, 64);
    let _ = l.register(LF_SLOT_STAT, 64, 192);
    let _ = l.register(LF_SLOT_DIR, 192, 240);
    let _ = l.register(LF_SLOT_LINK, 240, 336);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = LtpFileSuite::new();
    if l.register(LF_SLOT_OPEN, 90, 40) == Err(LF_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_suite();
    if l.register(LF_SLOT_STAT, 64, 100) == Err(LF_DUP) {
        caught += 1;
    }
    // 注 3：未知探针
    let mut l = LtpFileSuite::new();
    if l.register(7, 0, 10) == Err(LF_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = LtpFileSuite::new();
    let _ = l.register(LF_SLOT_OPEN, 0, 64);
    let _ = l.register(LF_SLOT_STAT, 128, 192);
    if !l.suite_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = LtpFileSuite::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 登记账；J2 登记语义；J3 封账印。
pub fn run_unx9082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9082");
    // J1-1 四探针全录
    let l = standard_suite();
    let ok = (0..LF_SLOT_COUNT).all(|i| l.probe(i).is_some()) && l.suite_ok();
    s.add("9082-J1-四探针全录", ok, if ok { "" } else { "登记账不全" });
    // J1-2 渲染导出齐
    let text = standard_suite().render();
    let ok = text.contains("open|0..64|dur64")
        && text.contains("link|240..336|dur96")
        && text.contains("LTPFS-TOTAL|336");
    s.add("9082-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（336）
    let ok = l.total_ms() == 336;
    s.add("9082-J2-总时长", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 stat 探针时长（128 最长）
    let ok = l.probe(LF_SLOT_STAT).map(|q| q.duration() == 128).unwrap_or(false);
    s.add("9082-J2-stat时长", ok, if ok { "" } else { "探针时长漂移" });
    // J2-3 无缝连续核
    let ps: Vec<FsProbe> = standard_suite().probes.iter().flatten().map(|q| q.clone()).collect();
    let ok = lf_continuous(&ps);
    s.add("9082-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = lf_no_overlap(&ps);
    s.add("9082-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = FsProbe { slot: LF_SLOT_OPEN, start_ms: 90, end_ms: 40 };
    let ok = !lf_order_ok(&bad);
    s.add("9082-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 瓶颈探针 = stat
    let ok = l.longest_slot() == Some(LF_SLOT_STAT);
    s.add("9082-J2-瓶颈探针", ok, if ok { "" } else { "瓶颈判定漂移" });
    // J3-1 登记指纹确定
    let ok = standard_suite().lf_fp() == standard_suite().lf_fp();
    s.add("9082-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_suite();
    let fp0 = l2.lf_fp();
    if let Some(q) = l2.probes[0].as_mut() {
        q.end_ms += 1;
    }
    let ok = l2.lf_fp() != fp0;
    s.add("9082-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_suite();
    let ok = l3.seal() && l3.sealed();
    s.add("9082-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(LF_SLOT_OPEN, 0, 1) == Err(LF_SEALED);
    s.add("9082-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9082-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9082_all_checks_green() {
        let set = run_unx9082_checks();
        assert!(set.all_passed(), "UNX-F9082 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9082_suite_stable() {
        assert!(standard_suite().suite_ok());
        assert_eq!(standard_suite().total_ms(), 336);
    }
}
