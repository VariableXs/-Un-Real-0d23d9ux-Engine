//! ves347_s19addr — VE-F4107 · T06 地址电话格式
//! 地址表单地区化（字段集×地区：邮编格式/省州层级/姓名顺序）+ 电话格式（区号/分隔/校验位）+ 校验器宽松校验+实时格式化（不硬拒绝合法变体）+ 未知地区国际化通用表单+标记 + 地址数据不用于画像（红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    Ok = 0,
    UnknownRegion = SEG_BASE + 0x01,
    Rejected = SEG_BASE + 0x02,
}

/// 地址字段集×地区。
#[derive(Clone)]
pub struct AddrSchema {
    pub region: &'static str,
    /// 字段序（"given-family" 或 "family-given"）。
    pub name_order: &'static str,
    /// 省州层级名（state/province/prefecture）。
    pub level_name: &'static str,
    /// 邮编格式（"NNNNN" / "NNNNNN" / "ANA NAL" 等）。
    pub zip_fmt: &'static str,
}

pub const GENERIC_SCHEMA: AddrSchema = AddrSchema { region: "*", name_order: "given-family", level_name: "region", zip_fmt: "*" };

pub struct AddrBook { pub schemas: Vec<AddrSchema> }

impl AddrBook {
    pub fn new() -> Self { AddrBook { schemas: Vec::new() } }

    pub fn put(&mut self, s: AddrSchema) {
        if !self.schemas.iter().any(|x| x.region == s.region) { self.schemas.push(s); }
    }

    /// 取 schema：未知地区 → 国际化通用表单 + 标记。
    pub fn schema_of(&self, region: &str) -> (&AddrSchema, bool) {
        match self.schemas.iter().find(|s| s.region == region) {
            Some(s) => (s, false),
            None => (&GENERIC_SCHEMA_PLACEHOLDER, true),
        }
    }
}

static GENERIC_SCHEMA_PLACEHOLDER: AddrSchema = GENERIC_SCHEMA;

/// 邮编宽松校验：按 schema 格式核对（A=字母 N=数字 空格与连字符按字面）；格式为 "*" 全放行。
pub fn zip_ok(zip: &str, fmt: &str) -> bool {
    if fmt == "*" { return true; }
    if zip.len() != fmt.len() { return false; }
    for (c, f) in zip.chars().zip(fmt.chars()) {
        let ok = match f {
            'N' => c.is_ascii_digit(),
            'A' => c.is_ascii_alphabetic(),
            ' ' | '-' => c == f,
            _ => false,
        };
        if !ok { return false; }
    }
    true
}

/// 电话规则：地区 →（国际区号, 分隔符, 号码位数）。
#[derive(Clone, Copy)]
pub struct PhoneRule { pub dial: &'static str, pub sep: char, pub digits: usize }

pub fn phone_rule_of(region: &str) -> PhoneRule {
    match region {
        "CN" => PhoneRule { dial: "+86", sep: '-', digits: 11 },
        "US" => PhoneRule { dial: "+1", sep: '-', digits: 10 },
        "JP" => PhoneRule { dial: "+81", sep: '-', digits: 10 },
        _ => PhoneRule { dial: "+", sep: ' ', digits: 0 }, // 未知地区不设限
    }
}

/// 电话实时格式化：去非数字 → 按位数分组（3-4-余）→ 失败原文保留。
pub fn fmt_phone(raw: &str, rule: &PhoneRule) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if rule.digits == 0 || digits.len() != rule.digits {
        return String::from(raw); // 保留原文 + 调用方提示
    }
    let b = digits.as_bytes();
    let mut out = String::from(rule.dial);
    out.push(' ');
    if b.len() > 7 {
        out.push_str(&digits[0..3]);
        out.push(rule.sep);
        out.push_str(&digits[3..7]);
        out.push(rule.sep);
        out.push_str(&digits[7..]);
    } else {
        out.push_str(&digits);
    }
    out
}

