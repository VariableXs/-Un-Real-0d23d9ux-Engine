//! UNX-F11169 域经快照一（判例汇编：R-C4-001 账实修正）（0x1f450 段）
//!
//! 纯功能模型：判例 R-C4-001 的账实修正以事实链留痕——每条事实带序号、
//! 动作、经办人与前序哈希，本事实哈希由序号、动作、经办人、前序哈希唯一
//! 导出，链尾必须是修正动作。链可逐条复算复查，锁值后不再增补事实。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f450;

/// 事实序号越界。
pub const CB_ENO: u32 = SEG_BASE + 0x01;
/// 事实序号重复。
pub const CB_EDUP: u32 = SEG_BASE + 0x02;
/// 事实序号倒序。
pub const CB_EORDER: u32 = SEG_BASE + 0x03;
/// 动作类别不符（越界或缺修正动作）。
pub const CB_EKIND: u32 = SEG_BASE + 0x04;
/// 经办人越出经办域。
pub const CB_EACT: u32 = SEG_BASE + 0x05;
/// 事实链断（前序或哈希不符）。
pub const CB_ECHAIN: u32 = SEG_BASE + 0x06;
/// 事实条数不齐。
pub const CB_ECOUNT: u32 = SEG_BASE + 0x07;
/// 终值已锁定。
pub const CB_EFROZEN: u32 = SEG_BASE + 0x08;

/// 事实条数。
pub const FACTS: usize = 6;
/// 动作类别数。
pub const KINDS: u8 = 4;
/// 经办人数。
pub const ACTORS: u8 = 3;
/// 修正动作。
pub const ACT_CORRECT: u8 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 事实哈希导出：前序哈希、序号、动作、经办人逐项混入。
pub fn stamp(seq: u8, kind: u8, actor: u8, prev: u32) -> u32 {
    let mut h = 0x811c_9dc5u32 ^ prev;
    h = (h ^ seq as u32).wrapping_mul(0x0100_0193);
    h = (h ^ kind as u32).wrapping_mul(0x0100_0193);
    h = (h ^ actor as u32).wrapping_mul(0x0100_0193);
    h ^ (h >> 13)
}

/// 一条判例事实。
#[derive(Clone, Copy, Debug)]
pub struct Fact {
    pub seq: u8,
    pub kind: u8,
    pub actor: u8,
    pub prev: u32,
    pub hash: u32,
}

impl Fact {
    /// 动作类别在域内。
    pub fn kind_ok(&self) -> bool {
        self.kind < KINDS
    }

    /// 经办人在域内。
    pub fn actor_ok(&self) -> bool {
        self.actor < ACTORS
    }

    /// 哈希与本事实内容相符。
    pub fn hash_ok(&self) -> bool {
        self.hash == stamp(self.seq, self.kind, self.actor, self.prev)
    }
}

/// 判例事实链。
#[derive(Clone, Debug)]
pub struct Casebook {
    items: Vec<Fact>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl Casebook {
    /// 空链。
    pub fn new() -> Casebook {
        Casebook {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已留痕条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空链。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按序号取事实。
    pub fn fact(&self, seq: usize) -> Option<Fact> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].seq as usize == seq {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 链尾哈希（空链为零）。
    pub fn last_hash(&self) -> u32 {
        match self.items.last() {
            Some(f) => f.hash,
            None => 0,
        }
    }

    /// 留痕一条：冻结 → 越界 → 重记 → 倒序 → 动作 → 经办 → 链。
    pub fn record(
        &mut self,
        seq: usize,
        kind: u8,
        actor: u8,
        prev: u32,
        hash: u32,
    ) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CB_EFROZEN);
        }
        if seq == 0 || seq > FACTS {
            return self.fail(CB_ENO);
        }
        if self.fact(seq).is_some() {
            return self.fail(CB_EDUP);
        }
        if let Some(last) = self.items.last() {
            if seq <= last.seq as usize {
                return self.fail(CB_EORDER);
            }
        }
        let f = Fact {
            seq: seq as u8,
            kind,
            actor,
            prev,
            hash,
        };
        if !f.kind_ok() {
            return self.fail(CB_EKIND);
        }
        if !f.actor_ok() {
            return self.fail(CB_EACT);
        }
        if prev != self.last_hash() || !f.hash_ok() {
            return self.fail(CB_ECHAIN);
        }
        self.items.push(f);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 缺号表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut seq = 1usize;
        while seq <= FACTS {
            if self.fact(seq).is_none() {
                out.push(seq as u32);
            }
            seq = seq.wrapping_add(1);
        }
        out
    }

    /// 序号严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].seq <= self.items[i - 1].seq {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 链尾是否修正动作。
    pub fn corrected(&self) -> bool {
        match self.items.last() {
            Some(f) => f.kind == ACT_CORRECT,
            None => false,
        }
    }

    /// 逐条复算链：前序接续且哈希可复现。
    pub fn chain_ok(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            let f = self.items[i];
            if f.prev != prev || !f.hash_ok() {
                return false;
            }
            prev = f.hash;
            i = i.wrapping_add(1);
        }
        true
    }
}

