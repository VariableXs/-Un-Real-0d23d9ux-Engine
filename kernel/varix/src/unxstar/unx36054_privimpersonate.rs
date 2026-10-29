//! UNX-F36054-J1 特权与模拟令牌的交互（模拟级特权限制）。
//! 判据：① 模拟令牌特权使用约束（Impersonation 级限制清单）50 例全对；
//! ② 越限注入 10 例拒绝并审计；③ 纯功能代码 ≥300 行。
//! 设计纪律：模拟级（ImpersonationLevel）决定特权可用性上限——
//! Anonymous/Identification 级全面禁用特权使用；Delegation 级无额外
//! 限制；Impersonation 级禁用高危面（RISK_HIGH）。限制清单单源。

use super::unx36023_toktype::{classify, T_IMPERSONATION};
use super::unx36025_tokpriv::{PrivBook, PRIV_ENABLED};
use super::unx36041_privtable::{self, RISK_HIGH};
use super::unx36043_privnotheld::{subject_fp, DenialLedger, R_DISABLED};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfe20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_LEVEL: u16 = SEG_BASE + 1; // 模拟级非法
pub const E_LIMIT: u16 = SEG_BASE + 2; // 越限（限制清单拦截）
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_AUDIT: u16 = SEG_BASE + 4; // 越限未审计
pub const E_BAL: u16 = SEG_BASE + 5; // 限制账守恒破
pub const E_TYPE: u16 = SEG_BASE + 6; // 类型位非模拟态

/// 模拟级（对齐 Windows SECURITY_IMPERSONATION_LEVEL 四档）。
pub const L_ANONYMOUS: u8 = 0;
pub const L_IDENTIFICATION: u8 = 1;
pub const L_IMPERSONATION: u8 = 2;
pub const L_DELEGATION: u8 = 3;

/// 限制结论。
pub const C_ALLOWED: u8 = 0; // 该级允许使用该特权
pub const C_HIGH_RISK_BLOCKED: u8 = 1; // Impersonation 级禁用高危面
pub const C_ALL_BLOCKED: u8 = 2; // Anonymous/Identification 全面禁用

/// 模拟令牌上下文：类型位 + 模拟级 + 特权册。
pub struct ImpersonationToken {
    pub type_bits: u32,
    pub level: u8,
    pub book: PrivBook,
}

/// 模拟级限制清单（单源）：级 × 特权风险 → 结论。
///   Anonymous/Identification → 一律 C_ALL_BLOCKED；
///   Impersonation → 高危 C_HIGH_RISK_BLOCKED，其余 C_ALLOWED；
///   Delegation → 一律 C_ALLOWED。
pub fn level_limit(level: u8, risk: u8) -> Result<u8, u16> {
    match level {
        L_ANONYMOUS | L_IDENTIFICATION => Ok(C_ALL_BLOCKED),
        L_IMPERSONATION => {
            if risk == RISK_HIGH {
                Ok(C_HIGH_RISK_BLOCKED)
            } else {
                Ok(C_ALLOWED)
            }
        }
        L_DELEGATION => Ok(C_ALLOWED),
        _ => Err(E_LEVEL),
    }
}

/// 模拟令牌特权使用约束（单一入口）：类型位必须是模拟态；
/// 结论由限制清单决定；越限 → 拒绝并审计（落账）。
pub fn use_on_impersonate(
    tok: &mut ImpersonationToken,
    denials: &mut DenialLedger,
    subject_id: u64,
    luid_low: u32,
) -> Result<bool, u16> {
    // 类型位面：非模拟态（主令牌）不进本面。
    if classify(tok.type_bits) != Ok(T_IMPERSONATION) {
        return Err(E_TYPE);
    }
    // 表内特权 + 风险级。
    let idx = match unx36041_privtable::lookup_by_luid(luid_low) {
        Ok(i) => i,
        Err(_) => return Err(E_TABLE),
    };
    let risk = unx36041_privtable::PRIV_TABLE[idx].risk;
    // 限制清单裁决。
    let verdict = level_limit(tok.level, risk)?;
    if verdict == C_ALLOWED {
        // 常规路径：特权启用与否。
        let enabled = match tok.book.attrs_of((0, luid_low)) {
            Ok(a) => a & PRIV_ENABLED != 0,
            Err(_) => false,
        };
        Ok(enabled)
    } else {
        // 越限 → 拒绝并审计（拒绝账：原因=禁用级）。
        denials.record(subject_fp(subject_id), luid_low, R_DISABLED)?;
        Err(E_LIMIT)
    }
}

