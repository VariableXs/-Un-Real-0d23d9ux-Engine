//! UNX-F56009 Invariant TSC 时钟源校准件（0x60100 段）
//!
//! 锚点：UNX-F56009-J1 Invariant TSC 可用性探测（可用 × 不变性双旗）
//! 与 wall-clock 交叉校准（校准频率偏差 ≤100ppb 级口径，对应 1 秒窗
//! 偏差 <100ns）落账；J2 双系统时钟源口径映射（Varix 最优源 tsc-
//! invariant ↔ Windows 最优源 QPC 声明等价性）；J3 校准记录随批归档
//! （逐批记录可查、封账只读）三判据全绿。判据锚定 §AI-71.2 专题二①。
//! 域：CoRun·UNX-F2-B40（环境冻结域·时钟基座面）。
//!
//! 结构：时钟源闭集登记册（tsc-invariant/hpet/acpi-pm/wallclock 逐源
//! 可用性落账）+ 探测双旗核（可用旗 × 不变旗全真才可作基座）+ 交叉
//! 校准核（ticks × 参考纳秒 → 频率，偏差 ppb 阈值闸）+ 校准归档账 +
//! 注入核（未知源/零频率/超阈偏差/封账改写/未校准开采必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60100 独占 0x20）。
pub const SEG_BASE: u32 = 0x60100;
pub const TSC_SRC_UNK: u32 = SEG_BASE + 0x01; // 未知时钟源（非闭集）
pub const TSC_HZ_ZERO: u32 = SEG_BASE + 0x02; // 校准频率为零
pub const TSC_DRIFT: u32 = SEG_BASE + 0x03; // 校准偏差超阈（>100ppb）
pub const TSC_SEALED: u32 = SEG_BASE + 0x04; // 校准账已封账拒改
pub const TSC_CAL_MISS: u32 = SEG_BASE + 0x05; // 未校准即开采
pub const TSC_UNAVAIL: u32 = SEG_BASE + 0x06; // 基座源不可用（可用旗假）
pub const TSC_NOT_INVARIANT: u32 = SEG_BASE + 0x07; // 基座源非不变（不变旗假）

/// 时钟源闭集。
pub const SRC_TSC: usize = 0;
pub const SRC_HPET: usize = 1;
pub const SRC_ACPM: usize = 2;
pub const SRC_WALL: usize = 3;
pub const SRC_COUNT: usize = 4;
pub const SRC_NAMES: [&str; SRC_COUNT] =
    ["tsc-invariant", "hpet", "acpi-pm", "wallclock"];

/// 双系统最优源等价声明：Varix ↔ Windows。
pub const VARIX_BEST: &str = "tsc-invariant";
pub const WIN_BEST: &str = "query-performance-counter";

/// 偏差阈值（ppb；1 秒窗下 100ppb ≈ 100ns）。
pub const DRIFT_PPB_MAX: u64 = 100;

/// FNV-1a 32 位指纹（校准记录归档指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 单源可用账：源 × 可用旗 × 不变旗 × 分辨率(ns) 四元组。
#[derive(Clone)]
pub struct SourceEntry {
    pub src: usize,
    pub available: bool,
    pub invariant: bool,
    pub res_ns: u64,
}

impl SourceEntry {
    pub fn new(src: usize, available: bool, invariant: bool, res_ns: u64) -> Result<Self, u32> {
        if src >= SRC_COUNT {
            return Err(TSC_SRC_UNK);
        }
        Ok(SourceEntry { src, available, invariant, res_ns })
    }

    /// 基座资格：可用 × 不变双旗全真。
    pub fn qualifies_base(&self) -> bool {
        self.available && self.invariant
    }
}

/// 校准记录：交叉校准三元组（ticks × 参考纳秒 × 推得频率）。
#[derive(Clone)]
pub struct CalibRecord {
    pub tsc_ticks: u64,
    pub ref_ns: u64,
    pub hz: u64,
    /// 偏差（ppb，相对标称频率）。
    pub drift_ppb: u64,
}

