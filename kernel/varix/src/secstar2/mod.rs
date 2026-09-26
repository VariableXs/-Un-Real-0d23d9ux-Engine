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
use alloc::vec::Vec;

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

/// 模块清单（聚合与对账共用一份——一处一事实）。
const MODULES: [(&str, &str); 15] = [
    ("F186", "syspart"),
    ("F187", "clockguard"),
    ("F188", "logring"),
    ("F189", "selfheal2"),
    ("F190", "slotview"),
    ("F191", "bootaudit"),
    ("F192", "paramwl"),
    ("F193", "safemode"),
    ("F194", "auditchain"),
    ("F195", "resquota"),
    ("F196", "batguard"),
    ("F197", "thermgov"),
    ("F198", "recenv"),
    ("F199", "lineage"),
    ("F200", "walkall"),
];

/// 本域自检聚合：逐模块七表（基检/深检/深2检/深3检/深4检/深5检/深6检）
/// 合并为一行——聚合器恒 15 行，单行绿=该模块七表全绿且均未截断。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——七表各自容量
/// 由 `check_ledger()` 逐块机检（全部 ≤64）；聚合器行数恒 15，永不超容。
///
/// 栈安全纪律：聚合改为循环内逐模块求值（函数指针 + 迭代结束即析构）——
/// 一次性持有全部 CheckSet 会溢出测试线程栈（v6 收口实测教训）。
pub fn run_secstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(SECSTAR2_DOMAIN);
    for (tag, module) in MODULES {
        let ok = seven_tables(module).iter().all(|f| {
            let cs = f();
            cs.all_passed() && !cs.truncated()
        });
        set.add(tag, ok, if ok { "" } else { "sub-checks red" });
    }
    set
}

// ---------------------------------------------------------------------------
// 检查项对账（机器钉数——报告引用的每个数字都由这里的断言保证）
// ---------------------------------------------------------------------------

/// 逐模块六表函数指针（表序=基检/深检/深2检/深3检/深4检/深5检）。
fn seven_tables(module: &str) -> [fn() -> CheckSet; 7] {
    match module {
        "syspart" => [syspart::run_syspart_checks, syspart::run_syspart_deep_checks, syspart::run_syspart_deep2_checks, syspart::run_syspart_deep3_checks, syspart::run_syspart_deep4_checks, syspart::run_syspart_deep5_checks, syspart::run_syspart_deep6_checks],
        "clockguard" => [clockguard::run_clockguard_checks, clockguard::run_clockguard_deep_checks, clockguard::run_clockguard_deep2_checks, clockguard::run_clockguard_deep3_checks, clockguard::run_clockguard_deep4_checks, clockguard::run_clockguard_deep5_checks, clockguard::run_clockguard_deep6_checks],
        "logring" => [logring::run_logring_checks, logring::run_logring_deep_checks, logring::run_logring_deep2_checks, logring::run_logring_deep3_checks, logring::run_logring_deep4_checks, logring::run_logring_deep5_checks, logring::run_logring_deep6_checks],
        "selfheal2" => [selfheal2::run_selfheal2_checks, selfheal2::run_selfheal2_deep_checks, selfheal2::run_selfheal2_deep2_checks, selfheal2::run_selfheal2_deep3_checks, selfheal2::run_selfheal2_deep4_checks, selfheal2::run_selfheal2_deep5_checks, selfheal2::run_selfheal2_deep6_checks],
        "slotview" => [slotview::run_slotview_checks, slotview::run_slotview_deep_checks, slotview::run_slotview_deep2_checks, slotview::run_slotview_deep3_checks, slotview::run_slotview_deep4_checks, slotview::run_slotview_deep5_checks, slotview::run_slotview_deep6_checks],
        "bootaudit" => [bootaudit::run_bootaudit_checks, bootaudit::run_bootaudit_deep_checks, bootaudit::run_bootaudit_deep2_checks, bootaudit::run_bootaudit_deep3_checks, bootaudit::run_bootaudit_deep4_checks, bootaudit::run_bootaudit_deep5_checks, bootaudit::run_bootaudit_deep6_checks],
        "paramwl" => [paramwl::run_paramwl_checks, paramwl::run_paramwl_deep_checks, paramwl::run_paramwl_deep2_checks, paramwl::run_paramwl_deep3_checks, paramwl::run_paramwl_deep4_checks, paramwl::run_paramwl_deep5_checks, paramwl::run_paramwl_deep6_checks],
        "safemode" => [safemode::run_safemode_checks, safemode::run_safemode_deep_checks, safemode::run_safemode_deep2_checks, safemode::run_safemode_deep3_checks, safemode::run_safemode_deep4_checks, safemode::run_safemode_deep5_checks, safemode::run_safemode_deep6_checks],
        "auditchain" => [auditchain::run_auditchain_checks, auditchain::run_auditchain_deep_checks, auditchain::run_auditchain_deep2_checks, auditchain::run_auditchain_deep3_checks, auditchain::run_auditchain_deep4_checks, auditchain::run_auditchain_deep5_checks, auditchain::run_auditchain_deep6_checks],
        "resquota" => [resquota::run_resquota_checks, resquota::run_resquota_deep_checks, resquota::run_resquota_deep2_checks, resquota::run_resquota_deep3_checks, resquota::run_resquota_deep4_checks, resquota::run_resquota_deep5_checks, resquota::run_resquota_deep6_checks],
        "batguard" => [batguard::run_batguard_checks, batguard::run_batguard_deep_checks, batguard::run_batguard_deep2_checks, batguard::run_batguard_deep3_checks, batguard::run_batguard_deep4_checks, batguard::run_batguard_deep5_checks, batguard::run_batguard_deep6_checks],
        "thermgov" => [thermgov::run_thermgov_checks, thermgov::run_thermgov_deep_checks, thermgov::run_thermgov_deep2_checks, thermgov::run_thermgov_deep3_checks, thermgov::run_thermgov_deep4_checks, thermgov::run_thermgov_deep5_checks, thermgov::run_thermgov_deep6_checks],
        "recenv" => [recenv::run_recenv_checks, recenv::run_recenv_deep_checks, recenv::run_recenv_deep2_checks, recenv::run_recenv_deep3_checks, recenv::run_recenv_deep4_checks, recenv::run_recenv_deep5_checks, recenv::run_recenv_deep6_checks],
        "lineage" => [lineage::run_lineage_checks, lineage::run_lineage_deep_checks, lineage::run_lineage_deep2_checks, lineage::run_lineage_deep3_checks, lineage::run_lineage_deep4_checks, lineage::run_lineage_deep5_checks, lineage::run_lineage_deep6_checks],
        "walkall" => [walkall::run_walkall_checks, walkall::run_walkall_deep_checks, walkall::run_walkall_deep2_checks, walkall::run_walkall_deep3_checks, walkall::run_walkall_deep4_checks, walkall::run_walkall_deep5_checks, walkall::run_walkall_deep6_checks],
        _ => unreachable!("MODULES 常量之外的模块名——常量与 match 必须同步"),
    }
}

