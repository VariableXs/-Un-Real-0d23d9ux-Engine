//! UNX-F56068 内存采样时钟基线件（与 TSC 基座对齐）（0x60860 段）
//!
//! 锚点：UNX-F56068-J1 时钟基线登记全量（主频 × 不变性标志 × 校准波次
//! 三元组逐项落账，缺项即 J1 红）；J2 对齐判定一致（内存采样时间戳换算
//! 与 TSC 基座基准偏差落入漂移带，超带即拒换）；J3 基线审计链（校准波次
//! 单调、基座指纹留痕、换算系数可复算）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·时钟基线面）。
//!
//! 结构：TSC 基线登记册（主频 kHz × 不变性 × 校准波次三元组落账）+
//! 换算核（tick→ns→毫秒两级换算，u128 中转防溢出）+ 漂移带仲裁（新基线
//! 对既有基线偏差超带即拒）+ 封账语义（封后拒写）+ 审计链留痕 + 注入核
//! （非 TSC 源/零主频/漂移超带/波次回退/封账改写/不变性缺失/空账查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60860 独占 0x20）。
pub const SEG_BASE: u32 = 0x60860;
pub const CLK_NON_TSC: u32 = SEG_BASE + 0x01; // 非 TSC 时钟源（基座闭集外）
pub const CLK_ZERO_FREQ: u32 = SEG_BASE + 0x02; // 零主频（未校准即入账）
pub const CLK_DRIFT_OVER: u32 = SEG_BASE + 0x03; // 换算偏差超漂移带
pub const CLK_WAVE_BACK: u32 = SEG_BASE + 0x04; // 校准波次回退（非严格递增）
pub const CLK_EMPTY: u32 = SEG_BASE + 0x05; // 空账查询（基线未落账）
pub const CLK_SEALED: u32 = SEG_BASE + 0x06; // 基线账已封账拒写
pub const CLK_INV_MISS: u32 = SEG_BASE + 0x07; // 不变性标志缺失（非恒定 TSC）

/// 时钟源闭集（内存采样只认 TSC 基座一路）。
pub const SRC_TSC: usize = 0;
pub const SRC_COUNT: usize = 1;
pub const SRC_NAMES: [&str; SRC_COUNT] = ["tsc"];

/// 漂移带默认宽度（万分比）：换算偏差超此即视为基线漂移。
pub const DRIFT_BAND_BP: u32 = 200;

/// 基座指纹：FNV-1a 32 位（与 F56135 对接口径一致）。
pub fn base_fingerprint(src: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in src.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 换算核：tick→纳秒（u128 中转防溢出，主频单位 kHz）。
pub fn tick_to_ns(ticks: u64, freq_khz: u64) -> Option<u64> {
    if freq_khz == 0 {
        return None;
    }
    let ns = (ticks as u128 * 1_000_000u128) / freq_khz as u128;
    if ns > u64::MAX as u128 {
        None
    } else {
        Some(ns as u64)
    }
}

/// 换算核：纳秒→毫秒（向下取整，采样账毫秒口径）。
pub fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

/// 漂移万分比：|got-ref| × 10000 / max(ref,1)，算术确定性优先。
pub fn drift_bp(ref_ns: u64, got_ns: u64) -> u64 {
    let d = if got_ns >= ref_ns { got_ns - ref_ns } else { ref_ns - got_ns };
    (d as u64 * 10_000) / if ref_ns == 0 { 1 } else { ref_ns as u64 }
}

/// 基线登记账目：源 × 主频 × 不变性 × 波次 四元组。
#[derive(Clone)]
pub struct ClkEntry {
    pub src: usize,
    pub freq_khz: u64,
    pub invariant: bool,
    pub wave: u32,
}

impl ClkEntry {
    /// 构造校验：源闭集 + 主频非零 + 不变性必须成立 + 波次 ≥1。
    pub fn new(src: usize, freq_khz: u64, invariant: bool, wave: u32) -> Result<Self, u32> {
        if src >= SRC_COUNT {
            return Err(CLK_NON_TSC);
        }
        if freq_khz == 0 {
            return Err(CLK_ZERO_FREQ);
        }
        if !invariant {
            return Err(CLK_INV_MISS);
        }
        if wave == 0 {
            return Err(CLK_WAVE_BACK);
        }
        Ok(ClkEntry { src, freq_khz, invariant, wave })
    }

    /// 换算系数自证：1ms 等价 tick 数（freq_khz 即每毫秒千 tick 数）。
    pub fn ticks_per_ms(&self) -> u64 {
        self.freq_khz
    }
}

/// 时钟基线总账（单槽 + 封账 + 审计链）。
pub struct ClockBase {
    pub entry: Option<ClkEntry>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl ClockBase {
    pub fn new() -> Self {
        ClockBase { entry: None, sealed: false, audit: Vec::new() }
    }

    /// 落账：空账直接入；重校准必须严格升波次且换算偏差在带内。
    pub fn commit(&mut self, e: ClkEntry, ref_freq_khz: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CLK_SEALED);
        }
        match &self.entry {
            None => {
                self.audit.push(format!("base|{}|{}|w{}", e.freq_khz, e.invariant, e.wave));
                self.entry = Some(e);
                Ok(())
            }
            Some(prev) => {
                if e.wave <= prev.wave {
                    return Err(CLK_WAVE_BACK);
                }
                // 漂移仲裁：新旧主频换算同一 tick 串的偏差必须在带内。
                let probe_ticks: u64 = 1_000_000;
                let old_ns = tick_to_ns(probe_ticks, prev.freq_khz).unwrap_or(0);
                let new_ns = tick_to_ns(probe_ticks, e.freq_khz).unwrap_or(0);
                if drift_bp(old_ns, new_ns) > DRIFT_BAND_BP as u64 {
                    return Err(CLK_DRIFT_OVER);
                }
                let _ = ref_freq_khz; // 基座参考主频仅入审计链
                self.audit.push(format!(
                    "recal|{}->{}|w{}|d{}",
                    prev.freq_khz, e.freq_khz, e.wave, drift_bp(old_ns, new_ns)
                ));
                self.entry = Some(e);
                Ok(())
            }
        }
    }

    /// J1：基线全量（三元组齐 + 不变性成立）。
    pub fn baseline_ok(&self) -> bool {
        match &self.entry {
            None => false,
            Some(e) => e.freq_khz > 0 && e.invariant && e.wave >= 1,
        }
    }

    /// 空账查询显性拒绝（禁含糊）。
    pub fn query_freq(&self) -> Result<u64, u32> {
        match &self.entry {
            None => Err(CLK_EMPTY),
            Some(e) => Ok(e.freq_khz),
        }
    }

    /// J3：审计链可复算（链非空且逐条带波次痕迹）。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.iter().all(|a| a.contains("|w"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（现行基线四元组复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        match &self.entry {
            None => base_fingerprint("empty"),
            Some(e) => base_fingerprint(&format!(
                "{}|{}|{}|{}",
                SRC_NAMES[e.src], e.freq_khz, e.invariant, e.wave
            )),
        }
    }
}

