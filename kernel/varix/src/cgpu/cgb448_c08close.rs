//! CGPU-F0448 · C08 组收口与 C09 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - C08 组收口：16 项自检入树/混沌演练与极限压测报告/组双签/台账更新/
//!   向 C09 组移交（高级仲裁特性就绪——C 域收口前最后的功能组）。判据：
//!   入树、报告、双签、移交、判据。
//!
//! 设计纪律：
//! - 16 项自检 = C08 组（F0433~F0448）逐项绿+名称对表（冒名红）。
//! - 演练报告双件：混沌报告（verdict 绿 + 注入次数 > 0）与极限压测
//!   报告（万账户达基线 + 峰值记账）——两件齐才准收口。
//! - 组双签：角色互异 + 台账指纹当下核对（跟真源比）。
//! - 移交闸：入树+双报告+双签三前置全绿才准移交；断点 = F0449
//!   （C09 边界场景组起点）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F30 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F30，全仓独占）。
pub const SEG_BASE: u16 = 0x0F30;

pub const E_C8_RED: u16 = SEG_BASE + 1; // 自检有红项
pub const E_C8_MISSING: u16 = SEG_BASE + 2; // 16 项缺/名对不上
pub const E_C8_CHAOS: u16 = SEG_BASE + 3; // 混沌报告缺/未绿
pub const E_C8_STRESS: u16 = SEG_BASE + 4; // 极限压测报告缺/未达基线
pub const E_C8_SIGN: u16 = SEG_BASE + 5; // 双签不成立
pub const E_C8_HANDOFF: u16 = SEG_BASE + 6; // 移交包裹缺件

/// C08 组 16 项自检（F0433~F0448）。
pub const C08_ITEMS: [&str; 16] = [
    "F0433 拓扑感知仲裁",
    "F0434 大小核异构预算",
    "F0435 优先级老化",
    "F0436 公平指数动态调节",
    "F0437 仲裁策略插件化",
    "F0438 形式化验证接口",
    "F0439 混沌测试",
    "F0440 压力极限",
    "F0441 抢占风暴防护",
    "F0442 决策解释器",
    "F0443 多仲裁器协同预留",
    "F0444 快照与热迁移预留",
    "F0445 高级特性遥测",
    "F0446 高级特性 fuzz",
    "F0447 高级特性 API 冻结",
    "F0448 C08 收口",
];

/// C09 起点锚。
pub const C09_ANCHOR: u32 = 4_449;

/// 混沌演练报告（收口侧摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaosSummary {
    /// 演练 verdict（0 = 绿）。
    pub verdict: u64,
    /// 注入次数（0 = 没练过，不算演练）。
    pub injections: u64,
}

impl ChaosSummary {
    pub fn validate(&self) -> Result<(), u16> {
        if self.verdict != 0 || self.injections == 0 {
            return Err(E_C8_CHAOS);
        }
        Ok(())
    }
}

/// 极限压测报告（收口侧摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressSummary {
    /// 达到的账户规模。
    pub accounts: usize,
    /// 基线账户规模（万账户）。
    pub baseline_accounts: usize,
    /// 峰值申请记账。
    pub peak_requests: u64,
}

impl StressSummary {
    pub fn validate(&self) -> Result<(), u16> {
        if self.accounts < self.baseline_accounts || self.peak_requests == 0 {
            return Err(E_C8_STRESS);
        }
        Ok(())
    }
}

/// 组双签。
#[derive(Clone, Copy, Debug, Default)]
pub struct C8DualSign {
    signers: [u64; 2],
    given: [Option<u64>; 2],
    ledger_fp: u64,
}

impl C8DualSign {
    pub fn new(ledger_fp: u64) -> Self {
        Self {
            ledger_fp,
            ..Default::default()
        }
    }

    pub fn sign(&mut self, role: u8, who: u64, ledger_fp: u64) -> Result<(), u16> {
        if role as usize >= 2 || ledger_fp != self.ledger_fp {
            return Err(E_C8_SIGN);
        }
        let other = 1 - role as usize;
        if self.given[other] == Some(who) {
            return Err(E_C8_SIGN);
        }
        self.signers[role as usize] = who;
        self.given[role as usize] = Some(who);
        Ok(())
    }

    pub fn complete(&self) -> Result<(), u16> {
        if self.given[0].is_some()
            && self.given[1].is_some()
            && self.signers[0] != self.signers[1]
        {
            Ok(())
        } else {
            Err(E_C8_SIGN)
        }
    }
}

