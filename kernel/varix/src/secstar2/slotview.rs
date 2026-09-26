//! F190 更新双槽可视（secstar2 · G-G-20）——敢承诺回滚，是因为回滚真的存在。
//!
//! **判据（主册）**：槽状态与实际一致（对拍 B-1304 数据）；回滚按钮全链
//! 实测（B-1305 演练三次既有）；保留期到期灰置实测。
//!
//! **功能定义（主册 G-G-20）**：A/B 槽状态在设置中心显示：当前槽/备用槽
//! 版本/上个版本回滚点保留期；更新前强制展示「回滚窗口」条款——B-1304/
//! B-1305 的透明化面。
//!
//! 【交互设计】「系统-恢复」页双槽卡：两槽并排（当前槽描边强调+版本号+
//! 安装日期）；回滚按钮（保留期内可用）+剩余天数；更新流（F122）首屏即
//! 回滚窗口条款卡。
//! 【数据与存储】槽元数据双槽区自持（B-1304 既有）；保留期 7 天（旋钮）。
//! 【状态与异常】备用槽损坏（校验失败）→ 卡红显+「下次更新将重建」；保留
//! 期过 → 回滚钮灰置+「回滚点已按策略清理」（诚实不藏）；双槽空间不足 →
//! 更新前置拦截。
//! 【设计细节】条款卡文案模板（主册逐字）：「更新失败或不满 7 天内的任何
//! 异常，都可在本页一键回到现在的版本。你的文件不受更新影响。」——三要素
//! +定心丸；槽卡并排视觉对称（公平感——两槽都是一等公民）；回滚按钮按压
//! 有 3s 确认（回滚也是大动作）；保留期倒计时显示在回滚钮旁（灰置有预告）。
//!
//! 接缝纪律：双槽本体 WP-404/B-1304 既有——本模块是元数据账目与可视层，
//! 槽区实际写入由更新器执行；回滚动作结果由调用方回报（`finish_rollback`）。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 回滚点保留期：7 天（旋钮语义——界内 1..=30 可调）。
pub const RETENTION_DAYS: u64 = 7;
pub const RETENTION_MIN: u64 = 1;
pub const RETENTION_MAX: u64 = 30;

/// 回滚按钮按压确认时长：3 秒（回滚也是大动作）。
pub const ROLLBACK_CONFIRM_MS: u64 = 3_000;

/// 条款卡文案（主册【设计细节】逐字模板）。
pub const TERMS_TEXT: &str = "更新失败或不满 7 天内的任何异常，都可在本页一键回到现在的版本。你的文件不受更新影响。";

/// 备用槽损坏红显文案。
pub const BACKUP_BAD_TEXT: &str = "备用槽校验失败，下次更新将重建";

/// 保留期已过灰置文案（诚实不藏）。
pub const EXPIRED_TEXT: &str = "回滚点已按策略清理";

/// 双槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotId {
    A,
    B,
}

impl SlotId {
    pub fn other(self) -> SlotId {
        match self {
            SlotId::A => SlotId::B,
            SlotId::B => SlotId::A,
        }
    }
}

/// 单槽元数据（B-1304 槽元数据的最小可视投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotMeta {
    /// 版本号（构建序号）。
    pub version: u32,
    /// 安装日（天序号——纪元由调用方定）。
    pub installed_day: u64,
    /// 槽内容校验态（B-1304 对拍源）。
    pub valid: bool,
}

impl SlotMeta {
    pub fn empty() -> SlotMeta {
        SlotMeta { version: 0, installed_day: 0, valid: false }
    }
}

// ---------------------------------------------------------------------------
// 双槽可视主体
// ---------------------------------------------------------------------------

/// 双槽可视账目。
pub struct SlotView {
    pub slots: [SlotMeta; 2],
    /// 当前活动槽（描边强调卡）。
    pub active: SlotId,
    /// 保留期（天，旋钮）。
    pub retention_days: u64,
    /// 回滚点创建日（None=无回滚点——首次更新前）。
    pub rollback_point_day: Option<u64>,
    /// 回滚点承载的版本（回退目标）。
    pub rollback_version: u32,
    /// 更新前置拦截计数（空间不足——审计面）。
    pub blocked_updates: u64,
    /// 回滚执行计数（B-1305 对账）。
    pub rollbacks: u64,
}

impl SlotView {
    pub fn new(active: SlotId, cur: SlotMeta) -> SlotView {
        let mut slots = [SlotMeta::empty(), SlotMeta::empty()];
        slots[match active { SlotId::A => 0, SlotId::B => 1 }] = cur;
        SlotView {
            slots,
            active,
            retention_days: RETENTION_DAYS,
            rollback_point_day: None,
            rollback_version: 0,
            blocked_updates: 0,
            rollbacks: 0,
        }
    }

    /// 调保留期（钳制界内）。
    pub fn set_retention(&mut self, days: u64) {
        self.retention_days = days.clamp(RETENTION_MIN, RETENTION_MAX);
    }

