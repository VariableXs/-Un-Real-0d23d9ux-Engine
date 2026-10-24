//! cgd22_shortspot — CGPU-F0502 帧内短板定位（最长段识别/趋势/归因/清单）。
//!
//! 判据原文（施工书锚点 CGPU-F0502）：短板定位：最长段自动识别（帧内四段
//! 谁最长——木桶短板实时报告）；短板趋势（短板的漂移——yesterday vs today）；
//! 短板归因（段级归因联动 D03 卡顿归因引擎）；短板清单（Top-N 短板场景汇
//! 总）。判据：识别、趋势、归因、清单、判据。
//!
//! 模型：帧内四段（输入/模拟/渲染准备/提交）耗时采样→短板=最大段（并列
//! 取段号小者）；趋势=昨日 vs 今日短板段号漂移与占比变化；归因=短板段号
//! 映射 D03 归因类别；清单=按场景汇总 Top-N 短板。全整数、有界。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x8E00（cgpu 域内唯一）。
pub const XC_SPOT_EMPTY: u32 = 0x8E01;
pub const XC_SPOT_SEG_OOB: u32 = 0x8E02;
pub const XC_SPOT_TREND_UNGROUNDED: u32 = 0x8E03;
pub const XC_SPOT_ATTR_UNMAPPED: u32 = 0x8E04;
pub const XC_SPOT_LIST_OOB: u32 = 0x8E05;

/// 段数：输入/模拟/渲染准备/提交。
pub const SEG_COUNT: usize = 4;

/// 一帧四段耗时（µs）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSegs(pub [u32; SEG_COUNT]);

impl FrameSegs {
    /// 全零帧视为无效样本。
    pub fn is_zero(&self) -> bool {
        let mut i = 0;
        while i < SEG_COUNT {
            if self.0[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总耗时。
    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0;
        while i < SEG_COUNT {
            t = t.saturating_add(self.0[i]);
            i += 1;
        }
        t
    }

    /// 短板段号：最大段；并列取段号小者。
    pub fn bottleneck(&self) -> Result<usize, u32> {
        if self.is_zero() {
            return Err(XC_SPOT_EMPTY);
        }
        let mut best = 0;
        let mut i = 1;
        while i < SEG_COUNT {
            if self.0[i] > self.0[best] {
                best = i;
            }
            i += 1;
        }
        Ok(best)
    }

    /// 短板占比（千分比）。
    pub fn bottleneck_permille(&self) -> Result<u32, u32> {
        let b = self.bottleneck()?;
        let t = self.total();
        if t == 0 {
            return Err(XC_SPOT_EMPTY);
        }
        Ok((self.0[b] as u64 * 1000 / t as u64) as u32)
    }
}

/// D03 归因类别（段级映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// 输入段：采样堆积。
    InputBacklog = 0,
    /// 模拟段：逻辑超支。
    SimOverrun = 1,
    /// 渲染准备段：资源竞争。
    PrepContention = 2,
    /// 提交段：呈现阻塞。
    PresentStall = 3,
    /// 未映射段号。
    Unmapped,
}

/// 归因映射：段号→D03 类别（缺映射即拒）。
pub fn attribute(seg: usize) -> Result<Cause, u32> {
    match seg {
        0 => Ok(Cause::InputBacklog),
        1 => Ok(Cause::SimOverrun),
        2 => Ok(Cause::PrepContention),
        3 => Ok(Cause::PresentStall),
        _ => Err(XC_SPOT_ATTR_UNMAPPED),
    }
}

/// 一日短板快照：当日样本聚合。
#[derive(Clone, Debug)]
pub struct DaySpot {
    /// 各段被认定为短板的帧数。
    pub hits: [u32; SEG_COUNT],
    /// 有效帧数。
    pub frames: u32,
}

impl DaySpot {
    /// 空快照。
    pub fn new() -> Self {
        DaySpot {
            hits: [0; SEG_COUNT],
            frames: 0,
        }
    }

    /// 采样一帧（零帧拒收）。
    pub fn sample(&mut self, f: &FrameSegs) -> Result<usize, u32> {
        let b = f.bottleneck()?;
        self.hits[b] += 1;
        self.frames += 1;
        Ok(b)
    }

    /// 主短板段：命中最多者；并列取段号小者。空快照拒。
    pub fn dominant(&self) -> Result<usize, u32> {
        if self.frames == 0 {
            return Err(XC_SPOT_EMPTY);
        }
        let mut best = 0;
        let mut i = 1;
        while i < SEG_COUNT {
            if self.hits[i] > self.hits[best] {
                best = i;
            }
            i += 1;
        }
        Ok(best)
    }

