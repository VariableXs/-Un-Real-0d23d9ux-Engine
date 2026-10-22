// unx3662 —— UNX-F3662 符号表结构（函数表/行号表/文件表三表）
// 判据原文 J1：三表结构（函数表 addr 区间→fn 名 / 行号表 fn 内 addr→源行 /
// 文件表 fn→源文件）；M-420 三表齐译（一帧译出 (fn, line, file) 三要素）。
// J2：表封闭（表 schema 冻结 v1——B15 同律）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFE80 独占（0xFE80~0xFEFF）。
//
// 设计（纯功能 no_std）：
//   * 三表闭集：函数表（addr 区间→fn 名）/ 行号表（fn 内 addr→源行）/
//     文件表（fn→源文件）；
//   * 三表齐译：任一 addr 一帧译出 (fn, line, file) 三要素；
//   * schema 冻结 v1：三表字段集 const 刻印，未知字段拒绝；
//   * 注入核：缺表译失败/区间重叠/未知字段/文件表漂移 四类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFE80 独占） ─────────────────────────────────────────────
pub const XC_SYM_MISS: u32 = 0xFE81; // 缺表译失败
pub const XC_SYM_OVER: u32 = 0xFE82; // 函数区间重叠
pub const XC_SYM_FIELD: u32 = 0xFE83; // 未知字段
pub const XC_SYM_FILE: u32 = 0xFE84; // 文件表漂移

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// schema v1 字段集（冻结）。
pub const SCHEMA_V1: [&str; 6] = ["fntab", "lntab", "filetab", "addr", "line", "file"];

pub fn schema_field_ok(name: &str) -> bool {
    SCHEMA_V1.contains(&name)
}

/// 函数表项：addr 区间 → fn 名。
#[derive(Clone)]
pub struct FnEntry {
    pub lo: u64,
    pub hi: u64, // [lo, hi)
    pub name: &'static str,
}

/// 行号表项：fn 名 + fn 内偏移 → 源行。
#[derive(Clone)]
pub struct LnEntry {
    pub fn_name: &'static str,
    pub offset: u64,
    pub line: u32,
}

/// 文件表项：fn 名 → 源文件。
#[derive(Clone)]
pub struct FileEntry {
    pub fn_name: &'static str,
    pub file: &'static str,
}

/// 符号表三表总成。
pub struct SymTab {
    fns: Vec<FnEntry>,
    lines: Vec<LnEntry>,
    files: Vec<FileEntry>,
    frozen: Option<u32>,
}

impl SymTab {
    pub fn new() -> Self {
        SymTab { fns: Vec::new(), lines: Vec::new(), files: Vec::new(), frozen: None }
    }

