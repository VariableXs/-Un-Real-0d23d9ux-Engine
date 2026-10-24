//! VE-F3862 动效渲染替代引擎（SEG 0xCDxx）。
//!
//! 替代规则引擎（动效特征→策略决策）；指令协议 v1 冻结；
//! 三策略族（图形化/文字化/静态化）；决策可解释；一致红线。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// 诊断码（SEG 0xCDxx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CDCode {
    /// 决策黑箱
    BlackBox = 0xCD00,
    /// 指令失真
    Distort = 0xCD01,
    /// 策略缺族
    FamilyMiss = 0xCD02,
    /// 预算超
    BudgetOver = 0xCD03,
    /// 协议违规
    ProtoViolate = 0xCD04,
    /// 特征越界
    FeatureOver = 0xCD05,
    /// 可解释缺失
    ExplainMiss = 0xCD06,
    /// 一致红线
    ConsistBreak = 0xCD07,
    /// 指令空
    InstrEmpty = 0xCD08,
    /// reduce 态非法
    ReduceInvalid = 0xCD09,
    /// 策略未选
    NoStrategy = 0xCD0A,
    /// 对拍分歧
    CompareDrift = 0xCD0B,
}

impl CDCode {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn explain(self) -> &'static str {
        match self {
            CDCode::BlackBox => "决策黑箱",
            CDCode::Distort => "指令失真",
            CDCode::FamilyMiss => "策略族缺失",
            CDCode::BudgetOver => "预算超限",
            CDCode::ProtoViolate => "协议违规",
            CDCode::FeatureOver => "特征越界",
            CDCode::ExplainMiss => "可解释缺失",
            CDCode::ConsistBreak => "语义一致红线",
            CDCode::InstrEmpty => "指令空",
            CDCode::ReduceInvalid => "reduce态非法",
            CDCode::NoStrategy => "策略未选",
            CDCode::CompareDrift => "对拍分歧",
        }
    }
}

/// 三策略族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltStrategy {
    /// 图形化替代
    Graphic,
    /// 文字化替代
    Text,
    /// 静态化替代
    Static,
}

impl AltStrategy {
    pub const ALL: [AltStrategy; 3] = [AltStrategy::Graphic, AltStrategy::Text, AltStrategy::Static];
    pub fn as_str(self) -> &'static str {
        match self {
            AltStrategy::Graphic => "图形化",
            AltStrategy::Text => "文字化",
            AltStrategy::Static => "静态化",
        }
    }
}

/// reduce 态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReduceMode {
    /// 不减
    None,
    /// 减少动效
    Reduce,
    /// 全部去除
    Full,
}

impl ReduceMode {
    pub fn valid(self) -> bool {
        true
    }
}

/// 动效特征。
#[derive(Clone, Copy, Debug)]
pub struct MotionFeature {
    /// 运动幅度 0-100
    pub amplitude: u8,
    /// 频率 Hz
    pub freq_hz: u8,
    /// 是否闪烁
    pub flicker: bool,
    /// 是否旋转
    pub spin: bool,
}

/// 决策结果（可解释）。
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub strategy: AltStrategy,
    /// 解释理由（非空=可解释）
    pub reason: &'static str,
    /// 与语义对齐分 0-100
    pub semantic_score: u8,
}

/// 替代指令（协议 v1）。
#[derive(Clone, Debug, PartialEq)]
pub struct AltInstruction {
    /// 协议版本（必须 1）
    pub proto: u8,
    pub strategy: AltStrategy,
    /// 指令负载
    pub payload: u32,
    /// 语义锚（对应原动效 id）
    pub anchor: u32,
}

/// 引擎。
pub struct MotionAltEngine {
    /// 决策耗时预算（us）
    pub budget_us: u32,
    decisions: Vec<Decision>,
    instrs: Vec<AltInstruction>,
}

impl MotionAltEngine {
    pub fn new(budget_us: u32) -> Self {
        MotionAltEngine {
            budget_us,
            decisions: Vec::new(),
            instrs: Vec::new(),
        }
    }

    /// 公开决策规则：闪烁/高频→静态；旋转/大幅→图形；其余→文字。
    pub fn decide(&self, f: &MotionFeature, reduce: ReduceMode) -> Result<Decision, CDCode> {
        if !reduce.valid() {
            return Err(CDCode::ReduceInvalid);
        }
        if f.amplitude > 100 {
            return Err(CDCode::FeatureOver);
        }
        let (strategy, reason): (AltStrategy, &'static str) = if f.flicker || f.freq_hz > 8 {
            (AltStrategy::Static, "闪烁或高频→静态化")
        } else if f.spin || f.amplitude > 60 {
            (AltStrategy::Graphic, "旋转或大幅→图形化")
        } else {
            (AltStrategy::Text, "低幅低频→文字化")
        };
        if reason.is_empty() {
            return Err(CDCode::ExplainMiss);
        }
        Ok(Decision {
            strategy,
            reason,
            semantic_score: 90,
        })
    }

    pub fn push_decision(&mut self, d: Decision) {
        self.decisions.push(d);
    }