impl Casebook {
    /// 事实链指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let f = self.items[i];
            h = (h ^ f.seq as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ f.kind as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ f.actor as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ f.prev as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ f.hash as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终核：六条齐 → 链尾修正 → 逐条复算 → 锁值。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CB_EFROZEN);
        }
        if self.items.len() != FACTS || !self.gaps().is_empty() || !self.order_strict() {
            return self.fail(CB_ECOUNT);
        }
        if !self.corrected() {
            return self.fail(CB_EKIND);
        }
        if !self.chain_ok() {
            return self.fail(CB_ECHAIN);
        }
        let v = self.fold();
        self.settled = Some(v);
        self.frozen = true;
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

/// 构建标准判例链：六条事实，链尾为修正动作。
pub fn build_final() -> Casebook {
    let mut c = Casebook::new();
    let mut prev = 0u32;
    let mut seq = 1usize;
    while seq <= FACTS {
        let kind = if seq == FACTS {
            ACT_CORRECT
        } else {
            (seq % 2) as u8
        };
        let actor = (seq % ACTORS as usize) as u8;
        let h = stamp(seq as u8, kind, actor, prev);
        let _ = c.record(seq, kind, actor, prev, h);
        prev = h;
        seq += 1;
    }
    let _ = c.settle();
    c
}

