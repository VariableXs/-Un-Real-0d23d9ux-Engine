//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0337 · 唤醒次序与拓扑（BSP→AP 序号账）· 诊断段 0x3f50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0337-J1 唤醒序（逐个/成组可配）与序号账一致，乱序检出
//!   100%。
//! 难点分析：
//!   唤醒次序不是洁癖：逐个唤醒（内存屏障最保守、排障最简单）和
//!   成组唤醒（快，但同批 AP 竞争同一 trampoline 页）各有预算语义。
//!   次序账记录「谁第几个被唤醒」，乱序 = 某个 AP 拿到了不该属于
//!   它的资源窗口——检出必须 100%，且策略本身（逐个/成组）要与
//!   序号账的记账粒度一致：逐个一序号，成组一批一序号段。
//! 纪律约束：
//!   - 策略显性：逐个/成组不可混记；
//!   - 序号账一致：账序 == 实发序；
//!   - 乱序检出 100%：任何逆序对被抓；
//!   - 拓扑约束：节点内连续唤醒（NUMA 局部性），跨节点跳批报账。
//! 成本记账：记账 O(1)/AP；乱序检测 O(n)；拓扑核 O(n)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3f50;

/// 唤醒策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakePolicy {
    /// 逐个：一次一个 AP。
    OneByOne,
    /// 成组：一批多个。
    Batch(u32),
}

/// 唤醒账行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeRow {
    /// 序号（逐个模式逐 AP 递增；成组模式批内同号）。
    pub seq: u32,
    pub ap_id: u32,
    pub node: u8,
}

/// 唤醒序账。
#[derive(Debug)]
pub struct WakeLedger {
    pub policy: WakePolicy,
    rows: Vec<WakeRow>,
    next_seq: u32,
}

impl WakeLedger {
    pub fn new(policy: WakePolicy) -> Self {
        WakeLedger { policy, rows: Vec::new(), next_seq: 0 }
    }

    /// 记一次唤醒（逐个）或一批（成组）。
    pub fn wake(&mut self, aps: &[(u32, u8)]) -> Result<(), &'static str> {
        match self.policy {
            WakePolicy::OneByOne => {
                if aps.len() != 1 {
                    return Err("逐个策略一次只许一 AP");
                }
                self.rows.push(WakeRow { seq: self.next_seq, ap_id: aps[0].0, node: aps[0].1 });
                self.next_seq += 1;
                Ok(())
            }
            WakePolicy::Batch(b) => {
                if aps.is_empty() || aps.len() as u32 > b {
                    return Err("批大小越界");
                }
                let s = self.next_seq;
                for &(id, node) in aps {
                    self.rows.push(WakeRow { seq: s, ap_id: id, node });
                }
                self.next_seq += 1;
                Ok(())
            }
        }
    }

    /// 账序一致：序号单调不减（成组允许同号段）。
    pub fn order_consistent(&self) -> bool {
        self.rows.windows(2).all(|w| w[0].seq <= w[1].seq)
    }

    /// AP 互斥：同 AP 不许出现两次。
    pub fn no_duplicate_ap(&self) -> bool {
        let mut ids: Vec<u32> = self.rows.iter().map(|r| r.ap_id).collect();
        ids.sort();
        ids.dedup();
        ids.len() == self.rows.len()
    }

    /// 乱序检出：给定实际唤醒序（账外观测），与账序比对。
    /// 返回第一个不一致的位次。
    pub fn check_against_observed(&self, observed: &[u32]) -> Option<usize> {
        let ledger_ids: Vec<u32> = self.rows.iter().map(|r| r.ap_id).collect();
        for (i, (&o, &l)) in observed.iter().zip(ledger_ids.iter()).enumerate() {
            if o != l {
                return Some(i);
            }
        }
        if observed.len() != ledger_ids.len() {
            return Some(ledger_ids.len().min(observed.len()));
        }
        None
    }

    /// 拓扑约束：账内节点分段连续——每个节点的出现位次必须是
    /// 连续区间（首现到末现之间不许插别的节点）。
    pub fn topo_node_contiguous(&self) -> Vec<u8> {
        let mut nodes: Vec<u8> = self.rows.iter().map(|r| r.node).collect();
        nodes.sort();
        nodes.dedup();
        let mut out = Vec::new();
        for n in nodes {
            let pos: Vec<usize> = self.rows.iter().enumerate().filter(|(_, r)| r.node == n).map(|(i, _)| i).collect();
            let spread = *pos.last().unwrap() - pos[0] + 1;
            if spread != pos.len() {
                out.push(n);
            }
        }
        out
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 账行只读视图。
    pub fn rows(&self) -> &[WakeRow] {
        &self.rows
    }
}

