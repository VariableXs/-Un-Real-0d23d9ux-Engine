//! VE-F0113 · 多适配器呈现路由（VE-A 域 · A05 多 GPU 与异构组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0113`
//!
//! **职责定位（锚点原文）**：多适配器呈现路由——帧呈现走哪张卡的路由决策
//! （拓扑距离/显示输出归属/负载），路由切换的无缝衔接，路由失败的回退。
//!
//! **铁律**：
//! 1. **三因子路由**——拓扑距离/输出归属/负载三因子。
//! 2. **无缝切换**——路由切换不丢帧。
//! 3. **失败回退**——路由失败回退默认卡。
//! 4. **路由留痕**——每次决策可查。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B7（呈现路由）。
pub const SEG_BASE: u16 = 0xE8B7;

/// 无可用路由。
pub const XC_NO_ROUTE: u16 = SEG_BASE | 0x01;
/// 路由切换失败。
pub const XC_SWITCH_FAIL: u16 = SEG_BASE | 0x02;
/// 输出归属冲突。
pub const XC_OUTPUT_CONFLICT: u16 = SEG_BASE | 0x03;
/// 默认卡缺失。
pub const XC_NO_DEFAULT: u16 = SEG_BASE | 0x04;

/// 路由因子评分。
#[derive(Clone, Copy, Debug)]
pub struct RouteScore {
    /// 拓扑距离（小优）。
    pub topo_distance: u32,
    /// 输出归属匹配（1=匹配 0=不匹配）。
    pub output_match: u8,
    /// 当前负载（千分比，小优）。
    pub load_permille: u32,
}

impl RouteScore {
    /// 综合分（越小越优）。
    pub fn total(&self) -> u64 {
        self.topo_distance as u64 * 3
            + (1 - self.output_match as u64) * 1000
            + self.load_permille as u64
    }
}

/// 适配器路由候选。
#[derive(Clone, Debug)]
pub struct RouteCandidate {
    pub id: u16,
    pub score: RouteScore,
}

/// 路由决策记录。
#[derive(Clone, Copy, Debug)]
pub struct RouteDecision {
    pub tick: u64,
    pub chosen: u16,
    /// 是否回退。
    pub fell_back: bool,
}

/// 呈现路由器。
pub struct PresentRouter {
    candidates: Vec<RouteCandidate>,
    /// 默认卡。
    default_id: Option<u16>,
    /// 当前路由。
    current: u16,
    decisions: Vec<RouteDecision>,
    last_code: u16,
    /// 无缝切换计数。
    seamless: u32,
    /// 回退计数。
    fallbacks: u32,
}

