//! UNX-F56133 帧率失败痕迹件（0x61080 段）
//!
//! 锚点：UNX-F56133-J1 痕迹全量（阶段 × 成因 × 备注逐条落账、阶段×
//! 成因配对唯一）；J2 痕迹语义（四阶段/四成因双闭集、账目矩阵闭式复
//! 算）；J3 痕迹审计链（封账门、指纹、渲染完整）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·失败痕迹面）。
//!
//! 结构：痕迹登记簿（阶段/成因闭集、配对唯一、备注必填）+ 账目矩阵
//! 闭式（4×4 计数复算）+ 指纹链 + 注入核（阶段越界/成因越界/配对重
//! 复/备注为空/缺痕查询/封账拒录/账目漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61080 独占 0x20）。
pub const SEG_BASE: u32 = 0x61080;
pub const FT_STAGE_OVER: u32 = SEG_BASE + 0x01; // 阶段越界
pub const FT_CAUSE_OVER: u32 = SEG_BASE + 0x02; // 成因越界
pub const FT_DUP_PAIR: u32 = SEG_BASE + 0x03; // 阶段×成因配对重复
pub const FT_NOTE_EMPTY: u32 = SEG_BASE + 0x04; // 痕迹备注为空
pub const FT_NO_TRACE: u32 = SEG_BASE + 0x05; // 缺痕查询
pub const FT_SEALED: u32 = SEG_BASE + 0x06; // 封账拒录
pub const FT_CENSUS_MISS: u32 = SEG_BASE + 0x07; // 痕迹账目漂移

/// 阶段四闭集。
pub const STAGES: [&str; 4] = ["collect", "classify", "stat", "commit"];

/// 成因四闭集。
pub const CAUSES: [&str; 4] = ["frame_drop", "clock_drift", "scene_cut", "timeout"];

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

/// 失败痕迹。
pub struct FailTrace {
    pub no: usize,
    pub stage: usize,
    pub cause: usize,
    pub note: String,
}

/// 帧率失败痕迹登记簿。
pub struct FailTraceBook {
    cap: usize,
    traces: Vec<FailTrace>,
    total: u64,
    chain: u32,
    sealed: bool,
}

impl FailTraceBook {
    pub fn new(cap: usize) -> Self {
        FailTraceBook {
            cap,
            traces: Vec::new(),
            total: 0,
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.traces.len()
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 痕迹切片只读访问。
    pub fn traces(&self) -> &[FailTrace] {
        &self.traces
    }

    /// 落痕迹：阶段闭集 → 成因闭集 → 备注必填 → 配对唯一。
    pub fn record(&mut self, stage: usize, cause: usize, note: &str) -> Result<usize, u32> {
        if self.sealed || self.traces.len() >= self.cap {
            return Err(FT_SEALED);
        }
        if stage >= STAGES.len() {
            return Err(FT_STAGE_OVER);
        }
        if cause >= CAUSES.len() {
            return Err(FT_CAUSE_OVER);
        }
        if note.is_empty() {
            return Err(FT_NOTE_EMPTY);
        }
        for t in &self.traces {
            if t.stage == stage && t.cause == cause {
                return Err(FT_DUP_PAIR);
            }
        }
        let no = self.traces.len();
        self.traces.push(FailTrace {
            no,
            stage,
            cause,
            note: note.to_string(),
        });
        Ok(no)
    }

    /// 按号取痕。
    pub fn get(&self, no: usize) -> Result<(usize, usize), u32> {
        for t in &self.traces {
            if t.no == no {
                return Ok((t.stage, t.cause));
            }
        }
        Err(FT_NO_TRACE)
    }

    /// 账目矩阵复算：4×4 计数。
    pub fn census_matrix(&self) -> [[u64; 4]; 4] {
        let mut m = [[0u64; 4]; 4];
        for t in &self.traces {
            m[t.stage][t.cause] += 1;
        }
        m
    }

    /// 账目对账：总数与矩阵合计一致。
    pub fn verify_census(&self) -> Result<(), u32> {
        let m = self.census_matrix();
        let mut sum = 0u64;
        for row in &m {
            for v in row {
                sum += *v;
            }
        }
        if sum != self.total {
            return Err(FT_CENSUS_MISS);
        }
        if self.chain != chain_of(&self.traces) {
            return Err(FT_CENSUS_MISS);
        }
        Ok(())
    }

    /// 封账：空账拒绝、定总数与指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        if self.traces.is_empty() {
            return Err(FT_NO_TRACE);
        }
        self.total = self.traces.len() as u64;
        self.chain = chain_of(&self.traces);
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 4 段（no|stage|cause|note）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.traces {
            out.push(format!(
                "{}|{}|{}|{}",
                t.no, STAGES[t.stage], CAUSES[t.cause], t.note
            ));
        }
        out
    }

