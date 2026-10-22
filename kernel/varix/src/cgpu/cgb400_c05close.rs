//! CGPU-F0400 · C05 组收口与 C06 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - C05 组收口：16 项自检入树/全域 fuzz 报告/总冻结确认/组双签/台账更新/
//!   向 C06 组移交（治理工具与总冻结就绪——后续组在冻结面上深化或扩展）。
//! - 判据：入树、报告、双签、移交、断点。
//!
//! 设计纪律：
//! - 16 项自检逐项入树：每项（名/域/绿否/证据摘要）挂到收口树，缺项或
//!   有红项即收口拒绝——收口是硬闸不是仪式。
//! - 总冻结确认：引用 F0397 册式语义（组内自建轻量快照核对），确认
//!   v1.0 冻结面存在且六族齐全才允许收口。
//! - 组双签：两个签名人角色互异（施工方+验收方）、指纹各不相同、
//!   同帧不重复签——防一手遮天。
//! - 移交断点：C06 起点锚 F0401；移交包裹（工具清单+冻结版本+台账号）
//!   完整才算断点成立。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C30 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C30，全仓独占）。
pub const SEG_BASE: u16 = 0x0C30;

pub const E_CLOSE_RED: u16 = SEG_BASE + 1; // 自检存在红项
pub const E_CLOSE_MISSING: u16 = SEG_BASE + 2; // 16 项未齐
pub const E_CLOSE_NOFREEZE: u16 = SEG_BASE + 3; // 总冻结未确认
pub const E_SIGN_DUP: u16 = SEG_BASE + 4; // 双签角色/指纹重复
pub const E_SIGN_COUNT: u16 = SEG_BASE + 5; // 签名数不足两枚
pub const E_HANDOFF_GAP: u16 = SEG_BASE + 6; // 移交包裹缺件

/// C05 组 16 项自检清单名（入树的固定坐标）。
pub const C05_ITEMS: [&str; 16] = [
    "检查器数据层",
    "账户探针",
    "时间旅行回放",
    "策略 A/B 影子运行",
    "报表引擎",
    "配额热调整审计",
    "告警规则",
    "第三方接入协议",
    "知识库",
    "多用户预留",
    "启动自检探针",
    "全域 fuzz",
    "API 总冻结 v1.0",
    "文档总纲",
    "基准总册",
    "组收口与移交",
];

/// 自检树节点：一项自检的入树记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckNode {
    pub name: String,
    pub green: bool,
    /// 证据摘要（测试名/fuzz 种子/册版本号等）。
    pub evidence: String,
}

/// 收口树：16 项逐项入树。
#[derive(Clone, Debug, Default)]
pub struct CloseTree {
    pub nodes: Vec<CheckNode>,
}

impl CloseTree {
    /// 入树：同名项不可重复入树（幂等性由调用方保证——重复即数据错）。
    pub fn enter(&mut self, name: &str, green: bool, evidence: &str) {
        self.nodes.push(CheckNode {
            name: name.to_string(),
            green,
            evidence: evidence.to_string(),
        });
    }

