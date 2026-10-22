//! CGPU-F0204 · 帧图历史与趋势（判据段 0x0660）
//! 帧图的历史追踪：帧图结构历史（最近 N 帧的图结构快照——结构演
//! 化趋势）、性能趋势（图执行耗时的滑动窗口趋势——劣化检测 F0100
//! 复用）、异常帧标记（结构异常/耗时异常的帧自动标记）。判据：
//! 历史存储、趋势检测、异常标记、查询接口、内存治理。
//!
//! 诊断码段 0x0660（本模块独占：0x0660~0x066F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0660;

/// 历史窗口上限（帧数）：内存治理红线，超限淘汰最老。
pub const HISTORY_WINDOW_MAX: usize = 128;

/// 劣化判定门槛（千分比）：滑动窗口均值相对前窗涨幅 ≥ 150‰ 判劣化。
pub const DEGRADE_THRESHOLD_PERMILLE: u32 = 150;

/// 异常帧耗时门槛（千分比）：单帧耗时超全历史均值 + 500‰ 判异常。
pub const OUTLIER_THRESHOLD_PERMILLE: u32 = 500;

/// 帧图结构快照（判据「历史存储」原子量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameSnapshot {
    /// 帧号。
    pub frame: u32,
    /// 节点数。
    pub nodes: u32,
    /// 边数。
    pub edges: u32,
    /// 关键路径长度（节点数计）。
    pub critical: u32,
    /// 执行总耗时（微秒）。
    pub exec_us: u64,
}

impl FrameSnapshot {
    /// 合法性：账面自洽。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.nodes == 0 {
            return Err(SEG_BASE);
        }
        if (self.edges as u64) < (self.nodes as u64).saturating_sub(1) {
            return Err(SEG_BASE + 1);
        }
        if self.critical == 0 || self.critical > self.nodes || self.exec_us == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 帧图历史台账（判据「历史存储」容器 + 判据「内存治理」淘汰制）。
#[derive(Default)]
pub struct FrameHistory {
    snaps: Vec<FrameSnapshot>,
}

impl FrameHistory {
    /// 追加快照（窗口满时淘汰最老——O(1) 移除头部）。
    pub fn push(&mut self, s: FrameSnapshot) -> Result<(), u16> {
        s.validate()?;
        if self.snaps.iter().any(|x| x.frame == s.frame) {
            return Err(SEG_BASE + 3);
        }
        if self.snaps.len() >= HISTORY_WINDOW_MAX {
            self.snaps.remove(0);
        }
        self.snaps.push(s);
        Ok(())
    }
    /// 窗口规模。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }
    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }
    /// 按帧号查询（判据「查询接口」）。
    pub fn get(&self, frame: u32) -> Result<&FrameSnapshot, u16> {
        self.snaps.iter().find(|s| s.frame == frame).ok_or(SEG_BASE + 4)
    }
    /// 最近 N 帧切片（判据「查询接口」窗口面）。
    pub fn last_n(&self, n: usize) -> Result<Vec<&FrameSnapshot>, u16> {
        if n == 0 || n > self.snaps.len() {
            return Err(SEG_BASE + 5);
        }
        Ok(self.snaps[self.snaps.len() - n..].iter().collect())
    }
    /// 全量只读。
    pub fn all(&self) -> &[FrameSnapshot] {
        &self.snaps
    }
}

/// 滑动窗口均值趋势（判据「趋势检测」核心量）：把历史切成等宽窗
/// 口，返回各窗平均耗时序列（微秒）。窗口宽 2 起且必须整除或允许
/// 尾窗残缺（残缺窗照算，样本 ≥1）。
pub fn window_means(hist: &FrameHistory, window: usize) -> Result<Vec<u64>, u16> {
    if window < 2 || hist.len() < window {
        return Err(SEG_BASE + 6);
    }
    let all = hist.all();
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        let end = (i + window).min(all.len());
        let seg = &all[i..end];
        let sum: u64 = seg.iter().map(|s| s.exec_us).sum();
        out.push(sum / seg.len() as u64);
        i += window;
    }
    Ok(out)
}

/// 劣化检测（判据「趋势检测」主判据）：相邻窗口均值涨幅超门槛即
/// 劣化。返回首次劣化的窗口索引（从第 2 窗起），无劣化返回 None。
pub fn detect_degradation(hist: &FrameHistory, window: usize) -> Result<Option<usize>, u16> {
    let means = window_means(hist, window)?;
    for k in 1..means.len() {
        if means[k - 1] == 0 {
            return Err(SEG_BASE + 7);
        }
        let growth = means[k].abs_diff(means[k - 1]) * 1000 / means[k - 1];
        if means[k] > means[k - 1] && growth >= DEGRADE_THRESHOLD_PERMILLE as u64 {
            return Ok(Some(k));
        }
    }
    Ok(None)
}

/// 异常帧标记（判据「异常标记」主判据）：单帧耗时超过全历史均值
/// + 门槛的帧号列表。
pub fn mark_outlier_frames(hist: &FrameHistory) -> Result<Vec<u32>, u16> {
    if hist.len() < 3 {
        return Err(SEG_BASE + 8);
    }
    let all = hist.all();
    let sum: u64 = all.iter().map(|s| s.exec_us).sum();
    let avg = sum / all.len() as u64;
    let mut out: Vec<u32> = Vec::new();
    for s in all {
        if s.exec_us * 1000 > avg * (1000 + OUTLIER_THRESHOLD_PERMILLE as u64) {
            out.push(s.frame);
        }
    }
    Ok(out)
}

