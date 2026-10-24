//! VE-F3842 · 焦点视觉追踪
//!
//! 读屏导航→视觉焦点跟随（≤1 帧）+ 视觉→读屏反追双向 + 语义几何对齐。
//! 双向红线：只跟单向 = 另一半用户迷失。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xBExx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBE00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBECode(pub u16);

impl CBECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ONE_WAY_ONLY: Self = Self(SEG_BASE + 1);
    pub const FOLLOW_LAG: Self = Self(SEG_BASE + 2);
    pub const ALIGN_BROKEN: Self = Self(SEG_BASE + 3);
    pub const CURSOR_LOST: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ONE_WAY_ONLY => "单向跟随（P1）：双向红线",
            Self::FOLLOW_LAG => "跟随滞后（>1 帧）",
            Self::ALIGN_BROKEN => "对齐断链（P0）",
            Self::CURSOR_LOST => "渲染丢游标，需修正",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 几何矩形（整数像素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w as i32 && py < self.y + self.h as i32
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w as i32 / 2, self.y + self.h as i32 / 2)
    }
}

/// 跟随方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowDirection {
    /// 读屏→视觉。
    SrToVisual,
    /// 视觉→读屏。
    VisualToSr,
}

impl FollowDirection {
    pub const BOTH: [FollowDirection; 2] = [FollowDirection::SrToVisual, FollowDirection::VisualToSr];
}

/// 跟随样本。
#[derive(Debug, Clone, Copy)]
pub struct FollowSample {
    /// 方向。
    pub dir: FollowDirection,
    /// 延迟（帧）。
    pub lag_frames: u8,
    /// 是否命中。
    pub hit: bool,
}

impl FollowSample {
    /// 1 帧断言。
    pub fn within_one_frame(&self) -> bool {
        self.lag_frames <= 1
    }
}

/// 语义节点↔渲染几何对齐。
#[derive(Debug, Clone)]
pub struct AlignEntry {
    /// 语义节点 id。
    pub node_id: u32,
    /// 渲染几何。
    pub rect: Rect,
    /// 对齐有效。
    pub aligned: bool,
}

/// 焦点追踪器。
#[derive(Debug, Clone)]
pub struct FocusTracker {
    /// 跟随样本。
    pub samples: Vec<FollowSample>,
    /// 对齐表。
    pub aligns: Vec<AlignEntry>,
    /// 游标是否在渲染面。
    pub cursor_visible: bool,
}

impl FocusTracker {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            aligns: Vec::new(),
            cursor_visible: true,
        }
    }

    /// 记一次跟随。
    pub fn record(&mut self, dir: FollowDirection, lag_frames: u8) {
        self.samples.push(FollowSample {
            dir,
            lag_frames,
            hit: lag_frames <= 1,
        });
    }

    /// 双向红线：两方向都必须有样本。
    pub fn bidirectional(&self) -> bool {
        FollowDirection::BOTH
            .iter()
            .all(|d| self.samples.iter().any(|s| s.dir == *d))
    }

    /// 1 帧跟随：全部样本 ≤1 帧。
    pub fn within_one_frame(&self) -> bool {
        self.samples.iter().all(|s| s.within_one_frame())
    }

    /// 对齐单源：全部 aligned。
    pub fn align_intact(&self) -> bool {
        self.aligns.iter().all(|a| a.aligned)
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CBECode> {
        if !self.cursor_visible {
            return Err(CBECode::CURSOR_LOST);
        }
        if !self.align_intact() {
            return Err(CBECode::ALIGN_BROKEN);
        }
        if !self.bidirectional() {
            return Err(CBECode::ONE_WAY_ONLY);
        }
        if !self.within_one_frame() {
            return Err(CBECode::FOLLOW_LAG);
        }
        Ok(())
    }

    /// 查节点几何。
    pub fn geometry_of(&self, node_id: u32) -> Option<Rect> {
        for a in self.aligns.iter() {
            if a.node_id == node_id && a.aligned {
                return Some(a.rect);
            }
        }
        None
    }
}

impl Default for FocusTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves142_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves142_focustrack");

    fn ready() -> FocusTracker {
        let mut t = FocusTracker::new();
        t.record(FollowDirection::SrToVisual, 1);
        t.record(FollowDirection::VisualToSr, 0);
        t.aligns.push(AlignEntry {
            node_id: 1,
            rect: Rect { x: 0, y: 0, w: 100, h: 20 },
            aligned: true,
        });
        t
    }

    // 01 1 帧跟随
    {
        let t = ready();
        chk(&mut set, "C3842-01", t.within_one_frame(), "err");
    }
    // 02 跟随滞后
    {
        let mut t = ready();
        t.record(FollowDirection::SrToVisual, 3);
        chk(&mut set, "C3842-02", !t.within_one_frame(), "err");
    }
    // 03 双向齐
    {
        let t = ready();
        chk(&mut set, "C3842-03", t.bidirectional(), "err");
    }
    // 04 单向红线
    {
        let mut t = FocusTracker::new();
        t.record(FollowDirection::SrToVisual, 0);
        chk(
            &mut set,
            "C3842-04",
            t.full_check() == Err(CBECode::ONE_WAY_ONLY) || !t.bidirectional(),
            "err",
        );
    }
    // 05 对齐断链 P0
    {
        let mut t = ready();
        t.aligns[0].aligned = false;
        chk(
            &mut set,
            "C3842-05",
            t.full_check() == Err(CBECode::ALIGN_BROKEN),
            "err",
        );
    }
    // 06 游标丢
    {
        let mut t = ready();
        t.cursor_visible = false;
        chk(
            &mut set,
            "C3842-06",
            t.full_check() == Err(CBECode::CURSOR_LOST),
            "err",
        );
    }
    // 07 跟随滞后检出
    {
        let mut t = ready();
        t.record(FollowDirection::VisualToSr, 2);
        chk(
            &mut set,
            "C3842-07",
            t.full_check() == Err(CBECode::FOLLOW_LAG),
            "err",
        );
    }
    // 08 全检通过
    {
        let t = ready();
        chk(&mut set, "C3842-08", t.full_check().is_ok(), "err");
    }
    // 09 几何对齐
    {
        let t = ready();
        let g = t.geometry_of(1);
        chk(&mut set, "C3842-09", g.is_some() && g.unwrap().w == 100, "err");
    }
    // 10 几何缺
    {
        let t = ready();
        chk(&mut set, "C3842-10", t.geometry_of(99).is_none(), "err");
    }
    // 11 矩形包含
    {
        let r = Rect { x: 10, y: 10, w: 50, h: 30 };
        chk(&mut set, "C3842-11", r.contains(20, 20) && !r.contains(5, 5), "err");
    }
    // 12 中心点
    {
        let r = Rect { x: 0, y: 0, w: 100, h: 20 };
        let (cx, cy) = r.center();
        chk(&mut set, "C3842-12", cx == 50 && cy == 10, "err");
    }

    set
}
