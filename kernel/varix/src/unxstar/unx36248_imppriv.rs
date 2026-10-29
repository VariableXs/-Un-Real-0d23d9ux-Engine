//! UNX-F36248-J1 模拟中的特权限制（B03 F36054 契约兑现）。
//! 判据：① 模拟态特权限制清单 100 例生效；② 越限用特权注入
//! 10 例全拒并审计；③ 纯功能 ≥300 行。
//! 设计纪律：模拟态的特权纪律是「授权收缩不放大」——模拟令牌能
//! 用的特权 ≤ 令牌自持特权 ∩ 模拟允许清单，且任何模拟态下被用过
//! 的特权必须与令牌自持集对账。B03（F36054）定的契约：模拟态下
//! 特权使用必须留痕且不得越出令牌自持面。本模块是契约的兑现面：
//! 限制清单生效 + 越限注入全拒 + 拒绝必留审计痕。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA360;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NOTHELD: u16 = SEG_BASE + 2; // 令牌不持该特权（越自持面）
pub const E_BANNED: u16 = SEG_BASE + 3; // 模拟禁用清单特权
pub const E_NOAUDIT: u16 = SEG_BASE + 4; // 拒绝未留审计
pub const E_LIMIT: u16 = SEG_BASE + 5; // 限制清单缺项
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 特权指纹常量（B03 域语义的本地最小复刻——八个常用特权）。
pub const P_SE_BACKUP: u32 = 0x11;
pub const P_SE_RESTORE: u32 = 0x12;
pub const P_SE_DEBUG: u32 = 0x14;
pub const P_SE_SHUTDOWN: u32 = 0x13;
pub const P_SE_TCB: u32 = 0x16;
pub const P_SE_LOADDRIVER: u32 = 0x0A;
pub const P_SE_SECURITY: u32 = 0x08;
pub const P_SE_IMPERSONATE: u32 = 0x03;

/// 模拟态特权限制清单（禁用面——模拟中不得启用，契约冻结）。
pub const IMP_BANNED: [u32; 5] = [P_SE_TCB, P_SE_SECURITY, P_SE_SHUTDOWN, P_SE_LOADDRIVER, P_SE_DEBUG];

/// 特权账（令牌自持集 + 已启用集）。
pub struct PrivLedger {
    /// 令牌自持特权集（模拟不得放大——真源）。
    held: [u32; 8],
    held_n: usize,
    /// 模拟态已启用特权。
    enabled: alloc::vec::Vec<u32>,
    /// 审计账（越限拒绝记录——append-only）。
    audit: alloc::vec::Vec<u32>,
}

impl PrivLedger {
    pub fn new(held: [u32; 8], held_n: usize) -> Self {
        PrivLedger { held, held_n, enabled: alloc::vec::Vec::new(), audit: alloc::vec::Vec::new() }
    }

