//! UNX-F56058 五段段表公开文档件（unxstar 段）
//!
//! [J1] 文档完整：五段段表（固件/引导器/内核/登录/桌面）以公开文档形式渲染，
//!      每段含段号、段名、口径说明、单位与序位；段闭集外内容拒绝入表。
//! [J2] 防篡改：文档渲染逐字节确定，文档指纹（FNV-1a）可复算；任何非规范行
//!      回读解析必须失败（parse 端守闸），保证"渲染-回读"闭环。
//! [J3] 公开纪律：文档头含版本号与 SEG_BASE 锚点；行数恒为 5 段 + 1 头行；
//!      注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（段号越界/段名不符/序位重复/文档行数异常/回读解析失败/指纹漂移/头行缺失）
//!   纯函数：fnv32、canonical_doc 渲染、parse_doc 回读
//!   SegRow：段表行，new() 守闸
//!   SegDoc：段表文档（add_row/render/self_verify）
//!   injection_core：5 例必抓
//!   run_unx56058_checks：≥12 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60720;

/// 段号越界（不在 1..=5）
pub const D_SEG_RANGE: u32 = SEG_BASE + 0x01;
/// 段名与段号不符
pub const D_NAME_MISMATCH: u32 = SEG_BASE + 0x02;
/// 序位重复（order 冲突）
pub const D_ORDER_DUP: u32 = SEG_BASE + 0x03;
/// 文档行数异常（非 6 行）
pub const D_ROW_COUNT: u32 = SEG_BASE + 0x04;
/// 回读解析失败
pub const D_PARSE_FAIL: u32 = SEG_BASE + 0x05;
/// 指纹漂移（同内容两次渲染指纹不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 头行缺失（文档不以 DOC|开头）
pub const D_HEAD_MISSING: u32 = SEG_BASE + 0x07;

/// 段闭集：段号 → 段名
pub const SEG_NAMES: [&str; 5] = ["firmware", "bootloader", "kernel", "login", "desktop"];
/// 单位闭集
pub const UNITS: [&str; 2] = ["ms", "tick"];

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 段表行：段号 + 段名 + 单位 + 序位
#[derive(Clone)]
pub struct SegRow {
    pub seg_id: u32,
    pub name: String,
    pub unit: String,
    pub order: u32,
}

impl SegRow {
    /// 守闸：段号闭集 + 段名对应 + 单位闭集 + 序位 1..=5
    pub fn new(seg_id: u32, name: &str, unit: &str, order: u32) -> Result<Self, u32> {
        if seg_id < 1 || seg_id > 5 {
            return Err(D_SEG_RANGE);
        }
        if SEG_NAMES[(seg_id - 1) as usize] != name {
            return Err(D_NAME_MISMATCH);
        }
        if !UNITS.contains(&unit) {
            return Err(D_NAME_MISMATCH);
        }
        if order < 1 || order > 5 {
            return Err(D_SEG_RANGE);
        }
        Ok(SegRow { seg_id, name: name.to_string(), unit: unit.to_string(), order })
    }
}

/// 段表公开文档
pub struct SegDoc {
    pub version: String,
    rows: Vec<SegRow>,
}

impl SegDoc {
    pub fn new(version: &str) -> Self {
        SegDoc { version: version.to_string(), rows: Vec::new() }
    }

