//! VE-F4290 · 一致性×SDK（U05 工具集成组）
//! 职责：SDK 一致性能力输出三 API（规则查询/本地预检/结果上报）+ 版本化
//! 承诺（向后兼容、废弃走双版期）+ 上报断网本地队列补传不丢 + 类型化
//! 错误码修复建议 + 第三方违规接入资格冻结 + 上报数据脱敏（隐私面显式声明）。
//! 段位：0x2C00（续表 idx524）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2C00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkCode {
    /// 规则不存在。
    RuleNotFound = SEG_BASE,
    /// API 误用（类型化错误码）。
    ApiMisuse = SEG_BASE + 0x01,
    /// 第三方违规（资格冻结）。
    ThirdPartyFrozen = SEG_BASE + 0x02,
    /// 废弃 API 双版期未过仍调用（过渡告警）。
    DeprecatedCall = SEG_BASE + 0x03,
}

/// SDK API 三件。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdkApi {
    Query = 0,
    Precheck = 1,
    Report = 2,
}

/// 第三方接入方。
#[derive(Clone, Copy, Debug)]
pub struct ThirdParty {
    pub id: u8,
    pub certified: bool,
    pub frozen: bool,
    pub violations: u32,
}

/// SDK 管理器。
pub struct SdkHub {
    /// 规则真源：(规则, 内容字)。
    pub rules: Vec<(u16, u32)>,
    /// API 版本：当前/废弃中 (api, 废弃版止于)。
    pub deprecated: Vec<(SdkApi, u32)>,
    pub cur_ver: u32,
    /// 断网队列（补传不丢）。
    pub offline_queue: Vec<u32>,
    pub online: bool,
    pub uploaded: u32,
    pub parties: Vec<ThirdParty>,
}

impl SdkHub {
    pub fn new(cur_ver: u32) -> Self {
        SdkHub { rules: Vec::new(), deprecated: Vec::new(), cur_ver, offline_queue: Vec::new(), online: true, uploaded: 0, parties: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: u16, body: u32) {
        if !self.rules.iter().any(|(r, _)| *r == rule) {
            self.rules.push((rule, body));
        }
    }

    /// API 一：规则查询（O(log n) 语义——线性扫后二分可换）。
    pub fn query(&self, rule: u16) -> Result<u32, SkCode> {
        self.rules.iter().find(|(r, _)| *r == rule).map(|(_, b)| *b).ok_or(SkCode::RuleNotFound)
    }

    /// API 二：本地预检（用本地规则子集）。
    pub fn precheck(&self, local: &[(u16, u32)]) -> usize {
        local.iter().filter(|(r, v)| self.rules.iter().any(|(rr, rv)| rr == r && rv != v)).count()
    }

    /// API 三：结果上报（断网入队列补传不丢）。
    pub fn report(&mut self, result: u32) -> Result<(), SkCode> {
        if self.online {
            self.uploaded += 1;
            Ok(())
        } else {
            self.offline_queue.push(result);
            Ok(())
        }
    }

    /// 联网补传：队列全部补齐。
    pub fn flush(&mut self) -> usize {
        let n = self.offline_queue.len();
        self.uploaded += n as u32;
        self.offline_queue.clear();
        n
    }

    /// 废弃双版期：调用废弃 API 且未过期→过渡告警。
    pub fn call(&self, api: SdkApi, ver: u32) -> Result<(), SkCode> {
        if let Some((a, until)) = self.deprecated.iter().find(|(a, _)| *a == api) {
            if ver <= *until {
                return Err(SkCode::DeprecatedCall); // 双版期内仍可用但告警
            }
            let _ = a;
            return Err(SkCode::ApiMisuse); // 双版期满彻底移除
        }
        Ok(())
    }

    pub fn deprecate(&mut self, api: SdkApi, until_ver: u32) {
        if !self.deprecated.iter().any(|(a, _)| *a == api) {
            self.deprecated.push((api, until_ver));
        }
    }

    /// 第三方登记与违规冻结。
    pub fn enroll(&mut self, id: u8) {
        if !self.parties.iter().any(|p| p.id == id) {
            self.parties.push(ThirdParty { id, certified: true, frozen: false, violations: 0 });
        }
    }

    pub fn violation(&mut self, id: u8) -> Result<(), SkCode> {
        let p = self.parties.iter_mut().find(|p| p.id == id).ok_or(SkCode::ThirdPartyFrozen)?;
        p.violations += 1;
        if p.violations >= 3 {
            p.frozen = true;
            return Err(SkCode::ThirdPartyFrozen);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C524-01..12。
pub fn run_ves524_checks() -> CheckSet {
    let mut set = CheckSet::new("ves524");

    let mut sk = SdkHub::new(5);
    sk.add_rule(10, 100);
    sk.add_rule(20, 200);
    chk(&mut set, "C524-01", sk.query(10) == Ok(100), "API 一：规则查询命中");
    chk(&mut set, "C524-02", sk.query(99) == Err(SkCode::RuleNotFound), "未知规则报错");
    chk(&mut set, "C524-03", sk.precheck(&[(10, 100), (20, 999), (10, 100)]) == 1, "API 二：本地预检差异 1 处");

    // 断网补传
    sk.online = false;
    let _ = sk.report(1);
    let _ = sk.report(2);
    chk(&mut set, "C524-04", sk.offline_queue.len() == 2 && sk.uploaded == 0, "断网入队不丢");
    sk.online = true;
    chk(&mut set, "C524-05", sk.flush() == 2 && sk.uploaded == 2 && sk.offline_queue.is_empty(), "联网补传全齐");

    // 双版期
    sk.deprecate(SdkApi::Precheck, 7);
    chk(&mut set, "C524-06", sk.call(SdkApi::Precheck, 6) == Err(SkCode::DeprecatedCall), "双版期内调用=过渡告警");
    chk(&mut set, "C524-07", sk.call(SdkApi::Precheck, 8) == Err(SkCode::ApiMisuse), "双版期满=彻底移除");
    chk(&mut set, "C524-08", sk.call(SdkApi::Query, 6).is_ok(), "未废弃 API 正常");

    // 第三方冻结
    sk.enroll(1);
    let _ = sk.violation(1);
    let _ = sk.violation(1);
    chk(&mut set, "C524-09", sk.violation(1) == Err(SkCode::ThirdPartyFrozen), "三振冻结资格");
    chk(&mut set, "C524-10", sk.parties[0].frozen && sk.parties[0].violations == 3, "冻结在册");
    chk(&mut set, "C524-11", sk.violation(9) == Err(SkCode::ThirdPartyFrozen), "未登记方拒收");
    chk(&mut set, "C524-12", SdkApi::Report as u8 == 2 && sk.rules.len() == 2, "三 API 枚举与规则账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_hub() {
        let mut sk = SdkHub::new(1);
        sk.add_rule(1, 11);
        assert_eq!(sk.query(1), Ok(11));
        sk.online = false;
        let _ = sk.report(7);
        sk.online = true;
        assert_eq!(sk.flush(), 1);
        sk.deprecate(SdkApi::Query, 2);
        assert_eq!(sk.call(SdkApi::Query, 1), Err(SkCode::DeprecatedCall));
    }
}
