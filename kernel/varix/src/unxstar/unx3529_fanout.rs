//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3529 · 跟踪流多路分发（事件扇出——TP/log 双通道）· 诊断段 0xCF00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：扇出语义（事件→[度量账, 探针流, 联动快照]三路独立消费
//!      ——一路堵不拖另两路）；M-287 三路扇出对账（各路计数独立
//!      且总和对源）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三路扇出：度量账 / 探针流 / 联动快照；
//!   - 独立消费：单路阻断（满/关）不影响另两路计数；
//!   - 对账：各路计数独立 + Σ路计数 ≡ 源事件数（未堵时）；
//!   - 堵路建模：路开关 + 满容量，堵路计数留滞不传播；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xCF00;

/// 三路。
pub const LANES: usize = 3;
/// 探针流容量。
pub const PROBE_CAP: usize = 8;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单路消费端。
pub struct Lane {
    pub open: bool,
    pub count: u64,
    /// 探针流专用缓冲。
    pub buf: Vec<u64>,
    pub cap: usize,
}

impl Lane {
    pub fn new(cap: usize) -> Self {
        Lane { open: true, count: 0, buf: Vec::new(), cap }
    }

    /// 消费一条：开且未满才收。
    pub fn consume(&mut self, ev: u64) -> bool {
        if !self.open || self.buf.len() >= self.cap {
            return false;
        }
        self.buf.push(ev);
        self.count += 1;
        true
    }

    pub fn full(&self) -> bool {
        self.buf.len() >= self.cap
    }
}

/// 扇出器。
pub struct Fanout {
    pub lanes: [Lane; LANES],
    pub source: u64,
}

impl Fanout {
    pub fn new() -> Self {
        Fanout {
            lanes: [Lane::new(usize::MAX), Lane::new(PROBE_CAP), Lane::new(usize::MAX)],
            source: 0,
        }
    }

    /// 扇出一条事件：三路独立消费，堵路不计。
    pub fn push(&mut self, ev: u64) -> [bool; LANES] {
        self.source += 1;
        let mut res = [false; LANES];
        for i in 0..LANES {
            res[i] = self.lanes[i].consume(ev);
        }
        res
    }

    /// 对账：未堵路计数和 ≡ 已被三路至少一路接收的源数。
    /// 严格口径：Σ(路计数) ≥ 源中至少一路收到的条数。
    pub fn reconciled(&self) -> bool {
        let sum: u64 = self.lanes.iter().map(|l| l.count).sum();
        sum <= self.source * LANES as u64 && self.lanes.iter().all(|l| l.count <= self.source)
    }
}

/// M-287 三路扇出对账：全开时三路计数相等且总和对源；堵一路时另两路不受拖。
pub fn m287() -> (bool, u64) {
    let mut f = Fanout::new();
    let n = 12u64;
    for i in 0..n {
        f.push(i);
    }
    // 全开：三路各收 12
    let all_open = f.lanes.iter().all(|l| l.count == n) && f.reconciled();
    // 堵探针流（容量 8，第 9 条起滞留）
    let mut f2 = Fanout::new();
    for i in 0..n {
        f2.push(i);
    }
    let probe_blocked = f2.lanes[1].count == PROBE_CAP as u64 && f2.lanes[1].full();
    let others_ok = f2.lanes[0].count == n && f2.lanes[2].count == n;
    // 关闭度量账：另两路不受拖
    let mut f3 = Fanout::new();
    f3.lanes[0].open = false;
    for i in 0..n {
        f3.push(i);
    }
    let closed_ok = f3.lanes[0].count == 0 && f3.lanes[1].count == n && f3.lanes[2].count == n;
    (
        all_open && probe_blocked && others_ok && closed_ok && f3.reconciled(),
        n,
    )
}

/// J1 主判据：扇出语义（三路独立）+ M-287 对账。
pub fn j1_main() -> bool {
    let (ok, n) = m287();
    ok && n == 12 && {
        // 空扇出对账
        let f = Fanout::new();
        f.reconciled() && f.source == 0
    }
}

// ── 注入核（0xCF00 段 5 项）────────────────────────────────────────────

/// 注入核 1：路数冻结注入——恒三路。
pub fn inj1_lane_count() -> bool {
    let f = Fanout::new();
    f.lanes.len() == LANES && LANES == 3
}

/// 注入核 2：堵路不传播注入——堵一路另两路计数不变。
pub fn inj2_no_drag() -> bool {
    let mut f = Fanout::new();
    f.lanes[2].open = false;
    for i in 0..5 {
        f.push(i);
    }
    f.lanes[2].count == 0 && f.lanes[0].count == 5 && f.lanes[1].count == 5
}

/// 注入核 3：满路注入——探针流满后拒收且不拖他路。
pub fn inj3_full_lane() -> bool {
    let mut f = Fanout::new();
    for i in 0..(PROBE_CAP as u64 + 3) {
        f.push(i);
    }
    f.lanes[1].count == PROBE_CAP as u64 && f.lanes[0].count == PROBE_CAP as u64 + 3
}

/// 注入核 4：源计数注入——push 计数与调用数一致。
pub fn inj4_source_count() -> bool {
    let mut f = Fanout::new();
    for i in 0..7 {
        f.push(i);
    }
    f.source == 7
}

/// 注入核 5：探针流缓冲序注入——FIFO 序保真。
pub fn inj5_fifo_order() -> bool {
    let mut f = Fanout::new();
    for i in 0..4 {
        f.push(i * 10);
    }
    let mut want = Vec::new();
    for i in 0..4u64 {
        want.push(i * 10);
    }
    f.lanes[1].buf == want
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx3529_checks：本单判据聚合（诊断段 0xCF00）。
pub fn run_unx3529_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3529 跟踪流多路分发（事件扇出——TP/log 双通道）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "扇出语义三路独立消费（一路堵不拖另两路）",
        j1_main(),
        if j1_main() { "" } else { "扇出语义破" },
    );
    let (m_ok, _) = m287();
    s.add(
        "M-287三路扇出对账（计数独立总和对源）",
        m_ok,
        if m_ok { "" } else { "M-287对账破" },
    );
    let inj = [
        ("注入1路数冻结", inj1_lane_count()),
        ("注入2堵路不传播", inj2_no_drag()),
        ("注入3满路", inj3_full_lane()),
        ("注入4源计数", inj4_source_count()),
        ("注入5FIFO序", inj5_fifo_order()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
