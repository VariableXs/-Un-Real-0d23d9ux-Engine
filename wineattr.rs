//! 崩溃归因五分类与上游回馈（WP-302 · B-1006 归因五分类 + B-1007 回馈命中）。
//!
//! MD2 篇 10.5：Wine 应用崩溃的归因五分类穷举——应用自身缺陷（星卡登记，
//! 等待上游修复）、Wine 缺陷（上游 issue 义务，18.4）、垫片缺陷（我们修，
//! 垫片测试补用例）、资源越限（配额提示，非缺陷）、未知（进诊断中心待归因
//! 池）。**第五类"未知"是覆盖 100% 的结构面：任何崩溃报告都能落到恰好一类，
//! 不存在无处安放的错误码**。五分类在崩溃报告界面即呈现（用户看得到归因
//! 而不是黑箱错误码）。回馈流程：上游 issue 模板（最小复现+环境指纹+变通
//! 方案）入库，季度窗口统计回馈命中率——回馈不是姿态，是维护成本的对冲
//! （上游修了我们就少维护一个补丁）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 归因五分类（穷举——类型面保证"必有归因"）
// ---------------------------------------------------------------------------

/// 归因五分类（穷举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootCause {
    /// 应用自身缺陷：星卡登记，等待上游修复。
    AppBug,
    /// Wine 缺陷：上游 issue 义务（MD2 18.4）。
    WineBug,
    /// 垫片缺陷：我们修，垫片测试补用例。
    ShimBug,
    /// 资源越限：配额提示，非缺陷。
    ResourceLimit,
    /// 未知：进诊断中心待归因池——兜底类，保证分类覆盖 100%。
    Unknown,
}

/// 归因判定材料（分类不是猜，是证据裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 同一数据在原生平台对照也崩——应用自身缺陷。
    NativeRepro,
    /// 仅 VARIX 上崩且垫片路径未参与——Wine 缺陷。
    BareWineRepro,
    /// 关闭垫片后恢复——垫片缺陷。
    ShimCorrelated,
    /// 资源水位超限在册（CPU/内存/句柄）——资源越限。
    QuotaBreached,
    /// 以上皆无——未知（待归因池）。
    NoSignal,
}

/// 证据到归因的**穷举 match 单源**（与 lxerrno 同结构防线：无默认分支，
/// 证据加一类编译器强制此处补一行——表外无静默成功）。
pub fn classify(ev: Evidence) -> RootCause {
    match ev {
        Evidence::NativeRepro => RootCause::AppBug,
        Evidence::BareWineRepro => RootCause::WineBug,
        Evidence::ShimCorrelated => RootCause::ShimBug,
        Evidence::QuotaBreached => RootCause::ResourceLimit,
        Evidence::NoSignal => RootCause::Unknown,
    }
}

/// 归因人话（崩溃报告界面归因行——五类各有呈现文案，黑箱错误码没有生存空间）。
pub fn cause_label(cause: RootCause) -> &'static str {
    match cause {
        RootCause::AppBug => "应用自身缺陷（星卡登记，等待上游修复）",
        RootCause::WineBug => "Wine 缺陷（上游 issue 义务）",
        RootCause::ShimBug => "垫片缺陷（我们修，垫片测试补用例）",
        RootCause::ResourceLimit => "资源越限（配额提示，非缺陷）",
        RootCause::Unknown => "未知（诊断中心待归因池）",
    }
}

// ---------------------------------------------------------------------------
// 崩溃报告（归因不可缺省 + 界面呈现，B-1006 达标线"分类覆盖 100%"）
// ---------------------------------------------------------------------------

/// 一份崩溃报告：归因结论由 classify 单源赋值（类型面必有），呈现是运行面
/// 纪律（shown_to_user 必须真——写了归因没给人看等于没写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrashReport {
    /// 崩溃签名（应用/判例指纹）。
    pub signature: u32,
    pub cause: RootCause,
    /// 报告界面已呈现归因行。
    pub shown_to_user: bool,
}

/// 分诊：证据进，报告出（cause 由 classify 单源赋值，无可逃逸路径）。
pub fn triage(signature: u32, ev: Evidence) -> CrashReport {
    CrashReport { signature, cause: classify(ev), shown_to_user: false }
}

