//! F021 深化批次四 · 内存诊断转储面（compatstar2/deep3 · G-A-21）。
//!
//! 批次一~三深化覆盖 VirtualProtect/DECOMMIT/堆句柄族/四态状态机与保护位
//! 边界；本批补齐主册【功能定义】「全语义对齐」的序列化/账本/容错面：
//! 区域表快照序列化（每区域一条 16 字节定长记录：基址/大小/状态/保护，
//! pack/parse round-trip）、堆走查诊断器（使用率 permille/碎片率/最大空闲
//! 块三账）、守护页陷阱日志（定长 16 环形：地址+类型+时戳，回绕覆盖最旧）、
//! 分配尺寸分级账（8/16/32/64/128/256/512+ 七桶计数）。
//!
//! 判据对账：主册 G-A-21（7-Zip 基准无降级/VirtualAlloc 压力无泄漏无碎片
//! 失控）+ MS VirtualQuery/HeapWalk 文档语义；序列化格式为域内模型口径
//! （16 字节小端定长）。与主层 memalign.rs、deep/f021d.rs（堆句柄族）、
//! deep2/f021e.rs（四态状态机）语义面互补不重叠。
//!
//! 零堆纪律：定长数组 + &'static str，无 alloc；错误显性化（Err/计数账）。

use crate::checks::CheckSet;

/// MEM_* 状态位（MS VirtualQuery MEMORY_BASIC_INFORMATION.State 真实值）。
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
/// PAGE_* 保护位（MS MemoryProtection 常量真实值）。
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE_READ: u32 = 0x20;

// ---- 区域表快照序列化（16 字节定长记录 pack/parse round-trip）----

/// 单条区域记录长度（小端：基址 4 + 大小 4 + 状态 4 + 保护 4）。
pub const REGION_REC_LEN: usize = 16;
/// 快照区域表容量。
pub const MAX_REGIONS: usize = 16;

/// 一条区域记录（MEMORY_BASIC_INFORMATION 域内四字段投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionRec { pub base: u32, pub size: u32, pub state: u32, pub protect: u32 }/// 区域记录 → 16 字节定长包（小端序列化）。
pub fn pack_region(r: &RegionRec) -> [u8; REGION_REC_LEN] {
    let mut out = [0u8; REGION_REC_LEN];
    out[0..4].copy_from_slice(&r.base.to_le_bytes());
    out[4..8].copy_from_slice(&r.size.to_le_bytes());
    out[8..12].copy_from_slice(&r.state.to_le_bytes());
    out[12..16].copy_from_slice(&r.protect.to_le_bytes());
    out
}

fn le32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

/// 16 字节定长包 → 区域记录；长度不符显性 Err（零静默）。
pub fn parse_region(buf: &[u8]) -> Result<RegionRec, &'static str> {
    if buf.len() != REGION_REC_LEN {
        return Err("bad-record-len");
    }
    Ok(RegionRec { base: le32(buf, 0), size: le32(buf, 4), state: le32(buf, 8), protect: le32(buf, 12) })
}

/// 区域表快照：dump 产出连续定长记录流，load 反向重建（round-trip 面）。
pub struct RegionTable { entries: [Option<RegionRec>; MAX_REGIONS], pub count: usize }

impl RegionTable {
    pub const fn new() -> Self {
        RegionTable { entries: [None; MAX_REGIONS], count: 0 }
    }

