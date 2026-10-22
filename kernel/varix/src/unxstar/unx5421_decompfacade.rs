//! UNX-F5421 · 解压器门面账（统一接口）账 · 诊断段 0x9880
//! 验收锚点：UNX-F5421-J1 三算法统一解压门面账落定——trait 签名（algo_id +
//! 输入 → 输出缓冲）/错误通道（不支持/损坏/输出不足三分账）/输出缓冲三段；
//! 未知算法 10/10 次按不支持拒止；门面清单可导出（算法 ID→名称）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x9880;

pub const ALGO_NONE: u8 = 0;
pub const ALGO_STORE: u8 = 1;  // 原样存储
pub const ALGO_RLE: u8 = 2;    // 游程编码
pub const ALGO_XOR: u8 = 3;    // XOR delta

pub const E_NOSUP: i32 = -95;  // ENOTSUP 域内码
pub const E_CORRUPT: i32 = -74;
pub const E_NOSPACE: i32 = -28;

/// 解压错误通道三分账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecErr {
    NotSupported,
    Corrupt,
    NoSpace,
}

impl DecErr {
    pub fn code(self) -> i32 {
        match self {
            DecErr::NotSupported => E_NOSUP,
            DecErr::Corrupt => E_CORRUPT,
            DecErr::NoSpace => E_NOSPACE,
        }
    }
}

/// 门面 trait：统一签名。
pub trait DecompressFace {
    fn decompress(&self, algo: u8, input: &[u8], out: &mut Vec<u8>) -> Result<usize, DecErr>;
}

/// STORE：原样拷贝（输出不足 → NoSpace）。
pub fn dec_store(input: &[u8], out: &mut Vec<u8>) -> Result<usize, DecErr> {
    if out.capacity() - out.len() < input.len() {
        return Err(DecErr::NoSpace);
    }
    for b in input {
        out.push(*b);
    }
    Ok(input.len())
}

/// RLE：对 (count, byte) 对解码；奇长度或 count=0 → Corrupt。
pub fn dec_rle(input: &[u8], out: &mut Vec<u8>) -> Result<usize, DecErr> {
    if input.len() % 2 != 0 {
        return Err(DecErr::Corrupt);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        let count = input[i] as usize;
        let byte = input[i + 1];
        if count == 0 {
            return Err(DecErr::Corrupt);
        }
        if out.capacity() - out.len() < count {
            return Err(DecErr::NoSpace);
        }
        for _ in 0..count {
            out.push(byte);
            n += 1;
        }
        i += 2;
    }
    Ok(n)
}

/// XOR delta：首字节原样，后续与前一字节异或。
pub fn dec_xor(input: &[u8], out: &mut Vec<u8>) -> Result<usize, DecErr> {
    if input.is_empty() {
        return Ok(0);
    }
    if out.capacity() - out.len() < input.len() {
        return Err(DecErr::NoSpace);
    }
    let mut prev = input[0];
    out.push(prev);
    for b in &input[1..] {
        prev ^= *b;
        out.push(prev);
    }
    Ok(input.len())
}

/// 门面实现。
pub struct Facade {
    pub registry: Vec<(u8, &'static str)>,
}

impl Facade {
    pub fn new() -> Self {
        Facade { registry: vec![(ALGO_STORE, "store"), (ALGO_RLE, "rle"), (ALGO_XOR, "xor")] }
    }

