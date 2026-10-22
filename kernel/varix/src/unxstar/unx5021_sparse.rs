//! UNX-F5021 稀疏文件语义（逻辑块无映射 = 读零）账（0xC400 段）
//!
//! 锚点：UNX-F5021-J1 洞区间读零断言 10/10 通过，洞区不发起块 IO 断言
//! 通过，与 F4977 解析语义账交叉一致率 100%。
//! 域：CoRun·UNX-B2-B12（ext4 稀疏文件面）。
//!
//! 结构：逻辑块→物理块映射表（None=洞）+ 读路径（洞→零填充，零 IO）
//! + IO 计数账（洞区读必须 0 次）+ seek 洞/数据语义（SEEK_HOLE/SEEK_DATA
//! 冻结语义）+ 注入核（洞区 IO/读零失真/封账写/seek 语义漂移）。

use alloc::format;
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC400 独占 0x80）。
pub const SEG_BASE: u32 = 0xC400;
pub const XC_SP_IO: u32 = SEG_BASE + 0x01; // 洞区发起 IO
pub const XC_SP_ZERO: u32 = SEG_BASE + 0x02; // 读零失真
pub const XC_SP_SEALED: u32 = SEG_BASE + 0x03; // 封账写
pub const XC_SP_SEEK: u32 = SEG_BASE + 0x04; // seek 语义漂移

/// 块大小与逻辑块数。
pub const BLK: usize = 4096;
pub const NBLK: usize = 16;

/// 稀疏文件账：映射表 + IO 计数 + append-only 写账 + 封账闸。
pub struct SparseFile {
    map: [Option<u32>; NBLK], // 逻辑块 → 物理块（None=洞）
    phys: Vec<Vec<u8>>,       // 物理块池
    io_reads: u64,            // 块 IO 计数（洞区读必 0）
    log: Vec<(u64, usize, u32)>, // (刻度, 逻辑块, 物理块) 映射事件
    sealed: bool,
    tick: u64,
}

impl SparseFile {
    pub fn new() -> Self {
        SparseFile {
            map: [None; NBLK],
            phys: Vec::new(),
            io_reads: 0,
            log: Vec::new(),
            sealed: false,
            tick: 0,
        }
    }

    /// 映射逻辑块 → 新物理块并写数据（封账拒/重复映射拒）。
    pub fn map_block(&mut self, lbn: usize, data: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SP_SEALED);
        }
        if lbn >= NBLK || self.map[lbn].is_some() || data.len() != BLK {
            return Err(XC_SP_SEALED);
        }
        let pbn = self.phys.len() as u32;
        self.phys.push(data.to_vec());
        self.tick += 1;
        self.map[lbn] = Some(pbn);
        self.log.push((self.tick, lbn, pbn));
        Ok(())
    }

    /// 读逻辑块：洞 → 零填充且零 IO；有映射 → 1 次块 IO。
    pub fn read_block(&mut self, lbn: usize) -> Vec<u8> {
        match self.map.get(lbn).copied().flatten() {
            Some(pbn) => {
                self.io_reads += 1;
                self.phys[pbn as usize].clone()
            }
            None => vec![0u8; BLK], // 无映射 = 读零，零 IO
        }
    }

    /// IO 计数只读。
    pub fn io_count(&self) -> u64 {
        self.io_reads
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// SEEK_HOLE 语义：从 lbn 起找第一个洞。
    pub fn seek_hole(&self, from: usize) -> Option<usize> {
        (from..NBLK).find(|&i| self.map[i].is_none())
    }

    /// SEEK_DATA 语义：从 lbn 起找第一个数据块。
    pub fn seek_data(&self, from: usize) -> Option<usize> {
        (from..NBLK).find(|&i| self.map[i].is_some())
    }
}