    pub fn push(&mut self, r: RegionRec) -> Result<(), &'static str> {
        if self.count >= MAX_REGIONS {
            return Err("region-table-full");
        }
        self.entries[self.count] = Some(r);
        self.count += 1;
        Ok(())
    }

    /// 序列化全部记录；缓冲不足显性 Err。
    pub fn dump(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        if out.len() < self.count * REGION_REC_LEN {
            return Err("dump-buffer-too-small");
        }
        for i in 0..self.count {
            let rec = self.entries[i].map(|r| pack_region(&r)).unwrap_or([0u8; REGION_REC_LEN]);
            out[i * REGION_REC_LEN..(i + 1) * REGION_REC_LEN].copy_from_slice(&rec);
        }
        Ok(self.count * REGION_REC_LEN)
    }

    /// 从记录流重建；非 16 倍数长度/超容显性 Err。
    pub fn load(&mut self, buf: &[u8]) -> Result<usize, &'static str> {
        if buf.len() % REGION_REC_LEN != 0 || buf.len() / REGION_REC_LEN > MAX_REGIONS {
            return Err(if buf.len() % REGION_REC_LEN != 0 { "bad-record-len" } else { "region-table-full" });
        }
        *self = RegionTable::new();
        for (i, chunk) in buf.chunks_exact(REGION_REC_LEN).enumerate() {
            self.entries[i] = Some(parse_region(chunk)?);
        }
        self.count = buf.len() / REGION_REC_LEN;
        Ok(self.count)
    }
}

// ---- 堆走查诊断器（使用率/碎片率/最大空闲块三账）----

/// 块表容量（域内模型口径，与 f021d 堆句柄族同容量）。
pub const WALK_BLOCKS: usize = 32;

/// 一条堆块（块表快照元素）。
#[derive(Clone, Copy)]
pub struct WalkBlock { pub offset: usize, pub size: usize, pub in_use: bool }

/// 堆走查三账（permille 口径；碎片失控预警的数据源）：used=在用/总×1000，
/// frag=非最大空闲块字节/总×1000。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkReport {
    pub used_permille: u32,
    pub frag_permille: u32,
    pub largest_free_bytes: usize,
    pub free_blocks: u32,
}

/// 堆走查诊断器：对块表快照输出三账（HeapWalk 诊断语义的域内投影）。
pub struct HeapWalkDiag { blocks: [Option<WalkBlock>; WALK_BLOCKS], pub count: usize }

impl HeapWalkDiag {
    pub const fn new() -> Self {
        HeapWalkDiag { blocks: [None; WALK_BLOCKS], count: 0 }
    }

    pub fn add_block(&mut self, offset: usize, size: usize, in_use: bool) -> Result<(), &'static str> {
        if self.count >= WALK_BLOCKS {
            return Err("walk-table-full");
        }
        self.blocks[self.count] = Some(WalkBlock { offset, size, in_use });
        self.count += 1;
        Ok(())
    }

    /// 遍历块表产出三账；空表显性 Err。
    pub fn walk(&self) -> Result<WalkReport, &'static str> {
        if self.count == 0 {
            return Err("no-blocks");
        }
        let vis = self.blocks.iter().take(self.count).flatten();
        let (mut used, mut free_total, mut largest, mut free_n) = (0usize, 0usize, 0usize, 0u32);
        let mut total = 0usize;
        for b in vis {
            total += b.size;
            if b.in_use {
                used += b.size;
            } else {
                free_total += b.size;
                free_n += 1;
                largest = largest.max(b.size);
            }
        }
        Ok(WalkReport {
            used_permille: (used * 1000 / total) as u32,
            frag_permille: ((free_total - largest) * 1000 / total) as u32,
            largest_free_bytes: largest,
            free_blocks: free_n,
        })
    }
}

// ---- 守护页陷阱日志（定长 16 环形：地址+类型+时戳）----

/// 环形日志容量。
pub const TRAP_RING: usize = 16;
/// 陷阱类型：守护页触碰（PAGE_GUARD 复位语义，deep/f021d 分类入口上游）。
pub const TRAP_KIND_GUARD: u8 = 1;
/// 陷阱类型：不可访问页触碰。
pub const TRAP_KIND_NOACCESS: u8 = 2;

/// 一条陷阱记录（地址 + 类型 + 毫秒时戳）。
/// 一条陷阱记录（地址 + 类型 + 毫秒时戳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrapRec { pub addr: u32, pub kind: u8, pub ts_ms: u64 }

/// 守护页陷阱日志：定长 16 环形，写满回绕覆盖最旧（累计数不丢——账面自洽）。
pub struct TrapRing { buf: [Option<TrapRec>; TRAP_RING], head: usize, total: u64 }

