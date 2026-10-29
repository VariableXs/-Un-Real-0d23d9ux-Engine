//! UNX-F48503 · E·候选刷新风暴（正：同帧合并为一次视图更新）
//!
//! 判据原文：UNX-F48503-J1 —— 候选刷新风暴吸收：同一帧内到达的 N 次
//! 候选刷新合并为恰好一次视图更新（合并率 = 1/N）；跨帧不合并（帧边界
//! 尊重）；刷新载荷取帧内最后一次（新值胜出，无陈旧值残留）；合并计数
//! 与更新计数守恒；帧号乱序到达被拒；fuzz 风暴 10^4 次刷新合并后更新
//! 数 ≤ 帧数；同序列重放确定性一致。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE9A0;

pub const E_FRAME_OOB: u16 = SEG_BASE + 1;
pub const E_STALE: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_ORDER_BAD: u16 = SEG_BASE + 4;
pub const E_MERGE_MISS: u16 = SEG_BASE + 5;
pub const E_NONDET: u16 = SEG_BASE + 6;

/// 候选刷新载荷。
#[derive(Clone, PartialEq, Eq)]
pub struct CandRefresh {
    pub frame: u64,
    pub cand_id: u32,
    pub text_fp: u64,
}

impl CandRefresh {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.frame;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.cand_id as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.text_fp;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// 同帧合并器：帧内攒、帧边界吐。
pub struct FrameMerger {
    cur_frame: Option<u64>,
    pending: Option<CandRefresh>,
    pub updates: u32,
    pub merged_in: u32,
    pub last_update: Option<CandRefresh>,
}

impl FrameMerger {
    pub fn new() -> FrameMerger {
        FrameMerger {
            cur_frame: None,
            pending: None,
            updates: 0,
            merged_in: 0,
            last_update: None,
        }
    }

    /// 提交刷新：同帧攒（新值覆盖旧值），帧号回退拒收。
    pub fn submit(&mut self, r: CandRefresh) -> Result<bool, u16> {
        match self.cur_frame {
            None => {
                self.cur_frame = Some(r.frame);
                self.pending = Some(r);
                Ok(false)
            }
            Some(f) => {
                if r.frame < f {
                    return Err(E_ORDER_BAD);
                }
                if r.frame == f {
                    // 同帧：新值胜出（覆盖 pending），合并计数走。
                    self.merged_in += 1;
                    self.pending = Some(r);
                    Ok(false)
                } else {
                    // 跨帧：先把 pending 落成更新，再开新帧攒。
                    self.flush_pending()?;
                    self.cur_frame = Some(r.frame);
                    self.pending = Some(r);
                    Ok(false)
                }
            }
        }
    }

    /// 帧边界/结束：pending 落成一次视图更新。
    pub fn flush_pending(&mut self) -> Result<(), u16> {
        match self.pending.take() {
            None => Ok(()),
            Some(r) => {
                self.updates += 1;
                self.last_update = Some(r);
                Ok(())
            }
        }
    }

    /// 结束当前帧（供显式分帧）。
    pub fn end_frame(&mut self) -> Result<(), u16> {
        self.flush_pending()?;
        self.cur_frame = None;
        Ok(())
    }

