//! VE-F0099 · 回放索引服务（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0099`
//!
//! **职责定位（锚点原文）**：为诊断回放流建时间轴索引——按事件时间戳秒级分桶，
//! 每桶记录流内偏移与事件数，支持按时间区间快速定位回放起点；索引随流写入
//! 增量更新，崩溃后可从流尾水位增量重建，不需全量重扫。
//!
//! **铁律**：
//! 1. **分桶索引**——秒级分桶，桶内记偏移与计数。
//! 2. **增量更新**——写入 O(1) 追加。
//! 3. **可重建**——从流尾水位增量重建，不全量重扫。
//! 4. **乱序标记**——时间戳乱序按到达序入桶并标记异常段。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8AA（回放索引）。
pub const SEG_BASE: u16 = 0xE8AA;

/// 索引损坏。
pub const XC_INDEX_CORRUPT: u16 = SEG_BASE | 0x01;
/// 流被截断。
pub const XC_STREAM_TRUNCATED: u16 = SEG_BASE | 0x02;
/// 时间区间非法。
pub const XC_BAD_RANGE: u16 = SEG_BASE | 0x03;
/// 水位非法。
pub const XC_BAD_WATERMARK: u16 = SEG_BASE | 0x04;

/// 索引桶（一秒一桶）。
#[derive(Clone, Copy, Debug)]
pub struct IndexBucket {
    /// 桶起始秒。
    pub second: u64,
    /// 流内起始偏移。
    pub offset: u64,
    /// 桶内事件数。
    pub count: u32,
    /// 乱序标记（本桶内有事件时间戳倒退）。
    pub out_of_order: bool,
}

/// 流尾水位。
#[derive(Clone, Copy, Debug)]
pub struct Watermark {
    /// 已索引到的流偏移。
    pub offset: u64,
    /// 已索引到的最大时间戳秒。
    pub second: u64,
}

/// 截断标记。
#[derive(Clone, Copy, Debug)]
pub struct TruncateMark {
    pub at_offset: u64,
    pub at_second: u64,
}

/// 回放索引。
pub struct ReplayIndex {
    buckets: Vec<IndexBucket>,
    watermark: Watermark,
    truncate: Option<TruncateMark>,
    /// 当前追加中的桶（未封口）。
    open_bucket: Option<IndexBucket>,
    /// 上一个事件时间戳（乱序检测）。
    last_ts: u64,
    last_code: u16,
    /// 增量重建次数。
    rebuilds: u32,
    /// 乱序事件计数。
    ooo_count: u32,
}

impl ReplayIndex {
    pub fn new() -> Self {
        ReplayIndex {
            buckets: Vec::new(),
            watermark: Watermark {
                offset: 0,
                second: 0,
            },
            truncate: None,
            open_bucket: None,
            last_ts: 0,
            last_code: 0,
            rebuilds: 0,
            ooo_count: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 追加一个事件（增量更新 O(1)）。
    pub fn append(&mut self, ts_sec: u64, offset: u64) -> Result<(), u16> {
        // 截断后拒绝追加（已截断流不接受新事件进旧索引）。
        if self.truncate.is_some() {
            self.last_code = XC_STREAM_TRUNCATED;
            return Err(XC_STREAM_TRUNCATED);
        }
        let ooo = ts_sec < self.last_ts;
        if ooo {
            self.ooo_count += 1;
        }
        // 同秒续桶，跨秒封口开新桶。
        let need_new = match &self.open_bucket {
            Some(b) => b.second != ts_sec,
            None => true,
        };
        if need_new {
            if let Some(b) = self.open_bucket.take() {
                self.buckets.push(b);
            }
            self.open_bucket = Some(IndexBucket {
                second: ts_sec,
                offset,
                count: 1,
                out_of_order: ooo,
            });
        } else if let Some(b) = self.open_bucket.as_mut() {
            b.count += 1;
            if ooo {
                b.out_of_order = true;
            }
        }
        self.last_ts = ts_sec;
        self.watermark.offset = offset;
        if ts_sec > self.watermark.second {
            self.watermark.second = ts_sec;
        }
        self.last_code = 0;
        Ok(())
    }

    /// 封口当前桶（流暂停/刷盘）。
    pub fn flush(&mut self) {
        if let Some(b) = self.open_bucket.take() {
            self.buckets.push(b);
        }
    }

    /// 按时间区间定位回放起点（二分 O(log 桶数)）。
    /// 返回桶下标（buckets 内，含封口桶）。
    pub fn locate(&mut self, from_sec: u64, to_sec: u64) -> Result<u64, u16> {
        if from_sec > to_sec {
            self.last_code = XC_BAD_RANGE;
            return Err(XC_BAD_RANGE);
        }
        self.flush();
        if self.buckets.is_empty() {
            self.last_code = XC_INDEX_CORRUPT;
            return Err(XC_INDEX_CORRUPT);
        }
        // 二分找第一个 second >= from_sec 的桶。
        let mut lo = 0usize;
        let mut hi = self.buckets.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.buckets[mid].second < from_sec {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo >= self.buckets.len() {
            // 全区间之前：返回末桶偏移（回放到尾）。
            return Ok(self.buckets.last().unwrap().offset);
        }
        // 校验落在区间内。
        if self.buckets[lo].second > to_sec {
            self.last_code = XC_BAD_RANGE;
            return Err(XC_BAD_RANGE);
        }
        self.last_code = 0;
        Ok(self.buckets[lo].offset)
    }

    /// 标记流截断。
    pub fn mark_truncated(&mut self, at_offset: u64) {
        self.flush();
        let sec = self.watermark.second;
        self.truncate = Some(TruncateMark {
            at_offset,
            at_second: sec,
        });
        self.watermark.offset = at_offset;
        self.last_code = XC_STREAM_TRUNCATED;
    }

    pub fn truncate_mark(&self) -> Option<TruncateMark> {
        self.truncate
    }

    /// 从流尾水位增量重建：丢弃水位后的半截桶，保留之前索引。
    pub fn rebuild_from_watermark(&mut self, stream_offsets: &[(u64, u64)]) -> Result<(), u16> {
        // stream_offsets: (ts_sec, offset) 流中水位之后的事件。
        self.rebuilds += 1;
        // 截断掉水位后的残桶。
        self.buckets.retain(|b| b.offset <= self.watermark.offset);
        self.open_bucket = None;
        self.truncate = None;
        for &(ts, off) in stream_offsets {
            self.append(ts, off)?;
        }
        self.flush();
        self.last_code = 0;
        Ok(())
    }

    /// 索引完整性自检：桶偏移单调、计数非零。
    pub fn verify(&mut self) -> bool {
        self.flush();
        let mut prev_off: u64 = 0;
        for b in &self.buckets {
            if b.count == 0 || b.offset < prev_off {
                self.last_code = XC_INDEX_CORRUPT;
                return false;
            }
            prev_off = b.offset;
        }
        true
    }

    /// 重建为损坏恢复入口（全量重扫，限速由调用方控）。
    pub fn rebuild_full(&mut self, events: &[(u64, u64)]) -> Result<(), u16> {
        self.buckets.clear();
        self.open_bucket = None;
        self.truncate = None;
        self.watermark = Watermark {
            offset: 0,
            second: 0,
        };
        self.last_ts = 0;
        for &(ts, off) in events {
            self.append(ts, off)?;
        }
        self.flush();
        self.rebuilds += 1;
        Ok(())
    }

    pub fn watermark(&self) -> Watermark {
        self.watermark
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len() + self.open_bucket.is_some() as usize
    }

    pub fn buckets(&self) -> &[IndexBucket] {
        &self.buckets
    }

    pub fn ooo_count(&self) -> u32 {
        self.ooo_count
    }

    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }
}

impl Default for ReplayIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：分桶索引、增量更新、可重建、乱序标记。
pub fn run_vea99_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea99_replayidx");

    // C99-01 分桶建索引
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(10, 0);
        let _ = ix.append(10, 100);
        let _ = ix.append(11, 200);
        ix.flush();
        chk(&mut set, "C99-01 分桶", ix.buckets().len() == 2, "桶数不对");
    }

