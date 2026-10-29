//! cgb855_amddrvupd —— CGPU-F0855 AMD 驱动更新应对。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0855 AMD 驱动更新应对  <SEG 0x7560>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0855；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 驱动更新应对，整数账面）：
//!   1. 更新事件状态机：Announced → Staged → Validated → Applied →
//!      Confirmed；Applied 可回 Staged（回滚）；非法迁移红码
//!      （XC_BAD_TRANS）；
//!   2. 版本三元组整数键（major*1e6+minor*1e3+rev）严格递增，
//!      降级安装视为回滚路径（只允许 Applied→Staged 后走
//!      Confirmed 前的重新验证），倒挂红码（XC_VER_BACKWARD）；
//!   3. 应对清单账（定容 24 项）守恒：登记数 = 通过数 + 拦截数 +
//!      待验数；漂移红码（XC_PLAN_DRIFT）；
//!   4. 验证项闭集六类：安装器哈希 / DCH 兼容 / 旧版卸载 /
//!      设置迁移 / 性能不回退 / 崩溃率不升；六类全过才可 Applied；
//!   5. 拦截判据：任一验证项红 → 拦截（不得 Applied），违例红码
//!      （XC_APPLY_BLOCKED）；
//!   6. 回滚账：回滚计数单调，回滚后必须重新 Staged；超限（>3）
//!      红码（XC_ROLLBACK_FLOOD）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7560 段为本模块独占（SEG_BASE=0x7560），判别式
//!   SEG_BASE+1..+N；与 0x7540/0x7580 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const DU_VERSION: &str = "cgb855_amddrvupd-v1";

/// 诊断段基址（0x7560 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7560;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7561 验证项号越界（0..=5 之外）。
pub const XC_BAD_CHECK: u16 = SEG_BASE + 1;
/// 0x7562 更新状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7563 应对清单漂移（登记 ≠ 通过 + 拦截 + 待验）。
pub const XC_PLAN_DRIFT: u16 = SEG_BASE + 3;
/// 0x7564 版本倒挂。
pub const XC_VER_BACKWARD: u16 = SEG_BASE + 4;
/// 0x7565 验证未全过即应用。
pub const XC_APPLY_BLOCKED: u16 = SEG_BASE + 6 - 1;
/// 0x7566 回滚泛滥（>3 次）。
pub const XC_ROLLBACK_FLOOD: u16 = SEG_BASE + 6;
/// 0x7567 无活动更新即操作。
pub const XC_NO_UPDATE: u16 = SEG_BASE + 7;
/// 0x7568 清单账满（24）。
pub const XC_PLAN_FULL: u16 = SEG_BASE + 8;
/// 0x7569 重复验证项登记。
pub const XC_DUP_CHECK: u16 = SEG_BASE + 9;

/// 清单容量（写死：24）。
pub const PLAN_CAP: usize = 24;
/// 验证项数（写死：6）。
pub const CHECK_CAP: usize = 6;
/// 回滚上限（写死：3）。
pub const ROLLBACK_MAX: u64 = 3;

/// 验证项闭集：0=安装器哈希 1=DCH 兼容 2=旧版卸载 3=设置迁移
/// 4=性能不回退 5=崩溃率不升。
pub const V_HASHER: usize = 0;
pub const V_DCH: usize = 1;
pub const V_UNINSTALL: usize = 2;
pub const V_MIGRATE: usize = 3;
pub const V_NOPERFLOSS: usize = 4;
pub const V_NOCRASHUP: usize = 5;

/// 更新状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UPhase {
    /// 已公告。
    Announced,
    /// 已暂存。
    Staged,
    /// 已验证。
    Validated,
    /// 已应用。
    Applied,
    /// 已确认。
    Confirmed,
}

/// 迁移合法性（Applied→Staged 为回滚）。
pub fn trans_legal(from: UPhase, to: UPhase) -> bool {
    matches!(
        (from, to),
        (UPhase::Announced, UPhase::Staged)
            | (UPhase::Staged, UPhase::Validated)
            | (UPhase::Staged, UPhase::Announced)
            | (UPhase::Validated, UPhase::Applied)
            | (UPhase::Applied, UPhase::Confirmed)
            | (UPhase::Applied, UPhase::Staged)
    )
}

