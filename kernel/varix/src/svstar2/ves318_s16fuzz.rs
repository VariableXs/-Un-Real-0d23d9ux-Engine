//! ves318_s16fuzz — VE-F4078 · T04 翻译 fuzz
//! fuzz 三面（抽取器畸形源码注入/指纹归一极端字符序列/占位符替换嵌套越界）+ 三零线（崩溃 0/挂起 0/标记破坏 0）+ 最小复现 + 语料入库 + 阻断权。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9040;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FzCode {
    Ok = 0,
    Crash = SEG_BASE + 0x01,
    Hang = SEG_BASE + 0x02,
    MarkerBroken = SEG_BASE + 0x03,
    MinimizedEmpty = SEG_BASE + 0x04,
    SurfaceMissing = SEG_BASE + 0x05,
}

/// fuzz 三面。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface { Extractor = 0, Fingerprint = 1, Placeholder = 2 }

impl Surface {
    pub const ALL: [Surface; 3] = [Surface::Extractor, Surface::Fingerprint, Surface::Placeholder];
    pub fn name(self) -> &'static str {
        match self { Surface::Extractor => "抽取器", Surface::Fingerprint => "指纹归一", Surface::Placeholder => "占位符" }
    }
}

/// 抽取器畸形输入守卫：不崩溃只拒绝。
pub fn extractor_guard(src: &str) -> Result<(), FzCode> {
    if src.contains('\0') { return Err(FzCode::Crash); }
    if src.len() > 4096 { return Err(FzCode::Hang); }
    Ok(())
}

/// 指纹归一极端序列守卫：控制字符剥离不崩溃。
pub fn fingerprint_guard(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in s.chars() {
        if c.is_control() { continue; }
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 占位符替换越界守卫：索引越界只拒绝。
pub fn placeholder_guard(template: &str, args: &[&str], index: usize) -> Result<String, FzCode> {
    let mut out = String::from(template);
    if index >= args.len() { return Err(FzCode::MarkerBroken); }
    // 嵌套越界：模板中 {index} 不存在也拒。
    let mut needle = String::from("{");
    push_num(&mut needle, index as u32);
    needle.push('}');
    if !template.contains(&needle) { return Err(FzCode::MarkerBroken); }
    out = out.replace(&needle, args[index]);
    Ok(out)
}

fn push_num(s: &mut String, mut n: u32) {
    if n == 0 { s.push('0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

/// 三零线记录。
pub struct ZeroLine {
    pub crashes: u32,
    pub hangs: u32,
    pub marker_breaks: u32,
}

impl ZeroLine {
    pub fn new() -> Self { ZeroLine { crashes: 0, hangs: 0, marker_breaks: 0 } }
    pub fn met(&self) -> bool {
        self.crashes == 0 && self.hangs == 0 && self.marker_breaks == 0
    }
    /// 线未达 → 阻断权。
    pub fn blocked(&self) -> bool {
        !self.met()
    }
}

/// 最小复现 + 语料入库。
pub struct FuzzCorpus {
    pub entries: Vec<(Surface, String)>,
}

impl FuzzCorpus {
    pub fn new() -> Self { FuzzCorpus { entries: Vec::new() } }
    pub fn admit(&mut self, s: Surface, minimized: &str) -> Result<(), FzCode> {
        if minimized.is_empty() { return Err(FzCode::MinimizedEmpty); }
        self.entries.push((s, String::from(minimized)));
        Ok(())
    }
    pub fn covers(&self) -> bool {
        Surface::ALL.iter().all(|s| self.entries.iter().any(|e| e.0 == *s))
    }
}

pub fn run_ves318_checks() -> CheckSet {
    let mut set = CheckSet::new("ves318_s16fuzz");

    // 1) 三面齐。
    set.add("C4078-01", Surface::ALL.len() == 3, "三面");

    // 2) 抽取器守卫。
    set.add("C4078-02", extractor_guard("normal key = value").is_ok(), "正常输入");

    // 3) 畸形输入只拒不崩。
    set.add("C4078-03", extractor_guard("bad\0input") == Err(FzCode::Crash)
        && extractor_guard(&"x".repeat(5000)) == Err(FzCode::Hang), "畸形拒绝");

    // 4) 指纹守卫：控制字符剥离且稳定。
    set.add("C4078-04", fingerprint_guard("a\u{0}b") == fingerprint_guard("ab"), "控制符剥离");

    // 5) 占位符替换。
    set.add("C4078-05", placeholder_guard("{0} 项", &["三"], 0) == Ok(String::from("三 项")), "占位符替换");

    // 6) 越界拒。
    set.add("C4078-06", placeholder_guard("{0} 项", &["三"], 1) == Err(FzCode::MarkerBroken), "越界拒");

    // 7) 嵌套越界拒。
    set.add("C4078-07", placeholder_guard("{0} 项", &["三", "四"], 1) == Err(FzCode::MarkerBroken), "嵌套越界拒");

    // 8) 三零线。
    let zl = ZeroLine::new();
    set.add("C4078-08", zl.met() && !zl.blocked(), "三零线");

    // 9) 线破阻断。
    let mut zl2 = ZeroLine::new();
    zl2.crashes = 1;
    set.add("C4078-09", zl2.blocked(), "线破阻断");

    // 10) 语料入库 + 最小复现。
    let mut fc = FuzzCorpus::new();
    let _ = fc.admit(Surface::Extractor, "\\0");
    let _ = fc.admit(Surface::Fingerprint, "\\u{7}");
    let _ = fc.admit(Surface::Placeholder, "{9}");
    set.add("C4078-10", fc.covers() && fc.entries.len() == 3, "语料入库");

    // 11) 最小复现空拒。
    set.add("C4078-11", fc.admit(Surface::Extractor, "") == Err(FzCode::MinimizedEmpty), "空复现拒");

    // 12) 面名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in Surface::ALL.iter() { names.push(s.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4078-12", !ndup, "面名双射");

    // 13) 段位自查。
    set.add("C4078-13", SEG_BASE == 0x9040, "段位 0x9040");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_guards() {
        assert!(extractor_guard("ok").is_ok());
        assert_eq!(extractor_guard("x\0y"), Err(FzCode::Crash));
        assert!(placeholder_guard("{0}", &["a"], 0).is_ok());
    }
}
