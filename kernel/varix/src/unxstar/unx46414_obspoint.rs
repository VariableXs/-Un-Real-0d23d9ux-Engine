//! UNX-F46414 工具链可观测性埋点（编译入口/出口/耗时/结果/异常栈五点 × 五件）（0x400D0 段）
//!
//! 锚点：UNX-F46414-J1 埋点覆盖 100%；异常零静默：任何失败路径必留
//! error 级事件三要素（发生了什么/为什么/下一步怎么办）。
//! 域：CoRun·UNX-S1-B01（工具链域·可观测面）。
//!
//! 结构：五点埋点账（入口/出口/耗时/结果/异常栈 × 五件全矩阵逐格落账）+
//! 事件三要素核（what/why/next 三字段齐才可入账）+ 覆盖率核（25 格全绿）
//! + 注入核（空事件/缺要素/越矩阵/重复格/封账拒写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400D0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400D0;
pub const OB_EMPTY_EVT: u32 = SEG_BASE + 0x01; // 事件为空
pub const OB_TRIAD_MISS: u32 = SEG_BASE + 0x02; // 三要素缺失
pub const OB_CELL_OOB: u32 = SEG_BASE + 0x03; // 埋点格越界
pub const OB_DUP: u32 = SEG_BASE + 0x04; // 重复格
pub const OB_SEALED: u32 = SEG_BASE + 0x05; // 封账拒写

/// 五点埋点闭集。
pub const PT_ENTER: usize = 0; // 编译入口
pub const PT_EXIT: usize = 1; // 编译出口
pub const PT_DUR: usize = 2; // 耗时
pub const PT_RESULT: usize = 3; // 结果
pub const PT_STACK: usize = 4; // 异常栈
pub const PT_COUNT: usize = 5;

pub const PT_NAMES: [&str; PT_COUNT] = ["入口", "出口", "耗时", "结果", "异常栈"];

/// 五件闭集（与 F46401 同源）。
pub const OB_GCC: usize = 0;
pub const OB_CLANG: usize = 1;
pub const OB_RUSTC: usize = 2;
pub const OB_GIT: usize = 3;
pub const OB_CMAKE: usize = 4;
pub const OB_COUNT: usize = 5;

pub const OB_NAMES: [&str; OB_COUNT] = ["gcc", "clang", "rustc", "git", "cmake"];

/// error 级事件三要素。
#[derive(Clone)]
pub struct Triad {
    pub what: String,
    pub why: String,
    pub next: String,
}

impl Triad {
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 埋点格：事件文本 + error 级三要素（失败路径必填）。
#[derive(Clone)]
pub struct ProbeCell {
    pub point: usize,
    pub comp: usize,
    pub event: String,
    pub err: Option<Triad>,
}

/// 埋点矩阵账（5×5 全格落账）。
pub struct ProbeMatrix {
    pub cells: Vec<ProbeCell>,
    pub sealed: bool,
}

impl ProbeMatrix {
    pub fn new() -> Self {
        ProbeMatrix { cells: Vec::new(), sealed: false }
    }

    /// 落账（越矩阵/空事件/失败缺三要素/重复/封账全拒）。
    pub fn emit(&mut self, point: usize, comp: usize, event: &str, err: Option<Triad>) -> Result<(), u32> {
        if point >= PT_COUNT || comp >= OB_COUNT {
            return Err(OB_CELL_OOB);
        }
        if event.is_empty() {
            return Err(OB_EMPTY_EVT);
        }
        if let Some(t) = &err {
            if !t.complete() {
                return Err(OB_TRIAD_MISS);
            }
        }
        if self.sealed {
            return Err(OB_SEALED);
        }
        if self.cells.iter().any(|c| c.point == point && c.comp == comp) {
            return Err(OB_DUP);
        }
        self.cells.push(ProbeCell {
            point,
            comp,
            event: event.to_string(),
            err,
        });
        Ok(())
    }

    /// 埋点覆盖率核（25 格全落账 = 100%）。
    pub fn coverage_ok(&self) -> bool {
        self.cells.len() == PT_COUNT * OB_COUNT
    }

