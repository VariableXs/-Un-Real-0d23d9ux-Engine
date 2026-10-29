//! UNX-F56129 分辨率/刷新率矩阵冻结件（0x61000 段）
//!
//! 锚点：UNX-F56129-J1 矩阵全量（w×h@hz 格逐格落账、格三元组唯一）；
//! J2 冻结语义（分辨率/刷新率闭带、去重统计闭式、格校验命中/拒绝可
//! 复算）；J3 矩阵审计链（冻结门、键序指纹、统计对账、渲染完整）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·分辨率矩阵面）。
//!
//! 结构：矩阵登记（格三元组、唯一格）+ 冻结门（冻结后拒增、重复冻
//! 结拒绝）+ 统计闭式（分辨率/刷新率去重数复算对账）+ 键序指纹链 +
//! 注入核（分辨率越界/刷新率越界/重复格/冻结拒增/校验缺格/空矩阵冻
//! 结/统计漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61000 独占 0x20）。
pub const SEG_BASE: u32 = 0x61000;
pub const RM_DIM_RANGE: u32 = SEG_BASE + 0x01; // 分辨率越界
pub const RM_HZ_RANGE: u32 = SEG_BASE + 0x02; // 刷新率越界
pub const RM_DUP_CELL: u32 = SEG_BASE + 0x03; // 重复格
pub const RM_FROZEN: u32 = SEG_BASE + 0x04; // 矩阵已冻结拒增/重复冻结
pub const RM_CELL_MISSING: u32 = SEG_BASE + 0x05; // 校验格不在矩阵
pub const RM_EMPTY_FREEZE: u32 = SEG_BASE + 0x06; // 空矩阵冻结
pub const RM_STAT_MISS: u32 = SEG_BASE + 0x07; // 矩阵统计漂移

/// 分辨率合法带（宽/高，像素）。
pub const DIM_MIN: u32 = 320;
pub const DIM_MAX: u32 = 7_680;

/// 刷新率合法带（Hz）。
pub const HZ_MIN: u32 = 30;
pub const HZ_MAX: u32 = 360;

pub fn dim_ok(v: u32) -> bool {
    (DIM_MIN..=DIM_MAX).contains(&v)
}

pub fn hz_ok(v: u32) -> bool {
    (HZ_MIN..=HZ_MAX).contains(&v)
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

/// 分辨率/刷新率矩阵。
pub struct ResMatrix {
    cap: usize,
    cells: Vec<(u32, u32, u32)>,
    distinct_res: u64,
    distinct_hz: u64,
    chain: u32,
    frozen: bool,
}

impl ResMatrix {
    pub fn new(cap: usize) -> Self {
        ResMatrix {
            cap,
            cells: Vec::new(),
            distinct_res: 0,
            distinct_hz: 0,
            chain: CHAIN_SEED,
            frozen: false,
        }
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    pub fn distinct_res(&self) -> u64 {
        self.distinct_res
    }

    pub fn distinct_hz(&self) -> u64 {
        self.distinct_hz
    }

    /// 格切片只读访问。
    pub fn cells(&self) -> &[(u32, u32, u32)] {
        &self.cells
    }

    /// 加格：维度带 → 刷新带 → 唯一格 → 冻结门。
    pub fn add_cell(&mut self, w: u32, h: u32, hz: u32) -> Result<usize, u32> {
        if self.frozen || self.cells.len() >= self.cap {
            return Err(RM_FROZEN);
        }
        if !dim_ok(w) || !dim_ok(h) {
            return Err(RM_DIM_RANGE);
        }
        if !hz_ok(hz) {
            return Err(RM_HZ_RANGE);
        }
        for c in &self.cells {
            if *c == (w, h, hz) {
                return Err(RM_DUP_CELL);
            }
        }
        self.cells.push((w, h, hz));
        Ok(self.cells.len() - 1)
    }

    /// 冻结：空矩阵拒绝；冻结时定去重统计与键序指纹。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        if self.cells.is_empty() {
            return Err(RM_EMPTY_FREEZE);
        }
        self.distinct_res = recount_res(&self.cells);
        self.distinct_hz = recount_hz(&self.cells);
        self.chain = chain_keyed(&self.cells);
        self.frozen = true;
        Ok(())
    }

    /// 格校验：命中放行、缺格拒绝。
    pub fn validate(&self, w: u32, h: u32, hz: u32) -> Result<(), u32> {
        for c in &self.cells {
            if *c == (w, h, hz) {
                return Ok(());
            }
        }
        Err(RM_CELL_MISSING)
    }

    /// 统计对账：去重数与指纹复算双不齐即漂移。
    pub fn verify_stats(&self) -> Result<(), u32> {
        if self.cells.is_empty() {
            return Err(RM_STAT_MISS);
        }
        if recount_res(&self.cells) != self.distinct_res
            || recount_hz(&self.cells) != self.distinct_hz
        {
            return Err(RM_STAT_MISS);
        }
        if self.chain != chain_keyed(&self.cells) {
            return Err(RM_STAT_MISS);
        }
        Ok(())
    }

    /// 渲染：每行 w×h@hz。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (w, h, hz) in &self.cells {
            out.push(format!("{}x{}@{}", w, h, hz));
        }
        out
    }

    /// 测试注入口：仅注入核使用。
    fn break_stats(&mut self) {
        self.distinct_res += 1;
    }
}

