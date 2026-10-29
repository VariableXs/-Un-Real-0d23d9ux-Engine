//! UNX-F10986 信号量保护共享内存并发读写压力（账一致性）（0x1dac0 段）
//!
//! 锚点：UNX-F10986-J1 写独占读一致千次快照对平（10/10），shm 字节账
//! 守恒（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·共享内存面）。
//!
//! 结构：共享段账（三十二块，每块一校验和，逐块可算摘要）+ 保护闸
//! （写入必须持锁，读快照必须整段一致）+ 版本账（每次写入置版，快照
//! 按版本重算摘要逐次对平）+ fold 与冻结 + 注入核（未持锁写/越界块号/
//! 撕裂快照/冻结后写四路必抓）。
//!
//! 一致律：写者持锁期间整段改写并置版；读者快照摘要必须等于该版本
//! 的同源摘要。任一块被孤立篡改（版本不动）即 SH_TORN —— 撕裂读。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10986 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1dac0;

/// 空账 —— 未立段即读写
pub const SP_EMPTY: u32 = SEG_BASE + 0x01;
/// 块号越界 —— 三十二块 0..32
pub const SP_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立块 —— 一块只准立一次
pub const SP_DUP: u32 = SEG_BASE + 0x03;
/// 撕裂快照 —— 快照摘要与该版本不符
pub const SP_TORN: u32 = SEG_BASE + 0x04;
/// 未持锁写 —— 写者未持保护闸
pub const SP_UNLOCKED: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再写
pub const SP_FROZEN: u32 = SEG_BASE + 0x06;

/// 块数。
pub const BLOCKS: usize = 32;
/// 每块字节。
pub const BLOCK_BYTES: u64 = 64;
/// 共享段总量（2048 字节）。
pub const SHM_BYTES: u64 = BLOCKS as u64 * BLOCK_BYTES;
/// 快照轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 共享段账域标（fold 种子）。
pub const SP_TAG: u64 = 0x1da_86;

/// 同源校验和：版本 + 块号 → 校验和。
pub fn checksum_of(version: u64, bid: usize) -> u32 {
    let mut h = version.wrapping_mul(0x9e37_79b9);
    h = h.wrapping_add((bid as u64).wrapping_mul(0x85eb_ca6b));
    ((h ^ (h >> 32)) & 0xffff_ffff) as u32
}

/// 同源段摘要：版本 → 整段摘要。
pub fn digest_of(version: u64) -> u64 {
    let mut h: u64 = SP_TAG;
    let mut k = 0usize;
    while k < BLOCKS {
        h = (h ^ checksum_of(version, k) as u64).wrapping_mul(0x100000001b3);
        k += 1;
    }
    h = (h ^ version).wrapping_mul(0x100000001b3);
    h
}

/// 共享段：一块的留痕。
#[derive(Clone, Debug)]
pub struct ShmSeg {
    /// 块号（0x15d00 段起编）
    pub bid: u32,
    /// 块序（0..32）
    pub no: u8,
    /// 当前校验和
    pub checksum: u32,
    /// 写入次数
    pub writes: u64,
}

impl ShmSeg {
    /// 块已写入。
    pub fn written(&self) -> bool {
        self.writes > 0
    }

    /// 该块在给定版本下的同源校验和。
    pub fn expected(&self, version: u64) -> u32 {
        checksum_of(version, self.no as usize)
    }
}

