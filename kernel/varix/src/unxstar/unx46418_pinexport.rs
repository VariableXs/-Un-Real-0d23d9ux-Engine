//! UNX-F46418 钉版账导出格式（波次闭账材料第 21 波组成部分接口）（0x40110 段）
//!
//! 锚点：UNX-F46418-J1 导出 ×1 与闭账材料 schema 字段完整率 100%。
//! 域：CoRun·UNX-S1-B01（工具链域·导出格式面）。
//!
//! 结构：闭账材料 schema（第 21 波十段闭集逐段登记）+ 导出器（账 →
//! 分段文本，段序敏感）+ 字段完整率核（十段 × 字段逐段齐）+ 注入核
//! （缺段/段序漂移/空账导出/封账改写/schema 漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x40110 独占 0x10）。
pub const SEG_BASE: u32 = 0x40110;
pub const EX_SEG_MISS: u32 = SEG_BASE + 0x01; // 缺段
pub const EX_SEG_ORDER: u32 = SEG_BASE + 0x02; // 段序漂移
pub const EX_EMPTY_BOOK: u32 = SEG_BASE + 0x03; // 空账导出拒绝
pub const EX_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const EX_SCHEMA_DRIFT: u32 = SEG_BASE + 0x05; // schema 漂移

/// 第 21 波闭账材料十段闭集。
pub const WAVE21_SEGMENTS: [&str; 10] = [
    "w21-01-钉版总账", "w21-02-三元组宪法", "w21-03-sysroot 布设", "w21-04-头集扫描",
    "w21-05-许可账", "w21-06-兼容矩阵", "w21-07-混装告警", "w21-08-可观测埋点",
    "w21-09-防重留痕", "w21-10-批收官印",
];

/// 段内容：段名 + 字段行集。
pub struct ExportSegment {
    pub name: &'static str,
    pub fields: Vec<(String, String)>,
}

impl ExportSegment {
    /// 段完整性核（至少一字段且键值非空）。
    pub fn complete(&self) -> bool {
        !self.fields.is_empty() && self.fields.iter().all(|(k, v)| !k.is_empty() && !v.is_empty())
    }

    /// 段渲染（#段名 + 键=值行）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n", self.name));
        for (k, v) in self.fields.iter() {
            out.push_str(&format!("{}={}\n", k, v));
        }
        out
    }
}

/// 闭账材料账（十段有序集）。
pub struct WaveCloseBook {
    pub segments: Vec<ExportSegment>,
    pub sealed: bool,
}

impl WaveCloseBook {
    pub fn new() -> Self {
        WaveCloseBook { segments: Vec::new(), sealed: false }
    }

    /// 段登记（段名必须 ∈ 十段闭集；段序 = 登记序；重复段拒绝；封账拒写）。
    pub fn put(&mut self, name: &'static str, fields: Vec<(String, String)>) -> Result<(), u32> {
        if !WAVE21_SEGMENTS.iter().any(|s| *s == name) {
            return Err(EX_SEG_MISS);
        }
        if self.sealed {
            return Err(EX_SEALED);
        }
        if self.segments.iter().any(|s| s.name == name) {
            return Err(EX_SEG_MISS);
        }
        self.segments.push(ExportSegment { name, fields });
        Ok(())
    }

    /// 段序核：登记序与十段闭集序一致（前缀子序合法）。
    pub fn order_ok(&self) -> bool {
        self.segments
            .iter()
            .enumerate()
            .all(|(i, s)| WAVE21_SEGMENTS[i] == s.name)
    }

    /// 字段完整率核：十段全登记 + 每段完整。
    pub fn completeness(&self) -> usize {
        let mut n = 0;
        for s in self.segments.iter() {
            if s.complete() {
                n += 1;
            }
        }
        n
    }

    /// 全量导出（空账/段序漂移拒绝；封账导出允许——导出是读面）。
    pub fn export(&self) -> Result<String, u32> {
        if self.segments.is_empty() {
            return Err(EX_EMPTY_BOOK);
        }
        if !self.order_ok() {
            return Err(EX_SEG_ORDER);
        }
        let mut out = String::new();
        out.push_str("## 波次闭账材料·第 21 波\n");
        for s in self.segments.iter() {
            out.push_str(&s.render());
        }
        Ok(out)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账指纹（段序敏感复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for s in self.segments.iter() {
            for b in format!("{}|{};", s.name, s.fields.len()).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 标准夹具：十段全登记（每段两字段：状态 + 摘要）。
pub fn standard_book() -> WaveCloseBook {
    let mut b = WaveCloseBook::new();
    for name in WAVE21_SEGMENTS.iter() {
        b.put(
            name,
            vec![
                ("状态".to_string(), "闭账".to_string()),
                ("摘要".to_string(), format!("{} 完成", name)),
            ],
        )
        .expect("fixture segment");
    }
    b
}

/// 注入核：6 注入必抓（缺段/段序漂移/空账导出/封账改写/schema 漂移/段不完整）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺段（段名越闭集）必抓
    let mut b = WaveCloseBook::new();
    if b.put("w21-11-不存在的段", Vec::new()) == Err(EX_SEG_MISS) {
        caught += 1;
    }
    // 2) 段序漂移必抓（先登 02 后登 01）
    let mut b2 = WaveCloseBook::new();
    b2.put(WAVE21_SEGMENTS[1], vec![("k".to_string(), "v".to_string())]).expect("seg2");
    b2.put(WAVE21_SEGMENTS[0], vec![("k".to_string(), "v".to_string())]).expect("seg1");
    if b2.export() == Err(EX_SEG_ORDER) {
        caught += 1;
    }
    // 3) 空账导出必抓
    if b.export() == Err(EX_EMPTY_BOOK) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_b = standard_book();
    std_b.seal();
    if std_b.put(WAVE21_SEGMENTS[0], Vec::new()) == Err(EX_SEALED) {
        caught += 1;
    }
    // 5) schema 漂移必抓（十段闭集长度漂移检测——闭集应恒为 10）
    if WAVE21_SEGMENTS.len() != 10 {
        caught += 1;
    }
    // 6) 段不完整必抓（空字段段 complete 假）
    let mut b3 = WaveCloseBook::new();
    b3.put(WAVE21_SEGMENTS[0], Vec::new()).expect("empty seg");
    if b3.completeness() == 0 {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46418_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46418");
    // J1-1 导出 ×1 成功
    let b = standard_book();
    let text = b.export().expect("export");
    let ok = text.starts_with("## 波次闭账材料·第 21 波\n")
        && text.contains("# w21-01-钉版总账")
        && text.contains("# w21-10-批收官印");
    s.add("46418-J1-导出×1", ok, if ok { "" } else { "导出漂移" });
    // J1-2 schema 字段完整率 100%（十段全登记全完整）
    let ok = b.order_ok() && b.completeness() == 10 && b.segments.len() == 10;
    s.add("46418-J1-schema完整率", ok, if ok { "" } else { "schema 漂移" });
    // J1-3 导出指纹稳定
    let ok = b.fingerprint() == standard_book().fingerprint();
    s.add("46418-J1-导出指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46418-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46418_all_checks_green() {
        let set = run_unx46418_checks();
        assert!(set.all_passed(), "UNX-F46418 checks must be green: {}", set.red_items_text());
    }
}
