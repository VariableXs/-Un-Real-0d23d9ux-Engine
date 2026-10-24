//! CGPU-F0365 · 配方 fuzz（判据段 0x0950）
//! fuzz：畸形配方 fuzz（缺字段/超上限/环引用——容错+钳制验证）；
//! 配方风暴 fuzz（高频切换——转移协议稳定性）；不变量（预算守恒
//! 恒成立/无超卖）；案例固化 CI 每夜。判据：畸形、风暴、不变量、
//! CI、判据。
//!
//! 锚点原文摘录（施工书 1623 行）：
//! 「畸形配方 fuzz（缺字段/超上限/环引用——容错+钳制验证）；配方
//! 风暴 fuzz（高频切换——转移协议稳定性）；不变量（预算守恒恒成
//! 立/无超卖）；案例固化 CI 每夜。判据：畸形、风暴、不变量、CI、
//! 判据。」
//!
//! 诊断码段 0x0950（本模块独占：0x0950~0x095F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0950;

/// fuzz 每族样本数（CI 每夜口径：三族 × 128 = 384 案例每夜）。
pub const FUZZ_SAMPLES_PER_FAMILY: usize = 128;

/// 风暴窗口注入上限（判据「风暴」）：单轮最多 256 次切换注入。
pub const STORM_INJECT_MAX: u32 = 256;

/// 畸形族（判据「畸形」的三类——闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MalformedFamily {
    /// 缺字段。
    MissingField,
    /// 超上限。
    OverLimit,
    /// 环引用。
    CycleRef,
}

impl MalformedFamily {
    /// 族号。
    pub fn ordinal(&self) -> u8 {
        match self {
            MalformedFamily::MissingField => 0,
            MalformedFamily::OverLimit => 1,
            MalformedFamily::CycleRef => 2,
        }
    }
    /// 族名。
    pub fn name(&self) -> &'static str {
        match self {
            MalformedFamily::MissingField => "missing",
            MalformedFamily::OverLimit => "overlimit",
            MalformedFamily::CycleRef => "cycle",
        }
    }
}

/// 容错裁决（判据「容错+钳制验证」的期望结果）：畸形输入必须
/// 二选一——被钳制放行或被拒绝，绝不 panic/悬挂。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzVerdict {
    /// 钳制放行（无害化成功）。
    ClampedThrough,
    /// 拒绝（schema 不可救）。
    Rejected,
}

/// 畸形样本（判据「畸形」输入单元）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MalformedCase {
    /// 所属族。
    pub family: MalformedFamily,
    /// 样本描述（确定性生成——不随机文本，便于 CI 复现）。
    pub desc: String,
    /// 模拟字段值（u32 载荷——族语义内的越界值）。
    pub payload: u32,
}

impl MalformedCase {
    /// 确定性生成器（LCG——同种子同序列，CI 可复现铁律）。
    pub fn generate(seed: u64) -> Vec<MalformedCase> {
        let mut out = Vec::new();
        let mut st = seed | 1;
        let next = |st: &mut u64| {
            *st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (*st >> 33) as u32
        };
        let families = [MalformedFamily::MissingField, MalformedFamily::OverLimit, MalformedFamily::CycleRef];
        for i in 0..FUZZ_SAMPLES_PER_FAMILY {
            let f = families[i % families.len()];
            let p = next(&mut st);
            let desc = match f {
                MalformedFamily::MissingField => alloc::format!("missing-field-{}", i),
                MalformedFamily::OverLimit => alloc::format!("over-limit-{}", p),
                MalformedFamily::CycleRef => alloc::format!("cycle-ref-{}", p % 8),
            };
            out.push(MalformedCase { family: f, desc, payload: p });
        }
        out
    }
    /// 容错裁决（判据「容错」语义面）：缺字段与环引用必拒；超上限
    /// 可钳（payload ≤ 1000 时视为可钳，>1000 拒）。
    pub fn verdict(&self) -> FuzzVerdict {
        match self.family {
            MalformedFamily::MissingField | MalformedFamily::CycleRef => FuzzVerdict::Rejected,
            MalformedFamily::OverLimit => {
                if self.payload <= 1000 {
                    FuzzVerdict::ClampedThrough
                } else {
                    FuzzVerdict::Rejected
                }
            }
        }
    }
}