    // C99-02 增量更新水位推进
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(5, 10);
        let _ = ix.append(6, 20);
        let ok = ix.watermark().offset == 20 && ix.watermark().second == 6;
        chk(&mut set, "C99-02 增量水位", ok, "水位未推进");
    }

    // C99-03 区间定位
    {
        let mut ix = ReplayIndex::new();
        for i in 0..10u64 {
            let _ = ix.append(100 + i, i * 50);
        }
        let r = ix.locate(103, 107);
        chk(&mut set, "C99-03 区间定位", r == Ok(150), "定位偏差");
    }

    // C99-04 非法区间拒绝
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(1, 0);
        let r = ix.locate(10, 5);
        chk(
            &mut set,
            "C99-04 非法区间",
            r.is_err() && ix.last_code() == XC_BAD_RANGE,
            "非法区间被放行",
        );
    }

    // C99-05 乱序标记
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(10, 0);
        let _ = ix.append(5, 10); // 乱序同秒？跨秒乱序
        ix.flush();
        let ok = ix.ooo_count() >= 1;
        chk(&mut set, "C99-05 乱序标记", ok, "乱序漏标");
    }

    // C99-06 截断标记
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(1, 0);
        let _ = ix.append(2, 100);
        ix.mark_truncated(150);
        let ok = ix.truncate_mark().is_some() && ix.append(3, 200).is_err();
        chk(&mut set, "C99-06 截断感知", ok, "截断未感知");
    }

    // C99-07 水位增量重建
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(1, 0);
        let _ = ix.append(2, 50);
        let r = ix.rebuild_from_watermark(&[(3, 100), (4, 150)]);
        let ok = r.is_ok() && ix.watermark().second == 4 && ix.rebuilds() == 1;
        chk(&mut set, "C99-07 增量重建", ok, "重建失败");
    }

    // C99-08 全量重建恢复损坏
    {
        let mut ix = ReplayIndex::new();
        let _ = ix.append(1, 0);
        // 制造损坏：手工塞零计数桶。
        ix.buckets.push(IndexBucket {
            second: 9,
            offset: 1,
            count: 0,
            out_of_order: false,
        });
        let bad = !ix.verify();
        let r = ix.rebuild_full(&[(1, 0), (2, 10)]);
        let ok = bad && r.is_ok() && ix.verify();
        chk(&mut set, "C99-08 全量重建", ok, "重建未修好");
    }

    // C99-09 空索引定位显性报错
    {
        let mut ix = ReplayIndex::new();
        let r = ix.locate(0, 10);
        chk(
            &mut set,
            "C99-09 空索引报错",
            r.is_err() && ix.last_code() == XC_INDEX_CORRUPT,
            "空索引被放行",
        );
    }

    // C99-10 桶偏移单调校验
    {
        let mut ix = ReplayIndex::new();
        for i in 0..5u64 {
            let _ = ix.append(10 + i, i * 10);
        }
        chk(&mut set, "C99-10 偏移单调", ix.verify(), "单调性破坏");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea99_all_green() {
        let s = run_vea99_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
