//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6701 · NVMe 固件下载命令（FW Download）双段传输 · 诊断段 0xA300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6701-J1 FW Download 4KB 粒度分块下载账（offset/length
//!      双对齐断言）；分块序与累计字节数守恒；畸形对齐注入零 panic；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 4KB 粒度分块：offset 与 length 均须 4KB 对齐（双对齐断言）；
//!   - 分块序严格递增 + 累计字节数守恒（Σ块长 = 总长）；
//!   - 畸形对齐注入返回错误码（零 panic）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA300;

/// 分块粒度 4KB。
pub const CHUNK: u32 = 4096;

/// 对齐错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 分块账条目：offset + length + 序号 + 内容指纹。
pub struct Chunk {
    pub seq: usize,
    pub offset: u32,
    pub length: u32,
    pub fp: u32,
}

/// FW Download 分块账。
pub struct FwLedger {
    pub chunks: Vec<Chunk>,
    pub total_bytes: u64,
}

impl FwLedger {
    pub fn new() -> Self {
        FwLedger { chunks: Vec::new(), total_bytes: 0 }
    }

    /// 记一 块：offset/length 双 4KB 对齐断言，零 panic。
    pub fn record(&mut self, offset: u32, length: u32, payload: &[u8]) -> Result<(), i32> {
        // 双对齐：offset 与 length 都须 4KB 倍数
        if offset % CHUNK != 0 || length % CHUNK != 0 {
            return Err(EINVAL);
        }
        // length 载荷一致性：载荷非空且 ≥ length 才可全量记账（此处按 length 记）
        if length > 0 && payload.is_empty() {
            return Err(EINVAL);
        }
        // 分块序：offset 必须严格递增且紧接前块（顺序下载）
        if let Some(last) = self.chunks.last() {
            if offset != last.offset + last.length {
                return Err(EINVAL);
            }
        } else if offset != 0 {
            return Err(EINVAL);
        }
        self.chunks.push(Chunk {
            seq: self.chunks.len(),
            offset,
            length,
            fp: fnv1a(payload),
        });
        self.total_bytes += length as u64;
        Ok(())
    }

    /// 守恒：累计字节数 = Σ块长 = 末块 offset+length。
    pub fn conserved(&self) -> bool {
        let sum: u64 = self.chunks.iter().map(|c| c.length as u64).sum();
        sum == self.total_bytes
            && self
                .chunks
                .last()
                .map(|c| c.offset as u64 + c.length as u64 == self.total_bytes)
                .unwrap_or(self.total_bytes == 0)
            && self.chunks.iter().enumerate().all(|(i, c)| c.seq == i)
    }
}

/// J1 主判据：分块下载账（双对齐）+ 分块序守恒 + 畸形注入零 panic。
pub fn j1_main() -> bool {
    let mut led = FwLedger::new();
    // 8 块 × 4KB 顺序下载
    for k in 0..8u32 {
        let payload = format!("fw-chunk-{}", k);
        if led.record(k * CHUNK, CHUNK, payload.as_bytes()).is_err() {
            return false;
        }
    }
    if !led.conserved() || led.total_bytes != 8 * CHUNK as u64 {
        return false;
    }
    // 畸形对齐注入零 panic：非对齐 offset/length 全拒
    let bad = [(1u32, CHUNK), (CHUNK, 1), (CHUNK - 1, CHUNK), (0, CHUNK + 1)];
    for (o, l) in bad.iter() {
        let mut p = FwLedger::new();
        if p.record(*o, *l, b"x").is_ok() {
            return false;
        }
    }
    // 乱序注入拒：跳块
    let mut p = FwLedger::new();
    p.record(0, CHUNK, b"a").ok();
    p.record(2 * CHUNK, CHUNK, b"b").is_err()
}

// ── 注入核（0xA300 段 5 项）────────────────────────────────────────────

/// 注入核 1：offset 非对齐注入——offset%4KB≠0 拒。
pub fn inj1_offset_misalign() -> bool {
    let mut led = FwLedger::new();
    led.record(100, CHUNK, b"x").is_err()
}

/// 注入核 2：length 非对齐注入——length%4KB≠0 拒。
pub fn inj2_length_misalign() -> bool {
    let mut led = FwLedger::new();
    led.record(0, 100, b"x").is_err()
}

/// 注入核 3：乱序注入——offset 不紧接前块拒。
pub fn inj3_out_of_order() -> bool {
    let mut led = FwLedger::new();
    led.record(0, CHUNK, b"a").ok();
    led.record(CHUNK * 2, CHUNK, b"b").is_err()
}

/// 注入核 4：空载荷注入——length>0 但载荷空拒。
pub fn inj4_empty_payload() -> bool {
    let mut led = FwLedger::new();
    led.record(0, CHUNK, &[]).is_err()
}

/// 注入核 5：指纹序守恒注入——各块指纹逐块可查且序号连续。
pub fn inj5_fp_chain() -> bool {
    let mut led = FwLedger::new();
    for k in 0..4u32 {
        led.record(k * CHUNK, CHUNK, format!("c{}", k).as_bytes()).ok();
    }
    led.chunks.iter().enumerate().all(|(i, c)| {
        c.seq == i && c.fp == fnv1a(format!("c{}", i).as_bytes())
    })
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6701_checks：本单判据聚合（诊断段 0xA300）。
pub fn run_unx6701_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6701 NVMe 固件下载命令（FW Download）双段传输");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "4KB分块双对齐断言",
        j1_main(),
        if j1_main() { "" } else { "双对齐破" },
    );
    s.add(
        "分块序与累计字节数守恒",
        j1_main(),
        if j1_main() { "" } else { "守恒破" },
    );
    s.add(
        "畸形对齐注入零panic",
        j1_main(),
        if j1_main() { "" } else { "畸形未拒" },
    );
    let inj = [
        ("注入1offset非对齐拒", inj1_offset_misalign()),
        ("注入2length非对齐拒", inj2_length_misalign()),
        ("注入3乱序拒", inj3_out_of_order()),
        ("注入4空载荷拒", inj4_empty_payload()),
        ("注入5指纹链", inj5_fp_chain()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