impl TrapRing {
    pub const fn new() -> Self {
        TrapRing { buf: [None; TRAP_RING], head: 0, total: 0 }
    }

    pub fn push(&mut self, addr: u32, kind: u8, ts_ms: u64) {
        self.buf[self.head] = Some(TrapRec { addr, kind, ts_ms });
        self.head = (self.head + 1) % TRAP_RING;
        self.total += 1;
    }

    /// 累计入环条数（含已被回绕覆盖的）。
    pub fn total(&self) -> u64 {
        self.total
    }
    /// 按时间序（最旧→最新）快照；返回（槽表，有效条数）。
    pub fn snapshot(&self) -> ([Option<TrapRec>; TRAP_RING], usize) {
        let valid = self.total.min(TRAP_RING as u64) as usize;
        let start = if self.total > TRAP_RING as u64 { self.head } else { 0 };
        let mut out: [Option<TrapRec>; TRAP_RING] = [None; TRAP_RING];
        for i in 0..valid {
            out[i] = self.buf[(start + i) % TRAP_RING];
        }
        (out, valid)
    }
}

// ---- 分配尺寸分级账（8/16/32/64/128/256/512+ 七桶）----

/// 分级桶数（七桶：8/16/32/64/128/256/512+，以桶下界命名）。
pub const SIZE_CLASS_COUNT: usize = 7;
/// 七桶下界（桶 k = [BOUNDS[k], BOUNDS[k+1])，末桶无上沿即 512+）。
pub const SIZE_CLASS_BOUNDS: [usize; SIZE_CLASS_COUNT] = [8, 16, 32, 64, 128, 256, 512];

/// 尺寸 → 桶号（8→0、16→1、…、512→6、4096→6；小于 8 如实落首桶）。
pub fn size_bucket(size: usize) -> usize {
    let mut b = 0usize;
    while b + 1 < SIZE_CLASS_COUNT && size >= SIZE_CLASS_BOUNDS[b + 1] {
        b += 1;
    }
    b
}

/// 分级账：七桶计数 + 总数（碎片成因归因的数据面）。
pub struct SizeClassLedger { pub counts: [u32; SIZE_CLASS_COUNT], pub total: u32 }

impl SizeClassLedger {
    pub const fn new() -> Self {
        SizeClassLedger { counts: [0; SIZE_CLASS_COUNT], total: 0 }
    }

    pub fn record(&mut self, size: usize) {
        self.counts[size_bucket(size)] += 1;
        self.total += 1;
    }

    /// 某桶占比 permille（总账为零或桶号越界如实返回 0）。
    pub fn share_permille(&self, bucket: usize) -> u32 {
        if self.total == 0 || bucket >= SIZE_CLASS_COUNT {
            return 0;
        }
        (self.counts[bucket] as u64 * 1000 / self.total as u64) as u32
    }
}

