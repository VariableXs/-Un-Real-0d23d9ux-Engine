//! VE-F0481 · 着色器变体系统总成（VE-C 域 · 变体组首件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0481`
//!
//! **锚点原文**：「变体系统总管：变体定义模型（源、参数集、目标后端三元组）、
//! 变体生命周期（注册、编译、缓存、失效四态）、变体与渲染系统联动接口（按
//! 材质参数请求变体）；系统与 F0477 参数化、F0475 缓存集成收口。判据：三元
//! 组模型、四态生命周期、联动接口、失败隔离、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三元组模型**（判据一）。变体 = (源, 参数集, 目标后端) 三元组，
//!    恒等键由此派生（三元任一不同即不同变体）。
//!
//! 2. **四态生命周期**（判据二）。注册 → 编译 → 缓存 → 失效 四态状态机
//!    （非法迁移拒绝：未编译不得缓存——迁移表机检）。
//!
//! 3. **联动接口**（判据三）。渲染系统按材质参数请求变体（[`request`]：
//!    缓存命中即返、未命中注册编译——F0475 缓存键复用双哈希）。
//!
//! 4. **失败隔离**（判据四）。单变体编译失败 → 标记失败不拖垮他变体（失败
//!    隔离：失败态可查、他变体照常）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0481";

/// 变体生命周期四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarState {
    Registered,
    Compiling,
    Cached,
    Failed,
    /// 失效（驱动或选项变更——待重编译）。
    Invalid,
}

impl VarState {
    pub fn tag(&self) -> &'static str {
        match self {
            VarState::Registered => "registered",
            VarState::Compiling => "compiling",
            VarState::Cached => "cached",
            VarState::Failed => "failed",
            VarState::Invalid => "invalid",
        }
    }
}

/// 合法迁移表（状态机面：其余迁移拒绝）。
pub static LEGAL_TRANSITIONS: &[(VarState, VarState)] = &[
    (VarState::Registered, VarState::Compiling),
    (VarState::Compiling, VarState::Cached),
    (VarState::Compiling, VarState::Failed),
    (VarState::Cached, VarState::Invalid),
    (VarState::Failed, VarState::Invalid),
    (VarState::Invalid, VarState::Registered),
];

/// 迁移合法性（表驱动机检）。
pub fn transition_ok(from: VarState, to: VarState) -> bool {
    LEGAL_TRANSITIONS.iter().any(|(a, b)| *a == from && *b == to)
}

/// 变体三元组模型（源, 参数集, 目标后端）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantDef {
    pub source: String,
    pub params: String,
    pub backend: String,
}

impl VariantDef {
    pub fn identity_key(&self) -> String {
        format!("{}|{}|{}", self.source, self.params, self.backend)
    }
    /// F0475 双哈希键控复用（源哈希 × 参数哈希）。
    pub fn cache_key(&self) -> (u64, u64) {
        let h = |b: &[u8]| {
            let mut x: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in b {
                x = (x ^ *byte as u64).wrapping_mul(0x0100_0000_01b3);
            }
            x
        };
        (h(self.source.as_bytes()), h(self.params.as_bytes()))
    }
}

/// 变体条目（定义 + 状态 + 产物摘要）。
#[derive(Clone, Debug)]
pub struct VariantEntry {
    pub def: VariantDef,
    pub state: VarState,
    /// 编译产物摘要（缓存态非空）。
    pub artifact: String,
}

/// 变体系统（总管面）。
#[derive(Debug, Default)]
pub struct VariantSystem {
    pub variants: Vec<VariantEntry>,
    /// 失败隔离账：失败变体不影响他变体的请求。
    pub failure_log: Vec<String>,
}

