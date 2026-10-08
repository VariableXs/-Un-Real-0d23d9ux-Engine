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

/// 本域自检聚合：逐模块合并各自全部自检表为一行——聚合器恒 15 行，
/// 单行绿 = 该模块全部表全绿且均未截断。
///
/// 表数按模块分档（上限口径差异的诚实呈现）：
/// - clockguard / logring：7 表（已超单项上限，v8 不再投入）；
/// - syspart / batguard / resquota / selfheal2：8 表（至 deep7）；
/// - 其余九模块：9 表（至 deep7b——v8 批次主战场）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——各表容量
/// 由 `check_ledger()` 逐块机检（全部 ≤64）；聚合器行数恒 15，永不超容。
///
/// 栈安全纪律：聚合为循环内逐模块求值（函数指针 + 迭代结束即析构）——
/// 一次性持有全部 CheckSet 会溢出测试线程栈（v6 收口实测教训）。
pub fn run_secstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(SECSTAR2_DOMAIN);
    for (tag, module) in MODULES {
        let ok = tables_for(module).iter().all(|f| {
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

/// 逐模块表函数指针清单（表序=基检/深检/深2检/…按各模块实有表）。
fn tables_for(module: &str) -> Vec<fn() -> CheckSet> {
    fn chain<const N: usize>(fns: [fn() -> CheckSet; N]) -> Vec<fn() -> CheckSet> {
        fns.to_vec()
    }
    match module {
        // 8 表（至 deep7）。
        "syspart" => chain([
            syspart::run_syspart_checks, syspart::run_syspart_deep_checks, syspart::run_syspart_deep2_checks, syspart::run_syspart_deep3_checks, syspart::run_syspart_deep4_checks, syspart::run_syspart_deep5_checks, syspart::run_syspart_deep6_checks, syspart::run_syspart_deep7_checks,
        ]),
        "batguard" => chain([
            batguard::run_batguard_checks, batguard::run_batguard_deep_checks, batguard::run_batguard_deep2_checks, batguard::run_batguard_deep3_checks, batguard::run_batguard_deep4_checks, batguard::run_batguard_deep5_checks, batguard::run_batguard_deep6_checks, batguard::run_batguard_deep7_checks,
        ]),
        "resquota" => chain([
            resquota::run_resquota_checks, resquota::run_resquota_deep_checks, resquota::run_resquota_deep2_checks, resquota::run_resquota_deep3_checks, resquota::run_resquota_deep4_checks, resquota::run_resquota_deep5_checks, resquota::run_resquota_deep6_checks, resquota::run_resquota_deep7_checks,
        ]),
        "selfheal2" => chain([
            selfheal2::run_selfheal2_checks, selfheal2::run_selfheal2_deep_checks, selfheal2::run_selfheal2_deep2_checks, selfheal2::run_selfheal2_deep3_checks, selfheal2::run_selfheal2_deep4_checks, selfheal2::run_selfheal2_deep5_checks, selfheal2::run_selfheal2_deep6_checks, selfheal2::run_selfheal2_deep7_checks,
        ]),
        // 9 表（至 deep7b）——v8 主战场。
        "slotview" => chain([
            slotview::run_slotview_checks, slotview::run_slotview_deep_checks, slotview::run_slotview_deep2_checks, slotview::run_slotview_deep3_checks, slotview::run_slotview_deep4_checks, slotview::run_slotview_deep5_checks, slotview::run_slotview_deep6_checks, slotview::run_slotview_deep7_checks, slotview::run_slotview_deep7b_checks,
            slotview::run_slotview_deep8_checks,
        ]),
        "bootaudit" => chain([
            bootaudit::run_bootaudit_checks, bootaudit::run_bootaudit_deep_checks, bootaudit::run_bootaudit_deep2_checks, bootaudit::run_bootaudit_deep3_checks, bootaudit::run_bootaudit_deep4_checks, bootaudit::run_bootaudit_deep5_checks, bootaudit::run_bootaudit_deep6_checks, bootaudit::run_bootaudit_deep7_checks, bootaudit::run_bootaudit_deep7b_checks,
            bootaudit::run_bootaudit_deep8_checks,
        ]),
        "paramwl" => chain([
            paramwl::run_paramwl_checks, paramwl::run_paramwl_deep_checks, paramwl::run_paramwl_deep2_checks, paramwl::run_paramwl_deep3_checks, paramwl::run_paramwl_deep4_checks, paramwl::run_paramwl_deep5_checks, paramwl::run_paramwl_deep6_checks, paramwl::run_paramwl_deep7_checks, paramwl::run_paramwl_deep7b_checks,
            paramwl::run_paramwl_deep8_checks,
        ]),
        "safemode" => chain([
            safemode::run_safemode_checks, safemode::run_safemode_deep_checks, safemode::run_safemode_deep2_checks, safemode::run_safemode_deep3_checks, safemode::run_safemode_deep4_checks, safemode::run_safemode_deep5_checks, safemode::run_safemode_deep6_checks, safemode::run_safemode_deep7_checks, safemode::run_safemode_deep7b_checks,
            safemode::run_safemode_deep8_checks,
        ]),
        "auditchain" => chain([
            auditchain::run_auditchain_checks, auditchain::run_auditchain_deep_checks, auditchain::run_auditchain_deep2_checks, auditchain::run_auditchain_deep3_checks, auditchain::run_auditchain_deep4_checks, auditchain::run_auditchain_deep5_checks, auditchain::run_auditchain_deep6_checks, auditchain::run_auditchain_deep7_checks, auditchain::run_auditchain_deep7b_checks,
            auditchain::run_auditchain_deep8_checks,
        ]),
        "thermgov" => chain([
            thermgov::run_thermgov_checks, thermgov::run_thermgov_deep_checks, thermgov::run_thermgov_deep2_checks, thermgov::run_thermgov_deep3_checks, thermgov::run_thermgov_deep4_checks, thermgov::run_thermgov_deep5_checks, thermgov::run_thermgov_deep6_checks, thermgov::run_thermgov_deep7_checks, thermgov::run_thermgov_deep7b_checks,
            thermgov::run_thermgov_deep8_checks,
        ]),
        "recenv" => chain([
            recenv::run_recenv_checks, recenv::run_recenv_deep_checks, recenv::run_recenv_deep2_checks, recenv::run_recenv_deep3_checks, recenv::run_recenv_deep4_checks, recenv::run_recenv_deep5_checks, recenv::run_recenv_deep6_checks, recenv::run_recenv_deep7_checks, recenv::run_recenv_deep7b_checks,
            recenv::run_recenv_deep8_checks,
        ]),
        "lineage" => chain([
            lineage::run_lineage_checks, lineage::run_lineage_deep_checks, lineage::run_lineage_deep2_checks, lineage::run_lineage_deep3_checks, lineage::run_lineage_deep4_checks, lineage::run_lineage_deep5_checks, lineage::run_lineage_deep6_checks, lineage::run_lineage_deep7_checks, lineage::run_lineage_deep7b_checks,
        ]),
        "walkall" => chain([
            walkall::run_walkall_checks, walkall::run_walkall_deep_checks, walkall::run_walkall_deep2_checks, walkall::run_walkall_deep3_checks, walkall::run_walkall_deep4_checks, walkall::run_walkall_deep5_checks, walkall::run_walkall_deep6_checks, walkall::run_walkall_deep7_checks, walkall::run_walkall_deep7b_checks,
            walkall::run_walkall_deep8_checks,
        ]),
        // 7 表（已超单项上限——v8 不再投入）。
        "clockguard" => chain([
            clockguard::run_clockguard_checks, clockguard::run_clockguard_deep_checks, clockguard::run_clockguard_deep2_checks, clockguard::run_clockguard_deep3_checks, clockguard::run_clockguard_deep4_checks, clockguard::run_clockguard_deep5_checks, clockguard::run_clockguard_deep6_checks,
        ]),
        "logring" => chain([
            logring::run_logring_checks, logring::run_logring_deep_checks, logring::run_logring_deep2_checks, logring::run_logring_deep3_checks, logring::run_logring_deep4_checks, logring::run_logring_deep5_checks, logring::run_logring_deep6_checks,
        ]),
        _ => unreachable!("MODULES 常量之外的模块名——常量与 match 必须同步"),
    }
}

/// 逐块清点（对账表的数据源：15 模块各表逐块条数；块数 = Σ 各模块表数）。
pub fn check_ledger() -> Vec<(&'static str, Vec<usize>)> {
    MODULES
        .iter()
        .map(|(tag, module)| {
            let counts: Vec<usize> = tables_for(module).iter().map(|f| f().len()).collect();
            (*tag, counts)
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
        // ① 15 行齐；
        // ② 每块非空且 ≤ 容量 64（截断即红——被丢的检查不算数）；
        // ③ 表数分档：clockguard/logring 7 表、四小模块 8 表、九大模块 9 表；
        // ④ 总数对账（报告引用的总数由本断言钉死——数字改动必先改这里）。
        let ledger = check_ledger();
        assert_eq!(ledger.len(), 15);
        let mut total = 0usize;
        for (tag, counts) in ledger {
            for (i, c) in counts.iter().enumerate() {
                assert!(*c > 0 && *c <= 64, "{} table#{} ledger {}", tag, i, c);
            }
            let expect_tables = match tag {
                "F187" | "F188" => 7,
                "F186" | "F195" | "F196" | "F189" => 8,
                "F199" => 9,
                _ => 10, // 九大模块 v8 起带 deep8 表（F199 无 deep8——收口小波走摘要行）
            };
            assert_eq!(counts.len(), expect_tables, "{} table count", tag);
            total += counts.iter().sum::<usize>();
        }
        // 总检查项（机器钉数：v8 批次含 deep7/deep7b/deep8 全部并入；数字变动
        // 必须同步本断言、check_ledger() 与完成报告对账表）。
        assert_eq!(total, 2251, "检查项总数变动必须同步本断言与完成报告对账表（实测 {}）", total);
    }

    #[test]
    fn secstar2_aggregate_rows_15_within_capacity() {
        // 聚合器行数=15（每模块一行）；聚合器自身零截断。
        let set = run_secstar2_checks();
        assert_eq!(set.len(), 15);
        assert!(!set.truncated());
    }
}
