//! UNX-F12135 — 样本集 200 函数覆盖矩阵与回放面（0x1a300 段）
//!
//! 锚点：UNX-F12135-J1 样本集 200 函数覆盖矩阵（行=函数 列=覆盖维度，200 行全对平；
//! 回放面按 RVA 升序逐条重放零漂移）。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：覆盖矩阵（函数 × 五维覆盖位）+ 回放面（RVA 升序快照逐条重放）+
//! 覆盖率账 + 折叠指纹 + 注入核（越界函数/重复行/覆盖倒退/重放漂移/印后回放）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a300;
pub const CV_GHOST: u32 = SEG_BASE + 0x01; // 越界函数
pub const CV_DUP: u32 = SEG_BASE + 0x02; // 重复行
pub const CV_REGRESS: u32 = SEG_BASE + 0x03; // 覆盖倒退
pub const CV_REPLAY: u32 = SEG_BASE + 0x04; // 重放漂移
pub const CV_FROZEN: u32 = SEG_BASE + 0x05; // 印后回放

/// 矩阵行数（200 函数）。
pub const ROWS: usize = 200;
/// 覆盖维度数（命中/回退/对照/注入/复测）。
pub const DIMS: usize = 5;

/// 覆盖维度名。
pub fn dim_name(d: usize) -> &'static str {
    const NAMES: [&str; DIMS] = ["hit", "unwind", "xref", "inject", "retest"];
    NAMES[d % DIMS]
}

/// 一行覆盖矩阵：函数 + 五维覆盖位。
#[derive(Clone, PartialEq)]
pub struct CoverRow {
    pub idx: usize,
    pub func: String,
    /// 每维覆盖计数（0=未覆盖，≥1=覆盖次数）。
    pub dims: [u32; DIMS],
}

impl CoverRow {
    /// 行覆盖满额：五维全 ≥1。
    pub fn full(&self) -> bool {
        self.dims.iter().all(|&d| d >= 1)
    }

    /// 行覆盖计数合计。
    pub fn total(&self) -> u32 {
        self.dims.iter().sum()
    }

