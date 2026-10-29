//! UNX-F56096 采集记录 schema 冻结件（O1 对下冻结接口①）（0x60BE0 段）
//!
//! 锚点：UNX-F56096-J1 schema 冻结全量（记录字段集逐字段落契：字段名 ×
//! 类型闭集 × 序号三元组）；J2 schema 对下冻结语义（字段集与序号不可
//! 变，重落契必须逐字段一致，漂移即拒）；J3 schema 审计链（schema 指纹、
//! 封账只读、字段闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·O1 冻结面）。
//!
//! 结构：字段类型闭集（u32/u64/string/bool 四型）+ schema 字段登记册
//! （字段名 → 类型 × 序号逐字段落契）+ 对下冻结核（schema 指纹固化、
//! 重落契逐字段比对）+ 序号连续核（1..n 无断档）+ 封账语义 + 注入核
//! （空字段名/未知类型/序号断档/重复字段/漂移落契/封账改写/空契校验
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60BE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60BE0;
pub const SC_NAME_EMPTY: u32 = SEG_BASE + 0x01; // 字段名为空
pub const SC_TYPE_UNK: u32 = SEG_BASE + 0x02; // 未知类型（四闭集外）
pub const SC_SEQ_GAP: u32 = SEG_BASE + 0x03; // 序号断档（非 1..n）
pub const SC_FIELD_DUP: u32 = SEG_BASE + 0x04; // 重复字段名
pub const SC_DRIFT: u32 = SEG_BASE + 0x05; // 漂移落契（与冻结 schema 不符）
pub const SC_SEALED: u32 = SEG_BASE + 0x06; // schema 已封账拒改
pub const SC_EMPTY: u32 = SEG_BASE + 0x07; // 空契校验

/// 字段类型闭集。
pub const T_U32: usize = 0;
pub const T_U64: usize = 1;
pub const T_STRING: usize = 2;
pub const T_BOOL: usize = 3;
pub const T_COUNT: usize = 4;
pub const T_NAMES: [&str; T_COUNT] = ["u32", "u64", "string", "bool"];

/// schema 字段：字段名 × 类型 × 序号。
#[derive(Clone)]
pub struct SchemaField {
    pub name: String,
    pub ty: usize,
    pub seq: u32,
}

/// schema 总契。
pub struct SchemaBook {
    pub fields: Vec<SchemaField>,
    pub sealed: bool,
    /// 冻结指纹（首次封账固化）。
    pub frozen_fp: Option<u32>,
}

impl SchemaBook {
    pub fn new() -> Self {
        SchemaBook { fields: Vec::new(), sealed: false, frozen_fp: None }
    }

    /// 落契：空名拒 + 类型闭集 + 序号连续 + 重复拒四闸。
    pub fn define(&mut self, name: &str, ty: usize, seq: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if name.is_empty() {
            return Err(SC_NAME_EMPTY);
        }
        if ty >= T_COUNT {
            return Err(SC_TYPE_UNK);
        }
        if seq != (self.fields.len() + 1) as u32 {
            return Err(SC_SEQ_GAP);
        }
        for f in self.fields.iter() {
            if f.name == name {
                return Err(SC_FIELD_DUP);
            }
        }
        self.fields.push(SchemaField { name: name.to_string(), ty, seq });
        Ok(())
    }

    /// schema 指纹（逐字段复合，冻结口径）。
    pub fn schema_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for f in self.fields.iter() {
            let line = format!("{}|{}|{}", f.name, T_NAMES[f.ty], f.seq);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }

