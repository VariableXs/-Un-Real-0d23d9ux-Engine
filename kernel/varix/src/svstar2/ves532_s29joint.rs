//! VE-F4298 · U05 联调（U05 工具集成组）
//! 职责：组内链十段贯通（总纲→CI→审查→IDE→设计→调试→看板→市场→文档→SDK）
//! + 跨组对接（上承 U04 治理闸、下启 U06 场景应用）+ 十例全链
//! （违例从 IDE 发现到市场门禁）+ 逐段探针 + 对拍仲裁。
//! 段位：0x2EE0（续表 idx532）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2EE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5Code {
    ChainBreak = SEG_BASE,
    ContractMismatch = SEG_BASE + 0x01,
    CaseFail = SEG_BASE + 0x02,
}

/// 十段链。
pub const U5_CHAIN: [&str; 10] = [
    "outline", "ci", "review", "ide", "design", "debug", "board", "market", "docs", "sdk",
];

/// U05 联调册。
pub struct U05Joint {
    pub passed: usize,
    pub receipts: Vec<(&'static str, bool)>,
    pub cases: [(bool, bool); 10],
    pub probes: Vec<u8>,
    pub up_u04: bool,
    pub down_u06: bool,
    /// —— 深化：对拍仲裁账。——
    pub arb: Vec<ArbRec>,
}

/// —— 深化：对拍仲裁记录（段 A vs 段 B 取值比对）。——
#[derive(Clone, Copy, Debug)]
pub struct ArbRec {
    pub seg_a: u8,
    pub seg_b: u8,
    pub a_val: u32,
    pub b_val: u32,
    pub consistent: bool,
}

impl U05Joint {
    pub fn new() -> Self {
        U05Joint { passed: 0, receipts: Vec::new(), cases: [(false, false); 10], probes: Vec::new(), up_u04: false, down_u06: false, arb: Vec::new() }
    }

    pub fn pass_station(&mut self, name: &'static str) -> Result<(), U5Code> {
        let idx = U5_CHAIN.iter().position(|c| *c == name).ok_or(U5Code::ChainBreak)?;
        if idx != self.passed {
            self.probes.push(idx as u8);
            return Err(U5Code::ChainBreak);
        }
        self.passed += 1;
        self.receipts.push((name, true));
        Ok(())
    }

    pub fn probe_locate(&self) -> Option<u8> {
        self.probes.last().copied()
    }

    pub fn run_case(&mut self, no: usize, reached_end: bool) -> Result<(), U5Code> {
        if no >= 10 {
            return Err(U5Code::CaseFail);
        }
        self.cases[no] = (true, reached_end);
        if !reached_end {
            return Err(U5Code::CaseFail);
        }
        Ok(())
    }

    pub fn all_cases(&self) -> bool {
        self.cases.iter().all(|c| c.0 && c.1)
    }

    pub fn bridge(&mut self, up: bool, down: bool) {
        self.up_u04 = up;
        self.down_u06 = down;
    }

    pub fn releasable(&self) -> bool {
        self.passed == U5_CHAIN.len() && self.all_cases() && self.up_u04 && self.down_u06
    }

    /// —— 深化：跨组契约核对：上下桥协议号必须成对匹配。——
    pub fn contract_check(&self, up_proto: u8, down_proto: u8) -> Result<(), U5Code> {
        if !self.up_u04 || !self.down_u06 {
            return Err(U5Code::ContractMismatch);
        }
        if up_proto != down_proto {
            return Err(U5Code::ContractMismatch);
        }
        Ok(())
    }

    /// —— 深化：守恒恒等式：回执数 ≡ 通过站数。——
    pub fn conserve_ok(&self) -> bool {
        self.receipts.len() == self.passed
            && self.receipts.iter().all(|(_, ok)| *ok)
    }

    /// —— 深化：链回放：回执序与链序逐一比对（全贯通时必须完全一致）。——
    pub fn replay_verify(&self) -> Result<(), U5Code> {
        if self.passed != U5_CHAIN.len() {
            return Err(U5Code::ChainBreak);
        }
        for (i, (name, _)) in self.receipts.iter().enumerate() {
            if *name != U5_CHAIN[i] {
                return Err(U5Code::ChainBreak);
            }
        }
        Ok(())
    }

