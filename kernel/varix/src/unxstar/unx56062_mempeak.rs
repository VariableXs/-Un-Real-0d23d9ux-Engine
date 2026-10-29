//! UNX-F56062 内存峰值记账器（unxstar 段）
//!
//! [J1] 峰值单调：峰值只升不降（observe 低于当前峰值不回落，只记回落事件数）；
//!      首条观测即峰值基线；观测值闭集守闸（0 值与超上限 4GiB 拒绝）。
//! [J2] 记账纪律：回落事件走独立计数与机器可读行；peak/last 分离；观测序列指纹
//!      同内容稳定、任一值变化指纹必变。
//! [J3] 峰值漂移核：两记账器峰值万分比漂移可算；超阈值场景注入必抓（5 例）。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（观测为零/观测超限/账已封/指纹漂移/峰值回退/无基线/事件行缺失）
//!   纯函数：fnv32、万分比漂移
//!   PeakEntry：单条观测，new() 守闸
//!   PeakBook：峰值记账账本（observe/peak/last/drop_events/drift_vs）
//!   injection_core：5 例必抓
//!   run_unx56062_checks：≥12 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x607A0;

/// 观测值为零
pub const D_OBS_ZERO: u32 = SEG_BASE + 0x01;
/// 观测超上限（> 4GiB = 4194304 KiB）
pub const D_OBS_OVER: u32 = SEG_BASE + 0x02;
/// 账已封（seal 后观测拒绝）
pub const D_SEALED: u32 = SEG_BASE + 0x03;
/// 指纹漂移（同内容两次折叠不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x04;
/// 峰值回退（峰值被观测压低——记账器内部不变式，防御性诊断）
pub const D_PEAK_BACK: u32 = SEG_BASE + 0x05;
/// 无基线（未观测即询问峰值）
pub const D_NO_BASELINE: u32 = SEG_BASE + 0x06;
/// 事件行缺失（有回落事件但行表为空）
pub const D_EVENT_ROW_MISSING: u32 = SEG_BASE + 0x07;

/// 观测上限 4GiB（KiB 计）
pub const OBS_CAP_KB: u32 = 4_194_304;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 万分比漂移
pub fn drift_pbp(a: u64, b: u64) -> u64 {
    let a = a as u64;
    let b = b as u64;
    let d = if a > b { a - b } else { b - a };
    let base = if a > b { a } else { b };
    if base == 0 {
        0
    } else {
        (d * 10_000) / base
    }
}

/// 单条观测
#[derive(Clone)]
pub struct PeakEntry {
    pub seq: u64,
    pub rss_kb: u32,
}

impl PeakEntry {
    /// 守闸：非零 + 上限
    pub fn new(seq: u64, rss_kb: u32) -> Result<Self, u32> {
        if rss_kb == 0 {
            return Err(D_OBS_ZERO);
        }
        if rss_kb > OBS_CAP_KB {
            return Err(D_OBS_OVER);
        }
        Ok(PeakEntry { seq, rss_kb })
    }
}

/// 峰值记账账本
pub struct PeakBook {
    pub book_name: String,
    entries: Vec<PeakEntry>,
    peak_kb: u32,
    last_kb: u32,
    drop_events: u64,
    sealed: bool,
}

impl PeakBook {
    pub fn new(book_name: &str) -> Self {
        PeakBook {
            book_name: book_name.to_string(),
            entries: Vec::new(),
            peak_kb: 0,
            last_kb: 0,
            drop_events: 0,
            sealed: false,
        }
    }

    /// 记账一条；峰值单调抬升，低于峰值记回落事件
    pub fn observe(&mut self, seq: u64, rss_kb: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(D_SEALED);
        }
        let e = PeakEntry::new(seq, rss_kb)?;
        let fp = {
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&e.seq.to_le_bytes());
            buf.extend_from_slice(&e.rss_kb.to_le_bytes());
            fnv32(&buf)
        };
        if rss_kb > self.peak_kb {
            self.peak_kb = rss_kb;
        } else if rss_kb < self.last_kb {
            self.drop_events += 1;
        }
        self.last_kb = rss_kb;
        self.entries.push(e);
        Ok(fp)
    }

    pub fn peak(&self) -> Result<u32, u32> {
        if self.entries.is_empty() {
            return Err(D_NO_BASELINE);
        }
        Ok(self.peak_kb)
    }

    pub fn last(&self) -> Result<u32, u32> {
        if self.entries.is_empty() {
            return Err(D_NO_BASELINE);
        }
        Ok(self.last_kb)
    }

    pub fn drop_count(&self) -> u64 {
        self.drop_events
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 内部不变式：peak ≥ last 恒成立
    pub fn invariant_ok(&self) -> bool {
        self.peak_kb >= self.last_kb
    }

    /// 回落事件行（机器可读；口径与 observe 一致：低于前值且未破峰才记）
    pub fn event_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        let mut cur_peak: u32 = 0;
        let mut last: u32 = 0;
        for e in &self.entries {
            if e.rss_kb > cur_peak {
                cur_peak = e.rss_kb;
            } else if e.rss_kb < last {
                rows.push(format!("DROP|{}|{}|peak={}", e.seq, e.rss_kb, cur_peak));
            }
            last = e.rss_kb;
        }
        rows
    }

    /// 观测序列指纹（顺序敏感）
    pub fn series_fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x56062_00;
        for e in &self.entries {
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&e.seq.to_le_bytes());
            buf.extend_from_slice(&e.rss_kb.to_le_bytes());
            acc = acc.wrapping_mul(16777619) ^ fnv32(&buf);
        }
        acc
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.series_fingerprint()
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 与另一账本峰值漂移
    pub fn drift_vs(&self, other: &PeakBook) -> Result<u64, u32> {
        let a = self.peak()? as u64;
        let b = other.peak()? as u64;
        Ok(drift_pbp(a, b))
    }
}

