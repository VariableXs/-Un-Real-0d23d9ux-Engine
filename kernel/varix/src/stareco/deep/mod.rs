//! 深化层聚合器（2026-09-26 回炉补深化批）。
//!
//! 二十项深化层逐项三文件：批次一（`f1xxd.rs`——主册【设计细节】
//! 机制深化）+ 批次二（`f1xxe.rs`——【数据与存储】持久化面与
//! 【状态与异常】错误边界）+ 批次三（`f1xxf.rs`——工程化工具面：
//! 查询引擎/提取器/依赖图/tracker/三方合并/计时统计）。每文件自带
//! `run_f1xx_deep{,2,3}_checks() -> CheckSet`。深化层只增不改：基础
//! 模块的判据面保持原样，深化判据在此聚合，随域聚合器一并入总账。

use crate::checks::CheckSet;

pub mod f131d;
pub mod f131e;
pub mod f131f;
pub mod f132d;
pub mod f132e;
pub mod f132f;
pub mod f133d;
pub mod f133e;
pub mod f133f;
pub mod f134d;
pub mod f134e;
pub mod f134f;
pub mod f135d;
pub mod f135e;
pub mod f135f;
pub mod f136d;
pub mod f136e;
pub mod f136f;
pub mod f137d;
pub mod f137e;
pub mod f137f;
pub mod f138d;
pub mod f138e;
pub mod f138f;
pub mod f139d;
pub mod f139e;
pub mod f139f;
pub mod f140d;
pub mod f140e;
pub mod f140f;
pub mod f141d;
pub mod f141e;
pub mod f141f;
pub mod f142d;
pub mod f142e;
pub mod f142f;
pub mod f143d;
pub mod f143e;
pub mod f143f;
pub mod f144d;
pub mod f144e;
pub mod f144f;
pub mod f145d;
pub mod f145e;
pub mod f145f;
pub mod f146d;
pub mod f146e;
pub mod f146f;
pub mod f147d;
pub mod f147e;
pub mod f147f;
pub mod f148d;
pub mod f148e;
pub mod f148f;
pub mod f149d;
pub mod f149e;
pub mod f149f;
pub mod f150d;
pub mod f150e;
pub mod f150f;

/// 深化域标识。
pub const DEEP_DOMAIN: &str = "stareco-v2-deep";

/// 深化层自检聚合：批次一 20 blocks + 批次二 20 blocks + 批次三 20
/// blocks = 60 blocks。
pub fn run_stareco_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(DEEP_DOMAIN);
    let blocks: [(&'static str, CheckSet); 60] = [
        ("F131d", f131d::run_f131_deep_checks()),
        ("F131e", f131e::run_f131_deep2_checks()),
        ("F131f", f131f::run_f131_deep3_checks()),
        ("F132d", f132d::run_f132_deep_checks()),
        ("F132e", f132e::run_f132_deep2_checks()),
        ("F132f", f132f::run_f132_deep3_checks()),
        ("F133d", f133d::run_f133_deep_checks()),
        ("F133e", f133e::run_f133_deep2_checks()),
        ("F133f", f133f::run_f133_deep3_checks()),
        ("F134d", f134d::run_f134_deep_checks()),
        ("F134e", f134e::run_f134_deep2_checks()),
        ("F134f", f134f::run_f134_deep3_checks()),
        ("F135d", f135d::run_f135_deep_checks()),
        ("F135e", f135e::run_f135_deep2_checks()),
        ("F135f", f135f::run_f135_deep3_checks()),
        ("F136d", f136d::run_f136_deep_checks()),
        ("F136e", f136e::run_f136_deep2_checks()),
        ("F136f", f136f::run_f136_deep3_checks()),
        ("F137d", f137d::run_f137_deep_checks()),
        ("F137e", f137e::run_f137_deep2_checks()),
        ("F137f", f137f::run_f137_deep3_checks()),
        ("F138d", f138d::run_f138_deep_checks()),
        ("F138e", f138e::run_f138_deep2_checks()),
        ("F138f", f138f::run_f138_deep3_checks()),
        ("F139d", f139d::run_f139_deep_checks()),
        ("F139e", f139e::run_f139_deep2_checks()),
        ("F139f", f139f::run_f139_deep3_checks()),
        ("F140d", f140d::run_f140_deep_checks()),
        ("F140e", f140e::run_f140_deep2_checks()),
        ("F140f", f140f::run_f140_deep3_checks()),
        ("F141d", f141d::run_f141_deep_checks()),
        ("F141e", f141e::run_f141_deep2_checks()),
        ("F141f", f141f::run_f141_deep3_checks()),
        ("F142d", f142d::run_f142_deep_checks()),
        ("F142e", f142e::run_f142_deep2_checks()),
        ("F142f", f142f::run_f142_deep3_checks()),
        ("F143d", f143d::run_f143_deep_checks()),
        ("F143e", f143e::run_f143_deep2_checks()),
        ("F143f", f143f::run_f143_deep3_checks()),
        ("F144d", f144d::run_f144_deep_checks()),
        ("F144e", f144e::run_f144_deep2_checks()),
        ("F144f", f144f::run_f144_deep3_checks()),
        ("F145d", f145d::run_f145_deep_checks()),
        ("F145e", f145e::run_f145_deep2_checks()),
        ("F145f", f145f::run_f145_deep3_checks()),
        ("F146d", f146d::run_f146_deep_checks()),
        ("F146e", f146e::run_f146_deep2_checks()),
        ("F146f", f146f::run_f146_deep3_checks()),
        ("F147d", f147d::run_f147_deep_checks()),
        ("F147e", f147e::run_f147_deep2_checks()),
        ("F147f", f147f::run_f147_deep3_checks()),
        ("F148d", f148d::run_f148_deep_checks()),
        ("F148e", f148e::run_f148_deep2_checks()),
        ("F148f", f148f::run_f148_deep3_checks()),
        ("F149d", f149d::run_f149_deep_checks()),
        ("F149e", f149e::run_f149_deep2_checks()),
        ("F149f", f149f::run_f149_deep3_checks()),
        ("F150d", f150d::run_f150_deep_checks()),
        ("F150e", f150e::run_f150_deep2_checks()),
        ("F150f", f150f::run_f150_deep3_checks()),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_aggregate_all_green() {
        let set = run_stareco_deep_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "STARECO-V2 深化层自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
