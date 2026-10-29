//! UNX-F56132 帧率缺侧 N/A 纪律实例件（0x61060 段）
//!
//! 锚点：UNX-F56132-J1 缺侧账全量（对照行逐条落账：键 × A 侧 × B 侧
//! × 缺侧标记）；J2 纪律语义（缺侧必标原因——四因闭集——缺侧禁填数、
//! 账目闭式复算）；J3 纪律审计链（封账门、指纹、渲染完整）三判据全
//! 绿。
//! 域：CoRun·UNX-F2-B40（帧率域·缺侧纪律面）。
//!
//! 结构：行登记簿（键唯一、数值带）+ 缺侧标记核（侧号/原因越界、缺
//! 侧禁填数）+ 原因账目闭式（按因计数复算）+ 指纹链 + 注入核（原因越
//! 界/侧号越界/缺侧填数/重复行/缺行标记/封账拒写/账目漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61060 独占 0x20）。
pub const SEG_BASE: u32 = 0x61060;
pub const NA_REASON_OVER: u32 = SEG_BASE + 0x01; // 缺侧原因越界
pub const NA_SIDE_OVER: u32 = SEG_BASE + 0x02; // 侧号越界
pub const NA_FABRIC: u32 = SEG_BASE + 0x03; // 缺侧填数/数值非法
pub const NA_DUP_ROW: u32 = SEG_BASE + 0x04; // 重复行/重复缺侧标记
pub const NA_NO_ROW: u32 = SEG_BASE + 0x05; // 缺行查询
pub const NA_SEALED: u32 = SEG_BASE + 0x06; // 封账拒写
pub const NA_CENSUS_MISS: u32 = SEG_BASE + 0x07; // 缺侧账目漂移

/// 缺侧原因四因闭集。
pub const REASONS: [&str; 4] = ["no_collector", "collect_fail", "below_min", "sealed_wave"];

pub fn reason_ok(r: usize) -> bool {
    r < REASONS.len()
}

/// 帧率 ×100 定点合法带。
pub const FPS_MIN: u64 = 100;
pub const FPS_MAX: u64 = 120_000;

pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

fn fold_str(mut h: u32, s: &str) -> u32 {
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(CHAIN_PRIME);
    }
    h
}

/// 缺侧行。
pub struct NaRow {
    pub no: usize,
    pub key: String,
    pub a: Option<u64>,
    pub b: Option<u64>,
    pub marks: Vec<(usize, usize)>, // (side, reason)
}

/// 帧率缺侧纪律登记簿。
pub struct NaDiscBook {
    cap: usize,
    rows: Vec<NaRow>,
    na_total: u64,
    chain: u32,
    sealed: bool,
}

