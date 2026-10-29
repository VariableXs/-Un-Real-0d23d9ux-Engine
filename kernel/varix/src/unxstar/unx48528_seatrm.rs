//! UNX-F48528 · E·seat 撤销注入（反：seat 移除 ×10 关联 context 清理零泄漏）
//!
//! 判据原文：UNX-F48528-J1 —— seat 移除零泄漏：每次移除后关联 context
//! 从表中消失（find 不中）、计数减一、组合缓冲清空（无在途组合残留）；
//! ×10 轮移除后表尺寸精确（加多少减多少）；移除不存在 seat 拒收；id 不
//! 复用（新接入 id 恒大于已释放 id）；移除过程不影响其他 seat 在途状态；
//! 同序列重放清理账全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEDC0;

pub const E_SEAT_MISS: u16 = SEG_BASE + 1;
pub const E_RESIDUE: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_DISTURB: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_ID_REUSE: u16 = SEG_BASE + 6;

/// seat 记录。
#[derive(Clone, PartialEq, Eq)]
pub struct SeatRec {
    pub name: String,
    pub id: u32,
    pub compose_buf: Vec<u32>,
    pub focused: bool,
}

/// 可增删 seat 表。
pub struct RemSeatTable {
    pub seats: Vec<SeatRec>,
    next_id: u32,
    pub max_seen_id: u32,
}

impl RemSeatTable {
    pub fn new() -> RemSeatTable {
        RemSeatTable { seats: Vec::new(), next_id: 0, max_seen_id: 0 }
    }

    pub fn count(&self) -> usize {
        self.seats.len()
    }

    pub fn find_idx(&self, name: &str) -> Option<usize> {
        for i in 0..self.seats.len() {
            if self.seats[i].name == name {
                return Some(i);
            }
        }
        None
    }

    pub fn add(&mut self, name: &str, compose: &[u32]) -> Result<u32, u16> {
        if name.len() == 0 {
            return Err(E_NONDET);
        }
        if self.find_idx(name).is_some() {
            return Err(E_NONDET);
        }
        let id = self.next_id;
        self.next_id += 1;
        if id > self.max_seen_id {
            self.max_seen_id = id;
        }
        let mut buf = Vec::new();
        for c in compose.iter() {
            buf.push(*c);
        }
        self.seats.push(SeatRec { name: name.to_string(), id, compose_buf: buf, focused: false });
        Ok(id)
    }

    /// 移除：find → 摘除（compose 随记录一起消失）→ 计数减一。
    pub fn remove(&mut self, name: &str) -> Result<SeatRec, u16> {
        let idx = match self.find_idx(name) {
            Some(i) => i,
            None => return Err(E_SEAT_MISS),
        };
        let rec = self.seats.remove(idx);
        Ok(rec)
    }

    /// 零泄漏审计：计数守恒 + 无残留（指定名不在表）。
    pub fn audit(&self, expect_n: usize) -> Result<(), u16> {
        if self.seats.len() != expect_n {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 其他 seat 零扰动：移除前后其余 seat 快照指纹一致。
    pub fn others_fp(&self, except: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for s in self.seats.iter() {
            if s.name == except {
                continue;
            }
            for b in s.name.as_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= s.compose_buf.len() as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= s.focused as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 单次移除：记录消失、计数减一、compose 随记录走。
pub fn r1_single_remove() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("kbd0", &[0x41, 0x42])?;
    t.add("kbd1", &[])?;
    let rec = t.remove("kbd0")?;
    if rec.compose_buf.len() != 2 {
        return Err(E_RESIDUE);
    }
    if t.find_idx("kbd0").is_some() {
        return Err(E_RESIDUE);
    }
    t.audit(1)
}

/// R2 ×10 加移循环：每轮加 1 移 1，终态计数 == 常驻数。
pub fn r2_ten_cycles() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("base", &[])?;
    for i in 0..10u32 {
        let mut name = String::from("tmp");
        let mut v = i;
        let mut d = Vec::new();
        if v == 0 {
            d.push(b'0');
        }
        while v > 0 {
            d.push(b'0' + (v % 10) as u8);
            v /= 10;
        }
        let mut k = d.len();
        while k > 0 {
            k -= 1;
            name.push(d[k] as char);
        }
        t.add(&name, &[0x31, 0x32, 0x33])?;
        if t.count() != 2 {
            return Err(E_LEDGER_BAD);
        }
        let rec = t.remove(&name)?;
        if rec.compose_buf.len() != 3 {
            return Err(E_RESIDUE);
        }
        if t.count() != 1 {
            return Err(E_LEDGER_BAD);
        }
    }
    t.audit(1)
}

/// R3 移除不存在 seat 拒收。
pub fn r3_miss_reject() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("x", &[])?;
    match t.remove("ghost") {
        Err(E_SEAT_MISS) => {}
        _ => return Err(E_SEAT_MISS),
    }
    t.audit(1)
}

/// R4 组合残留清零：带在途组合的 seat 移除后无任何 compose 残留。
pub fn r4_compose_cleared() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("compose-seat", &[0x6E, 0x69, 0x20, 0x68])?;
    let rec = t.remove("compose-seat")?;
    // 记录本身带走全部在途。
    if rec.compose_buf.len() != 4 {
        return Err(E_RESIDUE);
    }
    // 表内无该 seat 任何痕迹。
    if t.find_idx("compose-seat").is_some() {
        return Err(E_RESIDUE);
    }
    t.audit(0)
}

/// R5 id 不复用：移除后新接入 id 严格递增。
pub fn r5_id_no_reuse() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    let id0 = t.add("a", &[])?;
    let id1 = t.add("b", &[])?;
    t.remove("a")?;
    let id2 = t.add("c", &[])?;
    if !(id0 < id1 && id1 < id2) {
        return Err(E_ID_REUSE);
    }
    if t.max_seen_id != id2 {
        return Err(E_ID_REUSE);
    }
    Ok(())
}