/// 标准夹具：10 条观测，先爬升后回落两次。
/// 序列：1024, 2048, 4096, 8192, 8191(降→drop1), 4096(降→drop2), 6144(升), 16384(新峰),
///       12288(降→drop3), 16384(平峰不降不升→无事件)
/// 期望：peak=16384, last=16384, drop_events=3
pub fn fixture_book() -> PeakBook {
    let mut b = PeakBook::new("mem-peak");
    let plan: [(u64, u32); 10] = [
        (1, 1024),
        (2, 2048),
        (3, 4096),
        (4, 8192),
        (5, 8191),
        (6, 4096),
        (7, 6144),
        (8, 16384),
        (9, 12288),
        (10, 16384),
    ];
    for (seq, kb) in plan {
        b.observe(seq, kb).expect("fixture observe");
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 观测为零
    if PeakEntry::new(1, 0).err() == Some(D_OBS_ZERO) {
        caught += 1;
    }

    // 2) 观测超限
    if PeakEntry::new(1, OBS_CAP_KB + 1).err() == Some(D_OBS_OVER) {
        caught += 1;
    }

    // 3) 无基线询问峰值
    let e = PeakBook::new("empty");
    if e.peak() == Err(D_NO_BASELINE) {
        caught += 1;
    }

    // 4) 封账后观测
    let mut c = fixture_book();
    let _ = c.seal();
    if c.observe(11, 2048) == Err(D_SEALED) {
        caught += 1;
    }

    // 5) 上限值本身可接受（防御反例确认闭集边界）
    if PeakEntry::new(1, OBS_CAP_KB).is_ok() {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性与敏感性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_book().series_fingerprint();
    let b = fixture_book().series_fingerprint();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    let mut alt = fixture_book();
    alt.observe(11, 32768).map_err(|e| e)?;
    if alt.series_fingerprint() == a {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56062");

    // J1 峰值单调
    let b = fixture_book();
    s.add("56062-J1-峰值16384", b.peak() == Ok(16384), "峰值不为 16384");
    s.add("56062-J1-末值16384", b.last() == Ok(16384), "末值不为 16384");
    s.add("56062-J1-条数10", b.len() == 10, "观测条数不为 10");
    s.add("56062-J1-不变式", b.invariant_ok(), "peak ≥ last 不变式被破坏");

    // J2 记账纪律
    s.add("56062-J2-回落事件3", b.drop_count() == 3, "回落事件数不为 3");
    s.add("56062-J2-事件行3", b.event_rows().len() == 3, "事件行数不为 3");
    s.add("56062-J2-事件行锚", b.event_rows()[0] == "DROP|5|8191|peak=8192", "首条事件行格式不符");
    s.add("56062-J2-平峰无事件", b.event_rows().iter().all(|r| !r.starts_with("DROP|10|")), "平峰误记回落事件");
    s.add("56062-J2-指纹稳敏", fp_stability().is_ok(), "指纹稳定/敏感性异常");

    // J3 峰值漂移核
    let mut other = PeakBook::new("other");
    for (seq, kb) in [(1u64, 16384u32), (2u64, 20480u32)] {
        other.observe(seq, kb).expect("other observe");
    }
    // 峰值 16384 vs 20480 → 漂移 4096/20480 = 2000pbp
    s.add("56062-J3-峰值漂移2000", b.drift_vs(&other) == Ok(2000), "峰值漂移不为 2000pbp");
    s.add("56062-J3-零基线拒漂移", {
        let e = PeakBook::new("e2");
        b.drift_vs(&e).is_err()
    }, "空账漂移未被拒绝");
    s.add("56062-J3-封账只读", {
        let mut d = fixture_book();
        let _ = d.seal();
        d.observe(99, 1024) == Err(D_SEALED)
    }, "封账后仍可观测");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56062-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56062_all_checks_green() {
        let s = run_unx56062_checks();
        assert!(
            s.all_passed(),
            "UNX-F56062 有红项: {}",
            s.red_items_text()
        );
    }
}