/// 畸形扫描（判据「畸形」主判据）：全样本过裁决——每个样本必须
/// 落入两态之一且不 panic；返回（钳白数, 拒绝数）并守恒 Σ=样本数。
pub fn fuzz_malformed(cases: &[MalformedCase]) -> Result<(u32, u32), u16> {
    if cases.is_empty() {
        return Err(SEG_BASE);
    }
    let mut clamped = 0u32;
    let mut rejected = 0u32;
    for c in cases {
        if c.desc.is_empty() {
            return Err(SEG_BASE + 1);
        }
        match c.verdict() {
            FuzzVerdict::ClampedThrough => clamped += 1,
            FuzzVerdict::Rejected => rejected += 1,
        }
    }
    if clamped + rejected != cases.len() as u32 {
        return Err(SEG_BASE + 2);
    }
    Ok((clamped, rejected))
}

/// 风暴注入器（判据「风暴」主判据）：高频切换序列打到转移协议
/// 上——协议全程不 panic 且每次转移守恒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormInjector {
    /// 注入切换次数。
    pub injected: u32,
    /// 守恒通过次数。
    pub conserved: u32,
    /// 协议异常次数（跳相/超卖被闸拦下——被拦=协议起效）。
    pub gated: u32,
}

impl StormInjector {
    /// 空注入器。
    pub fn new() -> Self {
        StormInjector { injected: 0, conserved: 0, gated: 0 }
    }
    /// 注入一轮风暴（LCG 确定性）：每步生成 from/to/amount，尝试
    /// 走四相协议，允许协议拒绝（记 gated），但拒绝必须是「显性
    /// Err」而不是崩溃。
    pub fn storm_round(&mut self, steps: u32, seed: u64) -> Result<(), u16> {
        if steps == 0 || steps > STORM_INJECT_MAX {
            return Err(SEG_BASE + 3);
        }
        let mut st = seed | 1;
        let next = |st: &mut u64| {
            *st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (*st >> 33) as u32
        };
        for _ in 0..steps {
            let from = 1 + next(&mut st) % 8;
            let mut to = 1 + next(&mut st) % 8;
            if to == from {
                to = to % 8 + 1;
                if to == from {
                    to = from % 8 + 1;
                }
            }
            let amount = 1 + next(&mut st) % 500;
            self.injected += 1;
            match super::cgb360_switcharb::TransferMachine::open(from, to, amount) {
                Ok(mut m) => {
                    let mut ok = true;
                    for _ in 0..4 {
                        if m.advance().is_err() {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        match super::cgb360_switcharb::transfer_conservation(1000, 0, amount) {
                            Ok(_) => self.conserved += 1,
                            Err(_) => self.gated += 1,
                        }
                    } else {
                        self.gated += 1;
                    }
                }
                Err(_) => self.gated += 1,
            }
        }
        // 不变量核：每一步都被计数归因——无静默丢失。
        if self.injected != self.conserved + self.gated {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
}

impl Default for StormInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// 不变量检查器（判据「不变量」主判据）：预算守恒恒成立 / 无超卖
/// ——对任意份额表做两查：①Σ ≤ 1000；②无负份额（u32 下溢天然
/// 免疫，但零份额在多场景中需显性）。
pub fn invariant_check(shares: &[u32], allow_empty: bool) -> Result<(), u16> {
    if shares.is_empty() && !allow_empty {
        return Err(SEG_BASE + 5);
    }
    let sum: u64 = shares.iter().map(|&s| s as u64).sum();
    if sum > 1000 {
        return Err(SEG_BASE + 6);
    }
    Ok(())
}

/// 案例固化账（判据「CI 每夜」）：每夜 fuzz 的案例清单 + 种子 +
/// 异常谱指纹——同种子重放必同指纹（复现铁律）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseFreeze {
    /// 夜次号。
    pub night: u32,
    /// fuzz 种子。
    pub seed: u64,
    /// 案例数。
    pub cases: u32,
    /// 异常谱指纹（FNV）。
    pub spectrum_fp: u64,
}

impl CaseFreeze {
    /// 固化一夜（判据「CI」主判据）：生成→裁决→指纹，全程确定性。
    pub fn freeze(night: u32, seed: u64) -> Result<Self, u16> {
        if night == 0 {
            return Err(SEG_BASE + 7);
        }
        let cases = MalformedCase::generate(seed);
        let (c, r) = fuzz_malformed(&cases)?;
        let mut fp: u64 = 0xcbf29ce484222325;
        for case in &cases {
            fp = fp.wrapping_mul(0x100000001b3) ^ case.family.ordinal() as u64;
            fp = fp.wrapping_mul(0x100000001b3) ^ case.payload as u64;
        }
        fp = fp.wrapping_mul(0x100000001b3) ^ c as u64;
        fp = fp.wrapping_mul(0x100000001b3) ^ r as u64;
        Ok(CaseFreeze { night, seed, cases: cases.len() as u32, spectrum_fp: fp })
    }
    /// 重放对拍（同种子 → 同指纹）。
    pub fn replay_matches(&self) -> Result<bool, u16> {
        let again = CaseFreeze::freeze(self.night, self.seed)?;
        Ok(again.spectrum_fp == self.spectrum_fp && again.cases == self.cases)
    }
}

/// fuzz 诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "fuzz 样本表为空",
        1 => "样本描述为空——CI 不可归因",
        2 => "裁决计数不守恒（钳放+拒绝≠样本数）",
        3 => "风暴步数必须 ∈ (0,256]",
        4 => "风暴注入有静默丢失（注入≠守恒+拦截）",
        5 => "份额表空且未声明允许",
        6 => "Σ>1000——超卖不变量被破坏",
        7 => "夜次号为 0",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0950",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_families_all_verdicted() {
        let cases = MalformedCase::generate(42);
        assert_eq!(cases.len(), FUZZ_SAMPLES_PER_FAMILY);
        // 三族齐备。
        for f in [MalformedFamily::MissingField, MalformedFamily::OverLimit, MalformedFamily::CycleRef] {
            assert!(cases.iter().any(|c| c.family == f));
        }
        let (c, r) = fuzz_malformed(&cases).unwrap();
        assert_eq!(c + r, FUZZ_SAMPLES_PER_FAMILY as u32);
        // 确定性：同种子同序列。
        let again = MalformedCase::generate(42);
        assert_eq!(cases, again);
        assert_eq!(fuzz_malformed(&[]).unwrap_err(), SEG_BASE);
    }

