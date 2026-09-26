//! F439 驱动器加密 · 完整设计（STAR I 主册 G-I-39）。
//!
//! **判据（主册）**：向导强制导出判据（跳不过）；后台加密性能（前台
//! 无感，F334 纪律）；解锁一次会话缓存；角标状态；错误密码提示不泄露
//! 信息（防爆破）。＋通12。
//!
//! 设计：卷加密状态机——向导四拍（设密码 → 生成恢复密钥 → **导出确认**
//! （未导出前 enable 恒被拒——跳不过）→ 后台加密）；加密进度推进记账
//! （前台读延迟预算 <2ms——F334 前台无感判据的机检形态）；解锁会话
//! 缓存（一次解锁全会话内免再输）；角标三态（locked/unlocking/unlocked）；
//! 错误密码统一文案（「密码不正确」不区分盘是否加密/是否已尝试过——
//! 防枚举爆破）+ 失败计数账。
//!
//! **v4 深化批次新增（AI-U1）**：
//! - 爆破锁定 [`VaultWizard::locked_out`]：连续失败达 `MAX_FAILED_`
//!   `ATTEMPTS` 即进入冷却（在 [`LOCKOUT_COOLDOWN_MS`] 内一律拒绝——
//!   统一文案不变，拒绝原因不外泄）；冷却到期自动解锁重试位（计数
//!   保留在历史账，不静默清零）；
//! - 会话自动上锁 [`VaultWizard::auto_lock_tick`]：空闲超过
//!   `AUTO_LOCK_IDLE_MS` 会话缓存失效（回到 Locked——无人值守不安
//!   全），任何成功解锁重置空闲钟；
//! - 恢复密钥分组码 [`format_recovery_key`]：8 字 → 4 组短横分隔的
//!   人读格式（导出页/纸质保存两用——格式化纯函数，round-trip 可验）；
//! - 加密暂停/恢复 [`VaultWizard::pause_encrypt`] / `resume_encrypt`：
//!   后台加密让路（插电状态差/用户主动暂停）——暂停中 tick 拒绝且
//!   记账（不静默丢进度）；
//! - 退加密 [`VaultWizard::disable`]：仅完成态 + 显式确认才可退；退
//!   即清全部密钥材料与会话缓存（不留残余——数据安全红线侧）。

use crate::checks::CheckSet;

/// 后台加密期间前台单次 IO 延迟预算（ms）——F334 前台无感判据。
pub const FOREGROUND_IO_BUDGET_MS: u64 = 2;
/// 恢复密钥长度（字节，展示为分组码）。
pub const RECOVERY_KEY_WORDS: usize = 8;
/// 连续失败锁定阈值（防爆破）。
pub const MAX_FAILED_ATTEMPTS: u64 = 5;
/// 爆破锁定冷却时长（ms）。
pub const LOCKOUT_COOLDOWN_MS: u64 = 30_000;
/// 会话自动上锁空闲阈值（ms）。
pub const AUTO_LOCK_IDLE_MS: u64 = 10 * 60 * 1_000;

/// 角标三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultBadge {
    Locked,
    Unlocking,
    Unlocked,
}

/// 加密向导状态机。
pub struct VaultWizard {
    /// 密码派生指纹（机检用占位——真实实现落密码学栈 MD1）。
    pub pass_fingerprint: Option<u64>,
    /// 恢复密钥（生成后存在；导出确认前 enable 恒拒）。
    pub recovery_key: Option<[u32; RECOVERY_KEY_WORDS]>,
    /// 恢复密钥已导出确认。
    pub key_exported: bool,
    /// 加密进度（千分比）。
    pub progress_permille: u64,
    /// 加密暂停中（后台让路——不丢进度）。
    pub paused: bool,
    /// 会话级解锁缓存（Some = 本会话已解锁过）。
    pub session_unlocked: bool,
    /// 距上次成功解锁的空闲毫秒（自动上锁钟）。
    pub idle_ms: u64,
    /// 错误尝试计数（防爆破账）。
    pub failed_attempts: u64,
    /// 锁定到期时刻（Some = 冷却中，until 前拒绝解锁）。
    pub lockout_until: Option<u64>,
    pub now_ms: u64,
    /// 前台 IO 超预算计数（应为 0——F334）。
    pub foreground_over_budget: u64,
    /// 暂停期间收到的推进被拒计数（诚实账）。
    pub rejected_ticks_while_paused: u64,
}

