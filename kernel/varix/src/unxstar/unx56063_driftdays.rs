//! UNX-F56063 30 天漂移观察项采集件（unxstar 段）
//!
//! [J1] 观察项齐备：30 天逐日漂移值（万分比）逐日落账；日序 1..=30 闭集守闸、
//!      重复日拒绝、日序回退拒绝；观察项标签走闭集（tsc/fs/env/net）。
//! [J2] 滚动统计：滚动窗口（7 日）均值/峰值可算；超阈日（> 300pbp）计数并出
//!      机器可读标记行；零观察即询问统计拒绝。
//! [J3] 指纹链：逐日指纹折叠稳定；同内容两次折叠一致、任一日变化指纹必变；
//!      注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（标签未知/日序越界/重复日/统计未就绪/指纹漂移/日序回退/漂移值超界）
//!   纯函数：fnv32、万分比均值
//!   DriftDay：单日观察条目，new() 守闸
//!   DriftLedger：30 天观察账本（record/rolling/over_threshold/flag_rows）
//!   injection_core：5 例必抓
//!   run_unx56063_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x607C0;

/// 观察项标签未知
pub const D_TAG_UNKNOWN: u32 = SEG_BASE + 0x01;
/// 日序越界（不在 1..=30）
pub const D_DAY_RANGE: u32 = SEG_BASE + 0x02;
/// 重复日
pub const D_DAY_DUP: u32 = SEG_BASE + 0x03;
/// 统计未就绪（零观察即询问）
pub const D_STATS_NOT_READY: u32 = SEG_BASE + 0x04;
/// 指纹漂移
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x05;
/// 日序回退
pub const D_DAY_BACK: u32 = SEG_BASE + 0x06;
/// 漂移值超界（> 10000pbp）
pub const D_DRIFT_RANGE: u32 = SEG_BASE + 0x07;

/// 观察项标签闭集
pub const TAGS: [&str; 4] = ["tsc", "fs", "env", "net"];
/// 观察天数
pub const OBS_DAYS: u32 = 30;
/// 滚动窗口
pub const ROLL_WIN: usize = 7;
/// 超阈判据（万分比）
pub const OVER_GATE: u64 = 300;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 单日观察条目
#[derive(Clone)]
pub struct DriftDay {
    pub tag: String,
    pub day: u32,
    pub drift_pbp: u64,
}

impl DriftDay {
    /// 守闸：标签闭集 + 日序闭集 + 漂移值域 0..=10000
    pub fn new(tag: &str, day: u32, drift_pbp: u64) -> Result<Self, u32> {
        if !TAGS.contains(&tag) {
            return Err(D_TAG_UNKNOWN);
        }
        if day < 1 || day > OBS_DAYS {
            return Err(D_DAY_RANGE);
        }
        if drift_pbp > 10_000 {
            return Err(D_DRIFT_RANGE);
        }
        Ok(DriftDay { tag: tag.to_string(), day, drift_pbp })
    }

    pub fn fp(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.tag.as_bytes());
        buf.extend_from_slice(&self.day.to_le_bytes());
        buf.extend_from_slice(&self.drift_pbp.to_le_bytes());
        fnv32(&buf)
    }
}

/// 30 天观察账本（单标签序列）
pub struct DriftLedger {
    pub tag: String,
    days: Vec<DriftDay>,
}

impl DriftLedger {
    pub fn new(tag: &str) -> Result<Self, u32> {
        if !TAGS.contains(&tag) {
            return Err(D_TAG_UNKNOWN);
        }
        Ok(DriftLedger { tag: tag.to_string(), days: Vec::new() })
    }

    /// 落账：重复日、日序回退拒绝
    pub fn record(&mut self, day: u32, drift_pbp: u64) -> Result<u32, u32> {
        if let Some(last) = self.days.last() {
            if day == last.day {
                return Err(D_DAY_DUP);
            }
            if day < last.day {
                return Err(D_DAY_BACK);
            }
        }
        let d = DriftDay::new(&self.tag, day, drift_pbp)?;
        let fp = d.fp();
        self.days.push(d);
        Ok(fp)
    }

    pub fn len(&self) -> usize {
        self.days.len()
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    /// 滚动窗口均值（最后 ROLL_WIN 日，万分比整除）
    pub fn rolling_mean(&self) -> Result<u64, u32> {
        if self.days.is_empty() {
            return Err(D_STATS_NOT_READY);
        }
        let tail: Vec<u64> = self
            .days
            .iter()
            .rev()
            .take(ROLL_WIN)
            .map(|d| d.drift_pbp)
            .collect();
        let sum: u64 = tail.iter().sum();
        Ok(sum / tail.len() as u64)
    }

    /// 滚动窗口峰值
    pub fn rolling_peak(&self) -> Result<u64, u32> {
        if self.days.is_empty() {
            return Err(D_STATS_NOT_READY);
        }
        Ok(self
            .days
            .iter()
            .rev()
            .take(ROLL_WIN)
            .map(|d| d.drift_pbp)
            .max()
            .unwrap_or(0))
    }

    /// 超阈日计数
    pub fn over_threshold_count(&self) -> usize {
        self.days.iter().filter(|d| d.drift_pbp > OVER_GATE).count()
    }

    /// 超阈标记行
    pub fn flag_rows(&self) -> Vec<String> {
        self.days
            .iter()
            .filter(|d| d.drift_pbp > OVER_GATE)
            .map(|d| format!("OVER|{}|{}|{}", d.tag, d.day, d.drift_pbp))
            .collect()
    }

    /// 指纹链
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x56063_00;
        for d in &self.days {
            acc = acc.wrapping_mul(16777619) ^ d.fp();
        }
        acc
    }
}

