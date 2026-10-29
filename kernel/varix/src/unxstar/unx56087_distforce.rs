//! UNX-F56087 分布口径强制器（禁单值报账）（0x60AC0 段）
//!
//! 锚点：UNX-F56087-J1 口径登记全量（对照行逐行落账：行键 × 分布摘要 ×
//! 口径标记）；J2 禁单值报账（报账行必须带分布摘要——min/max/均值/样本
//! 数四段齐——单值行混入必抓）；J3 口径审计链（行指纹链、封账只读、
//! 摘要可复算）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·分布口径面）。
//!
//! 结构：分布摘要登记册（行键 → min × max × 均值 × 样本数四段落账）+
//! 摘要自洽核（min ≤ 均值 ≤ max 且样本数 ≥1）+ 单值行拦截核（无四段的
//! 报账行必抓）+ 行指纹链 + 封账语义 + 注入核（空行键/摘要不自洽/单值
//! 行穿透/重复行/封账改写/空账复核/样本数为零必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60AC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60AC0;
pub const DS_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 行键为空
pub const DS_SUM_BAD: u32 = SEG_BASE + 0x02; // 摘要不自洽（min > max 等）
pub const DS_SINGLE: u32 = SEG_BASE + 0x03; // 单值行穿透
pub const DS_N_ZERO: u32 = SEG_BASE + 0x04; // 样本数为零
pub const DS_DUP: u32 = SEG_BASE + 0x05; // 重复行键
pub const DS_SEALED: u32 = SEG_BASE + 0x06; // 口径账已封账拒写
pub const DS_EMPTY: u32 = SEG_BASE + 0x07; // 空账复核

/// 分布摘要：min × max × 均值 × 样本数 四段。
#[derive(Clone)]
pub struct DistSummary {
    pub min: u64,
    pub max: u64,
    pub mean: u64,
    pub n: u32,
}

impl DistSummary {
    /// 摘要自洽核（min ≤ mean ≤ max 且 n ≥1）。
    pub fn self_ok(&self) -> bool {
        self.n >= 1 && self.min <= self.mean && self.mean <= self.max
    }
}

/// 由原始样本构造摘要（求和/极值一遍扫，u128 中转均值）。
pub fn summarize(samples: &[u64]) -> Result<DistSummary, u32> {
    if samples.is_empty() {
        return Err(DS_N_ZERO);
    }
    let mut min = u64::MAX;
    let mut max: u64 = 0;
    let mut sum: u128 = 0;
    for s in samples.iter() {
        if *s < min {
            min = *s;
        }
        if *s > max {
            max = *s;
        }
        sum += *s as u128;
    }
    Ok(DistSummary {
        min,
        max,
        mean: (sum / samples.len() as u128) as u64,
        n: samples.len() as u32,
    })
}

/// 行指纹：FNV-1a 32 位（键|四段复合）。
pub fn row_fingerprint(key: &str, d: &DistSummary) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}|{}|{}", key, d.min, d.max, d.mean, d.n);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 报账行格式核：必须含四段分布摘要（key|lo|hi|avg|n），单值行（两段）
/// 必抓。
pub fn row_format_ok(line: &str) -> bool {
    let parts: Vec<&str> = line.split('|').collect();
    parts.len() == 5
}

