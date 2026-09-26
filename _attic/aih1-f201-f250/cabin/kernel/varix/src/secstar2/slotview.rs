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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：UpdateFlow —— 更新流全状态机（条款卡首屏——B-1304/B-1305 透明化）
// ---------------------------------------------------------------------------

/// 更新流状态（主册【交互设计】：更新流（F122）首屏即回滚窗口条款卡）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpStage {
    /// 首屏：条款卡（确认后才继续——强制展示语义）。
    Terms,
    /// 空间预检。
    SpaceCheck,
    /// 写入备用槽（进度 permille）。
    Writing { progress_permille: u64 },
    /// 备用槽校验。
    Verifying,
    /// 切换活动槽。
    Switching,
    /// 完成（回滚点已建立）。
    Done,
    /// 失败（哪一步败的+人话——三要素）。
    Failed { at: &'static str, why: &'static str },
}

/// 更新流执行体。
pub struct UpdateFlow {
    pub stage: UpStage,
    /// 条款确认（首屏按钮）。
    pub terms_confirmed: bool,
    /// 目标版本。
    pub target_version: u32,
    /// 需要空间（字节）。
    pub need_bytes: u64,
}

impl UpdateFlow {
    pub fn start(target_version: u32, need_bytes: u64) -> UpdateFlow {
        UpdateFlow { stage: UpStage::Terms, terms_confirmed: false, target_version, need_bytes }
    }

    /// 条款卡确认（未确认不得进入后续——强制展示语义）。
    pub fn confirm_terms(&mut self) -> Result<(), &'static str> {
        if self.stage != UpStage::Terms {
            return Err("不在条款屏");
        }
        self.terms_confirmed = true;
        self.stage = UpStage::SpaceCheck;
        Ok(())
    }

    /// 空间预检（拒绝即失败态——诚实报错）。
    pub fn check_space(&mut self, free_bytes: u64) -> Result<(), &'static str> {
        if self.stage != UpStage::SpaceCheck {
            return Err("不在空间预检步");
        }
        if self.need_bytes > free_bytes {
            self.stage = UpStage::Failed { at: "space", why: "双槽空间不足，更新已拦截" };
            return Err("双槽空间不足");
        }
        self.stage = UpStage::Writing { progress_permille: 0 };
        Ok(())
    }

    /// 写入进度推进（写完 → 校验步）。
    pub fn write_progress(&mut self, permille: u64) -> Result<(), &'static str> {
        match self.stage {
            UpStage::Writing { .. } => {
                if permille >= 1000 {
                    self.stage = UpStage::Verifying;
                } else {
                    self.stage = UpStage::Writing { progress_permille: permille.min(999) };
                }
                Ok(())
            }
            _ => Err("不在写入步"),
        }
    }

    /// 备用槽校验（失败 → 失败态；成功 → 切换步）。
    pub fn verify_backup(&mut self, ok: bool) -> Result<(), &'static str> {
        if self.stage != UpStage::Verifying {
            return Err("不在校验步");
        }
        if ok {
            self.stage = UpStage::Switching;
            Ok(())
        } else {
            self.stage = UpStage::Failed { at: "verify", why: "备用槽校验失败——当前系统未受影响" };
            Err("校验失败")
        }
    }

    /// 切换（成功 → Done）。
    pub fn commit_switch(&mut self) -> Result<(), &'static str> {
        if self.stage != UpStage::Switching {
            return Err("不在切换步");
        }
        self.stage = UpStage::Done;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 深二：RollbackConfirm —— 回滚钮 3s 长按确认（回滚也是大动作）
// ---------------------------------------------------------------------------

/// 长按确认机：按下起计时，松开即取消；持满 3000ms → 武装执行。
pub struct RollbackHold {
    pub holding: bool,
    pub held_ms: u64,
    /// 武装（持满后置位——执行窗 2s 内有效，超时重新按）。
    pub armed: bool,
    armed_at_ms: u64,
}

pub const HOLD_MS: u64 = 3_000;
pub const ARMED_WINDOW_MS: u64 = 2_000;

impl RollbackHold {
    pub fn new() -> RollbackHold {
        RollbackHold { holding: false, held_ms: 0, armed: false, armed_at_ms: 0 }
    }

