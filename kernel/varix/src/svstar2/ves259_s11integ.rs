//! VE-F4019 · T01 联调
//!
//! 联调（i18n 组合矩阵）：十映射域 × 五代表语言（en/zh/ar/de/th）× 三
//! 方向 = 抽 54 格（覆盖底线）；真实剧本（20 分钟 i18n 剧本：切语言→
//! 方向切换→格式验证→输入法混流——重演确定性 + 信号采集）；故障注入
//! （非法 Locale / 覆写攻击 / 硬编码注入三故障——红线实测场）；三分流。
//! 数据结构：54 格；剧本执行器；故障注入集；处置清单。
//! 错误路径：覆写攻击漏→P0；覆盖不达标→补跑；重演分歧→立案；处置
//! 无责→兜底。
//! 性能：单组合 O(分钟)；全集夜间；剧本 O(20 分钟)；归因 O(维数)。
//! 跨批对接：F3801-F3819 全对端；F3820 输入；N02/S03 对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x67C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x67C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T25JCode(pub u16);

impl T25JCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const OVERRIDE_LEAK: Self = Self(SEG_BASE + 1);
    pub const COVER_SHORT: Self = Self(SEG_BASE + 2);
    pub const REPLAY_SPLIT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::OVERRIDE_LEAK => "覆写攻击漏→P0",
            Self::COVER_SHORT => "覆盖不达标→补跑",
            Self::REPLAY_SPLIT => "重演分歧→立案",
            _ => "未知",
        }
    }
}

// ── 54 格组合矩阵 ───────────────────────────────────────────────────

pub const LANGS5: [&str; 5] = ["en", "zh", "ar", "de", "th"];
pub const DIRS3: [&str; 3] = ["LTR", "RTL", "AUTO"];

/// 组合格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    /// 映射槽 0..10。
    pub mapping: u8,
    /// 语言槽 0..5。
    pub lang: u8,
    /// 方向槽 0..3。
    pub dir: u8,
}

/// 54 格抽样：十映射域 → 每映射按 (lang+dir) 轮转抽格。
/// 抽样法：mapping i 抽 (i*7+3) % 5 语言 + (i*3+1) % 3 方向，再加
/// 固定边界格（ar×RTL、th×LTR）补齐至 54。
pub const SAMPLE_TOTAL: u8 = 54;

/// 组合矩阵册。
#[derive(Debug, Clone)]
pub struct ComboMatrix {
    pub cells: Vec<(Combo, bool)>,
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ComboMatrix {
    /// 构建 54 格。
    pub fn build() -> Self {
        let mut cells = Vec::new();
        // 主抽样：10 映射 × 5 语言 = 50 格（方向轮转）。
        for m in 0..10u8 {
            for l in 0..5u8 {
                let d = ((m * 3 + l) % 3) as u8;
                cells.push((Combo { mapping: m, lang: l, dir: d }, false));
            }
        }
        // 边界补格 4 格。
        cells.push((Combo { mapping: 1, lang: 2, dir: 1 }, false)); // 方向×ar×RTL
        cells.push((Combo { mapping: 2, lang: 4, dir: 0 }, false)); // 排版×th×LTR
        cells.push((Combo { mapping: 4, lang: 2, dir: 1 }, false)); // 格式×ar×RTL
        cells.push((Combo { mapping: 9, lang: 0, dir: 2 }, false)); // 调试×en×AUTO
        Self { cells }
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 跑格（结果登记）。
    pub fn run(&mut self, idx: usize, ok: bool) -> bool {
        if idx >= self.cells.len() {
            return false;
        }
        self.cells[idx].1 = ok;
        true
    }

    /// 覆盖达标：全格已跑。
    pub fn covered(&self) -> bool {
        self.cells.iter().all(|(_, run)| *run)
    }

    /// 红格清单。
    pub fn red_cells(&self) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, (_, ok))| !ok).map(|(i, _)| i).collect()
    }
}

// ── 剧本执行器（重演确定性）─────────────────────────────────────────

