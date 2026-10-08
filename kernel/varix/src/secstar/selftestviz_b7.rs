//! F172 自检可视化 · 批次七深化（v7）——依赖图校验、ETA 估计器、
//! 跨次运行差分、结果导出帧。零堆、no_std。

use crate::checks::CheckSet;

/// 套件数（与主册一致）。
pub const SUITES: usize = 4;
/// 总项数。
pub const TOTAL_ITEMS: usize = 36;
/// 依赖图最大边数。
pub const DEP_EDGE_CAP: usize = 24;
/// ETA 滑窗样本数。
pub const ETA_SAMPLES: usize = 8;
/// 导出帧长（12B）。
pub const EXPORT_FRAME_LEN: usize = 12;

/// 依赖边：项 i 依赖项 j（i 必须在 j 之后跑）。
#[derive(Clone, Copy, PartialEq)]
pub struct DepEdge {
    pub after: u8,
    pub before: u8,
}

/// 依赖图校验器：无环（拓扑可排）、边合法（两端都在 0..36）、无重复。
#[derive(Clone, Copy)]
pub struct DepGraph {
    edges: [DepEdge; DEP_EDGE_CAP],
    pub n: usize,
}

impl DepGraph {
    pub const fn new() -> DepGraph {
        DepGraph { edges: [DepEdge { after: 0, before: 0 }; DEP_EDGE_CAP], n: 0 }
    }

    /// 加边：越界/自环/重复全拒（构建期守住——不是跑到一半才炸）。
    pub fn add_edge(&mut self, after: u8, before: u8) -> bool {
        if after >= TOTAL_ITEMS as u8 || before >= TOTAL_ITEMS as u8 {
            return false;
        }
        if after == before {
            return false; // 自环 = 永远排不出
        }
        for i in 0..self.n {
            if self.edges[i] == (DepEdge { after, before }) {
                return false;
            }
        }
        if self.n >= DEP_EDGE_CAP {
            return false;
        }
        self.edges[self.n] = DepEdge { after, before };
        self.n += 1;
        true
    }

    /// 环检测：DFS 三色标记（0 白 1 灰 2 黑），灰遇灰 = 环。
    pub fn has_cycle(&self) -> bool {
        let mut color = [0u8; TOTAL_ITEMS];
        // 迭代 DFS——用显式栈（零堆、定深）。
        let mut stack = [0u8; TOTAL_ITEMS];
        for start in 0..TOTAL_ITEMS as u8 {
            if color[start as usize] != 0 {
                continue;
            }
            let mut sp = 0usize;
            stack[sp] = start;
            color[start as usize] = 1;
            while sp != usize::MAX {
                let cur = stack[sp];
                let mut pushed = false;
                for e in &self.edges[..self.n] {
                    if e.after == cur {
                        match color[e.before as usize] {
                            0 => {
                                sp += 1;
                                stack[sp] = e.before;
                                color[e.before as usize] = 1;
                                pushed = true;
                                break;
                            }
                            1 => return true, // 灰遇灰 = 环
                            _ => {}
                        }
                    }
                }
                if !pushed {
                    color[cur as usize] = 2;
                    if sp == 0 {
                        sp = usize::MAX;
                    } else {
                        sp -= 1;
                    }
                }
            }
        }
        false
    }

    /// 拓扑序可行性 = 无环（有环则调度必死锁）。
    pub fn schedulable(&self) -> bool {
        !self.has_cycle()
    }
}

/// ETA 估计器：按已完成项的滑动均耗时 × 剩余项 = 预计剩余。
/// 样本不足给保守上界（总预算——宁可多报不白等）。
#[derive(Clone, Copy)]
pub struct EtaEstimator {
    per_item_ms: [u32; ETA_SAMPLES],
    pub n: usize,
    pub total_budget_ms: u32,
    done: usize,
}

impl EtaEstimator {
    pub const fn new(total_budget_ms: u32) -> EtaEstimator {
        EtaEstimator { per_item_ms: [0; ETA_SAMPLES], n: 0, total_budget_ms, done: 0 }
    }

    /// 记一项耗时（ms）。
    pub fn record_item(&mut self, ms: u32) {
        self.per_item_ms[self.n % ETA_SAMPLES] = ms;
        self.n += 1;
        self.done += 1;
    }

    /// 滑动均耗时。
    pub fn mean_ms(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let k = self.n.min(ETA_SAMPLES);
        let mut sum = 0u64;
        for i in 0..k {
            sum += self.per_item_ms[i] as u64;
        }
        Some((sum / k as u64) as u32)
    }

