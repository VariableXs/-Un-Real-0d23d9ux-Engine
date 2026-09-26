//! Varix STAR I · 泳道四 D 服务守护域·前段（AI-V1 · F111-F130）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-V1 分队的施工落位：
//! 二十项功能（F111-F130）逐项一模块，共享底盘收在 [`vbase`]（SHA-256
//! /hex/WCAG 对比度/色温换算/semver/JSON 写出器/脱敏三查）。与既有内
//! 核模块的关系纪律（对齐 star/ 与 h3star/ 域既约）：
//!
//! - 旧代 F 编号（AI-01..AI-10 划分的 F001-F500）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以
//!   《Varix STAR I start.md》主册为准（G-C-41~G-C-55 / G-D-01~G-D-05）；
//! - 跨分队依赖（F056 脏区 / F057 下载调度 / F062 自检数据 / F076
//!   快速设置 / F079 音效通道 / F098 截图 / F101 天气 / F069/F190/F196
//!   等）未落地者一律以**显式参数/闭包注入口**承接，不反向制造编译
//!   依赖；已落地者直调公开接口；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑，注入钟确定
//!   复现）；本域模块零外部依赖（只依赖 `crate::checks` 与 alloc/core
//!   ——隔离舱可独立编译）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚（摘） |
//! | --- | --- | --- |
//! | [`vbase`]       | V1 共享底盘       | 散列/对比度/色温/semver/JSON/脱敏 |
//! | [`magnifier`]   | F111 放大镜       | 2x-16x 锐利；80fps 跟手；三跟随 |
//! | [`narrator`]    | F112 讲述人雏形   | 两场景 100% 可读名；快捷键全流程 |
//! | [`highcontrast`]| F113 高对比度主题 | 逐页 ≥7:1；两主题走查子集 |
//! | [`colorfilter`] | F114 色弱辅助滤镜 | 模拟矩阵对拍；帧增量 ≤0.5ms |
//! | [`focusmode`]   | F115 专注模式     | 四效果全链；统计对拍；500ms 防手滑 |
//! | [`nightlight`]  | F116 夜间模式     | ±200K 对拍；日落 ±5min；300ms 交叉 |
//! | [`oobe`]        | F117 首次开机向导 | 双路径；断电续走；汇总准确 |
//! | [`welcome`]     | F118 欢迎中心     | 五卡过审；直跳全对；不二弹 |
//! | [`helpcenter`]  | F119 帮助中心     | 50 页渲染；搜索 10/10；离线可用 |
//! | [`diagcenter`]  | F120 诊断中心     | 四灯一致；三修复回滚；脱敏三查 |
//! | [`restorept`]   | F121 系统还原点   | 四触发；哈希一致；断电百次 |
//! | [`updateux`]    | F122 更新体验面   | 全流程含预约；断电回滚；三要素 |
//! | [`aboutpage`]   | F123 关于本机页   | 规格全对；复制保真；诚实留白 |
//! | [`motioncore`]  | F124 动画曲线总谱 | 30 处抽查在谱；三强度档生效 |
//! | [`walkcheck`]   | F125 体验域总判据 | 覆盖率 100%；脚本生成即跑通 |
//! | [`openformat`]  | F126 开放格式宪法页 | 四规范页；示例包全过；双读条款 |
//! | [`vxapp`]       | F127 vxapp 打包工具 | 五形态全绿；安装闭环；分钟级 |
//! | [`stardata`]    | F128 星图开放数据面 | 下载-导入-查询闭环；签名双向 |
//! | [`casesub`]     | F129 社区判例提交线 | 端到端 <30 天；五态实时准确 |
//! | [`ossreg`]      | F130 开源项目登记册 | CI 零 diff；法律面在册 |

use crate::checks::CheckSet;

pub mod aboutpage;
pub mod casesub;
pub mod colorfilter;
pub mod diagcenter;
pub mod focusmode;
pub mod helpcenter;
pub mod highcontrast;
pub mod magnifier;
pub mod motioncore;
pub mod nightlight;
pub mod narrator;
pub mod oobe;
pub mod openformat;
pub mod ossreg;
pub mod restorept;
pub mod stardata;
pub mod updateux;
pub mod vbase;
pub mod vxapp;
pub mod walkcheck;
pub mod welcome;

/// 域标识（CheckSet 聚合用）。
pub const V1_DOMAIN: &str = "svstar-v1";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（vbase + 二十项全量）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限
/// 由该模块自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_svstar_checks() -> CheckSet {
    let mut set = CheckSet::new(V1_DOMAIN);
    let blocks: [(&'static str, CheckSet); 21] = [
        ("vbase", vbase::run_vbase_checks()),
        ("F111", magnifier::run_magnifier_checks()),
        ("F112", narrator::run_narrator_checks()),
        ("F113", highcontrast::run_highcontrast_checks()),
        ("F114", colorfilter::run_colorfilter_checks()),
        ("F115", focusmode::run_focusmode_checks()),
        ("F116", nightlight::run_nightlight_checks()),
        ("F117", oobe::run_oobe_checks()),
        ("F118", welcome::run_welcome_checks()),
        ("F119", helpcenter::run_helpcenter_checks()),
        ("F120", diagcenter::run_diagcenter_checks()),
        ("F121", restorept::run_restorept_checks()),
        ("F122", updateux::run_updateux_checks()),
        ("F123", aboutpage::run_aboutpage_checks()),
        ("F124", motioncore::run_motioncore_checks()),
        ("F125", walkcheck::run_walkcheck_checks()),
        ("F126", openformat::run_openformat_checks()),
        ("F127", vxapp::run_vxapp_checks()),
        ("F128", stardata::run_stardata_checks()),
        ("F129", casesub::run_casesub_checks()),
        ("F130", ossreg::run_ossreg_checks()),
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
    fn svstar_domain_aggregate_all_green() {
        let set = run_svstar_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "STAR-V1 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