/// 四幕：切语言 → 方向切换 → 格式验证 → 输入法混流。
pub const ACTS: [&str; 4] = ["切语言", "方向切换", "格式验证", "输入法混流"];

/// 剧本信号。
#[derive(Debug, Clone)]
pub struct ScriptSignal {
    /// 信号指纹（FNV）。
    pub fp: u64,
    /// 通过。
    pub ok: bool,
}

/// 剧本执行器。
#[derive(Debug, Clone)]
pub struct ScriptRunner {
    pub signals: Vec<ScriptSignal>,
}

impl ScriptRunner {
    pub fn new() -> Self {
        Self { signals: Vec::new() }
    }

    /// 执行剧本（确定性：同输入同指纹）。
    pub fn run(&mut self, seed: &str) -> ScriptSignal {
        let mut src = String::from(seed);
        for a in ACTS.iter() {
            src.push('|');
            src.push_str(a);
        }
        ScriptSignal { fp: fnv1a(src.as_bytes()), ok: true }
    }

    /// 重演比对：两次指纹一致。
    pub fn replay_ok(first: u64, second: u64) -> T25JCode {
        if first == second {
            T25JCode::OK
        } else {
            T25JCode::REPLAY_SPLIT
        }
    }
}

// ── 故障注入三故障 ──────────────────────────────────────────────────

/// 三故障。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// 非法 Locale。
    BadLocale,
    /// 覆写攻击。
    Override,
    /// 硬编码注入。
    HardCode,
}

pub const FAULT_KINDS: [FaultKind; 3] = [FaultKind::BadLocale, FaultKind::Override, FaultKind::HardCode];

/// 故障注入场：注入样本 → 防线检出。
#[derive(Debug, Clone)]
pub struct FaultField {
    pub detected: [bool; 3],
}

impl FaultField {
    pub fn new() -> Self {
        Self { detected: [false; 3] }
    }

    /// 注入一故障（防线检出登记）。
    pub fn inject(&mut self, kind: FaultKind, caught: bool) -> T25JCode {
        self.detected[kind as usize] = caught;
        if kind == FaultKind::Override && !caught {
            return T25JCode::OVERRIDE_LEAK;
        }
        T25JCode::OK
    }

    /// 三故障全检出。
    pub fn all_caught(&self) -> bool {
        self.detected.iter().all(|&d| d)
    }
}

// ── 三分流处置 ──────────────────────────────────────────────────────

/// 三分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// P0 即断。
    Halt,
    /// 立案。
    File,
    /// 兜底。
    Fallback,
}

/// 处置清单。
#[derive(Debug, Clone)]
pub struct RouteBook {
    pub routes: Vec<(usize, Route)>,
}

impl RouteBook {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// 红格归因分流。
    pub fn route(&mut self, cell: usize, r: Route) {
        self.routes.push((cell, r));
    }

