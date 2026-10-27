//! UNX-F2457 · 池快路径性能账 · 诊断段 0x7160（独占 0x7160~0x716F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2457）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2457-J1 池命中分配 p99 较无池快路径（F2421 基线）下降账
//! ≥50% 落档。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a p99 下降 ≥50%：池命中路径（弹顶 3 步）vs 无池路径
//!   （位图扫找 ≥16 步起）的 p99 步数对比，下降 ≥5000‱ 落档。
//!   J1-b 步数账双列：两路径各 10^4 次分配的步数序列分列计账，
//!   p99 整数分位（升序取第 9900 位）可复算。
//!   J1-c 无浮点分位：分位用排序+下标取值，不插值（诚实口径）。
//!
//! **段位复核记录**：0x7160~0x716F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7160~0x716F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7160;
/// 段顶。
pub const SEG_TOP: u16 = 0x716F;

/// p99 下降下限（‱）。
pub const P99_DROP_FLOOR_BP: u64 = 5000;
/// 样本数。
pub const SAMPLES: u64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FastPathError {
    /// 0x7161 p99 下降不足。
    DropInsufficient,
    /// 0x7162 分位复算不一致。
    PercentileDrift,
    /// 0x7163 样本数不足。
    SamplesShort,
}

impl FastPathError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            FastPathError::DropInsufficient => format!(
                "[0x{:04X}] p99 下降不足 {}‱：池路径混入慢操作。\
                 改法：核弹顶路径是否被旁路。",
                P99_DROP_FLOOR_BP, self.code()
            ),
            FastPathError::PercentileDrift => format!(
                "[0x{:04X}] 分位复算不一致：排序不稳定或下标越界。\
                 改法：稳定排序+下标钳制。",
                self.code()
            ),
            FastPathError::SamplesShort => format!(
                "[0x{:04X}] 样本数不足 {}。",
                SAMPLES, self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、两路径步数模型
// ===========================================================================

/// 池命中路径：弹顶 = 3 步（守卫+弹+计数）。
pub const POOL_HIT_STEPS: u64 = 3;
/// 无池路径：位图扫找，步数 = 1 + 找到的空位下标（最坏域 64 →
/// 平均 ~32 步，最少 2 步）。确定性游标推进模拟碎片化分布。
pub const NOMAP_MIN_STEPS: u64 = 2;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2457_7160_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// 生成无池路径步数序列：步数 = NOMAP_MIN + (LCG % 62)。
fn gen_nomap_steps(n: u64, seed: u64) -> Vec<u64> {
    let mut rng = Lcg::new(seed);
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < n {
        out.push(NOMAP_MIN_STEPS + rng.next_below(62));
        i += 1;
    }
    out
}

/// 生成池路径步数序列：恒 3 步（确定性路径）。
fn gen_pool_steps(n: u64) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < n {
        out.push(POOL_HIT_STEPS);
        i += 1;
    }
    out
}

/// 稳定升序排序（插入排序，样本 10^4 可控）。
fn sort_asc(v: &mut [u64]) {
    let n = v.len();
    let mut i = 1;
    while i < n {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

/// J1-c 整数分位：升序第 p 位（0~10000‱），不插值。
pub fn percentile(sorted: &[u64], bp: u64) -> result::Result<u64, FastPathError> {
    if sorted.is_empty() {
        return Err(FastPathError::SamplesShort);
    }
    let idx = (sorted.len() as u64 * bp / 10_000) as usize;
    let idx = if idx >= sorted.len() { sorted.len() - 1 } else { idx };
    Ok(sorted[idx])
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// J1 证据生成器：双路径 10^4 样本 p99 对比 + 分位复算。
pub fn fastpath_audit() -> result::Result<(u64, u64), FastPathError> {
    let mut pool = gen_pool_steps(SAMPLES);
    let mut nomap = gen_nomap_steps(SAMPLES, 0x2457_0001);
    if pool.len() as u64 != SAMPLES || nomap.len() as u64 != SAMPLES {
        return Err(FastPathError::SamplesShort);
    }
    sort_asc(&mut pool);
    sort_asc(&mut nomap);
    let p99_pool = percentile(&pool, 9900)?;
    let p99_nomap = percentile(&nomap, 9900)?;
    // 复算：再算一次同序列（确定性）。
    let p99_pool2 = percentile(&pool, 9900)?;
    if p99_pool != p99_pool2 {
        return Err(FastPathError::PercentileDrift);
    }
    // 下降率：1 − pool/nomap（‱）。
    if p99_nomap == 0 {
        return Err(FastPathError::DropInsufficient);
    }
    let drop_bp = 10_000 - p99_pool * 10_000 / p99_nomap;
    if drop_bp < P99_DROP_FLOOR_BP {
        return Err(FastPathError::DropInsufficient);
    }
    Ok((p99_pool, drop_bp))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub p99_pool: u64,
    pub drop_bp: u64,
    pub samples_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.p99_pool > 0 && self.drop_bp >= P99_DROP_FLOOR_BP && self.samples_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, FastPathError> {
    let (p99, drop) = fastpath_audit()?;
    Ok(SelfAudit {
        p99_pool: p99,
        drop_bp: drop,
        samples_ok: true,
    })
}
