//! UNX-F10333 联签审计账（三面双向操作留痕）（0x1b420 段）
//!
//! 锚点：UNX-F10333-J1 三面双向对称——审计账逐件记（发出面, 接收面,
//! 操作），六个有序面对的来去件数必须逐对相等（对称），且六对全部有
//! 痕、流水单调，才算三面审计账平。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：留痕账（件=流水+发面+收面+操作码）+ 对称核（i→j 件数等于
//! j→i 件数）+ 覆盖核（六有序对全有痕）+ 流水核（流水严格递增）+
//! 注入核（自发自收/坏面号/重流水/零操作四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b420 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b420;
pub const TA_FACE: u32 = SEG_BASE + 0x01; // 面号越界或自环
pub const TA_OP: u32 = SEG_BASE + 0x02; // 操作码越界
pub const TA_SEQ: u32 = SEG_BASE + 0x03; // 流水不递增
pub const TA_CAP: u32 = SEG_BASE + 0x04; // 账超容
pub const TA_SEALED: u32 = SEG_BASE + 0x05; // 封账后记

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 有序面对数（3×2）。
pub const PAIR_N: usize = 6;
/// 每对最少痕锚。
pub const PER_PAIR_NEED: usize = 1;
/// 操作码域 1..=4。
pub const OP_ENROLL: u8 = 1;
pub const OP_SIGN: u8 = 2;
pub const OP_QUERY: u8 = 3;
pub const OP_CLOSE: u8 = 4;
pub const OP_N: u8 = 5;
/// 账定容。
pub const CAP: usize = 24;

/// 审计留痕件。
#[derive(Clone)]
pub struct AuditEvent {
    pub seq: u64,
    pub from: u8,
    pub to: u8,
    pub op: u8,
}

/// 三面审计账。
pub struct TriAudit {
    pub events: Vec<AuditEvent>,
    pub sealed: bool,
    pub last_seq: u64,
}

impl TriAudit {
    pub fn new() -> TriAudit {
        TriAudit { events: Vec::new(), sealed: false, last_seq: 0 }
    }

