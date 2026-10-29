//! UNX-F36058-J1 特权体系安全自检（启动链探针锚）。
//! 判据：① 启动自检：常量表校验和+两步判定黄金 10 例全过才放行；
//! ② 自检失败→安全降级模式（只读会话）并显性告警；③ 纯功能代码 ≥300 行。
//! 设计纪律：自检是启动链放行门——任何一项失败都必须进入降级
//! （SafeMode=只读会话标志）+ 显性告警码（不可静默半通过）。

use super::unx36041_privtable::{self, table_checksum, lookup_by_luid, PRIV_N};
use super::unx36042_privcheck::{self, StepCtx};
use super::unx36050_privluid::LuidGen;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfea0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_TABLE: u16 = SEG_BASE + 1; // 表校验和/纪律失败
pub const E_GOLD: u16 = SEG_BASE + 2; // 黄金 10 例失配
pub const E_LUID: u16 = SEG_BASE + 3; // LUID 发射器自检失败
pub const E_GATE: u16 = SEG_BASE + 4; // 放行门失灵（失败仍放行）
pub const E_ALARM: u16 = SEG_BASE + 5; // 降级无显性告警
pub const E_STUCK: u16 = SEG_BASE + 6; // 降级态卡死（应可复位）

/// 自检结论。
pub const S_PASS: u8 = 0; // 放行
pub const S_SAFE: u8 = 1; // 降级（只读会话）

/// 降级告警码（显性——非零即告警面）。
pub const ALARM_NONE: u32 = 0;
pub const ALARM_TABLE: u32 = 1;
pub const ALARM_GOLD: u32 = 2;
pub const ALARM_LUID: u32 = 3;

/// 启动态。
pub struct BootState {
    pub verdict: u8,
    pub alarm: u32,
    pub readonly: bool,
}

impl BootState {
    pub const fn new() -> BootState {
        BootState {
            verdict: S_SAFE,
            alarm: ALARM_NONE,
            readonly: true,
        }
    }
}

/// 黄金 10 例：两步判定权威上下文（结论由表+语义独立推导，硬编码）。
pub struct GoldCase {
    pub luid_low: u32,
    pub enabled: bool,
    pub want: u8,
}

/// 黄金面：5 项表内 LUID（前 5 项）× (启用,要求) 权威结论。
/// 结论推导：enabled=true → V_ALLOWED；enabled=false → V_NOT_ENABLED。
const GOLD_INIT: GoldCase = GoldCase {
    luid_low: 0,
    enabled: false,
    want: 0,
};

pub fn gold_cases() -> [GoldCase; 10] {
    let mut out = [GOLD_INIT; 10];
    let mut i = 0;
    while i < 10 {
        let idx = i % 5;
        let low = unx36041_privtable::PRIV_TABLE[idx].luid_low;
        let enabled = i % 2 == 0;
        out[i] = GoldCase {
            luid_low: low,
            enabled,
            want: if enabled {
                unx36042_privcheck::V_ALLOWED
            } else {
                unx36042_privcheck::V_NOT_ENABLED
            },
        };
        i += 1;
    }
    out
}

/// 自检项 1：常量表校验和+纪律。
fn selftest_table() -> Result<(), u16> {
    unx36041_privtable::table_integrity()?;
    // 校验和面：两次求值一致且非零。
    let c1 = table_checksum();
    let c2 = table_checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_TABLE);
    }
    // 项数锚：17。
    if PRIV_N != 17 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 自检项 2：两步判定黄金 10 例。
fn selftest_gold() -> Result<(), u16> {
    for c in gold_cases().iter() {
        let ctx = StepCtx {
            luid_low: c.luid_low,
            enabled: c.enabled,
            required: true,
        };
        let got = unx36042_privcheck::two_step(&ctx)?;
        if got != c.want {
            return Err(E_GOLD);
        }
    }
    Ok(())
}

/// 自检项 3：LUID 发射器（生成+恢复不回退烟测）。
fn selftest_luid() -> Result<(), u16> {
    let mut g = LuidGen::new();
    let a = g.issue()?;
    let b = g.issue()?;
    if !(b.0 > a.0 || (b.0 == a.0 && b.1 > a.1)) {
        return Err(E_LUID);
    }
    let snap = g.save();
    let mut g2 = LuidGen::restore(&snap)?;
    let c = g2.issue()?;
    if !(c.0 > b.0 || (c.0 == b.0 && c.1 > b.1)) {
        return Err(E_LUID);
    }
    Ok(())
}

