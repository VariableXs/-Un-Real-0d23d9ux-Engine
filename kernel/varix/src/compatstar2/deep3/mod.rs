//! compatstar2/deep3 — 深化批次四（AI-C2 · F021-F040）。
//!
//! 批次四定位：批次一（run_*_deep）协议语义面；批次二（deep/）执行与治理
//! 面；批次三（deep2/）执行/边界/注入面；本批（f0NNf.rs）落 **序列化/
//! 账本/容错面**——内存诊断转储/时区规则表驱动/错误注入矩阵/证书链构建/
//! 假脱机与配额/混音总线/IME 词典学习/DPI 资源选型/拓扑变更检测/安装脚本
//! 求值/卸载顺序依赖/运行时环境账/构建缓存/流式转码状态/修复动作执行/
//! 星卡存储格式/策略规则编译/进程树隔离/帧账归因/账本开放导出——逐域一
//! 文件，判据摘自主册并登记于各文件头。
//!
//! 共同纪律（沿用批次一/二/三）：零堆热路径、一处一事实、异常零静默、
//! 数值真实，不与前四层语义重复。

pub mod f021f;
pub mod f022f;
pub mod f023f;
pub mod f024f;
pub mod f025f;
pub mod f026f;
pub mod f027f;
pub mod f028f;
pub mod f029f;
pub mod f030f;
pub mod f031f;
pub mod f032f;
pub mod f033f;
pub mod f034f;
pub mod f035f;
pub mod f036f;
pub mod f037f;
pub mod f038f;
pub mod f039f;
pub mod f040f;

/// 批次四全量自检（20 域聚合；域内红项如实记账——零静默）。
pub fn run_batch4_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("COMPAT-S2-D4");
    let blocks: [(&'static str, crate::checks::CheckSet); 20] = [
        ("F021f", f021f::run_f021f_checks()),
        ("F022f", f022f::run_f022f_checks()),
        ("F023f", f023f::run_f023f_checks()),
        ("F024f", f024f::run_f024f_checks()),
        ("F025f", f025f::run_f025f_checks()),
        ("F026f", f026f::run_f026f_checks()),
        ("F027f", f027f::run_f027f_checks()),
        ("F028f", f028f::run_f028f_checks()),
        ("F029f", f029f::run_f029f_checks()),
        ("F030f", f030f::run_f030f_checks()),
        ("F031f", f031f::run_f031f_checks()),
        ("F032f", f032f::run_f032f_checks()),
        ("F033f", f033f::run_f033f_checks()),
        ("F034f", f034f::run_f034f_checks()),
        ("F035f", f035f::run_f035f_checks()),
        ("F036f", f036f::run_f036f_checks()),
        ("F037f", f037f::run_f037f_checks()),
        ("F038f", f038f::run_f038f_checks()),
        ("F039f", f039f::run_f039f_checks()),
        ("F040f", f040f::run_f040f_checks()),
    ];
    for (tag, block) in blocks {
        let passed = block.all_passed() && !block.truncated();
        set.add(tag, passed, if passed { "" } else { "batch4 red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 批次四聚合全绿：二十子域逐项无红无截断。
    #[test]
    fn batch4_aggregate_all_green() {
        let set = run_batch4_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "批次四自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