/// 逐块清点（对账表的数据源：15 模块 × 7 表 = 105 块 CheckSet 逐块条数）。
pub fn check_ledger() -> Vec<(&'static str, usize, usize, usize, usize, usize, usize, usize)> {
    MODULES
        .iter()
        .map(|(tag, module)| {
            let t = seven_tables(module);
            (
                *tag,
                t[0]().len(),
                t[1]().len(),
                t[2]().len(),
                t[3]().len(),
                t[4]().len(),
                t[5]().len(),
                t[6]().len(),
            )
        })
        .collect()
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

    #[test]
    fn secstar2_check_ledger_reconciled() {
        // 检查项对账（机器钉数）：
        // ① 105 块齐（15 模块 × 7 表：…+深6检）；
        // ② 每块非空且 ≤ 容量（截断即红——被丢的检查不算数）；
        // ③ 总数对账（报告引用的总数由本断言钉死——数字改动必先改这里）。
        let ledger = check_ledger();
        assert_eq!(ledger.len(), 15);
        let mut total = 0usize;
        for (tag, base, deep, deep2, deep3, deep4, deep5, deep6) in ledger {
            assert!(base > 0 && base <= 64, "{} base ledger {}", tag, base);
            assert!(deep > 0 && deep <= 64, "{} deep ledger {}", tag, deep);
            assert!(deep2 > 0 && deep2 <= 64, "{} deep2 ledger {}", tag, deep2);
            assert!(deep3 > 0 && deep3 <= 64, "{} deep3 ledger {}", tag, deep3);
            assert!(deep4 > 0 && deep4 <= 64, "{} deep4 ledger {}", tag, deep4);
            assert!(deep5 > 0 && deep5 <= 64, "{} deep5 ledger {}", tag, deep5);
            assert!(deep6 > 0 && deep6 <= 64, "{} deep6 ledger {}", tag, deep6);
            total += base + deep + deep2 + deep3 + deep4 + deep5 + deep6;
        }
        // 总检查项 = 1610（机器钉数：v1-v6 合计 1492 + v7 118；数字变动
        // 必须同步本断言、check_ledger() 与完成报告对账表）。
        assert_eq!(total, 1610, "检查项总数变动必须同步本断言与完成报告对账表（实测 {}）", total);
    }

    #[test]
    fn secstar2_aggregate_rows_15_within_capacity() {
        // 聚合器行数=15（每模块一行，行内合六表）；聚合器自身零截断。
        let set = run_secstar2_checks();
        assert_eq!(set.len(), 15);
        assert!(!set.truncated());
    }
}