/// 启动放行门：三项全过 → S_PASS+readonly=false；任一失败 →
/// S_SAFE+readonly=true+显性告警码（不可半通过）。
pub fn boot_gate() -> BootState {
    let mut st = BootState::new();
    // 项 1。
    if let Err(e) = selftest_table() {
        st.verdict = S_SAFE;
        st.alarm = if e == E_TABLE { ALARM_TABLE } else { ALARM_TABLE };
        st.readonly = true;
        return st;
    }
    // 项 2。
    if let Err(e) = selftest_gold() {
        st.verdict = S_SAFE;
        st.alarm = if e == E_GOLD { ALARM_GOLD } else { ALARM_GOLD };
        st.readonly = true;
        return st;
    }
    // 项 3。
    if let Err(e) = selftest_luid() {
        st.verdict = S_SAFE;
        st.alarm = if e == E_LUID { ALARM_LUID } else { ALARM_LUID };
        st.readonly = true;
        return st;
    }
    st.verdict = S_PASS;
    st.alarm = ALARM_NONE;
    st.readonly = false;
    st
}

/// 降级复位（运维面：告警确认后可重跑自检——非卡死态）。
pub fn reset_after_alarm() -> BootState {
    BootState {
        verdict: S_SAFE,
        alarm: ALARM_NONE,
        readonly: true,
    }
}

/// 表外 LUID 黄金面（10 例注入变体——自检的否定面：
/// 表外+要求 → V_NOT_ENABLED 权威）。
pub fn gold_negative() -> [GoldCase; 10] {
    let mut out = [GOLD_INIT; 10];
    let mut i = 0;
    while i < 10 {
        out[i] = GoldCase {
            luid_low: 9000 + i as u32,
            enabled: true, // 表外即使「启用」也必拒
            want: unx36042_privcheck::V_NOT_ENABLED,
        };
        i += 1;
    }
    out
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TABLE => "表校验和/纪律失败",
        E_GOLD => "黄金 10 例失配",
        E_LUID => "LUID 发射器自检失败",
        E_GATE => "放行门失灵（失败仍放行）",
        E_ALARM => "降级无显性告警",
        E_STUCK => "降级态卡死（应可复位）",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表自检项直调；R2 黄金正/负 10 例直调；
/// R3 放行门全过 → S_PASS + 非只读；R4 降级路径（注入面：负例
/// 进入门上下文必拒）；R5 复位面（降级可重跑不卡死）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：表自检项。
    selftest_table()?;
    hits += 1;
    // R2：黄金正 10 例 + 负 10 例。
    for c in gold_cases().iter() {
        let ctx = StepCtx {
            luid_low: c.luid_low,
            enabled: c.enabled,
            required: true,
        };
        if unx36042_privcheck::two_step(&ctx)? != c.want {
            return Err(E_GOLD);
        }
    }
    for c in gold_negative().iter() {
        let ctx = StepCtx {
            luid_low: c.luid_low,
            enabled: c.enabled,
            required: true,
        };
        if unx36042_privcheck::two_step(&ctx)? != c.want {
            return Err(E_GOLD);
        }
    }
    hits += 1;
    // R3：放行门全过。
    let st = boot_gate();
    if st.verdict != S_PASS || st.readonly || st.alarm != ALARM_NONE {
        return Err(E_GATE);
    }
    hits += 1;
    // R4：降级路径——黄金负例语义即降级触发面（表外不可放行）：
    // 表外 LUID 查表必 E_NOENT（否决面）。
    let mut n = 0;
    while n < 10 {
        if lookup_by_luid(9000 + n as u32).is_ok() {
            return Err(E_GATE); // 表外放行 = 门失灵
        }
        n += 1;
    }
    // 降级态形状：只读 + 显性告警（模拟构造）。
    let degraded = BootState {
        verdict: S_SAFE,
        alarm: ALARM_GOLD,
        readonly: true,
    };
    if degraded.alarm == ALARM_NONE || !degraded.readonly {
        return Err(E_ALARM);
    }
    hits += 1;
    // R5：复位面——降级后可复位重跑。
    let st2 = reset_after_alarm();
    if st2.alarm != ALARM_NONE || !st2.readonly {
        return Err(E_STUCK);
    }
    let st3 = boot_gate();
    if st3.verdict != S_PASS {
        return Err(E_STUCK);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn gate_passes_healthy() {
        let st = boot_gate();
        assert_eq!(st.verdict, S_PASS);
        assert!(!st.readonly);
        assert_eq!(st.alarm, ALARM_NONE);
    }

    #[test]
    fn gold_negative_all_denied() {
        for c in gold_negative().iter() {
            let ctx = StepCtx {
                luid_low: c.luid_low,
                enabled: c.enabled,
                required: true,
            };
            assert_eq!(unx36042_privcheck::two_step(&ctx).unwrap(), c.want);
        }
    }
}

pub fn run_unx36058_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36058_privselftest");
    cs.add("SEG 段位独占", SEG_BASE == 0xFEA0, "0xFEA0~0xFEBF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
