//! UNX-F56181 启动链分段定位器（M 型深化旗舰）（0x616A0 段）
//!
//! 锚点：UNX-F56181-J1 分段定位账（启动链四段定位——引导段/内核段/
//! 登录桌面段/交互就绪段——段界时间戳锚定逐段落账：段 × 界标 × 起点
//! × 终点四元组）；J2 定位语义（段区间连续无缝 + 起点 ≤ 终点 + 总时
//! 长 = 各段和三性质）；J3 定位指纹（段界折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·M 型旗舰面）。
//!
//! 结构：四段闭集 + 段界登记核（连续无缝闸 + 序核）+ 定位账 + 定位指
//! 纹 + 注入核（段重叠/段断档/起点倒挂/未知段/空链印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x616A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x616A0;
pub const BL_OVERLAP: u32 = SEG_BASE + 0x01; // 段区间重叠
pub const BL_GAP: u32 = SEG_BASE + 0x02; // 段断档
pub const BL_INVERT: u32 = SEG_BASE + 0x03; // 起点倒挂
pub const BL_UNK_SEG: u32 = SEG_BASE + 0x04; // 未知段
pub const BL_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空链印

/// 启动链四段闭集。
pub const SG_BOOT: usize = 0; // 引导段（Limine→bootselect）
pub const SG_KERNEL: usize = 1; // 内核段（init→驱动→服务）
pub const SG_LOGIN: usize = 2; // 登录/桌面段
pub const SG_INTERACT: usize = 3; // 交互就绪段
pub const SEG_COUNT: usize = 4;

/// 段名渲染。
pub const SEGMENT_NAMES: [&str; SEG_COUNT] = ["boot", "kernel", "login", "interact"];

/// 单段定位账。
#[derive(Clone)]
pub struct SegmentMark {
    pub seg: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl SegmentMark {
    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|dur{}",
            SEGMENT_NAMES[self.seg],
            self.start_ms,
            self.end_ms,
            self.end_ms.saturating_sub(self.start_ms)
        )
    }

    /// 段时长。
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// 段序核：start ≤ end。
pub fn seg_order_ok(m: &SegmentMark) -> bool {
    m.start_ms <= m.end_ms
}

/// 段连续核：前段终点 = 后段起点（无缝启动链）。
pub fn chain_continuous(marks: &[SegmentMark]) -> bool {
    marks.windows(2).all(|w| w[0].end_ms == w[1].start_ms)
}

/// 段重叠核：无重叠。
pub fn chain_no_overlap(marks: &[SegmentMark]) -> bool {
    marks.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

/// 分段定位器。
pub struct BootLocator {
    marks: [Option<SegmentMark>; SEG_COUNT],
    sealed: bool,
}

impl BootLocator {
    pub fn new() -> Self {
        BootLocator {
            marks: [None, None, None, None],
            sealed: false,
        }
    }

    /// 定位一段：段闭集 + 序核 + 重定位拒 + 印后拒。
    pub fn locate(&mut self, seg: usize, start_ms: u64, end_ms: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BL_EMPTY_SEAL);
        }
        if seg >= SEG_COUNT {
            return Err(BL_UNK_SEG);
        }
        let m = SegmentMark { seg, start_ms, end_ms };
        if !seg_order_ok(&m) {
            return Err(BL_INVERT);
        }
        if self.marks[seg].is_some() {
            return Err(BL_OVERLAP);
        }
        self.marks[seg] = Some(m);
        Ok(())
    }

    pub fn mark(&self, seg: usize) -> Option<&SegmentMark> {
        if seg >= SEG_COUNT {
            return None;
        }
        self.marks[seg].as_ref()
    }

    /// 链完整性核：四段全定 + 按段序连续无缝 + 无重叠。
    pub fn chain_ok(&self) -> bool {
        let filled: Vec<&SegmentMark> = self.marks.iter().flatten().collect();
        if filled.len() != SEG_COUNT {
            return false;
        }
        chain_continuous(&filled.iter().map(|m| (*m).clone()).collect::<Vec<_>>())
            && chain_no_overlap(&filled.iter().map(|m| (*m).clone()).collect::<Vec<_>>())
    }

    /// 启动总时长 = 各段和。
    pub fn total_ms(&self) -> u64 {
        self.marks.iter().flatten().map(|m| m.duration()).sum()
    }

    /// 定位指纹。
    pub fn locator_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for m in self.marks.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (m.start_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (m.end_ms as u32);
            h ^= (m.end_ms >> 32) as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封链印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.chain_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_chain(&self) -> String {
        let mut out = String::new();
        for m in self.marks.iter().flatten() {
            out.push_str(&m.render_line());
            out.push('\n');
        }
        out.push_str(&format!("TOTAL|{}\n", self.total_ms()));
        if self.sealed {
            out.push_str("BOOTLOC-SEAL\n");
        }
        out
    }
}

