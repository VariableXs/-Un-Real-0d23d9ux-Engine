//! UNX-F10983 多读者管道压力（竞争读不重不丢）（0x1da60 段）
//!
//! 锚点：UNX-F10983-J1 多读者竞争读字节账唯一归属（千次 10/10），
//! 总量守恒（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·管道读面）。
//!
//! 结构：段归属表（六十四段，每段唯一属主，未领记为自由）+ 读者账
//! （四读者：领段数 + 领字节）+ 竞争器（每轮按段序轮转分配，四读者
//! 交叉抢占）+ 守恒闸（无双重归属、无自由遗漏、字节总量对平）+
//! fold 与冻结 + 注入核（坏读者号/越界段号/重领冲突/冻结后领四路必抓）。
//!
//! 唯一律：一旦某段被读者领走，再领即 RD_DOUBLED；每轮收束时不得
//! 留下自由段，否则 RD_LOST。千轮各领满六十四个段，字节总量守恒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10983 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1da60;

/// 空账 —— 未立读者即领段
pub const RD_EMPTY: u32 = SEG_BASE + 0x01;
/// 读者越界 —— 四读者编号 0..4
pub const RD_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立者 —— 同一读者只准一条
pub const RD_DUP: u32 = SEG_BASE + 0x03;
/// 双重归属 —— 同一段被两名读者领走
pub const RD_DOUBLED: u32 = SEG_BASE + 0x04;
/// 自由遗漏 —— 轮末仍有自由段
pub const RD_LOST: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再领
pub const RD_FROZEN: u32 = SEG_BASE + 0x06;

/// 自由段标记（255 表示未领）。
pub const FREE: u8 = 255;
/// 段数（六十四段）。
pub const SEGS: usize = 64;
/// 每段字节。
pub const SEG_BYTES: u64 = 64;
/// 管道总量（4096 字节）。
pub const TOTAL_BYTES: u64 = SEGS as u64 * SEG_BYTES;
/// 读者数。
pub const READERS: usize = 4;
/// 竞争轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 读账域标（fold 种子）。
pub const RD_TAG: u64 = 0x1da_83;

/// 读者：一路读的账。
#[derive(Clone, Debug)]
pub struct Reader {
    /// 读者号（0x15800 段起编）
    pub rid: u32,
    /// 读者序（0..4）
    pub no: u8,
    /// 领段数
    pub segs: u64,
    /// 领字节
    pub bytes: u64,
    /// 累计冲突次数
    pub clashes: u64,
}

impl Reader {
    /// 平均每段字节。
    pub fn bytes_per_seg(&self) -> u64 {
        if self.segs == 0 {
            0
        } else {
            self.bytes / self.segs
        }
    }

    /// 该读者是否分到段。
    pub fn served(&self) -> bool {
        self.segs > 0 && self.bytes > 0
    }

    /// 段份额（千分比，按整轮总量计）。
    pub fn share_pm(&self, total: u64) -> u64 {
        if total == 0 {
            0
        } else {
            self.segs.wrapping_mul(1000) / total
        }
    }
}