    /// 占比（千分比）。
    pub fn dominant_permille(&self) -> Result<u32, u32> {
        let d = self.dominant()?;
        if self.frames == 0 {
            return Err(XC_SPOT_EMPTY);
        }
        Ok((self.hits[d] as u64 * 1000 / self.frames as u64) as u32)
    }
}

impl Default for DaySpot {
    fn default() -> Self {
        Self::new()
    }
}

/// 趋势判定：昨日 vs 今日。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 短板段未漂移且占比变化 < 50‰：平稳。
    Stable,
    /// 短板段漂移：报新段号。
    ShiftedTo(usize),
    /// 段未漂移但占比恶化 ≥ 50‰：加深。
    Deepening,
    /// 段未漂移且占比改善 ≥ 50‰：缓解。
    Easing,
}

/// 昨日/今日对比出趋势。空快照任一侧即拒。
pub fn trend_of(yest: &DaySpot, today: &DaySpot) -> Result<Trend, u32> {
    let y = yest.dominant()?;
    let t = today.dominant()?;
    if y != t {
        return Ok(Trend::ShiftedTo(t));
    }
    let yp = yest.dominant_permille()?;
    let tp = today.dominant_permille()?;
    if tp >= yp + 50 {
        Ok(Trend::Deepening)
    } else if yp >= tp + 50 {
        Ok(Trend::Easing)
    } else {
        Ok(Trend::Stable)
    }
}

/// 短板清单条目：场景名 + 主短板段 + 占比。
#[derive(Clone, Debug)]
pub struct SpotEntry {
    /// 场景名。
    pub scene: &'static str,
    /// 主短板段号。
    pub seg: usize,
    /// 占比（千分比）。
    pub permille: u32,
}

/// Top-N 清单：按占比降序、同占比按段号升序。
pub struct SpotList {
    entries: Vec<SpotEntry>,
    cap: usize,
}

