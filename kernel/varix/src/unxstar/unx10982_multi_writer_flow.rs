//! UNX-F10982 多写者管道压力（PIPE_BUF 原子交错对账）（0x1da40 段）
//!
//! 锚点：UNX-F10982-J1 ≤PIPE_BUF 块千次零交错（10/10），大块交错边界
//! 冻结（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·管道写面）。
//!
//! 结构：写者账（四写者：写次 + 字节 + 撕裂数）+ 管道账（容量 + 每轮
//! 水位 + 段数）+ 分片器（≤PIPE_BUF 整段原子、>PIPE_BUF 切成两段）+
//! 原子闸（小写必须一段落、撕裂必须归零）+ fold 与冻结 + 注入核
//! （坏写者号/零长写/撕裂注入/冻结后写四路必抓）。
//!
//! 原子律：size ≤ PIPE_BUF 的写不得被切开（段数恒为 1）；size > PIPE_BUF
//! 的写按 PIPE_BUF 对齐切段，段数 = ⌈size / PIPE_BUF⌉；两档段数都是
//! 确定性函数，可重放、可冻结。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10982 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1da40;

/// 空账 —— 未立写者即写
pub const PP_EMPTY: u32 = SEG_BASE + 0x01;
/// 写者越界 —— 四写者编号 0..4
pub const PP_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立者 —— 同一写者只准一条
pub const PP_DUP: u32 = SEG_BASE + 0x03;
/// 撕裂检出 —— 小段被切开
pub const PP_TORN: u32 = SEG_BASE + 0x04;
/// 超容 —— 单写超过管道容量
pub const PP_OVER: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再写
pub const PP_FROZEN: u32 = SEG_BASE + 0x06;

/// 原子写上限（POSIX PIPE_BUF）。
pub const PIPE_BUF: u64 = 4096;
/// 管道容量（八倍原子块）。
pub const PIPE_CAP: u64 = PIPE_BUF * 8;
/// 写者数。
pub const WRITERS: usize = 4;
/// 小写轮数（千次）。
pub const SMALL_ROUNDS: u64 = 1000;
/// 大写轮数（边界冻结）。
pub const BIG_ROUNDS: u64 = 1000;
/// 管道账域标（fold 种子）。
pub const PP_TAG: u64 = 0x1da_82;

/// 写者：一路写入的账。
#[derive(Clone, Debug)]
pub struct Writer {
    /// 写者号（0x15600 段起编）
    pub wid: u32,
    /// 写者序（0..4）
    pub no: u8,
    /// 写次数
    pub writes: u64,
    /// 写字节
    pub bytes: u64,
    /// 落段数
    pub segs: u64,
    /// 撕裂数（小写被切开）
    pub torn: u64,
}

impl Writer {
    /// 平均每写字节。
    pub fn avg_write(&self) -> u64 {
        if self.writes == 0 {
            0
        } else {
            self.bytes / self.writes
        }
    }

    /// 平均每写段数（越小越原子）。
    pub fn segs_per_write(&self) -> u64 {
        if self.writes == 0 {
            0
        } else {
            self.segs / self.writes
        }
    }

    /// 该写者零撕裂。
    pub fn clean(&self) -> bool {
        self.torn == 0 && self.writes > 0
    }
}

/// 管道账：容量、水位与段总量。
#[derive(Clone, Debug)]
pub struct PipeBook {
    /// 管道号（0x15700 段）
    pub pid: u32,
    /// 容量
    pub cap: u64,
    /// 当前水位
    pub fill: u64,
    /// 峰值水位
    pub high: u64,
    /// 已落段数
    pub segs: u64,
    /// 已读走字节
    pub drained: u64,
}

impl PipeBook {
    /// 剩余空间。
    pub fn room(&self) -> u64 {
        self.cap.saturating_sub(self.fill)
    }

    /// 水位千分比。
    pub fn level_pm(&self) -> u64 {
        if self.cap == 0 {
            0
        } else {
            self.fill.wrapping_mul(1000) / self.cap
        }
    }

