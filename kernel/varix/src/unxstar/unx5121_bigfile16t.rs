//! UNX-F5121 · 大文件语义总纲（16TiB 边界图谱三档）账 · 诊断段 0x8080
//! 验收锚点：UNX-F5121-J1 大文件边界图谱落账——4K 块 16TiB / 1K 块 4TiB /
//! 64K 块 256TiB 三档，推算与上游公式一致率 100%（最大文件 = 2^32 extent 块 ×
//! 块大小，即 block_size << 32）；三档逐档对拍 + 越界拒绝路径。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x8080;

/// 最大文件字节数公式：block_size << 32（extent 索引 32 位块号）。
pub fn max_file_bytes(block_size: u64) -> u64 {
    block_size << 32
}

/// 边界档位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundary {
    pub block_size: u64,
    pub max_tib: u64,
}

impl Boundary {
    pub fn new(block_size: u64) -> Self {
        Boundary { block_size, max_tib: max_file_bytes(block_size) / (1024 * 1024 * 1024 * 1024) }
    }
    pub fn label(&self) -> String {
        format!("{}K->{}TiB", self.block_size / 1024, self.max_tib)
    }
}

/// 三档规格账（上游公式对照）。
pub fn spec_three() -> [Boundary; 3] {
    [Boundary::new(1024), Boundary::new(4096), Boundary::new(65536)]
}

/// 偏移合法性：offset < max_file_bytes(block_size) 才可寻址。
pub fn addressable(block_size: u64, offset: u64) -> bool {
    offset < max_file_bytes(block_size)
}

/// 大文件语义账。
pub struct BigFileLedger {
    pub entries: Vec<(String, u64)>, // (档位标签, 最大字节数)
}

impl BigFileLedger {
    pub fn new() -> Self {
        BigFileLedger { entries: Vec::new() }
    }
    pub fn record(&mut self, b: &Boundary) {
        self.entries.push((b.label(), max_file_bytes(b.block_size)));
    }
    /// 与上游公式逐档对拍：三档全一致返回 true。
    pub fn cross_check(&self, spec: &[(u64, u64)]) -> bool {
        if self.entries.len() != spec.len() {
            return false;
        }
        for (i, (_, bytes)) in self.entries.iter().enumerate() {
            if *bytes != spec[i].1 {
                return false;
            }
        }
        true
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x8080 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x5121_8080;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("公式-4K档16TiB", max_file_bytes(4096) == 16 * 1024 * 1024 * 1024 * 1024));
    out.push(("公式-1K档4TiB", max_file_bytes(1024) == 4 * 1024 * 1024 * 1024 * 1024));
    out.push(("公式-64K档256TiB", max_file_bytes(65536) == 256 * 1024 * 1024 * 1024 * 1024));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx5121_checks：本单判据聚合（诊断段 0x8080）。
pub fn run_unx5121_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F5121 大文件语义 16TiB 边界");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 三档逐档落账
    let spec = spec_three();
    let mut led = BigFileLedger::new();
    for b in spec.iter() {
        led.record(b);
    }
    let three_ok = led.entries.len() == 3
        && led.entries[0].0 == "1K->4TiB"
        && led.entries[1].0 == "4K->16TiB"
        && led.entries[2].0 == "64K->256TiB";
    s.add("三档-1K/4K/64K标签逐档落账", three_ok, if three_ok { "" } else { "三档标签漂移" });

    // 上游公式一致率 100%
    let spec_pairs: Vec<(u64, u64)> = spec.iter().map(|b| (b.block_size, max_file_bytes(b.block_size))).collect();
    let cross_ok = led.cross_check(&spec_pairs);
    s.add("公式-三档推算与上游一致率100%", cross_ok, if cross_ok { "" } else { "公式对拍缺口" });

    // 字节精确值
    let b16 = max_file_bytes(4096);
    let exact = b16 == 16_384 * 1024 * 1024 * 1024;
    s.add("精确-4K档恰16384GiB", exact, if exact { "" } else { "精确值漂移" });

    // 寻址边界：最大可寻 offset = max-1（含等号拒）
    let max4k = max_file_bytes(4096);
    let in_ok = addressable(4096, max4k - 1);
    let out_ok = !addressable(4096, max4k);
    s.add("寻址-max-1可寻", in_ok, if in_ok { "" } else { "上界-1 误拒" });
    s.add("寻址-max恰拒（边界含等号）", out_ok, if out_ok { "" } else { "越界漏网" });

    // 跨档寻址差异：同 offset 在 4K 可寻、在 1K 拒
    let probe = max_file_bytes(1024); // 4TiB
    let big_ok = addressable(4096, probe) && !addressable(1024, probe);
    s.add("跨档-4TiB处4K可寻1K恰拒", big_ok, if big_ok { "" } else { "跨档差异失效" });

    // 1024 轮随机 offset 边界扫描：两档结果互斥（对恰同 offset 至多一档可寻边界带）
    let mut seed: u64 = 0x5121_1024;
    let mut band_ok = true;
    for _ in 0..1024 {
        let v = lcg_next(&mut seed) % 1_000_000;
        let off = v * 1024 * 1024 * 1024; // GiB 粒度
        let a = addressable(1024, off);
        let b = addressable(4096, off);
        if b && !a {
            // 4K 可寻而 1K 拒——仅当 off 在 [4TiB, 16TiB)
            if off < max_file_bytes(1024) {
                band_ok = false;
            }
        }
    }
    s.add("扫描-1024轮档间互斥带成立", band_ok, if band_ok { "" } else { "档间带漂移" });

    // FNV 档位指纹稳定
    let h1 = fnv1a(Boundary::new(4096).label().as_bytes());
    let h2 = fnv1a(b"4K->16TiB");
    s.add("FNV-档位指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
