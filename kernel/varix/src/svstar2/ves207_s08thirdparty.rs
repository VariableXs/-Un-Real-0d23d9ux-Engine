#![allow(dead_code)]
//! VE-F3953 · 第三方无障碍工具接入
//!
//! 外部无障碍工具（第三方读屏/审计工具）数据交换协议——协议全域三态
//! （声明/协商/生效）。同待遇全域化：外部工具产物过完整链（不豁免任何
//! 一环）；集成清单全域化；双签全域化（内部 + 外部两签）。
//! 错误路径：绕检→P0；矩阵缺→补；失配→协商；漂移→对账。
//! 性能：协议 O(三态)；同待遇 O(全域断言)；清单 O(全域)；双签 O(全域)。
//! 跨批对接：F3352/F3305 单源全域复用声明；F3307 隔离对端；外部工具对端。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x10C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x10C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8MCode(pub u16);

impl S8MCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BYPASS: Self = Self(SEG_BASE + 1);
    pub const MATRIX_GAP: Self = Self(SEG_BASE + 2);
    pub const MISMATCH: Self = Self(SEG_BASE + 3);
    pub const DRIFT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BYPASS => "绕检→P0",
            Self::MATRIX_GAP => "矩阵缺→补",
            Self::MISMATCH => "失配→协商",
            Self::DRIFT => "漂移→对账",
            _ => "未知",
        }
    }
}

// ── 协议全域三态 ─────────────────────────────────────────────────────

/// 协议状态三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtoState {
    /// 已声明（登记协议版本）。
    Declared,
    /// 协商中（版本失配协商）。
    Negotiating,
    /// 已生效（可交换数据）。
    Live,
}

/// 第三方工具协议条目。
#[derive(Debug, Clone)]
pub struct ProtoEntry {
    pub tool: &'static str,
    /// 外部工具协议版本。
    pub ext_ver: u32,
    /// 本域协议版本。
    pub own_ver: u32,
    pub state: ProtoState,
}

/// 协议枢纽：三态机管理。
#[derive(Debug, Clone)]
pub struct ProtoHub {
    pub entries: Vec<ProtoEntry>,
}

impl ProtoHub {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 声明：登记即进入三态机；版本一致 → 生效，失配 → 协商中。
    pub fn declare(&mut self, tool: &'static str, ext_ver: u32, own_ver: u32) -> ProtoState {
        let state = if ext_ver == own_ver { ProtoState::Live } else { ProtoState::Negotiating };
        self.entries.push(ProtoEntry { tool, ext_ver, own_ver, state });
        state
    }

    /// 协商收敛：外部对齐到本域版本 → 生效。
    pub fn settle(&mut self, tool: &str, ext_ver: u32) -> S8MCode {
        match self.entries.iter_mut().find(|e| e.tool == tool) {
            Some(e) => {
                e.ext_ver = ext_ver;
                if e.ext_ver == e.own_ver {
                    e.state = ProtoState::Live;
                    S8MCode::OK
                } else {
                    S8MCode::MISMATCH
                }
            }
            None => S8MCode::MISMATCH,
        }
    }

    /// 生效清单。
    pub fn live_tools(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| e.state == ProtoState::Live).map(|e| e.tool).collect()
    }
}

impl Default for ProtoHub {
    fn default() -> Self {
        Self::new()
    }
}

// ── 同待遇（外部产物过完整链）────────────────────────────────────────

/// 完整链环节（外部产物必须逐环过检）。
pub const FULL_CHAIN: [&str; 4] = ["解析", "校验", "审计", "入账"];

/// 产物过链记录。
#[derive(Debug, Clone)]
pub struct ChainRec {
    pub artifact: &'static str,
    /// 来源是否第三方。
    pub external: bool,
    /// 已过环节。
    pub stages: Vec<&'static str>,
}

/// 同待遇审计：外部产物与内部产物过同一完整链。
#[derive(Debug, Clone)]
pub struct EqualChain {
    pub recs: Vec<ChainRec>,
}

impl EqualChain {
    pub fn new() -> Self {
        Self { recs: Vec::new() }
    }