/// 四探针：越界、重复/倒序、动作经办链断、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = Casebook::new();
    let h1 = stamp(1, 0, 0, 0);
    let _ = a.record(1, 0, 0, 0, h1);
    if a.record(0, 0, 0, 0, h1) == Err(CB_ENO)
        && a.record(FACTS + 1, 0, 0, h1, stamp(7, 0, 0, h1)) == Err(CB_ENO)
    {
        r[0] = true;
    }

    let mut b = Casebook::new();
    let h2 = stamp(2, 0, 0, 0);
    let _ = b.record(2, 0, 0, 0, h2);
    if b.record(1, 0, 0, 0, stamp(1, 0, 0, 0)) == Err(CB_EORDER)
        && b.record(2, 0, 0, 0, h2) == Err(CB_EDUP)
    {
        r[1] = true;
    }

    let mut c = Casebook::new();
    let h1b = stamp(1, 0, 0, 0);
    let _ = c.record(1, 0, 0, 0, h1b);
    if c.record(2, KINDS, 0, h1b, stamp(2, KINDS, 0, h1b)) == Err(CB_EKIND)
        && c.record(2, 0, ACTORS, h1b, stamp(2, 0, ACTORS, h1b)) == Err(CB_EACT)
        && c.record(2, 0, 0, h1b, h1b ^ 1) == Err(CB_ECHAIN)
    {
        r[2] = true;
    }

    let mut d = build_final();
    if d.is_frozen()
        && d.record(FACTS + 1, 0, 0, d.last_hash(), 1) == Err(CB_EFROZEN)
        && d.settle() == Err(CB_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11169_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11169");

    let base = build_final();
    s.add(
        "X1-判例留痕",
        base.len() == FACTS && base.accepts == 6 && base.rejects == 0 && base.is_frozen(),
        "六条事实全收、零拒收且已锁定",
    );
    s.add(
        "X2-序号连续",
        base.gaps().is_empty() && base.order_strict(),
        "事实序号 1..=6 无缺且严格递增",
    );
    s.add(
        "X3-链可复算",
        base.chain_ok(),
        "前序接续且逐条哈希可复现",
    );
    s.add(
        "X4-链尾修正",
        base.corrected() && base.fact(FACTS).map(|f| f.kind) == Some(ACT_CORRECT),
        "链尾事实为账实修正动作",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = Casebook::new();
    let _ = fl.record(1, 0, 0, 0, stamp(1, 0, 0, 0));
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复倒序/动作经办链断/冻结后写四码可达",
    );

    let mut lr = Casebook::new();
    let h1 = stamp(1, 0, 0, 0);
    let _ = lr.record(1, 0, 0, 0, h1);
    let r0 = lr.record(0, 0, 0, 0, h1);
    let r1 = lr.record(FACTS + 1, 0, 0, h1, stamp(7, 0, 0, h1));
    s.add(
        "X8-越界拒收",
        r0 == Err(CB_ENO) && r1 == Err(CB_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "序号 0 与 7 均判越界",
    );

    let mut lo = Casebook::new();
    let h2 = stamp(2, 0, 0, 0);
    let _ = lo.record(2, 0, 0, 0, h2);
    let o = lo.record(1, 0, 0, 0, stamp(1, 0, 0, 0));
    let d = lo.record(2, 0, 0, 0, h2);
    s.add(
        "X9-倒序与重复",
        o == Err(CB_EORDER) && d == Err(CB_EDUP) && lo.accepts == 1 && lo.rejects == 2,
        "2 后写 1 判倒序、重写 2 判重复",
    );

    let mut lv = Casebook::new();
    let h1b = stamp(1, 0, 0, 0);
    let _ = lv.record(1, 0, 0, 0, h1b);
    let k = lv.record(2, KINDS, 0, h1b, stamp(2, KINDS, 0, h1b));
    let a = lv.record(2, 0, ACTORS, h1b, stamp(2, 0, ACTORS, h1b));
    let ch = lv.record(2, 0, 0, h1b, h1b ^ 1);
    s.add(
        "X10-动作经办链",
        k == Err(CB_EKIND)
            && a == Err(CB_EACT)
            && ch == Err(CB_ECHAIN)
            && lv.accepts == 1
            && lv.rejects == 3,
        "动作越界、经办越界、哈希不符分别拒收",
    );

    let mut lc = Casebook::new();
    let mut prev = 0u32;
    let mut seq = 1usize;
    while seq < FACTS {
        let h = stamp(seq as u8, 0, 0, prev);
        let _ = lc.record(seq, 0, 0, prev, h);
        prev = h;
        seq += 1;
    }
    let ec = lc.settle();
    s.add(
        "X11-条数不全",
        ec == Err(CB_ECOUNT) && lc.len() == 5 && lc.rejects == 1,
        "缺第六条即判条数不齐",
    );

    let mut lk = Casebook::new();
    let mut p = 0u32;
    let mut q = 1usize;
    while q <= FACTS {
        let h = stamp(q as u8, 0, 0, p);
        let _ = lk.record(q, 0, 0, p, h);
        p = h;
        q += 1;
    }
    let ek = lk.settle();
    s.add(
        "X12-链尾非修正",
        ek == Err(CB_EKIND) && !lk.corrected() && lk.rejects == 1,
        "链尾非修正动作即判动作不符",
    );

    let mut z = build_final();
    let zf = z.freeze();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && z.accepts == 6 && z.chain_ok(),
        &format!("acc={} rej={} tail={:?}", z.accepts, z.rejects, z.fact(6).map(|f| f.kind)),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11169_flow() {
        let mut c = build_final();
        assert_eq!(c.len(), 6);
        assert!(c.gaps().is_empty());
        assert!(c.order_strict());
        assert!(c.chain_ok());
        assert!(c.corrected());
        assert!(c.is_frozen());
        assert_eq!((c.accepts, c.rejects), (6, 0));
        let v = c.settled_value();
        assert!(v.is_some());
        assert_eq!(c.freeze(), v);
        assert_eq!(c.freeze(), v);
        assert_eq!(c.fact(6).map(|f| f.kind), Some(ACT_CORRECT));
        assert_eq!(c.fact(1).map(|f| f.prev), Some(0));
        assert!(c.fact(7).is_none());
        assert!(c.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11169_gates() {
        let mut a = Casebook::new();
        let h2 = stamp(2, 0, 0, 0);
        assert!(a.record(2, 0, 0, 0, h2).is_ok());
        assert_eq!(a.record(1, 0, 0, 0, stamp(1, 0, 0, 0)), Err(CB_EORDER));
        assert_eq!(a.record(2, 0, 0, 0, h2), Err(CB_EDUP));
        assert_eq!((a.accepts, a.rejects), (1, 2));

        let mut b = Casebook::new();
        let h1 = stamp(1, 0, 0, 0);
        assert!(b.record(1, 0, 0, 0, h1).is_ok());
        assert_eq!(
            b.record(2, KINDS, 0, h1, stamp(2, KINDS, 0, h1)),
            Err(CB_EKIND)
        );
        assert_eq!(
            b.record(2, 0, ACTORS, h1, stamp(2, 0, ACTORS, h1)),
            Err(CB_EACT)
        );
        assert_eq!(b.record(2, 0, 0, h1, h1 ^ 1), Err(CB_ECHAIN));
        assert_eq!(b.record(0, 0, 0, 0, h1), Err(CB_ENO));
        assert_eq!(b.record(FACTS + 1, 0, 0, h1, stamp(7, 0, 0, h1)), Err(CB_ENO));
        assert_eq!((b.accepts, b.rejects), (1, 5));

        let mut c = Casebook::new();
        let mut prev = 0u32;
        let mut seq = 1usize;
        while seq < FACTS {
            let h = stamp(seq as u8, 0, 0, prev);
            assert!(c.record(seq, 0, 0, prev, h).is_ok());
            prev = h;
            seq += 1;
        }
        assert_eq!(c.settle(), Err(CB_ECOUNT));
        assert_eq!((c.accepts, c.rejects), (5, 1));

        let mut d = Casebook::new();
        let mut p = 0u32;
        let mut q = 1usize;
        while q <= FACTS {
            let h = stamp(q as u8, 0, 0, p);
            assert!(d.record(q, 0, 0, p, h).is_ok());
            p = h;
            q += 1;
        }
        assert_eq!(d.settle(), Err(CB_EKIND));
        assert_eq!((d.accepts, d.rejects), (6, 1));

        let mut e = build_final();
        assert_eq!(e.settle(), Err(CB_EFROZEN));
        assert_eq!(
            e.record(FACTS + 1, 0, 0, e.last_hash(), 1),
            Err(CB_EFROZEN)
        );
        assert_eq!((e.accepts, e.rejects), (6, 2));
    }

    #[test]
    fn f11169_checks_green() {
        let s = run_unx11169_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
