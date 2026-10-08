//! F022 深化批次四 · 时区规则表驱动面（compatstar2/deep3 · G-A-22）。
//!
//! 批次一~三深化覆盖单调钟/QPC/计时器周期治理/FILETIME 纪元换算/系统时间
//! 转换矩阵；本批补齐主册【功能定义】「全语义对齐」的表驱动/账本面：
//! DST 规则表（定长 16 条：生效年区间/起始月日时/结束月日时/偏移增量分钟）、
//! 规则查找（给定 civil 日期时间 → 命中规则与偏移，无命中如实走标准时）、
//! 年度转换点生成（给定年份输出两次切换时刻，定长 2 条）、切换歧义/缺口
//! 标记（回拨重复小时/前跳缺失小时两类账）。
//!
//! 判据对账：主册 G-A-22（tzdata 换算 10 时区 round-trip 全对）+ MS
//! GetTimeZoneInformation/TIME_ZONE_INFORMATION 文档语义（标准时/DST 偏移
//! 增量、切换日期模型）。规则样例取真实 tzdata 日期：US 2007 规则 2024 年
//! 3/10 02:00 起、11/3 02:00 止；EU 统一规则 2024 年 3/31 02:00 墙钟起、
//! 10/27 03:00 墙钟止；AU-NSW 2024-25 跨年规则。与主层 timefam.rs、
//! deep2/f022e.rs（civil 历法转换）语义面互补不重叠。
//!
//! civil 键粒度说明：键 = 月×10000+日×100+时（墙钟，切换不跨日；30 分钟
//! 级切换时区超出键粒度，不在此域建模）。零堆纪律：定长数组 + &'static str。

use crate::checks::CheckSet;

/// 规则表容量。
pub const MAX_DST_RULES: usize = 16;
/// 切换点定长输出（每年两切：进/出 DST）。
pub const TRANSITION_POINTS: usize = 2;

/// 一条 DST 规则（生效年区间 + 两次切换 + 增量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DstRule {
    pub first_year: i32,
    pub last_year: i32,
    /// 起始切换 civil 键（进入 DST 的月日时，当地墙钟）。
    pub start_key: u32,
    /// 结束切换 civil 键（退出 DST 的月日时，当地墙钟）。
    pub end_key: u32,
    /// DST 偏移增量（分钟，标准时之上；样例均为 60）。
    pub delta_min: i32,
}

/// civil 日期时间 → 键（月×10000+日×100+时）。
pub fn civil_key(mon: u8, day: u8, hour: u8) -> u32 {
    (mon as u32) * 10000 + (day as u32) * 100 + hour as u32
}

/// DST 规则表（定长 16 条）。
pub struct DstTable {
    rules: [Option<DstRule>; MAX_DST_RULES],
    pub count: usize,
}

/// 单点 civil 时刻分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CivClass {
    /// 标准时（含无规则生效年）。
    Standard,
    /// 夏令时正常时段。
    Daylight,
    /// 回拨重复小时（秋切：同一墙钟出现两次——账本须按序号消歧）。
    FallRepeat,
    /// 前跳缺失小时（春切：该墙钟本地不存在）。
    SpringGap,
}

/// 单日歧义/缺口两类账（切换日诊断账目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayScan {
    /// 前跳缺失小时数（春切日）。
    pub gap_hours: u32,
    /// 回拨重复小时数（秋切日）。
    pub repeat_hours: u32,
}

impl DstTable {
    pub const fn new() -> Self {
        DstTable { rules: [None; MAX_DST_RULES], count: 0 }
    }

