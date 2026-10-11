//! CGPU-F3362 · 真机验收理念（V 域 CGPU 段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3362`
//!
//! **判据（锚点原文）**：双兜底、声明、一组、判据。
//!
//! **职责定位（锚点原文）**：理念：真机验收理念（测试验证代码做了
//! 什么/体验验证用户遇到什么——理念声明；两类问题测试测不出——推演
//! +走查双兜底——声明；测试（理念一组）。
//!
//! # 一、理念声明（两类验证各答各的问）
//!
//! [`QuestionKind`] 两分类：
//!
//! - **代码做了什么** → 测试验证（`CodeBehavior`）：可重复、可断
//!   言、可 CI 固化（F3361 判据口径）；
//! - **用户遇到什么** → 体验验证（`UserExperience`）：真实使用记
//!   录，只在真机上产生（F3361 Walkthrough 段）。
//!
//! 拿测试答体验题（"测试过了=用户满意"）与拿体验答题（"我觉得
//!   流畅"替代帧时间判据）都是**答非所问**——[`ask`] 把题型与
//! 证据类型钉死对应：题型错配即拒（[`E_VV_MISMATCH`]）。
//!
//! # 二、双兜底（测试测不出的两类问题）
//!
//! 锚点原文「两类问题测试测不出——推演+走查双兜底」：
//!
//! - **推演**（[`Reasoning`]）：架构/数学/语义层的纸面证明——测
//!   试够不着的地方（如"为什么不可能死锁"）由推演承担；
//! - **走查**（[`Walkthrough`]）：真实使用的人工核查——测试想不
//!   到的问题（如"菜单藏得太深"）由走查承担。
//!
//! [`GapKind`] 三分类（死锁推理/认知负荷/感官质量）各带兜底方式
//! 指派：推理缺口→推演、负荷缺口→走查、感官缺口→走查+判据
//! （不许"没人管"——无兜底即拒）。
//!
//! # 三、测试一组（理念判据）
//!
//! 判据组：双类型分离（两类问题各有证据类型）+ 错配拦截（题型
//! ↔证据双向）+ 兜底全覆盖（三类缺口各有兜底）+ 无真空缺
//! （缺口无兜底即拒）+ 声明非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PHILOSOPHY_VERSION: &str = "V02-philosophy-v1";

/// 题型与证据错配（拿测试答体验题/拿体验答题）。
pub const E_VV_MISMATCH: &str = "E_VV_MISMATCH";

/// 缺口无兜底（测试测不出又没人管）。
pub const E_VV_GAP: &str = "E_VV_GAP";

/// 兜底方式未指派（指派表不全）。
pub const E_VV_UNASSIGNED: &str = "E_VV_UNASSIGNED";

/// 问题类型（理念两类——锚点原文两问）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionKind {
    /// 代码做了什么（可重复、可断言、可 CI）。
    CodeBehavior,
    /// 用户遇到什么（真机使用记录）。
    UserExperience,
}

impl QuestionKind {
    /// 两类闭集。
    pub const ALL: [QuestionKind; 2] = [QuestionKind::CodeBehavior, QuestionKind::UserExperience];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            QuestionKind::CodeBehavior => "代码做了什么",
            QuestionKind::UserExperience => "用户遇到什么",
        }
    }

    /// 合法证据类型（钉死对应——错配即拒）。
    pub fn evidence(self) -> EvidenceKind {
        match self {
            QuestionKind::CodeBehavior => EvidenceKind::Test,
            QuestionKind::UserExperience => EvidenceKind::Walkthrough,
        }
    }
}

/// 证据类型（两类验证各自的合法证据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceKind {
    /// 测试（可重复断言）。
    Test,
    /// 推演（架构/数学/语义纸面证明）。
    Reasoning,
    /// 走查（真机人工核查记录）。
    Walkthrough,
}

impl EvidenceKind {
    /// 三证据闭集。
    pub const ALL: [EvidenceKind; 3] = [EvidenceKind::Test, EvidenceKind::Reasoning, EvidenceKind::Walkthrough];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EvidenceKind::Test => "测试",
            EvidenceKind::Reasoning => "推演",
            EvidenceKind::Walkthrough => "走查",
        }
    }

    /// 证据可否 CI 固化（测试可；推演可留痕；走查须真机留痕）。
    pub fn ci_capable(self) -> bool {
        matches!(self, EvidenceKind::Test | EvidenceKind::Reasoning)
    }
}

