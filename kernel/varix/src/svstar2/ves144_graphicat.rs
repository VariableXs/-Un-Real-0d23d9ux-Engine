//! VE-F3844 · 图形读屏替代
//!
//! 装饰图形 aria-hidden 跳过（噪音红线）+ 信息图形替代文本 + 替代渲染顺序。
//! 同帧复用。装饰标注覆盖率。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码段（0xC0xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC0Code(pub u16);

impl CC0Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const NOISE_LEAK: Self = Self(SEG_BASE + 1);
    pub const ALT_MISSING: Self = Self(SEG_BASE + 2);
    pub const ORDER_BROKEN: Self = Self(SEG_BASE + 3);
    pub const SYNC_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NOISE_LEAK => "噪音红线：装饰图形未标注跳过",
            Self::ALT_MISSING => "替代文本缺失",
            Self::ORDER_BROKEN => "读序乱（顺序红线）",
            Self::SYNC_BROKEN => "同帧同步破（P0）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 图形类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicKind {
    /// 装饰。
    Decorative,
    /// 信息。
    Informative,
    /// 动态。
    Dynamic,
}

impl GraphicKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Decorative => "装饰",
            Self::Informative => "信息",
            Self::Dynamic => "动态",
        }
    }
}

/// 图形条目。
#[derive(Debug, Clone)]
pub struct GraphicEntry {
    /// 节点 id。
    pub node_id: u32,
    /// 类型。
    pub kind: GraphicKind,
    /// aria-hidden（装饰应为 true）。
    pub aria_hidden: bool,
    /// 替代文本（信息图形必须非空）。
    pub alt_text: String,
    /// 读序位置。
    pub order: u32,
    /// 标注帧。
    pub frame: u64,
    /// 渲染帧。
    pub render_frame: u64,
}

impl GraphicEntry {
    /// 装饰应跳过。
    pub fn decorative_ok(&self) -> bool {
        self.kind != GraphicKind::Decorative || self.aria_hidden
    }

    /// 信息应有替代。
    pub fn informative_ok(&self) -> bool {
        self.kind != GraphicKind::Informative || !self.alt_text.is_empty()
    }

    /// 同帧。
    pub fn sync_ok(&self) -> bool {
        self.frame == self.render_frame
    }
}

/// 图形替代器。
#[derive(Debug, Clone)]
pub struct GraphicAltRenderer {
    pub entries: Vec<GraphicEntry>,
}

impl GraphicAltRenderer {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 噪音红线：装饰必须 aria-hidden。
    pub fn noise_ok(&self) -> bool {
        self.entries.iter().all(|e| e.decorative_ok())
    }

    /// 替代单源：信息图形 alt 非空。
    pub fn alt_ok(&self) -> bool {
        self.entries.iter().all(|e| e.informative_ok())
    }

    /// 读序单调。
    pub fn order_ok(&self) -> bool {
        let mut prev = 0u32;
        for e in self.entries.iter() {
            if e.order < prev {
                return false;
            }
            prev = e.order;
        }
        true
    }

    /// 同帧。
    pub fn sync_ok(&self) -> bool {
        self.entries.iter().all(|e| e.sync_ok())
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC0Code> {
        if !self.noise_ok() {
            return Err(CC0Code::NOISE_LEAK);
        }
        if !self.alt_ok() {
            return Err(CC0Code::ALT_MISSING);
        }
        if !self.order_ok() {
            return Err(CC0Code::ORDER_BROKEN);
        }
        if !self.sync_ok() {
            return Err(CC0Code::SYNC_BROKEN);
        }
        Ok(())
    }

    /// 装饰标注覆盖率 ×100。
    pub fn decorative_coverage_pct(&self) -> u32 {
        let total = self.entries.iter().filter(|e| e.kind == GraphicKind::Decorative).count();
        if total == 0 {
            return 100;
        }
        let ok = self
            .entries
            .iter()
            .filter(|e| e.kind == GraphicKind::Decorative && e.aria_hidden)
            .count();
        ok as u32 * 100 / total as u32
    }
}

impl Default for GraphicAltRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves144_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves144_graphicat");

    fn ready() -> GraphicAltRenderer {
        let mut r = GraphicAltRenderer::new();
        r.entries.push(GraphicEntry {
            node_id: 1,
            kind: GraphicKind::Decorative,
            aria_hidden: true,
            alt_text: String::new(),
            order: 1,
            frame: 5,
            render_frame: 5,
        });
        r.entries.push(GraphicEntry {
            node_id: 2,
            kind: GraphicKind::Informative,
            aria_hidden: false,
            alt_text: String::from("图表：季度营收"),
            order: 2,
            frame: 5,
            render_frame: 5,
        });
        r
    }

    // 01 装饰跳过
    {
        let r = ready();
        chk(&mut set, "C3844-01", r.noise_ok(), "err");
    }
    // 02 噪音红线
    {
        let mut r = ready();
        r.entries[0].aria_hidden = false;
        chk(
            &mut set,
            "C3844-02",
            r.full_check() == Err(CC0Code::NOISE_LEAK),
            "err",
        );
    }
    // 03 替代齐
    {
        let r = ready();
        chk(&mut set, "C3844-03", r.alt_ok(), "err");
    }
    // 04 替代缺
    {
        let mut r = ready();
        r.entries[1].alt_text = String::new();
        chk(
            &mut set,
            "C3844-04",
            r.full_check() == Err(CC0Code::ALT_MISSING),
            "err",
        );
    }
    // 05 读序单调
    {
        let r = ready();
        chk(&mut set, "C3844-05", r.order_ok(), "err");
    }
    // 06 读序乱
    {
        let mut r = ready();
        r.entries[1].order = 0;
        chk(
            &mut set,
            "C3844-06",
            r.full_check() == Err(CC0Code::ORDER_BROKEN),
            "err",
        );
    }
    // 07 同帧
    {
        let r = ready();
        chk(&mut set, "C3844-07", r.sync_ok(), "err");
    }
    // 08 同步破
    {
        let mut r = ready();
        r.entries[0].frame = 6;
        chk(
            &mut set,
            "C3844-08",
            r.full_check() == Err(CC0Code::SYNC_BROKEN),
            "err",
        );
    }
    // 09 全检通过
    {
        let r = ready();
        chk(&mut set, "C3844-09", r.full_check().is_ok(), "err");
    }
    // 10 装饰覆盖 100
    {
        let r = ready();
        chk(&mut set, "C3844-10", r.decorative_coverage_pct() == 100, "err");
    }
    // 11 类型命名
    {
        chk(
            &mut set,
            "C3844-11",
            GraphicKind::Decorative.name() == "装饰"
                && GraphicKind::Informative.name() == "信息",
            "err",
        );
    }
    // 12 空集
    {
        let r = GraphicAltRenderer::new();
        chk(&mut set, "C3844-12", r.full_check().is_ok() && r.noise_ok(), "err");
    }

    set
}
