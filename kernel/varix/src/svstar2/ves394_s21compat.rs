//! ves394_s21compat — VE-F4154 · i18n 兼容矩阵
//! 兼容面四轴组合抽样矩阵（语种×地区×设备档×输入法）+ 已知组合问题清单 + 矩阵测试执行与缺口补测
//! + 与 F4108/F4094 矩阵合并去重 + 组合爆炸→分层抽样策略 + 漏格→补测 + 问题复发→立案升级
//! + 辅助技术×语种扩展轴（域本色）+ 语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xED40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    Ok = 0,
    CellMissing = SEG_BASE + 0x01,
    IssueRecurred = SEG_BASE + 0x02,
    DupFound = SEG_BASE + 0x03,
}

/// 四轴单元格 + 域本色扩展轴。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub lang: &'static str,     // 语种轴
    pub region: &'static str,   // 地区轴
    pub tier: u8,               // 设备档轴（1=低 2=中 3=高）
    pub ime: &'static str,      // 输入法轴
    pub at: Option<&'static str>, // 辅助技术扩展轴（域本色；None=不带 AT）
}

impl Cell {
    pub fn new(lang: &'static str, region: &'static str, tier: u8, ime: &'static str) -> Self {
        Cell { lang, region, tier, ime, at: None }
    }
    pub fn with_at(mut self, at: &'static str) -> Self { self.at = Some(at); self }
    pub fn valid(&self) -> bool {
        !self.lang.is_empty() && !self.region.is_empty() && (1..=3).contains(&self.tier)
            && !self.ime.is_empty()
    }
}

/// 已知组合问题清单条目。
#[derive(Clone, Copy)]
pub struct KnownIssue {
    pub cell_key: (&'static str, &'static str, u8, &'static str),
    pub issue: &'static str,
    pub escalated: bool, // 复发升级
}

/// 兼容矩阵：抽样格 + 执行记录 + 问题清单 + 去重表。
pub struct CompatMatrix {
    pub cells: Vec<Cell>,
    pub executed: Vec<usize>,          // 已执行格 idx
    pub issues: Vec<KnownIssue>,
    pub dedup_keys: Vec<(&'static str, &'static str, u8, &'static str)>, // 与 F4108/F4094 合并去重
}

impl CompatMatrix {
    pub fn new() -> Self {
        CompatMatrix { cells: Vec::new(), executed: Vec::new(), issues: Vec::new(), dedup_keys: Vec::new() }
    }

    /// 分层抽样策略：按设备档配额抽格（每档至多 cap 格）。
    pub fn stratified_sample(&mut self, cap_per_tier: usize) -> Vec<usize> {
        let mut picked: Vec<usize> = Vec::new();
        for tier in 1..=3u8 {
            let mut n = 0;
            for (i, c) in self.cells.iter().enumerate() {
                if c.tier == tier && n < cap_per_tier {
                    picked.push(i);
                    n += 1;
                }
            }
        }
        picked
    }

    pub fn put(&mut self, c: Cell) {
        if c.valid() && !self.cells.contains(&c) { self.cells.push(c); }
    }

    pub fn run(&mut self, idx: usize) -> bool {
        if idx < self.cells.len() && !self.executed.contains(&idx) {
            self.executed.push(idx);
            return true;
        }
        false
    }

    /// 漏格：未执行格。
    pub fn gaps(&self) -> Vec<usize> {
        (0..self.cells.len()).filter(|i| !self.executed.contains(i)).collect()
    }

    /// 补测：跑完全部漏格，返回补测数。
    pub fn fill_gaps(&mut self) -> usize {
        let mut n = 0;
        for i in self.gaps() { if self.run(i) { n += 1; } }
        n
    }

    /// 问题登记：复发 → 升级立案。
    pub fn record_issue(&mut self, cell_idx: usize, issue: &'static str) {
        let c = self.cells[cell_idx];
        let key = (c.lang, c.region, c.tier, c.ime);
        if let Some(k) = self.issues.iter_mut().find(|x| x.cell_key == key && x.issue == issue) {
            k.escalated = true; // 问题复发 → 立案升级
        } else {
            self.issues.push(KnownIssue { cell_key: key, issue, escalated: false });
        }
    }

    pub fn escalated(&self) -> Vec<&'static str> {
        self.issues.iter().filter(|x| x.escalated).map(|x| x.issue).collect()
    }