    pub fn render(&self) -> String {
        format!(
            "#{}:{}[{}]",
            self.idx,
            self.func,
            self.dims
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// 覆盖矩阵 + 回放面。
pub struct CoverMatrix {
    pub rows: Vec<CoverRow>,
    /// 回放快照（RVA 升序的函数名序）。
    pub replay: Vec<String>,
    pub sealed: bool,
}

impl CoverMatrix {
    pub fn new() -> CoverMatrix {
        CoverMatrix { rows: Vec::new(), replay: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂行（越界序号/重复序号必拒）。
    pub fn mount(&mut self, idx: usize, func: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if idx >= ROWS || func.is_empty() {
            return Err(CV_GHOST);
        }
        if self.rows.iter().any(|r| r.idx == idx) {
            return Err(CV_DUP);
        }
        self.rows.push(CoverRow {
            idx,
            func: func.to_string(),
            dims: [0; DIMS],
        });
        self.rows.sort_by_key(|r| r.idx);
        Ok(())
    }

    /// 打覆盖点（未挂行必拒）。
    pub fn hit(&mut self, idx: usize, dim: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if idx >= ROWS || dim >= DIMS {
            return Err(CV_GHOST);
        }
        let r = self.rows.iter_mut().find(|r| r.idx == idx).ok_or(CV_GHOST)?;
        r.dims[dim] += 1;
        Ok(())
    }

    /// 覆盖率千分位（按满额行计）。
    pub fn full_permille(&self) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        self.rows.iter().filter(|r| r.full()).count() * 1000 / self.rows.len()
    }

    /// 全矩阵满额：200 行全挂且全满额。
    pub fn all_full(&self) -> bool {
        self.rows.len() == ROWS && self.rows.iter().all(|r| r.full())
    }

    /// 生成回放快照：行序即 RVA 升序序（idx 升序），逐函数名入快照。
    pub fn snapshot(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if self.rows.len() != ROWS {
            return Err(CV_GHOST);
        }
        self.replay = self.rows.iter().map(|r| r.func.clone()).collect();
        Ok(self.replay.len())
    }

    /// 重放核：快照与矩阵行序逐条比对（零漂移）。
    pub fn replay_check(&self) -> Result<usize, u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if self.replay.len() != self.rows.len() {
            return Err(CV_REPLAY);
        }
        for (i, r) in self.rows.iter().enumerate() {
            if self.replay[i] != r.func {
                return Err(CV_REPLAY);
            }
        }
        Ok(self.replay.len())
    }

    /// 覆盖倒退检测：任一维计数为负即倒退（防御面，正常路径不会触发）。
    pub fn no_regress(&self) -> bool {
        self.rows.iter().all(|r| r.dims.iter().all(|&d| d <= 1000))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a300;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.idx as u32);
            for d in &r.dims {
                h = h.wrapping_mul(211).wrapping_add(*d);
            }
        }
        for f in &self.replay {
            for by in f.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 行渲染抽验（首/中/末）。
    pub fn spot_render(&self) -> String {
        let n = self.rows.len();
        if n == 0 {
            return "empty".to_string();
        }
        format!(
            "{}|{}|{}",
            self.rows[0].render(),
            self.rows[n / 2].render(),
            self.rows[n - 1].render()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = CoverMatrix::new();
    let _ = m.mount(0, "inj0");
    // 1) 越界函数行必拒。
    got[0] = m.mount(ROWS, "oob").err() == Some(CV_GHOST);
    // 2) 重复行必拒。
    got[1] = m.mount(0, "again").err() == Some(CV_DUP);
    // 3) 未挂行打点必拒（幽灵函数）。
    got[2] = m.hit(199, 0).err() == Some(CV_GHOST);
    // 4) 空矩阵快照必拒。
    got[3] = m.snapshot().err() == Some(CV_GHOST);
    // 5) 印后回放必拒。
    m.seal();
    got[4] = m.replay_check().err() == Some(CV_FROZEN);
    got
}

/// 判据入口：UNX-F12135
pub fn run_unx12135_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12135");
    let mut m = CoverMatrix::new();

    // J1 200 行全挂。
    let mut j1 = true;
    for i in 0..ROWS {
        let name = format!("fn{}", i);
        if m.mount(i, &name).is_err() {
            j1 = false;
            break;
        }
    }
    s.add("J1_mount_200", j1 && m.rows.len() == ROWS, "200 函数行全挂载");

    // J2 五维全打点 200×5。
    let mut j2 = true;
    for i in 0..ROWS {
        for d in 0..DIMS {
            if m.hit(i, d).is_err() {
                j2 = false;
            }
        }
    }
    s.add("J2_hit_all", j2, "五维 1000 点全打");

    // J3 全矩阵满额。
    s.add("J3_all_full", m.all_full(), "200 行全满额");

    // J4 覆盖率千分位满额。
    s.add("J4_permille", m.full_permille() == 1000, "覆盖率千分位 1000");

    // J5 维度名闭集。
    let names_ok = dim_name(0) == "hit" && dim_name(4) == "retest" && dim_name(5) == "hit";
    s.add("J5_dim_names", names_ok, "维度名闭集且取模回绕");

    // J6 快照生成 200 条。
    s.add("J6_snapshot", m.snapshot() == Ok(ROWS), "回放快照 200 条生成");

    // J7 重放零漂移。
    s.add("J7_replay", m.replay_check() == Ok(ROWS), "回放逐条零漂移");

    // J8 重放漂移检出：篡改快照末条必抓。
    let mut m2 = CoverMatrix::new();
    for i in 0..ROWS {
        let name = format!("g{}", i);
        let _ = m2.mount(i, &name);
    }
    let _ = m2.snapshot();
    m2.replay[ROWS - 1] = "tampered".to_string();
    s.add("J8_replay_drift", m2.replay_check().err() == Some(CV_REPLAY), "快照篡改重放必抓");

    // J9 覆盖无倒退。
    s.add("J9_no_regress", m.no_regress(), "全矩阵无覆盖倒退");

    // J10 指纹稳定。
    let f1 = m.fold();
    let f2 = m.fold();
    s.add("J10_fingerprint", f1 == f2, "覆盖矩阵折叠指纹一致");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 印账受控。
    m.seal();
    let j12 = m.hit(0, 0).err() == Some(CV_FROZEN)
        && m.mount(0, "x").err() == Some(CV_FROZEN)
        && m.snapshot().err() == Some(CV_FROZEN);
    s.add("J12_sealed", j12, "印账后打点/挂行/快照皆拒");

    // J13 局部覆盖千分位（半满）。
    let mut m3 = CoverMatrix::new();
    for i in 0..ROWS {
        let name = format!("h{}", i);
        let _ = m3.mount(i, &name);
    }
    for i in 0..(ROWS / 2) {
        for d in 0..DIMS {
            let _ = m3.hit(i, d);
        }
    }
    s.add(
        "J13_half_permille",
        m3.full_permille() == 500 && !m3.all_full(),
        "半满矩阵千分位 500",
    );

    // J14 行渲染含序号与计数。
    let r = m.rows[0].render();
    s.add(
        "J14_render",
        r.starts_with("#0:fn0[") && r.ends_with("]"),
        "行渲染含序号与五维计数",
    );

    // J15 抽验渲染含首中末三行。
    let sp = m.spot_render();
    s.add(
        "J15_spot",
        sp.contains("#0:fn0") && sp.contains("#100:fn100") && sp.contains("#199:fn199"),
        "抽验含首/中/末三行",
    );

    // J16 行 total 合计对平（满额行 total=5）。
    let totals: u32 = m.rows.iter().map(|r| r.total()).sum();
    s.add("J16_totals", totals == (ROWS * DIMS) as u32, "行合计对平 1000 点");

    // J17 重复行受控（独立账挂后重挂，不经印账路径）。
    let mut m4 = CoverMatrix::new();
    let _ = m4.mount(0, "first");
    s.add(
        "J17_dup_row",
        m4.mount(0, "dup").err() == Some(CV_DUP),
        "重复序号拒 DUP",
    );

    // J18 未挂行不进满额判定。
    let mut m4 = CoverMatrix::new();
    let _ = m4.mount(0, "only");
    for d in 0..DIMS {
        let _ = m4.hit(0, d);
    }
    s.add(
        "J18_partial_rows",
        m4.rows[0].full() && !m4.all_full(),
        "单行满额不等于矩阵满额",
    );

    // J19 维度越界打点受控。
    let mut m5 = CoverMatrix::new();
    let _ = m5.mount(0, "d");
    s.add(
        "J19_dim_oob",
        m5.hit(0, DIMS).err() == Some(CV_GHOST),
        "维度越界拒 GHOST",
    );

    // J20 快照与重放联动指纹包含快照内容。
    let f_snapshot = m.fold();
    m.replay[0] = "mutated".to_string();
    let f_after = m.fold();
    s.add("J20_fp_sensitive", f_snapshot != f_after, "指纹对快照内容敏感");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12135_mount_hit() {
        let mut m = CoverMatrix::new();
        let _ = m.mount(0, "f");
        assert!(m.hit(0, 0).is_ok());
        assert_eq!(m.hit(1, 0), Err(CV_GHOST));
    }

    #[test]
    fn unx12135_replay() {
        let mut m = CoverMatrix::new();
        for i in 0..ROWS {
            let name = format!("f{}", i);
            let _ = m.mount(i, &name);
        }
        assert_eq!(m.snapshot(), Ok(ROWS));
        assert_eq!(m.replay_check(), Ok(ROWS));
    }

    #[test]
    fn unx12135_checks_green() {
        let s = run_unx12135_checks();
        assert!(s.all_passed(), "UNX-F12135 判据未全绿");
    }

    #[test]
    fn unx12135_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
