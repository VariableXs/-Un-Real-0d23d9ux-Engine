//! ves343_s19residency — VE-F4103 · T06 数据驻留与隐私地区
//! 驻留三档（驻留地白名单/跨境传输禁令/本地加密要求）+ 存储路由器（按用户地区路由数据落点——不可绕过红线）+ 驻留变更迁移协议（断点续迁）+ 无白名单落点本地暂存告警 + 跨境禁令拒绝立案。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xAEC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    Ok = 0,
    NoWhitelisted = SEG_BASE + 0x01,
    CrossBorder = SEG_BASE + 0x02,
    BypassAttempt = SEG_BASE + 0x03,
}

/// 驻留三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResidencyTier { Whitelist = 0, NoCrossBorder = 1, LocalEncrypt = 2 }

impl ResidencyTier {
    pub const ALL: [ResidencyTier; 3] = [
        ResidencyTier::Whitelist, ResidencyTier::NoCrossBorder, ResidencyTier::LocalEncrypt];
    pub fn name(self) -> &'static str {
        match self { ResidencyTier::Whitelist => "whitelist",
            ResidencyTier::NoCrossBorder => "no-cross-border", ResidencyTier::LocalEncrypt => "local-encrypt" }
    }
}

/// 地区驻留策略：白名单落点 + 档位。
#[derive(Clone)]
pub struct ResidencyPolicy { pub region: &'static str, pub tier: ResidencyTier, pub whitelisted: Vec<&'static str> }

pub struct ResidencyBook { pub policies: Vec<ResidencyPolicy> }

impl ResidencyBook {
    pub fn new() -> Self { ResidencyBook { policies: Vec::new() } }

    pub fn put(&mut self, p: ResidencyPolicy) { self.policies.push(p); }

    pub fn policy_of(&self, region: &str) -> Option<&ResidencyPolicy> {
        self.policies.iter().find(|p| p.region == region)
    }
}

/// 路由器：按用户地区路由数据落点——驻留地路由不可绕过。
pub struct Router<'a> { pub book: &'a ResidencyBook }

impl<'a> Router<'a> {
    /// 路由：region 在其白名单内 → 落点；无白名单落点 → 本地暂存 + 告警。
    pub fn route(&self, region: &str, data_center: &str) -> Result<&'static str, RsCode> {
        let p = self.book.policy_of(region).ok_or(RsCode::NoWhitelisted)?;
        if let Some(d) = p.whitelisted.iter().find(|d| **d == data_center) {
            Ok(*d)
        } else if p.whitelisted.is_empty() || data_center.is_empty() {
            Ok("LOCAL-STAGING") // 本地暂存（调用方须告警）
        } else {
            Err(RsCode::NoWhitelisted) // 落点不在白名单 → 拒
        }
    }

    /// 跨境禁令触：NoCrossBorder 档地区请求境外落点 → 拒 + 立案。
    pub fn cross_border_gate(&self, region: &str, target: &str) -> Result<(), RsCode> {
        let p = self.book.policy_of(region).ok_or(RsCode::NoWhitelisted)?;
        let in_region = p.whitelisted.iter().any(|d| target.starts_with(d));
        if p.tier == ResidencyTier::NoCrossBorder && !in_region {
            return Err(RsCode::CrossBorder);
        }
        Ok(())
    }

    /// 绕过检测：LocalEncrypt 档地区任何"明落境外"路径 → 拒（不可绕过红线）。
    pub fn bypass_check(&self, region: &str, claimed: &str, actual: &str) -> Result<(), RsCode> {
        let p = self.book.policy_of(region).ok_or(RsCode::NoWhitelisted)?;
        if p.tier == ResidencyTier::LocalEncrypt && claimed != actual {
            return Err(RsCode::BypassAttempt);
        }
        Ok(())
    }
}

/// 驻留变更迁移协议：断点续迁。
pub struct Migration { pub from_region: &'static str, pub to_region: &'static str, pub items_total: usize, pub items_done: usize }

impl Migration {
    pub fn new(from: &'static str, to: &'static str, total: usize) -> Self {
        Migration { from_region: from, to_region: to, items_total: total, items_done: 0 }
    }

    pub fn progress(&self) -> usize { self.items_done }

    /// 迁移一步：成功推进；返回是否完成。
    pub fn step(&mut self, ok: bool) -> Result<bool, RsCode> {
        if !ok { return Err(RsCode::NoWhitelisted); }
        if self.items_done < self.items_total { self.items_done += 1; }
        Ok(self.items_done == self.items_total)
    }

    /// 断点续迁：从中断点继续（items_done 保留）。
    pub fn resume_point(&self) -> usize { self.items_done }
}

