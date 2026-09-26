//! 深化层 · F145 教育/作品集友好（2026-09-26 回炉补深化）。
//!
//! 补深：文章元数据完整模型、公开版与源文档 diff 可溯、版本窗同步、
//! 读者反馈路由 F139、导读决策注解校验（保留原 ADR 编号）。

use crate::checks::CheckSet;
use crate::stareco::eduportfolio::{Article, Portfolio, LEVELS, PER_THEME, THEMES};

// ---------------------------------------------------------------------------
// 文章元数据完整模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArticleMeta {
    pub theme: &'static str,
    pub level: &'static str,
    pub adr_no: u32,
    /// 源文档指纹（diff 可溯锚）。
    pub source_fp: u64,
}

/// 元数据完备性：主题/分级合法、ADR 注解在、源指纹非零。
pub fn meta_complete(m: &ArticleMeta) -> bool {
    THEMES.contains(&m.theme) && LEVELS.contains(&m.level) && m.adr_no > 0 && m.source_fp != 0
}

// ---------------------------------------------------------------------------
// 公开版 ↔ 源文档 diff 可溯
// ---------------------------------------------------------------------------

/// diff 可溯判定：公开版指纹必须 ≠ 源指纹（有脱敏改动）且改动记录在
/// 案（审计行非空）——「源文档与公开版 diff 可溯」的机制面。
pub fn diff_traceable(source_fp: u64, public_fp: u64, audit_line: &str) -> Result<(), &'static str> {
    if source_fp == 0 || public_fp == 0 {
        return Err("指纹缺失：无法追溯");
    }
    if source_fp == public_fp {
        return Err("公开版与源文档指纹相同：脱敏未执行");
    }
    if audit_line.is_empty() {
        return Err("改动审计行缺失：diff 不可溯");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 版本窗同步（F138 节奏联动）
// ---------------------------------------------------------------------------

/// 公开版同步：源文档更新（版本号递增）后，公开版必须随版本窗同步。
pub struct SyncLedger {
    pub source_version: u32,
    pub public_synced_version: u32,
}

impl SyncLedger {
    /// 滞后版本数：0 = 同步；>0 = 版本窗挂账。
    pub fn lag(&self) -> u32 {
        self.source_version.saturating_sub(self.public_synced_version)
    }

    /// 是否需要本窗同步（滞后 ≥1 即挂账）。
    pub fn needs_sync(&self) -> bool {
        self.lag() > 0
    }
}

// ---------------------------------------------------------------------------
// 读者反馈路由（学生反馈通道进 F139）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReaderFeedback {
    /// 看不懂 → 回流文档改进（F135）。
    Unclear,
    /// 内容有错 → 判例管线（F139 报告）。
    Wrong,
    /// 想要更多 → 选题池。
    WantMore,
}

/// 反馈路由：错误内容优先级最高（教材错字比缺章更毒）。
pub fn feedback_priority(f: ReaderFeedback) -> u8 {
    match f {
        ReaderFeedback::Wrong => 3,
        ReaderFeedback::Unclear => 2,
        ReaderFeedback::WantMore => 1,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145D_TAG: &str = "stareco-F145-deep";

pub fn run_f145_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F145D_TAG);

    // 元数据完备
    let good = ArticleMeta { theme: "boot", level: "starter", adr_no: 42, source_fp: 0xFEED };
    set.add("f145d meta complete", meta_complete(&good), "四元齐");
    let no_adr = ArticleMeta { adr_no: 0, ..good };
    set.add("f145d adr required", !meta_complete(&no_adr), "决策注解必填");
    let bad_theme = ArticleMeta { theme: "gossip", ..good };
    set.add("f145d theme law", !meta_complete(&bad_theme), "四主题限定");

    // diff 可溯
    set.add(
        "f145d diff traceable",
        diff_traceable(1, 2, "脱敏：删实机编号两处").is_ok(),
        "指纹异 + 审计行在",
    );
    set.add(
        "f145d undesorized caught",
        diff_traceable(7, 7, "x").is_err() && diff_traceable(7, 8, "").is_err(),
        "未脱敏/无审计均拒",
    );

    // 版本窗同步
    let sync = SyncLedger { source_version: 5, public_synced_version: 4 };
    set.add(
        "f145d sync lag counted",
        sync.lag() == 1 && sync.needs_sync(),
        "滞后一版挂账",
    );
    let synced = SyncLedger { source_version: 4, public_synced_version: 4 };
    set.add("f145d synced clean", !synced.needs_sync(), "同版即齐");

    // 反馈路由
    set.add(
        "f145d feedback priority",
        feedback_priority(ReaderFeedback::Wrong) > feedback_priority(ReaderFeedback::Unclear)
            && feedback_priority(ReaderFeedback::Unclear) > feedback_priority(ReaderFeedback::WantMore),
        "错误 > 看不懂 > 想更多",
    );

    // 配额常量复诵（深化层联动主判据）
    set.add(
        "f145d quota law intact",
        THEMES.len() == 4 && PER_THEME == 5,
        "四主题各 5 = 20",
    );

    // 登记表联动：收录后主题计数可查
    let mut pf = Portfolio::new();
    pf.admit(20260926, "引导三闸", "boot", "starter", 1, true).ok();
    set.add("f145d portfolio counts", pf.count_theme("boot") == 1, "收录即计数");

    // Article publishable 联动（脏文章拒收的深化复诵）
    let dirty = Article {
        id: crate::stareco::ebase::TraceId::new("EDU", 20260926, 1),
        title: "t",
        theme: "boot",
        level: "starter",
        adr_no: 9,
        clean: false,
    };
    set.add("f145d dirty never publishable", !dirty.publishable(), "硬删纪律");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn meta_law() {
        assert!(!meta_complete(&ArticleMeta { source_fp: 0, ..good_meta() }));
    }

    fn good_meta() -> ArticleMeta {
        ArticleMeta { theme: "storage", level: "deep", adr_no: 7, source_fp: 9 }
    }
}