/// 报告界面呈现归因行（呈现是显式动作，不做自动假设）。
pub fn present_cause(rep: &mut CrashReport) {
    rep.shown_to_user = true;
}

/// 分类覆盖 100% 审计：**类型面**保证每份报告必有五类之一的归因（enum
/// 穷举，不存在未分类态）；**运行面**检查全部已呈现给用户——两面同时成立
/// 才是 B-1006 的"覆盖 100%"。
pub fn coverage_100(reports: &[CrashReport]) -> bool {
    let mut i = 0;
    while i < reports.len() {
        if !reports[i].shown_to_user {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 上游回馈（B-1007：issue 模板三要素入库 + 季度命中率）
// ---------------------------------------------------------------------------

/// 上游 issue 模板三要素（MD2 10.5：最小复现+环境指纹+变通方案——缺一不入库）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IssueTemplate {
    /// 最小复现（上游拿到就能跑）。
    pub min_repro: bool,
    /// 环境指纹（VARIX 版本+Wine 版本+前缀模板版本）。
    pub env_fingerprint: bool,
    /// 变通方案（上游修好前用户的活路）。
    pub workaround: bool,
}

impl IssueTemplate {
    /// 入库门：三要素缺一拒收（半份 issue 是给上游添堵不是回馈）。
    pub fn well_formed(&self) -> bool {
        self.min_repro && self.env_fingerprint && self.workaround
    }
}

/// 季度回馈台账（一个季度一条——季度统计入库，不是散落的口头战报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuarterStats {
    /// 季度编号（自 1 起递增）。
    pub quarter: u16,
    /// 本季提交的上游 issue 数。
    pub submitted: usize,
    /// 本季命中数（上游已修复——我们就少维护一个补丁）。
    pub hits: usize,
}

impl QuarterStats {
    /// 台账良构：季度有编号且命中不超提交（命中多于提交是造假）。
    pub fn well_formed(&self) -> bool {
        self.quarter > 0 && self.hits <= self.submitted
    }

    /// 回馈命中率（permille，千分比纪律）。无提交返回 None——
    /// **无数据不入账不编数**，0‰ 与"没提交过"是两回事。
    pub fn hit_permille(&self) -> Option<u16> {
        if self.submitted == 0 {
            None
        } else {
            Some(((self.hits * 1000) / self.submitted) as u16)
        }
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1006/1007 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_wineattr_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1006/1007 崩溃归因五分类与回馈");
    // 1. 五证据→五归因：穷举 match 单源，第五类兜底保证覆盖 100% 的结构面。
    let m = [
        (Evidence::NativeRepro, RootCause::AppBug),
        (Evidence::BareWineRepro, RootCause::WineBug),
        (Evidence::ShimCorrelated, RootCause::ShimBug),
        (Evidence::QuotaBreached, RootCause::ResourceLimit),
        (Evidence::NoSignal, RootCause::Unknown),
    ];
    let mut i = 0;
    let mut all = true;
    while i < m.len() {
        if classify(m[i].0) != m[i].1 {
            all = false;
        }
        i += 1;
    }
    set.add(
        "B-1006 归因五分类穷举",
        all && classify(Evidence::NoSignal) == RootCause::Unknown,
        "证据穷举裁决五类——未知也是类，不存在无处安放的错误码",
    );
    // 2. 归因人话直出：五类各有界面呈现文案。
    let mut labels = true;
    let causes = [RootCause::AppBug, RootCause::WineBug, RootCause::ShimBug, RootCause::ResourceLimit, RootCause::Unknown];
    i = 0;
    while i < causes.len() {
        let s = cause_label(causes[i]);
        if s.is_empty() {
            labels = false;
        }
        i += 1;
    }
    set.add(
        "B-1006 归因人话直出",
        labels,
        "崩溃报告界面呈现归因而非黑箱错误码——用户看得到结论",
    );
    // 3. 分类覆盖 100%（B-1006 达标线）：全批归因（类型面）+ 全批呈现（运行面）。
    let mut batch = [
        triage(1, Evidence::NativeRepro),
        triage(2, Evidence::BareWineRepro),
        triage(3, Evidence::ShimCorrelated),
        triage(4, Evidence::QuotaBreached),
        triage(5, Evidence::NoSignal),
    ];
    let blind_before = coverage_100(&batch);
    i = 0;
    while i < batch.len() {
        present_cause(&mut batch[i]);
        i += 1;
    }
    set.add(
        "B-1006 分类覆盖 100%",
        !blind_before && coverage_100(&batch),
        "归因必有（enum 穷举）且全部呈现——漏一屏就是覆盖没满",
    );
    // 4. issue 模板三要素入库：缺一拒收。
    let full = IssueTemplate { min_repro: true, env_fingerprint: true, workaround: true };
    let no_workaround = IssueTemplate { min_repro: true, env_fingerprint: true, workaround: false };
    let no_repro = IssueTemplate { min_repro: false, env_fingerprint: true, workaround: true };
    set.add(
        "B-1007 模板三要素入库",
        full.well_formed() && !no_workaround.well_formed() && !no_repro.well_formed(),
        "最小复现+环境指纹+变通方案缺一不入库——半份 issue 是添堵",
    );
    // 5. 季度台账良构：有编号且命中不超提交。
    let ok_q = QuarterStats { quarter: 3, submitted: 12, hits: 5 };
    let fake = QuarterStats { quarter: 3, submitted: 5, hits: 12 };
    let no_num = QuarterStats { quarter: 0, submitted: 5, hits: 1 };
    set.add(
        "B-1007 季度统计入库",
        ok_q.well_formed() && !fake.well_formed() && !no_num.well_formed(),
        "季度窗口一条台账——命中多于提交或无编号都是假账",
    );
    // 6. 命中率 permille：有提交才算率，无提交 None 不编数（B-1007 达标线）。
    set.add(
        "B-1007 回馈命中率",
        ok_q.hit_permille() == Some(416) && QuarterStats { quarter: 1, submitted: 0, hits: 0 }.hit_permille().is_none(),
        "命中=上游修复=少维护一个补丁；没提交过与 0‰ 是两回事",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe05 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe05_classify_exhaustive_no_default() {
        // 五证据全映射：材料到归因一路单源，兜底类接住无信号崩溃。
        assert_eq!(classify(Evidence::NativeRepro), RootCause::AppBug);
        assert_eq!(classify(Evidence::BareWineRepro), RootCause::WineBug);
        assert_eq!(classify(Evidence::ShimCorrelated), RootCause::ShimBug);
        assert_eq!(classify(Evidence::QuotaBreached), RootCause::ResourceLimit);
        assert_eq!(classify(Evidence::NoSignal), RootCause::Unknown);
    }

    #[test]
    fn fe05_coverage_requires_user_visible() {
        // 归因写了没呈现 = 覆盖没满：呈现是运行面纪律。
        let mut rep = triage(7, Evidence::BareWineRepro);
        assert!(!coverage_100(&[rep]));
        present_cause(&mut rep);
        assert!(coverage_100(&[rep]));
        // 混批：一份漏呈现，全批判不达线。
        let mut a = triage(1, Evidence::NativeRepro);
        present_cause(&mut a);
        let b = triage(2, Evidence::NoSignal);
        assert!(!coverage_100(&[a, b]));
    }

    #[test]
    fn fe05_template_three_elements_gate() {
        // 逐要素缺席逐一拒收：三要素是合取不是投票。
        assert!(IssueTemplate { min_repro: true, env_fingerprint: true, workaround: true }.well_formed());
        assert!(!IssueTemplate { min_repro: false, env_fingerprint: true, workaround: true }.well_formed());
        assert!(!IssueTemplate { min_repro: true, env_fingerprint: false, workaround: true }.well_formed());
        assert!(!IssueTemplate { min_repro: true, env_fingerprint: true, workaround: false }.well_formed());
    }

    #[test]
    fn fe05_quarter_stats_and_hit_rate() {
        let q = QuarterStats { quarter: 2, submitted: 8, hits: 2 };
        assert!(q.well_formed());
        assert_eq!(q.hit_permille(), Some(250));
        // 命中多于提交是假账；无编号是假账。
        assert!(!QuarterStats { quarter: 2, submitted: 1, hits: 2 }.well_formed());
        assert!(!QuarterStats { quarter: 0, submitted: 8, hits: 2 }.well_formed());
        // 无提交返回 None——不编数。
        assert!(QuarterStats { quarter: 4, submitted: 0, hits: 0 }.hit_permille().is_none());
    }
}