/// 分辨率去重复算（独立 (w,h) 对数）。
pub fn recount_res(cells: &[(u32, u32, u32)]) -> u64 {
    let mut n = 0u64;
    for (i, c) in cells.iter().enumerate() {
        let mut first = true;
        for p in &cells[..i] {
            if (p.0, p.1) == (c.0, c.1) {
                first = false;
            }
        }
        if first {
            n += 1;
        }
    }
    n
}

/// 刷新率去重复算。
pub fn recount_hz(cells: &[(u32, u32, u32)]) -> u64 {
    let mut n = 0u64;
    for (i, c) in cells.iter().enumerate() {
        let mut first = true;
        for p in &cells[..i] {
            if p.2 == c.2 {
                first = false;
            }
        }
        if first {
            n += 1;
        }
    }
    n
}

/// 键序指纹链：按 (w,h,hz) 升序（插入排序、元组换位）重算。
pub fn chain_keyed(cells: &[(u32, u32, u32)]) -> u32 {
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..cells.len() {
        idx.push(i);
    }
    for i in 1..idx.len() {
        let mut j = i;
        while j > 0 && cells[idx[j]] < cells[idx[j - 1]] {
            let tmp = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = tmp;
            j -= 1;
        }
    }
    let mut h = CHAIN_SEED;
    for &i in &idx {
        let c = cells[i];
        h = fold_u32(h, c.0);
        h = fold_u32(h, c.1);
        h = fold_u32(h, c.2);
    }
    h
}

