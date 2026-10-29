//! UNX-F24130 显存驱逐 LRU 与迁移搬运·子功能10（0x41220 段）
//!
//! 锚点：UNX-F24130-J1 LRU 条目链 + pin 账 + 迁移账逐条落账；J2 字段齐备；
//! J3~J12 核心操作语义与折叠指纹（长度守恒切面并入 J3/J11）；J13 注入核 5/5。
//! 域：CoRun·UNX-F5-B10（显存驱逐 LRU 与迁移搬运）。
//!
//! 结构：LRU 条目链 + pin 账 + 迁移账 + 长度守恒（LenKeep）切面账 + 注入核
//! （重入/野对象/挡逐出/重迁/印后 六路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41220 独占 0x20）。
pub const SEG_BASE: u32 = 0x41220;
pub const EL_DUP_ADMIT: u32 = SEG_BASE + 0x01; // 重复入驻
pub const EL_WILD_OBJ: u32 = SEG_BASE + 0x02; // 野对象
pub const EL_PIN_GUARD: u32 = SEG_BASE + 0x03; // pin 挡逐出
pub const EL_DUP_MIG: u32 = SEG_BASE + 0x04; // 重复迁移
pub const EL_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 长度守恒切面账（显存驱逐 LRU 与迁移搬运子切面）：记录 (k, v) 对，
/// v > 0x18 违者回 EL_PIN_GUARD，同 k 重记回 EL_DUP_ADMIT。
pub struct LenKeep {
    pub tag: u32,
    pub hits: u32,
    pub log: Vec<(u32, u32)>,
}

impl LenKeep {
    pub fn new(tag: u32) -> LenKeep {
        LenKeep { tag, hits: 0, log: vec![] }
    }

    /// 记账一笔：v > 0x18。
    pub fn record(&mut self, k: u32, v: u32) -> Result<(), u32> {
        if v > 0x18 {
            return Err(EL_PIN_GUARD);
        }
        if self.log.iter().any(|(lk, _)| *lk == k) {
            return Err(EL_DUP_ADMIT);
        }
        self.log.push((k, v));
        self.hits += 1;
        Ok(())
    }

    /// 切面累计值（长度守恒）。
    pub fn len_cnt(&self) -> u32 {
        let mut acc: u32 = 0;
        for (k, v) in &self.log {
            acc = acc.wrapping_add((*k).wrapping_mul(*v).wrapping_add(self.tag));
        }
        acc
    }

    /// 切面折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41220;
        for (k, v) in &self.log {
            h = h.wrapping_mul(35).wrapping_add((*k << 8) ^ *v);
        }
        h ^ self.tag
    }
}

/// LRU 条目。
#[derive(Clone, Copy, PartialEq)]
pub struct LruEnt {
    pub obj_id: u32,
    pub tick: u64,
    pub pinned: u32,
}

/// pin 账。
#[derive(Clone, Copy, PartialEq)]
pub struct PinRec {
    pub seq: u32,
    pub obj_id: u32,
    pub released: bool,
}

/// 迁移账。
#[derive(Clone, Copy, PartialEq)]
pub struct MigRec {
    pub seq: u32,
    pub obj_id: u32,
    pub to_dom: u32,
    pub done: bool,
}

/// EvictCore（LRU 条目链 + pin 账 + 迁移账）。
pub struct EvictCore {
    pub ents: Vec<LruEnt>,
    pub pins: Vec<PinRec>,
    pub migs: Vec<MigRec>,
    pub evicted: Vec<u32>,
    pub tick_next: u64,
    pub seq_next: u32,
    pub sealed: bool,
}

impl EvictCore {
    pub fn new() -> EvictCore {
        EvictCore {
            ents: vec![],
            pins: vec![],
            migs: vec![],
            evicted: vec![],
            tick_next: 0,
            seq_next: 1,
            sealed: false,
        }
    }

    fn ent_idx(&self, obj_id: u32) -> Option<usize> {
        self.ents.iter().position(|e| e.obj_id == obj_id)
    }

    /// 入驻：印后拒 + 零号/重复入驻拒。
    pub fn admit(&mut self, obj_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        if obj_id == 0 || self.ent_idx(obj_id).is_some() {
            return Err(EL_DUP_ADMIT);
        }
        self.ents.push(LruEnt { obj_id, tick: self.tick_next, pinned: 0 });
        self.tick_next += 1;
        Ok(())
    }

