//! unx0877_audled —— UNX-F0877 · 分配审计账（分配/释放/复用全留痕）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0877 · 分配审计账。
//! 判据原文摘录（卡面验收行）：分配审计账（分配/释放/复用全留痕）
//!           ——每笔分配/释放/复用逐条留痕、流水环回可回放、
//!           留痕守恒（三流合计 = 总序号）。
//! 诊断段 0x7920（本单独占，0x7921~0x792F 闭集）。
//!
//! 设计纪律：
//! - 三流闭集：Alloc / Free / Reuse（同段释放后再分配记 Reuse）。
//! - 环回流水：定容 64 行环形账；写满回卷覆盖最旧行；head/tail 索引
//!   纪律：覆盖数 + 在账数 = 总发数。
//! - 回放面：按序号区间回放（seq 递增全序），被覆盖行以覆盖计数
//!   对账（不丢量，只丢行）。
//! - 每行留痕：流种 + 对象号 + 段基 + 段长 + 序号（五元组，零缺项）。
//! - 守恒恒等式：alloc 数 = free 数 + 在用段数；总序号 = alloc+free+reuse。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const AL_VERSION: &str = "unx0877_audled-v1";

/// 诊断段基址（0x7920 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7920;

/// XcCode 闭集（0x7921~0x792F）。
pub const XC_DUP_FREE: u16 = SEG_BASE + 1; // 重复释放
pub const XC_NOT_ALLOC: u16 = SEG_BASE + 2; // 释放未持有对象
pub const XC_OBJ_RANGE: u16 = SEG_BASE + 3; // 对象号越界
pub const XC_RANGE: u16 = SEG_BASE + 4; // 段界非法（len=0 或越顶）
pub const XC_SEQ_GAP: u16 = SEG_BASE + 5; // 回放序号失序/缺口
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 守恒破
pub const XC_EMPTY: u16 = SEG_BASE + 7; // 空账回放

/// 环账容量。
pub const RING_CAP: usize = 64;
/// 对象表容量。
pub const OBJ_CAP: usize = 32;
/// 段空间顶。
pub const SEG_TOP: usize = 256;

/// 流种闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// 分配。
    Alloc,
    /// 释放。
    Free,
    /// 复用。
    Reuse,
}

/// 环账行（五元组留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingRow {
    /// 流种。
    pub flow: Flow,
    /// 对象号。
    pub obj: u32,
    /// 段基。
    pub base: usize,
    /// 段长。
    pub len: usize,
    /// 序号。
    pub seq: u64,
}

/// 对象行（段持有表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjRow {
    /// 对象号。
    pub obj: u32,
    /// 当前在持段列表长度（简化：单段模型——每对象一活跃段）。
    pub live_seg: Option<(usize, usize)>,
    /// 累计分配段数。
    pub n_alloc: u64,
    /// 累计释放段数。
    pub n_free: u64,
}

/// 分配审计账。
pub struct AuditLedger {
    ring: Vec<RingRow>,
    /// 环写指针（下一写入位）。
    head: usize,
    /// 覆盖计数。
    overwrote: u64,
    objs: Vec<ObjRow>,
    seq: u64,
    /// 三流计数。
    c_alloc: u64,
    c_free: u64,
    c_reuse: u64,
}

impl AuditLedger {
    pub fn new() -> AuditLedger {
        AuditLedger {
            ring: Vec::new(),
            head: 0,
            overwrote: 0,
            objs: Vec::new(),
            seq: 0,
            c_alloc: 0,
            c_free: 0,
            c_reuse: 0,
        }
    }

    fn push(&mut self, flow: Flow, obj: u32, base: usize, len: usize) {
        self.seq += 1;
        let row = RingRow { flow, obj, base, len, seq: self.seq };
        if self.ring.len() < RING_CAP {
            self.ring.push(row);
        } else {
            // 环回覆盖最旧（head 位）
            self.ring[self.head] = row;
            self.head = (self.head + 1) % RING_CAP;
            self.overwrote += 1;
        }
    }

    fn obj_pos(&self, obj: u32) -> Option<usize> {
        self.objs.iter().position(|r| r.obj == obj)
    }

    /// 分配：对象开户 + 段登记 + Alloc/Reuse 判名落账。
    pub fn alloc(&mut self, obj: u32, base: usize, len: usize) -> Result<(), u16> {
        // 1. 段界
        if len == 0 || base + len > SEG_TOP {
            return Err(XC_RANGE);
        }
        // 2. 对象开户
        let pos = match self.obj_pos(obj) {
            Some(p) => p,
            None => {
                if self.objs.len() >= OBJ_CAP {
                    return Err(XC_OBJ_RANGE);
                }
                self.objs.push(ObjRow {
                    obj,
                    live_seg: None,
                    n_alloc: 0,
                    n_free: 0,
                });
                self.objs.len() - 1
            }
        };
        // 3. 单段模型：对象已持段再分配即红（须先放）
        if self.objs[pos].live_seg.is_some() {
            return Err(XC_DUP_FREE);
        }
        // 4. Reuse 判名：该对象曾有释放史 → 本次为复用
        let flow = if self.objs[pos].n_free > 0 { Flow::Reuse } else { Flow::Alloc };
        self.objs[pos].live_seg = Some((base, len));
        self.objs[pos].n_alloc += 1;
        match flow {
            Flow::Alloc => self.c_alloc += 1,
            Flow::Reuse => self.c_reuse += 1,
            Flow::Free => {}
        }
        self.push(flow, obj, base, len);
        self.check_conserve()
    }

