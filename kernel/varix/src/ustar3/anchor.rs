//! F550 I 域批次六验收锚点（ustar3 · anchor）——本批 25 项（F526-F550）
//! 验收锚点汇总进全域总检（F200/F375/F400 链）。
//!
//! 主册判据（验收标准第一句）：
//! **25 检查点入脚本并全绿基线；与 F400 脚本合并无冲突；锚点可执行性
//! 抽查（随机 5 条实机跑）；账册检对账。**
//!
//! 【无感标准】每批收官即入总检（不是写完就算）——账册检三处同源的纪律
//! 从 H 域（F400）延续到 I 域；验收锚点具体到可执行（每条能在实机上跑出
//! 数据）。
//!
//! 【锚点实现】判据点名的检查点（状态栏三段同源/树列表双向同步/空间预检
//! 拦截率/校验失败报告率/长路径 600 字符全操作/Win+数字映射一致性/Peek
//! 透明度与恢复精度/布局锁定拒绝反馈/诊断报告三要素/网络重置清单完整性/
//! ClickLock 阈值/分设备音量跟随/双滑杆独立/电量三处同源/接入通知三态/
//! 平衡试听实时/置顶不抢焦点/农历五年抽检等）逐项对应本批 25 项：每个
//! 锚点 = 对应域 run_f5XX_checks() 全绿（域内逐判据红绿在其子行展开，
//! 一处一事实不二次复述判据数字）；第 25 锚 = 账册检对账（25 域无截断、
//! 检查数齐备、红绿口径一致）。
//!
//! 【防递归纪律】本域对账直接逐域调用兄弟 run_f5XX_checks（F526-F549
//! 二十四域），不经 `ustar3::run_ustar3_all_checks` 聚合入口——本域
//! 自身在其聚合表内，走聚合会自引用递归。
//!
//! 与 F400 脚本合并无冲突：本域以独立 CheckSet 域入口注册进 robust.rs
//! 总检表（同表迭代、同口径 tally），不改动 F400/H 域既有检查点。

use crate::checks::CheckSet;
use crate::ustar3::{clockcal, copyops, explorerx, sysdev, winkeys};

/// 单域锚点判定：对应域自检全绿且无截断（截断 = 结果溢出被丢 = 账本不实）。
fn domain_green(run: fn() -> CheckSet) -> bool {
    let cs = run();
    cs.all_passed() && !cs.truncated() && cs.len() > 0
}