    pub fn export(&self) -> String {
        self.registry.iter().map(|(id, n)| format!("{}:{}", id, n)).collect::<Vec<String>>().join(",")
    }
}

impl DecompressFace for Facade {
    fn decompress(&self, algo: u8, input: &[u8], out: &mut Vec<u8>) -> Result<usize, DecErr> {
        match algo {
            ALGO_STORE => dec_store(input, out),
            ALGO_RLE => dec_rle(input, out),
            ALGO_XOR => dec_xor(input, out),
            _ => Err(DecErr::NotSupported),
        }
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x9880 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x5421_9880;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("门面-三算法注册", Facade::new().registry.len() == 3));
    out.push(("错误-三分账码互异", E_NOSUP != E_CORRUPT && E_CORRUPT != E_NOSPACE));
    out.push(("RLE-奇长度Corrupt", dec_rle(&[1], &mut Vec::new()) == Err(DecErr::Corrupt)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx5421_checks：本单判据聚合（诊断段 0x9880）。
pub fn run_unx5421_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F5421 解压器门面统一接口");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    let f = Facade::new();

    // 三算法往返：store / rle / xor
    let data: Vec<u8> = (0..32u8).collect();
    let mut o1 = Vec::with_capacity(256);
    let r1 = f.decompress(ALGO_STORE, &data, &mut o1);
    let store_ok = r1 == Ok(32) && o1 == data;
    s.add("store-原样解压32B", store_ok, if store_ok { "" } else { "store 通道漂移" });

    // RLE：编码 (4,'A')(2,'B')(1,'C') → 解出 AAAABBBC
    let rle_in = [4u8, b'A', 2, b'B', 1, b'C'];
    let mut o2 = Vec::with_capacity(256);
    let r2 = f.decompress(ALGO_RLE, &rle_in, &mut o2);
    let rle_ok = r2 == Ok(7) && o2 == b"AAAABBBC".to_vec();
    s.add("rle-对解码AAAABBBC恰7B", rle_ok, if rle_ok { "" } else { "rle 通道漂移" });

    // XOR：delta 链还原
    let xor_in = [10u8, 5, 3]; // 10, 15, 12
    let mut o3 = Vec::with_capacity(64);
    let r3 = f.decompress(ALGO_XOR, &xor_in, &mut o3);
    let xor_ok = r3 == Ok(3) && o3 == vec![10u8, 15, 12];
    s.add("xor-delta链还原3B", xor_ok, if xor_ok { "" } else { "xor 通道漂移" });

    // 未知算法 10/10 拒止
    let mut rej = 0u32;
    for bad in [4u8, 9, 16, 32, 64, 100, 128, 200, 250, 255] {
        let mut ob = Vec::with_capacity(64);
        if f.decompress(bad, &data, &mut ob) == Err(DecErr::NotSupported) {
            rej += 1;
        }
    }
    s.add("拒止-未知算法10/10次NotSupported", rej == 10, if rej == 10 { "" } else { "拒止面缺口" });

    // 错误通道三分账：Corrupt（rle count=0）/ NoSpace（容量不足）/ NotSupported
    let mut ob = Vec::with_capacity(4);
    let c1 = f.decompress(ALGO_RLE, &[0u8, b'X'], &mut ob) == Err(DecErr::Corrupt);
    let mut ob2 = Vec::with_capacity(1);
    let c2 = f.decompress(ALGO_STORE, &data, &mut ob2) == Err(DecErr::NoSpace);
    let mut ob3 = Vec::with_capacity(8);
    let c3 = f.decompress(77, &data, &mut ob3) == Err(DecErr::NotSupported);
    s.add("通道-Corrupt/NoSpace/NotSupported三分账", c1 && c2 && c3, if c1 && c2 && c3 { "" } else { "错误通道缺口" });

    // 错误码映射
    let codes_ok = DecErr::NotSupported.code() == E_NOSUP && DecErr::Corrupt.code() == E_CORRUPT && DecErr::NoSpace.code() == E_NOSPACE;
    s.add("通道-三分账错误码映射", codes_ok, if codes_ok { "" } else { "错误码漂移" });

    // 门面清单可导出
    let exp = f.export();
    s.add("导出-三算法清单1:store,2:rle,3:xor", exp == "1:store,2:rle,3:xor", if exp == "1:store,2:rle,3:xor" { "" } else { "导出漂移" });

    // 100 轮随机算法分发不炸：合法三算法任分发均出结果
    let mut seed: u64 = 0x5421_0100;
    let mut ok_n = 0u32;
    for _ in 0..100 {
        let algo = match lcg_next(&mut seed) % 3 {
            0 => ALGO_STORE,
            1 => ALGO_RLE,
            _ => ALGO_XOR,
        };
        let mut ob = Vec::with_capacity(1024);
        if f.decompress(algo, &[2u8, b'Z', 3, b'Y', 1, b'X'], &mut ob).is_ok() {
            ok_n += 1;
        }
    }
    s.add("随机-100轮合法分发全出结果", ok_n == 100, if ok_n == 100 { "" } else { "随机分发缺口" });

    // FNV 门面指纹稳定
    let h1 = fnv1a(f.export().as_bytes());
    let h2 = fnv1a(b"1:store,2:rle,3:xor");
    s.add("FNV-门面指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
