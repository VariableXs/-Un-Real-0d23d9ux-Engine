//! UNX-F56127 帧率采集自隔离件（0x60FC0 段）
//!
//! 锚点：UNX-F56127-J1 探针全量（采集器 × 帧预算 × 采集开销逐条落
//! 账）；J2 隔离语义（单样本开销 ≤2% 硬帽、全体均值开销 ≤1% 断言、
//! 开销不超帧预算）；J3 探针审计链（指纹、封账门、渲染完整）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·自隔离面）。
//!
//! 结构：探针登记簿（采集器闭集、预算带）+ 开销核（万分比定点、
//! u128 中转）+ 断言闭式（均值 bp ≤ 100）+ 指纹链 + 注入核（零预算/
//! 开销超预算/未知采集器/单样本超帽/均值超限/封账拒录/空探针集必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60FC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60FC0;
pub const IS_BUDGET_ZERO: u32 = SEG_BASE + 0x01; // 零/过小预算
pub const IS_OVER_BUDGET: u32 = SEG_BASE + 0x02; // 开销超帧预算
pub const IS_UNKNOWN_COL: u32 = SEG_BASE + 0x03; // 未知采集器
pub const IS_SAMPLE_CAP: u32 = SEG_BASE + 0x04; // 单样本开销超帽
pub const IS_MEAN_OVER: u32 = SEG_BASE + 0x05; // 均值开销超 1%
pub const IS_SEALED: u32 = SEG_BASE + 0x06; // 封账/账满拒录
pub const IS_EMPTY: u32 = SEG_BASE + 0x07; // 空探针集

/// 采集器闭集（5 器）。
pub const COLLECTORS: [&str; 5] =
    ["frame_sampler", "gap_meter", "glitch_scan", "stat_core", "archiver"];

pub fn collector_ok(idx: usize) -> bool {
    idx < COLLECTORS.len()
}

/// 帧预算合法带（微秒）。
pub const BUDGET_MIN: u64 = 1_000;
pub const BUDGET_MAX: u64 = 100_000_000;

/// 单样本开销硬帽（万分比 200 = 2%）。
pub const SAMPLE_CAP_BP: u64 = 200;
/// 均值开销断言（万分比 100 = 1%）。
pub const MEAN_LIMIT_BP: u64 = 100;

/// 开销核：rate_bp = overhead × 10000 / budget（u128 中转）。
pub fn rate_of(overhead_us: u64, budget_us: u64) -> u64 {
    (overhead_us as u128 * 10_000 / budget_us as u128) as u64
}

