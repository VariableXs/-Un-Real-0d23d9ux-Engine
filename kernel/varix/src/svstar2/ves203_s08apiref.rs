#![allow(dead_code)]
//! VE-F3949 · 无障碍 API 参考
//!
//! 全域无障碍 API 参考归一：S01/S04/S06 各 API + 全域态查询 → 全域 API
//! 册。生成单源全域化（手写检出→迁移）；覆盖硬门全域化（覆盖 < 100% →
//! 阻断）；四段齐备全域化（签名/语义/示例/错误）；版本对照全域化。
//! 错误路径：手写检出→迁移；覆盖<100%→阻断；四段缺→补；漂移→生成器
//! 修复。
//! 性能：归一 O(册)；生成 O(接口)；硬门 O(覆盖率)；对照 O(版本)。
//! 跨批对接：F3350/F3814/F3870/F3909 复用单源全域声明；F3347 SDK 对端；
//! CI 钩子全域。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0DC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0DC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8ICode(pub u16);

impl S8ICode {
    pub const OK: Self = Self(SEG_BASE);
    pub const HANDWRITTEN: Self = Self(SEG_BASE + 1);
    pub const COVER_BLOCK: Self = Self(SEG_BASE + 2);
    pub const SECTION_GAP: Self = Self(SEG_BASE + 3);
    pub const DRIFT_DETECTED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::HANDWRITTEN => "手写检出→迁移",
            Self::COVER_BLOCK => "覆盖<100%→阻断",
            Self::SECTION_GAP => "四段缺→补",
            Self::DRIFT_DETECTED => "漂移→生成器修复",
            _ => "未知",
        }
    }
}

// ── API 条目与四段齐备 ───────────────────────────────────────────────

/// API 参考条目：四段 = 签名/语义/示例/错误。
#[derive(Debug, Clone)]
pub struct ApiEntry {
    pub name: &'static str,
    /// 所属域册（S01/S04/S06/全域态）。
    pub book: &'static str,
    /// 四段齐备标记。
    pub has_signature: bool,
    pub has_semantics: bool,
    pub has_example: bool,
    pub has_errors: bool,
    /// 生成来源（单源生成器版本）。
    pub gen_ver: u32,
    /// 是否手写（手写红线锚）。
    pub handwritten: bool,
}

impl ApiEntry {
    /// 四段齐备断言。
    pub fn sections_complete(&self) -> bool {
        self.has_signature && self.has_semantics && self.has_example && self.has_errors
    }

    /// 缺段清单。
    pub fn missing_sections(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.has_signature {
            v.push("签名");
        }
        if !self.has_semantics {
            v.push("语义");
        }
        if !self.has_example {
            v.push("示例");
        }
        if !self.has_errors {
            v.push("错误");
        }
        v
    }

    /// 补段。
    pub fn fill_section(&mut self, which: &str) -> S8ICode {
        match which {
            "签名" => self.has_signature = true,
            "语义" => self.has_semantics = true,
            "示例" => self.has_example = true,
            "错误" => self.has_errors = true,
            _ => return S8ICode::SECTION_GAP,
        }
        S8ICode::OK
    }
}

// ── 全域 API 册（归一 + 覆盖硬门）────────────────────────────────────

/// 全域册：S01/S04/S06 + 全域态查询逐接口归一。
#[derive(Debug, Clone)]
pub struct ApiBook {
    pub entries: Vec<ApiEntry>,
    /// 生成器单源版本。
    pub gen_source_ver: u32,
}

pub const BOOKS: [&str; 4] = ["S01", "S04", "S06", "全域态"];

impl ApiBook {
    pub fn new(gen_source_ver: u32) -> Self {
        Self { entries: Vec::new(), gen_source_ver }
    }

    /// 归一登记：手写条目拒收（手写红线）。
    pub fn admit(&mut self, e: ApiEntry) -> S8ICode {
        if e.handwritten {
            return S8ICode::HANDWRITTEN;
        }
        self.entries.push(e);
        S8ICode::OK
    }

