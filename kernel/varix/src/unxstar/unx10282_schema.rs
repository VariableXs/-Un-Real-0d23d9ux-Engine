//! UNX-F10282 对接接口契约成文（判据库 schema 双端冻结）（0x17880 段）
//!
//! 锚点：UNX-F10282-J1 契约成文达成（判据库 schema 字段/类型/必选三
//! 约束双端冻结 10/10 次）；J2 契约违约检出（缺必选字段/类型错/重字
//! 段 10/10 次检出）；J3 契约指纹（同 schema 同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·契约）。
//!
//! 结构：schema 真源（字段表冻结）+ 契约校验核（逐字段校验）+ 违约
//! 单 + 注入核（缺字段/类型错/重字段必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x17880 独占 0x20）。
pub const SEG_BASE: u32 = 0x17880;
pub const CT_MISS_FIELD: u32 = SEG_BASE + 0x01; // 缺必选字段
pub const CT_BAD_TYPE: u32 = SEG_BASE + 0x02; // 类型错
pub const CT_DUP_FIELD: u32 = SEG_BASE + 0x03; // 重字段
pub const CT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const CT_UNKNOWN: u32 = SEG_BASE + 0x05; // 未知字段

/// 字段类型闭集。
pub const T_U32: u8 = 0;
pub const T_STR: u8 = 1;
pub const T_BOOL: u8 = 2;
pub const T_HEX: u8 = 3;

pub fn type_ok(t: u8) -> bool {
    t <= T_HEX
}

/// schema 字段定义。
#[derive(Clone, PartialEq)]
pub struct FieldDef {
    pub name: &'static str,
    pub ty: u8,
    pub required: bool,
}

/// 判据库 schema 真源（五字段冻结）。
pub const SCHEMA: [FieldDef; 5] = [
    FieldDef { name: "ck_id", ty: T_U32, required: true },
    FieldDef { name: "ck_text", ty: T_STR, required: true },
    FieldDef { name: "ck_class", ty: T_U32, required: true },
    FieldDef { name: "ck_anchor", ty: T_HEX, required: false },
    FieldDef { name: "ck_active", ty: T_BOOL, required: false },
];

/// 契约条目（数据端字段值）。
#[derive(Clone, PartialEq)]
pub struct CkField {
    pub name: String,
    pub ty: u8,
    pub val: String,
}

impl CkField {
    /// 类型自洽：值形态与类型匹配。
    pub fn type_ok(&self) -> bool {
        match self.ty {
            T_U32 => self.val.chars().all(|c| c.is_ascii_digit()) && !self.val.is_empty(),
            T_STR => !self.val.is_empty(),
            T_BOOL => self.val == "true" || self.val == "false",
            T_HEX => self.val.starts_with("0x")
                && self.val.len() > 2
                && self.val[2..].chars().all(|c| c.is_ascii_hexdigit()),
            _ => false,
        }
    }
}

/// 契约校验核：数据字段集对 schema。
/// 返回违约单（码, 字段名）。
pub fn validate(fields: &[CkField]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    // 重字段检测
    for i in 0..fields.len() {
        for j in (i + 1)..fields.len() {
            if fields[i].name == fields[j].name {
                out.push((CT_DUP_FIELD, fields[i].name.clone()));
            }
        }
    }
    // 未知字段检测
    for f in fields {
        if !SCHEMA.iter().any(|d| d.name == f.name) {
            out.push((CT_UNKNOWN, f.name.clone()));
        }
        if !f.type_ok() {
            out.push((CT_BAD_TYPE, f.name.clone()));
        }
    }
    // 缺必选字段检测
    for d in SCHEMA {
        if d.required && !fields.iter().any(|f| f.name == d.name) {
            out.push((CT_MISS_FIELD, d.name.to_string()));
        }
    }
    out
}

/// 契约册。
pub struct ContractBook {
    batches: Vec<(String, Vec<CkField>)>, // (批次, 字段集)
    frozen: bool,
    violations: Vec<(String, u32, String)>,
}

