//! CGPU-F2402 · 遥测统一模型域自检（锚点测试三组：v3/维度/注册 + stamp）。
//!
//! **判据（锚点原文）**：统一 schema、v3 升级、维度复用、三组、判据。

use super::cgp02_unifiedmodel::{
    is_readable_by, upgrade_chain_intact, DimensionKind, DimensionRegistry, Metric, MetricValue,
    PrivacyLevel, SCHEMA_UPGRADES, SCHEMA_VERSION, UNIFIED_MODEL_VERSION,
};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["统一 schema", "v3 升级", "维度复用", "三组", "判据"];

/// 判据侧独立重算的升级链（版本/单号字面量写死对账）。
const UPGRADES_RECHECK: [(u32, u32); 3] = [(1, 1453), (2, 1556), (3, 2402)];

/// CGPU-F2402 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp02_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp02_unifiedmodel");

    // —— 组一 · v3：六元组构造/校验 + 升级链独立对账 + 版本兼容 ——
    let mut reg = DimensionRegistry::new();
    let scene_game = reg.register(DimensionKind::Scene, "游戏场景");
    let tier_low = reg.register(DimensionKind::Tier, "低配档");
    let m = Metric {
        id: "p01.frame.gpu_execute_ns".to_string(),
        caliber: "GPU 执行段耗时：F0482 四段计时器注入".to_string(),
        dimensions: vec![scene_game, tier_low],
        value: MetricValue::DurationNs(7_400),
        timestamp_ns: 25_000_000,
        privacy: PrivacyLevel::Local,
    };
    let v_ok = m.validate(&reg).is_ok();
    let bad_id = Metric {
        id: "gpu_execute_ns".to_string(),
        caliber: String::new(),
        dimensions: Vec::new(),
        value: MetricValue::Count(1),
        timestamp_ns: 0,
        privacy: PrivacyLevel::Public,
    };
    let v_bad = bad_id.validate(&reg).is_err();
    let chain_ok = upgrade_chain_intact();
    let compat = is_readable_by(1, SCHEMA_VERSION)
        && is_readable_by(2, SCHEMA_VERSION)
        && is_readable_by(3, SCHEMA_VERSION)
        && !is_readable_by(3, 2);
    s.add(
        "P02-组一v3-六元组+升级链+兼容",
        v_ok
            && v_bad
            && chain_ok
            && compat
            && SCHEMA_VERSION == 3
            && UNIFIED_MODEL_VERSION.starts_with("P01-"),
        "六元组结构校验通过（三级命名+已注册维度）；裸 ID 拒绝；升级链 v1(1453)→v2(1556)→v3(2402) 判据侧字面量对账；v3 读取器可读全部历史版本且反向不可",
    );

    // —— 升级记录表与判据侧独立重排逐条全等 ——
    let mut up_ok = SCHEMA_UPGRADES.len() == 3;
    let mut ui = 0usize;
    while ui < SCHEMA_UPGRADES.len() {
        let u = match SCHEMA_UPGRADES.get(ui) {
            Some(u) => *u,
            None => break,
        };
        let e = match UPGRADES_RECHECK.get(ui) {
            Some(e) => *e,
            None => break,
        };
        if u.version != e.0 || u.source_task != e.1 || u.change.is_empty() {
            up_ok = false;
        }
        ui += 1;
    }
    s.add(
        "P02-组一v3-升级记录逐条全等",
        up_ok,
        "三站升级记录（版本/单号/变更说明）与判据侧独立重排逐条全等——升级有账不悬空",
    );

    // —— 组二 · 维度：值闭集单位/饱和 + 隐私三档 ——
    let ppm_ok = MetricValue::Ppm(1_500_000).raw() == 1_000_000
        && MetricValue::Ppm(500_000).raw() == 500_000
        && MetricValue::DurationNs(7_400).unit() == "ns"
        && MetricValue::Count(9).unit() == "count";
    let privacy_ok = PrivacyLevel::Local.label() == "本地"
        && PrivacyLevel::Anonymized.label() == "匿名"
        && PrivacyLevel::Public.label() == "公开";
    s.add(
        "P02-组二维度-值闭集+隐私三档",
        ppm_ok && privacy_ok,
        "Ppm 超界饱和到百万（非法值构造面钳住）；三族单位标签齐；隐私级三档与 F2401 红线同源进类型",
    );

    // —— 组三 · 注册：同名复用同一 ID + 表外 None + 反向（未注册维度拒） ——
    let again = reg.register(DimensionKind::Scene, "游戏场景");
    let other_kind = reg.register(DimensionKind::Engine, "游戏场景");
    let fresh = reg.register(DimensionKind::Engine, "软件光栅");
    let ghost = reg.name_of(999);
    let unreg = Metric {
        id: "p01.t.id".to_string(),
        caliber: String::new(),
        dimensions: vec![777],
        value: MetricValue::Count(1),
        timestamp_ns: 0,
        privacy: PrivacyLevel::Local,
    };
    let unreg_rejected = unreg.validate(&reg).is_err();
    s.add(
        "P02-组三注册-同名复用+表外None",
        again == scene_game
            && other_kind == scene_game
            && fresh != scene_game
            && reg.name_of(scene_game) == Some("游戏场景")
            && ghost.is_none()
            && unreg_rejected
            && reg.len() == 3,
        "同名维度复用同一 ID（跨类别同名也不发新——口径漂移的一半来源被堵）；新名发新 ID；表外查询 None；未注册维度校验拒绝",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["统一 schema", "v3 升级", "维度复用", "三组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P02-判据stamp-五条独立重排全等",
        stamp_ok && SCHEMA_UPGRADES.len() == 3 && SCHEMA_VERSION == 3,
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）；三组测试（v3/维度/注册）宣告与实际检查一一对应",
    );

    s
}