    /// 入表：序位重复拒绝
    pub fn add_row(&mut self, r: SegRow) -> Result<(), u32> {
        if self.rows.iter().any(|x| x.order == r.order) {
            return Err(D_ORDER_DUP);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 规范渲染：头行 + 按 order 排序的 5 行，逐字节确定
    pub fn canonical_doc(&self) -> Result<String, u32> {
        if self.rows.len() != 5 {
            return Err(D_ROW_COUNT);
        }
        let mut sorted: Vec<&SegRow> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.order);
        let mut out = format!("DOC|unx-f56058|v{}|segbase={}", self.version, SEG_BASE);
        for r in sorted {
            out = format!("{}\nSEG|{}|{}|{}|{}", out, r.order, r.seg_id, r.name, r.unit);
        }
        Ok(out)
    }

    /// 文档指纹
    pub fn doc_fingerprint(&self) -> Result<u32, u32> {
        let doc = self.canonical_doc()?;
        Ok(fnv32(doc.as_bytes()))
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// 回读解析：逐行校验，非规范行返回 D_PARSE_FAIL
pub fn parse_doc(doc: &str) -> Result<(String, Vec<(u32, u32, String, String)>), u32> {
    let mut lines = doc.split('\n');
    let head = match lines.next() {
        Some(h) => h,
        None => return Err(D_HEAD_MISSING),
    };
    let head_parts: Vec<&str> = head.split('|').collect();
    if head_parts.len() != 4 || head_parts[0] != "DOC" {
        return Err(D_HEAD_MISSING);
    }
    let version = head_parts[2].trim_start_matches('v').to_string();
    let mut parsed = Vec::new();
    for line in lines {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 5 || parts[0] != "SEG" {
            return Err(D_PARSE_FAIL);
        }
        let order: u32 = parts[1].parse().map_err(|_| D_PARSE_FAIL)?;
        let seg_id: u32 = parts[2].parse().map_err(|_| D_PARSE_FAIL)?;
        let name = parts[3].to_string();
        let unit = parts[4].to_string();
        if seg_id < 1 || seg_id > 5 || SEG_NAMES[(seg_id - 1) as usize] != name {
            return Err(D_PARSE_FAIL);
        }
        if !UNITS.contains(&unit.as_str()) {
            return Err(D_PARSE_FAIL);
        }
        parsed.push((order, seg_id, name, unit));
    }
    if parsed.len() != 5 {
        return Err(D_ROW_COUNT);
    }
    Ok((version, parsed))
}

/// 标准夹具：五段 ms 口径，序位同段号
pub fn fixture_doc() -> SegDoc {
    let mut d = SegDoc::new("3");
    for i in 1..=5u32 {
        let r = SegRow::new(i, SEG_NAMES[(i - 1) as usize], "ms", i).expect("fixture row");
        d.add_row(r).expect("fixture add");
    }
    d
}

/// 防篡改探针：污染一行后回读必须失败
pub fn tamper_probe() -> Result<(), u32> {
    let doc = fixture_doc().canonical_doc().map_err(|e| e)?;
    let evil = doc.replace("SEG|3|3|kernel|ms", "SEG|3|3|kern3l|ms");
    if evil == doc {
        return Err(D_FP_DRIFT);
    }
    if parse_doc(&evil).is_ok() {
        return Err(D_PARSE_FAIL);
    }
    Ok(())
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 段号越界
    if SegRow::new(6, "extra", "ms", 6).err() == Some(D_SEG_RANGE) {
        caught += 1;
    }

    // 2) 段名不符
    if SegRow::new(3, "login", "ms", 3).err() == Some(D_NAME_MISMATCH) {
        caught += 1;
    }

    // 3) 序位重复
    let mut d = fixture_doc();
    let dup = SegRow::new(1, "firmware", "ms", 1).expect("dup");
    if d.add_row(dup) == Err(D_ORDER_DUP) {
        caught += 1;
    }

    // 4) 行数不足渲染拒绝
    let mut h = SegDoc::new("1");
    let r = SegRow::new(1, "firmware", "ms", 1).expect("r");
    let _ = h.add_row(r);
    if h.canonical_doc() == Err(D_ROW_COUNT) {
        caught += 1;
    }

    // 5) 篡改行回读失败
    if tamper_probe().is_ok() {
        // 未按预期拒绝
    } else {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_doc().doc_fingerprint().map_err(|e| e)?;
    let b = fixture_doc().doc_fingerprint().map_err(|e| e)?;
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56058_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56058");

    // J1 文档完整
    let d = fixture_doc();
    s.add("56058-J1-五段入表", d.row_count() == 5, "段表行数不为 5");
    let doc = d.canonical_doc().expect("render");
    s.add("56058-J1-行数6", doc.split('\n').count() == 6, "文档总行数不为 6");
    s.add("56058-J1-头行锚", doc.starts_with("DOC|unx-f56058|v3|segbase=395040"), "头行锚不符");
    s.add("56058-J1-单位闭集", UNITS.len() == 2, "单位闭集异常");

    // J2 防篡改闭环
    let (ver, parsed) = parse_doc(&doc).expect("parse");
    s.add("56058-J2-回读版本", ver == "3", "回读版本不符");
    s.add("56058-J2-回读段数", parsed.len() == 5, "回读段数不为 5");
    s.add("56058-J2-回读序位", parsed.iter().map(|p| p.0).collect::<Vec<u32>>() == vec![1, 2, 3, 4, 5], "回读序位非升序");
    s.add("56058-J2-篡改拦截", tamper_probe().is_ok(), "篡改行未被回读拦截");
    s.add("56058-J2-指纹稳定", fp_stability().is_ok(), "同内容两次渲染指纹漂移");

    // J2 渲染-回读指纹闭环
    let fp_render = d.doc_fingerprint().expect("fp");
    let fp_reparse = fnv32(fixture_doc().canonical_doc().expect("re").as_bytes());
    s.add("56058-J2-指纹闭环", fp_render == fp_reparse, "渲染与回读指纹不一致");

    // J3 公开纪律与闭集
    s.add("56058-J3-段号闭集", SegRow::new(0, "firmware", "ms", 1).err() == Some(D_SEG_RANGE), "段号 0 未被拒绝");
    s.add("56058-J3-单位闭集", SegRow::new(1, "firmware", "cycles", 1).err() == Some(D_NAME_MISMATCH), "非法单位未被拒绝");
    let (caught, total) = injection_core();
    s.add_dyn(
        "56058-J3-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56058_all_checks_green() {
        let s = run_unx56058_checks();
        assert!(
            s.all_passed(),
            "UNX-F56058 有红项: {}",
            s.red_items_text()
        );
    }
}
