//! UNX-F12033 未实现三档：NOT_IMPLEMENTED/UNSUPPORTED/INVALID_DEVICE_REQUEST（0x19640 段）
//!
//! 判据原文：能力缺失三档分账——档一 NOT_IMPLEMENTED（0xC0000001，路径
//! 未铺）、档二 STATUS_UNSUCCESSFUL 面之外的 UNSUPPORTED（0xC00000BB，
//! 设施声明不支持）、档三 INVALID_DEVICE_REQUEST（0xC0000010，设备错收
//! 请求）；能力册闭集声明后按档路由；未声明能力不得路由成功。
//!
//! 结构：能力册（CapBook：请求类 × 三档声明）+ 路由核（四码守卫）+
//! 路由账（CiRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12033_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19640 独占 0x20）。
pub const SEG_BASE: u32 = 0x19640;
pub const CI_UNKNOWN_REQ: u32 = SEG_BASE + 0x01; // 未知请求类
pub const CI_UNDECLARED: u32 = SEG_BASE + 0x02; // 未声明能力
pub const CI_DUP_DECL: u32 = SEG_BASE + 0x03; // 重复声明
pub const CI_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造路由账
pub const CI_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 语义钉值。
pub const STATUS_NOT_IMPLEMENTED: u32 = 0xC000_0001;
pub const STATUS_UNSUPPORTED: u32 = 0xC000_00BB;
pub const STATUS_INVALID_DEVICE_REQUEST: u32 = 0xC000_0010;

/// 三档闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapTier {
    NotImplemented = 1,
    Unsupported = 2,
    InvalidDeviceRequest = 3,
}

pub fn tier_status(t: CapTier) -> u32 {
    match t {
        CapTier::NotImplemented => STATUS_NOT_IMPLEMENTED,
        CapTier::Unsupported => STATUS_UNSUPPORTED,
        CapTier::InvalidDeviceRequest => STATUS_INVALID_DEVICE_REQUEST,
    }
}

/// 请求类闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqClass {
    Read = 1,
    Write = 2,
    Ioctl = 3,
    Flush = 4,
    QueryVolume = 5,
}

pub fn req_of(v: u16) -> Option<ReqClass> {
    match v {
        1 => Some(ReqClass::Read),
        2 => Some(ReqClass::Write),
        3 => Some(ReqClass::Ioctl),
        4 => Some(ReqClass::Flush),
        5 => Some(ReqClass::QueryVolume),
        _ => None,
    }
}

/// 一笔路由的账目。
#[derive(Clone)]
pub struct CiRec {
    pub seq: u32,
    pub req: ReqClass,
    /// 结果：0=放行 否则档号。
    pub outcome: u16,
    pub note: String,
}

