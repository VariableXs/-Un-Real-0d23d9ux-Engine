//! compatstar2/deep2 — 深化批次三（AI-C2 · F021-F040）。
//!
//! 批次三定位：批次一（run_*_deep）覆盖主册【设计细节】的协议语义面；
//! 批次二（deep/f0NNd.rs）补齐【功能定义】「全语义对齐」的执行与治理面；
//! 本批（f0NNe.rs）落 **执行/边界/注入面**——四态区域矩阵/时间转换矩阵/
//! 缓冲收发与就绪集/证书扩展/印刷参数协商/DS 环形缓冲/组合窗避让/DPI 传播/
//! 显示模式协商/安装事务日志/残留扫描启发式/版本解析/构建图/双字节引擎/
//! 日志归因/遥测聚合/审计链验证/档位流水线/交接会话/账本对拍——逐域一文件，
//! 判据摘自主册并登记于各文件头。
//!
//! 共同纪律（沿用批次一/二）：零堆热路径、一处一事实、异常零静默、
//! 数值真实（MS 常量/规范值），不与前三层语义重复。

pub mod f021e;
pub mod f022e;
pub mod f023e;
pub mod f024e;
pub mod f025e;
pub mod f026e;
pub mod f027e;
pub mod f028e;
pub mod f029e;
pub mod f030e;
pub mod f031e;
pub mod f032e;
pub mod f033e;
pub mod f034e;
pub mod f035e;
pub mod f036e;
pub mod f037e;
pub mod f038e;
pub mod f039e;
pub mod f040e;

/// 批次三全量自检（20 域聚合；域内红项如实记账——零静默）。
pub fn run_batch3_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("COMPAT-S2-D3");
    let blocks: [(&'static str, crate::checks::CheckSet); 20] = [
        ("F021e", f021e::run_f021e_checks()),
        ("F022e", f022e::run_f022e_checks()),
        ("F023e", f023e::run_f023e_checks()),
        ("F024e", f024e::run_f024e_checks()),
        ("F025e", f025e::run_f025e_checks()),
        ("F026e", f026e::run_f026e_checks()),
        ("F027e", f027e::run_f027e_checks()),
        ("F028e", f028e::run_f028e_checks()),
        ("F029e", f029e::run_f029e_checks()),
        ("F030e", f030e::run_f030e_checks()),
        ("F031e", f031e::run_f031e_checks()),
        ("F032e", f032e::run_f032e_checks()),
        ("F033e", f033e::run_f033e_checks()),
        ("F034e", f034e::run_f034e_checks()),
        ("F035e", f035e::run_f035e_checks()),
        ("F036e", f036e::run_f036e_checks()),
        ("F037e", f037e::run_f037e_checks()),
        ("F038e", f038e::run_f038e_checks()),
        ("F039e", f039e::run_f039e_checks()),
        ("F040e", f040e::run_f040e_checks()),
    ];
    for (tag, block) in blocks {
        let passed = block.all_passed() && !block.truncated();
        set.add(tag, passed, if passed { "" } else { "batch3 red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 批次三聚合全绿：二十子域逐项无红无截断。
    #[test]
    fn batch3_aggregate_all_green() {
        let set = run_batch3_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "批次三自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