/// 探针样本。
pub struct IsolSample {
    pub no: usize,
    pub col: usize,
    pub budget_us: u64,
    pub overhead_us: u64,
    pub rate_bp: u64,
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

/// 自隔离探针登记簿。
pub struct IsolProbe {
    cap: usize,
    samples: Vec<IsolSample>,
    chain: u32,
    sealed: bool,
}

impl IsolProbe {
    pub fn new(cap: usize) -> Self {
        IsolProbe {
            cap,
            samples: Vec::new(),
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 样本切片只读访问。
    pub fn samples(&self) -> &[IsolSample] {
        &self.samples
    }

    /// 落一样本：预算带 → 采集器闭集 → 预算覆盖 → 单样本帽。
    pub fn record(&mut self, col: usize, budget_us: u64, overhead_us: u64) -> Result<usize, u32> {
        if self.sealed || self.samples.len() >= self.cap {
            return Err(IS_SEALED);
        }
        if budget_us < BUDGET_MIN || budget_us > BUDGET_MAX {
            return Err(IS_BUDGET_ZERO);
        }
        if !collector_ok(col) {
            return Err(IS_UNKNOWN_COL);
        }
        if overhead_us > budget_us {
            return Err(IS_OVER_BUDGET);
        }
        let rate_bp = rate_of(overhead_us, budget_us);
        if rate_bp > SAMPLE_CAP_BP {
            return Err(IS_SAMPLE_CAP);
        }
        let no = self.samples.len();
        self.samples.push(IsolSample {
            no,
            col,
            budget_us,
            overhead_us,
            rate_bp,
        });
        self.chain = fold_u32(self.chain, col as u32);
        self.chain = fold_u32(self.chain, budget_us as u32);
        self.chain = fold_u32(self.chain, overhead_us as u32);
        self.chain = fold_u32(self.chain, rate_bp as u32);
        Ok(no)
    }

    /// 均值开销断言：空集拒绝、均值 ≤ 1%。
    pub fn probe_mean(&self) -> Result<u64, u32> {
        if self.samples.is_empty() {
            return Err(IS_EMPTY);
        }
        let sum: u64 = self.samples.iter().map(|s| s.rate_bp).sum();
        let mean = sum / self.samples.len() as u64;
        if mean > MEAN_LIMIT_BP {
            return Err(IS_MEAN_OVER);
        }
        Ok(mean)
    }

    /// 封账：空集拒绝。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_SEALED);
        }
        if self.samples.is_empty() {
            return Err(IS_EMPTY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 5 段（no|col 名|budget|overhead|rate）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.samples {
            out.push(format!(
                "{}|{}|{}|{}|{}",
                s.no, COLLECTORS[s.col], s.budget_us, s.overhead_us, s.rate_bp
            ));
        }
        out
    }

    /// 指纹复算：链与样本重放一致。
    pub fn chain_ok(&self) -> bool {
        let mut h = CHAIN_SEED;
        for s in &self.samples {
            h = fold_u32(h, s.col as u32);
            h = fold_u32(h, s.budget_us as u32);
            h = fold_u32(h, s.overhead_us as u32);
            h = fold_u32(h, s.rate_bp as u32);
        }
        h == self.chain
    }
}

/// 标准探针：5 器各一，预算 8333us，开销 [50,60,40,70,30] → 均值 60bp。
fn fixture() -> IsolProbe {
    let mut p = IsolProbe::new(8);
    let _ = p.record(0, 8_333, 50);
    let _ = p.record(1, 8_333, 60);
    let _ = p.record(2, 8_333, 40);
    let _ = p.record(3, 8_333, 70);
    let _ = p.record(4, 8_333, 30);
    let _ = p.seal();
    p
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 零预算必抓。
    {
        let mut p = IsolProbe::new(8);
        caught += (p.record(0, 0, 50) == Err(IS_BUDGET_ZERO)) as usize;
    }
    // 2 开销超预算必抓。
    {
        let mut p = IsolProbe::new(8);
        caught += (p.record(0, 8_333, 9_000) == Err(IS_OVER_BUDGET)) as usize;
    }
    // 3 未知采集器必抓。
    {
        let mut p = IsolProbe::new(8);
        caught += (p.record(9, 8_333, 50) == Err(IS_UNKNOWN_COL)) as usize;
    }
    // 4 单样本超帽必抓（500us → 600bp > 200bp）。
    {
        let mut p = IsolProbe::new(8);
        caught += (p.record(0, 8_333, 500) == Err(IS_SAMPLE_CAP)) as usize;
    }
    // 5 均值超限必抓（五样本各 120bp → 均值 120 > 100）。
    {
        let mut p = IsolProbe::new(8);
        for _ in 0..5 {
            let _ = p.record(0, 8_333, 100);
        }
        caught += (p.probe_mean() == Err(IS_MEAN_OVER)) as usize;
    }
    // 6 封账拒录必抓。
    {
        let mut p = fixture();
        caught += (p.record(0, 8_333, 50) == Err(IS_SEALED)) as usize;
    }
    // 7 空探针集必抓。
    {
        let p = IsolProbe::new(8);
        caught += (p.probe_mean() == Err(IS_EMPTY)) as usize;
    }
    (caught, N)
}

/// UNX-F56127 全量判据（15 项）。
pub fn run_unx56127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56127");
    let p = fixture();

    // J1 探针全量。
    let ok = p.len() == 5;
    s.add("56127-J1-样本五条", ok, if ok { "" } else { "样本数不符" });
    let ok = p.samples().iter().all(|s| collector_ok(s.col));
    s.add("56127-J1-采集器闭集", ok, if ok { "" } else { "采集器越界" });
    let mut seen = [false; 5];
    for smp in p.samples() {
        seen[smp.col] = true;
    }
    let ok = seen.iter().all(|&v| v);
    s.add("56127-J1-五器全覆盖", ok, if ok { "" } else { "覆盖缺失" });
    let ok = p
        .samples()
        .iter()
        .all(|s| s.budget_us >= BUDGET_MIN && s.budget_us <= BUDGET_MAX);
    s.add("56127-J1-预算带", ok, if ok { "" } else { "预算越带" });

    // J2 隔离语义。
    let ok = p.samples().iter().all(|s| s.rate_bp <= SAMPLE_CAP_BP);
    s.add("56127-J2-单样本帽", ok, if ok { "" } else { "单样本超帽" });
    let r0 = p.samples()[0].rate_bp;
    let ok = r0 == 60;
    s.add("56127-J2-首样本率", ok, if ok { "" } else { "首样本非 60bp" });
    let r4 = p.samples()[4].rate_bp;
    let ok = r4 == 36;
    s.add("56127-J2-末样本率", ok, if ok { "" } else { "末样本非 36bp" });
    let ok = p.samples().iter().all(|s| s.overhead_us <= s.budget_us);
    s.add("56127-J2-开销不超预算", ok, if ok { "" } else { "开销超预算" });
    let mean = p.probe_mean().unwrap_or(u64::MAX);
    let ok = mean == 60;
    s.add("56127-J2-均值六十", ok, if ok { "" } else { "均值非 60bp" });
    let ok = p.probe_mean().unwrap_or(u64::MAX) <= MEAN_LIMIT_BP;
    s.add("56127-J2-百分一断言", ok, if ok { "" } else { "开销超 1%" });

    // J3 探针审计链。
    let ok = p.sealed();
    s.add("56127-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = p.chain() != CHAIN_SEED && p.chain_ok();
    s.add("56127-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = p.render();
    let mut ok = lines.len() == 5;
    for l in &lines {
        if l.split('|').count() != 5 {
            ok = false;
        }
    }
    s.add("56127-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });
    let ok = rate_of(50, 8_333) == 60 && rate_of(30, 8_333) == 36;
    s.add("56127-J3-开销核复算", ok, if ok { "" } else { "开销核漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56127-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56127_all_checks_green() {
        let set = run_unx56127_checks();
        assert!(set.all_passed(), "UNX-F56127 红项: {}", set.red_items_text());
    }
}
