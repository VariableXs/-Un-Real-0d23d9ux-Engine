//! VE-F3847 · 读屏与虚拟化
//!
//! 虚拟化语义全量保留 + 读屏触发渲染（≤100ms）+ 跳空 P0 + 滚动跟随。
//! 预算复用 6ms。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xC3xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC3Code(pub u16);

impl CC3Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SKIP_HOLE: Self = Self(SEG_BASE + 1);
    pub const TRIGGER_TIMEOUT: Self = Self(SEG_BASE + 2);
    pub const SEMANTIC_CLIPPED: Self = Self(SEG_BASE + 3);
    pub const SCROLL_LAG: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SKIP_HOLE => "跳空（P0）：读屏导航到未渲染项",
            Self::TRIGGER_TIMEOUT => "触发渲染超时（>100ms）",
            Self::SEMANTIC_CLIPPED => "语义被视口裁剪（P0）",
            Self::SCROLL_LAG => "滚动失随",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 虚拟项。
#[derive(Debug, Clone, Copy)]
pub struct VirtItem {
    pub id: u32,
    /// 语义是否保留（即使未渲染）。
    pub semantic_kept: bool,
    /// 是否已渲染像素。
    pub rendered: bool,
}

/// 触发渲染样本。
#[derive(Debug, Clone, Copy)]
pub struct TriggerSample {
    pub item_id: u32,
    pub latency_ms: u32,
    /// 是否滚动跟随。
    pub scroll_followed: bool,
}

impl TriggerSample {
    pub fn within_budget(&self) -> bool {
        self.latency_ms <= 100
    }
}

/// 虚拟化协同器。
#[derive(Debug, Clone)]
pub struct VirtA11y {
    pub items: Vec<VirtItem>,
    pub triggers: Vec<TriggerSample>,
    /// 帧预算（ms，复用 6ms）。
    pub frame_budget_ms: u32,
    /// 本帧触发渲染耗时。
    pub trigger_cost_ms: u32,
}

impl VirtA11y {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            triggers: Vec::new(),
            frame_budget_ms: 6,
            trigger_cost_ms: 0,
        }
    }

    /// 语义全量：全部 item.semantic_kept。
    pub fn semantics_full(&self) -> bool {
        self.items.iter().all(|i| i.semantic_kept)
    }

    /// 跳空扫描：存在 semantic_kept 但导航目标未渲染且未触发。
    pub fn skip_hole(&self) -> bool {
        for it in self.items.iter() {
            if it.semantic_kept && !it.rendered {
                let triggered = self.triggers.iter().any(|t| t.item_id == it.id);
                if !triggered {
                    return true;
                }
            }
        }
        false
    }

    /// 触发延迟。
    pub fn triggers_within_budget(&self) -> bool {
        self.triggers.iter().all(|t| t.within_budget())
    }

    /// 滚动跟随。
    pub fn scroll_intact(&self) -> bool {
        self.triggers.iter().all(|t| t.scroll_followed)
    }

    /// 帧预算。
    pub fn within_frame_budget(&self) -> bool {
        self.trigger_cost_ms <= self.frame_budget_ms
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC3Code> {
        if !self.semantics_full() {
            return Err(CC3Code::SEMANTIC_CLIPPED);
        }
        if self.skip_hole() {
            return Err(CC3Code::SKIP_HOLE);
        }
        if !self.scroll_intact() {
            return Err(CC3Code::SCROLL_LAG);
        }
        if !self.triggers_within_budget() {
            return Err(CC3Code::TRIGGER_TIMEOUT);
        }
        Ok(())
    }
}

impl Default for VirtA11y {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves147_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves147_virta11y");

    fn ready() -> VirtA11y {
        let mut v = VirtA11y::new();
        v.items.push(VirtItem { id: 1, semantic_kept: true, rendered: true });
        v.items.push(VirtItem { id: 2, semantic_kept: true, rendered: true });
        v.triggers.push(TriggerSample { item_id: 1, latency_ms: 50, scroll_followed: true });
        v.trigger_cost_ms = 3;
        v
    }

    // 01 语义全量
    {
        let v = ready();
        chk(&mut set, "C3847-01", v.semantics_full(), "err");
    }
    // 02 语义裁剪 P0
    {
        let mut v = ready();
        v.items[0].semantic_kept = false;
        chk(
            &mut set,
            "C3847-02",
            v.full_check() == Err(CC3Code::SEMANTIC_CLIPPED),
            "err",
        );
    }
    // 03 无跳空
    {
        let v = ready();
        chk(&mut set, "C3847-03", !v.skip_hole(), "err");
    }
    // 04 跳空 P0
    {
        let mut v = ready();
        v.items.push(VirtItem { id: 3, semantic_kept: true, rendered: false });
        chk(
            &mut set,
            "C3847-04",
            v.full_check() == Err(CC3Code::SKIP_HOLE),
            "err",
        );
    }
    // 05 触发 100ms
    {
        let v = ready();
        chk(&mut set, "C3847-05", v.triggers_within_budget(), "err");
    }
    // 06 触发超时
    {
        let mut v = ready();
        v.triggers[0].latency_ms = 150;
        chk(
            &mut set,
            "C3847-06",
            v.full_check() == Err(CC3Code::TRIGGER_TIMEOUT),
            "err",
        );
    }
    // 07 滚动失随
    {
        let mut v = ready();
        v.triggers[0].scroll_followed = false;
        chk(
            &mut set,
            "C3847-07",
            v.full_check() == Err(CC3Code::SCROLL_LAG),
            "err",
        );
    }
    // 08 帧预算
    {
        let v = ready();
        chk(&mut set, "C3847-08", v.within_frame_budget(), "err");
    }
    // 09 帧预算破
    {
        let mut v = ready();
        v.trigger_cost_ms = 10;
        chk(&mut set, "C3847-09", !v.within_frame_budget(), "err");
    }
    // 10 全检通过
    {
        let v = ready();
        chk(&mut set, "C3847-10", v.full_check().is_ok(), "err");
    }
    // 11 触发补跳空
    {
        let mut v = ready();
        v.items.push(VirtItem { id: 3, semantic_kept: true, rendered: false });
        v.triggers.push(TriggerSample { item_id: 3, latency_ms: 80, scroll_followed: true });
        chk(&mut set, "C3847-11", !v.skip_hole(), "err");
    }
    // 12 空集
    {
        let v = VirtA11y::new();
        chk(&mut set, "C3847-12", v.full_check().is_ok() && !v.skip_hole(), "err");
    }

    set
}
