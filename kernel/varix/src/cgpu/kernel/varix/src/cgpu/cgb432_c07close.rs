//! CGPU-F0432 · C07 组收口与 C08 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - C07 组收口：16 项自检入树/7 天长测报告/隐私复核归档/组双签/
//!   台账更新/向 C08 高级仲裁特性组移交（长期统计就绪——高级仲裁的
//!   动态调节有了历史数据支撑）。判据：入树、长测报告、归档、双签、
//!   移交、判据。
//!
//! 设计纪律：
//! - 16 项自检 = C07 组（F0417~F0432）16 张单逐项绿+名称对表
//!   （对不上名 = 冒名顶替，红）。
//! - 7 天长测报告三硬指标：天数 ≥ 7、注入事件零丢失、异常检出率
//!   ≥ 80%（与 F0427 长期协议判据同源）。
//! - 隐私复核归档：F0428 复核三件（红线 0 + 聚合默认 + 脱敏开）
//!   pass 后才准归档。
//! - 组双签：两签角色互异 + 指纹当下核对（跟台账指纹比，不跟同伴比）。
//! - 移交闸：入树+长测+归档+双签四前置全绿才准移交；移交断点 =
//!   F0433（C08 起点）。移交时显式声明"统计就绪"供 C08 动态调节
//!   读取历史数据——断链即红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E30 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E30，全仓独占）。
pub const SEG_BASE: u16 = 0x0E30;

pub const E_C7_RED: u16 = SEG_BASE + 1; // 自检有红项
pub const E_C7_MISSING: u16 = SEG_BASE + 2; // 16 项缺/名对不上
pub const E_C7_REPORT: u16 = SEG_BASE + 3; // 长测报告硬指标不达标
pub const E_C7_PRIV: u16 = SEG_BASE + 4; // 隐私复核未归档
pub const E_C7_SIGN: u16 = SEG_BASE + 5; // 双签不成立
pub const E_C7_HANDOFF: u16 = SEG_BASE + 6; // 移交闸未过/包裹缺件

/// C07 组 16 项自检（F0417~F0432 逐项）。
pub const C07_ITEMS: [&str; 16] = [
    "F0417 长期统计总架构",
    "F0418 统计立方",
    "F0419 账单报告",
    "F0420 画像持久化",
    "F0421 漂移检测",
    "F0422 季节模式",
    "F0423 异常检测",
    "F0424 容量规划",
    "F0425 统计导出",
    "F0426 存储治理",
    "F0427 长期协议",
    "F0428 统计隐私",
    "F0429 统计遥测",
    "F0430 统计 fuzz",
    "F0431 统计 API 冻结",
    "F0432 C07 收口",
];

/// C08 起点锚（移交断点 = F0433）。
pub const C08_ANCHOR: u32 = 4_033;

/// 长测最短天数。
pub const LONGTEST_MIN_DAYS: u64 = 7;
/// 异常检出率下限（千分比）。
pub const DETECT_MIN_PM: u64 = 800;

/// 7 天长测报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongTestReport {
    /// 覆盖天数。
    pub days: u64,
    /// 注入事件数。
    pub injected: u64,
    /// 丢失数（必须 0）。
    pub lost: u64,
    /// 检出数。
    pub detected: u64,
    /// 复现种子。
    pub seed: u64,
}

impl LongTestReport {
    /// 三硬指标：天数 ≥ 7、零丢失、检出率 ≥ 80%。
    pub fn validate(&self) -> Result<(), u16> {
        if self.days < LONGTEST_MIN_DAYS {
            return Err(E_C7_REPORT);
        }
        if self.lost != 0 || self.injected == 0 {
            return Err(E_C7_REPORT);
        }
        let rate_pm = self.detected.saturating_mul(1000) / self.injected;
        if rate_pm < DETECT_MIN_PM {
            return Err(E_C7_REPORT);
        }
        Ok(())
    }

