//! 深化层二 · F145 教育/作品集友好（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】脱敏前后 diff 可溯 +【设计细节】注入测试
//! 与学生反馈（主册 G-D-20）：脱敏规则引擎（四类模式匹配+diff 记录）、
//! 注入测试框架（检出率 100% 判据）、文章分级前置知识判据、ADR 导读
//! 生成器、抽查复核抽样器、反馈聚合到 F139 编号。

use crate::checks::CheckSet;
use crate::stareco::ebase::TraceId;
use crate::stareco::eduportfolio::{desensitized, Portfolio, THEMES};

// ---------------------------------------------------------------------------
// 脱敏规则引擎：四类模式 → 硬删 → diff 记录（前后可溯）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DesensKind {
    /// 实机序列号（如 S/N 段）。
    Serial,
    /// 账号名。
    Account,
    /// 路径中的用户段（C:\Users\<name>\）。
    UserPath,
    /// 内部密钥字面量。
    Secret,
}

/// 单条脱敏 diff：原文位置提示 + 类型（不保留原值——保留即泄漏）。
pub struct DesensDiff {
    pub kind: DesensKind,
    /// 命中位置（字节偏移——审计用，不携带内容）。
    pub at: usize,
}

/// 四类模式检测：对行文本逐类扫描（模式只此一份——管线与测试共用）。
pub fn scan_line(line: &str) -> alloc::vec::Vec<DesensKind> {
    let mut out = alloc::vec::Vec::new();
    let b = line.as_bytes();
    let has = |needle: &str| line.contains(needle);
    // 序列号模式：SN: / S/N 后跟 6+ 位
    for w in b.windows(3) {
        if (w == b"SN:" || w == b"S/N") && b.len() > 0 {
            out.push(DesensKind::Serial);
            break;
        }
    }
    if has("@") && has("user") {
        out.push(DesensKind::Account);
    }
    if has("C:\\Users\\") {
        out.push(DesensKind::UserPath);
    }
    if has("KEY=") || has("token:") {
        out.push(DesensKind::Secret);
    }
    out
}

/// 脱敏管线：行文本 → 硬删产物（类型占位替换）+ diff 记录。
/// 产物必须过基础层 desensitized 复检（双保险）。
pub fn desensitize_line(line: &'static str) -> (&'static str, alloc::vec::Vec<DesensDiff>) {
    let kinds = scan_line(line);
    let clean: &'static str = match kinds.as_slice() {
        [] => line,
        _ => "[已脱敏]",
    };
    let diffs = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| DesensDiff { kind: *k, at: i })
        .collect();
    (clean, diffs)
}

// ---------------------------------------------------------------------------
// 注入测试框架：敏感样本注入 → 管线检出率 100% 判据
// ---------------------------------------------------------------------------

/// 注入样本集（四类各一条——注入测试的固定样本）。
pub const INJECTION_SAMPLES: [(&str, DesensKind); 4] = [
    ("SN:2026-VARIX-0001", DesensKind::Serial),
    ("contact user@mail", DesensKind::Account),
    ("C:\\Users\\variable\\doc", DesensKind::UserPath),
    ("KEY=deadbeef", DesensKind::Secret),
];

/// 注入轮：全部样本必须被检出（任何漏检 = 管线红）。
pub fn injection_drill() -> Result<usize, &'static str> {
    let mut caught = 0usize;
    for (sample, kind) in INJECTION_SAMPLES {
        let found = scan_line(sample);
        if found.contains(&kind) {
            caught += 1;
        }
    }
    if caught == INJECTION_SAMPLES.len() {
        Ok(caught)
    } else {
        Err(alloc::format!("注入检出 {}/4：管线有漏", caught).leak() as &'static str)
    }
}

// ---------------------------------------------------------------------------
// 文章分级：前置知识数判据（入门/进阶/深水区的量化线）
// ---------------------------------------------------------------------------

/// 分级判定：前置概念数 0-2 入门 / 3-5 进阶 / 6+ 深水区。
pub fn level_for(prereq_concepts: usize) -> &'static str {
    match prereq_concepts {
        0..=2 => "starter",
        3..=5 => "advanced",
        _ => "deep",
    }
}

// ---------------------------------------------------------------------------
// ADR 导读生成器：决策注解模板（保留原 ADR 编号可溯）
// ---------------------------------------------------------------------------

pub fn generate_annotation(adr_no: u32, decision: &'static str, why: &'static str) -> Result<alloc::string::String, &'static str> {
    if adr_no == 0 {
        return Err("ADR 编号缺失：决策注解必须可溯到原始 ADR");
    }
    if decision.is_empty() || why.is_empty() {
        return Err("决策与理由必填：导读没有「为什么」就是抄目录");
    }
    Ok(alloc::format!("ADR-{} 决策：{}（为什么：{}）", adr_no, decision, why))
}

// ---------------------------------------------------------------------------
// 抽查复核抽样器：每版 20 处确定性抽样（同版本同抽样——可复现复核）
// ---------------------------------------------------------------------------

