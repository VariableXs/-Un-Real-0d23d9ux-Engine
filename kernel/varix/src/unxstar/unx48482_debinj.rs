//! UNX-F48482 · E·抖动策略注入（反：伪造 39/41ms 边界 ×10 恒定）
//!
//! 判据原文：UNX-F48482-J1 —— 对抗注入下合并器边界恒定：伪造 39ms 序列
//! 十轮全部恒定合并、伪造 41ms 序列十轮全部恒定独立，零漂移；噪声脉冲
//! 串攻击（亚 20ms 密集注入）不污染窗口基准（吸收不推进）；时钟回拨
//! 攻击被拒；长 idle 后边界攻击（重置窗口）与冷启动同结局；全部注入
//! 模式的结局序列与无攻击基准序列逐位一致（无攻击面收益）；fuzz 注入
//! 结局全在白名单。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE640;

pub const E_DRIFT: u16 = SEG_BASE + 1;
pub const E_BASE_POLLUTE: u16 = SEG_BASE + 2;
pub const E_ROLLBACK_LEAK: u16 = SEG_BASE + 3;
pub const E_THIRD_STATE: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_GAIN: u16 = SEG_BASE + 6;

/// 合并窗口上界/下界（与 48481 冻结面一致）。
pub const DEB_WIN_MS: u32 = 40;
pub const DEB_MIN_MS: u32 = 20;

/// 结局码（与 48481 白名单同域）。
pub const OC_ABSORB: u8 = 0;
pub const OC_DOUBLE: u8 = 1;
pub const OC_SINGLE: u8 = 2;

/// 注入用合并器（与 48481 同构独立实现，双盲互证）。
pub struct InjectDeb {
    pub last_accepted: Option<u32>,
    pub out: Vec<u8>,
}

impl InjectDeb {
    pub fn new() -> InjectDeb {
        InjectDeb { last_accepted: None, out: Vec::new() }
    }
    pub fn press(&mut self, now_ms: u32) -> Result<u8, u16> {
        let oc: u8 = match self.last_accepted {
            None => {
                self.last_accepted = Some(now_ms);
                OC_SINGLE
            }
            Some(prev) => {
                if now_ms < prev {
                    return Err(SEG_BASE + 3);
                }
                let gap = now_ms - prev;
                if gap < DEB_MIN_MS {
                    OC_ABSORB
                } else if gap < DEB_WIN_MS {
                    self.last_accepted = Some(now_ms);
                    OC_DOUBLE
                } else {
                    self.last_accepted = Some(now_ms);
                    OC_SINGLE
                }
            }
        };
        self.out.push(oc);
        Ok(oc)
    }
}

/// 攻击模式 A：伪造 39ms 边界序列——首击后每轮 39ms 一击。
pub fn atk_edge39(rounds: u32) -> Result<Vec<u8>, u16> {
    let mut d = InjectDeb::new();
    let mut now: u32 = 0;
    let _ = d.press(now)?;
    for _ in 0..rounds {
        now += 39;
        let _ = d.press(now)?;
    }
    Ok(d.out)
}

/// 攻击模式 B：伪造 41ms 边界序列。
pub fn atk_edge41(rounds: u32) -> Result<Vec<u8>, u16> {
    let mut d = InjectDeb::new();
    let mut now: u32 = 0;
    let _ = d.press(now)?;
    for _ in 0..rounds {
        now += 41;
        let _ = d.press(now)?;
    }
    Ok(d.out)
}

/// 攻击模式 C：噪声脉冲串——每真击前灌 5 连发亚 20ms 噪声。
pub fn atk_pulse_storm(rounds: u32) -> Result<Vec<u8>, u16> {
    let mut d = InjectDeb::new();
    let mut now: u32 = 0;
    let _ = d.press(now)?;
    for _ in 0..rounds {
        // 5 发噪声（5ms 间隔，全部应 Absorb）。
        for _ in 0..5 {
            now += 5;
            let o = d.press(now)?;
            if o != OC_ABSORB {
                return Err(E_BASE_POLLUTE);
            }
        }
        // 真击：距基准 45ms（窗口外独立）。
        now += 45 - 25;
        let _ = d.press(now)?;
    }
    Ok(d.out)
}