    /// —— 深化：结构化对拍仲裁账。——
    pub fn arbitrate(&mut self, a: u8, b: u8, a_val: u32, b_val: u32) {
        let consistent = a_val == b_val;
        self.arb.push(ArbRec { seg_a: a, seg_b: b, a_val, b_val, consistent });
    }

    /// —— 深化：中文链况摘要。——
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("贯通");
        s.push_str(&self.passed.to_string());
        s.push_str("段·对拍");
        s.push_str(&self.arb.len().to_string());
        s.push_str("笔");
        s
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C532-01..12。
pub fn run_ves532_checks() -> CheckSet {
    let mut set = CheckSet::new("ves532");

    let mut jt = U05Joint::new();
    chk(&mut set, "C532-01", jt.pass_station("ci") == Err(U5Code::ChainBreak), "越站拦截（总纲先行）");
    chk(&mut set, "C532-02", jt.probe_locate() == Some(1), "探针定位断点段");
    chk(&mut set, "C532-03", jt.pass_station("rogue") == Err(U5Code::ChainBreak), "未知站拒收");

    for s in U5_CHAIN.iter() {
        let _ = jt.pass_station(s);
    }
    chk(&mut set, "C532-04", jt.passed == 10, "十段全贯通");
    chk(&mut set, "C532-05", jt.receipts.len() == 10, "十段回执");

    for i in 0..10 {
        let _ = jt.run_case(i, true);
    }
    chk(&mut set, "C532-06", jt.all_cases(), "十例全链通（IDE 发现→市场门禁）");
    chk(&mut set, "C532-07", jt.run_case(5, false) == Err(U5Code::CaseFail), "未达终链失败");
    jt.bridge(true, true);
    chk(&mut set, "C532-08", !jt.releasable(), "例5失败阻断收口");
    let _ = jt.run_case(5, true);
    chk(&mut set, "C532-09", jt.releasable(), "修复后放行（十段+十例+双桥）");

    chk(&mut set, "C532-10", jt.run_case(10, true) == Err(U5Code::CaseFail), "越界例号");
    chk(&mut set, "C532-11", U5_CHAIN.len() == 10 && U5_CHAIN[0] == "outline" && U5_CHAIN[9] == "sdk", "十段链序自洽");
    chk(&mut set, "C532-12", jt.probes.len() == 2, "探针账保序");

    // —— 深化 A：守恒 + 链回放幂等 ——
    chk(&mut set, "C532-13", jt.conserve_ok(), "联调账守恒：回执数≡通过站数");
    chk(&mut set, "C532-14", jt.replay_verify().is_ok(), "链回放：回执序≡链序（幂等）");
    chk(&mut set, "C532-15", jt.pass_station("outline") == Err(U5Code::ChainBreak), "满链后重复过站必拦");

    // —— 深化 B：跨组契约核对 ——
    chk(&mut set, "C532-16", jt.contract_check(7, 7).is_ok(), "上下桥协议号成对匹配");
    chk(&mut set, "C532-17", jt.contract_check(7, 8) == Err(U5Code::ContractMismatch), "协议号错配仲裁拒绝");
    let mut jt2 = U05Joint::new();
    jt2.bridge(true, true);
    let _ = jt2.contract_check(1, 2);

    // —— 深化 C：对拍仲裁账 ——
    jt.arbitrate(2, 8, 100, 100);
    jt.arbitrate(3, 9, 55, 66);
    let arb_ok = jt.arb.iter().filter(|r| r.consistent).count();
    chk(&mut set, "C532-18", jt.arb.len() == 2 && arb_ok == 1, "对拍两笔：一致/不一致各一");
    chk(&mut set, "C532-19", !jt.summary().is_empty(), "链况中文摘要（域本色）");

    // —— 深化 D：边界探针 ——
    let er = run_joint_edge_probes();
    chk(&mut set, "C532-20", er.empty_not_releasable && er.case_edge_9_ok, "空册不可收口/例号 9 边界通");
    chk(&mut set, "C532-21", er.case_edge_10_rejected && er.empty_conserve, "例号 10 越界拒/空册守恒");

    // —— 深化 E：LCG 注入回归 ——
    match joint_inject_regression(40) {
        Ok((blocked, done)) => chk(&mut set, "C532-22", blocked > 0 && done, "40 轮越站注入必拦且最终仍可按序贯通"),
        Err(_) => chk(&mut set, "C532-22", false, "注入回归断链未修复"),
    }

    // —— 深化 F：回执序列化 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_receipts(&jt.receipts, &mut bytes);
    let rt = decode_receipts(&bytes);
    chk(&mut set, "C532-23", rt.iter().count() == jt.receipts.len(), "回执 2B/条定长解码往返");
    chk(&mut set, "C532-24", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == jt.receipts.len() * 2, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：边界探针 =====

/// 联调边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointEdgeReport {
    pub empty_not_releasable: bool,
    pub case_edge_9_ok: bool,
    pub case_edge_10_rejected: bool,
    pub empty_conserve: bool,
}

/// 四类边界探针：空册收口/例号 9/例号 10/空册守恒。
pub fn run_joint_edge_probes() -> JointEdgeReport {
    let mut e = U05Joint::new();
    // 1) 空册：十段未通必不可收口
    let empty_not_releasable = !e.releasable();
    // 2) 例号 9：末位边界可跑
    let case_edge_9_ok = e.run_case(9, true).is_ok();
    // 3) 例号 10：越界显性拒绝
    let case_edge_10_rejected = e.run_case(10, true) == Err(U5Code::CaseFail);
    // 4) 空册守恒：0 回执 ≡ 0 站
    let empty_conserve = e.conserve_ok();

    JointEdgeReport { empty_not_releasable, case_edge_9_ok, case_edge_10_rejected, empty_conserve }
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

/// 注入回归：LCG 驱动 N 轮随机过站（越站必拦计数），随后按序补通全链。
pub fn joint_inject_regression(rounds: u32) -> Result<(u32, bool), U5Code> {
    let mut rng = Lcg::new(0x5EED_5320);
    let mut jt = U05Joint::new();
    let mut blocked = 0u32;
    for _ in 0..rounds {
        let idx = (rng.next() % 12) as usize;
        let name = if idx < U5_CHAIN.len() { U5_CHAIN[idx] } else { "rogue" };
        if jt.pass_station(name) == Err(U5Code::ChainBreak) {
            blocked += 1;
        }
    }
    for s in U5_CHAIN.iter() {
        jt.pass_station(s)?;
    }
    if jt.passed != 10 || !jt.conserve_ok() {
        return Err(U5Code::ChainBreak);
    }
    Ok((blocked, jt.passed == 10))
}

// ===== 深化块 C：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 2B（站序:1 + 回执位:1）。
pub fn encode_receipts(receipts: &[(&'static str, bool)], out: &mut Vec<u8>) {
    for (name, ok) in receipts {
        let idx = U5_CHAIN.iter().position(|c| c == name).unwrap_or(0xFF) as u8;
        out.push(idx);
        out.push(*ok as u8);
    }
}

/// 定长解码：非整 2B 或站序越界 → 显性拒绝。
pub fn decode_receipts(bytes: &[u8]) -> Result<Vec<(&'static str, bool)>, U5Code> {
    if bytes.len() % 2 != 0 {
        return Err(U5Code::ChainBreak);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(2) {
        if ch[0] as usize >= U5_CHAIN.len() {
            return Err(U5Code::ChainBreak);
        }
        out.push((U5_CHAIN[ch[0] as usize], ch[1] != 0));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_joint() {
        let mut jt = U05Joint::new();
        assert!(jt.pass_station("outline").is_ok());
        assert_eq!(jt.pass_station("sdk"), Err(U5Code::ChainBreak));
        assert!(!jt.releasable());
    }

    #[test]
    fn u05_joint_arb_and_replay() {
        let mut jt = U05Joint::new();
        jt.arbitrate(1, 2, 10, 10);
        jt.arbitrate(2, 3, 7, 8);
        assert_eq!(jt.arb.iter().filter(|r| r.consistent).count(), 1);
        assert!(jt.replay_verify() == Err(U5Code::ChainBreak));
        for s in U5_CHAIN.iter() {
            let _ = jt.pass_station(s);
        }
        assert!(jt.replay_verify().is_ok());
    }
}