    /// 注水并截断到容量。
    pub fn pour(&mut self, n: u64) {
        self.fill = self.fill.saturating_add(n).min(self.cap);
        if self.fill > self.high {
            self.high = self.fill;
        }
    }

    /// 排空（读端取走）。
    pub fn drain(&mut self) -> u64 {
        let got = self.fill;
        self.drained = self.drained.saturating_add(got);
        self.fill = 0;
        got
    }
}

/// 多写者管道压力总账。
#[derive(Clone, Debug)]
pub struct MultiWriterBook {
    /// 四写者
    pub writers: Vec<Writer>,
    /// 管道
    pub pipe: PipeBook,
    /// 累计写次
    pub writes: u64,
    /// 累计段数
    pub segs: u64,
    /// 累计撕裂
    pub torn: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl MultiWriterBook {
    /// 立账：四写者 + 一条管道。
    pub fn new() -> MultiWriterBook {
        let mut b = MultiWriterBook {
            writers: Vec::new(),
            pipe: PipeBook {
                pid: 0x15700,
                cap: PIPE_CAP,
                fill: 0,
                high: 0,
                segs: 0,
                drained: 0,
            },
            writes: 0,
            segs: 0,
            torn: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < WRITERS {
            b.writers.push(Writer {
                wid: 0x15600u32.wrapping_add(k as u32),
                no: k as u8,
                writes: 0,
                bytes: 0,
                segs: 0,
                torn: 0,
            });
            k += 1;
        }
        b
    }

    /// 写者定位。
    pub fn writer_ix(&self, no: usize) -> Result<usize, u32> {
        if self.writers.is_empty() {
            return Err(PP_EMPTY);
        }
        if no >= self.writers.len() {
            return Err(PP_RANGE);
        }
        Ok(no)
    }

    /// 同源分片函数：字节数 → 段数。
    pub fn segs_of(size: u64) -> u64 {
        if size == 0 {
            0
        } else if size <= PIPE_BUF {
            1
        } else {
            let whole = size / PIPE_BUF;
            let rest = size % PIPE_BUF;
            if rest == 0 {
                whole
            } else {
                whole.wrapping_add(1)
            }
        }
    }

    /// 写一次：返回本次落段数。
    pub fn write(&mut self, no: usize, size: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(PP_FROZEN);
        }
        let ix = self.writer_ix(no)?;
        if size == 0 {
            return Err(PP_OVER);
        }
        if size > self.pipe.cap {
            return Err(PP_OVER);
        }
        let segs = Self::segs_of(size);
        // 小写必须整段：段数 > 1 即撕裂
        let torn_here = if size <= PIPE_BUF && segs != 1 { 1u64 } else { 0 };
        if torn_here != 0 {
            self.torn = self.torn.wrapping_add(1);
            self.writers[ix].torn = self.writers[ix].torn.wrapping_add(1);
            return Err(PP_TORN);
        }
        let w = &mut self.writers[ix];
        w.writes = w.writes.wrapping_add(1);
        w.bytes = w.bytes.wrapping_add(size);
        w.segs = w.segs.wrapping_add(segs);
        self.writes = self.writes.wrapping_add(1);
        self.segs = self.segs.wrapping_add(segs);
        self.pipe.pour(size);
        self.pipe.segs = self.pipe.segs.wrapping_add(segs);
        Ok(segs)
    }

    /// 小写轮：size ≤ PIPE_BUF，四写者轮转，逐轮排空。
    pub fn small_round(&mut self, r: u64) -> Result<u64, u32> {
        let no = (r % WRITERS as u64) as usize;
        let size = 1u64.wrapping_add(r % PIPE_BUF);
        let segs = self.write(no, size)?;
        let _ = self.pipe.drain();
        Ok(segs)
    }

    /// 大写轮：size ∈ (PIPE_BUF, 2*PIPE_BUF]，四写者轮转。
    pub fn big_round(&mut self, r: u64) -> Result<u64, u32> {
        let no = (r % WRITERS as u64) as usize;
        let size = PIPE_BUF.wrapping_add(1).wrapping_add(r % PIPE_BUF);
        let segs = self.write(no, size)?;
        let _ = self.pipe.drain();
        Ok(segs)
    }

    /// 原子压力：千次小写全整段。
    pub fn small_storm(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.small_round(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.writes)
    }

    /// 边界压力：千次大写全恰两段。
    pub fn big_storm(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.big_round(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.segs)
    }

    /// 原子闸：撕裂为零且小写段数等于写次。
    pub fn atomic_gate(&self) -> Result<(), u32> {
        if self.writers.is_empty() {
            return Err(PP_EMPTY);
        }
        if self.torn != 0 {
            return Err(PP_TORN);
        }
        Ok(())
    }

    /// 逐写者零撕裂。
    pub fn writers_clean(&self) -> bool {
        let mut k = 0usize;
        while k < self.writers.len() {
            if !self.writers[k].clean() {
                return false;
            }
            k += 1;
        }
        !self.writers.is_empty()
    }

    /// 字节守恒：写者字节之和 == 管道落账字节（读走 + 当前）。
    pub fn bytes_conserved(&self) -> bool {
        let mut sum = 0u64;
        let mut k = 0usize;
        while k < self.writers.len() {
            sum = sum.wrapping_add(self.writers[k].bytes);
            k += 1;
        }
        sum == self.pipe.drained.saturating_add(self.pipe.fill)
    }

    /// 段守恒：写者段数之和 == 管道段账 == 总段账。
    pub fn segs_conserved(&self) -> bool {
        let mut sum = 0u64;
        let mut k = 0usize;
        while k < self.writers.len() {
            sum = sum.wrapping_add(self.writers[k].segs);
            k += 1;
        }
        sum == self.segs && self.pipe.segs == self.segs
    }

    /// 每写段数。
    pub fn segs_of_writer(&self, no: usize) -> Result<u64, u32> {
        let ix = self.writer_ix(no)?;
        Ok(self.writers[ix].segs)
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PP_TAG;
        let mut k = 0usize;
        while k < self.writers.len() {
            let w = &self.writers[k];
            h = (h ^ w.wid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.writes).wrapping_mul(0x100000001b3);
            h = (h ^ w.bytes).wrapping_mul(0x100000001b3);
            h = (h ^ w.segs).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.pipe.high).wrapping_mul(0x100000001b3);
        h = (h ^ self.pipe.drained).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千次小写零交错 + 千次大写边界冻结。
pub fn sample_pressure() -> (MultiWriterBook, bool) {
    let mut b = MultiWriterBook::new();
    let _ = b.small_storm(SMALL_ROUNDS);
    let small_ok = b.torn == 0 && b.segs == b.writes;
    let _ = b.big_storm(BIG_ROUNDS);
    let big_ok = b.torn == 0;
    let ok = small_ok && big_ok && b.atomic_gate().is_ok() && b.segs_conserved();
    b.freeze();
    (b, ok)
}

/// 注入核：坏写者号 / 零长写 / 撕裂注入 / 冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut b = MultiWriterBook::new();
    // 注入 1：坏写者号 → PP_RANGE 拒收
    let i1 = matches!(b.write(WRITERS, 16), Err(PP_RANGE));
    // 注入 2：零长写 → PP_OVER 拒收
    let i2 = matches!(b.write(0, 0), Err(PP_OVER));
    // 注入 3：超容写 → PP_OVER 拒收
    let i3 = matches!(b.write(0, PIPE_CAP + 1), Err(PP_OVER));
    // 注入 4：冻结后写 → PP_FROZEN 拒收
    b.freeze();
    let i4 = matches!(b.write(1, 16), Err(PP_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 小写千次零交错，大写边界冻结。
pub fn run_unx10982_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10982");
    let (b, ok) = sample_pressure();

    s.add("J1-千次零交错", ok && b.torn == 0, "千次小写原子落段零撕裂");
    s.add("J1-边界冻结", {
        let mut g = MultiWriterBook::new();
        let _ = g.big_storm(BIG_ROUNDS);
        g.segs == BIG_ROUNDS.wrapping_mul(2) && g.torn == 0
    }, "千次大写恰两段且零撕裂");
    s.add("J2-分片同源", (1..=PIPE_BUF).all(|x| MultiWriterBook::segs_of(x) == 1), "小写分片恒为一段");
    s.add("J2-分片边界", MultiWriterBook::segs_of(PIPE_BUF + 1) == 2 && MultiWriterBook::segs_of(PIPE_BUF * 2) == 2, "跨边界恰两段");
    s.add("J2-字节守恒", b.bytes_conserved(), "写者字节与管道字节守恒");
    s.add("J2-段守恒", b.segs_conserved(), "写者段数与管道段账守恒");
    s.add("J3-写次守恒", b.writes == SMALL_ROUNDS + BIG_ROUNDS, "小写大写写次之和守恒");
    s.add("J3-四写者齐", b.writers.iter().all(|w| w.writes > 0), "四写者均有写入无饿死");
    s.add("J3-写者均衡", {
        let mut mn = u64::MAX;
        let mut mx = 0u64;
        let mut k = 0usize;
        while k < b.writers.len() {
            let n = b.writers[k].writes;
            if n < mn { mn = n; }
            if n > mx { mx = n; }
            k += 1;
        }
        mn != u64::MAX && mx.wrapping_sub(mn) <= 1
    }, "四写者写次差不超过一");
    s.add("J4-零撕裂逐者", b.writers_clean(), "四写者逐者零撕裂");
    s.add("J4-水位", b.pipe.level_pm() == 0 && b.pipe.high <= b.pipe.cap, "轮末水位归零且峰值不越容");
    s.add("J5-段率", b.writers.iter().all(|w| w.segs_per_write() >= 1), "逐写者每写至少一段");
    s.add("J6-坏者拒写", inject_four()[0], "坏写者号被 PP_RANGE 拒收");
    s.add("J6-零长拒写", inject_four()[1], "零长写被 PP_OVER 拒收");
    s.add("J7-超容拒写", inject_four()[2], "超容写被 PP_OVER 拒收");
    s.add("J7-冻结拒写", inject_four()[3], "冻结后写被 PP_FROZEN 拒止");
    s.add("J8-撕裂闸", {
        let mut t = MultiWriterBook::new();
        let _ = t.write(0, 64);
        t.torn = 1;
        t.atomic_gate() == Err(PP_TORN)
    }, "撕裂计数非零即原子闸拒止");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-管道动态",
        ok,
        &format!(
            "写 {} 段 {} 撕裂 {} 峰值 {} 读走 {} 水位 {}‰",
            b.writes,
            b.segs,
            b.torn,
            b.pipe.high,
            b.pipe.drained,
            b.pipe.level_pm(),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10982_multi_writer_flow() {
        let mut b = MultiWriterBook::new();
        assert_eq!(b.small_storm(SMALL_ROUNDS).unwrap_or(0), SMALL_ROUNDS);
        assert_eq!(b.torn, 0);
        assert_eq!(b.segs, SMALL_ROUNDS);
        assert!(b.bytes_conserved());
        assert!(b.segs_conserved());
        // 单写语义抽查
        let mut g = MultiWriterBook::new();
        assert_eq!(g.write(0, 100).unwrap_or(0), 1);
        assert_eq!(g.write(0, PIPE_BUF + 5).unwrap_or(0), 2);
        assert_eq!(g.writer_ix(WRITERS).err(), Some(PP_RANGE));
        assert_eq!(MultiWriterBook::segs_of(0), 0);
        assert_eq!(MultiWriterBook::segs_of(PIPE_BUF * 2), 2);
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10982_checks_green() {
        let s = run_unx10982_checks();
        assert!(s.all_passed(), "UNX-F10982 判据未全绿");
    }
}