    /// ETA：剩余项 × 均耗时；样本 <3 → 保守上界 = 总预算（不编造精确）。
    /// 剩余 = TOTAL_ITEMS - done。
    pub fn eta_ms(&self) -> u32 {
        let remaining = TOTAL_ITEMS.saturating_sub(self.done) as u64;
        if self.n < 3 {
            return self.total_budget_ms;
        }
        let mean = self.mean_ms().unwrap_or(0) as u64;
        ((remaining * mean) as u32).min(self.total_budget_ms)
    }

    pub fn items_done(&self) -> usize {
        self.done
    }
}

/// 跨次运行差分：两次全量结果的逐项对比。
/// 结果编码：u32 位图（bit i = 项 i 过）。36 项 → 2 个 u32（前 32 + 后 4）。
#[derive(Clone, Copy, PartialEq)]
pub struct RunBitmap {
    pub lo: u32,
    pub hi: u32,
}

impl RunBitmap {
    pub const fn new() -> RunBitmap {
        RunBitmap { lo: 0, hi: 0 }
    }

    pub fn set(&mut self, item: usize, passed: bool) {
        if item >= TOTAL_ITEMS {
            return;
        }
        if item < 32 {
            if passed {
                self.lo |= 1 << item;
            }
        } else if passed {
            self.hi |= 1 << (item - 32);
        }
    }

    pub fn get(&self, item: usize) -> Option<bool> {
        if item >= TOTAL_ITEMS {
            return None;
        }
        Some(if item < 32 { self.lo & (1 << item) != 0 } else { self.hi & (1 << (item - 32)) != 0 })
    }

    pub fn passed_count(&self) -> u32 {
        (self.lo.count_ones() + self.hi.count_ones()).min(TOTAL_ITEMS as u32)
    }

    /// 差分：本次相对上次 新失败/新恢复 两张位图。
    pub fn diff(&self, prev: &RunBitmap) -> (RunBitmap, RunBitmap) {
        let newly_failed = RunBitmap {
            lo: prev.lo & !self.lo,
            hi: prev.hi & !self.hi,
        };
        let newly_passed = RunBitmap {
            lo: !prev.lo & self.lo,
            hi: !prev.hi & self.hi,
        };
        (newly_failed, newly_passed)
    }
}

/// 结果导出帧（12B）：
/// [0..2) 魔数 "ST" · [2..4) 通过数 LE · [4..6) 失败数 LE · [6..8) 总项 LE ·
/// [8..10) 均耗时 ms LE · [10..12) 校验和（前 10B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_export(passed: u16, failed: u16, total: u16, mean_ms: u16, out: &mut [u8; EXPORT_FRAME_LEN]) -> bool {
    if passed + failed != total {
        return false; // 通过+失败 ≠ 总项 = 账不平（有项既没过也没败？拒绝）
    }
    out[0] = b'S';
    out[1] = b'T';
    out[2..4].copy_from_slice(&passed.to_le_bytes());
    out[4..6].copy_from_slice(&failed.to_le_bytes());
    out[6..8].copy_from_slice(&total.to_le_bytes());
    out[8..10].copy_from_slice(&mean_ms.to_le_bytes());
    let c = fnv16(&out[..10]);
    out[10] = (c & 0xFF) as u8;
    out[11] = (c >> 8) as u8;
    true
}

pub fn decode_export(frame: &[u8; EXPORT_FRAME_LEN]) -> Option<(u16, u16, u16, u16)> {
    if frame[0] != b'S' || frame[1] != b'T' {
        return None;
    }
    let want = (frame[11] as u16) << 8 | frame[10] as u16;
    if fnv16(&frame[..10]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?),
    ))
}