/// C09 移交包裹。
#[derive(Clone, Debug)]
pub struct C9Handoff {
    pub anchor: u32,
    /// 交付源（拓扑/定价/老化/公平/插件/形式化/混沌/压测/风暴/解释/协同/快照/遥测/fuzz/API）。
    pub sources: Vec<&'static str>,
    pub ledger_fp: u32,
}

impl C9Handoff {
    pub fn new(sources: &[&'static str], ledger_fp: u32) -> Self {
        Self {
            anchor: C09_ANCHOR,
            sources: sources.iter().copied().collect(),
            ledger_fp,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.anchor != C09_ANCHOR || self.sources.len() < 12 {
            return Err(E_C8_HANDOFF);
        }
        Ok(())
    }
}

/// C08 收口总闸。
#[derive(Debug)]
pub struct C8CloseGate {
    checks: Vec<(&'static str, bool)>,
    chaos: Option<ChaosSummary>,
    stress: Option<StressSummary>,
    sign: C8DualSign,
    handoff: Option<C9Handoff>,
}

impl C8CloseGate {
    pub fn new(ledger_fp: u64) -> Self {
        Self {
            checks: Vec::new(),
            chaos: None,
            stress: None,
            sign: C8DualSign::new(ledger_fp),
            handoff: None,
        }
    }

    pub fn check(&mut self, name: &'static str, green: bool) {
        self.checks.push((name, green));
    }

    /// 自检闭：16 项齐、名对表、全绿。
    pub fn close(&self) -> Result<(usize, usize), u16> {
        if self.checks.len() < C08_ITEMS.len() {
            return Err(E_C8_MISSING);
        }
        for want in C08_ITEMS {
            if !self.checks.iter().any(|(n, _)| *n == want) {
                return Err(E_C8_MISSING);
            }
        }
        let reds = self.checks.iter().filter(|(_, g)| !*g).count();
        if reds > 0 {
            return Err(E_C8_RED);
        }
        Ok((self.checks.len(), reds))
    }

    /// 收口全闭：自检闭 + 混沌报告 + 压测报告 + 双签。
    pub fn close_full(&self) -> Result<(usize, usize), u16> {
        let r = self.close()?;
        self.chaos.ok_or(E_C8_CHAOS)?.validate()?;
        self.stress.ok_or(E_C8_STRESS)?.validate()?;
        self.sign.complete()?;
        Ok(r)
    }

    pub fn attach_chaos(&mut self, c: ChaosSummary) {
        self.chaos = Some(c);
    }

    pub fn attach_stress(&mut self, s: StressSummary) {
        self.stress = Some(s);
    }

    pub fn sign_role(&mut self, role: u8, who: u64, ledger_fp: u64) -> Result<(), u16> {
        self.sign.sign(role, who, ledger_fp)
    }

    /// 移交：close_full 全绿后才准挂包裹。
    pub fn handover(&mut self, h: C9Handoff) -> Result<u32, u16> {
        self.close_full()?;
        h.validate()?;
        self.handoff = Some(h);
        Ok(C09_ANCHOR)
    }

    pub fn handoff_done(&self) -> bool {
        self.handoff.is_some()
    }
}

// ==========================================================================
// 判据（施工书：入树、报告、双签、移交、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn chaos_good() -> ChaosSummary {
        ChaosSummary { verdict: 0, injections: 12 }
    }

    fn stress_good() -> StressSummary {
        StressSummary {
            accounts: 10_000,
            baseline_accounts: 10_000,
            peak_requests: 1_000,
        }
    }

    fn gate_all_green() -> C8CloseGate {
        let mut g = C8CloseGate::new(0xC08C_0C08);
        for name in C08_ITEMS {
            g.check(name, true);
        }
        g.attach_chaos(chaos_good());
        g.attach_stress(stress_good());
        g
    }

    #[test]
    fn a01_十六项自检入树() {
        let g = gate_all_green();
        assert_eq!(g.close(), Ok((16, 0)));
        // 缺项红。
        let mut g2 = C8CloseGate::new(1);
        for name in &C08_ITEMS[..15] {
            g2.check(name, true);
        }
        assert_eq!(g2.close(), Err(E_C8_MISSING));
        // 冒名红。
        let mut g3 = C8CloseGate::new(1);
        for name in &C08_ITEMS[..15] {
            g3.check(name, true);
        }
        g3.check("F9999 冒名", true);
        assert_eq!(g3.close(), Err(E_C8_MISSING));
    }

    #[test]
    fn a02_双报告_混沌与压测() {
        assert_eq!(chaos_good().validate(), Ok(()));
        // 混沌未绿拒。
        let mut c = chaos_good();
        c.verdict = 5;
        assert_eq!(c.validate(), Err(E_C8_CHAOS));
        // 没练过拒。
        let mut c2 = chaos_good();
        c2.injections = 0;
        assert_eq!(c2.validate(), Err(E_C8_CHAOS));
        // 压测：达标放行。
        assert_eq!(stress_good().validate(), Ok(()));
        // 规模不足拒。
        let mut s = stress_good();
        s.accounts = 9_999;
        assert_eq!(s.validate(), Err(E_C8_STRESS));
        // 零峰值拒。
        let mut s2 = stress_good();
        s2.peak_requests = 0;
        assert_eq!(s2.validate(), Err(E_C8_STRESS));
        // 收口闸缺报告 → close_full 红。
        let mut g = C8CloseGate::new(1);
        for name in C08_ITEMS {
            g.check(name, true);
        }
        g.attach_stress(stress_good());
        assert_eq!(g.close_full(), Err(E_C8_CHAOS));
    }

    #[test]
    fn a03_组双签_互异与台账当下() {
        let mut g = gate_all_green();
        assert_eq!(g.close_full(), Err(E_C8_SIGN));
        assert_eq!(g.sign_role(0, 0xA11CE, 0xC08C_0C08), Ok(()));
        assert_eq!(g.close_full(), Err(E_C8_SIGN));
        // 台账不符拒。
        assert_eq!(g.sign_role(1, 0xB0B, 0xDEAD), Err(E_C8_SIGN));
        // 双签互异 → 绿。
        assert_eq!(g.sign_role(1, 0xB0B, 0xC08C_0C08), Ok(()));
        assert!(g.close_full().is_ok());
        // 同人双签拒。
        let mut g2 = C8CloseGate::new(7);
        g2.sign_role(0, 9, 7).ok();
        assert_eq!(g2.sign_role(1, 9, 7), Err(E_C8_SIGN));
        // 角色越界拒。
        assert_eq!(g2.sign_role(2, 10, 7), Err(E_C8_SIGN));
    }

    #[test]
    fn a04_移交断点_F449() {
        let mut g = gate_all_green();
        g.sign_role(0, 0xA11CE, 0xC08C_0C08).ok();
        g.sign_role(1, 0xB0B, 0xC08C_0C08).ok();
        assert!(g.close_full().is_ok());
        let h = C9Handoff::new(
            &["拓扑", "定价", "老化", "公平", "插件", "形式化", "混沌", "压测", "风暴", "解释", "协同", "快照"],
            0xC08C,
        );
        assert_eq!(h.validate(), Ok(()));
        assert_eq!(g.handover(h), Ok(C09_ANCHOR));
        assert_eq!(C09_ANCHOR, 4_449);
        assert!(g.handoff_done());
        // 源不足拒。
        let h2 = C9Handoff::new(&["拓扑"], 0xC08C);
        assert_eq!(h2.validate(), Err(E_C8_HANDOFF));
        // 断点错拒。
        let mut h3 = C9Handoff::new(
            &["拓扑", "定价", "老化", "公平", "插件", "形式化", "混沌", "压测", "风暴", "解释", "协同", "快照"],
            0xC08C,
        );
        h3.anchor = 4_450;
        assert_eq!(h3.validate(), Err(E_C8_HANDOFF));
    }

    #[test]
    fn a05_未收口先移交_闸红() {
        let mut g = C8CloseGate::new(1);
        g.check(C08_ITEMS[0], true);
        let h = C9Handoff::new(
            &["拓扑", "定价", "老化", "公平", "插件", "形式化", "混沌", "压测", "风暴", "解释", "协同", "快照"],
            0xC08C,
        );
        assert_eq!(g.handover(h), Err(E_C8_MISSING));
        // 红项闸。
        let mut g2 = C8CloseGate::new(1);
        for name in C08_ITEMS {
            g2.check(name, name != C08_ITEMS[3]);
        }
        g2.attach_chaos(chaos_good());
        g2.attach_stress(stress_good());
        assert_eq!(g2.close_full(), Err(E_C8_RED));
    }
}
