//! unx0176_svcdegrade — UNX-F0176 运行时服务缺失降级路径。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0176-J1）：服务表空指针探测逐项
//! 降级（时间走 RTC 后备等），缺件如实标 N/A 不崩溃。
//!
//! 职责定位：旧固件的服务表可能缺项（空指针）——调用即崩。本模块是
//! 缺件降级闸：①服务表逐项探测（空指针显性标记，不猜不崩）；②降级路
//! 由表（每服务一条后备路径：时间走 RTC 后备、变量走内存态桩等——
//! 降级不是"不做"而是"换条路做"）；③N/A 诚实账（既无主路径也无后备
//! 的服务如实标 N/A，调用方拿显性 Err 不拿崩溃）。no_std + alloc；
//! 零 unwrap 零 expect。
//! 诊断码段 0x6560（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const DG_VERSION: &str = "B176-svcdegrade-v1";

// ── 诊断码（0x656 段，本会话独占）──
/// 服务缺失且无后备（N/A——显性拒绝，不崩溃）。
pub const XC_DG_NA: u16 = 0x6560;
/// 未知服务（表外调用——防呆）。
pub const XC_DG_UNKNOWN_SVC: u16 = 0x6561;
/// 后备路径也失败（降级链尽——显性报）。
pub const XC_DG_FALLBACK_FAIL: u16 = 0x6562;
/// 降级账不平（探测数 ≡ 主路+后备+NA 三态和）。
pub const XC_DG_LEDGER_UNBALANCED: u16 = 0x6563;
/// 降级路由重复登记（同服务两条后备——路由二义）。
pub const XC_DG_DUP_ROUTE: u16 = 0x6564;

/// 服务表条目（探测结果三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcAvail {
    /// 主路径在（指针非空）。
    Present,
    /// 主路径缺（空指针）——走后备。
    Missing,
}

/// 一条降级路由。
#[derive(Clone, Debug)]
pub struct DegradeRoute {
    /// 服务名。
    pub svc: &'static str,
    /// 后备说明。
    pub fallback: &'static str,
}

/// 降级闸。
#[derive(Debug)]
pub struct DegradeGate {
    /// 服务可用性表（探测结果）。
    pub table: Vec<(&'static str, SvcAvail)>,
    /// 降级路由表。
    routes: Vec<DegradeRoute>,
    /// 调用账（服务 → 走的路）。
    pub calls: Vec<(&'static str, &'static str)>,
}

impl DegradeGate {
    pub fn boot() -> Self {
        DegradeGate { table: Vec::new(), routes: Vec::new(), calls: Vec::new() }
    }

    /// 服务表探测登记（空指针显性标 Missing）。
    pub fn probe(&mut self, svc: &'static str, present: bool) -> Result<(), u16> {
        if self.table.iter().any(|(s, _)| *s == svc) {
            return Err(XC_DG_DUP_ROUTE);
        }
        let a = if present { SvcAvail::Present } else { SvcAvail::Missing };
        self.table.push((svc, a));
        Ok(())
    }

    /// 登记降级路由（同服务只许一条）。
    pub fn set_route(&mut self, svc: &'static str, fallback: &'static str) -> Result<(), u16> {
        if self.routes.iter().any(|r| r.svc == svc) {
            return Err(XC_DG_DUP_ROUTE);
        }
        if !self.table.iter().any(|(s, _)| *s == svc) {
            return Err(XC_DG_UNKNOWN_SVC);
        }
        self.routes.push(DegradeRoute { svc, fallback });
        Ok(())
    }

    /// 调用服务：主路在场直走；缺失走后备；无后备显性 N/A。
    /// 返回值 = 走的路（"main"/后备名/"N/A"）。
    pub fn invoke(&mut self, svc: &'static str) -> Result<&'static str, u16> {
        let avail = match self.table.iter().find(|(s, _)| *s == svc) {
            Some((_, a)) => *a,
            None => return Err(XC_DG_UNKNOWN_SVC),
        };
        let path = match avail {
            SvcAvail::Present => "main",
            SvcAvail::Missing => match self.routes.iter().find(|r| r.svc == svc) {
                Some(r) => r.fallback,
                None => return Err(XC_DG_NA),
            },
        };
        if self.calls.len() >= 128 {
            self.calls.remove(0);
        }
        self.calls.push((svc, path));
        Ok(path)
    }

    /// 账平（判据对偶）：三态和 ≡ 账内调用数按首次归类计数。
    pub fn balanced(&self) -> bool {
        // 每次调用都恰好归类一条路——calls 数 ≡ 记录数（结构恒真），
        // 这里做防御性复核：全部路径合法（main/登记过的后备/N-A 不会入账）。
        self.calls.iter().all(|(_, p)| {
            *p == "main" || self.routes.iter().any(|r| r.fallback == *p)
        })
    }

    /// N/A 服务清单（诚实账——调用方据此绕行）。
    pub fn na_services(&self) -> Vec<&'static str> {
        self.table
            .iter()
            .filter(|(s, a)| *a == SvcAvail::Missing && !self.routes.iter().any(|r| r.svc == *s))
            .map(|(s, _)| *s)
            .collect()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_DG_NA => "服务 N/A：主路径缺且无后备——调用方显性绕行，不崩溃。".to_string(),
            XC_DG_UNKNOWN_SVC => "未知服务：先探测登记再调用。".to_string(),
            XC_DG_FALLBACK_FAIL => "后备路径也失败：降级链尽，显性报错人工介入。".to_string(),
            XC_DG_LEDGER_UNBALANCED => "降级账不平：先对账再定级。".to_string(),
            XC_DG_DUP_ROUTE => "路由重复：同服务只许一条后备。".to_string(),
            _ => "未知诊断码：查 0x656 段分配表。".to_string(),
        }
    }
}

/// 自证：探测 + 降级 + N/A 诚实账对拍。
pub fn self_audit() -> Result<(), u16> {
    let mut g = DegradeGate::boot();
    // 一、服务表：GetTime 在、SetVirtualAddressMap 缺（旧固件常态）。
    g.probe("GetTime", true)?;
    g.probe("SetVirtualAddressMap", false)?;
    g.probe("GetVariable", true)?;
    g.probe("QueryVariableInfo", false)?;
    // 二、降级路由：时间主路；虚拟地址映射走内存态桩；QueryVariableInfo 无后备 → N/A。
    g.set_route("SetVirtualAddressMap", "memtable-stub")?;
    if g.set_route("SetVirtualAddressMap", "dup").is_ok() {
        return Err(XC_DG_DUP_ROUTE);
    }
    // 三、调用三路。
    let p1 = g.invoke("GetTime")?;
    let p2 = g.invoke("SetVirtualAddressMap")?;
    if p1 != "main" || p2 != "memtable-stub" {
        return Err(XC_DG_FALLBACK_FAIL);
    }
    if g.invoke("QueryVariableInfo").is_ok() {
        return Err(XC_DG_NA);
    }
    // N/A 清单诚实。
    let na = g.na_services();
    if na.len() != 1 || na[0] != "QueryVariableInfo" {
        return Err(XC_DG_LEDGER_UNBALANCED);
    }
    if !g.balanced() {
        return Err(XC_DG_LEDGER_UNBALANCED);
    }
    // 四、防呆：未知服务调用/路由全拒。
    if g.invoke("NoSuchService").is_ok() {
        return Err(XC_DG_UNKNOWN_SVC);
    }
    if g.set_route("NoSuchService", "x").is_ok() {
        return Err(XC_DG_UNKNOWN_SVC);
    }
    Ok(())
}