impl CalibRecord {
    /// 交叉校准：hz = ticks × 1e9 / ref_ns；偏差相对标称频率。
    pub fn measure(ticks: u64, ref_ns: u64, nominal_hz: u64) -> Result<Self, u32> {
        if ticks == 0 || ref_ns == 0 {
            return Err(TSC_HZ_ZERO);
        }
        let hz = (ticks.wrapping_mul(1_000_000_000u64)) / ref_ns;
        if hz == 0 {
            return Err(TSC_HZ_ZERO);
        }
        let drift = if nominal_hz == 0 {
            u64::MAX
        } else {
            let d = (hz as i64 - nominal_hz as i64).abs();
            ((d as u64 * 1_000_000_000u64) / nominal_hz) as u64
        };
        Ok(CalibRecord { tsc_ticks: ticks, ref_ns: ref_ns, hz, drift_ppb: drift })
    }

    /// J1：偏差达标（≤100ppb）。
    pub fn within_threshold(&self) -> bool {
        self.drift_ppb <= DRIFT_PPB_MAX
    }

    /// 归档指纹（三元组折叠）。
    pub fn fingerprint(&self) -> u32 {
        fnv1a(
            format!("{}|{}|{}|{}", self.tsc_ticks, self.ref_ns, self.hz, self.drift_ppb)
                .as_bytes(),
        )
    }
}

/// 时钟基座总账：源账 + 校准归档 + 双旗门闸 + 封账。
pub struct ClockBook {
    pub sources: [Option<SourceEntry>; SRC_COUNT],
    pub records: Vec<CalibRecord>,
    pub base_src: usize,
    pub nominal_hz: u64,
    pub sealed: bool,
}

impl ClockBook {
    pub fn new(base_src: usize, nominal_hz: u64) -> Self {
        ClockBook {
            sources: [None, None, None, None],
            records: Vec::new(),
            base_src: base_src,
            nominal_hz,
            sealed: false,
        }
    }

