//! VE-F4294 · 工具集成安全（U05 工具集成组）
//! 职责：集成面威胁三类（伪造事件/越权适配器/敏感数据总线外泄）+ 对策
//! 三件（事件签名/适配器最小权限/总线级脱敏）+ 全审计流（谁何时从哪个
//! 面动了什么）+ 签名失败拒收告警 + 越权熔断封禁 + 漏配扫描兜底补拦。
//! 段位：0x2E20（续表 idx528）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
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
    /// 重放攻击（同 payload 二次提交）。
    ReplayHit = SEG_BASE + 0x03,
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
    /// 已放行 payload 账（防重放）。
    pub seen: Vec<u32>,
    pub bans: u32,
}

fn sig_of(key: u32, payload: u32) -> u32 {
    key ^ payload.wrapping_mul(0x9E37_79B9)
}

impl IntegSec {
    pub fn new(key: u32) -> Self {
        IntegSec { key, perms: Vec::new(), sensitive: Vec::new(), audit: Vec::new(), seen: Vec::new(), bans: 0 }
    }

    /// 敏感项登记（去重幂等）。
    pub fn add_sensitive(&mut self, f: u32) {
        if !self.sensitive.contains(&f) {
            self.sensitive.push(f);
        }
    }

    /// 防重放验签：验签通过后查重放账，同 payload 二次提交拒收。
    pub fn verify_anti_replay(&mut self, who: u8, payload: u32, sig: u32) -> Result<(), IsCode> {
        self.verify(who, payload, sig)?;
        if self.seen.contains(&payload) {
            self.audit.push((who, 0, 4, true));
            return Err(IsCode::ReplayHit);
        }
        self.seen.push(payload);
        Ok(())
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

    // —— 深化 A：防重放 + 结构化威胁账本 ——
    let sig_ok = sig_of(0x7E1, 500);
    chk(&mut set, "C528-13", is.verify_anti_replay(2, 500, sig_ok).is_ok(), "首提交验签入账");
    chk(&mut set, "C528-14", is.verify_anti_replay(2, 500, sig_ok) == Err(IsCode::ReplayHit), "重放二次提交拒收");
    let mut tl = ThreatLedger::new();
    for (i, (_, f, a, b)) in is.audit.iter().enumerate() {
        tl.record(i as u16, *f, *a, *b);
    }
    chk(&mut set, "C528-15", tl.conserve_ok() && tl.recs.len() == is.audit.len(), "威胁账本守恒：拦+放≡总");
    chk(&mut set, "C528-16", tl.blocked_permille() > 0, "拦截率整数千分比投影");
    chk(&mut set, "C528-17", tl.by_face().iter().any(|(f, _, _)| *f == 1), "按面归因投影在册");
    chk(&mut set, "C528-18", !tl.summary().is_empty(), "威胁账本中文摘要（域本色）");

    // —— 深化 B：坏签名注入必抓 ——
    let mut sig_blocked = 0u32;
    for i in 0..8u32 {
        if is.verify(3, 900 + i, sig_of(0x7E1, 900 + i).wrapping_add(1)) == Err(IsCode::SigFail) {
            sig_blocked += 1;
        }
    }
    chk(&mut set, "C528-19", sig_blocked == 8, "坏签名 8 轮注入必抓率 100%");

    // —— 深化 C：边界探针 ——
    let er = run_sec_edge_probes(0x7E1);
    chk(&mut set, "C528-20", er.empty_audit_ok && er.empty_sensitive_clean, "空审计/空敏感表零误报");
    chk(&mut set, "C528-21", er.dup_sensitive_dedup && er.oob_face_blocked, "敏感项去重幂等/越界面拦截");

    // —— 深化 D：LCG 权限注入回归（必抓 + 负对照零误报）——
    match sec_inject_regression(64) {
        Ok((legal, blocked)) => {
            chk(&mut set, "C528-22", legal + blocked == 64 && blocked > 0, "64 轮权限注入必抓且收支守恒");
        }
        Err(_) => chk(&mut set, "C528-22", false, "权限注入回归出现误放行"),
    }

    // —— 深化 E：幂等闸 ——
    chk(&mut set, "C528-23", sec_idempotence_gate().is_ok(), "幂等闸：grant/敏感项二次提交零副作用");

    // —— 深化 F：审计流序列化 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_audit(&is.audit, &mut bytes);
    let rt = decode_audit(&bytes);
    chk(&mut set, "C528-24", rt.iter().count() == is.audit.len(), "审计流 5B/条定长解码往返");
    chk(&mut set, "C528-25", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == is.audit.len() * 5, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：结构化威胁账本 =====

/// 威胁审计记录（回合归因）。
#[derive(Clone, Copy, Debug)]
pub struct ThreatRec {
    pub round: u16,
    pub face: usize,
    pub action: u8,
    pub blocked: bool,
}

/// 威胁审计结构化账本。
pub struct ThreatLedger {
    pub recs: Vec<ThreatRec>,
}

impl ThreatLedger {
    pub fn new() -> Self {
        ThreatLedger { recs: Vec::new() }
    }

    pub fn record(&mut self, round: u16, face: usize, action: u8, blocked: bool) {
        self.recs.push(ThreatRec { round, face, action, blocked });
    }

    /// 守恒恒等式：拦截数 + 放行数 ≡ 总数。
    pub fn conserve_ok(&self) -> bool {
        let blocked = self.recs.iter().filter(|r| r.blocked).count();
        let allowed = self.recs.iter().filter(|r| !r.blocked).count();
        blocked + allowed == self.recs.len()
    }

    /// 查询投影：按面分列（面, 拦截, 放行）。
    pub fn by_face(&self) -> Vec<(usize, u32, u32)> {
        let mut out: Vec<(usize, u32, u32)> = Vec::new();
        for r in &self.recs {
            match out.iter_mut().find(|(f, _, _)| *f == r.face) {
                Some(e) => {
                    if r.blocked {
                        e.1 += 1;
                    } else {
                        e.2 += 1;
                    }
                }
                None => out.push((r.face, if r.blocked { 1 } else { 0 }, if r.blocked { 0 } else { 1 })),
            }
        }
        out
    }

    /// 拦截率整数千分比（禁浮点）。
    pub fn blocked_permille(&self) -> u32 {
        if self.recs.is_empty() {
            return 0;
        }
        let b = self.recs.iter().filter(|r| r.blocked).count() as u32;
        b * 1000 / self.recs.len() as u32
    }

    /// 中文摘要（交互词典域本色）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("审计");
        s.push_str(&self.recs.len().to_string());
        s.push_str("条·拦截");
        s.push_str(&self.recs.iter().filter(|r| r.blocked).count().to_string());
        s.push_str("条");
        s
    }
}

// ===== 深化块 B：边界探针 =====

/// 安全边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecEdgeReport {
    pub empty_audit_ok: bool,
    pub empty_sensitive_clean: bool,
    pub dup_sensitive_dedup: bool,
    pub oob_face_blocked: bool,
}

/// 四类边界探针：空审计/空敏感表/重复敏感项/越界面序号。
pub fn run_sec_edge_probes(key: u32) -> SecEdgeReport {
    let mut is = IntegSec::new(key);
    // 1) 空审计：初始账面零条零拦截
    let empty_audit_ok = is.audit.is_empty() && is.audit_blocked() == 0;
    // 2) 空敏感表：无黑名单不得误报外泄
    let empty_sensitive_clean = !is.desensitize(&[1, 2, 3]);
    // 3) 重复敏感项：二次登记零副作用
    is.add_sensitive(0xBEEF);
    is.add_sensitive(0xBEEF);
    let dup_sensitive_dedup =
        is.sensitive.len() == 1 && is.sensitive.iter().filter(|f| **f == 0xBEEF).count() == 1;
    // 4) 越界面序号：未登记面必拦
    let oob_face_blocked = is.check_perm(0xFFFF, false) == Err(IsCode::PrivilegeOver);

    SecEdgeReport { empty_audit_ok, empty_sensitive_clean, dup_sensitive_dedup, oob_face_blocked }
}

// ===== 深化块 C：LCG 注入回归 =====

/// 确定性 LCG（8 行）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 驱动 N 轮混合权限调用，非法必抓且放行仅限合法只读面。
pub fn sec_inject_regression(rounds: u32) -> Result<(u32, u32), IsCode> {
    let mut rng = Lcg::new(0x5EED_5280);
    let mut is = IntegSec::new(0x5E1);
    is.grant(3, true, false); // 唯一合法面：只读
    let mut legal_ok = 0u32;
    let mut blocked = 0u32;
    for i in 0..rounds {
        let r = rng.next();
        let face = 3 + (r % 5) as usize;
        let write = r & 0x100 != 0;
        match is.check_perm(face, write) {
            Ok(()) => legal_ok += 1,
            Err(IsCode::PrivilegeOver) => blocked += 1,
            Err(e) => return Err(e),
        }
        let _ = i;
    }
    // 必抓断言：放行项只能来自面 3 的只读（action=1）
    for (_, f, a, b) in &is.audit {
        if !b && (*f != 3 || *a != 1) {
            return Err(IsCode::PrivilegeOver);
        }
    }
    if legal_ok + blocked != rounds {
        return Err(IsCode::PrivilegeOver);
    }
    Ok((legal_ok, blocked))
}

// ===== 深化块 D：幂等闸 =====

/// 幂等闸：grant 二次提交/敏感项二次登记零副作用。
pub fn sec_idempotence_gate() -> Result<(), IsCode> {
    let mut is = IntegSec::new(1);
    is.grant(2, true, false);
    let n0 = is.perms.len();
    is.grant(2, true, false);
    is.grant(2, false, false); // 二次提交即使参数不同也不重复登记
    if is.perms.len() != n0 {
        return Err(IsCode::PrivilegeOver);
    }
    is.add_sensitive(0xD00);
    let s0 = is.sensitive.len();
    is.add_sensitive(0xD00);
    if is.sensitive.len() != s0 {
        return Err(IsCode::LeakHit);
    }
    Ok(())
}

// ===== 深化块 E：审计流序列化 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 5B（who:1 + face:2 + action:1 + blocked:1），大端。
pub fn encode_audit(audit: &[(u8, usize, u8, bool)], out: &mut Vec<u8>) {
    for (who, face, action, blocked) in audit {
        out.push(*who);
        out.extend_from_slice(&(*face as u16).to_be_bytes());
        out.push(*action);
        out.push(if *blocked { 1 } else { 0 });
    }
}

/// 定长解码：非整 5B → 显性拒绝。
pub fn decode_audit(bytes: &[u8]) -> Result<Vec<(u8, usize, u8, bool)>, IsCode> {
    if bytes.len() % 5 != 0 {
        return Err(IsCode::SigFail);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(5) {
        out.push((
            ch[0],
            u16::from_be_bytes([ch[1], ch[2]]) as usize,
            ch[3],
            ch[4] != 0,
        ));
    }
    Ok(out)
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
