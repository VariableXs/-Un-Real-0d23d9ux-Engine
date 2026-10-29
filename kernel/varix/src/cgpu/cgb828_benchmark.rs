//! cgb828_benchmark — CGPU-F0828 Intel 基准。
//!
//! 判据原文（验收行摘录）：Intel 基准：固定工作负载集（定容负载清单，负
//! 载描述与工时单位入账）；整数计分（无浮点：像素/周期等整型吞吐记账）；
//! 中位数统计（奇偶两路取中，排序纯整数）；基线比对（相对基线的升降以
//! 千分比定点表达，跌破阈值即报）。判据：负载集、整分、中位数、基线
//! 比对。
//!
//! 诊断段 0x7200（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；计分全程整
//! 数；排序用插入排序（定容小样本）；账本守恒（样本数 = 有效+无效）；
//! drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb828-v1";

/// 诊断段基（本单独占 0x7200）。
pub const SEG_BASE: u16 = 0x7200;

/// 负载未知（跑未登记的负载号）。
pub const XC_LOAD_UNKNOWN: u16 = SEG_BASE + 1;
/// 样本不足（中位数要求最少样本）。
pub const XC_SAMPLES_SHORT: u16 = SEG_BASE + 2;
/// 工时为零（无效样本——计分除零防护）。
pub const XC_ZERO_TICKS: u16 = SEG_BASE + 3;
/// 基线未设（比对前未落基线）。
pub const XC_NO_BASELINE: u16 = SEG_BASE + 4;
/// 跌破阈值（相对基线千分比跌幅超限）。
pub const XC_REGRESSION: u16 = SEG_BASE + 5;
/// 样本账漂移（样本数守恒破坏）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;

/// 单负载样本数上限（定容）。
pub const SAMPLES_CAP: usize = 16;
/// 中位数最少样本数。
pub const MEDIAN_MIN: usize = 3;
/// 回归报警阈值（千分比，-50‰ 触发）。
pub const REGRESS_MILLI: u64 = 50;

/// 负载定义。
#[derive(Clone, Debug)]
pub struct Load {
    /// 负载名。
    pub name: String,
    /// 工作量单位（如像素数、宏块数）。
    pub units: u64,
}

/// 一次运行样本。
#[derive(Clone, Copy, Debug)]
pub struct Run {
    /// 耗时（周期数，整数）。
    pub ticks: u64,
    /// 是否有效（无效样本入账但剔除统计）。
    pub valid: bool,
}

/// 基准跑分器。
pub struct Bencher {
    /// 跑分器名。
    pub name: String,
    /// 负载集（判据"固定工作负载集"）。
    loads: Vec<Load>,
    /// 每负载样本账（与 loads 等长对位）。
    runs: Vec<Vec<Run>>,
    /// 每负载基线（0 = 未设）。
    baselines: Vec<u64>,
    /// 无效样本计数。
    pub invalid: u64,
}

/// 守恒恒等式：Σ runs[i].len() == 有效样本数 + invalid。
/// 每次入账后 verify() 复核。
impl Bencher {
    /// 构造。
    pub fn new(name: &str) -> Self {
        Bencher { name: name.to_string(), loads: Vec::new(), runs: Vec::new(), baselines: Vec::new(), invalid: 0 }
    }

    /// 登记负载。
    pub fn add_load(&mut self, name: &str, units: u64) -> Result<usize, u16> {
        if units == 0 {
            return Err(XC_ZERO_TICKS);
        }
        self.loads.push(Load { name: name.to_string(), units });
        self.runs.push(Vec::new());
        self.baselines.push(0);
        Ok(self.loads.len() - 1)
    }

    /// 负载数。
    pub fn load_count(&self) -> usize {
        self.loads.len()
    }

    /// 记一次运行样本（判据"整数计分"）。
    pub fn record(&mut self, load: usize, ticks: u64) -> Result<u64, u16> {
        if load >= self.loads.len() {
            return Err(XC_LOAD_UNKNOWN);
        }
        if ticks == 0 {
            self.invalid += 1;
            self.runs[load].push(Run { ticks: 0, valid: false });
            self.verify()?;
            return Err(XC_ZERO_TICKS);
        }
        if self.runs[load].len() >= SAMPLES_CAP {
            self.invalid += 1;
            self.runs[load].push(Run { ticks, valid: false });
            self.verify()?;
            return Err(XC_SAMPLES_SHORT);
        }
        // 计分 = units * 1e6 / ticks（整数吞吐）。
        let score = self.loads[load].units.wrapping_mul(1_000_000) / ticks;
        self.runs[load].push(Run { ticks, valid: true });
        self.verify()?;
        Ok(score)
    }

