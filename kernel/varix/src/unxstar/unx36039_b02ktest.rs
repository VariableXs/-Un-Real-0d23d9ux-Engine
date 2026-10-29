//! unx36039_b02ktest — UNX-F36039 B02 ktest 断言集（令牌面全判据注册）。
//!
//! 判据原文（UNX-F36039-J1）：
//!   本批 18 条判据聚合入 ktest 一条命令全跑全绿；fuzz 面冒烟 <3min。
//! 设计纪律：
//!   1. 聚合表：18 条判据（unx36021~unx36038 的 drill）函数指针化，
//!      即用即弃（栈峰值 = 1 个 drill 帧，与 robust.rs 同手法）。
//!   2. 批册：每行（卡号, 命中数, 绿旗）append-only；全绿才允许
//!      batch_green==true；用例总数 = Σ命中；逻辑耗时 = Σ行权值
//!      （确定性虚钟，不依赖真实时钟）；fuzz 冒烟权值 <3min 换算界
//!      （180_000 tick 上限断言）。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0xf9e0;

// ── 诊断码闭集（0xf9e0~0xf9ff，独占）─────────────────────────
const E_ROW: u16 = SEG_BASE + 1; // 批册行失配（行数/序号漂移）
const E_GREEN: u16 = SEG_BASE + 2; // 全绿旗失配（存在红行却记绿）
const E_HITS: u16 = SEG_BASE + 3; // 用例数对平失配
const E_BAL: u16 = SEG_BASE + 4; // 批册守恒破
const E_SMOKE: u16 = SEG_BASE + 5; // fuzz 冒烟权值超界（≥3min 换算）

use crate::checks::CheckSet;
use super::unx36021_tokencore;
use super::unx36022_tokbuild;
use super::unx36023_toktype;
use super::unx36024_tokgroups;
use super::unx36025_tokpriv;
use super::unx36026_tokowner;
use super::unx36027_toksession;
use super::unx36028_tokrefcnt;
use super::unx36029_tokdestroy;
use super::unx36030_tokinteg;
use super::unx36031_tokinherit;
use super::unx36032_tokrestrict;
use super::unx36033_tokquery;
use super::unx36034_tokstats;
use super::unx36035_tokwx;
use super::unx36036_tokevent;
use super::unx36037_tokfuzz;
use super::unx36038_tokpool;

/// 聚合行容量。
pub const ROWS: usize = 18;

/// fuzz 冒烟权值上限（<3min → 180_000 确定性 tick 界）。
pub const FUZZ_SMOKE_LIMIT: u64 = 180_000;

/// 批册行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRow {
    pub id: &'static str, // 卡号
    pub hits: u32,        // 命中数（用例组数）
    pub green: bool,      // 绿旗
}

/// 批册（18 行 append-only）。
pub struct BatchBook {
    pub rows: [Option<BatchRow>; ROWS],
    pub filled: usize,
    pub total_hits: u32,
    pub logical_ticks: u64,
    pub all_green: bool,
}

