//! ves341_s19arch — VE-F4101 · T06 地区适配总纲
//! T06 定位（法规合规/数据驻留/内容审查/支付商业/格式深化——地区维度全治理）+ 地区码模型（BCP47 语种+地区子标签统一——与 T01 语种矩阵单源）+ 地区能力开关表（每地区一册能力集，含无障碍法规列——域本色）+ 未收录地区默认降级显性 + 能力越权拒绝。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9EC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcCode {
    Ok = 0,
    BadTag = SEG_BASE + 0x01,
    Overreach = SEG_BASE + 0x02,
    Drift = SEG_BASE + 0x03,
}

/// BCP47 解析：`language[-REGION]`；语种 2-3 位小写，地区 2 位大写。
pub fn parse_bcp47(tag: &str) -> Result<(&str, Option<&str>), RcCode> {
    let parts: Vec<&str> = tag.split('-').collect();
    if parts.is_empty() || parts.len() > 2 { return Err(RcCode::BadTag); }
    let lang = parts[0];
    if lang.is_empty() || lang.len() > 3 || !lang.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err(RcCode::BadTag);
    }
    match parts.get(1) {
        None => Ok((lang, None)),
        Some(r) => {
            if r.len() != 2 || !r.bytes().all(|b| b.is_ascii_uppercase()) { return Err(RcCode::BadTag); }
            Ok((lang, Some(r)))
        }
    }
}

/// 能力集：每地区一册（含无障碍法规列——域本色）。
#[derive(Clone)]
pub struct RegionCaps {
    pub region: &'static str,
    /// 四类能力开关（法规/驻留/审查/支付——true=有特殊规则）。
    pub law: bool,
    pub residency: bool,
    pub content: bool,
    pub payment: bool,
    /// 无障碍法规列（地区无障碍合规——域本色）。
    pub a11y_law: bool,
}

pub struct CapTable { pub entries: Vec<RegionCaps> }

impl CapTable {
    pub fn new() -> Self { CapTable { entries: Vec::new() } }

    pub fn put(&mut self, e: RegionCaps) -> Result<(), RcCode> {
        if e.region.len() != 2 { return Err(RcCode::BadTag); }
        if self.entries.iter().any(|x| x.region == e.region) { return Err(RcCode::Drift); }
        self.entries.push(e);
        Ok(())
    }

    pub fn of(&self, region: &str) -> Option<&RegionCaps> {
        self.entries.iter().find(|e| e.region == region)
    }

    /// 未收录地区 → 默认地区 + 显性标记。
    pub fn resolve(&self, region: &str, default: &'static str) -> (Option<&RegionCaps>, bool) {
        match self.of(region) {
            Some(e) => (Some(e), false),
            None => (self.of(default), true), // 显性降级标记
        }
    }

    /// 能力越权拒绝：未声明能力面却请求特殊处理 → 拒。
    pub fn claim(&self, region: &str, face: u8) -> Result<(), RcCode> {
        match self.of(region) {
            None => Err(RcCode::Overreach),
            Some(e) => {
                let ok = match face {
                    0 => e.law, 1 => e.residency, 2 => e.content, 3 => e.payment, _ => false,
                };
                if ok { Ok(()) } else { Err(RcCode::Overreach) }
            }
        }
    }
}

/// 与 T01 语种矩阵单源复述：语种子标签必须与 T01 登记一致。
pub struct LangMatrixAnchor { pub langs: Vec<&'static str> }

impl LangMatrixAnchor {
    pub fn new(langs: Vec<&'static str>) -> Self { LangMatrixAnchor { langs } }
    pub fn agrees(&self, lang: &str) -> bool { self.langs.iter().any(|l| *l == lang) }
    /// BCP47 解析出的语种必须在本矩阵（单源对拍）。
    pub fn gate(&self, tag: &str) -> Result<(), RcCode> {
        let (lang, _) = parse_bcp47(tag)?;
        if self.agrees(lang) { Ok(()) } else { Err(RcCode::BadTag) }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves341_checks() -> CheckSet {
    let mut set = CheckSet::new("ves341");

    // C341-01 BCP47 解析：语种+地区。
    chk(&mut set, "C341-01", parse_bcp47("zh-CN") == Ok(("zh", Some("CN"))), "zh-CN 双段");

    // C341-02 纯语种段。
    chk(&mut set, "C341-02", parse_bcp47("ja") == Ok(("ja", None)), "ja 单段");

    // C341-03 畸形标签族：大写语种/小写地区/三段。
    chk(&mut set, "C341-03",
        parse_bcp47("ZH-cn").is_err() && parse_bcp47("zh-cn").is_err()
        && parse_bcp47("zh-CN-x").is_err(), "三类畸形全拒");

    // C341-04 能力表登记：五列齐备。
    let mut ct = CapTable::new();
    let _ = ct.put(RegionCaps { region: "CN", law: true, residency: true, content: true, payment: true, a11y_law: true });
    let _ = ct.put(RegionCaps { region: "US", law: true, residency: false, content: true, payment: true, a11y_law: true });
    let _ = ct.put(RegionCaps { region: "XX", law: false, residency: false, content: false, payment: false, a11y_law: false });
    chk(&mut set, "C341-04", ct.entries.len() == 3, "三地区登记");

    // C341-05 重复登记 → 漂移拒。
    let dup = RegionCaps { region: "CN", law: false, residency: false, content: false, payment: false, a11y_law: false };
    chk(&mut set, "C341-05", ct.put(dup) == Err(RcCode::Drift), "重复地区 => Drift");

    // C341-06 未收录地区 → 默认 + 显性标记。
    let (caps, degraded) = ct.resolve("ZZ", "US");
    chk(&mut set, "C341-06", degraded && caps.map_or(false, |c| c.region == "US"), "ZZ 未收录 → 默认 US 显性");

    // C341-07 已收录地区不降级。
    let (_, dg2) = ct.resolve("CN", "US");
    chk(&mut set, "C341-07", !dg2, "CN 已收录不降级");

    // C341-08 能力越权拒绝：未声明驻留面请求特殊处理。
    chk(&mut set, "C341-08", ct.claim("US", 1) == Err(RcCode::Overreach), "US 无驻留面 => 拒");

    // C341-09 已声明面放行。
    chk(&mut set, "C341-09", ct.claim("CN", 1).is_ok() && ct.claim("CN", 3).is_ok(), "CN 驻留/支付面放行");

    // C341-10 无障碍法规列（域本色）：三地区两列 true。
    chk(&mut set, "C341-10", ct.entries.iter().filter(|e| e.a11y_law).count() == 2, "无障碍合规列 2/3");

    // C341-11 T01 语种矩阵单源对拍。
    let anchor = LangMatrixAnchor::new(vec!["zh", "en", "ja", "ar"]);
    chk(&mut set, "C341-11", anchor.gate("zh-CN").is_ok() && anchor.gate("ko-KR").is_err(), "zh 在矩阵 ko 不在");

    // C341-12 非法标签过单源门先拒。
    chk(&mut set, "C341-12", anchor.gate("ZH").is_err(), "非法标签单源门先拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves341_smoke() {
        let set = run_ves341_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves341 red: {:?}", red);
    }

    #[test]
    fn ves341_bcp47() {
        assert_eq!(parse_bcp47("en-US"), Ok(("en", Some("US"))));
        assert!(parse_bcp47("").is_err());
        assert!(parse_bcp47("e-U").is_err());
    }
}
