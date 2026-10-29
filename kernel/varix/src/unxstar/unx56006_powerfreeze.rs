//! UNX-F56006 电源计划与 CPU 频率策略冻结件（0x600A0 段）
//!
//! 锚点：UNX-F56006-J1 双系统电源计划逐档位登记（省电/平衡/性能三档
//! × governor × 睿频 × 驻留比逐项落账）；J2 频率策略导出（governors/
//! 睿频开关/驻留比三要素闭集导出）；J3 采集窗频率漂移 >1% 即标记复测
//! （×5 注入全检出）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·电源策略面）。
//!
//! 结构：档位登记册（三档闭集逐档落账，同档重复拒）+ 策略冻结（封账
//! 拒改）+ 频率漂移核（基线 × 采样万分比 >100 标记）+ 复测标记账 +
//! 注入核（未知档位/非法 governor/越界驻留/封账改写/漂移复测必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x600A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x600A0;
pub const PWR_UNK_GEAR: u32 = SEG_BASE + 0x01; // 未知档位（非三档闭集）
pub const PWR_GOV_BAD: u32 = SEG_BASE + 0x02; // governor 非闭集值
pub const PWR_RANGE_BAD: u32 = SEG_BASE + 0x03; // 驻留比越界（非 0..=10000 万分比）
pub const PWR_SEALED: u32 = SEG_BASE + 0x04; // 策略账已封账拒改
pub const PWR_DUP_GEAR: u32 = SEG_BASE + 0x05; // 同档位重复登记
pub const PWR_TURBO_MISS: u32 = SEG_BASE + 0x06; // 睿频开关缺省未声明
pub const PWR_DRIFT: u32 = SEG_BASE + 0x07; // 采集窗频率漂移超阈

/// 电源档位闭集。
pub const GEAR_SAVE: usize = 0;
pub const GEAR_BALANCED: usize = 1;
pub const GEAR_PERF: usize = 2;
pub const GEAR_COUNT: usize = 3;
pub const GEAR_NAMES: [&str; GEAR_COUNT] = ["save", "balanced", "performance"];

/// governor 闭集。
pub const GOVS: [&str; 3] = ["powersave", "schedutil", "performance"];

/// 频率漂移阈值（万分比：1% = 100）。
pub const DRIFT_PBP: u32 = 100;

/// 万分比漂移核：|sample - base| × 10000 / base。
pub fn drift_pbp(base_mhz: u32, sample_mhz: u32) -> u32 {
    if base_mhz == 0 {
        return u32::MAX;
    }
    let d = (sample_mhz as i64 - base_mhz as i64).abs();
    ((d * 10_000) / base_mhz as i64) as u32
}

/// 档位策略账目：档位 × governor × 睿频 × 驻留比(万分比) 四元组。
#[derive(Clone)]
pub struct GearEntry {
    pub gear: usize,
    pub gov: String,
    pub turbo_on: bool,
    /// 驻留比（万分比 0..=10000）。
    pub residency_pbp: u32,
}

impl GearEntry {
    pub fn new(gear: usize, gov: &str, turbo_on: bool, residency_pbp: u32) -> Result<Self, u32> {
        if gear >= GEAR_COUNT {
            return Err(PWR_UNK_GEAR);
        }
        let mut gov_ok = false;
        for g in GOVS.iter() {
            if *g == gov {
                gov_ok = true;
            }
        }
        if !gov_ok {
            return Err(PWR_GOV_BAD);
        }
        if residency_pbp > 10_000 {
            return Err(PWR_RANGE_BAD);
        }
        Ok(GearEntry {
            gear,
            gov: gov.to_string(),
            turbo_on,
            residency_pbp,
        })
    }
}

/// 电源策略账：三档逐档落账 + 频率基线 + 复测标记账。
pub struct PowerBook {
    pub entries: Vec<GearEntry>,
    pub base_mhz: u32,
    pub flags: Vec<(u32, u32)>,
    pub sealed: bool,
}

impl PowerBook {
    pub fn new(base_mhz: u32) -> Self {
        PowerBook { entries: Vec::new(), base_mhz, flags: Vec::new(), sealed: false }
    }

