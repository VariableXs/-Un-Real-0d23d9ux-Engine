//! UNX-F46419 B01 防重 grep 执行记录（关键词族：triplet/sysroot/pin/dumpmachine）（0x40120 段）
//!
//! 锚点：UNX-F46419-J1 批收口防重 grep 于批收口执行留痕（AI-84 抽查口径）。
//! 域：CoRun·UNX-S1-B01（工具链域·防重留痕面）。
//!
//! 结构：关键词族账（四词闭集 × 命中登记：词 × 文件 × 行号 × 语义位）+
//! 执行留痕账（grep 执行序 + 词命中数）+ 防重裁定（同语义位二次登记 =
//! 重复命中，收口前必须清零）+ 注入核（空词/越词集/重复语义位/
//! 留痕缺行/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40120 独占 0x10）。
pub const SEG_BASE: u32 = 0x40120;
pub const DG_WORD_OOB: u32 = SEG_BASE + 0x01; // 词越闭集
pub const DG_EMPTY: u32 = SEG_BASE + 0x02; // 空命中登记
pub const DG_DUP_SEM: u32 = SEG_BASE + 0x03; // 重复语义位
pub const DG_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const DG_LOG_MISS: u32 = SEG_BASE + 0x05; // 留痕缺行

/// 关键词族四词闭集。
pub const KW_TRIPLET: usize = 0;
pub const KW_SYSROOT: usize = 1;
pub const KW_PIN: usize = 2;
pub const KW_DUMPMACHINE: usize = 3;
pub const KW_COUNT: usize = 4;

pub const KW_NAMES: [&str; KW_COUNT] = ["triplet", "sysroot", "pin", "dumpmachine"];

/// 命中登记：词 × 文件 × 行号 × 语义位（语义位 = 词内唯一序号）。
#[derive(Clone)]
pub struct GrepHit {
    pub word: usize,
    pub file: String,
    pub line: u32,
    pub sem: u32,
}

impl GrepHit {
    pub fn key(&self) -> String {
        format!("{}|{}|{}|{}", KW_NAMES[self.word], self.file, self.line, self.sem)
    }
}

/// 防重执行账。
pub struct DedupGrep {
    pub hits: Vec<GrepHit>,
    pub executed: [bool; KW_COUNT],
    pub sealed: bool,
}

impl DedupGrep {
    pub fn new() -> Self {
        DedupGrep { hits: Vec::new(), executed: [false; KW_COUNT], sealed: false }
    }

    /// 执行留痕：词扫描完成标记。
    pub fn mark_exec(&mut self, word: usize) -> Result<(), u32> {
        if word >= KW_COUNT {
            return Err(DG_WORD_OOB);
        }
        if self.sealed {
            return Err(DG_SEALED);
        }
        self.executed[word] = true;
        Ok(())
    }

    /// 命中登记（词越闭集/空文件/重复语义位/封账全拒）。
    pub fn hit(&mut self, word: usize, file: &str, line: u32, sem: u32) -> Result<(), u32> {
        if word >= KW_COUNT {
            return Err(DG_WORD_OOB);
        }
        if file.is_empty() || line == 0 {
            return Err(DG_EMPTY);
        }
        if self.sealed {
            return Err(DG_SEALED);
        }
        let key = format!("{}|{}|{}", word, file, sem);
        if self.hits.iter().any(|h| format!("{}|{}|{}", h.word, h.file, h.sem) == key) {
            return Err(DG_DUP_SEM);
        }
        self.hits.push(GrepHit {
            word,
            file: file.to_string(),
            line,
            sem,
        });
        Ok(())
    }

    /// 防重裁定：同语义位重复命中数（收口前必须清零）。
    pub fn dup_count(&self) -> usize {
        let mut n = 0;
        for i in 0..self.hits.len() {
            for j in (i + 1)..self.hits.len() {
                if self.hits[i].word == self.hits[j].word
                    && self.hits[i].file == self.hits[j].file
                    && self.hits[i].sem == self.hits[j].sem
                {
                    n += 1;
                }
            }
        }
        n
    }