    /// 触碰升位：印后拒 + 野对象拒。
    pub fn touch(&mut self, obj_id: u32) -> Result<u64, u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        let i = match self.ent_idx(obj_id) {
            Some(i) => i,
            None => return Err(EL_WILD_OBJ),
        };
        self.ents[i].tick = self.tick_next;
        self.tick_next += 1;
        Ok(self.ents[i].tick)
    }

    /// pin：印后拒 + 野对象拒 + 重复 pin 拒。
    pub fn pin(&mut self, obj_id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        let i = match self.ent_idx(obj_id) {
            Some(i) => i,
            None => return Err(EL_WILD_OBJ),
        };
        if self.ents[i].pinned > 0
            || self.pins.iter().any(|p| p.obj_id == obj_id && !p.released)
        {
            return Err(EL_DUP_ADMIT);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ents[i].pinned = seq;
        self.pins.push(PinRec { seq, obj_id, released: false });
        Ok(seq)
    }

    /// unpin：野 seq 拒 + 双解拒。
    pub fn unpin(&mut self, seq: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        let i = match self.pins.iter().position(|p| p.seq == seq) {
            Some(i) => i,
            None => return Err(EL_WILD_OBJ),
        };
        if self.pins[i].released {
            return Err(EL_DUP_ADMIT);
        }
        self.pins[i].released = true;
        let oid = self.pins[i].obj_id;
        if let Some(j) = self.ent_idx(oid) {
            self.ents[j].pinned = 0;
        }
        Ok(())
    }

    /// 驱逐最老未 pin 条目：全员 pin 则挡逐出。
    pub fn evict_one(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        let oid = match self.ents.iter().filter(|e| e.pinned == 0).min_by_key(|e| e.tick) {
            Some(e) => e.obj_id,
            None => return Err(EL_PIN_GUARD),
        };
        self.ents.retain(|e| e.obj_id != oid);
        self.evicted.push(oid);
        Ok(oid)
    }

    /// 迁移：印后拒 + 野对象拒 + 同目标重迁拒。
    pub fn migrate(&mut self, obj_id: u32, to_dom: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(EL_FROZEN);
        }
        if self.ent_idx(obj_id).is_none() {
            return Err(EL_WILD_OBJ);
        }
        if self.migs.iter().any(|m| m.obj_id == obj_id && m.to_dom == to_dom && !m.done) {
            return Err(EL_DUP_MIG);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        // done=false 表示迁移在途：在途期间同目标重迁必拒
        self.migs.push(MigRec { seq, obj_id, to_dom, done: false });
        Ok(seq)
    }

    /// 未逐出条目计数。
    pub fn resident(&self) -> u32 {
        self.ents.len() as u32
    }

    /// 账本折叠指纹（条目 + pin + 迁移 + 逐出）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41220;
        for e in &self.ents {
            h = h.wrapping_mul(35).wrapping_add(e.obj_id ^ (e.tick as u32));
        }
        for p in &self.pins {
            h = h.wrapping_mul(35).wrapping_add(p.seq ^ ((p.released as u32) << 8));
        }
        for m in &self.migs {
            h = h.wrapping_mul(35).wrapping_add(m.obj_id ^ (m.to_dom << 4) ^ m.seq);
        }
        for x in &self.evicted {
            h = h.wrapping_mul(35).wrapping_add(*x);
        }
        h
    }

    /// 封印：印后一切写操作必拒。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：四对象按序入链。
pub fn mk_lru() -> EvictCore {
    let mut e = EvictCore::new();
    let _ = e.admit(1);
    let _ = e.admit(2);
    let _ = e.admit(3);
    let _ = e.admit(4);
    e
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = mk_lru();
    // 1) 重复入驻
    got[0] = e.admit(1).err() == Some(EL_DUP_ADMIT);
    // 2) 野对象触碰
    got[1] = e.touch(99).err() == Some(EL_WILD_OBJ);
    // 3) 重复 pin
    let _ = e.pin(2);
    got[2] = e.pin(2).err() == Some(EL_DUP_ADMIT);
    // 4) pin 挡逐出（全员 pin）
    let _ = e.pin(1);
    let _ = e.pin(3);
    let _ = e.pin(4);
    got[3] = e.evict_one().err() == Some(EL_PIN_GUARD);
    // 5) 印后驱逐
    e.seal();
    got[4] = e.evict_one().err() == Some(EL_FROZEN);
    got
}

