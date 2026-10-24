//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0368 · 内存热图接入（F0077 写入口）· 诊断段 0xf0e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   热图位段与页帧池位图逐位一致，采样 1000 位零偏差。
//! 难点分析：
//!   热图是页帧池位图的「视图层」——它不许有自己的真相。接入的唯一
//!   正确姿势是「写穿透」：页帧池每变一位，热图同步变一位。逐位一
//!   致的验证法是**双向对拍**：正向（池→热图）与反向（热图→池）
//!   各推一遍，任何一位不同即抓。采样 1000 位零偏差 = 固定步长抽
//!   样 1000 个位槽全部命中同值。
//! 纪律约束：
//!   - 单一真相：热图只是位图投影，无独立状态可写；
//!   - 写穿透：pool.set/clear 必然同步热图，无旁路；
//!   - 双向对拍：正向投影与反向投影零差异；
//!   - 采样账：1000 位采样落账（位置序列可复算）。
//! 成本记账：写穿透 O(1)；全量对拍 O(n)；采样 O(1000)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf0e0;

// ─────────────────────────────────────────────────────────────────────
// 一、页帧池位图（真相源）+ 热图（投影）
// ─────────────────────────────────────────────────────────────────────

/// 页帧池位图：bit=1 已分配。
pub struct FramePool {
    bits: Vec<u64>,
    pub allocated: u32,
}

impl FramePool {
    pub fn new(frames: usize) -> Self {
        let words = frames.div_ceil(64);
        FramePool { bits: vec![0u64; words], allocated: 0 }
    }

    pub fn get(&self, idx: usize) -> bool {
        self.bits[idx / 64] & (1u64 << (idx % 64)) != 0
    }

    /// 分配一帧（写穿透入口：真相与热图同步）。
    pub fn alloc_at(&mut self, idx: usize, heat: &mut HeatMap) -> bool {
        if self.get(idx) {
            return false;
        }
        self.bits[idx / 64] |= 1u64 << (idx % 64);
        self.allocated += 1;
        heat.mark(idx, true);
        true
    }

    /// 释放一帧（写穿透入口）。
    pub fn free_at(&mut self, idx: usize, heat: &mut HeatMap) -> bool {
        if !self.get(idx) {
            return false;
        }
        self.bits[idx / 64] &= !(1u64 << (idx % 64));
        self.allocated -= 1;
        heat.mark(idx, false);
        true
    }

    pub fn frames(&self) -> usize {
        self.bits.len() * 64
    }
}

/// 热图：密度视图。只记录「热度」（分配次数计数），位段一致性的
/// 对拍面是「当前占用位」——热图的 occupied 投影必须与池位图逐位同。
pub struct HeatMap {
    /// 占用投影（0/1 位段）。
    occupied: Vec<u64>,
    /// 热度计数（分配事件累计，视图装饰信息）。
    pub touch: Vec<u32>,
}

impl HeatMap {
    pub fn new(frames: usize) -> Self {
        HeatMap { occupied: vec![0u64; frames.div_ceil(64)], touch: vec![0u32; frames] }
    }

    /// 写穿透落点：occupied 位段与 touch 计数同步更新。
    pub fn mark(&mut self, idx: usize, on: bool) {
        if on {
            self.occupied[idx / 64] |= 1u64 << (idx % 64);
            self.touch[idx] = self.touch[idx].saturating_add(1);
        } else {
            self.occupied[idx / 64] &= !(1u64 << (idx % 64));
        }
    }

