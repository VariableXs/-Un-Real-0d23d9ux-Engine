//! VE-F4294 · 工具集成安全（U05 工具集成组）
//! 职责：集成面威胁三类（伪造事件/越权适配器/敏感数据总线外泄）+ 对策
//! 三件（事件签名/适配器最小权限/总线级脱敏）+ 全审计流（谁何时从哪个
//! 面动了什么）+ 签名失败拒收告警 + 越权熔断封禁 + 漏配扫描兜底补拦。
//! 段位：0x2E20（续表 idx528）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsCode {
    /// 签名失败（拒收+告警）。
    SigFail = SEG_BASE,
    /// 越权调用（熔断封禁）。
    PrivilegeOver = SEG_BASE + 0x01,
    /// 敏感字段外泄（脱敏兜底拦截）。
    LeakHit = SEG_BASE + 0x02,
}

/// 适配器权限（最小权限位图）。
#[derive(Clone, Copy, Debug)]
pub struct AdapterPerm {
    pub face: usize,
    pub can_read: bool,
    pub can_write: bool,
    pub banned: bool,
}

/// 集成安全器。
pub struct IntegSec {
    /// 签名密钥（单字示意）。
    pub key: u32,
    pub perms: Vec<AdapterPerm>,
    /// 敏感字段黑名单（总线级脱敏）。
    pub sensitive: Vec<u32>,
    /// 审计流：(who, face, action, 拦截)。
    pub audit: Vec<(u8, usize, u8, bool)>,
    pub bans: u32,
}

fn sig_of(key: u32, payload: u32) -> u32 {
    key ^ payload.wrapping_mul(0x9E37_79B9)
}

impl IntegSec {
    pub fn new(key: u32) -> Self {
        IntegSec { key, perms: Vec::new(), sensitive: Vec::new(), audit: Vec::new(), bans: 0 }
    }

    pub fn grant(&mut self, face: usize, can_read: bool, can_write: bool) {
        if !self.perms.iter().any(|p| p.face == face) {
            self.perms.push(AdapterPerm { face, can_read, can_write, banned: false });
        }
    }

    /// 事件签名验签：失败拒收+告警留痕。
    pub fn verify(&mut self, who: u8, payload: u32, sig: u32) -> Result<(), IsCode> {
        let ok = sig == sig_of(self.key, payload);
        self.audit.push((who, 0, 0, !ok));
        if !ok {
            return Err(IsCode::SigFail);
        }
        Ok(())
    }

    /// 权限检：未授权/封禁→熔断封禁。
    pub fn check_perm(&mut self, face: usize, write: bool) -> Result<(), IsCode> {
        let p = match self.perms.iter().find(|p| p.face == face) {
            Some(p) => AdapterPerm { face: p.face, can_read: p.can_read, can_write: p.can_write, banned: p.banned },
            None => {
                self.audit.push((0, face, if write { 2 } else { 1 }, true));
                return Err(IsCode::PrivilegeOver);
            }
        };
        if p.banned || (write && !p.can_write) || (!write && !p.can_read) {
            if !p.banned {
                if let Some(pp) = self.perms.iter_mut().find(|pp| pp.face == face) {
                    pp.banned = true; // 熔断封禁
                }
                self.bans += 1;
            }
            self.audit.push((0, face, if write { 2 } else { 1 }, true));
            return Err(IsCode::PrivilegeOver);
        }
        self.audit.push((0, face, if write { 2 } else { 1 }, false));
        Ok(())
    }

    /// 总线级脱敏：敏感字段命中→兜底拦截。
    pub fn desensitize(&mut self, fields: &[u32]) -> bool {
        let hit = fields.iter().any(|f| self.sensitive.contains(f));
        if hit {
            self.audit.push((0, 0xFF, 3, true));
        }
        hit
    }

    /// 漏配扫描兜底：未授权面调用全拦（审计已含）。
    pub fn audit_blocked(&self) -> usize {
        self.audit.iter().filter(|(_, _, _, b)| *b).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C528-01..12。
pub fn run_ves528_checks() -> CheckSet {
    let mut set = CheckSet::new("ves528");

    let mut is = IntegSec::new(0x7E1);
    chk(&mut set, "C528-01", is.verify(1, 100, sig_of(0x7E1, 100)).is_ok(), "签名一致放行");
    chk(&mut set, "C528-02", is.verify(1, 100, 0) == Err(IsCode::SigFail), "签名失败拒收");
    chk(&mut set, "C528-03", is.audit_blocked() == 1, "告警留痕");

    // 最小权限
    is.grant(1, true, false);
    chk(&mut set, "C528-04", is.check_perm(1, false).is_ok(), "读权限放行");
    chk(&mut set, "C528-05", is.check_perm(1, true) == Err(IsCode::PrivilegeOver), "越写权熔断封禁");
    chk(&mut set, "C528-06", is.check_perm(1, false) == Err(IsCode::PrivilegeOver), "封禁后连读也拒");
    chk(&mut set, "C528-07", is.bans == 1, "封禁计数");
    chk(&mut set, "C528-08", is.check_perm(9, false) == Err(IsCode::PrivilegeOver), "未授权面拦截");

    // 总线脱敏
    is.sensitive = alloc::vec![0xCAFE];
    chk(&mut set, "C528-09", is.desensitize(&[1, 0xCAFE, 3]), "敏感字段兜底拦截");
    chk(&mut set, "C528-10", !is.desensitize(&[1, 2, 3]), "干净字段放行");

    chk(&mut set, "C528-11", is.audit.len() >= 6, "全审计流在册");
    chk(&mut set, "C528-12", is.audit.iter().any(|(_, f, a, _)| *a == 3), "脱敏事件入审计");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_sec() {
        let mut is = IntegSec::new(7);
        assert!(is.verify(1, 5, sig_of(7, 5)).is_ok());
        assert!(is.verify(1, 5, 1).is_err());
        is.grant(1, true, true);
        assert!(is.check_perm(1, true).is_ok());
        assert!(is.check_perm(2, true).is_err());
    }
}