    /// 逐档落账：同档重复拒 + 封账拒写。
    pub fn register(&mut self, e: GearEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(PWR_SEALED);
        }
        for p in self.entries.iter() {
            if p.gear == e.gear {
                return Err(PWR_DUP_GEAR);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 某档现行账。
    pub fn current(&self, gear: usize) -> Option<&GearEntry> {
        for p in self.entries.iter() {
            if p.gear == gear {
                return Some(p);
            }
        }
        None
    }

    /// J1：三档全量核（每档四元组齐）。
    pub fn full_ok(&self) -> bool {
        if self.entries.len() != GEAR_COUNT {
            return false;
        }
        for g in 0..GEAR_COUNT {
            match self.current(g) {
                None => return false,
                Some(e) => {
                    let mut gov_ok = false;
                    for gv in GOVS.iter() {
                        if *gv == e.gov {
                            gov_ok = true;
                        }
                    }
                    if !gov_ok || e.residency_pbp > 10_000 {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// J3：采集窗频率采样——漂移 >1% 标记复测，返回是否被标记。
    pub fn observe_freq(&mut self, sample_mhz: u32) -> bool {
        let d = drift_pbp(self.base_mhz, sample_mhz);
        if d > DRIFT_PBP {
            self.flags.push((sample_mhz, d));
            return true;
        }
        false
    }

    /// 复测标记计数。
    pub fn flag_count(&self) -> usize {
        self.flags.len()
    }

    /// 封账（封后策略不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 策略导出（三要素闭集逐档一行）。
pub fn render_policy(b: &PowerBook) -> String {
    let mut out = String::new();
    for g in 0..GEAR_COUNT {
        match b.current(g) {
            Some(e) => {
                out.push_str(&format!(
                    "{}|gov={}|turbo={}|res={}",
                    GEAR_NAMES[g], e.gov, e.turbo_on, e.residency_pbp
                ));
                out.push('\n');
            }
            None => {
                out.push_str(&format!("{}|UNDECLARED\n", GEAR_NAMES[g]));
            }
        }
    }
    out
}

/// 标准夹具：三档全落账（基准频率 3600 MHz，性能档睿频开）。
pub fn standard_power() -> PowerBook {
    let mut b = PowerBook::new(3600);
    let e1 = GearEntry::new(GEAR_SAVE, "powersave", false, 3_000).expect("save");
    let e2 = GearEntry::new(GEAR_BALANCED, "schedutil", true, 7_000).expect("balanced");
    let e3 = GearEntry::new(GEAR_PERF, "performance", true, 9_500).expect("perf");
    b.register(e1).expect("reg1");
    b.register(e2).expect("reg2");
    b.register(e3).expect("reg3");
    b
}

/// 注入核：5 类非法操作 + ×5 频率漂移标记必检出。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知档位（gear=7）
    if GearEntry::new(7, "performance", true, 9_000).err() == Some(PWR_UNK_GEAR) {
        caught += 1;
    }
    // 2) 非法 governor
    if GearEntry::new(GEAR_PERF, "turbo-extreme", true, 9_000).err() == Some(PWR_GOV_BAD) {
        caught += 1;
    }
    // 3) 驻留比越界（11000 > 10000）
    if GearEntry::new(GEAR_PERF, "performance", true, 11_000).err() == Some(PWR_RANGE_BAD) {
        caught += 1;
    }
    // 4) 同档重复登记
    let mut b = standard_power();
    if b.register(GearEntry::new(GEAR_PERF, "performance", true, 9_000).expect("dup"))
        == Err(PWR_DUP_GEAR)
    {
        caught += 1;
    }
    // 5) 封账后改写
    let mut b2 = standard_power();
    b2.seal();
    if b2.register(GearEntry::new(GEAR_SAVE, "powersave", true, 1_000).expect("sealed"))
        == Err(PWR_SEALED)
    {
        caught += 1;
    }
    // ×5 漂移注入：4 常态 + 1 超阈（3900 = 8.33% 漂移必标记）
    let mut b3 = standard_power();
    let cases = [3600u32, 3610, 3620, 3630, 3900];
    let mut flagged = 0usize;
    for m in cases.iter() {
        if b3.observe_freq(*m) {
            flagged += 1;
        }
    }
    // 3600/3610/3620/3630 漂移 0/27/55/83 万分比均 ≤100 不标；3900 漂移 833 必标。
    if flagged == 1 && b3.flag_count() == 1 {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56006_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56006");
    // J1-1 三档全量核
    let b = standard_power();
    let ok = b.full_ok() && !b.sealed;
    s.add("56006-J1-三档全量落账", ok, if ok { "" } else { "三档不全" });
    // J1-2 驻留比语义（性能档 9500 = 95%）
    let perf = b.current(GEAR_PERF).expect("perf");
    let ok = perf.residency_pbp == 9_500;
    s.add("56006-J1-驻留比语义", ok, if ok { "" } else { "驻留比漂移" });
    // J1-3 governor 闭集导出
    let bal = b.current(GEAR_BALANCED).expect("bal");
    let ok = bal.gov == "schedutil";
    s.add("56006-J1-governor闭集", ok, if ok { "" } else { "governor 越集" });
    // J2-1 策略导出面齐（三行无 UNDECLARED）
    let text = render_policy(&standard_power());
    let ok = text.lines().count() == GEAR_COUNT
        && !text.contains("UNDECLARED")
        && text.contains("performance|gov=performance|turbo=true");
    s.add("56006-J2-策略导出面齐", ok, if ok { "" } else { "导出面漂移" });
    // J2-2 未声明档显性化
    let empty = PowerBook::new(3600);
    let ok = render_policy(&empty).contains("save|UNDECLARED");
    s.add("56006-J2-未声明显性化", ok, if ok { "" } else { "缺档被吞" });
    // J3-1 频率漂移阈值语义（万分比 1% = 100）
    let ok = drift_pbp(3600, 3610) == 27 && drift_pbp(3600, 3900) == 833;
    s.add("56006-J3-漂移万分比语义", ok, if ok { "" } else { "漂移算式漂移" });
    // J3-2 ×5 注入全检出（注入核内含漂移标记必抓）
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56006-J3-注入核必抓", ok, &detail);
    // J3-3 复测标记账留痕（标记含采样值与万分比）
    let mut b3 = standard_power();
    let _ = b3.observe_freq(4100); // 漂移 1388 万分比
    let ok = b3.flag_count() == 1 && b3.flags[0].1 == 1_388;
    s.add("56006-J3-复测标记留痕", ok, if ok { "" } else { "标记账缺" });
    // J1-4 常态采样零标记（3610 等低漂移不误报）
    let mut b4 = standard_power();
    let mut flagged = 0usize;
    for m in [3600u32, 3610, 3620, 3630].iter() {
        if b4.observe_freq(*m) {
            flagged += 1;
        }
    }
    let ok = flagged == 0 && b4.flag_count() == 0;
    s.add("56006-J1-常态零误报", ok, if ok { "" } else { "常态误报" });
    // J1-5 封账只读
    let mut b5 = standard_power();
    b5.seal();
    let ok = b5.observe_freq(3600) == false && b5.register(
        GearEntry::new(GEAR_SAVE, "powersave", false, 1_000).expect("sealed"),
    ) == Err(PWR_SEALED);
    s.add("56006-J1-封账只读", ok, if ok { "" } else { "封账后可写" });
    // J2-3 档位名渲染共用同源
    let ok = GEAR_NAMES[GEAR_BALANCED] == "balanced" && GOVS.contains(&"powersave");
    s.add("56006-J2-命名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-6 睿频开关显性（性能档 turbo=true 必须落账）
    let ok = standard_power().current(GEAR_PERF).expect("p").turbo_on;
    s.add("56006-J1-睿频开关显性", ok, if ok { "" } else { "睿频未声明" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56006_all_checks_green() {
        let set = run_unx56006_checks();
        assert!(set.all_passed(), "UNX-F56006 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56006_drift_math() {
        assert_eq!(drift_pbp(3600, 3900), 833);
        assert_eq!(drift_pbp(3600, 3630), 83);
    }
}
