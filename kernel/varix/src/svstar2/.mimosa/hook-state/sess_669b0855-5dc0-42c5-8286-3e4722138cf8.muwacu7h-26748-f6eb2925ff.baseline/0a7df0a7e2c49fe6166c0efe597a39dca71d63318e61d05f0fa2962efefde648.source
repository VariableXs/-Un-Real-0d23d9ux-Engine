//! VE-F0015 · 域自检（判据逐条对应，见 `vea15_errclass.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四类分类（资源/命令/同步/驱动） → `A15-分类-四类齐全`
//! - 错误码三要素（发生了什么/为什么/下一步） → `A15-三要素-齐备构造`
//! - 码缺三要素→补齐（源头拒绝） → `A15-三要素-缺要素拒绝`
//! - 自动归因建议（四类常见根因对照表） → `A15-分类-自动归因建议`
//! - 分类错误→修正 → `A15-分类-错分修正`
//! - 上抛纪律（零静默必达） → `A15-上抛-必达日志中心`
//! - 背压保护（错误风暴不淹日志中心） → `A15-上抛-背压聚合`
//! - 静默→阻断级 → `A15-上抛-静默阻断级`
//! - 三要素含读屏完整播报 → `A15-读屏-三要素完整播报`
//! - 双向检索（码→日志 / 日志→详情） → `A15-检索-双向`
//! - 总日志中心对接 → `A15-衔接-日志中心契约`
//!
//! 逻辑时钟注入、零墙钟，回归可复现。

use super::vea15_errclass::*;
use crate::checks::CheckSet;