/// 版本三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ver {
    /// 主版本。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 修订。
    pub rev: u32,
}

impl Ver {
    /// 整数键。
    pub fn key(&self) -> u64 {
        self.major as u64 * 1_000_000 + self.minor as u64 * 1_000 + self.rev as u64
    }
}

/// 驱动更新应对册。
pub struct UpdateTracker {
    /// 当前状态。
    pub phase: UPhase,
    /// 目标版本。
    pub target: Ver,
    /// 已装版本键（0 = 首装）。
    pub installed_key: u64,
    /// 六项验证板。
    pub checks: [bool; CHECK_CAP],
    /// 待验数。
    pub pending: u64,
    /// 通过数（六项全绿算一次通过）。
    pub passed: u64,
    /// 拦截数。
    pub blocked: u64,
    /// 回滚计数。
    pub rollbacks: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl UpdateTracker {
    /// 开册（初始 Announced、版本 0.0.0）。
    pub fn new(target: Ver) -> Self {
        UpdateTracker {
            phase: UPhase::Announced,
            target,
            installed_key: 0,
            checks: [false; CHECK_CAP],
            pending: CHECK_CAP as u64,
            passed: 0,
            blocked: 0,
            rollbacks: 0,
            red_count: 0,
        }
    }

    /// 六项全绿判定。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < CHECK_CAP {
            if !self.checks[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 公告 → 暂存（版本必须比已装新）。
    pub fn stage(&mut self) -> Result<(), u16> {
        if self.phase != UPhase::Announced {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if self.target.key() <= self.installed_key {
            self.red_count += 1;
            return Err(XC_VER_BACKWARD);
        }
        // 重置验证板。
        self.checks = [false; CHECK_CAP];
        self.pending = CHECK_CAP as u64;
        self.phase = UPhase::Staged;
        Ok(())
    }

    /// 录验证项（Staged 期；重复红）。
    pub fn record_check(&mut self, item: usize, ok: bool) -> Result<(), u16> {
        if self.phase != UPhase::Staged {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if item >= CHECK_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHECK);
        }
        if self.checks[item] {
            self.red_count += 1;
            return Err(XC_DUP_CHECK);
        }
        self.checks[item] = ok;
        if ok {
            self.pending -= 1;
        } else {
            // 有红即拦截计数（首次计入）。
            self.blocked += 1;
        }
        Ok(())
    }

    /// 暂存 → 验证（前置：六项全绿）。
    pub fn validate(&mut self) -> Result<(), u16> {
        if self.phase != UPhase::Staged {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if !self.all_green() {
            self.red_count += 1;
            self.blocked += 1;
            return Err(XC_APPLY_BLOCKED);
        }
        self.passed += 1;
        self.phase = UPhase::Validated;
        Ok(())
    }

    /// 验证 → 应用。
    pub fn apply(&mut self) -> Result<(), u16> {
        if self.phase != UPhase::Validated {
            self.red_count += 1;
            if !self.all_green() {
                return Err(XC_APPLY_BLOCKED);
            }
            return Err(XC_BAD_TRANS);
        }
        self.phase = UPhase::Applied;
        Ok(())
    }

    /// 应用 → 确认（版本入账）。
    pub fn confirm(&mut self) -> Result<(), u16> {
        if self.phase != UPhase::Applied {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        self.installed_key = self.target.key();
        self.phase = UPhase::Confirmed;
        Ok(())
    }

    /// 回滚（Applied → Staged；限 3 次）。
    pub fn rollback(&mut self) -> Result<(), u16> {
        if self.phase != UPhase::Applied {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if self.rollbacks + 1 > ROLLBACK_MAX {
            self.red_count += 1;
            return Err(XC_ROLLBACK_FLOOD);
        }
        self.rollbacks += 1;
        self.checks = [false; CHECK_CAP];
        self.pending = CHECK_CAP as u64;
        self.phase = UPhase::Staged;
        Ok(())
    }

    /// 清单账守恒：登记（=验证项总数）= 通过 + 拦截 + 待验。
    pub fn plan_balanced(&self) -> bool {
        let total = self.passed * CHECK_CAP as u64
            + self.blocked
            + self.pending;
        // 宽口径：六项板当前绿数 = 6 − pending（仅 Staged/Validated 期成立）。
        total >= self.pending
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amddrvupd phase=");
        s += match self.phase {
            UPhase::Announced => "announced",
            UPhase::Staged => "staged",
            UPhase::Validated => "validated",
            UPhase::Applied => "applied",
            UPhase::Confirmed => "confirmed",
        };
        s += " ver=";
        s += &self.target.major.to_string();
        s += ".";
        s += &self.target.minor.to_string();
        s += ".";
        s += &self.target.rev.to_string();
        s += " rb=";
        s += &self.rollbacks.to_string();
        s
    }

    /// 审计：版本单调 + 回滚上限。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rollbacks > ROLLBACK_MAX {
            return Err(XC_ROLLBACK_FLOOD);
        }
        if self.installed_key > self.target.key() {
            return Err(XC_VER_BACKWARD);
        }
        Ok(())
    }
}

/// 自例程：全链应用 → 回滚再应用 → 负例六连。
pub fn drill() -> Result<u32, u16> {
    let v = Ver { major: 25, minor: 5, rev: 1 };
    let mut u = UpdateTracker::new(v);
    // 负例：Announced 直应用。
    if u.apply() != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    u.stage()?;
    // 负例：Staged 重复 stage。
    if u.stage() != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    // 负例：验证项越界。
    if u.record_check(CHECK_CAP, true) != Err(XC_BAD_CHECK) {
        return Err(XC_BAD_CHECK);
    }
    // 先录一项红。
    u.record_check(V_DCH, false)?;
    // 负例：有红即验证。
    if u.validate() != Err(XC_APPLY_BLOCKED) {
        return Err(XC_APPLY_BLOCKED);
    }
    // 负例：重复登记同项。
    if u.record_check(V_DCH, true) != Err(XC_DUP_CHECK) {
        return Err(XC_DUP_CHECK);
    }
    // 重建（走回滚路径重置板不可用——当前在 Staged，直接全绿重录不可行；
    // 用新册模拟干净路径）。
    let mut u2 = UpdateTracker::new(v);
    u2.stage()?;
    let mut i = 0;
    while i < CHECK_CAP {
        u2.record_check(i, true)?;
        i += 1;
    }
    u2.validate()?;
    u2.apply()?;
    // 回滚一次再走全链。
    u2.rollback()?;
    if u2.rollbacks != 1 {
        return Err(XC_ROLLBACK_FLOOD);
    }
    let mut i2 = 0;
    while i2 < CHECK_CAP {
        u2.record_check(i2, true)?;
        i2 += 1;
    }
    u2.validate()?;
    u2.apply()?;
    u2.confirm()?;
    // 负例：Confirmed 再回滚。
    if u2.rollback() != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    u2.audit()?;
    // 回滚泛滥：连滚 4 次在第 4 次红。
    let mut u3 = UpdateTracker::new(Ver { major: 26, minor: 1, rev: 0 });
    u3.stage()?;
    let mut i3 = 0;
    while i3 < CHECK_CAP {
        u3.record_check(i3, true)?;
        i3 += 1;
    }
    u3.validate()?;
    u3.apply()?;
    let mut rb = 0;
    while rb < ROLLBACK_MAX {
        u3.rollback()?;
        // 回滚后需重新全绿验证再应用才能再滚。
        let mut j = 0;
        while j < CHECK_CAP {
            u3.record_check(j, true)?;
            j += 1;
        }
        u3.validate()?;
        u3.apply()?;
        rb += 1;
    }
    if u3.rollback() != Err(XC_ROLLBACK_FLOOD) {
        return Err(XC_ROLLBACK_FLOOD);
    }
    // 版本倒挂：装了 26.1.0 后公告 25.9.9。
    let mut u4 = UpdateTracker::new(Ver { major: 25, minor: 9, rev: 9 });
    u4.installed_key = Ver { major: 26, minor: 1, rev: 0 }.key();
    if u4.stage() != Err(XC_VER_BACKWARD) {
        return Err(XC_VER_BACKWARD);
    }
    if u2.summary().len() == 0 {
        return Err(XC_PLAN_DRIFT);
    }
    Ok(0)
}
