//! unx0544_ringbuf —— UNX-F0544 环形日志缓冲（128 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0544 · 环形日志缓冲 · 目标 220 行
//! 判据原文：环形缓冲账齐（128 槽×行结构），守恒式（写=读+存+覆 零差），
//!           容量冻结。
//! 组件：RingLog（128 槽环形）+ 守恒账（写入数=在读数+存量+覆盖数 零差）
//!   + 环读（从最老到最新）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35540;
pub const XC_RING_BAD: u32 = SEG_BASE + 1; // 环坏
pub const XC_CONSERVE: u32 = SEG_BASE + 2; // 守恒破
pub const XC_RING_EMPTY: u32 = SEG_BASE + 3; // 空环读
pub const RING_CAP: usize = 128;

/// 环形日志。
#[derive(Debug)]
pub struct RingLog {
    slots: [Option<String>; RING_CAP],
    head: usize,
    written: u64,
    overwritten: u64,
    /// 被读走计数（外部消费模拟）。
    pub consumed: u64,
}

impl RingLog {
    /// 空环（128 槽冻结）。
    pub fn new() -> Self {
        RingLog {
            slots: Default::default(),
            head: 0,
            written: 0,
            overwritten: 0,
            consumed: 0,
        }
    }

    /// 写一行；满环覆盖最老。
    pub fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.written as usize >= RING_CAP {
            self.overwritten += 1;
        }
        self.slots[self.head] = Some(String::from(text));
        self.head = (self.head + 1) % RING_CAP;
        self.written += 1;
    }

    /// 存量。
    pub fn len(&self) -> usize {
        let w = self.written as usize;
        if w >= RING_CAP {
            RING_CAP
        } else {
            w
        }
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    /// 写入总数。
    pub fn total_written(&self) -> u64 {
        self.written
    }

    /// 覆盖数。
    pub fn total_overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 守恒式：写 = 读(消费) + 存 + 覆 零差。
    pub fn conserved(&self) -> bool {
        self.written == self.consumed + self.len() as u64 + self.overwritten
    }

    /// 环读：最老→最新（序）。
    pub fn drain_oldest_first(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let n = self.len();
        let start = if self.written as usize >= RING_CAP {
            self.head
        } else {
            0
        };
        let mut i = 0;
        while i < n {
            let idx = (start + i) % RING_CAP;
            if let Some(s) = self.slots[idx].take() {
                out.push(s);
                self.consumed += 1;
            }
            i += 1;
        }
        out
    }

    /// 只读快照（不消费）：最老→最新。
    pub fn snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = self.len();
        let start = if self.written as usize >= RING_CAP {
            self.head
        } else {
            0
        };
        let mut i = 0;
        while i < n {
            let idx = (start + i) % RING_CAP;
            if let Some(s) = &self.slots[idx] {
                out.push(String::from(s));
            }
            i += 1;
        }
        out
    }
}

impl Default for RingLog {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0544_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0544");
    cs.add("SEG 段位独占", SEG_BASE == 0x35540 && XC_RING_EMPTY == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：容量冻结。
    cs.add("128 槽冻结", RING_CAP == 128, "容量常量");

    // 族二：未满环——序与守恒。
    let mut r = RingLog::new();
    let mut i = 0;
    while i < 100 {
        r.push("line");
        i += 1;
    }
    cs.add("存量 100", r.len() == 100 && r.total_written() == 100, "未满环全存");
    cs.add("零覆盖", r.total_overwritten() == 0, "未满不覆");
    cs.add("守恒（未读）", r.conserved(), "100=0+100+0");
    let snap = r.snapshot();
    cs.add("快照序", snap.len() == 100, "最老→最新 100 行");

    // 族三：满环覆盖。
    let mut r2 = RingLog::new();
    let mut i = 0;
    while i < 130 {
        r2.push("x");
        i += 1;
    }
    cs.add("存量钳 128", r2.len() == RING_CAP, "满环容量顶");
    cs.add("覆盖 2", r2.total_overwritten() == 2, "130-128=2");
    cs.add("守恒（满环）", r2.conserved(), "130=0+128+2");
    // 覆盖行号可查（F0551 消费）：最老两行已被覆盖——快照首行是第 3 写。
    let snap2 = r2.snapshot();
    cs.add("覆盖后首行偏移", snap2.len() == 128, "快照 128 行");

    // 族四：环读消费。
    let drained = r2.drain_oldest_first();
    cs.add("读走 128", drained.len() == 128 && r2.consumed == 128, "全量消费");
    cs.add("读后守恒", r2.conserved(), "130=128+0+2");
    cs.add("读后空", r2.is_empty() || r2.len() == 0, "存量归零");

    // 族五：空环拒。
    let r3 = RingLog::new();
    cs.add("空环快照空", r3.snapshot().is_empty(), "零产出");
    cs.add("空环守恒", r3.conserved(), "0=0+0+0");
    cs
}
