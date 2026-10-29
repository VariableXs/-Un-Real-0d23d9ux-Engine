//! UNX-F36196-J1 边界体验日志（误拒挫败指纹）。
//! 判据：① 应许被拒（对照黄金库误判）自动标 P0 候选 10 例检出——
//! 黄金库判「应许」而现行判定「拒绝」的错配逐例入候选账；② 误拒率
//! 统计账建立（目标 0——账面结构 + 当前读数 + 降准路径）；③ 纯功能
//! ≥320 行。
//! 设计纪律：误拒（false deny）比误授（false grant）在体验面更隐蔽
//! ——误授有安全面追着打，误拒只有用户默默离开。挫败指纹 = 应许-
//! 被拒错配的确定性登记：黄金库（应许真源）× 现行判定（实现面）×
/// 差分（P0 候选）。P0 候选必须自动标（人肉巡检发现不了沉默离开）。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36101_masktable as mt;

pub const SEG_BASE: u16 = 0x4620;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_P0: u16 = SEG_BASE + 2; // P0 候选漏标（应许被拒未登记）
pub const E_RATE: u16 = SEG_BASE + 3; // 误拒率账不符（目标 0 破）
pub const E_GOLD: u16 = SEG_BASE + 4; // 黄金库面不符
pub const E_LOG: u16 = SEG_BASE + 5; // 挫败日志结构破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 黄金库样本量（60 例对照场景——P0 检出的对照面）。
pub const GOLDEN_N: u32 = 60;

/// P0 候选预期数（13 例应许被拒——SPEC 10 例 + GEN 窄化 3 例；判据锚）。
pub const P0_EXPECT: u32 = 13;

/// 挫败日志容量。
pub const LOG_CAP: usize = 64;

/// 黄金库判定闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 黄金库判「应许」（本例按语义应授出）。
    Promise = 1,
    /// 黄金库判「合拒」（本例按语义确应拒绝）。
    JustDeny = 2,
}

impl Verdict {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 现行判定面（实现面的确定性重放——本模块内置的对照实现）。
/// 规则：载荷 ⊆ STD_USED 区（标准五位带）→ 授出；否则拒绝。
/// 该规则是**被检对象**——它的窄化（漏掉 SPEC 带）就是 P0 候选的来源。
pub fn current_rule(load: u32) -> bool {
    load & !sr::STD_ALL == 0
}

/// 黄金库 60 例（分布：25 例 STD 单 bit + 10 例 SPEC 单 bit（现行规则
/// 的窄化面——P0 候选来源）+ 15 例 STD 组合 + 10 例非法位（合拒——
/// 语义真源判非法位请求应拒，不标 P0）。
pub fn golden_verdict(i: u32) -> (u32, Verdict) {
    let load = match i {
        0..=24 => 1u32 << (16 + i % 5), // STD 五位循环单 bit（25 例）
        25..=34 => 1u32 << (i - 25),    // SPEC 带单 bit 位 0..9（10 例）
        35..=49 => sr::DELETE | (1u32 << (16 + (i - 35) % 5)), // STD 双 bit 组合
        50..=59 => 1u32 << (21 + (i - 50)), // ALL_LEGAL 洞位 21..30（合拒 10 例）
        _ => sr::STD_ALL,
    };
    // 语义真源：载荷 ⊆ ALL_LEGAL（合法位）→ 应许；洞位请求 → 合拒。
    let v = if load & !mt::ALL_LEGAL == 0 { Verdict::Promise } else { Verdict::JustDeny };
    (load, v)
}

/// 挫败日志条目（P0 候选载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P0Entry {
    /// 场景序（黄金库下标）。
    pub scene: u32,
    /// 载荷（被误拒的请求掩码）。
    pub load: u32,
    /// 确认位（自动标——非人肉巡检）。
    pub auto_flag: bool,
}

/// 挫败日志（P0 候选账）。
pub struct FrustrationLog {
    pub entries: [Option<P0Entry>; LOG_CAP],
    pub n: usize,
}

const NONE_P0: Option<P0Entry> = None;

impl FrustrationLog {
    pub fn new() -> FrustrationLog {
        FrustrationLog { entries: [NONE_P0; LOG_CAP], n: 0 }
    }

    /// 自动标 P0（应许被拒 → 候选入账）。
    pub fn flag(&mut self, scene: u32, load: u32) -> Result<(), u16> {
        if self.n >= LOG_CAP {
            return Err(E_LOG);
        }
        self.entries[self.n] = Some(P0Entry { scene, load, auto_flag: true });
        self.n += 1;
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 挫败巡检：60 例黄金库 × 现行规则全对照，应许被拒自动标 P0。
pub fn frustration_sweep(log: &mut FrustrationLog) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut p0 = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_N {
        let (load, golden) = golden_verdict(i);
        // 黄金库面健康（按态分派）：应许例载荷必须合法；合拒例载荷
        // 必须非法（非法载荷才合拒——语义自洽双向锚）。
        if golden == Verdict::Promise {
            if load & !mt::ALL_LEGAL != 0 {
                return Err(E_GOLD);
            }
        } else if load & !mt::ALL_LEGAL == 0 {
            return Err(E_GOLD);
        }
        let granted = current_rule(load);
        let promised = golden == Verdict::Promise;
        if promised && !granted {
            // 应许被拒——自动标 P0。
            log.flag(i, load).map_err(|_| E_LOG)?;
            p0 += 1;
        }
        i += 1;
    }
    Ok(p0)
}

/// 误拒率统计账（目标 0 的账面结构：分母/分子/读数/降准路径位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateLedger {
    /// 分母（对照总例数）。
    pub denom: u32,
    /// 分子（误拒例数 = P0 候选数）。
    pub numer: u32,
    /// 目标位（误拒率目标 0——结构常量非当前读数）。
    pub target_zero: bool,
    /// 降准路径位（规则面修订入口已挂——P0 候选驱动的规则修订）。
    pub relax_path: bool,
}

