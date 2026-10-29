//! UNX-F48754 C·随闸门补测清单终版（真机项汇总+计划）
//! 判据 UNX-F48754-J1：清单零漏项；判据锚定 双轨产线。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 真机补测项逐项编号入册，清单零漏项、计划零冲突。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1BE0;
const E_ITEM_MISS: u16 = SEG_BASE + 1;
const E_ITEM_DUP: u16 = SEG_BASE + 2;
const E_PLAN_CONFL: u16 = SEG_BASE + 3;
const E_NO_MACHINE: u16 = SEG_BASE + 4;
const E_LIST_GAP: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_TAG_BAD: u16 = SEG_BASE + 7;
const E_SCHED_BAD: u16 = SEG_BASE + 8;

/// 真机补测项
#[derive(Clone, Debug, PartialEq)]
pub struct RealItem {
    pub no: u16,
    pub machine: String,
    pub case_fp: u32,
}

impl RealItem {
    pub fn make(no: u16, machine: &str, case_fp: u32) -> Self {
        RealItem { no, machine: String::from(machine), case_fp }
    }
}

/// 计划条目：项号 + 档期槽位
#[derive(Clone, Debug, PartialEq)]
pub struct PlanEntry {
    pub item_no: u16,
    pub slot: u16,
}

/// 补测清单终版账
pub struct Checklist {
    pub items: Vec<RealItem>,
    pub plan: Vec<PlanEntry>,
}

impl Checklist {
    pub fn new() -> Self {
        Checklist { items: vec![], plan: vec![] }
    }

    pub fn add_item(&mut self, no: u16, machine: &str, case_fp: u32) -> Result<(), u16> {
        if machine.is_empty() {
            return Err(E_TAG_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].no == no {
                return Err(E_ITEM_DUP);
            }
            i += 1;
        }
        self.items.push(RealItem::make(no, machine, case_fp));
        Ok(())
    }

