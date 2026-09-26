//! compatstar2/deep — 深化批次二（AI-C2 · F021-F040）。
//!
//! 批次二定位：批次一（run_*_deep）覆盖主册【设计细节】的协议语义面；
//! 本批（f0NNd.rs）补齐主册【功能定义】「全语义对齐」口径的执行与治理面
//! ——堆句柄族/历法核/字节序/通配符/PDF 结构/DS 环形锁/属性段压缩/DPI
//! 三出口/枚举索引/MSI 特性树/卸载差集/PEP503/依赖闭包/UTF-8 编码器/
//! 失败指纹/JSONL 帧/布隆前置/进程代际/回程账/快照差分，逐域一文件，
//! 判据摘自主册并登记于各文件头。
//!
//! 共同纪律（沿用批次一）：零堆热路径、一处一事实、异常零静默、
//! 历法/比较器等公共底座只此一份（跨文件复用 f022d/runtimes 面）。

pub mod f021d;
pub mod f022d;
pub mod f023d;
pub mod f024d;
pub mod f025d;
pub mod f026d;
pub mod f027d;
pub mod f028d;
pub mod f029d;
pub mod f030d;
pub mod f031d;
pub mod f032d;
pub mod f033d;
pub mod f034d;
pub mod f035d;
pub mod f036d;
pub mod f037d;
pub mod f038d;
pub mod f039d;
pub mod f040d;

/// 批次二全量自检（20 域聚合；域内红项如实记账——零静默）。
pub fn run_batch2_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("COMPAT-S2-D2");
    let blocks: [(&'static str, crate::checks::CheckSet); 20] = [
        ("F021d", f021d::run_f021d_checks()),
        ("F022d", f022d::run_f022d_checks()),
        ("F023d", f023d::run_f023d_checks()),
        ("F024d", f024d::run_f024d_checks()),
        ("F025d", f025d::run_f025d_checks()),
        ("F026d", f026d::run_f026d_checks()),
        ("F027d", f027d::run_f027d_checks()),
        ("F028d", f028d::run_f028d_checks()),
        ("F029d", f029d::run_f029d_checks()),
        ("F030d", f030d::run_f030d_checks()),
        ("F031d", f031d::run_f031d_checks()),
        ("F032d", f032d::run_f032d_checks()),
        ("F033d", f033d::run_f033d_checks()),
        ("F034d", f034d::run_f034d_checks()),
        ("F035d", f035d::run_f035d_checks()),
        ("F036d", f036d::run_f036d_checks()),
        ("F037d", f037d::run_f037d_checks()),
        ("F038d", f038d::run_f038d_checks()),
        ("F039d", f039d::run_f039d_checks()),
        ("F040d", f040d::run_f040d_checks()),
    ];
    for (tag, block) in blocks {
        let passed = block.all_passed() && !block.truncated();
        set.add(tag, passed, if passed { "" } else { "batch2 red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 批次二聚合全绿：二十子域逐项无红无截断。
    #[test]
    fn batch2_aggregate_all_green() {
        let set = run_batch2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "批次二自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
