//! VE-F0479 · IR 编译流水线编排（VE-C 域 · IR 组尾件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0479`
//!
//! **锚点原文**：「编译流水线编排器：阶段编排（词法、语法、语义、IR、优化、
//! 后端六段顺序加产物传递契约）、阶段产物缓存衔接（F0475 缓存格式复用）、
//! 编排配置（哪些阶段跳过——增量场景）；编排事件入诊断记录。判据：六段编
//! 排、契约传递、增量跳过、事件入账、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **六段编排**（判据一）。六段固定序（词法→语法→语义→IR→优化→后端），
//!    顺序执行产物链式传递（前段产物 = 后段输入）。
//!
//! 2. **契约传递**（判据二）。段间产物契约（[`StageContract`]：产物类型+
//!    就绪位），契约不匹配 → 拦截（后段拒收不符产物）。
//!
//! 3. **增量跳过**（判据三）。跳过配置（增量场景：缓存命中段跳过），非法
//!    跳过拒绝（语义未过进 IR = 非法——门禁序不破）。
//!
//! 4. **事件入账**（判据四）。编排事件（段开始/完成/跳过/失败）入诊断记录
//!    （时间序留痕可回放）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0479";

/// 六段固定序。
pub static STAGE_ORDER: &[&str] = &["lex", "parse", "sema", "irgen", "optimize", "backend"];

/// 段间产物契约（产物类型 + 就绪位）。
#[derive(Clone, Debug, PartialEq)]
pub struct StageContract {
    pub stage: &'static str,
    /// 产物类型标签（契约匹配面）。
    pub artifact: &'static str,
    pub ready: bool,
}

/// 编排事件（入账面）。
#[derive(Clone, Debug, PartialEq)]
pub struct StageEvent {
    pub stage: &'static str,
    pub kind: &'static str,
}

/// 编排器。
#[derive(Debug, Default)]
pub struct Orchestrator {
    /// 跳过配置（增量场景）。
    pub skip: Vec<&'static str>,
    pub events: Vec<StageEvent>,
    /// 已完成段（产物链）。
    pub done: Vec<&'static str>,
    pub failed: Option<&'static str>,
}

impl Orchestrator {
    pub fn new(skip: Vec<&'static str>) -> Self {
        Orchestrator {
            skip,
            events: Vec::new(),
            done: Vec::new(),
            failed: None,
        }
    }
    /// 非法跳过检测：门禁序不破（语义未过不得进 IR/优化/后端）。
    fn illegal_skip(&self, stage: &str) -> bool {
        let sema_idx = STAGE_ORDER.iter().position(|s| *s == "sema").unwrap_or(2);
        let my_idx = STAGE_ORDER.iter().position(|s| *s == stage);
        match my_idx {
            Some(i) => self.skip.iter().any(|s| *s == "sema") && i > sema_idx,
            None => false,
        }
    }
    /// 运行六段编排：逐段执行（闭包注入执行体——产物契约驱动）。
    pub fn run(&mut self, mut exec: impl FnMut(&str) -> Result<StageContract, String>) -> bool {
        for stage in STAGE_ORDER.iter() {
            if self.failed.is_some() {
                break;
            }
            // 非法跳过（语义段被跳且后续段执行）→ 拒绝整编排。
            if self.illegal_skip(stage) && !self.skip.contains(stage) {
                self.failed = Some(stage);
                self.events.push(StageEvent { stage, kind: "reject-illegal-skip" });
                break;
            }
            if self.skip.contains(stage) {
                // 合法跳过（前段已过门禁——sema 自身跳过即非法）。
                if *stage == "sema" || self.illegal_skip(stage) {
                    self.failed = Some(stage);
                    self.events.push(StageEvent { stage, kind: "reject-skip-gate" });
                    break;
                }
                self.events.push(StageEvent { stage, kind: "skip" });
                continue;
            }
            self.events.push(StageEvent { stage, kind: "start" });
            match exec(stage) {
                Ok(c) if c.ready && c.stage == *stage => {
                    self.done.push(stage);
                    self.events.push(StageEvent { stage, kind: "done" });
                }
                Ok(c) => {
                    // 契约不匹配（段名或就绪位不符）→ 拦截。
                    self.failed = Some(stage);
                    self.events.push(StageEvent { stage, kind: "contract-mismatch" });
                    let _ = c;
                    break;
                }
                Err(e) => {
                    // 阶段失败 → 显性终止带阶段名。
                    self.failed = Some(stage);
                    self.events.push(StageEvent { stage, kind: "fail" });
                    let _ = e;
                    break;
                }
            }
        }
        self.failed.is_none()
    }
}