    /// 有效样本数。
    pub fn valid_count(&self, load: usize) -> Result<usize, u16> {
        if load >= self.loads.len() {
            return Err(XC_LOAD_UNKNOWN);
        }
        let mut n = 0;
        for r in self.runs[load].iter() {
            if r.valid {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 插入排序（升序，定容小样本，纯整数）。
    fn sort_asc(v: &mut Vec<u64>) {
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] {
                let t = v[j - 1];
                v[j - 1] = v[j];
                v[j] = t;
                j -= 1;
            }
        }
    }

    /// 中位数（判据"奇偶两路取中"——整数均值）。
    pub fn median(&self, load: usize) -> Result<u64, u16> {
        if load >= self.loads.len() {
            return Err(XC_LOAD_UNKNOWN);
        }
        let mut vals: Vec<u64> = Vec::new();
        for r in self.runs[load].iter() {
            if r.valid {
                let s = self.loads[load].units.wrapping_mul(1_000_000) / r.ticks;
                vals.push(s);
            }
        }
        if vals.len() < MEDIAN_MIN {
            return Err(XC_SAMPLES_SHORT);
        }
        Self::sort_asc(&mut vals);
        let n = vals.len();
        if n % 2 == 1 {
            Ok(vals[n / 2])
        } else {
            Ok((vals[n / 2 - 1] + vals[n / 2]) / 2)
        }
    }

    /// 落基线（当前中位数即基线——判据"基线比对"前置）。
    pub fn set_baseline(&mut self, load: usize) -> Result<u64, u16> {
        if load >= self.loads.len() {
            return Err(XC_LOAD_UNKNOWN);
        }
        let m = self.median(load)?;
        self.baselines[load] = m;
        Ok(m)
    }

    /// 基线比对：返回千分比变化（正=提升，负=回退，定点 ×1000 基于基线）。
    pub fn compare(&self, load: usize) -> Result<i64, u16> {
        if load >= self.loads.len() {
            return Err(XC_LOAD_UNKNOWN);
        }
        if self.baselines[load] == 0 {
            return Err(XC_NO_BASELINE);
        }
        let m = self.median(load)?;
        let base = self.baselines[load] as i64;
        let now = m as i64;
        // 千分比 = (now - base) * 1000 / base。
        Ok((now - base) * 1000 / base)
    }

    /// 回归判定（跌破 REGRESS_MILLI 即报——判据"跌破阈值即报"）。
    pub fn regress_check(&self, load: usize) -> Result<(), u16> {
        let d = self.compare(load)?;
        if d < -(REGRESS_MILLI as i64) {
            return Err(XC_REGRESSION);
        }
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        let mut total = 0u64;
        let mut valid = 0u64;
        for l in 0..self.loads.len() {
            for r in self.runs[l].iter() {
                total += 1;
                if r.valid {
                    valid += 1;
                }
            }
        }
        if total != valid + self.invalid {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb828[");
        s.push_str(&self.name);
        s.push_str("] loads=");
        s.push_str(&self.load_count().to_string());
        s.push_str(" invalid=");
        s.push_str(&self.invalid.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut b = Bencher::new("w3-drill-828");

    // 负样本 1：未登记负载即记录——应拒。
    match b.record(0, 100) {
        Err(code) if code == XC_LOAD_UNKNOWN => {}
        Ok(_) => return Err(XC_LOAD_UNKNOWN),
        Err(other) => return Err(other),
    }

    // 登记 2 个负载。
    let l0 = b.add_load("fill_4k", 8_294_400)?;
    let l1 = b.add_load("decode_1080p", 2_073_600)?;
    if b.load_count() != 2 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 2：零单位负载——应拒。
    match b.add_load("bad", 0) {
        Err(code) if code == XC_ZERO_TICKS => {}
        Ok(_) => return Err(XC_ZERO_TICKS),
        Err(other) => return Err(other),
    }

    // 正样本：负载 0 记 5 个样本（含一个无效零工时）。
    match b.record(l0, 0) {
        Err(code) if code == XC_ZERO_TICKS => {}
        Ok(_) => return Err(XC_ZERO_TICKS),
        Err(other) => return Err(other),
    }
    for t in [1000u64, 1100, 900, 1050, 950] {
        b.record(l0, t)?;
    }
    if b.valid_count(l0)? != 5 {
        return Err(XC_LEDGER_DRIFT);
    }
    let med0 = b.median(l0)?;
    // 吞吐 = 8_294_400 * 1e6 / 1000 = 8_294_400_000 量级；中位数应落在 900~1100 换算间。
    let hi0 = 8_294_400u64 * 1_000_000 / 900;
    let lo0 = 8_294_400u64 * 1_000_000 / 1100;
    if med0 < lo0 || med0 > hi0 {
        return Err(XC_LEDGER_DRIFT);
    }

    // 基线落账 + 比对。
    b.set_baseline(l0)?;
    // 再记 2 个接近样本（不回归）。
    for t in [1000u64, 1020] {
        b.record(l0, t)?;
    }
    let d0 = b.compare(l0)?;
    if d0 < -(REGRESS_MILLI as i64) {
        return Err(XC_REGRESSION);
    }
    b.regress_check(l0)?;

    // 回归路径：负载 1 先记快样本落基线，再记慢样本跌破 50‰。
    for t in [500u64, 510, 505] {
        b.record(l1, t)?;
    }
    b.set_baseline(l1)?;
    for t in [900u64, 920, 910] {
        b.record(l1, t)?;
    }
    let d1 = b.compare(l1)?;
    if d1 >= 0 {
        return Err(XC_NO_BASELINE);
    }
    match b.regress_check(l1) {
        Err(code) if code == XC_REGRESSION => {}
        Ok(_) => return Err(XC_REGRESSION),
        Err(other) => return Err(other),
    }

    // 负样本 3：未落基线负载比对——用新负载 2。
    let l2 = b.add_load("scale_720p", 921_600)?;
    match b.compare(l2) {
        Err(code) if code == XC_SAMPLES_SHORT => {}
        Ok(_) => return Err(XC_SAMPLES_SHORT),
        Err(other) => return Err(other),
    }
    // 补 2 样本仍不足中位数。
    b.record(l2, 300)?;
    b.record(l2, 310)?;
    match b.median(l2) {
        Err(code) if code == XC_SAMPLES_SHORT => {}
        Ok(_) => return Err(XC_SAMPLES_SHORT),
        Err(other) => return Err(other),
    }
    b.record(l2, 320)?;
    match b.compare(l2) {
        Err(code) if code == XC_NO_BASELINE => {}
        Ok(_) => return Err(XC_NO_BASELINE),
        Err(other) => return Err(other),
    }

    b.verify()?;
    if b.invalid != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