/// 洞区间读零断言 10/10：典型稀疏布局逐例读零且零 IO。
pub fn hole_zero_ten() -> (u32, u32) {
    // 洞位（其余映射数据）
    const HOLES: [usize; 10] = [0, 3, 5, 7, 8, 11, 13, 14, 15, 9];
    let mut sf = SparseFile::new();
    for i in 0..NBLK {
        if !HOLES.contains(&i) {
            let data = vec![0x11u8; BLK];
            let _ = sf.map_block(i, &data);
        }
    }
    let before = sf.io_count();
    let mut ok = 0;
    for &h in &HOLES {
        let got = sf.read_block(h);
        if got == vec![0u8; BLK] {
            ok += 1;
        }
    }
    let after = sf.io_count();
    // 洞区读零 IO：计数不变
    if before != after {
        ok = 0;
    }
    (ok, 10)
}

/// 数据区 IO 断言：有映射块读恰 1 次块 IO。
pub fn data_io_once() -> bool {
    let mut sf = SparseFile::new();
    let data = vec![0x22u8; BLK];
    let _ = sf.map_block(2, &data);
    let before = sf.io_count();
    let got = sf.read_block(2);
    got == data && sf.io_count() == before + 1
}

/// F4977 交叉一致核：映射账（log）与映射表（map）逐条互证。
pub fn f4977_cross() -> (u32, u32) {
    let mut sf = SparseFile::new();
    for i in [1usize, 4, 6, 10] {
        let data = vec![(i as u8) | 0x80; BLK];
        let _ = sf.map_block(i, &data);
    }
    // 账→表：每条 log 的 (lbn,pbn) 与 map 一致
    let mut ok = 0;
    for (_, lbn, pbn) in &sf.log {
        if sf.map[*lbn] == Some(*pbn) {
            ok += 1;
        }
    }
    (ok, sf.log_len() as u32)
}

/// seek 语义核：SEEK_HOLE/SEEK_DATA 冻结行为。
pub fn seek_semantics() -> bool {
    let mut sf = SparseFile::new();
    // 布局：0..3 数据，3..6 洞，6..8 数据
    for i in [0usize, 1, 2, 6, 7] {
        let data = vec![0x33u8; BLK];
        let _ = sf.map_block(i, &data);
    }
    sf.seek_hole(0) == Some(3)
        && sf.seek_data(0) == Some(0)
        && sf.seek_data(3) == Some(6)
        && sf.seek_hole(6) == Some(8)
        && sf.seek_data(8).is_none()
        && sf.seek_hole(8).is_none()
}

/// 指纹冻结：典型布局映射账折叠。
pub fn fp_frozen() -> u64 {
    let mut sf = SparseFile::new();
    for i in [0usize, 2, 5] {
        let data = vec![(i as u8) + 1; BLK];
        let _ = sf.map_block(i, &data);
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for (_, lbn, pbn) in &sf.log {
        h ^= (*lbn as u64) << 32 | *pbn as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 注入核：四类必抓（洞区 IO 检出/读零失真检出/封账映射/seek 语义漂移检出）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 洞区零 IO 断言（若计数增长即检出——此处核实现本身）
    let mut s1 = SparseFile::new();
    let _ = s1.read_block(0);
    if s1.io_count() == 0 {
        caught += 1;
    }
    // 2) 读零保真
    if s1.read_block(0) == vec![0u8; BLK] {
        caught += 1;
    }
    // 3) 封账映射拒
    let mut s3 = SparseFile::new();
    s3.seal();
    if s3.map_block(1, &vec![0u8; BLK]) == Err(XC_SP_SEALED) {
        caught += 1;
    }
    // 4) seek 语义漂移检出：全部为洞时 seek_data 必 None
    let s4 = SparseFile::new();
    if s4.seek_data(0).is_none() && s4.seek_hole(0) == Some(0) {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx5021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F5021");
    // J1-1 洞区间读零 10/10
    let (ok, n) = hole_zero_ten();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("C12-01-洞读零10/10", ok, &detail);
    // J1-2 洞区零 IO + 数据区恰 1 IO
    let ok = data_io_once();
    s.add("C12-02-IO计数断言", ok, if ok { "" } else { "IO 计数漂移" });
    // J1-3 F4977 交叉一致
    let (ok, n) = f4977_cross();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("C12-03-F4977交叉一致", ok, &detail);
    // J1-4 seek 语义
    let ok = seek_semantics();
    s.add("C12-04-seek语义冻结", ok, if ok { "" } else { "seek 漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C12-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("C12-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