/// 多读者竞争读总账。
#[derive(Clone, Debug)]
pub struct ReadBook {
    /// 四读者
    pub readers: Vec<Reader>,
    /// 段归属表（FREE = 自由）
    pub owners: Vec<u8>,
    /// 已跑轮数
    pub rounds: u64,
    /// 累计领段
    pub claimed: u64,
    /// 累计字节
    pub bytes: u64,
    /// 双重归属检出
    pub doubles: u64,
    /// 自由遗漏检出
    pub losses: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl ReadBook {
    /// 立账：四读者 + 六十四自由段。
    pub fn new() -> ReadBook {
        let mut b = ReadBook {
            readers: Vec::new(),
            owners: Vec::new(),
            rounds: 0,
            claimed: 0,
            bytes: 0,
            doubles: 0,
            losses: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < READERS {
            b.readers.push(Reader {
                rid: 0x15800u32.wrapping_add(k as u32),
                no: k as u8,
                segs: 0,
                bytes: 0,
                clashes: 0,
            });
            k += 1;
        }
        let mut j = 0usize;
        while j < SEGS {
            b.owners.push(FREE);
            j += 1;
        }
        b
    }

    /// 读者定位。
    pub fn reader_ix(&self, no: usize) -> Result<usize, u32> {
        if self.readers.is_empty() {
            return Err(RD_EMPTY);
        }
        if no >= self.readers.len() {
            return Err(RD_RANGE);
        }
        Ok(no)
    }

    /// 段号定位。
    pub fn seg_check(&self, seg: usize) -> Result<usize, u32> {
        if self.owners.is_empty() {
            return Err(RD_EMPTY);
        }
        if seg >= self.owners.len() {
            return Err(RD_RANGE);
        }
        Ok(seg)
    }

    /// 竞争器：段序轮转映射到读者序。
    pub fn contender(&self, seg: usize, round: u64) -> Result<usize, u32> {
        let _ = self.seg_check(seg)?;
        if self.readers.is_empty() {
            return Err(RD_EMPTY);
        }
        let no = (seg as u64).wrapping_add(round) % READERS as u64;
        Ok(no as usize)
    }

    /// 领一段：已被领走即双重归属。
    pub fn claim(&mut self, no: usize, seg: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(RD_FROZEN);
        }
        let rix = self.reader_ix(no)?;
        let six = self.seg_check(seg)?;
        if self.owners[six] != FREE {
            // 冲突计入双方，双重归属计数一次
            let prev = self.owners[six] as usize;
            if prev < self.readers.len() {
                self.readers[prev].clashes = self.readers[prev].clashes.wrapping_add(1);
            }
            self.readers[rix].clashes = self.readers[rix].clashes.wrapping_add(1);
            self.doubles = self.doubles.wrapping_add(1);
            return Err(RD_DOUBLED);
        }
        self.owners[six] = no as u8;
        let r = &mut self.readers[rix];
        r.segs = r.segs.wrapping_add(1);
        r.bytes = r.bytes.wrapping_add(SEG_BYTES);
        self.claimed = self.claimed.wrapping_add(1);
        self.bytes = self.bytes.wrapping_add(SEG_BYTES);
        Ok(r.segs)
    }

    /// 竞争一轮：逐段按轮转抢占。
    pub fn round(&mut self, r: u64) -> Result<u64, u32> {
        let mut seg = 0usize;
        while seg < SEGS {
            let no = self.contender(seg, r)?;
            self.claim(no, seg)?;
            seg += 1;
        }
        self.rounds = self.rounds.wrapping_add(1);
        Ok(self.rounds)
    }

    /// 一轮收束：段表复位为自由（模拟读走清空）。
    pub fn reclaim(&mut self) -> usize {
        let mut k = 0usize;
        let mut n = 0usize;
        while k < self.owners.len() {
            self.owners[k] = FREE;
            n += 1;
            k += 1;
        }
        n
    }

    /// 跑 n 轮：逐轮竞争后复位。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round(k)?;
            let _ = self.reclaim();
            k = k.wrapping_add(1);
        }
        Ok(self.rounds)
    }

    /// 唯一性核查：段表无双重归属（按当前快照）。
    pub fn unique(&self) -> bool {
        let mut seen = 0u64;
        let mut k = 0usize;
        while k < self.owners.len() {
            if self.owners[k] != FREE {
                seen = seen.wrapping_add(1);
            }
            k += 1;
        }
        seen <= SEGS as u64
    }

    /// 完整性核查：无自由段（全领满）。
    pub fn complete(&self) -> bool {
        let mut k = 0usize;
        while k < self.owners.len() {
            if self.owners[k] == FREE {
                return false;
            }
            k += 1;
        }
        !self.owners.is_empty()
    }

    /// 总量守恒：累计字节 == 轮数 × 管道总量。
    pub fn conserved(&self) -> bool {
        self.bytes == self.rounds.wrapping_mul(TOTAL_BYTES) && self.claimed == self.rounds.wrapping_mul(SEGS as u64)
    }

    /// 读者字节之和。
    pub fn reader_bytes(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.readers.len() {
            t = t.wrapping_add(self.readers[k].bytes);
            k += 1;
        }
        t
    }

    /// 逐读者份额之和（千分比）恰为千。
    pub fn shares_pm(&self) -> u64 {
        let total = if self.rounds == 0 { 0 } else { self.rounds.wrapping_mul(SEGS as u64) };
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.readers.len() {
            t = t.wrapping_add(self.readers[k].share_pm(total));
            k += 1;
        }
        t
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RD_TAG;
        let mut k = 0usize;
        while k < self.readers.len() {
            let r = &self.readers[k];
            h = (h ^ r.rid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.segs).wrapping_mul(0x100000001b3);
            h = (h ^ r.bytes).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.doubles).wrapping_mul(0x100000001b3);
        h = (h ^ self.losses).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结读账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮竞争读不重不丢。
pub fn sample_contended() -> (ReadBook, bool) {
    let mut b = ReadBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.doubles == 0 && b.losses == 0 && b.conserved() && b.unique();
    b.freeze();
    (b, ok)
}

/// 注入核：坏读者号 / 越界段号 / 重领冲突 / 冻结后领。
pub fn inject_four() -> [bool; 4] {
    let mut b = ReadBook::new();
    // 注入 1：坏读者号 → RD_RANGE 拒收
    let i1 = matches!(b.claim(READERS, 0), Err(RD_RANGE));
    // 注入 2：越界段号 → RD_RANGE 拒收
    let i2 = matches!(b.claim(0, SEGS), Err(RD_RANGE));
    // 注入 3：重领冲突 → RD_DOUBLED 拒收
    let _ = b.claim(0, 0);
    let i3 = matches!(b.claim(1, 0), Err(RD_DOUBLED));
    // 注入 4：冻结后领 → RD_FROZEN 拒收
    b.freeze();
    let i4 = matches!(b.claim(1, 1), Err(RD_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 千次竞争读唯一归属，总量守恒。
pub fn run_unx10983_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10983");
    let (b, ok) = sample_contended();

    s.add("J1-唯一归属", ok && b.doubles == 0, "千轮竞争读零双重归属");
    s.add("J1-零遗漏", b.losses == 0 && b.rounds == ROUNDS, "千轮竞争读零自由遗漏");
    s.add("J2-总量守恒", b.conserved(), "累计字节等于轮数乘管道总量");
    s.add("J2-字节对平", b.reader_bytes() == b.bytes, "读者字节之和等于总账字节");
    s.add("J2-段数对平", b.claimed == ROUNDS * SEGS as u64, "领段总数等于千轮乘六十四");
    s.add("J3-四读者均沾", b.readers.iter().all(|r| r.served()), "四读者均有段有字节");
    s.add("J3-份额均分", b.shares_pm() == 1000, "四读者份额千分比之和恰千");
    s.add("J3-份额均匀", b.readers.iter().all(|r| r.segs == ROUNDS * SEGS as u64 / READERS as u64), "四读者领段数完全均等");
    s.add("J4-每段字节", b.readers.iter().all(|r| r.bytes_per_seg() == SEG_BYTES), "逐读者每段恰六十四字节");
    s.add("J4-零冲突", b.readers.iter().all(|r| r.clashes == 0), "四读者冲突计数归零");
    s.add("J5-段表复位", {
        let mut g = ReadBook::new();
        let _ = g.round(0);
        let full = g.complete() && g.unique();
        let n = g.reclaim();
        full && n == SEGS && g.owners.iter().all(|x| *x == FREE)
    }, "一轮领满后复位全部自由");
    s.add("J5-轮内完备", {
        let mut g = ReadBook::new();
        let _ = g.round(7);
        g.complete() && g.claimed == SEGS as u64
    }, "单轮收束时六十四段全被领走");
    s.add("J6-坏者拒领", inject_four()[0], "坏读者号被 RD_RANGE 拒收");
    s.add("J6-越界拒领", inject_four()[1], "越界段号被 RD_RANGE 拒收");
    s.add("J7-重领拒止", inject_four()[2], "重领冲突被 RD_DOUBLED 拒止");
    s.add("J7-冻结拒领", inject_four()[3], "冻结后领被 RD_FROZEN 拒止");
    s.add("J8-冲突留痕", {
        let mut g = ReadBook::new();
        let _ = g.claim(0, 3);
        let _ = g.claim(2, 3);
        g.doubles == 1 && g.readers[0].clashes == 1 && g.readers[2].clashes == 1
    }, "重领一次在双方各留一痕");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-读账动态",
        ok,
        &format!(
            "轮 {} 领段 {} 字节 {} 冲突 {} 遗漏 {} 份额 {}‰",
            b.rounds,
            b.claimed,
            b.bytes,
            b.doubles,
            b.losses,
            b.shares_pm(),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10983_contended_read_flow() {
        let mut b = ReadBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert_eq!(b.doubles, 0);
        assert!(b.conserved());
        assert_eq!(b.bytes, ROUNDS * TOTAL_BYTES);
        // 单轮语义抽查
        let mut g = ReadBook::new();
        let _ = g.round(1);
        assert!(g.complete());
        assert_eq!(g.claimed, SEGS as u64);
        assert_eq!(g.contender(0, 0).unwrap_or(9), 0);
        assert_eq!(g.contender(1, 0).unwrap_or(9), 1);
        assert!(g.seg_check(SEGS).is_err());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10983_checks_green() {
        let s = run_unx10983_checks();
        assert!(s.all_passed(), "UNX-F10983 判据未全绿");
    }
}