/// 域自检（深化批次四）。
pub fn run_f021f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F021-memdiag-d4");
    // 1) 区域记录 pack/parse round-trip（字段全保真）。
    let rec = RegionRec { base: 0x0040_0000, size: 0x1_0000, state: MEM_COMMIT, protect: PAGE_READWRITE };
    cs.add("region_pack_roundtrip", parse_region(&pack_region(&rec)) == Ok(rec), "");
    // 2) 区域表 dump/load round-trip（三区域快照往返一致）。
    let mut t = RegionTable::new();
    let _ = t.push(rec);
    let _ = t.push(RegionRec { base: 0x1000_0000, size: 0x2000, state: MEM_RESERVE, protect: PAGE_NOACCESS });
    let _ = t.push(RegionRec { base: 0x7ffe_0000, size: 0x1000, state: MEM_COMMIT, protect: PAGE_READONLY });
    let mut buf = [0u8; MAX_REGIONS * REGION_REC_LEN];
    let n = t.dump(&mut buf).expect("缓冲必足");
    let mut t2 = RegionTable::new();
    let loaded = t2.load(&buf[..n]).expect("记录流合法");
    cs.add("region_dump_load_roundtrip", loaded == 3 && t2.count == 3
        && t2.entries[2].map(|r| r.protect).unwrap_or(0) == PAGE_READONLY, "");
    // 3) 非法记录长度显性 Err（零静默）。
    cs.add("region_parse_bad_len", parse_region(&[0u8; 15]) == Err("bad-record-len"), "");
    // 4) 堆走查三账：60 在用 + 20/20 空闲 → 使用率 600‰、最大空闲 20、碎片率 200‰。
    let mut w = HeapWalkDiag::new();
    let _ = w.add_block(0, 60, true);
    let _ = w.add_block(64, 20, false);
    let _ = w.add_block(128, 20, false);
    let rep = w.walk().expect("非空表必成");
    cs.add("heap_walk_accounts", rep.used_permille == 600 && rep.frag_permille == 200
        && rep.largest_free_bytes == 20 && rep.free_blocks == 2, "");
    // 5) 空表走查显性 Err。
    cs.add("heap_walk_empty_err", HeapWalkDiag::new().walk() == Err("no-blocks"), "");
    // 6) 守护页陷阱环形日志：20 条入环 → 累计账 20、有效快照 16、最旧为第 4 条。
    let mut ring = TrapRing::new();
    for i in 0..20u64 {
        ring.push(0x0040_0000 + (i as u32) * 0x1000, TRAP_KIND_GUARD, 1000 + i);
    }
    let (snap, valid) = ring.snapshot();
    cs.add("trap_ring_wrap", ring.total() == 20 && valid == 16
        && snap[0].map(|t| t.ts_ms).unwrap_or(0) == 1004
        && snap[15].map(|t| t.ts_ms).unwrap_or(0) == 1019, "");
    // 7) 快照时间序（未回绕时最旧→最新，类型如实记录）。
    let mut r2 = TrapRing::new();
    r2.push(0x1000, TRAP_KIND_GUARD, 7);
    r2.push(0x2000, TRAP_KIND_NOACCESS, 8);
    let (s2, v2) = r2.snapshot();
    cs.add("trap_snapshot_order", v2 == 2 && s2[0].map(|t| t.ts_ms) == Some(7)
        && s2[1].map(|t| t.kind) == Some(TRAP_KIND_NOACCESS), "");
    // 8) 尺寸分级边界：8→0、9→0、16→1、257→5、512→6、4096→6。
    let edges = [size_bucket(8), size_bucket(9), size_bucket(16), size_bucket(257), size_bucket(512), size_bucket(4096)];
    cs.add("size_bucket_edges", edges == [0, 0, 1, 5, 6, 6], "");
    // 9) 分级账计数与占比（300/400 落 256 桶、600/700 落 512+ 桶 → 各 250‰）。
    let mut led = SizeClassLedger::new();
    for sz in [300usize, 400, 600, 700, 10, 20, 30, 40] {
        led.record(sz);
    }
    cs.add("size_ledger_share", led.total == 8 && led.counts[5] == 2 && led.counts[6] == 2
        && led.share_permille(5) == 250 && led.share_permille(6) == 250, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_all_used_has_zero_frag() {
        let mut w = HeapWalkDiag::new();
        for i in 0..8usize {
            w.add_block(i * 64, 64, true).expect("容量内必成");
        }
        let rep = w.walk().expect("非空表必成");
        assert_eq!(rep.used_permille, 1000);
        assert_eq!(rep.frag_permille, 0);
        assert_eq!(rep.largest_free_bytes, 0);
    }

    #[test]
    fn trap_ring_overwrite_keeps_newest() {
        let mut r = TrapRing::new();
        for i in 0..17u64 {
            r.push(i as u32, TRAP_KIND_GUARD, i);
        }
        let (s, v) = r.snapshot();
        assert_eq!(v, 16);
        // 第 0 条已被覆盖，最旧为 ts=1，最新为 ts=16。
        assert_eq!(s[0].map(|t| t.ts_ms), Some(1));
        assert_eq!(s[15].map(|t| t.ts_ms), Some(16));
        assert_eq!(r.total(), 17);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f021f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