    /// 手写检出：册内手写条目名单（应为空）。
    pub fn handwritten_list(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| e.handwritten).map(|e| e.name).collect()
    }

    /// 覆盖率：四段齐备条目 / 总条目（万分比）。
    pub fn cover_ppm(&self) -> u32 {
        if self.entries.is_empty() {
            return 0;
        }
        let ok = self.entries.iter().filter(|e| e.sections_complete()).count();
        (ok * 10_000 / self.entries.len()) as u32
    }

    /// 覆盖硬门：< 100% 阻断。
    pub fn hard_gate(&self) -> S8ICode {
        if self.cover_ppm() == 10_000 {
            S8ICode::OK
        } else {
            S8C_CODE_BLOCK
        }
    }

    /// 漂移检出：条目生成版本 ≠ 单源版本。
    pub fn drifts(&self) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|e| e.gen_ver != self.gen_source_ver)
            .map(|e| e.name)
            .collect()
    }

    /// 生成器修复：全部条目重打单源版本戳。
    pub fn repin(&mut self) -> usize {
        let v = self.gen_source_ver;
        let mut n = 0;
        for e in self.entries.iter_mut() {
            if e.gen_ver != v {
                e.gen_ver = v;
                n += 1;
            }
        }
        n
    }

    /// 域册覆盖：四册全有接口才算全域归一。
    pub fn books_covered(&self) -> Vec<&'static str> {
        BOOKS
            .iter()
            .filter(|b| !self.entries.iter().any(|e| e.book == **b))
            .map(|b| *b)
            .collect()
    }
}

// 常量别名：阻断码（避免长表达式内嵌）。
const S8C_CODE_BLOCK: S8ICode = S8ICode::COVER_BLOCK;

// ── 版本对照（漂移审计）──────────────────────────────────────────────

/// 版本对照表：册版本 vs 生成器版本逐对核对。
#[derive(Debug, Clone)]
pub struct VerCompare {
    pub pairs: Vec<(&'static str, u32, u32)>,
}

impl VerCompare {
    pub fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    pub fn record(&mut self, book: &'static str, doc_ver: u32, gen_ver: u32) {
        self.pairs.push((book, doc_ver, gen_ver));
    }

    /// 对照分歧检出。
    pub fn diverged(&self) -> Vec<&'static str> {
        self.pairs.iter().filter(|(_, a, b)| a != b).map(|(n, _, _)| *n).collect()
    }

    pub fn aligned(&self) -> bool {
        self.diverged().is_empty() && !self.pairs.is_empty()
    }
}

impl Default for VerCompare {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves203_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves203_s08apiref");

    fn entry(name: &'static str, book: &'static str, ver: u32, full: bool) -> ApiEntry {
        ApiEntry {
            name,
            book,
            has_signature: full,
            has_semantics: full,
            has_example: full,
            has_errors: full,
            gen_ver: ver,
            handwritten: false,
        }
    }