impl NaDiscBook {
    pub fn new(cap: usize) -> Self {
        NaDiscBook {
            cap,
            rows: Vec::new(),
            na_total: 0,
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn na_total(&self) -> u64 {
        self.na_total
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 行切片只读访问。
    pub fn rows(&self) -> &[NaRow] {
        &self.rows
    }

    /// 落行：键唯一、缺侧（None）自由、填数必须合法。
    pub fn add_row(
        &mut self,
        scene: &str,
        wave: u32,
        a: Option<u64>,
        b: Option<u64>,
    ) -> Result<usize, u32> {
        if self.sealed || self.rows.len() >= self.cap {
            return Err(NA_SEALED);
        }
        if scene.is_empty() {
            return Err(NA_NO_ROW);
        }
        if let Some(v) = a {
            if v == 0 || !fps_ok(v) {
                return Err(NA_FABRIC);
            }
        }
        if let Some(v) = b {
            if v == 0 || !fps_ok(v) {
                return Err(NA_FABRIC);
            }
        }
        let key = format!("fps|{}|w{}", scene, wave);
        for r in &self.rows {
            if r.key == key {
                return Err(NA_DUP_ROW);
            }
        }
        let no = self.rows.len();
        self.rows.push(NaRow {
            no,
            key,
            a,
            b,
            marks: Vec::new(),
        });
        Ok(no)
    }

    fn find(&self, key: &str) -> Result<usize, u32> {
        for (i, r) in self.rows.iter().enumerate() {
            if r.key == key {
                return Ok(i);
            }
        }
        Err(NA_NO_ROW)
    }

    /// 缺侧标记：侧号/原因越界 → 缺行 → 缺侧禁填数 → 重复标记。
    pub fn mark_na(&mut self, key: &str, side: usize, reason: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_SEALED);
        }
        if side > 1 {
            return Err(NA_SIDE_OVER);
        }
        if !reason_ok(reason) {
            return Err(NA_REASON_OVER);
        }
        let i = self.find(key)?;
        let filled = if side == 0 {
            self.rows[i].a.is_some()
        } else {
            self.rows[i].b.is_some()
        };
        if filled {
            return Err(NA_FABRIC);
        }
        for m in &self.rows[i].marks {
            if m.0 == side {
                return Err(NA_DUP_ROW);
            }
        }
        self.rows[i].marks.push((side, reason));
        self.chain = fold_str(self.chain, &self.rows[i].key);
        self.chain = fold_u32(self.chain, side as u32);
        self.chain = fold_u32(self.chain, reason as u32);
        Ok(())
    }

    /// 纪律复核：任何缺侧标记对侧不得有数。
    pub fn verify_rows(&self) -> Result<(), u32> {
        for r in &self.rows {
            for m in &r.marks {
                let filled = if m.0 == 0 { r.a.is_some() } else { r.b.is_some() };
                if filled {
                    return Err(NA_FABRIC);
                }
            }
        }
        Ok(())
    }

    /// 按因计数复算。
    pub fn reason_census(&self) -> [u64; 4] {
        let mut c = [0u64; 4];
        for r in &self.rows {
            for m in &r.marks {
                c[m.1] += 1;
            }
        }
        c
    }

    /// 账目对账：总数与按因计数复算一致。
    pub fn verify_census(&self) -> Result<(), u32> {
        let c = self.reason_census();
        let sum: u64 = c.iter().sum();
        if sum != self.na_total {
            return Err(NA_CENSUS_MISS);
        }
        if self.chain != chain_of(&self.rows) {
            return Err(NA_CENSUS_MISS);
        }
        Ok(())
    }

    /// 封账：空账拒绝、定缺侧总数与指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_SEALED);
        }
        if self.rows.is_empty() {
            return Err(NA_NO_ROW);
        }
        self.na_total = self
            .rows
            .iter()
            .map(|r| r.marks.len() as u64)
            .sum();
        self.chain = chain_of(&self.rows);
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 7 段（no|key(3)|a|b|标记数）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in &self.rows {
            let da = r.a.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
            let db = r.b.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
            out.push(format!(
                "{}|{}|{}|{}|{}",
                r.no,
                r.key,
                da,
                db,
                r.marks.len()
            ));
        }
        out
    }

    /// 测试注入口：仅注入核使用。
    fn break_census(&mut self) {
        self.na_total += 1;
    }
}

/// 指纹链：行键序（插入序）× 标记展开重算。
pub fn chain_of(rows: &[NaRow]) -> u32 {
    let mut h = CHAIN_SEED;
    for r in rows {
        h = fold_str(h, &r.key);
        h = fold_u32(h, r.a.unwrap_or(0) as u32);
        h = fold_u32(h, r.b.unwrap_or(0) as u32);
        for m in &r.marks {
            h = fold_u32(h, m.0 as u32);
            h = fold_u32(h, m.1 as u32);
        }
    }
    h
}