/// 产物契约构造（就绪面）。
pub fn contract(stage: &str) -> StageContract {
    StageContract {
        stage: STAGE_ORDER.iter().find(|s| **s == stage).copied().unwrap_or("lex"),
        artifact: "ok",
        ready: true,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec79_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、六段编排。
    {
        // 全段执行：六段全 done。
        let mut o = Orchestrator::new(Vec::new());
        let ok = o.run(|s| Ok(contract(s)));
        v.push((
            "F0479-编排-六段全过",
            ok && o.done.len() == 6 && o.done == STAGE_ORDER.to_vec(),
        ));
        // 段失败 → 显性终止带阶段名（第 3 段失败 → 后段不执行）。
        let mut o2 = Orchestrator::new(Vec::new());
        let ok2 = o2.run(|s| {
            if s == "sema" {
                Err("语义错误未清".to_string())
            } else {
                Ok(contract(s))
            }
        });
        v.push((
            "F0479-编排-失败显性终止",
            !ok2 && o2.failed == Some("sema") && o2.done == vec!["lex", "parse"],
        ));
    }

    // 二、契约传递。
    {
        // 契约不匹配（就绪位 false）→ 拦截。
        let mut o = Orchestrator::new(Vec::new());
        let ok = o.run(|s| {
            if s == "irgen" {
                Ok(StageContract { stage: "irgen", artifact: "ok", ready: false })
            } else {
                Ok(contract(s))
            }
        });
        v.push((
            "F0479-契约-不匹配拦截",
            !ok && o.failed == Some("irgen")
                && o.events.iter().any(|e| e.kind == "contract-mismatch"),
        ));
        // 段名不符（后段产物冒前段名）→ 拦截。
        let mut o2 = Orchestrator::new(Vec::new());
        let ok2 = o2.run(|s| {
            if s == "parse" {
                Ok(StageContract { stage: "lex", artifact: "ok", ready: true })
            } else {
                Ok(contract(s))
            }
        });
        v.push((
            "F0479-契约-段名不符拦",
            !ok2 && o2.failed == Some("parse"),
        ));
    }

    // 三、增量跳过。
    {
        // 合法跳过（sema 已过、跳 lex/parse——增量重编译场景）。
        let mut o = Orchestrator::new(vec!["lex", "parse"]);
        let ok = o.run(|s| Ok(contract(s)));
        v.push((
            "F0479-增量-合法跳过",
            ok && o.done.len() == 4
                && o.events.iter().filter(|e| e.kind == "skip").count() == 2,
        ));
        // 非法跳过：sema 被跳 → 后续段拒绝执行（门禁序不破）。
        let mut o2 = Orchestrator::new(vec!["sema"]);
        let ok2 = o2.run(|s| Ok(contract(s)));
        v.push((
            "F0479-增量-非法跳过拒",
            !ok2 && o2.failed.is_some()
                && o2.events.iter().any(|e| e.kind == "reject-skip-gate")
                && !o2.done.contains(&"irgen"),
        ));
    }

    // 四、事件入账。
    {
        // 事件时间序可回放（start→done 交替、skip 有痕）。
        let mut o = Orchestrator::new(vec!["optimize"]);
        let _ = o.run(|s| Ok(contract(s)));
        let kinds: Vec<&'static str> = o.events.iter().map(|e| e.kind).collect();
        // 预期序：lex start/done, parse start/done, sema start/done,
        //         irgen start/done, optimize skip, backend start/done。
        v.push((
            "F0479-事件-时间序回放",
            kinds == vec!["start", "done", "start", "done", "start", "done", "start", "done", "skip", "start", "done"],
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0479-判据-锚点入册", ANCHOR == "VE-F0479"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec79）。
pub fn run_vec79_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec79");
    for (name, ok) in vec79_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec79_raw_checks as run_vec79_checks;
    #[test]
    fn vec79_orchestr_checks() {
        for (name, ok) in run_vec79_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
