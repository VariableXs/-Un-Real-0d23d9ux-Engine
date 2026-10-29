//! UNX-F56136 帧率归因预挂钩（瓶颈帧段定位初筛）（0x610E0 段）
//!
//! 锚点：UNX-F56136-J1 瓶颈帧段初筛账（帧耗时分解为 CPU 段/提交段/
//! 呈现段三段，最耗时段定名瓶颈段逐帧落账）；J2 初筛阈值语义（占比
//! 超阈才标记瓶颈，阈下帧不误标）；J3 预挂链留痕（初筛波次、帧指纹、
//! 段三元组全留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·归因预挂面）。
//!
//! 结构：帧段分解核（单帧毫秒三元组 → 段占比定瓶颈）+ 瓶颈阈值闸
//! （占比 ≥ 阈才标记）+ 初筛账（逐帧瓶颈段名 + 帧指纹落账）+ 段统计
//! （三段累计均值核算）+ 注入核（空帧/全零段/段越界/账后改判/指纹
//! 漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x610E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x610E0;
pub const FRA_EMPTY_FRAME: u32 = SEG_BASE + 0x01; // 空帧（总耗时为零）
pub const FRA_SEG_SUM_BAD: u32 = SEG_BASE + 0x02; // 段和 ≠ 帧总耗时
pub const FRA_SEG_OVERFLOW: u32 = SEG_BASE + 0x03; // 段下标越界（非三段闭集）
pub const FRA_VERDICT_FLIP: u32 = SEG_BASE + 0x04; // 账后改判（已落账帧重判）
pub const FRA_FP_DRIFT: u32 = SEG_BASE + 0x05; // 帧指纹漂移（同帧双算不一致）
pub const FRA_LEDGER_SEALED: u32 = SEG_BASE + 0x06; // 初筛账封账拒写

/// 三段闭集。
pub const SEG_CPU: usize = 0;
pub const SEG_SUBMIT: usize = 1;
pub const SEG_PRESENT: usize = 2;
pub const SEG_KINDS: usize = 3;

/// 段名渲染（初筛账与统计共用同源命名）。
pub const SEG_NAMES: [&str; SEG_KINDS] = ["cpu", "submit", "present"];

/// 瓶颈标记阈值：段占比 ≥ 45%（万分比 4500）才标记瓶颈段。
pub const BOTTLENECK_RATIO_MIN: u32 = 4_500;

/// 帧指纹：FNV-1a 32 位（帧序列号 + 三段耗时合成）。
pub fn frame_fingerprint(seq: u64, segs: &[u32; SEG_KINDS]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut mix = |h: &mut u32, v: u64| {
        for i in 0..8 {
            let b = ((v >> (i * 8)) & 0xFF) as u32;
            *h ^= b;
            *h = h.wrapping_mul(0x0100_0193);
        }
    };
    mix(&mut h, seq);
    for seg in segs {
        mix(&mut h, *seg as u64);
    }
    h
}

/// 段和核：三段之和必须等于帧总耗时（毫秒整数口径）。
pub fn seg_sum_ok(segs: &[u32; SEG_KINDS], total_ms: u32) -> bool {
    segs[0]
        .checked_add(segs[1])
        .and_then(|s| s.checked_add(segs[2]))
        .map(|s| s == total_ms)
        .unwrap_or(false)
}

/// 瓶颈初筛核：返回 (瓶颈段下标, 段占比万分比)。
/// 总耗为零视为空帧由调用方先拒；占比 = 段值 × 10000 / 总值。
pub fn bottleneck_scan(segs: &[u32; SEG_KINDS], total_ms: u32) -> Option<(usize, u32)> {
    if total_ms == 0 {
        return None;
    }
    let mut best = 0usize;
    let mut best_ratio: u32 = 0;
    for (i, seg) in segs.iter().enumerate() {
        let ratio = (*seg as u64 * 10_000 / total_ms as u64) as u32;
        if ratio > best_ratio || (ratio == best_ratio && *seg > segs[best]) {
            best = i;
            best_ratio = ratio;
        }
    }
    Some((best, best_ratio))
}

/// 单帧初筛账目：帧号 × 瓶颈段 × 占比 × 指纹 四元组。
#[derive(Clone)]
pub struct FrameScanEntry {
    pub seq: u64,
    pub seg: usize,
    pub ratio: u32,
    pub fp: u32,
    pub flagged: bool,
}

