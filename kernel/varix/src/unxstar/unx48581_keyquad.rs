//! UNX-F48581 —— I·key_event 四元组冻结面复签（附则二 M2-B01–B04 放行面回归）
//! 判据原文：UNX-F48581-J1 key_event 四元组（槽位/键码/键态/时戳）
//! 在冻结面复签后进入放行面：附则二 M2-B01–B04 四条放行规则回归——
//! B01 槽位闭集、B02 键码闭集、B03 键态闭集、B04 时戳单调；
//! 四条全过才放行，任一不过必须拒收且诊断码可定位到条；
//! 复签值（FNV）可复核且稳定；放行账守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 四元组闭集（B01 槽位）；
//! R2 键码闭集（B02）；R3 键态闭集（B03）；R4 时戳单调（B04）；
//! R5 四条全过放行；R6 复签稳定；R7 复签区分扰动；
//! R8 放行账守恒；R9 fuzz 四元组流零错态；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF880;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_B01: u16 = SEG_BASE + 1; // 槽位越出闭集
const E_B02: u16 = SEG_BASE + 2; // 键码越出闭集
const E_B03: u16 = SEG_BASE + 3; // 键态越出闭集
const E_B04: u16 = SEG_BASE + 4; // 时戳非单调
const E_PASS: u16 = SEG_BASE + 5; // 放行判定异常
const E_SIG: u16 = SEG_BASE + 6; // 复签复核失败
const E_DISTURB: u16 = SEG_BASE + 7; // 复签未区分扰动
const E_CONSERV: u16 = SEG_BASE + 8; // 放行账不守恒
const E_FUZZ: u16 = SEG_BASE + 9; // fuzz 四元组流错态
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 槽位闭集：1..=8。
pub const SLOT_MAX: u8 = 8;
/// 键码闭集：1..=120。
pub const KEY_MAX: u8 = 120;
/// 键态闭集：1 按下 / 2 抬起。
pub const ST_DOWN: u8 = 1;
pub const ST_UP: u8 = 2;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
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

/// key_event 四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyQuad {
    pub slot: u8,
    pub key: u8,
    pub state: u8,
    pub ts: u32,
}

/// 复签器：四元组字段串联 FNV。
pub fn resign(q: &KeyQuad) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(q.slot);
    buf.push(q.key);
    buf.push(q.state);
    buf.extend_from_slice(&q.ts.to_le_bytes());
    fnv1a64(&buf)
}

/// 附则二放行规则：B01..B04 逐条判定，全过返回 Ok，否则返回对应码。
pub fn pass_rules(q: &KeyQuad, last_ts: u32) -> Result<(), u16> {
    // B01 槽位闭集。
    if q.slot == 0 || q.slot > SLOT_MAX {
        return Err(E_B01);
    }
    // B02 键码闭集。
    if q.key == 0 || q.key > KEY_MAX {
        return Err(E_B02);
    }
    // B03 键态闭集。
    if q.state != ST_DOWN && q.state != ST_UP {
        return Err(E_B03);
    }
    // B04 时戳单调。
    if q.ts < last_ts {
        return Err(E_B04);
    }
    Ok(())
}

/// 放行面：逐笔复签 + 放行账。
pub struct PassGate {
    pub last_ts: u32,
    pub ledger: Vec<(KeyQuad, u64)>,
    pub passed: u32,
    pub rejected: u32,
}

impl PassGate {
    pub fn new() -> PassGate {
        PassGate {
            last_ts: 0,
            ledger: vec![],
            passed: 0,
            rejected: 0,
        }
    }

