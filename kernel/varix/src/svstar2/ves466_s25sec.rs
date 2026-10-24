//! ves466_s25sec — VE-F4232 · 契约安全
//! 三道：DSL 注入面（畸形表达式的沙箱求值）+签名验证（契约注册必签）+越权变更拦截（提供方身份核验）；
//! 沙箱逃逸企图→阻断+最高级立案；签名错→拒注册；身份不符→拒绝+告警；
//! 越权变更含无障碍判据防弱化专项（域色红线）；沙箱无网络无文件访问。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1860;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    SandboxEscape = SEG_BASE + 0x01, // 0x1861 最高级立案
    SigWrong = SEG_BASE + 0x02,      // 0x1862
    IdentityMismatch = SEG_BASE + 0x03, // 0x1863
    A11yWeaken = SEG_BASE + 0x04,    // 0x1864 防弱化（域色红线）
}

/// 沙箱求值：允许算术/比较，禁止网络（net）/文件（fs）/执行（exec）。
pub fn sandbox_eval(expr: &str) -> Result<u32, ScCode> {
    for bad in ["net(", "fs(", "exec("] {
        if expr.contains(bad) { return Err(ScCode::SandboxEscape); }
    }
    // 简易求值：仅支持 "N+N" / "N" 形态（合成）。
    if let Some(p) = expr.find('+') {
        let a: u32 = expr[..p].trim().parse().map_err(|_| ScCode::SandboxEscape)?;
        let b: u32 = expr[p + 1..].trim().parse().map_err(|_| ScCode::SandboxEscape)?;
        Ok(a + b)
    } else {
        expr.trim().parse().map_err(|_| ScCode::SandboxEscape)
    }
}

/// FNV 签名。
pub fn fnv(data: &[u32]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for d in data { h ^= *d; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 注册安全闸。
pub struct RegistryGuard {
    pub owners: Vec<(u32, &'static str)>, // 契约 id → 提供方
    pub filed_critical: u32,
    pub alerts: u32,
}

impl RegistryGuard {
    pub fn new() -> Self { RegistryGuard { owners: Vec::new(), filed_critical: 0, alerts: 0 } }
    /// 注册必签：签名错→拒注册。
    pub fn register(&mut self, cid: u32, payload: &[u32], sig: u32, owner: &'static str) -> Result<(), ScCode> {
        if fnv(payload) != sig { return Err(ScCode::SigWrong); }
        self.owners.retain(|(c, _)| *c != cid);
        self.owners.push((cid, owner));
        Ok(())
    }
    /// 越权变更拦截：提供方身份核验+防弱化专项（域色红线）。
    pub fn change(&mut self, cid: u32, who: &'static str, weakens_a11y: bool) -> Result<(), ScCode> {
        if weakens_a11y { self.filed_critical += 1; return Err(ScCode::A11yWeaken); }
        match self.owners.iter().find(|(c, _)| *c == cid) {
            Some((_, o)) if *o == who => Ok(()),
            _ => { self.alerts += 1; Err(ScCode::IdentityMismatch) }
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves466_checks() -> CheckSet {
    let mut set = CheckSet::new("ves466");

    // C466-01 沙箱算术求值通过。
    chk(&mut set, "C466-01", sandbox_eval("2+3") == Ok(5), "沙箱求值");

    // C466-02 网络调用→逃逸企图阻断。
    chk(&mut set, "C466-02", sandbox_eval("net(host)") == Err(ScCode::SandboxEscape), "网络阻断");

    // C466-03 文件/执行→阻断。
    chk(&mut set, "C466-03", sandbox_eval("fs(/etc)") == Err(ScCode::SandboxEscape)
        && sandbox_eval("exec(cmd)") == Err(ScCode::SandboxEscape), "文件执行阻断");

    // C466-04 畸形表达式→逃逸码（不可解析=逃逸企图）。
    chk(&mut set, "C466-04", sandbox_eval("2+&*x") == Err(ScCode::SandboxEscape), "畸形阻断");

    // C466-05 注册必签：签名对→注册。
    let mut g = RegistryGuard::new();
    let payload = [1u32, 2, 3];
    chk(&mut set, "C466-05", g.register(7001, &payload, fnv(&payload), "prov-a").is_ok(), "必签注册");

    // C466-06 签名错→拒注册。
    chk(&mut set, "C466-06", g.register(7002, &payload, 12345, "prov-a") == Err(ScCode::SigWrong), "签名错拒");

    // C466-07 同提供方变更→放行。
    chk(&mut set, "C466-07", g.change(7001, "prov-a", false).is_ok(), "同方放行");

    // C466-08 身份不符→拒绝+告警。
    chk(&mut set, "C466-08", g.change(7001, "prov-b", false) == Err(ScCode::IdentityMismatch)
        && g.alerts == 1, "越权告警");

    // C466-09 域色红线：弱化无障碍判据的变更→最高级立案（优先于身份）。
    chk(&mut set, "C466-09", g.change(7001, "prov-a", true) == Err(ScCode::A11yWeaken)
        && g.filed_critical == 1, "防弱化红线");

    // C466-10 未知契约变更→身份不符。
    chk(&mut set, "C466-10", g.change(9999, "prov-a", false) == Err(ScCode::IdentityMismatch), "未知契约拒");

    // C466-11 FNV 可复现。
    chk(&mut set, "C466-11", fnv(&[1, 2]) == fnv(&[1, 2]) && fnv(&[]) == 0x811C_9DC5, "签名可复现");

    // C466-12 错误码段位单源。
    chk(&mut set, "C466-12", ScCode::A11yWeaken as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves466_smoke() {
        let set = run_ves466_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves466 red: {:?}", red);
    }
}