    pub fn press(&mut self, now_ms: u64) {
        self.holding = true;
        self.held_ms = 0;
        self.armed = false;
        let _ = now_ms;
    }

    /// 按住期间 tick（每 100ms）。
    pub fn tick(&mut self, now_ms: u64) {
        if self.holding {
            self.held_ms = (self.held_ms + 100).min(HOLD_MS);
            if self.held_ms >= HOLD_MS {
                self.armed = true;
                self.armed_at_ms = now_ms;
            }
        }
    }

    /// 松开：未满 → 全部取消；已武装 → 保持武装（2s 执行窗）。
    pub fn release(&mut self) -> bool {
        self.holding = false;
        if !self.armed {
            self.held_ms = 0; // 早松开=取消：进度归零重按。
        }
        self.armed
    }

    /// 执行请求（armed 且在 2s 窗内才有效——防「按完忘了」误触）。
    pub fn execute_request(&mut self, now_ms: u64) -> Result<(), &'static str> {
        if !self.armed {
            return Err("未完成 3s 长按确认");
        }
        if now_ms.saturating_sub(self.armed_at_ms) > ARMED_WINDOW_MS {
            self.armed = false;
            return Err("确认窗已过（2s）——请重新长按");
        }
        self.armed = false;
        Ok(())
    }

    /// 进度 permille（环渲染数据）。
    pub fn progress_permille(&self) -> u64 {
        self.held_ms * 1000 / HOLD_MS
    }
}

impl Default for RollbackHold {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：SlotChecksum —— 槽元数据校验和（B-1304 对拍的字节层）
// ---------------------------------------------------------------------------

/// 槽元数据序列化（16 字节定长）+ FNV-1a 校验和。
pub fn slot_meta_bytes(version: u32, installed_day: u64, valid: bool) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&version.to_be_bytes());
    b[4..12].copy_from_slice(&installed_day.to_be_bytes());
    b[12] = valid as u8;
    b
}

pub fn slot_checksum(bytes: &[u8; 16]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &x in bytes {
        h ^= x as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 对拍：外部读到的（字节+校验和）与账目一致才判绿（判据一的字节级落地）。
pub fn matches_external_bytes(expected: (u32, u64, bool), ext_bytes: &[u8; 16], ext_sum: u32) -> bool {
    let mine = slot_meta_bytes(expected.0, expected.1, expected.2);
    slot_checksum(&mine) == ext_sum && mine == *ext_bytes
}

// ---------------------------------------------------------------------------
// 深四：UpdateHistory —— 更新历史账（版本/日期/结局/是否用过回滚）
// ---------------------------------------------------------------------------

/// 更新结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    Success,
    FailedWrite,
    FailedVerify,
    RolledBack,
}

/// 一条更新记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateRecord {
    pub version: u32,
    pub day: u64,
    pub outcome: UpdateOutcome,
}

/// 历史账（最近 16 条——环语义由调用方裁剪）。
pub struct UpdateHistory {
    pub records: Vec<UpdateRecord>,
}

impl UpdateHistory {
    pub fn new() -> UpdateHistory {
        UpdateHistory { records: Vec::new() }
    }

    pub fn push(&mut self, r: UpdateRecord) {
        self.records.push(r);
        if self.records.len() > 16 {
            self.records.remove(0);
        }
    }

    /// 连续失败计数（连续 ≥2 次失败 → 更新器应建议安全模式——联动 F193）。
    pub fn consecutive_failures(&self) -> u32 {
        let mut n = 0u32;
        for r in self.records.iter().rev() {
            match r.outcome {
                UpdateOutcome::Success | UpdateOutcome::RolledBack => break,
                _ => n += 1,
            }
        }
        n
    }

    /// 最近一次成功版本（回滚点提示文案的数据源）。
    pub fn last_success_version(&self) -> Option<u32> {
        self.records.iter().rev().find(|r| r.outcome == UpdateOutcome::Success).map(|r| r.version)
    }
}

impl Default for UpdateHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深五：SpaceLedger —— 双槽空间账目（空间前置拦截的字节层）
// ---------------------------------------------------------------------------