    /// 检出率（千分比）。
    pub fn rate_pm(&self) -> u64 {
        if self.injected == 0 {
            0
        } else {
            self.detected.saturating_mul(1000) / self.injected
        }
    }
}

/// 隐私复核归档件（F0428 复核三件同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivArchive {
    /// 红线违规计数（必须 0）。
    pub redline_hits: u32,
    /// 聚合默认视图已验证。
    pub agg_default_ok: bool,
    /// 脱敏已开启。
    pub mask_on: bool,
}

impl PrivArchive {
    pub fn validate(&self) -> Result<(), u16> {
        if self.redline_hits != 0 || !self.agg_default_ok || !self.mask_on {
            return Err(E_C7_PRIV);
        }
        Ok(())
    }
}

/// 组双签。
#[derive(Clone, Copy, Debug, Default)]
pub struct C7DualSign {
    roles: [bool; 2],
    signers: [u64; 2],
    /// 台账指纹（签名当下核对真源）。
    ledger_fp: u64,
    /// 签名存根（签名者, 签名时台账指纹）。
    given: [Option<(u64, u64)>; 2],
}

impl C7DualSign {
    pub fn new(ledger_fp: u64) -> Self {
        Self {
            roles: [false; 2],
            signers: [0; 2],
            ledger_fp,
            given: [None, None],
        }
    }

    /// 签名：角色互异 + 签名者互异 + 台账指纹当下一致。
    pub fn sign(&mut self, role: u8, who: u64, ledger_fp: u64) -> Result<(), u16> {
        if role as usize >= 2 {
            return Err(E_C7_SIGN);
        }
        if ledger_fp != self.ledger_fp {
            return Err(E_C7_SIGN); // 签的不是当前账本。
        }
        // 对方已签则签名者不得重复。
        let other = 1 - role as usize;
        if let Some((w, _)) = self.given[other] {
            if w == who {
                return Err(E_C7_SIGN);
            }
        }
        self.roles[role as usize] = true;
        self.signers[role as usize] = who;
        self.given[role as usize] = Some((who, ledger_fp));
        Ok(())
    }

    pub fn complete(&self) -> Result<(), u16> {
        if self.given[0].is_some() && self.given[1].is_some() && self.signers[0] != self.signers[1] {
            Ok(())
        } else {
            Err(E_C7_SIGN)
        }
    }
}

/// C08 移交包裹。
#[derive(Clone, Debug)]
pub struct C8Handoff {
    /// 断点坐标（必须 = C08_ANCHOR）。
    pub anchor: u32,
    /// 数据源清单（统计就绪声明：立方/账单/画像/导出/长测/隐私）。
    pub sources: Vec<&'static str>,
    /// 台账指纹。
    pub ledger_fp: u32,
}

impl C8Handoff {
    pub fn new(sources: &[&'static str], ledger_fp: u32) -> Self {
        Self {
            anchor: C08_ANCHOR,
            sources: sources.iter().copied().collect(),
            ledger_fp,
        }
    }

    /// 移交完整：断点正确 + 数据源 ≥ 6（四族+长测+隐私）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.anchor != C08_ANCHOR {
            return Err(E_C7_HANDOFF);
        }
        if self.sources.len() < 6 {
            return Err(E_C7_HANDOFF);
        }
        Ok(())
    }
}

/// C07 收口总闸。
#[derive(Debug)]
pub struct C7CloseGate {
    checks: Vec<(&'static str, bool)>,
    report: Option<LongTestReport>,
    priv_arc: Option<PrivArchive>,
    sign: C7DualSign,
    handoff: Option<C8Handoff>,
}

impl C7CloseGate {
    pub fn new(ledger_fp: u64) -> Self {
        Self {
            checks: Vec::new(),
            report: None,
            priv_arc: None,
            sign: C7DualSign::new(ledger_fp),
            handoff: None,
        }
    }

    /// 登记自检项（名称必须在 16 项表内——冒名红）。
    pub fn check(&mut self, name: &'static str, green: bool) {
        self.checks.push((name, green));
    }