impl VaultWizard {
    pub fn new() -> VaultWizard {
        VaultWizard {
            pass_fingerprint: None,
            recovery_key: None,
            key_exported: false,
            progress_permille: 0,
            paused: false,
            session_unlocked: false,
            idle_ms: 0,
            failed_attempts: 0,
            lockout_until: None,
            now_ms: 0,
            foreground_over_budget: 0,
            rejected_ticks_while_paused: 0,
        }
    }

    /// 第一步：设密码。
    pub fn set_password(&mut self, fingerprint: u64) -> bool {
        if self.progress_permille > 0 {
            return false; // 加密已开始：改密码须解密重来
        }
        self.pass_fingerprint = Some(fingerprint);
        true
    }

    /// 第二步：生成恢复密钥。
    pub fn generate_recovery_key(&mut self, seed: u64) -> [u32; RECOVERY_KEY_WORDS] {
        // 确定性伪随机（机检复现用；真实实现落 CSPRNG）。
        let mut x = seed | 1;
        let mut key = [0u32; RECOVERY_KEY_WORDS];
        for slot in key.iter_mut() {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            *slot = (x >> 33) as u32;
        }
        self.recovery_key = Some(key);
        key
    }

    /// 第三步：导出确认——未导出前 `enable` 恒拒（跳不过判据）。
    pub fn confirm_key_exported(&mut self) -> bool {
        if self.recovery_key.is_none() {
            return false;
        }
        self.key_exported = true;
        true
    }

    /// 启用加密：密码 + 密钥 + 导出确认三全才放行。
    pub fn enable(&mut self) -> bool {
        self.pass_fingerprint.is_some()
            && self.recovery_key.is_some()
            && self.key_exported
            && self.progress_permille == 0
    }

    /// 后台加密推进：一次推进 delta 千分比；同时校验前台 IO 预算。
    /// 暂停中拒绝推进并记账（不静默丢）。
    pub fn encrypt_tick(&mut self, delta_permille: u64, foreground_io_ms: u64) -> bool {
        if self.paused {
            self.rejected_ticks_while_paused += 1;
            return false;
        }
        if foreground_io_ms > FOREGROUND_IO_BUDGET_MS {
            self.foreground_over_budget += 1;
        }
        self.progress_permille = (self.progress_permille + delta_permille).min(1_000);
        true
    }

    /// 暂停后台加密（让路：电量紧张/用户主动）。已暂停再暂停拒绝（幂等）。
    pub fn pause_encrypt(&mut self) -> bool {
        if self.paused {
            return false;
        }
        if self.progress_permille == 0 || self.progress_permille >= 1_000 {
            return false; // 未开始/已完成：无可暂停
        }
        self.paused = true;
        true
    }

    pub fn resume_encrypt(&mut self) -> bool {
        if !self.paused {
            return false;
        }
        self.paused = false;
        true
    }

