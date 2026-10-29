//! cgb842_telemetry_sum — CGPU-F0842 Intel 遥测汇总。
//!
//! 判据原文（验收行摘录）：Intel 遥测汇总：分桶直方图（事件计数按类别×
//! 严重度二维桶归账）；桶守恒（桶总和 = 受理事件总数）；峰值探测（最大
//! 桶计数与对应桶名——并列取首桶）；空账保护（零事件汇总返回零值不报
//! 错）。判据：二维桶、守恒、峰值、空账。
//!
//! 诊断段 0x73C0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容二维桶（4 类 × 3 级）；drill() 自例程
//! 返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb842-v1";

/// 诊断段基（本单独占 0x73C0）。
pub const SEG_BASE: u16 = 0x73C0;

/// 类别越界（类别号 ≥ 4）。
pub const XC_CAT_RANGE: u16 = SEG_BASE + 1;
/// 严重度越界（级号 ≥ 3）。
pub const XC_SEV_RANGE: u16 = SEG_BASE + 2;
/// 桶账漂移（桶总和 ≠ 受理总数）。
pub const XC_BUCKET_DRIFT: u16 = SEG_BASE + 3;
/// 汇总未关闭（未 seal 即读峰）。
pub const XC_NOT_SEALED: u16 = SEG_BASE + 4;
/// 重复封账（已 seal 再 seal）。
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 5;
/// 账满（明细账定容耗尽）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 6;

/// 类别数（4 类）。
pub const CAT_N: usize = 4;
/// 严重度级数（3 级）。
pub const SEV_N: usize = 3;
/// 明细账定容。
pub const DETAIL_CAP: usize = 48;

/// 类别名。
pub fn cat_name(c: usize) -> &'static str {
    match c {
        0 => "perf",
        1 => "power",
        2 => "mem",
        _ => "media",
    }
}

/// 严重度名。
pub fn sev_name(s: usize) -> &'static str {
    match s {
        0 => "info",
        1 => "warn",
        _ => "fault",
    }
}

/// 遥测事件（汇总输入）。
#[derive(Clone, Copy, Debug)]
pub struct Ev {
    /// 类别（0..4）。
    pub cat: usize,
    /// 严重度（0..3）。
    pub sev: usize,
}

/// 汇总器。
pub struct Summarizer {
    /// 汇总器名。
    pub name: String,
    /// 二维桶（判据"类别×严重度二维桶"）。
    buckets: [[u64; SEV_N]; CAT_N],
    /// 受理总数。
    total: u64,
    /// 明细账（定容）。
    pub details: Vec<Ev>,
    /// 封账位。
    sealed: bool,
    /// 峰值缓存（封账时定格）。
    peak: Option<(usize, usize, u64)>,
}

/// 守恒恒等式：Σ buckets[c][s] == total（全部桶计数之和 == 受理总数）。
/// 封账后桶冻结、total 冻结；每次入账后 verify() 复核。
impl Summarizer {
    /// 构造（空账）。
    pub fn new(name: &str) -> Self {
        Summarizer {
            name: name.to_string(),
            buckets: [[0; SEV_N]; CAT_N],
            total: 0,
            details: Vec::new(),
            sealed: false,
            peak: None,
        }
    }