    // 01 全域归一：四册全有接口
    {
        let mut b = ApiBook::new(3);
        b.admit(entry("a11y_get_state", "全域态", 3, true));
        b.admit(entry("s01_contrast", "S01", 3, true));
        b.admit(entry("s04_motion", "S04", 3, true));
        b.admit(entry("s06_focus", "S06", 3, true));
        chk(&mut set, "C3949-01", b.books_covered().is_empty() && b.entries.len() == 4, "err");
    }
    // 02 手写红线：拒收 + 检出
    {
        let mut b = ApiBook::new(1);
        let mut h = entry("手写接口", "S01", 1, true);
        h.handwritten = true;
        chk(&mut set, "C3949-02", b.admit(h) == S8ICode::HANDWRITTEN && b.entries.is_empty(), "err");
    }
    // 03 四段齐备断言
    {
        let e = entry("完整", "S04", 1, true);
        chk(&mut set, "C3949-03", e.sections_complete() && e.missing_sections().is_empty(), "err");
    }
    // 04 四段缺→补实测
    {
        let mut e = entry("缺段", "S06", 1, false);
        let miss = e.missing_sections();
        chk(&mut set, "C3949-04", miss.len() == 4
            && e.fill_section("签名") == S8ICode::OK
            && e.fill_section("语义") == S8ICode::OK
            && e.fill_section("示例") == S8ICode::OK
            && e.fill_section("错误") == S8ICode::OK
            && e.sections_complete(), "err");
    }
    // 05 未知段名拒补
    {
        let mut e = entry("x", "S01", 1, false);
        chk(&mut set, "C3949-05", e.fill_section("不存在") == S8ICode::SECTION_GAP, "err");
    }
    // 06 覆盖硬门：100% 放行
    {
        let mut b = ApiBook::new(2);
        b.admit(entry("a", "S01", 2, true));
        b.admit(entry("b", "S04", 2, true));
        chk(&mut set, "C3949-06", b.cover_ppm() == 10_000 && b.hard_gate() == S8ICode::OK, "err");
    }
    // 07 覆盖 <100% → 阻断实测
    {
        let mut b = ApiBook::new(2);
        b.admit(entry("a", "S01", 2, true));
        b.admit(entry("b", "S04", 2, false));
        chk(&mut set, "C3949-07", b.cover_ppm() == 5_000 && b.hard_gate() == S8ICode::COVER_BLOCK, "err");
    }
    // 08 漂移检出
    {
        let mut b = ApiBook::new(5);
        b.admit(entry("新", "S01", 5, true));
        b.admit(entry("旧", "S04", 4, true));
        chk(&mut set, "C3949-08", b.drifts() == vec!["旧"], "err");
    }
    // 09 生成器修复归一
    {
        let mut b = ApiBook::new(5);
        b.admit(entry("新", "S01", 5, true));
        b.admit(entry("旧", "S04", 4, true));
        let n = b.repin();
        chk(&mut set, "C3949-09", n == 1 && b.drifts().is_empty(), "err");
    }
    // 10 版本对照一致
    {
        let mut v = VerCompare::new();
        v.record("S01", 3, 3);
        v.record("S04", 3, 3);
        chk(&mut set, "C3949-10", v.aligned(), "err");
    }
    // 11 版本对照分歧检出
    {
        let mut v = VerCompare::new();
        v.record("S01", 3, 3);
        v.record("S06", 2, 3);
        chk(&mut set, "C3949-11", v.diverged() == vec!["S06"] && !v.aligned(), "err");
    }
    // 12 空册覆盖为零
    {
        let b = ApiBook::new(1);
        chk(&mut set, "C3949-12", b.cover_ppm() == 0 && b.hard_gate() == S8ICode::COVER_BLOCK, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3949-13", S8ICode::DRIFT_DETECTED.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8ICode::OK, S8ICode::HANDWRITTEN, S8ICode::COVER_BLOCK, S8ICode::SECTION_GAP, S8ICode::DRIFT_DETECTED];
        chk(&mut set, "C3949-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_apiref_all_green() {
        let s = run_ves203_checks();
        assert!(s.all_passed(), "ves203 red: {}", s.red_items_text());
    }

    #[test]
    fn test_cover_gate_boundary() {
        let mut b = ApiBook::new(1);
        for i in 0..4 {
            b.admit(ApiEntry {
                name: "x",
                book: "S01",
                has_signature: i < 3,
                has_semantics: true,
                has_example: true,
                has_errors: true,
                gen_ver: 1,
                handwritten: false,
            })
            ;
        }
        assert_eq!(b.cover_ppm(), 7_500);
        assert_eq!(b.hard_gate(), S8ICode::COVER_BLOCK);
    }

    #[test]
    fn test_handwritten_redline_list() {
        let mut b = ApiBook::new(1);
        b.admit(ApiEntry {
            name: "h",
            book: "S04",
            has_signature: true,
            has_semantics: true,
            has_example: true,
            has_errors: true,
            gen_ver: 1,
            handwritten: true,
        })
        ; // 拒收
        assert!(b.handwritten_list().is_empty());
    }
}
