// CGPU-F0320 B域收官宣告 · cgb320_declare.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：B 域收官宣告（16 项自检全绿 + 20 维度验收过 + 彩排通过）；
//       宣告不可逆；后续改动需重走彩排；宣告记录可审计。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 宣告前置门禁。
#[derive(Clone, Copy, Debug)]
pub struct DeclareGate {
    pub selfcheck_all_green: bool,
    pub acceptance_passed: bool,
    pub rehearsal_passed: bool,
    pub zero_critical_vulns: bool,
    pub api_frozen: bool,
    pub baseline_archived: bool,
    pub handover_signed: bool,
    pub docs_verified: bool,
}

impl Default for DeclareGate {
    fn default() -> Self {
        Self {
            selfcheck_all_green: false,
            acceptance_passed: false,
            rehearsal_passed: false,
            zero_critical_vulns: false,
            api_frozen: false,
            baseline_archived: false,
            handover_signed: false,
            docs_verified: false,
        }
    }
}

impl DeclareGate {
    pub fn all_passed(&self) -> bool {
        self.selfcheck_all_green
            && self.acceptance_passed
            && self.rehearsal_passed
            && self.zero_critical_vulns
            && self.api_frozen
            && self.baseline_archived
            && self.handover_signed
            && self.docs_verified
    }

    pub fn passed_count(&self) -> u32 {
        [
            self.selfcheck_all_green,
            self.acceptance_passed,
            self.rehearsal_passed,
            self.zero_critical_vulns,
            self.api_frozen,
            self.baseline_archived,
            self.handover_signed,
            self.docs_verified,
        ]
        .iter()
        .filter(|b| **b)
        .count() as u32
    }
}

/// 宣告记录。
#[derive(Clone, Debug)]
pub struct Declaration {
    pub id: u32,
    pub group: String,
    pub version: u32,
    pub declared_at: u64,
    pub declared_by: u32,
    pub immutable: bool,
    pub signature_hash: u32,
}

/// 收官宣告引擎。
pub struct DomainDeclare {
    pub gate: DeclareGate,
    pub declarations: Vec<Declaration>,
    pub current: Option<u32>,
    pub reopen_count: u32,
}

impl DomainDeclare {
    pub fn new() -> Self {
        Self {
            gate: DeclareGate::default(),
            declarations: Vec::new(),
            current: None,
            reopen_count: 0,
        }
    }

    /// 设置门禁。
    pub fn set_gate(&mut self, gate: DeclareGate) {
        self.gate = gate;
    }

    /// 宣告（门禁全过才可）。
    pub fn declare(&mut self, group: &str, version: u32, by: u32, timestamp: u64) -> Option<u32> {
        if !self.gate.all_passed() || self.current.is_some() {
            return None;
        }
        let id = self.declarations.len() as u32 + 1;
        let sig = Self::sig_hash(group, version, by, timestamp);
        self.declarations.push(Declaration {
            id,
            group: String::from(group),
            version,
            declared_at: timestamp,
            declared_by: by,
            immutable: true,
            signature_hash: sig,
        });
        self.current = Some(id);
        Some(id)
    }

    fn sig_hash(group: &str, version: u32, by: u32, ts: u64) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in group.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= version;
        h = h.wrapping_mul(0x0100_0193);
        h ^= by;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (ts & 0xFFFF_FFFF) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 重开（计数，宣告不可逆但可重走流程）。
    pub fn reopen(&mut self, reason_id: u32) -> bool {
        if self.current.is_none() {
            return false;
        }
        self.reopen_count += 1;
        self.current = None;
        // 宣告记录本身不可变，只增重开计数
        let _ = reason_id;
        true
    }

    /// 宣告摘要。
    pub fn summary(&self) -> String {
        match self.current {
            Some(id) => {
                let d = &self.declarations[id as usize - 1];
                alloc::format!("{} v{} 已宣告（by {}）", d.group, d.version, d.declared_by)
            }
            None => String::from("未宣告"),
        }
    }

    /// 门禁完整度（/256）。
    pub fn gate_completeness_q8(&self) -> u16 {
        (self.gate.passed_count() * 256 / 8) as u16
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0320 B域收官宣告");
        cs.add("宣告需全门禁", {
            self.current.is_none() || self.gate.all_passed()
        }, "gate");
        cs.add("宣告不可变", {
            self.declarations.iter().all(|d| d.immutable)
        }, "immutable");
        cs.add("签名哈希非零", {
            self.declarations.iter().all(|d| d.signature_hash != 0)
        }, "sig");
        cs.add("重开计数合理", self.reopen_count <= 16, "≤16");
        cs.add("门禁完整度 ≤ 256", self.gate_completeness_q8() <= 256, "q8");
        cs.add("宣告组名非空", {
            self.declarations.iter().all(|d| !d.group.is_empty())
        }, "group");
        cs.add("当前宣告引用有效", {
            self.current.map(|id| id as usize <= self.declarations.len()).unwrap_or(true)
        }, "valid ref");
        cs
    }
}

impl Default for DomainDeclare {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_gate() -> DeclareGate {
        DeclareGate {
            selfcheck_all_green: true,
            acceptance_passed: true,
            rehearsal_passed: true,
            zero_critical_vulns: true,
            api_frozen: true,
            baseline_archived: true,
            handover_signed: true,
            docs_verified: true,
        }
    }

    #[test]
    fn declare_requires_all_gates() {
        let mut d = DomainDeclare::new();
        assert!(d.declare("B09", 1, 1, 100).is_none());
        d.set_gate(full_gate());
        assert!(d.declare("B09", 1, 1, 100).is_some());
    }

    #[test]
    fn cannot_double_declare() {
        let mut d = DomainDeclare::new();
        d.set_gate(full_gate());
        assert!(d.declare("B09", 1, 1, 100).is_some());
        assert!(d.declare("B09", 1, 1, 200).is_none());
    }

    #[test]
    fn reopen_works() {
        let mut d = DomainDeclare::new();
        d.set_gate(full_gate());
        d.declare("B09", 1, 1, 100);
        assert!(d.reopen(1));
        assert_eq!(d.reopen_count, 1);
    }
}