/// 宽松校验器：只拒明确违规（位数不符且非未知地区），合法变体不硬拒。
pub fn lax_phone_check(raw: &str, region: &str) -> Result<(), AdCode> {
    let rule = phone_rule_of(region);
    if rule.digits == 0 { return Ok(()); } // 未知地区放行
    let digits = raw.chars().filter(|c| c.is_ascii_digit()).count();
    if digits != rule.digits { return Err(AdCode::Rejected); }
    Ok(())
}

/// 画像红线：地址字段白名单外字段不入画像库。
pub fn profile_safe(fields: &[&str]) -> bool {
    !fields.iter().any(|f| *f == "home-geo" || *f == "movement")
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves347_checks() -> CheckSet {
    let mut set = CheckSet::new("ves347");
    let mut ab = AddrBook::new();

    // C347-01 字段集×地区：姓名序/层级/邮编三列。
    ab.put(AddrSchema { region: "CN", name_order: "family-given", level_name: "province", zip_fmt: "NNNNNN" });
    ab.put(AddrSchema { region: "US", name_order: "given-family", level_name: "state", zip_fmt: "NNNNN" });
    ab.put(AddrSchema { region: "GB", name_order: "given-family", level_name: "county", zip_fmt: "ANA NAL" });
    chk(&mut set, "C347-01", ab.schemas.len() == 3, "三地区字段集");

    // C347-02 姓名序差异。
    chk(&mut set, "C347-02",
        ab.schema_of("CN").0.name_order == "family-given" && ab.schema_of("US").0.name_order == "given-family", "姓名序地区差异");

    // C347-03 邮编宽松校验：CN 六位。
    chk(&mut set, "C347-03", zip_ok("100089", "NNNNNN") && !zip_ok("10008", "NNNNNN"), "CN 六位过 五位拒");

    // C347-04 字母邮编（GB）。
    chk(&mut set, "C347-04", zip_ok("EC1A 1BB", "ANA NAL") && !zip_ok("EC11 1BB", "ANA NAL"), "GB 字母数字位型");

    // C347-05 未知地区 → 通用表单 + 标记。
    let (s, marked) = ab.schema_of("ZZ");
    chk(&mut set, "C347-05", marked && s.region == "*" && s.zip_fmt == "*", "ZZ → 通用表单显性");

    // C347-06 通用表单邮编全放行。
    chk(&mut set, "C347-06", zip_ok("anything", "*"), "通用型全放行");

    // C347-07 电话规则单源。
    chk(&mut set, "C347-07", phone_rule_of("CN").digits == 11 && phone_rule_of("CN").dial == "+86", "CN 规则单源");

    // C347-08 实时格式化。
    chk(&mut set, "C347-08", fmt_phone("13812345678", &phone_rule_of("CN")) == "+86 138-1234-5678", "CN 手机实时分组");

    // C347-09 变体输入（带空格横线）同结果。
    chk(&mut set, "C347-09", fmt_phone("138 1234-5678", &phone_rule_of("CN")) == "+86 138-1234-5678", "变体输入同格式");

    // C347-10 位数不符 → 原文保留。
    let raw = "12345";
    chk(&mut set, "C347-10", fmt_phone(raw, &phone_rule_of("CN")) == raw, "失败原文保留");

    // C347-11 宽松校验：位数不符拒、未知地区放行。
    chk(&mut set, "C347-11",
        lax_phone_check("12345", "CN") == Err(AdCode::Rejected) && lax_phone_check("anything", "ZZ").is_ok(), "位数不符拒 未知放行");

    // C347-12 画像红线：地理追踪字段不入画像。
    chk(&mut set, "C347-12", profile_safe(&["zip", "city"]) && !profile_safe(&["home-geo", "zip"]), "地理追踪字段被拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves347_smoke() {
        let set = run_ves347_checks();
        assert!(set.failed_count() == 0, "ves347 red: {}", set.red_items_text());
    }

    #[test]
    fn ves347_zip() {
        assert!(zip_ok("12345", "NNNNN"));
        assert!(!zip_ok("12A45", "NNNNN"));
        assert!(zip_ok("A1B 2C3", "ANA NAL"));
    }
}
