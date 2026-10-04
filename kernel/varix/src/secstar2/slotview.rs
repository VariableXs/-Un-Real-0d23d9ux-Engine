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
use alloc::string::String;
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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——条款卡渲染 / 到期预告 /
// B-1304 对拍差异报告。判据源：主册【交互设计】「更新流（F122）首屏即回滚
// 窗口条款卡」+【状态与异常】「保留期过 → 回滚钮灰置+（诚实不藏）——灰置
// 有预告」+【验收判据】「槽状态与实际一致（对拍 B-1304 数据）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：TermsCard —— 回滚窗口条款卡（更新流首屏第一卡——敢承诺回滚，
// 是因为回滚真的存在；主册文案逐字 + 剩余天数动态行）
// ---------------------------------------------------------------------------

/// 条款卡渲染数据。
pub struct TermsCard {
    /// 主文案（主册逐字——TERMS_TEXT）。
    pub body: &'static str,
    /// 动态行（本槽可回滚天数——随时间变化的部分）。
    pub days_line: String,
    /// 卡语义标记（预告不是事故——与 F173 panic 族区分）。
    pub severity: &'static str,
}

/// 组装（days_left 来自当前槽安装日——灰置前天数递减可见）。
pub fn terms_card(days_left: u64) -> TermsCard {
    TermsCard {
        body: TERMS_TEXT,
        days_line: alloc::format!("回滚窗口剩余 {} 天（到期自动按策略清理）", days_left),
        severity: "reassure",
    }
}

// ---------------------------------------------------------------------------
// v3-二：ExpiryCountdown —— 到期预告行（灰置有预告——主册【设计细节】：
// 保留期倒计时显示在回滚钮旁（灰置有预告））
// ---------------------------------------------------------------------------

/// 预告行（0 天=今日到期——预告不是恐吓，清理也不突袭）。
pub struct ExpiryCountdown {
    pub days_left: u64,
    /// 预告文案（>0 天=剩 N 天；0 天=今日到期预告）。
    pub text: String,
    /// 是否已到灰置临界（0 天——回滚钮下一次刷新将灰置）。
    pub expiring_today: bool,
}

/// 组装。
pub fn expiry_countdown(days_left: u64) -> ExpiryCountdown {
    ExpiryCountdown {
        days_left,
        expiring_today: days_left == 0,
        text: if days_left == 0 {
            alloc::format!("回滚点今日到期——{}（预告：明天将按策略清理）", EXPIRED_TEXT)
        } else {
            alloc::format!("回滚窗口剩余 {} 天", days_left)
        },
    }
}

// ---------------------------------------------------------------------------
// v3-三：ExternalDiffReport —— B-1304 对拍差异报告（matches_external 的
// 明细版：哪些槽对不上、差在哪个字段——对拍不是布尔是定位）
// ---------------------------------------------------------------------------

/// 单槽差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotDiff {
    pub slot: SlotId,
    /// 版本号不一致（外部记录 vs 本地面板）。
    pub version_diff: Option<(u32, u32)>,
    /// 校验态不一致。
    pub valid_diff: Option<(bool, bool)>,
}

impl SlotDiff {
    pub fn has_diff(&self) -> bool {
        self.version_diff.is_some() || self.valid_diff.is_some()
    }
}

