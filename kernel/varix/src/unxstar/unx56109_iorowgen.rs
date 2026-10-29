//! UNX-F56109 IO 对照行生成器（metric=io）（0x60D80 段）
//!
//! 锚点：UNX-F56109-J1 对照行全量（IO 对照行逐行落账：行键 × 值 × 单位
//! × 档名四元组）；J2 行语义一致（metric 必为 io、单位闭集命中、档名闭
//! 集命中，错径即拒）；J3 行审计链（行指纹链、封账只读、均值闭式）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·对照行面）。
//!
//! 结构：对照行登记册（行键 → 值 × 单位 × 档名逐行落账）+ 行格式核
//! （metric=io 五段格式）+ 单位闭集（iops/mbps/us）+ 档名闭集（与 F56101
//! 五档同源）+ 指纹链 + 注入核（错 metric/错单位/错档名/空行键/重复行/
//! 封账改写/空账查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60D80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60D80;
pub const IR_METRIC_MISS: u32 = SEG_BASE + 0x01; // metric 错（非 io）
pub const IR_UNIT_UNK: u32 = SEG_BASE + 0x02; // 单位越闭集
pub const IR_TIER_UNK: u32 = SEG_BASE + 0x03; // 档名越闭集
pub const IR_KEY_EMPTY: u32 = SEG_BASE + 0x04; // 行键为空
pub const IR_DUP: u32 = SEG_BASE + 0x05; // 重复行键
pub const IR_SEALED: u32 = SEG_BASE + 0x06; // 对照账已封账拒写
pub const IR_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// metric 正本口径。
pub const CANON_METRIC: &str = "io";

/// 单位闭集。
pub const U_IOPS: usize = 0;
pub const U_MBPS: usize = 1;
pub const U_US: usize = 2;
pub const U_COUNT: usize = 3;
pub const U_NAMES: [&str; U_COUNT] = ["iops", "mbps", "us"];

/// 档名闭集（与 F56101 五档同源）。
pub const R_TIER_NAMES: [&str; 5] =
    ["4k_randread", "4k_randwrite", "1m_seqread", "1m_seqwrite", "mix_70_30"];

/// 单位闭集核。
pub fn unit_ok(unit: usize) -> bool {
    unit < U_COUNT
}

/// 档名闭集核。
pub fn tier_name_ok(name: &str) -> bool {
    for t in R_TIER_NAMES.iter() {
        if *t == name {
            return true;
        }
    }
    false
}