    /// J2：对下冻结（封账固化指纹；此后重落契按指纹比对）。
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if self.fields.is_empty() {
            return Err(SC_EMPTY);
        }
        let fp = self.schema_fingerprint();
        self.frozen_fp = Some(fp);
        self.sealed = true;
        Ok(fp)
    }

    /// 对下校验：给定字段集（名|型 序列）与冻结 schema 逐字段比对。
    pub fn validate_against(&self, names: &[&str], types: &[usize]) -> Result<bool, u32> {
        if self.fields.is_empty() {
            return Err(SC_EMPTY);
        }
        if names.len() != types.len() || names.len() != self.fields.len() {
            return Ok(false);
        }
        for i in 0..self.fields.len() {
            if self.fields[i].name != names[i] {
                return Ok(false);
            }
            if self.fields[i].ty >= T_COUNT || self.fields[i].ty != types[i] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// J1：字段集全量（逐字段合法 + 序号 1..n 连续）。
    pub fn fields_ok(&self) -> bool {
        if self.fields.is_empty() {
            return false;
        }
        for (i, f) in self.fields.iter().enumerate() {
            if f.name.is_empty() || f.ty >= T_COUNT || f.seq != (i + 1) as u32 {
                return false;
            }
        }
        true
    }

    /// J2：冻结指纹复核（封账后指纹与固化值一致）。
    pub fn frozen_ok(&self) -> bool {
        match self.frozen_fp {
            None => false,
            Some(fp) => fp == self.schema_fingerprint(),
        }
    }
}

/// 标准夹具：采集记录八字段（O1 对下冻结口径）。
pub fn standard_schema() -> SchemaBook {
    let mut b = SchemaBook::new();
    let plan: [(&str, usize); 8] = [
        ("rec_id", T_U64),
        ("env_hash", T_STRING),
        ("wave", T_U32),
        ("window", T_U32),
        ("metric", T_STRING),
        ("value", T_U64),
        ("unit", T_STRING),
        ("side", T_BOOL),
    ];
    for (i, p) in plan.iter().enumerate() {
        if b.define(p.0, p.1, (i + 1) as u32).is_err() {
            panic!("standard fixture define must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空字段名/未知类型/序号断档/重复字段/封账改契/空契校验/漂移落契）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空字段名
    let mut b = SchemaBook::new();
    if b.define("", T_U32, 1) == Err(SC_NAME_EMPTY) {
        caught += 1;
    }
    // 2) 未知类型
    if b.define("k", 9, 1) == Err(SC_TYPE_UNK) {
        caught += 1;
    }
    // 3) 序号断档（第 2 字段序号给 5）
    if b.define("k", T_U32, 1).is_ok() && b.define("k2", T_U32, 5) == Err(SC_SEQ_GAP) {
        caught += 1;
    }
    // 4) 重复字段
    if b.define("dup", T_U32, 2).is_ok() && b.define("dup", T_U64, 3) == Err(SC_FIELD_DUP) {
        caught += 1;
    }
    // 5) 空契校验
    if SchemaBook::new().validate_against(&["x"], &[T_U32]).err() == Some(SC_EMPTY) {
        caught += 1;
    }
    // 6) 漂移落契（封账后改契拒）
    let mut sb = standard_schema();
    let _ = sb.freeze();
    if sb.define("extra", T_BOOL, 9) == Err(SC_SEALED) {
        caught += 1;
    }
    // 7) 封账后重复冻结拒
    if sb.freeze() == Err(SC_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// schema 渲染导出（逐字段一行）。
pub fn render_schema(b: &SchemaBook) -> String {
    let mut out = String::new();
    for f in b.fields.iter() {
        out.push_str(&format!("{}|{}|{}\n", f.seq, f.name, T_NAMES[f.ty]));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行三段 + 类型闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 3 {
            return false;
        }
        let mut hit = false;
        for t in T_NAMES.iter() {
            if parts[2] == *t {
                hit = true;
            }
        }
        hit
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56096_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56096");
    // J1-1 schema 全量（八字段逐字段合法）
    let b = standard_schema();
    let ok = b.fields_ok() && b.fields.len() == 8;
    s.add("56096-J1-schema全量", ok, if ok { "" } else { "字段残缺" });
    // J1-2 类型闭集齐
    let ok = T_NAMES.len() == T_COUNT && T_NAMES[0] == "u32" && T_NAMES[3] == "bool";
    s.add("56096-J1-类型闭集齐", ok, if ok { "" } else { "类型漂移" });
    // J1-3 序号连续（1..8）
    let ok = b.fields[0].seq == 1 && b.fields[7].seq == 8;
    s.add("56096-J1-序号连续", ok, if ok { "" } else { "序号断档" });
    // J2-1 对下冻结（封账固化指纹）
    let mut b2 = standard_schema();
    let fp = b2.freeze();
    let ok = match fp {
        Ok(fp) => b2.sealed && b2.frozen_ok() && fp == b2.schema_fingerprint(),
        Err(_) => false,
    };
    s.add("56096-J2-对下冻结", ok, if ok { "" } else { "冻结语义漂移" });
    // J2-2 对下校验命中（同字段集逐字段一致）
    let names = ["rec_id", "env_hash", "wave", "window", "metric", "value", "unit", "side"];
    let types = [T_U64, T_STRING, T_U32, T_U32, T_STRING, T_U64, T_STRING, T_BOOL];
    let ok = b2.validate_against(&names, &types) == Ok(true);
    s.add("56096-J2-对下校验命中", ok, if ok { "" } else { "校验漂移" });
    // J2-3 对下校验报警（字段漂移）
    let mut names2 = ["rec_id", "env_hash", "wave", "window", "metric", "value", "unit", "extra"];
    let ok = b2.validate_against(&names2, &types) == Ok(false);
    s.add("56096-J2-对下校验报警", ok, if ok { "" } else { "漂移漏抓" });
    // J2-4 对下校验报警（类型漂移）
    let mut types2 = [T_U32, T_STRING, T_U32, T_U32, T_STRING, T_U64, T_STRING, T_BOOL];
    let ok = ok && b2.validate_against(&names, &types2) == Ok(false);
    let _ = &mut names2;
    s.add("56096-J2-类型漂移报警", ok, if ok { "" } else { "类型漏抓" });
    // J3-1 schema 指纹确定性
    let ok = standard_schema().schema_fingerprint() == standard_schema().schema_fingerprint();
    s.add("56096-J3-指纹确定性", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 指纹敏感（字段序变指纹变）
    let mut b3 = standard_schema();
    b3.fields[0].seq = 2;
    let ok = b3.schema_fingerprint() != standard_schema().schema_fingerprint();
    s.add("56096-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-3 空契冻结拒
    let ok = SchemaBook::new().freeze() == Err(SC_EMPTY);
    s.add("56096-J3-空契冻结拒", ok, if ok { "" } else { "空契穿透" });
    // J3-4 封账改契拒
    let ok = b2.define("late", T_U32, 9) == Err(SC_SEALED);
    s.add("56096-J3-封账改契拒", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出八行齐
    let text = render_schema(&standard_schema());
    let ok = render_complete(&text, 8) && text.starts_with("1|rec_id|u64");
    s.add("56096-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56096-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56096_all_checks_green() {
        let set = run_unx56096_checks();
        assert!(set.all_passed(), "UNX-F56096 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56096_fingerprint_stable() {
        assert_eq!(standard_schema().schema_fingerprint(), standard_schema().schema_fingerprint());
    }
}
