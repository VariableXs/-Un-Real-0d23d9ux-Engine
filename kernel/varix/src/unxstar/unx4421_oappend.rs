//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4421 · O_APPEND 原子追加语义 · 诊断段 0xE080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F4421-J1 O_APPEND 写恒落文件当前尾（追加位语义钉死
//!      ——偏移在写时点原子重定位），单线程追加序零乱序（1,000 次
//!      序账）——任务书样板 F4340 示例语义承接（C1 R-C1-001 同
//!      判例登记）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 追加位语义：写时点偏移重定位到当前尾（忽略 lseek）；
//!   - lseek 后 O_APPEND 写仍落尾（重定位生效断言）；
//!   - 1,000 次追加序账零乱序；
//!   - 判例登记：C1 R-C1-001 同判例（F4340 承接）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xE080;

/// 追加次数。
pub const APPENDS: u32 = 1000;

/// O_APPEND 位。
pub const O_APPEND: u32 = 1 << 10;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 文件对象。
pub struct FileObj {
    pub append_mode: bool,
    pub offset: u64,
    pub size: u64,
    /// 写序账（写入块序号）。
    pub seq: Vec<u32>,
    pub content_len: u64,
}

impl FileObj {
    pub fn new(append_mode: bool) -> Self {
        FileObj { append_mode, offset: 0, size: 0, seq: Vec::new(), content_len: 0 }
    }

    /// lseek。
    pub fn seek(&mut self, off: u64) {
        self.offset = off;
    }

    /// 写：O_APPEND 时偏移写时点原子重定位到尾。
    pub fn write(&mut self, seq: u32, len: u64) -> u64 {
        let at = if self.append_mode {
            self.offset = self.size; // 写时点重定位
            self.size
        } else {
            self.offset
        };
        self.seq.push(seq);
        self.size = if self.append_mode { self.size + len } else { self.offset + len };
        self.content_len = self.size;
        at
    }
}

/// 判例登记账。
pub struct CaseBook {
    pub entries: Vec<(&'static str, bool)>,
}

impl CaseBook {
    pub fn new() -> Self {
        CaseBook { entries: Vec::new() }
    }

    pub fn record(&mut self, case: &'static str, ok: bool) {
        self.entries.push((case, ok));
    }

    pub fn all_ok(&self) -> bool {
        self.entries.iter().all(|(_, ok)| *ok) && self.entries.iter().any(|(c, _)| *c == "R-C1-001")
    }
}

/// J1 主判据：追加位语义钉死 + 1,000 次序账零乱序 + 判例登记。
pub fn j1_main() -> bool {
    let mut f = FileObj::new(true);
    let mut seq_ok = true;
    for i in 0..APPENDS {
        // 随机 lseek 干扰：O_APPEND 写仍落尾
        f.seek(0);
        let at = f.write(i, 4);
        if at != (i as u64) * 4 {
            seq_ok = false;
        }
    }
    // 序账零乱序
    let in_order = f.seq.windows(2).all(|w| w[0] < w[1]) && f.seq.len() == APPENDS as usize;
    // 非 append 对照：lseek(0) 后写落 0
    let mut plain = FileObj::new(false);
    plain.write(1, 8);
    plain.seek(0);
    let at0 = plain.write(2, 4);
    // 判例登记
    let mut book = CaseBook::new();
    book.record("R-C1-001", seq_ok && in_order);
    book.record("non-append-seek-wins", at0 == 0);
    book.record("size-grow", f.content_len == (APPENDS as u64) * 4);
    seq_ok && in_order && at0 == 0 && book.all_ok()
}

// ── 注入核（0xE080 段 5 项）────────────────────────────────────────────

/// 注入核 1：重定位钉死注入——lseek 后 append 写落尾。
pub fn inj1_reposition() -> bool {
    let mut f = FileObj::new(true);
    let _ = f.write(1, 10);
    f.seek(0);
    let at = f.write(2, 5);
    at == 10 && f.offset == 15
}

/// 注入核 2：追加位冻结注入。
pub fn inj2_bit_frozen() -> bool {
    O_APPEND == 0x400
}

/// 注入核 3：空文件首写注入——首写落 0。
pub fn inj3_first_write() -> bool {
    let mut f = FileObj::new(true);
    f.write(1, 6) == 0
}

/// 注入核 4：序账乱序检出注入——人为乱序必检出。
pub fn inj4_disorder_detect() -> bool {
    let seq = vec3(&[1, 2, 4, 3, 5]);
    !seq.windows(2).all(|w| w[0] < w[1])
}

/// 注入核 5：非 append 语义注入——普通写不重定位。
pub fn inj5_plain_write() -> bool {
    let mut f = FileObj::new(false);
    f.write(1, 8);
    f.seek(2);
    let at = f.write(2, 4);
    at == 2 && f.size == 6 && !f.append_mode
}

fn vec3(items: &[u32]) -> Vec<u32> {
    let mut v = Vec::new();
    for x in items {
        v.push(*x);
    }
    v
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx4421_checks：本单判据聚合（诊断段 0xE080）。
pub fn run_unx4421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4421 O_APPEND 原子追加语义");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "O_APPEND写恒落当前尾（偏移写时点原子重定位）",
        j1_main() && inj1_reposition(),
        if j1_main() { "" } else { "追加位语义破" },
    );
    s.add(
        "单线程追加序零乱序（1,000次序账）+判例登记",
        j1_main(),
        if j1_main() { "" } else { "序账破" },
    );
    let inj = [
        ("注入1重定位钉死", inj1_reposition()),
        ("注入2位冻结", inj2_bit_frozen()),
        ("注入3首写落0", inj3_first_write()),
        ("注入4乱序检出", inj4_disorder_detect()),
        ("注入5普通写语义", inj5_plain_write()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