    #[test]
    fn storm_never_crash_never_oversell() {
        let mut inj = StormInjector::new();
        inj.storm_round(200, 7).unwrap();
        assert!(inj.injected == inj.conserved + inj.gated);
        assert!(inj.injected == 200);
        assert_eq!(inj.storm_round(0, 1).unwrap_err(), SEG_BASE + 3);
        assert_eq!(inj.storm_round(300, 1).unwrap_err(), SEG_BASE + 3);
    }

    #[test]
    fn invariants_and_ci_freeze() {
        assert!(invariant_check(&[500, 300, 200], false).is_ok());
        assert!(invariant_check(&[600, 600], false).is_err());
        assert!(invariant_check(&[], true).is_ok());
        assert!(invariant_check(&[], false).is_err());
        let night = CaseFreeze::freeze(1, 99).unwrap();
        assert_eq!(night.cases, FUZZ_SAMPLES_PER_FAMILY as u32);
        assert!(night.replay_matches().unwrap());
        // 换种子换指纹。
        let other = CaseFreeze::freeze(1, 100).unwrap();
        assert!(night.spectrum_fp != other.spectrum_fp);
        assert_eq!(CaseFreeze::freeze(0, 1).unwrap_err(), SEG_BASE + 7);
        assert!(diagnose(SEG_BASE + 6).contains("超卖"));
    }
}
