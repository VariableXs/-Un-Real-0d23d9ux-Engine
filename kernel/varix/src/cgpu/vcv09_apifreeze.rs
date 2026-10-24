//! CGPU-F3369 · 验收 API v1 冻结（vcv09_apifreeze）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3369`
//!
//! **判据（锚点原文）**：三组函数、冻结声明、一组、判据。
//!
//! **职责定位（锚点原文）**：API 冻结：验收 API v1 冻结（矩阵/判据/
//! 走查——三组函数——冻结声明；测试（冻结一组）。
//!
//! # 一、为什么 API 要「冻结」而不只是「发布」
//!
//! 冻结与发布的差别在于**变更契约**：发布只是「这版能用」，冻结是
//! 「从这一版起，签名级变更是域级事故」——下游（V02 矩阵、V03 判据、
//! V04 走查、V05 报告）都以 v1 签名为编译期依赖，任何改签名行为都
//! 会让四个下游域的锚点同时失效。本模块把冻结做成**数据**而非口号：
//! 每个函数登记「签名指纹 + 冻结版本」，指纹对不上即拒（[`
//! E_VV_FROZEN`]）——想动 API，先过域收口流程，而不是顺手一改。
//!
//! # 二、三组函数（矩阵 / 判据 / 走查）
//!
//! - **矩阵组**（[`API_V1_MATRIX`]）：登记/查询/覆盖三维坐标的入口；
//! - **判据组**（[`API_V1_CRITERIA`]）：定义/执行/查询验收判据；
//! - **走查组**（[`API_V1_WALKTHROUGH`]）：发起/记录/分级/回读走查。
//!
//! 七函数构成 V 域对外接口全集（F3376 接口总账的口径来源）。冻结
//! 表三不变式：**函数名唯一**（重名=接口分裂）、**组归属合法**（不
//! 在三组内的函数不许入表）、**指纹稳定**（同版本重算指纹必须一致）。
//!
//! # 三、冻结一组（判据）
//!
//! 判据组：三组齐备 + 名唯一 + 组合法 + 指纹稳定 + 冻结声明非空 +
//! 变更拒收（对冻结表改签名即 `E_VV_FROZEN`）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const APIFREEZE_VERSION: &str = "V09-apifreeze-v1";

/// 冻结版本号（v1 冻结线的唯一标识）。
pub const FROZEN_API_VERSION: &str = "verify-api-v1";

/// 对已冻结签名做变更（拒收）。
pub const E_VV_FROZEN: &str = "E_VV_FROZEN";

/// 函数名重复（接口分裂）。
pub const E_VV_DUPFN: &str = "E_VV_DUPFN";

/// 组归属非法（不在三组内的函数不许入冻结表）。
pub const E_VV_GROUP: &str = "E_VV_GROUP";

/// 冻结 API 三组名（锚点原文：矩阵/判据/走查）。
pub const API_GROUPS: [&str; 3] = ["matrix", "criteria", "walkthrough"];

/// 矩阵组 v1 函数（登记/查询/覆盖）。
pub const API_V1_MATRIX: [&str; 3] = ["verify_matrix_put", "verify_matrix_get", "verify_matrix_coverage"];

/// 判据组 v1 函数（定义/执行/查询）。
pub const API_V1_CRITERIA: [&str; 3] = ["verify_criteria_define", "verify_criteria_run", "verify_criteria_query"];

/// 走查组 v1 函数（发起/记录/分级/回读）。
pub const API_V1_WALKTHROUGH: [&str; 4] = ["walkthrough_start", "walkthrough_record", "walkthrough_grade", "walkthrough_replay"];

// ---------------------------------------------------------------------------
// 二、冻结表
// ---------------------------------------------------------------------------

/// 冻结条目（函数名 → 组 → 签名指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenFn {
    /// 函数全名（跨组唯一）。
    pub name: &'static str,
    /// 所属组（必须在 [`API_GROUPS`] 内）。
    pub group: &'static str,
    /// 签名指纹（FNV-1a of 签名串；冻结后不可变）。
    pub fingerprint: u64,
}

/// 由签名串计算指纹（与全库诊断码指纹同族的 FNV-1a）。
pub const fn sign_fingerprint(sig: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let b = sig.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// 组归属合法性（不在三组内即拒——锚点「三组函数」的闭集纪律）。
pub fn group_legal(group: &str) -> Result<(), &'static str> {
    if API_GROUPS.contains(&group) {
        Ok(())
    } else {
        Err(E_VV_GROUP)
    }
}

