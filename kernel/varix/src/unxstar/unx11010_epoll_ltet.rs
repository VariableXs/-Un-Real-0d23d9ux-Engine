//! UNX-F11010 epoll LT/ET 挂点行为联测（两模式事件账）（0x1e140 段）
//!
//! 锚点：UNX-F11010-J1 LT 重复通知账逐次对平（10/10），ET 一次通知账
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：事件账（就绪沿登记 + 两模式通知计数）+ LT 重复通知器（未确认
//! 就绪逐轮重复通知）+ ET 一次通知器（每就绪沿恰通知一次）+ 守恒器（ET
//! 计数 = 就绪沿数，LT 计数 ≥ 就绪沿数）+ fold 指纹与冻结 + 注入核（未
//! 确认沿越权/零 fd/重复沿/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e140 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e140;
pub const LT_UNACKED: u32 = SEG_BASE + 0x01; // 未确认沿越权
pub const LT_NOFD: u32 = SEG_BASE + 0x02; // 零 fd
pub const LT_DUPEDGE: u32 = SEG_BASE + 0x03; // 就绪沿重号
pub const LT_CONSERVE: u32 = SEG_BASE + 0x04; // 两账守恒破坏
pub const LT_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const LT_CAP: u32 = SEG_BASE + 0x06; // 沿表超容

/// 千轮联测轮数。
pub const ROUNDS: u32 = 1000;
/// 沿表定容。
pub const EDGE_CAP: usize = 128;
/// 事件账域标（fold 种子）。
pub const LT_TAG: u64 = 0x1e14_0a;

/// 单条就绪沿：fd + 沿号 + LT 确认位。
#[derive(Clone, Debug)]
pub struct ReadyEdge {
    pub fd: u32,
    pub edge: u32,
    pub lt_acked: bool,
    pub et_notified: bool,
}

