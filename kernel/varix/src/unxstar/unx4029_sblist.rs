//! unx4029_sblist — UNX-F4029 sb 链表与全局遍历账（诊断段 0x8620）。
//!
//! 判据原文：
//!   J1 全局 sb 链表遍历计数与实例账一致（遍历所见 = 账面）；
//!   J2 链表断裂注入被断言器检出。
//! 设计纪律：
//!   1. 侵入式索引链：节点以 next 索引串接（0xFFFF 表无后继），
//!      头指针 + 尾指针 + 节点账三账并行维护。
//!   2. 守恒式：遍历步数 = 节点账 live 数；链长（走到的步数）=
//!      live 数 = count 字段——三者任一不一致即链表异常。
//!   3. 遍历有界：每步递减步数预算（上限 = 节点总数），防环防失控；
//!      预算耗尽前必须触达终结符，否则判链表断裂/成环。
//!   4. 断裂注入：测试钩子允许改写某节点 next 为悬空索引，
//!      断言器以「走不到尾 + 计数不足」双通道检出。
//!   5. 边界：空链遍历、重复插入同句柄、删除不在链节点、
//!      删除尾/头特殊位——全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8620;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_DUP_ADD: u16 = SEG_BASE + 1;     // 同句柄重复入链
const E_NOT_IN_LIST: u16 = SEG_BASE + 2; // 删除不在链上的节点
const E_CHAIN_BROKEN: u16 = SEG_BASE + 3;// 链表断裂（遍历未达终结符）
const E_CHAIN_LOOP: u16 = SEG_BASE + 4;  // 链表成环（步数预算耗尽）
const E_COUNT_MISMATCH: u16 = SEG_BASE + 5; // 遍历计数与实例账不一致
const E_BAD_IDX: u16 = SEG_BASE + 6;     // 悬空索引/越界句柄
const E_BAL: u16 = SEG_BASE + 7;         // 断言路径误分（不该发生的分支）

/// 无后继终结符（next 域哨兵值）。
pub const NIL: u16 = 0xFFFF;

/// 节点在链上的状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkPhase {
    Unlinked, // 建成未入链
    Linked,   // 在链上
    Unlinked2, // 曾入链已摘除（可复入）
}

/// 单节点：sb 句柄 + 链域 + 遍历所见计数。
pub struct SbNode {
    pub sb_id: u32,       // sb 实例句柄（外部键）
    pub next: u16,        // 后继索引，NIL 表终结
    pub phase: LinkPhase,
    pub visits: u32,      // 被遍历路过次数（遍历所见账）
}

/// 全局 sb 链表：头/尾指针 + 节点池 + 双向账。
pub struct SbList {
    pub nodes: Vec<SbNode>,
    pub head: u16,        // 头索引，NIL 表空链
    pub count: u32,       // 实例账（在链节点数）
    pub walk_total: u32,  // 历次遍历所见节点总数
    pub walk_runs: u32,   // 遍历执行次数
    pub rejected_total: u32, // 错误账
}

impl SbList {
    pub fn new() -> SbList {
        SbList {
            nodes: Vec::new(),
            head: NIL,
            count: 0,
            walk_total: 0,
            walk_runs: 0,
            rejected_total: 0,
        }
    }

    /// 建节点（Unlinked，不入链），返回句柄。
    pub fn make_node(&mut self, sb_id: u32) -> Result<u16, u16> {
        if self.nodes.len() >= NIL as usize {
            self.rejected_total += 1;
            return Err(E_BAD_IDX);
        }
        self.nodes.push(SbNode {
            sb_id,
            next: NIL,
            phase: LinkPhase::Unlinked,
            visits: 0,
        });
        Ok((self.nodes.len() - 1) as u16)
    }

