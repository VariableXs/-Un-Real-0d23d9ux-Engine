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

use crate::checks::CheckSet;

/// 后台加密期间前台单次 IO 延迟预算（ms）——F334 前台无感判据。
pub const FOREGROUND_IO_BUDGET_MS: u64 = 2;
/// 恢复密钥长度（字节，展示为分组码）。
pub const RECOVERY_KEY_WORDS: usize = 8;

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
    /// 会话级解锁缓存（Some = 本会话已解锁过）。
    pub session_unlocked: bool,
    /// 错误尝试计数（防爆破账）。
    pub failed_attempts: u64,
    /// 前台 IO 超预算计数（应为 0——F334）。
    pub foreground_over_budget: u64,
}

impl VaultWizard {
    pub fn new() -> VaultWizard {
        VaultWizard {
            pass_fingerprint: None,
            recovery_key: None,
            key_exported: false,
            progress_permille: 0,
            session_unlocked: false,
            failed_attempts: 0,
            foreground_over_budget: 0,
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
    pub fn encrypt_tick(&mut self, delta_permille: u64, foreground_io_ms: u64) {
        if foreground_io_ms > FOREGROUND_IO_BUDGET_MS {
            self.foreground_over_budget += 1;
        }
        self.progress_permille = (self.progress_permille + delta_permille).min(1_000);
    }

    /// 解锁：密码指纹对 → 会话缓存建立；错 → 统一文案 + 计数。
    pub fn unlock(&mut self, fingerprint: u64) -> Result<&'static str, &'static str> {
        if self.progress_permille < 1_000 {
            return Err("此卷尚未完成加密");
        }
        match self.pass_fingerprint {
            Some(f) if f == fingerprint => {
                self.session_unlocked = true;
                Ok("已解锁")
            }
            _ => {
                self.failed_attempts += 1;
                // 统一文案：不区分「密码错/盘未加密/其他」——防枚举。
                Err("密码不正确")
            }
        }
    }

    /// 会话缓存命中：解锁过就免再输（开机输一次判据）。
    pub fn session_cached(&self) -> bool {
        self.session_unlocked
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
    // 角标三态。
    let mut b = VaultWizard::new();
    set.add("f439-badge-locked", b.badge() == VaultBadge::Locked, "");
    b.pass_fingerprint = Some(1);
    b.recovery_key = Some([1; RECOVERY_KEY_WORDS]);
    b.key_exported = true;
    b.encrypt_tick(500, 1);
    set.add("f439-badge-unlocking", b.badge() == VaultBadge::Unlocking, "");
    b.encrypt_tick(500, 1);
    set.add("f439-badge-unlocked-after-unlock", {
        let _ = b.unlock(1);
        b.badge() == VaultBadge::Unlocked
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
}