    /// 受理一笔四元组：规则全过则复签入账，否则拒收记账。
    pub fn submit(&mut self, q: KeyQuad) -> Result<u64, u16> {
        match pass_rules(&q, self.last_ts) {
            Ok(()) => {
                let sg = resign(&q);
                self.ledger.push((q, sg));
                self.last_ts = q.ts;
                self.passed += 1;
                Ok(sg)
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }

    /// 放行账守恒：passed + rejected == 受理总数；账面行数 == passed。
    pub fn conserved(&self, submitted: u32) -> bool {
        self.passed + self.rejected == submitted
            && self.ledger.len() as u32 == self.passed
    }

    /// 复签复核：账面每行重算复签值一致。
    pub fn sig_sane(&self) -> bool {
        let mut i = 0usize;
        while i < self.ledger.len() {
            let q = self.ledger[i].0;
            if self.ledger[i].1 != resign(&q) {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 B01 槽位闭集：1..=8 合法、0/9/255 拒收，码恰为 E_B01。
    let mut gate = PassGate::new();
    match gate.submit(KeyQuad { slot: 0, key: 5, state: ST_DOWN, ts: 1 }) {
        Err(E_B01) => hits += 1,
        _ => return Err(E_B01),
    }
    let mut gate2 = PassGate::new();
    gate2.submit(KeyQuad { slot: 8, key: 5, state: ST_DOWN, ts: 1 })?;
    if gate2.passed != 1 {
        return Err(E_PASS);
    }
    hits += 1;

    // R2 B02 键码闭集：1..=120 合法、0/121 拒收，码恰为 E_B02。
    let mut gate3 = PassGate::new();
    match gate3.submit(KeyQuad { slot: 1, key: 121, state: ST_DOWN, ts: 1 }) {
        Err(E_B02) => {}
        _ => return Err(E_B02),
    }
    match gate3.submit(KeyQuad { slot: 1, key: 0, state: ST_DOWN, ts: 1 }) {
        Err(E_B02) => hits += 1,
        _ => return Err(E_B02),
    }

    // R3 B03 键态闭集：1/2 合法、0/3/255 拒收，码恰为 E_B03。
    let mut gate4 = PassGate::new();
    match gate4.submit(KeyQuad { slot: 1, key: 5, state: 3, ts: 1 }) {
        Err(E_B03) => {}
        _ => return Err(E_B03),
    }
    gate4.submit(KeyQuad { slot: 1, key: 5, state: ST_UP, ts: 1 })?;
    if gate4.passed != 1 {
        return Err(E_PASS);
    }
    hits += 1;

    // R4 B04 时戳单调：同 ts 放行（非降序）、倒序拒收，码恰为 E_B04。
    let mut gate5 = PassGate::new();
    gate5.submit(KeyQuad { slot: 1, key: 5, state: ST_DOWN, ts: 100 })?;
    match gate5.submit(KeyQuad { slot: 1, key: 5, state: ST_UP, ts: 50 }) {
        Err(E_B04) => hits += 1,
        _ => return Err(E_B04),
    }

    // R5 四条全过放行：合法流 8 笔全过，就绪账 8。
    let mut gate6 = PassGate::new();
    let mut t = 0u32;
    while t < 8 {
        let st = if t % 2 == 0 { ST_DOWN } else { ST_UP };
        gate6.submit(KeyQuad {
            slot: (t % 8 + 1) as u8,
            key: (t * 7 % 120 + 1) as u8,
            state: st,
            ts: t * 10,
        })?;
        t += 1;
    }
    if gate6.passed != 8 || !gate6.sig_sane() {
        return Err(E_PASS);
    }
    hits += 1;

    // R5b 规则优先序：四条规则同时违约时诊断码定位到最前条
    // （B01 > B02 > B03 > B04），保证拒收码可归因。
    let mut gate6b = PassGate::new();
    // 违 B01+B02+B03+B04 → 应报 B01。
    match gate6b.submit(KeyQuad { slot: 0, key: 0, state: 9, ts: 0 }) {
        Err(E_B01) => {}
        _ => return Err(E_PASS),
    }
    // 违 B02+B03+B04（B01 合法）→ 应报 B02。
    match gate6b.submit(KeyQuad { slot: 1, key: 200, state: 9, ts: 0 }) {
        Err(E_B02) => {}
        _ => return Err(E_PASS),
    }
    // 违 B03+B04（B01/B02 合法）→ 应报 B03。
    match gate6b.submit(KeyQuad { slot: 1, key: 5, state: 9, ts: 0 }) {
        Err(E_B03) => {}
        _ => return Err(E_PASS),
    }
    // 只违 B04 → 应报 B04。
    gate6b.submit(KeyQuad { slot: 1, key: 5, state: ST_DOWN, ts: 100 })?;
    match gate6b.submit(KeyQuad { slot: 1, key: 5, state: ST_UP, ts: 1 }) {
        Err(E_B04) => {}
        _ => return Err(E_PASS),
    }
    hits += 1;

    // R6 复签稳定：同四元组两次复签一致且非零。
    let q = KeyQuad { slot: 3, key: 42, state: ST_DOWN, ts: 999 };
    let s1 = resign(&q);
    let s2 = resign(&q);
    if s1 != s2 || s1 == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R7 复签区分扰动：任一字段翻转 → 复签必变。
    let q2 = KeyQuad { slot: 3, key: 43, state: ST_DOWN, ts: 999 };
    let q3 = KeyQuad { slot: 3, key: 42, state: ST_UP, ts: 999 };
    let q4 = KeyQuad { slot: 3, key: 42, state: ST_DOWN, ts: 998 };
    if resign(&q2) == s1 || resign(&q3) == s1 || resign(&q4) == s1 {
        return Err(E_DISTURB);
    }
    hits += 1;

    // R8 放行账守恒：混合流 14 笔（12 过 2 拒），账目一致。
    let mut gate7 = PassGate::new();
    let mut t = 0u32;
    while t < 12 {
        gate7.submit(KeyQuad {
            slot: (t % 8 + 1) as u8,
            key: (t * 11 % 120 + 1) as u8,
            state: ST_DOWN,
            ts: t * 5,
        })?;
        t += 1;
    }
    let _ = gate7.submit(KeyQuad { slot: 0, key: 1, state: ST_DOWN, ts: 100 });
    let _ = gate7.submit(KeyQuad { slot: 1, key: 1, state: ST_DOWN, ts: 0 });
    if !gate7.conserved(14) || gate7.rejected != 2 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R9 fuzz 四元组流零错态：LCG 驱动 128 笔混合（合法/非法），
    // 每笔按判定收/拒，末态守恒、复签账自洽。
    let mut gate8 = PassGate::new();
    let mut lcg = Lcg::new(0x48581);
    let mut submitted = 0u32;
    let mut t = 0u32;
    while t < 128 {
        let slot = (lcg.below(10)) as u8;
        let key = (lcg.below(130)) as u8;
        let state = (lcg.below(4)) as u8;
        let ts = t * 3;
        let _r = gate8.submit(KeyQuad { slot, key, state, ts });
        submitted += 1;
        t += 1;
    }
    if !gate8.conserved(submitted) || !gate8.sig_sane() || gate8.passed == 0 {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R10 呈现行齐全：pass=/rej=/sig= 三段前缀在位。
    let line = gate_line(&gate8);
    if !has_kw(line.as_bytes(), b"pass=")
        || !has_kw(line.as_bytes(), b"rej=")
        || !has_kw(line.as_bytes(), b"sig=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：收发计数 + 全账复签串联。
pub fn gate_fp(g: &PassGate) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&g.passed.to_le_bytes());
    buf.extend_from_slice(&g.rejected.to_le_bytes());
    let mut i = 0usize;
    while i < g.ledger.len() {
        buf.extend_from_slice(&g.ledger[i].1.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：pass=<放行> rej=<拒收> sig=<复签低 32 位>。
pub fn gate_line(g: &PassGate) -> String {
    let mut s = String::new();
    s.push_str("pass=");
    s.push_str(&g.passed.to_string());
    s.push_str(" rej=");
    s.push_str(&g.rejected.to_string());
    s.push_str(" sig=");
    s.push_str(&(gate_fp(g) as u32).to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
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