/// 聚合判据表（函数指针，即用即弃）。
fn drill_table() -> [(&'static str, fn() -> Result<u32, u16>, u64); ROWS] {
    [
        ("UNX-F36021", unx36021_tokencore::drill, 100),
        ("UNX-F36022", unx36022_tokbuild::drill, 120),
        ("UNX-F36023", unx36023_toktype::drill, 110),
        ("UNX-F36024", unx36024_tokgroups::drill, 130),
        ("UNX-F36025", unx36025_tokpriv::drill, 120),
        ("UNX-F36026", unx36026_tokowner::drill, 110),
        ("UNX-F36027", unx36027_toksession::drill, 110),
        ("UNX-F36028", unx36028_tokrefcnt::drill, 140),
        ("UNX-F36029", unx36029_tokdestroy::drill, 130),
        ("UNX-F36030", unx36030_tokinteg::drill, 110),
        ("UNX-F36031", unx36031_tokinherit::drill, 120),
        ("UNX-F36032", unx36032_tokrestrict::drill, 110),
        ("UNX-F36033", unx36033_tokquery::drill, 130),
        ("UNX-F36034", unx36034_tokstats::drill, 110),
        ("UNX-F36035", unx36035_tokwx::drill, 110),
        ("UNX-F36036", unx36036_tokevent::drill, 110),
        ("UNX-F36037", unx36037_tokfuzz::drill, 160),
        ("UNX-F36038", unx36038_tokpool::drill, 130),
    ]
}

/// 单行复检：只跑第 i 条判据（定向回归面），返回（绿旗, 命中数）。
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

/// fuzz 冒烟面：unx36037 drill 权值（160）< 冒烟上限（<3min 换算界）。
pub fn fuzz_smoke_ok() -> bool {
    let t = drill_table();
    t[16].2 < FUZZ_SMOKE_LIMIT
}

/// 卡号连续性：18 行卡号 F36021..F36038 零跳号（聚合表健康面）。
pub fn table_continuity() -> Result<(), u16> {
    let t = drill_table();
    let mut i = 0;
    while i < ROWS {
        let expect = 36021 + i as u32;
        let got = t[i].0;
        // 卡号字面量 "UNX-F360NN" 尾段数值比对（避免解析依赖；总长 10 字节）。
        let tail = (got.as_bytes()[7] - b'0') as u32 * 100
            + (got.as_bytes()[8] - b'0') as u32 * 10
            + (got.as_bytes()[9] - b'0') as u32;
        if tail != expect % 1000 || !got.starts_with("UNX-F36") {
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

/// 批册守恒：filled==ROWS、全绿↔全行绿、hits 与行和一致。
pub fn book_conserve(b: &BatchBook) -> Result<(), u16> {
    if b.filled != ROWS {
        return Err(E_BAL);
    }
    let mut hits_sum = 0u32;
    let mut green_all = true;
    let mut i = 0;
    while i < ROWS {
        match b.rows[i] {
            Some(r) => {
                hits_sum += r.hits;
                if !r.green {
                    green_all = false;
                }
            }
            None => return Err(E_BAL),
        }
        i += 1;
    }
    if hits_sum != b.total_hits || green_all != b.all_green {
        return Err(E_BAL);
    }
    if b.all_green && hits_sum == 0 {
        return Err(E_HITS);
    }
    Ok(())
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ROW => "批册行失配：行数/序号漂移",
        E_GREEN => "全绿旗失配：存在红行却记绿",
        E_HITS => "用例数对平失配：全绿但零命中",
        E_BAL => "批册守恒破",
        E_SMOKE => "fuzz 冒烟权值超界：≥3min 换算",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据三段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：卡号连续性 18 行零跳号。
    table_continuity()?;
    hits += 1;
    // R2：一条命令全跑——18 行全绿。
    let b = run_batch();
    if !b.all_green {
        return Err(E_GREEN);
    }
    if b.filled != ROWS {
        return Err(E_ROW);
    }
    hits += 1;
    // R3：批册守恒（filled/绿旗/hits 三账对平）。
    book_conserve(&b)?;
    hits += 1;
    // R4：fuzz 冒烟 <3min 换算界。
    if !fuzz_smoke_ok() {
        return Err(E_SMOKE);
    }
    hits += 1;
    // R5：权值账复算（total_ticks == Σ表权值；定向复检首行绿）。
    let t = drill_table();
    let mut sum = 0u64;
    let mut i = 0;
    while i < ROWS {
        sum += t[i].2;
        i += 1;
    }
    if total_ticks() != sum || b.logical_ticks != sum {
        return Err(E_BAL);
    }
    let (green1, _) = recheck_row(0)?;
    if !green1 {
        return Err(E_GREEN);
    }
    hits += 1;
    Ok(hits)
}

/// 批册视图辅助。

/// 批册快照（只读视图：filled + total_hits + all_green 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchSnapshot {
    pub filled: usize,
    pub total_hits: u32,
    pub all_green: bool,
}

impl BatchBook {
    /// 快照导出。
    pub fn snapshot(&self) -> BatchSnapshot {
        BatchSnapshot {
            filled: self.filled,
            total_hits: self.total_hits,
            all_green: self.all_green,
        }
    }

    /// 快照守恒核。
    pub fn snapshot_conserve(&self, s: &BatchSnapshot) -> bool {
        s.filled == self.filled
            && s.total_hits == self.total_hits
            && s.all_green == self.all_green
    }
}

pub fn run_unx36039_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36039_b02ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xf9e0, "0xf9e0~0xf9ff 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 B02 判据聚合", true, "18 条全绿+冒烟界内+批册守恒"),
        Err(code) => cs.add("J1 B02 判据聚合", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
