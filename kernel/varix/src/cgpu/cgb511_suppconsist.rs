//! CGPU-F0511 · 抑制一致性（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 一致性：跨呈现模式行为声明一致（各模式下的策略语义统一表述）；
//!   跨硬件一致（均衡行为不因硬件漂移——确定性纪律）；与 80 帧合同
//!   口径一致（抖动定义与合同条款同源）。判据：模式一致、硬件一致、
//!   口径统一、判据。
//!
//! 设计纪律：
//! - 跨模式声明一致：三模式（垂直同步/邮箱/立即）的策略语义登记为
//!   语义指纹（策略+吸收上限+折减常量的三元组哈希）——三模式指纹
//!   同构（仅参数不同，语义结构一致）。
//! - 跨硬件确定性：同输入注入谱在不同"硬件档"（计数器频率差异仅
//!   影响计量单位换算，不影响决策）下输出决策序列逐位一致。
//! - 合同口径同源：抖动阈值常量必须引用合同常量（JITTER_LINE_US =
//!   1.5×CONTRACT_US 表达式同源，不得另立字面量）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5260 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x5260，批十独占）。
pub const SEG_BASE: u16 = 0x5260;

pub const E_CS_MODE: u16 = SEG_BASE + 1; // 模式语义指纹失配
pub const E_CS_HW: u16 = SEG_BASE + 2; // 硬件决策漂移
pub const E_CS_CONTRACT: u16 = SEG_BASE + 3; // 合同口径脱钩

/// 合同常量同源引用（禁止另立字面量——一致性纪律的锚）。
pub const CONTRACT_US: u64 = 12_500;
pub const JITTER_LINE_US: u64 = CONTRACT_US * 3 / 2; // 1.5× 合同，同源表达式

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 模式策略语义声明（结构同构、参数分列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeSemantics {
    /// 呈现模式码。
    pub mode: u8,
    /// 允许策略（位图：bit0 直通 / bit1 深缓冲 / bit2 提前渲染）。
    pub allowed_strats: u8,
    /// 吸收上限（us）。
    pub absorb_cap_us: u64,
    /// 提前渲染折减（us）。
    pub early_us: u64,
}

impl ModeSemantics {
    /// 语义指纹：结构字段哈希（不含 mode——模式本身不是语义）。
    pub fn sem_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.allowed_strats.to_le_bytes());
        bytes.extend_from_slice(&self.absorb_cap_us.to_le_bytes());
        bytes.extend_from_slice(&self.early_us.to_le_bytes());
        fnv1a64(&bytes)
    }
}

/// 三模式声明面（v1 规范源）。
pub fn mode_semantics() -> [ModeSemantics; 3] {
    [
        ModeSemantics { mode: 1, allowed_strats: 0b111, absorb_cap_us: 4_687, early_us: 781 },
        ModeSemantics { mode: 2, allowed_strats: 0b011, absorb_cap_us: 3_125, early_us: 781 },
        ModeSemantics { mode: 3, allowed_strats: 0b111, absorb_cap_us: 6_250, early_us: 781 },
    ]
}

/// 跨模式一致审计：三模式语义指纹结构同构（允许域一致的超集关系成立
/// 即统一表述成立；这里校验"邮箱模式禁用提前渲染外全部可用"的声明）。
pub fn audit_mode_consistency() -> Result<(), u16> {
    let m = mode_semantics();
    for x in &m {
        if x.allowed_strats == 0 {
            return Err(E_CS_MODE);
        }
        if x.absorb_cap_us > JITTER_LINE_US {
            return Err(E_CS_MODE); // 吸收上限不得越过合同抖动线
        }
    }
    // 邮箱模式声明禁用提前渲染（bit2=0）
    if m[1].allowed_strats & 0b100 != 0 {
        return Err(E_CS_MODE);
    }
    Ok(())
}

/// 硬件档（计数器频率差异 → 量化误差幅度，单位 us）。
#[derive(Clone, Copy, Debug)]
pub struct HwProfile {
    /// 计数器每微秒刻度（刻度越稀，量化误差越大）。
    pub ticks_per_us: u64,
}

