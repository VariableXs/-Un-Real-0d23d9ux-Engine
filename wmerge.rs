//! wmerge — WP-203 · B-704 写合并收益（判据实装层，MD2 篇 7.1/7.3 + MD1 19.1）。
//!
//! 判据 B-704：元数据吞吐 ≥ BOT 极限八成（MD1 19.1 预算表）。
//! MD1 19.1 硬约束来源列明文：**U 盘 BOT 顺序读 ~35MB/s**——即 BOT 极限
//! 口径；八成线 = 28MB/s。测量方式 = 存储基准（19.2 vxbench）——本模块是
//! 判据逻辑面（合并器 + 吞吐模型 + 对练），vxbench 实测回补随接线。
//! MD2 篇 7.1："请求合并与排序——相邻逻辑块的写请求合并成大请求
//! （BOT 喜欢大块顺序传输）。"
//! MD2 篇 7.3：写合并枢纽 + 五秒窗口（与 B-702 同一数字）。
//!
//! 吞吐模型（整数运算，诚实标注为宿主近似）：
//! - 每请求固定开销 T_OVERHEAD_US（BOT 三段式 CBW/数据/CSW 半双工往返）；
//! - 每块传输 T_PER_BLOCK_US = 4096B ÷ 35MB/s ≈ 117us；
//! - 零散 1 块/请求 ≈ 4096/(150+117) ≈ 15.3MB/s（四成极限——不合并的惨状）；
//! - 合并平均段长 B：4096/(150/B+117)——B ≥ 2.3 即过八成线；
//! - 元数据区集中写（ext4 inode 表/位图相邻是事实）排序归并后平均段长
//!   轻松 ≥ 5——收益来源明确，不是模型美化。

use crate::checks::CheckSet;
// no_std 态走 alloc 的 Vec/vec!（kvsrv/power_shutdown 同范式，两态通吃）。
use alloc::vec::Vec;

/// BOT 通道极限（MD1 19.1 硬约束来源列：顺序读 ~35MB/s）。
pub const BOT_LIMIT_KB_S: u64 = 35_000;
/// 八成线（B-704 判据线）。
pub const EIGHTY_LINE_KB_S: u64 = BOT_LIMIT_KB_S * 8 / 10; // 28000
/// 每请求固定开销（us）——BOT 三段式协议往返的宿主模型值。
pub const T_OVERHEAD_US: u64 = 150;
/// 每块传输时间（us）——4096B ÷ 35MB/s，整数化 117。
pub const T_PER_BLOCK_US: u64 = 117;
pub const BLOCK_BYTES: u64 = 4096;
/// 合并窗口（与 B-702/B-703 同一数字——附录 K 旋钮联动）。
pub const MERGE_WINDOW_MS: u32 = 5_000;
/// 元数据窗口块数（模型：ext4 元数据区集中性的形态化——flex_bg 下
/// 一个块组的 inode 表 + 块位图 + inode 位图连续 32 块量级 = 128KB 窗）。
pub const META_WINDOW_BLOCKS: u64 = 32;
/// 五秒窗内元数据刷盘次数（模型参数：128 次 ≈ 25.6 次/秒——
/// ext4 日志提交节奏量级；vxbench 实测回补校准）。
pub const META_WRITES_PER_WINDOW: usize = 128;

/// 一个合并后的大请求（连续段：起点 + 块数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segment {
    pub start_lba: u64,
    pub blocks: u64,
}

/// 合并器：无序写块序列 → 排序去重 → 相邻归并成连续段（BOT 喜欢大块顺序）。
pub fn merge_sorted(blocks: &[u64]) -> Vec<Segment> {
    let mut sorted = blocks.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out: Vec<Segment> = Vec::new();
    for &b in &sorted {
        match out.last_mut() {
            Some(seg) if seg.start_lba + seg.blocks == b => seg.blocks += 1,
            _ => out.push(Segment { start_lba: b, blocks: 1 }),
        }
    }
    out
}

/// 吞吐模型（整数 KB/s）：总字节 ÷ 总时间。
/// 总时间 = Σ(每段 T_OVERHEAD + 段长 × T_PER_BLOCK)。
pub fn throughput_kb_s(segments: &[Segment]) -> u64 {
    let total_blocks: u64 = segments.iter().map(|s| s.blocks).sum();
    if total_blocks == 0 {
        return 0;
    }
    let total_us: u64 = segments
        .iter()
        .map(|s| T_OVERHEAD_US + s.blocks * T_PER_BLOCK_US)
        .sum();
    // KB/s（十进制口径，与标称带宽 35MB/s 同口径）：
    // B/s = bytes × 1e6 / total_us；KB/s = B/s / 1000
    total_blocks * BLOCK_BYTES * 1000 / total_us
}

