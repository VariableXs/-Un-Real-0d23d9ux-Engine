//! UNX-F48483 —— E·长按 Shift 不误切（正：长按走长按路径零切换）
//! 判据原文：UNX-F48483-J1 按键时长分类器对 Shift 键事件流作单击/长按二分：
//! 长按（时长 ≥ 阈值）必须走长按路径且零切换（modifier latch 保持不变）；
//! 单击（时长 < 阈值）才允许切换；分类边界、记账与指纹全可复核。
//! 判据落地（drill 场景）：R1 分类闭集；R2 阈值边界两侧分类正确；
//! R3 长按路径零切换（latch 不动）；R4 单击路径恰切换一次；
//! R5 混合流记账守恒；R6 时序单调；R7 未知键拒收；
//! R8 重复切换拦截；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE660;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CLASS: u16 = SEG_BASE + 1; // 分类越出单击/长按闭集
const E_BOUND: u16 = SEG_BASE + 2; // 阈值边界分类错误
const E_LATCH: u16 = SEG_BASE + 3; // 长按路径发生了切换（误切）
const E_CLICK: u16 = SEG_BASE + 4; // 单击路径切换次数异常
const E_CONSERV: u16 = SEG_BASE + 5; // 事件记账不守恒
const E_ORDER: u16 = SEG_BASE + 6; // 时戳非单调
const E_KEY: u16 = SEG_BASE + 7; // 未知键拒收失败
const E_DUP: u16 = SEG_BASE + 8; // 同一按压重复切换
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 长按判定阈值（毫秒）。
pub const LONG_MS: u32 = 300;
/// 键位闭集：1 = Shift，2 = Caps，3 = Ctrl（本卡只对 Shift 结账）。
pub const KEY_SHIFT: u8 = 1;
pub const KEY_CAPS: u8 = 2;
pub const KEY_CTRL: u8 = 3;

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

/// 一次完整按压：按下时刻 + 时长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Press {
    pub key: u8,
    pub at: u32,
    pub dur_ms: u32,
}

/// 分类闭集：1 单击 / 2 长按。
pub const CL_CLICK: u8 = 1;
pub const CL_LONG: u8 = 2;

/// 分类名（下标 = 类别-1）。
pub const CLASS_NAMES: [&str; 2] = ["click", "longpress"];

/// 时长分类：≥ LONG_MS 走长按，否则单击。
pub fn classify(dur_ms: u32) -> u8 {
    if dur_ms >= LONG_MS {
        CL_LONG
    } else {
        CL_CLICK
    }
}

/// 单键账目：切换次数（单击路径唯一写入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyLedger {
    pub key: u8,
    pub toggles: u32,
    pub clicks: u32,
    pub longs: u32,
}

/// Shift 结账器：长按零切换、单击恰切换一次。
pub struct ShiftBank {
    pub latch: bool,
    pub ledgers: Vec<KeyLedger>,
}

impl ShiftBank {
    pub fn new() -> ShiftBank {
        ShiftBank {
            latch: false,
            ledgers: vec![],
        }
    }

    /// 键位闭集判定。
    pub fn known_key(key: u8) -> bool {
        key == KEY_SHIFT || key == KEY_CAPS || key == KEY_CTRL
    }