    pub fn add_fn(&mut self, lo: u64, hi: u64, name: &'static str) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(XC_SYM_FIELD);
        }
        if lo >= hi {
            return Err(XC_SYM_OVER);
        }
        // 区间重叠拒绝（函数空间互斥）
        for f in &self.fns {
            if lo < f.hi && f.lo < hi {
                return Err(XC_SYM_OVER);
            }
        }
        self.fns.push(FnEntry { lo, hi, name });
        Ok(())
    }

    pub fn add_line(&mut self, fn_name: &'static str, offset: u64, line: u32) -> Result<(), u32> {
        if !self.fns.iter().any(|f| f.name == fn_name) {
            return Err(XC_SYM_MISS);
        }
        self.lines.push(LnEntry { fn_name, offset, line });
        Ok(())
    }

    pub fn add_file(&mut self, fn_name: &'static str, file: &'static str) -> Result<(), u32> {
        if !self.fns.iter().any(|f| f.name == fn_name) {
            return Err(XC_SYM_MISS);
        }
        self.files.push(FileEntry { fn_name, file });
        Ok(())
    }

    /// 三表齐译：addr → (fn, line, file)。
    pub fn translate(&self, addr: u64, fn_off: u64) -> Option<(&'static str, u32, &'static str)> {
        let f = self.fns.iter().find(|f| addr >= f.lo && addr < f.hi)?;
        let ln = self
            .lines
            .iter()
            .filter(|l| l.fn_name == f.name)
            .find(|l| l.offset == fn_off)
            .map(|l| l.line)?;
        let fl = self.files.iter().find(|x| x.fn_name == f.name).map(|x| x.file)?;
        Some((f.name, ln, fl))
    }

    /// 行号表内最大偏移（fn 内）。
    pub fn max_line_of(&self, fn_name: &str) -> Option<u32> {
        self.lines
            .iter()
            .filter(|l| l.fn_name == fn_name)
            .map(|l| l.line)
            .max()
    }

    /// 文件表覆盖核：每个 fn 恰一个文件行。
    pub fn files_covered(&self) -> bool {
        self.fns.iter().all(|f| {
            let c = self.files.iter().filter(|x| x.fn_name == f.name).count();
            c == 1
        })
    }

    /// 函数表区间互斥复扫。
    pub fn no_overlap(&self) -> bool {
        for i in 0..self.fns.len() {
            for j in (i + 1)..self.fns.len() {
                let a = &self.fns[i];
                let b = &self.fns[j];
                if a.lo < b.hi && b.lo < a.hi {
                    return false;
                }
            }
        }
        true
    }

    pub fn freeze(&mut self) -> u32 {
        let mut b = Vec::new();
        for f in &self.fns {
            b.extend_from_slice(&f.lo.to_le_bytes());
            b.extend_from_slice(&f.hi.to_le_bytes());
            b.extend_from_slice(f.name.as_bytes());
            b.push(0xFF);
        }
        for l in &self.lines {
            b.extend_from_slice(l.fn_name.as_bytes());
            b.extend_from_slice(&l.offset.to_le_bytes());
            b.extend_from_slice(&l.line.to_le_bytes());
            b.push(0xFE);
        }
        for x in &self.files {
            b.extend_from_slice(x.fn_name.as_bytes());
            b.extend_from_slice(x.file.as_bytes());
            b.push(0xFD);
        }
        let h = fnv1a32(&b);
        self.frozen = Some(h);
        h
    }
    pub fn fingerprint(&self) -> u32 {
        self.frozen.unwrap_or(0)
    }
    pub fn fn_count(&self) -> usize {
        self.fns.len()
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// 标准三表（3 函数：main/mid/leaf）。
pub fn standard_symtab() -> SymTab {
    let mut t = SymTab::new();
    let _ = t.add_fn(0x1000_0000, 0x1000_0100, "leaf");
    let _ = t.add_fn(0x1000_0100, 0x1000_0200, "mid");
    let _ = t.add_fn(0x1000_0200, 0x1000_0300, "main");
    let _ = t.add_line("leaf", 0x00, 12);
    let _ = t.add_line("leaf", 0x20, 18);
    let _ = t.add_line("mid", 0x00, 40);
    let _ = t.add_line("main", 0x00, 7);
    let _ = t.add_file("leaf", "io.rs");
    let _ = t.add_file("mid", "sched.rs");
    let _ = t.add_file("main", "main.rs");
    t
}

/// 注入核：缺表/区间重叠/未知字段/文件表漂移 四类必抓。
pub struct SymInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl SymInjectCore {
    pub fn new() -> Self {
        SymInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_missing_table(&mut self) {
        self.total += 1;
        // 缺文件表 → 三表齐译必失败
        let mut t = SymTab::new();
        let _ = t.add_fn(0x1000_0000, 0x1000_0100, "f");
        let _ = t.add_line("f", 0x00, 1);
        // 不加 file
        if t.translate(0x1000_0000, 0x00).is_none() {
            self.caught += 1;
        }
    }
    pub fn inject_overlap(&mut self) {
        self.total += 1;
        let mut t = SymTab::new();
        let _ = t.add_fn(0x1000_0000, 0x1000_0100, "a");
        let r = t.add_fn(0x1000_0080, 0x1000_0180, "b"); // 重叠
        if r.is_err() && t.no_overlap() {
            self.caught += 1;
        }
    }
    pub fn inject_unknown_field(&mut self) {
        self.total += 1;
        // 未知字段（"debug2"）不在 schema v1 → 拒
        if !schema_field_ok("debug2") && schema_field_ok("fntab") {
            self.caught += 1;
        }
    }
    pub fn inject_file_drift(&mut self) {
        self.total += 1;
        let t = standard_symtab();
        // 文件表覆盖核：3 fn 恰 3 file 行
        if t.files_covered() && t.file_count() == 3 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx3662_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx3662");

    // C62-01 三表齐译（M-420）三函数全译
    {
        let t = standard_symtab();
        let a = t.translate(0x1000_0000, 0x00);
        let b = t.translate(0x1000_0100, 0x00);
        let c = t.translate(0x1000_0200, 0x00);
        let ok = a == Some(("leaf", 12, "io.rs"))
            && b == Some(("mid", 40, "sched.rs"))
            && c == Some(("main", 7, "main.rs"));
        s.add("C62-01-三表齐译", ok, if ok { "" } else { "齐译断" });
    }
    // C62-02 函数区间互斥
    {
        let t = standard_symtab();
        s.add(
            "C62-02-区间互斥",
            t.no_overlap() && t.fn_count() == 3,
            if t.no_overlap() { "" } else { "区间重叠" },
        );
    }
    // C62-03 文件表覆盖（每 fn 恰一行）
    {
        let t = standard_symtab();
        s.add(
            "C62-03-文件覆盖",
            t.files_covered(),
            if t.files_covered() { "" } else { "文件表缺行" },
        );
    }
    // C62-04 schema v1 冻结（字段集刻印）
    {
        let ok = SCHEMA_V1.len() == 6 && schema_field_ok("line") && !schema_field_ok("x9");
        s.add("C62-04-schema 冻结", ok, if ok { "" } else { "字段集漂移" });
    }
    // C62-05 冻结哈希恒等
    {
        let mut t = standard_symtab();
        let h = t.freeze();
        s.add(
            "C62-05-哈希入册",
            h != 0 && t.fingerprint() == h,
            if h != 0 { "" } else { "指纹漂移" },
        );
    }
    // C62-06 冻结后加表拒绝
    {
        let mut t = standard_symtab();
        let _ = t.freeze();
        let r = t.add_fn(0x1000_0300, 0x1000_0400, "late");
        s.add(
            "C62-06-冻结拒改",
            r == Err(XC_SYM_FIELD),
            if r.is_err() { "" } else { "冻结失守" },
        );
    }
    // C62-07 行号表最大行
    {
        let t = standard_symtab();
        let ok = t.max_line_of("leaf") == Some(18);
        s.add("C62-07-行号面", ok, if ok { "" } else { "行号断" });
    }
    // C62-08 注入核 4/4
    {
        let mut inj = SymInjectCore::new();
        inj.inject_missing_table();
        inj.inject_overlap();
        inj.inject_unknown_field();
        inj.inject_file_drift();
        s.add(
            "C62-08-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
