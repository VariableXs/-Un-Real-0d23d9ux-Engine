//! VE-F0005 · 域自检（判据逐条对应，见 `vea05_budget.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 预算仲裁、三档预算、水位可见、挤占拒绝 → 基础四项
//! - 超支→降级+告知（降级矩阵）→ `A05-降级-*`
//! - 抢占规则文档（谁可以挤谁写明）→ `A05-规则-文档在册`
//! - 水位表含历史曲线（何时吃紧可回溯）→ `A05-水位-历史曲线可回溯`
//! - 三档预算的申请模板随 SDK 分发 → `A05-模板-随SDK分发`
//! - 挤占拒绝含申请方与持有方双方告知 → `A05-挤占-双方告知`
//! - 域级加总复核（A06 联动）→ `A05-对账-域级加总`
//! - 水位异常→归因（降级矩阵）→ `A05-水位-异常归因`
//! - 无障碍读屏可达 → `A05-读屏-*`
//! - 零静默错误账本 → `A05-错误-零静默入账`
//!
//! 逻辑时钟注入、无墙钟，回归可复现。

use super::vea05_budget::*;
use crate::checks::CheckSet;

use alloc::string::ToString;
use alloc::vec::Vec;

/// 标准三档：64Ki / 128Ki / 256Ki。
fn tier_std() -> BudgetTier {
    BudgetTier::new(64 * 1024, 128 * 1024, 256 * 1024).expect("标准三档合法")
}

/// 域承诺 512Ki / 物理 1Mi 的标准仲裁器。
fn arb_std() -> VramArbiter {
    VramArbiter::new(512 * 1024, 1024 * 1024)
}