/// VE-F0015 域自检。
pub fn run_vea15_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea15");

    fn err(code: &'static str, class: ErrClass) -> RenderError {
        RenderError::new(code, class, "测试错误", "根因补充", "下一步动作").unwrap()
    }

    // ---- 四类分类 ----

    // 判据：分类学四类齐全且类名可读。
    {
        let all = [
            (ErrClass::Resource, "资源"),
            (ErrClass::Command, "命令"),
            (ErrClass::Sync, "同步"),
            (ErrClass::Driver, "驱动"),
        ];
        let ok = all.iter().all(|(c, n)| c.name() == *n);
        set.add("A15-分类-四类齐全", ok, "");
    }

    // ---- 三要素 ----

    // 判据：三要素齐备（what/why/next 全非空且带归因建议）。
    {
        let e = err("E_OOM_VRAM", ErrClass::Resource);
        let ok = !e.what.is_empty()
            && e.why.contains("水位")
            && e.next.contains("预算仲裁")
            && e.why.contains("调用方补充")
            && e.next.contains("调用方补充");
        set.add("A15-三要素-齐备构造", ok, "");
    }

    // 判据：码缺三要素→补齐——源头拒绝（空要素/非法码不构造）。
    {
        let a = RenderError::new("E_X", ErrClass::Sync, "", "y", "z");
        let b = RenderError::new("E_Y", ErrClass::Sync, "w", "  ", "z");
        let c = RenderError::new("NOPE", ErrClass::Sync, "w", "y", "z");
        let ok = matches!(a, Err("E_TRIAD_INCOMPLETE"))
            && matches!(b, Err("E_TRIAD_INCOMPLETE"))
            && matches!(c, Err("E_CODE_INVALID"));
        set.add("A15-三要素-缺要素拒绝", ok, "");
    }

    // ---- 自动归因建议 ----

    // 判据：四类 × 常见根因对照表在册（每类至少 2 档归因非空）。
    {
        let probe = [
            (ErrClass::Resource, "E_OOM_1", "水位"),
            (ErrClass::Resource, "E_HANDLE_2", "句柄"),
            (ErrClass::Command, "E_SUBMIT_1", "帧边界"),
            (ErrClass::Sync, "E_DEADLOCK_1", "等待环"),
            (ErrClass::Driver, "E_TDR_1", "复位"),
            (ErrClass::Driver, "E_LOST_1", "丢失"),
        ];
        let ok = probe.iter().all(|(cl, code, kw)| {
            let (cause, advice) = cl.common_causes(code);
            cause.contains(kw) && !advice.is_empty()
        });
        set.add("A15-分类-自动归因建议", ok, "");
    }

    // ---- 分类错误→修正 ----

    {
        let mut p = RaisePipeline::new();
        p.raise(err("E_TDR_RESET", ErrClass::Resource)); // 驱动码报资源类 → 修正建议
        let one = p.corrections.len() == 1
            && p.corrections[0] == ("E_TDR_RESET".to_string(), "资源", "驱动");
        p.raise(err("E_TDR_RESET", ErrClass::Driver)); // 分对 → 零新增
        let honest = p.corrections.len() == 1;
        set.add("A15-分类-错分修正", one && honest, "");
    }

    // ---- 上抛纪律：零静默必达 ----

    {
        let mut p = RaisePipeline::new();
        let reached = p.raise(err("E_LOST_DEV", ErrClass::Driver));
        let logged = !p.center.query_by_code("E_LOST_DEV").is_empty() && p.raised() == 1;
        set.add("A15-上抛-必达日志中心", reached && logged, "");
    }

    // ---- 背压保护 ----

    // 判据：错误风暴不淹日志中心（同码风暴聚合，吸收量显性入账）。
    {
        let mut p = RaisePipeline::new();
        for _ in 0..40 {
            assert!(p.raise(err("E_LOST_DEV", ErrClass::Driver)));
        }
        let absorbed = p.center.backpressure_absorbed >= 1;
        let compact = p.center.entries.len() < 40;
        let counted = p.center.entries.last().map(|e| e.aggregated >= 2).unwrap_or(false);
        set.add("A15-上抛-背压聚合", absorbed && compact && counted, "");
    }

    // ---- 静默→阻断级 ----

    {
        let mut p = RaisePipeline::new();
        let (clean, _) = p.audit_silence();
        p.direct_writes = 3;
        let (bad, msg) = p.audit_silence();
        set.add(
            "A15-上抛-静默阻断级",
            clean && !bad && msg.contains("阻断级") && msg.contains("3"),
            "",
        );
    }

    // ---- 读屏完整播报 ----

    {
        let e = err("E_TIMEOUT_WAIT", ErrClass::Sync);
        let t = e.screen_text();
        set.add(
            "A15-读屏-三要素完整播报",
            t.contains("渲染错误 E_TIMEOUT_WAIT")
                && t.contains("同步类")
                && t.contains("发生了什么")
                && t.contains("为什么")
                && t.contains("下一步"),
            "",
        );
    }

    // ---- 双向检索 ----

    // 判据：正查（码→条目）+ 反查（条目→详情全文）双向可达。
    {
        let mut p = RaisePipeline::new();
        p.raise(err("E_DEADLOCK_RING", ErrClass::Sync));
        p.raise(err("E_OOM_VRAM", ErrClass::Resource));
        let fwd = p.center.query_by_code("E_DEADLOCK_RING");
        let fwd_ok = fwd.len() == 1 && fwd[0].class == "同步";
        let back = p.center.detail_of("E_DEADLOCK_RING", fwd[0].tick);
        let back_ok = back.map(|d| d.contains("等待环") && d.contains("下一步")).unwrap_or(false);
        let miss = p.center.query_by_code("E_ABSENT").is_empty()
            && p.center.detail_of("E_DEADLOCK_RING", 9999).is_none();
        set.add("A15-检索-双向", fwd_ok && back_ok && miss, "");
    }

    // ---- 总日志中心对接 ----

    {
        set.add(
            "A15-衔接-日志中心契约",
            LOG_LINK_VERSION >= 1
                && LOG_CENTER_CAPACITY >= 16
                && BACKPRESSURE_WINDOW >= 1,
            "",
        );
    }

    set
}