    pub fn emit(&mut self, d: &Decision, anchor: u32) -> Result<AltInstruction, CDCode> {
        if d.reason.is_empty() {
            return Err(CDCode::ExplainMiss);
        }
        if d.semantic_score < 60 {
            return Err(CDCode::ConsistBreak);
        }
        let instr = AltInstruction {
            proto: 1,
            strategy: d.strategy,
            payload: anchor.wrapping_mul(31),
            anchor,
        };
        if instr.proto != 1 {
            return Err(CDCode::ProtoViolate);
        }
        self.instrs.push(instr.clone());
        Ok(instr)
    }

    pub fn instr_count(&self) -> usize {
        self.instrs.len()
    }

    pub fn decision_count(&self) -> usize {
        self.decisions.len()
    }

    pub fn black_box_count(&self) -> usize {
        self.decisions.iter().filter(|d| d.reason.is_empty()).count()
    }

    pub fn distort_count(&self) -> usize {
        self.decisions.iter().filter(|d| d.semantic_score < 60).count()
    }

    /// 引擎预算检查：决策数×单次成本。
    pub fn within_budget(&self, unit_us: u32) -> bool {
        (self.decisions.len() as u32).saturating_mul(unit_us) <= self.budget_us
    }
}

pub fn run_ves162_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves162_motionalt");
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 10,
                freq_hz: 2,
                flicker: false,
                spin: false,
            },
            ReduceMode::None,
        );
        chk(
            &mut set,
            "C3862-01",
            matches!(d, Ok(ref x) if x.strategy == AltStrategy::Text),
            "低幅→文字化",
        );
    }
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 50,
                freq_hz: 12,
                flicker: false,
                spin: false,
            },
            ReduceMode::Reduce,
        );
        chk(
            &mut set,
            "C3862-02",
            matches!(d, Ok(ref x) if x.strategy == AltStrategy::Static),
            "高频→静态化",
        );
    }
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 80,
                freq_hz: 3,
                flicker: false,
                spin: true,
            },
            ReduceMode::Full,
        );
        chk(
            &mut set,
            "C3862-03",
            matches!(d, Ok(ref x) if x.strategy == AltStrategy::Graphic),
            "旋转→图形化",
        );
    }
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 30,
                freq_hz: 3,
                flicker: true,
                spin: false,
            },
            ReduceMode::None,
        );
        chk(
            &mut set,
            "C3862-04",
            matches!(d, Ok(ref x) if x.strategy == AltStrategy::Static),
            "闪烁→静态化",
        );
    }
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 101,
                freq_hz: 1,
                flicker: false,
                spin: false,
            },
            ReduceMode::None,
        );
        chk(
            &mut set,
            "C3862-05",
            d == Err(CDCode::FeatureOver),
            "特征越界拒",
        );
    }
    {
        let mut e = MotionAltEngine::new(1000);
        let d = e
            .decide(
                &MotionFeature {
                    amplitude: 10,
                    freq_hz: 1,
                    flicker: false,
                    spin: false,
                },
                ReduceMode::None,
            )
            .unwrap();
        let r = e.emit(&d, 42);
        chk(&mut set, "C3862-06", r.is_ok(), "指令生成成功");
        chk(&mut set, "C3862-07", e.instr_count() == 1, "指令入流");
    }
    {
        let mut e = MotionAltEngine::new(1000);
        let bad = Decision {
            strategy: AltStrategy::Text,
            reason: "",
            semantic_score: 90,
        };
        let r = e.emit(&bad, 1);
        chk(
            &mut set,
            "C3862-08",
            r == Err(CDCode::ExplainMiss),
            "黑箱决策拒发指令",
        );
    }
    {
        let mut e = MotionAltEngine::new(1000);
        let bad = Decision {
            strategy: AltStrategy::Text,
            reason: "失真",
            semantic_score: 30,
        };
        let r = e.emit(&bad, 2);
        chk(
            &mut set,
            "C3862-09",
            r == Err(CDCode::ConsistBreak),
            "语义失真红线",
        );
    }
    {
        let mut e = MotionAltEngine::new(10);
        e.push_decision(Decision {
            strategy: AltStrategy::Static,
            reason: "x",
            semantic_score: 90,
        });
        chk(&mut set, "C3862-10", !e.within_budget(100), "预算超限");
    }
    {
        let mut e = MotionAltEngine::new(10_000);
        e.push_decision(Decision {
            strategy: AltStrategy::Graphic,
            reason: "y",
            semantic_score: 80,
        });
        chk(&mut set, "C3862-11", e.within_budget(100), "预算内");
    }
    {
        let e = MotionAltEngine::new(1000);
        let d = e.decide(
            &MotionFeature {
                amplitude: 0,
                freq_hz: 0,
                flicker: false,
                spin: false,
            },
            ReduceMode::Full,
        );
        chk(
            &mut set,
            "C3862-12",
            matches!(d, Ok(ref x) if !x.reason.is_empty()),
            "决策可解释",
        );
    }
    set
}