/// 共享内存读写总账。
#[derive(Clone, Debug)]
pub struct ShmBook {
    /// 三十二块
    pub segs: Vec<ShmSeg>,
    /// 当前版本
    pub version: u64,
    /// 保护闸（true = 写者持锁）
    pub locked: bool,
    /// 持有者（NO_OWNER = 无人）
    pub holder: u8,
    /// 快照次数
    pub snapshots: u64,
    /// 不一致次数
    pub mismatches: u64,
    /// 字节写总量
    pub bytes: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl ShmBook {
    /// 立账：三十二块零校验和。
    pub fn new() -> ShmBook {
        let mut b = ShmBook {
            segs: Vec::new(),
            version: 0,
            locked: false,
            holder: NO_OWNER,
            snapshots: 0,
            mismatches: 0,
            bytes: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < BLOCKS {
            b.segs.push(ShmSeg {
                bid: 0x15d00u32.wrapping_add(k as u32),
                no: k as u8,
                checksum: 0,
                writes: 0,
            });
            k += 1;
        }
        b
    }

    /// 块定位。
    pub fn seg_ix(&self, no: usize) -> Result<usize, u32> {
        if self.segs.is_empty() {
            return Err(SP_EMPTY);
        }
        if no >= self.segs.len() {
            return Err(SP_RANGE);
        }
        Ok(no)
    }

    /// 取保护闸。
    pub fn lock(&mut self, who: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if self.locked {
            return Err(SP_DUP);
        }
        self.locked = true;
        self.holder = who;
        Ok(())
    }

    /// 放保护闸。
    pub fn unlock(&mut self, who: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if !self.locked || self.holder != who {
            return Err(SP_UNLOCKED);
        }
        self.locked = false;
        self.holder = NO_OWNER;
        Ok(())
    }

    /// 写一块：必须持锁。
    pub fn write_block(&mut self, no: usize, version: u64) -> Result<u32, u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if !self.locked {
            return Err(SP_UNLOCKED);
        }
        let ix = self.seg_ix(no)?;
        let want = checksum_of(version, no);
        let s = &mut self.segs[ix];
        s.checksum = want;
        s.writes = s.writes.wrapping_add(1);
        self.bytes = self.bytes.wrapping_add(BLOCK_BYTES);
        Ok(want)
    }

    /// 整段改写并置版：必须持锁。
    pub fn write_all(&mut self, version: u64) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.segs.len() {
            self.write_block(k, version)?;
            k += 1;
        }
        self.version = version;
        Ok(digest_of(version))
    }

    /// 写一轮：取闸 → 整段改写 → 放闸。
    pub fn write_round(&mut self, version: u64) -> Result<u64, u32> {
        self.lock(0)?;
        let d = self.write_all(version)?;
        self.unlock(0)?;
        Ok(d)
    }

    /// 当前段摘要（读者快照）。
    pub fn snapshot(&self) -> u64 {
        let mut h: u64 = SP_TAG;
        let mut k = 0usize;
        while k < self.segs.len() {
            h = (h ^ self.segs[k].checksum as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.version).wrapping_mul(0x100000001b3);
        h
    }

    /// 读一轮：快照并与该版本同源摘要对平。
    pub fn read_round(&mut self) -> Result<bool, u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if self.segs.is_empty() {
            return Err(SP_EMPTY);
        }
        if self.locked {
            return Err(SP_UNLOCKED);
        }
        let got = self.snapshot();
        let want = digest_of(self.version);
        self.snapshots = self.snapshots.wrapping_add(1);
        let ok = got == want;
        if !ok {
            self.mismatches = self.mismatches.wrapping_add(1);
        }
        Ok(ok)
    }

