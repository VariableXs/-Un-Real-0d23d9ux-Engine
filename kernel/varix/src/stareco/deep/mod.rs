//! 深化层聚合器（2026-09-26 回炉补深化批）。
//!
//! 二十项深化层逐项一文件（`f1x xd.rs`），每项自带
//! `run_f1xx_deep_checks() -> CheckSet`。深化层只增不改：基础模块的
//! 判据面保持原样，深化判据在此聚合，随域聚合器一并入总账。

use crate::checks::CheckSet;

pub mod f131d;
pub mod f132d;
pub mod f133d;
pub mod f134d;
pub mod f135d;
pub mod f136d;
pub mod f137d;
pub mod f138d;
pub mod f139d;
pub mod f140d;
pub mod f141d;
pub mod f142d;
pub mod f143d;
pub mod f144d;
pub mod f145d;
pub mod f146d;
pub mod f147d;
pub mod f148d;
pub mod f149d;
pub mod f150d;

/// 深化域标识。
pub const DEEP_DOMAIN: &str = "stareco-v2-deep";

/// 深化层自检聚合：20 blocks。
pub fn run_stareco_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(DEEP_DOMAIN);
    let blocks: [(&'static str, CheckSet); 20] = [
        ("F131d", f131d::run_f131_deep_checks()),
        ("F132d", f132d::run_f132_deep_checks()),
        ("F133d", f133d::run_f133_deep_checks()),
        ("F134d", f134d::run_f134_deep_checks()),
        ("F135d", f135d::run_f135_deep_checks()),
        ("F136d", f136d::run_f136_deep_checks()),
        ("F137d", f137d::run_f137_deep_checks()),
        ("F138d", f138d::run_f138_deep_checks()),
        ("F139d", f139d::run_f139_deep_checks()),
        ("F140d", f140d::run_f140_deep_checks()),
        ("F141d", f141d::run_f141_deep_checks()),
        ("F142d", f142d::run_f142_deep_checks()),
        ("F143d", f143d::run_f143_deep_checks()),
        ("F144d", f144d::run_f144_deep_checks()),
        ("F145d", f145d::run_f145_deep_checks()),
        ("F146d", f146d::run_f146_deep_checks()),
        ("F147d", f147d::run_f147_deep_checks()),
        ("F148d", f148d::run_f148_deep_checks()),
        ("F149d", f149d::run_f149_deep_checks()),
        ("F150d", f150d::run_f150_deep_checks()),
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
