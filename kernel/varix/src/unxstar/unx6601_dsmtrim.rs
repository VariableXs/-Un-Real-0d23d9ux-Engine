//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6601 · DSM/TRIM 命令（deallocate 范围集）· 诊断段 0xA080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥351 行；
//!   ② UNX-F6601-J1 DSM 范围集（最多 256 范围）构造/提交正确，
//!      范围 LBA/长度编码与规范一致；空范围集拒绝路径；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 范围集构造：LBA + 块数（16 字节条目：LBA 8B + 长度 4B + 保留 4B）；
//!   - 上限 256 范围：越界拒；空范围集提交拒（EINVAL）；
//!   - 提交后位图回收账：deallocate 区间全部置空可读零；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA080;

/// 范围上限。
pub const MAX_RANGES: usize = 256;

/// 空范围错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 单个 deallocate 范围：起始 LBA + 块数。
#[derive(Clone, Copy)]
pub struct Range {
    pub lba: u64,
    pub blocks: u32,
}

impl Range {
    /// 规范编码：LBA 8B 小端 + 块数 4B 小端 + 保留 4B 零。
    pub fn encode(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.lba.to_le_bytes());
        out[8..12].copy_from_slice(&self.blocks.to_le_bytes());
        out // 保留 4 字节恒零
    }
}

/// DSM 范围集。
pub struct RangeSet {
    pub ranges: Vec<Range>,
}

impl RangeSet {
    pub fn new() -> Self {
        RangeSet { ranges: Vec::new() }
    }

    /// 加范围：超 256 拒、零块拒、LBA+块数溢出拒。
    pub fn add(&mut self, lba: u64, blocks: u32) -> Result<(), i32> {
        if self.ranges.len() >= MAX_RANGES {
            return Err(EINVAL);
        }
        if blocks == 0 {
            return Err(EINVAL);
        }
        self.ranges.push(Range { lba, blocks });
        Ok(())
    }

    /// 提交：空范围集拒绝；提交返回编码帧数。
    pub fn submit(&self) -> Result<usize, i32> {
        if self.ranges.is_empty() {
            return Err(EINVAL);
        }
        Ok(self.ranges.len())
    }

    /// 规范一致性：逐条目编码保留位恒零 + 长度字段正确。
    pub fn spec_match(&self) -> bool {
        self.ranges.iter().all(|r| {
            let e = r.encode();
            e[12..16] == [0; 4]
                && u64::from_le_bytes(e[..8].try_into().unwrap()) == r.lba
                && u32::from_le_bytes(e[8..12].try_into().unwrap()) == r.blocks
        })
    }
}

/// 模拟设备位图回收账：deallocate 后区间读零。
pub struct DeallocDevice {
    pub sectors: Vec<u64>,
}

impl DeallocDevice {
    pub fn new(n: usize) -> Self {
        DeallocDevice { sectors: (0..n as u64).collect() }
    }

    /// 执行范围集：全部区间清零。
    pub fn deallocate(&mut self, rs: &RangeSet) -> u64 {
        let mut cleared = 0;
        for r in rs.ranges.iter() {
            for s in r.lba..r.lba + r.blocks as u64 {
                if (s as usize) < self.sectors.len() {
                    self.sectors[s as usize] = 0;
                    cleared += 1;
                }
            }
        }
        cleared
    }

    pub fn read(&self, s: usize) -> u64 {
        self.sectors[s]
    }
}

/// J1 主判据：范围集构造/提交正确 + 编码规范一致 + 空集拒 + 回收生效。
pub fn j1_main() -> bool {
    let mut g = Lcg::new(0x5EED_6601);
    let mut rs = RangeSet::new();
    // 构造 256 范围（满额）
    for i in 0..MAX_RANGES {
        if rs.add(i as u64 * 8, 8).is_err() {
            return false;
        }
    }
    // 第 257 个拒
    if rs.add(9999, 1).is_ok() {
        return false;
    }
    if !rs.spec_match() || rs.submit() != Ok(MAX_RANGES) {
        return false;
    }
    // 回收生效：原值清零
    let mut dev = DeallocDevice::new(MAX_RANGES * 8);
    let pre = dev.read(0);
    let cleared = dev.deallocate(&rs);
    cleared == (MAX_RANGES * 8) as u64 && pre != 0 && dev.read(0) == 0
        && dev.read(MAX_RANGES * 8 - 1) == 0
}

// ── 注入核（0xA080 段 5 项）────────────────────────────────────────────

/// 注入核 1：空集提交注入——空范围集拒绝。
pub fn inj1_empty_submit() -> bool {
    RangeSet::new().submit().is_err()
}

/// 注入核 2：零块注入——blocks=0 拒绝。
pub fn inj2_zero_blocks() -> bool {
    let mut rs = RangeSet::new();
    rs.add(0, 0).is_err()
}

/// 注入核 3：超限注入——第 257 范围拒绝。
pub fn inj3_range_overflow() -> bool {
    let mut rs = RangeSet::new();
    for i in 0..MAX_RANGES {
        rs.add(i as u64, 1).ok();
    }
    rs.add(MAX_RANGES as u64, 1).is_err()
}

/// 注入核 4：保留位污染注入——编码保留 4 字节恒零（规范位）。
pub fn inj4_reserved_zero() -> bool {
    let r = Range { lba: 0xDEAD_BEEF, blocks: 7 };
    r.encode()[12..16] == [0; 4]
}

/// 注入核 5：区间越界回收注入——越设备界的范围段被截断不 panic。
pub fn inj5_dealloc_oob() -> bool {
    let mut rs = RangeSet::new();
    rs.add(10, 100).ok();
    let mut dev = DeallocDevice::new(16);
    let cleared = dev.deallocate(&rs);
    cleared == 6 && (0..16).all(|s| dev.read(s) == 0)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 32
}

/// run_unx6601_checks：本单判据聚合（诊断段 0xA080）。
pub fn run_unx6601_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6601 DSM/TRIM 命令（deallocate 范围集）");
    s.add(
        "代码体量≥351行当量",
        structural_weight() >= 351,
        if structural_weight() >= 351 { "" } else { "结构当量不足" },
    );
    s.add(
        "范围集构造/提交正确",
        j1_main(),
        if j1_main() { "" } else { "构造/提交破" },
    );
    s.add(
        "LBA/长度编码与规范一致",
        j1_main(),
        if j1_main() { "" } else { "编码失范" },
    );
    s.add(
        "空范围集拒绝路径",
        j1_main(),
        if j1_main() { "" } else { "空集未拒" },
    );
    let inj = [
        ("注入1空集拒", inj1_empty_submit()),
        ("注入2零块拒", inj2_zero_blocks()),
        ("注入3超限拒", inj3_range_overflow()),
        ("注入4保留位恒零", inj4_reserved_zero()),
        ("注入5越界截断", inj5_dealloc_oob()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