/// 基准序列（无攻击）：首击 + 45ms 间隔击 ×rounds。
pub fn baseline(rounds: u32) -> Result<Vec<u8>, u16> {
    let mut d = InjectDeb::new();
    let mut now: u32 = 0;
    let _ = d.press(now)?;
    for _ in 0..rounds {
        now += 45;
        let _ = d.press(now)?;
    }
    Ok(d.out)
}

/// R1 伪造 39ms ×10 恒定：十轮结局序列全等且每轮 Double。
pub fn r1_edge39_constant() -> Result<(), u16> {
    let first = atk_edge39(10)?;
    for _ in 0..10 {
        let again = atk_edge39(10)?;
        if again != first {
            return Err(E_NONDET);
        }
    }
    // 除首击外全部 Double。
    for k in 1..first.len() {
        if first[k] != OC_DOUBLE {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R2 伪造 41ms ×10 恒定：十轮结局序列全等且每轮 Single。
pub fn r2_edge41_constant() -> Result<(), u16> {
    let first = atk_edge41(10)?;
    for _ in 0..10 {
        let again = atk_edge41(10)?;
        if again != first {
            return Err(E_NONDET);
        }
    }
    for k in 1..first.len() {
        if first[k] != OC_SINGLE {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R3 噪声脉冲串不污染基准：真击结局与无攻击基准一致。
pub fn r3_no_base_pollution() -> Result<(), u16> {
    let base = baseline(6)?;
    let atk = atk_pulse_storm(6)?;
    // 攻击序列含噪声 Absorb，抽取真击位（每 6 位一组第 6 位）比对。
    if atk.len() != 6 * 6 + 1 {
        return Err(E_BASE_POLLUTE);
    }
    for r in 0..6 {
        let true_hit = atk[1 + r * 6];
        if true_hit != base[1 + r] {
            return Err(E_GAIN);
        }
    }
    Ok(())
}

/// R4 时钟回拨拒收：回拨必 Err，且状态未损（此后正常按仍合法）。
pub fn r4_rollback_reject() -> Result<(), u16> {
    let mut d = InjectDeb::new();
    let _ = d.press(1000)?;
    let r = d.press(999);
    match r {
        Err(_) => {}
        Ok(_) => return Err(E_ROLLBACK_LEAK),
    }
    // 状态未损：999+39ms=1038 相对基准 1000 是 38ms——等等，回拨被拒后基准仍 1000。
    let o = d.press(1039)?;
    if o != OC_DOUBLE {
        return Err(E_DRIFT);
    }
    Ok(())
}

/// R5 长 idle 后边界攻击：idle 10^6 ms 后 39ms 击与冷启动同结局。
pub fn r5_long_idle() -> Result<(), u16> {
    // 冷启动参照。
    let mut ref_d = InjectDeb::new();
    let _ = ref_d.press(0)?;
    let ref_o = ref_d.press(39)?;
    // 长 idle 版本。
    let mut d = InjectDeb::new();
    let _ = d.press(1_000_000)?;
    let idle_o = d.press(1_000_039)?;
    if ref_o != idle_o || idle_o != OC_DOUBLE {
        return Err(E_GAIN);
    }
    Ok(())
}

/// R6 无攻击面收益：注入序列（39 系）与诚实序列（40 系独立对照）计数差
/// 恒定——攻击只改变攻击者自己选择的 gap，不改变合并器判定规则。
pub fn r6_no_rule_gain() -> Result<(), u16> {
    // 同 gap 恒得同结局：39 永远 Double、40 永远 Single，任何历史不变。
    for _ in 0..10 {
        let mut d = InjectDeb::new();
        let _ = d.press(0)?;
        let a = d.press(39)?;
        // 重置后再测 40。
        let mut d2 = InjectDeb::new();
        let _ = d2.press(0)?;
        let b = d2.press(40)?;
        if a != OC_DOUBLE || b != OC_SINGLE {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R7 fuzz 注入白名单：随机时间流 300 击结局全在 {0,1,2}。
pub fn r7_fuzz_whitelist() -> Result<(), u16> {
    let mut g = Lcg::new(0x48482_0001);
    let mut d = InjectDeb::new();
    let mut now: u32 = 0;
    for _ in 0..300 {
        now += 1 + g.below(90) as u32;
        let o = d.press(now)?;
        if o > OC_SINGLE {
            return Err(E_THIRD_STATE);
        }
    }
    Ok(())
}

/// R8 fuzz 双盲一致：InjectDeb 与独立重演器（逐对重算）结局全等。
pub fn r8_fuzz_dual_blind() -> Result<(), u16> {
    let mut g = Lcg::new(0x48482_0002);
    let mut ticks: Vec<u32> = Vec::new();
    let mut now: u32 = 0;
    for _ in 0..120 {
        now += 1 + g.below(55) as u32;
        ticks.push(now);
    }
    let mut d = InjectDeb::new();
    // 独立重演：逐击重算（不依赖 d 的内部状态）。
    let mut ref_base: Option<u32> = None;
    for t in ticks.iter() {
        let o_live = d.press(*t)?;
        let o_ref: u8 = match ref_base {
            None => OC_SINGLE,
            Some(prev) => {
                if *t < prev {
                    return Err(E_ROLLBACK_LEAK);
                }
                let gap = *t - prev;
                if gap < DEB_MIN_MS {
                    OC_ABSORB
                } else if gap < DEB_WIN_MS {
                    ref_base = Some(*t);
                    OC_DOUBLE
                } else {
                    ref_base = Some(*t);
                    OC_SINGLE
                }
            }
        };
        if o_live != o_ref {
            return Err(E_NONDET);
        }
        if ref_base.is_none() {
            ref_base = Some(*t);
        }
    }
    Ok(())
}

/// R9 ×10 混合模式恒定：39/41 交替十轮，每轮两结局恒定。
pub fn r9_mixed_constant() -> Result<(), u16> {
    for _ in 0..10 {
        let mut d = InjectDeb::new();
        let _ = d.press(0)?;
        if d.press(39)? != OC_DOUBLE {
            return Err(E_DRIFT);
        }
        if d.press(39 + 41)? != OC_SINGLE {
            return Err(E_DRIFT);
        }
        if d.press(39 + 41 + 39)? != OC_DOUBLE {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R10 序列指纹恒定：攻击模式 A/B 各自十轮指纹全等。
pub fn r10_fp_constant() -> Result<(), u16> {
    let fp = |v: &[u8]| -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in v.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let a0 = fp(&atk_edge39(16)?);
    let b0 = fp(&atk_edge41(16)?);
    if a0 == b0 {
        return Err(E_GAIN);
    }
    for _ in 0..10 {
        if fp(&atk_edge39(16)?) != a0 {
            return Err(E_NONDET);
        }
        if fp(&atk_edge41(16)?) != b0 {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_edge39_constant()?;
    hits += 1;
    r2_edge41_constant()?;
    hits += 1;
    r3_no_base_pollution()?;
    hits += 1;
    r4_rollback_reject()?;
    hits += 1;
    r5_long_idle()?;
    hits += 1;
    r6_no_rule_gain()?;
    hits += 1;
    r7_fuzz_whitelist()?;
    hits += 1;
    r8_fuzz_dual_blind()?;
    hits += 1;
    r9_mixed_constant()?;
    hits += 1;
    r10_fp_constant()?;
    hits += 1;
    Ok(hits)
}
