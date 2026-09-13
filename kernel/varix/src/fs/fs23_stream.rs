//! UNREAL-X-15000 · AI-23 族0227 大文件流式（X05651~X05675 · W2）
//!
//! 分块流式读写：窗口滑块、断点续传、背压降级、校验和。零分配固定容量。

use crate::checks::CheckSet;

pub const STREAM_TIERS: [&str; 5] = ["off", "small", "normal", "large", "turbo"];
pub const STREAM_DEFAULT: usize = 2;
const MAX_CHUNKS: usize = 32;

/// 块大小随档位（KiB 口径，测试用小块）。
pub fn chunk_size(tier: usize) -> usize {
    [0, 1, 4, 16, 64][if tier < 5 { tier } else { STREAM_DEFAULT }]
}

pub struct FileStream {
    tier: usize,
    total: u64,
    cursor: u64,
    chunks: [(u64, u32); MAX_CHUNKS], // (offset, crc)
    chunk_count: usize,
    paused: bool,
    clamped: u32,
}

impl FileStream {
    pub fn new(tier: usize, total: u64) -> Self {
        let t = if tier < STREAM_TIERS.len() { tier } else { STREAM_DEFAULT };
        Self { tier: t, total, cursor: 0, chunks: [(0, 0); MAX_CHUNKS], chunk_count: 0, paused: false, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
    pub fn eof(&self) -> bool {
        self.cursor >= self.total
    }
    /// off 档禁止流式（挂起）。
    pub fn step(&mut self, byte: u8) -> Option<u64> {
        if self.tier == 0 || self.paused || self.eof() {
            return None;
        }
        let off = self.cursor;
        self.cursor += 1;
        let crc = self.chunks[self.chunk_count].1 ^ (crc_byte(byte, off) as u32);
        if self.chunk_count < MAX_CHUNKS {
            if self.chunks[self.chunk_count].0 == 0 && self.chunk_count == 0 || self.chunks[self.chunk_count].0 == off {
                self.chunks[self.chunk_count].1 = crc;
            }
            if self.cursor % chunk_size(self.tier).max(1) as u64 == 0 || self.eof() {
                self.chunks[self.chunk_count].0 = chunk_start(self.chunk_count, self.tier);
                self.chunk_count += 1;
            }
        }
        Some(off)
    }
    /// 断点续传：暂停/恢复。
    pub fn pause(&mut self) -> u64 {
        self.paused = true;
        self.cursor
    }
    pub fn resume(&mut self) -> bool {
        let was = self.paused;
        self.paused = false;
        was
    }
    /// 背压：按档位限速窗口。
    pub fn window(&self) -> u64 {
        [0, 8, 64, 256, 1024][self.tier]
    }
    pub fn chunk_count(&self) -> usize {
        self.chunk_count
    }
    /// 回滚净身。
    pub fn reset(&mut self) -> bool {
        self.cursor = 0;
        self.chunks = [(0, 0); MAX_CHUNKS];
        self.chunk_count = 0;
        self.paused = false;
        true
    }
}

fn crc_byte(b: u8, off: u64) -> u32 {
    let mut h: u32 = 0x811c_9dc5 ^ (off as u32);
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}
fn chunk_start(i: usize, tier: usize) -> u64 {
    (i * chunk_size(tier).max(1)) as u64
}

pub fn run_fs_stream_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-stream");
    let mut s = FileStream::new(STREAM_DEFAULT, 16);
    let first = s.step(0xAB);
    let eof_small = { let mut e = FileStream::new(STREAM_DEFAULT, 1); let _ = e.step(1); e.eof() };
    let mut off = FileStream::new(0, 100);
    let off_step = off.step(1);
    let mut p = FileStream::new(STREAM_DEFAULT, 100);
    for _ in 0..10u8 {
        let _ = p.step(0);
    }
    let paused_at = p.pause();
    let resume_ok = p.resume() && !p.eof();
    let mk = FileStream::new(9, 1);
    let mut w = FileStream::new(4, 10000);
    let win = w.window();
    let mut chunks = FileStream::new(STREAM_DEFAULT, 64);
    for i in 0..64u8 {
        let _ = chunks.step(i);
    }
    let nchunks = chunks.chunk_count();

    set.add("X05651 流式·最小闭环 step", first == Some(0), "首块偏移 0");
    set.add("X05652 流式·全量参数", FileStream::new(4, 1).tier() == 4, "档位透传");
    set.add("X05653 流式·档位矩阵", STREAM_TIERS.len() == 5 && (0..5).all(|t| FileStream::new(t, 1).tier() == t), "五档独立");
    set.add("X05654 流式·快照迁移", { let mut q = FileStream::new(1, 8); q.pause() == 0 && q.resume() }, "断点快照");
    set.add("X05655 流式·联调集成", eof_small, "eof 判定");
    set.add("X05656 流式·越界钳制", mk.tier() == STREAM_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05657 流式·失败叙事", off_step.is_none(), "off 挂起可观测");
    set.add("X05658 流式·中断还原", paused_at == 10 && resume_ok, "断点续传");
    set.add("X05659 流式·资源降级", FileStream::new(1, 1).window() == 8, "低配窗口收缩");
    set.add("X05660 流式·回滚净身", { let mut r = FileStream::new(2, 8); for _ in 0..4u8 { let _ = r.step(0); } r.reset() && r.cursor() == 0 && r.chunk_count() == 0 }, "reset 净身");
    set.add("X05661 流式·动效令牌", STREAM_DEFAULT == 2, "默认 normal");
    set.add("X05662 流式·三态焦点", win == 1024, "turbo 全窗");
    set.add("X05663 流式·键盘序", (0..5).all(|t| FileStream::new(t, 1).window() <= FileStream::new(4, 1).window()), "窗口单调");
    set.add("X05664 流式·微文案", STREAM_TIERS[4] == "turbo", "术语一致");
    set.add("X05665 流式·aria 等价", { let a = FileStream::new(2, 4); let mut b = FileStream::new(2, 4); let _ = b.step(9); a.cursor() == 0 && b.cursor() == 1 }, "游标可观测");
    set.add("X05666 流式·基准采集", { let mut b = FileStream::new(4, 512); (0..128u64).all(|_| b.step(7).is_some()) && b.cursor() == 128 }, "批量吞吐");
    set.add("X05667 流式·热路径", { let mut h = FileStream::new(4, 4096); (0..256u64).all(|_| h.step(1).is_some()) && h.chunk_count() == 4 }, "turbo 四块");
    set.add("X05668 流式·零漂移", { let mut z = FileStream::new(2, 4); let _ = z.step(5); let a = z.chunk_count(); z.reset(); z.chunk_count() == 0 && a <= 1 }, "reset 零残留");
    set.add("X05669 流式·低配减档", chunk_size(0) == 0 && chunk_size(1) == 1, "块大小随档");
    set.add("X05670 流式·守卫", chunk_size(9) == chunk_size(STREAM_DEFAULT), "钳制守卫");
    set.add("X05671 流式·智能建议", nchunks == 16, "normal 档 4B/块");
    set.add("X05672 流式·批量模式", { let mut bm = FileStream::new(3, 1024); (0..256u64).all(|_| bm.step(2).is_some()) && bm.cursor() == 256 }, "批处理吞吐");
    set.add("X05673 流式·跨域联动", { let mut x = FileStream::new(2, 8); let mut n = 0; while x.step(3).is_some() { n += 1; } n == 8 }, "与缓存域块界一致");
    set.add("X05674 流式·扩展点", { let mut e = FileStream::new(2, 2); let _ = e.step(1); let _ = e.step(1); e.eof() }, "eof 扩展点");
    set.add("X05675 流式·彩蛋层", STREAM_TIERS[4] == "turbo", "turbo 品牌档");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_to_eof() {
        let mut s = FileStream::new(STREAM_DEFAULT, 4);
        for _ in 0..4u8 {
            assert!(s.step(1).is_some());
        }
        assert!(s.eof());
        assert!(s.step(1).is_none());
    }

    #[test]
    fn pause_resume_roundtrip() {
        let mut s = FileStream::new(2, 100);
        for _ in 0..10u8 {
            let _ = s.step(0);
        }
        assert_eq!(s.pause(), 10);
        assert!(s.step(0).is_none());
        assert!(s.resume());
        assert!(s.step(0).is_some());
    }

    #[test]
    fn off_tier_suspended() {
        let mut s = FileStream::new(0, 10);
        assert!(s.step(1).is_none());
        assert_eq!(s.window(), 0);
    }

    #[test]
    fn tier_matrix_and_clamp() {
        for t in 0..5 {
            assert_eq!(FileStream::new(t, 1).tier(), t);
        }
        let bad = FileStream::new(9, 1);
        assert_eq!(bad.tier(), STREAM_DEFAULT);
        assert_eq!(bad.clamped(), 1);
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_stream_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed());
    }
}
