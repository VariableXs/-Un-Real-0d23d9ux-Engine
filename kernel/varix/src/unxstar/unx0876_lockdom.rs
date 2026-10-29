//! unx0876_lockdom —— UNX-F0876 · 分配器并发纪律与锁域一致性。
//! 锚点（CoRun Unxreal 主册）：UNX-F0876 · 分配器并发纪律与锁域一致性。
//! 判据原文摘录（卡面验收行）：分配器并发纪律与锁域一致性
//!           ——锁域获取/释放成对、嵌套违序与重入全拦截、锁序图
//!           无环（一致性可静态核）。
//! 诊断段 0x7900（本单独占，0x7901~0x790F 闭集）。
//!
//! 设计纪律：
//! - 锁域模型：每域一编号（0~15），获取栈逐层入栈，释放须栈顶对拍
//!   （LIFO 纪律）——非栈顶释放即违序红账。
//! - 重入拦截：同域二次获取（未释放再取）即重入红账。
//! - 锁序纪律：全域单调锁序图（域号严格递增获取合法，递减获取即
//!   逆序红账，防死锁）；释放不受限。
//! - 一致性静态核：锁序图以邻接矩阵建模，环检测（Floyd 传递闭包：
//!   若 i→j 且 j→i 则环）——无环即一致。
//! - 守恒恒等式：获取次数 = 释放次数 + 在栈深度（每域）；全域操作
//!   流水 = 获取 + 释放 + 拦截。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const LD_VERSION: &str = "unx0876_lockdom-v1";

/// 诊断段基址（0x7900 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7900;

/// XcCode 闭集（0x7901~0x790F）。
pub const XC_REENTER: u16 = SEG_BASE + 1; // 同域重入
pub const XC_ORDER_INV: u16 = SEG_BASE + 2; // 获取逆序（锁序破）
pub const XC_NOT_TOP: u16 = SEG_BASE + 3; // 非栈顶释放（LIFO 破）
pub const XC_EMPTY_POP: u16 = SEG_BASE + 4; // 空栈释放
pub const XC_DOM_RANGE: u16 = SEG_BASE + 5; // 域号越界（>15）
pub const XC_DEPTH: u16 = SEG_BASE + 6; // 栈深越界（>8）
pub const XC_CONSERVE: u16 = SEG_BASE + 7; // 守恒破
pub const XC_CYCLE: u16 = SEG_BASE + 8; // 锁序图有环

/// 域号上限（0~15）。
pub const DOM_MAX: usize = 16;
/// 栈深上限。
pub const DEPTH_MAX: usize = 8;

/// 锁域纪律器：单执行流视角的获取/释放序检查 + 全域锁序图。
pub struct LockDomain {
    /// 获取栈（域号序）。
    stack: Vec<usize>,
    /// 每域获取计数。
    acq: [u64; DOM_MAX],
    /// 每域释放计数。
    rel: [u64; DOM_MAX],
    /// 锁序图邻接矩阵：ord[a][b]=true 表示允许 a→b（a 先取 b 后取）。
    ord: [[bool; DOM_MAX]; DOM_MAX],
    /// 拦截计数。
    rejects: u64,
    /// 操作序号。
    seq: u64,
}

impl LockDomain {
    pub fn new() -> LockDomain {
        let mut ord = [[false; DOM_MAX]; DOM_MAX];
        // 缺省锁序：域号递增方向全通（a<b ⇒ a→b）
        for a in 0..DOM_MAX {
            for b in 0..DOM_MAX {
                if a < b {
                    ord[a][b] = true;
                }
            }
        }
        LockDomain {
            stack: Vec::new(),
            acq: [0; DOM_MAX],
            rel: [0; DOM_MAX],
            ord,
            rejects: 0,
            seq: 0,
        }
    }

    /// 自定义锁序边（覆盖缺省，测试面用）：声明 a→b 合法。
    pub fn set_order(&mut self, a: usize, b: usize, allow: bool) -> Result<(), u16> {
        if a >= DOM_MAX || b >= DOM_MAX {
            return Err(XC_DOM_RANGE);
        }
        self.ord[a][b] = allow;
        Ok(())
    }

    /// 获取域 d：重入 / 越界 / 逆序拦截。
    pub fn acquire(&mut self, d: usize) -> Result<(), u16> {
        self.seq += 1;
        // 1. 域号界
        if d >= DOM_MAX {
            self.rejects += 1;
            return Err(XC_DOM_RANGE);
        }
        // 2. 重入：在栈即红
        if self.stack.iter().any(|s| *s == d) {
            self.rejects += 1;
            return Err(XC_REENTER);
        }
        // 3. 栈深界
        if self.stack.len() >= DEPTH_MAX {
            self.rejects += 1;
            return Err(XC_DEPTH);
        }
        // 4. 锁序：对栈内每个已持域 a，须有 a→d 边
        for a in self.stack.iter() {
            if !self.ord[*a][d] {
                self.rejects += 1;
                return Err(XC_ORDER_INV);
            }
        }
        // 5. 入栈
        self.stack.push(d);
        self.acq[d] += 1;
        Ok(())
    }