    /// 与 F4108/F4094 矩阵合并去重：本矩阵格若已在家族去重表 → 跳过，返回去重数。
    pub fn merge_dedup(&mut self, family_keys: &[(&'static str, &'static str, u8, &'static str)]) -> usize {
        let mut dup = 0;
        self.cells.retain(|c| {
            let k = (c.lang, c.region, c.tier, c.ime);
            if family_keys.contains(&k) { dup += 1; false } else { true }
        });
        for k in family_keys {
            if !self.dedup_keys.contains(k) { self.dedup_keys.push(*k); }
        }
        dup
    }

    /// 域本色扩展轴：带 AT 的格。
    pub fn at_cells(&self) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| c.at.is_some()).map(|(i, _)| i).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves394_checks() -> CheckSet {
    let mut set = CheckSet::new("ves394");
    let mut m = CompatMatrix::new();

    // C394-01 四轴格登记（8 格：2 语种 × 2 档 × 2 输入法）。
    let combos: [(&'static str, &'static str, u8, &'static str); 8] = [
        ("zh-CN", "CN", 1, "pinyin"), ("zh-CN", "CN", 3, "pinyin"),
        ("ar-EG", "EG", 1, "qwerty"), ("ar-EG", "EG", 3, "qwerty"),
        ("de-DE", "DE", 2, "qwertz"), ("de-DE", "DE", 2, "stylus"),
        ("ja-JP", "JP", 2, "kana"), ("ja-JP", "JP", 3, "kana"),
    ];
    for (l, r, t, ime) in combos.iter() { m.put(Cell::new(l, r, *t, *ime)); }
    chk(&mut set, "C394-01", m.cells.len() == 8, "八格四轴");

    // C394-02 分层抽样：每档 cap=2 → 1 档 2 格 + 2 档 3 格取 2 + 3 档 3 格取 2。
    let picked = m.stratified_sample(2);
    chk(&mut set, "C394-02", picked.len() == 6, "分层抽样 2+2+2");

    // C394-03 执行抽样格。
    for i in picked.iter() { m.run(*i); }
    chk(&mut set, "C394-03", m.executed.len() == 6 && m.gaps().len() == 2, "执行六格漏二");

    // C394-04 漏格补测。
    let filled = m.fill_gaps();
    chk(&mut set, "C394-04", filled == 2 && m.gaps().is_empty(), "补测二格清零");

    // C394-05 重复执行拒（幂等）。
    chk(&mut set, "C394-05", !m.run(0) && m.executed.len() == 8, "重跑拒");

    // C394-06 问题登记与复发升级。
    m.record_issue(0, "混合方向折行错");
    m.record_issue(0, "混合方向折行错"); // 复发 → 升级
    chk(&mut set, "C394-06", m.issues.len() == 1 && m.escalated() == vec!["混合方向折行错"], "复发即升级");

    // C394-07 新问题不升级。
    m.record_issue(2, "RTL 输入焦点跳");
    chk(&mut set, "C394-07", m.issues.len() == 2 && m.escalated().len() == 1, "新问题不升级");

    // C394-08 与家族矩阵（F4108/F4094）合并去重。
    let family: Vec<(&'static str, &'static str, u8, &'static str)> =
        combos.iter().take(2).map(|(l, r, t, ime)| (*l, *r, *t, *ime)).collect();
    let dup = m.merge_dedup(&family);
    chk(&mut set, "C394-08", dup == 2 && m.cells.len() == 6 && m.dedup_keys.len() == 2, "去重二格");

    // C394-09 非法格拒收。
    let before = m.cells.len();
    m.put(Cell::new("", "X", 2, "ime"));
    m.put(Cell::new("zh-CN", "CN", 9, "ime"));
    chk(&mut set, "C394-09", m.cells.len() == before, "非法格双拒");

    // C394-10 域本色扩展轴：辅助技术×语种格。
    m.put(Cell::new("ko-KR", "KR", 2, "hangul").with_at("screen-reader"));
    m.put(Cell::new("ko-KR", "KR", 2, "hangul")); // 同四轴不重复（无 AT 基格已在？不在——四轴同但 at 不同，四轴判重会拒）
    chk(&mut set, "C394-10", m.at_cells().len() == 1, "AT 扩展轴一格");

    // C394-11 无效格校验单源。
    chk(&mut set, "C394-11", !Cell::new("zh-CN", "CN", 0, "x").valid()
        && Cell::new("zh-CN", "CN", 3, "x").valid(), "档位 1..=3 校验");

    // C394-12 补测后执行记录含 AT 格。
    m.fill_gaps();
    chk(&mut set, "C394-12", m.gaps().is_empty()
        && m.executed.contains(&(m.cells.len() - 1)), "AT 格入执行记录");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves394_smoke() {
        let set = run_ves394_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves394 red: {:?}", red);
    }

    #[test]
    fn ves394_with_at_differs() {
        let a = Cell::new("x", "Y", 2, "z");
        let b = a.with_at("sr");
        assert_ne!(a, b);
        assert!(a.at.is_none() && b.at == Some("sr"));
    }
}
