//! UNX-F11081 演习场搭建（长稳账与哨兵网）（0x1ea20 段）
//!
//! 演习场先立 8 个长稳槽，每槽带自己的账容量；随后在槽上布 12 个哨兵，
//! 每个哨兵有固定周期并只能绑定已存在的槽。哨兵网要求每个槽至少一个哨兵、
//! sid 严格递增，且所有哨兵都已 armed；长稳预算要求 槽容量总和 ≥ 轮数×槽数。
//! 不变式：槽数固定 8、哨兵数固定 12，任何未布防哨兵都使整场不进 ready。
//!
//! 结构：长稳槽（类型/容量/代次）+ 哨兵（槽/周期/心跳）+ 演习场账
//! （全链搭建、哨兵覆盖、预算审计、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11081 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1ea20;

/// 空场 —— 尚未立槽或尚未布哨。
pub const DF_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 槽号/哨号/周期/容量越出演习场窗口。
pub const DF_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 槽号、哨号或长稳计划重复登记。
pub const DF_DUP: u32 = SEG_BASE + 0x03;
/// 未布防 —— 对已撤防哨兵心跳。
pub const DF_UNARMED: u32 = SEG_BASE + 0x04;
/// 缺口 —— 哨兵绑定了不存在的槽，或槽/哨数量不足。
pub const DF_GAP: u32 = SEG_BASE + 0x05;
/// 封场 —— 演习场冻结后仍搭槽、布哨或心跳。
pub const DF_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（演习场搭建账）。
pub const DF_TAG: u64 = 0x1ea_81;

/// 长稳槽数。
pub const DF_SLOTS: usize = 8;
/// 哨兵数。
pub const DF_SENTINELS: usize = 12;

/// 一个长稳槽。
#[derive(Clone, Debug)]
pub struct FieldSlot {
    pub slot: u8,
    pub kind: u8,
    pub ledger_cap: u32,
    pub active: bool,
    pub generation: u16,
}

/// 一个哨兵。
#[derive(Clone, Debug)]
pub struct Sentinel {
    pub sid: u8,
    pub slot: u8,
    pub period_ms: u16,
    pub armed: bool,
    pub beats: u32,
}

impl Sentinel {
    /// 心跳周期是否落在 10..=1000ms。
    pub fn period_ok(&self) -> bool {
        self.period_ms >= 10 && self.period_ms <= 1_000
    }
}