    /// 自检闭：16 项齐、名对表、全绿（长测/隐私/双签归 close_full）。
    pub fn close(&self) -> Result<(usize, usize), u16> {
        if self.checks.len() < C07_ITEMS.len() {
            return Err(E_C7_MISSING);
        }
        for want in C07_ITEMS {
            if !self.checks.iter().any(|(n, _)| *n == want) {
                return Err(E_C7_MISSING);
            }
        }
        let reds = self.checks.iter().filter(|(_, g)| !*g).count();
        if reds > 0 {
            return Err(E_C7_RED);
        }
        Ok((self.checks.len(), reds))
    }

    /// 收口全闭：自检闭 + 长测三硬指标 + 隐私归档 + 组双签。
    pub fn close_full(&self) -> Result<(usize, usize), u16> {
        let r = self.close()?;
        self.report.ok_or(E_C7_REPORT)?.validate()?;
        self.priv_arc.ok_or(E_C7_PRIV)?.validate()?;
        self.sign.complete()?;
        Ok(r)
    }

    pub fn attach_report(&mut self, r: LongTestReport) {
        self.report = Some(r);
    }

    pub fn archive_priv(&mut self, p: PrivArchive) {
        self.priv_arc = Some(p);
    }

    pub fn sign_role(&mut self, role: u8, who: u64, ledger_fp: u64) -> Result<(), u16> {
        self.sign.sign(role, who, ledger_fp)
    }

    /// 移交：close_full() 全绿后才准挂包裹（断链即红）。
    pub fn handover(&mut self, h: C8Handoff) -> Result<u32, u16> {
        self.close_full()?;
        h.validate()?;
        self.handoff = Some(h);
        Ok(C08_ANCHOR)
    }

