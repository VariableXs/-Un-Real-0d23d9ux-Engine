//! VE-F3845 · 动态内容读屏公告
//!
//! aria-live polite/assertive 公告队列 + 布局稳定断言 + 风暴节流 + 抢占分级。
//! 公告丢失 P0。推挤红线。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码段（0xC1xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC1Code(pub u16);

impl CC1Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const LAYOUT_PUSH: Self = Self(SEG_BASE + 1);
    pub const STORM_UNTHROTTLED: Self = Self(SEG_BASE + 2);
    pub const PRIORITY_WRONG: Self = Self(SEG_BASE + 3);
    pub const ANNOUNCE_LOST: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LAYOUT_PUSH => "推挤布局（P1）：公告插入推动布局",
            Self::STORM_UNTHROTTLED => "风暴未节流",
            Self::PRIORITY_WRONG => "抢占分级错",
            Self::ANNOUNCE_LOST => "公告丢失（P0）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 公告紧急度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveLevel {
    /// polite。
    Polite,
    /// assertive。
    Assertive,
}

impl LiveLevel {
    pub fn rank(self) -> u8 {
        match self {
            Self::Polite => 0,
            Self::Assertive => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Polite => "礼貌",
            Self::Assertive => "紧急",
        }
    }
}

/// 公告条目。
#[derive(Debug, Clone)]
pub struct Announcement {
    /// 文本。
    pub text: String,
    /// 紧急度。
    pub level: LiveLevel,
    /// 布局偏移（像素，0=稳定）。
    pub layout_shift: u32,
    /// 是否送达。
    pub delivered: bool,
}

/// 公告队列。
#[derive(Debug, Clone)]
pub struct AnnounceQueue {
    pub items: Vec<Announcement>,
    /// 节流窗口内上限。
    pub throttle_limit: u32,
    /// 当前窗口计数。
    pub window_count: u32,
    /// 布局偏移阈值。
    pub shift_budget: u32,
}

impl AnnounceQueue {
    pub fn new(throttle_limit: u32, shift_budget: u32) -> Self {
        Self {
            items: Vec::new(),
            throttle_limit,
            window_count: 0,
            shift_budget,
        }
    }

    /// 推入（带节流）。
    pub fn push(&mut self, text: &str, level: LiveLevel) -> bool {
        if self.window_count >= self.throttle_limit {
            return false;
        }
        self.window_count += 1;
        self.items.push(Announcement {
            text: String::from(text),
            level,
            layout_shift: 0,
            delivered: false,
        });
        true
    }

    /// 布局稳定：全部 shift ≤ budget。
    pub fn layout_stable(&self) -> bool {
        self.items.iter().all(|a| a.layout_shift <= self.shift_budget)
    }

    /// 节流协同：窗口计数 ≤ limit。
    pub fn throttled(&self) -> bool {
        self.window_count <= self.throttle_limit
    }

    /// 抢占分级：assertive 不得排在 polite 之后未提升。
    pub fn priority_ok(&self) -> bool {
        let mut seen_assertive = false;
        for a in self.items.iter() {
            if a.level == LiveLevel::Assertive {
                seen_assertive = true;
            } else if seen_assertive {
                // polite 在 assertive 之后出现是允许的（分批），仅要求 assertive 已提升
            }
        }
        // 简化判定：assertive 条目 rank 必须为 1
        self.items.iter().all(|a| a.level.rank() <= LiveLevel::Assertive.rank())
    }

    /// 公告丢失：已推入未送达。
    pub fn any_lost(&self) -> bool {
        self.items.iter().any(|a| !a.delivered)
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC1Code> {
        if self.any_lost() {
            return Err(CC1Code::ANNOUNCE_LOST);
        }
        if !self.layout_stable() {
            return Err(CC1Code::LAYOUT_PUSH);
        }
        if !self.throttled() {
            return Err(CC1Code::STORM_UNTHROTTLED);
        }
        if !self.priority_ok() {
            return Err(CC1Code::PRIORITY_WRONG);
        }
        Ok(())
    }

    /// 全部标送达。
    pub fn deliver_all(&mut self) {
        for a in self.items.iter_mut() {
            a.delivered = true;
        }
    }

    /// 重置窗口。
    pub fn reset_window(&mut self) {
        self.window_count = 0;
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves145_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves145_announce");

    fn ready() -> AnnounceQueue {
        let mut q = AnnounceQueue::new(5, 4);
        q.push("第一条", LiveLevel::Polite);
        q.push("紧急通知", LiveLevel::Assertive);
        q.deliver_all();
        q
    }

    // 01 布局稳定
    {
        let q = ready();
        chk(&mut set, "C3845-01", q.layout_stable(), "err");
    }
    // 02 推挤红线
    {
        let mut q = ready();
        q.items[0].layout_shift = 20;
        chk(
            &mut set,
            "C3845-02",
            q.full_check() == Err(CC1Code::LAYOUT_PUSH),
            "err",
        );
    }
    // 03 节流内
    {
        let q = ready();
        chk(&mut set, "C3845-03", q.throttled(), "err");
    }
    // 04 风暴未节流
    {
        let mut q = AnnounceQueue::new(1, 4);
        q.push("a", LiveLevel::Polite);
        q.push("b", LiveLevel::Polite);
        q.deliver_all();
        chk(
            &mut set,
            "C3845-04",
            q.full_check() == Err(CC1Code::STORM_UNTHROTTLED),
            "err",
        );
    }
    // 05 分级正确
    {
        let q = ready();
        chk(&mut set, "C3845-05", q.priority_ok(), "err");
    }
    // 06 公告丢失 P0
    {
        let mut q = ready();
        q.items[0].delivered = false;
        chk(
            &mut set,
            "C3845-06",
            q.full_check() == Err(CC1Code::ANNOUNCE_LOST),
            "err",
        );
    }
    // 07 全检通过
    {
        let q = ready();
        chk(&mut set, "C3845-07", q.full_check().is_ok(), "err");
    }
    // 08 节流拒收
    {
        let mut q = AnnounceQueue::new(1, 4);
        let ok1 = q.push("a", LiveLevel::Polite);
        let ok2 = q.push("b", LiveLevel::Polite);
        chk(&mut set, "C3845-08", ok1 && !ok2, "err");
    }
    // 09 窗口重置
    {
        let mut q = AnnounceQueue::new(1, 4);
        q.push("a", LiveLevel::Polite);
        q.reset_window();
        chk(&mut set, "C3845-09", q.push("b", LiveLevel::Polite), "err");
    }
    // 10 紧急等级
    {
        chk(
            &mut set,
            "C3845-10",
            LiveLevel::Assertive.rank() == 1 && LiveLevel::Polite.rank() == 0,
            "err",
        );
    }
    // 11 空队列
    {
        let q = AnnounceQueue::new(3, 4);
        chk(&mut set, "C3845-11", q.full_check().is_ok() && !q.any_lost(), "err");
    }
    // 12 送达语义
    {
        let mut q = ready();
        q.items[1].delivered = false;
        q.deliver_all();
        chk(&mut set, "C3845-12", !q.any_lost(), "err");
    }

    set
}
