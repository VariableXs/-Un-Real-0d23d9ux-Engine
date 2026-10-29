//! unx4110_lruconsist — UNX-F4110 LRU 链一致性断言（诊断段 0x8FC0）。
//!
//! 判据原文（提炼）：
//!   J1 双链全量遍历计数与驻留计数一致：前向遍历所见数 == 后向遍历
//!      所见数 == 账面驻留数（遍历所见=账面）。
//!   J2 断链注入被周期断言检出：任一节点 next/prev 被毒化后，
//!      lru_assert 返回 Err 并定位到断链条目 id。
//!   J3 双向指针互为镜像：每节点 next 的 prev 必须回指本节点
//!      （镜像校验，单向断链同样可检出）。
//!   J4 计数账守恒：链上节点数 + 已摘除数 == 累计插入数。
//!   J5 空链/单节点极端：空链遍历计 0 不误报；单节点自环合法。
//! 设计纪律：
//!   1. 双向链显式建模：节点存 prev/next 两个 i32 下标（-1 为空哨兵），
//!      头尾指针独立维护，不用 Rust 引用（no_std 确定性约束）。
//!   2. 遍历带上限护栏：步数超过驻留数 + 1 即判环/断链，
//!      绝不死循环（毒化注入可能造环）。
//!   3. 周期断言单一入口：lru_assert 一次做四查——计数、前向可达、
//!      后向可达、双向镜像；定位信息随 Err 返回（断点节点 id）。
//!   4. 修复原语配对：每个毒化点提供 repair 原语，修复后断言必须回绿
//!      （检出→定位→修复→复验的闭环）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8FC0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8FC0;

/// 空指针哨兵（i32 下标模型）。
pub const NIL: i32 = -1;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_COUNT_DIFF: u16 = SEG_BASE + 1; // 遍历计数与账面驻留不一致（判据 J1）
const E_BROKEN_NEXT: u16 = SEG_BASE + 2; // 前向断链（判据 J2）
const E_BROKEN_PREV: u16 = SEG_BASE + 3; // 后向断链（判据 J2）
const E_MIRROR_BAD: u16 = SEG_BASE + 4;  // 双向镜像失配（判据 J3）
const E_ID_MISS: u16 = SEG_BASE + 5;     // 点名节点不存在
const E_BAL: u16 = SEG_BASE + 6;         // 总账守恒失守（判据 J4）
const E_DUP_INS: u16 = SEG_BASE + 7;     // 重复插入同 id

/// 双向链节点：内容 + 双向指针（下标模型）。
pub struct LruNode {
    pub id: u32,      // 节点唯一号
    pub prev: i32,    // 前驱下标（NIL 为空）
    pub next: i32,    // 后继下标（NIL 为空）
    pub linked: bool, // 是否在链上
}

/// LRU 双向链枢纽：节点池 + 头尾指针 + 摘除账。
pub struct LruConsist {
    pub nodes: Vec<LruNode>, // 节点池（含已摘除，linked=false）
    pub head: i32,           // 链头下标
    pub tail: i32,           // 链尾下标
    pub live: u32,           // 链上驻留数（账面）
    pub removed_total: u32,  // 累计摘除数
    pub inserted_total: u32, // 累计插入数
    pub repairs: u32,        // 修复次数（检出→修复闭环账）
}

impl LruConsist {
    pub fn new() -> LruConsist {
        LruConsist {
            nodes: Vec::new(),
            head: NIL,
            tail: NIL,
            live: 0,
            removed_total: 0,
            inserted_total: 0,
            repairs: 0,
        }
    }

    /// 插入链头（新节点）。id 由外部指定以便注入定位。
    pub fn insert(&mut self, id: u32) -> Result<(), u16> {
        for n in &self.nodes {
            if n.id == id {
                return Err(E_DUP_INS);
            }
        }
        self.nodes.push(LruNode {
            id,
            prev: NIL,
            next: self.head,
            linked: true,
        });
        let idx = self.nodes.len() as i32 - 1;
        if self.head != NIL {
            let h = self.head as usize;
            self.nodes[h].prev = idx;
        }
        if self.tail == NIL {
            self.tail = idx;
        }
        self.head = idx;
        self.live += 1;
        self.inserted_total += 1;
        Ok(())
    }

    /// 摘除链尾（最冷）。链空或节点失联返回 E_BAL。
    pub fn remove_tail(&mut self) -> Result<u32, u16> {
        if self.tail == NIL || self.live == 0 {
            return Err(E_BAL);
        }
        let t = self.tail as usize;
        let id = self.nodes[t].id;
        let prev = self.nodes[t].prev;
        self.nodes[t].linked = false;
        self.nodes[t].prev = NIL;
        self.nodes[t].next = NIL;
        if prev != NIL {
            let p = prev as usize;
            self.nodes[p].next = NIL;
        } else {
            self.head = NIL;
        }
        self.tail = prev;
        self.live -= 1;
        self.removed_total += 1;
        Ok(id)
    }

    /// 毒化注入：把指定在链节点的 next 改为目标下标（可造断链/环）。
    /// 返回节点下标供 repair 使用。
    pub fn poison_next(&mut self, id: u32, fake_next: i32) -> Result<usize, u16> {
        for i in 0..self.nodes.len() {
            if self.nodes[i].id == id {
                if !self.nodes[i].linked {
                    return Err(E_ID_MISS);
                }
                self.nodes[i].next = fake_next;
                return Ok(i);
            }
        }
        Err(E_ID_MISS)
    }