/// 零散写基线（不合并：每块一请求）的吞吐——收益对照面。
pub fn unmerged_throughput_kb_s(block_count: u64) -> u64 {
    if block_count == 0 {
        return 0;
    }
    let total_us = block_count * (T_OVERHEAD_US + T_PER_BLOCK_US);
    block_count * BLOCK_BYTES * 1000 / total_us
}

// ---------------------------------------------------------------- 对练

use crate::comprecover::Lcg;

/// 合并对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct MergeDrillSummary {
    pub rounds: u32,
    /// 归并后平均段长 ×100（整数）
    pub avg_segment_len_x100: u64,
    /// 合并后吞吐（取所有轮最小值——保守口径）
    pub min_throughput_kb_s: u64,
    /// 不合并基线吞吐（对照）
    pub unmerged_kb_s: u64,
}

/// 元数据集中写对练：META_WINDOW_BLOCKS 块窗口内随机写 40 次（模拟五秒窗
/// 的元数据刷盘），排序归并 → 段分布 → 吞吐对八成线。
pub fn run_merge_drills(seed: u64, rounds: u32) -> MergeDrillSummary {
    let mut g = Lcg(seed);
    let mut sum = MergeDrillSummary::default();
    sum.rounds = rounds;
    sum.min_throughput_kb_s = u64::MAX;
    let mut total_segments = 0u64;
    let mut total_blocks_written = 0u64;
    for _ in 0..rounds {
        let mut writes: Vec<u64> = Vec::new();
        for _ in 0..META_WRITES_PER_WINDOW {
            writes.push(g.next() % META_WINDOW_BLOCKS);
        }
        let segs = merge_sorted(&writes);
        let uniq: u64 = segs.iter().map(|s| s.blocks).sum();
        total_segments += segs.len() as u64;
        total_blocks_written += uniq;
        let tp = throughput_kb_s(&segs);
        if tp < sum.min_throughput_kb_s {
            sum.min_throughput_kb_s = tp;
        }
        sum.unmerged_kb_s = unmerged_throughput_kb_s(uniq);
    }
    if rounds > 0 {
        sum.avg_segment_len_x100 = total_blocks_written * 100 / total_segments.max(1);
    }
    if sum.min_throughput_kb_s == u64::MAX {
        sum.min_throughput_kb_s = 0;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_wmerge_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-704 写合并收益");
    {
        // 相邻归并：连续块合一段
        let segs = merge_sorted(&[5, 2, 3, 4, 9]);
        set.add(
            "B-704 相邻块归并成段",
            segs == alloc::vec![Segment { start_lba: 2, blocks: 4 }, Segment { start_lba: 9, blocks: 1 }],
            "排序去重后 2-5 连段",
        );
    }
    {
        // 同块重写去重
        let segs = merge_sorted(&[7, 7, 7]);
        set.add(
            "B-704 同块重写去重",
            segs == alloc::vec![Segment { start_lba: 7, blocks: 1 }],
            "一请求承载",
        );
    }
    {
        // 模型自洽：段长 1 的吞吐 = 零散基线
        let segs = alloc::vec![Segment { start_lba: 0, blocks: 1 }];
        set.add(
            "B-704 模型自洽零散基线",
            throughput_kb_s(&segs) == unmerged_throughput_kb_s(1),
            "单块一请求两口径同值",
        );
    }
    {
        // 零散写远低于八成线（不合并的惨状是真实的）
        let tp = unmerged_throughput_kb_s(40);
        set.add(
            "B-704 不合并低于八成线",
            tp < EIGHTY_LINE_KB_S,
            "40 块零散写约 15MB/s < 28MB/s",
        );
    }
    {
        // 合并收益单调：段长 8 的吞吐 > 段长 2
        let s2 = alloc::vec![Segment { start_lba: 0, blocks: 2 }];
        let s8 = alloc::vec![Segment { start_lba: 0, blocks: 8 }];
        set.add(
            "B-704 合并收益单调",
            throughput_kb_s(&s8) > throughput_kb_s(&s2),
            "开销摊薄吞吐升",
        );
    }
    {
        // 元数据集中写对练：最差轮吞吐 ≥ 八成线
        let sum = run_merge_drills(0xB704, 100);
        set.add(
            "B-704 对练最差轮过八成线",
            sum.min_throughput_kb_s >= EIGHTY_LINE_KB_S,
            "元数据区集中性是收益来源",
        );
    }
    {
        // 平均段长 ≥ 2.3 块（八成线的结构要求）
        let sum = run_merge_drills(0xB704, 100);
        set.add(
            "B-704 平均段长结构达标",
            sum.avg_segment_len_x100 >= 230,
            "八成线需平均 ≥2.3 块/请求",
        );
    }
    {
        // 合并 vs 不合并收益比 ≥ 1.5×（合并有效性）
        let sum = run_merge_drills(0xB704, 100);
        set.add(
            "B-704 合并有效性",
            sum.min_throughput_kb_s * 2 >= sum.unmerged_kb_s * 3,
            "最差轮 ≥ 基线 1.5×",
        );
    }
    {
        // 旋钮联动：合并窗口与承诺窗口同一数字
        set.add(
            "B-704 合并窗口=承诺窗口",
            MERGE_WINDOW_MS == crate::fsyncp::COALESCE_WINDOW_MS,
            "附录 K 旋钮联动 5000ms",
        );
    }
    {
        // 八成线数字来源（MD1 19.1 BOT 极限 35MB/s）
        set.add(
            "B-704 八成线口径",
            BOT_LIMIT_KB_S == 35_000 && EIGHTY_LINE_KB_S == 28_000,
            "MD1 19.1 硬约束来源列 35MB/s × 0.8",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f504_merge_basics() {
        assert!(merge_sorted(&[]).is_empty());
        assert_eq!(
            merge_sorted(&[5, 2, 3, 4, 9]),
            alloc::vec![
                Segment { start_lba: 2, blocks: 4 },
                Segment { start_lba: 9, blocks: 1 }
            ]
        );
        assert_eq!(merge_sorted(&[7, 7, 7]), alloc::vec![Segment { start_lba: 7, blocks: 1 }]);
    }

    #[test]
    fn f504_throughput_model() {
        // 单块 = 零散基线
        let one = alloc::vec![Segment { start_lba: 0, blocks: 1 }];
        assert_eq!(throughput_kb_s(&one), unmerged_throughput_kb_s(1));
        // 段长单调
        let s1 = alloc::vec![Segment { start_lba: 0, blocks: 1 }];
        let s4 = alloc::vec![Segment { start_lba: 0, blocks: 4 }];
        let s16 = alloc::vec![Segment { start_lba: 0, blocks: 16 }];
        let t1 = throughput_kb_s(&s1);
        let t4 = throughput_kb_s(&s4);
        let t16 = throughput_kb_s(&s16);
        assert!(t4 > t1 && t16 > t4, "段长增吞吐增");
        // 大段逼近纯传输极限（开销 150/2022 ≈ 7.4%，吞吐 ≈ 32.4MB/s）
        assert!(t16 > 32_000, "段长 16 开销占比 <10%，逼近 BOT 极限");
    }

    #[test]
    fn f504_unmerged_below_line() {
        let tp = unmerged_throughput_kb_s(40);
        // 4096e6/1024/267 ≈ 14990 KB/s（四成极限）
        assert!(tp < 16_000 && tp > 14_000);
        assert!(tp < EIGHTY_LINE_KB_S);
    }

    #[test]
    fn f504_drill_passes_line() {
        let sum = run_merge_drills(0xB704, 100);
        assert_eq!(sum.rounds, 100);
        assert!(sum.min_throughput_kb_s >= EIGHTY_LINE_KB_S, "最差轮 ≥ 28MB/s");
        assert!(sum.avg_segment_len_x100 >= 230, "平均段长 ≥ 2.3");
        assert!(sum.min_throughput_kb_s * 2 >= sum.unmerged_kb_s * 3, "合并有效性 1.5×");
    }

    #[test]
    fn f504_window_one_number() {
        assert_eq!(MERGE_WINDOW_MS, 5_000);
        assert_eq!(MERGE_WINDOW_MS, crate::fsyncp::COALESCE_WINDOW_MS);
        assert_eq!(MERGE_WINDOW_MS, crate::pwrdrl::POWER_LOSS_WINDOW_MS);
    }

    #[test]
    fn f504_self_checks_pass() {
        let set = run_wmerge_checks();
        assert!(set.all_passed(), "B-704 自检全绿");
    }
}