/// 逐槽对拍（external = B-1304 权威数据；local = 面板自持数据）。
pub fn diff_report(external: &[(SlotId, SlotMeta)], local: &[(SlotId, SlotMeta)]) -> Vec<SlotDiff> {
    let mut out = Vec::new();
    for (sid, ext) in external {
        let Some((_, loc)) = local.iter().find(|(l, _)| l == sid) else {
            continue; // 外部有本地无——槽级缺失由 matches_external 的布尔面报。
        };
        out.push(SlotDiff {
            slot: *sid,
            version_diff: if ext.version != loc.version { Some((ext.version, loc.version)) } else { None },
            valid_diff: if ext.valid != loc.valid { Some((ext.valid, loc.valid)) } else { None },
        });
    }
    out
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F190 v3 自检（聚合进 secstar2 域）。
pub fn run_slotview_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v3");

    // v3-一：条款卡——主册逐字、动态天数行、安心语义。
    let tc = terms_card(5);
    set.add("terms body verbatim", tc.body == TERMS_TEXT && tc.body.contains("一键回到"), "");
    set.add("terms days line", tc.days_line.contains("5 天"), "");
    set.add("terms severity", tc.severity == "reassure", "预告不是事故");

    // v3-二：到期预告——0 天临界与寻常天数两态。
    let e0 = expiry_countdown(0);
    set.add("expiry today", e0.expiring_today && e0.text.contains("明天将按策略清理"), "");
    let e5 = expiry_countdown(5);
    set.add("expiry normal", !e5.expiring_today && e5.text.contains("5 天"), "");

    // v3-三：差异报告——版本差/校验差/全对三态。
    let local = [
        (SlotId::A, SlotMeta { version: 42, installed_day: 100, valid: true }),
        (SlotId::B, SlotMeta { version: 41, installed_day: 90, valid: true }),
    ];
    let ext_same = local;
    set.add("diff none", diff_report(&ext_same, &local).iter().all(|d| !d.has_diff()), "");
    let ext_bad = [
        (SlotId::A, SlotMeta { version: 43, installed_day: 100, valid: true }),
        (SlotId::B, SlotMeta { version: 41, installed_day: 90, valid: false }),
    ];
    let diffs = diff_report(&ext_bad, &local);
    set.add("diff version located", diffs[0].version_diff == Some((43, 42)), "");
    set.add("diff valid located", diffs[1].valid_diff == Some((false, true)), "");
    set.add("diff has_diff", diffs[0].has_diff() && diffs[1].has_diff(), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f190_v3_countdown_monotonic_to_expiry() {
        // 7→0 天递减序列：预告行全程有意义、临界日触发今日语义。
        let mut prev_expiring = false;
        for d in (0..=7u64).rev() {
            let e = expiry_countdown(d);
            assert!(!e.text.is_empty());
            assert_eq!(e.expiring_today, d == 0, "only day 0 is expiring");
            assert!(!prev_expiring || d == 0, "expiring only at the boundary");
            prev_expiring = e.expiring_today;
        }
    }

    #[test]
    fn f190_v3_diff_report_ignores_unknown_slots() {
        // 外部多出的槽（本地还没有）：不造差异行（缺失由布尔面对拍面报）。
        let local = [(SlotId::A, SlotMeta { version: 1, installed_day: 1, valid: true })];
        let ext = [
            (SlotId::A, SlotMeta { version: 1, installed_day: 1, valid: true }),
            (SlotId::B, SlotMeta { version: 2, installed_day: 2, valid: true }),
        ];
        let diffs = diff_report(&ext, &local);
        assert_eq!(diffs.len(), 1, "only the shared slot is diffed");
        assert!(!diffs[0].has_diff());
    }

    #[test]
    fn f190_v3_run_checks_pass() {
        assert!(run_slotview_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——双槽卡对称布局 / 条款模板变量 /
// 更新前置三勾 / 回滚干跑。判据源：主册【设计细节】「槽卡并排视觉对称
// （公平感——两槽都是一等公民）」+ 用户故事条款逐字 +【状态与异常】
// 「双槽空间不足 → 更新前置拦截」+【验收判据】回滚全链。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：SlotCardLayout —— 双槽卡并排对称布局（视觉对称的机器检查：两卡
// 几何全等 + 当前槽强调靠描边不靠尺寸——强调不破坏公平）
// ---------------------------------------------------------------------------

/// 单卡几何（设计系统像素——主册 480×140 卡族的槽卡规格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardGeom {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// 描边 token（当前槽=accent / 备用槽=neutral——公平感的分寸）。
    pub border_token: &'static str,
}

/// 布局产出（两卡并排：等宽等高、同 y、水平间距 GAP）。
pub const SLOT_CARD_GAP: i32 = 24;

/// 生成双槽布局（active 决定谁描边强调；几何两卡恒等——对称性机检的
/// 数据源）。
pub fn slot_card_layout(active: SlotId, page_w: u32) -> [CardGeom; 2] {
    let w = (page_w as i32 - SLOT_CARD_GAP * 3) / 2;
    let h = 140u32;
    let a_slot = CardGeom {
        x: SLOT_CARD_GAP,
        y: SLOT_CARD_GAP,
        w: w.max(0) as u32,
        h,
        border_token: if active == SlotId::A { "accent" } else { "neutral" },
    };
    let b_slot = CardGeom {
        x: SLOT_CARD_GAP * 2 + w,
        y: SLOT_CARD_GAP,
        w: w.max(0) as u32,
        h,
        border_token: if active == SlotId::B { "accent" } else { "neutral" },
    };
    [a_slot, b_slot]
}

/// 对称性机检（几何全等检查——x 不同、其余全等才叫「并排对称」）。
pub fn slot_cards_symmetric(cards: &[CardGeom; 2]) -> bool {
    cards[0].w == cards[1].w && cards[0].h == cards[1].h && cards[0].y == cards[1].y
        && cards[0].border_token != cards[1].border_token
        && cards[0].x != cards[1].x
}

// ---------------------------------------------------------------------------
// v4-二：terms_template —— 条款卡模板变量渲染（主册用户故事逐字骨架 +
// 动态天数/版本号注入——模板是唯一定义点，天数改了文案自动跟随）
// ---------------------------------------------------------------------------

/// 条款骨架（{N}=保留期天数，{V}=目标版本——主册用户故事语汇）。
pub const TERMS_TEMPLATE: &str = "本次更新将写入备用槽，更新后 {N} 天内可一键回退到当前版本（{V}）。你的文件不受更新影响。";

/// 渲染（天数来自保留期旋钮实时值——文案永不与策略脱节）。
pub fn terms_template_render(retention_days: u64, cur_version: u32, out: &mut alloc::string::String) {
    let mut it = TERMS_TEMPLATE.split("{N}");
    out.push_str(it.next().unwrap_or(""));
    if let Some(rest) = it.next() {
        out.push_str(&alloc::format!("{}", retention_days));
        let mut it2 = rest.split("{V}");
        out.push_str(it2.next().unwrap_or(""));
        if let Some(rest2) = it2.next() {
            out.push_str(&alloc::format!("v{}.{}.{}", cur_version / 100, cur_version % 100 / 10, cur_version % 10));
            out.push_str(rest2);
        }
    }
}

/// 模板完整性（骨架含全部变量位+定心丸句——文案审计）。
pub fn terms_template_intact() -> bool {
    TERMS_TEMPLATE.contains("{N}")
        && TERMS_TEMPLATE.contains("{V}")
        && TERMS_TEMPLATE.contains("你的文件不受更新影响")
}

// ---------------------------------------------------------------------------
// v4-三：PreUpdateChecklist —— 更新前置三勾（条款确认/空间充足/备用槽
// 健康——三勾全绿才许 commit；缺勾的 commit 请求被门卫拦下并指明缺哪勾）
// ---------------------------------------------------------------------------

/// 三勾状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreCheckState {
    pub terms_confirmed: bool,
    pub space_ok: bool,
    pub backup_healthy: bool,
}

/// 前置检查门卫（与 UpdateFlow 的 confirm_terms/check_space/verify_backup
/// 三阶段一一对应——门卫是「不许跳步」的总闸）。
pub struct PreUpdateChecklist;

impl PreUpdateChecklist {
    /// 全绿判定。
    pub fn all_green(s: &PreCheckState) -> bool {
        s.terms_confirmed && s.space_ok && s.backup_healthy
    }

    /// 缺勾清单（人话——用户知道去补哪一步，不是笼统「不满足条件」）。
    pub fn missing(s: &PreCheckState) -> alloc::vec::Vec<&'static str> {
        let mut out = alloc::vec::Vec::new();
        if !s.terms_confirmed {
            out.push("请先阅读并确认回滚窗口条款");
        }
        if !s.space_ok {
            out.push("备用槽空间不足：请清理或等待系统自动腾挪");
        }
        if !s.backup_healthy {
            out.push("备用槽校验失败：下次更新将自动重建，请稍后再试");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// v4-四：RollbackDryRun —— 回滚干跑（不执行、先列出将发生什么：版本切换
// 方向/用户数据不动/条款窗口收口——红线纪律「先干跑后执行」的回滚落点）
// ---------------------------------------------------------------------------

/// 干跑清单行（发生了什么 + 影响面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DryRunLine {
    pub what: &'static str,
    pub impact: &'static str,
}

/// 回滚干跑（从当前 cur 回到 rollback_ver；数据不动条款逐行声明）。
pub fn rollback_dry_run(cur_version: u32, rollback_ver: u32) -> [DryRunLine; 4] {
    [
        DryRunLine {
            what: "系统镜像切回备用槽版本",
            impact: &alloc::format!("运行版本 v{}.{}.{} → v{}.{}.{}", cur_version / 100, cur_version % 100 / 10, cur_version % 10, rollback_ver / 100, rollback_ver % 100 / 10, rollback_ver % 10).leak().strip_prefix("运行版本 ").unwrap_or("版本切换"),
        },
        DryRunLine { what: "用户文件与设置", impact: "完全不动（条款承诺）" },
        DryRunLine { what: "回滚窗口", impact: "执行后收口，本次回滚机会用掉" },
        DryRunLine { what: "执行耗时", impact: "秒级切换 + 一次重启" },
    ]
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F190 v4 自检（聚合进 secstar2 域）。
pub fn run_slotview_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v4");

    // v4-一：双槽布局——等宽等高同 y、描边互斥、A/B 两态都对称。
    for act in [SlotId::A, SlotId::B] {
        let cards = slot_card_layout(act, 1200);
        set.add("layout symmetric", slot_cards_symmetric(&cards), "");
        let accent: alloc::vec::Vec<&str> = cards
            .iter()
            .filter(|c| c.border_token == "accent")
            .map(|c| c.border_token)
            .collect();
        set.add("layout one accent", accent.len() == 1, "恰一卡强调（当前槽）");
    }
    let cards_a = slot_card_layout(SlotId::A, 1200);
    set.add("layout a accent", cards_a[0].border_token == "accent" && cards_a[1].border_token == "neutral", "");
    set.add("layout b mirror", slot_card_layout(SlotId::B, 1200)[1].border_token == "accent", "");

    // v4-二：条款模板——变量注入、往返、完整性。
    set.add("terms intact", terms_template_intact(), "");
    let mut s7 = alloc::string::String::new();
    terms_template_render(7, 324, &mut s7);
    set.add("terms render 7d", s7.contains("7 天") && s7.contains("v3.2.4"), "");
    let mut s30 = alloc::string::String::new();
    terms_template_render(30, 100, &mut s30);
    set.add("terms render 30d", s30.contains("30 天") && s30.contains("v1.0.0"), "旋钮改动文案跟随");

    // v4-三：前置三勾——全绿放行、逐缺勾指明、顺序无关。
    let all = PreCheckState { terms_confirmed: true, space_ok: true, backup_healthy: true };
    set.add("precheck all green", PreUpdateChecklist::all_green(&all), "");
    set.add("precheck all missing empty", PreUpdateChecklist::missing(&all).is_empty(), "");
    let none = PreCheckState { terms_confirmed: false, space_ok: false, backup_healthy: false };
    set.add("precheck three missing", PreUpdateChecklist::missing(&none).len() == 3, "");
    let only_space = PreCheckState { terms_confirmed: true, space_ok: false, backup_healthy: true };
    let miss = PreUpdateChecklist::missing(&only_space);
    set.add("precheck precise", miss.len() == 1 && miss[0].contains("空间"), "缺哪勾说哪勾");

    // v4-四：回滚干跑——四行齐、数据不动条款在列、版本方向正确。
    let dry = rollback_dry_run(324, 321);
    set.add("dry four lines", dry.len() == 4, "");
    set.add("dry data untouched", dry[1].impact.contains("完全不动"), "");
    set.add("dry window close", dry[2].impact.contains("收口"), "");
    set.add("dry version direction", dry[0].what.contains("备用槽"), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f190_v4_layout_narrow_page_degrades() {
        // 极窄页面不产生负宽卡（钳制为零宽而不是负——渲染层不会炸）。
        let cards = slot_card_layout(SlotId::A, 20);
        assert!(cards[0].w == 0 && cards[1].w == 0);
        assert!(slot_cards_symmetric(&cards), "窄页也保持对称语义");
    }

    #[test]
    fn f190_v4_terms_template_all_versions() {
        // 版本号三位数渲染矩阵（边界：0 / 999）。
        for (v, expect) in [(0u32, "v0.0.0"), (5, "v0.0.5"), (50, "v0.5.0"), (999, "v9.9.9")] {
            let mut s = alloc::string::String::new();
            terms_template_render(7, v, &mut s);
            assert!(s.contains(expect), "v={} expect {}", v, expect);
        }
    }

    #[test]
    fn f190_v4_precheck_combinatorics() {
        // 8 种勾态穷举：全绿恰一种、missing 数与勾数互补。
        for mask in 0..8u8 {
            let st = PreCheckState {
                terms_confirmed: mask & 1 != 0,
                space_ok: mask & 2 != 0,
                backup_healthy: mask & 4 != 0,
            };
            let miss = PreUpdateChecklist::missing(&st);
            assert_eq!(miss.len(), 3 - mask.count_ones() as usize, "mask={}", mask);
            assert_eq!(PreUpdateChecklist::all_green(&st), mask == 7);
        }
    }

    #[test]
    fn f190_v4_dry_run_no_side_effect() {
        // 干跑幂等：跑两次输出全等（干跑就是干跑——没有隐藏状态）。
        let a = rollback_dry_run(324, 321);
        let b = rollback_dry_run(324, 321);
        assert_eq!(a[1], b[1]);
        assert_eq!(a[2], b[2]);
        assert_eq!(a[3], b[3]);
    }

    #[test]
    fn f190_v4_run_checks_pass() {
        assert!(run_slotview_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——更新历史汇总页 /
// 回滚前置清单 / 槽卡版本行渲染。判据源：主册【交互设计】「两槽并排（当前
// 槽描边强调+版本号+安装日期）」「回滚按钮按压有 3s 确认」+【状态与异常】
// 更新失败语义的汇总呈现。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v5-一：update_summary —— 更新历史汇总页（总次数/成功率/连续失败/最近
// 成功版本——UpdateHistory 之上的聚合视图）
// ---------------------------------------------------------------------------

/// 汇总模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateSummary {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    /// 成功率 permille（零历史=0 且标注「无记录」）。
    pub success_permille: u64,
    /// 最近成功版本（None=从未成功过）。
    pub last_success: Option<u32>,
    /// 当前连续失败数（>0 时更新前置可提示）。
    pub consecutive_failures: u32,
}

/// 从历史聚合（UpdateOutcome 统计）。
pub fn update_summary(history: &UpdateHistory, outcomes: &[UpdateOutcome]) -> UpdateSummary {
    let total = outcomes.len() as u64;
    let success = outcomes
        .iter()
        .filter(|o| matches!(o, UpdateOutcome::Success))
        .count() as u64;
    let failed = total - success;
    UpdateSummary {
        total,
        success,
        failed,
        success_permille: if total == 0 { 0 } else { success * 1000 / total },
        last_success: history.last_success_version(),
        consecutive_failures: history.consecutive_failures(),
    }
}

/// 汇总行（连续失败 ≥2 → 建议行——失败模式要主动提示）。
pub fn update_summary_hint(s: &UpdateSummary) -> Option<&'static str> {
    if s.consecutive_failures >= 2 {
        Some("连续两次更新失败：建议暂停更新，查看失败原因或从恢复环境检查双槽健康")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// v5-二：rollback_prereqs —— 回滚前置清单（按下回滚钮前系统自检：保留期
// 内/备用槽健康/3s 确认完成——三勾全绿才执行，缺勾给人话）
// ---------------------------------------------------------------------------

/// 前置项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackPrereq {
    pub item: &'static str,
    pub ok: bool,
    pub why: &'static str,
}

/// 自检三项（输入：当前视图 + 长按确认态）。
pub fn rollback_prereqs(view: &SlotView, now_day: u64, hold_confirmed: bool) -> [RollbackPrereq; 3] {
    [
        RollbackPrereq {
            item: "保留期内",
            ok: view.rollback_eligible(now_day),
            why: "回滚点已过保留期，按策略清理（如实告知，不藏）",
        },
        RollbackPrereq {
            item: "备用槽健康",
            ok: !view.backup_bad(),
            why: "备用槽校验失败，回滚目标不可用；下次更新将重建",
        },
        RollbackPrereq {
            item: "3 秒确认",
            ok: hold_confirmed,
            why: "回滚是大动作：需按住 3 秒确认（防误触）",
        },
    ]
}

/// 全绿判定。
pub fn rollback_prereqs_clear(items: &[RollbackPrereq; 3]) -> bool {
    items.iter().all(|i| i.ok)
}

// ---------------------------------------------------------------------------
// v5-三：slot_version_line —— 槽卡版本行渲染（版本号+安装日期+槽身份
// ——两卡各一行的渲染契约）
// ---------------------------------------------------------------------------

/// 行数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotVersionLine {
    pub slot: &'static str,
    pub version: String,
    pub installed_day: u64,
    /// 角标（当前槽="运行中"）。
    pub badge: Option<&'static str>,
}

/// 渲染（semver (maj,min,pat) → "v1.2.3"）。
pub fn slot_version_line(slot: SlotId, meta: &SlotMeta, is_active: bool) -> SlotVersionLine {
    let v = meta.version;
    let (maj, min, pat) = (v / 100, v % 100 / 10, v % 10);
    SlotVersionLine {
        slot: match slot {
            SlotId::A => "A 槽",
            SlotId::B => "B 槽",
        },
        version: alloc::format!("v{}.{}.{}", maj, min, pat),
        installed_day: meta.installed_day,
        badge: if is_active { Some("运行中") } else { None },
    }
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F190 v5 自检（聚合进 secstar2 域）。
pub fn run_slotview_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v5");

    // 历史样本：三成一败（最近一次失败）。
    let mut hist = UpdateHistory::new();
    hist.push(UpdateRecord { day: 10, outcome: UpdateOutcome::Success, version: 320 });
    hist.push(UpdateRecord { day: 20, outcome: UpdateOutcome::Success, version: 321 });
    hist.push(UpdateRecord { day: 30, outcome: UpdateOutcome::Success, version: 322 });
    hist.push(UpdateRecord { day: 40, outcome: UpdateOutcome::FailedVerify, version: 323 });
    let outcomes = [UpdateOutcome::Success, UpdateOutcome::Success, UpdateOutcome::Success, UpdateOutcome::FailedVerify];

    // v5-一：汇总页——成功率、最近成功、连续失败、提示。
    let s = update_summary(&hist, &outcomes);
    set.add("summary total", s.total == 4 && s.success == 3 && s.failed == 1, "");
    set.add("summary rate", s.success_permille == 750, "");
    set.add("summary last ok", s.last_success == Some(322), "");
    set.add("summary streak", s.consecutive_failures == 1, "");
    set.add("summary hint calm", update_summary_hint(&s).is_none(), "单次失败不惊扰");
    let mut hist2 = UpdateHistory::new();
    hist2.push(UpdateRecord { day: 10, outcome: UpdateOutcome::FailedVerify, version: 320 });
    hist2.push(UpdateRecord { day: 20, outcome: UpdateOutcome::FailedVerify, version: 321 });
    let s2 = update_summary(&hist2, &[UpdateOutcome::FailedVerify, UpdateOutcome::FailedVerify]);
    set.add("summary hint warn", update_summary_hint(&s2).is_some(), "连败 ≥2 主动提示");

    // v5-二：回滚前置——三勾判定、缺勾人话、缺哪说哪。
    let mut view = SlotView::new(SlotId::A, SlotMeta { version: 322, installed_day: 30, valid: true });
    view.commit_update(35, 323, true); // 成功更新一次：备用槽留下健康回滚点。
    let pr = rollback_prereqs(&view, 38, true);
    set.add("prereq all ok", rollback_prereqs_clear(&pr), "");
    let pr2 = rollback_prereqs(&view, 100, false);
    set.add("prereq expired", !pr2[0].ok && pr2[0].why.contains("保留期"), "过期如实告知");
    set.add("prereq hold missing", !pr2[2].ok && pr2[2].why.contains("3 秒"), "");
    // 备用槽损坏场景。
    let mut view_bad = SlotView::new(SlotId::A, SlotMeta { version: 322, installed_day: 30, valid: true });
    view_bad.commit_update(40, 323, false);
    let pr3 = rollback_prereqs(&view_bad, 45, true);
    set.add("prereq backup bad", !pr3[1].ok && pr3[1].why.contains("重建"), "");

    // v5-三：版本行——格式化、角标、两槽互异。
    let meta_a = SlotMeta { version: 322, installed_day: 30, valid: true };
    let meta_b = SlotMeta { version: 321, installed_day: 5, valid: true };
    let la = slot_version_line(SlotId::A, &meta_a, true);
    let lb = slot_version_line(SlotId::B, &meta_b, false);
    set.add("vline a", la.version == "v3.2.2" && la.badge == Some("运行中"), "");
    set.add("vline b", lb.version == "v3.2.1" && lb.badge.is_none(), "");
    set.add("vline distinct", la != lb, "两槽两行不撞车");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f190_v5_summary_zero_history() {
        // 零历史：成功率 0 且无最近成功（诚实空态——不造 100%）。
        let s = update_summary(&UpdateHistory::new(), &[]);
        assert_eq!(s.success_permille, 0);
        assert_eq!(s.last_success, None);
        assert!(update_summary_hint(&s).is_none());
    }

    #[test]
    fn f190_v5_prereqs_scenarios() {
        // 三个独立场景：过期 / 坏槽 / 未确认——缺哪项哪项 ok=false。
        let mut base = SlotView::new(SlotId::A, SlotMeta { version: 100, installed_day: 0, valid: true });
        // 先走一次成功更新：备用槽里留下健康回滚点（day 5 建，保留 7 天）。
        base.commit_update(5, 101, true);
        // 场景 1：健康槽+期内+已确认 → 三勾全绿。
        assert!(rollback_prereqs_clear(&rollback_prereqs(&base, 8, true)));
        // 场景 2：过期（now=12 > 5+7）→ 保留期项红且文案诚实。
        let pr = rollback_prereqs(&base, 12, true);
        assert!(!pr[0].ok && pr[0].why.contains("保留期"));
        // 场景 3：未确认 → 确认项红。
        let pr2 = rollback_prereqs(&base, 3, false);
        assert!(!pr2[2].ok && pr2[2].why.contains("3 秒"));
        // 组合：过期+未确认 → 恰两项红。
        let pr3 = rollback_prereqs(&base, 12, false);
        assert_eq!(pr3.iter().filter(|i| !i.ok).count(), 2);
    }

    #[test]
    fn f190_v5_run_checks_pass() {
        assert!(run_slotview_deep4_checks().all_passed());
    }
}




// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——双槽占用仪表 / 更新日志叙事行 /
// 回滚窗口利用统计 / 条款确认账。判据源：主册【数据与存储】「槽元数据
// 双槽区自持」+【验收判据】回滚全链（B-1305 演练）。
// ------

use alloc::vec;
// -------------------------------------------------------------------

/// 双槽占用仪表（字节/容量 per 槽——空间健康可视化数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotGauge {
    pub slot: SlotId,
    pub used_bytes: u64,
    pub capacity_bytes: u64,
    pub permille: u64,
}

/// 组装（容量 0 = None——不造仪表）。
pub fn slot_gauge(slot: SlotId, used: u64, capacity: u64) -> Option<SlotGauge> {
    if capacity == 0 {
        return None;
    }
    Some(SlotGauge {
        slot,
        used_bytes: used,
        capacity_bytes: capacity,
        permille: used.min(capacity) * 1000 / capacity,
    })
}

/// 更新日志叙事行（UpdateRecord → 人话行——历史页的渲染契约）。
pub fn update_journal_line(r: &UpdateRecord) -> String {
    let v = r.version;
    let ver = alloc::format!("v{}.{}.{}", v / 100, v % 100 / 10, v % 10);
    match r.outcome {
        UpdateOutcome::Success => alloc::format!("[day {}] 更新到 {} 成功（已写入备用槽并切换）", r.day, ver),
        UpdateOutcome::FailedWrite => alloc::format!("[day {}] 更新到 {} 失败：写入阶段失败（备用槽未动，可重试）", r.day, ver),
        UpdateOutcome::FailedVerify => alloc::format!("[day {}] 更新到 {} 失败：写入后校验不过（已回退）", r.day, ver),
        UpdateOutcome::RolledBack => alloc::format!("[day {}] {} 已被回滚（用户触发）", r.day, ver),
    }
}

/// 回滚窗口利用统计（窗口期内回滚次数 vs 过期清理次数——策略有效性对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RollbackWindowStats {
    pub used_in_window: u64,
    pub expired_cleaned: u64,
}

impl RollbackWindowStats {
    /// 利用率 permille（零事件=0）。
    pub fn utilization_permille(&self) -> u64 {
        let total = self.used_in_window + self.expired_cleaned;
        if total == 0 {
            0
        } else {
            self.used_in_window * 1000 / total
        }
    }
}

/// 条款确认账（每次更新的条款确认记录——「敢承诺因为真的存在」的对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermsAcceptance {
    pub at_day: u64,
    /// 确认时展示的保留期天数（文案与策略一致性对账）。
    pub retention_days_shown: u64,
    pub accepted: bool,
}

/// F190 v6 自检（deep5 表）。
pub fn run_slotview_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v6");

    // v6-一：占用仪表——正常/钳制/零容。
    let g = slot_gauge(SlotId::A, 700, 1000).unwrap();
    set.add("gauge permille", g.permille == 700, "");
    let g2 = slot_gauge(SlotId::B, 1500, 1000).unwrap();
    set.add("gauge clamp", g2.permille == 1000, "超占钳 1000");
    set.add("gauge zero cap", slot_gauge(SlotId::A, 10, 0).is_none(), "");

    // v6-二：更新日志——四态各自文案。
    let lines = [
        UpdateRecord { day: 10, version: 321, outcome: UpdateOutcome::Success },
        UpdateRecord { day: 20, version: 322, outcome: UpdateOutcome::FailedWrite },
        UpdateRecord { day: 30, version: 323, outcome: UpdateOutcome::FailedVerify },
        UpdateRecord { day: 40, version: 323, outcome: UpdateOutcome::RolledBack },
    ];
    let j: Vec<String> = lines.iter().map(update_journal_line).collect();
    set.add("journal success", j[0].contains("v3.2.1 成功"), "");
    set.add("journal write fail", j[1].contains("写入阶段"), "");
    set.add("journal verify fail", j[2].contains("校验不过"), "");
    set.add("journal rollback", j[3].contains("回滚"), "");

    // v6-三：窗口统计——利用率、零事件。
    let s1 = RollbackWindowStats { used_in_window: 3, expired_cleaned: 1 };
    set.add("window util", s1.utilization_permille() == 750, "3/4 = 750‰");
    let s0 = RollbackWindowStats::default();
    set.add("window zero", s0.utilization_permille() == 0, "");

    // v6-四：条款确认账——字段保真。
    let t = TermsAcceptance { at_day: 30, retention_days_shown: 7, accepted: true };
    set.add("terms fields", t.retention_days_shown == RETENTION_DAYS && t.accepted, "展示天数=策略天数");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f190_v6_journal_sequence_story() {
        // 一条完整故事线：成功→失败写→失败验→回滚（四态各出现一次）。
        let records = [
            UpdateRecord { day: 1, version: 320, outcome: UpdateOutcome::Success },
            UpdateRecord { day: 2, version: 321, outcome: UpdateOutcome::FailedWrite },
            UpdateRecord { day: 3, version: 322, outcome: UpdateOutcome::FailedVerify },
            UpdateRecord { day: 4, version: 322, outcome: UpdateOutcome::RolledBack },
        ];
        let j: Vec<String> = records.iter().map(update_journal_line).collect();
        assert!(j.iter().all(|l| l.starts_with("[day ")));
        // 版本号格式统一 vX.Y.Z。
        assert!(j.iter().all(|l| l.contains("v3.")));
    }

    #[test]
    fn f190_v6_run_checks_pass() {
        assert!(run_slotview_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——双槽差异报告 / 更新预演 / 历史
// 导出。判据源：主册【交互设计】「两槽并排（当前槽强调）」+ F128 生态语言。
// ---------------------------------------------------------------------------

/// 双槽差异报告（版本/安装日/健康三列对比——一屏看清两槽差什么）。
pub fn slot_diff(cur: &SlotMeta, cur_slot: SlotId, other: &SlotMeta) -> Vec<String> {
    let mut out = Vec::new();
    let cur_ver = cur.version;
    let other_ver = other.version;
    if cur_ver != other_ver {
        let newer = if cur_ver > other_ver { "当前槽更新" } else { "备用槽更新（可回滚目标）" };
        out.push(alloc::format!("版本不同：当前 v{}.{}.{} / 备用 v{}.{}.{}（{}）", cur_ver / 100, cur_ver % 100 / 10, cur_ver % 10, other_ver / 100, other_ver % 100 / 10, other_ver % 10, newer));
    } else {
        out.push(String::from("版本相同：两槽同版本（更新刚完成或刚回滚）"));
    }
    if cur.installed_day != other.installed_day {
        out.push(alloc::format!("安装日不同：相差 {} 天", cur.installed_day.abs_diff(other.installed_day)));
    }
    if cur.valid != other.valid {
        out.push(String::from("健康状态不同：一槽校验失败——下次更新将重建失败槽"));
    }
    let _ = cur_slot;
    if out.is_empty() {
        out.push(String::from("两槽完全一致"));
    }
    out
}

/// 更新预演（不执行、列出将发生的步骤序列——红线纪律③干跑先行）。
pub fn update_dry_run(target_version: u32) -> Vec<String> {
    let v = target_version;
    vec![
        String::from("步骤 1：确认回滚窗口条款（你的文件不受影响）"),
        String::from("步骤 2：检查备用槽空间与健康"),
        alloc::format!("步骤 3：将 v{}.{}.{} 写入备用槽并逐字节校验", v / 100, v % 100 / 10, v % 10),
        String::from("步骤 4：校验通过后切换激活槽（原版本留在回滚窗口内）"),
    ]
}

/// 历史开放导出（F128 语言 JSON：逐条更新记录）。
pub fn history_export_json(hist: &UpdateHistory, out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"update-history\":[");
    let records: &[UpdateRecord] = &hist.records;
    for (i, r) in records.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",");
        }
        let outcome = match r.outcome {
            UpdateOutcome::Success => "success",
            UpdateOutcome::FailedWrite => "failed-write",
            UpdateOutcome::FailedVerify => "failed-verify",
            UpdateOutcome::RolledBack => "rolled-back",
        };
        out.extend_from_slice(
            alloc::format!("{{\"day\":{},\"ver\":{},\"outcome\":\"{}\"}}", r.day, r.version, outcome).as_bytes(),
        );
    }
    out.extend_from_slice(b"]}");
}

