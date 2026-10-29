//! UNX-F48665 —— I·AI-73 判据库收录登记（三账本收入机器人库）
//! 判据原文：UNX-F48665-J1 收录留痕可查；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 三账本齐收；R2 留痕可查；R3 摘要非零；
//! R4 重复收录拒；R5 查无此账；R6 收录序守恒；R7 全量对账；
//! R8 指纹稳定；R9 呈现行；R10 幂等复登被拒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：AI-73 收录以三账本（准确率账/延迟账/输入包账）为闭集，
//! 收录即冻结摘要；留痕可查 = 按 ID 可查回账本名与摘要；
//! 重复收录、零摘要、查无此账均为收录红线。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0A80;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ID: u16 = SEG_BASE + 1; // 账本 ID 越出闭集
const E_DUP: u16 = SEG_BASE + 2; // 重复收录
const E_DIGEST: u16 = SEG_BASE + 3; // 摘要为零
const E_NOTFOUND: u16 = SEG_BASE + 4; // 查无此账
const E_COUNT: u16 = SEG_BASE + 5; // 收录数不足 3
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 8; // 收录序漂移
const E_TITLE: u16 = SEG_BASE + 9; // 账本名非法

/// 三账本 ID 闭集。
pub const LB_ACC: u32 = 1; // 三体裁准确率账
pub const LB_LAT: u32 = 2; // 延迟账
pub const LB_PACK: u32 = 3; // 输入包账

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 账本 ID 闭集判定。
pub fn ledger_ok(id: u32) -> bool {
    id == LB_ACC || id == LB_LAT || id == LB_PACK
}

/// 一条收录留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegEntry {
    pub ledger_id: u32,
    pub title: String,
    pub digest: u64, // 账本摘要（收录即冻结）
    pub seq: u32,    // 收录序
}

/// AI-73 收录登记库。
pub struct RegLedger {
    pub entries: Vec<RegEntry>,
    pub next_seq: u32,
}

impl RegLedger {
    pub fn new() -> RegLedger {
        RegLedger { entries: vec![], next_seq: 1 }
    }

    /// 收录一账：ID 闭集 + 名非空 + 摘要非零 + 重复拒。
    pub fn register(&mut self, ledger_id: u32, title: &str, digest: u64) -> Result<u32, u16> {
        if !ledger_ok(ledger_id) {
            return Err(E_ID);
        }
        if title.len() < 3 {
            return Err(E_TITLE);
        }
        if digest == 0 {
            return Err(E_DIGEST);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].ledger_id == ledger_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let seq = self.next_seq;
        self.entries.push(RegEntry {
            ledger_id,
            title: title.to_string(),
            digest,
            seq,
        });
        self.next_seq += 1;
        Ok(seq)
    }

    /// 留痕可查：按 ID 查收录项。
    pub fn lookup(&self, ledger_id: u32) -> Result<(String, u64), u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].ledger_id == ledger_id {
                return Ok((self.entries[i].title.clone(), self.entries[i].digest));
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 收录序守恒：seq 连续 1..n。
    pub fn order_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].seq != i as u32 + 1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全量对账：三账本齐。
    pub fn reconcile(&self) -> Result<u32, u16> {
        if self.entries.len() != 3 {
            return Err(E_COUNT);
        }
        let mut k = 1u32;
        while k <= 3 {
            self.lookup(k)?;
            k += 1;
        }
        Ok(3)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].ledger_id.to_le_bytes());
            buf.extend_from_slice(self.entries[i].title.as_bytes());
            buf.extend_from_slice(&self.entries[i].digest.to_le_bytes());
            buf.extend_from_slice(&self.entries[i].seq.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造三账本摘要（演示源：确定性数值）。
pub fn make_digests(g: &mut Lcg) -> (u64, u64, u64) {
    let d1 = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
    let d2 = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
    let d3 = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
    (d1, d2, d3)
}

/// 按序取收录项（收录序守恒断言用）。
pub fn entry_at(l: &RegLedger, idx: usize) -> Option<&RegEntry> {
    if idx >= l.entries.len() {
        return None;
    }
    Some(&l.entries[idx])
}

/// 收录名表冻结断言：三账本名逐一比对。
pub fn titles_frozen(l: &RegLedger) -> Result<(), u16> {
    let want = ["acc-ledger", "lat-ledger", "pack-ledger"];
    if l.entries.len() != want.len() {
        return Err(E_COUNT);
    }
    let mut i = 0usize;
    while i < want.len() {
        match entry_at(l, i) {
            Some(e) => {
                if e.title != want[i] {
                    return Err(E_NOTFOUND);
                }
            }
            None => return Err(E_NOTFOUND),
        }
        i += 1;
    }
    Ok(())
}

/// 呈现行：reg=<收录数> seq=<末序>。
pub fn ledger_line(l: &RegLedger) -> String {
    let mut out = String::new();
    out.push_str("reg=");
    out.push_str(&l.entries.len().to_string());
    out.push_str(" seq=");
    let last = if l.entries.is_empty() {
        0
    } else {
        l.entries[l.entries.len() - 1].seq
    };
    out.push_str(&last.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_65);

    // R1 三账本齐收。
    let mut led = RegLedger::new();
    let (d1, d2, d3) = make_digests(&mut g);
    led.register(LB_ACC, "acc-ledger", d1)?;
    led.register(LB_LAT, "lat-ledger", d2)?;
    led.register(LB_PACK, "pack-ledger", d3)?;
    if led.entries.len() != 3 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 留痕可查：三账逐一查回原名与摘要。
    let t1 = led.lookup(LB_ACC)?;
    let t2 = led.lookup(LB_LAT)?;
    let t3 = led.lookup(LB_PACK)?;
    if t1.0 != "acc-ledger" || t1.1 != d1 || t2.1 != d2 || t3.1 != d3 {
        return Err(E_NOTFOUND);
    }
    hits += 1;

    // R3 摘要非零：构造零摘要账被拒。
    let mut probe = RegLedger::new();
    match probe.register(LB_ACC, "zero-digest", 0) {
        Err(E_DIGEST) => hits += 1,
        _ => return Err(E_DIGEST),
    }

    // R4 重复收录拒。
    match led.register(LB_ACC, "acc-again", 777) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 查无此账：未收录 ID 查询红。
    let mut probe2 = RegLedger::new();
    match probe2.lookup(LB_LAT) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R6 收录序守恒：seq 连续 1..3，名表冻结比对。
    if !led.order_ok() {
        return Err(E_ORDER);
    }
    titles_frozen(&led)?;
    hits += 1;

    // R7 全量对账：三账齐可查。
    // 对账即收录终态复核：三账逐一回查，缺一即收录不完整。
    let n = led.reconcile()?;
    if n != 3 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：reg=3 seq=3。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"reg=3") || !has_kw(line.as_bytes(), b"seq=3") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 幂等复登被拒 + ID 闭集拒收 + 名非法拒收。
    match led.register(LB_PACK, "pack-again", 999) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    match probe2.register(9, "out-of-set", 5) {
        Err(E_ID) => {}
        _ => return Err(E_ID),
    }
    match probe2.register(LB_ACC, "ab", 5) {
        Err(E_TITLE) => hits += 1,
        _ => return Err(E_TITLE),
    }

    Ok(hits)
}
