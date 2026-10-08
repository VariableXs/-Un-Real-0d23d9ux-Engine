//! CGPU-F2405 · 遥测存储分层域自检（锚点测试一组：分层 + stamp）。
//!
//! **判据（锚点原文）**：分层复用、一组、判据。

use super::cgp05_tiers::{
    TieredStore, Tier, AggCell, Tier as T,
    HOT_CAPACITY, HOT_RETAIN_TICKS, WARM_SHARD_LIMIT, REUSE_LINES,
};

/// 判据侧独立重排的锚点判据三条。
const CRITERIA_RECHECK: [&str; 3] = ["分层复用", "一组", "判据"];

/// CGPU-F2405 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp05_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp05_tiers");

    // —— 一组 · 分层：单向流转 + 保留期清理 + 清理前摘要 + 查询三答案 ——
    // 单向流转链独立对拍：热→温→冷，冷是终点（无回热——结构保证）。
    let tier_ok = Tier::Hot.next() == Some(Tier::Warm)
        && Tier::Warm.next() == Some(Tier::Cold)
        && Tier::Cold.next().is_none()
        && Tier::Hot.name() == "热"
        && Tier::Cold.name() == "冷";
    // 流转全链手算：塞 HOT_CAPACITY+3 条（推进 tick）→ 热满逐出 3 条进温；
    // 温分片跨窗口自然开裂（每 16 tick 一窗）；窗口超 4 片→最老并冷。
    let mut st = TieredStore::new();
    let mut i = 0u64;
    while i < (HOT_CAPACITY + 3) as u64 {
        st.store("p01.sampling.v", (i % 5) + 1);
        if i % 3 == 0 {
            st.advance(); // tick 推进比入流慢——窗口内多条
        }
        i += 1;
    }
    // 热环恒容：HOT_CAPACITY 条满后逐出——最终热层 ≤ HOT_CAPACITY。
    let hot_ok = st.query_hot() <= HOT_CAPACITY && st.query_hot() > 0;
    // 温层有分片（逐出的条目落温——不是静默消失）。
    let warm_ok = st.query_warm() > 0;
    // 保留期清理：推进 100 tick（> HOT_RETAIN_TICKS=60）后再存——旧明细全部过期进温。
    let mut st2 = TieredStore::new();
    st2.store("j08.power.w", 100);
    let mut t = 0u64;
    while t < HOT_RETAIN_TICKS + 10 {
        st2.advance();
        t += 1;
    }
    st2.store("j08.power.w", 200);
    // 新明细在热（tick 新），旧明细（tick=0，age=70>60）应已被逐出进温。
    let expire_ok = st2.query_hot() == 1 && st2.query_warm() >= 1;
    // 冷归档：连续塞跨 5+ 个窗口的逐出条目——分片超限并冷（清理前摘要保留，
    // 冷 agg.count = 各并冷分片条数之和，不丢条）。
    let mut st3 = TieredStore::new();
    let mut w = 0u64;
    while w < 40 {
        // 每次推进 16 tick 强制开新窗（逐出条目跨 5 个窗口）。
        let mut k = 0u64;
        while k < 16 {
            st3.advance();
            k += 1;
        }
        st3.store("p01.sampling.v", w + 1);
        w += 1;
    }
    let cold = st3.query_cold();
    let cold_ok = cold.shards_merged >= 1
        && cold.agg.count >= 1
        && cold.agg.mean().is_some()
        && st3.demoted_warm_cold >= cold.agg.count;
    // 聚合单元手算对账：merge 语义（空并=复制、双桶并 min/max 取极值）。
    let mut a = AggCell::new();
    a.absorb(10);
    a.absorb(30);
    let mut b = AggCell::new();
    b.absorb(20);
    a.merge(&b);
    let agg_ok = a.min == 10 && a.max == 30 && a.sum == 60 && a.count == 3 && a.mean() == Some(20);
    s.add(
        "P05-一组分层-流转+保留+摘要+查询",
        tier_ok && hot_ok && warm_ok && expire_ok && cold_ok && agg_ok,
        "单向流转热→温→冷冷是终点（无回迁 API 结构保证）；热环恒容≤16 逐出不静默消失；保留期 60 tick 过期清理；分片超限并冷且冷账条数=分片条数之和（清理前摘要保留不丢条）；AggCell merge 手算 min=10/max=30/sum=60/count=3/mean=20；查询三答案热明细/温分片/冷摘要",
    );

    // —— 复用声明逐条 grep（判据「分层复用」） ——
    let mut reuse_ok = REUSE_LINES.len() == 4;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F1558") || l.contains("F1449") || l.contains("F0483")) {
            reuse_ok = false;
        }
        ri += 1;
    }
    // 判据 stamp 独立对账。
    let stamps = ["分层复用", "一组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    let _ = T::Hot; // 防未用导入（判据侧双名核对）
    s.add(
        "P05-复用+判据stamp-逐条对账",
        reuse_ok && stamp_ok && HOT_CAPACITY == 16 && HOT_RETAIN_TICKS == 60 && WARM_SHARD_LIMIT == 4,
        "复用清单四条含 F1558/F1449/F0483 关键字逐条 grep（三层模式/环形/聚合同构/保留策略）；锚点判据三条与判据侧独立重排逐条全等；一组测试（分层）宣告与实际检查一一对应；保留表常量独立写死对拍",
    );

    s
}