/// 标准账：一完整行 + 一单侧缺 + 一双侧缺（缺侧总数 3）。
fn fixture() -> NaDiscBook {
    let mut b = NaDiscBook::new(8);
    let _ = b.add_row("scroll", 1, Some(12_000), Some(11_950));
    let _ = b.add_row("drag", 1, Some(9_000), None);
    let _ = b.mark_na("fps|drag|w1", 1, 1); // collect_fail
    let _ = b.add_row("animation", 1, None, None);
    let _ = b.mark_na("fps|animation|w1", 0, 0); // no_collector
    let _ = b.mark_na("fps|animation|w1", 1, 2); // below_min
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 原因越界必抓。
    {
        let mut b = NaDiscBook::new(8);
        let _ = b.add_row("scroll", 1, Some(12_000), None);
        caught += (b.mark_na("fps|scroll|w1", 1, 9) == Err(NA_REASON_OVER)) as usize;
    }
    // 2 侧号越界必抓。
    {
        let mut b = NaDiscBook::new(8);
        let _ = b.add_row("scroll", 1, Some(12_000), None);
        caught += (b.mark_na("fps|scroll|w1", 2, 0) == Err(NA_SIDE_OVER)) as usize;
    }
    // 3 缺侧填数必抓（有数侧禁标）。
    {
        let mut b = NaDiscBook::new(8);
        let _ = b.add_row("scroll", 1, Some(12_000), Some(11_950));
        caught += (b.mark_na("fps|scroll|w1", 0, 0) == Err(NA_FABRIC)) as usize;
    }
    // 4 重复行必抓。
    {
        let mut b = NaDiscBook::new(8);
        let _ = b.add_row("scroll", 1, Some(12_000), None);
        caught += (b.add_row("scroll", 1, None, None) == Err(NA_DUP_ROW)) as usize;
    }
    // 5 缺行标记必抓。
    {
        let mut b = NaDiscBook::new(8);
        let _ = b.add_row("scroll", 1, Some(12_000), None);
        caught += (b.mark_na("fps|web|w1", 1, 0) == Err(NA_NO_ROW)) as usize;
    }
    // 6 封账拒写必抓。
    {
        let mut b = fixture();
        caught += (b.add_row("web", 1, Some(12_000), None) == Err(NA_SEALED)) as usize;
    }
    // 7 账目漂移必抓。
    {
        let mut b = fixture();
        b.break_census();
        caught += (b.verify_census() == Err(NA_CENSUS_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56132 全量判据（15 项）。
pub fn run_unx56132_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56132");
    let b = fixture();

    // J1 缺侧账全量。
    let ok = b.len() == 3;
    s.add("56132-J1-行三条", ok, if ok { "" } else { "行数不符" });
    let ok = REASONS.len() == 4 && reason_ok(3) && !reason_ok(4);
    s.add("56132-J1-原因闭集", ok, if ok { "" } else { "原因闭集漂移" });
    let ok = b.na_total() == 3;
    s.add("56132-J1-缺侧总数", ok, if ok { "" } else { "缺侧总数漂移" });

    // J2 纪律语义。
    let ok = b.rows()[0].marks.is_empty();
    s.add("56132-J2-完整行无标", ok, if ok { "" } else { "完整行误标" });
    let drag = &b.rows()[1];
    let ok = drag.b.is_none() && drag.marks.len() == 1 && drag.marks[0] == (1, 1);
    s.add("56132-J2-缺侧有因", ok, if ok { "" } else { "缺侧标记漂移" });
    let anim = &b.rows()[2];
    let ok = anim.a.is_none() && anim.b.is_none() && anim.marks.len() == 2;
    s.add("56132-J2-双侧缺标", ok, if ok { "" } else { "双侧缺标记漂移" });
    let c = b.reason_census();
    let ok = c[0] == 1 && c[1] == 1 && c[2] == 1 && c[3] == 0;
    s.add("56132-J2-按因计数", ok, if ok { "" } else { "按因计数漂移" });
    let ok = b.verify_rows().is_ok();
    s.add("56132-J2-缺侧禁填数", ok, if ok { "" } else { "纪律复核失败" });
    let ok = b.rows()[0].a.unwrap_or(0) == 12_000 && b.rows()[0].b.unwrap_or(0) == 11_950;
    s.add("56132-J2-数值带在案", ok, if ok { "" } else { "在案数值漂移" });

    // J3 纪律审计链。
    let ok = b.sealed();
    s.add("56132-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.verify_census().is_ok();
    s.add("56132-J3-账目对账", ok, if ok { "" } else { "账目对账失败" });
    let ok = b.chain() != CHAIN_SEED && b.chain() == chain_of(b.rows());
    s.add("56132-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = b.render();
    let mut ok = lines.len() == 3;
    for l in &lines {
        if l.split('|').count() != 7 {
            ok = false;
        }
    }
    s.add("56132-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });
    let ok = b.find("fps|web|w1") == Err(NA_NO_ROW);
    s.add("56132-J3-缺行查询", ok, if ok { "" } else { "查询语义漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56132-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56132_all_checks_green() {
        let set = run_unx56132_checks();
        assert!(set.all_passed(), "UNX-F56132 红项: {}", set.red_items_text());
    }
}