/// 标准矩阵：四格（三种分辨率、两档刷新率）后冻结。
fn fixture() -> ResMatrix {
    let mut m = ResMatrix::new(8);
    let _ = m.add_cell(1_920, 1_080, 60);
    let _ = m.add_cell(1_920, 1_080, 120);
    let _ = m.add_cell(2_560, 1_440, 120);
    let _ = m.add_cell(3_840, 2_160, 60);
    let _ = m.freeze();
    m
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 分辨率越界必抓。
    {
        let mut m = ResMatrix::new(8);
        caught += (m.add_cell(0, 1_080, 60) == Err(RM_DIM_RANGE)) as usize;
    }
    // 2 刷新率越界必抓。
    {
        let mut m = ResMatrix::new(8);
        caught += (m.add_cell(1_920, 1_080, 0) == Err(RM_HZ_RANGE)) as usize;
    }
    // 3 重复格必抓。
    {
        let mut m = ResMatrix::new(8);
        let _ = m.add_cell(1_920, 1_080, 60);
        caught += (m.add_cell(1_920, 1_080, 60) == Err(RM_DUP_CELL)) as usize;
    }
    // 4 冻结拒增必抓。
    {
        let mut m = fixture();
        caught += (m.add_cell(1_280, 720, 60) == Err(RM_FROZEN)) as usize;
    }
    // 5 校验缺格必抓。
    {
        let m = fixture();
        caught += (m.validate(1_280, 720, 60) == Err(RM_CELL_MISSING)) as usize;
    }
    // 6 空矩阵冻结必抓。
    {
        let mut m = ResMatrix::new(8);
        caught += (m.freeze() == Err(RM_EMPTY_FREEZE)) as usize;
    }
    // 7 统计漂移必抓。
    {
        let mut m = fixture();
        m.break_stats();
        caught += (m.verify_stats() == Err(RM_STAT_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56129 全量判据（15 项）。
pub fn run_unx56129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56129");
    let m = fixture();

    // J1 矩阵全量。
    let ok = m.len() == 4;
    s.add("56129-J1-矩阵四格", ok, if ok { "" } else { "格数不符" });
    let ok = m.cells().iter().all(|c| dim_ok(c.0) && dim_ok(c.1));
    s.add("56129-J1-分辨率带", ok, if ok { "" } else { "分辨率越带" });
    let ok = m.cells().iter().all(|c| hz_ok(c.2));
    s.add("56129-J1-刷新率带", ok, if ok { "" } else { "刷新率越带" });
    let mut uniq = 4;
    for i in 0..m.cells().len() {
        for j in 0..i {
            if m.cells()[i] == m.cells()[j] {
                uniq = j + 1;
            }
        }
    }
    let ok = uniq == 4;
    s.add("56129-J1-无重复格", ok, if ok { "" } else { "存在重复格" });

    // J2 冻结语义。
    let ok = m.distinct_res() == 3;
    s.add("56129-J2-分辨率去重", ok, if ok { "" } else { "分辨率去重非 3" });
    let ok = m.distinct_hz() == 2;
    s.add("56129-J2-刷新率去重", ok, if ok { "" } else { "刷新率去重非 2" });
    let ok = m.validate(1_920, 1_080, 120).is_ok() && m.validate(3_840, 2_160, 60).is_ok();
    s.add("56129-J2-格校验命中", ok, if ok { "" } else { "在格误拒" });
    let ok = recount_res(m.cells()) == 3;
    s.add("56129-J2-分辨率复算", ok, if ok { "" } else { "分辨率复算漂移" });
    let ok = recount_hz(m.cells()) == 2;
    s.add("56129-J2-刷新率复算", ok, if ok { "" } else { "刷新率复算漂移" });

    // J3 矩阵审计链。
    let ok = m.frozen();
    s.add("56129-J3-冻结门", ok, if ok { "" } else { "未冻结" });
    let ok = m.chain() != CHAIN_SEED && m.chain() == chain_keyed(m.cells());
    s.add("56129-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let ok = m.verify_stats().is_ok();
    s.add("56129-J3-统计对账", ok, if ok { "" } else { "统计对账失败" });
    let lines = m.render();
    let mut ok = lines.len() == 4;
    for l in &lines {
        if l.split('@').count() != 2 || l.split('x').count() != 2 {
            ok = false;
        }
    }
    s.add("56129-J3-渲染完整", ok, if ok { "" } else { "渲染段式漂移" });
    let mut m2 = ResMatrix::new(8);
    let _ = m2.add_cell(1_920, 1_080, 60);
    let _ = m2.freeze();
    let ok = m2.freeze() == Err(RM_FROZEN);
    s.add("56129-J3-重复冻结拒", ok, if ok { "" } else { "重复冻结未拒" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56129-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56129_all_checks_green() {
        let set = run_unx56129_checks();
        assert!(set.all_passed(), "UNX-F56129 红项: {}", set.red_items_text());
    }
}
