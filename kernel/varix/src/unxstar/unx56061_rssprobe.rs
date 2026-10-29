//! UNX-F56061 RSS 采集器族旗舰（unxstar 段）
//!
//! [J1] 采集族齐备：四族 RSS 采集口径（smaps/statm/pagewalk/sysinfo）全部落账，
//!      每族每轮采样一条；样本值闭集守闸（0 值与超上限 1GiB 拒绝）。
//! [J2] 暖机纪律：暖机轮数阈值 10^4，未达阈值拒绝出正式样本（warm gate）；
//!      正式轮计数与暖机轮计数分离，互不污染。
//! [J3] 统计核：每族 p50/p95/min/max 百分位可算且判据期望值与夹具逐一心算一致；
//!      指纹链（逐样本 FNV 折叠）同内容稳定；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（族未知/样本为零/样本超限/暖机未足/重复族/指纹漂移/轮次回退）
//!   纯函数：fnv32、万分比、排序百分位
//!   RssSample：单样本条目，new() 守闸
//!   RssFamily：单族采集账（observe/warm_gate/percentiles）
//!   RssProbeBook：旗舰总账（register/warmup_all/stats_all/fingerprint_chain）
//!   injection_core：5 例必抓
//!   run_unx56061_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60780;

/// 采集族未知（不在四族闭集）
pub const D_FAMILY_UNKNOWN: u32 = SEG_BASE + 0x01;
/// 样本值为零
pub const D_SAMPLE_ZERO: u32 = SEG_BASE + 0x02;
/// 样本超上限（> 1GiB = 1048576 KiB）
pub const D_SAMPLE_OVER: u32 = SEG_BASE + 0x03;
/// 暖机未足（正式轮先于暖机阈值）
pub const D_WARM_SHORT: u32 = SEG_BASE + 0x04;
/// 重复注册族
pub const D_DUP_FAMILY: u32 = SEG_BASE + 0x05;
/// 指纹漂移（同内容两次折叠不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 轮次回退（正式轮号不递增）
pub const D_ROUND_BACK: u32 = SEG_BASE + 0x07;

/// 四族闭集
pub const FAMILIES: [&str; 4] = ["smaps", "statm", "pagewalk", "sysinfo"];
/// 暖机阈值 10^4
pub const WARMUP_ROUNDS: u64 = 10_000;
/// 样本上限 1GiB（KiB 计）
pub const SAMPLE_CAP_KB: u32 = 1_048_576;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 万分比
pub fn pbp(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        part * 10_000 / whole
    }
}

/// 排序百分位：升序取 idx = n*pct/100（截断），夹到 n-1
pub fn percentile(sorted: &[u32], pct: u64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    let mut idx = n * pct as usize / 100;
    if idx >= n {
        idx = n - 1;
    }
    sorted[idx]
}

/// 单样本条目
#[derive(Clone)]
pub struct RssSample {
    pub family: String,
    pub round: u64,
    pub rss_kb: u32,
}

impl RssSample {
    /// 守闸：族闭集 + 值非零 + 值上限
    pub fn new(family: &str, round: u64, rss_kb: u32) -> Result<Self, u32> {
        if !FAMILIES.contains(&family) {
            return Err(D_FAMILY_UNKNOWN);
        }
        if rss_kb == 0 {
            return Err(D_SAMPLE_ZERO);
        }
        if rss_kb > SAMPLE_CAP_KB {
            return Err(D_SAMPLE_OVER);
        }
        Ok(RssSample { family: family.to_string(), round, rss_kb })
    }

    pub fn fp(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.family.as_bytes());
        buf.extend_from_slice(&self.round.to_le_bytes());
        buf.extend_from_slice(&self.rss_kb.to_le_bytes());
        fnv32(&buf)
    }
}

/// 单族采集账
pub struct RssFamily {
    pub name: String,
    warmup_done: u64,
    formal_round: u64,
    values: Vec<u32>,
}

impl RssFamily {
    pub fn new(name: &str) -> Result<Self, u32> {
        if !FAMILIES.contains(&name) {
            return Err(D_FAMILY_UNKNOWN);
        }
        Ok(RssFamily {
            name: name.to_string(),
            warmup_done: 0,
            formal_round: 0,
            values: Vec::new(),
        })
    }

    /// 暖机观测：只计数不出正式样本
    pub fn warmup(&mut self, n: u64) {
        self.warmup_done += n;
    }

    pub fn warm_ready(&self) -> bool {
        self.warmup_done >= WARMUP_ROUNDS
    }

    /// 正式采样：暖机未足拒绝；轮次回退拒绝
    pub fn observe(&mut self, round: u64, rss_kb: u32) -> Result<u32, u32> {
        if !self.warm_ready() {
            return Err(D_WARM_SHORT);
        }
        if round <= self.formal_round {
            return Err(D_ROUND_BACK);
        }
        let s = RssSample::new(&self.name, round, rss_kb)?;
        let fp = s.fp();
        self.formal_round = round;
        self.values.push(rss_kb);
        Ok(fp)
    }

    pub fn sample_count(&self) -> usize {
        self.values.len()
    }

    pub fn last_round(&self) -> u64 {
        self.formal_round
    }

    /// 统计核：升序百分位 (p50, p95, min, max)
    pub fn stats(&self) -> (u32, u32, u32, u32) {
        let mut sorted = self.values.clone();
        sorted.sort();
        (
            percentile(&sorted, 50),
            percentile(&sorted, 95),
            *sorted.first().unwrap_or(&0),
            *sorted.last().unwrap_or(&0),
        )
    }
}

/// 旗舰总账
pub struct RssProbeBook {
    pub book_name: String,
    families: Vec<RssFamily>,
}

