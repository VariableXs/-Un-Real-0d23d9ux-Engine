//! VE-F3841 · 读屏协同总架构
//!
//! 语义事件→渲染反馈→读屏消费三向闭环。七域映射。N08 事实源单源。
//! 断流红线 P0：链路任何一环缺失 = 读屏用户信息断流。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xBDxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBD00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBDCode(pub u16);

impl CBDCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const LOOP_BROKEN: Self = Self(SEG_BASE + 1);
    pub const DOMAIN_MISSING: Self = Self(SEG_BASE + 2);
    pub const SOURCE_FORK: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LOOP_BROKEN => "闭环断流（P0）：读屏用户信息断流",
            Self::DOMAIN_MISSING => "七域映射缺域，需补齐",
            Self::SOURCE_FORK => "事实源分叉，需归一",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 七域协同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11yDomain {
    /// 焦点。
    Focus,
    /// 文本。
    Text,
    /// 图形。
    Graphic,
    /// 动态。
    Dynamic,
    /// 导航。
    Navigate,
    /// 虚拟化。
    Virtual,
    /// 表面。
    Surface,
}

impl A11yDomain {
    pub const ALL: [A11yDomain; 7] = [
        A11yDomain::Focus,
        A11yDomain::Text,
        A11yDomain::Graphic,
        A11yDomain::Dynamic,
        A11yDomain::Navigate,
        A11yDomain::Virtual,
        A11yDomain::Surface,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Focus => "焦点",
            Self::Text => "文本",
            Self::Graphic => "图形",
            Self::Dynamic => "动态",
            Self::Navigate => "导航",
            Self::Virtual => "虚拟化",
            Self::Surface => "表面",
        }
    }
}

/// 三向闭环节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStage {
    /// 语义事件。
    SemanticEvent,
    /// 渲染反馈。
    RenderFeedback,
    /// 读屏消费。
    ScreenReaderConsume,
}

impl LoopStage {
    pub const ALL: [LoopStage; 3] = [
        LoopStage::SemanticEvent,
        LoopStage::RenderFeedback,
        LoopStage::ScreenReaderConsume,
    ];

    pub fn order(self) -> u8 {
        match self {
            Self::SemanticEvent => 0,
            Self::RenderFeedback => 1,
            Self::ScreenReaderConsume => 2,
        }
    }
}

/// 闭环链路追踪。
#[derive(Debug, Clone)]
pub struct LoopTrace {
    /// 各阶段是否送达。
    pub delivered: [bool; 3],
    /// 域。
    pub domain: A11yDomain,
}

impl LoopTrace {
    pub fn new(domain: A11yDomain) -> Self {
        Self {
            delivered: [false; 3],
            domain,
        }
    }

    /// 断流检出：任一环缺失。
    pub fn broken(&self) -> bool {
        !self.delivered[0] || !self.delivered[1] || !self.delivered[2]
    }

    /// 送达一环。
    pub fn deliver(&mut self, stage: LoopStage) {
        self.delivered[stage.order() as usize] = true;
    }

    /// 链路完整。
    pub fn complete(&self) -> bool {
        !self.broken()
    }
}

/// 事实源声明（N08 为真源）。
#[derive(Debug, Clone)]
pub struct SourceDeclaration {
    /// 真源标识。
    pub source_id: &'static str,
    /// 是否单源。
    pub single_source: bool,
    /// 分叉检出。
    pub fork_detected: bool,
}

impl SourceDeclaration {
    pub fn n08() -> Self {
        Self {
            source_id: "N08-semantic-tree",
            single_source: true,
            fork_detected: false,
        }
    }

    pub fn assert_single(&self) -> Result<(), CBDCode> {
        if self.fork_detected || !self.single_source {
            return Err(CBDCode::SOURCE_FORK);
        }
        Ok(())
    }
}

/// 协同架构。
#[derive(Debug, Clone)]
pub struct ScreenReaderBridge {
    /// 链路追踪（每域一条）。
    pub traces: Vec<LoopTrace>,
    /// 七域映射。
    pub mapped: Vec<A11yDomain>,
    /// 事实源。
    pub source: SourceDeclaration,
}

impl ScreenReaderBridge {
    pub fn new() -> Self {
        Self {
            traces: Vec::new(),
            mapped: Vec::new(),
            source: SourceDeclaration::n08(),
        }
    }

    /// 注册域。
    pub fn map_domain(&mut self, d: A11yDomain) {
        if !self.mapped.contains(&d) {
            self.mapped.push(d);
        }
    }

