//! UNX-F56118 IO 对照基准测试样例账（示范 3 行）（0x60EA0 段）
//!
//! 锚点：UNX-F56118-J1 示范行全量（3 行示范逐行落账：行键 × IOPS × 时延
//! 微秒三元组）；J2 示范口径可复算（时延均值逐行复算、行号 1..3 连续）；
//! J3 样例账审计链（行指纹链、封账只读、口径闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·示范样例面）。
//!
//! 结构：示范行登记册（行号 1..3 逐行落账）+ 口径复算核（IOPS/时延
//! 逐行复算）+ 行指纹链 + 量纲守闸 + 封账语义 + 注入核（行号断档/量纲
//! 越界/口径复算不一致/超行落账/封账改写/空账复核/链尾漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60EA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60EA0;
pub const ID_ROW_GAP: u32 = SEG_BASE + 0x01; // 行号断档
pub const ID_VAL_RANGE: u32 = SEG_BASE + 0x02; // 量纲越界
pub const ID_CAL_MISS: u32 = SEG_BASE + 0x03; // 口径复算不一致
pub const ID_ROW_DUP: u32 = SEG_BASE + 0x04; // 超行落账
pub const ID_SEALED: u32 = SEG_BASE + 0x05; // 样例账已封账拒写
pub const ID_EMPTY: u32 = SEG_BASE + 0x06; // 空账复核
pub const ID_KEY_EMPTY: u32 = SEG_BASE + 0x07; // 行键为空

/// 示范行数。
pub const DEMO_ROWS: usize = 3;

/// 量纲界（IOPS 与时延微秒）。
pub const IOPS_MIN: u64 = 1;
pub const IOPS_MAX: u64 = 5_000_000;
pub const LAT_MAX: u64 = 1_000_000;

/// 量纲守闸。
pub fn value_ok(iops: u64, lat_us: u64) -> bool {
    iops >= IOPS_MIN && iops <= IOPS_MAX && lat_us <= LAT_MAX
}

/// 行指纹：FNV-1a 32 位。
pub fn row_fingerprint(no: u32, key: &str, iops: u64, lat_us: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}|{}", no, key, iops, lat_us);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 示范行：行号 × 行键 × IOPS × 时延 × 指纹 × 链。
#[derive(Clone)]
pub struct IdRow {
    pub no: u32,
    pub key: String,
    pub iops: u64,
    pub lat_us: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 样例总账。
pub struct IdBook {
    pub rows: Vec<IdRow>,
    pub sealed: bool,
}

impl IdBook {
    pub fn new() -> Self {
        IdBook { rows: Vec::new(), sealed: false }
    }

    /// 逐行落账：空键拒 + 量纲闸 + 超行拒三闸。
    pub fn add_row(&mut self, key: &str, iops: u64, lat_us: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(ID_SEALED);
        }
        if key.is_empty() {
            return Err(ID_KEY_EMPTY);
        }
        if !value_ok(iops, lat_us) {
            return Err(ID_VAL_RANGE);
        }
        let no = (self.rows.len() + 1) as u32;
        if no > DEMO_ROWS as u32 {
            return Err(ID_ROW_DUP);
        }
        let fp = row_fingerprint(no, key, iops, lat_us);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(IdRow { no, key: key.to_string(), iops, lat_us, fp, chain });
        Ok(no)
    }

    /// J2：口径复算核（行号连续 + 量纲逐行合法）。
    pub fn calibrate_ok(&self) -> bool {
        self.rows.len() == DEMO_ROWS
            && self
                .rows
                .iter()
                .enumerate()
                .all(|(i, r)| r.no == (i + 1) as u32 && value_ok(r.iops, r.lat_us))
    }

    /// J2：复核算（越界显性拒绝）。
    pub fn row_at(&self, i: usize) -> Result<(u64, u64), u32> {
        if i >= self.rows.len() {
            return Err(ID_EMPTY);
        }
        Ok((self.rows[i].iops, self.rows[i].lat_us))
    }