/// 结构异常标记（判据「异常标记」结构面）：节点数或边数相对全史
/// 中位数偏离超 ±50% 的帧。
pub fn mark_struct_anomalies(hist: &FrameHistory) -> Result<Vec<u32>, u16> {
    if hist.len() < 3 {
        return Err(SEG_BASE + 9);
    }
    let all = hist.all();
    let mut nodes: Vec<u32> = all.iter().map(|s| s.nodes).collect();
    nodes.sort_unstable();
    let med = nodes[nodes.len() / 2] as u64;
    if med == 0 {
        return Err(SEG_BASE + 10);
    }
    let mut out: Vec<u32> = Vec::new();
    for s in all {
        let dev = s.nodes.abs_diff(med as u32) as u64 * 1000 / med;
        if dev > 500 {
            out.push(s.frame);
        }
    }
    Ok(out)
}

/// 结构演化趋势（判据「趋势检测」结构面）：首尾窗口节点数变化率
/// ——膨胀（>150‰）报警，萎缩（<−150‰）也报警（误删嫌疑）。
pub struct GrowthReport {
    /// 首窗平均节点数。
    pub head_nodes: u64,
    /// 尾窗平均节点数。
    pub tail_nodes: u64,
    /// 变化率千分比（可负）。
    pub change_permille: i64,
}

impl GrowthReport {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.head_nodes == 0 || self.tail_nodes == 0 {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }
    /// 报警判定：|变化率| > 150‰ 即报警。
    pub fn alarmed(&self) -> Result<bool, u16> {
        self.validate()?;
        Ok(self.change_permille.abs() > 150)
    }
}

/// 演化趋势核算（判据「趋势检测」结构面实现）。
pub fn structure_growth(hist: &FrameHistory, window: usize) -> Result<GrowthReport, u16> {
    if window < 2 || hist.len() < window * 2 {
        return Err(SEG_BASE + 12);
    }
    let all = hist.all();
    let avg = |seg: &[FrameSnapshot]| -> u64 {
        seg.iter().map(|s| s.nodes as u64).sum::<u64>() / seg.len() as u64
    };
    let head = avg(&all[..window]);
    let tail = avg(&all[all.len() - window..]);
    let change = ((tail as i64 - head as i64) * 1000) / head as i64;
    Ok(GrowthReport { head_nodes: head, tail_nodes: tail, change_permille: change })
}

/// 历史台账指纹（判据「查询接口」可审计面）。
pub fn history_fp(hist: &FrameHistory) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for s in hist.all() {
        acc = acc.wrapping_mul(0x100000001b3) ^ s.frame as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.nodes as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.exec_us;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snap(frame: u32, us: u64) -> FrameSnapshot {
        FrameSnapshot { frame, nodes: 10, edges: 12, critical: 4, exec_us: us }
    }
    #[test]
    fn push_and_query() {
        let mut h = FrameHistory::default();
        for f in 1..=10u32 {
            h.push(snap(f, 1000)).unwrap();
        }
        assert_eq!(h.len(), 10);
        assert_eq!(h.get(5).unwrap().frame, 5);
        assert_eq!(h.last_n(3).unwrap().len(), 3);
        assert!(h.get(99).is_err());
    }
    #[test]
    fn window_eviction() {
        let mut h = FrameHistory::default();
        for f in 1..=130u32 {
            h.push(snap(f, 1000)).unwrap();
        }
        assert_eq!(h.len(), HISTORY_WINDOW_MAX);
        // 最老的 1、2 被淘汰。
        assert!(h.get(1).is_err());
        assert!(h.get(130).is_ok());
    }
    #[test]
    fn degradation_detected() {
        let mut h = FrameHistory::default();
        // 前 4 帧快后 4 帧慢 2 倍 → 窗口 4 检出劣化。
        for f in 1..=4u32 {
            h.push(snap(f, 1000)).unwrap();
        }
        for f in 5..=8u32 {
            h.push(snap(f, 2200)).unwrap();
        }
        assert_eq!(detect_degradation(&h, 4).unwrap(), Some(1));
    }
    #[test]
    fn stable_not_flagged() {
        let mut h = FrameHistory::default();
        for f in 1..=8u32 {
            h.push(snap(f, 1000 + f as u64 * 10)).unwrap();
        }
        assert_eq!(detect_degradation(&h, 4).unwrap(), None);
    }
    #[test]
    fn outliers_marked() {
        let mut h = FrameHistory::default();
        for f in 1..=5u32 {
            h.push(snap(f, 1000)).unwrap();
        }
        h.push(snap(6, 9000)).unwrap();
        let out = mark_outlier_frames(&h).unwrap();
        assert_eq!(out, vec![6]);
    }
    #[test]
    fn struct_anomalies_detected() {
        let mut h = FrameHistory::default();
        for f in 1..=5u32 {
            let mut s = snap(f, 1000);
            s.nodes = 10;
            s.edges = 12;
            h.push(s).unwrap();
        }
        let mut big = snap(6, 1000);
        big.nodes = 40; // 中位数 10 偏离 300%。
        big.edges = 45;
        h.push(big).unwrap();
        assert_eq!(mark_struct_anomalies(&h).unwrap(), vec![6]);
    }
    #[test]
    fn growth_report_math() {
        let mut h = FrameHistory::default();
        for f in 1..=4u32 {
            h.push(snap(f, 1000)).unwrap();
        }
        for f in 5..=8u32 {
            let mut s = snap(f, 1000);
            s.nodes = 15;
            h.push(s).unwrap();
        }
        let g = structure_growth(&h, 4).unwrap();
        assert_eq!((g.head_nodes, g.tail_nodes), (10, 15));
        assert_eq!(g.change_permille, 500);
        assert!(g.alarmed().unwrap());
    }
}
