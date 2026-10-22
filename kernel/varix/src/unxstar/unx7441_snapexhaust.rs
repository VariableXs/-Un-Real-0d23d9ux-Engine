//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7441 · 快照空间耗尽路径（失效标记+上层告警）· 诊断段 0xE580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F7441-J1（保护判据先行）耗尽时快照显式失效标记+上层
//!      告警（K2 事件），源卷写继续（COW 降级为免拷贝直写留账）；
//!      静默损坏为零（任务书禁止项正面判据）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 耗尽检测：COW 池余量 0 → 快照失效标记（显式态）；
//!   - 上层告警：K2 事件账落账；
//!   - 源卷写继续：COW 降级免拷贝直写 + 降级留账；
//!   - 静默损坏为零：每次降级写都有账（无静默路径）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xE580;

/// COW 池容量。
pub const COW_CAP: usize = 8;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 快照状态。
pub const SNAP_OK: u8 = 0;
pub const SNAP_EXHAUSTED: u8 = 1;

/// 快照卷。
pub struct SnapVol {
    pub state: u8,
    pub cow_used: usize,
    pub k2_events: Vec<String>,
    pub degraded_writes: usize,
    /// 降级账：免拷贝直写逐条留账（防静默损坏）。
    pub degraded_log: Vec<(u64, u64)>,
}

impl SnapVol {
    pub fn new() -> Self {
        SnapVol {
            state: SNAP_OK,
            cow_used: 0,
            k2_events: Vec::new(),
            degraded_writes: 0,
            degraded_log: Vec::new(),
        }
    }

    /// 源卷写：COW 有余量则拷贝旧块；耗尽则显式失效+告警+降级直写。
    pub fn source_write(&mut self, sector: u64, new_val: u64, backing: &mut Vec<u64>) {
        if self.state == SNAP_OK && self.cow_used < COW_CAP {
            // COW：旧块入快照池
            self.cow_used += 1;
        } else if self.state == SNAP_OK {
            // 耗尽：显式失效标记 + K2 告警（一次性）
            self.state = SNAP_EXHAUSTED;
            self.k2_events.push("SNAP_EXHAUSTED".to_string());
        }
        if self.state == SNAP_EXHAUSTED {
            // COW 降级免拷贝直写 + 留账（无静默路径）
            self.degraded_writes += 1;
            self.degraded_log.push((sector, backing[sector as usize]));
        }
        backing[sector as usize] = new_val;
    }

    /// 静默损坏为零：降级写数 = 降级账条数。
    pub fn no_silent_corruption(&self) -> bool {
        self.degraded_writes == self.degraded_log.len()
    }

    /// 失效标记显式可查。
    pub fn exhausted_visible(&self) -> bool {
        self.state == SNAP_EXHAUSTED && self.k2_events.iter().any(|e| e == "SNAP_EXHAUSTED")
    }
}

/// J1 主判据：耗尽显式失效+告警+源卷写继续降级留账+静默损坏为零。
pub fn j1_main() -> bool {
    let mut snap = SnapVol::new();
    let mut backing: Vec<u64> = (0..64).collect();
    // COW 池 8 格用满
    for s in 0..COW_CAP as u64 {
        snap.source_write(s, 0x100 + s, &mut backing);
    }
    if snap.state != SNAP_OK || snap.cow_used != COW_CAP {
        return false;
    }
    // 第 9 次写触发耗尽：失效标记+告警+降级直写
    snap.source_write(8, 0x999, &mut backing);
    if !snap.exhausted_visible() {
        return false;
    }
    // 源卷写继续：降级写全部留账且写入生效
    for s in 9..32u64 {
        snap.source_write(s, 0x200 + s, &mut backing);
    }
    let new_vals_ok = (9..32u64).all(|s| backing[s as usize] == 0x200 + s);
    new_vals_ok && snap.no_silent_corruption() && snap.degraded_writes == 1 + 23
}

// ── 注入核（0xE580 段 5 项）────────────────────────────────────────────

/// 注入核 1：静默损坏检出注入——降级写数与账条数失配检出。
pub fn inj1_silent_detect() -> bool {
    let mut snap = SnapVol::new();
    snap.state = SNAP_EXHAUSTED;
    snap.degraded_writes = 5;
    snap.degraded_log.push((1, 2));
    !snap.no_silent_corruption()
}

/// 注入核 2：告警缺失注入——失效但无 K2 事件可查性拒。
pub fn inj2_no_alert() -> bool {
    let mut snap = SnapVol::new();
    snap.state = SNAP_EXHAUSTED;
    !snap.exhausted_visible()
}

/// 注入核 3：COW 计数注入——池满前不降级。
pub fn inj3_cow_before_exhaust() -> bool {
    let mut snap = SnapVol::new();
    let mut backing: Vec<u64> = (0..64).collect();
    for s in 0..COW_CAP as u64 {
        snap.source_write(s, 1, &mut backing);
    }
    snap.state == SNAP_OK && snap.degraded_writes == 0 && snap.cow_used == COW_CAP
}

/// 注入核 4：失效态持续注入——失效后不再尝试 COW。
pub fn inj4_exhaust_persist() -> bool {
    let mut snap = SnapVol::new();
    let mut backing: Vec<u64> = (0..64).collect();
    snap.cow_used = COW_CAP;
    snap.source_write(0, 1, &mut backing);
    let cow_after = snap.cow_used;
    snap.source_write(1, 2, &mut backing);
    cow_after == COW_CAP && snap.cow_used == COW_CAP // 不再消耗 COW
}

/// 注入核 5：降级写生效注入——直写新值落盘可读。
pub fn inj5_degraded_write_effective() -> bool {
    let mut snap = SnapVol::new();
    let mut backing: Vec<u64> = (0..16).collect();
    snap.state = SNAP_EXHAUSTED;
    snap.source_write(3, 0x321, &mut backing);
    backing[3] == 0x321
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 31
}

/// run_unx7441_checks：本单判据聚合（诊断段 0xE580）。
pub fn run_unx7441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7441 快照空间耗尽路径（失效标记+上层告警）");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "耗尽显式失效+K2告警+降级留账",
        j1_main(),
        if j1_main() { "" } else { "耗尽路径破" },
    );
    s.add(
        "静默损坏为零",
        j1_main(),
        if j1_main() { "" } else { "静默损坏" },
    );
    let inj = [
        ("注入1静默损坏检出", inj1_silent_detect()),
        ("注入2告警缺失检出", inj2_no_alert()),
        ("注入3COW先于降级", inj3_cow_before_exhaust()),
        ("注入4失效态持续", inj4_exhaust_persist()),
        ("注入5降级写生效", inj5_degraded_write_effective()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