/// 行指纹：FNV-1a 32 位。
pub fn row_fingerprint(key: &str, val: u64, unit: usize, tier: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let u = if unit < U_COUNT { U_NAMES[unit] } else { "?" };
    let line = format!("{}|{}|{}|{}", key, val, u, tier);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 对照行：行键 × 值 × 单位 × 档名 × 指纹 × 链。
#[derive(Clone)]
pub struct IoRow {
    pub key: String,
    pub val: u64,
    pub unit: usize,
    pub tier: String,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 对照行总账。
pub struct IoRowBook {
    pub rows: Vec<IoRow>,
    pub sealed: bool,
}

impl IoRowBook {
    pub fn new() -> Self {
        IoRowBook { rows: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 生成对照行：空键拒 + metric 对口径 + 单位/档名闭集 + 重复拒四闸。
    pub fn gen(
        &mut self,
        key: &str,
        val: u64,
        unit: usize,
        tier: &str,
        metric: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(IR_SEALED);
        }
        if key.is_empty() {
            return Err(IR_KEY_EMPTY);
        }
        if metric != CANON_METRIC {
            return Err(IR_METRIC_MISS);
        }
        if !unit_ok(unit) {
            return Err(IR_UNIT_UNK);
        }
        if !tier_name_ok(tier) {
            return Err(IR_TIER_UNK);
        }
        if self.find(key).is_some() {
            return Err(IR_DUP);
        }
        let fp = row_fingerprint(key, val, unit, tier);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(IoRow {
            key: key.to_string(),
            val,
            unit,
            tier: tier.to_string(),
            fp,
            chain,
        });
        Ok(())
    }

    /// 行查询（未登记显性拒绝）。
    pub fn row_of(&self, key: &str) -> Result<(u64, usize, String), u32> {
        match self.find(key) {
            Some(i) => {
                let r = &self.rows[i];
                Ok((r.val, r.unit, r.tier.clone()))
            }
            None => Err(IR_EMPTY),
        }
    }

    /// J2：行格式核（对照行文本必须五段：key|val|unit|tier|fp）。
    pub fn format_ok(line: &str) -> bool {
        line.split('|').count() == 5
    }

    /// J1：对照行全量（逐行合法）。
    pub fn rows_ok(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| {
                !r.key.is_empty() && unit_ok(r.unit) && tier_name_ok(&r.tier)
            })
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

    /// J3：iops 行均值闭式。
    pub fn mean_iops_rows(&self) -> Result<u64, u32> {
        let mut sum: u64 = 0;
        let mut n = 0usize;
        for r in self.rows.iter() {
            if r.unit == U_IOPS {
                sum += r.val;
                n += 1;
            }
        }
        if n == 0 {
            return Err(IR_EMPTY);
        }
        Ok(sum / n as u64)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.rows.last() {
            None => CHAIN_SEED,
            Some(r) => r.chain,
        }
    }
}

/// 标准夹具：4 行对照（两档 × 两单位）。
pub fn standard_book() -> IoRowBook {
    let mut b = IoRowBook::new();
    let plan: [(&str, u64, usize, &str); 4] = [
        ("io|4k_randread|iops", 412_000, U_IOPS, "4k_randread"),
        ("io|4k_randread|lat", 310, U_US, "4k_randread"),
        ("io|1m_seqread|mbps", 7_120, U_MBPS, "1m_seqread"),
        ("io|1m_seqread|lat", 1_820, U_US, "1m_seqread"),
    ];
    for p in plan.iter() {
        if b.gen(p.0, p.1, p.2, p.3, CANON_METRIC).is_err() {
            panic!("standard fixture gen must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（错 metric/错单位/错档名/空行键/重复行/封账改写/空账查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 错 metric
    let mut b = IoRowBook::new();
    if b.gen("k", 100, U_IOPS, "4k_randread", "fps") == Err(IR_METRIC_MISS) {
        caught += 1;
    }
    // 2) 错单位
    if b.gen("k", 100, 9, "4k_randread", CANON_METRIC) == Err(IR_UNIT_UNK) {
        caught += 1;
    }
    // 3) 错档名
    if b.gen("k", 100, U_IOPS, "mem", CANON_METRIC) == Err(IR_TIER_UNK) {
        caught += 1;
    }
    // 4) 空行键
    if b.gen("", 100, U_IOPS, "4k_randread", CANON_METRIC) == Err(IR_KEY_EMPTY) {
        caught += 1;
    }
    // 5) 重复行
    if b.gen("k", 100, U_IOPS, "4k_randread", CANON_METRIC).is_ok()
        && b.gen("k", 200, U_IOPS, "4k_randread", CANON_METRIC) == Err(IR_DUP)
    {
        caught += 1;
    }
    // 6) 空账查询
    if IoRowBook::new().row_of("k").err() == Some(IR_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.gen("late", 100, U_IOPS, "4k_randread", CANON_METRIC) == Err(IR_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 对照账渲染导出（逐行五段）。
pub fn render_book(b: &IoRowBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}|{:08x}\n",
            r.key, r.val, U_NAMES[r.unit], r.tier, r.fp
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
pub fn run_unx56109_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56109");
    // J1-1 对照行全量（四行逐行合法）
    let b = standard_book();
    let ok = b.rows_ok() && b.rows.len() == 4;
    s.add("56109-J1-对照行全量", ok, if ok { "" } else { "对照行残缺" });
    // J1-2 metric 正本口径
    let ok = CANON_METRIC == "io";
    s.add("56109-J1-metric正本", ok, if ok { "" } else { "metric 漂移" });
    // J1-3 单位闭集齐
    let ok = U_NAMES.len() == U_COUNT && U_NAMES[0] == "iops" && U_NAMES[2] == "us";
    s.add("56109-J1-单位闭集齐", ok, if ok { "" } else { "单位漂移" });
    // J2-1 档名闭集命中（五档同源）
    let ok = tier_name_ok("mix_70_30") && !tier_name_ok("mem") && !tier_name_ok("fps");
    s.add("56109-J2-档名闭集命中", ok, if ok { "" } else { "档名漂移" });
    // J2-2 行格式核（五段真、四段假）
    let ok = IoRowBook::format_ok("k|100|iops|4k_randread|abcd1234")
        && !IoRowBook::format_ok("k|100|iops|4k_randread");
    s.add("56109-J2-行格式核", ok, if ok { "" } else { "格式核漂移" });
    // J2-3 行查询命中
    let ok = b.row_of("io|4k_randread|iops") == Ok((412_000, U_IOPS, "4k_randread".to_string()));
    s.add("56109-J2-行查询命中", ok, if ok { "" } else { "查询漂移" });
    // J2-4 iops 行均值闭式（仅 1 行 iops → 412000）
    let ok = b.mean_iops_rows() == Ok(412_000);
    s.add("56109-J2-iops均值闭式", ok, if ok { "" } else { "均值漂移" });
    // J3-1 行指纹链复核
    let ok = b.chain_ok();
    s.add("56109-J3-行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56109-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改值链尾变）
    let mut b3 = standard_book();
    b3.rows[0].val += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56109-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.gen("late", 100, U_IOPS, "4k_randread", CANON_METRIC) == Err(IR_SEALED);
    s.add("56109-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出四行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4) && text.starts_with("io|4k_randread|iops|412000|iops|");
    s.add("56109-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56109-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56109_all_checks_green() {
        let set = run_unx56109_checks();
        assert!(set.all_passed(), "UNX-F56109 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56109_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
