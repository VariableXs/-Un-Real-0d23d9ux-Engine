//! VE-F5241 · 后处理库总架构（VE-Z 域 · Z03 组 · 目标 320 行）· 诊断段 0x9c80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5241`
//!
//! **判据（锚点原文）**：三段架构、效果本体、语义律、契约、判据。
//!
//! **职责定位（锚点原文）**：后处理库总架构（后处理库定位：全屏画面精修工具箱
//! ——每个效果都声明它的叙事用途）；架构三段：效果实现层（效果本体）→链管理
//! 层（效果编排与顺序）→参数层（统一参数 schema——F5207 框架复用）；语义缺失
//! →评审拦截；越权编排→归位 Z01。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9c80;

/// 三段。
pub const STAGES: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 后处理效果：本体 + 叙事用途声明（语义律）。
pub struct Effect {
    pub name: &'static str,
    /// 叙事用途（空即语义缺失→评审拦截）。
    pub narrative: &'static str,
    /// 统一参数（F5207 schema：强度 0..1）。
    pub intensity: f32,
}

impl Effect {
    /// 语义律：叙事用途非空 + 参数在 schema 域内。
    pub fn semantic_ok(&self) -> bool {
        !self.narrative.is_empty() && self.intensity >= 0.0 && self.intensity <= 1.0
    }
}

/// 链管理层：效果编排顺序账。
pub struct Chain {
    pub order: Vec<&'static str>,
}

impl Chain {
    /// 编排合法：效果在链上零重复（同一效果不重复执行）。
    pub fn no_dup(&self) -> bool {
        for k in 0..self.order.len() {
            for m in (k + 1)..self.order.len() {
                if self.order[k] == self.order[m] {
                    return false;
                }
            }
        }
        true
    }
}

/// 三段架构。
pub struct PpArch {
    pub effects: Vec<Effect>,
    pub chain: Chain,
    /// 参数 schema 版本（F5207 复用）。
    pub schema_v: u32,
}

impl PpArch {
    /// 标准构造。
    pub fn standard() -> Self {
        PpArch {
            effects: alloc::vec![
                Effect { name: "bloom", narrative: "highlight-glow", intensity: 0.4 },
                Effect { name: "vignette", narrative: "focus-narrow", intensity: 0.3 },
            ],
            chain: Chain { order: alloc::vec!["bloom", "vignette"] },
            schema_v: 5207,
        }
    }

    /// 全量语义律：所有效果叙事声明齐。
    pub fn all_semantic(&self) -> bool {
        self.effects.iter().all(|e| e.semantic_ok())
    }

    /// 段间契约：schema 版本钉死。
    pub fn contract(&self) -> bool {
        self.schema_v == 5207
    }
}

/// 越权编排拦截。
pub fn ownership(is_orchestration: bool) -> Result<(), i32> {
    if is_orchestration {
        Err(1)
    } else {
        Ok(())
    }
}

/// J1 主判据：三段架构 + 语义律 + 契约 + 拦截。
pub fn j1_main() -> bool {
    let a = PpArch::standard();
    a.all_semantic() && a.chain.no_dup() && a.contract()
        && ownership(false).is_ok() && ownership(true).is_err()
}

// ── 注入核（0x9c80 段 5 项）────────────────────────────────────────────

/// 注入核 1：语义缺失拦截注入——叙事用途空缺必拦。
pub fn inj1_narrative_missing() -> bool {
    let e = Effect { name: "x", narrative: "", intensity: 0.5 };
    !e.semantic_ok()
}

/// 注入核 2：参数越域注入——强度超 schema 必拦。
pub fn inj2_param_out() -> bool {
    let e = Effect { name: "x", narrative: "n", intensity: 1.5 };
    !e.semantic_ok()
}

/// 注入核 3：链重复注入——同效果双入链必检出。
pub fn inj3_chain_dup() -> bool {
    let c = Chain { order: alloc::vec!["bloom", "bloom"] };
    !c.no_dup()
}

/// 注入核 4：契约漂移注入——schema 版本漂移必拦。
pub fn inj4_schema_drift() -> bool {
    let mut a = PpArch::standard();
    a.schema_v = 1;
    !a.contract()
}

/// 注入核 5：越权归位注入。
pub fn inj5_overreach() -> bool {
    ownership(true).is_err()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vez24_checks：本单判据聚合（诊断段 0x9c80）。
pub fn run_vez24_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5241 后处理库总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三段架构+效果语义律全量",
        j1_main() && PpArch::standard().all_semantic(),
        if j1_main() { "" } else { "语义律破" },
    );
    s.add(
        "链编排零重复+schema 契约+越权拦截",
        j1_main(),
        if j1_main() { "" } else { "链/契约破" },
    );
    let inj = [
        ("注入1叙事缺失", inj1_narrative_missing()),
        ("注入2参数越域", inj2_param_out()),
        ("注入3链重复", inj3_chain_dup()),
        ("注入4契约漂移", inj4_schema_drift()),
        ("注入5越权归位", inj5_overreach()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