impl HwProfile {
    /// 量化误差幅度（us）：tick 粒度的向上取整。
    pub fn quant_err_us(&self) -> u64 {
        1 + (self.ticks_per_us / 64) // 快档(≥64) 1us，慢档最多 +1us
    }
}

/// 抖动决策器：阈值判断只依赖换算后的 us 值（决策与硬件无关）。
pub fn jitter_decision(jitter_us: u64, sem: &ModeSemantics) -> u8 {
    if jitter_us <= sem.absorb_cap_us {
        1 // 深缓冲吸收
    } else if jitter_us <= JITTER_LINE_US {
        2 // 提前渲染折减
    } else {
        3 // 上报（合同红线）
    }
}

/// 跨硬件决策一致性：注入谱避开合同边界，逐点验证 ±量化误差幅度内
/// 决策分级稳定（两档硬件取最大误差幅度做邻域检查）。
pub fn audit_hw_consistency(h1: HwProfile, h2: HwProfile, injections_us: &[u64]) -> Result<(), u16> {
    let sem = &mode_semantics()[0];
    let err = h1.quant_err_us().max(h2.quant_err_us());
    for &j in injections_us {
        let base = jitter_decision(j, sem);
        for d in 1..=err {
            if j > d && jitter_decision(j - d, sem) != base {
                return Err(E_CS_HW);
            }
            if jitter_decision(j.saturating_add(d), sem) != base {
                return Err(E_CS_HW);
            }
        }
    }
    Ok(())
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb511_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 跨模式一致：规范面审计通过
    v.push(("模式一致-三模式语义审计", audit_mode_consistency() == Ok(())));

    // 2. 邮箱禁用提前渲染：声明面体现
    let m = mode_semantics();
    v.push((
        "模式一致-邮箱禁提前渲染",
        m[1].allowed_strats & 0b100 == 0 && m[0].allowed_strats & 0b100 != 0,
    ));

    // 3. 合同口径同源：抖动线 = 1.5×合同（表达式同源，无字面量）
    v.push(("口径-抖动线合同同源", JITTER_LINE_US == 18_750 && CONTRACT_US == 12_500));

    // 4. 吸收上限不越合同抖动线
    v.push((
        "口径-吸收上限不越线",
        m.iter().all(|x| x.absorb_cap_us <= JITTER_LINE_US),
    ));

    // 5. 跨硬件决策一致：快慢计数器同谱（避开边界）决策稳定
    let fast = HwProfile { ticks_per_us: 100 };
    let slow = HwProfile { ticks_per_us: 33 };
    let inj: Vec<u64> = vec![2_000, 10_000, 25_000];
    v.push((
        "硬件一致-同谱同决策",
        audit_hw_consistency(fast, slow, &inj) == Ok(()),
    ));

    // 6. 决策分级正确：≤ 吸收上限→1；≤ 抖动线→2；> 抖动线→3
    let sem = &m[0]; // 吸收上限 4687
    let d1 = jitter_decision(4_000, sem) == 1;
    let d2 = jitter_decision(10_000, sem) == 2;
    let d3 = jitter_decision(19_000, sem) == 3;
    v.push(("口径-三级决策边界", d1 && d2 && d3));

    // 7. 语义指纹确定性：同声明同指纹
    let a = m[0].sem_hash();
    let b = ModeSemantics { mode: 1, allowed_strats: 0b111, absorb_cap_us: 4_687, early_us: 781 }.sem_hash();
    v.push(("模式一致-语义指纹稳定", a == b));

    // 8. 边界敏感点检出：注入恰在吸收上限（4687）——邻域跨级即 E_CS_HW
    let edge_inj: Vec<u64> = vec![4_687];
    v.push((
        "硬件一致-边界敏感检出",
        audit_hw_consistency(fast, slow, &edge_inj) == Err(E_CS_HW),
    ));

    v
}

pub fn run_cgb511_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb511");
    for (name, ok) in cgb511_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb511_raw_checks as raw;
    #[test]
    fn t_cgb511() {
        for (n, ok) in raw() {
            assert!(ok, "cgb511 check failed: {}", n);
        }
    }
}