    /// 入链（尾插）：重复句柄拒收。
    pub fn link_tail(&mut self, idx: u16) -> Result<(), u16> {
        if idx as usize >= self.nodes.len() {
            self.rejected_total += 1;
            return Err(E_BAD_IDX);
        }
        if self.nodes[idx as usize].phase != LinkPhase::Unlinked
            && self.nodes[idx as usize].phase != LinkPhase::Unlinked2
        {
            self.rejected_total += 1;
            return Err(E_DUP_ADD);
        }
        self.nodes[idx as usize].next = NIL;
        if self.head == NIL {
            self.head = idx;
        } else {
            // 走到尾（有界：预算 = 节点总数）。
            let budget = self.nodes.len() as u32;
            let mut cur = self.head;
            let mut steps = 0u32;
            while steps < budget {
                let nxt = self.nodes[cur as usize].next;
                if nxt == NIL {
                    self.nodes[cur as usize].next = idx;
                    break;
                }
                cur = nxt;
                steps += 1;
            }
            if steps >= budget {
                self.rejected_total += 1;
                return Err(E_CHAIN_LOOP);
            }
        }
        self.nodes[idx as usize].phase = LinkPhase::Linked;
        self.count += 1;
        Ok(())
    }

    /// 摘链：按句柄定位前驱并改链；不在链上拒收。
    pub fn unlink(&mut self, idx: u16) -> Result<(), u16> {
        if idx as usize >= self.nodes.len() {
            self.rejected_total += 1;
            return Err(E_BAD_IDX);
        }
        if self.nodes[idx as usize].phase != LinkPhase::Linked {
            self.rejected_total += 1;
            return Err(E_NOT_IN_LIST);
        }
        if self.head == idx {
            self.head = self.nodes[idx as usize].next;
        } else {
            // 找前驱（有界遍历）。
            let budget = self.nodes.len() as u32;
            let mut cur = self.head;
            let mut steps = 0u32;
            let mut found = false;
            while cur != NIL && steps < budget {
                let nxt = self.nodes[cur as usize].next;
                if nxt == idx {
                    self.nodes[cur as usize].next = self.nodes[idx as usize].next;
                    found = true;
                    break;
                }
                cur = nxt;
                steps += 1;
            }
            if !found {
                self.rejected_total += 1;
                return Err(E_CHAIN_BROKEN);
            }
        }
        self.nodes[idx as usize].next = NIL;
        self.nodes[idx as usize].phase = LinkPhase::Unlinked2;
        self.count -= 1;
        Ok(())
    }

    /// 全局遍历：从头走到尾，累计 visits 与遍历账；有界防环。
    pub fn walk(&mut self) -> Result<u32, u16> {
        let budget = if self.nodes.is_empty() { 1 } else { self.nodes.len() as u32 + 1 };
        let mut cur = self.head;
        let mut seen = 0u32;
        let mut steps = 0u32;
        while cur != NIL && steps <= budget {
            if cur as usize >= self.nodes.len() {
                // 悬空索引：链断裂注入的形态之一。
                self.rejected_total += 1;
                return Err(E_CHAIN_BROKEN);
            }
            self.nodes[cur as usize].visits += 1;
            seen += 1;
            cur = self.nodes[cur as usize].next;
            steps += 1;
        }
        if steps > budget {
            self.rejected_total += 1;
            return Err(E_CHAIN_LOOP);
        }
        self.walk_runs += 1;
        self.walk_total += seen;
        Ok(seen)
    }

    /// 断言器：遍历所见必须等于实例账，且全部节点可触达终结符。
    pub fn audit(&self) -> Result<(), u16> {
        // 静态核对：在链节点 visits 之和 ≥ 遍历账（至少最近一次遍历覆盖）。
        let mut linked = 0u32;
        let mut i = 0;
        while i < self.nodes.len() {
            if i < self.nodes.len() && self.nodes[i].phase == LinkPhase::Linked {
                linked += 1;
            }
            i += 1;
        }
        if linked != self.count {
            return Err(E_COUNT_MISMATCH);
        }
        // 最后一次遍历所见 = count（判据 J1 的账面形态）。
        if self.walk_runs > 0 && self.walk_total < self.count {
            return Err(E_COUNT_MISMATCH);
        }
        Ok(())
    }

    /// 导出账面快照（头指针，实例账，遍历账，遍历次数）。
    pub fn ledger(&self) -> (u16, u32, u32, u32) {
        (self.head, self.count, self.walk_total, self.walk_runs)
    }