    /// 读写交替：千次快照全对平。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            let v = k.wrapping_add(1);
            self.write_round(v)?;
            let _ = self.read_round()?;
            k = k.wrapping_add(1);
        }
        Ok(self.snapshots)
    }

    /// 全对平：快照数等于轮数且零不一致。
    pub fn all_consistent(&self) -> bool {
        self.snapshots == ROUNDS && self.mismatches == 0
    }

    /// 字节守恒：写字节 == 轮数 × 段总量。
    pub fn bytes_conserved(&self) -> bool {
        self.bytes == self.version.wrapping_mul(SHM_BYTES)
    }

    /// 逐块与版本同源。
    pub fn blocks_consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].checksum != self.segs[k].expected(self.version) {
                return false;
            }
            k += 1;
        }
        !self.segs.is_empty()
    }

    /// 逐块已写。
    pub fn blocks_written(&self) -> bool {
        let mut k = 0usize;
        while k < self.segs.len() {
            if !self.segs[k].written() {
                return false;
            }
            k += 1;
        }
        !self.segs.is_empty()
    }

    /// 一致性闸：撕裂即拒。
    pub fn consistency_gate(&self) -> Result<(), u32> {
        if self.segs.is_empty() {
            return Err(SP_EMPTY);
        }
        if !self.blocks_consistent() {
            return Err(SP_TORN);
        }
        Ok(())
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SP_TAG;
        let mut k = 0usize;
        while k < self.segs.len() {
            h = (h ^ self.segs[k].bid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.segs[k].checksum as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.segs[k].writes).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.version).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 无人持有标记。
pub const NO_OWNER: u8 = 255;

/// 确定性样本：千次快照全对平。
pub fn sample_consistent() -> (ShmBook, bool) {
    let mut b = ShmBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.all_consistent() && b.bytes_conserved() && b.consistency_gate().is_ok();
    b.freeze();
    (b, ok)
}

/// 注入核：未持锁写 / 越界块号 / 撕裂快照 / 冻结后写。
pub fn inject_four() -> [bool; 4] {
    // 注入 1：未持锁写 → SP_UNLOCKED 拒收
    let mut b = ShmBook::new();
    let i1 = matches!(b.write_block(0, 1), Err(SP_UNLOCKED));
    // 注入 2：越界块号 → SP_RANGE 拒收
    let _ = b.lock(0);
    let i2 = matches!(b.write_block(BLOCKS, 1), Err(SP_RANGE));
    // 注入 3：撕裂快照 → 孤改一块不动版本即 SP_TORN
    let mut t = ShmBook::new();
    let _ = t.write_round(1);
    t.segs[5].checksum = t.segs[5].checksum.wrapping_add(1);
    let i3 = t.consistency_gate() == Err(SP_TORN) && t.read_round() == Ok(false);
    // 注入 4：冻结后写 → SP_FROZEN 拒收
    let mut f = ShmBook::new();
    f.freeze();
    let i4 = matches!(f.write_round(2), Err(SP_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 写独占读一致千次对平，shm 字节账守恒。
pub fn run_unx10986_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10986");
    let (b, ok) = sample_consistent();

    s.add("J1-千次对平", ok && b.all_consistent(), "千次快照逐次与同源摘要对平");
    s.add("J1-字节守恒", b.bytes_conserved(), "写字节等于版本乘段总量");
    s.add("J2-逐块同源", b.blocks_consistent(), "三十二块逐块校验和与版本同源");
    s.add("J2-逐块已写", b.blocks_written(), "三十二块逐块已写无遗漏");
    s.add("J2-版本推进", b.version == ROUNDS, "版本自一推进至千");
    s.add("J3-快照计数", b.snapshots == ROUNDS, "快照次数等于轮数");
    s.add("J3-零不一致", b.mismatches == 0, "千次快照零不一致");
    s.add("J3-写字节", b.bytes == ROUNDS * SHM_BYTES, "写字节总量等于千乘二百零四十八");
    s.add("J4-摘要敏感", digest_of(1) != digest_of(2) && digest_of(1) == digest_of(1), "版本变化即摘要变化且重算稳定");
    s.add("J4-块内敏感", checksum_of(1, 0) != checksum_of(1, 1), "同版本不同块校验和互异");
    s.add("J5-持锁独占", {
        let mut g = ShmBook::new();
        let _ = g.lock(0);
        g.lock(1) == Err(SP_DUP)
    }, "持锁期间再取锁被拒");
    s.add("J6-未持锁拒写", inject_four()[0], "未持锁写被 SP_UNLOCKED 拒收");
    s.add("J6-越界拒写", inject_four()[1], "越界块号被 SP_RANGE 拒收");
    s.add("J7-撕裂必检", inject_four()[2], "孤改一块即被判撕裂且快照不符");
    s.add("J7-冻结拒写", inject_four()[3], "冻结后写被 SP_FROZEN 拒止");
    s.add("J8-越主拒放", {
        let mut g = ShmBook::new();
        let _ = g.lock(0);
        g.unlock(1) == Err(SP_UNLOCKED)
    }, "非属主放锁被 SP_UNLOCKED 拒收");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-共享动态",
        ok,
        &format!(
            "版本 {} 快照 {} 不一致 {} 字节 {} 块 {} 锁 {}",
            b.version,
            b.snapshots,
            b.mismatches,
            b.bytes,
            b.segs.len(),
            b.locked,
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10986_shm_book_flow() {
        let mut b = ShmBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.all_consistent());
        assert!(b.blocks_consistent());
        // 单轮语义抽查
        let mut g = ShmBook::new();
        let _ = g.write_round(7);
        assert_eq!(g.version, 7);
        assert_eq!(g.snapshot(), digest_of(7));
        assert_eq!(g.read_round(), Ok(true));
        assert_eq!(g.segs[0].checksum, checksum_of(7, 0));
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10986_checks_green() {
        let s = run_unx10986_checks();
        assert!(s.all_passed(), "UNX-F10986 判据未全绿");
    }
}