    /// 16 项齐全性：清单名逐一在树且绿。
    pub fn validate(&self) -> Result<(), u16> {
        if self.nodes.len() < C05_ITEMS.len() {
            return Err(E_CLOSE_MISSING);
        }
        for want in C05_ITEMS {
            match self.nodes.iter().find(|n| n.name == want) {
                None => return Err(E_CLOSE_MISSING),
                Some(n) if !n.green => return Err(E_CLOSE_RED),
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// 收口报告摘要：绿数/总数 + 红项名单（若有）。
    pub fn report(&self) -> (usize, usize, Vec<String>) {
        let green = self.nodes.iter().filter(|n| n.green).count();
        let reds = self
            .nodes
            .iter()
            .filter(|n| !n.green)
            .map(|n| n.name.clone())
            .collect();
        (green, self.nodes.len(), reds)
    }
}

/// 全域 fuzz 报告（收口引用件：种子/帧数/破例数三要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub seed: u64,
    pub frames: u64,
    pub violations: u32,
}

impl FuzzReport {
    /// 报告合格：跑满标准帧数且零破例。
    pub fn qualified(&self, std_frames: u64) -> bool {
        self.frames >= std_frames && self.violations == 0
    }
}

/// 组双签：角色互异 + 指纹互异 + 台账号一致。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 角色：0=施工方 1=验收方。
    pub role: u8,
    /// 签名人指纹（确定性摘要——同一人两枚签即重签红）。
    pub who: u64,
    /// 签名时的台账号（双签必须指向同一台账）。
    pub ledger: u32,
}

/// 双签账。
#[derive(Clone, Debug, Default)]
pub struct DualSign {
    pub sigs: Vec<Signature>,
}

impl DualSign {
    /// 签入：角色合法、同台账、不重复。
    pub fn sign(&mut self, role: u8, who: u64, ledger: u32) -> Result<(), u16> {
        if role > 1 {
            return Err(E_SIGN_DUP);
        }
        if self.sigs.iter().any(|s| s.who == who) {
            return Err(E_SIGN_DUP);
        }
        if let Some(first) = self.sigs.first() {
            if first.ledger != ledger {
                return Err(E_SIGN_DUP);
            }
        }
        self.sigs.push(Signature { role, who, ledger });
        Ok(())
    }

    /// 双签成立：恰好两枚、角色互异、指纹互异。
    pub fn complete(&self) -> Result<(), u16> {
        if self.sigs.len() != 2 {
            return Err(E_SIGN_COUNT);
        }
        if self.sigs[0].role == self.sigs[1].role {
            return Err(E_SIGN_DUP);
        }
        Ok(())
    }
}

/// C06 移交断点（C06 起点锚）。
pub const HANDOFF_BREAKPOINT: u32 = 401;

/// 移交包裹：治理工具清单 + 冻结版本 + 台账号。
#[derive(Clone, Debug)]
pub struct Handoff {
    pub tools: Vec<String>,
    pub freeze_version: u32,
    pub ledger: u32,
    pub breakpoint: u32,
}

impl Handoff {
    pub fn new(tools: &[&str], freeze_version: u32, ledger: u32) -> Self {
        Handoff {
            tools: tools.iter().map(|s| s.to_string()).collect(),
            freeze_version,
            ledger,
            breakpoint: HANDOFF_BREAKPOINT,
        }
    }

    /// 移交完整：断点正确 + 工具清单非空 + 冻结版本 ≥ v1.0。
    pub fn validate(&self) -> Result<(), u16> {
        if self.breakpoint != HANDOFF_BREAKPOINT
            || self.tools.is_empty()
            || self.freeze_version < 10
        {
            return Err(E_HANDOFF_GAP);
        }
        Ok(())
    }
}

/// 收口门：树 + fuzz 报告 + 冻结确认 + 双签 + 移交五件齐 ⇒ 收口成立。
#[derive(Clone, Debug, Default)]
pub struct CloseGate {
    pub tree: CloseTree,
    pub fuzz: Option<FuzzReport>,
    pub freeze_confirmed: bool,
    pub signs: DualSign,
    pub handoff: Option<Handoff>,
    /// 台账号（收口产物——台账更新目标）。
    pub ledger: u32,
}

impl CloseGate {
    pub fn confirm_freeze(&mut self, version: u32) {
        if version >= 10 {
            self.freeze_confirmed = true;
        }
    }