    /// 无责格（兜底）清点。
    pub fn fallbacks(&self) -> Vec<usize> {
        self.routes.iter().filter(|(_, r)| *r == Route::Fallback).map(|(c, _)| *c).collect()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves259_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves259_s11integ");

    // C4019-01 诊断码段不越窗。
    chk(&mut set, "C4019-01",
        T25JCode::REPLAY_SPLIT.0 < SEG_BASE + 16 && SEG_BASE == 0x67C0, "err");

    // C4019-02 五语言三方向齐备。
    chk(&mut set, "C4019-02",
        LANGS5.len() == 5 && DIRS3.len() == 3
            && LANGS5[2] == "ar" && DIRS3[1] == "RTL", "err");

    // C4019-03 54 格构建（50 主抽样 + 4 边界）。
    let mut m = ComboMatrix::build();
    chk(&mut set, "C4019-03", m.len() == 54, "err");

    // C4019-04 覆盖不达标检出（未跑）。
    chk(&mut set, "C4019-04", !m.covered(), "err");

    // C4019-05 全跑后覆盖达标。
    for i in 0..m.len() {
        m.run(i, i % 9 != 4);
    }
    chk(&mut set, "C4019-05", m.covered(), "err");

    // C4019-06 红格清单（i%9==4 → 4,13,22,31,40,49）。
    let reds = m.red_cells();
    chk(&mut set, "C4019-06", reds == vec![4usize, 13, 22, 31, 40, 49], "err");

    // C4019-07 越界跑格拒绝。
    chk(&mut set, "C4019-07", !m.run(99, true), "err");

    // C4019-08 剧本四幕齐备。
    chk(&mut set, "C4019-08", ACTS.len() == 4 && ACTS[0] == "切语言" && ACTS[3] == "输入法混流", "err");

    // C4019-09 重演确定性：同种子同指纹。
    let mut r = ScriptRunner::new();
    let s1 = r.run("seed-x");
    let s2 = r.run("seed-x");
    chk(&mut set, "C4019-09", s1.fp == s2.fp, "err");

    // C4019-10 重演比对：一致 OK。
    chk(&mut set, "C4019-10", ScriptRunner::replay_ok(s1.fp, s2.fp) == T25JCode::OK, "err");

    // C4019-11 重演分歧 → 立案。
    let s3 = r.run("seed-y");
    chk(&mut set, "C4019-11", ScriptRunner::replay_ok(s1.fp, s3.fp) == T25JCode::REPLAY_SPLIT, "err");

    // C4019-12 三故障齐备。
    chk(&mut set, "C4019-12",
        FAULT_KINDS.len() == 3 && FAULT_KINDS[1] == FaultKind::Override, "err");

    // C4019-13 三故障全检出。
    let mut f = FaultField::new();
    f.inject(FaultKind::BadLocale, true);
    f.inject(FaultKind::Override, true);
    f.inject(FaultKind::HardCode, true);
    chk(&mut set, "C4019-13", f.all_caught(), "err");

    // C4019-14 覆写攻击漏 → P0。
    let mut f2 = FaultField::new();
    f2.inject(FaultKind::BadLocale, true);
    chk(&mut set, "C4019-14",
        f2.inject(FaultKind::Override, false) == T25JCode::OVERRIDE_LEAK, "err");

    // C4019-15 故障枚举双射。
    chk(&mut set, "C4019-15",
        FaultKind::BadLocale as usize != FaultKind::Override as usize
            && FaultKind::Override as usize != FaultKind::HardCode as usize, "err");

    // C4019-16 三分流处置。
    let mut rb = RouteBook::new();
    rb.route(4, Route::Halt);
    rb.route(13, Route::File);
    rb.route(22, Route::Fallback);
    chk(&mut set, "C4019-16",
        rb.fallbacks() == vec![22usize] && rb.routes.len() == 3, "err");

    // C4019-17 三分流枚举互异。
    chk(&mut set, "C4019-17",
        Route::Halt != Route::File && Route::File != Route::Fallback, "err");

    // C4019-18 边界格在册（ar×RTL 方向映射格）。
    let has_boundary = m.cells.iter().any(|(c, _)| c.mapping == 1 && c.lang == 2 && c.dir == 1);
    chk(&mut set, "C4019-18", has_boundary, "err");

    // C4019-19 组格语言/方向槽域钳制。
    let valid = m.cells.iter().all(|(c, _)| c.mapping < 10 && c.lang < 5 && c.dir < 3);
    chk(&mut set, "C4019-19", valid, "err");

    // C4019-20 错误码互异。
    let codes = [T25JCode::OK.0, T25JCode::OVERRIDE_LEAK.0, T25JCode::COVER_SHORT.0, T25JCode::REPLAY_SPLIT.0];
    chk(&mut set, "C4019-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_54_cells() {
        let m = ComboMatrix::build();
        assert_eq!(m.len(), 54);
    }

    #[test]
    fn replay_determinism() {
        let mut r = ScriptRunner::new();
        let a = r.run("s");
        let b = r.run("s");
        assert_eq!(a.fp, b.fp);
    }

    #[test]
    fn override_leak_p0() {
        let mut f = FaultField::new();
        assert_eq!(f.inject(FaultKind::Override, false), T25JCode::OVERRIDE_LEAK);
    }
}
