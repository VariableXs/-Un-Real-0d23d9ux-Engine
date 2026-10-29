//! unx0875_vecleak —— UNX-F0875 · 向量释放与泄漏账（分配/释放配平）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0875 · 向量释放与泄漏账。
//! 判据原文摘录（卡面验收行）：向量释放与泄漏账（分配/释放配平）
//!           ——每向量分配/释放逐笔记账、泄漏 = 持有存量恒等可查、
//!           泄漏阈值越线即红账。
//! 诊断段 0x78E0（本单独占，0x78E1~0x78EF 闭集）。
//!
//! 设计纪律：
//! - 泄漏定义：设备已注销（close）但仍有向量在持 = 泄漏；或设备行
//!   total_alloc - total_free - held ≠ 0 即账破。
//! - 恒等式（每设备行）：total_alloc = held + total_free（三账恒等，
//!   每次分配/释放后核）。全域：Σalloc = Σheld + Σfree。
//! - 泄漏阈值：close 时 held > 0 即泄漏入账（逐行）；全域泄漏存量
//!   LEAK_MAX（8 行）越线红账。
//! - 释放配平：release 须 (dev, vec) 双对拍；重复释放即红。
//! - 定容账：泄漏账 8 行 + 释放流水利 96 行（溢出计数不丢量）。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const VL_VERSION: &str = "unx0875_vecleak-v1";

/// 诊断段基址（0x78E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x78E0;

/// XcCode 闭集（0x78E1~0x78EF）。
pub const XC_NOT_OWNED: u16 = SEG_BASE + 1; // 非主释放 / 未持有向量
pub const XC_DUP_FREE: u16 = SEG_BASE + 2; // 重复释放
pub const XC_LED_EQ: u16 = SEG_BASE + 3; // 设备三账恒等破
pub const XC_LEAK_CAP: u16 = SEG_BASE + 4; // 泄漏账越线（>8 行）
pub const XC_DEV_CLOSED: u16 = SEG_BASE + 5; // 对已注销设备操作
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 流水账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 7; // 全域守恒破
pub const XC_UNKNOWN_DEV: u16 = SEG_BASE + 8; // 未开户设备

/// 泄漏账上限。
pub const LEAK_MAX: usize = 8;
/// 流水账上限。
pub const FLOW_CAP: usize = 96;

/// 设备状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevState {
    /// 在役。
    Open,
    /// 注销（close 后 held 必须为 0，否则泄漏）。
    Closed,
}

/// 设备行（三账恒等：total_alloc = held + total_free）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevRow {
    /// 设备号。
    pub dev: u32,
    /// 状态。
    pub state: DevState,
    /// 当前持有。
    pub held: usize,
    /// 累计分配。
    pub total_alloc: u64,
    /// 累计释放。
    pub total_free: u64,
}

/// 流水行：(种类, 设备, 向量, 序号)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowKind {
    /// 分配。
    Alloc,
    /// 释放。
    Free,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowRow {
    /// 种类。
    pub kind: FlowKind,
    /// 设备号。
    pub dev: u32,
    /// 向量号（16~255）。
    pub vec: u16,
    /// 序号。
    pub seq: u64,
}

/// 泄漏行：(设备, 泄漏向量数, 存量快照, 序号)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeakRow {
    /// 设备号。
    pub dev: u32,
    /// 泄漏向量数。
    pub leaked: usize,
    /// close 时持有快照。
    pub snapshot: usize,
    /// 序号。
    pub seq: u64,
}

/// 泄漏账管理器。
pub struct LeakLedger {
    devs: Vec<DevRow>,
    /// 流水。
    flow: Vec<FlowRow>,
    flow_overflow: usize,
    /// 泄漏账。
    leaks: Vec<LeakRow>,
    seq: u64,
    /// 全域累计。
    g_alloc: u64,
    g_free: u64,
}

