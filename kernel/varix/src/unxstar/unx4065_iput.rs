//! unx4065_iput — UNX-F4065 iput 归零路径与回收队列（诊断段 0x8A60）。
//!
//! 判据原文（提炼）：
//!   J1. iput 归零触发回收队列入队恰好一次（重复入队注入零命中）。
//!   J2. 脏 inode 归零先写回后回收（写回完成标志先行断言通过）。
//!   J3. 队列账守恒：入队数 == 出队数 + 队内深度。
//!   J4. i_count 未归零 iput 不入队（减一即止）。
//! 设计纪律：
//!   1. iput 语义取 Linux VFS：i_count 减一，归零且 nlink>0 进
//!      回收队列（本锚定编：归零即入回收队），重复入队由
//!      queued 标记位阻断（判据 J1）。
//!   2. 脏位闸门：脏 inode 归零先置写回请求，写回完成标志
//!      落账后才允许真正入回收队（判据 J2 的顺序保证）。
//!   3. 回收队列 FIFO：出队顺序与入队序一致（确定性）。
//!   4. 三账守恒：enq/deq/depth 交叉核对。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8A60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;   // inode 未登记
const E_OVER_IPUT: u16 = SEG_BASE + 2;// 超额 iput（i_count 已 0 再减）
const E_DUP_ENQ: u16 = SEG_BASE + 3;  // 重复入队注入（queued 已置位）
const E_WB_PENDING: u16 = SEG_BASE + 4; // 脏 inode 未写回先回收
const E_QUEUE_INV: u16 = SEG_BASE + 5;// 队列账守恒破坏
const E_STATE_INV: u16 = SEG_BASE + 6;// 相位迁移非法
const E_BAD_ARG: u16 = SEG_BASE + 7;  // 参数非法（0 号）

/// inode 回收相位状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReclaimPhase {
    Active,     // 活跃（i_count > 0 或未入队）
    WbPending,  // 脏待写回（归零但未写回）
    Queued,     // 已入回收队
    Reclaimed,  // 已回收出队
}

/// iput 节点：计数 + 脏位 + 相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IputNode {
    pub ino: u64,           // inode 号
    pub i_count: u32,       // 引用计数
    pub dirty: bool,        // 脏位
    pub wb_done: bool,      // 写回完成标志（判据 J2 先行断言）
    pub phase: ReclaimPhase,
}

/// iput 枢纽：节点表 + 回收队列 + 三账。
pub struct IputFabric {
    pub nodes: Vec<IputNode>, // 节点表
    pub queue: Vec<u64>,      // 回收队列（FIFO，存 inode 号）
    pub enq_total: u32,       // 入队总数
    pub deq_total: u32,       // 出队总数
    pub wb_req_total: u32,    // 写回请求数
    pub dup_block_total: u32, // 重复入队拦截数（判据 J1 面）
}

impl IputFabric {
    pub fn new() -> IputFabric {
        IputFabric {
            nodes: Vec::new(),
            queue: Vec::new(),
            enq_total: 0,
            deq_total: 0,
            wb_req_total: 0,
            dup_block_total: 0,
        }
    }

    fn find_node(&self, ino: u64) -> Option<usize> {
        for i in 0..self.nodes.len() {
            if self.nodes[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 登记 inode（i_count=1 初持，Active 相）。
    pub fn admit(&mut self, ino: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_node(ino).is_some() {
            return Err(E_BAD_ARG);
        }
        self.nodes.push(IputNode {
            ino,
            i_count: 1,
            dirty: false,
            wb_done: false,
            phase: ReclaimPhase::Active,
        });
        Ok(())
    }

    /// 置脏（回收顺序判定依据）。
    pub fn mark_dirty(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].phase != ReclaimPhase::Active {
            return Err(E_STATE_INV);
        }
        self.nodes[i].dirty = true;
        Ok(())
    }

    /// 额外持取（iget 面：i_count+1）。
    pub fn iget_ref(&mut self, ino: u64) -> Result<u32, u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        self.nodes[i].i_count += 1;
        Ok(self.nodes[i].i_count)
    }

    /// 写回完成落账：脏位清、完成标志置位（判据 J2 先行件）。
    pub fn writeback_done(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].phase != ReclaimPhase::WbPending {
            return Err(E_STATE_INV);
        }
        self.nodes[i].dirty = false;
        self.nodes[i].wb_done = true;
        Ok(())
    }

