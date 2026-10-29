//! UNX-F48681 —— I·J3 密钥单点消费复签（用户词/配置加密面年审）
//! 判据原文：UNX-F48681-J1 复签 ×10 密钥一致；判据锚定 §AI-61 §2 专题六。
//! 判据落地（drill 场景）：R1 密钥环构造；R2 单点一致性；R3 复签 ×10；
//! R4 消费槽闭集；R5 密钥指纹；R6 漂移检出；R7 越槽消费拒收；
//! R8 轮次一致；R9 呈现行；R10 环指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：J3 密钥单点消费 = 全域唯一主密钥（master_id/master_hash），
//! 用户词面与配置面皆为消费槽，槽内哈希必须与主哈希全等；
//! 复签 ×10 = 逐轮逐槽重算引用比对；漂移、越槽、环空均红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0C80;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SLOT: u16 = SEG_BASE + 1; // 消费槽越界
const E_DRIFT: u16 = SEG_BASE + 2; // 密钥漂移
const E_RESIGN: u16 = SEG_BASE + 3; // 复签失配
const E_EMPTY: u16 = SEG_BASE + 4; // 密钥环为空
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_HASH: u16 = SEG_BASE + 7; // 密钥哈希为零
const E_CONSUME: u16 = SEG_BASE + 8; // 单点消费被破坏

/// 消费槽闭集。
pub const S_USERLEX: u8 = 1; // 用户词加密面
pub const S_CFG: u8 = 2; // 配置加密面
/// 复签轮次。
pub const RESIGN_N: u32 = 10;

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

/// 消费槽闭集判定。
pub fn slot_ok(s: u8) -> bool {
    s == S_USERLEX || s == S_CFG
}

/// 一个消费槽：引用主密钥的哈希。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeySlot {
    pub slot: u8,
    pub key_hash: u64,
}

/// J3 密钥环：单主密钥，多消费槽引用。
pub struct KeyRing {
    pub master_id: u32,
    pub master_hash: u64,
    pub slots: Vec<KeySlot>,
}

impl KeyRing {
    pub fn new(master_id: u32, master_hash: u64) -> KeyRing {
        KeyRing { master_id, master_hash, slots: vec![] }
    }

    /// 单点消费：槽必须引用主密钥哈希。
    pub fn bind(&mut self, slot: u8) -> Result<(), u16> {
        if !slot_ok(slot) {
            return Err(E_SLOT);
        }
        if self.master_hash == 0 {
            return Err(E_HASH);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].slot == slot {
                return Err(E_SLOT);
            }
            i += 1;
        }
        self.slots.push(KeySlot { slot, key_hash: self.master_hash });
        Ok(())
    }

    /// 单点一致性：全槽哈希 == 主哈希。
    pub fn single_point(&self) -> Result<(), u16> {
        if self.slots.is_empty() {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].key_hash != self.master_hash {
                return Err(E_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }

    /// 复签 ×n：逐槽重算哈希引用并比对。
    pub fn resign(&self, n: u32) -> Result<u32, u16> {
        if n != RESIGN_N {
            return Err(E_RESIGN);
        }
        if self.slots.is_empty() {
            return Err(E_EMPTY);
        }
        let mut ok: u32 = 0;
        let mut round: u32 = 0;
        while round < n {
            let mut i = 0usize;
            while i < self.slots.len() {
                if self.slots[i].key_hash != self.master_hash {
                    return Err(E_RESIGN);
                }
                ok += 1;
                i += 1;
            }
            round += 1;
        }
        Ok(ok)
    }

    /// 漂移注入：直接改写某槽哈希（模拟异常）。
    pub fn tamper(&mut self, slot: u8, new_hash: u64) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].slot == slot {
                self.slots[i].key_hash = new_hash;
                return Ok(());
            }
            i += 1;
        }
        Err(E_SLOT)
    }

    /// 环指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.master_id.to_le_bytes());
        buf.extend_from_slice(&self.master_hash.to_le_bytes());
        let mut i = 0usize;
        while i < self.slots.len() {
            buf.push(self.slots[i].slot);
            buf.extend_from_slice(&self.slots[i].key_hash.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造标准环：单主密钥 + 两槽。
pub fn build_ring(seed: u32) -> KeyRing {
    let mut g = Lcg::new(seed);
    let h = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
    let mut r = KeyRing::new(3, h);
    let _ = r.bind(S_USERLEX);
    let _ = r.bind(S_CFG);
    r
}

/// 槽名表（呈现辅助）。
pub fn slot_name(s: u8) -> &'static str {
    if s == S_USERLEX {
        "userlex"
    } else {
        "cfg"
    }
}

/// 全槽绑定辅助：闭集内槽逐一绑定。
pub fn bind_all(r: &mut KeyRing) -> Result<(), u16> {
    r.bind(S_USERLEX)?;
    r.bind(S_CFG)?;
    Ok(())
}

/// 一轮复签留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResignTrace {
    pub round: u32,
    pub slots_ok: u32,
}