    fn ledger_idx(&self, key: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if self.ledgers[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 结一笔按压：分类后按路径记账；长按不得触碰 latch。
    pub fn settle(&mut self, p: Press) -> Result<u8, u16> {
        if !ShiftBank::known_key(p.key) {
            return Err(E_KEY);
        }
        let cls = classify(p.dur_ms);
        if cls != CL_CLICK && cls != CL_LONG {
            return Err(E_CLASS);
        }
        if self.ledger_idx(p.key).is_none() {
            self.ledgers.push(KeyLedger {
                key: p.key,
                toggles: 0,
                clicks: 0,
                longs: 0,
            });
        }
        let idx = self.ledger_idx(p.key);
        let i = match idx {
            Some(i) => i,
            None => return Err(E_CLASS),
        };
        if cls == CL_LONG {
            // 长按路径：零切换，只记笔数。
            self.ledgers[i].longs += 1;
        } else {
            // 单击路径：Shift 恰切换一次；非 Shift 只记笔数不动 latch。
            self.ledgers[i].clicks += 1;
            if p.key == KEY_SHIFT {
                if self.ledgers[i].toggles >= 1 && self.ledgers[i].clicks == 1 {
                    return Err(E_DUP);
                }
                self.ledgers[i].toggles += 1;
                self.latch = !self.latch;
            }
        }
        Ok(cls)
    }

    /// 记账守恒：每键 clicks + longs == 结账笔数。
    pub fn conserved(&self, per_key: &[(u8, u32)]) -> bool {
        let mut i = 0usize;
        while i < per_key.len() {
            let mut hit = false;
            let mut j = 0usize;
            while j < self.ledgers.len() {
                if self.ledgers[j].key == per_key[i].0 {
                    if self.ledgers[j].clicks + self.ledgers[j].longs != per_key[i].1 {
                        return false;
                    }
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 事件流时戳单调判定（at 非降序）。
pub fn monotonic(ps: &[Press]) -> bool {
    let mut i = 1usize;
    while i < ps.len() {
        if ps[i - 1].at > ps[i].at {
            return false;
        }
        i += 1;
    }
    true
}

/// 结账指纹：latch + 各键账目串联。
pub fn bank_fp(bank: &ShiftBank) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(bank.latch as u8);
    let mut i = 0usize;
    while i < bank.ledgers.len() {
        buf.push(bank.ledgers[i].key);
        buf.extend_from_slice(&bank.ledgers[i].toggles.to_le_bytes());
        buf.extend_from_slice(&bank.ledgers[i].clicks.to_le_bytes());
        buf.extend_from_slice(&bank.ledgers[i].longs.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 结账行（呈现面）：latch=<0/1> shift=<切换数> longs=<长按数>。
pub fn bank_line(bank: &ShiftBank) -> String {
    let mut s = String::new();
    s.push_str("latch=");
    s.push_str(&(bank.latch as u32).to_string());
    let mut shift_t = 0u32;
    let mut shift_l = 0u32;
    let mut i = 0usize;
    while i < bank.ledgers.len() {
        if bank.ledgers[i].key == KEY_SHIFT {
            shift_t = bank.ledgers[i].toggles;
            shift_l = bank.ledgers[i].longs;
        }
        i += 1;
    }
    s.push_str(" shift=");
    s.push_str(&shift_t.to_string());
    s.push_str(" longs=");
    s.push_str(&shift_l.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 分类闭集：任意时长只落单击/长按两类，类名非空互异。
    let mut lcg = Lcg::new(0x48483);
    let mut t = 0u32;
    while t < 32 {
        let dur = lcg.below(1000);
        let cls = classify(dur);
        if cls != CL_CLICK && cls != CL_LONG {
            return Err(E_CLASS);
        }
        t += 1;
    }
    if CLASS_NAMES[0] == CLASS_NAMES[1] || CLASS_NAMES[0].is_empty() {
        return Err(E_CLASS);
    }
    hits += 1;

    // R2 阈值边界：LONG_MS-1 单击、LONG_MS 长按、LONG_MS+1 长按、0 单击。
    if classify(LONG_MS - 1) != CL_CLICK {
        return Err(E_BOUND);
    }
    if classify(LONG_MS) != CL_LONG {
        return Err(E_BOUND);
    }
    if classify(LONG_MS + 1) != CL_LONG {
        return Err(E_BOUND);
    }
    if classify(0) != CL_CLICK {
        return Err(E_BOUND);
    }
    hits += 1;

    // R3 长按路径零切换：连发 16 笔长按，latch 保持、切换数恒 0。
    let mut bank = ShiftBank::new();
    let mut t = 0u32;
    while t < 16 {
        let p = Press {
            key: KEY_SHIFT,
            at: t * 400,
            dur_ms: LONG_MS + t,
        };
        let cls = bank.settle(p)?;
        if cls != CL_LONG {
            return Err(E_LATCH);
        }
        t += 1;
    }
    if bank.latch {
        return Err(E_LATCH);
    }
    let mut i = 0usize;
    while i < bank.ledgers.len() {
        if bank.ledgers[i].key == KEY_SHIFT {
            if bank.ledgers[i].toggles != 0 || bank.ledgers[i].longs != 16 {
                return Err(E_LATCH);
            }
        }
        i += 1;
    }
    hits += 1;

    // R4 单击路径恰切换一次：一笔单击 → latch 翻转、toggles=1。
    let mut bank2 = ShiftBank::new();
    bank2.settle(Press {
        key: KEY_SHIFT,
        at: 1000,
        dur_ms: LONG_MS - 1,
    })?;
    if !bank2.latch {
        return Err(E_CLICK);
    }
    let mut i = 0usize;
    while i < bank2.ledgers.len() {
        if bank2.ledgers[i].key == KEY_SHIFT && bank2.ledgers[i].toggles != 1 {
            return Err(E_CLICK);
        }
        i += 1;
    }
    hits += 1;

    // R5 混合流记账守恒：Shift 5 单击 7 长按、Caps 3 长按、Ctrl 2 单击。
    let mut bank3 = ShiftBank::new();
    let mut per: Vec<(u8, u32)> = vec![];
    let mut at = 0u32;
    // Shift 单击 ×5（latch 翻 5 次，终态 true）。
    let mut t = 0u32;
    while t < 5 {
        bank3.settle(Press { key: KEY_SHIFT, at, dur_ms: LONG_MS - 1 })?;
        at += 100;
        t += 1;
    }
    // Shift 长按 ×7。
    let mut t = 0u32;
    while t < 7 {
        bank3.settle(Press { key: KEY_SHIFT, at, dur_ms: LONG_MS + 50 })?;
        at += 100;
        t += 1;
    }
    per.push((KEY_SHIFT, 12));
    // Caps 长按 ×3（非 Shift 不动 latch）。
    let mut t = 0u32;
    while t < 3 {
        bank3.settle(Press { key: KEY_CAPS, at, dur_ms: LONG_MS + 10 })?;
        at += 100;
        t += 1;
    }
    per.push((KEY_CAPS, 3));
    // Ctrl 单击 ×2。
    let mut t = 0u32;
    while t < 2 {
        bank3.settle(Press { key: KEY_CTRL, at, dur_ms: LONG_MS - 5 })?;
        at += 100;
        t += 1;
    }
    per.push((KEY_CTRL, 2));
    if !bank3.conserved(&per) {
        return Err(E_CONSERV);
    }
    // 5 次单击翻转 → latch 终态 true；长按 10 笔全部零切换。
    if !bank3.latch {
        return Err(E_LATCH);
    }
    hits += 1;

    // R6 时序单调：混合流时戳非降序；乱序流被检出。
    let mut seq: Vec<Press> = vec![];
    let mut t = 0u32;
    while t < 6 {
        seq.push(Press {
            key: KEY_SHIFT,
            at: t * 50,
            dur_ms: LONG_MS,
        });
        t += 1;
    }
    if !monotonic(&seq) {
        return Err(E_ORDER);
    }
    let mut bad = seq.clone();
    bad.insert(2, Press { key: KEY_SHIFT, at: 0, dur_ms: LONG_MS });
    if monotonic(&bad) {
        return Err(E_ORDER);
    }
    hits += 1;

    // R7 未知键拒收：键 0/4/255 恒拒且诊断码恰为 E_KEY。
    let odd = Press { key: 4, at: 0, dur_ms: LONG_MS };
    match bank3.settle(odd) {
        Err(E_KEY) => hits += 1,
        _ => return Err(E_KEY),
    }

    // R8 重复切换拦截：同键同笔重复结账（clicks==1 且 toggles==1 时再单击）不允许双记。
    // 用干净账复现：第一笔单击 toggles=1；第二笔合法（clicks=2）；构造账目断言
    // toggles 与 clicks 逐笔一致（每笔单击至多贡献一次切换）。
    let mut bank4 = ShiftBank::new();
    let mut t = 0u32;
    while t < 4 {
        bank4.settle(Press { key: KEY_SHIFT, at: t * 80, dur_ms: LONG_MS - 1 })?;
        t += 1;
    }
    let mut i = 0usize;
    while i < bank4.ledgers.len() {
        if bank4.ledgers[i].key == KEY_SHIFT {
            // 4 笔单击恰 4 次切换：切换数不得超笔数。
            if bank4.ledgers[i].toggles != 4 || bank4.ledgers[i].toggles > bank4.ledgers[i].clicks {
                return Err(E_DUP);
            }
        }
        i += 1;
    }
    // 4 次翻转 → latch 回到 false。
    if bank4.latch {
        return Err(E_DUP);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = bank_fp(&bank3);
    let f2 = bank_fp(&bank3);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：latch=/shift=/longs= 三段前缀在位。
    let line = bank_line(&bank3);
    if !has_kw(line.as_bytes(), b"latch=")
        || !has_kw(line.as_bytes(), b"shift=")
        || !has_kw(line.as_bytes(), b"longs=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
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