    fn pair_count(&self, from: u8, to: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].from == from && self.events[k].to == to {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 记一件：面号 0..2、异面、操作码 1..=4、流水严格递增。
    pub fn record(&mut self, seq: u64, from: u8, to: u8, op: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_SEALED);
        }
        if from >= FACE_N || to >= FACE_N || from == to {
            return Err(TA_FACE);
        }
        if op == 0 || op >= OP_N {
            return Err(TA_OP);
        }
        if seq <= self.last_seq {
            return Err(TA_SEQ);
        }
        if self.events.len() >= CAP {
            return Err(TA_CAP);
        }
        self.events.push(AuditEvent { seq, from, to, op });
        self.last_seq = seq;
        Ok(())
    }

    /// 对称核：每个有序对 i→j 的件数等于 j→i。
    pub fn symmetric(&self) -> bool {
        let mut i = 0u8;
        while i < FACE_N {
            let mut j = 0u8;
            while j < FACE_N {
                if i != j && self.pair_count(i, j) != self.pair_count(j, i) {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 覆盖核：六有序对全有痕。
    pub fn pair_covered(&self) -> bool {
        let mut i = 0u8;
        while i < FACE_N {
            let mut j = 0u8;
            while j < FACE_N {
                if i != j && self.pair_count(i, j) < PER_PAIR_NEED {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 流水单调核：账内流水严格递增。
    pub fn seq_monotonic(&self) -> bool {
        let mut k = 1usize;
        while k < self.events.len() {
            if self.events[k].seq <= self.events[k - 1].seq {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 缺痕有序对清单。
    pub fn missing_pairs(&self) -> Vec<(u8, u8)> {
        let mut v: Vec<(u8, u8)> = Vec::new();
        let mut i = 0u8;
        while i < FACE_N {
            let mut j = 0u8;
            while j < FACE_N {
                if i != j && self.pair_count(i, j) == 0 {
                    v.push((i, j));
                }
                j += 1;
            }
            i += 1;
        }
        v
    }

    /// 审计账平：覆盖+对称+单调。
    pub fn balanced(&self) -> bool {
        self.pair_covered() && self.symmetric() && self.seq_monotonic()
    }

    /// 渲染单件（"seq:from->to op"）。
    pub fn render(&self, k: usize) -> String {
        match self.events.get(k) {
            None => String::new(),
            Some(e) => format!("{}:{}->{} op{}", e.seq, e.from, e.to, e.op),
        }
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_SEALED);
        }
        if !self.balanced() {
            return Err(TA_FACE);
        }
        self.sealed = true;
        Ok(())
    }

    /// 审计账指纹（64 位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1b4201b42;
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.events.len() as u64);
        let mut k = 0usize;
        while k < self.events.len() {
            let e = &self.events[k];
            h = h.wrapping_mul(0x100000001b3).wrapping_add(e.seq);
            h = h
                .wrapping_mul(0x100000001b3)
                .wrapping_add(e.from as u64 * 8 + e.to as u64);
            h = h.wrapping_mul(0x100000001b3).wrapping_add(e.op as u64);
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut a = TriAudit::new();
    // 1) 自发自收必拒
    got[0] = a.record(1, 0, 0, OP_SIGN).err() == Some(TA_FACE);
    // 2) 坏面号必拒
    got[1] = a.record(2, 3, 0, OP_SIGN).err() == Some(TA_FACE)
        && a.record(3, 0, 9, OP_SIGN).err() == Some(TA_FACE);
    // 3) 流水回退必拒
    let _ = a.record(10, 0, 1, OP_ENROLL);
    got[2] = a.record(10, 1, 0, OP_ENROLL).err() == Some(TA_SEQ)
        && a.record(5, 1, 0, OP_ENROLL).err() == Some(TA_SEQ);
    // 4) 零操作码必拒
    got[3] = a.record(11, 0, 1, 0).err() == Some(TA_OP);
    got
}

/// 批内判据。
pub fn run_unx10333_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10333");

    // J1 正路径：六有序对来去成对留痕且对称
    let mut a = TriAudit::new();
    let pairs = [(0u8, 1u8), (1, 0), (0, 2), (2, 0), (1, 2), (2, 1)];
    let mut ok = true;
    let mut seq = 1u64;
    let mut k = 0usize;
    while k < pairs.len() {
        // 来去各一件
        let (i, j) = (pairs[k].0, pairs[k].1);
        if a.record(seq, i, j, OP_SIGN).is_err() {
            ok = false;
        }
        seq += 1;
        if a.record(seq, j, i, OP_QUERY).is_err() {
            ok = false;
        }
        seq += 1;
        k += 1;
    }
    let sealed = a.seal();
    s.add(
        "J1-三面双向对称",
        ok && a.balanced() && sealed.is_ok() && a.sealed,
        "六有序对来去各一件全对称并封账",
    );

    // J2 覆盖核：六对全有痕
    s.add("J2-六对覆盖", a.pair_covered(), "六个有序对全部有痕");

    // J3 流水单调
    s.add("J3-流水单调", a.seq_monotonic(), "流水严格递增");

    // J4 注入检出：自环 + 坏面号
    let inj = inject_four();
    s.add("J4-自环坏面检出", inj[0] && inj[1], "自发自收与坏面号必拒");

    // J5 注入检出：流水回退 + 零操作码
    s.add("J5-流水操作检出", inj[2] && inj[3], "流水回退与零操作码必拒");

    // J6 不对称不成账（0→1 两件 1→0 一件）
    let j6 = {
        let mut t = TriAudit::new();
        let _ = t.record(1, 0, 1, OP_ENROLL);
        let _ = t.record(2, 0, 1, OP_ENROLL);
        let _ = t.record(3, 1, 0, OP_ENROLL);
        !t.symmetric() && !t.balanced()
    };
    s.add("J6-不对称不平", j6, "来去件数不等即不对称");

    // J7 缺对不成账（缺 2→1 对）
    let j7 = {
        let mut t = TriAudit::new();
        let _ = t.record(1, 0, 1, OP_SIGN);
        let _ = t.record(2, 1, 0, OP_SIGN);
        let _ = t.record(3, 0, 2, OP_SIGN);
        let _ = t.record(4, 2, 0, OP_SIGN);
        let _ = t.record(5, 1, 2, OP_SIGN);
        !t.pair_covered() && t.missing_pairs().contains(&(2u8, 1u8))
    };
    s.add("J7-缺对不平", j7, "任一有序对无痕即不平");

    // J8 渲染非空且含操作码
    let j8 = a.render(0).len() > 0 && a.render(99).is_empty();
    s.add("J8-渲染可读", j8, "在账件渲染非空、越界件渲染空");

    // J9 封账后记必拒
    let j9 = {
        let mut t = TriAudit::new();
        let mut q = 0usize;
        let mut sq = 1u64;
        while q < pairs.len() {
            let (i, j) = (pairs[q].0, pairs[q].1);
            let _ = t.record(sq, i, j, OP_SIGN);
            sq += 1;
            let _ = t.record(sq, j, i, OP_QUERY);
            sq += 1;
            q += 1;
        }
        let _ = t.seal();
        t.record(999, 0, 1, OP_CLOSE).err() == Some(TA_SEALED)
    };
    s.add("J9-封账拒记", j9, "封账后再记必拒");

    // J10 容量边界
    let j10 = {
        let mut t = TriAudit::new();
        let mut sq = 1u64;
        let mut filled = true;
        while sq <= CAP as u64 {
            let f = ((sq as usize) % 2) as u8;
            let to = 1 - f;
            if t.record(sq, f, to, OP_QUERY).is_err() {
                filled = false;
            }
            sq += 1;
        }
        filled && t.record(CAP as u64 + 1, 0, 1, OP_QUERY).err() == Some(TA_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原账不损");

    // J11 指纹随记账推进且封账后稳定
    let j11 = {
        let mut t = TriAudit::new();
        let f0 = t.fold();
        let _ = t.record(1, 0, 1, OP_SIGN);
        let f1 = t.fold();
        let _ = t.record(2, 1, 0, OP_SIGN);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J11-指纹推进", j11, "每记一步指纹一步且终态稳定");

    // J12 动态明细：留痕件数落账
    let en = a.events.len();
    s.add_dyn(
        "J12-留痕件数",
        en == PAIR_N * 2,
        &format!("events={}", en),
    );

    // J13 动态明细：审计账指纹落账
    let fp = a.fold();
    s.add_dyn("J13-审计指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10333_tri_audit_flow() {
        let mut a = TriAudit::new();
        assert!(a.record(1, 0, 0, OP_SIGN).err() == Some(TA_FACE));
        assert!(a.record(1, 3, 1, OP_SIGN).err() == Some(TA_FACE));
        assert!(a.record(1, 0, 1, 0).err() == Some(TA_OP));
        assert!(a.record(1, 0, 1, 9).err() == Some(TA_OP));
        assert!(a.record(1, 0, 1, OP_SIGN).is_ok());
        assert!(a.record(1, 1, 0, OP_SIGN).err() == Some(TA_SEQ));
        assert!(a.record(2, 1, 0, OP_SIGN).is_ok());
        assert!(a.seal().err() == Some(TA_FACE));
        // 补齐六对
        let _ = a.record(3, 0, 2, OP_ENROLL);
        let _ = a.record(4, 2, 0, OP_ENROLL);
        let _ = a.record(5, 1, 2, OP_CLOSE);
        let _ = a.record(6, 2, 1, OP_CLOSE);
        assert!(a.symmetric() && a.pair_covered() && a.balanced());
        assert!(a.seal().is_ok());
        assert!(a.record(7, 0, 1, OP_SIGN).err() == Some(TA_SEALED));
    }

    #[test]
    fn unx10333_checks_green() {
        let s = run_unx10333_checks();
        assert!(s.all_passed(), "UNX-F10333 判据未全绿");
    }
}