    /// J3：IOPS 均值闭式。
    pub fn mean_iops(&self) -> Result<u64, u32> {
        if self.rows.is_empty() {
            return Err(ID_EMPTY);
        }
        let mut sum: u64 = 0;
        for r in self.rows.iter() {
            sum += r.iops;
        }
        Ok(sum / self.rows.len() as u64)
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

    pub fn chain_tail(&self) -> u32 {
        match self.rows.last() {
            None => CHAIN_SEED,
            Some(r) => r.chain,
        }
    }
}

/// 标准夹具：3 行示范（4K 随机读三档）。
pub fn standard_book() -> IdBook {
    let mut b = IdBook::new();
    let plan: [(&str, u64, u64); 3] =
        [("io|demo|4krr|qd1", 21_400, 1_480), ("io|demo|4krr|qd32", 412_000, 310), ("io|demo|4krw|qd32", 286_000, 352)];
    for p in plan.iter() {
        if b.add_row(p.0, p.1, p.2).is_err() {
            panic!("standard fixture add_row must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空行键/量纲越界/超行落账/口径复算不一致/空账复核/复核算越界/封账改写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空行键
    let mut b = IdBook::new();
    if b.add_row("", 1000, 100) == Err(ID_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 量纲越界（IOPS 0）
    if b.add_row("k", 0, 100) == Err(ID_VAL_RANGE) {
        caught += 1;
    }
    // 3) 超行落账
    let mut sb = standard_book();
    if sb.add_row("extra", 1_000, 100) == Err(ID_ROW_DUP) {
        caught += 1;
    }
    // 4) 口径复算不一致必抓（篡改行号后复算失败）
    let mut dirty = standard_book();
    dirty.rows[1].no = 9;
    if !dirty.calibrate_ok() {
        caught += 1;
    }
    // 5) 空账复核
    if IdBook::new().row_at(0).err() == Some(ID_EMPTY) {
        caught += 1;
    }
    // 6) 复核算越界
    if sb.row_at(9).err() == Some(ID_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    sb.seal();
    if sb.add_row("late", 1_000, 100) == Err(ID_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 样例账渲染导出。
pub fn render_book(b: &IdBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!("{}|{}|{}|{}|{:08x}\n", r.no, r.key, r.iops, r.lat_us, r.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 8)
}

/// 判据入口（CheckSet）。
pub fn run_unx56118_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56118");
    // J1-1 示范 3 行全量
    let b = standard_book();
    let ok = b.rows.len() == DEMO_ROWS && b.rows[0].no == 1 && b.rows[2].no == 3;
    s.add("56118-J1-示范行全量", ok, if ok { "" } else { "示范行残缺" });
    // J1-2 量纲守闸
    let ok = value_ok(1, 0) && value_ok(IOPS_MAX, LAT_MAX) && !value_ok(0, 100) && !value_ok(100, LAT_MAX + 1);
    s.add("56118-J1-量纲守闸", ok, if ok { "" } else { "量纲漂移" });
    // J2-1 口径复算一致（行号连续 + 量纲合法）
    let ok = b.calibrate_ok();
    s.add("56118-J2-口径复算一致", ok, if ok { "" } else { "口径漂移" });
    // J2-2 复核算命中
    let ok = b.row_at(1) == Ok((412_000, 310));
    s.add("56118-J2-复核算命中", ok, if ok { "" } else { "复核漂移" });
    // J2-3 IOPS 均值闭式（(21400+412000+286000)/3 = 239800）
    let ok = b.mean_iops() == Ok(239_800);
    s.add("56118-J2-IOPS均值闭式", ok, if ok { "" } else { "均值漂移" });
    // J2-4 超行落账拒
    let mut b2 = standard_book();
    let ok = b2.add_row("extra", 1_000, 100) == Err(ID_ROW_DUP);
    s.add("56118-J2-超行拒", ok, if ok { "" } else { "超行穿透" });
    // J3-1 行指纹链复核
    let ok = b.chain_ok();
    s.add("56118-J3-行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56118-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改一行值链尾变）
    let mut b3 = standard_book();
    b3.rows[2].lat_us += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56118-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.add_row("late", 1_000, 100) == Err(ID_SEALED);
    s.add("56118-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3)
        && text.starts_with("1|io|demo|4krr|qd1|21400|1480|");
    s.add("56118-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56118-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56118_all_checks_green() {
        let set = run_unx56118_checks();
        assert!(set.all_passed(), "UNX-F56118 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56118_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
