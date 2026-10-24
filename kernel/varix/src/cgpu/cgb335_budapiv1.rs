// CGPU-F0335 预算API冻结v1与文档 · cgb335_budapiv1.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算 API v1 签名冻结（查询/分配/交易/审计四族）；
//       文档同步生成；版本兼容矩阵；签名哈希校验。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 预算 API 族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetApiFamily {
    Query,
    Allocate,
    Trade,
    Audit,
}

impl BudgetApiFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Allocate => "allocate",
            Self::Trade => "trade",
            Self::Audit => "audit",
        }
    }
}

/// 冻结签名。
#[derive(Clone, Debug)]
pub struct BudgetApiSig {
    pub id: u32,
    pub family: BudgetApiFamily,
    pub name: String,
    pub params: Vec<String>,
    pub ret: String,
    pub frozen: bool,
    pub hash: u32,
    pub doc_id: u32,
}

/// 文档条目。
#[derive(Clone, Debug)]
pub struct BudgetApiDoc {
    pub sig_id: u32,
    pub summary_id: u32,
    pub example_id: u32,
    pub errors_listed: bool,
}

/// API 冻结引擎。
pub struct BudgetApiFreeze {
    pub sigs: Vec<BudgetApiSig>,
    pub docs: Vec<BudgetApiDoc>,
    pub version: u32,
    pub freeze_hash: u32,
}

impl BudgetApiFreeze {
    pub fn new(version: u32) -> Self {
        Self {
            sigs: Vec::new(),
            docs: Vec::new(),
            version,
            freeze_hash: 0,
        }
    }

    fn fnv32(data: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for &b in data {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn register(&mut self, family: BudgetApiFamily, name: &str, params: Vec<String>, ret: &str) -> u32 {
        let id = self.sigs.len() as u32 + 1;
        let mut hash_input = name.as_bytes().to_vec();
        for p in &params {
            hash_input.extend_from_slice(p.as_bytes());
        }
        hash_input.extend_from_slice(ret.as_bytes());
        let hash = Self::fnv32(&hash_input);
        self.sigs.push(BudgetApiSig {
            id,
            family,
            name: String::from(name),
            params,
            ret: String::from(ret),
            frozen: false,
            hash,
            doc_id: id,
        });
        self.docs.push(BudgetApiDoc {
            sig_id: id,
            summary_id: id,
            example_id: id,
            errors_listed: false,
        });
        id
    }

    /// 冻结四族 API。
    pub fn freeze_all(&mut self) -> u32 {
        let mut frozen_count = 0u32;
        for s in self.sigs.iter_mut() {
            s.frozen = true;
            frozen_count += 1;
        }
        let mut h: u32 = 0x811C_9DC5;
        let mut sorted: Vec<&BudgetApiSig> = self.sigs.iter().collect();
        sorted.sort_by_key(|s| s.id);
        for s in sorted {
            h ^= s.hash;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.freeze_hash = h;
        frozen_count
    }

    /// 文档覆盖。
    pub fn doc_coverage_pct(&self) -> u32 {
        if self.sigs.is_empty() {
            return 100;
        }
        let covered = self.sigs.iter().filter(|s| self.docs.iter().any(|d| d.sig_id == s.id)).count();
        (covered as u32 * 100) / self.sigs.len() as u32
    }

    /// 四族覆盖。
    pub fn family_coverage(&self) -> u32 {
        let families = [
            BudgetApiFamily::Query,
            BudgetApiFamily::Allocate,
            BudgetApiFamily::Trade,
            BudgetApiFamily::Audit,
        ];
        families
            .iter()
            .filter(|f| self.sigs.iter().any(|s| s.family == **f))
            .count() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0335 预算 API 冻结 v1");
        cs.add("签名 id 唯一", {
            let mut ids: Vec<u32> = self.sigs.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.sigs.len()
        }, "unique");
        cs.add("哈希可重算", {
            self.sigs.iter().all(|s| {
                let mut input = s.name.as_bytes().to_vec();
                for p in &s.params {
                    input.extend_from_slice(p.as_bytes());
                }
                input.extend_from_slice(s.ret.as_bytes());
                Self::fnv32(&input) == s.hash
            })
        }, "hash");
        cs.add("四族覆盖", self.family_coverage() == 4, "4/4");
        cs.add("文档覆盖 ≥ 90%", self.doc_coverage_pct() >= 90, ">=90%");
        cs.add("冻结哈希非零", self.freeze_hash != 0, "hash");
        cs.add("全部已冻结或部分", {
            self.sigs.iter().all(|s| s.frozen) || self.sigs.is_empty()
        }, "frozen");
        cs.add("文档条目与签名对应", {
            self.docs.iter().all(|d| self.sigs.iter().any(|s| s.id == d.sig_id))
        }, "doc refs");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_four_families() {
        let mut f = BudgetApiFreeze::new(1);
        f.register(BudgetApiFamily::Query, "get_budget", vec![String::from("id")], "u64");
        f.register(BudgetApiFamily::Allocate, "alloc", vec![String::from("id"), String::from("amt")], "Result");
        f.register(BudgetApiFamily::Trade, "trade", vec![String::from("from"), String::from("to")], "Result");
        f.register(BudgetApiFamily::Audit, "audit_log", vec![], "Vec");
        let n = f.freeze_all();
        assert_eq!(n, 4);
        assert_eq!(f.family_coverage(), 4);
        assert_eq!(f.doc_coverage_pct(), 100);
    }
}
