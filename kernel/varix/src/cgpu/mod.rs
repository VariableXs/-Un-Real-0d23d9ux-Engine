//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。

pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
pub mod vcj01_powerarch;
pub mod vcj01_powerarch_checks;
//! vcj01_powerarch — CGPU-F1441 J 域开工与功耗架构总览（域使命/五主题十组/三处核验/五段流水线/边界/采样不耗样本/风险回退/0x52xx）

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {

    crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            cgi01_bandwidth_checks::run_cgi01_checks(),
            vcj01_powerarch_checks::run_vcj01_checks(),
        ),
    )
}