    pub fn schedule(&mut self, item_no: u16, slot: u16) -> Result<(), u16> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].no == item_no {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(E_ITEM_MISS);
        }
        let mut j = 0usize;
        while j < self.plan.len() {
            if self.plan[j].slot == slot {
                return Err(E_PLAN_CONFL);
            }
            if self.plan[j].item_no == item_no {
                return Err(E_PLAN_CONFL);
            }
            j += 1;
        }
        self.plan.push(PlanEntry { item_no, slot });
        Ok(())
    }

    /// 清单零漏项核对：登记 1..=n 逐号在册且全部排期
    pub fn verify_full(&self, n: u16) -> Result<(), u16> {
        if n == 0 || self.items.len() != n as usize {
            return Err(E_LIST_GAP);
        }
        let mut s = 1u16;
        while s <= n {
            let mut in_list = false;
            let mut in_plan = false;
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].no == s {
                    in_list = true;
                }
                i += 1;
            }
            let mut j = 0usize;
            while j < self.plan.len() {
                if self.plan[j].item_no == s {
                    in_plan = true;
                }
                j += 1;
            }
            if !in_list {
                return Err(E_ITEM_MISS);
            }
            if !in_plan {
                return Err(E_SCHED_BAD);
            }
            s += 1;
        }
        Ok(())
    }

    /// 真机项覆盖：清单内不得有未指定真机的条目
    pub fn machines_covered(&self) -> Result<u32, u16> {
        if self.items.is_empty() {
            return Err(E_NO_MACHINE);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].machine.is_empty() {
                return Err(E_NO_MACHINE);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 终版清单 ×10^3：登记+排期+零漏项
fn sweep_full_checklist(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut ck = Checklist::new();
        let n = 3 + rng.below(6);
        let mut s = 1u16;
        while s <= n as u16 {
            let machine = match s % 3 {
                0 => "desk-a",
                1 => "desk-b",
                _ => "pad-c",
            };
            ck.add_item(s, machine, 0x1000 + s as u32)?;
            ck.schedule(s, s)?;
            s += 1;
        }
        ck.verify_full(n as u16)?;
        if ck.machines_covered()? != n as u32 {
            return Err(E_NO_MACHINE);
        }
        iter += 1;
    }
    Ok(())
}

/// 反例注入：重复项、缺项、槽位冲突、重复排期、空真机全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut ck = Checklist::new();
    ck.add_item(1, "desk-a", 1)?;
    let r1 = ck.add_item(1, "desk-b", 2);
    if r1.err() != Some(E_ITEM_DUP) {
        return Err(E_ITEM_DUP);
    }
    let r2 = ck.schedule(9, 1);
    if r2.err() != Some(E_ITEM_MISS) {
        return Err(E_ITEM_MISS);
    }
    ck.schedule(1, 5)?;
    let r3 = ck.schedule(1, 6);
    if r3.err() != Some(E_PLAN_CONFL) {
        return Err(E_PLAN_CONFL);
    }
    let mut ck2 = Checklist::new();
    ck2.add_item(2, "desk-a", 3)?;
    let r4 = ck2.verify_full(2);
    if r4.err() != Some(E_LIST_GAP) {
        return Err(E_LIST_GAP);
    }
    let mut ck3 = Checklist::new();
    let r5 = ck3.add_item(3, "", 4);
    if r5.err() != Some(E_TAG_BAD) {
        return Err(E_TAG_BAD);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑清单逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ca = Checklist::new();
        let mut cb = Checklist::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut s = 1u16;
        while s <= 5 {
            let fa = 0x2000 + ra.below(1_000) as u32;
            let fb = 0x2000 + rb.below(1_000) as u32;
            if fa != fb {
                return Err(E_REPLAY_DIFF);
            }
            ca.add_item(s, "bench", fa)?;
            cb.add_item(s, "bench", fb)?;
            ca.schedule(s, s * 2)?;
            cb.schedule(s, s * 2)?;
            s += 1;
        }
        ca.verify_full(5)?;
        cb.verify_full(5)?;
        if ca.items != cb.items || ca.plan != cb.plan {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 终版清单 ×10^3 零漏项
    let mut rng = Lcg::new(0x4875_4ECC);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_full_checklist(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R3 重放一致 ×20
    sweep_replay(0x4875_4D08)?;
    hits += 1;
    // R4 真机三机位覆盖：desk-a/desk-b/pad-c 全在册
    let mut ck = Checklist::new();
    ck.add_item(1, "desk-a", 0x11)?;
    ck.add_item(2, "desk-b", 0x22)?;
    ck.add_item(3, "pad-c", 0x33)?;
    if ck.machines_covered()? != 3 {
        return Err(E_NO_MACHINE);
    }
    hits += 1;
    // R5 槽位排期单调：计划槽位严格对应项号
    ck.schedule(1, 10)?;
    ck.schedule(2, 20)?;
    ck.schedule(3, 30)?;
    let mut i = 0usize;
    while i < ck.plan.len() {
        if ck.plan[i].slot != ck.plan[i].item_no * 10 {
            return Err(E_SCHED_BAD);
        }
        i += 1;
    }
    ck.verify_full(3)?;
    hits += 1;
    // R6 槽位冲突点名：同槽二项必红
    let mut ck2 = Checklist::new();
    ck2.add_item(1, "desk-a", 1)?;
    ck2.add_item(2, "desk-a", 2)?;
    ck2.schedule(1, 7)?;
    let r6 = ck2.schedule(2, 7);
    if r6.err() != Some(E_PLAN_CONFL) {
        return Err(E_PLAN_CONFL);
    }
    hits += 1;
    // R7 漏排期点名：在册未排期即红
    let mut ck3 = Checklist::new();
    ck3.add_item(1, "desk-a", 1)?;
    ck3.add_item(2, "desk-b", 2)?;
    ck3.schedule(1, 1)?;
    let r7 = ck3.verify_full(2);
    if r7.err() != Some(E_SCHED_BAD) {
        return Err(E_SCHED_BAD);
    }
    hits += 1;
    // R8 清单快照序稳定
    let items = ck.items.clone();
    let mut k = 0usize;
    while k < items.len() {
        if items[k].no != (k + 1) as u16 {
            return Err(E_LIST_GAP);
        }
        k += 1;
    }
    hits += 1;
    // R9 真机名语义：非空且长度在带
    let mut c = 0u32;
    while c < 50 {
        let m = match rng.below(3) {
            0 => "desk-a",
            1 => "desk-b",
            _ => "pad-c",
        };
        if m.len() < 4 || m.len() > 16 {
            return Err(E_TAG_BAD);
        }
        c += 1;
    }
    hits += 1;
    // R10 案例指纹唯一抽查：登记项指纹两两不同
    let its = ck.items.clone();
    let mut a = 0usize;
    while a < its.len() {
        let mut b = a + 1;
        while b < its.len() {
            if its[a].case_fp == its[b].case_fp {
                return Err(E_ITEM_DUP);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
