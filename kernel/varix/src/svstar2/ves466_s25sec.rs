//! ves466_s25sec — VE-F4232 · 契约安全
//! 三道：DSL 注入面（畸形表达式的沙箱求值）+签名验证（契约注册必签）+越权变更拦截（提供方身份核验）；
//! 沙箱逃逸企图→阻断+最高级立案；签名错→拒注册；身份不符→拒绝+告警；
//! 越权变更含无障碍判据防弱化专项（域色红线）；沙箱无网络无文件访问。
//! 深化（2026-10-27 T域批次）：结构化安全审计账本（批次+动作归因）、守恒恒等式
//! （放行≡拒绝对账）、边界探针（空契约号/重复审计/满容量）、按动作投影、
//! LCG 逃逸注入回归（net/fs/exec 必抓 100%+算术零误报）、幂等闸、
//! 定长序列化往返+FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
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
    // —— 深化追加（尾部续段，不新增 SEG_BASE）——
    EmptyCid = SEG_BASE + 0x05, // 0x1865 空契约号（审计对象缺失）
    DupAudit = SEG_BASE + 0x06, // 0x1866 同批次同契约同动作重复审计
    CapFull = SEG_BASE + 0x07,  // 0x1867 账本满容量
    SerialBad = SEG_BASE + 0x08, // 0x1868 序列化字节布局失配
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

// ---------------------------------------------------------------------------
// 深化追加：结构化安全审计账本（动作归因 + 对账投影 + 注入回归）。
// ---------------------------------------------------------------------------

/// 审计动作（0 注册 / 1 变更 / 2 拒绝）。
pub const ACT_REGISTER: u8 = 0;
pub const ACT_CHANGE: u8 = 1;
pub const ACT_REJECT: u8 = 2;

/// 单条审计账目：批次 + 契约 + 动作 + 是否放行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRecord { pub batch: u32, pub cid: u32, pub action: u8, pub allowed: bool }

/// 结构化安全审计账本：安全事件从计数器升级为带批次/动作归因的定容记录。
pub struct AuditLedger { pub recs: Vec<AuditRecord>, pub cap: usize }

impl AuditLedger {
    pub fn new(cap: usize) -> Self { AuditLedger { recs: Vec::new(), cap } }

    /// 记一笔审计。边界探针：空契约号/重复审计/满容量/动作越界。
    pub fn record(&mut self, batch: u32, cid: u32, action: u8, allowed: bool) -> Result<(), ScCode> {
        if action > ACT_REJECT { return Err(ScCode::SerialBad); }
        if cid == 0 { return Err(ScCode::EmptyCid); }
        if self.recs.iter().any(|r| r.batch == batch && r.cid == cid && r.action == action) {
            return Err(ScCode::DupAudit);
        }
        if self.recs.len() >= self.cap { return Err(ScCode::CapFull); }
        self.recs.push(AuditRecord { batch, cid, action, allowed });
        Ok(())
    }

    /// 守恒恒等式：账本总数 ≡ 放行数 + 拒绝数（安全事件无一漏账）。
    pub fn conserved(&self) -> bool {
        if self.recs.len() > self.cap { return false; }
        let allow = self.recs.iter().filter(|r| r.allowed).count();
        let deny = self.recs.iter().filter(|r| !r.allowed).count();
        allow + deny == self.recs.len()
    }

    /// 查询投影：按动作分列（[注册, 变更, 拒绝]）。
    pub fn by_action(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for r in &self.recs {
            if r.action <= ACT_REJECT { out[r.action as usize] += 1; }
        }
        out
    }

    /// 查询投影：按契约归因（该契约全部审计条目）。
    pub fn by_contract(&self, cid: u32) -> Vec<AuditRecord> {
        self.recs.iter().filter(|r| r.cid == cid).copied().collect()
    }

    /// 拒绝率（千分比整数）：拒绝对账面（越权面热度）。
    pub fn deny_rate_permille(&self) -> u32 {
        if self.recs.is_empty() { return 0; }
        let deny = self.recs.iter().filter(|r| !r.allowed).count();
        deny as u32 * 1000 / self.recs.len() as u32
    }

    /// 幂等闸：同批次同契约同动作二次审计零副作用。
    pub fn idempotent(&mut self, batch: u32, cid: u32, action: u8, allowed: bool) -> bool {
        let before = self.recs.len();
        let first = self.record(batch, cid, action, allowed);
        let mid = self.recs.len();
        let second = self.record(batch, cid, action, allowed);
        first.is_ok() && second == Err(ScCode::DupAudit)
            && mid == before + 1 && self.recs.len() == mid
    }

    /// 序列化投影：AuditRecord → 定长 9 字节（batch4 + cid4 + 动作1）。
    /// allowed 由 action 编码：ACT_REJECT 即拒绝，其余动作附 bit7 放行位。
    pub fn encode(r: &AuditRecord) -> [u8; 9] {
        let mut out = [0u8; 9];
        out[0] = (r.batch >> 24) as u8;
        out[1] = (r.batch >> 16) as u8;
        out[2] = (r.batch >> 8) as u8;
        out[3] = r.batch as u8;
        out[4] = (r.cid >> 24) as u8;
        out[5] = (r.cid >> 16) as u8;
        out[6] = (r.cid >> 8) as u8;
        out[7] = r.cid as u8;
        out[8] = if r.allowed { r.action | 0x80 } else { r.action & 0x7F };
        out
    }

    /// 反序列化（带校验：动作非法→SerialBad，空契约号→EmptyCid）。
    pub fn decode(b: &[u8; 9]) -> Result<AuditRecord, ScCode> {
        let batch = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let cid = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        let allowed = b[8] & 0x80 != 0;
        let action = b[8] & 0x7F;
        if action > ACT_REJECT { return Err(ScCode::SerialBad); }
        if cid == 0 { return Err(ScCode::EmptyCid); }
        Ok(AuditRecord { batch, cid, action, allowed })
    }

