//! 深化层三 · F138 版本发布节奏公开（2026-09-26 深化批次三）。
//!
//! 补深日历工程面（主册 G-D-13）：窗重叠检测（季度窗/修复窗互斥）、
//! 公告提前量审计器（30/90 天分层机器面）、iCal 行折叠（RFC 5545
//! 75 八位组续行规则）、节奏校验（季度窗间距 90±10 天单调递增）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 窗重叠检测：闭区间 [start, end] 相交即重叠
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Window {
    pub name: &'static str,
    pub start: u32,
    pub end: u32,
}

impl Window {
    pub fn new(name: &'static str, start: u32, end: u32) -> Result<Window, &'static str> {
        if end < start {
            return Err("窗止早于窗起：日期倒置");
        }
        Ok(Window { name, start, end })
    }

    pub fn overlaps(&self, other: &Window) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// 全表两两互斥校验（同类窗不得重叠）；返回冲突对清单。
pub fn overlap_pairs(ws: &[Window]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    let mut pairs: alloc::vec::Vec<(&'static str, &'static str)> = alloc::vec::Vec::new();
    for i in 0..ws.len() {
        for j in (i + 1)..ws.len() {
            if ws[i].overlaps(&ws[j]) {
                pairs.push((ws[i].name, ws[j].name));
            }
        }
    }
    pairs
}

// ---------------------------------------------------------------------------
// 公告提前量审计：常规窗 ≥30 天；破坏性变更 ≥1 窗（90 天）
// ---------------------------------------------------------------------------

pub const NOTICE_LEAD_DAYS: u32 = 30;
pub const BREAK_LEAD_DAYS: u32 = 90;

pub fn notice_audit(announce_day: u32, window_day: u32, breaking: bool) -> Result<u32, &'static str> {
    if window_day < announce_day {
        return Err("窗日在公告前：时间线矛盾");
    }
    let lead = window_day - announce_day;
    let required = if breaking { BREAK_LEAD_DAYS } else { NOTICE_LEAD_DAYS };
    if lead < required {
        return Err("提前量不足：公告埋窗违规");
    }
    Ok(lead)
}

// ---------------------------------------------------------------------------
// iCal 行折叠：>75 八位组按 RFC 5545 折行（CRLF+空格续行，ASCII 口径）
// ---------------------------------------------------------------------------

/// 输入不含 CRLF（裸内容行）；输出为折叠后的行序列。
pub fn ical_fold(line: &str) -> alloc::vec::Vec<alloc::string::String> {
    const LIMIT: usize = 75;
    let bytes = line.as_bytes();
    if bytes.len() <= LIMIT {
        return alloc::vec![alloc::string::String::from(line)];
    }
    let mut out: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    let mut first = true;
    let mut pos = 0usize;
    while pos < bytes.len() {
        // 首行 75，续行因前缀 CRLF+SP 占 2 字节预算 → 内容 74。
        let budget = if first { LIMIT } else { LIMIT - 1 };
        let take = (bytes.len() - pos).min(budget);
        // 不劈开多字节 UTF-8：回退到字符边界。
        let mut take = take;
        while take > 0 && pos + take < bytes.len() && (bytes[pos + take] & 0xC0) == 0x80 {
            take -= 1;
        }
        if first {
            out.push(alloc::string::String::from(&line[pos..pos + take]));
            first = false;
        } else {
            let mut s = alloc::string::String::from(" ");
            s.push_str(&line[pos..pos + take]);
            out.push(s);
        }
        pos += take;
    }
    out
}

// ---------------------------------------------------------------------------
// 节奏校验：季度窗起日单调递增且间距 90±10 天
// ---------------------------------------------------------------------------

pub fn cadence_ok(quarter_starts: &[u32]) -> Result<(), &'static str> {
    if quarter_starts.len() < 2 {
        return Err("窗数不足：无法校验节奏");
    }
    for w in quarter_starts.windows(2) {
        if w[1] <= w[0] {
            return Err("窗起日非单调递增：倒序或并列");
        }
        let gap = w[1] - w[0];
        if gap < 80 || gap > 100 {
            return Err("季度间距越出 90±10：节奏漂移");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138F_TAG: &str = "stareco-F138-deep3";

pub fn run_f138_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F138F_TAG);

    // 窗重叠
    let q1 = Window::new("q1", 1, 90).expect("ok");
    let fix = Window::new("fix", 50, 60).expect("ok");
    let q2 = Window::new("q2", 91, 180).expect("ok");
    set.add("f138f overlap hit", q1.overlaps(&fix), "内嵌修复窗相交");
    set.add("f138f overlap miss", !q1.overlaps(&q2), "相邻窗不相交");
    set.add("f138f inverted", Window::new("bad", 10, 5).is_err(), "日期倒置拒绝");
    let pairs = overlap_pairs(&[q1, fix, q2]);
    set.add("f138f pairs", pairs == alloc::vec![("q1", "fix")], "冲突对清单");

    // 公告提前量
    set.add("f138f lead ok", notice_audit(1, 31, false) == Ok(30), "常规 30 天压线");
    set.add("f138f lead short", notice_audit(1, 20, false).is_err(), "提前量不足拦截");
    set.add("f138f lead break", notice_audit(1, 90, true).is_err(), "破坏性 89 天拦截");
    set.add("f138f lead break ok", notice_audit(1, 91, true) == Ok(90), "破坏性 90 天放行");
    set.add("f138f time paradox", notice_audit(50, 40, false).is_err(), "窗在公告前拒绝");

    // iCal 折叠
    let short = ical_fold("SUMMARY:hi");
    set.add("f138f fold short", short.len() == 1, "短行不折");
    let mut long = alloc::string::String::from("DESCRIPTION:");
    for _ in 0..200 {
        long.push('A');
    }
    let folded = ical_fold(&long);
    set.add("f138f fold count", folded.len() == 3, "212 字节折三行");
    set.add(
        "f138f fold budget",
        folded[0].len() == 75 && folded[1].len() == 75 && folded[2].len() == 64,
        "75/74/63 内容预算（续行含引导空格）",
    );
    let mut cjk = alloc::string::String::from("SUMMARY:");
    for _ in 0..40 {
        cjk.push('好'); // 每字 3 字节 → 120+8=128 字节
    }
    let fc = ical_fold(&cjk);
    set.add("f138f fold utf8", fc.len() >= 2 && fc[0].len() <= 75, "多字节不劈开且预算不超");

    // 节奏
    set.add(
        "f138f cadence ok",
        cadence_ok(&[1, 91, 181, 271]).is_ok(),
        "90 天间距通过",
    );
    set.add("f138f cadence drift", cadence_ok(&[1, 80, 170]).is_err(), "79 天间距越界拒绝");
    set.add("f138f cadence order", cadence_ok(&[1, 1]).is_err(), "并列窗起拒绝");
    set.add("f138f cadence single", cadence_ok(&[1]).is_err(), "单窗无法校验");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn fold_exact_75() {
        let line = "X".repeat(75);
        assert_eq!(ical_fold(&line).len(), 1);
        let line76 = "X".repeat(76);
        let f = ical_fold(&line76);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].len(), 75);
        assert_eq!(f[1], " X");
    }

    #[test]
    fn notice_boundaries() {
        assert!(notice_audit(0, 0, false).is_err()); // 0 天提前量
        assert_eq!(notice_audit(0, 30, false), Ok(30));
        assert!(notice_audit(0, 89, true).is_err());
        assert_eq!(notice_audit(0, 90, true), Ok(90));
    }
}