    /// 受理事件（判据"分桶归账"）。
    pub fn ingest(&mut self, e: Ev) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_ALREADY_SEALED);
        }
        if e.cat >= CAT_N {
            return Err(XC_CAT_RANGE);
        }
        if e.sev >= SEV_N {
            return Err(XC_SEV_RANGE);
        }
        if self.details.len() >= DETAIL_CAP {
            return Err(XC_LEDGER_FULL);
        }
        self.buckets[e.cat][e.sev] += 1;
        self.total += 1;
        self.details.push(e);
        self.verify()?;
        Ok(())
    }

    /// 受理总数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 桶计数只读。
    pub fn bucket(&self, c: usize, s: usize) -> Result<u64, u16> {
        if c >= CAT_N {
            return Err(XC_CAT_RANGE);
        }
        if s >= SEV_N {
            return Err(XC_SEV_RANGE);
        }
        Ok(self.buckets[c][s])
    }

    /// 类别合计。
    pub fn cat_total(&self, c: usize) -> Result<u64, u16> {
        if c >= CAT_N {
            return Err(XC_CAT_RANGE);
        }
        let mut n = 0;
        for s in 0..SEV_N {
            n += self.buckets[c][s];
        }
        Ok(n)
    }

    /// 封账（判据"峰值定格"）。
    pub fn seal(&mut self) -> Result<(usize, usize, u64), u16> {
        if self.sealed {
            return Err(XC_ALREADY_SEALED);
        }
        self.verify()?;
        self.sealed = true;
        // 峰值探测：最大桶（并列取首桶——按 c 再 s 字典序）。
        let mut best = (0usize, 0usize, 0u64);
        for c in 0..CAT_N {
            for s in 0..SEV_N {
                if self.buckets[c][s] > best.2 {
                    best = (c, s, self.buckets[c][s]);
                }
            }
        }
        self.peak = Some(best);
        Ok(best)
    }

    /// 峰值读取（判据"空账保护"：零事件返回零值不报错）。
    pub fn peak(&self) -> Result<(usize, usize, u64), u16> {
        if !self.sealed {
            return Err(XC_NOT_SEALED);
        }
        match self.peak {
            Some(p) => Ok(p),
            None => Ok((0, 0, 0)),
        }
    }

    /// 峰值桶名（判据"对应桶名"）。
    pub fn peak_name(&self) -> Result<String, u16> {
        let (c, s, _) = self.peak()?;
        let mut out = String::new();
        out.push_str(cat_name(c));
        out.push('/');
        out.push_str(sev_name(s));
        Ok(out)
    }

    /// 守恒校验（判据"桶守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for c in 0..CAT_N {
            for s in 0..SEV_N {
                sum += self.buckets[c][s];
            }
        }
        if sum != self.total {
            return Err(XC_BUCKET_DRIFT);
        }
        if sum != self.details.len() as u64 {
            return Err(XC_BUCKET_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb842[");
        s.push_str(&self.name);
        s.push_str("] total=");
        s.push_str(&self.total.to_string());
        s.push_str(" sealed=");
        s.push_str(if self.sealed { "1" } else { "0" });
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut z = Summarizer::new("w3-drill-842-empty");

    // 空账保护：零事件封账 + 峰值零值不报错。
    let pz = z.seal()?;
    if pz != (0, 0, 0) {
        return Err(XC_BUCKET_DRIFT);
    }
    if z.peak()? != (0, 0, 0) {
        return Err(XC_BUCKET_DRIFT);
    }
    let nm = z.peak_name()?;
    if nm != "perf/info" {
        return Err(XC_BUCKET_DRIFT);
    }
    // 负样本 1：封账后再受理。
    match z.ingest(Ev { cat: 0, sev: 0 }) {
        Err(code) if code == XC_ALREADY_SEALED => {}
        Ok(_) => return Err(XC_ALREADY_SEALED),
        Err(other) => return Err(other),
    }
    // 负样本 2：重复封账。
    match z.seal() {
        Err(code) if code == XC_ALREADY_SEALED => {}
        Ok(_) => return Err(XC_ALREADY_SEALED),
        Err(other) => return Err(other),
    }

    // 正账：三类别混合受理。
    let mut m = Summarizer::new("w3-drill-842");
    // 负样本 3：类别越界。
    match m.ingest(Ev { cat: CAT_N, sev: 0 }) {
        Err(code) if code == XC_CAT_RANGE => {}
        Ok(_) => return Err(XC_CAT_RANGE),
        Err(other) => return Err(other),
    }
    // 负样本 4：严重度越界。
    match m.ingest(Ev { cat: 0, sev: SEV_N }) {
        Err(code) if code == XC_SEV_RANGE => {}
        Ok(_) => return Err(XC_SEV_RANGE),
        Err(other) => return Err(other),
    }
    // 受理：perf/fault ×3、power/warn ×2、media/info ×5、mem/fault ×1。
    for _ in 0..3 {
        m.ingest(Ev { cat: 0, sev: 2 })?;
    }
    for _ in 0..2 {
        m.ingest(Ev { cat: 1, sev: 1 })?;
    }
    for _ in 0..5 {
        m.ingest(Ev { cat: 3, sev: 0 })?;
    }
    m.ingest(Ev { cat: 2, sev: 2 })?;
    m.verify()?;
    if m.total() != 11 {
        return Err(XC_BUCKET_DRIFT);
    }
    if m.cat_total(3)? != 5 || m.cat_total(0)? != 3 {
        return Err(XC_BUCKET_DRIFT);
    }
    if m.bucket(2, 2)? != 1 {
        return Err(XC_BUCKET_DRIFT);
    }
    // 封账 + 峰值：media/info = 5 最大。
    let p = m.seal()?;
    if p != (3, 0, 5) {
        return Err(XC_BUCKET_DRIFT);
    }
    let pn = m.peak_name()?;
    if pn != "media/info" {
        return Err(XC_BUCKET_DRIFT);
    }
    // 负样本 5：未封账读峰。
    let mut m2 = Summarizer::new("w3-drill-842-noseal");
    match m2.peak() {
        Err(code) if code == XC_NOT_SEALED => {}
        Ok(_) => return Err(XC_NOT_SEALED),
        Err(other) => return Err(other),
    }
    // 并列取首桶：两桶同为 2，应取 (0,0)。
    let mut m3 = Summarizer::new("w3-drill-842-tie");
    m3.ingest(Ev { cat: 2, sev: 1 })?;
    m3.ingest(Ev { cat: 2, sev: 1 })?;
    m3.ingest(Ev { cat: 0, sev: 0 })?;
    m3.ingest(Ev { cat: 0, sev: 0 })?;
    let p3 = m3.seal()?;
    if p3 != (0, 0, 2) {
        return Err(XC_BUCKET_DRIFT);
    }
    m.verify()?;
    m3.verify()?;
    Ok(0)
}