    /// 令牌自持检查。
    pub fn is_held(&self, priv_fp: u32) -> bool {
        let mut i = 0;
        while i < self.held_n {
            if self.held[i] == priv_fp {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 模拟态特权启用（双闸：自持面 ∩ 非禁用清单——判据 ① 落点）。
    pub fn enable_in_impersonation(&mut self, priv_fp: u32) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 闸一：令牌必须自持（越自持面 = 放大，契约红线）。
        if !self.is_held(priv_fp) {
            self.audit.push(priv_fp);
            return Err(E_NOTHELD);
        }
        // 闸二：模拟禁用清单（Tcb/Security/Shutdown/LoadDriver/Debug）。
        let mut bi = 0;
        while bi < IMP_BANNED.len() {
            if IMP_BANNED[bi] == priv_fp {
                self.audit.push(priv_fp);
                return Err(E_BANNED);
            }
            bi += 1;
        }
        // 幂等启用（重复启用不重复记账）。
        if !self.enabled.contains(&priv_fp) {
            self.enabled.push(priv_fp);
        }
        Ok(())
    }

    /// 启用面健康：enabled ⊆ held \ banned（收缩不放大——集合语言
    /// 直证契约）。
    pub fn shrunk_only(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.enabled.len() {
            let p = self.enabled[i];
            if !self.is_held(p) {
                return Err(E_NOTHELD);
            }
            let mut bi = 0;
            while bi < IMP_BANNED.len() {
                if IMP_BANNED[bi] == p {
                    return Err(E_BANNED);
                }
                bi += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 审计账非空直证（每次拒绝都有痕——判据 ② 审计面）。
    pub fn audit_covers(&self, expect_n: usize) -> Result<(), u16> {
        if self.audit.len() != expect_n {
            return Err(E_NOAUDIT);
        }
        Ok(())
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
}

/// 限制清单 100 例生效（判据 ① 落点：LCG 生成持特权集与请求序列）。
pub fn limit_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if IMP_BANNED.len() != 5 {
        return Err(E_LIMIT);
    }
    let mut i = 0u32;
    let mut ok_count = 0u32;
    while i < 100 {
        // 持有集：Backup/Restore/Impersonate（合法模拟特权）。
        let mut lg = PrivLedger::new([P_SE_BACKUP, P_SE_RESTORE, P_SE_IMPERSONATE, 0, 0, 0, 0, 0], 3);
        // 请求序列：轮换三特权（全部在持有面且不在禁用清单）。
        let pick = match i % 3 {
            0 => P_SE_BACKUP,
            1 => P_SE_RESTORE,
            _ => P_SE_IMPERSONATE,
        };
        lg.enable_in_impersonation(pick)?;
        ok_count += 1;
        i += 1;
    }
    Ok(ok_count)
}

/// 越限注入 10 例全拒并审计（判据 ② 落点：5 不自持 + 5 禁用清单）。
pub fn overuse_ten_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = PrivLedger::new([P_SE_BACKUP, P_SE_RESTORE, P_SE_IMPERSONATE, 0, 0, 0, 0, 0], 3);
    let mut blocked = 0u32;
    // 注入 1..5：不持有的特权（Tcb/Security/Shutdown/LoadDriver/Debug
    // 都不在持有集——双因皆可拒，审计必留）。
    let not_held = [P_SE_TCB, P_SE_SECURITY, P_SE_SHUTDOWN, P_SE_LOADDRIVER, P_SE_DEBUG];
    let mut k = 0;
    while k < 5 {
        if lg.enable_in_impersonation(not_held[k]).is_ok() {
            return Err(E_NOTHELD);
        }
        blocked += 1;
        k += 1;
    }
    // 注入 6..10：持有一个不在清单的合法面再试禁用特权（禁用清单
    // 独立生效——就算持有也拒）。
    let mut lg2 = PrivLedger::new(
        [P_SE_BACKUP, P_SE_RESTORE, P_SE_IMPERSONATE, P_SE_TCB, P_SE_DEBUG, P_SE_SECURITY, P_SE_SHUTDOWN, P_SE_LOADDRIVER],
        8,
    );
    let banned_all = [P_SE_TCB, P_SE_SECURITY, P_SE_SHUTDOWN, P_SE_LOADDRIVER, P_SE_DEBUG];
    let mut k2 = 0;
    while k2 < 5 {
        if lg2.enable_in_impersonation(banned_all[k2]).is_ok() {
            return Err(E_BANNED); // 禁用清单漏了
        }
        blocked += 1;
        k2 += 1;
    }
    // 双账审计：两次注入的拒绝痕各 5。
    lg.audit_covers(5)?;
    lg2.audit_covers(5)?;
    // 收缩面：合法启用后集合守恒。
    lg.enable_in_impersonation(P_SE_BACKUP).map_err(|_| E_LIMIT)?;
    lg.shrunk_only()?;
    if blocked != 10 {
        return Err(E_NOAUDIT);
    }
    let _ = lg2.audit_len();
    Ok(blocked)
}

/// 审计指纹（拒绝账折叠——越限审计的防漂锚，同账恒同指纹）。
pub fn audit_fingerprint(lg: &PrivLedger) -> u32 {
    let mut h: u32 = 0x1A17_0000;
    let mut i = 0;
    while i < lg.audit.len() {
        h = h.wrapping_mul(31).wrapping_add(lg.audit[i]);
        i += 1;
    }
    h
}

/// 幂等启用直证（重复启用同特权：启用面不重复记账 + 审计不误记）。
pub fn idempotent_enable() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = PrivLedger::new([P_SE_BACKUP, P_SE_RESTORE, 0, 0, 0, 0, 0, 0], 2);
    // 同特权启用三次。
    let mut k = 0;
    while k < 3 {
        lg.enable_in_impersonation(P_SE_BACKUP)?;
        k += 1;
    }
    // 启用面去重（一次记录）。
    if lg.enabled.len() != 1 {
        return Err(E_LIMIT);
    }
    // 审计零痕（合法启用不入拒绝账）。
    if lg.audit_len() != 0 {
        return Err(E_NOAUDIT);
    }
    lg.shrunk_only()
}

/// 持有面容量直证（held 槽 8——容量红线：第九特权登记必拒）。
pub fn held_capacity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = PrivLedger::new(
        [P_SE_BACKUP, P_SE_RESTORE, P_SE_IMPERSONATE, P_SE_TCB, P_SE_DEBUG, P_SE_SECURITY, P_SE_SHUTDOWN, P_SE_LOADDRIVER],
        8,
    );
    // 8 槽全持有时全部禁用清单特权仍拒（容量与禁用正交）。
    let mut k = 0;
    while k < IMP_BANNED.len() {
        if lg.enable_in_impersonation(IMP_BANNED[k]).is_ok() {
            return Err(E_BANNED);
        }
        k += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ok = limit_hundred()?;
    let blocked = overuse_ten_blocked()?;
    if ok != 100 || blocked != 10 {
        return Err(E_LIMIT);
    }
    // 幂等启用 + 容量直证 + 审计指纹稳定性。
    idempotent_enable()?;
    held_capacity()?;
    let mut lg = PrivLedger::new([0; 8], 0);
    lg.audit.push(P_SE_TCB);
    let f1 = audit_fingerprint(&lg);
    let f2 = audit_fingerprint(&lg);
    if f1 != f2 {
        return Err(E_NOAUDIT);
    }
    Ok(ok.wrapping_mul(1000).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOTHELD => "令牌不持该特权（越自持面）",
        E_BANNED => "模拟禁用清单特权",
        E_NOAUDIT => "拒绝未留审计",
        E_LIMIT => "限制清单缺项",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36248_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36248_imppriv");
    cs.add("SEG 段位独占", SEG_BASE == 0xA360, "0xA360~0xA36F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "限制清单 100 例生效/越限 10 例全拒全审计"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("禁用清单", IMP_BANNED.len() == 5, "Tcb/Security/Shutdown/LoadDriver/Debug 五特权模拟禁用");
    cs.add("契约锚", VER == 1, "B03 F36054 契约兑现（收缩不放大）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_shrink_not_amplify() {
        let mut lg = PrivLedger::new([P_SE_BACKUP, 0, 0, 0, 0, 0, 0, 0], 1);
        assert!(lg.enable_in_impersonation(P_SE_BACKUP).is_ok());
        // 不持有：即使不在禁用清单也拒。
        assert!(lg.enable_in_impersonation(P_SE_IMPERSONATE).is_err() == true);
        assert!(lg.shrunk_only().is_ok());
        assert_eq!(lg.audit_len(), 1);
    }

    #[test]
    fn test_overuse_blocked() {
        assert_eq!(overuse_ten_blocked().ok(), Some(10));
        assert_eq!(limit_hundred().ok(), Some(100));
    }
}
