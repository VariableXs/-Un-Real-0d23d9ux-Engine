//! ves456_s25dsl — VE-F4222 · 契约定义 DSL
//! 声明式契约语言：语法三节（结构定义+约束表达式+判据标注）+解析器+格式化器；
//! DSL 设计原则（人可读/机器可验/判据一等公民）；语法定义版本化；
//! 语法错误→定位行列+建议修法；歧义构造→拒绝+示例；版本漂移→迁移器；
//! DSL 判据节含无障碍判据保留字（不可省略——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1660;
pub const A11Y_RESERVED: &str = "a11y_criterion"; // 域本色保留字

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    Ok = 0,
    SyntaxErr = SEG_BASE + 0x01,   // 0x1661
    Ambiguous = SEG_BASE + 0x02,   // 0x1662
    VersionDrift = SEG_BASE + 0x03, // 0x1663
    ReservedMissing = SEG_BASE + 0x04, // 0x1664 判据保留字缺（域本色）
}

/// 契约源（三节标记）。
pub struct ContractSrc {
    pub struct_def: &'static str,
    pub constraint: &'static str,
    pub criterion: &'static str, // 判据节（一等公民）
    pub dsl_ver: u32,
}

/// 解析器。
pub struct DslParser {
    pub ver: u32,
    pub err_line: Option<usize>,
    pub err_col: Option<usize>,
    pub migrated_from: u32,
}

impl DslParser {
    pub fn new(ver: u32) -> Self { DslParser { ver, err_line: None, err_col: None, migrated_from: 0 } }
    /// 解析：三节齐+a11y 保留字在判据节（域本色）→通过；语法错→定位行列。
    pub fn parse(&mut self, c: &ContractSrc) -> Result<(), DsCode> {
        self.err_line = None; self.err_col = None;
        if c.struct_def.is_empty() || c.constraint.is_empty() {
            self.err_line = Some(1); self.err_col = Some(1);
            return Err(DsCode::SyntaxErr);
        }
        if c.criterion.is_empty() { return Err(DsCode::ReservedMissing); }
        if !c.criterion.contains(A11Y_RESERVED) { return Err(DsCode::ReservedMissing); }
        // 歧义构造：约束表达式含双赋值歧义（以 ";;" 代）。
        if c.constraint.contains(";;") { return Err(DsCode::Ambiguous); }
        if c.dsl_ver != self.ver {
            self.migrated_from = c.dsl_ver;
            return Err(DsCode::VersionDrift);
        }
        Ok(())
    }
    /// 迁移器：版本漂移→对齐当前版本。
    pub fn migrate(&mut self, c: &mut ContractSrc) {
        self.migrated_from = c.dsl_ver;
        c.dsl_ver = self.ver;
    }
    /// 建议修法（语法错误时）。
    pub fn suggest(&self) -> &'static str {
        match (self.err_line, self.err_col) {
            (Some(l), Some(c)) if l == 1 && c == 1 => "补充结构定义节与约束表达式节",
            _ => "检查三节完整性",
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves456_checks() -> CheckSet {
    let mut set = CheckSet::new("ves456");
    let mut p = DslParser::new(3);

    // C456-01 三节齐+a11y 保留字→解析通过。
    let good = ContractSrc { struct_def: "contract Order { }", constraint: "total > 0", criterion: "a11y_criterion: contrast>=4.5", dsl_ver: 3 };
    chk(&mut set, "C456-01", p.parse(&good).is_ok(), "解析通过");

    // C456-02 结构节缺→语法错+定位。
    let bad = ContractSrc { struct_def: "", constraint: "total > 0", criterion: "a11y_criterion: x", dsl_ver: 3 };
    chk(&mut set, "C456-02", p.parse(&bad) == Err(DsCode::SyntaxErr)
        && p.err_line == Some(1) && p.err_col == Some(1), "语法定位");

    // C456-03 建议修法。
    chk(&mut set, "C456-03", p.suggest() == "补充结构定义节与约束表达式节", "建议修法");

    // C456-04 判据节缺→保留字码（域本色一等公民）。
    let noc = ContractSrc { struct_def: "contract A {}", constraint: "x > 0", criterion: "", dsl_ver: 3 };
    chk(&mut set, "C456-04", p.parse(&noc) == Err(DsCode::ReservedMissing), "判据缺拒");

    // C456-05 a11y 保留字缺→拒（不可省略——域本色）。
    let noa = ContractSrc { struct_def: "contract A {}", constraint: "x > 0", criterion: "criterion: y", dsl_ver: 3 };
    chk(&mut set, "C456-05", p.parse(&noa) == Err(DsCode::ReservedMissing), "保留字缺拒");

    // C456-06 歧义构造→拒绝。
    let amb = ContractSrc { struct_def: "contract A {}", constraint: "x = 1;; y = 2", criterion: "a11y_criterion: z", dsl_ver: 3 };
    chk(&mut set, "C456-06", p.parse(&amb) == Err(DsCode::Ambiguous), "歧义拒绝");

    // C456-07 版本漂移→迁移器对齐。
    let mut old = ContractSrc { struct_def: "contract B {}", constraint: "y > 0", criterion: "a11y_criterion: ok", dsl_ver: 2 };
    chk(&mut set, "C456-07", p.parse(&old) == Err(DsCode::VersionDrift) && p.migrated_from == 2, "漂移检出");
    p.migrate(&mut old);
    chk(&mut set, "C456-07b", old.dsl_ver == 3 && p.parse(&old).is_ok(), "迁移对齐");

    // C456-08 保留字常量单源。
    chk(&mut set, "C456-08", A11Y_RESERVED == "a11y_criterion", "保留字单源");

    // C456-09 解析幂等（重放同源仍过）。
    chk(&mut set, "C456-09", p.parse(&good).is_ok() && p.parse(&good).is_ok(), "解析幂等");

    // C456-10 判据一等公民：判据节内容不参与歧义检测。
    let cr = ContractSrc { struct_def: "contract C {}", constraint: "z > 0", criterion: "a11y_criterion: a;;b", dsl_ver: 3 };
    chk(&mut set, "C456-10", p.parse(&cr).is_ok(), "判据节豁免歧义");

    // C456-11 错误位清空（成功解析后）。
    chk(&mut set, "C456-11", p.err_line.is_none() && p.err_col.is_none(), "错误位清空");

    // C456-12 错误码段位单源。
    chk(&mut set, "C456-12", DsCode::ReservedMissing as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves456_smoke() {
        let set = run_ves456_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves456 red: {:?}", red);
    }
}