    /// 释放域 d：LIFO 栈顶对拍。
    pub fn release(&mut self, d: usize) -> Result<(), u16> {
        self.seq += 1;
        // 1. 域号界 + 空栈
        if d >= DOM_MAX {
            self.rejects += 1;
            return Err(XC_DOM_RANGE);
        }
        if self.stack.is_empty() {
            self.rejects += 1;
            return Err(XC_EMPTY_POP);
        }
        // 2. 栈顶对拍
        let top = self.stack[self.stack.len() - 1];
        if top != d {
            self.rejects += 1;
            return Err(XC_NOT_TOP);
        }
        self.stack.pop();
        self.rel[d] += 1;
        Ok(())
    }

    /// 在栈深度。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 每域守恒：acq = rel + 在栈层数。
    pub fn per_dom_conserve(&self) -> bool {
        let mut on_stack = [0usize; DOM_MAX];
        for d in self.stack.iter() {
            on_stack[*d] += 1;
        }
        for d in 0..DOM_MAX {
            if self.acq[d] != self.rel[d] + on_stack[d] as u64 {
                return false;
            }
        }
        true
    }

    /// 全域守恒：流水 = 获取 + 释放 + 拦截。
    pub fn op_conserve(&self) -> bool {
        let acq_sum: u64 = self.acq.iter().sum();
        let rel_sum: u64 = self.rel.iter().sum();
        acq_sum + rel_sum + self.rejects == self.seq
    }

    /// 锁序图环检测：Floyd 传递闭包，存在 i→i 即环。
    pub fn order_acyclic(&self) -> bool {
        let mut r = self.ord;
        for k in 0..DOM_MAX {
            for i in 0..DOM_MAX {
                if r[i][k] {
                    for j in 0..DOM_MAX {
                        if r[k][j] {
                            r[i][j] = true;
                        }
                    }
                }
            }
        }
        for i in 0..DOM_MAX {
            if r[i][i] {
                return false;
            }
        }
        true
    }

    /// 拦截计数。
    pub fn rejects(&self) -> u64 {
        self.rejects
    }

    /// 某域获取计数。
    pub fn acq_of(&self, d: usize) -> Result<u64, u16> {
        if d >= DOM_MAX {
            return Err(XC_DOM_RANGE);
        }
        Ok(self.acq[d])
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线：0→1→2 递增获取，逆序释放 2→1→0
    let mut ld = LockDomain::new();
    ld.acquire(0)?;
    ld.acquire(1)?;
    ld.acquire(2)?;
    if ld.depth() != 3 {
        return Err(XC_CONSERVE);
    }
    ld.release(2)?;
    ld.release(1)?;
    ld.release(0)?;
    if ld.depth() != 0 || !ld.per_dom_conserve() || !ld.op_conserve() {
        return Err(XC_CONSERVE);
    }
    // 2. 错误路径①：同域重入
    ld.acquire(3)?;
    if ld.acquire(3).is_ok() {
        return Err(XC_REENTER);
    }
    ld.release(3)?;
    // 3. 错误路径②：非栈顶释放（取 4 后试图放 3）
    ld.acquire(3)?;
    ld.acquire(4)?;
    if ld.release(3).is_ok() {
        return Err(XC_NOT_TOP);
    }
    ld.release(4)?;
    ld.release(3)?;
    // 4. 错误路径③：空栈释放
    if ld.release(0).is_ok() {
        return Err(XC_EMPTY_POP);
    }
    // 5. 错误路径④：域号越界
    if ld.acquire(16).is_ok() {
        return Err(XC_DOM_RANGE);
    }
    // 6. 错误路径⑤：缺省锁序下的逆序获取（栈持 7 后取 3 → 7→3 无边）
    ld.acquire(7)?;
    if ld.acquire(3).is_ok() {
        return Err(XC_ORDER_INV);
    }
    ld.release(7)?;
    // 7. 自定义锁序边制造环：设 9→8 边（缺省已有 8→9）→ 闭包有环
    let mut ld2 = LockDomain::new();
    ld2.set_order(9, 8, true)?;
    ld2.acquire(8)?;
    ld2.acquire(9)?;
    ld2.release(9)?;
    ld2.release(8)?;
    if ld2.order_acyclic() {
        return Err(XC_CYCLE);
    }
    // 8. 无环面：撤销自设边 → 传递闭包无环
    ld2.set_order(9, 8, false)?;
    if !ld2.order_acyclic() {
        return Err(XC_CYCLE);
    }
    // 9. 无环图上逆序拦截生效：栈持 9 后取 8 拒（9→8 已撤）
    ld2.acquire(9)?;
    if ld2.acquire(8).is_ok() {
        return Err(XC_ORDER_INV);
    }
    ld2.release(9)?;
    // 10. 守恒核
    if !ld.per_dom_conserve() || !ld.op_conserve() || ld.rejects() < 4 {
        return Err(XC_CONSERVE);
    }
    if ld2.acq_of(9).is_err() {
        return Err(XC_DOM_RANGE);
    }
    Ok(0)
}
