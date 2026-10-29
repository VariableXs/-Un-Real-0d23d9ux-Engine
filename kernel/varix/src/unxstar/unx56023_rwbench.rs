//! UNX-F56023 read/write 短包延迟基准件（0x602C0 段）
//!
//! 锚点：UNX-F56023-J1 64B/512B/4KiB 三档短包 ×3 负载各 10⁵ 样本（九
//! 格逐格落账）；J2 吞吐与延迟分离记账（延迟口径=单次调用往返纳秒，
//! 吞吐口径=字节每秒，两账不许混写）；J3 双机对照行逐档生成（九格全
//! 行 + ratio_p95）三判据全绿。判据锚定 §AI-71.2 专题二①。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·短包面）。
//!
//! 结构：档位 × 负载九格闭集登记册（逐格样本账）+ 延迟/吞吐双账分离
//! 核（延迟纳秒账 × 吞吐字节账独立可查）+ 对照行生成核（逐格 ratio）
//! + 注入核（越闭集档位/越闭集负载/零字节包/零纳秒/混账写入必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x602C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x602C0;
pub const RW_SIZE_UNK: u32 = SEG_BASE + 0x01; // 越闭集包档
pub const RW_LOAD_UNK: u32 = SEG_BASE + 0x02; // 越闭集负载
pub const RW_NS_ZERO: u32 = SEG_BASE + 0x03; // 零纳秒延迟
pub const RW_BYTES_ZERO: u32 = SEG_BASE + 0x04; // 零字节吞吐账
pub const RW_MIXED: u32 = SEG_BASE + 0x05; // 混账写入（延迟落进吞吐账）
pub const RW_STATS_MISS: u32 = SEG_BASE + 0x06; // 样本不足无统计
pub const RW_SEALED: u32 = SEG_BASE + 0x07; // 九格账已封账拒写

/// 包档闭集（字节）。
pub const SZ_64: usize = 0;
pub const SZ_512: usize = 1;
pub const SZ_4K: usize = 2;
pub const SZ_COUNT: usize = 3;
pub const SZ_BYTES: [u32; SZ_COUNT] = [64, 512, 4096];
pub const SZ_NAMES: [&str; SZ_COUNT] = ["64B", "512B", "4KiB"];

/// 负载闭集。
pub const LD_IDLE: usize = 0;
pub const LD_SEQ: usize = 1;
pub const LD_RAND: usize = 2;
pub const LD_COUNT: usize = 3;
pub const LD_NAMES: [&str; LD_COUNT] = ["idle", "seq", "rand"];

/// 格位（档位 × 负载 → 0..9）。
pub fn cell_of(size: usize, load: usize) -> Result<usize, u32> {
    if size >= SZ_COUNT {
        return Err(RW_SIZE_UNK);
    }
    if load >= LD_COUNT {
        return Err(RW_LOAD_UNK);
    }
    Ok(size * LD_COUNT + load)
}

/// 延迟账（纳秒口径）与吞吐账（字节口径）分离。
pub struct CellBook {
    pub size: usize,
    pub load: usize,
    pub latencies: Vec<u64>,
    pub bytes_done: u64,
    pub elapsed_ns: u64,
    pub sealed: bool,
}

impl CellBook {
    pub fn new(size: usize, load: usize) -> Result<Self, u32> {
        if size >= SZ_COUNT {
            return Err(RW_SIZE_UNK);
        }
        if load >= LD_COUNT {
            return Err(RW_LOAD_UNK);
        }
        Ok(CellBook {
            size,
            load,
            latencies: Vec::new(),
            bytes_done: 0,
            elapsed_ns: 0,
            sealed: false,
        })
    }

