//! UNREAL-X-15000 · AI-28 工具内核与收官 K 线（族0271~0280 · X06751~X07000）。
//! 子模块注册表：CLI 工具集 / 脚本宿主 / 定时任务 / 通知联动 / 无障碍 /
//! 本地化 / 体检 / 扩展生态 / 彩蛋 / 收官。零堆、整数运算。

pub mod a11yk;
pub mod cli;
pub mod cron;
pub mod egg;
pub mod exteco;
pub mod finale;
pub mod healthck;
pub mod loc;
pub mod notifylink;
pub mod scripthost;

use crate::checks::CheckSet;

/// AI-28 K 线全量聚合：10 族 × 25 = 250 检。
///
/// 返回族数组而非单集——`MAX_CHECKS = 112` 装不下 250 检，合并会静默丢弃；
/// 注册表侧逐族登记（`quality.rs` 的 `for cs in ...` 惯例），全量保真。
pub fn run_task_all_checks() -> [CheckSet; 10] {
    [
        cli::run_cli_checks(),
        scripthost::run_scripthost_checks(),
        cron::run_cron_checks(),
        notifylink::run_notifylink_checks(),
        a11yk::run_a11yk_checks(),
        loc::run_loc_checks(),
        healthck::run_healthck_checks(),
        exteco::run_exteco_checks(),
        egg::run_egg_checks(),
        finale::run_finale_checks(),
    ]
}
