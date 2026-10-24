//! VE-F3807 · 读屏渲染协同总架构（VE-T 域 · 无障碍渲染 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3807`
//!
//! **判据（锚点原文）**：三向闭环、双向跟随、断链 P0、协同架构、判据。
//!
//! **本条是什么**：读屏与渲染的**协同架构**——语义树↔像素↔读屏播报
//! 三向闭环（断流即 P0）；焦点双向跟随（1 帧内对齐）；渲染态变更事件
//! 同步读屏；断链扫描器接入。三向任一断 = 信息在无障碍通道丢失。
//!
//! 诊断码段：独占 0xC0xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC0xx）
// ─────────────────────────────────────────────────────────────

/// F3807 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C0Code(pub u16);

impl C0Code {
    /// 断链检出——P0。
    pub const CHAIN_BREAK: C0Code = C0Code(0xC001);
    /// 跟随超 1 帧——P1。
    pub const FOLLOW_LATE: C0Code = C0Code(0xC002);
    /// 三向缺环——补。
    pub const LOOP_MISS: C0Code = C0Code(0xC003);
    /// 播报未同步——补。
    pub const ANNOUNCE_MISS: C0Code = C0Code(0xC004);
    /// 节点映射缺。
    pub const NODE_MISS: C0Code = C0Code(0xC005);
    /// 断链扫描未接。
    pub const SCAN_MISS: C0Code = C0Code(0xC006);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC001 => "三向闭环断链，P0——信息在无障碍通道丢失。",
            0xC002 => "焦点跟随超 1 帧，P1。",
            0xC003 => "三向闭环缺环，需补齐。",
            0xC004 => "渲染变更未同步读屏播报。",
            0xC005 => "语义节点映射缺失。",
            0xC006 => "断链扫描器未接入。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三向闭环
// ─────────────────────────────────────────────────────────────

/// 闭环方向（三向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 语义树 → 像素。
    SemToPixel,
    /// 像素 → 读屏播报。
    PixelToSpeech,
    /// 读屏 → 语义树（反向校验）。
    SpeechToSem,
}

impl Direction {
    /// 全集。
    pub const ALL: [Direction; 3] = [
        Direction::SemToPixel,
        Direction::PixelToSpeech,
        Direction::SpeechToSem,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Direction::SemToPixel => 0,
            Direction::PixelToSpeech => 1,
            Direction::SpeechToSem => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Direction::SemToPixel => "语义→像素",
            Direction::PixelToSpeech => "像素→读屏",
            Direction::SpeechToSem => "读屏→语义",
        }
    }
}

/// 三向闭环状态机。
#[derive(Clone, Debug)]
pub struct TriLoop {
    alive: [bool; 3],
    /// 断链计数（P0）。
    pub breaks: u32,
}

impl TriLoop {
    /// 新建（全通）。
    pub const fn new() -> Self {
        TriLoop {
            alive: [true; 3],
            breaks: 0,
        }
    }

    /// 置一环断。
    pub fn cut(&mut self, d: Direction) {
        self.alive[d.ordinal()] = false;
        self.breaks += 1;
    }

    /// 修复一环。
    pub fn repair(&mut self, d: Direction) -> bool {
        let i = d.ordinal();
        if !self.alive[i] {
            self.alive[i] = true;
            true
        } else {
            false
        }
    }

    /// 全通。
    pub fn complete(&self) -> bool {
        self.alive[0] && self.alive[1] && self.alive[2]
    }

    /// 断环数。
    pub fn broken_count(&self) -> usize {
        self.alive.iter().filter(|a| !**a).count()
    }