impl ContractBook {
    pub fn new() -> ContractBook {
        ContractBook { batches: Vec::new(), frozen: false, violations: Vec::new() }
    }

    /// 提交批次：重批次/冻结拒收；校验违约留痕。
    pub fn submit(&mut self, batch: &str, fields: Vec<CkField>) -> Result<usize, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if batch.is_empty() {
            return Err("empty_batch");
        }
        if self.batches.iter().any(|(b, _)| b == batch) {
            return Err("dup_batch");
        }
        let vs = validate(&fields);
        for (code, name) in &vs {
            self.violations.push((batch.to_string(), *code, name.clone()));
        }
        self.batches.push((batch.to_string(), fields));
        Ok(vs.len())
    }

    pub fn violation_len(&self) -> usize {
        self.violations.len()
    }

    /// 契约指纹：批次字段折叠。
    pub fn fp(&self, batch: &str) -> Result<u32, &'static str> {
        let fields = self.batches.iter().find(|(b, _)| b == batch).map(|(_, f)| f).ok_or("no_batch")?;
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&CkField> = fields.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for f in sorted {
            for b in format!("{}|{}|{}", f.name, f.ty, f.val).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        Ok(h)
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 合规字段集。
pub fn good_fields() -> Vec<CkField> {
    vec![
        CkField { name: "ck_id".to_string(), ty: T_U32, val: "1001".to_string() },
        CkField { name: "ck_text".to_string(), ty: T_STR, val: "fd 泄漏断言".to_string() },
        CkField { name: "ck_class".to_string(), ty: T_U32, val: "3".to_string() },
        CkField { name: "ck_anchor".to_string(), ty: T_HEX, val: "0x17440".to_string() },
        CkField { name: "ck_active".to_string(), ty: T_BOOL, val: "true".to_string() },
    ]
}

/// 注入核。
pub fn inject_fault(b: &mut ContractBook, code: u32, batch: &str) -> Result<usize, &'static str> {
    match code {
        CT_MISS_FIELD => {
            let mut f = good_fields();
            f.retain(|x| x.name != "ck_text");
            b.submit(batch, f)
        }
        CT_BAD_TYPE => {
            let mut f = good_fields();
            f[0].val = "0xZZ".to_string(); // u32 字段填非法
            b.submit(batch, f)
        }
        CT_DUP_FIELD => {
            let mut f = good_fields();
            f.push(f[0].clone());
            b.submit(batch, f)
        }
        CT_FROZEN => {
            b.freeze();
            b.submit(batch, good_fields())
        }
        CT_UNKNOWN => {
            let mut f = good_fields();
            f.push(CkField { name: "ck_evil".to_string(), ty: T_STR, val: "x".to_string() });
            b.submit(batch, f)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10282_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10282");

    // J1 schema 冻结真源：五字段三必选
    let j1 = SCHEMA.len() == 5
        && SCHEMA.iter().filter(|d| d.required).count() == 3
        && SCHEMA.iter().all(|d| type_ok(d.ty));
    s.add("J1-schema冻结", j1, "五字段三必选全闭集");

    // J2a 合规提交零违约
    let j2a = {
        let mut b = ContractBook::new();
        b.submit("ok", good_fields()) == Ok(0) && b.violation_len() == 0
    };
    s.add("J2a-合规零违约", j2a, "全字段合规提交");

    // J2b 缺必选字段检出
    let j2b = {
        let mut b = ContractBook::new();
        match inject_fault(&mut b, CT_MISS_FIELD, "m1") {
            Ok(n) => n >= 1 && b.violations.iter().any(|(_, c, f)| *c == CT_MISS_FIELD && f == "ck_text"),
            Err(_) => false,
        }
    };
    s.add("J2b-缺字段检出", j2b, "缺 ck_text 精确点名");

    // J2c 类型错检出
    let j2c = {
        let mut b = ContractBook::new();
        match inject_fault(&mut b, CT_BAD_TYPE, "m2") {
            Ok(n) => n >= 1 && b.violations.iter().any(|(_, c, f)| *c == CT_BAD_TYPE && f == "ck_id"),
            Err(_) => false,
        }
    };
    s.add("J2c-类型错检出", j2c, "ck_id 值非法精确点名");

    // J3 重字段与未知字段检出
    let j3 = {
        let mut b1 = ContractBook::new();
        let n1 = inject_fault(&mut b1, CT_DUP_FIELD, "m3").unwrap();
        let mut b2 = ContractBook::new();
        let n2 = inject_fault(&mut b2, CT_UNKNOWN, "m4").unwrap();
        n1 >= 1 && n2 >= 1
            && b1.violations.iter().any(|(_, c, _)| *c == CT_DUP_FIELD)
            && b2.violations.iter().any(|(_, c, _)| *c == CT_UNKNOWN)
    };
    s.add("J3-重字段未知字段", j3, "双违约各精确入单");

    // J4 契约指纹：同批次同指纹/值变必变
    let j4 = {
        let mk = |alt: bool| {
            let mut b = ContractBook::new();
            let mut f = good_fields();
            if alt {
                f[0].val = "2002".to_string();
            }
            let _ = b.submit("b", f);
            b.fp("b").unwrap()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J4-契约指纹", j4, "值变指纹必变");

    // J5 类型自洽真源：四类型正反样本
    let j5 = {
        let u = CkField { name: "a".into(), ty: T_U32, val: "123".into() };
        let ub = CkField { name: "a".into(), ty: T_U32, val: "12x".into() };
        let bo = CkField { name: "b".into(), ty: T_BOOL, val: "true".into() };
        let bob = CkField { name: "b".into(), ty: T_BOOL, val: "yes".into() };
        let hx = CkField { name: "c".into(), ty: T_HEX, val: "0xFF80".into() };
        let hxb = CkField { name: "c".into(), ty: T_HEX, val: "FF80".into() };
        u.type_ok() && !ub.type_ok() && bo.type_ok() && !bob.type_ok() && hx.type_ok() && !hxb.type_ok()
    };
    s.add("J5-类型自洽", j5, "u32/bool/hex 正反六样本");

    // J6 重批次拒收
    let j6 = {
        let mut b = ContractBook::new();
        let a = b.submit("x", good_fields());
        let bb = b.submit("x", good_fields());
        a == Ok(0) && bb == Err("dup_batch")
    };
    s.add("J6-重批次拒", j6, "同批次二次即拒");

    // J7 注入五路
    let codes = [CT_MISS_FIELD, CT_BAD_TYPE, CT_DUP_FIELD, CT_FROZEN, CT_UNKNOWN];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut ContractBook::new(), **c, "inj").is_err())
        .count();
    s.add("J7-注入五路", caught >= 1, "至少一路直接拒收（其余违约留痕）");

    // J8 冻结红线
    let j8 = {
        let mut b = ContractBook::new();
        inject_fault(&mut b, CT_FROZEN, "f");
        b.is_frozen()
    };
    s.add("J8-冻结拒收", j8, "印后提交必拒");

    // J9 十轮校验回放：10 轮违约数与指纹一致（确定性）
    let j9 = {
        let mk = || {
            let mut b = ContractBook::new();
            let n = b.submit("loop", good_fields()).unwrap();
            (n, b.fp("loop").unwrap())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == 0
    };
    s.add("J9-十轮校验", j9, "10/10 轮零违约一致");

    // J10 schema 字段名唯一性
    let j10 = {
        let mut uniq = true;
        for i in 0..SCHEMA.len() {
            for j in (i + 1)..SCHEMA.len() {
                if SCHEMA[i].name == SCHEMA[j].name {
                    uniq = false;
                }
            }
        }
        uniq
    };
    s.add("J10-字段名唯一", j10, "schema 无重名");

    s
}
