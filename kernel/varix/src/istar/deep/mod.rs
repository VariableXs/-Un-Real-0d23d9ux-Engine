//! istar 深化层聚合（对齐 stareco::deep 先例：域聚合器只加 1 块，
//! 深化子行在本聚合器内逐项展开——CheckSet 64 容量纪律不受影响）。
//!
//! 深化批次二（AI-U4 第二会话）：22 项逐项深化文件。
//! 深化批次三（AI-U4 第三会话）：剩余 28 项（F553/F559/F561/F564/F566/
//! F568/F570/F573/F574/F575/F577/F578/F580-F585/F587-F589/F591-F595/
//! F597/F600）逐项深化文件——至此 **F551-F600 五十项深化层全量在位**
//! （每项一个 `run_fXXX_deep_checks`，批次二头注登记的「28 项属宿主 UI
//! 层未深化」缺口自此闭合，机制账见各行头注与对账表）。

use crate::checks::CheckSet;

pub mod f551d;
pub mod f552d;
pub mod f553d;
pub mod f554d;
pub mod f555d;
pub mod f556d;
pub mod f557d;
pub mod f558d;
pub mod f559d;
pub mod f560d;
pub mod f561d;
pub mod f562d;
pub mod f563d;
pub mod f564d;
pub mod f565d;
pub mod f566d;
pub mod f567d;
pub mod f568d;
pub mod f569d;
pub mod f570d;
pub mod f571d;
pub mod f572d;
pub mod f573d;
pub mod f574d;
pub mod f575d;
pub mod f576d;
pub mod f577d;
pub mod f578d;
pub mod f579d;
pub mod f580d;
pub mod f581d;
pub mod f582d;
pub mod f583d;
pub mod f584d;
pub mod f585d;
pub mod f586d;
pub mod f587d;
pub mod f588d;
pub mod f589d;
pub mod f590d;
pub mod f591d;
pub mod f592d;
pub mod f593d;
pub mod f594d;
pub mod f595d;
pub mod f596d;
pub mod f597d;
pub mod f598d;
pub mod f599d;
pub mod f600d;

/// 深化域标识。
pub const ISTAR_DEEP: &str = "istar-u4-deep";

/// 深化层聚合：50 项逐项红绿（每项一个子行，50 ≤ 64 容量）。
pub fn run_istar_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DEEP);
    let blocks: [(&'static str, CheckSet); 50] = [
        ("F551d", f551d::run_f551_deep_checks()),
        ("F552d", f552d::run_f552_deep_checks()),
        ("F553d", f553d::run_f553_deep_checks()),
        ("F554d", f554d::run_f554_deep_checks()),
        ("F555d", f555d::run_f555_deep_checks()),
        ("F556d", f556d::run_f556_deep_checks()),
        ("F557d", f557d::run_f557_deep_checks()),
        ("F558d", f558d::run_f558_deep_checks()),
        ("F559d", f559d::run_f559_deep_checks()),
        ("F560d", f560d::run_f560_deep_checks()),
        ("F561d", f561d::run_f561_deep_checks()),
        ("F562d", f562d::run_f562_deep_checks()),
        ("F563d", f563d::run_f563_deep_checks()),
        ("F564d", f564d::run_f564_deep_checks()),
        ("F565d", f565d::run_f565_deep_checks()),
        ("F566d", f566d::run_f566_deep_checks()),
        ("F567d", f567d::run_f567_deep_checks()),
        ("F568d", f568d::run_f568_deep_checks()),
        ("F569d", f569d::run_f569_deep_checks()),
        ("F570d", f570d::run_f570_deep_checks()),
        ("F571d", f571d::run_f571_deep_checks()),
        ("F572d", f572d::run_f572_deep_checks()),
        ("F573d", f573d::run_f573_deep_checks()),
        ("F574d", f574d::run_f574_deep_checks()),
        ("F575d", f575d::run_f575_deep_checks()),
        ("F576d", f576d::run_f576_deep_checks()),
        ("F577d", f577d::run_f577_deep_checks()),
        ("F578d", f578d::run_f578_deep_checks()),
        ("F579d", f579d::run_f579_deep_checks()),
        ("F580d", f580d::run_f580_deep_checks()),
        ("F581d", f581d::run_f581_deep_checks()),
        ("F582d", f582d::run_f582_deep_checks()),
        ("F583d", f583d::run_f583_deep_checks()),
        ("F584d", f584d::run_f584_deep_checks()),
        ("F585d", f585d::run_f585_deep_checks()),
        ("F586d", f586d::run_f586_deep_checks()),
        ("F587d", f587d::run_f587_deep_checks()),
        ("F588d", f588d::run_f588_deep_checks()),
        ("F589d", f589d::run_f589_deep_checks()),
        ("F590d", f590d::run_f590_deep_checks()),
        ("F591d", f591d::run_f591_deep_checks()),
        ("F592d", f592d::run_f592_deep_checks()),
        ("F593d", f593d::run_f593_deep_checks()),
        ("F594d", f594d::run_f594_deep_checks()),
        ("F595d", f595d::run_f595_deep_checks()),
        ("F596d", f596d::run_f596_deep_checks()),
        ("F597d", f597d::run_f597_deep_checks()),
        ("F598d", f598d::run_f598_deep_checks()),
        ("F599d", f599d::run_f599_deep_checks()),
        ("F600d", f600d::run_f600_deep_checks()),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}