    /// P0 门（零断链）。
    pub fn p0_gate(&self) -> bool {
        self.breaks == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 双向跟随 + 播报同步
// ─────────────────────────────────────────────────────────────

/// 焦点跟随。
#[derive(Clone, Copy, Debug)]
pub struct FocusFollow {
    /// 语义侧焦点 id。
    pub sem_focus: u16,
    /// 渲染侧焦点 id。
    pub render_focus: u16,
    /// 跟随延迟帧。
    pub lag_frames: u8,
}

impl FocusFollow {
    /// 新建。
    pub const fn new(sem: u16, render: u16, lag: u8) -> Self {
        FocusFollow {
            sem_focus: sem,
            render_focus: render,
            lag_frames: lag,
        }
    }

    /// 双向对齐（id 一致且 ≤1 帧）。
    pub fn aligned(self) -> bool {
        self.sem_focus == self.render_focus && self.lag_frames <= 1
    }

    /// 超帧。
    pub fn late(self) -> bool {
        self.lag_frames > 1
    }
}

/// 渲染变更事件 → 播报。
#[derive(Clone, Copy, Debug)]
pub struct RenderEvent {
    /// 节点 id。
    pub node: u16,
    /// 变更类型（1=文本 2=结构 3=状态）。
    pub kind: u8,
    /// 已播报。
    pub announced: bool,
}

impl RenderEvent {
    /// 构造。
    pub const fn new(node: u16, kind: u8) -> Self {
        RenderEvent {
            node,
            kind,
            announced: false,
        }
    }

    /// 同步播报。
    pub fn announce(&mut self) {
        self.announced = true;
    }

    /// 已同步。
    pub fn synced(self) -> bool {
        self.announced
    }
}

/// 事件队列。
#[derive(Clone, Debug)]
pub struct EventQueue {
    events: Vec<RenderEvent>,
}

impl EventQueue {
    /// 新建。
    pub fn new() -> Self {
        EventQueue {
            events: Vec::new(),
        }
    }

    /// 入队。
    pub fn push(&mut self, e: RenderEvent) -> usize {
        self.events.push(e);
        self.events.len() - 1
    }

    /// 播报一条。
    pub fn announce(&mut self, idx: usize) -> bool {
        match self.events.get_mut(idx) {
            Some(e) => {
                e.announce();
                true
            }
            None => false,
        }
    }

    /// 未播报数。
    pub fn unannounced(&self) -> usize {
        self.events.iter().filter(|e| !e.announced).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        EventQueue::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 断链扫描器接入
// ─────────────────────────────────────────────────────────────

/// 扫描器。
#[derive(Clone, Debug)]
pub struct BreakScanner {
    /// 已接入协同。
    pub wired: bool,
    /// 扫描节点数。
    pub scanned: u32,
    /// 断链发现数。
    pub found: u32,
}

impl BreakScanner {
    /// 新建。
    pub const fn new() -> Self {
        BreakScanner {
            wired: false,
            scanned: 0,
            found: 0,
        }
    }

    /// 接入。
    pub fn wire(&mut self) {
        self.wired = true;
    }

    /// 扫一节点。
    pub fn scan_node(&mut self, broken: bool) {
        self.scanned += 1;
        if broken {
            self.found += 1;
        }
    }

    /// 零断链。
    pub fn zero_break(&self) -> bool {
        self.found == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3807 判据集。
pub fn run_f3807_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3807");

    // 0xC003 三向闭环
    {
        let mut l = TriLoop::new();
        set.add("0xC003-初始全通", l.complete() && l.broken_count() == 0, "");
        l.cut(Direction::PixelToSpeech);
        set.add("0xC003-断一环检出", l.broken_count() == 1 && !l.complete(), "");
        set.add("0xC001-断链计数", l.breaks == 1 && !l.p0_gate(), "");
        set.add("0xC003-修复进", l.repair(Direction::PixelToSpeech), "");
        set.add("0xC003-修后全通", l.complete(), "");
        set.add("0xC003-重修拒", !l.repair(Direction::PixelToSpeech), "");
    }

    // 0xC002 双向跟随
    {
        let f = FocusFollow::new(5, 5, 1);
        set.add("0xC002-1帧对齐", f.aligned() && !f.late(), "");
        let f2 = FocusFollow::new(5, 6, 0);
        set.add("0xC002-id不齐", !f2.aligned(), "");
        let f3 = FocusFollow::new(5, 5, 2);
        set.add("0xC002-超帧检出", f3.late() && !f3.aligned(), "");
    }

    // 0xC004 播报同步
    {
        let mut q = EventQueue::new();
        set.add("0xC004-空队", q.is_empty(), "");
        q.push(RenderEvent::new(1, 1));
        q.push(RenderEvent::new(2, 2));
        set.add("0xC004-两未播", q.unannounced() == 2, "");
        q.announce(0);
        set.add("0xC004-播一剩一", q.unannounced() == 1, "");
        set.add("0xC004-越界拒", !q.announce(9), "");
    }

    // 0xC006 扫描器
    {
        let mut s = BreakScanner::new();
        set.add("0xC006-初始未接", !s.wired, "");
        s.wire();
        s.scan_node(false);
        s.scan_node(true);
        set.add("0xC006-断链发现", s.found == 1 && !s.zero_break(), "");
        set.add("0xC006-扫描计数", s.scanned == 2, "");
        let mut s2 = BreakScanner::new();
        s2.wire();
        s2.scan_node(false);
        set.add("0xC001-零断过P0", s2.zero_break(), "");
    }

    // 方向序自洽
    {
        let mut ok = true;
        for (i, d) in Direction::ALL.iter().enumerate() {
            if d.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC003-方向序自洽", ok, "");
    }

    set
}
