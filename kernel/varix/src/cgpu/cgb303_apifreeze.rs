// CGPU-F0303 可视化 API 冻结与文档 · cgb303_apifreeze.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：四大族签名冻结（场景/布局/渲染/导出）；弃用策略成文；版本兼容矩阵；
//       文档同源生成；签名哈希可校验。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// API 族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    Scene,
    Layout,
    Render,
    Export,
}

impl ApiFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Layout => "layout",
            Self::Render => "render",
            Self::Export => "export",
        }
    }
}

/// 签名稳定性级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    Frozen,     // 永不变更
    Stable,     // 仅加不减
    Experimental,
    Deprecated,
}

/// 冻结的 API 签名。
#[derive(Clone, Debug)]
pub struct FrozenSignature {
    pub id: u32,
    pub family: ApiFamily,
    pub name: String,
    pub params: Vec<String>,
    pub ret: String,
    pub stability: Stability,
    pub since_version: u32,
    pub doc_id: u32,
    pub sig_hash: u32,
}

/// 弃用记录。
#[derive(Clone, Debug)]
pub struct Deprecation {
    pub sig_id: u32,
    pub deprecated_at: u32,
    pub remove_at: u32,
    pub replacement_id: Option<u32>,
    pub reason_id: u32,
}

/// 兼容矩阵条目。
#[derive(Clone, Copy, Debug)]
pub struct CompatEntry {
    pub from_ver: u32,
    pub to_ver: u32,
    pub breaking: bool,
    pub sig_count: u32,
}

/// API 冻结引擎。
pub struct ApiFreeze {
    pub signatures: Vec<FrozenSignature>,
    pub deprecations: Vec<Deprecation>,
    pub compat: Vec<CompatEntry>,
    pub current_version: u32,
    pub doc_bindings: Vec<(u32, u32)>,
    pub freeze_hash: u32,
}

impl ApiFreeze {
    pub fn new(version: u32) -> Self {
        Self {
            signatures: Vec::new(),
            deprecations: Vec::new(),
            compat: Vec::new(),
            current_version: version,
            doc_bindings: Vec::new(),
            freeze_hash: 0,
        }
    }

    /// FNV-1a 签名哈希（确定性）。
    pub fn hash_signature(name: &str, params: &[String], ret: &str) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        for p in params {
            for b in p.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        for b in ret.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn register(&mut self, family: ApiFamily, name: &str, params: Vec<String>, ret: &str, stability: Stability) -> u32 {
        let id = self.signatures.len() as u32 + 1;
        let sig_hash = Self::hash_signature(name, &params, ret);
        self.signatures.push(FrozenSignature {
            id,
            family,
            name: String::from(name),
            params,
            ret: String::from(ret),
            stability,
            since_version: self.current_version,
            doc_id: id,
            sig_hash,
        });
        self.doc_bindings.push((id, id));
        id
    }

    /// 弃用（必须给替代）。
    pub fn deprecate(&mut self, sig_id: u32, replacement_id: Option<u32>, reason_id: u32) -> bool {
        if let Some(s) = self.signatures.iter_mut().find(|s| s.id == sig_id) {
            if s.stability == Stability::Frozen {
                return false; // 冻结的不能弃用
            }
            s.stability = Stability::Deprecated;
            self.deprecations.push(Deprecation {
                sig_id,
                deprecated_at: self.current_version,
                remove_at: self.current_version + 2,
                replacement_id,
                reason_id,
            });
            true
        } else {
            false
        }
    }

    /// 兼容矩阵记录。
    pub fn record_compat(&mut self, from: u32, to: u32, breaking: bool, sig_count: u32) {
        self.compat.push(CompatEntry {
            from_ver: from,
            to_ver: to,
            breaking,
            sig_count,
        });
    }

    /// 计算冻结哈希（所有 Frozen 签名的哈希聚合）。
    pub fn compute_freeze_hash(&mut self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut frozen: Vec<&FrozenSignature> = self.signatures.iter().filter(|s| s.stability == Stability::Frozen).collect();
        frozen.sort_by_key(|s| s.id);
        for s in frozen {
            h ^= s.sig_hash;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.freeze_hash = h;
        h
    }

    /// 文档一致性（每个签名有 doc 绑定）。
    pub fn doc_coverage_pct(&self) -> u32 {
        if self.signatures.is_empty() {
            return 100;
        }
        let bound = self.signatures.iter().filter(|s| self.doc_bindings.iter().any(|(a, _)| *a == s.id)).count();
        (bound as u32 * 100) / self.signatures.len() as u32
    }

    /// 按族统计。
    pub fn family_count(&self, family: ApiFamily) -> usize {
        self.signatures.iter().filter(|s| s.family == family).count()
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0303 可视化 API 冻结");
        cs.add("签名 id 唯一", {
            let mut ids: Vec<u32> = self.signatures.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.signatures.len()
        }, "unique");
        cs.add("签名哈希可重算一致", {
            self.signatures.iter().all(|s| {
                Self::hash_signature(&s.name, &s.params, &s.ret) == s.sig_hash
            })
        }, "hash match");
        cs.add("弃用有替代或理由", {
            self.deprecations.iter().all(|d| d.replacement_id.is_some() || d.reason_id > 0)
        }, "replacement or reason");
        cs.add("弃用的不是 Frozen", {
            self.deprecations.iter().all(|d| {
                self.signatures.iter().find(|s| s.id == d.sig_id).map(|s| s.stability != Stability::Frozen).unwrap_or(false)
            })
        }, "no frozen deprecated");
        cs.add("兼容矩阵版本号有序", {
            self.compat.windows(2).all(|w| w[0].from_ver <= w[1].from_ver)
        }, "sorted");
        cs.add("文档覆盖率 >= 90%", self.doc_coverage_pct() >= 90, ">=90%");
        cs.add("四族均有签名", {
            self.family_count(ApiFamily::Scene) > 0
                && self.family_count(ApiFamily::Layout) > 0
                && self.family_count(ApiFamily::Render) > 0
                && self.family_count(ApiFamily::Export) > 0
        }, "all families");
        cs.add("冻结哈希非零或无 Frozen", {
            self.freeze_hash != 0 || !self.signatures.iter().any(|s| s.stability == Stability::Frozen)
        }, "hash");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_freeze() {
        let mut af = ApiFreeze::new(1);
        af.register(ApiFamily::Scene, "add_node", vec![String::from("id"), String::from("label")], "u32", Stability::Frozen);
        af.register(ApiFamily::Layout, "layout_step", vec![String::from("impact")], "()", Stability::Stable);
        let h = af.compute_freeze_hash();
        assert_ne!(h, 0);
        assert_eq!(af.doc_coverage_pct(), 100);
    }

    #[test]
    fn frozen_cannot_deprecate() {
        let mut af = ApiFreeze::new(1);
        let id = af.register(ApiFamily::Render, "draw", vec![], "()", Stability::Frozen);
        assert!(!af.deprecate(id, None, 1));
    }

    #[test]
    fn hash_deterministic() {
        let p = vec![String::from("a"), String::from("b")];
        assert_eq!(ApiFreeze::hash_signature("f", &p, "u32"), ApiFreeze::hash_signature("f", &p, "u32"));
    }
}