    /// 入表；年区间倒挂或表满显性 Err。
    pub fn add_rule(&mut self, r: DstRule) -> Result<(), &'static str> {
        if r.last_year < r.first_year {
            return Err("bad-year-range");
        }
        if self.count >= MAX_DST_RULES {
            return Err("rule-table-full");
        }
        self.rules[self.count] = Some(r);
        self.count += 1;
        Ok(())
    }

    fn rule_for(&self, year: i32) -> Option<DstRule> {
        for slot in self.rules.iter().take(self.count) {
            if let Some(r) = slot {
                if r.first_year <= year && year <= r.last_year {
                    return Some(*r);
                }
            }
        }
        None
    }

    /// 规则查找：DST 生效 → (true, delta)；未生效/无规则年 → (false, 0)
    /// 走标准时（增量只在 DST 生效时非零——账面与语义一致）。
    pub fn lookup(&self, year: i32, mon: u8, day: u8, hour: u8) -> (bool, i32) {
        match self.rule_for(year) {
            None => (false, 0),
            Some(r) => {
                if in_dst(civil_key(mon, day, hour), &r) {
                    (true, r.delta_min)
                } else {
                    (false, 0)
                }
            }
        }
    }

    /// 年度转换点生成：定长 2 条（起始切换、结束切换）。
    pub fn transitions_for_year(&self, year: i32) -> Result<[(u32, i32); TRANSITION_POINTS], &'static str> {
        match self.rule_for(year) {
            None => Err("no-rule"),
            Some(r) => Ok([(r.start_key, r.delta_min), (r.end_key, r.delta_min)]),
        }
    }

    /// 切换歧义/缺口标记：春切 [start, start+span) 缺失、秋切
    /// [end, end+span) 重复（span = 增量整小时数；切换不跨日故键算术安全）。
    pub fn classify_civil(&self, year: i32, mon: u8, day: u8, hour: u8) -> Result<CivClass, &'static str> {
        let r = match self.rule_for(year) {
            None => return Ok(CivClass::Standard),
            Some(r) => r,
        };
        if r.delta_min % 60 != 0 {
            return Err("key-granularity-exceeded");
        }
        let span = (r.delta_min / 60) as u32;
        let k = civil_key(mon, day, hour);
        if k >= r.start_key && k < r.start_key + span {
            return Ok(CivClass::SpringGap);
        }
        if k >= r.end_key && k < r.end_key + span {
            return Ok(CivClass::FallRepeat);
        }
        Ok(if in_dst(k, &r) { CivClass::Daylight } else { CivClass::Standard })
    }

    /// 单日歧义/缺口两类账：扫描当日 24 个整点小时，统计缺失/重复小时数
    /// （春跳缺口与秋拨重复各记一账——切换日账本，与 CivClass 标记互补）。
    pub fn scan_day(&self, year: i32, mon: u8, day: u8) -> Result<DayScan, &'static str> {
        let mut rep = DayScan { gap_hours: 0, repeat_hours: 0 };
        for h in 0..24u8 {
            match self.classify_civil(year, mon, day, h)? {
                CivClass::SpringGap => rep.gap_hours += 1,
                CivClass::FallRepeat => rep.repeat_hours += 1,
                _ => {}
            }
        }
        Ok(rep)
    }
}

/// 在 DST 段内判定（北半球 start<end 同年夹段；南半球跨年 start>end 绕段）。
fn in_dst(k: u32, r: &DstRule) -> bool {
    if r.start_key < r.end_key {
        k >= r.start_key && k < r.end_key
    } else {
        k >= r.start_key || k < r.end_key
    }
}