    pub fn receive(&mut self, artifact: &'static str, external: bool) {
        self.recs.push(ChainRec { artifact, external, stages: Vec::new() });
    }

    /// 过环：环节序必须按 FULL_CHAIN 顺序推进。
    pub fn advance(&mut self, artifact: &str, stage: &'static str) -> S8MCode {
        let idx = match FULL_CHAIN.iter().position(|s| *s == stage) {
            Some(i) => i,
            None => return S8MCode::BYPASS,
        };
        match self.recs.iter_mut().find(|r| r.artifact == artifact) {
            Some(r) => {
                if r.stages.len() != idx {
                    // 跳环 = 绕检。
                    return S8MCode::BYPASS;
                }
                r.stages.push(stage);
                S8MCode::OK
            }
            None => S8MCode::BYPASS,
        }
    }

    /// 绕检 P0 检出：未过完整链的产物。
    pub fn bypassed(&self) -> Vec<&'static str> {
        self.recs
            .iter()
            .filter(|r| r.stages.len() < FULL_CHAIN.len())
            .map(|r| r.artifact)
            .collect()
    }

    /// 同待遇断言：外部产物过链环数 ≥ 内部产物过链环数（不豁免）。
    pub fn equal_treatment(&self) -> bool {
        let int_min = self.recs.iter().filter(|r| !r.external).map(|r| r.stages.len()).min();
        let ext_min = self.recs.iter().filter(|r| r.external).map(|r| r.stages.len()).min();
        match (int_min, ext_min) {
            (Some(i), Some(e)) => e >= i,
            _ => true,
        }
    }
}

impl Default for EqualChain {
    fn default() -> Self {
        Self::new()
    }
}

// ── 集成清单 + 双签 ──────────────────────────────────────────────────

/// 集成清单条目：工具 × 能力 × 双签。
#[derive(Debug, Clone)]
pub struct IntegrItem {
    pub tool: &'static str,
    /// 集成能力（读屏数据/审计报告…）。
    pub capability: &'static str,
    /// 内签。
    pub signed_in: bool,
    /// 外签。
    pub signed_out: bool,
}

impl IntegrItem {
    pub fn dual_signed(&self) -> bool {
        self.signed_in && self.signed_out
    }
}

/// 集成清单。
#[derive(Debug, Clone)]
pub struct IntegrBook {
    pub items: Vec<IntegrItem>,
}

impl IntegrBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, i: IntegrItem) {
        self.items.push(i);
    }

    /// 双签全域断言：全部条目双签齐备。
    pub fn all_dual_signed(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.dual_signed())
    }

    /// 缺签名单。
    pub fn unsigned(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| !i.dual_signed()).map(|i| i.tool).collect()
    }
}

impl Default for IntegrBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 漂移对账 ─────────────────────────────────────────────────────────

/// 协议漂移对账：登记版本 vs 实际交换版本。
#[derive(Debug, Clone)]
pub struct DriftRec {
    pub tool: &'static str,
    pub declared_ver: u32,
    pub actual_ver: u32,
}

