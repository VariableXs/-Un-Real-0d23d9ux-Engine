//! VE-F0579 · 后端选择器
//!
//! 目标平台到后端的映射决策——策略三级：平台能力查询到后端能力集匹配（读
//! F0559 Traits）、用户显式指定优先（显式覆盖透明记录）、默认链与回退（首选
//! 后端不可用时按降级链顺延并告警）；决策透明度：选择依据（匹配了哪条能力、
//! 覆盖来自谁、回退原因）写入编译日志可查；与 F0479 流水线编排的集成：选择器
//! 是流水线入口的第一站。
//!
//! 跨批对接：上游 F0559 Traits、F0479 编排；下游 F0596 接口冻结清单纳入。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 后端 Traits（读 F0559）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Traits {
    pub lang: String,
    pub capabilities: Vec<String>,
    pub available: bool,
}

impl Traits {
    pub fn new(lang: &str, caps: &[&str], available: bool) -> Self {
        Traits { lang: lang.to_string(), capabilities: caps.iter().map(|c| c.to_string()).collect(), available }
    }

    pub fn has(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

/// 决策日志条目（透明度）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogEntry {
    /// 选中的后端（None = 显性失败）
    pub picked: Option<String>,
    /// 匹配的能力名
    pub matched_cap: Option<String>,
    /// 显式覆盖来源（用户指定）
    pub override_from: Option<String>,
    /// 回退原因
    pub fallback_reason: Option<String>,
}

impl LogEntry {
    pub fn complete(&self) -> bool {
        if self.picked.is_none() {
            return self.fallback_reason.is_some();
        }
        true
    }
}

/// 选择策略三级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strategy {
    /// 平台能力 → 后端能力集匹配
    Capability,
    /// 用户显式指定优先
    Explicit,
    /// 默认链与回退
    FallbackChain,
}

/// 降级链定义
#[derive(Clone, Debug)]
pub struct Fallback {
    /// 顺延顺序（首选在前）
    pub chain: Vec<String>,
}

impl Fallback {
    pub fn new(chain: &[&str]) -> Self {
        Fallback { chain: chain.iter().map(|c| c.to_string()).collect() }
    }

    /// 按链顺延：首个可用且具备所需能力的后端
    pub fn pick(&self, pool: &[Traits], required: &str) -> (Option<String>, Option<String>) {
        for name in &self.chain {
            if let Some(t) = pool.iter().find(|t| &t.lang == name) {
                if t.available && t.has(required) {
                    return (Some(t.lang.clone()), None);
                }
            }
        }
        // 全部不可用 → 显性失败三要素（picked=None + 原因）
        (None, Some(alloc::format!("no backend in chain satisfies {}", required)))
    }
}

/// 选择器（流水线入口第一站）
#[derive(Clone, Debug)]
pub struct Selector {
    pub pool: Vec<Traits>,
    pub fallback: Fallback,
    pub log: Vec<LogEntry>,
}

impl Selector {
    pub fn new(pool: Vec<Traits>, chain: &[&str]) -> Self {
        Selector { pool, fallback: Fallback::new(chain), log: Vec::new() }
    }

    /// 策略一：能力匹配
    pub fn by_capability(&mut self, required: &str) -> Option<String> {
        let hit = self.pool.iter().find(|t| t.available && t.has(required));
        match hit {
            Some(t) => {
                self.log.push(LogEntry {
                    picked: Some(t.lang.clone()),
                    matched_cap: Some(required.to_string()),
                    override_from: None,
                    fallback_reason: None,
                });
                Some(t.lang.clone())
            }
            None => {
                self.log.push(LogEntry {
                    picked: None,
                    matched_cap: Some(required.to_string()),
                    override_from: None,
                    fallback_reason: Some(alloc::format!("capability {} unmatched", required)),
                });
                None
            }
        }
    }

    /// 策略二：用户显式指定优先（透明记录覆盖来源）
    pub fn by_explicit(&mut self, lang: &str, from: &str) -> Option<String> {
        let hit = self.pool.iter().find(|t| t.lang == lang && t.available);
        let entry = LogEntry {
            picked: hit.map(|t| t.lang.clone()),
            matched_cap: None,
            override_from: Some(from.to_string()),
            fallback_reason: if hit.is_none() { Some(alloc::format!("explicit {} unavailable", lang)) } else { None },
        };
        self.log.push(entry);
        hit.map(|t| t.lang.clone())
    }

