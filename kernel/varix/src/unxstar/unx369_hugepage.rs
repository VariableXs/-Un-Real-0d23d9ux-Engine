//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0369 · 大页候选区盘点（2MB 对齐窗）· 诊断段 0xf0f0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   2MB 对齐候选窗计数与位图扫描一致，最大窗尺寸账可复算。
//! 难点分析：
//!   大页（2MB）要求物理地址 2MB 对齐且窗内 512 个 4KB 帧全部空闲。
//!   盘点的陷阱在**对齐窗口的边角**：位图扫描按帧粒度走，对齐窗按
//!   2MB 粒度落——扫描必须以「窗首帧 = addr/4096，addr % 2MB == 0」
//!   为锚，跳窗步进 512 帧。最大窗尺寸账可复算 = 连续空闲窗合并
//!   后的帧数能用「窗数 × 512」从零重算。
//! 纪律约束：
//!   - 对齐锚定：窗首帧号 ≡ 0 (mod 512)；
//!   - 全空才算：窗内一帧占用即废窗；
//!   - 计数可复算：候选窗总数、最大连片帧数两条账独立重算一致；
//!   - 尾窗不越界：窗尾帧 < 总帧数。
//! 成本记账：单遍扫描 O(n/512)；复算 O(k)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf0f0;

/// 2MB 窗 = 512 个 4KB 帧。
pub const FRAMES_PER_WINDOW: usize = 512;

// ─────────────────────────────────────────────────────────────────────
// 一、位图扫描：对齐候选窗
// ─────────────────────────────────────────────────────────────────────

/// 扫描位图（bit=0 空闲）中的全部 2MB 对齐空窗。
/// 返回窗首帧号列表（升序）。
pub fn scan_2m_windows(bits: &[u64], total_frames: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut w = 0usize; // 窗序
    while (w + 1) * FRAMES_PER_WINDOW <= total_frames {
        let base = w * FRAMES_PER_WINDOW;
        if window_all_free(bits, base) {
            out.push(base);
        }
        w += 1;
    }
    out
}

/// 窗内 512 帧全空闲？
fn window_all_free(bits: &[u64], base: usize) -> bool {
    for i in 0..FRAMES_PER_WINDOW {
        let idx = base + i;
        if bits[idx / 64] & (1u64 << (idx % 64)) != 0 {
            return false;
        }
    }
    true
}

/// 最大连片空闲帧数（不限对齐，用于可复算对冲）。
pub fn max_free_run(bits: &[u64], total_frames: usize) -> usize {
    let mut best = 0usize;
    let mut cur = 0usize;
    for idx in 0..total_frames {
        if bits[idx / 64] & (1u64 << (idx % 64)) == 0 {
            cur += 1;
            if cur > best {
                best = cur;
            }
        } else {
            cur = 0;
        }
    }
    best
}

/// 复算账：候选窗数与最大窗尺寸两条账独立重算。
/// 最大窗尺寸 = 最多连续多少个**相邻对齐空窗**（窗号连续）× 512 帧。
pub fn max_contig_windows(wins: &[usize]) -> usize {
    if wins.is_empty() {
        return 0;
    }
    let mut best = 1usize;
    let mut cur = 1usize;
    for i in 1..wins.len() {
        if wins[i] == wins[i - 1] + FRAMES_PER_WINDOW {
            cur += 1;
            if cur > best {
                best = cur;
            }
        } else {
            cur = 1;
        }
    }
    best
}

// ─────────────────────────────────────────────────────────────────────
// 二、黄金位图夹具
// ─────────────────────────────────────────────────────────────────────

/// 黄金夹具：4096 帧 = 8 个 2MB 窗。
/// 窗 0/1 全空、窗 2 占 1 帧、窗 3/4/5 连片全空、窗 6 占尾帧、窗 7 全空。
pub fn golden_bits() -> (Vec<u64>, usize) {
    let total: usize = 4096;
    let mut bits = vec![0u64; total.div_ceil(64)];
    let set = |bits: &mut Vec<u64>, idx: usize| bits[idx / 64] |= 1u64 << (idx % 64);
    set(&mut bits, 2 * 512 + 100); // 窗 2 中间占一帧
    set(&mut bits, 6 * 512 + 511); // 窗 6 尾帧占
    (bits, total)
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0369 判据全集。
pub fn run_unx369_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0369 大页候选区盘点");
    let (bits, total) = golden_bits();

    // J1 主判据：候选窗计数 + 最大窗可复算
    let wins = scan_2m_windows(&bits, total);
    cs.add(
        "J1 候选窗 5 个",
        wins == vec![0, 512, 1536, 2048, 3584],
        "窗 0/1/3/4/7",
    );
    let mcw = max_contig_windows(&wins);
    cs.add("J1 最大连片 3 窗", mcw == 3, "窗 3/4/5 连片");
    cs.add("J1 可复算", mcw * FRAMES_PER_WINDOW == 1536, "3×512=1536 帧");

    // 独立重算对冲：全量连片空闲帧 ≥ 对齐连片帧
    let run = max_free_run(&bits, total);
    cs.add("J1 对冲上界", run >= mcw * FRAMES_PER_WINDOW, "非对齐连片 ≥ 对齐连片");

    // 注入 1：窗首非对齐帧占用不影响该窗判定（锚定语义）
    let (b1, t1) = golden_bits();
    let mut b1 = b1;
    b1[100 / 64] |= 1u64 << (100 % 64); // 帧 100 = 窗 0 内部，非对齐锚
    let w1 = scan_2m_windows(&b1, t1);
    cs.add(
        "注入1 内部占用废窗",
        !w1.contains(&0) && w1.len() == 4,
        "窗 0 废、其余保留",
    );

    // 注入 2：尾帧占用废窗（窗 6 已在黄金夹具）+ 边界窗数
    cs.add("注入2 尾帧废窗", !wins.contains(&3072usize), "窗 6 不候选");
    // 总帧 4095（去一帧）→ 第 8 窗不完整不参评
    let w2 = scan_2m_windows(&bits, 4095);
    cs.add("注入2 尾窗不越界", w2.len() == 5, "残窗不参评");

    // 注入 3：全空位图 → 全窗候选 + 计数公式
    let b3 = vec![0u64; 64];
    let w3 = scan_2m_windows(&b3, 4096);
    cs.add("注入3 全空 8 窗", w3.len() == 8 && max_contig_windows(&w3) == 8, "8×512 复算");

    // 注入 4：全占位图 → 零候选零窗
    let mut b4 = vec![0u64; 64];
    for x in b4.iter_mut() {
        *x = !0u64;
    }
    cs.add("注入4 全占零窗", scan_2m_windows(&b4, 4096).is_empty(), "无候选");

    cs
}