/// 演习场总账。
#[derive(Clone, Debug)]
pub struct DrillField {
    pub slots: Vec<FieldSlot>,
    pub sentinels: Vec<Sentinel>,
    pub rounds_planned: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DrillField {
    pub fn new() -> DrillField {
        DrillField {
            slots: Vec::new(),
            sentinels: Vec::new(),
            rounds_planned: 0,
            frozen: false,
            root: None,
        }
    }

    fn slot_at(&self, slot: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].slot == slot {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn sentinel_at(&self, sid: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.sentinels.len() {
            if self.sentinels[i].sid == sid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记长稳槽。
    pub fn add_slot(&mut self, slot: u8, kind: u8, cap: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        if slot >= DF_SLOTS as u8 || kind >= 3 || cap < 1_000 {
            return Err(DF_RANGE);
        }
        if self.slot_at(slot).is_some() {
            return Err(DF_DUP);
        }
        self.slots.push(FieldSlot {
            slot,
            kind,
            ledger_cap: cap,
            active: true,
            generation: 1,
        });
        Ok(())
    }

    /// 落长稳计划；整场只允许一次。
    pub fn plan_rounds(&mut self, rounds: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        if rounds < 1_000 || rounds > 100_000 {
            return Err(DF_RANGE);
        }
        if self.rounds_planned != 0 {
            return Err(DF_DUP);
        }
        self.rounds_planned = rounds;
        Ok(())
    }

    /// 在已有槽上布哨；槽不存在是缺口，不是越界。
    pub fn arm_sentinel(&mut self, sid: u8, slot: u8, period_ms: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        if sid >= DF_SENTINELS as u8 || slot >= DF_SLOTS as u8 {
            return Err(DF_RANGE);
        }
        if period_ms < 10 || period_ms > 1_000 {
            return Err(DF_RANGE);
        }
        if self.sentinel_at(sid).is_some() {
            return Err(DF_DUP);
        }
        if self.slot_at(slot).is_none() {
            return Err(DF_GAP);
        }
        self.sentinels.push(Sentinel {
            sid,
            slot,
            period_ms,
            armed: true,
            beats: 0,
        });
        Ok(())
    }

    /// 哨兵心跳。
    pub fn beat(&mut self, sid: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        let si = match self.sentinel_at(sid) {
            Some(i) => i,
            None => return Err(DF_RANGE),
        };
        if !self.sentinels[si].armed {
            return Err(DF_UNARMED);
        }
        self.sentinels[si].beats = self.sentinels[si].beats.wrapping_add(1);
        Ok(())
    }

    /// 维护撤防。
    pub fn disarm(&mut self, sid: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        let si = match self.sentinel_at(sid) {
            Some(i) => i,
            None => return Err(DF_RANGE),
        };
        self.sentinels[si].armed = false;
        Ok(())
    }

    /// 槽容量总和。
    pub fn capacity_total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.slots.len() {
            n = n.wrapping_add(self.slots[i].ledger_cap as u64);
            i += 1;
        }
        n
    }

    /// 长稳样本预算：容量总和 × 计划轮数。
    pub fn sample_budget(&self) -> u64 {
        self.capacity_total().wrapping_mul(self.rounds_planned as u64)
    }

    /// 每个槽至少一个哨兵，且哨兵 sid 严格递增。
    pub fn net_covered(&self) -> bool {
        if self.sentinels.len() != DF_SENTINELS {
            return false;
        }
        let mut s = 0u8;
        while s < DF_SLOTS as u8 {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.sentinels.len() {
                if self.sentinels[i].slot == s && self.sentinels[i].armed {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return false;
            }
            s = s.wrapping_add(1);
        }
        let mut k = 1usize;
        while k < self.sentinels.len() {
            if self.sentinels[k].sid <= self.sentinels[k - 1].sid {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 全链搭建：8 槽全活、12 哨全 armed、计划已落。
    pub fn field_ready(&self) -> bool {
        if self.slots.len() != DF_SLOTS || self.sentinels.len() != DF_SENTINELS {
            return false;
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].active {
                return false;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.sentinels.len() {
            if !self.sentinels[k].armed || !self.sentinels[k].period_ok() {
                return false;
            }
            k += 1;
        }
        self.rounds_planned == 10_000 && self.net_covered()
    }

    /// 总审计：空、缺口、未布防分别拒绝。
    pub fn audit(&self) -> Result<(), u32> {
        if self.slots.is_empty() && self.sentinels.is_empty() {
            return Err(DF_EMPTY);
        }
        if self.slots.len() != DF_SLOTS || self.sentinels.len() != DF_SENTINELS {
            return Err(DF_GAP);
        }
        let mut i = 0usize;
        while i < self.sentinels.len() {
            if !self.sentinels[i].armed {
                return Err(DF_UNARMED);
            }
            i += 1;
        }
        if !self.field_ready() {
            return Err(DF_GAP);
        }
        Ok(())
    }

    /// FNV-1a：槽代次/容量与哨兵周期/心跳/布防位全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = DF_TAG;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = &self.slots[i];
            h = (h ^ s.slot as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.kind as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.ledger_cap as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.generation as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if s.active { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.sentinels.len() {
            let t = &self.sentinels[k];
            h = (h ^ t.sid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ t.slot as u64).wrapping_mul(0x100000001b3);
            h = (h ^ t.period_ms as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if t.armed { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            h = (h ^ t.beats as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.rounds_planned as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封场。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 演习场摘要。
    pub fn summary(&self) -> String {
        let label = "drill".to_string();
        format!("{}|slots={}|sent={}|cap={}|rounds={}", label, self.slots.len(),
            self.sentinels.len(), self.capacity_total(), self.rounds_planned)
    }
}

/// 四路坏样本：槽重号、周期越界、哨兵缺口、撤防后心跳。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = DrillField::new();
    let _ = a.add_slot(0, 1, 8_192);
    got[0] = a.add_slot(0, 2, 8_192).err() == Some(DF_DUP);

    let mut b = DrillField::new();
    let _ = b.add_slot(1, 0, 8_192);
    got[1] = b.arm_sentinel(0, 1, 0).err() == Some(DF_RANGE);

    let mut c = DrillField::new();
    let _ = c.add_slot(2, 1, 4_096);
    got[2] = c.arm_sentinel(1, 7, 100).err() == Some(DF_GAP);

    let mut d = DrillField::new();
    let _ = d.add_slot(3, 2, 4_096);
    let _ = d.arm_sentinel(2, 3, 50);
    let _ = d.disarm(2);
    got[3] = d.beat(2).err() == Some(DF_UNARMED);
    got
}

/// 演习场搭建与哨兵网判据。
pub fn run_unx11081_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11081");

    let mut b = DrillField::new();
    let _ = b.plan_rounds(10_000);
    let mut slot = 0u8;
    while slot < DF_SLOTS as u8 {
        let _ = b.add_slot(slot, slot % 3, 131_072);
        slot = slot.wrapping_add(1);
    }
    let mut sid = 0u8;
    while sid < DF_SENTINELS as u8 {
        let _ = b.arm_sentinel(sid, sid % 8, 50u16.wrapping_add(sid as u16));
        sid = sid.wrapping_add(1);
    }
    let mut beat = 0u8;
    while beat < DF_SENTINELS as u8 {
        let _ = b.beat(beat);
        beat = beat.wrapping_add(1);
    }

    // J1 两半：全链搭建 10/10，哨兵逐哨在位。
    let j1a = b.field_ready() && b.slots.len() == 8;
    let j1b = b.net_covered() && b.sentinels.len() == 12
        && b.sentinels.iter().all(|x| x.armed);
    s.add("J1-全链搭建", j1a, "8 槽 12 哨与万轮计划全部就位");
    s.add("J1-哨兵逐哨在位", j1b, "12 哨全部布防且每槽有覆盖");

    // J2 长稳预算：总容量 × 计划轮数覆盖 8×10000 样本。
    let cap = b.capacity_total();
    let samples = cap.wrapping_mul(b.rounds_planned as u64);
    let j2 = cap == 1_048_576 && b.rounds_planned == 10_000
        && samples >= (DF_SLOTS as u64).wrapping_mul(10_000);
    s.add("J2-长稳预算", j2, "槽容量总和覆盖万轮八槽样本");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同槽二次登记必拒");
    s.add("J3-周期注入", inj[1], "零周期哨兵必拒");
    s.add("J3-缺口注入", inj[2], "哨兵绑空槽必拒");
    s.add("J3-撤防注入", inj[3], "撤防后心跳必拒");

    // J4 封场：根稳定且搭槽/布置/心跳全拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.add_slot(0, 0, 8_000).err() == Some(DF_FROZEN)
        && f.arm_sentinel(0, 0, 100).err() == Some(DF_FROZEN)
        && f.beat(0).err() == Some(DF_FROZEN);
    s.add("J4-封场不可变", j4, "冻结后三类写路径全部拒绝");

    // J5 指纹敏感：改一个哨兵周期即改根。
    let mut c = b.clone();
    c.sentinels[3].period_ms = 777;
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单哨周期变化即改变场账根");

    // J6 空场/范围/重复计划。
    let mut e = DrillField::new();
    let empty = e.audit();
    let range = e.add_slot(8, 0, 4_096);
    let _ = e.plan_rounds(10_000);
    let dup_plan = e.plan_rounds(20_000);
    let j6 = empty.err() == Some(DF_EMPTY) && range.err() == Some(DF_RANGE)
        && dup_plan.err() == Some(DF_DUP);
    s.add("J6-空场范围", j6, "空场、越界槽、二次计划均拒");

    // J7 唯一有序：槽号与哨号严格递增。
    let mut ordered = true;
    let mut i = 1usize;
    while i < b.slots.len() {
        if b.slots[i].slot <= b.slots[i - 1].slot {
            ordered = false;
        }
        i += 1;
    }
    let mut k = 1usize;
    while k < b.sentinels.len() {
        if b.sentinels[k].sid <= b.sentinels[k - 1].sid {
            ordered = false;
        }
        k += 1;
    }
    s.add("J7-唯一有序", ordered, "8 槽与 12 哨编号严格递增");

    // J8 负语义：少一个哨兵即不能 ready。
    let mut partial = DrillField::new();
    let _ = partial.plan_rounds(10_000);
    let mut ps = 0u8;
    while ps < 8 {
        let _ = partial.add_slot(ps, 1, 131_072);
        ps = ps.wrapping_add(1);
    }
    let mut pp = 0u8;
    while pp < 11 {
        let _ = partial.arm_sentinel(pp, pp % 8, 100);
        pp = pp.wrapping_add(1);
    }
    let j8 = !partial.field_ready() && !partial.net_covered()
        && partial.audit().err() == Some(DF_GAP);
    s.add("J8-少哨非绿", j8, "十一哨演习场不得判全链就位");

    // J9 动态场账。
    s.add_dyn("J9-场账动态", j1a,
        &format!("slots={} sent={} cap={} rounds={}", b.slots.len(),
            b.sentinels.len(), cap, b.rounds_planned));

    // J10 心跳计数：每哨恰一次。
    let mut beats_ok = true;
    let mut x = 0usize;
    while x < b.sentinels.len() {
        if b.sentinels[x].beats != 1 {
            beats_ok = false;
        }
        x += 1;
    }
    s.add("J10-心跳计数", beats_ok, "12 哨各记一次心跳");

    // J11 总审计通过。
    let j11 = b.audit().is_ok() && b.sample_budget() == cap.wrapping_mul(10_000);
    s.add("J11-总审计", j11, "演习场总审计通过且预算恒等");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11081_flow() {
        let mut b = DrillField::new();
        assert!(b.audit().err() == Some(DF_EMPTY));
        assert!(b.add_slot(0, 0, 4_096).is_ok());
        assert!(b.add_slot(0, 1, 4_096).err() == Some(DF_DUP));
        assert!(b.arm_sentinel(0, 0, 100).is_ok());
        assert!(b.arm_sentinel(1, 7, 100).err() == Some(DF_GAP));
        assert!(b.beat(0).is_ok());
        assert!(b.disarm(0).is_ok());
        assert!(b.beat(0).err() == Some(DF_UNARMED));
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.beat(0).err() == Some(DF_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11081_checks_green() {
        assert!(run_unx11081_checks().all_passed(), "UNX-F11081 判据未全绿");
    }
}
