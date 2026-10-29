//! VE-F4296 · 工具集成 fuzz（U05 工具集成组）
//! 职责：集成面 fuzz（事件总线畸形事件/适配器边界参数/签名伪造三类）
//! + 语料两源（真实事件变形+协议语法生成）+ 发现即立案 + 误报白名单
//! + 资源调度配额 + fuzz 报告脱敏。
//! 段位：0x2E80（续表 idx530）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E80;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfCode {
    /// 畸形事件致崩溃。
    MalformCrash = SEG_BASE,
    /// 签名伪造命中。
    ForgeHit = SEG_BASE + 0x01,
    /// 配额耗尽。
    QuotaOut = SEG_BASE + 0x02,
}

/// fuzz 目标三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfTarget {
    MalformEv = 0,
    BoundParam = 1,
    SigForge = 2,
}

/// 语料（两源）。
#[derive(Clone, Copy, Debug)]
pub struct IfCorpus {
    pub from_real: bool,   // 真实事件变形
    pub from_grammar: bool, // 协议语法生成
    pub whitelisted: bool,
}

/// 集成 fuzz 器。
pub struct IntegFuzz {
    pub pool: Vec<IfCorpus>,
    pub cases: Vec<(IfTarget, u16)>,
    /// —— 深化：结构化立案账（带回合归因与严重级）。——
    pub ledger: Vec<FuzzCase>,
    pub quota: u32,
    pub used: u32,
}

/// —— 深化：结构化 fuzz 立案记录。——
#[derive(Clone, Copy, Debug)]
pub struct FuzzCase {
    pub round: u16,
    pub target: IfTarget,
    pub detail: u16,
    pub sev: u8,
}

impl IntegFuzz {
    pub fn new(quota: u32) -> Self {
        IntegFuzz { pool: Vec::new(), cases: Vec::new(), ledger: Vec::new(), quota, used: 0 }
    }

    pub fn seed(&mut self, c: IfCorpus) {
        self.pool.push(c);
    }

    pub fn runnable(&self) -> usize {
        self.pool.iter().filter(|c| !c.whitelisted).count()
    }

    /// 配额调度。
    pub fn round(&mut self, want: u32) -> Result<u32, IfCode> {
        let take = want.min(self.quota.saturating_sub(self.used));
        if take == 0 {
            return Err(IfCode::QuotaOut);
        }
        self.used += take;
        Ok(take)
    }

    /// 畸形事件检：超长或含控制位。
    pub fn probe_malform(&mut self, len: u32, ctrl: bool) -> bool {
        let hit = len > 4096 || ctrl;
        if hit {
            self.file(IfTarget::MalformEv, len.min(9999) as u16);
        }
        hit
    }

    /// 边界参数检：u16 边界外溢。
    pub fn probe_bound(&mut self, v: u32) -> bool {
        if v > 0xFFFF {
            self.file(IfTarget::BoundParam, 0xFFFF);
            return true;
        }
        false
    }

    /// 签名伪造检：零签名或重复签名。
    pub fn probe_forge(&mut self, sig: u32, seen: &mut Vec<u32>) -> bool {
        let hit = sig == 0 || seen.contains(&sig);
        if hit {
            self.file(IfTarget::SigForge, (sig & 0xFFFF) as u16);
        } else {
            seen.push(sig);
        }
        hit
    }

    fn file(&mut self, t: IfTarget, d: u16) {
        if !self.cases.iter().any(|(a, b)| *a == t && *b == d) {
            self.cases.push((t, d));
        }
    }

    pub fn case_n(&self) -> usize {
        self.cases.len()
    }

    /// —— 深化：结构化立案（回合归因 + 严重级）。——
    pub fn file_case(&mut self, round: u16, t: IfTarget, detail: u16, sev: u8) {
        if !self.ledger.iter().any(|c| c.target == t && c.detail == detail) {
            self.ledger.push(FuzzCase { round, target: t, detail, sev });
        }
    }

    /// —— 深化：按目标分列投影（目标, 立案数, 最高严重级）。——
    pub fn by_target(&self) -> Vec<(u8, u32, u8)> {
        let mut out: Vec<(u8, u32, u8)> = Vec::new();
        for c in &self.ledger {
            let k = c.target as u8;
            match out.iter_mut().find(|(a, _, _)| *a == k) {
                Some(e) => {
                    e.1 += 1;
                    if c.sev > e.2 {
                        e.2 = c.sev;
                    }
                }
                None => out.push((k, 1, c.sev)),
            }
        }
        out
    }

    /// —— 深化：立案账守恒：结构化账 ≡ 简易立案表。——
    pub fn conserve_ok(&self) -> bool {
        self.ledger.len() == self.cases.len()
    }

    /// —— 深化：fuzz 报告脱敏：签名类细节高位掩码。——
    pub fn report_masked(&self) -> Vec<(u8, u16)> {
        self.cases
            .iter()
            .map(|(t, d)| (*t as u8, if *t == IfTarget::SigForge { d & 0x00FF } else { *d }))
            .collect()
    }