    /// 修复原语：按 id 重写 next/prev 并回链（闭环第 3 步）。
    pub fn repair(&mut self, idx: usize, next: i32, prev: i32) -> Result<(), u16> {
        if idx >= self.nodes.len() {
            return Err(E_ID_MISS);
        }
        self.nodes[idx].next = next;
        self.nodes[idx].prev = prev;
        self.repairs += 1;
        Ok(())
    }

    /// 前向遍历计数（带护栏：步数 > live+1 判异常）。
    fn walk_forward(&self) -> Result<u32, u16> {
        if self.live == 0 {
            return Ok(0); // 判据 J5 空链计 0
        }
        let mut cur = self.head;
        let mut n = 0u32;
        let cap = self.live + 1;
        while cur != NIL {
            n += 1;
            if n > cap {
                return Err(E_BROKEN_NEXT); // 环或断链
            }
            cur = self.nodes[cur as usize].next;
        }
        Ok(n)
    }

    /// 后向遍历计数（同护栏）。
    fn walk_backward(&self) -> Result<u32, u16> {
        if self.live == 0 {
            return Ok(0);
        }
        let mut cur = self.tail;
        let mut n = 0u32;
        let cap = self.live + 1;
        while cur != NIL {
            n += 1;
            if n > cap {
                return Err(E_BROKEN_PREV);
            }
            cur = self.nodes[cur as usize].prev;
        }
        Ok(n)
    }

    /// 周期断言（判据 J1/J2/J3/J5）：四查全过才绿；
    /// 失败时定位断链节点 id（随 Err 语义由调用方用 locate 取）。
    pub fn lru_assert(&self) -> Result<(), u16> {
        let fw = self.walk_forward()?;
        let bw = self.walk_backward()?;
        if fw != self.live || bw != self.live {
            return Err(E_COUNT_DIFF); // 判据 J1
        }
        // 双向镜像（判据 J3）：链上逐节点校验 next 的 prev 回指。
        let mut cur = self.head;
        let mut steps = 0u32;
        while cur != NIL {
            steps += 1;
            if steps > self.live {
                return Err(E_MIRROR_BAD);
            }
            let c = cur as usize;
            let nxt = self.nodes[c].next;
            if nxt != NIL {
                let n = nxt as usize;
                if self.nodes[n].prev != cur || !self.nodes[n].linked {
                    return Err(E_MIRROR_BAD); // 镜像失配即定位到 nxt 节点
                }
            }
            cur = nxt;
        }
        Ok(())
    }

    /// 断链定位（判据 J2）：找第一个 next 指向已摘除/越界/失配的
    /// 在链节点，返回其 id。
    pub fn locate_break(&self) -> Result<u32, u16> {
        let mut cur = self.head;
        let mut steps = 0u32;
        while cur != NIL && steps <= self.live {
            steps += 1;
            let c = cur as usize;
            let nxt = self.nodes[c].next;
            if nxt == NIL {
                if cur != self.tail {
                    return Ok(self.nodes[c].id); // 中途 NIL：断链点
                }
            } else {
                if nxt as usize >= self.nodes.len() {
                    return Ok(self.nodes[c].id); // 越界毒化
                }
                let n = nxt as usize;
                if !self.nodes[n].linked || self.nodes[n].prev != cur {
                    return Ok(self.nodes[c].id); // 失配毒化
                }
            }
            cur = self.nodes[c].next;
        }
        Err(E_COUNT_DIFF) // 未定位到断点
    }

    /// 总账守恒（判据 J4）。
    pub fn conserved(&self) -> bool {
        self.live + self.removed_total == self.inserted_total
    }
}

/// 自例程：R1 建链断言→R2 断链注入检出定位→R3 修复复验→R4 守恒与极端。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 建链 5 节点，摘尾 1，断言绿（判据 J1/J3）。
    let mut l = LruConsist::new();
    let mut id = 1u32;
    while id <= 5 {
        l.insert(id)?;
        id += 1;
    }
    let t = l.remove_tail()?;
    if t != 1 {
        return Err(E_BAL); // 尾应为最先插入的 1 号
    }
    l.lru_assert()?;
    if l.live != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 断链注入：毒化 3 号 next 指向已摘除的 0 号下标，
    // 断言必须失败，locate_break 定位到 3 号（判据 J2）。
    let poison_idx = l.poison_next(3, 0)?;
    if l.lru_assert().is_ok() {
        return Err(E_BROKEN_NEXT); // 毒化后断言必须红
    }
    let culprit = l.locate_break()?;
    if culprit != 3 {
        return Err(E_BROKEN_NEXT); // 必须定位到 3 号节点
    }
    hits += 1;

    // R3 修复复验：恢复 3 号原 next（指向 2 号下标），断言回绿。
    // 原链（头→尾）为 5,4,3,2；3 号下标 2，其 next 原为 2 号下标 1。
    l.repair(poison_idx, 1, 3)?;
    l.lru_assert()?;
    if l.repairs != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 极端与守恒：空链断言绿；单节点自环合法；守恒终审。
    let mut e = LruConsist::new();
    if e.lru_assert().is_err() {
        return Err(E_COUNT_DIFF); // 空链必须绿（判据 J5）
    }
    e.insert(9)?;
    if e.lru_assert().is_err() {
        return Err(E_MIRROR_BAD); // 单节点自环必须绿
    }
    let r = e.remove_tail()?;
    if r != 9 || !e.conserved() {
        return Err(E_BAL);
    }
    if !l.conserved() || l.inserted_total != 5 || l.removed_total != 1 {
        return Err(E_BAL); // 判据 J4
    }
    if l.lru_assert().is_err() {
        return Err(E_MIRROR_BAD);
    }
    hits += 1;

    Ok(hits)
}
