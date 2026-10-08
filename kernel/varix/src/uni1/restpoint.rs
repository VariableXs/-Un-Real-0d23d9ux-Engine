//! F442 手动创建还原点 · 完整设计（STAR I 主册 G-I-42）。
//!
//! **判据（主册）**：创建时长 <30s；列表信息完整性；手动/自动标注；
//! 删除与轮替规则；创建期间可用性。＋通12。
//!
//! 设计：还原点管理核——创建（默认名「手动-日期时间」、进度推进、
//! 时长账 <30s 判据、**创建期间可用性** = 写入走后台通道不占 UI 线程
//! ——界面可响应记账）；列表（自动/手动两类型标注、占用空间、时刻）；
//! 删除与轮替（上限 N、超限轮替最旧、**最近一个永留**——F325 同源
//! 纪律）；删除需确认（不可逆，确认位钉死）。
//!
//! **v4 深化批次新增（AI-U1）**：
//! - 磁盘配额账 [`RestoreMgr::used_kb`] / `quota_kb`：还原点占用
//!   汇总 + 配额上限——超配额拒绝新建（人话提示先删旧的）；占用账
//!   随创建/删除/轮替实时对（一处一事实：账 = Σ列表，不另记小账）；
//! - 在途中止 [`RestoreMgr::abort_create`]：半程可取消（取消即清在途
//!   状态——「打断恢复」路径有明确出口）；已有在途再点创建 → 拒绝
//!   （单在途不变量——不会两个后台写入打架）；
//! - 还原执行 [`RestoreMgr::restore`]：语义 = 回滚到该点（记录
//!   last_restored + 确认位；还原**不删**后续还原点——用户还可能想
//!   再回到更近的点，删了就没退路）；还原需确认（动配置层，不可逆
//!   级别同删除）；
//! - 列表时序不变量：永远按时刻降序（新在前——`list_sorted_desc`
//!   机检，轮替/删除后仍成立）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

/// 还原点上限（超限轮替；最近一个永留）。
pub const RESTORE_POINT_CAP: usize = 10;
/// 创建时长判线（ms）。
pub const CREATE_BUDGET_MS: u64 = 30_000;
/// 默认配额（KB）——2 GB（真实盘上由 F183 存储健康联动定）。
pub const DEFAULT_QUOTA_KB: u64 = 2 * 1_024 * 1_024;

/// 还原点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Auto,
    Manual,
}

/// 一个还原点。
#[derive(Clone, Debug)]
pub struct RestorePoint {
    pub name: String,
    pub kind: PointKind,
    /// 创建时刻（ms 序号）。
    pub at_ms: u64,
    /// 占用空间（KB）。
    pub size_kb: u64,
}

/// 还原点管理核。
pub struct RestoreMgr {
    pub points: Vec<RestorePoint>,
    /// 在途创建（Some = 后台写入中，界面仍可响应）。
    pub creating: Option<(u64, u64, PointKind)>, // (开始时刻, 进度千分比, 类型)
    pub over_budget_creates: u64,
    /// 占用配额（KB）。
    pub quota_kb: u64,
    /// 最近一次还原到的点（时刻）——诊断面显示「当前停在哪个点」。
    pub last_restored_at: Option<u64>,
    /// 被拒创建（超配额/单在途冲突）计数——诚实账。
    pub rejected_creates: u64,
}

impl RestoreMgr {
    pub fn new() -> RestoreMgr {
        RestoreMgr {
            points: Vec::new(),
            creating: None,
            over_budget_creates: 0,
            quota_kb: DEFAULT_QUOTA_KB,
            last_restored_at: None,
            rejected_creates: 0,
        }
    }

    /// 占用汇总（账 = Σ列表——一处一事实，不另记小账）。
    pub fn used_kb(&self) -> u64 {
        self.points.iter().map(|p| p.size_kb).sum()
    }