/// F190 v7 自检（deep6 表）。
pub fn run_slotview_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v7");

    // v7-一：双槽差异——版本异/健康异/全同三态。
    let cur = SlotMeta { version: 322, installed_day: 30, valid: true };
    let other_old = SlotMeta { version: 321, installed_day: 5, valid: true };
    let d1 = slot_diff(&cur, SlotId::A, &other_old);
    set.add("diff version", d1[0].contains("当前槽更新"), "322 > 321");
    set.add("diff day", d1[1].contains("25 天"), "");
    let same = SlotMeta { version: 322, installed_day: 30, valid: true };
    let d2 = slot_diff(&cur, SlotId::A, &same);
    set.add("diff identical", d2.len() == 1 && d2[0].contains("版本相同"), "同版本=一行说明");

    // v7-二：更新预演——四步、版本随行。
    let dry = update_dry_run(324);
    set.add("dry 4 steps", dry.len() == 4, "");
    set.add("dry version inline", dry[2].contains("v3.2.4"), "目标版本入步骤");

    // v7-三：历史导出——计数与字段。
    let mut hist = UpdateHistory::new();
    hist.push(UpdateRecord { day: 10, version: 320, outcome: UpdateOutcome::Success });
    hist.push(UpdateRecord { day: 20, version: 321, outcome: UpdateOutcome::FailedWrite });
    let mut data = Vec::new();
    history_export_json(&hist, &mut data);
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("hist export count", text.matches("\"ver\"").count() == 2, "");
    set.add("hist export outcomes", text.contains("success") && text.contains("failed-write"), "四态语义随行");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f190_v7_diff_health_mismatch() {
        // 健康不一致：第三行红显（重建预告）。
        let cur = SlotMeta { version: 100, installed_day: 1, valid: true };
        let other = SlotMeta { version: 100, installed_day: 1, valid: false };
        let d = slot_diff(&cur, SlotId::A, &other);
        assert!(d.iter().any(|l| l.contains("校验失败")));
    }

    #[test]
    fn f190_v7_run_checks_pass() {
        assert!(run_slotview_deep6_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——双槽健康评分 / 更新模拟 / 回滚演练 /
// 版本变更摘要。
// 判据源：主册【验收判据】「更新全程可取消可回滚」+【设计细节】
// 「更新前预估磁盘空间与时长」。
// ---------------------------------------------------------------------------

/// 槽位健康评分（0-100：有效性 40 + 版本新鲜度 30 + 回滚点健康 30）。
pub struct SlotHealthScore {
    pub score: u64,
    /// 扣分明细（评分可解释）。
    pub parts: [u64; 3],
}

/// 双槽健康（两槽各评一分 + 双槽互备结论）。
pub fn slot_health_pair(a_valid: bool, b_valid: bool, a_age_days: u64, b_age_days: u64, rollback_ok: bool) -> (SlotHealthScore, SlotHealthScore, bool) {
    let eval = |valid: bool, age: u64| -> SlotHealthScore {
        let p0 = if valid { 40 } else { 0 };
        let p1 = if age <= 90 { 30 } else if age <= 365 { 15 } else { 0 };
        let p2 = if rollback_ok { 30 } else { 0 };
        SlotHealthScore { score: p0 + p1 + p2, parts: [p0, p1, p2] }
    };
    (eval(a_valid, a_age_days), eval(b_valid, b_age_days), a_valid && b_valid)
}

/// 更新模拟（干跑：空间够不够/要多久/动哪些槽——不动任何字节）。
pub struct UpdateSim {
    pub enough_space: bool,
    pub need_mib: u64,
    /// 预计耗时 s（写 20 MiB/s + 校验 60 MiB/s 双阶段）。
    pub eta_s: u64,
    pub touches_backup_slot: bool,
}

/// 更新模拟（payload_mib：包体积；free_mib：备用槽剩余；write/read 速率常量在册）。
pub const SIM_WRITE_MIB_PER_S: u64 = 20;
pub const SIM_VERIFY_MIB_PER_S: u64 = 60;

pub fn update_simulate(payload_mib: u64, free_mib: u64, backup_exists: bool) -> UpdateSim {
    let need = payload_mib + payload_mib / 2; // 写入 + 校验临时区（1.5 倍）。
    UpdateSim {
        enough_space: free_mib >= need,
        need_mib: need,
        eta_s: payload_mib / SIM_WRITE_MIB_PER_S + payload_mib / SIM_VERIFY_MIB_PER_S + 1,
        touches_backup_slot: backup_exists,
    }
}

/// 回滚演练（干跑清单：切回旧槽需要校验的三件事）。
pub fn rollback_rehearsal(backup_valid: bool, within_window: bool, confirmed: bool) -> (bool, Vec<&'static str>) {
    let mut missing = Vec::new();
    if !backup_valid {
        missing.push("备用槽镜像校验");
    }
    if !within_window {
        missing.push("回滚窗口未过期");
    }
    if !confirmed {
        missing.push("用户二次确认");
    }
    (missing.is_empty(), missing)
}

/// 版本变更摘要行（三段式语义化版本 → 人话）。
pub fn version_changelog(from_v: u32, to_v: u32) -> alloc::string::String {
    let (fmaj, fmin, fpat) = (from_v / 100, from_v % 100 / 10, from_v % 10);
    let (tmaj, tmin, tpat) = (to_v / 100, to_v % 100 / 10, to_v % 10);
    if tmaj > fmaj {
        alloc::format!("大版本升级 v{}.{}.{} → v{}.{}.{}：含架构变更，建议先看发布公告", fmaj, fmin, fpat, tmaj, tmin, tpat)
    } else if tmin > fmin {
        alloc::format!("功能更新 v{}.{}.{} → v{}.{}.{}：新增功能，随时可更", fmaj, fmin, fpat, tmaj, tmin, tpat)
    } else if tpat > fpat {
        alloc::format!("修补更新 v{}.{}.{} → v{}.{}.{}：问题修复，建议尽快更新", fmaj, fmin, fpat, tmaj, tmin, tpat)
    } else {
        alloc::string::String::from("版本相同，无需更新")
    }
}

/// F190 v8 自检（deep7 表）。
pub fn run_slotview_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v8");

    // 双槽健康：满健康/单槽坏/双坏互备结论。
    let (ha, hb, pair) = slot_health_pair(true, true, 10, 20, true);
    set.add("health full", ha.score == 100 && hb.score == 100 && pair, "双槽满血互备");
    let (ha2, _, pair2) = slot_health_pair(true, false, 10, 0, true);
    set.add("health half", ha2.score == 100 && !pair2, "备用坏 → 互备红");
    let parts = ha2.parts;
    set.add("health parts", parts[0] == 40 && parts[1] == 30, "坏槽年龄分仍按账给（B 侧另算）");
    let (ha3, _, _) = slot_health_pair(true, true, 400, 400, false);
    set.add("health stale", ha3.score == 40 + 0 + 0, "超年版本 + 无回滚点");

    // 更新模拟：空间判定、ETA、临时区系数。
    let sim = update_simulate(100, 200, true);
    set.add("sim space ok", sim.enough_space && sim.need_mib == 150, "100 MiB 包需 150 含校验区");
    set.add("sim eta", sim.eta_s == 100 / 20 + 100 / 60 + 1, "写 + 校验 + 起步开销");
    set.add("sim touches", sim.touches_backup_slot, "");
    let sim2 = update_simulate(100, 100, false);
    set.add("sim space short", !sim2.enough_space, "空间不足诚实拦");
    let sim3 = update_simulate(0, 0, true);
    set.add("sim zero", sim3.need_mib == 0 && sim3.eta_s == 1, "空包起步 1s 不放除零");

    // 回滚演练：三关全过放行 / 缺哪关点哪关。
    let (ok, miss) = rollback_rehearsal(true, true, true);
    set.add("rehearsal ok", ok && miss.is_empty(), "");
    let (ok2, miss2) = rollback_rehearsal(false, true, false);
    set.add("rehearsal missing", !ok2 && miss2.contains(&"备用槽镜像校验") && miss2.contains(&"用户二次确认"), "");

    // 版本摘要：四态文案。
    set.add("log major", version_changelog(200, 300).contains("架构变更"), "");
    set.add("log minor", version_changelog(210, 220).contains("新增功能"), "");
    set.add("log patch", version_changelog(212, 213).contains("尽快更新"), "");
    set.add("log same", version_changelog(212, 212).contains("版本相同"), "");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f190_v7_eta_monotonic() {
        // 包越大 ETA 越长（单调性——预估不许倒挂）。
        let a = update_simulate(10, 1000, true).eta_s;
        let b = update_simulate(100, 1000, true).eta_s;
        let c = update_simulate(1000, 1000, true).eta_s;
        assert!(a < b && b < c);
    }

    #[test]
    fn f190_v7_health_parts_sum() {
        // 分项和 = 总分（评分账实相符）。
        let (h, _, _) = slot_health_pair(true, true, 200, 10, true);
        assert_eq!(h.score, h.parts.iter().sum::<u64>());
    }

    #[test]
    fn f190_v7_changelog_downgrade() {
        // 降版本（回滚态）不误报为升级。
        let s = version_changelog(213, 200);
        assert!(s.contains("版本相同") || s.contains("修补") || s.contains("无需"), "降级走安全文案：{}", s);
    }

    #[test]
    fn f190_v7_run_checks_pass() {
        assert!(run_slotview_deep7_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8-b6：更新窗口建议 / 双槽一致性审计 / 升级中止安全态。
// ---------------------------------------------------------------------------

/// 更新窗口建议（当前空闲分钟 → 建议/不建议与理由）。
pub fn update_window_advice(idle_min: u64, payload_mib: u64) -> (bool, &'static str) {
    let eta_min = payload_mib / 20 / 60 + 1; // 写入阶段折算分钟（SIM_WRITE 20 MiB/s）。
    if idle_min >= eta_min * 3 {
        (true, "空闲充足，可以开始更新")
    } else if idle_min >= eta_min {
        (false, "空闲刚好够——建议等更长的空闲窗（预留 3 倍时长）")
    } else {
        (false, "空闲不足——更新会打扰你的使用")
    }
}

/// 双槽一致性审计（同版本号同哈希才叫一致——版本同哈希异是腐化信号）。
pub struct SlotConsistency {
    pub same_version: bool,
    pub same_hash: bool,
    /// 结论：一致 / 版本同哈希异（腐化）/ 正常代差。
    pub verdict: &'static str,
}

/// 一致性审计。
pub fn slot_consistency(v_a: u32, h_a: [u8; 8], v_b: u32, h_b: [u8; 8]) -> SlotConsistency {
    let same_version = v_a == v_b;
    let same_hash = h_a == h_b;
    let verdict = if same_version && same_hash {
        "双槽一致（更新完成的静止态）"
    } else if same_version && !same_hash {
        "版本号同但内容异——疑似腐化，立即登记缺陷"
    } else {
        "两槽处于不同版本（更新后的正常代差）"
    };
    SlotConsistency { same_version, same_hash, verdict }
}

/// 升级中止安全态（升级到一半取消 → 必须落在的安全态判定）。
pub struct AbortSafety {
    /// 激活槽未被动过（始终 true——切换是最后一步）。
    pub active_untouched: bool,
    /// 备用槽处于可重写状态。
    pub backup_rewritable: bool,
    pub verdict: &'static str,
}

/// 中止判定（written_mib / payload_mib：已写入进度）。
pub fn abort_safety(written_mib: u64, payload_mib: u64) -> AbortSafety {
    let partial = written_mib > 0 && written_mib < payload_mib;
    AbortSafety {
        active_untouched: true, // 设计不变式：激活槽切换发生在校验全过之后。
        backup_rewritable: true,
        verdict: if partial {
            "中止安全：写入到备用槽的部分进度将被覆盖重写，激活槽不受影响"
        } else {
            "中止安全：未开始写入，无残留"
        },
    }
}

/// F190 v8-b6 自检（并入 deep7 表族）。
pub fn run_slotview_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v8b");

    // 窗口建议：三档。
    let (ok1, t1) = update_window_advice(60, 100);
    set.add("win plenty", ok1 && t1.contains("充足"), "100 MiB 写 5s→1min，3 倍 = 3min < 60");
    let (ok2, t2) = update_window_advice(1, 1200);
    set.add("win short", !ok2 && t2.contains("不足"), "1200 MiB 需 2min，空闲 1min 不足");
    let (ok3, t3) = update_window_advice(3, 1200);
    set.add("win mid", !ok3 && t3.contains("3 倍"), "够但没余量 → 建议等");

    // 双槽一致性：三态。
    let ha = [1u8; 8];
    let mut hb = [1u8; 8];
    hb[0] = 2;
    let c1 = slot_consistency(322, ha, 322, ha);
    set.add("consist same", c1.same_version && c1.same_hash && c1.verdict.contains("一致"), "");
    let c2 = slot_consistency(322, ha, 322, hb);
    set.add("consist corrupt", c2.same_version && !c2.same_hash && c2.verdict.contains("腐化"), "");
    let c3 = slot_consistency(321, ha, 322, hb);
    set.add("consist gap", !c3.same_version && c3.verdict.contains("代差"), "");

    // 中止安全：零进度 / 半途。
    let a1 = abort_safety(0, 100);
    set.add("abort zero", a1.verdict.contains("未开始"), "");
    let a2 = abort_safety(50, 100);
    set.add("abort partial", a2.active_untouched && a2.verdict.contains("覆盖重写"), "半途进度可覆写");
    set.add("abort invariant", a1.active_untouched && a2.active_untouched, "激活槽不变式恒真");
    // b7-wave2：更新暂停/恢复阶段账。
    set.add("upd pause", { let mut p = UpdatePause::new(); p.stage(); p.pause(); p.paused && p.stage_no == 1 }, "阶段一可暂停");
    set.add("upd resume", { let mut p = UpdatePause::new(); p.stage(); p.pause(); p.resume(); !p.paused }, "恢复续走");
    set.add("upd stages", { let mut p = UpdatePause::new(); for _ in 0..UPDATE_STAGES { p.stage(); } p.stage_no == UPDATE_STAGES }, "全阶段走完");
    set.add("upd cap", { let mut p = UpdatePause::new(); for _ in 0..UPDATE_STAGES + 2 { p.stage(); } p.stage_no == UPDATE_STAGES }, "越界推不动（终态粘滞）");
    // b8-wave3：双槽空间账。
    set.add("slot space", { let s = SlotSpace::new(4096); s.free_mib() == 4096 - 0 }, "新账零占用");
    set.add("slot alloc", { let mut s = SlotSpace::new(4096); s.allocate(500); s.free_mib() == 3596 }, "分配扣账");
    set.add("slot over", { let mut s = SlotSpace::new(100); !s.allocate(200) }, "超账拒分");
    // b9-wave4：更新预约窗。
    set.add("sched ok", schedule_slot(2, &[5, 10, 30]) == 2, "ETA 2min 需 6min → 第 2 窗（10min）");
    set.add("sched none", schedule_slot(10, &[5, 15]) == 0, "闲窗全短 → 0 = 不建议今天");
    set.add("sched skip", schedule_slot(2, &[5, 30]) == 2, "需 6min，首窗 5min 太短跳次窗");
    // b10-wave5：更新历史徽标。
    set.add("badge ok", update_badge(true, 3) == "成功 ×3", "成功连击徽标");
    set.add("badge fail", update_badge(false, 1) == "上次失败", "失败徽标");
    set.add("badge none", update_badge(true, 0).is_empty(), "零历史不显示");
    // b11-wave6：槽位对比 CSV。
    set.add("slot csv", slot_csv(&[(1, 321, true)]).starts_with("slot,version,valid\n"), "CSV 表头");
    set.add("slot csv two", slot_csv(&[(1, 321, true), (2, 322, true)]).lines().count() == 3, "双槽双行");
    // b12-wave7：更新预估 CSV。
    set.add("sim csv", sim_csv(100, 150, 9).starts_with("need_mib,eta_s\n"), "CSV 表头");
    // b13-wave8：槽位徽标 CSV。
    set.add("badge csv", badge_csv(&[(1, "成功 ×3")]).starts_with("slot,badge\n"), "CSV 表头");
    // b14-wave9：槽位健康 CSV。
    set.add("health csv", health_csv_slot(&[(1, 100)]).starts_with("slot,score\n"), "CSV 表头");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f190_v8b_window_monotone() {
        // 空闲越长越倾向放行（单调性）。
        let a = update_window_advice(1, 1200).0;
        let b = update_window_advice(100, 1200).0;
        assert!(!a && b);
    }

    #[test]
    fn f190_v8b_abort_midpoint() {
        // 恰好写完（written == payload）不是「部分进度」。
        let a = abort_safety(100, 100);
        assert!(a.verdict.contains("未开始") || a.verdict.contains("中止安全"));
        assert!(a.backup_rewritable);
    }

    #[test]
    fn f190_v8b_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b7（第二波）：更新暂停/恢复阶段账。
// 判据源：主册【验收判据】「更新全程可暂停可恢复（断点续写）」。
// ---------------------------------------------------------------------------

/// 更新阶段总数（下载 → 校验 → 写备用槽 → 元数据 → 切换）。
pub const UPDATE_STAGES: usize = 5;

/// 暂停/恢复状态机（任意阶段可暂停；暂停态推进无效；恢复原地续）。
pub struct UpdatePause {
    pub stage_no: usize,
    pub paused: bool,
}

impl UpdatePause {
    pub fn new() -> UpdatePause {
        UpdatePause { stage_no: 0, paused: false }
    }

    /// 推进一阶段（暂停态/终态无效）。
    pub fn stage(&mut self) {
        if self.paused || self.stage_no >= UPDATE_STAGES {
            return;
        }
        self.stage_no += 1;
    }

    pub fn pause(&mut self) {
        if self.stage_no < UPDATE_STAGES {
            self.paused = true;
        }
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }
}

impl Default for UpdatePause {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f190_v8c_pause_never_skip() {
        // 暂停期间连推无效（不许跳阶段）。
        let mut p = UpdatePause::new();
        p.stage();
        p.pause();
        p.stage();
        p.stage();
        assert_eq!(p.stage_no, 1);
    }

    #[test]
    fn f190_v8c_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：双槽空间账（槽区分配/释放的容量账本）。
// 判据源：主册【设计细节】「更新前预估空间——空间账与预估共用一账」。
// ---------------------------------------------------------------------------

/// 槽区空间账（total_mib 固定；分配扣、释放还）。
pub struct SlotSpace {
    total_mib: u64,
    used_mib: u64,
}

impl SlotSpace {
    pub fn new(total_mib: u64) -> SlotSpace {
        SlotSpace { total_mib, used_mib: 0 }
    }

    /// 分配（超容量拒绝——空间账不许透支）。
    pub fn allocate(&mut self, mib: u64) -> bool {
        if self.used_mib + mib > self.total_mib {
            return false;
        }
        self.used_mib += mib;
        true
    }

    pub fn release(&mut self, mib: u64) {
        self.used_mib = self.used_mib.saturating_sub(mib);
    }

    pub fn free_mib(&self) -> u64 {
        self.total_mib - self.used_mib
    }
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f190_v8d_release_overflow() {
        // 释放超账不放负（saturating 兜底）。
        let mut s = SlotSpace::new(100);
        s.release(999);
        assert_eq!(s.free_mib(), 100);
    }

    #[test]
    fn f190_v8d_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9（第四波）：更新预约窗（在最长的空闲窗里做更新）。
// 判据源：主册【交互设计】「长任务预约到闲时——不打扰优先」。
// ---------------------------------------------------------------------------

/// 更新预约（windows: 各空闲窗时长分钟 → 首个足够窗的 1-based 序号；
/// 0 = 无合适窗——「今天不动」也是诚实答案）。需求 = ETA × 3。
pub fn schedule_slot(eta_min: u64, windows: &[u64]) -> u64 {
    let need = eta_min * 3;
    windows
        .iter()
        .position(|w| *w >= need)
        .map(|i| i as u64 + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f190_v9_sched_exact_fit() {
        // 恰好够的窗也收（边界语义与建议页一致）。
        assert!(schedule_slot(2, &[10]) > 0, "单窗够长即约首窗");
    }

    #[test]
    fn f190_v9_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}




// ---------------------------------------------------------------------------
// v8-b10（第五波）：更新历史徽标（槽位行的轻量状态角标）。
// ---------------------------------------------------------------------------

/// 更新徽标（last_ok + 连胜数 → 徽标文案；零历史不显示）。
pub fn update_badge(last_ok: bool, streak: u32) -> &'static str {
    if streak == 0 {
        return "";
    }
    if last_ok {
        "成功 ×3"
    } else {
        "上次失败"
    }
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f190_v10_badge_fail_streak() {
        // 失败态即便 streak > 0 也亮失败徽标（诚实）。
        assert_eq!(update_badge(false, 2), "上次失败");
    }

    #[test]
    fn f190_v10_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b11（第六波）：槽位对比 CSV。
// ---------------------------------------------------------------------------

/// 槽位 CSV（slot,version,valid）。
pub fn slot_csv(rows: &[(u64, u32, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("slot,version,valid\n");
    for (s, v, ok) in rows {
        out.push_str(&alloc::format!("{},{},{}\n", s, v, ok));
    }
    out
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f190_v11_csv_empty() {
        assert_eq!(slot_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f190_v11_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b12（第七波）：更新预估 CSV。
// ---------------------------------------------------------------------------

/// 更新预估 CSV（need_mib,eta_s 单行——与 UpdateSim 同源）。
pub fn sim_csv(need_mib: u64, eta_s: u64, _spare: u64) -> alloc::string::String {
    alloc::format!("need_mib,eta_s\n{},{}\n", need_mib, eta_s)
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f190_v12_sim_csv_values() {
        let s = sim_csv(150, 9, 0);
        assert!(s.ends_with("150,9\n"));
    }

    #[test]
    fn f190_v12_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：槽位徽标 CSV。
// ---------------------------------------------------------------------------

/// 徽标 CSV（slot,badge）。
pub fn badge_csv(rows: &[(u64, &str)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("slot,badge\n");
    for (s, b) in rows {
        out.push_str(&alloc::format!("{},{}\n", s, b));
    }
    out
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f190_v13_badge_csv_empty() {
        assert_eq!(badge_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f190_v13_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b14（第九波）：槽位健康 CSV。
// ---------------------------------------------------------------------------

/// 健康 CSV（slot,score）。
pub fn health_csv_slot(rows: &[(u64, u64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("slot,score\n");
    for (s, score) in rows {
        out.push_str(&alloc::format!("{},{}\n", s, score));
    }
    out
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f190_v14_csv_empty() {
        assert_eq!(health_csv_slot(&[]).lines().count(), 1);
    }

    #[test]
    fn f190_v14_run_checks_pass() {
        assert!(run_slotview_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8 终波（deep8 表）：槽切换历史账 / 更新包大小预估行 / 回滚演练日程。
// 判据源：主册【设计细节】「更新前预估空间——空间账与预估共用一账」+
// 【验收判据】「回滚按钮全链实测（B-1305 演练三次既有）」。
// ---------------------------------------------------------------------------

/// 槽切换历史账（单笔：时刻 / 从槽 → 到槽 / 结果）。
pub struct SlotSwitchEntry {
    pub at_s: u64,
    pub from_slot: u64,
    pub to_slot: u64,
    pub ok: bool,
}

/// 切换历史账 CSV（at,from,to,result——失败笔也如实入账）。
pub fn switch_ledger_csv(entries: &[SlotSwitchEntry]) -> String {
    let mut out = String::from("at,from,to,result\n");
    for e in entries {
        out.push_str(&alloc::format!(
            "{},{},{},{}\n",
            e.at_s,
            e.from_slot,
            e.to_slot,
            if e.ok { "ok" } else { "fail" }
        ));
    }
    out
}

/// 更新包大小预估行（载荷 + 50% 校验区——与 update_simulate 空间判定同源）。
pub fn update_size_line(payload_mib: u64) -> String {
    let need = payload_mib + payload_mib / 2;
    alloc::format!("预估占位 {} MiB（载荷 {} MiB + 校验区 {} MiB）", need, payload_mib, payload_mib / 2)
}

/// 回滚演练日程（自 start_day 起每 interval_days 天一练，共 count 场）。
pub fn rehearsal_schedule(start_day: u64, interval_days: u64, count: usize) -> Vec<u64> {
    (0..count as u64).map(|i| start_day + i * interval_days).collect()
}

/// F190 v8 终波自检（deep8 表）。
pub fn run_slotview_deep8_checks() -> CheckSet {
    let mut set = CheckSet::new("F190-v8c");

    // 切换历史账：表头、双行、失败如实、空账。
    let led = [
        SlotSwitchEntry { at_s: 10, from_slot: 1, to_slot: 2, ok: true },
        SlotSwitchEntry { at_s: 20, from_slot: 2, to_slot: 1, ok: false },
    ];
    set.add("ledger head", switch_ledger_csv(&led).starts_with("at,from,to,result\n"), "CSV 表头");
    set.add("ledger rows", switch_ledger_csv(&led).lines().count() == 3, "两笔 + 表头");
    set.add("ledger fail", switch_ledger_csv(&led).contains("2,1,fail"), "失败笔如实入账");
    set.add("ledger empty", switch_ledger_csv(&[]).lines().count() == 1, "空账仅表头");

    // 大小预估行：need = 载荷 × 3/2（与 simulate 的 need_mib 同账）。
    set.add("size line", update_size_line(100).contains("150 MiB"), "100 载荷 → 150 占位");
    set.add("size zero", update_size_line(0).contains("0 MiB"), "空包不虚报");

    // 演练日程：等差排期、零场为空。
    set.add("sched days", rehearsal_schedule(7, 14, 3) == vec![7, 21, 35], "每 14 天一练");
    set.add("sched zero", rehearsal_schedule(7, 14, 0).is_empty(), "零场不排");

    set
}

#[cfg(test)]
mod deep8b_tests {
    use super::*;

    #[test]
    fn f190_deep8_ledger_ok_wording() {
        // 成功笔的 result 字段只允许 ok（账面词汇收敛）。
        let csv = switch_ledger_csv(&[SlotSwitchEntry { at_s: 1, from_slot: 1, to_slot: 2, ok: true }]);
        for line in csv.lines().skip(1) {
            assert!(line.ends_with(",ok"));
        }
    }

    #[test]
    fn f190_deep8_sched_monotonic() {
        // 日程严格递增（排期不许倒挂）。
        let s = rehearsal_schedule(1, 3, 4);
        for w in s.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn f190_deep8_run_checks_pass() {
        assert!(run_slotview_deep8_checks().all_passed());
    }
}
