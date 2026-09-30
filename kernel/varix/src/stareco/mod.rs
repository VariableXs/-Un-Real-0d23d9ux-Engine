//! Varix STAR I · 泳道四 D 生态开放域·后段（AI-V2 · F131-F150）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-V2 分队的施工落位：
//! 二十项功能（F131-F150）逐项一模块，共享底盘收在 [`ebase`]（追踪
//! 编号、序号链台账、五态状态机、限频器、指纹、覆盖度）。与既有内核
//! 模块的关系纪律：
//!
//! - 旧代 F 编号与本目录的 STAR I 新编号**同号不同义**——本目录所有
//!   模块的判据一律以《Varix STAR I start.md》主册为准（G-D-06~
//!   G-D-25 段），引用格式 `F1xx` 均指 STAR I 语义；
//! - 依赖锚点指向的未落地分队功能（F040 账本、F120 诊断、F129 判例
//!   五态等）一律以**显式参数/闭包注入口**承接，不反向制造对未落地
//!   模块的编译依赖（F129 五态引擎暂由 ebase::State5 供面，F129 落地
//!   时按「一处一事实」收编为唯一实现）；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合
//!   器）与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`upstream`]    | F131 上游回馈通道 | 首批 ≥2 件在册；模板/CLA/礼节三件 |
//! | [`difftable`]   | F132 差异表公开 | 与账本零矛盾 CI；逐条追踪编号 |
//! | [`iconpack`]    | F133 第三方图标包规范 | 官方 2 套过；社区 5 套报错逐条准 |
//! | [`themeshare`]  | F134 主题分享页 | 首批 10 套；三不承诺宪法句 |
//! | [`devportal`]   | F135 开发者文档站 | 五区清单全绿；提取覆盖 >90% |
//! | [`examples`]    | F136 示例应用仓库 | CI 连绿 30 天；新手 5/5 |
//! | [`apistab`]     | F137 API 稳定性承诺 | 100% 标注；假变更被拦；迁移窗兑现 |
//! | [`releasecal`]  | F138 版本发布节奏公开 | 两窗兑现/如实归档；公告提前 ≥1 窗 |
//! | [`feedbackloop`]| F139 反馈闭环通道 | 端到端演练；脱敏强制 |
//! | [`l10nopen`]    | F140 本地化开放 | 双语零漏翻；包 round-trip 无损 |
//! | [`a11yopen`]    | F141 无障碍开放标准 | 新应用一次过门禁；判定一致率 100% |
//! | [`secdisclose`] | F142 安全披露通道 | security.txt 过校验；时限达标 |
//! | [`brandkit`]    | F143 星徽与品牌资产包 | 全分辨率齐（含 4K）；条款过审 |
//! | [`craftbadge`]  | F144 「Crafted for VARIX」徽标 | 3 应用演练；验真双向；可撤销 |
//! | [`eduportfolio`]| F145 教育/作品集友好 | 20 篇公开；脱敏三查 100% |
//! | [`starmapprov`] | F146 插件化星图后端 | 三源一致；回退链；签名拦截 |
//! | [`syncroam`]    | F147 跨设备主题同步 | 插拔全链；20 档自适应；仅本次零残留 |
//! | [`governance`]  | F148 社区规则与治理 | 三文档过审；仲裁演练；裁决可查 |
//! | [`ecoreport`]   | F149 季度生态报告 | 连续两季按期；四指标交叉一致 |
//! | [`ecogate`]     | F150 生态域总判据 | 首轮 30 分钟线；逐季不回退 |

use crate::checks::CheckSet;

pub mod a11yopen;
pub mod apistab;
pub mod brandkit;
pub mod craftbadge;
pub mod deep;
pub mod devportal;
pub mod difftable;
pub mod ebase;
pub mod ecogate;
pub mod ecoreport;
pub mod eduportfolio;
pub mod examples;
pub mod feedbackloop;
pub mod governance;
pub mod iconpack;
pub mod l10nopen;
pub mod releasecal;
pub mod secdisclose;
pub mod starmapprov;
pub mod syncroam;
pub mod themeshare;
pub mod upstream;

/// 域标识（CheckSet 聚合用）。
pub const STARECO_DOMAIN: &str = "stareco-v2";

/// 本域自检聚合：基础层 20 项 + ebase（21 blocks）+ 深化层 20 项
/// （deep 1 block，其内部 20 子行展开）= 22 blocks。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——本聚合器
/// 按「每模块一行」登记，永不超容；单模块自身超限时由该模块负责裁剪。
pub fn run_stareco_checks() -> CheckSet {
    let mut set = CheckSet::new(STARECO_DOMAIN);
    let blocks: [(&'static str, CheckSet); 22] = [
        ("ebase", ebase::run_ebase_checks()),
        ("F131", upstream::run_upstream_checks()),
        ("F132", difftable::run_difftable_checks()),
        ("F133", iconpack::run_iconpack_checks()),
        ("F134", themeshare::run_themeshare_checks()),
        ("F135", devportal::run_devportal_checks()),
        ("F136", examples::run_examples_checks()),
        ("F137", apistab::run_apistab_checks()),
        ("F138", releasecal::run_releasecal_checks()),
        ("F139", feedbackloop::run_feedbackloop_checks()),
        ("F140", l10nopen::run_l10nopen_checks()),
        ("F141", a11yopen::run_a11yopen_checks()),
        ("F142", secdisclose::run_secdisclose_checks()),
        ("F143", brandkit::run_brandkit_checks()),
        ("F144", craftbadge::run_craftbadge_checks()),
        ("F145", eduportfolio::run_eduportfolio_checks()),
        ("F146", starmapprov::run_starmapprov_checks()),
        ("F147", syncroam::run_syncroam_checks()),
        ("F148", governance::run_governance_checks()),
        ("F149", ecoreport::run_ecoreport_checks()),
        ("F150", ecogate::run_ecogate_checks()),
        ("deep", deep::run_stareco_deep_checks()),
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
    fn stareco_domain_aggregate_all_green() {
        let set = run_stareco_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "STARECO-V2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