/// 域自检（深化批次四）。
pub fn run_f022f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F022-tzrule-d4");
    // 0) 表模型口径：一张规则表 = 一个区域时区的规则集（tzdata zone 行的
    //    域内投影）；跨区域样例各建一表，避免同年规则歧义。
    // 1) 真实 tzdata 样例入表：US 2024/2025、EU 2024、AU-NSW 2024-25。
    let mut t_us = DstTable::new();
    let mut t_eu = DstTable::new();
    let mut t_au = DstTable::new();
    let _ = t_us.add_rule(DstRule { first_year: 2024, last_year: 2024, start_key: civil_key(3, 10, 2), end_key: civil_key(11, 3, 2), delta_min: 60 });
    let _ = t_us.add_rule(DstRule { first_year: 2025, last_year: 2025, start_key: civil_key(3, 9, 2), end_key: civil_key(11, 2, 2), delta_min: 60 });
    let _ = t_eu.add_rule(DstRule { first_year: 2024, last_year: 2024, start_key: civil_key(3, 31, 2), end_key: civil_key(10, 27, 3), delta_min: 60 });
    let _ = t_au.add_rule(DstRule { first_year: 2024, last_year: 2025, start_key: civil_key(10, 6, 2), end_key: civil_key(4, 6, 3), delta_min: 60 });
    cs.add("rules_load_real_samples", t_us.count == 2 && t_eu.count == 1 && t_au.count == 1, "");
    // 2) 夏季命中 DST（2024-07-01 12:00 美国规则 → +60min）。
    cs.add("lookup_summer_dst", t_us.lookup(2024, 7, 1, 12) == (true, 60), "");
    // 3) 冬季走标准时；无规则年（2030）也如实走标准时。
    cs.add("lookup_winter_std", t_us.lookup(2024, 1, 15, 12) == (false, 0)
        && t_us.lookup(2030, 7, 1, 12) == (false, 0), "");
    // 4) 年度转换点生成：2024 美国规则 → 3/10 02:00 与 11/3 02:00 两点。
    let tr = t_us.transitions_for_year(2024).expect("2024 有规则");
    cs.add("transitions_two_points", tr == [(31002, 60), (110302, 60)], "");
    // 5) 无规则年转换点显性 Err（零静默）。
    cs.add("transitions_no_rule_err", t_us.transitions_for_year(2030) == Err("no-rule"), "");
    // 6) 春跳缺口：2024-03-10 02:00 本地不存在；03:00 已入 DST。
    cs.add("spring_gap_flagged", t_us.classify_civil(2024, 3, 10, 2) == Ok(CivClass::SpringGap)
        && t_us.classify_civil(2024, 3, 10, 3) == Ok(CivClass::Daylight), "");
    // 7) 秋拨重复：2024-11-03 02:00 出现两次；00:30 仍在 DST。
    cs.add("fall_repeat_flagged", t_us.classify_civil(2024, 11, 3, 2) == Ok(CivClass::FallRepeat)
        && t_us.classify_civil(2024, 11, 3, 0) == Ok(CivClass::Daylight), "");
    // 8) 南半球跨年规则：AU 2025-01-15 在 DST 段；2025-07-15 标准时。
    cs.add("southern_cross_year", t_au.classify_civil(2025, 1, 15, 12) == Ok(CivClass::Daylight)
        && t_au.classify_civil(2025, 7, 15, 12) == Ok(CivClass::Standard), "");
    // 9) EU 规则切换日交界：2024-10-27 03:00 为重复段（03:00 墙钟出现两次）。
    cs.add("eu_fall_boundary", t_eu.classify_civil(2024, 10, 27, 3) == Ok(CivClass::FallRepeat)
        && t_eu.classify_civil(2024, 10, 26, 12) == Ok(CivClass::Daylight)
        && t_eu.classify_civil(2024, 10, 28, 3) == Ok(CivClass::Standard), "");
    // 10) 非整小时增量超出键粒度 → 显性 Err（Lord Howe 类，见文件头说明）。
    let mut t2 = DstTable::new();
    let _ = t2.add_rule(DstRule { first_year: 2024, last_year: 2024, start_key: 100602, end_key: 40603, delta_min: 30 });
    cs.add("halfhour_delta_err", t2.classify_civil(2024, 12, 1, 12) == Err("key-granularity-exceeded"), "");
    // 11) 单日两类账：2024-03-10 春跳缺口 1 小时、无重复；2024-11-03 秋拨
    //     重复 1 小时、无缺口；非切换日两账皆零。
    let gap_day = t_us.scan_day(2024, 3, 10).expect("2024 有规则");
    let repeat_day = t_us.scan_day(2024, 11, 3).expect("2024 有规则");
    let plain_day = t_us.scan_day(2024, 6, 15).expect("2024 有规则");
    cs.add("day_scan_two_ledgers", gap_day == DayScan { gap_hours: 1, repeat_hours: 0 }
        && repeat_day == DayScan { gap_hours: 0, repeat_hours: 1 }
        && plain_day == DayScan { gap_hours: 0, repeat_hours: 0 }, "");
    // 12) 南半球跨年规则的单日账：AU 2025-04-06 秋拨重复 1 小时。
    let au_day = t_au.scan_day(2025, 4, 6).expect("2025 命中跨年规则");
    cs.add("southern_day_scan", au_day == DayScan { gap_hours: 0, repeat_hours: 1 }, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_2025_transitions_real_dates() {
        let mut t = DstTable::new();
        let _ = t.add_rule(DstRule {
            first_year: 2025, last_year: 2025,
            start_key: civil_key(3, 9, 2), end_key: civil_key(11, 2, 2), delta_min: 60,
        });
        let tr = t.transitions_for_year(2025).expect("2025 有规则");
        assert_eq!(tr, [(30902, 60), (110202, 60)], "US 2025：3/9 与 11/2 两次切换");
        assert_eq!(t.classify_civil(2025, 3, 9, 2), Ok(CivClass::SpringGap));
        assert_eq!(t.classify_civil(2025, 11, 2, 1), Ok(CivClass::Daylight));
    }

    #[test]
    fn bad_year_range_rejected() {
        let mut t = DstTable::new();
        assert_eq!(
            t.add_rule(DstRule { first_year: 2025, last_year: 2024, start_key: 1, end_key: 2, delta_min: 60 }),
            Err("bad-year-range")
        );
    }

    #[test]
    fn eu_2025_transitions_real_dates() {
        let mut t = DstTable::new();
        let _ = t.add_rule(DstRule {
            first_year: 2025, last_year: 2025,
            start_key: civil_key(3, 30, 2), end_key: civil_key(10, 26, 3), delta_min: 60,
        });
        let tr = t.transitions_for_year(2025).expect("2025 有规则");
        assert_eq!(tr, [(33002, 60), (102603, 60)], "EU 2025：3/30 与 10/26 两次切换");
        assert_eq!(t.classify_civil(2025, 3, 30, 2), Ok(CivClass::SpringGap));
        assert_eq!(t.classify_civil(2025, 10, 26, 3), Ok(CivClass::FallRepeat));
    }

    #[test]
    fn scan_day_needs_a_rule() {
        let t = DstTable::new();
        // 无规则生效年：单日扫描如实走标准时（两账皆零，不 Err）。
        assert_eq!(t.scan_day(2030, 3, 10), Ok(DayScan { gap_hours: 0, repeat_hours: 0 }));
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f022f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