/// 标准夹具：tsc 标签 30 天，漂移 100 + (day % 5) * 100（100/200/300/400/500 循环）。
/// 心算：>300 的日为漂移 400/500 的日，即 day%5 ∈ {4,0}（day%5==0 → 500？100+0=100，
/// 更正：day%5∈{0..4} → 100..500；400/500 出现在 day%5==3/4）→ 30 天中每天 cycle，
/// day%5==3 或 4 共 12 天超阈（>300 严格大于：400/500 计，300 不计）。
pub fn fixture_ledger() -> DriftLedger {
    let mut l = DriftLedger::new("tsc").expect("ledger");
    for day in 1..=30u32 {
        let drift = 100 + (day % 5) * 100;
        l.record(day, drift as u64).expect("record");
    }
    l
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 标签未知
    if DriftDay::new("disk", 1, 100).err() == Some(D_TAG_UNKNOWN) {
        caught += 1;
    }

    // 2) 日序越界
    if DriftDay::new("tsc", 31, 100).err() == Some(D_DAY_RANGE) {
        caught += 1;
    }

    // 3) 漂移值超界
    if DriftDay::new("tsc", 1, 10_001).err() == Some(D_DRIFT_RANGE) {
        caught += 1;
    }

    // 4) 重复日
    let mut l = DriftLedger::new("fs").expect("l");
    let _ = l.record(1, 100);
    if l.record(1, 200) == Err(D_DAY_DUP) {
        caught += 1;
    }

    // 5) 统计未就绪
    let e = DriftLedger::new("net").expect("e");
    if e.rolling_mean() == Err(D_STATS_NOT_READY) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性与敏感性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_ledger().fingerprint();
    let b = fixture_ledger().fingerprint();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    let mut alt = DriftLedger::new("tsc").expect("alt");
    for day in 1..=30u32 {
        let drift = if day == 15 { 999 } else { 100 + (day % 5) * 100 };
        alt.record(day, drift as u64).expect("record");
    }
    if alt.fingerprint() == a {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56063_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56063");

    // J1 观察项齐备
    let l = fixture_ledger();
    s.add("56063-J1-三十日落账", l.len() == 30, "落账日数不为 30");
    s.add("56063-J1-标签闭集", DriftDay::new("gpu", 1, 1).err() == Some(D_TAG_UNKNOWN), "未知标签未被拒绝");
    s.add("56063-J1-日序闭集", DriftDay::new("tsc", 0, 1).err() == Some(D_DAY_RANGE), "日序 0 未被拒绝");
    s.add("56063-J1-值域闭集", DriftDay::new("tsc", 1, 10_000).is_ok(), "上限 10000 应可接受");

    // J2 滚动统计
    // 末 7 日为 day 24..30：drift = 100+(day%5)*100 → day24:%4=400 day25:100 day26:200 day27:300 day28:400 day29:500 day30:100
    // sum = 400+100+200+300+400+500+100 = 2000 → 均值 2000/7 = 285
    s.add("56063-J2-滚动均值285", l.rolling_mean() == Ok(285), "滚动均值不为 285");
    s.add("56063-J2-滚动峰值500", l.rolling_peak() == Ok(500), "滚动峰值不为 500");
    // 超阈（>300）：day%5==3 → 400 与 day%5==4 → 500，各 6 天 → 12
    s.add("56063-J2-超阈日12", l.over_threshold_count() == 12, "超阈日数不为 12");
    s.add("56063-J2-标记行12", l.flag_rows().len() == 12, "标记行数不为 12");
    s.add("56063-J2-标记行锚", l.flag_rows()[0] == "OVER|tsc|4|500", "首条标记行不符");

    // J3 指纹链与纪律
    s.add("56063-J3-指纹稳敏", fp_stability().is_ok(), "指纹稳定/敏感性异常");
    s.add("56063-J3-重复日拒", {
        let mut m = DriftLedger::new("env").expect("m");
        let _ = m.record(5, 100);
        m.record(5, 300) == Err(D_DAY_DUP)
    }, "重复日未被拒绝");
    s.add("56063-J3-日序回退拒", {
        let mut n = DriftLedger::new("net").expect("n");
        let _ = n.record(5, 100);
        n.record(4, 100) == Err(D_DAY_BACK)
    }, "日序回退未被拒绝");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56063-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56063_all_checks_green() {
        let s = run_unx56063_checks();
        assert!(
            s.all_passed(),
            "UNX-F56063 有红项: {}",
            s.red_items_text()
        );
    }
}