impl CiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 能力册模型。
pub struct CapBook {
    /// 声明表：请求类 → 档。
    pub decls: Vec<(ReqClass, CapTier)>,
    pub ledger: Vec<CiRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CapBook {
    pub fn new() -> CapBook {
        CapBook { decls: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 声明核：闭集请求类 + 单次声明。
    pub fn declare(&mut self, req: ReqClass, tier: CapTier, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        if note.is_empty() {
            return Err(CI_UNKNOWN_REQ);
        }
        if self.decls.iter().any(|d| d.0 == req) {
            return Err(CI_DUP_DECL);
        }
        self.decls.push((req, tier));
        Ok(())
    }

    /// 路由核：未声明 → 未实现拒（CI_UNDECLARED）；已声明 → 档状态码拒。
    pub fn route(&mut self, req: ReqClass, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        if note.is_empty() {
            return Err(CI_UNKNOWN_REQ);
        }
        let outcome = match self.decls.iter().find(|d| d.0 == req) {
            Some(d) => d.1 as u16,
            None => {
                self.ledger.push(CiRec {
                    seq: self.seq_next,
                    req,
                    outcome: 0xFFFF,
                    note: note.to_string(),
                });
                self.seq_next += 1;
                return Err(CI_UNDECLARED);
            }
        };
        let rec = CiRec { seq: self.seq_next, req, outcome, note: note.to_string() };
        if !rec.ok() {
            return Err(CI_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        // 三档皆拒：返回各档状态码。
        match outcome {
            1 => Err(STATUS_NOT_IMPLEMENTED),
            2 => Err(STATUS_UNSUPPORTED),
            _ => Err(STATUS_INVALID_DEVICE_REQUEST),
        }
    }

    /// 档清点。
    pub fn count_tier(&self, t: CapTier) -> usize {
        self.decls.iter().filter(|d| d.1 == t).count()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.decls {
            h = h.wrapping_mul(211).wrapping_add(d.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(d.1 as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CapBook::new();
    let _ = b.declare(ReqClass::Ioctl, CapTier::Unsupported, "d");
    // 1) 未知请求类值（闭集外）→ req_of None。
    got[0] = req_of(0).is_none() && req_of(6).is_none() && req_of(3).is_some();
    // 2) 未声明能力路由 → CI_UNDECLARED。
    got[1] = b.route(ReqClass::Flush, "u").err() == Some(CI_UNDECLARED);
    // 3) 重复声明 → CI_DUP_DECL。
    got[2] =
        b.declare(ReqClass::Ioctl, CapTier::NotImplemented, "dup").err() == Some(CI_DUP_DECL);
    // 4) 空留痕 → CI_UNKNOWN_REQ。
    got[3] = b.route(ReqClass::Ioctl, "").err() == Some(CI_UNKNOWN_REQ);
    // 5) 印账后动作 → CI_FROZEN。
    b.seal();
    got[4] = b.route(ReqClass::Ioctl, "post").err() == Some(CI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12033_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12033");

    // J1 三档状态码钉值。
    let j1 = tier_status(CapTier::NotImplemented) == 0xC000_0001
        && tier_status(CapTier::Unsupported) == 0xC000_00BB
        && tier_status(CapTier::InvalidDeviceRequest) == 0xC000_0010;
    s.add("J1-三档钉值", j1, "0x01/0xBB/0x10");

    // J2 请求类闭集：1..5 收，0/6 拒。
    let j2 = req_of(1).is_some()
        && req_of(5).is_some()
        && req_of(0).is_none()
        && req_of(6).is_none();
    s.add("J2-请求类闭集", j2, "闭集边界");

    // J3 已声明三档路由各回各码。
    let j3 = {
        let mut b = CapBook::new();
        let _ = b.declare(ReqClass::Read, CapTier::NotImplemented, "a");
        let _ = b.declare(ReqClass::Write, CapTier::Unsupported, "b");
        let _ = b.declare(ReqClass::Ioctl, CapTier::InvalidDeviceRequest, "c");
        b.route(ReqClass::Read, "r") == Err(STATUS_NOT_IMPLEMENTED)
            && b.route(ReqClass::Write, "w") == Err(STATUS_UNSUPPORTED)
            && b.route(ReqClass::Ioctl, "i") == Err(STATUS_INVALID_DEVICE_REQUEST)
    };
    s.add("J3-三档路由", j3, "各档各码");

    // J4 未声明能力拒。
    let j4 = {
        let mut b = CapBook::new();
        b.route(ReqClass::Flush, "u").err() == Some(CI_UNDECLARED)
    };
    s.add("J4-未声明拒", j4, "路由不得假绿");

    // J5 重复声明拒。
    let j5 = {
        let mut b = CapBook::new();
        let _ = b.declare(ReqClass::Flush, CapTier::Unsupported, "a");
        b.declare(ReqClass::Flush, CapTier::NotImplemented, "b").err() == Some(CI_DUP_DECL)
    };
    s.add("J5-重复声明拒", j5, "首声明终身");

    // J6 档清点。
    let j6 = {
        let mut b = CapBook::new();
        let _ = b.declare(ReqClass::Read, CapTier::NotImplemented, "a");
        let _ = b.declare(ReqClass::Write, CapTier::NotImplemented, "b");
        let _ = b.declare(ReqClass::Ioctl, CapTier::Unsupported, "c");
        b.count_tier(CapTier::NotImplemented) == 2 && b.count_tier(CapTier::Unsupported) == 1
    };
    s.add("J6-档清点", j6, "2+1 对账");

    // J7 路由账留痕：档号入账。
    let j7 = {
        let mut b = CapBook::new();
        let _ = b.declare(ReqClass::Read, CapTier::NotImplemented, "a");
        let _ = b.route(ReqClass::Read, "r");
        b.ledger[0].outcome == 1 && b.ledger_ok()
    };
    s.add("J7-路由留痕", j7, "档号=1 对账");

    // J8 空留痕拒。
    let j8 = {
        let mut b = CapBook::new();
        b.route(ReqClass::Read, "").err() == Some(CI_UNKNOWN_REQ)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = CapBook::new();
        let _ = a.declare(ReqClass::Read, CapTier::NotImplemented, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = CapBook::new();
        let _ = c.declare(ReqClass::Read, CapTier::Unsupported, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异册异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = CapBook::new();
        let _ = b.declare(ReqClass::Read, CapTier::Unsupported, "a");
        b.seal();
        b.route(ReqClass::Read, "post").err() == Some(CI_FROZEN)
            && b.declare(ReqClass::Write, CapTier::Unsupported, "post").err() == Some(CI_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：典型设备能力面。
    let mut b = CapBook::new();
    let _ = b.declare(ReqClass::Flush, CapTier::NotImplemented, "sum-f");
    let _ = b.declare(ReqClass::QueryVolume, CapTier::Unsupported, "sum-q");
    let r = b.route(ReqClass::Flush, "sum-r");
    s.add("样例路由回码", r == Err(STATUS_NOT_IMPLEMENTED), "Flush 未铺");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12033_route_flow() {
        let mut b = CapBook::new();
        assert!(b.declare(ReqClass::Read, CapTier::Unsupported, "a").is_ok());
        assert_eq!(b.declare(ReqClass::Read, CapTier::Unsupported, "b"), Err(CI_DUP_DECL));
        assert_eq!(b.route(ReqClass::Read, "r"), Err(STATUS_UNSUPPORTED));
        assert_eq!(b.route(ReqClass::Write, "w"), Err(CI_UNDECLARED));
    }

    #[test]
    fn unx12033_checks_green() {
        let s = run_unx12033_checks();
        assert!(s.all_passed(), "UNX-F12033 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12033_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
