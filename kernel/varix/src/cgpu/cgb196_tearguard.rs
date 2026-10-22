//! CGPU-F0196 · 并行合成的帧撕裂防护（判据段 0x05E0）
//! 多线程合成的撕裂防护：帧完整性保证（一帧的所有瓦片必须来自同一
//! 帧数据——瓦片间帧号一致性校验）、帧边界同步（帧切换时全部并行
//! 任务的统一屏障——防止上一帧任务写入下一帧缓冲）、撕裂检测
//! （F0081 花屏检测的并行特化：帧边界水印校验——并行引入的新撕裂
//! 模式检测）。判据：帧完整性校验、边界同步、撕裂检测、并行合成
//! 对拍、零撕裂断言。
//!
//! 诊断码段 0x05E0（本模块独占：0x05E0~0x05EF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05E0;

/// 帧水印模数：帧号写入水印时取模（64 帧循环）。
pub const WATERMARK_MOD: u32 = 64;

/// 屏障超时（轮询轮数）：并行合成统一屏障的超时上限。
pub const BARRIER_TIMEOUT_ROUNDS: u32 = 100_000;

/// 瓦片完成记录（判据「帧完整性校验」原子量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileCommit {
    /// 瓦片号。
    pub tile: u32,
    /// 该瓦片合成所用帧号。
    pub frame: u32,
    /// 帧水印（帧号 mod 64——像素级水印回读值）。
    pub watermark: u32,
}