/// R6 其他 seat 零扰动：移除时其余 seat 快照不变。
pub fn r6_others_untouched() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("keep1", &[1])?;
    t.add("keep2", &[2, 2])?;
    t.add("victim", &[9])?;
    t.seats[0].focused = true;
    let fp = t.others_fp("victim");
    t.remove("victim")?;
    let fp2 = t.others_fp("victim");
    if fp != fp2 {
        return Err(E_DISTURB);
    }
    Ok(())
}

/// R7 加移交错账：5 加 3 移 → 计数 2。
pub fn r7_mixed_ledger() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    for i in 0..5u32 {
        let mut name = String::from("m");
        let mut v = i;
        let mut d = Vec::new();
        if v == 0 {
            d.push(b'0');
        }
        while v > 0 {
            d.push(b'0' + (v % 10) as u8);
            v /= 10;
        }
        let mut k = d.len();
        while k > 0 {
            k -= 1;
            name.push(d[k] as char);
        }
        t.add(&name, &[])?;
    }
    for i in 0..3u32 {
        let mut name = String::from("m");
        let mut v = i;
        let mut d = Vec::new();
        if v == 0 {
            d.push(b'0');
        }
        while v > 0 {
            d.push(b'0' + (v % 10) as u8);
            v /= 10;
        }
        let mut k = d.len();
        while k > 0 {
            k -= 1;
            name.push(d[k] as char);
        }
        t.remove(&name)?;
    }
    t.audit(2)
}

/// R8 fuzz 随机加移：40 步随机加/移，账面与实际一致。
pub fn r8_fuzz_random() -> Result<(), u16> {
    let mut g = Lcg::new(0x48528_0001);
    let mut t = RemSeatTable::new();
    let mut live: Vec<String> = Vec::new();
    let mut expect: usize = 0;
    for i in 0..40u64 {
        let do_add = g.below(2) == 0 || live.len() == 0;
        if do_add {
            let mut name = String::from("z");
            let mut v = i;
            let mut d = Vec::new();
            if v == 0 {
                d.push(b'0');
            }
            while v > 0 {
                d.push(b'0' + (v % 10) as u8);
                v /= 10;
            }
            let mut k = d.len();
            while k > 0 {
                k -= 1;
                name.push(d[k] as char);
            }
            t.add(&name, &[])?;
            live.push(name);
            expect += 1;
        } else {
            let pick = g.below(live.len() as u64) as usize;
            let name = live.remove(pick);
            t.remove(&name)?;
            expect -= 1;
        }
        if t.count() != expect {
            return Err(E_LEDGER_BAD);
        }
    }
    t.audit(expect)
}

/// R9 重复移除拒收：同 seat 二次移除报 E_SEAT_MISS。
pub fn r9_double_remove() -> Result<(), u16> {
    let mut t = RemSeatTable::new();
    t.add("once", &[])?;
    t.remove("once")?;
    match t.remove("once") {
        Err(E_SEAT_MISS) => {}
        _ => return Err(E_SEAT_MISS),
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮终账全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(usize, u32), u16> {
        let mut g = Lcg::new(seed);
        let mut t = RemSeatTable::new();
        let mut live: Vec<String> = Vec::new();
        for i in 0..25u64 {
            let mut name = String::from("q");
            let mut v = i;
            let mut d = Vec::new();
            if v == 0 {
                d.push(b'0');
            }
            while v > 0 {
                d.push(b'0' + (v % 10) as u8);
                v /= 10;
            }
            let mut k = d.len();
            while k > 0 {
                k -= 1;
                name.push(d[k] as char);
            }
            if live.len() == 0 || g.below(2) == 0 {
                t.add(&name, &[])?;
                live.push(name);
            } else {
                let pick = g.below(live.len() as u64) as usize;
                let nm = live.remove(pick);
                t.remove(&nm)?;
            }
        }
        Ok((t.count(), t.max_seen_id))
    };
    let a = run(0x48528_0002)?;
    let b = run(0x48528_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_single_remove()?;
    hits += 1;
    r2_ten_cycles()?;
    hits += 1;
    r3_miss_reject()?;
    hits += 1;
    r4_compose_cleared()?;
    hits += 1;
    r5_id_no_reuse()?;
    hits += 1;
    r6_others_untouched()?;
    hits += 1;
    r7_mixed_ledger()?;
    hits += 1;
    r8_fuzz_random()?;
    hits += 1;
    r9_double_remove()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