/// epoll LT/ET 事件账：就绪沿登记 → LT 重复通知 → ET 一次通知 → 冻结。
pub struct ModeLedger {
    pub edges: Vec<ReadyEdge>,
    pub lt_count: u32,
    pub et_count: u32,
    /// 本轮起算计数（沿表清面重用时带过去的累计基）。
    pub lt_base: u32,
    pub et_base: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ModeLedger {
    pub fn new() -> ModeLedger {
        ModeLedger {
            edges: Vec::new(),
            lt_count: 0,
            et_count: 0,
            lt_base: 0,
            et_base: 0,
            frozen: false,
            root: None,
        }
    }

    /// 沿表轮转重用：清沿面并推进双账起算基（计数跨轮累计不重置）。
    pub fn reopen_round(&mut self) {
        self.edges.clear();
        self.lt_base = self.lt_count;
        self.et_base = self.et_count;
    }

    /// 就绪沿登记：冻结、零 fd、同 fd 同沿重号、超容均拒。
    pub fn edge(&mut self, fd: u32, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        if fd == 0 {
            return Err(LT_NOFD);
        }
        if self.edges.iter().any(|e| e.fd == fd && e.edge == no) {
            return Err(LT_DUPEDGE);
        }
        if self.edges.len() >= EDGE_CAP {
            return Err(LT_CAP);
        }
        self.edges.push(ReadyEdge { fd, edge: no, lt_acked: false, et_notified: false });
        Ok(())
    }

    /// LT 轮：所有未确认就绪沿逐次通知一轮（重复通知语义）。
    pub fn lt_round(&mut self) -> u32 {
        if self.frozen {
            return 0;
        }
        let mut fired = 0u32;
        let mut k = 0usize;
        while k < self.edges.len() {
            if !self.edges[k].lt_acked {
                self.lt_count = self.lt_count.wrapping_add(1);
                fired += 1;
            }
            k += 1;
        }
        fired
    }

    /// LT 确认：逐沿确认后不再重复通知。
    pub fn lt_ack(&mut self, fd: u32, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        let mut k = 0usize;
        while k < self.edges.len() {
            if self.edges[k].fd == fd && self.edges[k].edge == no {
                if self.edges[k].lt_acked {
                    return Err(LT_UNACKED);
                }
                self.edges[k].lt_acked = true;
                return Ok(());
            }
            k += 1;
        }
        Err(LT_NOFD)
    }

    /// ET 通知：每沿恰一次，重复通知必拒。
    pub fn et_notify(&mut self, fd: u32, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        let mut k = 0usize;
        while k < self.edges.len() {
            if self.edges[k].fd == fd && self.edges[k].edge == no {
                if self.edges[k].et_notified {
                    return Err(LT_DUPEDGE);
                }
                self.edges[k].et_notified = true;
                self.et_count = self.et_count.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(LT_NOFD)
    }

    /// ET 守恒：本轮增量 = 已通知沿数且每沿恰一次。
    pub fn et_conserved(&self) -> bool {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.edges.len() {
            if self.edges[k].et_notified {
                n += 1;
            }
            k += 1;
        }
        n == self.et_count.wrapping_sub(self.et_base)
    }

    /// LT 守恒：本轮增量 ≥ 已登记沿数（未确认沿至少通知过一次）。
    pub fn lt_conserved(&self) -> bool {
        self.lt_count.wrapping_sub(self.lt_base) >= self.edges.len() as u32
    }

    /// 未确认沿清单。
    pub fn unacked(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.edges.len() {
            if !self.edges[k].lt_acked {
                g.push(self.edges[k].edge);
            }
            k += 1;
        }
        g
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/沿/双确认位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = LT_TAG;
        let mut k = 0usize;
        while k < self.edges.len() {
            let e = &self.edges[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.edge as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.lt_acked as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.et_notified as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.lt_count as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.et_count as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 沿号确定性样本。
pub fn lt_sample(no: u32) -> u32 {
    no.wrapping_mul(11).wrapping_add(1)
}

/// 千轮联测全链：每轮登记十沿、LT 十轮重复通知、ET 各通知一次。
pub fn run_mode_thousand() -> (ModeLedger, bool) {
    let mut l = ModeLedger::new();
    let mut ok = true;
    let mut r = 1u32;
    while r <= ROUNDS {
        // 沿表轮转重用：每轮先清面并推进起算基（两账计数跨轮累计）
        l.reopen_round();
        let mut i = 1u32;
        while i <= 10u32 {
            if l.edge(i, lt_sample(r)).is_err() {
                ok = false;
            }
            i += 1;
        }
        // LT：十轮重复通知后逐沿确认
        let mut t = 0u32;
        while t < 10u32 {
            let fired = l.lt_round();
            if fired != 10 {
                ok = false;
            }
            t += 1;
        }
        let mut i = 1u32;
        while i <= 10u32 {
            if l.lt_ack(i, lt_sample(r)).is_err() {
                ok = false;
            }
            i += 1;
        }
        // ET：每沿恰一次
        let mut i = 1u32;
        while i <= 10u32 {
            if l.et_notify(i, lt_sample(r)).is_err() {
                ok = false;
            }
            i += 1;
        }
        r += 1;
    }
    let l_ok = ok && l.et_count == ROUNDS * 10 && l.lt_count == ROUNDS * 100;
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复 LT 确认必拒
    let mut a = ModeLedger::new();
    let _ = a.edge(3, 1);
    let _ = a.lt_ack(3, 1);
    got[0] = a.lt_ack(3, 1).err() == Some(LT_UNACKED);
    // 2) 零 fd 必拒
    let mut b = ModeLedger::new();
    got[1] = b.edge(0, 1).err() == Some(LT_NOFD);
    // 3) 重复 ET 通知必拒
    let mut c = ModeLedger::new();
    let _ = c.edge(3, 1);
    let _ = c.et_notify(3, 1);
    got[2] = c.et_notify(3, 1).err() == Some(LT_DUPEDGE);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_mode_thousand();
    d.freeze();
    got[3] = d.edge(3, 1).err() == Some(LT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11010");

    // J1 LT 重复通知账逐次对平：千轮十沿十通知 = 十万
    let (mut book, green) = run_mode_thousand();
    s.add("J1-LT 重复通知账", green && book.lt_count == ROUNDS * 100, "千轮 LT 逐次通知零漂移");

    // J2 ET 一次通知账：千轮十沿各恰一次 = 万
    s.add("J2-ET 一次通知账", book.et_count == ROUNDS * 10 && book.et_conserved(), "每就绪沿恰通知一次零重复");

    // J3 LT 未确认清空：全部确认后 unacked 清空
    s.add("J3-未确认清空", book.unacked().is_empty(), "千轮确认后未确认清单清零");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：重复确认 + 零 fd
    let inj = inject_four();
    s.add("J5-确认零 fd 检出", inj[0] && inj[1], "重复确认与零 fd 两路必抓");

    // J6 注入检出：重复 ET + 冻结后写
    s.add("J6-重复冻结检出", inj[2] && inj[3], "重复 ET 必拒且冻结后写必抓");

    // J7 确认后 LT 停发：全确认后 lt_round 通知数为零
    let j7 = {
        let mut c = ModeLedger::new();
        let mut i = 1u32;
        while i <= 4u32 {
            let _ = c.edge(i, lt_sample(i));
            i += 1;
        }
        let _ = c.lt_round();
        let mut i = 1u32;
        while i <= 4u32 {
            let _ = c.lt_ack(i, lt_sample(i));
            i += 1;
        }
        c.lt_round() == 0 && c.unacked().is_empty()
    };
    s.add("J7-确认后停发", j7, "全确认后 LT 不再重复通知");

    // J8 LT 重复性实证：未确认沿两轮通知数相同（重复语义）
    let j8 = {
        let mut c = ModeLedger::new();
        let mut i = 1u32;
        while i <= 3u32 {
            let _ = c.edge(i, lt_sample(i));
            i += 1;
        }
        let f1 = c.lt_round();
        let f2 = c.lt_round();
        f1 == 3 && f2 == 3 && c.lt_count == 6
    };
    s.add("J8-重复语义实证", j8, "未确认沿逐轮重复通知");

    // J9 越界沿通知必拒（不存在的 fd）
    let j9 = {
        let mut c = ModeLedger::new();
        let _ = c.edge(3, 1);
        c.et_notify(99, 1).err() == Some(LT_NOFD)
    };
    s.add("J9-越界通知拒", j9, "通知不存在的沿必拒");

    // J10 超容拒：第 EDGE_CAP+1 沿必拒
    let j10 = {
        let mut c = ModeLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= EDGE_CAP as u32 {
            if c.edge(n.wrapping_add(1), n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.edge(999, 1).err() == Some(LT_CAP)
    };
    s.add("J10-超容边界", j10, "超出定容必拒且原账不损");

    // J11 LT/ET 比例不变式：LT 计数恰为 ET 十倍
    s.add("J11-双账十倍比", book.lt_count == book.et_count * 10, "同就绪面下 LT 恰为 ET 十倍");

    // J12 样本确定性：沿号同入同出且互异
    let v3a = lt_sample(3);
    let v3b = lt_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 16u32 {
        let mut j = i.wrapping_add(1);
        while j <= 16u32 {
            if lt_sample(i) == lt_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J12-样本确定性", v3a == v3b && distinct, "沿号样本同入同出且互异");

    // J13 部分确认：确认半数后 unacked 恰余半数且 LT 续发
    let j13 = {
        let mut c = ModeLedger::new();
        let mut i = 1u32;
        while i <= 6u32 {
            let _ = c.edge(i, lt_sample(i));
            i += 1;
        }
        let _ = c.lt_round();
        let mut i = 1u32;
        while i <= 3u32 {
            let _ = c.lt_ack(i, lt_sample(i));
            i += 1;
        }
        c.unacked().len() == 3 && c.lt_round() == 3
    };
    s.add("J13-部分确认续发", j13, "未确认半数续发确认半数停发");

    // J14 动态明细：双账计数落账
    let lc = book.lt_count;
    let ec = book.et_count;
    s.add_dyn(
        "J14-双账明细",
        lc == ROUNDS * 100 && ec == ROUNDS * 10,
        &format!("lt={} et={}", lc, ec),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11010_mode_flow() {
        let mut l = ModeLedger::new();
        assert!(l.edge(0, 1).err() == Some(LT_NOFD));
        assert!(l.edge(3, 1).is_ok());
        assert!(l.edge(3, 1).err() == Some(LT_DUPEDGE));
        assert_eq!(l.lt_round(), 1);
        assert!(l.lt_ack(3, 1).is_ok());
        assert!(l.lt_ack(3, 1).err() == Some(LT_UNACKED));
        assert!(l.et_notify(3, 1).is_ok());
        assert!(l.et_notify(3, 1).err() == Some(LT_DUPEDGE));
        l.freeze();
        assert!(l.edge(4, 2).err() == Some(LT_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11010_checks_green() {
        let s = run_unx11010_checks();
        assert!(s.all_passed(), "UNX-F11010 判据未全绿");
    }
}