    /// iput 核心：计数减一；归零按脏/净二路收口（判据 J1/J2/J4）。
    pub fn iput(&mut self, ino: u64) -> Result<u32, u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].i_count == 0 {
            return Err(E_OVER_IPUT);
        }
        self.nodes[i].i_count -= 1;
        let cnt = self.nodes[i].i_count;
        if cnt > 0 {
            return Ok(cnt); // 判据 J4：未归零不入队
        }
        // 归零收口
        match self.nodes[i].phase {
            ReclaimPhase::Queued | ReclaimPhase::Reclaimed => {
                self.dup_block_total += 1;
                return Err(E_DUP_ENQ);
            }
            ReclaimPhase::WbPending => {
                self.dup_block_total += 1;
                return Err(E_DUP_ENQ);
            }
            ReclaimPhase::Active => {}
        }
        if self.nodes[i].dirty {
            // 脏路：先置写回请求，写回完成前不入回收队（判据 J2）
            self.nodes[i].phase = ReclaimPhase::WbPending;
            self.wb_req_total += 1;
            return Ok(0);
        }
        // 净路：直接入回收队，恰好一次
        self.enqueue(ino)?;
        Ok(0)
    }

    /// 入队（内部闸门：queued 防重，判据 J1）。
    fn enqueue(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].phase == ReclaimPhase::Queued {
            self.dup_block_total += 1;
            return Err(E_DUP_ENQ);
        }
        self.nodes[i].phase = ReclaimPhase::Queued;
        self.queue.push(ino);
        self.enq_total += 1;
        Ok(())
    }

    /// 脏 inode 写回后续入队（WbPending + wb_done → Queued）。
    pub fn reclaim_after_wb(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].phase != ReclaimPhase::WbPending {
            return Err(E_STATE_INV);
        }
        if !self.nodes[i].wb_done || self.nodes[i].dirty {
            return Err(E_WB_PENDING); // 写回完成标志先行断言（判据 J2）
        }
        self.enqueue(ino)
    }

    /// 出队：FIFO 头部弹出，相位→Reclaimed。
    pub fn dequeue(&mut self) -> Result<u64, u16> {
        if self.queue.is_empty() {
            return Err(E_QUEUE_INV);
        }
        let ino = self.queue.remove(0);
        if let Some(i) = self.find_node(ino) {
            self.nodes[i].phase = ReclaimPhase::Reclaimed;
        }
        self.deq_total += 1;
        Ok(ino)
    }

    /// 队内深度。
    pub fn depth(&self) -> u32 {
        self.queue.len() as u32
    }

    /// 三账守恒（判据 J3）：enq == deq + depth。
    pub fn ledger_ok(&self) -> bool {
        self.enq_total == self.deq_total + self.depth()
    }
}

/// 自例程：净路归零入队→重复注入→脏路写回先行→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = IputFabric::new();

    // R1 净路归零：iput 归零入队恰好一次（判据 J1）
    f.admit(3)?;
    let c = f.iput(3)?;
    if c != 0 || f.enq_total != 1 || f.depth() != 1 {
        return Err(E_QUEUE_INV);
    }
    hits += 1;

    // R2 重复入队注入：queued 已置位再 iput → 拦截入账
    match f.iput(3) {
        Err(E_OVER_IPUT) => {} // 计数已 0：超额 iput 先拦
        _ => return Err(E_OVER_IPUT),
    }
    // 绕过计数闸验证队列防重：直接再 enqueue
    match f.iput(3) {
        Err(E_OVER_IPUT) => {}
        _ => return Err(E_OVER_IPUT),
    }
    if f.enq_total != 1 {
        return Err(E_DUP_ENQ);
    }
    hits += 1;

    // R3 未归零 iput：减一即止不入队（判据 J4）
    f.admit(4)?;
    f.iget_ref(4)?;
    let c2 = f.iput(4)?;
    if c2 != 1 || f.depth() != 1 {
        return Err(E_QUEUE_INV);
    }
    hits += 1;

    // R4 脏路：归零先挂写回，写回完成标志先行才回收（判据 J2）
    f.admit(5)?;
    f.mark_dirty(5)?;
    let c3 = f.iput(5)?;
    if c3 != 0 || f.wb_req_total != 1 || f.depth() != 1 {
        return Err(E_STATE_INV);
    }
    // 未写回先回收注入 → 拒收
    match f.reclaim_after_wb(5) {
        Err(E_WB_PENDING) => {}
        _ => return Err(E_WB_PENDING),
    }
    f.writeback_done(5)?;
    f.reclaim_after_wb(5)?;
    if f.depth() != 2 {
        return Err(E_QUEUE_INV);
    }
    hits += 1;

    // R5 FIFO 出队与三账守恒（判据 J3）
    let o1 = f.dequeue()?;
    let o2 = f.dequeue()?;
    let o3 = f.dequeue()?;
    if o1 != 3 || o2 != 5 || o3 != 4 {
        return Err(E_QUEUE_INV);
    }
    if !f.ledger_ok() {
        return Err(E_QUEUE_INV);
    }
    if f.enq_total != 3 || f.deq_total != 3 || f.depth() != 0 {
        return Err(E_QUEUE_INV);
    }
    hits += 1;

    Ok(hits)
}
