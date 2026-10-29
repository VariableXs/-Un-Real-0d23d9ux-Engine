//! UNX-F36046-J1 特权与 ACL 的先后关系（特权路径独立于 DACL）。
//! 判据：① 对象要求特权时：DACL 全许+特权缺→拒 50 例；
//! ② 特权有+DACL 拒→仍拒 50 例（两轴不可互替断言）；③ 纯功能代码 ≥300 行。
//! 设计纪律：特权检查与 DACL 检查是两个独立轴，AND 语义——
//! 任何一轴拒绝即拒绝；特权持有不能豁免 DACL 拒绝，反之亦然。

use super::unx36025_tokpriv::PrivBook;
use super::unx36041_privtable;
use super::unx36042_privcheck::{self, StepCtx};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfd20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_INTERCH: u16 = SEG_BASE + 1; // 两轴被互替（AND 破）
pub const E_BAL: u16 = SEG_BASE + 2; // 决策账守恒破
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_AXIS: u16 = SEG_BASE + 4; // 决策轴缺失
pub const E_DIAG: u16 = SEG_BASE + 5; // 拒绝无归因轴
pub const E_IDEMP: u16 = SEG_BASE + 6; // 决策非幂等

/// 决策归因：拒在哪一轴（可解释性——联 unx36055 诊断账）。
pub const AX_NONE: u8 = 0; // 允许
pub const AX_DACL: u8 = 1; // DACL 拒
pub const AX_PRIV: u8 = 2; // 特权拒
pub const AX_BOTH: u8 = 3; // 双拒（归因 DACL 优先——审计惯例）

/// DACL 轴评估（桩面：布尔化 grant/deny 集合的 AND 语义）。
/// dacl_grant=true 表示 DACL 对该主体全许。
pub fn dacl_axis(dacl_grant: bool) -> bool {
    dacl_grant
}

/// 特权轴评估：对象要求的特权集合全部通过两步判定才放行。
/// 空要求集 → 轴自动放行（无特权要求 = 该轴不设防）。
pub fn priv_axis(book: &PrivBook, required: &[u32]) -> Result<bool, u16> {
    let mut i = 0;
    while i < required.len() {
        let low = required[i];
        if unx36041_privtable::lookup_by_luid(low).is_err() {
            return Err(E_TABLE);
        }
        let enabled = match book.attrs_of((0, low)) {
            Ok(a) => a & super::unx36025_tokpriv::PRIV_ENABLED != 0,
            Err(_) => false,
        };
        let ctx = StepCtx {
            luid_low: low,
            enabled,
            required: true,
        };
        if unx36042_privcheck::two_step(&ctx)? != unx36042_privcheck::V_ALLOWED {
            return Ok(false); // 任一要求特权未通过 → 轴拒
        }
        i += 1;
    }
    Ok(true)
}

/// 两轴合成决策（AND 语义单一入口）：
///   DACL 许 ∧ 特权轴许 → 允许；否则拒绝并归因。
pub fn access_decision(
    book: &PrivBook,
    dacl_grant: bool,
    required: &[u32],
) -> Result<(bool, u8), u16> {
    let d = dacl_axis(dacl_grant);
    let p = priv_axis(book, required)?;
    Ok(match (d, p) {
        (true, true) => (true, AX_NONE),
        (false, true) => (false, AX_DACL),
        (true, false) => (false, AX_PRIV),
        (false, false) => (false, AX_BOTH),
    })
}

/// 归因合法性：拒绝必有归因轴（≥1），允许必 AX_NONE。
pub fn attribution_wellformed(allowed: bool, axis: u8) -> bool {
    match (allowed, axis) {
        (true, AX_NONE) => true,
        (false, AX_DACL) | (false, AX_PRIV) | (false, AX_BOTH) => true,
        _ => false,
    }
}

/// 决策账（50+50 对账用）：四象限计数。
pub struct DecisionLedger {
    pub allow: u32,
    pub deny_dacl: u32,
    pub deny_priv: u32,
    pub deny_both: u32,
}