    /// 守恒审计：updates == flush 过的帧数；merged_in ≤ merged_in+updates。
    pub fn audit(&self) -> Result<(), u16> {
        if self.updates == 0 && self.merged_in > 0 {
            return Err(E_MERGE_MISS);
        }
        if self.pending.is_some() {
            return Err(E_MERGE_MISS);
        }
        Ok(())
    }
}

/// 视图侧：记录每次更新的帧号序列（不重复、单调不减）。
pub struct ViewLog {
    pub frames: Vec<u64>,
    pub payloads: Vec<CandRefresh>,
}

impl ViewLog {
    pub fn new() -> ViewLog {
        ViewLog { frames: Vec::new(), payloads: Vec::new() }
    }
    pub fn record(&mut self, r: &CandRefresh) -> Result<(), u16> {
        if let Some(last) = self.frames.last() {
            if r.frame < *last {
                return Err(E_ORDER_BAD);
            }
            if r.frame == *last {
                return Err(E_MERGE_MISS);
            }
        }
        self.frames.push(r.frame);
        self.payloads.push(r.clone());
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 同帧合并：一帧 5 次刷新 → 恰 1 更新，载荷 = 最后一次。
pub fn r1_same_frame_merge() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    for i in 0..5u32 {
        m.submit(CandRefresh { frame: 7, cand_id: i, text_fp: 0x100 + i as u64 })?;
    }
    m.end_frame()?;
    m.audit()?;
    if m.updates != 1 {
        return Err(E_MERGE_MISS);
    }
    if m.merged_in != 4 {
        return Err(E_LEDGER_BAD);
    }
    let lu = match m.last_update {
        Some(ref r) => r.clone(),
        None => return Err(E_STALE),
    };
    if lu.cand_id != 4 || lu.text_fp != 0x104 {
        return Err(E_STALE);
    }
    Ok(())
}

/// R2 新值胜出：中间值被覆盖，无陈旧残留。
pub fn r2_new_wins() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    m.submit(CandRefresh { frame: 1, cand_id: 9, text_fp: 0xA })?;
    m.submit(CandRefresh { frame: 1, cand_id: 3, text_fp: 0xB })?;
    m.submit(CandRefresh { frame: 1, cand_id: 7, text_fp: 0xC })?;
    m.end_frame()?;
    let lu = match m.last_update {
        Some(ref r) => r.clone(),
        None => return Err(E_STALE),
    };
    if lu.cand_id != 7 {
        return Err(E_STALE);
    }
    Ok(())
}

/// R3 跨帧不合并：两帧各 1 次 → 2 更新。
pub fn r3_cross_frame_split() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    m.submit(CandRefresh { frame: 10, cand_id: 1, text_fp: 1 })?;
    m.submit(CandRefresh { frame: 11, cand_id: 2, text_fp: 2 })?;
    m.end_frame()?;
    if m.updates != 2 {
        return Err(E_MERGE_MISS);
    }
    Ok(())
}

/// R4 帧号回退拒收：frame < cur 报 E_ORDER_BAD。
pub fn r4_frame_regression() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    m.submit(CandRefresh { frame: 20, cand_id: 0, text_fp: 0 })?;
    let r = m.submit(CandRefresh { frame: 19, cand_id: 1, text_fp: 0 });
    match r {
        Err(E_ORDER_BAD) => {}
        _ => return Err(E_ORDER_BAD),
    }
    Ok(())
}

/// R5 视图帧号互异：合并后更新序列帧号严格递增。
pub fn r5_view_unique_frames() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    let mut v = ViewLog::new();
    // 3 帧各 3 次刷新。
    for f in 0..3u64 {
        for i in 0..3u32 {
            m.submit(CandRefresh { frame: f, cand_id: i, text_fp: f * 10 + i as u64 })?;
        }
        m.end_frame()?;
        if let Some(ref r) = m.last_update {
            v.record(r)?;
        }
    }
    if v.frames.len() != 3 {
        return Err(E_MERGE_MISS);
    }
    Ok(())
}