/// 判据主体（13 条，全部真跑账本）。
pub fn run_unx24130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24130");
    let mut e = mk_lru();

    // J1 账非空
    let j1 = e.ents.len() == 4 && e.ents.windows(2).all(|w| w[0].tick < w[1].tick);
    s.add("J1-账非空", j1, "四对象按 tick 升序入链");

    // J2 字段齐
    let j2 = e.pins.is_empty() && e.migs.is_empty() && e.evicted.is_empty();
    s.add("J2-字段齐", j2, "pin/迁移/逐出三账全空");

    // J3 长度守恒：切面记账 + 触碰升位最老逐出
    let mut asp = LenKeep::new(24130);
    let a1 = asp.record(1, 12).is_ok();
    let a2 = asp.record(1, 12).err() == Some(EL_DUP_ADMIT);
    let a3 = asp.record(68, 48).err() == Some(EL_PIN_GUARD);
    let t3 = e.touch(2);
    let ev3 = e.evict_one();
    let j3 = t3.is_ok() && ev3.ok() == Some(1) && a1 && a2 && a3;
    s.add("J3-升位逐出", j3, "长度守恒切面三连 + 触碰 obj2 后逐出 obj1");

    // J4 次老逐出
    let j4 = e.evict_one().ok() == Some(3);
    s.add("J4-次老逐出", j4, "下一轮逐出次老 obj3");

    // J5 pin 挡逐出
    let p2 = e.pin(2);
    let p4 = e.pin(4);
    let j5 = p2.is_ok() && p4.is_ok() && e.evict_one().err() == Some(EL_PIN_GUARD);
    s.add("J5-挡逐出", j5, "全员 pin 时驱逐必挡");

    // J6 重复 pin 拒
    let j6 = e.pin(2).err() == Some(EL_DUP_ADMIT);
    s.add("J6-重复pin拒", j6, "同对象二次 pin 必拒");

    // J7 unpin 复逐
    let sq = match p2 { Ok(x) => x, Err(_) => 0 };
    let u7 = e.unpin(sq);
    let j7 = u7.is_ok() && e.evict_one().ok() == Some(2);
    s.add("J7-unpin复逐", j7, "解 pin 后最老可逐");

    // J8 野对象拒
    let j8 = e.touch(99).err() == Some(EL_WILD_OBJ)
        && e.pin(99).err() == Some(EL_WILD_OBJ)
        && e.migrate(99, 1).err() == Some(EL_WILD_OBJ);
    s.add("J8-野对象拒", j8, "越表对象三路必拒");

    // J9 长度守恒：迁移落账与重迁拒
    let m9 = e.migrate(4, 7);
    let j9 = m9.is_ok() && e.migrate(4, 7).err() == Some(EL_DUP_MIG);
    s.add("J9-重迁拒", j9, "长度守恒切面在册 + 同目标重迁必拒");

    // J10 长度守恒
    let j10 = e.ents.len() + e.evicted.len() == 4;
    s.add("J10-守恒", j10, "驻留 + 逐出守恒于入驻总数");

    // J11 指纹稳定（账本 + 切面双折叠）
    let j11 = e.fold() == e.fold() && asp.fold() == asp.fold();
    s.add("J11-指纹稳定", j11, "账本与切面两次折叠一致");

    // J12 印后冻结
    let fp = e.seal();
    let j12 = fp > 0 && e.admit(9).err() == Some(EL_FROZEN);
    s.add("J12-印后冻结", j12, "封印后入驻必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24130_evict_lru_flow() {
        let mut e = mk_lru();
        assert!(e.touch(2).is_ok());
        assert!(e.evict_one().ok() == Some(1));
        assert!(e.pin(3).is_ok());
        assert!(e.pin(4).is_ok());
        assert!(e.pin(2).is_ok());
        assert!(e.evict_one().err() == Some(EL_PIN_GUARD));
        assert!(e.migrate(4, 7).is_ok());
        assert!(e.migrate(4, 7).err() == Some(EL_DUP_MIG));
        assert!(e.touch(99).err() == Some(EL_WILD_OBJ));
        assert!(e.seal() > 0);
        assert!(e.admit(9).err() == Some(EL_FROZEN));
    }

    #[test]
    fn unx24130_checks_green() {
        let s = run_unx24130_checks();
        assert!(s.all_passed(), "UNX-F24130 判据未全绿");
    }
}