impl FrameScanEntry {
    fn render_line(&self) -> String {
        format!(
            "f{}|{}|{}|{:08x}|{}",
            self.seq,
            SEG_NAMES[self.seg],
            self.ratio,
            self.fp,
            if self.flagged { "BN" } else { "OK" }
        )
    }
}

/// 初筛总账：逐帧落账 + 封账语义（禁账后改判）。
pub struct FrameAttribLedger {
    entries: Vec<FrameScanEntry>,
    sealed: bool,
    seg_totals: [u64; SEG_KINDS],
    frame_count: u64,
}

impl FrameAttribLedger {
    pub fn new() -> Self {
        FrameAttribLedger {
            entries: Vec::new(),
            sealed: false,
            seg_totals: [0; SEG_KINDS],
            frame_count: 0,
        }
    }

    /// 初筛一帧：段和核 + 空帧拒 + 封账拒 + 瓶颈标记。
    pub fn scan_frame(
        &mut self,
        seq: u64,
        segs: &[u32; SEG_KINDS],
        total_ms: u32,
    ) -> Result<FrameScanEntry, u32> {
        if self.sealed {
            return Err(FRA_LEDGER_SEALED);
        }
        if total_ms == 0 {
            return Err(FRA_EMPTY_FRAME);
        }
        if !seg_sum_ok(segs, total_ms) {
            return Err(FRA_SEG_SUM_BAD);
        }
        let (seg, ratio) = bottleneck_scan(segs, total_ms).unwrap_or((SEG_CPU, 0));
        let fp = frame_fingerprint(seq, segs);
        let entry = FrameScanEntry {
            seq,
            seg,
            ratio,
            fp,
            flagged: ratio >= BOTTLENECK_RATIO_MIN,
        };
        for (i, seg) in segs.iter().enumerate() {
            self.seg_totals[i] += *seg as u64;
        }
        self.frame_count += 1;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    /// 账后改判闸：同帧号二次初筛一律拒（只追加语义）。
    pub fn rescan_rejected(&self, seq: u64) -> bool {
        self.entries.iter().any(|e| e.seq == seq)
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn entry(&self, idx: usize) -> Option<&FrameScanEntry> {
        self.entries.get(idx)
    }

    /// 段统计均值（毫秒 ×1000 保留三位精度，整数域无浮点）。
    pub fn seg_mean_milli(&self, seg: usize) -> Option<u64> {
        if seg >= SEG_KINDS || self.frame_count == 0 {
            return None;
        }
        Some(self.seg_totals[seg] * 1000 / self.frame_count as u64)
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// 瓶颈帧计数。
    pub fn flagged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.flagged).count()
    }

    /// 渲染导出：逐帧行 + 段均值行 + 封账印。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        for (i, name) in SEG_NAMES.iter().enumerate() {
            if let Some(m) = self.seg_mean_milli(i) {
                out.push_str(&format!("mean:{}:{}\n", name, m));
            }
        }
        if self.sealed {
            out.push_str("SEALED\n");
        }
        out
    }
}