/// VE-F0005 域自检。
pub fn run_vea05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea05");

    // ---- 基础四判据 ----

    // 判据：预算仲裁（额度内申请按目标档全额授出，账目可查）。
    {
        let mut a = arb_std();
        let r = a.apply(BudgetRequest::new("vxcomp", tier_std(), PriorityClass::Compositor));
        let ok = matches!(r, Ok(GrantOutcome::Granted { granted }) if granted == 128 * 1024)
            && a.entry("vxcomp").map(|l| l.granted) == Some(128 * 1024)
            && a.granted_total() == 128 * 1024;
        set.add("A05-仲裁-额度内全额授出", ok, "");
    }

    // 判据：三档预算（promise ≤ target ≤ ceiling 不变式入口强制）。
    {
        let bad = BudgetTier::new(256 * 1024, 128 * 1024, 64 * 1024);
        let good = tier_std();
        // 违反不变式 → 拒绝（不静默钳制）；合法三档 → 通过。
        let rejected = matches!(&bad, Err(e) if e.code == "E_TIER_INVARIANT");
        // 粒度：非 4KiB 对齐的申请被拒。
        let mut a = arb_std();
        let unaligned = BudgetTier::new(64 * 1024 + 1, 128 * 1024 + 1, 256 * 1024 + 1);
        let r = a.apply(BudgetRequest::new(
            "vxweb",
            unaligned.expect("仅构造用"),
            PriorityClass::Foreground,
        ));
        let gran = matches!(r, Err(e) if e.code == "E_NOT_ALIGNED");
        set.add(
            "A05-三档-不变式与粒度强制",
            rejected && good.aligned() && gran,
            "",
        );
    }

    // 判据：水位可见（实时等级 + 读屏文本带数字）。
    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new(
            "vxcomp",
            BudgetTier::new(512 * 1024, 768 * 1024, 1024 * 1024).expect("合法"),
            PriorityClass::Compositor,
        ));
        let _ = a.report_usage("vxcomp", 900 * 1024);
        // 先上报用量再采样：900Ki / 1Mi = 87.8% → High 档。
        a.advance_tick();
        let s = a.watermark().screen_text();
        let txt = a.screen_text();
        set.add(
            "A05-水位-实时可见且读屏带数",
            a.watermark().current_level() == WatermarkLevel::High
                && s.contains("921600") && s.contains("吃紧")
                && txt.contains("已授"),
            "",
        );
    }

    // 判据：挤占拒绝（规则外组合全拒 + 拒绝入审计流）。
    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new("bg预热", tier_std(), PriorityClass::Background));
        let _ = a.apply(BudgetRequest::new(
            "前台A",
            tier_std(),
            PriorityClass::Foreground,
        ));
        // R1：挤合成器 → 拒。
        let _ = a.apply(BudgetRequest::new(
            "合成器",
            tier_std(),
            PriorityClass::Compositor,
        ));
        let r1 = a.try_preempt("前台A", "合成器", 16 * 1024);
        // R3：同级互挤 → 拒。
        let _ = a.apply(BudgetRequest::new(
            "前台B",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let r3 = a.try_preempt("前台A", "前台B", 16 * 1024);
        let denied = matches!(r1, Err(e) if e.code == "E_PREEMPT_DENIED")
            && matches!(r3, Err(e) if e.code == "E_PREEMPT_DENIED")
            && a.preempt_audits().len() == 2
            && a.preempt_audits().iter().all(|x| !x.allowed);
        set.add("A05-挤占-规则外全拒", denied, "");
    }

    // ---- 降级矩阵：超支→降级+告知 ----

    {
        let mut a = VramArbiter::new(192 * 1024, 1024 * 1024); // 域承诺很小
        // 第一单按目标档拿走 128Ki（期望值），剩 64Ki。
        let _ = a.apply(BudgetRequest::new(
            "vxweb",
            tier_std(),
            PriorityClass::Compositor,
        ));
        // 第二单目标档 128Ki 放不进剩余 64Ki → 降到承诺档（保底值）64Ki。
        let r = a.apply(BudgetRequest::new(
            "vx3d",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let ok = matches!(
            r,
            Ok(GrantOutcome::Downgraded { granted, reason })
                if granted == 64 * 1024 && reason == DowngradeReason::TargetToPromise
        ) && a.notices().len() == 1
            && a.notices()[0].applicant == "vx3d"
            && a.notices()[0].screen_text().contains("65536");
        set.add("A05-降级-超支降级并告知", ok, "");
    }

    {
        // 剩余空间连目标档都放不下 → 降到剩余；剩余不足最小预算 → 拒绝且不挤占。
        let mut a = VramArbiter::new(96 * 1024, 1024 * 1024);
        let _ = a.apply(BudgetRequest::new(
            "vxweb",
            BudgetTier::new(64 * 1024, 64 * 1024, 128 * 1024).expect("合法"),
            PriorityClass::Compositor,
        ));
        // 剩 32Ki < 最小预算 64Ki → E_DOMAIN_EXHAUSTED，他人账目原封不动。
        let r = a.apply(BudgetRequest::new(
            "vx3d",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let ok = matches!(r, Err(e) if e.code == "E_DOMAIN_EXHAUSTED")
            && a.entry("vxweb").map(|l| l.granted) == Some(64 * 1024);
        set.add("A05-降级-耗尽拒绝不挤占", ok, "");
    }

    // ---- 抢占规则 ----

    // 判据：抢占规则文档在册（谁可以挤谁写明）。
    {
        let d = PREEMPTION_RULES_DOC;
        let ok = d.contains("R1") && d.contains("R2") && d.contains("R3")
            && d.contains("合成器保底")
            && d.contains("后台预热")
            && d.contains("一律拒绝");
        set.add("A05-规则-文档在册", ok, "");
    }

    // 判据：R2 合法挤占（前台挤后台，最多一半，双方入审计）+ 账目转账。
    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new(
            "bg预热",
            tier_std(),
            PriorityClass::Background,
        ));
        let _ = a.apply(BudgetRequest::new(
            "前台A",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let _ = a.report_usage("bg预热", 64 * 1024);
        let r = a.try_preempt("前台A", "bg预热", 32 * 1024);
        let holder = a.entry("bg预热").expect("在账");
        let applicant = a.entry("前台A").expect("在账");
        let ok = matches!(r, Ok(g) if g == 32 * 1024)
            && holder.in_use == 32 * 1024
            && applicant.in_use == 32 * 1024
            && a.preempt_audits().len() == 1
            && a.preempt_audits()[0].allowed;
        set.add("A05-规则-R2挤占转账", ok, "");
    }

    // 判据：挤占拒绝含申请方与持有方双方告知（责任方字段点名双方）。
    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new(
            "合成器",
            tier_std(),
            PriorityClass::Compositor,
        ));
        let _ = a.apply(BudgetRequest::new(
            "前台A",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let r1 = a.try_preempt("前台A", "合成器", 16 * 1024);
        let who_named_both = matches!(&r1, Err(e) if e.who.contains("前台A") && e.who.contains("合成器"));
        set.add("A05-挤占-双方告知", who_named_both, "");
    }

    // ---- 水位表 ----

    // 判据：历史曲线可回溯（环形保留 256 点，时序正确）。
    {
        let mut a = arb_std();
        for i in 0..300u64 {
            a.advance_tick();
            let _ = i;
        }
        let curve = a.watermark().curve();
        let ok = curve.len() == WATERMARK_HISTORY
            && curve.first().map(|s| s.tick) == Some(300 - WATERMARK_HISTORY as u64 + 1)
            && curve.last().map(|s| s.tick) == Some(300)
            && curve.windows(2).all(|w| w[0].tick + 1 == w[1].tick);
        set.add("A05-水位-历史曲线可回溯", ok, "");
    }

    // 判据：水位异常→归因（账实背离给候选原因，不硬猜）。
    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new(
            "vxcomp",
            tier_std(),
            PriorityClass::Compositor,
        ));
        let _ = a.report_usage("vxcomp", 64 * 1024);
        // 外部直测比账本多 8Ki（未记账分配）→ 异常 + 归因建议。
        a.reconcile_measured(64 * 1024 + 8 * 1024);
        let an = a.watermark().anomaly().map(|x| (x.kind, x.attribution_hint()));
        let ok = matches!(an, Some(("未记账分配", _)))
            && a.watermark().anomaly().unwrap().delta_bytes == 8 * 1024;
        set.add("A05-水位-异常归因", ok, "");
    }

    // ---- 域级加总复核 ----

    // 判据：加总复核（授出总和 ≤ 域承诺预算，超记即审计红项）。
    {
        let mut a = arb_std(); // 域承诺 512Ki
        let big = BudgetTier::new(256 * 1024, 256 * 1024, 512 * 1024).expect("合法");
        let _ = a.apply(BudgetRequest::new("vxweb", big, PriorityClass::Foreground));
        let _ = a.apply(BudgetRequest::new("vx3d", big, PriorityClass::Background));
        let audit = a.domain_audit();
        let ok = audit.within
            && audit.sum_granted == 512 * 1024
            && audit.over_by == 0
            && audit.subsystems == 2;
        set.add("A05-对账-域级加总复核", ok, "");
    }

    // ---- 申请模板 ----

    // 判据：三档预算的申请模板随 SDK 分发（模板在册且字段齐全）。
    {
        let t = BudgetRequest::sdk_template();
        let ok = t.contains("承诺档") && t.contains("目标档") && t.contains("上限档")
            && t.contains("优先级") && t.contains("E_TIER_INVARIANT");
        set.add("A05-模板-随SDK分发", ok, "");
    }

    // ---- 错误账本（零静默）----

    {
        let mut a = arb_std();
        // 三类错误各入账一次：不变式 / 未知方释放 / 越顶钳制。
        // 坏档直接用字面量构造（BudgetTier::new 会拒绝它——测试的是 apply 入口）。
        let _ = a.apply(BudgetRequest::new(
            "坏档",
            BudgetTier { promise: 1, target: 2, ceiling: 3 },
            PriorityClass::Foreground,
        ));
        let _ = a.apply(BudgetRequest::new(
            "vxweb",
            tier_std(),
            PriorityClass::Foreground,
        ));
        let _ = a.release("ghost");
        let _ = a.report_usage("vxweb", 512 * 1024);
        let codes: Vec<&str> = a.errors().iter().map(|e| e.code).collect();
        let ok = codes.contains(&"E_TIER_INVARIANT")
            && codes.contains(&"E_UNKNOWN_APPLICANT")
            && codes.contains(&"E_OVER_CEILING")
            && a.errors().iter().all(|e| !e.next.is_empty())
            && a.entry("vxweb").map(|l| l.in_use) == Some(256 * 1024);
        set.add("A05-错误-零静默入账", ok, "");
    }

    // ---- 释放纪律 ----

    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new("vxweb", tier_std(), PriorityClass::Foreground));
        let _ = a.report_usage("vxweb", 16 * 1024);
        // 有占用时释放被拒（防泄漏），清占用后释放成功。
        let blocked = matches!(a.release("vxweb"), Err(e) if e.code == "E_RELEASE_WITH_USAGE");
        let _ = a.report_usage("vxweb", 0);
        let freed = matches!(a.release("vxweb"), Ok(n) if n == 128 * 1024);
        set.add("A05-释放-占用未清拒绝", blocked && freed && a.granted_total() == 0, "");
    }

    // ---- 重复申请与账本容量 ----

    {
        let mut a = arb_std();
        let _ = a.apply(BudgetRequest::new("vxweb", tier_std(), PriorityClass::Foreground));
        let dup = a.apply(BudgetRequest::new("vxweb", tier_std(), PriorityClass::Foreground));
        let ok = matches!(dup, Err(e) if e.code == "E_DUPLICATE_APPLICANT");
        set.add("A05-仲裁-重复申请拒绝", ok, "");
    }

    // ---- 读屏可达 ----

    {
        let a = arb_std();
        let t = a.screen_text();
        set.add(
            "A05-读屏-总摘要可播",
            t.contains("显存预算") && t.contains("子系统在账"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea05_all_judgements_green() {
        let set = run_vea05_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F0005 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn vea05_watermark_levels_ordered() {
        // 等级阈值边界：69/70/84/85/94/95 六点。
        let total = 1000u64;
        assert_eq!(WatermarkLevel::from_ratio(690, total), WatermarkLevel::Normal);
        assert_eq!(WatermarkLevel::from_ratio(700, total), WatermarkLevel::Watch);
        assert_eq!(WatermarkLevel::from_ratio(840, total), WatermarkLevel::Watch);
        assert_eq!(WatermarkLevel::from_ratio(850, total), WatermarkLevel::High);
        assert_eq!(WatermarkLevel::from_ratio(940, total), WatermarkLevel::High);
        assert_eq!(WatermarkLevel::from_ratio(950, total), WatermarkLevel::Critical);
    }
}