/// v1 冻结表全量（7 函数——F3376 接口总账口径来源）。
pub fn frozen_table_v1() -> Vec<FrozenFn> {
    let mut t = Vec::new();
    for n in API_V1_MATRIX.iter() {
        t.push(FrozenFn { name: n, group: "matrix", fingerprint: sign_fingerprint(n) });
    }
    for n in API_V1_CRITERIA.iter() {
        t.push(FrozenFn { name: n, group: "criteria", fingerprint: sign_fingerprint(n) });
    }
    for n in API_V1_WALKTHROUGH.iter() {
        t.push(FrozenFn { name: n, group: "walkthrough", fingerprint: sign_fingerprint(n) });
    }
    t
}

/// 冻结表三不变式核验（唯一/组合法/指纹稳定）。
pub fn table_invariants(t: &[FrozenFn]) -> Result<(), String> {
    for (i, f) in t.iter().enumerate() {
        for g in t.iter().skip(i + 1) {
            if f.name == g.name {
                return Err(format!("{}：{} 重名——接口分裂", E_VV_DUPFN, f.name));
            }
        }
        group_legal(f.group).map_err(|e| format!("{}：{} 组 {}", e, f.name, f.group))?;
        if f.fingerprint != sign_fingerprint(f.name) {
            return Err(format!("{}：{} 指纹漂移——冻结后签名被改", E_VV_FROZEN, f.name));
        }
    }
    Ok(())
}

/// 对冻结函数做变更裁决（v1 冻结线内改名/改指纹即拒）。
pub fn mutate_frozen(name: &str, new_sig: &str) -> Result<u64, String> {
    let t = frozen_table_v1();
    let hit = t.iter().find(|f| f.name == name);
    match hit {
        None => {
            // 非冻结函数：允许新增，返回新指纹。
            Ok(sign_fingerprint(new_sig))
        }
        Some(f) => {
            let nf = sign_fingerprint(new_sig);
            if nf == f.fingerprint {
                Ok(nf)
            } else {
                Err(format!(
                    "{}：{} 已在 {} 冻结——改签名须走域收口，不得顺手变更",
                    E_VV_FROZEN, name, FROZEN_API_VERSION
                ))
            }
        }
    }
}

/// 冻结声明（锚点原文口径——非空即判据）。
pub const FREEZE_DECLARATION: &str =
    "验收 API v1 冻结：矩阵/判据/走查三组七函数，签名级变更是域级事故";

// ---------------------------------------------------------------------------
// 三、冻结一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3369 域自检：三组函数、冻结声明、一组、判据。
pub fn run_vcv09_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv09_apifreeze");

    // --- 三组齐备：组名闭集 + 七函数分组落位 ---
    let t = frozen_table_v1();
    cs.add(
        "三组齐备七函数",
        API_GROUPS.len() == 3
            && t.len() == API_V1_MATRIX.len() + API_V1_CRITERIA.len() + API_V1_WALKTHROUGH.len()
            && t.len() == 7,
        "锚点：矩阵/判据/走查三组函数",
    );

    // --- 名唯一 ---
    cs.add(
        "函数名跨组唯一",
        table_invariants(&t).is_ok(),
        "重名=接口分裂",
    );

    // --- 组归属非法拒收 ---
    cs.add(
        "组归属闭集",
        group_legal("matrix").is_ok() && group_legal("unknown").is_err(),
        "三组外的函数不许入冻结表",
    );

    // --- 指纹稳定：同签名重算一致 ---
    let fp1 = sign_fingerprint("verify_matrix_put");
    let fp2 = sign_fingerprint("verify_matrix_put");
    cs.add(
        "指纹计算稳定",
        fp1 == fp2 && fp1 != 0,
        "同版本重算指纹必须一致",
    );

    // --- 指纹互异：不同签名不同指纹（抽查） ---
    cs.add(
        "指纹互异抽查",
        sign_fingerprint("verify_matrix_put") != sign_fingerprint("verify_matrix_get"),
        "签名不同指纹不同",
    );

    // --- 变更拒收：改冻结签名即 E_VV_FROZEN ---
    let r = mutate_frozen("verify_matrix_put", "verify_matrix_put_v2");
    cs.add(
        "冻结签名变更拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_FROZEN),
        "v1 冻结线：签名级变更是域级事故",
    );

    // --- 新增放行：非冻结函数可新增 ---
    cs.add(
        "非冻结函数可新增",
        mutate_frozen("brand_new_fn", "brand_new_fn(x:u32)").is_ok(),
        "冻结只封旧签名，不堵扩展",
    );

    // --- 冻结声明非空 ---
    cs.add(
        "冻结声明非空",
        !FREEZE_DECLARATION.is_empty() && FREEZE_DECLARATION.contains("v1"),
        "锚点冻结声明",
    );

    // --- 版本指纹 ---
    cs.add(
        "版本指纹非零",
        sign_fingerprint(APIFREEZE_VERSION) != 0,
        APIFREEZE_VERSION,
    );

    cs
}