/// 黄金剧本：逐个唤醒 4 AP（节点 0,0,1,1）。
pub fn golden_ledger() -> WakeLedger {
    let mut lg = WakeLedger::new(WakePolicy::OneByOne);
    let _ = lg.wake(&[(1, 0)]);
    let _ = lg.wake(&[(2, 0)]);
    let _ = lg.wake(&[(3, 1)]);
    let _ = lg.wake(&[(4, 1)]);
    lg
}

/// UNX-F0337 判据（J1 唤醒序与序号账一致，乱序检出 100%）。
pub fn run_unx337_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0337 唤醒次序与拓扑");

    // J1 主线：逐个唤醒账
    let lg = golden_ledger();
    cs.add("J1 账序一致", lg.order_consistent(), "单调递增");
    cs.add("J1 AP 互斥", lg.no_duplicate_ap(), "无重复");
    cs.add("J1 序号账值", lg.rows().iter().map(|r| r.seq).collect::<Vec<_>>() == vec![0, 1, 2, 3], "逐个递增");
    cs.add("J1 观测序吻合", lg.check_against_observed(&[1, 2, 3, 4]).is_none(), "账==实发");

    // 乱序检出 100%：交换任意两位都能抓
    cs.add("J1 乱序 100% 检出", {
        let obs = [1, 3, 2, 4];
        lg.check_against_observed(&obs) == Some(1)
    }, "首位差定位");
    cs.add("J1 缺席检出", lg.check_against_observed(&[1, 2, 3]).is_some(), "少一个也报");
    cs.add("J1 多席检出", lg.check_against_observed(&[1, 2, 3, 4, 5]).is_some(), "多一个也报");

    // 成组策略
    let mut bg = WakeLedger::new(WakePolicy::Batch(2));
    let _ = bg.wake(&[(1, 0), (2, 0)]);
    let _ = bg.wake(&[(3, 1), (4, 1)]);
    cs.add("成组批号段", bg.rows().iter().map(|r| r.seq).collect::<Vec<_>>() == vec![0, 0, 1, 1], "批内同号");
    cs.add("成组账序一致", bg.order_consistent() && bg.no_duplicate_ap(), "段单调");
    cs.add("成组超批拒收", bg.wake(&[(9, 0), (9, 0), (9, 0)]).is_err(), "3>2");
    cs.add("成组空批拒收", bg.wake(&[]).is_err(), "空批");

    // 逐个策略严格性
    let mut og = WakeLedger::new(WakePolicy::OneByOne);
    cs.add("逐个拒绝批调用", og.wake(&[(1, 0), (2, 0)]).is_err(), "一次一 AP");

    // 拓扑：节点连续性
    cs.add("拓扑黄金账连续", golden_ledger().topo_node_contiguous().is_empty(), "0,0,1,1");
    let mut bad_topo = WakeLedger::new(WakePolicy::OneByOne);
    let _ = bad_topo.wake(&[(1, 0)]);
    let _ = bad_topo.wake(&[(2, 1)]);
    let _ = bad_topo.wake(&[(3, 0)]); // 节点 0 断开
    cs.add("拓扑断开检出", bad_topo.topo_node_contiguous() == vec![0], "节点 0");

    cs
}