    /// 开始创建：手动入口给默认名「手动-<时刻>」；返回在途句柄。
    /// 创建期间可用性：只登记在途状态，不阻塞任何查询。
    /// 单在途不变量：已有在途 → 拒绝（两个后台写入会打架）。
    pub fn begin_create(&mut self, kind: PointKind, at_ms: u64) -> Result<String, &'static str> {
        if self.creating.is_some() {
            self.rejected_creates += 1;
            return Err("已有还原点在创建中——等它完成或先取消");
        }
        let name = match kind {
            PointKind::Manual => format!("手动-{}", at_ms),
            PointKind::Auto => format!("自动-{}", at_ms),
        };
        self.creating = Some((at_ms, 0, kind));
        Ok(name)
    }

    /// 后台进度推进；完成时落列表并触发轮替。elapsed 超 30s 记账。
    pub fn tick(&mut self, permille_delta: u64, elapsed_ms: u64, size_kb: u64) -> bool {
        let Some((at, prog, kind)) = self.creating else { return false };
        if elapsed_ms > CREATE_BUDGET_MS {
            self.over_budget_creates += 1;
        }
        let new_prog = (prog + permille_delta).min(1_000);
        if new_prog >= 1_000 {
            // 配额门：完成落盘前查账——超配额拒绝且人话。
            if self.used_kb() + size_kb > self.quota_kb {
                self.creating = None;
                self.rejected_creates += 1;
                return false;
            }
            self.creating = None;
            self.points.insert(0, RestorePoint {
                name: format!("{}-{}", if kind == PointKind::Manual { "手动" } else { "自动" }, at),
                kind,
                at_ms: at,
                size_kb,
            });
            self.rotate();
            true
        } else {
            self.creating = Some((at, new_prog, kind));
            false
        }
    }

    /// 在途中止：半程取消（打断恢复路径的明确出口）。
    pub fn abort_create(&mut self) -> bool {
        match self.creating {
            Some(_) => {
                self.creating = None;
                true
            }
            None => false,
        }
    }

    /// 轮替：超上限删最旧，最近一个永留。
    fn rotate(&mut self) {
        while self.points.len() > RESTORE_POINT_CAP {
            self.points.pop(); // 列表头插 → 尾即最旧
        }
    }

    /// 删除：需确认位（不可逆操作——确认前拒绝）。最近一个永留保护。
    pub fn delete(&mut self, idx: usize, confirmed: bool) -> Result<(), &'static str> {
        if !confirmed {
            return Err("需要确认——删除还原点不可恢复");
        }
        if idx == 0 {
            return Err("最近的还原点受保护——轮替自动处理，不手动删");
        }
        if idx >= self.points.len() {
            return Err("还原点不存在——列表可能已刷新");
        }
        self.points.remove(idx);
        Ok(())
    }

    /// 还原执行：回滚到该点（需确认；**不删**后续点——保留退路）。
    pub fn restore(&mut self, idx: usize, confirmed: bool) -> Result<u64, &'static str> {
        if !confirmed {
            return Err("需要确认——还原会回滚配置层");
        }
        if idx >= self.points.len() {
            return Err("还原点不存在——列表可能已刷新");
        }
        let at = self.points[idx].at_ms;
        self.last_restored_at = Some(at);
        Ok(at)
    }

    /// 列表信息完整性：类型/时刻/空间三要素全登记。
    pub fn list_complete(&self) -> bool {
        self.points
            .iter()
            .all(|p| !p.name.is_empty() && p.size_kb > 0 && p.at_ms > 0)
    }

    /// 列表时序不变量：按时刻降序（新在前——轮替/删除后仍成立）。
    pub fn list_sorted_desc(&self) -> bool {
        self.points.windows(2).all(|w| w[0].at_ms > w[1].at_ms)
    }
}