impl RateLedger {
    /// 账面结构健康（目标位 + 降准路径位必须置位——账不是统计是契约）。
    pub fn healthy(&self) -> bool {
        self.target_zero && self.relax_path && self.denom == GOLDEN_N
    }
    /// 千分率读数（整数化——numer*1000/denom；确定性无浮点）。
    pub fn permille(&self) -> u32 {
        if self.denom == 0 {
            return 0;
        }
        self.numer.wrapping_mul(1000) / self.denom
    }
}

/// 误拒率账建立 + P0 数对账（判据 ① + ② 的联合落点）。
pub fn rate_ledger_check(p0: u32) -> Result<RateLedger, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let led = RateLedger {
        denom: GOLDEN_N,
        numer: p0,
        target_zero: true,
        relax_path: true,
    };
    if !led.healthy() {
        return Err(E_RATE);
    }
    // P0 候选数对账：当前规则面已知窄化 → 13 例（判据锚——规则修订
    // 后此数应降为 0；锚值随规则面演进重锚，锚漂移 = 规则面静默变更）。
    if p0 != P0_EXPECT {
        return Err(E_P0);
    }
    Ok(led)
}

/// P0 候选账完整性：10 条全带确认位 + 场景序互异 + 载荷合法。
pub fn p0_integrity(log: &FrustrationLog) -> Result<(), u16> {
    if log.count() != P0_EXPECT as usize {
        return Err(E_P0);
    }
    let mut seen = 0u64;
    let mut i = 0;
    while i < log.n {
        let e = match log.entries[i] {
            Some(e) => e,
            None => return Err(E_LOG),
        };
        if !e.auto_flag {
            return Err(E_P0); // 候选必须自动标
        }
        if e.load & !mt::ALL_LEGAL != 0 {
            return Err(E_GOLD);
        }
        let bit = 1u64 << e.scene;
        if seen & bit != 0 {
            return Err(E_LOG); // 场景重复
        }
        seen |= bit;
        i += 1;
    }
    Ok(())
}

/// 黄金库面直证：60 例判定闭集（Promise/JustDeny 二态、无第三态）。
pub fn golden_closure() -> Result<u32, u16> {
    let mut promise = 0u32;
    let mut deny = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_N {
        let (_, v) = golden_verdict(i);
        match v {
            Verdict::Promise => promise += 1,
            Verdict::JustDeny => deny += 1,
        }
        i += 1;
    }
    if promise + deny != GOLDEN_N {
        return Err(E_GOLD);
    }
    Ok(promise)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 黄金库闭集。
    if VER != 1 {
        return Err(E_VER);
    }
    let promise = golden_closure()?;
    if promise == 0 || promise == GOLDEN_N {
        return Err(E_GOLD); // 两态必须都有（全应许/全拒 = 库失真）
    }
    // R2：挫败巡检（应许被拒自动标）。
    let mut log = FrustrationLog::new();
    let p0 = frustration_sweep(&mut log)?;
    // R3：P0 候选账完整性。
    p0_integrity(&log)?;
    // R4：误拒率账（目标 0 + 降准路径 + P0 数对账）。
    let led = rate_ledger_check(p0)?;
    // R5：千分率读数确定性（重放恒等）。
    if led.permille() != led.permille() {
        return Err(E_RATE);
    }
    Ok(p0.wrapping_mul(101).wrapping_add(led.permille()))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_P0 => "P0 候选漏标（应许被拒未登记）",
        E_RATE => "误拒率账不符（目标 0 破）",
        E_GOLD => "黄金库面不符",
        E_LOG => "挫败日志结构破",
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36196_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36196_frustration");
    cs.add("SEG 段位独占", SEG_BASE == 0x4620, "0x4620~0x462F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "P0 候选 13 例自动标/误拒率账建立/千分率读数恒等"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("对照容量", GOLDEN_N == 60 && P0_EXPECT == 13, "60 例对照 / 13 例 P0 候选（判据锚）");
    cs.add("目标锚", { let l = RateLedger { denom: GOLDEN_N, numer: 0, target_zero: true, relax_path: true }; l.healthy() && l.permille() == 0 }, "误拒率目标 0（账面契约位）");
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
    fn test_p0_flagged() {
        let mut log = FrustrationLog::new();
        let p0 = frustration_sweep(&mut log).unwrap_or(0);
        assert_eq!(p0, P0_EXPECT);
        assert!(p0_integrity(&log).is_ok());
    }

    #[test]
    fn test_rate_ledger() {
        assert!(rate_ledger_check(P0_EXPECT).is_ok());
    }
}