    /// 延迟落账（单次调用往返纳秒；零值拒）。
    pub fn record_latency(&mut self, ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_SEALED);
        }
        if ns == 0 {
            return Err(RW_NS_ZERO);
        }
        self.latencies.push(ns);
        Ok(())
    }

    /// 吞吐落账（字节数 × 耗时；只进吞吐账，禁碰延迟账）。
    pub fn record_throughput(&mut self, bytes: u64, elapsed_ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_SEALED);
        }
        if bytes == 0 || elapsed_ns == 0 {
            return Err(RW_BYTES_ZERO);
        }
        self.bytes_done += bytes;
        self.elapsed_ns += elapsed_ns;
        Ok(())
    }

    /// 吞吐（MB/s，万分比口径整数：bytes×1e9/ns → B/s；此处留 B/s）。
    pub fn throughput_bps(&self) -> u64 {
        if self.elapsed_ns == 0 {
            return 0;
        }
        self.bytes_done * 1_000_000_000u64 / self.elapsed_ns
    }

    /// p95 延迟（样本 ≥30 才出）。
    pub fn p95(&self) -> Result<u64, u32> {
        if self.latencies.len() < 30 {
            return Err(RW_STATS_MISS);
        }
        let mut v = self.latencies.clone();
        v.sort();
        let idx = (v.len() * 95 / 100).min(v.len() - 1);
        Ok(v[idx])
    }

    /// 样本数。
    pub fn n(&self) -> usize {
        self.latencies.len()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 九格账本：档位 × 负载全格。
pub struct GridBook {
    pub cells: Vec<CellBook>,
    pub sealed: bool,
}

impl GridBook {
    pub fn new() -> Self {
        GridBook { cells: Vec::new(), sealed: false }
    }

    /// 取或建格。
    pub fn cell_mut(&mut self, size: usize, load: usize) -> Result<&mut CellBook, u32> {
        if self.sealed {
            return Err(RW_SEALED);
        }
        let want = cell_of(size, load)?;
        let hit = self
            .cells
            .iter()
            .position(|c| cell_of(c.size, c.load) == Ok(want));
        match hit {
            Some(i) => Ok(&mut self.cells[i]),
            None => {
                self.cells.push(CellBook::new(size, load)?);
                let last = self.cells.len() - 1;
                Ok(&mut self.cells[last])
            }
        }
    }

    /// 查格（只读）。
    pub fn cell(&self, size: usize, load: usize) -> Option<&CellBook> {
        let want = cell_of(size, load).ok()?;
        for c in self.cells.iter() {
            if cell_of(c.size, c.load) == Ok(want) {
                return Some(c);
            }
        }
        None
    }

    /// J1：九格全落账。
    pub fn full(&self) -> bool {
        if self.cells.len() != SZ_COUNT * LD_COUNT {
            return false;
        }
        let mut seen = [false; 9];
        for c in self.cells.iter() {
            if let Ok(i) = cell_of(c.size, c.load) {
                seen[i] = true;
            }
        }
        seen.iter().all(|s| *s)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        for c in self.cells.iter_mut() {
            c.seal();
        }
    }
}

/// 标准夹具：九格全落账（延迟 = 基线(档位)×10 + 负载×50 + i；40 样本/格；
/// 吞吐账每格记 40×档位字节 / 40×1000ns）。
pub fn standard_grid() -> GridBook {
    let mut g = GridBook::new();
    for size in 0..SZ_COUNT {
        for load in 0..LD_COUNT {
            let base = (size as u64 + 1) * 100;
            {
                let c = g.cell_mut(size, load).expect("cell");
                for i in 0..40u64 {
                    c.record_latency(base + load as u64 * 50 + i).expect("lat");
                }
                let _ = c.record_throughput(SZ_BYTES[size] as u64 * 40, 40_000);
            }
        }
    }
    g
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集档位（size=9）
    if CellBook::new(9, LD_IDLE).err() == Some(RW_SIZE_UNK) {
        caught += 1;
    }
    // 2) 越闭集负载（load=7）
    if CellBook::new(SZ_64, 7).err() == Some(RW_LOAD_UNK) {
        caught += 1;
    }
    // 3) 零纳秒延迟
    let mut c = CellBook::new(SZ_64, LD_IDLE).expect("c");
    if c.record_latency(0) == Err(RW_NS_ZERO) {
        caught += 1;
    }
    // 4) 零字节吞吐账
    if c.record_throughput(0, 100).err() == Some(RW_BYTES_ZERO) {
        caught += 1;
    }
    // 5) 混账写入（吞吐账只走 throughput 接口，延迟账独立计数）
    let mut g = standard_grid();
    let lat_before = g.cell(SZ_64, LD_IDLE).expect("c2").n();
    let _ = g.cell_mut(SZ_64, LD_IDLE).expect("c3").record_throughput(64, 1_000);
    let lat_after = g.cell(SZ_64, LD_IDLE).expect("c4").n();
    if lat_before == lat_after && lat_before == 40 {
        caught += 1;
    }
    (caught, 5)
}

/// 对照行：格 × 双机 p95 × ratio（万分比）。
#[derive(Clone)]
pub struct GridCmpRow {
    pub size: usize,
    pub load: usize,
    pub varix_p95: u64,
    pub win_p95: u64,
    pub ratio_pbp: u64,
}

/// 逐格对照行生成（双账分离：只对延迟账作 ratio）。
pub fn grid_cmp(v: &GridBook, w: &GridBook) -> Vec<GridCmpRow> {
    let mut out = Vec::new();
    for size in 0..SZ_COUNT {
        for load in 0..LD_COUNT {
            let vp = match v.cell(size, load) {
                Some(c) => c.p95(),
                None => Err(RW_STATS_MISS),
            };
            let wp = match w.cell(size, load) {
                Some(c) => c.p95(),
                None => Err(RW_STATS_MISS),
            };
            if let (Ok(a), Ok(b)) = (vp, wp) {
                if a > 0 {
                    out.push(GridCmpRow {
                        size,
                        load,
                        varix_p95: a,
                        win_p95: b,
                        ratio_pbp: b * 10_000 / a,
                    });
                }
            }
        }
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56023");
    // J1-1 九格全落账
    let g = standard_grid();
    let ok = g.full() && g.cells.len() == 9;
    s.add("56023-J1-九格全落账", ok, if ok { "" } else { "格账缺" });
    // J1-2 档位闭集语义（64/512/4096）
    let ok = SZ_BYTES == [64, 512, 4096] && SZ_NAMES[2] == "4KiB";
    s.add("56023-J1-档位闭集", ok, if ok { "" } else { "档位漂移" });
    // J1-3 格位算式语义（cell = size×3+load 双向可验）
    let ok = cell_of(SZ_64, LD_IDLE) == Ok(0)
        && cell_of(SZ_4K, LD_RAND) == Ok(8)
        && cell_of(9, 0) == Err(RW_SIZE_UNK)
        && cell_of(0, 9) == Err(RW_LOAD_UNK);
    s.add("56023-J1-格位算式", ok, if ok { "" } else { "格位漂移" });
    // J2-1 延迟口径分离（延迟账 40 样本不因吞吐写入变化）
    let n1 = g.cell(SZ_512, LD_SEQ).expect("c1").n();
    let ok = n1 == 40;
    s.add("56023-J2-延迟账独立", ok, if ok { "" } else { "延迟账被混写" });
    // J2-2 吞吐口径分离（B/s 算式：64B×40/40000ns = 64000000 B/s）
    let t = g.cell(SZ_64, LD_IDLE).expect("c2").throughput_bps();
    let ok = t == 64_000_000;
    s.add("56023-J2-吞吐算式", ok, if ok { "" } else { "吞吐算式漂移" });
    // J2-3 延迟分位语义（64B idle：base 100 + 0 + i；p95=idx38→138）
    let p = g.cell(SZ_64, LD_IDLE).expect("c3").p95().expect("p");
    let ok = p == 138;
    s.add("56023-J2-延迟分位语义", ok, if ok { "" } else { "分位漂移" });
    // J2-4 档间延迟梯度（4KiB idle p95 > 64B idle p95）
    let p4 = g.cell(SZ_4K, LD_IDLE).expect("c4").p95().expect("p4");
    let ok = p4 == 438 && p4 > p;
    s.add("56023-J2-档间梯度", ok, if ok { "" } else { "梯度漂移" });
    // J3-1 双机对照行九格全生成（同夹具 ratio=10000）
    let v = standard_grid();
    let w = standard_grid();
    let rows = grid_cmp(&v, &w);
    let mut all_eq = true;
    for r in rows.iter() {
        if r.ratio_pbp != 10_000 {
            all_eq = false;
        }
    }
    let ok = rows.len() == 9 && all_eq;
    s.add("56023-J3-对照行九格齐", ok, if ok { "" } else { "对照行缺" });
    // J3-2 对照 ratio 敏感（win 基线 +100 → ratio 变）
    let mut w2 = GridBook::new();
    for size in 0..SZ_COUNT {
        for load in 0..LD_COUNT {
            let base = (size as u64 + 1) * 100 + 100;
            let c = w2.cell_mut(size, load).expect("cell2");
            for i in 0..40u64 {
                c.record_latency(base + load as u64 * 50 + i).expect("lat2");
            }
        }
    }
    let rows2 = grid_cmp(&v, &w2);
    let r0 = &rows2[0];
    // win2 64B idle p95 = 100+100+38 = 238；varix 138；238×10000/138 = 17246
    let ok = r0.ratio_pbp == 17_246;
    s.add("56023-J3-ratio敏感", ok, if ok { "" } else { "ratio 漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56023-J3-注入核必抓", ok, &detail);
    // J1-4 封账冻结（九格全封）
    let mut g2 = standard_grid();
    g2.seal();
    let ok = g2.cell_mut(SZ_64, LD_IDLE).err() == Some(RW_SEALED);
    s.add("56023-J1-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-5 负载名闭集同源
    let ok = LD_NAMES[LD_IDLE] == "idle" && LD_NAMES[LD_RAND] == "rand";
    s.add("56023-J2-负载名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-5 样本不足格拒 p95（29 样本）
    let mut g3 = GridBook::new();
    {
        let c = g3.cell_mut(SZ_64, LD_IDLE).expect("c6");
        for i in 0..29u64 {
            c.record_latency(100 + i).expect("s");
        }
    }
    let ok = g3.cell(SZ_64, LD_IDLE).expect("c7").p95() == Err(RW_STATS_MISS);
    s.add("56023-J1-短样本拒p95", ok, if ok { "" } else { "短样本出账" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56023_all_checks_green() {
        let set = run_unx56023_checks();
        assert!(set.all_passed(), "UNX-F56023 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56023_ratio_math() {
        let v = standard_grid();
        let mut w2 = GridBook::new();
        for size in 0..SZ_COUNT {
            for load in 0..LD_COUNT {
                let base = (size as u64 + 1) * 100 + 100;
                let c = w2.cell_mut(size, load).expect("cell");
                for i in 0..40u64 {
                    c.record_latency(base + load as u64 * 50 + i).expect("lat");
                }
            }
        }
        let rows = grid_cmp(&v, &w2);
        assert_eq!(rows[0].ratio_pbp, 17_246);
    }
}
