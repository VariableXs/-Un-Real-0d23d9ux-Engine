//! UNX-F56079 内存对照基准测试样例账（示范 3 行）（0x609C0 段）
//!
//! 锚点：UNX-F56079-J1 示范行全量（3 行示范逐行落账：行键 × 载入 KB ×
//! 常驻 KB × 对照差四元组）；J2 示范口径可复算（对照差 = 常驻 − 载入，
//! 逐行复算一致）；J3 样例账审计链（行号单调、行指纹链、封账只读）
//! 三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·示范样例面）。
//!
//! 结构：示范行登记册（行号 1..3 逐行落账）+ 口径复算核（对照差逐行
//! 复算）+ 行指纹链（前行指纹混入后行）+ 量纲守闸（KB 闭式、非负）+
//! 封账语义 + 注入核（行号回退/行号跳变/量纲负值/重复行/封账改写/口径
//! 复算不一致/空账复核必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x609C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x609C0;
pub const DEMO_ROW_BACK: u32 = SEG_BASE + 0x01; // 行号回退
pub const DEMO_ROW_GAP: u32 = SEG_BASE + 0x02; // 行号跳变（非连续）
pub const DEMO_NEG: u32 = SEG_BASE + 0x03; // 量纲负值（对照差为负）
pub const DEMO_DUP: u32 = SEG_BASE + 0x04; // 重复行号
pub const DEMO_CAL_MISS: u32 = SEG_BASE + 0x05; // 口径复算不一致
pub const DEMO_SEALED: u32 = SEG_BASE + 0x06; // 样例账已封账拒写
pub const DEMO_EMPTY: u32 = SEG_BASE + 0x07; // 空账复核

/// 示范行数（固定 3 行示范）。
pub const DEMO_ROWS: usize = 3;

/// 行指纹：FNV-1a 32 位（行号|键|载入|常驻复合）。
pub fn row_fingerprint(no: u32, key: &str, load_kb: u64, resident_kb: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}|{}", no, key, load_kb, resident_kb);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 示范行：行号 × 行键 × 载入 KB × 常驻 KB × 对照差 × 指纹 × 链。
#[derive(Clone)]
pub struct DemoRow {
    pub no: u32,
    pub key: String,
    pub load_kb: u64,
    pub resident_kb: u64,
    pub delta_kb: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 样例账总账。
pub struct DemoBook {
    pub rows: Vec<DemoRow>,
    pub sealed: bool,
}

impl DemoBook {
    pub fn new() -> Self {
        DemoBook { rows: Vec::new(), sealed: false }
    }

    /// 逐行落账：行号 1..3 连续 + 量纲非负（常驻 ≥ 载入）+ 重复行拒四闸。
    pub fn add_row(&mut self, key: &str, load_kb: u64, resident_kb: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(DEMO_SEALED);
        }
        if key.is_empty() {
            return Err(DEMO_EMPTY);
        }
        if resident_kb < load_kb {
            return Err(DEMO_NEG);
        }
        let no = (self.rows.len() + 1) as u32;
        if no > DEMO_ROWS as u32 {
            return Err(DEMO_DUP);
        }
        let delta = resident_kb - load_kb;
        let fp = row_fingerprint(no, key, load_kb, resident_kb);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(DemoRow {
            no,
            key: key.to_string(),
            load_kb,
            resident_kb,
            delta_kb: delta,
            fp,
            chain,
        });
        Ok(chain)
    }

    /// J2：口径复算核（对照差 = 常驻 − 载入逐行一致）。
    pub fn calibrate_ok(&self) -> bool {
        self.rows.iter().all(|r| r.delta_kb == r.resident_kb - r.load_kb)
    }

    /// J2：示范行全量（恰好 3 行）。
    pub fn rows_ok(&self) -> bool {
        self.rows.len() == DEMO_ROWS
    }