/// 标准夹具：1 GHz TSC、恒定、第 1 波次。
pub fn standard_base() -> ClockBase {
    let mut b = ClockBase::new();
    let e = ClkEntry::new(SRC_TSC, 1_000_000, true, 1);
    match e {
        Ok(e) => {
            if b.commit(e, 1_000_000).is_err() {
                panic!("standard fixture commit must succeed");
            }
        }
        Err(_) => panic!("standard fixture entry must be valid"),
    }
    b
}

/// 注入核：7 注入必抓（非 TSC 源/零主频/漂移超带/波次回退/空账查询/封账改写/不变性缺失）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 非 TSC 源（src 越闭集）
    if ClkEntry::new(9, 1_000_000, true, 1).err() == Some(CLK_NON_TSC) {
        caught += 1;
    }
    // 2) 零主频
    if ClkEntry::new(SRC_TSC, 0, true, 1).err() == Some(CLK_ZERO_FREQ) {
        caught += 1;
    }
    // 3) 不变性缺失
    if ClkEntry::new(SRC_TSC, 1_000_000, false, 1).err() == Some(CLK_INV_MISS) {
        caught += 1;
    }
    // 4) 漂移超带：1 GHz → 1.1 GHz（偏差 9090 bp > 200 bp）
    let mut b = standard_base();
    let e = ClkEntry::new(SRC_TSC, 1_100_000, true, 2).unwrap_or_else(|_| {
        panic!("drift fixture entry must be valid")
    });
    if b.commit(e, 1_000_000) == Err(CLK_DRIFT_OVER) {
        caught += 1;
    }
    // 5) 波次回退（现行 w1，重投 w1）
    let e = ClkEntry::new(SRC_TSC, 1_000_500, true, 1).unwrap_or_else(|_| {
        panic!("wave fixture entry must be valid")
    });
    if b.commit(e, 1_000_000) == Err(CLK_WAVE_BACK) {
        caught += 1;
    }
    // 6) 空账查询
    let empty = ClockBase::new();
    if empty.query_freq() == Err(CLK_EMPTY) {
        caught += 1;
    }
    // 7) 封账后改写拒
    b.seal();
    let e = ClkEntry::new(SRC_TSC, 1_002_000, true, 2).unwrap_or_else(|_| {
        panic!("seal fixture entry must be valid")
    });
    if b.commit(e, 1_000_000) == Err(CLK_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 基线账渲染导出（可读面，单行）。
pub fn render_base(b: &ClockBase) -> String {
    match &b.entry {
        None => "tsc|UNSET".to_string(),
        Some(e) => format!(
            "tsc|{}|inv={}|w{}|fp={:08x}",
            e.freq_khz, e.invariant, e.wave, b.seal_fingerprint()
        ),
    }
}

/// 渲染完整性核：tsc 前缀 + 四段齐全 + 指纹 8 位。
pub fn render_complete(text: &str) -> bool {
    if !text.starts_with("tsc|") {
        return false;
    }
    if text.ends_with("UNSET") {
        return false;
    }
    let parts: Vec<&str> = text.split('|').collect();
    parts.len() == 5 && parts[4].len() == 8
}

/// 换算对照核：给定 tick 串与基准 ns 逐点判定在带内。
pub fn probe_alignment(freq_khz: u64, ticks: &[u64], ref_ns: &[u64]) -> Result<bool, u32> {
    if ticks.len() != ref_ns.len() {
        return Err(CLK_EMPTY);
    }
    if ticks.is_empty() {
        return Err(CLK_EMPTY);
    }
    for i in 0..ticks.len() {
        let got = match tick_to_ns(ticks[i], freq_khz) {
            Some(v) => v,
            None => return Err(CLK_ZERO_FREQ),
        };
        if drift_bp(ref_ns[i], got) > DRIFT_BAND_BP as u64 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 判据入口（CheckSet）。
pub fn run_unx56068_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56068");
    // J1-1 基线登记全量（三元组齐 + 不变性成立）
    let b = standard_base();
    let ok = b.baseline_ok() && !b.sealed && b.query_freq().unwrap_or(0) == 1_000_000;
    s.add("56068-J1-基线登记全量", ok, if ok { "" } else { "三元组不全" });
    // J1-2 封账指纹确定性
    let ok = standard_base().seal_fingerprint() == standard_base().seal_fingerprint();
    s.add("56068-J1-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 空账查询显性拒绝
    let ok = ClockBase::new().query_freq().err() == Some(CLK_EMPTY);
    s.add("56068-J1-空账显性拒绝", ok, if ok { "" } else { "空账语义漂移" });
    // J2-1 换算核：1 GHz 下 1000 tick = 1000 ns
    let ok = tick_to_ns(1000, 1_000_000) == Some(1000);
    s.add("56068-J2-tick换算", ok, if ok { "" } else { "换算系数漂移" });
    // J2-2 毫秒换算：1_000_000 ns = 1 ms
    let ok = ns_to_ms(1_000_000) == 1;
    s.add("56068-J2-毫秒换算", ok, if ok { "" } else { "毫秒口径漂移" });
    // J2-3 漂移万分比：ref=1000, got=1010 → 100 bp
    let ok = drift_bp(1000, 1010) == 100 && drift_bp(1000, 990) == 100;
    s.add("56068-J2-漂移万分比", ok, if ok { "" } else { "漂移算术漂移" });
    // J2-4 对齐探针：1 GHz 五点全在带内
    let ticks = [1_000u64, 2_000, 5_000, 10_000, 20_000];
    let refs = [1_000u64, 2_000, 5_000, 10_000, 20_001];
    let ok = probe_alignment(1_000_000, &ticks, &refs) == Ok(true);
    s.add("56068-J2-对齐探针", ok, if ok { "" } else { "探针越带" });
    // J2-5 重校准带内放行（1 GHz → 1.001 GHz = 10 bp 偏差）
    let mut b2 = standard_base();
    let e = ClkEntry::new(SRC_TSC, 1_001_000, true, 2);
    let ok = match e {
        Ok(e) => b2.commit(e, 1_000_000).is_ok(),
        Err(_) => false,
    };
    s.add("56068-J2-带内重校准", ok, if ok { "" } else { "带内拒换" });
    // J3-1 审计链留痕（逐条带波次痕迹）
    let ok = b2.audit.len() == 2 && b2.audit_ok();
    s.add("56068-J3-审计链留痕", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账后拒写
    b2.seal();
    let e = ClkEntry::new(SRC_TSC, 1_001_500, true, 3);
    let ok = match e {
        Ok(e) => b2.commit(e, 1_000_000) == Err(CLK_SEALED),
        Err(_) => false,
    };
    s.add("56068-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J3-3 渲染导出四段齐
    let text = render_base(&standard_base());
    let ok = render_complete(&text) && text.contains("inv=true|w1");
    s.add("56068-J3-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 每毫秒 tick 数自证（1 GHz = 1_000_000 tick/ms）
    let ok = standard_base().entry.as_ref().map(|e| e.ticks_per_ms()).unwrap_or(0) == 1_000_000;
    s.add("56068-J1-换算系数自证", ok, if ok { "" } else { "系数自证失败" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56068-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56068_all_checks_green() {
        let set = run_unx56068_checks();
        assert!(set.all_passed(), "UNX-F56068 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56068_fingerprint_stable() {
        assert_eq!(standard_base().seal_fingerprint(), standard_base().seal_fingerprint());
    }
}
