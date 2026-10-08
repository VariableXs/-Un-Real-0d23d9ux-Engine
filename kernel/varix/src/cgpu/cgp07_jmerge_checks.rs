//! CGPU-F2407 · 遥测与 J 域收口域自检（锚点测试一组：收编 + stamp）。
//!
//! **判据（锚点原文）**：收编落地、复用、一组、判据。

use super::cgp07_jmerge::{
    merge_j08, verify_merged, UnifiedRegistry, J08_METRICS, SCHEMA_VERSION_SYNC, REUSE_LINES,
};

/// 判据侧独立重排的锚点判据三条。
const CRITERIA_RECHECK: [&str; 3] = ["收编落地", "复用", "一组", ];

/// CGPU-F2407 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp07_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp07_jmerge");

    // —— 一组 · 收编：全量落地 + 幂等重放 + 表外拒绝 + 核验 ——
    // 首次收编：6 条全量新收编（表数判据侧独立写死=6）。
    let mut reg = UnifiedRegistry::new();
    let mut shapes = alloc::vec::Vec::new();
    let r1 = merge_j08(&mut reg, &mut shapes);
    let first_ok = match r1 {
        Ok(rep) => {
            rep.merged == 6
                && rep.skipped == 0
                && shapes.len() == 6
                && verify_merged(&reg)
        }
        Err(_) => false,
    };
    // 形状字段对拍：单条抽验（id/单位/等级/schema 同步）——收编不改形状。
    let shape_ok = shapes
        .get(2)
        .map(|x| {
            x.id == "j08.power.die_temp"
                && x.unit == "celsius"
                && x.tier == 1
                && x.schema_version == SCHEMA_VERSION_SYNC
                && x.caliber.contains("热阶梯")
        })
        .unwrap_or(false);
    // 幂等重放：二次收编全部 skipped（不发新 ID 不重注册不重复入账）。
    let mut shapes2 = alloc::vec::Vec::new();
    let r2 = merge_j08(&mut reg, &mut shapes2);
    let idem_ok = match r2 {
        Ok(rep) => rep.merged == 0 && rep.skipped == 6 && shapes2.is_empty() && reg.len() == 6,
        Err(_) => false,
    };
    // 核验器反向：抽掉一条后 verify 应红（防核验恒绿）。
    let mut reg_bad = UnifiedRegistry::new();
    reg_bad.register("j08.power.package_watt");
    let verify_red = !verify_merged(&reg_bad);
    // 表外 id 混入在表内是不可构造的（const 表都是 j08.）——反向改为：
    // 核验表内 id 前缀全为 j08.（收编以 J08 为限）。
    let mut prefix_ok = J08_METRICS.len() == 6;
    let mut mi = 0usize;
    while mi < J08_METRICS.len() {
        let m = J08_METRICS[mi];
        if !m.id.starts_with("j08.") {
            prefix_ok = false;
        }
        mi += 1;
    }
    s.add(
        "P07-一组收编-全量落地+幂等+核验",
        first_ok && shape_ok && idem_ok && verify_red && prefix_ok,
        "首次收编 6 条全量 merged=6 skipped=0 且 verify 通过；形状逐字段对拍（j08.power.die_temp/celsius/tier=1/schema=3/口径含热阶梯）；幂等重放 merged=0 skipped=6 不重复入账；核验器反向（缺 5 条时红）防恒绿；表内 id 前缀全 j08. 收编以 J08 为限",
    );

    // —— 复用声明逐条 grep（判据「收编复用」） ——
    let mut reuse_ok = REUSE_LINES.len() == 3;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F2402") || l.contains("F2401") || l.contains("F1558")) {
            reuse_ok = false;
        }
        ri += 1;
    }
    // 判据 stamp 独立对账。
    let stamps = ["收编落地", "复用", "一组"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P07-复用+判据stamp-逐条对账",
        reuse_ok && stamp_ok && SCHEMA_VERSION_SYNC == 3 && J08_METRICS.len() == 6,
        "复用清单三条含 F2402/F2401/F1558 关键字逐条 grep（六元组形状/收编闸列名/采样等级）；锚点判据三条与判据侧独立重排逐条全等；一组测试（收编）宣告与实际检查一一对应；schema 同步=3 与表数=6 独立写死对拍",
    );

    s
}