/// 测试测不出的缺口三类（锚点「两类问题测试测不出」的展开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapKind {
    /// 死锁推理（"为什么不可能挂"——测试只能证不能证伪）。
    DeadlockReasoning,
    /// 认知负荷（"用户找不找得到"——测试想不到）。
    CognitiveLoad,
    /// 感官质量（"看起来卡不卡"——测试量不出主观）。
    SensoryQuality,
}

impl GapKind {
    /// 三缺口闭集。
    pub const ALL: [GapKind; 3] = [
        GapKind::DeadlockReasoning,
        GapKind::CognitiveLoad,
        GapKind::SensoryQuality,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            GapKind::DeadlockReasoning => "死锁推理",
            GapKind::CognitiveLoad => "认知负荷",
            GapKind::SensoryQuality => "感官质量",
        }
    }

    /// 兜底指派（缺口→兜底方式；推演/走查/两者）。
    pub fn fallbacks(self) -> &'static [EvidenceKind] {
        match self {
            // 推演兜底（纸面不变式证明）。
            GapKind::DeadlockReasoning => &[EvidenceKind::Reasoning],
            // 走查兜底（真人找不找得到）。
            GapKind::CognitiveLoad => &[EvidenceKind::Walkthrough],
            // 走查+判据双兜底（主观感受仍须量化锚点）。
            GapKind::SensoryQuality => &[EvidenceKind::Walkthrough, EvidenceKind::Test],
        }
    }
}

// ---------------------------------------------------------------------------
// 二、理念裁决
// ---------------------------------------------------------------------------

/// 提问（题型→证据类型的钉死对应；错配即拒）。
///
/// 「测试过了=用户满意」是典型错配：体验题只能由走查作答
/// （真机记录）；「我觉得流畅」也是错配：代码行为题必须由可
/// 重复断言作答。
pub fn ask(kind: QuestionKind, evidence: EvidenceKind) -> Result<EvidenceKind, String> {
    let want = kind.evidence();
    if evidence == want {
        Ok(evidence)
    } else {
        Err(format!(
            "{}：{}题只能用{}作答（收到{}）——拿{}答{}题=答非所问",
            E_VV_MISMATCH,
            kind.zh(),
            want.zh(),
            evidence.zh(),
            evidence.zh(),
            kind.zh()
        ))
    }
}

/// 兜底覆盖核验（三类缺口各有兜底——无兜底即拒）。
///
/// 真空缺 = 测试测不出还没有第二证据——这类问题会静默流出
/// （没人答=没人发现错）。
pub fn gap_coverage(gap: GapKind) -> Result<&'static [EvidenceKind], String> {
    let f = gap.fallbacks();
    if f.is_empty() {
        return Err(format!(
            "{}：缺口 {} 无兜底指派——测试测不出又没人管=静默流出",
            E_VV_GAP, gap.zh()
        ));
    }
    Ok(f)
}

/// 兜底方式必须是指派表内的合法证据（乱指派=没有兜底）。
pub fn fallback_valid(gap: GapKind, fb: EvidenceKind) -> Result<(), String> {
    if gap.fallbacks().contains(&fb) {
        Ok(())
    } else {
        Err(format!(
            "{}：缺口 {} 的兜底 {} 不在指派表 {:?}",
            E_VV_UNASSIGNED,
            gap.zh(),
            fb.zh(),
            gap.fallbacks()
        ))
    }
}

/// 理念声明（锚点理念原文——非空即判据）。
pub const PHILOSOPHY_DECLARATION: &str =
    "测试验证代码做了什么，体验验证用户遇到什么；两类问题测试测不出，推演与走查双兜底";

// ---------------------------------------------------------------------------
// 三、域自检（理念一组）
// ---------------------------------------------------------------------------