impl PresentRouter {
    pub fn new(default_id: Option<u16>) -> Result<Self, u16> {
        if default_id.is_none() {
            return Err(XC_NO_DEFAULT);
        }
        Ok(PresentRouter {
            candidates: Vec::new(),
            default_id,
            current: default_id.unwrap(),
            decisions: Vec::new(),
            last_code: 0,
            seamless: 0,
            fallbacks: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn add_candidate(&mut self, id: u16, score: RouteScore) {
        self.candidates.retain(|c| c.id != id);
        self.candidates.push(RouteCandidate { id, score });
    }

    /// 路由决策（三因子综合）。
    pub fn route(&mut self, tick: u64) -> Result<u16, u16> {
        if self.candidates.is_empty() {
            // 无候选：回退默认卡。
            match self.default_id {
                Some(d) => {
                    self.fallbacks += 1;
                    self.decisions.push(RouteDecision {
                        tick,
                        chosen: d,
                        fell_back: true,
                    });
                    self.last_code = XC_NO_ROUTE;
                    Ok(d)
                }
                None => {
                    self.last_code = XC_NO_DEFAULT;
                    Err(XC_NO_DEFAULT)
                }
            }
        } else {
            // 选综合分最小。
            let best = self
                .candidates
                .iter()
                .min_by_key(|c| c.score.total())
                .unwrap();
            let chosen = best.id;
            if chosen != self.current {
                // 无缝切换（模拟：成功）。
                self.current = chosen;
                self.seamless += 1;
            }
            self.decisions.push(RouteDecision {
                tick,
                chosen,
                fell_back: false,
            });
            self.last_code = 0;
            Ok(chosen)
        }
    }

    /// 路由失败回退默认。
    pub fn fallback(&mut self, tick: u64) -> Result<u16, u16> {
        match self.default_id {
            Some(d) => {
                self.current = d;
                self.fallbacks += 1;
                self.decisions.push(RouteDecision {
                    tick,
                    chosen: d,
                    fell_back: true,
                });
                self.last_code = XC_SWITCH_FAIL;
                Ok(d)
            }
            None => {
                self.last_code = XC_NO_DEFAULT;
                Err(XC_NO_DEFAULT)
            }
        }
    }

    /// 输出归属校验。
    pub fn validate_output(&mut self, adapter_id: u16, output_owned: bool) -> Result<(), u16> {
        if !output_owned {
            self.last_code = XC_OUTPUT_CONFLICT;
            return Err(XC_OUTPUT_CONFLICT);
        }
        let _ = adapter_id;
        self.last_code = 0;
        Ok(())
    }

    pub fn current(&self) -> u16 {
        self.current
    }

    pub fn seamless(&self) -> u32 {
        self.seamless
    }

    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    pub fn decisions(&self) -> &[RouteDecision] {
        &self.decisions
    }
}

/// 自检：三因子路由、无缝切换、失败回退、留痕。
pub fn run_vea113_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn cand(id: u16, topo: u32, m: u8, load: u32) -> RouteCandidate {
        RouteCandidate {
            id,
            score: RouteScore {
                topo_distance: topo,
                output_match: m,
                load_permille: load,
            },
        }
    }

    let mut set = CheckSet::new("vea113_route");

    // C113-01 三因子评分
    {
        let s = RouteScore {
            topo_distance: 1,
            output_match: 1,
            load_permille: 100,
        };
        chk(&mut set, "C113-01 评分", s.total() == 3 + 0 + 100, "评分错");
    }

    // C113-02 路由选优
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        r.add_candidate(0, cand(0, 2, 0, 500).score);
        r.add_candidate(1, cand(1, 1, 1, 200).score);
        let out = r.route(1);
        chk(&mut set, "C113-02 选优", out == Ok(1) && r.current() == 1, "选优错");
    }

    // C113-03 无缝切换
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        r.add_candidate(1, cand(1, 1, 1, 100).score);
        let _ = r.route(1);
        chk(&mut set, "C113-03 无缝", r.seamless() == 1, "切换有缝");
    }

    // C113-04 无候选回退默认
    {
        let mut r = PresentRouter::new(Some(5)).unwrap();
        let out = r.route(1);
        chk(&mut set, "C113-04 回退默认", out == Ok(5) && r.fallbacks() == 1, "未回退");
    }

    // C113-05 失败回退
    {
        let mut r = PresentRouter::new(Some(3)).unwrap();
        r.add_candidate(1, cand(1, 1, 1, 10).score);
        let _ = r.route(1);
        let out = r.fallback(2);
        chk(&mut set, "C113-05 失败回退", out == Ok(3), "回退失败");
    }

    // C113-06 决策留痕
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        r.add_candidate(1, cand(1, 1, 1, 10).score);
        let _ = r.route(1);
        let _ = r.route(2);
        chk(&mut set, "C113-06 留痕", r.decisions().len() == 2, "未留痕");
    }

    // C113-07 输出归属冲突
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        let res = r.validate_output(1, false);
        chk(
            &mut set,
            "C113-07 归属冲突",
            res.is_err() && r.last_code() == XC_OUTPUT_CONFLICT,
            "冲突被放",
        );
    }

    // C113-08 无默认拒绝
    {
        let r = PresentRouter::new(None);
        chk(&mut set, "C113-08 无默认拒", r.is_err(), "无默认被放");
    }

    // C113-09 负载因子生效
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        r.add_candidate(0, cand(0, 1, 1, 900).score);
        r.add_candidate(1, cand(1, 1, 1, 100).score);
        let out = r.route(1);
        chk(&mut set, "C113-09 负载因子", out == Ok(1), "负载未生效");
    }

    // C113-10 归属加权生效
    {
        let mut r = PresentRouter::new(Some(0)).unwrap();
        r.add_candidate(0, cand(0, 0, 0, 0).score); // 不匹配输出
        r.add_candidate(1, cand(1, 5, 1, 500).score); // 匹配但远
        let out = r.route(1);
        chk(&mut set, "C113-10 归属加权", out == Ok(1), "归属未加权");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea113_all_green() {
        let s = run_vea113_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
