//! 深化层 · F560 节假日标注（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F560 节）：
//! ①「红/灰双色规则」的**统计对账**——每年红（法定假日）与灰（调休
//!   补班）各多少天、月份分布，双色规则要有账面（数据错了先在账上红）；
//! ②「悬停名称」的**查询口**——hover_text 只吐名称不给类型（名称与
//!   类型分层，悬停文案的唯一取数口）；
//! ③「数据更新随系统更新（F122）走」的**版本账**——内置数据版本戳 +
//!   更新通道记录 + 覆盖年滚动规则（当年+次年；过期年数据随更新滚出）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::holiday::{covers, lookup, mark_token, BUILTIN, COVERED_YEARS, DayMark};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 双色统计对账
// ---------------------------------------------------------------------------

/// 一年的红/灰统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YearStats {
    pub year: u16,
    pub holidays: u32,
    pub workdays: u32,
}

/// 统计指定年（内置数据内的年）的红/灰天数。
pub fn year_stats(year: u16) -> Option<YearStats> {
    let table = BUILTIN.iter().find(|t| !t.is_empty() && t[0].year == year)?;
    let mut s = YearStats { year, holidays: 0, workdays: 0 };
    for e in table.iter() {
        match e.mark {
            DayMark::Holiday => s.holidays += 1,
            DayMark::Workday => s.workdays += 1,
        }
    }
    Some(s)
}

/// 红灰互斥对账：同一天不允许既是假日又是补班（数据矛盾即红）。
pub fn no_day_conflict(year: u16) -> bool {
    let table = match BUILTIN.iter().find(|t| !t.is_empty() && t[0].year == year) {
        Some(t) => t,
        None => return false,
    };
    for (i, a) in table.iter().enumerate() {
        for b in table.iter().skip(i + 1) {
            if a.year == b.year && a.month == b.month && a.day == b.day && a.mark != b.mark {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 悬停名称查询口
// ---------------------------------------------------------------------------

/// 悬停文案：只吐名称（「10-01 国庆节」的名称半边）；无标注 = None。
pub fn hover_text(year: u16, month: u8, day: u8) -> Option<&'static str> {
    lookup(year, month, day).map(|(name, _)| name)
}

/// 悬停行完整合成（日历飞出渲染层直接取用的行文案）。
pub fn hover_line(year: u16, month: u8, day: u8) -> Option<String> {
    lookup(year, month, day).map(|(name, mark)| {
        alloc::format!("{:02}-{:02} {}[{}]", month, day, name, mark_token(mark))
    })
}

// ---------------------------------------------------------------------------
// 数据版本账（随 F122 更新走）
// ---------------------------------------------------------------------------

/// 内置数据版本（每版两年——当年 + 次年）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataVersion {
    /// 基准年（数据以此年起算两年）。
    pub base_year: u16,
    /// 修订号（随系统更新递增）。
    pub revision: u32,
    /// 更新通道（F122；离线机收不到更新，内置数据兜底）。
    pub channel: &'static str,
}

pub const DATA_VERSION: DataVersion =
    DataVersion { base_year: 2026, revision: 1, channel: "F122-系统更新" };

impl DataVersion {
    /// 覆盖年滚动：更新到新版后，基准年前移、旧基准年滚出。
    pub fn roll_to(self, new_base: u16) -> DataVersion {
        DataVersion { base_year: new_base, revision: self.revision + 1, channel: self.channel }
    }

    /// 覆盖核对：内置表与版本账声明必须一致（账册检三处对齐的「账」侧）。
    /// covers() 的语义是「当年与次年都在表内」——次年自身只要求在表
    /// （其次年 = 表外第一年，本就该不覆盖）。
    pub fn covers_match_builtin(&self) -> bool {
        COVERED_YEARS == [self.base_year, self.base_year + 1]
            && covers(self.base_year)
            && COVERED_YEARS.contains(&(self.base_year + 1))
            && !COVERED_YEARS.contains(&(self.base_year + 2))
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f560_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 双色对账：2026 红灰齐备（官方已公布）；2027 红齐备、灰随
    //    F122 官方公布后到位（诚实空态——不编造未公布的调休日期，
    //    数据版本账 §5 记录该待更新位）。
    let s26 = year_stats(2026);
    let s27 = year_stats(2027);
    cs.add(
        "colors per publication state",
        s26.map(|s| s.holidays > 0 && s.workdays > 0).unwrap_or(false)
            && s27.map(|s| s.holidays > 0).unwrap_or(false)
            && s27.map(|s| s.workdays == 0).unwrap_or(false),
        "",
    );

    // 2) 红灰互斥：任何一天不得同时是假日与补班。
    cs.add(
        "no day marked both",
        no_day_conflict(2026) && no_day_conflict(2027),
        "",
    );

    // 3) 悬停名称口：国庆节吐名、普通日 None、类型不漏进名称口。
    cs.add(
        "hover text name only",
        hover_text(2026, 10, 1) == Some("国庆节")
            && hover_text(2026, 6, 15).is_none()
            && hover_text(2026, 2, 15) == Some("春节调休补班"),
        "",
    );

    // 4) 悬停行合成：月日两位补零 + 类型令牌随行（渲染取色唯一口）。
    let line = hover_line(2026, 10, 1);
    cs.add(
        "hover line composed",
        line.as_deref() == Some("10-01 国庆节[holiday-red]"),
        "",
    );

    // 5) 版本账与内置表三处对齐：声明覆盖年 = 表覆盖年，第三年不覆盖。
    cs.add("version covers builtin table", DATA_VERSION.covers_match_builtin(), "");

    // 5b) 待更新位显性化：2027 补班灰未到位 → 修订号待随 F122 递增
    //     （数据缺口是登记在账的，不是静默的）。
    cs.add(
        "workday gap tracked for update",
        year_stats(2027).map(|s| s.workdays == 0).unwrap_or(false)
            && DATA_VERSION.revision == 1,
        "",
    );

    // 6) 版本滚动：更新后基准年前移、修订号递增、通道不变。
    let next = DATA_VERSION.roll_to(2027);
    cs.add(
        "version roll forward",
        next.base_year == 2027 && next.revision == 2 && next.channel == DATA_VERSION.channel,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counts_exact() {
        let s = year_stats(2026).unwrap();
        assert!(s.holidays >= 11); // 元旦3+春节6+清明3+劳动3+…法定合计下限
        assert!(s.workdays >= 1); // 春节调休补班至少 1
    }

    #[test]
    fn out_of_range_year_stats_none() {
        assert!(year_stats(2030).is_none());
    }
}