impl RssProbeBook {
    pub fn new(book_name: &str) -> Self {
        RssProbeBook { book_name: book_name.to_string(), families: Vec::new() }
    }

    /// 注册族；重复注册拒绝
    pub fn register(&mut self, name: &str) -> Result<(), u32> {
        if self.families.iter().any(|f| f.name == name) {
            return Err(D_DUP_FAMILY);
        }
        let f = RssFamily::new(name)?;
        self.families.push(f);
        Ok(())
    }

    pub fn family(&mut self, name: &str) -> Option<&mut RssFamily> {
        self.families.iter_mut().find(|f| f.name == name)
    }

    pub fn family_count(&self) -> usize {
        self.families.len()
    }

    pub fn all_families(&self) -> &[RssFamily] {
        &self.families
    }

    /// 指纹链：按族序折叠全部样本指纹
    pub fn fingerprint_chain(&self) -> u32 {
        let mut acc: u32 = 0x56061_00;
        for f in &self.families {
            for (i, &v) in f.values.iter().enumerate() {
                let mut buf: Vec<u8> = Vec::new();
                buf.extend_from_slice(f.name.as_bytes());
                buf.extend_from_slice(&(i as u64).to_le_bytes());
                buf.extend_from_slice(&v.to_le_bytes());
                acc = acc.wrapping_mul(16777619) ^ fnv32(&buf);
            }
        }
        acc
    }
}

/// 标准夹具：四族全部暖机 10^4，各正式采样 8 轮。
/// 族 f（0 起）第 i 轮（0 起）值 = (f+1)*1000 + i*10：
///   smaps:   1000..1070 → 8 样本 p50=idx4=1040 p95=idx7=1070 min=1000 max=1070
///   statm:   2000..2070 → p50=2040 p95=2070
///   pagewalk:3000..3070 → p50=3040 p95=3070
///   sysinfo: 4000..4070 → p50=4040 p95=4070
pub fn fixture_book() -> RssProbeBook {
    let mut b = RssProbeBook::new("rss-flagship");
    for f in 0..4 {
        let name = FAMILIES[f];
        b.register(name).expect("register");
        let fam = b.family(name).expect("fam");
        fam.warmup(WARMUP_ROUNDS);
        for i in 0..8u64 {
            let v = (f as u32 + 1) * 1000 + i as u32 * 10;
            fam.observe(i + 1, v).expect("observe");
        }
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 族未知
    if RssSample::new("rssfile", 1, 100).err() == Some(D_FAMILY_UNKNOWN) {
        caught += 1;
    }

    // 2) 样本为零
    if RssSample::new("smaps", 1, 0).err() == Some(D_SAMPLE_ZERO) {
        caught += 1;
    }

    // 3) 样本超限
    if RssSample::new("smaps", 1, SAMPLE_CAP_KB + 1).err() == Some(D_SAMPLE_OVER) {
        caught += 1;
    }

    // 4) 暖机未足
    let mut f = RssFamily::new("statm").expect("f");
    if f.observe(1, 2048) == Err(D_WARM_SHORT) {
        caught += 1;
    }

    // 5) 重复注册族
    let mut b = fixture_book();
    if b.register("smaps") == Err(D_DUP_FAMILY) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_book().fingerprint_chain();
    let b = fixture_book().fingerprint_chain();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56061");

    // J1 采集族齐备
    let b = fixture_book();
    s.add("56061-J1-四族注册", b.family_count() == 4, "采集族数不为 4");
    s.add("56061-J1-样本落账", b.all_families().iter().all(|f| f.sample_count() == 8), "存在样本数不为 8 的族");
    s.add("56061-J1-轮次递进", b.all_families().iter().all(|f| f.last_round() == 8), "存在末轮不为 8 的族");
    s.add("56061-J1-值闭集", RssSample::new("sysinfo", 1, SAMPLE_CAP_KB) .is_ok(), "上限值应可接受");

    // J2 暖机纪律
    let mut f = RssFamily::new("smaps").expect("f");
    f.warmup(WARMUP_ROUNDS - 1);
    s.add("56061-J2-暖机差一轮拒", f.observe(1, 1024) == Err(D_WARM_SHORT), "暖机未足未被拒绝");
    f.warmup(1);
    s.add("56061-J2-暖机足后收样", f.observe(1, 1024).is_ok(), "暖机足后收样失败");
    s.add("56061-J2-轮次回退拒", f.observe(1, 1024) == Err(D_ROUND_BACK), "轮次回退未被拒绝");

    // J3 统计核（心算：8 样本升序 1000..1070 步长 10，p50 = idx 4 = 1040）
    // 修正：idx = 8*50/100 = 4 → 第 5 个值 = 1000+4*10 = 1040
    let (p50, p95, min, max) = b.all_families()[0].stats();
    s.add("56061-J3-smaps-p50", p50 == 1040, "smaps p50 应为 1040");
    s.add("56061-J3-smaps-p95", p95 == 1070, "smaps p95 应为 1070（idx=7 截断到 n-1）");
    s.add("56061-J3-smaps-minmax", min == 1000 && max == 1070, "smaps min/max 应为 1000/1070");
    let (p50b, _, _, _) = b.all_families()[3].stats();
    s.add("56061-J3-sysinfo-p50", p50b == 4040, "sysinfo p50 应为 4040");
    s.add("56061-J3-指纹稳定", fp_stability().is_ok(), "指纹链漂移");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56061-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56061_all_checks_green() {
        let s = run_unx56061_checks();
        assert!(
            s.all_passed(),
            "UNX-F56061 有红项: {}",
            s.red_items_text()
        );
    }
}