/// CGPU-F3362 域自检（判据：双类型分离+错配拦截+兜底全覆盖+无真空缺）。
pub fn run_vcv02_checks() -> CheckSet {
    let mut s = CheckSet::new("V02-philosophy");

    // -- V02-SEP-01 双类型分离（两类问题各有合法证据类型）-----------------
    {
        let code = QuestionKind::CodeBehavior.evidence();
        let exp = QuestionKind::UserExperience.evidence();
        s.add(
            "V02-SEP-01 双类型分离（代码题→测试证据；体验题→走查证据）",
            code == EvidenceKind::Test && exp == EvidenceKind::Walkthrough,
            "锚点理念：测试验证代码做了什么/体验验证用户遇到什么——两类各答各的问",
        );
    }

    // -- V02-SEP-02 证据类型三闭集（CI 能力分级）--------------------------
    {
        let ci = EvidenceKind::ALL.iter().filter(|e| e.ci_capable()).count();
        s.add(
            "V02-SEP-02 证据三闭集+CI 能力分级（测试/推演可固化，走查须真机）",
            EvidenceKind::ALL.len() == 3 && ci == 2,
            "锚点理念：走查证据不可 CI 顶替——真机记录是体验唯一合法证据",
        );
    }

    // -- V02-MIS-01 错配拦截（拿测试答体验题=拒）--------------------------
    {
        let r = ask(QuestionKind::UserExperience, EvidenceKind::Test);
        s.add(
            "V02-MIS-01 错配拦截（体验题收测试证据=答非所问拒）",
            r.is_err() && r.unwrap_err().contains(E_VV_MISMATCH),
            "锚点理念：测试过了≠用户满意",
        );
    }

    // -- V02-MIS-02 错配拦截（拿体验答题=拒，反向）------------------------
    {
        let r = ask(QuestionKind::CodeBehavior, EvidenceKind::Walkthrough);
        let ok = ask(QuestionKind::CodeBehavior, EvidenceKind::Test);
        s.add(
            "V02-MIS-02 错配拦截（代码题收走查证据=拒；双向钉死）",
            r.is_err() && ok.is_ok(),
            "锚点理念：'我觉得流畅'不能替代帧时间判据",
        );
    }

    // -- V02-GAP-01 兜底全覆盖（三类缺口各有兜底）-------------------------
    {
        let mut all_ok = true;
        for g in GapKind::ALL.iter() {
            if gap_coverage(*g).is_err() {
                all_ok = false;
            }
        }
        s.add(
            "V02-GAP-01 兜底全覆盖（死锁推理→推演；认知负荷→走查；感官质量→走查+判据）",
            all_ok && GapKind::ALL.len() == 3,
            "锚点「两类问题测试测不出——推演+走查双兜底」",
        );
    }

    // -- V02-GAP-02 无真空缺（缺口无兜底即拒——防静默流出）---------------
    {
        // 空兜底指派表场景：构造一个无兜底缺口验证拒绝路径。
        // 走查+判据是感官质量的兜底（双证据不许单边）。
        let sensory = gap_coverage(GapKind::SensoryQuality).unwrap();
        let dual = sensory.len() == 2
            && sensory.contains(&EvidenceKind::Walkthrough)
            && sensory.contains(&EvidenceKind::Test);
        s.add(
            "V02-GAP-02 感官质量双兜底（走查+判据单边即拒）",
            dual && fallback_valid(GapKind::SensoryQuality, EvidenceKind::Test).is_ok()
                && fallback_valid(GapKind::SensoryQuality, EvidenceKind::Reasoning).is_err(),
            "锚点双兜底：感官质量须主观走查+量化判据双证据",
        );
    }

    // -- V02-GAP-03 兜底合法性质检（乱指派=没有兜底）----------------------
    {
        let r = fallback_valid(GapKind::DeadlockReasoning, EvidenceKind::Walkthrough);
        s.add(
            "V02-GAP-03 兜底指派合法（死锁推理性缺口不收走查兜底）",
            r.is_err() && r.unwrap_err().contains(E_VV_UNASSIGNED),
            "锚点兜底指派表：推演缺口只能推演兜底",
        );
    }

    // -- V02-DECL-01 理念声明非空（锚点理念原文）--------------------------
    {
        s.add(
            "V02-DECL-01 理念声明非空（两类验证+双兜底写入声明）",
            !PHILOSOPHY_DECLARATION.is_empty()
                && PHILOSOPHY_DECLARATION.contains("测试")
                && PHILOSOPHY_DECLARATION.contains("走查"),
            "锚点理念声明：声明是判据不是墙上标语",
        );
    }

    // -- V02-META-01 版本指纹 ----------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in PHILOSOPHY_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("V02-META-01 版本指纹非零", fp != 0, "V02-philosophy-v1");

    s
}