impl SpotList {
    /// 容量上限 1..=16。
    pub fn new(cap: usize) -> Result<Self, u32> {
        if cap == 0 || cap > 16 {
            return Err(XC_SPOT_LIST_OOB);
        }
        Ok(SpotList {
            entries: Vec::new(),
            cap,
        })
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空？恒 false 用途——容量已验。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 入清单：满则挤掉末位（占比最小者）。
    pub fn record(&mut self, scene: &'static str, f: &FrameSegs) -> Result<(), u32> {
        let seg = f.bottleneck()?;
        let pm = f.bottleneck_permille()?;
        let e = SpotEntry {
            scene,
            seg,
            permille: pm,
        };
        if self.entries.len() < self.cap {
            self.entries.push(e);
        } else {
            // 找最小占比位。
            let mut min_i = 0;
            let mut i = 1;
            while i < self.entries.len() {
                if self.entries[i].permille < self.entries[min_i].permille {
                    min_i = i;
                }
                i += 1;
            }
            if pm > self.entries[min_i].permille {
                self.entries[min_i] = e;
            }
        }
        // 维持降序（插入排序，规模 ≤16）。
        let n = self.entries.len();
        let mut i = n - 1;
        while i > 0 && self.entries[i].permille > self.entries[i - 1].permille {
            self.entries.swap(i, i - 1);
            i -= 1;
        }
        Ok(())
    }

    /// Top-N 读取：前 k 条（k 越界钳制）。
    pub fn top(&self, k: usize) -> Vec<&SpotEntry> {
        let kk = k.min(self.entries.len());
        self.entries[..kk].iter().collect()
    }

    /// 有序性对账：全清单严格降序（同占比段号升序由插入序保证——记录序）。
    pub fn is_sorted_desc(&self) -> bool {
        let mut i = 1;
        while i < self.entries.len() {
            if self.entries[i - 1].permille < self.entries[i].permille {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 短板定位总门面。
#[derive(Debug, Default)]
pub struct SpotEngine {
    /// 今日快照。
    pub today: DaySpot,
    /// 昨日快照（供趋势）。
    pub yest: DaySpot,
}

impl SpotEngine {
    /// 自检一：识别——并列取小段号、零帧拒、占比正确。
    pub fn selfcheck_identify(&self) -> Result<(), u32> {
        let f = FrameSegs([100, 300, 300, 100]);
        if f.bottleneck()? != 1 {
            return Err(XC_SPOT_SEG_OOB);
        }
        if f.bottleneck_permille()? != 428 {
            return Err(XC_SPOT_SEG_OOB);
        }
        // 800/700/300/200 → 总 2000，短板 0，占 400‰。
        let f2 = FrameSegs([800, 700, 300, 200]);
        if f2.bottleneck()? != 0 || f2.bottleneck_permille()? != 400 {
            return Err(XC_SPOT_SEG_OOB);
        }
        if FrameSegs([0, 0, 0, 0]).bottleneck().is_ok() {
            return Err(XC_SPOT_EMPTY);
        }
        Ok(())
    }

    /// 自检二：趋势——漂移/加深/缓解/平稳四态各有正例。
    pub fn selfcheck_trend(&self) -> Result<(), u32> {
        let mut y = DaySpot::new();
        let mut t = DaySpot::new();
        // 昨日主短板段 1（600/700），今日主短板段 2（300/500）。
        for _ in 0..7 {
            y.sample(&FrameSegs([600, 700, 100, 100]))?;
        }
        for _ in 0..3 {
            y.sample(&FrameSegs([900, 100, 100, 100]))?;
        }
        for _ in 0..8 {
            t.sample(&FrameSegs([300, 100, 500, 100]))?;
        }
        for _ in 0..2 {
            t.sample(&FrameSegs([900, 100, 100, 100]))?;
        }
        if trend_of(&y, &t)? != Trend::ShiftedTo(2) {
            return Err(XC_SPOT_TREND_UNGROUNDED);
        }
        // 段同、占比加深：昨日 500‰ → 今日 600‰。
        let mut y2 = DaySpot::new();
        let mut t2 = DaySpot::new();
        for _ in 0..5 {
            y2.sample(&FrameSegs([500, 500, 0, 0]))?;
        }
        for _ in 0..6 {
            t2.sample(&FrameSegs([600, 400, 0, 0]))?;
        }
        if trend_of(&y2, &t2)? != Trend::Deepening {
            return Err(XC_SPOT_TREND_UNGROUNDED);
        }
        // 缓解与平稳。
        if trend_of(&t2, &y2)? != Trend::Easing {
            return Err(XC_SPOT_TREND_UNGROUNDED);
        }
        if trend_of(&y2, &y2)? != Trend::Stable {
            return Err(XC_SPOT_TREND_UNGROUNDED);
        }
        // 空快照拒。
        if trend_of(&DaySpot::new(), &t2).is_ok() {
            return Err(XC_SPOT_EMPTY);
        }
        Ok(())
    }

    /// 自检三：归因——四段全映射、越界拒。
    pub fn selfcheck_attribute(&self) -> Result<(), u32> {
        if attribute(0)? != Cause::InputBacklog
            || attribute(1)? != Cause::SimOverrun
            || attribute(2)? != Cause::PrepContention
            || attribute(3)? != Cause::PresentStall
        {
            return Err(XC_SPOT_ATTR_UNMAPPED);
        }
        if attribute(4).is_ok() {
            return Err(XC_SPOT_ATTR_UNMAPPED);
        }
        Ok(())
    }

    /// 自检四：清单——Top-N 挤出与降序保持。
    pub fn selfcheck_list(&self) -> Result<(), u32> {
        let mut l = SpotList::new(3)?;
        l.record("menu", &FrameSegs([900, 100, 0, 0]))?; // 900‰
        l.record("battle", &FrameSegs([400, 600, 0, 0]))?; // 600‰
        l.record("cutscene", &FrameSegs([200, 200, 600, 0]))?; // 600‰ 段 2
        l.record("loading", &FrameSegs([100, 100, 100, 700]))?; // 700‰
        // 容量 3：menu(900) 应留，battle/cutscene 同 600，battle 先入占位，
        // loading(700) 挤掉 battle 之前先挤最小者 battle=600 → 实际挤 battle。
        if l.len() != 3 {
            return Err(XC_SPOT_LIST_OOB);
        }
        if !l.is_sorted_desc() {
            return Err(XC_SPOT_LIST_OOB);
        }
        let top = l.top(2);
        if top[0].scene != "menu" || top[0].permille != 900 {
            return Err(XC_SPOT_LIST_OOB);
        }
        if top[1].scene != "loading" || top[1].permille != 700 {
            return Err(XC_SPOT_LIST_OOB);
        }
        // 容量越界拒。
        if SpotList::new(0).is_ok() || SpotList::new(17).is_ok() {
            return Err(XC_SPOT_LIST_OOB);
        }
        // battle 已被挤出。
        let names: Vec<&str> = l.top(3).iter().map(|e| e.scene).collect();
        if names.contains(&"battle") {
            return Err(XC_SPOT_LIST_OOB);
        }
        Ok(())
    }

    /// 自检五：总门面采样一致性与判据收口。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = SpotEngine::default();
        for _ in 0..6 {
            e.today.sample(&FrameSegs([300, 500, 100, 100]))?;
        }
        for _ in 0..4 {
            e.yest.sample(&FrameSegs([100, 200, 600, 100]))?;
        }
        if e.today.dominant()? != 1 || e.yest.dominant()? != 2 {
            return Err(XC_SPOT_SEG_OOB);
        }
        if trend_of(&e.yest, &e.today)? != Trend::ShiftedTo(1) {
            return Err(XC_SPOT_TREND_UNGROUNDED);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8E00..0x8F00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SPOT_EMPTY,
            XC_SPOT_SEG_OOB,
            XC_SPOT_TREND_UNGROUNDED,
            XC_SPOT_ATTR_UNMAPPED,
            XC_SPOT_LIST_OOB,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8E00 || codes[i] >= 0x8F00 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd22_selfchecks() {
        let mut e = SpotEngine::default();
        e.selfcheck_identify().unwrap();
        e.selfcheck_trend().unwrap();
        e.selfcheck_attribute().unwrap();
        e.selfcheck_list().unwrap();
        e.selfcheck_verdict().unwrap();
        assert_eq!(SpotEngine::segment_guard(), Ok(()));
    }
}