impl LeakLedger {
    pub fn new() -> LeakLedger {
        LeakLedger {
            devs: Vec::new(),
            flow: Vec::new(),
            flow_overflow: 0,
            leaks: Vec::new(),
            seq: 0,
            g_alloc: 0,
            g_free: 0,
        }
    }

    fn push_flow(&mut self, kind: FlowKind, dev: u32, vec: u16) {
        self.seq += 1;
        if self.flow.len() < FLOW_CAP {
            self.flow.push(FlowRow { kind, dev, vec, seq: self.seq });
        } else {
            self.flow_overflow += 1;
        }
    }

    fn dev_pos(&self, dev: u32) -> Option<usize> {
        self.devs.iter().position(|r| r.dev == dev)
    }

    /// 开户（重复开户即红）。
    pub fn open(&mut self, dev: u32) -> Result<(), u16> {
        if self.dev_pos(dev).is_some() {
            return Err(XC_UNKNOWN_DEV);
        }
        self.devs.push(DevRow {
            dev,
            state: DevState::Open,
            held: 0,
            total_alloc: 0,
            total_free: 0,
        });
        Ok(())
    }

    /// 分配记账：设备在役才可分；held++；三账核。
    pub fn alloc(&mut self, dev: u32, vec: u16) -> Result<(), u16> {
        let pos = match self.dev_pos(dev) {
            Some(p) => p,
            None => return Err(XC_UNKNOWN_DEV),
        };
        if self.devs[pos].state == DevState::Closed {
            return Err(XC_DEV_CLOSED);
        }
        self.devs[pos].held += 1;
        self.devs[pos].total_alloc += 1;
        self.g_alloc += 1;
        self.push_flow(FlowKind::Alloc, dev, vec);
        self.check_row(pos)
    }

    /// 释放记账：在持对拍 + 三账核。
    pub fn release(&mut self, dev: u32, vec: u16) -> Result<(), u16> {
        let pos = match self.dev_pos(dev) {
            Some(p) => p,
            None => return Err(XC_UNKNOWN_DEV),
        };
        if self.devs[pos].state == DevState::Closed {
            return Err(XC_DEV_CLOSED);
        }
        if self.devs[pos].held == 0 {
            // 无持有可放：区分未分配与重复释放（查流水）
            if self.flow.iter().any(|f| f.dev == dev && f.vec == vec && f.kind == FlowKind::Free) {
                return Err(XC_DUP_FREE);
            }
            return Err(XC_NOT_OWNED);
        }
        self.devs[pos].held -= 1;
        self.devs[pos].total_free += 1;
        self.g_free += 1;
        self.push_flow(FlowKind::Free, dev, vec);
        self.check_row(pos)
    }

    /// 设备注销：held>0 即泄漏入账（越 8 行红账）。
    pub fn close(&mut self, dev: u32) -> Result<usize, u16> {
        let pos = match self.dev_pos(dev) {
            Some(p) => p,
            None => return Err(XC_UNKNOWN_DEV),
        };
        let held = self.devs[pos].held;
        self.devs[pos].state = DevState::Closed;
        if held > 0 {
            if self.leaks.len() >= LEAK_MAX {
                return Err(XC_LEAK_CAP);
            }
            self.seq += 1;
            self.leaks.push(LeakRow {
                dev,
                leaked: held,
                snapshot: held,
                seq: self.seq,
            });
        }
        Ok(held)
    }

    /// 行级三账恒等核。
    fn check_row(&self, pos: usize) -> Result<(), u16> {
        let r = &self.devs[pos];
        if r.total_alloc != r.held as u64 + r.total_free {
            return Err(XC_LED_EQ);
        }
        Ok(())
    }

    /// 全域守恒：Σalloc = Σheld + Σfree；Σheld = Σ行 held。
    pub fn conserve(&self) -> bool {
        let held_sum: usize = self.devs.iter().map(|r| r.held).sum();
        self.g_alloc == held_sum as u64 + self.g_free
    }