#[inline(never)]
pub fn run_selftestviz_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b7");

    // 1) 依赖图合法边：双向不重复、自环拒、越界拒。
    let mut g = DepGraph::new();
    let ok = g.add_edge(3, 1);
    let dup = g.add_edge(3, 1);
    let self_loop = g.add_edge(5, 5);
    let oob = g.add_edge(200, 1);
    cs.add("dep_legit_edges", ok && !dup && !self_loop && !oob && g.n == 1, "");

    // 2) 无环可行：链 1←2←3（1 依赖 2、2 依赖 3）→ 可调度。
    let mut g2 = DepGraph::new();
    g2.add_edge(2, 1);
    g2.add_edge(3, 2);
    cs.add("dep_chain_schedulable", g2.schedulable(), "");

    // 3) 环检出：1←2←1 成环 → 不可调度（死锁在图上先死，不在运行时死）。
    let mut g3 = DepGraph::new();
    g3.add_edge(2, 1);
    g3.add_edge(1, 2);
    cs.add("dep_cycle_detected", !g3.schedulable(), "");

    // 4) 自环即环：add 已拒所以建不出——验证「拒收即防线」的一致口径。
    let mut g4 = DepGraph::new();
    cs.add("dep_selfloop_blocked_at_door", !g4.add_edge(7, 7) && g4.schedulable(), "");

    // 5) ETA：样本 <3 → 总预算（保守上界不编造精确值）。
    let mut eta = EtaEstimator::new(2_000);
    eta.record_item(50);
    cs.add("eta_conservative_start", eta.eta_ms() == 2_000, "");

    // 6) ETA 稳态：3 样本各 50ms、剩 33 项 → 1650ms（均耗时×剩余）。
    eta.record_item(50);
    eta.record_item(50);
    cs.add("eta_steady_state", eta.eta_ms() == 1_650, "");

    // 7) ETA 钳制：均耗时 1000ms × 剩余 30 = 30000 > 预算 → 钳到预算（不吓人）。
    let mut eta2 = EtaEstimator::new(2_000);
    for _ in 0..5 {
        eta2.record_item(1_000);
    }
    cs.add("eta_clamped_to_budget", eta2.eta_ms() == 2_000, "");

    // 8) 位图：set/get 往返、越界静默忽略、passed_count 恰 36 上限。
    let mut bm = RunBitmap::new();
    bm.set(0, true);
    bm.set(31, true);
    bm.set(32, true);
    bm.set(35, true);
    bm.set(36, true); // 越界忽略
    cs.add(
        "bitmap_roundtrip",
        bm.get(0) == Some(true) && bm.get(31) == Some(true) && bm.get(32) == Some(true) && bm.get(35) == Some(true) && bm.get(36).is_none() && bm.passed_count() == 4,
        "",
    );

    // 9) 差分两向：新失败 = 上过这次没过；新恢复 = 上没过这次过。
    let mut prev = RunBitmap::new();
    prev.set(1, true);
    prev.set(2, true);
    let mut cur = RunBitmap::new();
    cur.set(2, true);
    cur.set(3, true);
    let (nf, np) = cur.diff(&prev);
    cs.add(
        "diff_two_directions",
        nf.get(1) == Some(true) && nf.get(2) == Some(false) && np.get(3) == Some(true) && np.get(2) == Some(false),
        "",
    );

    // 10) 导出帧 round-trip + 账平校验：passed+failed≠total 拒编码。
    let mut f = [0u8; EXPORT_FRAME_LEN];
    let ok = encode_export(33, 3, 36, 42, &mut f);
    let bad = encode_export(33, 4, 36, 42, &mut f);
    cs.add(
        "export_frame_accounted",
        ok && decode_export(&f) == Some((33, 3, 36, 42)) && !bad && f[0] == b'S',
        "",
    );

    // 11) 导出帧撕裂拒：单字节翻转全拦（12 字节逐一）。
    let mut tear_ok = true;
    for i in 0..EXPORT_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0xA5;
        if decode_export(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("export_frame_tear_proof", tear_ok, "");

    // 12) 常量自洽：36 项、4 套件、帧 12B（与主册名册对齐）。
    cs.add(
        "b7_constants",
        TOTAL_ITEMS == 36 && SUITES == 4 && EXPORT_FRAME_LEN == 12,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_longer_cycle_detected() {
        // 三项环：1←2←3←1 → 拒调度（长环比双环更隐蔽，必须同检）。
        let mut g = DepGraph::new();
        g.add_edge(2, 1);
        g.add_edge(3, 2);
        g.add_edge(1, 3);
        assert!(!g.schedulable());
    }

    #[test]
    fn eta_full_run_ends_zero() {
        // 全部 36 项跑完 → ETA 0（不悬挂在最后一项）。
        let mut eta = EtaEstimator::new(10_000);
        for _ in 0..TOTAL_ITEMS {
            eta.record_item(10);
        }
        assert_eq!(eta.items_done(), TOTAL_ITEMS);
        assert_eq!(eta.eta_ms(), 0);
    }

    #[test]
    fn diff_no_change_is_empty() {
        // 两次结果全同 → 差分两向全空（零差异是合法状态）。
        let mut a = RunBitmap::new();
        for i in (0..TOTAL_ITEMS).step_by(2) {
            a.set(i, true);
        }
        let (nf, np) = a.diff(&a);
        assert_eq!(nf.passed_count(), 0);
        assert_eq!(np.passed_count(), 0);
    }
}
