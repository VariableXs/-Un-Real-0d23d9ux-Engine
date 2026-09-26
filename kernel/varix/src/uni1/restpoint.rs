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

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

/// 还原点上限（超限轮替；最近一个永留）。
pub const RESTORE_POINT_CAP: usize = 10;
/// 创建时长判线（ms）。
pub const CREATE_BUDGET_MS: u64 = 30_000;

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
}

impl RestoreMgr {
    pub fn new() -> RestoreMgr {
        RestoreMgr { points: Vec::new(), creating: None, over_budget_creates: 0 }
    }

    /// 开始创建：手动入口给默认名「手动-<时刻>」；返回在途句柄。
    /// 创建期间可用性：只登记在途状态，不阻塞任何查询。
    pub fn begin_create(&mut self, kind: PointKind, at_ms: u64) -> String {
        let name = match kind {
            PointKind::Manual => format!("手动-{}", at_ms),
            PointKind::Auto => format!("自动-{}", at_ms),
        };
        self.creating = Some((at_ms, 0, kind));
        name
    }

    /// 后台进度推进；完成时落列表并触发轮替。elapsed 超 30s 记账。
    pub fn tick(&mut self, permille_delta: u64, elapsed_ms: u64, size_kb: u64) -> bool {
        let Some((at, prog, kind)) = self.creating else { return false };
        if elapsed_ms > CREATE_BUDGET_MS {
            self.over_budget_creates += 1;
        }
        let new_prog = (prog + permille_delta).min(1_000);
        if new_prog >= 1_000 {
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

    /// 列表信息完整性：类型/时刻/空间三要素全登记。
    pub fn list_complete(&self) -> bool {
        self.points
            .iter()
            .all(|p| !p.name.is_empty() && p.size_kb > 0 && p.at_ms > 0)
    }
}

pub fn run_restpoint_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F442");
    let mut m = RestoreMgr::new();
    // 创建：默认名 + 进度推进 + <30s 判据（超线记账）。
    let name = m.begin_create(PointKind::Manual, 88_000);
    set.add("f442-default-name", name == "手动-88000", "");
    set.add("f442-usable-while-creating", m.points.is_empty() && m.creating.is_some(), "");
    set.add("f442-created-under-budget", m.tick(1_000, 12_000, 51_200), "");
    // 手动/自动标注 + 列表信息完整。
    let mut m2 = RestoreMgr::new();
    let _ = m2.begin_create(PointKind::Auto, 1_000);
    let _ = m2.tick(1_000, 5_000, 10_000);
    set.add(
        "f442-kind-labeled-and-complete",
        m2.points[0].kind == PointKind::Auto && m2.points[0].name == "自动-1000" && m2.list_complete(),
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
        r.points.len() == RESTORE_POINT_CAP && r.points[0].at_ms == 13_000 && r.points.last().unwrap().at_ms == 4_000,
        "",
    );
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
}