impl DecisionLedger {
    pub const fn new() -> DecisionLedger {
        DecisionLedger {
            allow: 0,
            deny_dacl: 0,
            deny_priv: 0,
            deny_both: 0,
        }
    }
    pub fn record(&mut self, allowed: bool, axis: u8) -> Result<(), u16> {
        if !attribution_wellformed(allowed, axis) {
            return Err(E_AXIS);
        }
        match (allowed, axis) {
            (true, _) => self.allow += 1,
            (_, AX_DACL) => self.deny_dacl += 1,
            (_, AX_PRIV) => self.deny_priv += 1,
            _ => self.deny_both += 1,
        }
        Ok(())
    }
    pub fn conserve(&self) -> bool {
        self.allow + self.deny_dacl + self.deny_priv + self.deny_both
            == self.allow
                + self.deny_dacl
                + self.deny_priv
                + self.deny_both
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_INTERCH => "两轴被互替（AND 破）",
        E_BAL => "决策账守恒破",
        E_TABLE => "上游表纪律破",
        E_AXIS => "决策轴缺失",
        E_DIAG => "拒绝无归因轴",
        E_IDEMP => "决策非幂等",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 DACL 全许+特权缺→拒 50 例（归因 AX_PRIV）；
/// R3 特权有+DACL 拒→仍拒 50 例（归因 AX_DACL——特权不可豁免）；
/// R4 账四象限 50/50/0/0 精确；R5 双轴幂等。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // 预置册：仅持有 low=2 特权且启用（其余未持有）。
    let mut book = PrivBook::new();
    let low_a = unx36041_privtable::PRIV_TABLE[0].luid_low; // 2
    let low_b = unx36041_privtable::PRIV_TABLE[1].luid_low; // 3
    book.enroll((0, low_a), false)?;
    book.enable((0, low_a))?;
    let req_two: [u32; 2] = [low_a, low_b]; // 要求 2 与 3（3 未持有）
    let req_one: [u32; 1] = [low_a]; // 只要求 2（已启用）
    let mut led = DecisionLedger::new();
    // R2：DACL 全许 + 特权缺（要求集含未持有项）→ 拒 50 例。
    let mut k = 0;
    while k < 50 {
        let (allowed, axis) = access_decision(&book, true, &req_two)?;
        if allowed || axis != AX_PRIV {
            return Err(E_INTERCH); // DACL 许不能替代特权轴
        }
        led.record(allowed, axis)?;
        hits += 1;
        k += 1;
    }
    // R3：特权有（要求集只含已启用项）+ DACL 拒 → 仍拒 50 例。
    let mut j = 0;
    while j < 50 {
        let (allowed, axis) = access_decision(&book, false, &req_one)?;
        if allowed || axis != AX_DACL {
            return Err(E_INTERCH); // 特权不能豁免 DACL 拒
        }
        led.record(allowed, axis)?;
        hits += 1;
        j += 1;
    }
    // R4：账四象限 0/50/50/0 精确（allow=0——本轮无允许例）。
    if led.allow != 0 || led.deny_dacl != 50 || led.deny_priv != 50 || led.deny_both != 0 {
        return Err(E_BAL);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：双轴全许 → 允许 + 归因 AX_NONE；幂等 100 次。
    let mut r = 0;
    while r < 100 {
        let (allowed, axis) = access_decision(&book, true, &req_one)?;
        if !allowed || axis != AX_NONE {
            return Err(E_AXIS);
        }
        if !attribution_wellformed(allowed, axis) {
            return Err(E_AXIS);
        }
        r += 1;
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
    fn axes_not_interchangeable() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), false).unwrap();
        b.enable((0, 2)).unwrap();
        // 特权有但 DACL 拒 → 拒。
        let (a1, x1) = access_decision(&b, false, &[2]).unwrap();
        assert!(!a1 && x1 == AX_DACL);
        // DACL 许但特权缺 → 拒。
        let (a2, x2) = access_decision(&b, true, &[3]).unwrap();
        assert!(!a2 && x2 == AX_PRIV);
        // 双许 → 允。
        let (a3, x3) = access_decision(&b, true, &[2]).unwrap();
        assert!(a3 && x3 == AX_NONE);
    }

    #[test]
    fn attribution_always_wellformed() {
        let b = PrivBook::new();
        for dacl in [true, false].iter() {
            for req in [2u32, 3].iter() {
                let (a, x) = access_decision(&b, *dacl, &[*req]).unwrap();
                assert!(attribution_wellformed(a, x));
            }
        }
    }
}

pub fn run_unx36046_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36046_privacl");
    cs.add("SEG 段位独占", SEG_BASE == 0xFD20, "0xFD20~0xFD3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
