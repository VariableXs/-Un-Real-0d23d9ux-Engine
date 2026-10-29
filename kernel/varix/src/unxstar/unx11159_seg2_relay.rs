//! UNX-F11159 清账段二接力账（向 B39 验收总核销接力）（0x1f3b0 段）
//!
//! 纯功能模型：段二收口后把三路实数接力给 B39 验收域——批数 16、判据 320、
//! 行数 96,000 各成一路接力件，逐路标注已签收。接力成立要求三路齐、数额与
//! 段二实数逐路相等、逐路签收；一旦冻结，接力账不得改数也不得补签。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3b0;
/// 接力路号越界（只认 0..3）
pub const RL_ENO: u32 = SEG_BASE + 0x01;
/// 接力路重记
pub const RL_EDUP: u32 = SEG_BASE + 0x02;
/// 接力路序倒挂
pub const RL_EORDER: u32 = SEG_BASE + 0x03;
/// 接力数额为零（形状违规）
pub const RL_EAMT: u32 = SEG_BASE + 0x04;
/// 三路未齐
pub const RL_ESHORT: u32 = SEG_BASE + 0x05;
/// 某路数额与段二实数不符
pub const RL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 某路未签收
pub const RL_EPEND: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const RL_EFROZEN: u32 = SEG_BASE + 0x08;

/// 接力路数。
pub const LEGS: usize = 3;
/// 路名。
pub const LEG_NAMES: [&str; LEGS] = ["batches", "criteria", "lines"];
/// 段二批数。
pub const SEG2_BATCHES: u64 = 16;
/// 段二判据。
pub const SEG2_CRITERIA: u64 = 320;
/// 段二行数。
pub const SEG2_LINES: u64 = 96_000;
/// 接力目标域。
pub const TARGET_DOMAIN: &str = "B39";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一路接力件。
#[derive(Clone, Copy, Debug)]
pub struct RelayLeg {
    pub leg: u8,
    pub amount: u64,
    pub acked: bool,
}

impl RelayLeg {
    /// 该路口径值。
    pub fn expected(&self) -> u64 {
        match self.leg {
            0 => SEG2_BATCHES,
            1 => SEG2_CRITERIA,
            2 => SEG2_LINES,
            _ => 0,
        }
    }

    /// 该路是否对平且签收。
    pub fn cleared(&self) -> bool {
        self.amount == self.expected() && self.acked
    }
}