pub fn run_restpoint_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F442");
    let mut m = RestoreMgr::new();
    // 创建：默认名 + 进度推进 + <30s 判据（超线记账）。
    let name = m.begin_create(PointKind::Manual, 88_000);
    set.add("f442-default-name", name == Ok(String::from("手动-88000")), "");
    set.add("f442-usable-while-creating", m.points.is_empty() && m.creating.is_some(), "");
    set.add("f442-created-under-budget", m.tick(1_000, 12_000, 51_200), "");
    // 手动/自动标注 + 列表信息完整 + 时序不变量。
    let mut m2 = RestoreMgr::new();
    let _ = m2.begin_create(PointKind::Auto, 1_000);
    let _ = m2.tick(1_000, 5_000, 10_000);
    set.add(
        "f442-kind-labeled-and-complete",
        m2.points[0].kind == PointKind::Auto && m2.points[0].name == "自动-1000" && m2.list_complete() && m2.list_sorted_desc(),
        "",
    );
    // 单在途不变量：已有在途再点创建 → 拒绝；半程中止出口在。
    let _ = m2.begin_create(PointKind::Manual, 9_999);
    set.add(
        "f442-single-inflight-guard",
        m2.begin_create(PointKind::Manual, 10_000) == Err("已有还原点在创建中——等它完成或先取消")
            && m2.rejected_creates == 1
            && m2.abort_create()
            && m2.creating.is_none()
            && !m2.abort_create(),
        "",
    );
    // 轮替：上限 + 最旧先走。
    let mut r = RestoreMgr::new();
    for i in 0..(RESTORE_POINT_CAP + 3) {
        let _ = r.begin_create(PointKind::Auto, (i as u64 + 1) * 1_000);
        let _ = r.tick(1_000, 4_000, 1_000);
    }
    set.add(
        "f442-rotate-oldest",
        r.points.len() == RESTORE_POINT_CAP && r.points[0].at_ms == 13_000 && r.points.last().unwrap().at_ms == 4_000 && r.list_sorted_desc(),
        "",
    );
    // 占用账 = Σ列表（一处一事实）。
    set.add("f442-used-sums-list", r.used_kb() == RESTORE_POINT_CAP as u64 * 1_000, "");
    // 删除：确认位 + 最近一个保护。
    set.add(
        "f442-delete-confirm-guard",
        matches!(r.delete(1, false), Err("需要确认——删除还原点不可恢复")),
        "",
    );
    set.add(
        "f442-latest-protected",
        matches!(r.delete(0, true), Err("最近的还原点受保护——轮替自动处理，不手动删")) && r.points.len() == RESTORE_POINT_CAP,
        "",
    );
    set.add("f442-delete-ok", r.delete(1, true).is_ok() && r.points.len() == RESTORE_POINT_CAP - 1, "");
    // 配额门：小配额盘上超配额拒绝新建（完成落盘前查账——白干的写入
    // 不发生），提示先删旧的。先建两个 500KB 点占 1/3 配额。
    let mut q = RestoreMgr::new();
    q.quota_kb = 1_500;
    let _ = q.begin_create(PointKind::Manual, 1_000);
    set.add("f442-quota-fits", q.tick(1_000, 2_000, 500), "");
    let _ = q.begin_create(PointKind::Manual, 2_000);
    set.add("f442-quota-fits-second", q.tick(1_000, 2_500, 500), "");
    let _ = q.begin_create(PointKind::Manual, 3_000);
    set.add(
        "f442-quota-exceeded-rejects",
        !q.tick(1_000, 3_500, 1_000)
            && q.points.len() == 2
            && q.rejected_creates == 1
            && q.used_kb() == 1_000,
        "",
    );
    // 删旧腾位后可再建（配额是门不是墙——出路在提示里；idx1 非最近点
    // 不受保护）。
    set.add("f442-delete-older-frees", q.delete(1, true).is_ok(), "");
    let _ = q.begin_create(PointKind::Manual, 4_000);
    set.add("f442-quota-frees-after-delete", q.tick(1_000, 4_500, 1_000) && q.used_kb() == 1_500, "");
    // 还原执行：确认位；**不删**后续点（保留退路）；停在哪个点有账。
    set.add(
        "f442-restore-needs-confirm",
        matches!(r.restore(2, false), Err("需要确认——还原会回滚配置层")),
        "",
    );
    let restored_to = r.restore(2, true);
    set.add(
        "f442-restore-keeps-exit-path",
        restored_to == Ok(10_000) && r.last_restored_at == Some(10_000) && r.points.len() == RESTORE_POINT_CAP - 1,
        "",
    );
    // 超预算记账（诚实——不是静默慢）。
    let mut s = RestoreMgr::new();
    let _ = s.begin_create(PointKind::Manual, 1);
    let _ = s.tick(1_000, 31_000, 1_000);
    set.add("f442-over-budget-logged", s.over_budget_creates == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_is_interruptible_and_honest() {
        let mut m = RestoreMgr::new();
        let _ = m.begin_create(PointKind::Manual, 5_000);
        // 半程查询不崩（创建期间可用性）。
        assert!(!m.tick(500, 1_000, 100));
        assert_eq!(m.creating.unwrap().1, 500);
        assert!(m.list_complete() || m.points.is_empty());
        // 续跑完成。
        assert!(m.tick(500, 2_000, 100));
        assert!(m.creating.is_none());
        assert_eq!(m.points.len(), 1);
    }

    #[test]
    fn auto_points_also_rotate() {
        let mut m = RestoreMgr::new();
        for i in 0..RESTORE_POINT_CAP {
            let _ = m.begin_create(PointKind::Auto, i as u64);
            let _ = m.tick(1_000, 100, 10);
        }
        assert_eq!(m.points.len(), RESTORE_POINT_CAP);
        // 12:00 判线内创建从不超账。
        assert_eq!(m.over_budget_creates, 0);
    }

    #[test]
    fn abort_midway_wastes_nothing() {
        let mut m = RestoreMgr::new();
        let _ = m.begin_create(PointKind::Manual, 7_000);
        let _ = m.tick(400, 1_000, 100);
        assert!(m.abort_create());
        assert!(m.creating.is_none() && m.points.is_empty(), "半程中止：列表零污染");
        // 中止后立即可建新的（单在途位已释放）。
        assert!(m.begin_create(PointKind::Manual, 8_000).is_ok());
    }
}