    /// 七域齐备。
    pub fn domains_complete(&self) -> bool {
        A11yDomain::ALL.iter().all(|d| self.mapped.contains(d))
    }

    /// 开一条链路。
    pub fn open_trace(&mut self, d: A11yDomain) {
        self.traces.push(LoopTrace::new(d));
    }

    /// 断流扫描：任一链路断 = P0。
    pub fn any_broken(&self) -> bool {
        self.traces.iter().any(|t| t.broken())
    }

    /// 全断流检出 → P0 语义。
    pub fn assert_no_break(&self) -> Result<(), CBDCode> {
        if self.any_broken() {
            return Err(CBDCode::LOOP_BROKEN);
        }
        Ok(())
    }

    /// 映射缺域检出。
    pub fn assert_mapped(&self) -> Result<(), CBDCode> {
        if !self.domains_complete() {
            return Err(CBDCode::DOMAIN_MISSING);
        }
        Ok(())
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CBDCode> {
        self.source.assert_single()?;
        self.assert_mapped()?;
        self.assert_no_break()?;
        Ok(())
    }

    /// 送达（便捷）。
    pub fn deliver(&mut self, idx: usize, stage: LoopStage) {
        if idx < self.traces.len() {
            self.traces[idx].deliver(stage);
        }
    }
}

impl Default for ScreenReaderBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves141_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves141_srbridge");

    fn ready() -> ScreenReaderBridge {
        let mut b = ScreenReaderBridge::new();
        for d in A11yDomain::ALL.iter() {
            b.map_domain(*d);
        }
        b.open_trace(A11yDomain::Focus);
        b.deliver(0, LoopStage::SemanticEvent);
        b.deliver(0, LoopStage::RenderFeedback);
        b.deliver(0, LoopStage::ScreenReaderConsume);
        b
    }

    // 01 三向闭环完整
    {
        let b = ready();
        chk(&mut set, "C3841-01", b.assert_no_break().is_ok(), "err");
    }
    // 02 断流 P0
    {
        let mut b = ready();
        b.traces[0].delivered[1] = false;
        chk(
            &mut set,
            "C3841-02",
            b.assert_no_break() == Err(CBDCode::LOOP_BROKEN),
            "err",
        );
    }
    // 03 断流扫描
    {
        let mut b = ready();
        b.open_trace(A11yDomain::Text);
        chk(&mut set, "C3841-03", b.any_broken(), "err");
    }
    // 04 七域齐备
    {
        let b = ready();
        chk(&mut set, "C3841-04", b.domains_complete(), "err");
    }
    // 05 缺域
    {
        let mut b = ready();
        b.mapped.clear();
        b.map_domain(A11yDomain::Focus);
        chk(
            &mut set,
            "C3841-05",
            b.assert_mapped() == Err(CBDCode::DOMAIN_MISSING),
            "err",
        );
    }
    // 06 事实源单源
    {
        let b = ready();
        chk(&mut set, "C3841-06", b.source.assert_single().is_ok(), "err");
    }
    // 07 事实源分叉
    {
        let mut b = ready();
        b.source.fork_detected = true;
        chk(
            &mut set,
            "C3841-07",
            b.full_check() == Err(CBDCode::SOURCE_FORK),
            "err",
        );
    }
    // 08 全检通过
    {
        let b = ready();
        chk(&mut set, "C3841-08", b.full_check().is_ok(), "err");
    }
    // 09 阶段序
    {
        let ok = LoopStage::SemanticEvent.order() == 0
            && LoopStage::RenderFeedback.order() == 1
            && LoopStage::ScreenReaderConsume.order() == 2;
        chk(&mut set, "C3841-09", ok, "err");
    }
    // 10 送达语义
    {
        let mut t = LoopTrace::new(A11yDomain::Dynamic);
        t.deliver(LoopStage::SemanticEvent);
        t.deliver(LoopStage::RenderFeedback);
        t.deliver(LoopStage::ScreenReaderConsume);
        chk(&mut set, "C3841-10", t.complete() && !t.broken(), "err");
    }
    // 11 域去重
    {
        let mut b = ScreenReaderBridge::new();
        b.map_domain(A11yDomain::Focus);
        b.map_domain(A11yDomain::Focus);
        chk(&mut set, "C3841-11", b.mapped.len() == 1, "err");
    }
    // 12 N08 真源标识
    {
        let s = SourceDeclaration::n08();
        chk(&mut set, "C3841-12", s.source_id == "N08-semantic-tree" && s.single_source, "err");
    }

    set
}