    /// 测试注入口：仅注入核使用。
    fn break_total(&mut self) {
        self.total += 1;
    }
}

/// 指纹链：痕迹落账序 × 备注展开重算。
pub fn chain_of(traces: &[FailTrace]) -> u32 {
    let mut h = CHAIN_SEED;
    for t in traces {
        h = fold_u32(h, t.stage as u32);
        h = fold_u32(h, t.cause as u32);
        h = fold_str(h, &t.note);
    }
    h
}

/// 标准账：四痕迹（四阶段各一、四成因各一）后封账。
fn fixture() -> FailTraceBook {
    let mut b = FailTraceBook::new(8);
    let _ = b.record(0, 0, "采集期掉帧 3 帧");
    let _ = b.record(1, 2, "场景切换打断分类");
    let _ = b.record(2, 1, "时钟漂移致统计偏差");
    let _ = b.record(3, 3, "提交超时 5000ms");
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 阶段越界必抓。
    {
        let mut b = FailTraceBook::new(8);
        caught += (b.record(9, 0, "x") == Err(FT_STAGE_OVER)) as usize;
    }
    // 2 成因越界必抓。
    {
        let mut b = FailTraceBook::new(8);
        caught += (b.record(0, 9, "x") == Err(FT_CAUSE_OVER)) as usize;
    }
    // 3 配对重复必抓。
    {
        let mut b = FailTraceBook::new(8);
        let _ = b.record(0, 0, "a");
        caught += (b.record(0, 0, "b") == Err(FT_DUP_PAIR)) as usize;
    }
    // 4 备注为空必抓。
    {
        let mut b = FailTraceBook::new(8);
        caught += (b.record(0, 0, "") == Err(FT_NOTE_EMPTY)) as usize;
    }
    // 5 缺痕查询必抓。
    {
        let b = fixture();
        caught += (b.get(99) == Err(FT_NO_TRACE)) as usize;
    }
    // 6 封账拒录必抓。
    {
        let mut b = fixture();
        caught += (b.record(0, 1, "x") == Err(FT_SEALED)) as usize;
    }
    // 7 账目漂移必抓。
    {
        let mut b = fixture();
        b.break_total();
        caught += (b.verify_census() == Err(FT_CENSUS_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56133 全量判据（15 项）。
pub fn run_unx56133_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56133");
    let b = fixture();

    // J1 痕迹全量。
    let ok = b.len() == 4;
    s.add("56133-J1-痕迹四条", ok, if ok { "" } else { "痕迹数不符" });
    let ok = STAGES.len() == 4 && CAUSES.len() == 4;
    s.add("56133-J1-双闭集", ok, if ok { "" } else { "闭集漂移" });
    let ok = b.traces().iter().all(|t| !t.note.is_empty());
    s.add("56133-J1-备注必填", ok, if ok { "" } else { "存在空备注" });
    let ok = b.traces().iter().enumerate().all(|(i, t)| t.no == i);
    s.add("56133-J1-痕号连续", ok, if ok { "" } else { "痕号断档" });

    // J2 痕迹语义。
    let m = b.census_matrix();
    let ok = m[0][0] == 1 && m[1][2] == 1 && m[2][1] == 1 && m[3][3] == 1;
    s.add("56133-J2-矩阵四落点", ok, if ok { "" } else { "矩阵落点漂移" });
    let mut max_cell = 0u64;
    for row in &m {
        for v in row {
            if *v > max_cell {
                max_cell = *v;
            }
        }
    }
    let ok = max_cell == 1;
    s.add("56133-J2-配对唯一", ok, if ok { "" } else { "存在重复配对" });
    let sum: u64 = b.census_matrix().iter().flatten().sum();
    let ok = sum == 4 && b.total() == 4;
    s.add("56133-J2-账目合计", ok, if ok { "" } else { "账目合计漂移" });
    let ok = b.get(0) == Ok((0, 0)) && b.get(3) == Ok((3, 3));
    s.add("56133-J2-按号取痕", ok, if ok { "" } else { "取痕语义漂移" });

    // J3 痕迹审计链。
    let ok = b.sealed();
    s.add("56133-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.verify_census().is_ok();
    s.add("56133-J3-账目对账", ok, if ok { "" } else { "账目对账失败" });
    let ok = b.chain() != CHAIN_SEED && b.chain() == chain_of(b.traces());
    s.add("56133-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = b.render();
    let mut ok = lines.len() == 4;
    for l in &lines {
        if l.split('|').count() != 4 {
            ok = false;
        }
    }
    s.add("56133-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });
    let ok = lines[0].contains("collect") && lines[0].contains("frame_drop");
    s.add("56133-J3-名称渲染", ok, if ok { "" } else { "名称渲染漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56133-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56133_all_checks_green() {
        let set = run_unx56133_checks();
        assert!(set.all_passed(), "UNX-F56133 红项: {}", set.red_items_text());
    }
}