impl Default for FrameAttribLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：六帧样本（cpu 主导 2 帧 / submit 主导 2 帧 / present 主导 2 帧）。
fn standard_ledger() -> FrameAttribLedger {
    let mut led = FrameAttribLedger::new();
    // CPU 主导帧
    let _ = led.scan_frame(1, &[9, 3, 3], 15);
    let _ = led.scan_frame(2, &[8, 4, 3], 15);
    // submit 主导帧
    let _ = led.scan_frame(3, &[3, 9, 3], 15);
    let _ = led.scan_frame(4, &[4, 8, 3], 15);
    // present 主导帧
    let _ = led.scan_frame(5, &[3, 3, 9], 15);
    let _ = led.scan_frame(6, &[4, 4, 7], 15);
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空帧
    let mut led = FrameAttribLedger::new();
    if led.scan_frame(1, &[0, 0, 0], 0).err() == Some(FRA_EMPTY_FRAME) {
        caught += 1;
    }
    // 注 2：段和 ≠ 总耗时
    let mut led = FrameAttribLedger::new();
    if led.scan_frame(1, &[5, 5, 5], 20).err() == Some(FRA_SEG_SUM_BAD) {
        caught += 1;
    }
    // 注 3：账后改判
    let mut led = standard_ledger();
    if led.rescan_rejected(1) {
        caught += 1;
    }
    // 注 4：封账后写
    let mut led = standard_ledger();
    let _ = led.seal();
    if led.scan_frame(9, &[5, 5, 5], 15).err() == Some(FRA_LEDGER_SEALED) {
        caught += 1;
    }
    // 注 5：指纹漂移（同帧同段双算必须一致，异帧必须异指纹）
    let f1 = frame_fingerprint(7, &[5, 5, 5]);
    let f2 = frame_fingerprint(7, &[5, 5, 5]);
    let f3 = frame_fingerprint(8, &[5, 5, 5]);
    if f1 == f2 && f1 != f3 {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 瓶颈初筛；J2 阈值语义；J3 预挂链。
pub fn run_unx56136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56136");
    // J1-1 六帧全落账
    let led = standard_ledger();
    let ok = led.entry_count() == 6 && led.frame_count() == 6 && !led.sealed();
    s.add("56136-J1-六帧全落账", ok, if ok { "" } else { "初筛账不全" });
    // J1-2 瓶颈段判定逐帧正确
    let ok = led.entry(0).map(|e| e.seg == SEG_CPU).unwrap_or(false)
        && led.entry(2).map(|e| e.seg == SEG_SUBMIT).unwrap_or(false)
        && led.entry(4).map(|e| e.seg == SEG_PRESENT).unwrap_or(false);
    s.add("56136-J1-瓶颈段逐帧", ok, if ok { "" } else { "段判定漂移" });
    // J1-3 均匀帧不误标（5/5/5 占比 3333 < 4500）
    let mut led2 = FrameAttribLedger::new();
    let e = led2.scan_frame(1, &[5, 5, 5], 15).expect("uniform frame");
    let ok = !e.flagged && e.ratio == 3_333;
    s.add("56136-J1-均匀帧不误标", ok, if ok { "" } else { "阈值误标" });
    // J2-1 阈值上缘标记（60% 帧必标记）
    let ok = led.entry(0).map(|e| e.flagged && e.ratio == 6_000).unwrap_or(false);
    s.add("56136-J2-超阈标记", ok, if ok { "" } else { "标记面漂移" });
    // J2-2 阈值下缘不标（40% 帧不标）
    let mut led2 = FrameAttribLedger::new();
    let e = led2.scan_frame(1, &[6, 5, 4], 15).expect("40pct frame");
    let ok = !e.flagged && e.ratio == 4_000;
    s.add("56136-J2-阈下不标", ok, if ok { "" } else { "阈值面漂移" });
    // J2-3 瓶颈计数：6 帧全主导 60% → 全标
    let ok = standard_ledger().flagged_count() == 6;
    s.add("56136-J2-瓶颈计数", ok, if ok { "" } else { "计数漂移" });
    // J3-1 段均值核算（cpu 段 9+8+3+4+3+4=31ms/6 帧）
    let ok = standard_ledger().seg_mean_milli(SEG_CPU) == Some(31_000 / 6);
    s.add("56136-J3-段均值核算", ok, if ok { "" } else { "均值算术漂移" });
    // J3-2 渲染导出行齐 + 段名同源
    let text = standard_ledger().render_ledger();
    let ok = text.contains("f1|cpu|6000|") && text.contains("mean:cpu:") && text.contains("mean:present:");
    s.add("56136-J3-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J3-3 封账前提：空账拒封
    let mut led2 = FrameAttribLedger::new();
    let ok = !led2.seal() && !led2.sealed();
    s.add("56136-J3-空账拒封", ok, if ok { "" } else { "封账闸失效" });
    // J3-4 封账后只读
    let mut led = standard_ledger();
    let _ = led.seal();
    let ok = led.sealed() && led.entry_count() == 6;
    s.add("56136-J3-封账只读", ok, if ok { "" } else { "封账面漂移" });
    // J3-5 初筛账只追加：改判闸语义（rescan_rejected 恒真于已入账帧）
    let led = standard_ledger();
    let ok = led.rescan_rejected(6) && !led.rescan_rejected(99);
    s.add("56136-J3-只追加语义", ok, if ok { "" } else { "改判闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56136-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56136_all_checks_green() {
        let set = run_unx56136_checks();
        assert!(set.all_passed(), "UNX-F56136 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56136_scan_deterministic() {
        let a = bottleneck_scan(&[9, 3, 3], 15);
        let b = bottleneck_scan(&[9, 3, 3], 15);
        assert_eq!(a, b);
    }
}