/// 段二接力账。
#[derive(Clone, Debug)]
pub struct RelayLedger {
    legs: Vec<RelayLeg>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl RelayLedger {
    /// 空账。
    pub fn new() -> RelayLedger {
        RelayLedger {
            legs: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已接力路数。
    pub fn len(&self) -> usize {
        self.legs.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.legs.is_empty()
    }

    /// 取某路。
    pub fn leg(&self, leg: usize) -> Option<RelayLeg> {
        let mut i = 0usize;
        while i < self.legs.len() {
            if self.legs[i].leg as usize == leg {
                return Some(self.legs[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 接一路：冻结 → 越界 → 重记 → 倒序 → 零额。
    pub fn relay(&mut self, leg: usize, amount: u64, acked: bool) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RL_EFROZEN);
        }
        if leg >= LEGS {
            return self.fail(RL_ENO);
        }
        if self.leg(leg).is_some() {
            return self.fail(RL_EDUP);
        }
        if let Some(last) = self.legs.last() {
            if leg <= last.leg as usize {
                return self.fail(RL_EORDER);
            }
        }
        if amount == 0 {
            return self.fail(RL_EAMT);
        }
        self.legs.push(RelayLeg {
            leg: leg as u8,
            amount,
            acked,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 签收某路（已签则原样）。
    pub fn ack(&mut self, leg: usize) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RL_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.legs.len() {
            if self.legs[i].leg as usize == leg {
                if self.legs[i].acked {
                    return Ok(self.fold());
                }
                self.legs[i].acked = true;
                self.accepts = self.accepts.wrapping_add(1);
                return Ok(self.fold());
            }
            i = i.wrapping_add(1);
        }
        self.fail(RL_ENO)
    }

    /// 未签收路数。
    pub fn pending(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.legs.len() {
            if !self.legs[i].acked {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 数额对平路数。
    pub fn matched_legs(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.legs.len() {
            if self.legs[i].amount == self.legs[i].expected() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 数额总和（三路合计仅作明细核对）。
    pub fn amount_sum(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.legs.len() {
            t = t.wrapping_add(self.legs[i].amount);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 缺路表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut leg = 0usize;
        while leg < LEGS {
            if self.leg(leg).is_none() {
                out.push(leg as u32);
            }
            leg = leg.wrapping_add(1);
        }
        out
    }

    /// 路序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.legs.len() {
            if self.legs[i].leg <= self.legs[i - 1].leg {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 接力账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.legs.len() {
            let l = self.legs[i];
            h = (h ^ l.leg as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ l.amount).wrapping_mul(FNV_PRIME);
            h = (h ^ l.acked as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.legs.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.pending() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 接力成立：三路齐 → 数额逐路对平 → 无未签收 → 逐路闭合。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RL_EFROZEN);
        }
        if self.legs.len() != LEGS || !self.gaps().is_empty() {
            return self.fail(RL_ESHORT);
        }
        if self.matched_legs() != LEGS as u32 {
            return self.fail(RL_EMISMATCH);
        }
        if self.pending() != 0 {
            return self.fail(RL_EPEND);
        }
        let mut i = 0usize;
        while i < self.legs.len() {
            if !self.legs[i].cleared() {
                return self.fail(RL_EPEND);
            }
            i = i.wrapping_add(1);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：三路实数（16 批 / 320 判据 / 96,000 行）逐路签收，再接力。
pub fn build_relay() -> (RelayLedger, bool) {
    let mut l = RelayLedger::new();
    let mut leg = 0usize;
    while leg < LEGS {
        let probe = RelayLeg {
            leg: leg as u8,
            amount: 0,
            acked: true,
        };
        if l.relay(leg, probe.expected(), true).is_err() {
            return (l, false);
        }
        leg = leg.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：路号越界 / 重记与倒序 / 零额 / 终账后再接。
pub fn inject_four() -> [bool; 4] {
    // ① 路号越界
    let mut a0 = RelayLedger::new();
    let r0 = a0.relay(LEGS, 1, true).err() == Some(RL_ENO)
        && a0.relay(LEGS.wrapping_add(2), 1, true).err() == Some(RL_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = RelayLedger::new();
    let s0 = a1.relay(0, SEG2_BATCHES, true).is_ok();
    let s1 = a1.relay(1, SEG2_CRITERIA, true).is_ok();
    let dup = s0 && s1 && a1.relay(0, SEG2_BATCHES, true).err() == Some(RL_EDUP);
    let mut a2 = RelayLedger::new();
    let _ = a2.relay(2, SEG2_LINES, true);
    let order = a2.relay(1, SEG2_CRITERIA, true).err() == Some(RL_EORDER);
    let r1 = dup && order;

    // ③ 零额与数额不符
    let mut a3 = RelayLedger::new();
    let zero = a3.relay(0, 0, true).err() == Some(RL_EAMT);
    let mut a4 = RelayLedger::new();
    let _ = a4.relay(0, SEG2_BATCHES.wrapping_add(1), true);
    let _ = a4.relay(1, SEG2_CRITERIA, true);
    let _ = a4.relay(2, SEG2_LINES, true);
    let mismatch = a4.settle().err() == Some(RL_EMISMATCH);
    let r2 = zero && mismatch;

    // ④ 终账后再接 / 补签 / 再清
    let (mut a5, ok5) = build_relay();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.relay(0, SEG2_BATCHES, true).err() == Some(RL_EFROZEN)
        && a5.ack(0).err() == Some(RL_EFROZEN)
        && a5.settle().err() == Some(RL_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11159 清账段二接力账。
pub fn run_unx11159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11159");
    let (mut l, ok) = build_relay();

    // X1 三路齐备
    s.add_dyn(
        "X1-三路齐备",
        ok && l.len() == LEGS && l.gaps().is_empty(),
        &format!("legs={}/{} gaps={}", l.len(), LEGS, l.gaps().len()),
    );

    // X2 批数接力对平
    s.add_dyn(
        "X2-批数对平",
        l.leg(0).map(|x| x.amount) == Some(SEG2_BATCHES),
        &format!("batches={}/{}", l.leg(0).map(|x| x.amount).unwrap_or(0), SEG2_BATCHES),
    );

    // X3 判据对平
    s.add_dyn(
        "X3-判据对平",
        l.leg(1).map(|x| x.amount) == Some(SEG2_CRITERIA),
        &format!("criteria={}/{}", l.leg(1).map(|x| x.amount).unwrap_or(0), SEG2_CRITERIA),
    );

    // X4 行数对平
    s.add_dyn(
        "X4-行数对平",
        l.leg(2).map(|x| x.amount) == Some(SEG2_LINES),
        &format!("lines={}/{}", l.leg(2).map(|x| x.amount).unwrap_or(0), SEG2_LINES),
    );

    // X5 数额逐路对平
    s.add_dyn(
        "X5-数额逐路对平",
        l.matched_legs() == LEGS as u32,
        &format!("matched={}/{}", l.matched_legs(), LEGS),
    );

    // X6 零未签收
    s.add_dyn(
        "X6-零未签收",
        l.pending() == 0,
        &format!("pending={} target={}", l.pending(), TARGET_DOMAIN),
    );

    // X7 路序严格
    s.add("X7-路序严格", l.order_strict(), "batches/criteria/lines 按 0..2 递增");

    // X8 终值即指纹
    let root = l.settled_value();
    s.add("X8-终值即指纹", root == Some(l.fold()), "接力单值等于三路指纹");

    // X9 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X9-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X10 冻结后拒接拒签
    s.add(
        "X10-冻结后拒接拒签",
        l.is_frozen()
            && l.relay(0, SEG2_BATCHES, true).err() == Some(RL_EFROZEN)
            && l.ack(1).err() == Some(RL_EFROZEN)
            && l.settle().err() == Some(RL_EFROZEN),
        "冻结后 relay/ack/settle 都落 RL_EFROZEN",
    );

    // X11 越界 / 重记 / 倒序 / 零额
    let mut g = RelayLedger::new();
    let g0 = g.relay(0, SEG2_BATCHES, true).is_ok();
    let mut o = RelayLedger::new();
    let _ = o.relay(2, SEG2_LINES, true);
    s.add(
        "X11-四路拒止",
        g.relay(LEGS, 1, true).err() == Some(RL_ENO)
            && g0
            && g.relay(0, SEG2_BATCHES, true).err() == Some(RL_EDUP)
            && o.relay(1, SEG2_CRITERIA, true).err() == Some(RL_EORDER)
            && g.relay(1, 0, true).err() == Some(RL_EAMT),
        "越界/重记/倒序/零额各按其码",
    );

    // X12 未齐 / 数额不符 / 未签收
    let mut q1 = RelayLedger::new();
    let _ = q1.relay(0, SEG2_BATCHES, true);
    let _ = q1.relay(1, SEG2_CRITERIA, true);
    let mut q2 = RelayLedger::new();
    let _ = q2.relay(0, SEG2_BATCHES.wrapping_add(2), true);
    let _ = q2.relay(1, SEG2_CRITERIA, true);
    let _ = q2.relay(2, SEG2_LINES, true);
    let mut q3 = RelayLedger::new();
    let _ = q3.relay(0, SEG2_BATCHES, true);
    let _ = q3.relay(1, SEG2_CRITERIA, false);
    let _ = q3.relay(2, SEG2_LINES, true);
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_relay();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(RL_ESHORT)
            && q2.settle().err() == Some(RL_EMISMATCH)
            && q3.settle().err() == Some(RL_EPEND)
            && all
            && ok2
            && fresh.accepts == LEGS as u64
            && fresh.rejects == 0
            && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11159_flow() {
        let (mut l, ok) = build_relay();
        assert!(ok);
        assert_eq!(l.len(), LEGS);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert_eq!(l.matched_legs(), LEGS as u32);
        assert_eq!(l.pending(), 0);
        assert_eq!(l.amount_sum(), SEG2_BATCHES + SEG2_CRITERIA + SEG2_LINES);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.relay(0, SEG2_BATCHES, true), Err(RL_EFROZEN));
        assert_eq!(l.settle(), Err(RL_EFROZEN));
    }

    #[test]
    fn f11159_gates() {
        let mut l = RelayLedger::new();
        assert!(l.relay(0, SEG2_BATCHES, true).is_ok());
        assert!(l.relay(1, SEG2_CRITERIA, true).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.relay(LEGS, 1, true), Err(RL_ENO));
        assert_eq!(l.relay(0, SEG2_BATCHES, true), Err(RL_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut z = RelayLedger::new();
        assert_eq!(z.relay(0, 0, true), Err(RL_EAMT));
        assert_eq!((z.accepts, z.rejects), (0, 1));

        let mut o = RelayLedger::new();
        let _ = o.relay(2, SEG2_LINES, true);
        assert_eq!(o.relay(1, SEG2_CRITERIA, true), Err(RL_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut q = RelayLedger::new();
        let _ = q.relay(0, SEG2_BATCHES, true);
        let _ = q.relay(1, SEG2_CRITERIA, true);
        assert_eq!(q.settle(), Err(RL_ESHORT));
        assert_eq!((q.accepts, q.rejects), (2, 1));

        let mut m = RelayLedger::new();
        let _ = m.relay(0, SEG2_BATCHES.wrapping_add(1), true);
        let _ = m.relay(1, SEG2_CRITERIA, true);
        let _ = m.relay(2, SEG2_LINES, true);
        assert_eq!(m.settle(), Err(RL_EMISMATCH));
        assert_eq!((m.accepts, m.rejects), (3, 1));

        let mut p = RelayLedger::new();
        let _ = p.relay(0, SEG2_BATCHES, true);
        let _ = p.relay(1, SEG2_CRITERIA, false);
        let _ = p.relay(2, SEG2_LINES, true);
        assert_eq!(p.pending(), 1);
        assert_eq!(p.settle(), Err(RL_EPEND));
        assert_eq!((p.accepts, p.rejects), (3, 1));

        let (mut d, ok) = build_relay();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.relay(0, SEG2_BATCHES, true), Err(RL_EFROZEN));
        assert_eq!(d.ack(0), Err(RL_EFROZEN));
        assert_eq!(d.settle(), Err(RL_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (3, 3));
    }

    #[test]
    fn f11159_checks_green() {
        assert!(run_unx11159_checks().all_passed());
    }
}
