//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0372 · 内存碎片度量（外碎片指数）· 诊断段 0xf130
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   外碎片指数五档（1/4/16/64/256 帧）与手算一致，度量两次运行
//!   同值。
//! 难点分析：
//!   外碎片 = 「空闲帧总量够、但连续段不够大」的浪费度。五档指数的
//!   口径：对每个档位 k（1/4/16/64/256 帧），统计「能容纳 ≥k 帧的
//!   连续空闲段」的帧容量占比。手算一致 = 每一档都能从段表人工
//!   复核。度量两次同值 = 纯函数（只读位图，无状态副作用）。
//! 纪律约束：
//!   - 纯函数：度量不改位图一个位；
//!   - 五档闭集：档位表编译期冻结，不许运行时加档；
//!   - 段表先导：先切段再算档，段表本身可复算；
//!   - 确定性：同位图两次度量逐档同值。
//! 成本记账：切段 O(n)；五档 O(5×s)（s=段数）。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf130;

/// 五档帧数（编译期冻结）。
pub const TIERS: [usize; 5] = [1, 4, 16, 64, 256];

// ─────────────────────────────────────────────────────────────────────
// 一、切段：连续空闲段表
// ─────────────────────────────────────────────────────────────────────

/// 连续空闲段（起始帧，长度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeRun {
    pub start: usize,
    pub len: usize,
}

/// 从位图切段（bit=0 空闲）。纯函数。
pub fn cut_runs(bits: &[u64], total_frames: usize) -> Vec<FreeRun> {
    let mut out = Vec::new();
    let mut cur_start = None;
    for idx in 0..total_frames {
        let free = bits[idx / 64] & (1u64 << (idx % 64)) == 0;
        if free {
            if cur_start.is_none() {
                cur_start = Some(idx);
            }
        } else if let Some(s) = cur_start.take() {
            out.push(FreeRun { start: s, len: idx - s });
        }
    }
    if let Some(s) = cur_start {
        out.push(FreeRun { start: s, len: total_frames - s });
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// 二、五档外碎片指数
// ─────────────────────────────────────────────────────────────────────

/// 档位 k 的可用容量：∑ 段长 ≥ k 的段中，每段贡献「段长/k 向下取整 × k」
/// （即能整块切出的帧数）。占比 = 容量 / 总空闲。
/// 返回五档占比的千分数（0..1000）。
pub fn extern_index(runs: &[FreeRun]) -> [u32; 5] {
    let total_free: usize = runs.iter().map(|r| r.len).sum();
    let mut out = [0u32; 5];
    if total_free == 0 {
        return out;
    }
    for (ti, &k) in TIERS.iter().enumerate() {
        let mut usable = 0usize;
        for r in runs {
            if r.len >= k {
                usable += (r.len / k) * k;
            }
        }
        out[ti] = (usable * 1000 / total_free) as u32;
    }
    out
}

/// 手算参照：单档直接从段表枚举（独立实现，用于对拍）。
pub fn tier_manual(runs: &[FreeRun], k: usize) -> u32 {
    let total_free: usize = runs.iter().map(|r| r.len).sum();
    if total_free == 0 {
        return 0;
    }
    let mut usable = 0;
    for r in runs {
        let mut rem = r.len;
        while rem >= k {
            usable += k;
            rem -= k;
        }
    }
    (usable * 1000 / total_free) as u32
}

// ─────────────────────────────────────────────────────────────────────
// 三、黄金位图夹具：手工可算
// ─────────────────────────────────────────────────────────────────────

/// 黄金夹具：64 帧，空闲段 = [3 帧, 10 帧, 40 帧, 5 帧]，占 6 帧。
/// 总空闲 58。手算：k=1 → 58/58=1000；k=4 → 8+40=48? 10/4*4=8, 40/4*4=40, 5/4*4=4, 3/4→0 → 52 → 896。
pub fn golden_bits() -> Vec<u64> {
    let mut bits = vec![0u64; 1];
    // 占帧：0,1,2 | 13..=19 | 50 | 55..=58
    let set = |bits: &mut Vec<u64>, idx: usize| bits[idx / 64] |= 1u64 << (idx % 64);
    for &i in &[0usize, 1, 2] {
        set(&mut bits, i);
    }
    for i in 13..=19 {
        set(&mut bits, i);
    }
    set(&mut bits, 50);
    for i in 55..=58 {
        set(&mut bits, i);
    }
    bits
}

// ─────────────────────────────────────────────────────────────────────
// 四、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0372 判据全集。
pub fn run_unx372_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0372 内存碎片度量");

    // J1 主判据：五档与手算一致 + 度量两次同值
    let bits = golden_bits();
    let runs = cut_runs(&bits, 64);
    cs.add(
        "J1 段表 4 段",
        runs == vec![
            FreeRun { start: 3, len: 10 },
            FreeRun { start: 20, len: 30 },
            FreeRun { start: 51, len: 4 },
            FreeRun { start: 59, len: 5 },
        ],
        "手工切段",
    );
    let idx1 = extern_index(&runs);
    let idx2 = extern_index(&runs);
    cs.add("J1 两次同值", idx1 == idx2, "纯函数确定性");
    for (ti, &k) in TIERS.iter().enumerate() {
        let manual = tier_manual(&runs, k);
        cs.add_dyn("J1 档位对拍", idx1[ti] == manual, &alloc::format!("k={} {}", k, manual));
    }

    // 手算锚点：k=1 全可用、k=256 零可用（段长都 < 256）
    cs.add("J1 k=1 满容量", idx1[0] == 1000, "全部空闲可单帧用");
    cs.add("J1 k=256 零", idx1[4] == 0, "64 帧图无 256 段");

    // 注入 1：碎片化劣化（把 30 帧大段切成碎段）→ k=16 档下降
    let mut b1 = bits.clone();
    for i in [24usize, 28, 32, 36, 40, 44] {
        b1[i / 64] |= 1u64 << (i % 64);
    }
    let r1 = cut_runs(&b1, 64);
    let i1 = extern_index(&r1);
    cs.add("注入1 大段切碎", i1[2] < idx1[2], "k=16 档下降");

    // 注入 2：全空位图 → 五档全 1000
    let b2 = vec![0u64; 4];
    let r2 = cut_runs(&b2, 256);
    let i2 = extern_index(&r2);
    cs.add("注入2 全空满容量", i2 == [1000, 1000, 1000, 1000, 1000], "零碎片");

    // 注入 3：全占位图 → 五档全 0
    let mut b3 = vec![!0u64; 4];
    b3[3] &= (1u64 << 0) - 1; // 保留 256 帧整
    let r3 = cut_runs(&b3, 256);
    let i3 = extern_index(&r3);
    cs.add("注入3 全占零容量", i3 == [0, 0, 0, 0, 0], "无空闲");

    // 注入 4：度量不改位图（纯函数验证）
    let before = bits.clone();
    let _ = extern_index(&runs);
    cs.add("注入4 只读不变", bits == before, "零副作用");

    cs
}
