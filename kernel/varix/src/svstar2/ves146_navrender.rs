//! VE-F3846 · 读屏导航渲染
//!
//! 快捷导航跟随 + 地标边界可视化 + 导航序一致 + 跳转 200ms 预算。
//! 地标缺失 = 导航失效。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xC2xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC2Code(pub u16);

impl CC2Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const LANDMARK_MISSING: Self = Self(SEG_BASE + 1);
    pub const ORDER_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const JUMP_TIMEOUT: Self = Self(SEG_BASE + 3);
    pub const FOLLOW_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LANDMARK_MISSING => "地标缺失（P1）：导航失效",
            Self::ORDER_MISMATCH => "导航序与视觉序不一致",
            Self::JUMP_TIMEOUT => "跳转超时（>200ms）",
            Self::FOLLOW_BROKEN => "跟随断链（P0）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 导航类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    /// 标题跳转。
    Heading,
    /// 地标跳转。
    Landmark,
    /// 链接列表。
    LinkList,
}

impl NavKind {
    pub const ALL: [NavKind; 3] = [NavKind::Heading, NavKind::Landmark, NavKind::LinkList];
}

/// 地标。
#[derive(Debug, Clone, Copy)]
pub struct Landmark {
    pub id: u32,
    /// 是否有视觉边界（开发者模式）。
    pub boundary_rendered: bool,
    /// 导航序。
    pub order: u32,
}

/// 跳转实测。
#[derive(Debug, Clone, Copy)]
pub struct JumpSample {
    pub kind: NavKind,
    pub elapsed_ms: u32,
    /// 视觉滚动+焦点渲染是否跟随。
    pub follow_hit: bool,
}

impl JumpSample {
    pub fn within_budget(&self) -> bool {
        self.elapsed_ms <= 200
    }
}

/// 导航渲染器。
#[derive(Debug, Clone)]
pub struct NavRenderer {
    pub landmarks: Vec<Landmark>,
    pub jumps: Vec<JumpSample>,
    /// 视觉序（下标=视觉位，值=地标 id）。
    pub visual_order: Vec<u32>,
}

impl NavRenderer {
    pub fn new() -> Self {
        Self {
            landmarks: Vec::new(),
            jumps: Vec::new(),
            visual_order: Vec::new(),
        }
    }

    /// 地标覆盖：全部 landmark 有边界。
    pub fn landmark_coverage(&self) -> bool {
        self.landmarks.iter().all(|l| l.boundary_rendered)
    }

    /// 序一致：landmark.order 序列 == visual_order。
    pub fn order_consistent(&self) -> bool {
        if self.landmarks.len() != self.visual_order.len() {
            return false;
        }
        let mut sorted: Vec<(u32, u32)> =
            self.landmarks.iter().map(|l| (l.order, l.id)).collect();
        sorted.sort_by_key(|x| x.0);
        for (i, (_, id)) in sorted.iter().enumerate() {
            if self.visual_order[i] != *id {
                return false;
            }
        }
        true
    }

    /// 跳转预算。
    pub fn jumps_within_budget(&self) -> bool {
        self.jumps.iter().all(|j| j.within_budget())
    }

    /// 跟随单源。
    pub fn follow_intact(&self) -> bool {
        self.jumps.iter().all(|j| j.follow_hit)
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC2Code> {
        if !self.landmark_coverage() {
            return Err(CC2Code::LANDMARK_MISSING);
        }
        if !self.order_consistent() {
            return Err(CC2Code::ORDER_MISMATCH);
        }
        if !self.follow_intact() {
            return Err(CC2Code::FOLLOW_BROKEN);
        }
        if !self.jumps_within_budget() {
            return Err(CC2Code::JUMP_TIMEOUT);
        }
        Ok(())
    }
}

impl Default for NavRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves146_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves146_navrender");

    fn ready() -> NavRenderer {
        let mut r = NavRenderer::new();
        r.landmarks.push(Landmark { id: 1, boundary_rendered: true, order: 1 });
        r.landmarks.push(Landmark { id: 2, boundary_rendered: true, order: 2 });
        r.visual_order.push(1);
        r.visual_order.push(2);
        r.jumps.push(JumpSample { kind: NavKind::Heading, elapsed_ms: 120, follow_hit: true });
        r.jumps.push(JumpSample { kind: NavKind::Landmark, elapsed_ms: 150, follow_hit: true });
        r
    }

    // 01 地标覆盖
    {
        let r = ready();
        chk(&mut set, "C3846-01", r.landmark_coverage(), "err");
    }
    // 02 地标缺
    {
        let mut r = ready();
        r.landmarks[0].boundary_rendered = false;
        chk(
            &mut set,
            "C3846-02",
            r.full_check() == Err(CC2Code::LANDMARK_MISSING),
            "err",
        );
    }
    // 03 序一致
    {
        let r = ready();
        chk(&mut set, "C3846-03", r.order_consistent(), "err");
    }
    // 04 序不一致
    {
        let mut r = ready();
        r.visual_order.reverse();
        chk(
            &mut set,
            "C3846-04",
            r.full_check() == Err(CC2Code::ORDER_MISMATCH),
            "err",
        );
    }
    // 05 跳转预算
    {
        let r = ready();
        chk(&mut set, "C3846-05", r.jumps_within_budget(), "err");
    }
    // 06 跳转超时
    {
        let mut r = ready();
        r.jumps[0].elapsed_ms = 350;
        chk(
            &mut set,
            "C3846-06",
            r.full_check() == Err(CC2Code::JUMP_TIMEOUT),
            "err",
        );
    }
    // 07 跟随断链
    {
        let mut r = ready();
        r.jumps[0].follow_hit = false;
        chk(
            &mut set,
            "C3846-07",
            r.full_check() == Err(CC2Code::FOLLOW_BROKEN),
            "err",
        );
    }
    // 08 全检通过
    {
        let r = ready();
        chk(&mut set, "C3846-08", r.full_check().is_ok(), "err");
    }
    // 09 空集
    {
        let r = NavRenderer::new();
        chk(&mut set, "C3846-09", r.full_check().is_ok(), "err");
    }
    // 10 三类导航
    {
        chk(&mut set, "C3846-10", NavKind::ALL.len() == 3, "err");
    }
    // 11 预算语义
    {
        let j = JumpSample { kind: NavKind::LinkList, elapsed_ms: 200, follow_hit: true };
        chk(&mut set, "C3846-11", j.within_budget(), "err");
    }
    // 12 跟随语义
    {
        let r = ready();
        chk(&mut set, "C3846-12", r.follow_intact(), "err");
    }

    set
}