pub fn drift_tools(recs: &[DriftRec]) -> Vec<&'static str> {
    recs.iter().filter(|r| r.declared_ver != r.actual_ver).map(|r| r.tool).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves207_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves207_s08thirdparty");

    // 01 协议三态：版本一致直接生效
    {
        let mut h = ProtoHub::new();
        let st = h.declare("读屏甲", 3, 3);
        chk(&mut set, "C3953-01", st == ProtoState::Live && h.live_tools() == vec!["读屏甲"], "err");
    }
    // 02 版本失配→协商中
    {
        let mut h = ProtoHub::new();
        let st = h.declare("审计乙", 2, 3);
        chk(&mut set, "C3953-02", st == ProtoState::Negotiating && h.live_tools().is_empty(), "err");
    }
    // 03 协商收敛→生效
    {
        let mut h = ProtoHub::new();
        h.declare("审计乙", 2, 3);
        chk(&mut set, "C3953-03", h.settle("审计乙", 3) == S8MCode::OK && h.live_tools() == vec!["审计乙"], "err");
    }
    // 04 协商仍失配
    {
        let mut h = ProtoHub::new();
        h.declare("审计乙", 2, 3);
        chk(&mut set, "C3953-04", h.settle("审计乙", 2) == S8MCode::MISMATCH, "err");
    }
    // 05 同待遇：外部产物过完整链
    {
        let mut c = EqualChain::new();
        c.receive("外部报告", true);
        let mut ok = true;
        for s in FULL_CHAIN.iter() {
            ok &= c.advance("外部报告", s) == S8MCode::OK;
        }
        chk(&mut set, "C3953-05", ok && c.bypassed().is_empty() && c.equal_treatment(), "err");
    }
    // 06 绕检→P0：跳环拒收
    {
        let mut c = EqualChain::new();
        c.receive("内部数据", false);
        c.advance("内部数据", "解析");
        chk(&mut set, "C3953-06", c.advance("内部数据", "审计") == S8MCode::BYPASS && c.bypassed() == vec!["内部数据"], "err");
    }
    // 07 未登记产物过链拒收
    {
        let mut c = EqualChain::new();
        chk(&mut set, "C3953-07", c.advance("幽灵", "解析") == S8MCode::BYPASS, "err");
    }
    // 08 未知环节拒收
    {
        let mut c = EqualChain::new();
        c.receive("x", false);
        chk(&mut set, "C3953-08", c.advance("x", "黑环节") == S8MCode::BYPASS, "err");
    }
    // 09 同待遇断言：外部不得少环
    {
        let mut c = EqualChain::new();
        c.receive("内", false);
        for s in FULL_CHAIN.iter() {
            c.advance("内", s);
        }
        c.receive("外", true);
        c.advance("外", "解析");
        chk(&mut set, "C3953-09", !c.equal_treatment(), "err");
    }
    // 10 双签全域断言
    {
        let mut b = IntegrBook::new();
        b.add(IntegrItem { tool: "甲", capability: "读屏数据", signed_in: true, signed_out: true });
        b.add(IntegrItem { tool: "乙", capability: "审计报告", signed_in: true, signed_out: false });
        chk(&mut set, "C3953-10", !b.all_dual_signed() && b.unsigned() == vec!["乙"], "err");
    }
    // 11 补签后闭环
    {
        let mut b = IntegrBook::new();
        b.add(IntegrItem { tool: "乙", capability: "审计报告", signed_in: true, signed_out: false });
        b.items[0].signed_out = true;
        chk(&mut set, "C3953-11", b.all_dual_signed() && b.unsigned().is_empty(), "err");
    }
    // 12 漂移对账检出
    {
        let recs = vec![
            DriftRec { tool: "甲", declared_ver: 3, actual_ver: 3 },
            DriftRec { tool: "乙", declared_ver: 2, actual_ver: 4 },
        ];
        chk(&mut set, "C3953-12", drift_tools(&recs) == vec!["乙"], "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3953-13", S8MCode::DRIFT.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8MCode::OK, S8MCode::BYPASS, S8MCode::MATRIX_GAP, S8MCode::MISMATCH, S8MCode::DRIFT];
        chk(&mut set, "C3953-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_thirdparty_all_green() {
        let s = run_ves207_checks();
        assert!(s.all_passed(), "ves207 red: {}", s.red_items_text());
    }

    #[test]
    fn test_proto_three_states() {
        let mut h = ProtoHub::new();
        assert_eq!(h.declare("t1", 1, 1), ProtoState::Live);
        assert_eq!(h.declare("t2", 1, 2), ProtoState::Negotiating);
        assert_eq!(h.live_tools(), vec!["t1"]);
    }

    #[test]
    fn test_chain_strict_order() {
        let mut c = EqualChain::new();
        c.receive("a", true);
        assert_eq!(c.advance("a", "校验"), S8MCode::BYPASS);
        assert_eq!(c.advance("a", "解析"), S8MCode::OK);
        assert_eq!(c.advance("a", "校验"), S8MCode::OK);
    }
}