/// 复签 ×n 带留痕：逐轮逐槽比对，全绿落痕，红即断链。
pub fn resign_traced(r: &KeyRing, n: u32) -> Result<Vec<ResignTrace>, u16> {
    if n != RESIGN_N {
        return Err(E_RESIGN);
    }
    if r.slots.is_empty() {
        return Err(E_EMPTY);
    }
    let mut traces: Vec<ResignTrace> = vec![];
    let mut round: u32 = 1;
    while round <= n {
        let mut slots_ok: u32 = 0;
        let mut i = 0usize;
        while i < r.slots.len() {
            if r.slots[i].key_hash != r.master_hash {
                return Err(E_RESIGN);
            }
            slots_ok += 1;
            i += 1;
        }
        traces.push(ResignTrace { round, slots_ok });
        round += 1;
    }
    Ok(traces)
}

/// 呈现行：m=<主密钥 ID> slots=<槽数> sane=<0|1>。
pub fn ring_line(r: &KeyRing) -> String {
    let mut out = String::new();
    out.push_str("m=");
    out.push_str(&r.master_id.to_string());
    out.push_str(" slots=");
    out.push_str(&r.slots.len().to_string());
    let sane = match r.single_point() {
        Ok(()) => 1,
        Err(_) => 0,
    };
    out.push_str(" sane=");
    out.push_str(&sane.to_string());
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

    // R1 密钥环构造：主哈希非零、两槽在位。
    let ring = build_ring(0x486_81);
    if ring.master_hash == 0 || ring.slots.len() != 2 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R2 单点一致性：全槽引用同哈希。
    ring.single_point()?;
    hits += 1;

    // R3 复签 ×10：逐轮全槽比对，命中 20（2 槽 ×10 轮）。
    let ok = ring.resign(RESIGN_N)?;
    if ok != RESIGN_N * 2 {
        return Err(E_RESIGN);
    }
    hits += 1;

    // R4 消费槽闭集：槽 0 与槽 9 拒收。
    let mut probe = KeyRing::new(3, 0xDEAD_BEEF);
    match probe.bind(0) {
        Err(E_SLOT) => {}
        _ => return Err(E_SLOT),
    }
    match probe.bind(9) {
        Err(E_SLOT) => hits += 1,
        _ => return Err(E_SLOT),
    }

    // R5 密钥指纹：非零、变态即变。
    let h1 = ring.master_hash;
    if h1 == 0 {
        return Err(E_HASH);
    }
    let mut probe2 = build_ring(0x486_82);
    if probe2.master_hash == h1 {
        return Err(E_FP);
    }
    hits += 1;

    // R6 漂移检出：篡改一槽哈希 → 单点一致红；修回 → 复签全过。
    let mut bad = build_ring(0x486_81);
    bad.tamper(S_CFG, h1.wrapping_add(1))?;
    match bad.single_point() {
        Err(E_DRIFT) => {}
        _ => return Err(E_DRIFT),
    }
    bad.tamper(S_CFG, h1)?;
    bad.single_point()?;
    hits += 1;

    // R7 越槽消费拒收：重复绑同槽红。
    let mut probe3 = build_ring(0x486_81);
    match probe3.bind(S_USERLEX) {
        Err(E_SLOT) => hits += 1,
        _ => return Err(E_SLOT),
    }

    // R8 轮次一致：非 10 轮复签拒收。
    match ring.resign(9) {
        Err(E_RESIGN) => hits += 1,
        _ => return Err(E_RESIGN),
    }

    // R9 呈现行：m=3 slots=2 sane=1。
    let line = ring_line(&ring);
    if !has_kw(line.as_bytes(), b"m=3") || !has_kw(line.as_bytes(), b"slots=2")
        || !has_kw(line.as_bytes(), b"sane=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 环指纹稳定：同环两次一致且非零；
    // 槽名表冻结比对（userlex/cfg）。
    let f1 = ring.fp();
    let f2 = ring.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    if slot_name(S_USERLEX) != "userlex" || slot_name(S_CFG) != "cfg" {
        return Err(E_SLOT);
    }
    hits += 1;

    // R11 复签留痕 ×10：逐轮落痕、轮号连续、每轮槽全绿。
    let traces = resign_traced(&ring, RESIGN_N)?;
    if traces.len() as u32 != RESIGN_N {
        return Err(E_RESIGN);
    }
    let mut i = 0usize;
    while i < traces.len() {
        if traces[i].round != i as u32 + 1 || traces[i].slots_ok != 2 {
            return Err(E_RESIGN);
        }
        i += 1;
    }
    hits += 1;

    Ok(hits)
}