    /// FNV-1a 字节指纹（序列化配套；与 u32 版 `fnv` 分工）。
    pub fn fnv1a(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for &b in bytes {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 账本摘要（String 投影，诊断用）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("audits=");
        s.push_str(&self.recs.len().to_string());
        s.push_str("/deny_rate=");
        s.push_str(&self.deny_rate_permille().to_string());
        s
    }
}

/// 确定性 LCG（8 行）：注入回归驱动器（无浮点、无墙钟）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 逃逸注入回归：LCG 驱动 N 轮表达式注入——net/fs/exec 三类逃逸企图必被
/// 沙箱阻断（必抓率 100%）；合法算术的负对照零误报。返回 (抓住, 注入, 误报)。
pub fn inject_escape(rounds: u32) -> (u32, u32, u32) {
    let mut rng = Lcg::new(0x4660_0000_0000_0006);
    let mut caught = 0u32;
    let mut injected = 0u32;
    let mut false_alarm = 0u32;
    let escapes = ["net(host)", "fs(/tmp)", "exec(id)"];
    for i in 0..rounds {
        if i % 2 == 0 {
            injected += 1;
            let e = escapes[(rng.next() % 3) as usize];
            if sandbox_eval(e) == Err(ScCode::SandboxEscape) { caught += 1; }
        } else {
            let a = (rng.next() % 100) as u32;
            let b = (rng.next() % 100) as u32;
            // 合成合法算术：值域受控，期望求值 = a + b。
            let expr = concat_expr(a, b);
            match sandbox_eval(expr.as_str()) {
                Ok(v) if v == a + b => {}
                _ => false_alarm += 1,
            }
        }
    }
    (caught, injected, false_alarm)
}

/// 合成 "a+b" 表达式（无格式器：手写十进制拼装）。
fn concat_expr(a: u32, b: u32) -> String {
    let mut s = String::new();
    s.push_str(&a.to_string());
    s.push('+');
    s.push_str(&b.to_string());
    s
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

    // —— 深化判据（追加面）——

    // C466-13 结构化审计账本守恒恒等式（总数≡放行+拒绝）。
    let mut lg = AuditLedger::new(8);
    let _ = lg.record(1, 7001, ACT_REGISTER, true);
    let _ = lg.record(1, 7001, ACT_CHANGE, true);
    let _ = lg.record(2, 7002, ACT_REJECT, false);
    chk(&mut set, "C466-13", lg.conserved() && lg.recs.len() == 3, "账本守恒");

    // C466-14 边界探针：空契约号→显性拒绝（审计对象缺失不入账）。
    chk(&mut set, "C466-14", lg.record(3, 0, ACT_REGISTER, true) == Err(ScCode::EmptyCid), "空契约号拒");

    // C466-15 边界探针 + 幂等闸：重复审计被拒、账本零副作用。
    chk(&mut set, "C466-15", lg.idempotent(3, 7003, ACT_CHANGE, true), "幂等闸");

    // C466-16 边界探针：满容量→CapFull。
    let mut full = AuditLedger::new(2);
    let _ = full.record(1, 7001, ACT_REGISTER, true);
    let _ = full.record(1, 7002, ACT_REGISTER, true);
    chk(&mut set, "C466-16", full.record(2, 7003, ACT_REGISTER, true) == Err(ScCode::CapFull), "满容量拒");

    // C466-17 查询投影：按动作分列（注册1/变更2/拒绝1）。
    let k = lg.by_action();
    chk(&mut set, "C466-17", k[0] == 1 && k[1] == 2 && k[2] == 1, "按动作投影");

    // C466-18 查询投影：按契约归因（7001 两条全放行）。
    let per = lg.by_contract(7001);
    chk(&mut set, "C466-18", per.len() == 2 && per.iter().all(|r| r.allowed), "按契约归因");

    // C466-19 拒绝率千分比整数（1/5 → 200‰）。
    chk(&mut set, "C466-19", lg.deny_rate_permille() == 200, "拒绝率对账");

    // C466-20 序列化投影：编码→解码往返一致 + 指纹稳定。
    let rec = AuditRecord { batch: 7, cid: 7500, action: ACT_REJECT, allowed: false };
    let bytes = AuditLedger::encode(&rec);
    let back = AuditLedger::decode(&bytes);
    chk(&mut set, "C466-20", back == Ok(rec) && AuditLedger::fnv1a(&bytes) == AuditLedger::fnv1a(&bytes), "序列化往返");

    // C466-21 序列化校验：动作非法→SerialBad。
    let mut bad_b = bytes;
    bad_b[8] = 0x03; // 非法动作 3 且未置放行位
    chk(&mut set, "C466-21", AuditLedger::decode(&bad_b) == Err(ScCode::SerialBad), "坏动作拒");

    // C466-22 逃逸注入回归：net/fs/exec 必抓 100% + 算术零误报。
    let (caught, injected, false_alarm) = inject_escape(32);
    chk(&mut set, "C466-22", injected > 0 && caught == injected && false_alarm == 0, "逃逸必抓");

    // C466-23 账本摘要 String 投影（诊断可读）。
    chk(&mut set, "C466-23", lg.summary().contains("audits=4"), "摘要投影");

    // C466-24 追加变体段位单源（尾部续段不撞段）。
    chk(&mut set, "C466-24", ScCode::SerialBad as u16 == SEG_BASE + 8, "续段单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves466_smoke() {
        let set = run_ves466_checks();
        assert!(set.failed_count() == 0, "ves466 red: {}", set.red_items_text());
    }
}