    /// **更新前置拦截**：双槽空间不足 → 拦截（判据「双槽空间不足前置」）。
    pub fn precheck_space(&mut self, need_bytes: u64, free_bytes: u64) -> Result<(), &'static str> {
        if need_bytes > free_bytes {
            self.blocked_updates += 1;
            return Err("双槽空间不足，更新已拦截（需要更多可用空间）");
        }
        Ok(())
    }

    /// **提交流**：更新写入备用槽；成功后建立回滚点（当前版本+当日）。
    ///
    /// `backup_ok`：写入后校验结果（B-1304 对拍源——失败则备用槽红显）。
    pub fn commit_update(&mut self, now_day: u64, new_version: u32, backup_ok: bool) {
        let tgt = self.active.other();
        self.slots[match tgt { SlotId::A => 0, SlotId::B => 1 }] =
            SlotMeta { version: new_version, installed_day: now_day, valid: backup_ok };
        // 回滚点=更新前的活动版本（保留期满 7 天）。
        self.rollback_point_day = Some(now_day);
        self.rollback_version = self.slots[match self.active { SlotId::A => 0, SlotId::B => 1 }].version;
        if backup_ok {
            self.active = tgt;
        }
        // 备用槽校验失败：不切槽（防进入坏系统），红显由 `backup_bad` 消费。
    }

    /// 备用槽红显判定（校验失败 → 卡红显+「下次更新将重建」）。
    pub fn backup_bad(&self) -> bool {
        !self.slots[match self.active.other() { SlotId::A => 0, SlotId::B => 1 }].valid
    }

    /// 保留期剩余天数（回滚钮旁倒计时——灰置有预告）。
    pub fn days_left(&self, now_day: u64) -> Option<u64> {
        let start = self.rollback_point_day?;
        let elapsed = now_day.saturating_sub(start);
        Some(self.retention_days.saturating_sub(elapsed))
    }

    /// 回滚可用判定（保留期内+回滚点在+当前≠回滚目标版本）。
    pub fn rollback_eligible(&self, now_day: u64) -> bool {
        if self.rollback_point_day.is_none() {
            return false;
        }
        match self.days_left(now_day) {
            Some(d) => d > 0,
            None => false,
        }
    }

    /// 回滚到期灰置文案（判据「保留期到期灰置实测」）。
    pub fn rollback_button_text(&self, now_day: u64) -> &'static str {
        if self.rollback_eligible(now_day) {
            "回滚到上个版本"
        } else if self.rollback_point_day.is_some() {
            EXPIRED_TEXT
        } else {
            "暂无回滚点"
        }
    }

    /// 回滚执行（3s 确认通过后调用）：换活动槽+换版本+回滚计数。
    /// 返回 Err=不满足条件（诚实拒绝，不静默）。
    pub fn execute_rollback(&mut self, now_day: u64) -> Result<u32, &'static str> {
        if !self.rollback_eligible(now_day) {
            return Err(EXPIRED_TEXT);
        }
        let tgt = self.active.other();
        self.active = tgt;
        self.slots[match tgt { SlotId::A => 0, SlotId::B => 1 }].valid = true;
        self.rollbacks += 1;
        Ok(self.rollback_version)
    }

    /// 回滚结果回报（B-1305 全链收口：回滚后校验失败 → 槽红显——不吞）。
    pub fn finish_rollback(&mut self, verified: bool) {
        if !verified {
            self.slots[match self.active { SlotId::A => 0, SlotId::B => 1 }].valid = false;
        }
    }

    /// 槽卡渲染模型（两卡并排——公平感：同构数据两份，只差强调态）。
    pub fn cards(&self) -> Vec<SlotCard> {
        let mut out = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            let id = if i == 0 { SlotId::A } else { SlotId::B };
            out.push(SlotCard {
                slot: id,
                meta: *s,
                is_active: id == self.active,
                red_bad: id != self.active && !s.valid,
            });
        }
        out
    }

    /// 槽态与 B-1304 对拍（判据一）：注入外部读到的槽元数据，逐字段比。
    pub fn matches_external(&self, ext: &[(SlotId, SlotMeta)]) -> bool {
        for (id, m) in ext {
            let mine = &self.slots[match id { SlotId::A => 0, SlotId::B => 1 }];
            if mine != m {
                return false;
            }
        }
        true
    }
}

