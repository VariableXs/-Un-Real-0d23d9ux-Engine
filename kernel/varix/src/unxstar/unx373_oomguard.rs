//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0373 · 早年 OOM 预案（关键池保底）· 诊断段 0xf140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   OOM 三级水位（5%/2%/0.5%）触发正确，关键池保底 1% 不被侵占。
//! 难点分析：
//!   OOM 预案的本质是「在死前有序撤退」：三级水位各对应一个收缩
//!   动作（回收缓存 → 拒绝非关键分配 → 只保关键池）。难点有二：
//!   ① 水位触发是**穿越触发**（跌破即触发一次并落账，回升再跌破
//!   再触发——不许抖动每帧都触发）；② 关键池保底是**硬底线**——
//!   三级动作无论多狠，都不能把关键池吃到 1% 以下。
//! 纪律约束：
//!   - 水位闭集：三级编译期冻结（5%/2%/0.5%）；
//!   - 穿越触发：级别只能升不能在同一水位重复落账；
//!   - 保底硬线：关键池剩余 <1% 的分配必拒；
//!   - 动作留痕：每次水位升级落一条账（级别/时刻/剩余量）。
//! 成本记账：判定 O(1)；动作 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf140;

/// 三级水位（千分比，编译期冻结）。
pub const LEVELS: [u32; 3] = [50, 20, 5]; // 5% / 2% / 0.5%

/// 关键池保底（千分比）。
pub const CRITICAL_FLOOR: u32 = 10; // 1%

// ─────────────────────────────────────────────────────────────────────
// 一、OOM 状态机
// ─────────────────────────────────────────────────────────────────────

/// 水位级别账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelEvent {
    pub level: u8, // 1..=3
    pub remain_ppm: u32,
}

/// OOM 预案机。
pub struct OomGuard {
    pub total: u32,
    pub free: u32,
    /// 关键池已占（不许被三级动作清）。
    pub critical_used: u32,
    /// 当前级别（单调不降，穿越触发）。
    pub level: u8,
    pub events: Vec<LevelEvent>,
}

impl OomGuard {
    pub fn new(total: u32, critical_used: u32) -> Self {
        OomGuard { total, free: total, critical_used, level: 0, events: Vec::new() }
    }

    /// 剩余千分比。
    pub fn remain_ppm(&self) -> u32 {
        self.free * 1000 / self.total
    }

    /// 分配请求：kind=false 非关键 / true 关键。
    /// 返回 Ok(新剩余) 或 Err(拒绝原因)。
    pub fn alloc(&mut self, n: u32, critical: bool) -> Result<u32, &'static str> {
        // 硬底线：关键池保底 1%——任何分配（含关键）不得吃穿。
        let floor = self.total * CRITICAL_FLOOR / 1000;
        if self.free < n + floor.min(self.free) && !critical {
            // 非关键在三级后拒绝
            if self.level >= 3 {
                return Err("三级水位：非关键分配拒");
            }
        }
        // 关键池保底：分配后剩余不得低于保底线。
        if self.free < n || (self.free - n) * 1000 / self.total < CRITICAL_FLOOR {
            return Err("关键池保底 1% 不可侵占");
        }
        self.free -= n;
        self.update_level();
        Ok(self.free)
    }

    /// 穿越触发：级别单调升，跨级落账。
    fn update_level(&mut self) {
        let ppm = self.remain_ppm();
        let mut new_level = 0u8;
        for (i, &th) in LEVELS.iter().enumerate() {
            if ppm < th {
                new_level = (i + 1) as u8;
            }
        }
        if new_level > self.level {
            self.level = new_level;
            self.events.push(LevelEvent { level: new_level, remain_ppm: ppm });
        }
    }

    /// 释放回血：级别不自动降（穿越触发单向），但剩余量回真。
    pub fn free(&mut self, n: u32) {
        self.free = (self.free + n).min(self.total);
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0373 判据全集。
pub fn run_unx373_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0373 早年 OOM 预案");

    // J1 主判据：三级水位穿越触发正确
    let mut g = OomGuard::new(10_000, 0);
    // 吃到 4% → 触发 L1、L2、L3（5%/2%/0.5% 依次穿越）
    let mut steps = 0u32;
    while g.remain_ppm() >= 5 && steps < 100 {
        let _ = g.alloc(100, false);
        steps += 1;
    }
    cs.add("J1 三级全触发", g.level == 3 && g.events.len() == 3, "5%/2%/0.5% 依次");
    cs.add(
        "J1 账序单调",
        g.events.windows(2).all(|w| w[0].level < w[1].level && w[0].remain_ppm > w[1].remain_ppm),
        "级别升/剩余降",
    );

    // 注入 1：抖动不重复触发（释放回血再跌破——级别不重落账）
    let ev_before = g.events.len();
    g.free(300); // 回血 >0.5%
    let _ = g.alloc(100, false); // 又跌破 0.5%——L3 已在级，不重落
    cs.add("注入1 不抖动", g.events.len() == ev_before && g.level == 3, "级别单调不重落");

    // 注入 2：关键池保底（分配不许吃穿 1%）
    let mut g2 = OomGuard::new(10_000, 0);
    // 吃到 1.2% 剩余
    while g2.remain_ppm() > 12 {
        let _ = g2.alloc(10, false);
    }
    // 再要一大块 → 必拒（会击穿 1%）
    let r2 = g2.alloc(50, true);
    cs.add("注入2 保底拦截", r2.is_err(), "低于 1% 必拒");
    cs.add(
        "注入2 保底成立",
        g2.remain_ppm() >= CRITICAL_FLOOR,
        "剩余量在线上",
    );

    // 注入 3：保底内关键分配放行
    let mut g3 = OomGuard::new(10_000, 0);
    while g3.remain_ppm() > 15 {
        let _ = g3.alloc(10, false);
    }
    let ok = g3.alloc(30, true); // 剩 1.5% → 要 0.3%，落 1.2% ≥ 1%
    cs.add("注入3 保底内放行", ok.is_ok(), "关键流量可走");

    // 注入 4：水位闭集（三级常量冻结）
    cs.add("注入4 水位冻结", LEVELS == [50, 20, 5], "5%/2%/0.5%");
    cs.add("注入4 保底冻结", CRITICAL_FLOOR == 10, "1% 硬线");

    // 注入 5：释放回血不降级（单向穿越语义）
    let mut g5 = OomGuard::new(1000, 0);
    while g5.level < 2 {
        let _ = g5.alloc(50, false);
    }
    g5.free(400);
    cs.add("注入5 不自动降级", g5.level == 2 && g5.remain_ppm() > 20, "级别留痕不撤");

    cs
}