impl VariantSystem {
    pub fn new() -> Self {
        Self::default()
    }
    /// 注册（四态入口：Registered）。
    pub fn register(&mut self, def: VariantDef) -> Result<(), String> {
        let key = def.identity_key();
        if self.variants.iter().any(|v| v.def.identity_key() == key) {
            return Err(format!("变体重复注册：{}", key));
        }
        self.variants.push(VariantEntry {
            def,
            state: VarState::Registered,
            artifact: String::new(),
        });
        Ok(())
    }
    /// 状态迁移（非法迁移拒绝 + 失败隔离入账）。
    pub fn transition(&mut self, key: &str, to: VarState) -> Result<(), String> {
        let e = self
            .variants
            .iter_mut()
            .find(|v| v.def.identity_key() == *key)
            .ok_or_else(|| format!("变体不存在：{}", key))?;
        if !transition_ok(e.state, to) {
            return Err(format!("非法迁移：{} → {}", e.state.tag(), to.tag()));
        }
        if to == VarState::Failed {
            self.failure_log.push(key.to_string());
        }
        e.state = to;
        Ok(())
    }
    /// 编译落库（Compiling → Cached 的产物面）。
    pub fn cache_artifact(&mut self, key: &str, artifact: &str) -> Result<(), String> {
        let e = self
            .variants
            .iter_mut()
            .find(|v| v.def.identity_key() == *key)
            .ok_or_else(|| format!("变体不存在：{}", key))?;
        if e.state != VarState::Compiling {
            return Err(format!("未编译不得缓存：当前 {}", e.state.tag()));
        }
        e.artifact = artifact.to_string();
        e.state = VarState::Cached;
        Ok(())
    }
    /// 联动接口：按材质参数请求变体（缓存命中即返；未命中注册+编译）。
    /// 失败隔离：编译失败标记本变体，返回 Err 但不影响其他变体。
    pub fn request(&mut self, def: VariantDef, compile_ok: bool) -> Result<String, String> {
        let key = def.identity_key();
        if let Some(e) = self.variants.iter().find(|v| v.def.identity_key() == key) {
            if e.state == VarState::Cached {
                return Ok(e.artifact.clone());
            }
            if e.state == VarState::Failed {
                return Err(format!("变体处于失败态：{}", key));
            }
        } else {
            self.register(def.clone())?;
        }
        self.transition(&key, VarState::Compiling)?;
        if compile_ok {
            let artifact = format!("bin[{}]", key);
            self.cache_artifact(&key, &artifact)?;
            Ok(artifact)
        } else {
            self.transition(&key, VarState::Failed)?;
            Err(format!("编译失败（已隔离）：{}", key))
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec81_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三元组模型。
    {
        let d1 = VariantDef {
            source: "lit_vs".to_string(),
            params: "SHADOWS=1".to_string(),
            backend: "gl450".to_string(),
        };
        let d2 = VariantDef {
            source: "lit_vs".to_string(),
            params: "SHADOWS=0".to_string(),
            backend: "gl450".to_string(),
        };
        let d3 = d1.clone();
        // 三元任一不同即不同变体；全同即同。
        v.push((
            "F0481-三元-恒等键",
            d1.identity_key() != d2.identity_key() && d1.identity_key() == d3.identity_key(),
        ));
        // 双哈希键面：参数变 → 键变（F0475 复用）。
        v.push(("F0481-三元-双哈希键", d1.cache_key() != d2.cache_key() && d1.cache_key() == d3.cache_key()));
    }

    // 二、四态生命周期。
    {
        // 合法链：Registered → Compiling → Cached → Invalid → Registered。
        v.push((
            "F0481-四态-合法链",
            transition_ok(VarState::Registered, VarState::Compiling)
                && transition_ok(VarState::Compiling, VarState::Cached)
                && transition_ok(VarState::Cached, VarState::Invalid)
                && transition_ok(VarState::Invalid, VarState::Registered),
        ));
        // 非法迁移：未编译不得缓存（锚点点名面）。
        v.push((
            "F0481-四态-未编译不缓存",
            !transition_ok(VarState::Registered, VarState::Cached),
        ));
        // 失败态直接缓存也拒。
        v.push((
            "F0481-四态-失败不直缓存",
            !transition_ok(VarState::Failed, VarState::Cached),
        ));
    }

    // 三、联动接口。
    {
        let mut sys = VariantSystem::new();
        let d = VariantDef {
            source: "lit_vs".to_string(),
            params: "SHADOWS=1".to_string(),
            backend: "gl450".to_string(),
        };
        // 首次请求：注册+编译+缓存。
        let a1 = sys.request(d.clone(), true);
        v.push((
            "F0481-联动-首请编译",
            a1.is_ok() && sys.variants[0].state == VarState::Cached,
        ));
        // 二次请求：缓存命中即返（同产物）。
        let a2 = sys.request(d.clone(), true);
        v.push((
            "F0481-联动-命中即返",
            a1 == a2 && sys.variants.len() == 1,
        ));
    }

    // 四、失败隔离。
    {
        let mut sys = VariantSystem::new();
        let bad = VariantDef {
            source: "bad_vs".to_string(),
            params: "".to_string(),
            backend: "gl450".to_string(),
        };
        let good = VariantDef {
            source: "good_vs".to_string(),
            params: "".to_string(),
            backend: "gl450".to_string(),
        };
        // 坏变体编译失败。
        let rb = sys.request(bad, false);
        // 好变体不受影响（失败隔离核心）。
        let rg = sys.request(good, true);
        v.push((
            "F0481-隔离-失败不拖垮",
            rb.is_err() && rg.is_ok() && sys.variants[1].state == VarState::Cached,
        ));
        // 失败态可查 + 失败账记录。
        v.push((
            "F0481-隔离-失败态可查",
            sys.variants[0].state == VarState::Failed
                && sys.failure_log.len() == 1
                && sys.failure_log[0].contains("bad_vs"),
        ));
        // 失败态再请求 → 显性 Err（不静默重编）。
        let r2 = sys.request(
            VariantDef { source: "bad_vs".to_string(), params: "".to_string(), backend: "gl450".to_string() },
            true,
        );
        v.push(("F0481-隔离-失败态显性", r2.is_err()));
    }

    // 五、判据自检。
    {
        v.push(("F0481-判据-锚点入册", ANCHOR == "VE-F0481"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec81）。
pub fn run_vec81_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec81");
    for (name, ok) in vec81_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec81_raw_checks as run_vec81_checks;
    #[test]
    fn vec81_variantsys_checks() {
        for (name, ok) in run_vec81_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