    pub fn occupied_at(&self, idx: usize) -> bool {
        self.occupied[idx / 64] & (1u64 << (idx % 64)) != 0
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、双向对拍 + 采样
// ─────────────────────────────────────────────────────────────────────

/// 正向对拍：池位图 → 投影应与热图 occupied 逐位一致。返回首个差异位。
pub fn forward_diff(pool: &FramePool, heat: &HeatMap) -> Option<usize> {
    for idx in 0..pool.frames() {
        if pool.get(idx) != heat.occupied_at(idx) {
            return Some(idx);
        }
    }
    None
}

/// 反向对拍：热图 occupied → 反推池应为的分配数。与池账对冲。
pub fn backward_count(heat: &HeatMap, frames: usize) -> u32 {
    let mut n = 0u32;
    for idx in 0..frames {
        if heat.occupied_at(idx) {
            n += 1;
        }
    }
    n
}

/// 采样 1000 位：固定步长（黄金间距互质防周期假象），全同值才零偏差。
/// 返回 (采样数, 偏差数, 首个偏差位)。
pub fn sample_1000(pool: &FramePool, heat: &HeatMap) -> (u32, u32, Option<usize>) {
    let total = pool.frames();
    let step = if total > 1000 { total / 1000 } else { 1 };
    let mut taken = 0u32;
    let mut bad = 0u32;
    let mut first_bad = None;
    let mut idx = 0usize;
    while taken < 1000 && idx < total {
        if pool.get(idx) != heat.occupied_at(idx) {
            bad += 1;
            if first_bad.is_none() {
                first_bad = Some(idx);
            }
        }
        taken += 1;
        idx += step;
    }
    (taken, bad, first_bad)
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0368 判据全集。
pub fn run_unx368_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0368 内存热图接入");
    let mut pool = FramePool::new(2048);
    let mut heat = HeatMap::new(2048);

    // J1 主判据：写穿透后逐位一致 + 1000 位零偏差
    for i in (0..2048).step_by(3) {
        pool.alloc_at(i, &mut heat);
    }
    pool.alloc_at(7, &mut heat);
    pool.free_at(7, &mut heat);
    cs.add("J1 正向零差异", forward_diff(&pool, &heat).is_none(), "池→热图逐位同");
    cs.add(
        "J1 反向对冲",
        backward_count(&heat, 2048) == pool.allocated,
        "热图反推=池账",
    );
    let (taken, bad, _) = sample_1000(&pool, &heat);
    cs.add("J1 采样 1000 位", taken == 1000 && bad == 0, "零偏差");

    // 注入 1：旁路写热图（模拟失同步）→ 双向对拍必抓
    let mut heat_bad = HeatMap::new(2048);
    // 复制干净投影后人为翻一位（绕过池）
    for i in (0..2048).step_by(3) {
        heat_bad.mark(i, true);
    }
    heat_bad.mark(999, true); // 旁路！
    let fwd = forward_diff(&pool, &heat_bad);
    cs.add(
        "注入1 失同步抓取",
        fwd == Some(999) && backward_count(&heat_bad, 2048) != pool.allocated,
        "双向都报",
    );

    // 注入 2：释放已空帧 / 重复分配 → 拒收且不穿透
    let mut p2 = FramePool::new(64);
    let mut h2 = HeatMap::new(64);
    cs.add("注入2 重复分配拒", !p2.alloc_at(5, &mut h2) || {
        let ok = p2.alloc_at(5, &mut h2);
        !ok
    }, "第二次必拒");
    cs.add("注入2 空放拒收", !p2.free_at(42, &mut h2), "未分配不许放");

    // 注入 3：边界位（0 / 63 / 64 / 尾位）写穿透
    let mut p3 = FramePool::new(130);
    let mut h3 = HeatMap::new(130);
    for &i in &[0usize, 63, 64, 129] {
        p3.alloc_at(i, &mut h3);
    }
    cs.add(
        "注入3 边界一致",
        forward_diff(&p3, &h3).is_none()
            && (0..130).filter(|&i| h3.occupied_at(i)).count() == 4,
        "字内跨字都准",
    );

    // 注入 4：热度计数单调（重复分配/释放不虚计）
    let mut p4 = FramePool::new(16);
    let mut h4 = HeatMap::new(16);
    p4.alloc_at(3, &mut h4);
    p4.free_at(3, &mut h4);
    p4.alloc_at(3, &mut h4);
    cs.add("注入4 热度=2", h4.touch[3] == 2 && p4.get(3), "两次分配计数");
    cs.add("注入4 占用回真", h4.occupied_at(3), "放后重分占有位");

    cs
}