    /// 源账落账。
    pub fn register_source(&mut self, e: SourceEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(TSC_SEALED);
        }
        if e.src >= SRC_COUNT {
            return Err(TSC_SRC_UNK);
        }
        let src = e.src;
        self.sources[src] = Some(e);
        Ok(())
    }

    /// J1：基座资格核（基座源已落账且双旗真）。
    pub fn base_qualified(&self) -> bool {
        if self.base_src >= SRC_COUNT {
            return false;
        }
        match self.sources[self.base_src].as_ref() {
            None => false,
            Some(e) => {
                if !e.qualifies_base() {
                    return false;
                }
                if self.base_src == SRC_TSC {
                    true
                } else {
                    e.res_ns > 0
                }
            }
        }
    }

    /// 交叉校准落账：基座未合格拒收；记录随批归档。
    pub fn calibrate(&mut self, ticks: u64, ref_ns: u64) -> Result<CalibRecord, u32> {
        if self.sealed {
            return Err(TSC_SEALED);
        }
        if !self.base_qualified() {
            if self.base_src >= SRC_COUNT {
                return Err(TSC_SRC_UNK);
            }
            match self.sources[self.base_src].as_ref() {
                None => return Err(TSC_UNAVAIL),
                Some(e) => {
                    if !e.available {
                        return Err(TSC_UNAVAIL);
                    }
                    return Err(TSC_NOT_INVARIANT);
                }
            }
        }
        let rec = CalibRecord::measure(ticks, ref_ns, self.nominal_hz)?;
        self.records.push(rec.clone());
        Ok(rec)
    }

    /// J3：未校准开采闸（归档空即未校准）。
    pub fn calibrated(&self) -> bool {
        !self.records.is_empty()
    }

    /// 最近一条校准记录。
    pub fn last_record(&self) -> Option<&CalibRecord> {
        self.records.last()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 双系统口径映射核：等价声明双端非空且基座名在闭集内。
pub fn mapping_declared_ok() -> bool {
    VARIX_BEST == SRC_NAMES[SRC_TSC]
        && !WIN_BEST.is_empty()
        && VARIX_BEST != WIN_BEST
}

/// 标准夹具：TSC 基座（3600 MHz 标称）+ 交叉校准一条。
/// ticks = 3_600_000_000 × 1000 / 1_000_000_000 → 1000ms 窗 3.6e12 ticks。
pub fn standard_clock() -> ClockBook {
    let mut b = ClockBook::new(SRC_TSC, 3_600_000_000u64);
    let _ = b.register_source(SourceEntry::new(SRC_TSC, true, true, 1).expect("tsc"));
    let _ = b.register_source(SourceEntry::new(SRC_WALL, true, false, 100_000).expect("wall"));
    let _ = b.register_source(SourceEntry::new(SRC_HPET, true, true, 69).expect("hpet"));
    // 1 秒窗：3.6e12 ticks / 1e9 ns = 3.6 GHz，偏差 0。
    let _ = b.calibrate(3_600_000_000_000u64, 1_000_000_000u64).expect("calib");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知时钟源（src=9）
    if SourceEntry::new(9, true, true, 1).err() == Some(TSC_SRC_UNK) {
        caught += 1;
    }
    // 2) 零参考纳秒（校准除零守闸）
    if CalibRecord::measure(3_600_000_000_000, 0, 3_600_000_000).err() == Some(TSC_HZ_ZERO) {
        caught += 1;
    }
    // 3) 超阈偏差（标称 3.6G，实测 3.7G → 偏差 2.78e10 ppb）
    let rec = CalibRecord::measure(3_700_000_000_000, 1_000_000_000, 3_600_000_000)
        .expect("drift rec");
    if !rec.within_threshold() {
        caught += 1;
    }
    // 4) 基座不可用（可用旗假）拒校准
    let mut b = ClockBook::new(SRC_TSC, 3_600_000_000);
    let _ = b.register_source(SourceEntry::new(SRC_TSC, false, true, 1).expect("tsc off"));
    if b.calibrate(3_600_000_000_000, 1_000_000_000).err() == Some(TSC_UNAVAIL) {
        caught += 1;
    }
    // 5) 基座非不变（不变旗假）拒校准
    let mut b2 = ClockBook::new(SRC_TSC, 3_600_000_000);
    let _ = b2.register_source(SourceEntry::new(SRC_TSC, true, false, 1).expect("tsc var"));
    if b2.calibrate(3_600_000_000_000, 1_000_000_000).err() == Some(TSC_NOT_INVARIANT) {
        caught += 1;
    }
    (caught, 5)
}

/// 校准归档渲染（随批归档可读面）。
pub fn render_records(b: &ClockBook) -> String {
    let mut out = String::new();
    for r in b.records.iter() {
        out.push_str(&format!(
            "CAL|ticks={}|ref_ns={}|hz={}|drift_ppb={}\n",
            r.tsc_ticks, r.ref_ns, r.hz, r.drift_ppb
        ));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56009");
    // J1-1 基座双旗资格核
    let b = standard_clock();
    let ok = b.base_qualified() && b.base_src == SRC_TSC;
    s.add("56009-J1-基座双旗资格", ok, if ok { "" } else { "基座资格缺" });
    // J1-2 交叉校准频率语义（1e12ns 窗 3.6e12 ticks = 3.6GHz）
    let rec = b.last_record().expect("rec").clone();
    let ok = rec.hz == 3_600_000_000u64 && rec.drift_ppb == 0;
    s.add("56009-J1-校准频率语义", ok, if ok { "" } else { "频率推算漂移" });
    // J1-3 偏差阈值语义（100ppb 达标，101ppb 超阈）
    let ok1 = CalibRecord::measure(3_600_000_000_360, 1_000_000_000, 3_600_000_000)
        .expect("p1")
        .within_threshold();
    let ok2 = !CalibRecord::measure(3_600_000_000_360_100, 1_000_000_000, 3_600_000_000)
        .expect("p2")
        .within_threshold();
    let ok = ok1 && ok2;
    s.add("56009-J1-偏差阈值语义", ok, if ok { "" } else { "阈值语义漂移" });
    // J2-1 双系统最优源等价声明
    let ok = mapping_declared_ok();
    s.add("56009-J2-双系统源等价", ok, if ok { "" } else { "等价声明缺" });
    // J2-2 源名闭集同源（tsc-invariant 在闭集 0 号位）
    let ok = SRC_NAMES[SRC_TSC] == VARIX_BEST;
    s.add("56009-J2-源名同源", ok, if ok { "" } else { "源名漂移" });
    // J3-1 校准记录随批归档（两批两记录逐条可查）
    let mut b2 = standard_clock();
    let _ = b2.calibrate(3_600_000_000_000, 1_000_000_000).expect("batch2");
    let ok = b2.records.len() == 2
        && b2.records[0].fingerprint() == b2.records[1].fingerprint();
    s.add("56009-J3-校准随批归档", ok, if ok { "" } else { "归档漂移" });
    // J3-2 未校准开采闸
    let mut b3 = ClockBook::new(SRC_TSC, 3_600_000_000);
    let _ = b3.register_source(SourceEntry::new(SRC_TSC, true, true, 1).expect("tsc"));
    let ok = !b3.calibrated();
    s.add("56009-J3-未校准闸", ok, if ok { "" } else { "未校准即采" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56009-J3-注入核必抓", ok, &detail);
    // J1-4 源账三源齐（tsc/wall/hpet 落账）
    let mut has = 0usize;
    for src in [SRC_TSC, SRC_WALL, SRC_HPET].iter() {
        if b.sources[*src].is_some() {
            has += 1;
        }
    }
    let ok = has == 3;
    s.add("56009-J1-源账齐", ok, if ok { "" } else { "源账缺" });
    // J1-5 归档渲染面齐（CAL 行逐条带漂移）
    let text = render_records(&standard_clock());
    let ok = text.lines().count() == 1 && text.contains("drift_ppb=0");
    s.add("56009-J1-归档渲染齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-3 非基座源不作基座（wallclock 双旗不真）
    let mut b4 = ClockBook::new(SRC_WALL, 3_600_000_000);
    let _ = b4.register_source(SourceEntry::new(SRC_WALL, true, false, 100_000).expect("wall"));
    let ok = !b4.base_qualified();
    s.add("56009-J2-非基座拒", ok, if ok { "" } else { "非不变源作基座" });
    // J3-4 封账冻结（封后校准拒）
    let mut b5 = standard_clock();
    b5.seal();
    let ok = b5.calibrate(3_600_000_000_000, 1_000_000_000).err() == Some(TSC_SEALED);
    s.add("56009-J3-封账冻结", ok, if ok { "" } else { "封账后可校准" });
    // J1-6 零 ticks 守闸
    let ok = CalibRecord::measure(0, 1_000_000_000, 3_600_000_000).err() == Some(TSC_HZ_ZERO);
    s.add("56009-J1-零ticks守闸", ok, if ok { "" } else { "零 ticks 穿透" });
    // J1-7 校准记录指纹稳定
    let r1 = CalibRecord::measure(3_600_000_000_000, 1_000_000_000, 3_600_000_000).expect("r1");
    let r2 = CalibRecord::measure(3_600_000_000_000, 1_000_000_000, 3_600_000_000).expect("r2");
    let ok = r1.fingerprint() == r2.fingerprint();
    s.add("56009-J1-记录指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56009_all_checks_green() {
        let set = run_unx56009_checks();
        assert!(set.all_passed(), "UNX-F56009 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56009_hz_math() {
        let r = CalibRecord::measure(3_600_000_000_000, 1_000_000_000, 3_600_000_000)
            .expect("rec");
        assert_eq!(r.hz, 3_600_000_000);
        assert_eq!(r.drift_ppb, 0);
    }
}