    /// 释放：持有对拍 + Free 落账。
    pub fn free(&mut self, obj: u32) -> Result<(usize, usize), u16> {
        let pos = match self.obj_pos(obj) {
            Some(p) => p,
            None => return Err(XC_NOT_ALLOC),
        };
        let seg = match self.objs[pos].live_seg {
            Some(s) => s,
            None => return Err(XC_DUP_FREE),
        };
        self.objs[pos].live_seg = None;
        self.objs[pos].n_free += 1;
        self.c_free += 1;
        self.push(Flow::Free, obj, seg.0, seg.1);
        self.check_conserve()?;
        Ok(seg)
    }

    /// 守恒恒等式：分配事件（Alloc+Reuse）= 释放 + 在用段（对象面）；
    /// 总序号 = 三流合计（流水面）。
    pub fn check_conserve(&self) -> Result<(), u16> {
        let live = self.objs.iter().filter(|r| r.live_seg.is_some()).count();
        if self.c_alloc + self.c_reuse != self.c_free + live as u64 {
            return Err(XC_CONSERVE);
        }
        if self.c_alloc + self.c_free + self.c_reuse != self.seq {
            return Err(XC_CONSERVE);
        }
        Ok(())
    }

    /// 对象行读取。
    pub fn obj_row(&self, obj: u32) -> Result<ObjRow, u16> {
        match self.obj_pos(obj) {
            Some(p) => Ok(self.objs[p]),
            None => Err(XC_NOT_ALLOC),
        }
    }

    /// 环账在账行数。
    pub fn ring_len(&self) -> usize {
        self.ring.len()
    }

    /// 环账逻辑序读取：i 为在账内逻辑位（0=最旧在账行）。
    pub fn ring_row(&self, i: usize) -> Result<RingRow, u16> {
        if i >= self.ring.len() {
            return Err(XC_SEQ_GAP);
        }
        let phys = if self.overwrote == 0 {
            i
        } else {
            (self.head + i) % RING_CAP
        };
        Ok(self.ring[phys])
    }

    /// 回放：按序号区间 [from, to] 逐行核对全序递增（缺口即红）。
    pub fn replay(&self, from: u64, to: u64) -> Result<usize, u16> {
        if self.ring.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut n = 0usize;
        let mut prev = from.wrapping_sub(1);
        for i in 0..self.ring.len() {
            let r = self.ring_row(i)?;
            if r.seq >= from && r.seq <= to {
                if r.seq != prev + 1 {
                    return Err(XC_SEQ_GAP);
                }
                prev = r.seq;
                n += 1;
            }
        }
        if n == 0 {
            return Err(XC_SEQ_GAP);
        }
        Ok(n)
    }

    /// 环账守恒：覆盖数 + 在账数 = 总序号。
    pub fn ring_conserve(&self) -> bool {
        self.overwrote as usize + self.ring.len() == self.seq as usize
    }

    /// 三流计数读取。
    pub fn counters(&self) -> (u64, u64, u64) {
        (self.c_alloc, self.c_free, self.c_reuse)
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线：obj1 分配/释放/复分（Reuse 判名）
    let mut a = AuditLedger::new();
    a.alloc(1, 0x10, 8)?;
    if a.free(1)? != (0x10, 8) {
        return Err(XC_CONSERVE);
    }
    a.alloc(1, 0x10, 8)?;
    let (ca, cf, cr) = a.counters();
    if ca != 1 || cf != 1 || cr != 1 {
        return Err(XC_CONSERVE);
    }
    let o = a.obj_row(1)?;
    if o.n_alloc != 2 || o.n_free != 1 || o.live_seg != Some((0x10, 8)) {
        return Err(XC_CONSERVE);
    }
    // 2. 多对象并行：obj2/obj3 各持一段
    a.alloc(2, 0x20, 4)?;
    a.alloc(3, 0x30, 4)?;
    if a.ring_len() != 5 {
        return Err(XC_CONSERVE);
    }
    // 3. 错误路径①：已持段再分配
    if a.alloc(2, 0x28, 4).is_ok() {
        return Err(XC_DUP_FREE);
    }
    // 4. 错误路径②：释放未持有对象（obj4 未开户）
    if a.free(4).is_ok() {
        return Err(XC_NOT_ALLOC);
    }
    // 5. 错误路径③：段界非法（越顶）
    if a.alloc(4, 0xF0, 0x20).is_ok() {
        return Err(XC_RANGE);
    }
    // 6. 回放：全序递增核对 [1..5]
    if a.replay(1, 5)? != 5 {
        return Err(XC_SEQ_GAP);
    }
    // 7. 回放错误路径：区间无行
    if a.replay(100, 200).is_ok() {
        return Err(XC_SEQ_GAP);
    }
    // 8. 环回覆盖：灌满 64+ 行 → 覆盖计数对账
    for i in 0..70u32 {
        // obj 轮换分/放，制造 70 笔流水
        let obj = 10 + (i % 4);
        let live = match a.obj_row(obj) {
            Ok(r) => r.live_seg.is_some(),
            Err(_) => false,
        };
        if live {
            a.free(obj)?;
        } else {
            a.alloc(obj, (0x40 + i * 2) as usize, 2)?;
        }
    }
    if !a.ring_conserve() {
        return Err(XC_CONSERVE);
    }
    // 9. 覆盖后逻辑序仍全序递增（在账 64 行）
    if a.ring_len() != 64 {
        return Err(XC_CONSERVE);
    }
    let first = a.ring_row(0)?.seq;
    let last = a.ring_row(63)?.seq;
    if last != first + 63 {
        return Err(XC_SEQ_GAP);
    }
    a.check_conserve()?;
    Ok(0)
}