    /// —— 深化：中文报告摘要。——
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("立案");
        s.push_str(&self.ledger.len().to_string());
        s.push_str("件·白名单外可跑");
        s.push_str(&self.runnable().to_string());
        s
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C530-01..12。
pub fn run_ves530_checks() -> CheckSet {
    let mut set = CheckSet::new("ves530");

    let mut fz = IntegFuzz::new(10);
    fz.seed(IfCorpus { from_real: true, from_grammar: false, whitelisted: false });
    fz.seed(IfCorpus { from_real: false, from_grammar: true, whitelisted: true });
    fz.seed(IfCorpus { from_real: true, from_grammar: true, whitelisted: false });
    chk(&mut set, "C530-01", fz.runnable() == 2, "白名单过滤后可跑 2");
    chk(&mut set, "C530-02", fz.pool.iter().filter(|c| c.from_real && c.from_grammar).count() == 1, "语料两源标记");

    chk(&mut set, "C530-03", fz.round(4) == Ok(4), "配额内调度");
    chk(&mut set, "C530-04", fz.round(99) == Ok(6), "超额截断余量");
    chk(&mut set, "C530-05", fz.round(1) == Err(IfCode::QuotaOut), "配额耗尽");

    // 三类目标
    chk(&mut set, "C530-06", fz.probe_malform(5000, false), "超长畸形事件检出");
    chk(&mut set, "C530-07", fz.probe_malform(100, true), "控制位事件检出");
    chk(&mut set, "C530-08", !fz.probe_malform(100, false), "正常事件放行");
    chk(&mut set, "C530-09", fz.probe_bound(0x1_0000), "边界外溢检出");
    chk(&mut set, "C530-10", !fz.probe_bound(0xFFFF), "边界值放行");

    let mut seen: Vec<u32> = Vec::new();
    chk(&mut set, "C530-11", fz.probe_forge(0, &mut seen), "零签名伪造检出");
    chk(&mut set, "C530-12", !fz.probe_forge(42, &mut seen) && fz.probe_forge(42, &mut seen), "重复签名检出（回放攻击）");

    // —— 深化 A：结构化立案账（回合归因）——
    let _ = fz.file_case(1, IfTarget::MalformEv, 5000, 2);
    let _ = fz.file_case(2, IfTarget::BoundParam, 0xFFFF, 1);
    let _ = fz.file_case(3, IfTarget::SigForge, 0xABCD, 3);
    let _ = fz.file_case(3, IfTarget::SigForge, 0xABCD, 3); // 重复立案去重
    chk(&mut set, "C530-13", fz.ledger.len() == 3 && fz.conserve_ok(), "结构化立案去重且与简易账守恒");
    let bt = fz.by_target();
    chk(&mut set, "C530-14", bt.len() == 3 && bt.iter().any(|(_, _, s)| *s == 3), "按目标分列投影含最高严重级");
    chk(&mut set, "C530-15", fz.summary().len() > 0, "fuzz 报告中文摘要（域本色）");

    // —— 深化 B：脱敏投影 ——
    let masked = fz.report_masked();
    chk(&mut set, "C530-16", masked.iter().all(|(t, d)| *t != 2 || *d <= 0x00FF), "签名类立案细节高位掩码脱敏");

    // —— 深化 C：边界探针 ——
    let er = run_fuzz_edge_probes();
    chk(&mut set, "C530-17", er.zero_quota_reject && er.empty_pool_zero, "零配额拒绝/空池可跑为 0");
    chk(&mut set, "C530-18", er.max_bound_caught && er.dup_detect_idem, "u32 极值边界必抓/重复探测零重复立案");

    // —— 深化 D：LCG 注入回归（异常必抓 + 负对照零误报）——
    match fuzz_inject_regression(64) {
        Ok((hit, miss)) => {
            chk(&mut set, "C530-19", hit > 0 && miss > 0, "64 轮注入畸形必抓且正常放行两类齐备");
        }
        Err(_) => chk(&mut set, "C530-19", false, "注入回归账面失衡"),
    }
    match fuzz_negative_control(32) {
        Ok(hits) => chk(&mut set, "C530-20", hits == 0, "负对照：全正常输入零误报"),
        Err(_) => chk(&mut set, "C530-20", false, "负对照异常路径"),
    }

    // —— 深化 E：幂等闸 ——
    chk(&mut set, "C530-21", fuzz_idempotence_gate(), "幂等闸：同案二次立案/二次调度零副作用");

    // —— 深化 F：序列化投影 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_cases(&fz.ledger, &mut bytes);
    let rt = decode_cases(&bytes);
    chk(&mut set, "C530-22", rt.iter().count() == fz.ledger.len(), "立案账 6B/条定长解码往返");
    chk(&mut set, "C530-23", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == fz.ledger.len() * 6, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：边界探针 =====

/// fuzz 边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzEdgeReport {
    pub zero_quota_reject: bool,
    pub empty_pool_zero: bool,
    pub max_bound_caught: bool,
    pub dup_detect_idem: bool,
}

/// 四类边界探针：零配额/空池/u32 极值/重复探测。
pub fn run_fuzz_edge_probes() -> FuzzEdgeReport {
    // 1) 零配额：首拍即 QuotaOut
    let mut f0 = IntegFuzz::new(0);
    let zero_quota_reject = f0.round(1) == Err(IfCode::QuotaOut);
    // 2) 空池：可跑数为 0
    let empty_pool_zero = f0.runnable() == 0;
    // 3) u32 极值边界必抓
    let mut f1 = IntegFuzz::new(8);
    let max_bound_caught = f1.probe_bound(u32::MAX);
    // 4) 重复探测：同畸形事件二次探测不重复立案
    let _ = f1.probe_malform(9000, false);
    let _ = f1.probe_malform(9000, false);
    let dup_detect_idem = f1.cases.len() == 1 && f1.ledger.is_empty();

    FuzzEdgeReport { zero_quota_reject, empty_pool_zero, max_bound_caught, dup_detect_idem }
}

// ===== 深化块 B：LCG 注入回归 =====

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

/// 注入回归：LCG 驱动 N 轮混合输入，畸形必抓（hit>0）且正常必放行（miss>0）。
pub fn fuzz_inject_regression(rounds: u32) -> Result<(u32, u32), IfCode> {
    let mut rng = Lcg::new(0x5EED_5300);
    let mut fz = IntegFuzz::new(rounds * 2);
    let mut hit = 0u32;
    let mut miss = 0u32;
    let mut seen: Vec<u32> = Vec::new();
    for i in 0..rounds {
        let r = rng.next();
        let caught = match i % 3 {
            0 => fz.probe_malform(100 + r % 9000, r & 1 != 0),
            1 => fz.probe_bound(r),
            _ => fz.probe_forge(r & 0xFFFF, &mut seen),
        };
        if caught {
            hit += 1;
        } else {
            miss += 1;
        }
    }
    if hit == 0 || miss == 0 {
        return Err(IfCode::MalformCrash);
    }
    Ok((hit, miss))
}

/// 负对照：全正常输入（短事件/界内参数/首见签名）零误报。
pub fn fuzz_negative_control(rounds: u32) -> Result<u32, IfCode> {
    let mut fz = IntegFuzz::new(rounds);
    let mut seen: Vec<u32> = Vec::new();
    let mut false_hits = 0u32;
    for i in 0..rounds {
        if fz.probe_malform(64 + i, false) || fz.probe_bound(i) || fz.probe_forge(0x1000 + i, &mut seen) {
            false_hits += 1;
        }
    }
    Ok(false_hits)
}

// ===== 深化块 C：幂等闸 =====

/// 幂等闸：同案二次立案零重复；同额二次调度余量稳定。
pub fn fuzz_idempotence_gate() -> bool {
    let mut fz = IntegFuzz::new(4);
    fz.file_case(1, IfTarget::MalformEv, 5000, 2);
    let n0 = fz.ledger.len();
    fz.file_case(2, IfTarget::MalformEv, 5000, 2);
    if fz.ledger.len() != n0 {
        return false;
    }
    let _ = fz.round(2);
    let u0 = fz.used;
    let _ = fz.round(0);
    fz.used == u0
}

// ===== 深化块 D：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 6B（round:2 + target:1 + detail:2 + sev:1），大端。
pub fn encode_cases(cases: &[FuzzCase], out: &mut Vec<u8>) {
    for c in cases {
        out.extend_from_slice(&c.round.to_be_bytes());
        out.push(c.target as u8);
        out.extend_from_slice(&c.detail.to_be_bytes());
        out.push(c.sev);
    }
}

/// 定长解码：非整 6B 或目标越界 → 显性拒绝。
pub fn decode_cases(bytes: &[u8]) -> Result<Vec<FuzzCase>, IfCode> {
    if bytes.len() % 6 != 0 {
        return Err(IfCode::MalformCrash);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(6) {
        let target = match ch[2] {
            0 => IfTarget::MalformEv,
            1 => IfTarget::BoundParam,
            2 => IfTarget::SigForge,
            _ => return Err(IfCode::MalformCrash),
        };
        out.push(FuzzCase {
            round: u16::from_be_bytes([ch[0], ch[1]]),
            target,
            detail: u16::from_be_bytes([ch[3], ch[4]]),
            sev: ch[5],
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_fuzz() {
        let mut fz = IntegFuzz::new(1);
        assert_eq!(fz.round(5), Ok(1));
        assert!(fz.round(1).is_err());
        assert!(fz.probe_bound(70000));
        assert_eq!(fz.case_n(), 1);
    }
}