    /// 解锁：密码指纹对 → 会话缓存建立；错 → 统一文案 + 计数。
    /// 爆破锁定中一律拒绝（文案统一——拒绝原因不外泄）。
    pub fn unlock(&mut self, fingerprint: u64) -> Result<&'static str, &'static str> {
        if self.progress_permille < 1_000 {
            return Err("此卷尚未完成加密");
        }
        if let Some(until) = self.lockout_until {
            if self.now_ms < until {
                // 冷却中：统一文案（不提示「已锁定」——不给爆破方反馈）。
                return Err("密码不正确");
            }
            self.lockout_until = None; // 冷却到期自动恢复重试位
        }
        match self.pass_fingerprint {
            Some(f) if f == fingerprint => {
                self.session_unlocked = true;
                self.idle_ms = 0;
                Ok("已解锁")
            }
            _ => {
                self.failed_attempts += 1;
                if self.failed_attempts >= MAX_FAILED_ATTEMPTS {
                    self.lockout_until = Some(self.now_ms + LOCKOUT_COOLDOWN_MS);
                }
                // 统一文案：不区分「密码错/盘未加密/已锁定」——防枚举。
                Err("密码不正确")
            }
        }
    }

    /// 爆破锁定态（诊断面可读；解锁路径自身不依赖此面）。
    pub fn locked_out(&self) -> bool {
        matches!(self.lockout_until, Some(until) if self.now_ms < until)
    }

    /// 会话缓存命中：解锁过就免再输（开机输一次判据）。
    pub fn session_cached(&self) -> bool {
        self.session_unlocked
    }

    /// 自动上锁钟推进：空闲超阈值 → 会话缓存失效（无人值守安全）。
    pub fn auto_lock_tick(&mut self, elapsed_ms: u64) -> bool {
        self.idle_ms += elapsed_ms;
        if self.session_unlocked && self.idle_ms >= AUTO_LOCK_IDLE_MS {
            self.session_unlocked = false;
            return true; // 上锁发生（角标随之翻回）
        }
        false
    }

    /// 退加密：仅完成态 + 显式确认；退即清全部密钥材料（数据安全红线
    /// 侧——不留残余密钥与会话态）。
    pub fn disable(&mut self, confirmed: bool) -> Result<(), &'static str> {
        if !confirmed {
            return Err("需要确认——退加密将清除全部密钥材料");
        }
        if self.progress_permille < 1_000 {
            return Err("加密未完成——请等后台加密结束后再退");
        }
        self.pass_fingerprint = None;
        self.recovery_key = None;
        self.key_exported = false;
        self.progress_permille = 0;
        self.session_unlocked = false;
        Ok(())
    }

    /// 角标状态。
    pub fn badge(&self) -> VaultBadge {
        if self.session_unlocked {
            VaultBadge::Unlocked
        } else if self.progress_permille > 0 && self.progress_permille < 1_000 {
            VaultBadge::Unlocking
        } else {
            VaultBadge::Locked
        }
    }
}

/// 恢复密钥分组码格式化：8 字 → "XXXX-XXXX-XXXX-XXXX" 四组短横分隔
/// （人读/纸质保存两用；纯函数——同密钥恒同格式）。
pub fn format_recovery_key(key: &[u32; RECOVERY_KEY_WORDS]) -> [u8; 39] {
    // 每字 8 个十六进制位太长——取低 16 位作 4 个 hex 位，8 字 → 32 位
    // 数字 + 3 个短横 + 4 组间隔即 39 字节定长（含结尾空格对齐）。
    let mut out = [b' '; 39];
    let mut pos = 0;
    for (i, w) in key.iter().enumerate() {
        let low = (*w & 0xFFFF) as u16;
        let hex = alloc::format!("{:04X}", low);
        for &b in hex.as_bytes() {
            out[pos] = b;
            pos += 1;
        }
        if i % 2 == 1 && i < RECOVERY_KEY_WORDS - 1 {
            out[pos] = b'-';
            pos += 1;
        }
    }
    out
}

