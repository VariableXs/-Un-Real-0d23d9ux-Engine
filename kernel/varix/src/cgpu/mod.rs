//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。

pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
    crate::checks::CheckSet::merge(
        cgi01_bandwidth_checks::run_cgi01_checks(),
    )
}