/// 告警册：本地暂存与拒绝立案。
pub struct AlarmBook { pub alarms: Vec<(&'static str, &'static str)> }

impl AlarmBook {
    pub fn new() -> Self { AlarmBook { alarms: Vec::new() } }
    pub fn raise(&mut self, what: &'static str, detail: &'static str) { self.alarms.push((what, detail)); }
    pub fn count(&self) -> usize { self.alarms.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves343_checks() -> CheckSet {
    let mut set = CheckSet::new("ves343");
    let mut book = ResidencyBook::new();

    book.put(ResidencyPolicy { region: "DE", tier: ResidencyTier::NoCrossBorder, whitelisted: vec!["eu-fra", "eu-ber"] });
    book.put(ResidencyPolicy { region: "CN", tier: ResidencyTier::LocalEncrypt, whitelisted: vec!["cn-bj", "cn-sh"] });
    book.put(ResidencyPolicy { region: "US", tier: ResidencyTier::Whitelist, whitelisted: vec![] });

    // C343-01 三档建模齐备。
    chk(&mut set, "C343-01",
        ResidencyTier::ALL.len() == 3 && ResidencyTier::LocalEncrypt.name() == "local-encrypt", "三档单源");

    // C343-02 白名单路由：DE → eu-fra 落点放行。
    let r = Router { book: &book };
    chk(&mut set, "C343-02", r.route("DE", "eu-fra") == Ok("eu-fra"), "白名单内落点放行");

    // C343-03 无白名单地区 → 本地暂存。
    chk(&mut set, "C343-03", r.route("US", "us-east") == Ok("LOCAL-STAGING"), "空白名单 → 本地暂存");

    // C343-04 本地暂存必须告警。
    let mut ab = AlarmBook::new();
    if r.route("US", "us-east") == Ok("LOCAL-STAGING") { ab.raise("local-staging", "US 无白名单落点"); }
    chk(&mut set, "C343-04", ab.count() == 1, "暂存即告警");

    // C343-05 白名单外落点 → 拒。
    chk(&mut set, "C343-05", r.route("DE", "us-east") == Err(RsCode::NoWhitelisted), "白名单外 => 拒");

    // C343-06 跨境禁令触：DE（NoCrossBorder）→ us-east 拒 + 立案。
    let cb = r.cross_border_gate("DE", "us-east");
    if cb == Err(RsCode::CrossBorder) { ab.raise("cross-border", "DE→us-east 禁令触"); }
    chk(&mut set, "C343-06", cb == Err(RsCode::CrossBorder) && ab.count() == 2, "跨境禁令拒 + 立案");

    // C343-07 跨境白名单内放行。
    chk(&mut set, "C343-07", r.cross_border_gate("DE", "eu-ber").is_ok(), "eu-ber 在 DE 白名单");

    // C343-08 不可绕过：声称本地实际境外 → 拒。
    chk(&mut set, "C343-08", r.bypass_check("CN", "cn-bj", "us-east") == Err(RsCode::BypassAttempt), "claim≠actual => 绕过拒");

    // C343-09 如实落点放行。
    chk(&mut set, "C343-09", r.bypass_check("CN", "cn-bj", "cn-bj").is_ok(), "claim=actual 放行");

    // C343-10 迁移协议：推进至完成。
    let mut mg = Migration::new("US", "DE", 3);
    let _ = mg.step(true);
    let done_mid = mg.step(true).unwrap_or(false);
    let done = mg.step(true).unwrap_or(false);
    chk(&mut set, "C343-10", !done_mid && done && mg.progress() == 3, "三步迁移完成");

    // C343-11 断点续迁：中断后从断点继续。
    let mut mg2 = Migration::new("US", "DE", 4);
    let _ = mg2.step(true);
    let _ = mg2.step(false); // 中断
    chk(&mut set, "C343-11", mg2.resume_point() == 1, "断点保留 1 项");

    // C343-12 续迁完成。
    let mut done_final = false;
    for _ in 1..4 { done_final = mg2.step(true).unwrap_or(false); }
    chk(&mut set, "C343-12", done_final && mg2.items_done == 4, "断点后续迁至完成");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves343_smoke() {
        let set = run_ves343_checks();
        assert!(set.failed_count() == 0, "ves343 red: {}", set.red_items_text());
    }

    #[test]
    fn ves343_gate() {
        let mut b = ResidencyBook::new();
        b.put(ResidencyPolicy { region: "XX", tier: ResidencyTier::NoCrossBorder, whitelisted: vec!["x1"] });
        let r = Router { book: &b };
        assert!(r.cross_border_gate("XX", "x1").is_ok());
        assert_eq!(r.cross_border_gate("XX", "y9"), Err(RsCode::CrossBorder));
    }
}