impl TileCommit {
    /// 合法性：水印与帧号自洽。
    pub fn validate(&self) -> Result<(), u16> {
        if self.tile == 0 || self.frame == 0 {
            return Err(SEG_BASE);
        }
        if self.watermark != self.frame % WATERMARK_MOD {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 帧完整性账（判据「帧完整性校验」容器）：一帧内全部瓦片提交，
/// 帧号必须同源。
#[derive(Default)]
pub struct FrameIntegrityBook {
    commits: Vec<TileCommit>,
}

impl FrameIntegrityBook {
    /// 登记一瓦片提交。
    pub fn commit(&mut self, c: TileCommit) -> Result<(), u16> {
        c.validate()?;
        if self.commits.iter().any(|x| x.tile == c.tile) {
            return Err(SEG_BASE + 2);
        }
        self.commits.push(c);
        Ok(())
    }
    /// 已登记瓦片数。
    pub fn count(&self) -> usize {
        self.commits.len()
    }
    /// 帧一致性校验（判据主断言）：全部瓦片同帧号返回该帧号，否则
    /// 报撕裂错误（携带错误码 SEG_BASE+3）。
    pub fn assert_single_frame(&self) -> Result<u32, u16> {
        if self.commits.is_empty() {
            return Err(SEG_BASE + 4);
        }
        let f = self.commits[0].frame;
        if self.commits.iter().any(|c| c.frame != f) {
            return Err(SEG_BASE + 3);
        }
        Ok(f)
    }
    /// 清空（下一帧复用账本）。
    pub fn reset(&mut self) {
        self.commits.clear();
    }
}

/// 水印注入值计算（合成器写像素时的水印域）。
pub fn watermark_for(frame: u32) -> u32 {
    frame % WATERMARK_MOD
}

/// 撕裂检出（判据「撕裂检测」主判据）：回读一组瓦片水印，与期望
/// 帧号比对——任何不一致即检出撕裂，返回 (检出数, 总数)。
pub fn detect_tear(
    read_marks: &[TileCommit],
    expect_frame: u32,
) -> Result<(usize, usize), u16> {
    if read_marks.is_empty() {
        return Err(SEG_BASE + 5);
    }
    if expect_frame == 0 {
        return Err(SEG_BASE + 6);
    }
    let expect_mark = watermark_for(expect_frame);
    let torn = read_marks
        .iter()
        .filter(|c| c.watermark != expect_mark)
        .count();
    Ok((torn, read_marks.len()))
}

/// 零撕裂断言（判据「零撕裂断言」）：全组水印一致才通过。
pub fn assert_zero_tear(read_marks: &[TileCommit], expect_frame: u32) -> Result<(), u16> {
    let (torn, _) = detect_tear(read_marks, expect_frame)?;
    if torn > 0 {
        return Err(SEG_BASE + 7);
    }
    Ok(())
}

/// 统一屏障（判据「边界同步」载体）：帧切换时全部并行任务到达
/// 后一起放行。
pub struct FrameBarrier {
    /// 需要到达的参与方数。
    pub parties: u32,
    /// 已到达数。
    arrived: u32,
    /// 已放行代数（帧切换次数）。
    generation: u32,
}

impl FrameBarrier {
    /// 建屏障。
    pub fn new(parties: u32) -> Result<Self, u16> {
        if parties == 0 {
            return Err(SEG_BASE + 8);
        }
        Ok(Self { parties, arrived: 0, generation: 0 })
    }
    /// 一方到达：最后一名到达者负责翻代并放行全员。返回 (是否最后
    /// 一方, 当前代数)。
    pub fn arrive(&mut self) -> Result<(bool, u32), u16> {
        self.arrived += 1;
        if self.arrived > self.parties {
            return Err(SEG_BASE + 9);
        }
        if self.arrived == self.parties {
            self.arrived = 0;
            self.generation += 1;
            return Ok((true, self.generation));
        }
        Ok((false, self.generation))
    }
    /// 当前代数。
    pub fn gen(&self) -> u32 {
        self.generation
    }
}

/// 帧边界同步演练（判据「边界同步」主判据）：parties 个并行任务
/// 逐一到达（模拟任意到达序），全员到达后统一翻代；重复 N 帧无
/// 超时即通过。
pub fn barrier_frame_switch(b: &mut FrameBarrier, frames: u32) -> Result<(), u16> {
    if frames == 0 {
        return Err(SEG_BASE + 10);
    }
    for f in 0..frames {
        for _ in 0..b.parties {
            let (last, gen) = b.arrive()?;
            if last && gen != f + 1 {
                return Err(SEG_BASE + 11);
            }
        }
        if b.gen() != f + 1 {
            return Err(SEG_BASE + 12);
        }
    }
    Ok(())
}

/// 并行合成对拍（判据「并行合成对拍」）：串行合成指纹 vs 并行合成
/// 指纹——并行只改执行序不改结果（指纹必等）。
pub fn composite_par_compare(fp_serial: u64, fp_parallel: u64) -> Result<bool, u16> {
    if fp_serial == 0 || fp_parallel == 0 {
        return Err(SEG_BASE + 13);
    }
    Ok(fp_serial == fp_parallel)
}

/// 瓦片-帧台账指纹（可审计面）。
pub fn integrity_book_fp(b: &FrameIntegrityBook) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for c in &b.commits {
        acc = acc.wrapping_mul(0x100000001b3) ^ c.tile as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ c.frame as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ c.watermark as u64;
    }
    acc
}

/// 撕裂事件台账（判据「撕裂检测」遥测面）：记录每次撕裂检出的
/// 模式，供上游诊断并行引入的新撕裂类型。
#[derive(Default)]
pub struct TearLedger {
    /// (帧号, 撕裂瓦片数) 序列。
    events: Vec<(u32, u32)>,
}

impl TearLedger {
    /// 记录一次检出。
    pub fn record(&mut self, frame: u32, torn_tiles: u32) -> Result<(), u16> {
        if frame == 0 || torn_tiles == 0 {
            return Err(SEG_BASE + 14);
        }
        if self.events.len() >= 4096 {
            self.events.remove(0);
        }
        self.events.push((frame, torn_tiles));
        Ok(())
    }
    /// 撕裂总事件数。
    pub fn count(&self) -> usize {
        self.events.len()
    }
    /// 撕裂模式聚类：连续帧均出现撕裂数相同的模式 → 系统性撕裂
    /// （并行调度缺陷）；偶发 → 竞态。返回 (连续模式数, 偶发数)。
    pub fn classify_patterns(&self) -> Result<(u32, u32), u16> {
        if self.events.len() < 3 {
            return Err(SEG_BASE + 15);
        }
        let mut systematic = 0u32;
        let mut sporadic = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            let mut j = i + 1;
            while j < self.events.len()
                && self.events[j].1 == self.events[i].1
                && self.events[j].0 == self.events[j - 1].0 + 1
            {
                j += 1;
            }
            if j - i >= 3 {
                systematic += 1;
            } else {
                sporadic += (j - i) as u32;
            }
            i = j;
        }
        Ok((systematic, sporadic))
    }
}

/// 跨帧缓冲写防护（独立函数面）：帧 N 的任务欲写帧 N+1 的缓冲时
/// 拒绝（判据「边界同步」语义核心）。返回 Ok(()) 表示写合法。
pub fn guard_cross_frame_write(writer_frame: u32, buffer_frame: u32) -> Result<(), u16> {
    if writer_frame == 0 || buffer_frame == 0 {
        return Err(SEG_BASE + 16);
    }
    if buffer_frame > writer_frame {
        return Err(SEG_BASE + 17);
    }
    Ok(())
}

/// 完整性-屏障联合审计（独立函数面）：帧合成完成时双验——瓦片账
/// 全员同帧 + 屏障代数等于帧序号，才允许呈现。
pub fn present_admission(
    book: &FrameIntegrityBook,
    barrier_gen: u32,
    frame_no: u32,
) -> Result<(), u16> {
    let f = book.assert_single_frame()?;
    if f != frame_no {
        return Err(SEG_BASE + 18);
    }
    if barrier_gen != frame_no {
        return Err(SEG_BASE + 19);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tc(tile: u32, frame: u32) -> TileCommit {
        TileCommit { tile, frame, watermark: watermark_for(frame) }
    }
    #[test]
    fn single_frame_ok() {
        let mut b = FrameIntegrityBook::default();
        for t in 1..=4 {
            b.commit(tc(t, 7)).unwrap();
        }
        assert_eq!(b.assert_single_frame().unwrap(), 7);
    }
    #[test]
    fn mixed_frames_rejected() {
        let mut b = FrameIntegrityBook::default();
        b.commit(tc(1, 7)).unwrap();
        b.commit(tc(2, 8)).unwrap();
        assert_eq!(b.assert_single_frame().unwrap_err(), SEG_BASE + 3);
    }
    #[test]
    fn watermark_modulo() {
        assert_eq!(watermark_for(1), 1);
        assert_eq!(watermark_for(64), 0);
        assert_eq!(watermark_for(65), 1);
    }
    #[test]
    fn tear_detection_counts() {
        let mut reads = vec![tc(1, 10), tc(2, 10), tc(3, 10)];
        reads[2].watermark = 3; // 模拟上一帧残留。
        let (torn, total) = detect_tear(&reads, 10).unwrap();
        assert_eq!((torn, total), (1, 3));
        assert!(assert_zero_tear(&reads, 10).is_err());
        let clean = vec![tc(1, 10), tc(2, 10)];
        assert!(assert_zero_tear(&clean, 10).is_ok());
    }
    #[test]
    fn barrier_switches_frames() {
        let mut b = FrameBarrier::new(3).unwrap();
        barrier_frame_switch(&mut b, 5).unwrap();
        assert_eq!(b.gen(), 5);
    }
    #[test]
    fn par_composite_semantics() {
        assert!(composite_par_compare(0x1234_5678, 0x1234_5678).unwrap());
        assert!(!composite_par_compare(0x1234_5678, 0x8765_4321).unwrap());
    }
    #[test]
    fn tear_pattern_classification() {
        let mut led = TearLedger::default();
        for f in 10..=13 {
            led.record(f, 2).unwrap();
        }
        led.record(20, 1).unwrap();
        led.record(21, 1).unwrap();
        let (sys, spo) = led.classify_patterns().unwrap();
        assert_eq!((sys, spo), (1, 2));
    }
    #[test]
    fn cross_frame_write_guard() {
        assert!(guard_cross_frame_write(5, 5).is_ok());
        assert!(guard_cross_frame_write(5, 4).is_ok());
        assert!(guard_cross_frame_write(5, 6).is_err());
    }
    #[test]
    fn present_admission_double_gate() {
        let mut b = FrameIntegrityBook::default();
        for t in 1..=2 {
            b.commit(tc(t, 9)).unwrap();
        }
        assert!(present_admission(&b, 9, 9).is_ok());
        assert!(present_admission(&b, 8, 9).is_err());
    }
}
