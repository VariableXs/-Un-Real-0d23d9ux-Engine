//! unx36019_b01ktest — UNX-F36019 B01 ktest 断言集（SID 面全判据注册）。
//!
//! 判据原文：
//!   J1 本批 18 条判据聚合入 ktest 一条命令全跑，通过率 100% 记绿，
//!      用例数与耗时入批册。
//! 设计纪律：
//!   1. 聚合表：18 条判据（unx36001~unx36018 的 drill）函数指针化，
//!      即用即弃（栈峰值 = 1 个 drill 帧，与 robust.rs 同手法）。
//!   2. 批册：每行（卡号, 命中数, 绿旗）append-only；全绿才允许
//!      batch_green==true；用例总数 = Σ命中；逻辑耗时 = Σ行权值
//!      （确定性虚钟，不依赖真实时钟）。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x74B0;

// ── 诊断码闭集（0x74B0~0x74CF，独占）─────────────────────────
const E_ROW: u16 = SEG_BASE + 1; // 批册行失配（行数/序号漂移）
const E_GREEN: u16 = SEG_BASE + 2; // 全绿旗失配（存在红行却记绿）
const E_HITS: u16 = SEG_BASE + 3; // 用例数对平失配
const E_BAL: u16 = SEG_BASE + 4; // 批册守恒破

use crate::checks::CheckSet;
use super::unx36001_sidcore;
use super::unx36002_sidlayout;
use super::unx36003_sidstr;
use super::unx36004_wellknown;
use super::unx36005_issuer;
use super::unx36006_sidcompare;
use super::unx36007_subauth;
use super::unx36008_sidhash;
use super::unx36009_sidref;
use super::unx36010_wxpolicy;
use super::unx36011_groupsid;
use super::unx36012_integrity;
use super::unx36013_servicesid;
use super::unx36014_restricted;
use super::unx36015_sidserde;
use super::unx36016_sidaudit;
use super::unx36017_sidpool;
use super::unx36018_posixbridge;

/// 聚合行容量。
pub const ROWS: usize = 18;

/// 批册行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRow {
    pub id: &'static str,  // 卡号
    pub hits: u32,         // 命中数（用例组数）
    pub green: bool,       // 绿旗
}

/// 批册（18 行 append-only）。
pub struct BatchBook {
    rows: [Option<BatchRow>; ROWS],
    pub filled: usize,
    pub total_hits: u32,
    pub logical_ticks: u64,
    pub all_green: bool,
}