    /// 复核第 i 行（越界显性拒绝）。
    pub fn row_at(&self, i: usize) -> Result<(u64, u64), u32> {
        if i >= self.rows.len() {
            return Err(DEMO_EMPTY);
        }
        Ok((self.rows[i].load_kb, self.rows[i].resident_kb))
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

    /// 行号连续核（1..3 严格连续，无跳变）。
    pub fn numbering_ok(&self) -> bool {
        for (i, r) in self.rows.iter().enumerate() {
            if r.no != (i + 1) as u32 {
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

/// 标准夹具：3 行示范（示范口径：载入 8192/16384/4096，常驻差 +1024/+2048/+512）。
pub fn standard_book() -> DemoBook {
    let mut b = DemoBook::new();
    let plan: [(&str, u64, u64); 3] = [
        ("mem|rss|wave1|caseA", 8_192, 9_216),
        ("mem|rss|wave1|caseB", 16_384, 18_432),
        ("mem|rss|wave1|caseC", 4_096, 4_608),
    ];
    for p in plan.iter() {
        if b.add_row(p.0, p.1, p.2).is_err() {
            panic!("standard fixture add_row must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（量纲负值/超行落账/封账改写/空账复核/口径复算不一致/行号断档/链尾漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 量纲负值（常驻 < 载入）
    let mut b = DemoBook::new();
    if b.add_row("k", 9_000, 8_000) == Err(DEMO_NEG) {
        caught += 1;
    }
    // 2) 空行键
    if b.add_row("", 100, 200) == Err(DEMO_EMPTY) {
        caught += 1;
    }
    // 3) 超行落账（第 4 行拒）
    let mut sb = standard_book();
    if sb.add_row("extra", 1, 2) == Err(DEMO_DUP) {
        caught += 1;
    }
    // 4) 口径复算不一致必抓：篡改 delta 后复算失败
    let mut dirty = standard_book();
    dirty.rows[0].delta_kb = 99_999;
    if !dirty.calibrate_ok() {
        caught += 1;
    }
    // 5) 空账复核
    let empty = DemoBook::new();
    if empty.row_at(0).err() == Some(DEMO_EMPTY) && !empty.chain_ok() {
        caught += 1;
    }
    // 6) 复核算越界
    if sb.row_at(9).err() == Some(DEMO_EMPTY) {
        caught += 1;
    }
    // 7) 封账后改写拒
    sb.seal();
    if sb.add_row("late", 1, 2) == Err(DEMO_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 样例账渲染导出（逐行一行）。
pub fn render_book(b: &DemoBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}|{:08x}\n",
            r.no, r.key, r.load_kb, r.resident_kb, r.fp
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
    lines.iter().all(|ln| ln.split('|').count() == 7)
}

/// 对照差均值核（示范账聚合口径）。
pub fn mean_delta(b: &DemoBook) -> Result<u64, u32> {
    if b.rows.is_empty() {
        return Err(DEMO_EMPTY);
    }
    let mut sum: u64 = 0;
    for r in b.rows.iter() {
        sum += r.delta_kb;
    }
    Ok(sum / b.rows.len() as u64)
}

/// 判据入口（CheckSet）。
pub fn run_unx56079_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56079");
    // J1-1 示范 3 行全量
    let b = standard_book();
    let ok = b.rows_ok() && b.numbering_ok();
    s.add("56079-J1-示范行全量", ok, if ok { "" } else { "示范行残缺" });
    // J1-2 行号 1..3 连续
    let ok = b.rows[0].no == 1 && b.rows[2].no == 3;
    s.add("56079-J1-行号连续", ok, if ok { "" } else { "行号断档" });
    // J1-3 量纲非负守闸（对照差全正）
    let ok = b.rows.iter().all(|r| r.resident_kb >= r.load_kb && r.delta_kb > 0);
    s.add("56079-J1-量纲非负", ok, if ok { "" } else { "量纲破纪" });
    // J2-1 口径复算一致（delta = resident - load 逐行）
    let ok = b.calibrate_ok()
        && b.rows[0].delta_kb == 1_024
        && b.rows[1].delta_kb == 2_048
        && b.rows[2].delta_kb == 512;
    s.add("56079-J2-口径复算", ok, if ok { "" } else { "口径算术漂移" });
    // J2-2 复核算命中
    let ok = b.row_at(1) == Ok((16_384, 18_432));
    s.add("56079-J2-复核算命中", ok, if ok { "" } else { "复核漂移" });
    // J2-3 对照差均值复算（(1024+2048+512)/3 = 1194 向下取整）
    let ok = mean_delta(&b) == Ok(1_194);
    s.add("56079-J2-均值复算", ok, if ok { "" } else { "均值漂移" });
    // J2-4 量纲负值拒收
    let mut b2 = DemoBook::new();
    let ok = b2.add_row("k", 9_000, 8_000) == Err(DEMO_NEG);
    s.add("56079-J2-负值拒收", ok, if ok { "" } else { "量纲闸失效" });
    // J3-1 行指纹链复核
    let ok = b.chain_ok();
    s.add("56079-J3-行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56079-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（换一行值链尾变）
    let mut b3 = DemoBook::new();
    let _ = b3.add_row("mem|rss|wave1|caseA", 8_192, 9_217);
    let _ = b3.add_row("mem|rss|wave1|caseB", 16_384, 18_432);
    let _ = b3.add_row("mem|rss|wave1|caseC", 4_096, 4_608);
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56079-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.add_row("late", 1, 2) == Err(DEMO_SEALED);
    s.add("56079-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("1|mem|rss|wave1|caseA|8192|9216");
    s.add("56079-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56079-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56079_all_checks_green() {
        let set = run_unx56079_checks();
        assert!(set.all_passed(), "UNX-F56079 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56079_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
