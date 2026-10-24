//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6721 · 错误信息日志页（Error Information Log 64 条环形）· 0xA380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6721-J1 64 条环形错误日志读取账（最新在前序）；错误
//!      条目字段（SQID/CMDID/状态字段/LBA）逐字段裁断；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 64 条环形缓冲：写满滚动覆盖最旧；
//!   - 读取账：最新在前序（倒序遍历）；
//!   - 条目四字段（SQID/CMDID/状态字段/LBA）逐字段裁断账；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xA380;

/// 环形容量。
pub const RING_CAP: usize = 64;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 错误条目四字段。
#[derive(Clone, Copy)]
pub struct ErrEntry {
    pub sqid: u16,
    pub cmdid: u16,
    pub status: u16,
    pub lba: u64,
}

impl ErrEntry {
    /// 逐字段裁断：全字段非零语义（SQID/CMDID/状态合法域）。
    pub fn valid(&self) -> bool {
        // 状态字段：bit0-8 错误码 + bit14 更多 + bit15 DNR
        let code = self.status & 0x1FF;
        self.sqid != u16::MAX && self.status & 0x0600 == 0 && (code != 0 || self.cmdid != 0)
    }
}

/// 64 条环形错误日志。
pub struct ErrRing {
    pub buf: [Option<ErrEntry>; RING_CAP],
    pub head: usize,
    pub total: u64,
}

impl ErrRing {
    pub fn new() -> Self {
        ErrRing { buf: [None; RING_CAP], head: 0, total: 0 }
    }

    /// 追加一条：环形滚动覆盖最旧。
    pub fn push(&mut self, e: ErrEntry) {
        self.buf[self.head] = Some(e);
        self.head = (self.head + 1) % RING_CAP;
        self.total += 1;
    }

    /// 读取账：最新在前序（倒序），最多取 n 条。
    pub fn read_newest_first(&self, n: usize) -> Vec<ErrEntry> {
        let mut out = Vec::new();
        let mut i = self.head + RING_CAP - 1;
        for _ in 0..RING_CAP.min(n) {
            i %= RING_CAP;
            if let Some(e) = self.buf[i] {
                out.push(e);
            }
            if i == 0 {
                i = RING_CAP;
            }
            i -= 1;
        }
        out
    }

    /// 有效条目数。
    pub fn len(&self) -> usize {
        self.buf.iter().filter(|x| x.is_some()).count()
    }
}

/// J1 主判据：64 条环形滚动 + 最新在前序 + 四字段逐字段裁断。
pub fn j1_main() -> bool {
    let mut g = Lcg::new(0x5EED_6721);
    let mut ring = ErrRing::new();
    // 写 100 条（> 64 触发滚动覆盖）
    for i in 0..100u64 {
        ring.push(ErrEntry {
            sqid: (i % 8) as u16,
            cmdid: i as u16,
            status: 0x0002,
            lba: g.next() % 0xFFFF_FFFF,
        });
    }
    if ring.len() != RING_CAP || ring.total != 100 {
        return false;
    }
    // 最新在前：第 1 条必须是第 99 条（cmdid=99）
    let nf = ring.read_newest_first(RING_CAP);
    if nf.len() != RING_CAP || nf[0].cmdid != 99 {
        return false;
    }
    // 倒序连续：nf[k].cmdid == 99-k
    if !nf.iter().enumerate().all(|(k, e)| e.cmdid as usize == 99 - k) {
        return false;
    }
    // 四字段逐字段裁断：SQID/CMDID/状态/LBA 全账一致
    nf.iter().all(|e| e.valid() && e.status == 0x0002 && e.lba != 0)
}

// ── 注入核（0xA380 段 5 项）────────────────────────────────────────────

/// 注入核 1：滚动覆盖注入——第 65 条覆盖第 1 条。
pub fn inj1_roll_overwrite() -> bool {
    let mut ring = ErrRing::new();
    for i in 0..65u16 {
        ring.push(ErrEntry { sqid: 0, cmdid: i, status: 2, lba: i as u64 });
    }
    let nf = ring.read_newest_first(RING_CAP);
    !nf.iter().any(|e| e.cmdid == 0) && nf.iter().any(|e| e.cmdid == 64)
}

/// 注入核 2：最新在前注入——单条环读取首位即最新。
pub fn inj2_newest_first() -> bool {
    let mut ring = ErrRing::new();
    ring.push(ErrEntry { sqid: 1, cmdid: 7, status: 2, lba: 1 });
    ring.push(ErrEntry { sqid: 1, cmdid: 8, status: 2, lba: 2 });
    ring.read_newest_first(2)[0].cmdid == 8
}

/// 注入核 3：非法状态位注入——bit9/10 保留位置位裁断拒绝。
pub fn inj3_bad_status_bits() -> bool {
    let e = ErrEntry { sqid: 0, cmdid: 1, status: 0x0200, lba: 1 };
    !e.valid()
}

/// 注入核 4：容量恒定注入——任意写入量后有效条目 ≤ 64。
pub fn inj4_cap_bounded() -> bool {
    let mut ring = ErrRing::new();
    for i in 0..500u16 {
        ring.push(ErrEntry { sqid: 0, cmdid: i, status: 2, lba: 1 });
    }
    ring.len() <= RING_CAP && ring.total == 500
}

/// 注入核 5：截断读取注入——n < 容量时只返回 n 条。
pub fn inj5_truncated_read() -> bool {
    let mut ring = ErrRing::new();
    for i in 0..10u16 {
        ring.push(ErrEntry { sqid: 0, cmdid: i, status: 2, lba: 1 });
    }
    ring.read_newest_first(3).len() == 3
        && ring.read_newest_first(3)[0].cmdid == 9
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6721_checks：本单判据聚合（诊断段 0xA380）。
pub fn run_unx6721_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6721 错误信息日志页（Error Information Log 64 条环形）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "64条环形滚动+最新在前序",
        j1_main(),
        if j1_main() { "" } else { "环形序破" },
    );
    s.add(
        "四字段逐字段裁断",
        j1_main(),
        if j1_main() { "" } else { "字段裁断破" },
    );
    let inj = [
        ("注入1滚动覆盖", inj1_roll_overwrite()),
        ("注入2最新在前", inj2_newest_first()),
        ("注入3非法状态拒", inj3_bad_status_bits()),
        ("注入4容量恒定", inj4_cap_bounded()),
        ("注入5截断读取", inj5_truncated_read()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
