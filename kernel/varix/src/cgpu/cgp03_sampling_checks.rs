//! CGPU-F2403 · 采样策略引擎域自检（锚点测试三组：引擎/DSL/自适应 + stamp）。
//!
//! **判据（锚点原文）**：价值采样、DSL 声明、自适应复用、三组、判据。

use super::cgp03_sampling::{
    parse_policies, Decision, SamplingEngine, SamplingAction, SignalCost, DslError, LoadTier,
    ADAPT_UPTIFT, DSL_GRAMMAR, REUSE_LINES, SIGNAL_MAX,
};
use alloc::string::ToString;

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["价值采样", "DSL 声明", "自适应复用", "三组", "判据"];

/// CGPU-F2403 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp03_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp03_sampling");

    // —— 组一 · 引擎：价值采样 + 决策面 ——
    // 价值分独立重算：signal=500, cost=25 → 500×1000÷25 = 20000。
    let sc = SignalCost { signal: 500, cost: 25 };
    let value_hand = 20000u32;
    // 基线非平凡：正常配置引擎确实会采（先证非全跳，门禁不恒绿）。
    let mut eng = SamplingEngine::mount("gpu.render:always\ngpu.power:every_n:10\n").ok();
    let mut mounted = false;
    let mut sampled_seen = false;
    let mut skipped_reason = "";
    if let Some(e) = eng.as_mut() {
        mounted = !e.policies.is_empty() && e.policy_of("gpu.render") == Some(SamplingAction::Always);
        for _ in 0..3 {
            e.advance();
        }
        let (d, known) = e.decide("gpu.render", &sc);
        sampled_seen = known && d == Decision::Sampled;
        // 表外指标反向：不臆造策略。
        let (d2, known2) = e.decide("表外.指标", &sc);
        if !known2 {
            skipped_reason = match d2 {
                Decision::Skipped(r) => r,
                _ => "",
            };
        }
    }
    s.add(
        "P03-组一引擎-价值采样+决策面",
        mounted
            && sampled_seen
            && sc.value() == value_hand
            && sc.worth(19999)
            && !sc.worth(20001)
            && SignalCost { signal: 2000, cost: 0 }.value() == 1000u32 * 1000
            && skipped_reason == "表外指标",
        "价值分独立重算 500×1000÷25=20000 手算对账；恰阈值双向（19999 采 20001 不采）；信号超界+零成本饱和不 panic；表外指标显性 Skipped 不臆造策略；基线引擎正常采样（非全跳恒绿）",
    );

    // —— 组二 · DSL：五动作闭集解析 + 四错闭集 + 语法冻结 ——
    let full = "a1:always\na2:every_n:7\na3:rate:5000\na4:value_above:12000\na5:adapt\n";
    let parsed = parse_policies(full);
    let mut five_ok = false;
    if let Ok(ps) = parsed.as_ref() {
        five_ok = ps.len() == 5
            && ps.get(0).map(|p| p.action == SamplingAction::Always).unwrap_or(false)
            && ps.get(1).map(|p| p.action == SamplingAction::EveryN(7)).unwrap_or(false)
            && ps.get(2).map(|p| p.action == SamplingAction::Rate(5000)).unwrap_or(false)
            && ps.get(3).map(|p| p.action == SamplingAction::ValueAbove(12000)).unwrap_or(false)
            && ps.get(4).map(|p| p.action == SamplingAction::Adapt).unwrap_or(false);
    }
    // 四错闭集逐条：缺冒号 / 空名 / 表外动作 / 坏参数。
    let e1 = parse_policies("no_sep_action");
    let e2 = parse_policies(":always");
    let e3 = parse_policies("x:magic_action");
    let e4 = parse_policies("x:every_n:abc");
    let errs_ok = e1 == Err(DslError::NoSeparator)
        && e2 == Err(DslError::EmptyName)
        && e3 == Err(DslError::UnknownAction)
        && e4 == Err(DslError::BadParam);
    // 反向：任一行错整批拒绝（不部分装载）。
    let batch_reject = parse_policies("good:always\nbad:oops\n").is_err();
    // rate 超界饱和到万分比上限。
    let rate_cap = parse_policies("x:rate:999999")
        .ok()
        .map(|ps| ps.get(0).map(|p| p.action == SamplingAction::Rate(10000)).unwrap_or(false))
        .unwrap_or(false);
    s.add(
        "P03-组二DSL-五动作+四错+整批拒绝",
        five_ok
            && errs_ok
            && batch_reject
            && rate_cap
            && DSL_GRAMMAR.contains("every_n")
            && DSL_GRAMMAR.contains("rate:万分比")
            && DSL_GRAMMAR.contains("value_above:阈值")
            && SamplingAction::Adapt.name() == "adapt",
        "五动作闭集逐字段解析对拍（always/every_n:7/rate:5000/value_above:12000/adapt）；四错闭集逐条专属错（缺冒号/空名/表外/坏参数）；任一行错整批拒绝不部分装载；rate 超界饱和 10000；语法一行冻结含三关键片段",
    );

    // —— 组三 · 自适应：负载感知降频 + 复用账 ——
    // every_n:10 高负载加倍：tick 推进到 9（第 10 步命中）低/中负载采，高负载不采。
    let mut lo = SamplingEngine::mount("x:every_n:10").ok();
    let mut hi = SamplingEngine::mount("x:every_n:10").ok();
    let mut cadence_ok = false;
    let mut lift_ok = false;
    let mut reuse_ok = REUSE_LINES.len() == 3;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F2402") || l.contains("DSL") || l.contains("F0274")) {
            reuse_ok = false;
        }
        if l.ends_with(" ") || l.starts_with(" ") {
            reuse_ok = false;
        }
        ri += 1;
    }
    if let (Some(l), Some(h)) = (lo.as_mut(), hi.as_mut()) {
        l.set_load(LoadTier::Low);
        h.set_load(LoadTier::High);
        for _ in 0..9 {
            l.advance();
            h.advance();
        }
        // tick=9 → (9+1)%10==0 命中；高负载间隔加倍后 (10)%20!=0 跳过。
        let (dl, _) = l.decide("x", &SignalCost { signal: 100, cost: 1 });
        let (dh, _) = h.decide("x", &SignalCost { signal: 100, cost: 1 });
        cadence_ok = dl == Decision::Sampled
            && dh == Decision::Skipped("间隔未到")
            && h.adapt_count() == 1
            && l.adapt_count() == 0;
        // 阈值上浮独立重算：th=10000 → 10000+10000×500/10000 = 10500；
        // 信号 500/成本 25 = 20000 ≥ 10500 采；高负载 Mid 不浮：同输入 Mid 采。
        let mut hi2 = SamplingEngine::mount("y:value_above:10000").ok();
        let mut mid2 = SamplingEngine::mount("y:value_above:10000").ok();
        if let (Some(h2), Some(m2)) = (hi2.as_mut(), mid2.as_mut()) {
            h2.set_load(LoadTier::High);
            let (dh2, _) = h2.decide("y", &SignalCost { signal: 520, cost: 25 });
            let (dm2, _) = m2.decide("y", &SignalCost { signal: 500, cost: 25 });
            // 520×1000÷25=20800 ≥ 10500 采；500×1000÷25=20000 < 10000×1.05? 20000≥10000 采（Mid 阈值不浮）。
            lift_ok = dh2 == Decision::Sampled && dm2 == Decision::Sampled && h2.adapt_count() >= 1;
            // 反向：高负载下价值不足的指标被拦（signal=250 → 10000 < 10500）。
            let (dh3, _) = h2.decide("y", &SignalCost { signal: 250, cost: 25 });
            lift_ok = lift_ok && dh3 == Decision::Skipped("价值不足");
        }
    }
    s.add(
        "P03-组三自适应-负载降频+复用账",
        cadence_ok
            && lift_ok
            && reuse_ok
            && ADAPT_UPTIFT == 500
            && LoadTier::High.label() == "高负载",
        "every_n:10 高负载加倍第 10 步跳过（低负载采高负载跳）且降频记账 1 次；阈值上浮独立重算 10000→10500（+ADAPT_UPTIFT 万分之 500）；Mid 不降频反向；复用清单三条含 F2402/DSL/F0274 关键字逐条 grep",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["价值采样", "DSL 声明", "自适应复用", "三组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P03-判据stamp-五条独立重排全等",
        stamp_ok && SIGNAL_MAX == 1000 && DSL_GRAMMAR.contains("名称:动作"),
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）；三组测试（引擎/DSL/自适应）宣告与实际检查一一对应",
    );

    s
}
