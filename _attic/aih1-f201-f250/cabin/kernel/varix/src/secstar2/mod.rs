//! Varix STAR I · 泳道一 G 安全加固域·后段（AI-S2 · F186-F200）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-S2 分队的施工落位：
//! 十五项功能（F186-F200）逐项一模块。与既有内核模块的关系纪律：
//!
//! - 旧代 F 编号（AI-01..AI-10 等历史计划的 F001-F700）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以《Varix STAR I
//!   start.md》主册为准，引用格式 `F1xx` 均指 STAR I 语义（主册 G-G-16~
//!   G-G-30 段）；
//! - 与 AI-S1（F171-F185，`secstar/` 目录）同泳道不共目录：分队落位互相
//!   独立，不依赖对方未注册的代码；跨分队接缝一律以**显式参数注入口**
//!   承接（如 F186 的哈希态来自 F191 自查，由调用方注入）；
//! - 与 K 分队共享底盘（`crate::star::sbase` 的旋钮/环账/分位工具）直接
//!   复用——零冗余纪律，一处一事实；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`syspart`]    | F186 系统分区不可见 | 零枚举实测；管理页信息准确；异常红显 |
//! | [`clockguard`] | F187 时区与时钟守护 | 漂移注入 60s 校正；时区全链；推断标注 |
//! | [`logring`]    | F188 日志三环       | 对齐 ±50ms；轮转零丢；10 万条流畅 |
//! | [`selfheal2`]  | F189 静默自愈集     | 三类注入 5 次全愈；报备 100%；升级触发 |
//! | [`slotview`]   | F190 更新双槽可视   | 槽态对拍；回滚全链；到期灰置 |
//! | [`bootaudit`]  | F191 安全启动链自查 | 三注入全拦；自查 <100ms；恢复链通 |
//! | [`paramwl`]    | F192 内核参数白名单 | 三族全通；20 非法全拒+建议；钳制实测 |
//! | [`safemode`]   | F193 安全模式       | 进-修-退全链；白名单外灰置；异常关机询问 |
//! | [`auditchain`] | F194 审计日志完整性 | 三篡改全检出；第三方可验；开销 <1% |
//! | [`resquota`]   | F195 资源配额执行   | 四级阶梯顺序正确；帧率不掉；豁免零误伤 |
//! | [`batguard`]   | F196 电池保护策略   | 两级阈值实测；取消即时；账目完整 |
//! | [`thermgov`]   | F197 温度感知降档   | 三档触发实测；graceful 跳过；回落归因 |
//! | [`recenv`]     | F198 恢复环境       | 三卡全流程；两级降级；零写问题盘 |
//! | [`lineage`]    | F199 版本与谱系页   | 谱系对拍；复制保真；清单跳转 |
//! | [`walkall`]    | F200 全域总检       | 覆盖率 100%；季检 <2h；增补走 ADR |

use crate::checks::CheckSet;

pub mod auditchain;
pub mod batguard;
pub mod bootaudit;
pub mod clockguard;
pub mod logring;
pub mod lineage;
pub mod paramwl;
pub mod recenv;
pub mod resquota;
pub mod safemode;
pub mod selfheal2;
pub mod slotview;
pub mod syspart;
pub mod thermgov;
pub mod walkall;

/// 域标识（CheckSet 聚合用）。
pub const SECSTAR2_DOMAIN: &str = "secstar-s2";

/// 本域自检聚合：逐模块 `run_*_checks` + `run_*_deep_checks` 汇总（v2 深化
/// 批次起深检随基检同表登记——每模块两行）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——15 模块 × 2 行
/// = 30 行，余量充足；单模块自身超限时由该模块负责裁剪。
pub fn run_secstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(SECSTAR2_DOMAIN);
    let blocks: [(&'static str, CheckSet, CheckSet); 15] = [
        ("F186", syspart::run_syspart_checks(), syspart::run_syspart_deep_checks()),
        ("F187", clockguard::run_clockguard_checks(), clockguard::run_clockguard_deep_checks()),
        ("F188", logring::run_logring_checks(), logring::run_logring_deep_checks()),
        ("F189", selfheal2::run_selfheal2_checks(), selfheal2::run_selfheal2_deep_checks()),
        ("F190", slotview::run_slotview_checks(), slotview::run_slotview_deep_checks()),
        ("F191", bootaudit::run_bootaudit_checks(), bootaudit::run_bootaudit_deep_checks()),
        ("F192", paramwl::run_paramwl_checks(), paramwl::run_paramwl_deep_checks()),
        ("F193", safemode::run_safemode_checks(), safemode::run_safemode_deep_checks()),
        ("F194", auditchain::run_auditchain_checks(), auditchain::run_auditchain_deep_checks()),
        ("F195", resquota::run_resquota_checks(), resquota::run_resquota_deep_checks()),
        ("F196", batguard::run_batguard_checks(), batguard::run_batguard_deep_checks()),
        ("F197", thermgov::run_thermgov_checks(), thermgov::run_thermgov_deep_checks()),
        ("F198", recenv::run_recenv_checks(), recenv::run_recenv_deep_checks()),
        ("F199", lineage::run_lineage_checks(), lineage::run_lineage_deep_checks()),
        ("F200", walkall::run_walkall_checks(), walkall::run_walkall_deep_checks()),
    ];
    for (tag, sub, deep) in blocks {
        let base_ok = sub.all_passed() && !sub.truncated();
        let deep_ok = deep.all_passed() && !deep.truncated();
        set.add(tag, base_ok && deep_ok, if base_ok && deep_ok { "" } else { "sub-checks red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secstar2_domain_aggregate_all_green() {
        let set = run_secstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "SECSTAR-S2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