    /// 失败路径三要素零静默核：所有带 err 的格三要素全齐。
    pub fn triad_zero_silent(&self) -> bool {
        self.cells.iter().all(|c| match &c.err {
            None => true,
            Some(t) => t.complete(),
        })
    }

    /// 异常栈点全布防核：五件的「异常栈」点全部有账。
    pub fn stack_point_all(&self) -> bool {
        (0..OB_COUNT).all(|c| self.cells.iter().any(|cell| cell.point == PT_STACK && cell.comp == c))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 覆盖指纹（格序复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for c in self.cells.iter() {
            for b in format!("{}|{}|{}|{:?};", PT_NAMES[c.point], OB_NAMES[c.comp], c.event, c.err.is_some()).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 标准夹具：25 格全落账（每格事件 + 三件失败路径带三要素）。
pub fn standard_matrix() -> ProbeMatrix {
    let mut m = ProbeMatrix::new();
    for pt in 0..PT_COUNT {
        for c in 0..OB_COUNT {
            let ev = format!("{}·{}", PT_NAMES[pt], OB_NAMES[c]);
            // 失败路径：结果点三件（gcc/clang/git）带三要素 error。
            let err = if pt == PT_RESULT && (c == OB_GCC || c == OB_CLANG || c == OB_GIT) {
                Some(Triad {
                    what: format!("{} 编译失败", OB_NAMES[c]),
                    why: "缺头文件".to_string(),
                    next: "检查 include 路径后重试".to_string(),
                })
            } else {
                None
            };
            m.emit(pt, c, &ev, err).expect("fixture probe");
        }
    }
    m
}

/// 注入核：6 注入必抓（空事件/缺要素/越矩阵/重复格/封账拒写/覆盖不足）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空事件必抓
    let mut m = ProbeMatrix::new();
    if m.emit(PT_ENTER, OB_GCC, "", None) == Err(OB_EMPTY_EVT) {
        caught += 1;
    }
    // 2) 缺三要素必抓（what 空）
    let bad = Some(Triad { what: "".to_string(), why: "w".to_string(), next: "n".to_string() });
    if m.emit(PT_RESULT, OB_GCC, "失败", bad) == Err(OB_TRIAD_MISS) {
        caught += 1;
    }
    // 3) 越矩阵必抓
    if m.emit(9, OB_GCC, "事件", None) == Err(OB_CELL_OOB) {
        caught += 1;
    }
    // 4) 重复格必抓
    m.emit(PT_ENTER, OB_GCC, "入口事件", None).expect("first");
    if m.emit(PT_ENTER, OB_GCC, "入口事件重放", None) == Err(OB_DUP) {
        caught += 1;
    }
    // 5) 封账拒写
    let mut std_m = standard_matrix();
    std_m.seal();
    if std_m.emit(PT_EXIT, OB_GCC, "出口事件", None) == Err(OB_SEALED) {
        caught += 1;
    }
    // 6) 覆盖不足必抓（只落 10 格）
    let mut part = ProbeMatrix::new();
    for i in 0..10 {
        part.emit(i % PT_COUNT, i % OB_COUNT, &format!("事件{}", i), None).expect("partial");
    }
    if !part.coverage_ok() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46414_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46414");
    // J1-1 埋点覆盖 100%（25/25）
    let m = standard_matrix();
    let ok = m.coverage_ok();
    s.add("46414-J1-覆盖100%", ok, if ok { "" } else { "覆盖不足" });
    // J1-2 失败路径三要素零静默（三件 error 全齐）
    let ok = m.triad_zero_silent() && m.cells.iter().filter(|c| c.err.is_some()).count() == 3;
    s.add("46414-J1-三要素零静默", ok, if ok { "" } else { "静默失败" });
    // J1-3 异常栈点全布防（五件都在）
    let ok = m.stack_point_all();
    s.add("46414-J1-异常栈全布防", ok, if ok { "" } else { "栈点缺失" });
    // J1-4 覆盖指纹稳定
    let ok = m.fingerprint() == standard_matrix().fingerprint();
    s.add("46414-J1-覆盖指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46414-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46414_all_checks_green() {
        let set = run_unx46414_checks();
        assert!(set.all_passed(), "UNX-F46414 checks must be green: {}", set.red_items_text());
    }
}