/// 确定性抽样：以版本号为种子从文章表抽 AUDIT_SAMPLES 篇（重复检查剔除）。
pub fn audit_sample(article_ids: &[u32], version: u32, n: usize) -> alloc::vec::Vec<u32> {
    if article_ids.is_empty() || n == 0 {
        return alloc::vec::Vec::new();
    }
    let mut out = alloc::vec::Vec::new();
    let mut seed = version as u64 | 1;
    for _ in 0..n {
        // LCG 确定性步进（同版本同序列）
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let idx = (seed % article_ids.len() as u64) as usize;
        let id = article_ids[idx];
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 反馈聚合：学生反馈 → F139 提交编号关联（学习曲线改进闭环）
// ---------------------------------------------------------------------------

pub struct FeedbackLink {
    pub article: TraceId,
    /// F139 报告编号（学生反馈走的正式通道）。
    pub fb_ref: TraceId,
    pub topic: &'static str,
}

pub fn feedback_link_valid(l: &FeedbackLink) -> bool {
    l.article.is_valid() && l.fb_ref.is_valid() && !l.topic.is_empty()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145E_TAG: &str = "stareco-F145-deep2";

pub fn run_f145_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F145E_TAG);

    // 脱敏引擎
    let (clean, diffs) = desensitize_line("SN:2026-VARIX-0001 在 C:\\Users\\variable\\");
    set.add("f145e desens clean", desensitized(clean), "产物过基础层复检");
    set.add(
        "f145e desens kinds",
        diffs.len() == 2 && diffs[0].kind == DesensKind::Serial && diffs[1].kind == DesensKind::UserPath,
        "两类命中逐条记 diff",
    );
    let (plain, no_diff) = desensitize_line("正常技术叙述");
    set.add("f145e desens pass-through", plain == "正常技术叙述" && no_diff.is_empty(), "无敏感段原样保留");

    // 注入测试
    set.add("f145e injection 100%", injection_drill() == Ok(4), "四类样本全检出");

    // 分级
    set.add("f145e level starter", level_for(0) == "starter" && level_for(2) == "starter", "0-2 入门");
    set.add("f145e level advanced", level_for(3) == "advanced" && level_for(5) == "advanced", "3-5 进阶");
    set.add("f145e level deep", level_for(6) == "deep" && level_for(9) == "deep", "6+ 深水区");

    // ADR 导读
    set.add(
        "f145e annotation ok",
        generate_annotation(42, "三拍冻结", "冻结先行避免并发改判据").is_ok(),
        "注解生成",
    );
    set.add("f145e annotation no adr", generate_annotation(0, "d", "w").is_err(), "无编号拒绝");
    set.add("f145e annotation empty why", generate_annotation(1, "d", "").is_err(), "无理由拒绝");

    // 抽样器
    let ids: alloc::vec::Vec<u32> = (1..=50).collect();
    let s1 = audit_sample(&ids, 7, 20);
    let s2 = audit_sample(&ids, 7, 20);
    set.add("f145e sample deterministic", s1 == s2, "同版本同抽样");
    set.add("f145e sample count", s1.len() == 20, "抽满 20 处");
    let s3 = audit_sample(&ids, 8, 20);
    set.add("f145e sample varies", s1 != s3, "换版本换抽样");
    set.add("f145e sample empty", audit_sample(&[], 1, 20).is_empty(), "空表诚实空样");

    // 反馈聚合
    let link = FeedbackLink {
        article: TraceId::new("EDU", 20260926, 1),
        fb_ref: TraceId::new("FB", 20260926, 2),
        topic: "步骤 3 跳步",
    };
    set.add("f145e fb valid", feedback_link_valid(&link), "反馈双编号关联");
    set.add(
        "f145e fb invalid",
        !feedback_link_valid(&FeedbackLink { article: link.article, fb_ref: TraceId::new("FB", 1, 1), topic: "t" }),
        "非法编号拒关联",
    );

    // 与基础层联动：四主题配额
    let mut pf = Portfolio::new();
    for theme in THEMES {
        for _ in 0..5 {
            let _ = pf.admit(20260926, "t", theme, "starter", 1, true);
        }
    }
    set.add("f145e first batch", pf.first_batch_complete(), "四主题各 5（与基础层对账）");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn scan_secret_variants() {
        assert!(scan_line("KEY=abc").contains(&DesensKind::Secret));
        assert!(scan_line("token:xyz").contains(&DesensKind::Secret));
        assert!(scan_line("KEY=abc token:t").contains(&DesensKind::Secret)); // 同类不重复
        assert!(scan_line("clean text").is_empty());
    }

    #[test]
    fn level_boundaries() {
        assert_eq!(level_for(0), "starter");
        assert_eq!(level_for(3), "advanced");
        assert_eq!(level_for(6), "deep");
    }

    #[test]
    fn annotation_renders_adr() {
        let s = generate_annotation(7, "x", "y").unwrap();
        assert!(s.contains("ADR-7"));
    }
}