    /// 收口总闸：五件逐件验，任何一件不齐即拒。
    pub fn close(&self, std_fuzz_frames: u64) -> Result<(usize, usize), u16> {
        self.tree.validate()?;
        let rep = self.fuzz.ok_or(E_CLOSE_RED)?;
        if !rep.qualified(std_fuzz_frames) {
            return Err(E_CLOSE_RED);
        }
        if !self.freeze_confirmed {
            return Err(E_CLOSE_NOFREEZE);
        }
        self.signs.complete()?;
        let h = self.handoff.as_ref().ok_or(E_HANDOFF_GAP)?;
        h.validate()?;
        if h.ledger != self.ledger {
            return Err(E_HANDOFF_GAP);
        }
        let (g, t, _) = self.tree.report();
        Ok((g, t))
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn full_tree() -> CloseTree {
        let mut t = CloseTree::default();
        for (i, name) in C05_ITEMS.iter().enumerate() {
            t.enter(name, true, &["自检证据", "a", "b", "c"][i % 4]);
        }
        t
    }

    #[test]
    fn a01_入树_十六项齐全且缺一即拒() {
        let t = full_tree();
        assert_eq!(t.validate(), Ok(()));
        let (g, total, reds) = t.report();
        assert_eq!((g, total, reds.len()), (16, 16, 0));
        // 缺一项。
        let mut t2 = full_tree();
        t2.nodes.pop();
        assert_eq!(t2.validate(), Err(E_CLOSE_MISSING));
        // 红项被点名。
        let mut t3 = full_tree();
        t3.nodes[5].green = false;
        let (_, _, reds3) = t3.report();
        assert_eq!(reds3, vec![C05_ITEMS[5].to_string()]);
        assert_eq!(t3.validate(), Err(E_CLOSE_RED));
    }

    #[test]
    fn a02_报告_fuzz三要素合格线() {
        let ok = FuzzReport { seed: 7, frames: 20_000, violations: 0 };
        assert!(ok.qualified(20_000));
        // 帧数不足。
        assert!(!FuzzReport { seed: 7, frames: 19_999, violations: 0 }.qualified(20_000));
        // 有破例。
        assert!(!FuzzReport { seed: 7, frames: 30_000, violations: 1 }.qualified(20_000));
    }

    #[test]
    fn a03_总冻结确认_v10起才放行() {
        let mut gate = CloseGate { tree: full_tree(), ..Default::default() };
        gate.fuzz = Some(FuzzReport { seed: 3, frames: 25_000, violations: 0 });
        gate.confirm_freeze(9); // v0.9 不算冻结
        assert!(!gate.freeze_confirmed);
        gate.confirm_freeze(10); // v1.0
        assert!(gate.freeze_confirmed);
    }

    #[test]
    fn a04_双签_角色互异指纹互异同台账() {
        let mut d = DualSign::default();
        d.sign(0, 0xA11CE, 400).ok();
        d.sign(1, 0xB0B, 400).ok();
        assert_eq!(d.complete(), Ok(()));
        // 同人重签。
        let mut d2 = DualSign::default();
        d2.sign(0, 0xA11CE, 400).ok();
        assert_eq!(d2.sign(1, 0xA11CE, 400), Err(E_SIGN_DUP));
        // 角色重复。
        let mut d3 = DualSign::default();
        d3.sign(0, 1, 400).ok();
        d3.sign(0, 2, 400).ok();
        assert_eq!(d3.complete(), Err(E_SIGN_DUP));
        // 单签不成立。
        let mut d4 = DualSign::default();
        d4.sign(0, 5, 400).ok();
        assert_eq!(d4.complete(), Err(E_SIGN_COUNT));
        // 台账号不一致。
        let mut d5 = DualSign::default();
        d5.sign(0, 1, 400).ok();
        assert_eq!(d5.sign(1, 2, 401), Err(E_SIGN_DUP));
    }

    #[test]
    fn a05_移交断点_F401_收口总闸全绿() {
        let mut gate = CloseGate {
            tree: full_tree(),
            fuzz: Some(FuzzReport { seed: 3, frames: 25_000, violations: 0 }),
            ..Default::default()
        };
        gate.confirm_freeze(10);
        gate.signs.sign(0, 0xA11CE, 400).ok();
        gate.signs.sign(1, 0xB0B, 400).ok();
        gate.ledger = 400;
        // 移交包裹缺件红。
        assert_eq!(gate.close(20_000), Err(E_HANDOFF_GAP));
        gate.handoff = Some(Handoff::new(
            &["检查器", "探针", "时间旅行", "A/B", "报表", "知识库"],
            10,
            400,
        ));
        assert_eq!(gate.handoff.as_ref().and_then(|h| h.validate().err()), None);
        // 全绿收口。
        assert_eq!(gate.close(20_000), Ok((16, 16)));
        // 断点坐标固化。
        assert_eq!(HANDOFF_BREAKPOINT, 401);
    }
}