impl Default for BootLocator {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四段连续链（0..800/800..2600/2600..5200/5200..6000）。
fn standard_locator() -> BootLocator {
    let mut l = BootLocator::new();
    let _ = l.locate(SG_BOOT, 0, 800);
    let _ = l.locate(SG_KERNEL, 800, 2600);
    let _ = l.locate(SG_LOGIN, 2600, 5200);
    let _ = l.locate(SG_INTERACT, 5200, 6000);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = BootLocator::new();
    if l.locate(SG_BOOT, 500, 300) == Err(BL_INVERT) {
        caught += 1;
    }
    // 注 2：重复定位
    let mut l = standard_locator();
    if l.locate(SG_BOOT, 0, 100) == Err(BL_OVERLAP) {
        caught += 1;
    }
    // 注 3：未知段
    let mut l = BootLocator::new();
    if l.locate(9, 0, 100) == Err(BL_UNK_SEG) {
        caught += 1;
    }
    // 注 4：断档链不过（kernel 起点跳过 boot 终点）
    let mut l = BootLocator::new();
    let _ = l.locate(SG_BOOT, 0, 800);
    let _ = l.locate(SG_KERNEL, 900, 2600);
    if !l.chain_ok() {
        caught += 1;
    }
    // 注 5：空链印
    let mut l = BootLocator::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 定位账；J2 定位语义；J3 定位指纹。
pub fn run_unx56181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56181");
    // J1-1 四段全定位
    let l = standard_locator();
    let ok = (0..SEG_COUNT).all(|i| l.mark(i).is_some()) && l.chain_ok();
    s.add("56181-J1-四段全定位", ok, if ok { "" } else { "定位账不全" });
    // J1-2 渲染导出齐
    let text = standard_locator().render_chain();
    let ok = text.contains("boot|0..800|dur800") && text.contains("interact|5200..6000|dur800") && text.contains("TOTAL|6000");
    s.add("56181-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总时长核（6000）
    let ok = l.total_ms() == 6_000;
    s.add("56181-J2-总时长核", ok, if ok { "" } else { "总时长漂移" });
    // J2-2 kernel 段时长（1800 最长）
    let ok = l.mark(SG_KERNEL).map(|m| m.duration() == 1800).unwrap_or(false);
    s.add("56181-J2-kernel时长", ok, if ok { "" } else { "段时长漂移" });
    // J2-3 无缝连续核
    let marks: Vec<SegmentMark> = standard_locator().marks.iter().flatten().map(|m| m.clone()).collect();
    let ok = chain_continuous(&marks);
    s.add("56181-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = chain_no_overlap(&marks);
    s.add("56181-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = SegmentMark { seg: SG_BOOT, start_ms: 500, end_ms: 300 };
    let ok = !seg_order_ok(&bad);
    s.add("56181-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J3-1 定位指纹确定
    let ok = standard_locator().locator_fp() == standard_locator().locator_fp();
    s.add("56181-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_locator();
    let fp0 = l2.locator_fp();
    if let Some(m) = l2.marks[0].as_mut() {
        m.end_ms += 1;
    }
    let ok = l2.locator_fp() != fp0;
    s.add("56181-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封链印
    let mut l3 = standard_locator();
    let ok = l3.seal() && l3.sealed();
    s.add("56181-J3-封链印", ok, if ok { "" } else { "封链印失败" });
    // J3-4 印后拒定
    let ok = l3.locate(SG_BOOT, 0, 1) == Err(BL_EMPTY_SEAL);
    s.add("56181-J3-印后拒定", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56181-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56181_all_checks_green() {
        let set = run_unx56181_checks();
        assert!(set.all_passed(), "UNX-F56181 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56181_chain_stable() {
        assert!(standard_locator().chain_ok());
    }
}
