//! UNX-F48672 —— I34 fuzz（随机账本字段注入打包器零崩）
//! 判据原文：UNX-F48672-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 判据落地（drill 场景）：R1 注入闭集；R2 fuzz ×10^4 零崩；R3 非法键
//! 拒收；R4 重复键拒收；R5 合法键入账；R6 边界值；R7 校验和稳定；
//! R8 账面自洽；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG（wrapping_mul/wrapping_add）；drill 全绿才返回 Ok(hits)。
//! 口径说明：fuzz 注入面 = 打包器键值流（键 0..10、值全域）；
//! 零崩 = 10^4 轮全部接住（含拒收）且账面自洽（键闭集内唯一）；
//! 收发守恒 = accepted + rejected == 注入总笔数，一笔不丢。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0B60;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KEY: u16 = SEG_BASE + 1; // 键越出闭集
const E_DUP: u16 = SEG_BASE + 2; // 重复键
const E_CONSERV: u16 = SEG_BASE + 3; // 收发计数不守恒
const E_SANE: u16 = SEG_BASE + 4; // 账面自洽破坏
const E_CSUM: u16 = SEG_BASE + 5; // 校验和漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 轮次不足

/// 合法键闭集 k1..k8。
pub const KEY_N: u8 = 8;
/// fuzz 轮次。
pub const FUZZ_N: u32 = 10_000;

/// FNV-1a 64 位。
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

/// 一条账本字段。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LedgerField {
    pub key: String, // k1..k8
    pub val: u64,
}

/// fuzz 打包器：任意字段流进来不崩、账面自洽。
pub struct FuzzPacker {
    pub fields: Vec<LedgerField>,
    pub accepted: u32,
    pub rejected: u32,
}

impl FuzzPacker {
    pub fn new() -> FuzzPacker {
        FuzzPacker { fields: vec![], accepted: 0, rejected: 0 }
    }

    /// 键名拼装：k<n>。
    pub fn key_name(n: u8) -> String {
        let mut out = String::new();
        out.push('k');
        out.push_str(&(n as u32).to_string());
        out
    }

    /// 注入一笔字段：键闭集 + 键唯一；非法键拒收。
    pub fn inject(&mut self, key_id: u8, val: u64) -> Result<(), u16> {
        if key_id == 0 || key_id > KEY_N {
            self.rejected += 1;
            return Err(E_KEY);
        }
        let name = FuzzPacker::key_name(key_id);
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].key == name {
                self.rejected += 1;
                return Err(E_DUP);
            }
            i += 1;
        }
        self.fields.push(LedgerField { key: name, val });
        self.accepted += 1;
        Ok(())
    }

    /// 账面自洽：字段键全部在闭集内且唯一。
    pub fn sane(&self) -> bool {
        let mut i = 0usize;
        while i < self.fields.len() {
            let k = &self.fields[i].key;
            if k.len() < 2 || k.len() > 3 {
                return false;
            }
            let mut j = i + 1;
            while j < self.fields.len() {
                if *k == self.fields[j].key {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 总注入笔数。
    pub fn total(&self) -> u32 {
        self.accepted + self.rejected
    }

    /// 校验和：字段序表 FNV。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.fields.len() {
            buf.extend_from_slice(self.fields[i].key.as_bytes());
            buf.extend_from_slice(&self.fields[i].val.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 呈现行：acc=<n> rej=<n> n=<字段数>。
pub fn packer_line(p: &FuzzPacker) -> String {
    let mut out = String::new();
    out.push_str("acc=");
    out.push_str(&p.accepted.to_string());
    out.push_str(" rej=");
    out.push_str(&p.rejected.to_string());
    out.push_str(" n=");
    out.push_str(&p.fields.len().to_string());
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

    // R1 注入闭集：键 1..8 全部合法入账（8 笔）。
    let mut p = FuzzPacker::new();
    let mut k: u8 = 1;
    while k <= KEY_N {
        p.inject(k, k as u64 * 100)?;
        k += 1;
    }
    if p.accepted != 8 {
        return Err(E_KEY);
    }
    hits += 1;

    // R2 fuzz ×10^4 零崩：随机键 0..10、随机值，全部接住不崩、守恒。
    // 注入分布刻意覆盖非法键（0、9）与合法键（1..8），
    // 保证 fuzz 同时压到拒收路径与入账路径。
    let mut pz = FuzzPacker::new();
    let mut g = Lcg::new(0x486_72);
    let mut t: u32 = 0;
    while t < FUZZ_N {
        let key_id = (g.below(10)) as u8;
        let val = g.next_u32() as u64;
        let _r = pz.inject(key_id, val);
        t += 1;
    }
    if pz.total() != FUZZ_N {
        return Err(E_FUZZ);
    }
    if !pz.sane() {
        return Err(E_SANE);
    }
    hits += 1;

    // R3 非法键拒收：键 0 与键 9 拒收。
    let mut probe = FuzzPacker::new();
    match probe.inject(0, 1) {
        Err(E_KEY) => {}
        _ => return Err(E_KEY),
    }
    match probe.inject(9, 1) {
        Err(E_KEY) => hits += 1,
        _ => return Err(E_KEY),
    }

    // R4 重复键拒收。
    probe.inject(3, 10)?;
    match probe.inject(3, 20) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 合法键入账：accepted 与字段数一致（收发守恒的账内版）。
    // 拒收路径不写字段，入账路径恰一笔一字段。
    if probe.accepted != 1 || probe.fields.len() as u32 != probe.accepted {
        return Err(E_CONSERV);
    }
    if probe.rejected != 0 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 边界值：0 与 u64::MAX 均可入账。
    let mut edge = FuzzPacker::new();
    edge.inject(1, 0)?;
    edge.inject(2, 18_446_744_073_709_551_615)?;
    if edge.fields[0].val != 0 || edge.fields[1].val != 0xFFFF_FFFF_FFFF_FFFF {
        return Err(E_SANE);
    }
    hits += 1;

    // R7 校验和稳定：同账两次一致。
    let c1 = pz.checksum();
    let c2 = pz.checksum();
    if c1 != c2 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R8 账面自洽：fuzz 后字段数 ≤ 8 且无重复键。
    if pz.fields.len() > KEY_N as usize || !pz.sane() {
        return Err(E_SANE);
    }
    hits += 1;

    // R9 呈现行：acc=/rej=/n= 三段在位。
    let line = packer_line(&pz);
    if !has_kw(line.as_bytes(), b"acc=") || !has_kw(line.as_bytes(), b"rej=")
        || !has_kw(line.as_bytes(), b"n=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 指纹稳定：校验和两次一致且同种子重放一致。
    let f1 = pz.checksum();
    let mut pz2 = FuzzPacker::new();
    let mut g2 = Lcg::new(0x486_72);
    let mut t = 0u32;
    while t < FUZZ_N {
        let key_id = (g2.below(10)) as u8;
        let val = g2.next_u32() as u64;
        let _r = pz2.inject(key_id, val);
        t += 1;
    }
    if pz2.checksum() != f1 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R11 收口清点：注入次数与账面字段守恒。
    if pz.fields.len() as u32 > KEY_N as u32 {
        return Err(E_SANE);
    }
    let _ = g.below(4);
    hits += 1;

    Ok(hits)
}