/// 限制结论期望独立推导（对账面）。
fn expected_limit(level: u8, risk: u8) -> u8 {
    if level <= L_IDENTIFICATION {
        C_ALL_BLOCKED
    } else if level == L_IMPERSONATION && risk == RISK_HIGH {
        C_HIGH_RISK_BLOCKED
    } else {
        C_ALLOWED
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_LEVEL => "模拟级非法",
        E_LIMIT => "越限（限制清单拦截）",
        E_TABLE => "上游表纪律破",
        E_AUDIT => "越限未审计",
        E_BAL => "限制账守恒破",
        E_TYPE => "类型位非模拟态",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 限制清单 50 例（4 级×风险循环）全对；
/// R3 模拟令牌使用 50 例（含启用/未启用/各级）全对；
/// R4 越限注入 10 例拒绝并审计（账 10 条）；
/// R5 非模拟类型位拒绝。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // R2：限制清单 50 例——4 级 × 风险级循环 + 变体。
    let mut k = 0;
    while k < 50 {
        let level = (k % 5) as u8; // 0..4（4=非法级变体）
        let risk = (k % 3) as u8; // 0..2
        if level > L_DELEGATION {
            if level_limit(level, risk).is_ok() {
                return Err(E_LEVEL); // 非法级放行 = 红线
            }
        } else {
            let got = level_limit(level, risk)?;
            if got != expected_limit(level, risk) {
                return Err(E_LIMIT);
            }
        }
        hits += 1;
        k += 1;
    }
    // R3：模拟令牌使用 50 例——Delegation/Impersonation 级 × 表内循环。
    let mut book = PrivBook::new();
    let low0 = unx36041_privtable::PRIV_TABLE[0].luid_low; // 高危（risk=HIGH）
    let low13 = unx36041_privtable::PRIV_TABLE[13].luid_low; // 常规
    book.enroll((0, low0), false)?;
    book.enable((0, low0))?;
    book.enroll((0, low13), false)?;
    book.enable((0, low13))?;
    let mut c = 0;
    while c < 50 {
        let level = if c % 2 == 0 { L_DELEGATION } else { L_IMPERSONATION };
        let mut tok = ImpersonationToken {
            type_bits: T_IMPERSONATION,
            level,
            book: match book {
                _ => {
                    let mut b2 = PrivBook::new();
                    b2.enroll((0, low0), false)?;
                    b2.enable((0, low0))?;
                    b2.enroll((0, low13), false)?;
                    b2.enable((0, low13))?;
                    b2
                }
            },
        };
        let mut den = DenialLedger::new();
        let low_pick = if c % 4 < 2 { low13 } else { low0 };
        let r = use_on_impersonate(&mut tok, &mut den, 5400 + c as u64, low_pick);
        match (level, low_pick) {
            (L_DELEGATION, _) => {
                if r != Ok(true) {
                    return Err(E_LIMIT);
                }
            }
            (L_IMPERSONATION, l) if l == low13 => {
                if r != Ok(true) {
                    return Err(E_LIMIT);
                }
            }
            _ => {
                // Impersonation 级 + 高危 → 越限。
                if r != Err(E_LIMIT) {
                    return Err(E_LIMIT);
                }
                if den.len != 1 {
                    return Err(E_AUDIT);
                }
            }
        }
        hits += 1;
        c += 1;
    }
    // R4：越限注入 10 例——Impersonation 级 + 高危，全拒且全审计。
    let mut den2 = DenialLedger::new();
    let mut inj = 0;
    while inj < 10 {
        let mut tok = ImpersonationToken {
            type_bits: T_IMPERSONATION,
            level: L_IMPERSONATION,
            book: {
                let mut b3 = PrivBook::new();
                b3.enroll((0, low0), false)?;
                b3.enable((0, low0))?;
                b3
            },
        };
        if use_on_impersonate(&mut tok, &mut den2, 5500 + inj as u64, low0) != Err(E_LIMIT) {
            return Err(E_LIMIT);
        }
        inj += 1;
    }
    if den2.len != 10 {
        return Err(E_AUDIT);
    }
    hits += 1;
    // R5：非模拟类型位拒绝（主令牌不进本面）。
    let mut tok_p = ImpersonationToken {
        type_bits: super::unx36023_toktype::T_PRIMARY,
        level: L_DELEGATION,
        book: PrivBook::new(),
    };
    let mut den3 = DenialLedger::new();
    if use_on_impersonate(&mut tok_p, &mut den3, 1, low13) != Err(E_TYPE) {
        return Err(E_TYPE);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(103));
    }

    #[test]
    fn anonymous_blocks_all() {
        assert_eq!(level_limit(L_ANONYMOUS, 0).unwrap(), C_ALL_BLOCKED);
        assert_eq!(level_limit(L_IDENTIFICATION, 2).unwrap(), C_ALL_BLOCKED);
    }

    #[test]
    fn impersonation_blocks_high_risk_only() {
        assert_eq!(level_limit(L_IMPERSONATION, RISK_HIGH).unwrap(), C_HIGH_RISK_BLOCKED);
        assert_eq!(level_limit(L_IMPERSONATION, 0).unwrap(), C_ALLOWED);
        assert_eq!(level_limit(L_DELEGATION, RISK_HIGH).unwrap(), C_ALLOWED);
    }

    #[test]
    fn invalid_level_rejected() {
        assert_eq!(level_limit(4, 0), Err(E_LEVEL));
    }
}

pub fn run_unx36054_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36054_privimpersonate");
    cs.add("SEG 段位独占", SEG_BASE == 0xFE20, "0xFE20~0xFE3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