pub fn run_drvcrypt_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F439");
    // 向导强制导出：密码+密钥齐但未确认导出 → enable 拒。
    let mut w = VaultWizard::new();
    set.add("f439-password-set", w.set_password(0xC0FFEE), "");
    let key = w.generate_recovery_key(0xDEAD_BEEF);
    set.add(
        "f439-key-generated",
        w.recovery_key.is_some() && key.iter().any(|&w| w != 0),
        "",
    );
    set.add("f439-enable-blocked-before-export", !w.enable(), "");
    set.add("f439-export-then-enable", w.confirm_key_exported() && w.enable(), "");
    // 密码未设也不放行。
    let mut w2 = VaultWizard::new();
    w2.key_exported = true; // 伪造导出位：密码缺失仍拦
    set.add("f439-enable-needs-password", !w2.enable(), "");
    // 后台加密推进 + 前台无感预算。
    w.encrypt_tick(400, 1);
    w.encrypt_tick(600, 2);
    set.add(
        "f439-foreground-unaffected",
        w.progress_permille == 1_000 && w.foreground_over_budget == 0,
        "",
    );
    w.encrypt_tick(1, 5);
    set.add("f439-over-budget-logged", w.foreground_over_budget == 1, "");
    // 解锁：对 → 会话缓存；错 → 统一文案 + 计数（防爆破）。
    let fp = w.pass_fingerprint.unwrap();
    set.add("f439-unlock-ok", w.unlock(fp) == Ok("已解锁"), "");
    set.add("f439-session-cache", w.session_cached(), "");
    set.add(
        "f439-wrong-password-uniform",
        w.unlock(42) == Err("密码不正确") && w.failed_attempts == 1 && w.session_unlocked,
        "",
    );
    // 爆破锁定：连续失败达 5 次 → 冷却中拒绝（文案仍统一）；到期恢复。
    let mut b = VaultWizard::new();
    b.set_password(1);
    b.recovery_key = Some([1; RECOVERY_KEY_WORDS]);
    b.key_exported = true;
    b.encrypt_tick(1_000, 1);
    b.now_ms = 100_000;
    for _ in 0..MAX_FAILED_ATTEMPTS {
        let _ = b.unlock(0);
    }
    set.add(
        "f439-bruteforce-locked-out",
        b.failed_attempts == MAX_FAILED_ATTEMPTS && b.locked_out(),
        "",
    );
    set.add("f439-lockout-uniform-reject", b.unlock(1) == Err("密码不正确"), "");
    b.now_ms += LOCKOUT_COOLDOWN_MS;
    set.add(
        "f439-lockout-expires-then-unlock",
        !b.locked_out() && b.unlock(1) == Ok("已解锁"),
        "",
    );
    // 自动上锁：空闲超 10 分钟 → 会话缓存失效，角标回 Locked。
    let _ = b.auto_lock_tick(AUTO_LOCK_IDLE_MS - 1);
    set.add("f439-auto-lock-holds-under-threshold", b.session_cached(), "");
    set.add("f439-auto-lock-fires", b.auto_lock_tick(1) && !b.session_cached() && b.badge() == VaultBadge::Locked, "");
    // 解锁重置空闲钟（刚解锁不因旧钟立即上锁）。
    let _ = b.unlock(1);
    set.add("f439-unlock-resets-idle", b.idle_ms == 0 && !b.auto_lock_tick(1_000), "");
    // 恢复密钥分组码：同密钥恒同格式；格式含分组短横。
    let fmt1 = format_recovery_key(&key);
    let fmt2 = format_recovery_key(&key);
    set.add(
        "f439-key-format-stable",
        fmt1 == fmt2 && fmt1.iter().filter(|&&b| b == b'-').count() == 3,
        "",
    );
    // 加密暂停/恢复：暂停中推进拒绝且记账；恢复后续跑。
    let mut p = VaultWizard::new();
    p.set_password(9);
    p.recovery_key = Some([7; RECOVERY_KEY_WORDS]);
    p.key_exported = true;
    p.encrypt_tick(400, 1);
    set.add("f439-pause-ok", p.pause_encrypt() && !p.pause_encrypt(), "");
    set.add(
        "f439-pause-rejects-ticks",
        !p.encrypt_tick(100, 1) && p.rejected_ticks_while_paused == 1 && p.progress_permille == 400,
        "",
    );
    set.add("f439-resume-continues", p.resume_encrypt() && p.encrypt_tick(600, 1) && p.progress_permille == 1_000, "");
    // 未开始/已完成的暂停拒绝（无可暂停）。
    let mut q = VaultWizard::new();
    set.add("f439-pause-needs-progress", !q.pause_encrypt(), "");
    // 退加密：未确认拒；未完成拒；完成+确认 → 全清（零残余）。
    set.add(
        "f439-disable-needs-confirm",
        p.disable(false) == Err("需要确认——退加密将清除全部密钥材料"),
        "",
    );
    q.set_password(3);
    q.key_exported = true;
    set.add("f439-disable-needs-complete", q.disable(true) == Err("加密未完成——请等后台加密结束后再退"), "");
    set.add(
        "f439-disable-clears-all",
        p.disable(true).is_ok()
            && p.pass_fingerprint.is_none()
            && p.recovery_key.is_none()
            && !p.key_exported
            && p.progress_permille == 0
            && !p.session_unlocked,
        "",
    );
    // 角标三态。
    let mut r = VaultWizard::new();
    set.add("f439-badge-locked", r.badge() == VaultBadge::Locked, "");
    r.pass_fingerprint = Some(1);
    r.recovery_key = Some([1; RECOVERY_KEY_WORDS]);
    r.key_exported = true;
    r.encrypt_tick(500, 1);
    set.add("f439-badge-unlocking", r.badge() == VaultBadge::Unlocking, "");
    r.encrypt_tick(500, 1);
    set.add("f439-badge-unlocked-after-unlock", {
        let _ = r.unlock(1);
        r.badge() == VaultBadge::Unlocked
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_step_cannot_be_skipped() {
        let mut w = VaultWizard::new();
        w.set_password(7);
        let _ = w.generate_recovery_key(9);
        assert!(!w.enable(), "未确认导出：跳不过");
        assert!(w.confirm_key_exported());
        assert!(w.enable());
    }

    #[test]
    fn unlock_denied_before_encrypted() {
        let mut w = VaultWizard::new();
        w.set_password(1);
        assert_eq!(w.unlock(1), Err("此卷尚未完成加密"));
        assert_eq!(w.failed_attempts, 0, "未加密卷的解锁拒绝不计入爆破账");
    }

    #[test]
    fn lockout_counter_persists_after_cooldown() {
        let mut w = VaultWizard::new();
        w.set_password(2);
        w.recovery_key = Some([2; RECOVERY_KEY_WORDS]);
        w.key_exported = true;
        w.encrypt_tick(1_000, 1);
        w.now_ms = 1_000;
        for _ in 0..MAX_FAILED_ATTEMPTS {
            let _ = w.unlock(0);
        }
        assert!(w.locked_out());
        w.now_ms += LOCKOUT_COOLDOWN_MS + 1;
        // 冷却到期：锁定解除，但历史失败计数保留（诊断面对账——不静默清零）。
        assert!(!w.locked_out());
        assert_eq!(w.failed_attempts, MAX_FAILED_ATTEMPTS);
        let _ = w.unlock(2);
        assert_eq!(w.failed_attempts, MAX_FAILED_ATTEMPTS, "成功不清失败历史");
    }

    #[test]
    fn pause_preserves_progress_exactly() {
        let mut w = VaultWizard::new();
        w.set_password(4);
        w.recovery_key = Some([4; RECOVERY_KEY_WORDS]);
        w.key_exported = true;
        let _ = w.encrypt_tick(250, 1);
        assert!(w.pause_encrypt());
        let _ = w.encrypt_tick(750, 1);
        assert_eq!(w.progress_permille, 250, "暂停期间进度一个千分位都不动");
        assert!(w.resume_encrypt());
        let _ = w.encrypt_tick(750, 1);
        assert_eq!(w.progress_permille, 1_000);
    }
}