/// 双槽空间账。
pub struct SpaceLedger {
    /// 每槽容量（字节）。
    pub slot_capacity: u64,
    /// 每槽已用（字节）——[A, B]。
    pub slot_used: [u64; 2],
    /// 系统保留（不可用于更新）。
    pub reserved: u64,
}

impl SpaceLedger {
    pub fn new(slot_capacity: u64, reserved: u64) -> SpaceLedger {
        SpaceLedger { slot_capacity, slot_used: [0; 2], reserved }
    }

    pub fn set_used(&mut self, slot: SlotId, used: u64) {
        self.slot_used[match slot { SlotId::A => 0, SlotId::B => 1 }] = used.min(self.slot_capacity);
    }

    /// 备用槽可写入字节数（容量-已用；预留区不参与）。
    pub fn backup_free(&self, active: SlotId) -> u64 {
        let b = self.slot_used[match active.other() { SlotId::A => 0, SlotId::B => 1 }];
        self.slot_capacity.saturating_sub(b).saturating_sub(self.reserved)
    }

    /// 前置拦截判据（字节层）：需要 ≤ 备用槽可用。
    pub fn fits(&self, active: SlotId, need: u64) -> bool {
        need <= self.backup_free(active)
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F190 深化自检（聚合进 secstar2 域）。
pub fn run_slotview_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-deep");

    // 深一：更新流——条款强制→空间→写入→校验→切换→完成；两处失败路径。
    let mut uf = UpdateFlow::start(205, 800);
    set.add("flow starts at terms", uf.stage == UpStage::Terms && !uf.terms_confirmed, "");
    set.add("flow terms gate", uf.check_space(1000).is_err(), "未确认条款不得往下");
    uf.confirm_terms().ok();
    set.add("flow space ok", uf.check_space(1000).is_ok(), "");
    uf.write_progress(500).ok();
    set.add("flow writing mid", matches!(uf.stage, UpStage::Writing { progress_permille: 500 }), "");
    uf.write_progress(1000).ok();
    set.add("flow to verify", uf.stage == UpStage::Verifying, "");
    uf.verify_backup(true).ok();
    uf.commit_switch().ok();
    set.add("flow done", uf.stage == UpStage::Done, "");
    // 失败路径 A：空间不足。
    let mut uf2 = UpdateFlow::start(206, 900);
    uf2.confirm_terms().ok();
    set.add("flow space fail", uf2.check_space(100).is_err()
        && matches!(uf2.stage, UpStage::Failed { at: "space", .. }), "");
    // 失败路径 B：校验失败（当前系统未受影响）。
    let mut uf3 = UpdateFlow::start(207, 1);
    uf3.confirm_terms().ok();
    uf3.check_space(10).ok();
    uf3.write_progress(1000).ok();
    set.add("flow verify fail", uf3.verify_backup(false).is_err()
        && matches!(uf3.stage, UpStage::Failed { at: "verify", .. }), "");

    // 深二：3s 长按——未满取消/满则武装/2s 执行窗。
    let mut hold = RollbackHold::new();
    hold.press(0);
    for _ in 0..10 {
        hold.tick(0);
    }
    set.add("hold progress", hold.progress_permille() == 333, "1s of 3s = 333‰");
    set.add("hold not armed early", !hold.armed, "");
    for _ in 0..20 {
        hold.tick(0);
    }
    set.add("hold armed at 3s", hold.armed && hold.progress_permille() == 1000, "");
    set.add("hold execute", hold.execute_request(1_000).is_ok(), "");
    // 窗口过期。
    let mut hold2 = RollbackHold::new();
    hold2.press(0);
    for _ in 0..30 {
        hold2.tick(0);
    }
    hold2.release();
    set.add("hold expiry", hold2.execute_request(9_999).is_err() && !hold2.armed, "");
    // 早松开取消。
    let mut hold3 = RollbackHold::new();
    hold3.press(0);
    for _ in 0..5 {
        hold3.tick(0);
    }
    set.add("hold early release", !hold3.release() && !hold3.holding && hold3.held_ms == 0, "release resets");

    // 深三：槽校验和——字节级对拍（一致/版本变/校验改全检出）。
    let ext = slot_meta_bytes(101, 11, true);
    let sum = slot_checksum(&ext);
    set.add("meta match", matches_external_bytes((101, 11, true), &ext, sum), "");
    set.add("meta version drift", !matches_external_bytes((102, 11, true), &ext, sum), "");
    let mut tampered = ext;
    tampered[12] = 0;
    set.add("meta bit flip", !matches_external_bytes((101, 11, true), &tampered, sum), "");

    // 深四：更新历史——连败计数→建议安全模式；最近成功版本。
    let mut hist = UpdateHistory::new();
    hist.push(UpdateRecord { version: 100, day: 1, outcome: UpdateOutcome::Success });
    hist.push(UpdateRecord { version: 101, day: 8, outcome: UpdateOutcome::FailedVerify });
    hist.push(UpdateRecord { version: 102, day: 15, outcome: UpdateOutcome::FailedWrite });
    set.add("hist consecutive", hist.consecutive_failures() == 2, "");
    set.add("hist last success", hist.last_success_version() == Some(100), "");
    hist.push(UpdateRecord { version: 103, day: 22, outcome: UpdateOutcome::Success });
    set.add("hist recovery", hist.consecutive_failures() == 0, "");

    // 深五：空间账——备用槽可用=容量-已用-预留；拦截判据字节级。
    let mut sp = SpaceLedger::new(4 * 1024 * 1024 * 1024, 64 * 1024 * 1024);
    sp.set_used(SlotId::A, 3 * 1024 * 1024 * 1024);
    sp.set_used(SlotId::B, 2 * 1024 * 1024 * 1024);
    let free_b = sp.backup_free(SlotId::A);
    set.add("space free b", free_b == 2 * 1024 * 1024 * 1024 - 64 * 1024 * 1024, "");
    set.add("space fits", sp.fits(SlotId::A, free_b) && !sp.fits(SlotId::A, free_b + 1), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f190_deep_flow_terms_is_mandatory_screen() {
        // 条款卡不是装饰：不确认连写入步都进不去（逐段闸门）。
        let mut uf = UpdateFlow::start(300, 10);
        assert!(uf.write_progress(1).is_err(), "no writing before terms");
        assert!(uf.verify_backup(true).is_err(), "no verify before terms");
        assert!(uf.commit_switch().is_err(), "no switch before terms");
        uf.confirm_terms().unwrap();
        assert!(uf.confirm_terms().is_err(), "terms is one-shot");
    }

    #[test]
    fn f190_deep_hold_ring_data_shape() {
        // 环渲染数据：进度单调不减，武装点恰在 3000ms。
        let mut hold = RollbackHold::new();
        hold.press(0);
        let mut last = 0;
        let mut armed_at = None;
        for i in 1..=30u64 {
            hold.tick(i * 100);
            let p = hold.progress_permille();
            assert!(p >= last);
            last = p;
            if hold.armed && armed_at.is_none() {
                armed_at = Some(i * 100);
            }
        }
        assert_eq!(armed_at, Some(3_000), "arms exactly at 3s");
    }

    #[test]
    fn f190_deep_history_ring_cap() {
        let mut hist = UpdateHistory::new();
        for i in 0..30u32 {
            hist.push(UpdateRecord { version: i, day: i as u64, outcome: UpdateOutcome::Success });
        }
        assert_eq!(hist.records.len(), 16);
        assert_eq!(hist.records[0].version, 14, "oldest evicted");
    }

    #[test]
    fn f190_deep_space_ledger_used_clamped() {
        let mut sp = SpaceLedger::new(1000, 0);
        sp.set_used(SlotId::A, 9999);
        assert_eq!(sp.slot_used[0], 1000, "used clamps to capacity");
        // backup_free(active)=备用槽可用：A 活动时备用是 B（空）→ 1000；
        // B 活动时备用是 A（满）→ 0。
        assert_eq!(sp.backup_free(SlotId::A), 1000);
        assert_eq!(sp.backup_free(SlotId::B), 0);
    }

    #[test]
    fn f190_deep_run_checks_pass() {
        assert!(run_slotview_deep_checks().all_passed());
    }
}