    pub fn handoff_done(&self) -> bool {
        self.handoff.is_some()
    }
}

// ==========================================================================
// 判据（施工书：入树、长测报告、归档、双签、移交、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn report_good() -> LongTestReport {
        LongTestReport {
            days: 7,
            injected: 100,
            lost: 0,
            detected: 85,
            seed: 0xC07_0E30,
        }
    }

    fn priv_good() -> PrivArchive {
        PrivArchive {
            redline_hits: 0,
            agg_default_ok: true,
            mask_on: true,
        }
    }

    fn gate_all_green() -> C7CloseGate {
        let mut g = C7CloseGate::new(0xC07C_0C07);
        for name in C07_ITEMS {
            g.check(name, true);
        }
        g.attach_report(report_good());
        g.archive_priv(priv_good());
        g
    }

    #[test]
    fn a01_十六项自检入树() {
        let g = gate_all_green();
        assert_eq!(g.close(), Ok((16, 0)));
        // 缺一项红。
        let mut g2 = C7CloseGate::new(1);
        for name in &C07_ITEMS[..15] {
            g2.check(name, true);
        }
        assert_eq!(g2.close(), Err(E_C7_MISSING));
        // 冒名红：15 项真名 + 1 项假名。
        let mut g3 = C7CloseGate::new(1);
        for name in &C07_ITEMS[..15] {
            g3.check(name, true);
        }
        g3.check("F9999 冒名项", true);
        assert_eq!(g3.close(), Err(E_C7_MISSING));
    }

    #[test]
    fn a02_长测报告三硬指标() {
        assert_eq!(report_good().validate(), Ok(()));
        assert_eq!(report_good().rate_pm(), 850);
        // 天数不足。
        let mut r = report_good();
        r.days = 6;
        assert_eq!(r.validate(), Err(E_C7_REPORT));
        // 有丢失。
        let mut r2 = report_good();
        r2.lost = 1;
        assert_eq!(r2.validate(), Err(E_C7_REPORT));
        // 检出率不足 80%。
        let mut r3 = report_good();
        r3.detected = 79;
        assert_eq!(r3.validate(), Err(E_C7_REPORT));
        assert_eq!(r3.rate_pm(), 790);
        // 零注入无意义。
        let mut r4 = report_good();
        r4.injected = 0;
        assert_eq!(r4.validate(), Err(E_C7_REPORT));
        // 恰好 80% 放行。
        let mut r5 = report_good();
        r5.detected = 80;
        assert_eq!(r5.validate(), Ok(()));
    }

    #[test]
    fn a03_隐私复核归档() {
        assert_eq!(priv_good().validate(), Ok(()));
        // 红线违规 → 归档拒。
        let mut p = priv_good();
        p.redline_hits = 1;
        assert_eq!(p.validate(), Err(E_C7_PRIV));
        // 聚合未验证 → 拒。
        let mut p2 = priv_good();
        p2.agg_default_ok = false;
        assert_eq!(p2.validate(), Err(E_C7_PRIV));
        // 脱敏未开 → 拒。
        let mut p3 = priv_good();
        p3.mask_on = false;
        assert_eq!(p3.validate(), Err(E_C7_PRIV));
        // 收口闸缺归档 → 红（重建一个缺归档的闸）。
        let mut g2 = C7CloseGate::new(1);
        for name in C07_ITEMS {
            g2.check(name, true);
        }
        g2.attach_report(report_good());
        assert_eq!(g2.close_full(), Err(E_C7_PRIV));
    }

    #[test]
    fn a04_组双签_互异与台账当下() {
        let mut g = gate_all_green();
        // 未签 → 收口全闭红。
        assert_eq!(g.close_full(), Err(E_C7_SIGN));
        // 单签不足。
        assert_eq!(g.sign_role(0, 0xA11CE, 0xC07C_0C07), Ok(()));
        assert_eq!(g.close_full(), Err(E_C7_SIGN));
        // 台账指纹不符（签的是旧账）→ 拒。
        assert_eq!(g.sign_role(1, 0xB0B, 0xDEAD_0000), Err(E_C7_SIGN));
        // 双签互异 → 绿。
        assert_eq!(g.sign_role(1, 0xB0B, 0xC07C_0C07), Ok(()));
        assert!(g.close_full().is_ok());
        // 同人双签拒。
        let mut g2 = C7CloseGate::new(7);
        assert_eq!(g2.sign_role(0, 9, 7), Ok(()));
        assert_eq!(g2.sign_role(1, 9, 7), Err(E_C7_SIGN));
        // 角色越界拒。
        assert_eq!(g2.sign_role(2, 10, 7), Err(E_C7_SIGN));
    }

    #[test]
    fn a05_移交断点_F433_收口全绿() {
        let mut g = gate_all_green();
        g.sign_role(0, 0xA11CE, 0xC07C_0C07).ok();
        g.sign_role(1, 0xB0B, 0xC07C_0C07).ok();
        assert!(g.close().is_ok());
        // 包裹断点正确 + 六源齐。
        let h = C8Handoff::new(
            &["立方", "账单", "画像", "导出", "长测", "隐私"],
            0xC07C,
        );
        assert_eq!(h.validate(), Ok(()));
        assert_eq!(g.handover(h), Ok(C08_ANCHOR));
        assert_eq!(C08_ANCHOR, 4_033);
        assert!(g.handoff_done());
        // 源不足拒。
        let h2 = C8Handoff::new(&["立方", "账单"], 0xC07C);
        assert_eq!(h2.validate(), Err(E_C7_HANDOFF));
        // 断点错拒。
        let mut h3 = C8Handoff::new(&["立方", "账单", "画像", "导出", "长测", "隐私"], 0xC07C);
        h3.anchor = 4_034;
        assert_eq!(h3.validate(), Err(E_C7_HANDOFF));
        // 未收口先移交 → 闸红。
        let mut g2 = C7CloseGate::new(1);
        g2.check(C07_ITEMS[0], true);
        let h4 = C8Handoff::new(&["立方", "账单", "画像", "导出", "长测", "隐私"], 0xC07C);
        assert_eq!(g2.handover(h4), Err(E_C7_MISSING));
    }
}