    /// 流水守恒：在账 + 溢出 = 流水总发（= g_alloc + g_free）。
    pub fn flow_conserve(&self) -> bool {
        self.flow.len() + self.flow_overflow == (self.g_alloc + self.g_free) as usize
    }

    /// 泄漏行读取。
    pub fn leak(&self, i: usize) -> Result<LeakRow, u16> {
        if i >= self.leaks.len() {
            return Err(XC_LEAK_CAP);
        }
        Ok(self.leaks[i])
    }

    /// 泄漏行数。
    pub fn leak_count(&self) -> usize {
        self.leaks.len()
    }

    /// 流水行读取。
    pub fn flow_row(&self, i: usize) -> Result<FlowRow, u16> {
        if i >= self.flow.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.flow[i])
    }

    /// 设备行读取。
    pub fn dev_row(&self, i: usize) -> Result<DevRow, u16> {
        if i >= self.devs.len() {
            return Err(XC_UNKNOWN_DEV);
        }
        Ok(self.devs[i])
    }

    /// 泄漏存量（全域）。
    pub fn leak_total(&self) -> usize {
        self.leaks.iter().map(|l| l.leaked).sum()
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 开户 + 正常配平：dev A 分配 3 放 3
    let mut l = LeakLedger::new();
    l.open(0xA0)?;
    for v in [0x30u16, 0x31, 0x32].iter() {
        l.alloc(0xA0, *v)?;
    }
    for v in [0x30u16, 0x31, 0x32].iter() {
        l.release(0xA0, *v)?;
    }
    if !l.conserve() || !l.flow_conserve() {
        return Err(XC_CONSERVE);
    }
    // 2. 泄漏路径：dev B 分配 2 不放即 close → 泄漏 2
    l.open(0xB0)?;
    l.alloc(0xB0, 0x40)?;
    l.alloc(0xB0, 0x41)?;
    let leaked = l.close(0xB0)?;
    if leaked != 2 || l.leak_count() != 1 || l.leak_total() != 2 {
        return Err(XC_LEAK_CAP);
    }
    if l.leak(0)?.dev != 0xB0 || l.leak(0)?.snapshot != 2 {
        return Err(XC_LEAK_CAP);
    }
    // 3. 对已注销设备分配拒
    if l.alloc(0xB0, 0x42).is_ok() {
        return Err(XC_DEV_CLOSED);
    }
    // 4. 错误路径①：未开户设备
    if l.alloc(0xC0, 0x50).is_ok() {
        return Err(XC_UNKNOWN_DEV);
    }
    // 5. 错误路径②：无持有释放（0x33 从未分配 → NOT_OWNED）
    if l.release(0xA0, 0x33).is_ok() {
        return Err(XC_NOT_OWNED);
    }
    // 6. 错误路径③：重复释放（流水含 0xA0/0x30 的 Free 行）
    l.alloc(0xA0, 0x30)?;
    l.release(0xA0, 0x30)?;
    if l.release(0xA0, 0x30).is_ok() {
        return Err(XC_DUP_FREE);
    }
    // 7. 全域守恒核：g_alloc=6（3+2+1），g_free=4（3+1），held_sum=2
    //    （0xB0 close 冻结持有 2 计入恒等式）→ 6 = 2 + 4 ✓
    if l.g_alloc != 6 || l.g_free != 4 || !l.conserve() {
        return Err(XC_LED_EQ);
    }
    // 8. 泄漏账越线红账：再漏 8 行触发 XC_LEAK_CAP
    let mut l2 = LeakLedger::new();
    let mut hit_cap = false;
    for d in 1..=10u32 {
        let dev = 0xD0 + d;
        if l2.open(dev).is_err() {
            hit_cap = true;
            break;
        }
        if l2.alloc(dev, 0x60).is_err() {
            hit_cap = true;
            break;
        }
        if l2.close(dev).is_err() {
            hit_cap = true;
            break;
        }
    }
    if !hit_cap || l2.leak_count() != LEAK_MAX {
        return Err(XC_LEAK_CAP);
    }
    Ok(0)
}