/// 域自检。
pub fn run_f550_checks() -> CheckSet {
    let mut cs = CheckSet::new("F550-i-batch6-anchor");
    // 25 检查点（判据点名 18 项 + 本批补齐 6 项 + 账册检 1 项）。
    // F526 状态栏三段同源。
    cs.add("cp01_statusbar_three_source", domain_green(explorerx::run_f526_checks), "");
    // F527 导航树折叠展开（双击/箭头两路+两命令+持久化+万节点）。
    cs.add("cp02_navtree_fold_persist", domain_green(explorerx::run_f527_checks), "");
    // 树列表双向同步。
    cs.add("cp03_treelist_bidir_sync", domain_green(explorerx::run_f528_checks), "");
    // 空间预检拦截率（进度框前唯一调用点+10% 缓冲）。
    cs.add("cp04_space_precheck_block", domain_green(copyops::run_f529_checks), "");
    // 校验失败报告率（报告完整性+补发时序）。
    cs.add("cp05_verify_fail_report", domain_green(copyops::run_f530_checks), "");
    // 队列同盘串行/异盘并行判定。
    cs.add("cp06_copy_queue_serial_same_disk", domain_green(copyops::run_f531_checks), "");
    // 打开失败四类归因（注入用例准确率）。
    cs.add("cp07_openfail_four_way_diag", domain_green(copyops::run_f532_checks), "");
    // 只读介质前置提醒（一次不重复）。
    cs.add("cp08_readonly_pre_notice", domain_green(copyops::run_f533_checks), "");
    // 长路径 600 字符全操作。
    cs.add("cp09_longpath_600_full_ops", domain_green(copyops::run_f534_checks), "");
    // Win+数字映射一致性（十键与任务栏顺序一致）。
    cs.add("cp10_win_num_map_consistency", domain_green(winkeys::run_f535_checks), "");
    // Win+T 五链路（进场/遍历/激活/菜单/退出）。
    cs.add("cp11_win_t_traverse", domain_green(winkeys::run_f536_checks), "");
    // Peek 透明度与恢复精度（15%±3%、120ms±20ms、<1px）。
    cs.add("cp12_peek_alpha_and_restore", domain_green(winkeys::run_f537_checks), "");
    // Alt+Esc Z 序正确性。
    cs.add("cp13_alt_esc_zorder", domain_green(winkeys::run_f538_checks), "");
    // 布局锁定拒绝反馈。
    cs.add("cp14_layout_lock_reject", domain_green(winkeys::run_f539_checks), "");
    // 诊断报告三要素（结论/地址段/建议）。
    cs.add("cp15_memdiag_report_elements", domain_green(sysdev::run_f540_checks), "");
    // 网络重置清单完整性。
    cs.add("cp16_netreset_checklist", domain_green(sysdev::run_f541_checks), "");
    // ClickLock 阈值（1.1s 抓起）。
    cs.add("cp17_clicklock_threshold", domain_green(sysdev::run_f542_checks), "");
    // 分设备音量跟随。
    cs.add("cp18_per_device_volume_follow", domain_green(sysdev::run_f543_checks), "");
    // 双滑杆独立（媒体/通知）。
    cs.add("cp19_dual_volume_independent", domain_green(sysdev::run_f544_checks), "");
    // 电量三处同源。
    cs.add("cp20_battery_three_source", domain_green(sysdev::run_f545_checks), "");
    // 接入通知三态。
    cs.add("cp21_arrive_banner_states", domain_green(sysdev::run_f546_checks), "");
    // 平衡试听实时。
    cs.add("cp22_balance_live_monitor", domain_green(sysdev::run_f547_checks), "");
    // 置顶不抢焦点。
    cs.add("cp23_pin_top_no_focus_steal", domain_green(winkeys::run_f548_checks), "");
    // 农历五年抽检（2026-2030 春节锚点+中秋+闰月）。
    cs.add("cp24_lunar_five_year_spot", domain_green(clockcal::run_f549_checks), "");
    // 第 25 锚：账册检对账——24 域检查数齐备、无截断、口径一致。
    let domains: [fn() -> CheckSet; 24] = [
        explorerx::run_f526_checks,
        explorerx::run_f527_checks,
        explorerx::run_f528_checks,
        copyops::run_f529_checks,
        copyops::run_f530_checks,
        copyops::run_f531_checks,
        copyops::run_f532_checks,
        copyops::run_f533_checks,
        copyops::run_f534_checks,
        winkeys::run_f535_checks,
        winkeys::run_f536_checks,
        winkeys::run_f537_checks,
        winkeys::run_f538_checks,
        winkeys::run_f539_checks,
        sysdev::run_f540_checks,
        sysdev::run_f541_checks,
        sysdev::run_f542_checks,
        sysdev::run_f543_checks,
        sysdev::run_f544_checks,
        sysdev::run_f545_checks,
        sysdev::run_f546_checks,
        sysdev::run_f547_checks,
        winkeys::run_f548_checks,
        clockcal::run_f549_checks,
    ];
    let mut total = 0usize;
    let mut ledger_ok = true;
    for run in domains {
        let c = run();
        total += c.len();
        if c.truncated() || c.len() == 0 || !c.all_passed() {
            ledger_ok = false;
        }
    }
    cs.add("cp25_ledger_audit_all_green", ledger_ok && total >= 24 * 8, "");
    cs
}

#[cfg(test)]
mod f550_tests {
    use super::*;

    #[test]
    fn anchor_has_25_checkpoints_all_green() {
        let cs = run_f550_checks();
        assert_eq!(cs.len(), 25, "25 检查点一个不少");
        assert!(cs.all_passed(), "全绿基线");
        assert!(!cs.truncated(), "账册无截断");
    }

    #[test]
    fn ledger_audit_covers_24_domains() {
        // 账册检：24 域（F526-F549）全部 non-empty、全绿、无截断。
        let domains: [fn() -> CheckSet; 24] = [
            explorerx::run_f526_checks,
            explorerx::run_f527_checks,
            explorerx::run_f528_checks,
            copyops::run_f529_checks,
            copyops::run_f530_checks,
            copyops::run_f531_checks,
            copyops::run_f532_checks,
            copyops::run_f533_checks,
            copyops::run_f534_checks,
            winkeys::run_f535_checks,
            winkeys::run_f536_checks,
            winkeys::run_f537_checks,
            winkeys::run_f538_checks,
            winkeys::run_f539_checks,
            sysdev::run_f540_checks,
            sysdev::run_f541_checks,
            sysdev::run_f542_checks,
            sysdev::run_f543_checks,
            sysdev::run_f544_checks,
            sysdev::run_f545_checks,
            sysdev::run_f546_checks,
            sysdev::run_f547_checks,
            winkeys::run_f548_checks,
            clockcal::run_f549_checks,
        ];
        let mut total = 0;
        for run in domains {
            let c = run();
            assert!(c.all_passed(), "域 {} 必须全绿", c.domain);
            assert!(!c.truncated(), "域 {} 不得截断", c.domain);
            total += c.len();
        }
        assert!(total >= 24 * 8, "24 域合计检查数 {} 应 ≥192（每域 ≥8）", total);
    }
}