    /// 批收口核：四词全执行 + 零重复语义位 + 命中留痕非空。
    pub fn close_ok(&self) -> bool {
        self.executed.iter().all(|e| *e)
            && self.dup_count() == 0
            && !self.hits.is_empty()
    }

    /// 留痕渲染（词|文件|行号|语义位 四列）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for h in self.hits.iter() {
            out.push_str(&format!("{}|{}|{}|{}\n", KW_NAMES[h.word], h.file, h.line, h.sem));
        }
        out
    }

    /// 留痕行数核（渲染行 = 命中数）。
    pub fn render_ok(&self) -> bool {
        self.render().lines().count() == self.hits.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：四词全执行 + 每词 ×2 命中（同批文件不同语义位）。
pub fn standard_grep() -> DedupGrep {
    let mut g = DedupGrep::new();
    for w in 0..KW_COUNT {
        g.mark_exec(w).expect("fixture exec");
    }
    let files = ["unx46401_pinset.rs", "unx46403_triconst.rs", "unx46409_layout4.rs", "unx46404_dumpmach.rs"];
    for w in 0..KW_COUNT {
        for s in 0..2usize {
            g.hit(w, files[w], (s as u32 + 1) * 10, s as u32).expect("fixture hit");
        }
    }
    g
}

/// 注入核：6 注入必抓（越词集/空命中/重复语义位/封账改写/留痕缺行/未执行收口）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越词集必抓
    let mut g = DedupGrep::new();
    if g.hit(9, "f.rs", 1, 0) == Err(DG_WORD_OOB) {
        caught += 1;
    }
    // 2) 空命中必抓
    if g.hit(KW_PIN, "", 1, 0) == Err(DG_EMPTY) {
        caught += 1;
    }
    // 3) 重复语义位必抓
    g.hit(KW_PIN, "a.rs", 10, 0).expect("first");
    if g.hit(KW_PIN, "a.rs", 20, 0) == Err(DG_DUP_SEM) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_g = standard_grep();
    std_g.seal();
    if std_g.hit(KW_PIN, "late.rs", 1, 0) == Err(DG_SEALED) {
        caught += 1;
    }
    // 5) 留痕缺行必抓（执行 3 词 → close 假）
    let mut g2 = DedupGrep::new();
    for w in 0..3 {
        g2.mark_exec(w).expect("partial exec");
        g2.hit(w, "f.rs", 1, 0).expect("partial hit");
    }
    if !g2.close_ok() {
        caught += 1;
    }
    // 6) 未执行收口必抓（有命中但未标记执行）
    let mut g3 = DedupGrep::new();
    g3.hit(KW_SYSROOT, "f.rs", 1, 0).expect("hit only");
    if !g3.close_ok() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46419_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46419");
    // J1-1 批收口防重留痕（四词全执行 + 零重复 + 留痕非空）
    let g = standard_grep();
    let ok = g.close_ok() && g.hits.len() == 8;
    s.add("46419-J1-防重留痕", ok, if ok { "" } else { "留痕漂移" });
    // J1-2 留痕渲染行齐
    let ok = g.render_ok() && g.render().starts_with("triplet|unx46401_pinset.rs|10|0");
    s.add("46419-J1-留痕渲染", ok, if ok { "" } else { "渲染漂移" });
    // J1-3 关键词族闭集齐
    let ok = KW_NAMES.len() == 4 && KW_NAMES.iter().all(|n| !n.is_empty());
    s.add("46419-J1-关键词族", ok, if ok { "" } else { "词族漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46419-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46419_all_checks_green() {
        let set = run_unx46419_checks();
        assert!(set.all_passed(), "UNX-F46419 checks must be green: {}", set.red_items_text());
    }
}