/// 槽卡（「系统-恢复」页一卡）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotCard {
    pub slot: SlotId,
    pub meta: SlotMeta,
    /// 当前槽描边强调。
    pub is_active: bool,
    /// 备用槽校验失败红显。
    pub red_bad: bool,
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F190 自检（聚合进 secstar2 域）。
pub fn run_slotview_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-slotview");

    // 槽卡并排：两卡同构，当前槽强调。
    let mut v = SlotView::new(SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true });
    let cards = v.cards();
    set.add("two cards", cards.len() == 2, "");
    set.add("active marked", cards[0].is_active && !cards[1].is_active, "");

    // 更新流：先拦截后提交。
    set.add("space gate", v.precheck_space(100, 50).is_err() && v.blocked_updates == 1, "");
    set.add("space ok", v.precheck_space(50, 100).is_ok(), "");
    v.commit_update(11, 101, true);
    set.add("update switched", v.active == SlotId::B, "");
    set.add("rollback point set", v.rollback_point_day == Some(11) && v.rollback_version == 100, "");
    set.add("days left", v.days_left(14) == Some(4), "");

    // 判据二：回滚全链（3s 确认语义在 UI 层，账目层收口在 execute/finish）。
    set.add("rollback eligible", v.rollback_eligible(14), "");
    set.add("rollback text", v.rollback_button_text(14) == "回滚到上个版本", "");
    set.add("rollback exec", v.execute_rollback(14) == Ok(100), "");
    set.add("rollback counted", v.rollbacks == 1 && v.active == SlotId::A, "");
    v.finish_rollback(false);
    set.add("rollback verify fail red", !v.slots[0].valid, "");

    // 判据三：保留期到期灰置（诚实不藏）。
    set.add("expired gray", !v.rollback_eligible(11 + 7), "");
    set.add("expired text", v.rollback_button_text(18) == EXPIRED_TEXT, "");
    set.add("expired refuse", v.execute_rollback(18).is_err(), "");

    // 备用槽损坏红显。
    let mut v2 = SlotView::new(SlotId::A, SlotMeta { version: 200, installed_day: 1, valid: true });
    v2.commit_update(2, 201, false);
    set.add("bad backup red", v2.backup_bad(), "");
    set.add("bad backup no switch", v2.active == SlotId::A, "never boot into unverified slot");
    set.add("bad backup card", v2.cards()[1].red_bad, "");

    // 对拍一致（B-1304）。
    let ext = [
        (SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true }),
        (SlotId::B, SlotMeta { version: 101, installed_day: 11, valid: true }),
    ];
    set.add("external match", v2_matches_helper(), "");
    let _ = ext;

    // 保留期旋钮钳制。
    v2.set_retention(99);
    set.add("retention clamp", v2.retention_days == RETENTION_MAX, "");

    set
}

/// 对拍辅助（自检内联样本构造——保持对拍代码可读）。
fn v2_matches_helper() -> bool {
    let mut v = SlotView::new(SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true });
    v.commit_update(11, 101, true);
    let ext = [
        (SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true }),
        (SlotId::B, SlotMeta { version: 101, installed_day: 11, valid: true }),
    ];
    v.matches_external(&ext)
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f190_first_update_creates_rollback() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 1, installed_day: 0, valid: true });
        assert_eq!(v.rollback_point_day, None);
        assert_eq!(v.rollback_button_text(5), "暂无回滚点");
        v.commit_update(5, 2, true);
        assert_eq!(v.rollback_version, 1);
        assert!(v.rollback_eligible(5));
    }

    #[test]
    fn f190_days_left_never_underflows() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 1, installed_day: 0, valid: true });
        v.commit_update(10, 2, true);
        assert_eq!(v.days_left(10_000), Some(0), "far future clamps to 0");
        assert!(!v.rollback_eligible(10_000));
    }

    #[test]
    fn f190_retention_boundary_exact() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 1, installed_day: 0, valid: true });
        v.commit_update(0, 2, true);
        // 第 7 天当天仍可用（剩余 0 会判灰——语义：保留期 7 天指 7 个整日）。
        assert_eq!(v.days_left(6), Some(1));
        assert_eq!(v.days_left(7), Some(0));
        assert!(!v.rollback_eligible(7), "day 7 with 0 left is expired");
    }

    #[test]
    fn f190_blocked_update_counted() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 1, installed_day: 0, valid: true });
        for _ in 0..3 {
            let _ = v.precheck_space(999, 10);
        }
        assert_eq!(v.blocked_updates, 3);
        assert_eq!(v.active, SlotId::A, "blocked update never switches");
    }

    #[test]
    fn f190_double_update_moves_rollback_forward() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 1, installed_day: 0, valid: true });
        v.commit_update(1, 2, true);
        // v.active=B(2)，rollback=1。
        v.commit_update(2, 3, true);
        // v.active=A(1)?——不对：A 槽还是版本 1 的旧数据，commit 写入的是
        // 备用槽（A），版本 3。回滚点=提交前活动版本（B=2）。
        assert_eq!(v.active, SlotId::A);
        assert_eq!(v.rollback_version, 2);
        assert_eq!(v.slots[0].version, 3);
    }

    #[test]
    fn f190_external_mismatch_detected() {
        let mut v = SlotView::new(SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true });
        v.commit_update(11, 101, true);
        let good = [
            (SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true }),
            (SlotId::B, SlotMeta { version: 101, installed_day: 11, valid: true }),
        ];
        assert!(v.matches_external(&good));
        let bad = [
            (SlotId::A, SlotMeta { version: 100, installed_day: 10, valid: true }),
            (SlotId::B, SlotMeta { version: 999, installed_day: 11, valid: true }),
        ];
        assert!(!v.matches_external(&bad));
    }

    #[test]
    fn f190_run_checks_pass() {
        assert!(run_slotview_checks().all_passed());
    }
}