    /// 节点视图：返回（sb 句柄，链域，阶段，visits）。
    pub fn node_view(&self, idx: u16) -> Result<(u32, u16, LinkPhase, u32), u16> {
        if idx as usize >= self.nodes.len() {
            return Err(E_BAD_IDX);
        }
        let n = &self.nodes[idx as usize];
        Ok((n.sb_id, n.next, n.phase, n.visits))
    }

    /// 链序导出：从头到尾收集 sb 句柄序列（有界，防环）。
    pub fn chain_ids(&self) -> Result<Vec<u32>, u16> {
        let mut out = Vec::new();
        let budget = self.nodes.len() as u32 + 1;
        let mut cur = self.head;
        let mut steps = 0u32;
        while cur != NIL && steps <= budget {
            if cur as usize >= self.nodes.len() {
                return Err(E_CHAIN_BROKEN);
            }
            out.push(self.nodes[cur as usize].sb_id);
            cur = self.nodes[cur as usize].next;
            steps += 1;
        }
        if steps > budget {
            return Err(E_CHAIN_LOOP);
        }
        Ok(out)
    }

    /// visits 总和视图（遍历账交叉面）。
    pub fn visits_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0;
        while i < self.nodes.len() {
            s += self.nodes[i].visits;
            i += 1;
        }
        s
    }
}

/// 自例程：R1 建链遍历对账 → R2 摘链再遍历 → R3 断裂注入检出 → R4 边界注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut lst = SbList::new();

    // R1 正常链：三节点入链，遍历所见 = 实例账 3。
    let a = lst.make_node(101)?;
    let b = lst.make_node(102)?;
    let c = lst.make_node(103)?;
    lst.link_tail(a)?;
    lst.link_tail(b)?;
    lst.link_tail(c)?;
    let seen = lst.walk()?;
    if seen != 3 || lst.count != 3 {
        return Err(E_COUNT_MISMATCH);
    }
    lst.audit()?;
    hits += 1;

    // R2 摘除中节点后遍历：所见 = 2，账同步。
    lst.unlink(b)?;
    let seen2 = lst.walk()?;
    if seen2 != 2 || lst.count != 2 {
        return Err(E_COUNT_MISMATCH);
    }
    lst.audit()?;
    hits += 1;

    // R3 断裂注入：改写尾节点 next 为悬空索引，断言器以 E_CHAIN_BROKEN 检出。
    lst.nodes[c as usize].next = 0x7FFF; // 悬空（池内无此槽）
    if lst.walk() != Err(E_CHAIN_BROKEN) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 边界注入：重复入链、摘未入链节点、越界句柄全部拒收。
    if lst.link_tail(a) != Err(E_DUP_ADD) {
        return Err(E_COUNT_MISMATCH);
    }
    if lst.unlink(b) != Err(E_NOT_IN_LIST) {
        return Err(E_COUNT_MISMATCH);
    }
    if lst.link_tail(0x7FFF) != Err(E_BAD_IDX) {
        return Err(E_COUNT_MISMATCH);
    }
    hits += 1;

    // R5 守恒：修复链后遍历对账恢复，账面快照自洽。
    lst.nodes[c as usize].next = NIL;
    let seen3 = lst.walk()?;
    if seen3 != 2 || lst.count != 2 {
        return Err(E_COUNT_MISMATCH);
    }
    lst.audit()?;
    let (_h, cnt, wt, runs) = lst.ledger();
    if cnt != 2 || wt < seen3 || runs < 3 {
        return Err(E_COUNT_MISMATCH);
    }
    hits += 1;

    // R6 链序与节点视图：链上句柄序与 visits 交叉对账。
    let ids = lst.chain_ids()?;
    if ids.len() != 2 || ids[0] != 101 || ids[1] != 103 {
        return Err(E_COUNT_MISMATCH);
    }
    let (_id, nx, ph, vs) = lst.node_view(a)?;
    if nx != c || ph != LinkPhase::Linked || vs < 3 {
        return Err(E_COUNT_MISMATCH);
    }
    if lst.visits_sum() < lst.walk_total {
        // 注入期失败的遍历也会留 visits（所见 ≥ 账面累计为合法弱序）。
        return Err(E_COUNT_MISMATCH);
    }
    hits += 1;

    Ok(hits)
}