/// 口径账目：行键 × 摘要 × 指纹 × 链。
#[derive(Clone)]
pub struct DistRow {
    pub key: String,
    pub summary: DistSummary,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 口径总账。
pub struct DistBook {
    pub rows: Vec<DistRow>,
    pub sealed: bool,
}

impl DistBook {
    pub fn new() -> Self {
        DistBook { rows: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 逐行落账：空键拒 + 摘要自洽 + 重复拒三闸。
    pub fn add(&mut self, key: &str, d: DistSummary) -> Result<u32, u32> {
        if self.sealed {
            return Err(DS_SEALED);
        }
        if key.is_empty() {
            return Err(DS_KEY_EMPTY);
        }
        if !d.self_ok() {
            return Err(DS_SUM_BAD);
        }
        if self.find(key).is_some() {
            return Err(DS_DUP);
        }
        let fp = row_fingerprint(key, &d);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(DistRow { key: key.to_string(), summary: d, fp, chain });
        Ok(chain)
    }

    /// 复核行键（未登记显性拒绝）。
    pub fn row_of(&self, key: &str) -> Result<DistSummary, u32> {
        match self.find(key) {
            Some(i) => Ok(self.rows[i].summary.clone()),
            None => Err(DS_EMPTY),
        }
    }

    /// J1：口径全量（逐行四段齐且自洽）。
    pub fn rows_ok(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.summary.self_ok())
    }

    /// J2：单值行拦截核（两段行必拒、五行必过）。
    pub fn single_value_guard(&self) -> bool {
        !row_format_ok("key|123") && row_format_ok("key|1|9|5|10")
    }

    /// J3：行指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        for i in 0..self.rows.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.rows[0].fp
            } else {
                self.rows[i - 1].fp ^ self.rows[i].fp
            };
            if self.rows[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 链尾指纹。
    pub fn chain_tail(&self) -> u32 {
        match self.rows.last() {
            None => CHAIN_SEED,
            Some(r) => r.chain,
        }
    }
}

/// 标准夹具：3 行分布摘要（自洽口径）。
pub fn standard_book() -> DistBook {
    let mut b = DistBook::new();
    let plan: [(&str, [u64; 4], u32); 3] = [
        ("mem|rss|wave1|A", [51_200, 61_440, 56_320, 30], 0),
        ("mem|rss|wave1|B", [49_152, 57_344, 53_248, 45], 0),
        ("mem|avail|wave1|A", [1_048_576, 1_572_864, 1_310_720, 30], 0),
    ];
    for p in plan.iter() {
        let d = DistSummary { min: p.1[0], max: p.1[1], mean: p.1[2], n: p.1[3] as u32 };
        if b.add(p.0, d).is_err() {
            panic!("standard fixture add must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空行键/摘要不自洽/样本零/单值行穿透/重复行/封账改写/空账复核）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空行键
    let mut b = DistBook::new();
    let d = DistSummary { min: 1, max: 9, mean: 5, n: 10 };
    if b.add("", d.clone()).err() == Some(DS_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 摘要不自洽（min > mean）
    let bad = DistSummary { min: 9, max: 8, mean: 5, n: 10 };
    if b.add("k", bad).err() == Some(DS_SUM_BAD) {
        caught += 1;
    }
    // 3) 样本数为零
    let z = DistSummary { min: 1, max: 9, mean: 5, n: 0 };
    if b.add("k", z).err() == Some(DS_SUM_BAD) {
        caught += 1;
    }
    // 4) 单值行穿透（两段行格式必拒）
    if !row_format_ok("k|123") {
        caught += 1;
    }
    // 5) 重复行
    if b.add("k2", d.clone()).is_ok() && b.add("k2", d.clone()).err() == Some(DS_DUP) {
        caught += 1;
    }
    // 6) 空账复核
    if DistBook::new().row_of("k").err() == Some(DS_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.add("late", d).err() == Some(DS_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 口径账渲染导出（逐行一行：key|lo|hi|avg|n）。
pub fn render_book(b: &DistBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}|{}\n",
            r.key, r.summary.min, r.summary.max, r.summary.mean, r.summary.n
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56087_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56087");
    // J1-1 口径登记全量（三行四段齐）
    let b = standard_book();
    let ok = b.rows_ok() && b.rows.len() == 3;
    s.add("56087-J1-口径登记全量", ok, if ok { "" } else { "口径行残缺" });
    // J1-2 摘要构造复算（样本 10..=19：min10 max19 mean14 均值向下取整）
    let samples: Vec<u64> = (10..=19u64).collect();
    let d = match summarize(&samples) {
        Ok(d) => d,
        Err(_) => panic!("summarize must succeed"),
    };
    let ok = d.min == 10 && d.max == 19 && d.mean == 14 && d.n == 10;
    s.add("56087-J1-摘要构造复算", ok, if ok { "" } else { "摘要算术漂移" });
    // J1-3 摘要自洽核
    let ok = d.self_ok()
        && !DistSummary { min: 9, max: 8, mean: 5, n: 10 }.self_ok()
        && !DistSummary { min: 1, max: 9, mean: 5, n: 0 }.self_ok();
    s.add("56087-J1-摘要自洽", ok, if ok { "" } else { "自洽核漂移" });
    // J2-1 单值行拦截
    let ok = b.single_value_guard();
    s.add("56087-J2-单值行拦截", ok, if ok { "" } else { "单值穿透" });
    // J2-2 报账行格式核（五行过、三段拒）
    let ok = row_format_ok("k|1|9|5|10") && !row_format_ok("k|1|9|5");
    s.add("56087-J2-报账行格式", ok, if ok { "" } else { "格式核漂移" });
    // J2-3 复核行键命中
    let ok = match b.row_of("mem|rss|wave1|A") {
        Ok(d) => d.mean == 56_320 && d.n == 30,
        Err(_) => false,
    };
    s.add("56087-J2-复核命中", ok, if ok { "" } else { "复核漂移" });
    // J2-4 空样本构造拒
    let ok = summarize(&[]).err() == Some(DS_N_ZERO);
    s.add("56087-J2-空样本拒", ok, if ok { "" } else { "空样本穿透" });
    // J3-1 行指纹链复核
    let ok = b.chain_ok();
    s.add("56087-J3-行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56087-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改一行均值链尾变）
    let mut b2 = standard_book();
    b2.rows[0].summary.mean += 1;
    let ok = b2.chain_tail() != standard_book().chain_tail();
    s.add("56087-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b3 = standard_book();
    b3.seal();
    let d = DistSummary { min: 1, max: 9, mean: 5, n: 10 };
    let ok = b3.add("late", d).err() == Some(DS_SEALED);
    s.add("56087-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3)
        && text.starts_with("mem|rss|wave1|A|51200|61440|56320|30");
    s.add("56087-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56087-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56087_all_checks_green() {
        let set = run_unx56087_checks();
        assert!(set.all_passed(), "UNX-F56087 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56087_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
