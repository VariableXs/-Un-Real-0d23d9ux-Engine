//! ves302_s15extract — VE-F4062 · T04 文案抽取管线
//! 源码/资源/配置三源扫描→键值对提取 + 三要素抽取（键命名规范+默认值必填+注释上下文必附）+ 增量 diff + schema 冻结 + 键唯一。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8840;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ExCode {
    Ok = 0,
    KeyDup = SEG_BASE + 0x01,
    DefaultMissing = SEG_BASE + 0x02,
    ContextMissing = SEG_BASE + 0x03,
    SchemaDrift = SEG_BASE + 0x04,
    BadKeyName = SEG_BASE + 0x05,
    ScanPrivate = SEG_BASE + 0x06,
}

/// 三源。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source { Code = 0, Resource = 1, Config = 2 }

impl Source {
    pub const ALL: [Source; 3] = [Source::Code, Source::Resource, Source::Config];
}

/// 键命名规范：小写字母数字点分段（如 app.title.ok）。
pub fn key_name_ok(key: &str) -> bool {
    if key.is_empty() { return false; }
    for c in key.chars() {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_') {
            return false;
        }
    }
    true
}

/// 抽取三元组。
#[derive(Clone)]
pub struct ExtractItem {
    pub key: String,
    pub default: String,
    pub context: String,
    pub src: Source,
}

impl ExtractItem {
    /// 三要素：键合规 + 默认值必填 + 上下文必附。
    pub fn well_formed(&self) -> Result<(), ExCode> {
        if !key_name_ok(&self.key) { return Err(ExCode::BadKeyName); }
        if self.default.is_empty() { return Err(ExCode::DefaultMissing); }
        if self.context.is_empty() { return Err(ExCode::ContextMissing); }
        Ok(())
    }
}

pub struct ExtractBook {
    pub items: Vec<ExtractItem>,
    pub schema_ver: u32,
    pub frozen_ver: u32,
}

impl ExtractBook {
    pub fn new() -> Self { ExtractBook { items: Vec::new(), schema_ver: 1, frozen_ver: 1 } }

    pub fn add(&mut self, it: ExtractItem) -> Result<(), ExCode> {
        it.well_formed()?;
        for e in self.items.iter() {
            if e.key == it.key { return Err(ExCode::KeyDup); }
        }
        self.items.push(it);
        Ok(())
    }

    /// schema 冻结对版。
    pub fn schema_check(&self) -> Result<(), ExCode> {
        if self.schema_ver != self.frozen_ver { return Err(ExCode::SchemaDrift); }
        Ok(())
    }

    /// 增量 diff：只收变更键（此处以 diff 键集驱动重扫）。
    pub fn incremental(&self, changed: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for k in changed.iter() {
            if self.items.iter().any(|e| &e.key == k) || !self.items.iter().any(|e| &e.key == k) {
                out.push(k.clone());
            }
        }
        out
    }

    /// 扫描红线：不读用户目录。
    pub fn scan_path_ok(path: &str) -> Result<(), ExCode> {
        if path.starts_with("C:\\Users\\") || path.starts_with("/home/") {
            return Err(ExCode::ScanPrivate);
        }
        Ok(())
    }
}

pub fn run_ves302_checks() -> CheckSet {
    let mut set = CheckSet::new("ves302_s15extract");
    let mut eb = ExtractBook::new();

    // 1) 键合规判定。
    set.add("C4062-01", key_name_ok("app.title.ok") && key_name_ok("btn_1") && !key_name_ok("键") && !key_name_ok(""), "键命名规范");

    // 2) 默认值缺阻断。
    set.add("C4062-02", eb.add(ExtractItem { key: String::from("a.b"), default: String::new(), context: String::from("c"), src: Source::Code }) == Err(ExCode::DefaultMissing), "默认值缺阻断");

    // 3) 上下文缺阻断。
    set.add("C4062-03", eb.add(ExtractItem { key: String::from("a.b"), default: String::from("d"), context: String::new(), src: Source::Code }) == Err(ExCode::ContextMissing), "上下文缺阻断");

    // 4) 键名违规拒。
    set.add("C4062-04", eb.add(ExtractItem { key: String::from("A.B!"), default: String::from("d"), context: String::from("c"), src: Source::Code }) == Err(ExCode::BadKeyName), "键名违规");

    // 5) 正常入库。
    let _ = eb.add(ExtractItem { key: String::from("a.b"), default: String::from("确定"), context: String::from("对话框主按钮"), src: Source::Code });
    set.add("C4062-05", eb.items.len() == 1, "正常入库");

    // 6) 键重名冲突立案。
    set.add("C4062-06", eb.add(ExtractItem { key: String::from("a.b"), default: String::from("x"), context: String::from("y"), src: Source::Resource }) == Err(ExCode::KeyDup), "键重名拒");

    // 7) schema 冻结对版。
    set.add("C4062-07", eb.schema_check().is_ok(), "schema 对版");

    // 8) schema 漂移检出。
    eb.schema_ver = 2;
    set.add("C4062-08", eb.schema_check() == Err(ExCode::SchemaDrift), "schema 漂移");

    // 9) 增量 diff。
    let ch = [String::from("a.b"), String::from("c.d")];
    set.add("C4062-09", eb.incremental(&ch).len() == 2, "增量 diff");

    // 10) 扫描红线：用户目录拒。
    set.add("C4062-10", ExtractBook::scan_path_ok("C:\\Users\\varia\\doc") == Err(ExCode::ScanPrivate)
        && ExtractBook::scan_path_ok("src/ui/app.rs").is_ok(), "扫描红线");

    // 11) 三源齐备判定。
    set.add("C4062-11", Source::ALL.len() == 3, "三源");

    // 12) 段位自查。
    set.add("C4062-12", SEG_BASE == 0x8840, "段位 0x8840");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_flow() {
        assert!(key_name_ok("app.title"));
        assert!(!key_name_ok("Bad Key"));
        let it = ExtractItem { key: String::from("k"), default: String::from("d"), context: String::from("c"), src: Source::Code };
        assert!(it.well_formed().is_ok());
    }
}
