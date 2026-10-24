// CGPU-F0350 预算API冻结v2与文档 · cgb350_budapiv2.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：v2 扩展签名冻结（预测/迁移/回放/对比）；v1→v2 兼容；
//       弃用标记；文档增量更新。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// v2 API 条目。
#[derive(Clone, Debug)]
pub struct V2Api {
    pub id: u32,
    pub name: String,
    pub params: Vec<String>,
    pub ret: String,
    pub replaces_v1: Option<u32>,
    pub frozen: bool,
    pub hash: u32,
}

/// v1→v2 兼容映射。
#[derive(Clone, Copy, Debug)]
pub struct CompatMap {
    pub v1_id: u32,
    pub v2_id: u32,
    pub breaking: bool,
}

/// API v2 冻结引擎。
pub struct BudgetApiV2 {
    pub apis: Vec<V2Api>,
    pub compat: Vec<CompatMap>,
    pub version: u32,
    pub freeze_hash: u32,
}

impl BudgetApiV2 {
    pub fn new() -> Self {
        Self {
            apis: Vec::new(),
            compat: Vec::new(),
            version: 2,
            freeze_hash: 0,
        }
    }

    fn fnv32(s: &str) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in s.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn register(&mut self, name: &str, params: Vec<String>, ret: &str, replaces: Option<u32>) -> u32 {
        let id = self.apis.len() as u32 + 1;
        let mut sig = String::from(name);
        for p in &params {
            sig.push_str(p);
        }
        sig.push_str(ret);
        let hash = Self::fnv32(&sig);
        self.apis.push(V2Api {
            id,
            name: String::from(name),
            params,
            ret: String::from(ret),
            replaces_v1: replaces,
            frozen: false,
            hash,
        });
        if let Some(v1) = replaces {
            self.compat.push(CompatMap {
                v1_id: v1,
                v2_id: id,
                breaking: false,
            });
        }
        id
    }

    pub fn freeze_all(&mut self) -> u32 {
        let mut n = 0u32;
        for a in self.apis.iter_mut() {
            a.frozen = true;
            n += 1;
        }
        let mut h: u32 = 0x811C_9DC5;
        let mut sorted: Vec<&V2Api> = self.apis.iter().collect();
        sorted.sort_by_key(|a| a.id);
        for a in sorted {
            h ^= a.hash;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.freeze_hash = h;
        n
    }

    pub fn compat_coverage(&self) -> u32 {
        self.compat.len() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0350 预算 API 冻结 v2");
        cs.add("API id 唯一", {
            let mut ids: Vec<u32> = self.apis.iter().map(|a| a.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.apis.len()
        }, "unique");
        cs.add("全部冻结", self.apis.iter().all(|a| a.frozen) || self.apis.is_empty(), "frozen");
        cs.add("冻结哈希非零", self.freeze_hash != 0 || self.apis.is_empty(), "hash");
        cs.add("兼容映射引用有效", {
            self.compat.iter().all(|c| self.apis.iter().any(|a| a.id == c.v2_id))
        }, "valid");
        cs.add("v2 版本号", self.version == 2, "ver=2");
        cs.add("名称非空", self.apis.iter().all(|a| !a.name.is_empty()), "name");
        cs
    }
}

impl Default for BudgetApiV2 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_v2() {
        let mut v2 = BudgetApiV2::new();
        v2.register("predict", vec![String::from("h")], "u32", Some(1));
        v2.register("migrate", vec![String::from("from")], "Result", Some(2));
        v2.freeze_all();
        assert_eq!(v2.compat_coverage(), 2);
    }
}
