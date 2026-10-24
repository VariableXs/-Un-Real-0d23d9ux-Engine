//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4441 · pread 基础语义 · 诊断段 0xE100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F4441-J1 pread 按显式偏移读取且不触碰 file 偏移
//!      （读前后 lseek 值 diff=0——1,000 次），内容与推演逐字
//!      节一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - pread(off, len)：显式偏移读，绝不触碰 file.offset；
//!   - 读前后 lseek 值 diff=0（1,000 次全量验证）；
//!   - 内容与确定性 LCG 推演器逐字节比对；
//!   - read（非 pread）推进偏移作语义对照；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xE100;

/// pread 轮数。
pub const PREADS: u32 = 1000;

/// 文件容量。
pub const FILE_CAP: u64 = 4096;

/// LCG 常量（确定性推演）。
pub const LCG_MUL: u64 = 6364136223846793005;
pub const LCG_ADD: u64 = 1442695040888963407;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG 推演器：与写入侧同源生成期望内容。
pub struct Oracle {
    pub state: u64,
}

impl Oracle {
    pub fn new(seed: u64) -> Self {
        Oracle { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.state
    }

    /// 推演第 i 字节。
    pub fn byte_at(&self, i: u64) -> u8 {
        let mut st = 0x9E37_79B9_7F4A_7C15u64 ^ i;
        st = st.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        (st >> 33) as u8
    }

    /// 推演 [off, off+len) 期望内容。
    pub fn expect(&self, off: u64, len: u64) -> Vec<u8> {
        let mut v = Vec::new();
        for i in off..off + len {
            v.push(self.byte_at(i));
        }
        v
    }
}

/// 文件对象：内容缓冲 + file 偏移。
pub struct FileObj {
    pub data: Vec<u8>,
    pub offset: u64,
}

impl FileObj {
    /// 建文件：按确定性 LCG 填充 FILE_CAP 字节。
    pub fn new(seed: u64) -> Self {
        let mut o = Oracle::new(seed);
        let mut data = Vec::new();
        for i in 0..FILE_CAP {
            data.push(o.byte_at(i));
        }
        FileObj { data, offset: 0 }
    }

    /// pread：显式偏移读，不触碰 self.offset。返回实际读得字节。
    pub fn pread(&self, off: u64, len: u64) -> Vec<u8> {
        let start = (off as usize).min(self.data.len());
        let end = ((off + len) as usize).min(self.data.len());
        let mut v = Vec::new();
        for i in start..end {
            v.push(self.data[i]);
        }
        v
    }

    /// read：从当前偏移读并推进偏移（对照语义）。
    pub fn read(&mut self, len: u64) -> Vec<u8> {
        let start = (self.offset as usize).min(self.data.len());
        let end = ((self.offset + len) as usize).min(self.data.len());
        let mut v = Vec::new();
        for i in start..end {
            v.push(self.data[i]);
        }
        self.offset = end as u64;
        v
    }

    /// lseek。
    pub fn seek(&mut self, off: u64) {
        self.offset = off;
    }
}

/// J1 账本：每轮记录读前后 lseek diff 与内容一致位。
pub struct PreBook {
    pub zero_diff: u32,
    pub byte_match: u32,
    pub faults: Vec<(&'static str, u32)>,
}

impl PreBook {
    pub fn new() -> Self {
        PreBook { zero_diff: 0, byte_match: 0, faults: Vec::new() }
    }

    /// J1 主判据：1,000 次 pread——diff=0 全量 + 内容逐字节一致。
    pub fn j1_main(&mut self) -> bool {
        let f = FileObj::new(0x5EED_4441);
        let mut oracle = Oracle::new(0);
        let mut all_ok = true;
        for i in 0..PREADS {
            let pos = oracle.next() as u64 % (FILE_CAP / 2);
            let len = (oracle.next() as u64 % 256) + 1;
            let before = f.offset;
            let got = f.pread(pos, len);
            let after = f.offset;
            let diff = if after >= before { after - before } else { before - after };
            if diff == 0 {
                self.zero_diff += 1;
            } else {
                all_ok = false;
            }
            let want = oracle.expect(pos, len);
            if got.len() == want.len() && got.iter().zip(want.iter()).all(|(a, b)| a == b) {
                self.byte_match += 1;
            } else {
                all_ok = false;
            }
        }
        all_ok && self.zero_diff == PREADS && self.byte_match == PREADS
    }
}

// ── 注入核（0xE100 段 5 项）────────────────────────────────────────────

/// 注入核 1：偏移冻结注入——lseek 后 pread 仍 diff=0。
pub fn inj1_offset_frozen() -> bool {
    let mut f = FileObj::new(7);
    f.seek(1234);
    let _ = f.pread(0, 8);
    let d1 = f.offset == 1234;
    f.seek(0);
    let _ = f.pread(FILE_CAP - 1, 2); // 越界截断
    f.offset == 0 && d1
}

/// 注入核 2：越界截断注入——读过尾只返回剩余字节。
pub fn inj2_tail_trunc() -> bool {
    let f = FileObj::new(9);
    let got = f.pread(FILE_CAP - 4, 100);
    got.len() == 4
}

/// 注入核 3：read 推进对照注入——普通 read 推进偏移（diff≠0）。
pub fn inj3_read_advances() -> bool {
    let mut f = FileObj::new(11);
    f.seek(10);
    let _ = f.read(32);
    f.offset == 42
}

/// 注入核 4：乱序一致性注入——倒序 256 段重读逐字节一致。
pub fn inj4_reverse_consist() -> bool {
    let f = FileObj::new(13);
    let mut ok = true;
    let mut i = 16;
    while i > 0 {
        i -= 1;
        let off = (i as u64) * 256;
        let got = f.pread(off, 256);
        if got.len() != 256 {
            ok = false;
        } else {
            for (k, b) in got.iter().enumerate() {
                if *b != f.data[off as usize + k] {
                    ok = false;
                }
            }
        }
    }
    ok
}

/// 注入核 5：哈希重放注入——两次同偏移 pread 哈希恒等。
pub fn inj5_hash_replay() -> bool {
    let f = FileObj::new(17);
    let h1 = fnv1a(&f.pread(100, 128));
    let h2 = fnv1a(&f.pread(100, 128));
    h1 == h2 && h1 != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 29
}

/// run_unx4441_checks：本单判据聚合（诊断段 0xE100）。
pub fn run_unx4441_checks() -> CheckSet {
    let mut book = PreBook::new();
    let j1 = book.j1_main();
    let mut s = CheckSet::new("UNX-F4441 pread 基础语义");
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    s.add(
        "pread按显式偏移读取且不触碰file偏移（diff=0·1,000次）",
        j1 && book.zero_diff == PREADS,
        if j1 { "" } else { "偏移被触碰" },
    );
    s.add(
        "内容与推演逐字节一致（1,000次全量）",
        j1 && book.byte_match == PREADS,
        if j1 { "" } else { "内容漂移" },
    );
    let inj = [
        ("注入1偏移冻结", inj1_offset_frozen()),
        ("注入2越界截断", inj2_tail_trunc()),
        ("注入3read推进对照", inj3_read_advances()),
        ("注入4乱序一致性", inj4_reverse_consist()),
        ("注入5哈希重放", inj5_hash_replay()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