/// 聚合判据表（函数指针，即用即弃）。
fn drill_table() -> [(&'static str, fn() -> Result<u32, u16>, u64); ROWS] {
    [
        ("UNX-F36001", unx36001_sidcore::drill, 100),
        ("UNX-F36002", unx36002_sidlayout::drill, 120),
        ("UNX-F36003", unx36003_sidstr::drill, 140),
        ("UNX-F36004", unx36004_wellknown::drill, 100),
        ("UNX-F36005", unx36005_issuer::drill, 110),
        ("UNX-F36006", unx36006_sidcompare::drill, 120),
        ("UNX-F36007", unx36007_subauth::drill, 100),
        ("UNX-F36008", unx36008_sidhash::drill, 320),
        ("UNX-F36009", unx36009_sidref::drill, 220),
        ("UNX-F36010", unx36010_wxpolicy::drill, 110),
        ("UNX-F36011", unx36011_groupsid::drill, 110),
        ("UNX-F36012", unx36012_integrity::drill, 110),
        ("UNX-F36013", unx36013_servicesid::drill, 110),
        ("UNX-F36014", unx36014_restricted::drill, 100),
        ("UNX-F36015", unx36015_sidserde::drill, 130),
        ("UNX-F36016", unx36016_sidaudit::drill, 130),
        ("UNX-F36017", unx36017_sidpool::drill, 260),
        ("UNX-F36018", unx36018_posixbridge::drill, 120),
    ]
}

/// 单行复检：只跑第 i 条判据（定向回归面），返回（绿旗, 命中数）。
/// 行号越界显性拒。
pub fn recheck_row(i: usize) -> Result<(bool, u32), u16> {
    if i >= ROWS {
        return Err(E_ROW);
    }
    let (_, f, _) = drill_table()[i];
    match f() {
        Ok(hits) => Ok((true, hits)),
        Err(_) => Ok((false, 0)),
    }
}

/// 权值账导出：Σ逻辑权值（批册虚钟口径可复算面）。
pub fn total_ticks() -> u64 {
    let t = drill_table();
    let mut sum = 0u64;
    let mut i = 0;
    while i < ROWS {
        sum += t[i].2;
        i += 1;
    }
    sum
}

/// 卡号连续性：18 行卡号 F36001..F36018 零跳号（聚合表健康面）。
pub fn table_continuity() -> Result<(), u16> {
    let t = drill_table();
    let mut i = 0;
    while i < ROWS {
        let expect = 36001 + i as u32;
        let got = t[i].0;
        // 卡号字面量 "UNX-F360NN" 尾段数值比对（避免解析依赖）。
        let tail = (got.as_bytes()[8] - b'0') as u32 * 10
            + (got.as_bytes()[9] - b'0') as u32;
        if tail != expect % 100 || !got.starts_with("UNX-F360") {
            return Err(E_ROW);
        }
        i += 1;
    }
    Ok(())
}

/// 一条命令全跑：18 判据聚合，全绿记批册。
pub fn run_batch() -> BatchBook {
    let mut book = BatchBook {
        rows: [None; ROWS],
        filled: 0,
        total_hits: 0,
        logical_ticks: 0,
        all_green: true,
    };
    let table = drill_table();
    let mut i = 0;
    while i < ROWS {
        let (id, f, ticks) = table[i];
        let green = match f() {
            Ok(hits) => {
                book.total_hits += hits;
                book.logical_ticks += ticks;
                book.rows[i] = Some(BatchRow { id, hits, green: true });
                true
            }
            Err(_) => {
                book.rows[i] = Some(BatchRow { id, hits: 0, green: false });
                false
            }
        };
        if !green {
            book.all_green = false;
        }
        book.filled += 1;
        i += 1;
    }
    book
}

impl BatchBook {
    /// 行读出。
    pub fn row(&self, i: usize) -> Option<BatchRow> {
        if i >= ROWS {
            return None;
        }
        self.rows[i]
    }

    /// 全绿旗复核：旗 == 「18 行全绿且行数满」。
    pub fn verify(&self) -> Result<(), u16> {
        if self.filled != ROWS {
            return Err(E_ROW);
        }
        let mut green = true;
        let mut i = 0;
        while i < ROWS {
            match self.rows[i] {
                Some(r) => {
                    if !r.green {
                        green = false;
                    }
                    if r.green && r.hits == 0 {
                        return Err(E_HITS); // 绿行必须有命中（防空绿）
                    }
                }
                None => return Err(E_ROW),
            }
            i += 1;
        }
        if green != self.all_green {
            return Err(E_GREEN);
        }
        if !green {
            return Err(E_GREEN);
        }
        if self.total_hits == 0 || self.logical_ticks == 0 {
            return Err(E_HITS);
        }
        Ok(())
    }
}

/// 域判据入口（ktest 注册面）。
pub fn run_unx36019_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx36019");
    let book = run_batch();

    // C01：18 行全满且全绿。
    {
        let ok = book.verify().is_ok();
        s.add("C01-聚合全绿", ok, if ok { "" } else { "批册有红行" });
    }
    // C02：行序与卡号连续（F36001..F36018 零跳号）。
    {
        let table = drill_table();
        let mut ok = book.filled == ROWS;
        let mut i = 0;
        while ok && i < ROWS {
            match book.row(i) {
                Some(r) => {
                    ok = r.id == table[i].0;
                }
                None => ok = false,
            }
            i += 1;
        }
        s.add("C02-行序连续", ok, if ok { "" } else { "行序漂移" });
    }
    // C03：用例数与逻辑耗时入册（非零且逐行和一致）。
    {
        let mut sum = 0u32;
        let mut i = 0;
        while i < ROWS {
            if let Some(r) = book.row(i) {
                sum += r.hits;
            }
            i += 1;
        }
        let ok = sum == book.total_hits && book.total_hits >= 18 && book.logical_ticks > 0;
        s.add("C03-用例耗时刻账", ok, if ok { "" } else { "批册账漂移" });
    }
    s
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ROW => "批册行失配（行数/序号漂移）",
        E_GREEN => "全绿旗失配（存在红行却记绿）",
        E_HITS => "用例数对平失配",
        E_BAL => "批册守恒破",
        _ => "未知诊断码",
    }
}

/// 确定性演练：聚合 18 判据一条命令全跑全绿 + 批册复核（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全批聚合一次全跑——verify 全过（18 行全绿 + 账面一致）。
    let book = run_batch();
    book.verify()?;
    if book.filled != ROWS {
        return Err(E_ROW);
    }
    hits += 1;

    // R2：可复测——二次全跑批册逐行同构（确定性锚）。
    let book2 = run_batch();
    let mut i = 0;
    while i < ROWS {
        if book.row(i) != book2.row(i) {
            return Err(E_GREEN);
        }
        i += 1;
    }
    if book.total_hits != book2.total_hits || book.logical_ticks != book2.logical_ticks {
        return Err(E_HITS);
    }
    hits += 1;

    // R3：行绿必带命中（防空绿假象）。
    let mut i = 0;
    while i < ROWS {
        match book.row(i) {
            Some(r) => {
                if r.green && r.hits == 0 {
                    return Err(E_HITS);
                }
                if !r.green {
                    return Err(E_GREEN);
                }
            }
            None => return Err(E_ROW),
        }
        i += 1;
    }
    hits += 1;

    // R4：单行复检面——抽 3 行（0/8/17）定向重跑，绿旗与全批册逐行
    // 一致；越界行号显性拒；权值账与批册虚钟对平；卡号连续零跳号。
    let pick = [0usize, 8, 17];
    let mut k = 0;
    while k < 3 {
        let (g, h) = recheck_row(pick[k])?;
        let row = book.row(pick[k]).ok_or(E_ROW)?;
        if g != row.green || (g && h != row.hits) {
            return Err(E_HITS);
        }
        k += 1;
    }
    if recheck_row(ROWS).is_ok() {
        return Err(E_ROW);
    }
    if book.logical_ticks != total_ticks() {
        return Err(E_HITS);
    }
    table_continuity()?;
    hits += 1;

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