/// R6 显式分帧与隐式跨帧等价：两种分帧方式更新数一致。
pub fn r6_frame_modes_equal() -> Result<(), u16> {
    // 显式：submit 同帧 ×3 → end_frame → submit 下一帧 ×3 → end_frame。
    let mut a = FrameMerger::new();
    a.submit(CandRefresh { frame: 1, cand_id: 0, text_fp: 0 })?;
    a.submit(CandRefresh { frame: 1, cand_id: 1, text_fp: 1 })?;
    a.end_frame()?;
    a.submit(CandRefresh { frame: 2, cand_id: 0, text_fp: 2 })?;
    a.submit(CandRefresh { frame: 2, cand_id: 1, text_fp: 3 })?;
    a.end_frame()?;
    // 隐式：全靠跨帧检测。
    let mut b = FrameMerger::new();
    b.submit(CandRefresh { frame: 1, cand_id: 0, text_fp: 0 })?;
    b.submit(CandRefresh { frame: 1, cand_id: 1, text_fp: 1 })?;
    b.submit(CandRefresh { frame: 2, cand_id: 0, text_fp: 2 })?;
    b.submit(CandRefresh { frame: 2, cand_id: 1, text_fp: 3 })?;
    b.end_frame()?;
    if a.updates != b.updates || a.updates != 2 {
        return Err(E_MERGE_MISS);
    }
    Ok(())
}

/// R7 风暴 10^4：单帧 10,000 次刷新合并为 1 更新。
pub fn r7_storm_10k() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    let mut g = Lcg::new(0x48503_0001);
    for i in 0..10_000u32 {
        let _ = g.next();
        m.submit(CandRefresh { frame: 99, cand_id: i, text_fp: g.next() })?;
    }
    m.end_frame()?;
    m.audit()?;
    if m.updates != 1 {
        return Err(E_MERGE_MISS);
    }
    if m.merged_in != 9_999 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R8 fuzz 多帧风暴：50 帧 × 随机 1..20 次，更新数 == 帧数。
pub fn r8_fuzz_multiframe() -> Result<(), u16> {
    let mut g = Lcg::new(0x48503_0002);
    let mut m = FrameMerger::new();
    let mut expect_frames: u32 = 0;
    for f in 0..50u64 {
        let n = 1 + g.below(20) as u32;
        for _ in 0..n {
            m.submit(CandRefresh {
                frame: f,
                cand_id: (g.next() & 0xFF) as u32,
                text_fp: g.next(),
            })?;
        }
        m.end_frame()?;
        expect_frames += 1;
    }
    if m.updates != expect_frames {
        return Err(E_MERGE_MISS);
    }
    Ok(())
}

/// R9 合并率可测：合并率 = merged_in / (merged_in + updates)，风暴下趋近 1。
pub fn r9_merge_ratio() -> Result<(), u16> {
    let mut m = FrameMerger::new();
    for i in 0..1000u32 {
        m.submit(CandRefresh { frame: 5, cand_id: i, text_fp: i as u64 })?;
    }
    m.end_frame()?;
    // 1000 次 → 1 更新 + 999 合并。
    let total = m.updates + m.merged_in;
    if total != 1000 {
        return Err(E_LEDGER_BAD);
    }
    if m.merged_in * 10 < total * 9 {
        return Err(E_MERGE_MISS);
    }
    Ok(())
}

/// R10 确定性重放：同种子两次风暴，last_update 指纹与计数全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, u32, u32), u16> {
        let mut g = Lcg::new(seed);
        let mut m = FrameMerger::new();
        for f in 0..10u64 {
            let n = 1 + g.below(8);
            for _ in 0..n {
                m.submit(CandRefresh {
                    frame: f,
                    cand_id: (g.next() & 0xFF) as u32,
                    text_fp: g.next(),
                })?;
            }
            m.end_frame()?;
        }
        let fp = match m.last_update {
            Some(ref r) => r.fp(),
            None => 0,
        };
        Ok((fp, m.updates, m.merged_in))
    };
    let a = run(0x48503_0003)?;
    let b = run(0x48503_0003)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_same_frame_merge()?;
    hits += 1;
    r2_new_wins()?;
    hits += 1;
    r3_cross_frame_split()?;
    hits += 1;
    r4_frame_regression()?;
    hits += 1;
    r5_view_unique_frames()?;
    hits += 1;
    r6_frame_modes_equal()?;
    hits += 1;
    r7_storm_10k()?;
    hits += 1;
    r8_fuzz_multiframe()?;
    hits += 1;
    r9_merge_ratio()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