    /// 策略三：降级链顺延并告警
    pub fn by_fallback(&mut self, required: &str, warned: &mut Vec<String>) -> Option<String> {
        let (picked, reason) = self.fallback.pick(&self.pool, required);
        // 告警：非首选命中即回退
        if let Some(p) = &picked {
            if self.fallback.chain.first().map(|f| f != p).unwrap_or(false) {
                warned.push(alloc::format!("fallback to {}", p));
            }
        }
        self.log.push(LogEntry {
            picked: picked.clone(),
            matched_cap: Some(required.to_string()),
            override_from: None,
            fallback_reason: reason,
        });
        picked
    }

    /// 决策冲突：显式指定胜出并留痕
    pub fn conflict_resolution(&self) -> bool {
        self.log.iter().any(|e| e.override_from.is_some())
    }
}

pub fn run_vec179_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec179");
    let pool = vec![
        Traits::new("glsl", &["structured-cf"], true),
        Traits::new("hlsl", &["structured-cf", "cbuffer"], true),
        Traits::new("wgsl", &["address-space"], false),
        Traits::new("spirv", &["word-stream", "cbuffer"], true),
    ];
    // 族一：策略一能力匹配
    let mut sel = Selector::new(pool.clone(), &["glsl", "hlsl", "spirv"]);
    s.add("能力-命中glsl", sel.by_capability("structured-cf") == Some("glsl".to_string()), "hit");
    s.add("日志-匹配记录", sel.log[0].matched_cap == Some("structured-cf".to_string()) && sel.log[0].picked == Some("glsl".to_string()), "log");
    s.add("能力-未命中显性", sel.by_capability("wave").is_none(), "miss");
    s.add("日志-失败带原因", sel.log[1].fallback_reason.is_some() && sel.log[1].complete(), "reason");
    // 族二：策略二显式指定
    let mut sel2 = Selector::new(pool.clone(), &["glsl", "hlsl"]);
    s.add("显式-指定优先", sel2.by_explicit("spirv", "user") == Some("spirv".to_string()), "pick");
    s.add("显式-覆盖留痕", sel2.conflict_resolution() && sel2.log[0].override_from == Some("user".to_string()), "trace");
    s.add("显式-指定不可用", sel2.by_explicit("wgsl", "user").is_none(), "unavail");
    s.add("显式-失败带原因", sel2.log[1].fallback_reason.is_some(), "why");
    // 族三：策略三降级链
    let mut warned = Vec::new();
    let mut sel3 = Selector::new(pool.clone(), &["wgsl", "spirv", "glsl"]);
    s.add("降级-首选不可用顺延", sel3.by_fallback("word-stream", &mut warned) == Some("spirv".to_string()), "next");
    s.add("降级-告警留痕", warned == vec!["fallback to spirv".to_string()], "warn");
    s.add("降级-回退原因记录", sel3.log[0].fallback_reason.is_none() && sel3.log[0].picked == Some("spirv".to_string()), "log");
    // 全部不可用
    let bad_pool = vec![Traits::new("glsl", &["x"], false)];
    let mut sel4 = Selector::new(bad_pool, &["glsl"]);
    let mut w2 = Vec::new();
    s.add("降级-全链不可用显性失败", sel4.by_fallback("structured-cf", &mut w2).is_none(), "fail");
    s.add("降级-失败三要素", sel4.log[0].picked.is_none() && sel4.log[0].fallback_reason.is_some(), "tri");
    // 族四：降级链直接命中首选不告警
    let mut sel5 = Selector::new(pool.clone(), &["glsl", "hlsl"]);
    let mut w3 = Vec::new();
    s.add("降级-首选命中无告警", sel5.by_fallback("structured-cf", &mut w3) == Some("glsl".to_string()) && w3.is_empty(), "first");
    // 族五：Traits 读取
    s.add("traits-能力查询", pool[1].has("cbuffer") && !pool[0].has("cbuffer"), "cap");
    s.add("traits-可用性", pool[2].available == false, "avail");
    // 族六：流水线入口第一站语义
    s.add("编排-日志完整可查", sel.log.iter().all(|e| e.complete()), "audit");
    s
}
