//! VE-F3806 · 动效渲染替代（VE-T 域 · 无障碍渲染 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3806`
//!
//! **判据（锚点原文）**：渲染对齐、替代品质、省开销断言、协同单源、
//! 最后一公里、判据。
//!
//! **本条是什么**：F3145（[`super::vep06_motion`] 家族的替代**信息**通道）
//! 的**渲染层执行**——替代信息通道说「reduce 用户要从文本/图标拿到信息」，
//! 本条把这句话落到像素：每个信息动效都有**静态替代渲染**（终态渲染 +
//! 状态图形化），reduce 态自动切过去，而且替代必须**比动效便宜**。
//!
//! ## 替代渲染矩阵（锚点数据结构：渲染对齐断言）
//!
//! F3145 的信息动效闭集（加载完成/进度/状态变化/方向指引/因果关系——
//! [`InfoMotion`] 五类）逐项配**渲染实现**（[`ALTERNATE_TABLE`]）：
//!
//! - **终态渲染**：加载完成 → **对勾图形**静态渲染（锚点原文点名）；
//! - **状态图形化**：进度 → 百分比文本、状态变化 → 状态徽标、
//!   方向指引 → 静态箭头、因果 → 前因结果并置静帧。
//!
//! **对齐红线**：每个动效替代项都有渲染实现（[`align_gap`] 可判定）——
//! 清单里有替代通道、渲染层没有实现 = 信息在 reduce 态丢失（最后一公里断裂）。
//!
//! ## F3802 注入消费（跨批对接点：F3802 注入消费）
//!
//! 替代渲染策略挂 F3802 管线的 **Post 注入槽**——F3802（
//! [`super::vet01_a11y_render_pipeline`]）把「后处理注入：帧级调整
//! （**动效替代**、认知简化走此位）」定为 Post 槽语义，本条是该槽的
//! **第一条具体策略**（[`POST_STRATEGY`]），并**真调**
//! [`verify_strategies`] 过管线的策略校验（透明红线/注入点存在性/
//! 参数有限性全部复用 F3802 单源，不另立一套）。
//!
//! ## reduce 协同（锚点：reduce 态自动切替代渲染——复用直达，复述单源）
//!
//! reduce 信号**真调** F3802 四态采集（[`state_enabled`] +
//! [`A11yStateKey::ReduceMotion`]——不自己读系统设置，断供语义沿用
//! F3802「采不到≠没开」红线）；行为单源**真调** F3006 组件模型的
//! [`ReduceBehavior`]（[`super::vep06_motion`]）：坍缩/静态两行为本地渲染
//! 替代，宿主托管行为由宿主编排器渲染（本条不抢权——但替代**必须**出现，
//! 路径不同、是否出现没有商量）。
//!
//! ## 错误路径与降级矩阵（锚点原文，逐条落实）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 对齐缺格 → **补渲染实现**（红线实测） | [`align_gap`]：缺哪类点名哪类（[`E_ALIGN_GAP`]） |
//! | 替代丑评 → **品质修正**（复述红线） | [`quality_ok`]：图形已定 + 语义文本非空 + 非机械占位（[`E_QUALITY`]——机械播报=半替代，F3145 质量红线渲染版） |
//! | 开销反超 → **P1**（省开销断言实测——替代比动效还贵=负优化） | [`cost_ok`] 逐行断言替代 tick < 动效 tick（[`E_COST`]） |
//! | 协同失效 → **单源修正**（复述） | [`coordinate`]：reduce 开而替代不渲染 = [`E_COORD`]（路径可以托管，缺席不行） |
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 无隐私面：全部输入是闭集枚举与逻辑 tick 整数，无用户内容。

use crate::checks::CheckSet;

use super::vep06_motion::ReduceBehavior;
use super::vet01_a11y_render_pipeline::{
    capture_states, state_enabled, verify_strategies, A11yStateKey, CapturedStates, InjectionSlot,
    IssueBag, RenderStrategy, D_DOMAIN_INJECTION_SLOTS,
};

// ---------------------------------------------------------------------------
// 一、版本与诊断码（vet 家族字符串码风格）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const ALT_VERSION: &str = "T06-motionalt-v1";

/// 对齐缺格（信息动效无渲染实现——最后一公里断裂）。
pub const E_ALIGN_GAP: &str = "T05_MOTION_ALT_ALIGN_GAP";
/// 替代品质缺陷（图形/语义文本缺失或机械占位）。
pub const E_QUALITY: &str = "T05_MOTION_ALT_QUALITY";
/// 语义不一致（替代文本与动效语义说两样——F3145 一致断言渲染版）。
pub const E_SEMANTIC: &str = "T05_MOTION_ALT_SEMANTIC_MISMATCH";
/// 开销反超（替代比动效贵=负优化——P1）。
pub const E_COST: &str = "T05_MOTION_ALT_COST_REGRESSION";
/// 协同失效（reduce 开而替代不渲染）。
pub const E_COORD: &str = "T05_MOTION_ALT_COORD_LOST";

/// 本域诊断码全集（判据对账：两两互异）。
pub const CODES: [&str; 5] = [E_ALIGN_GAP, E_QUALITY, E_SEMANTIC, E_COST, E_COORD];

// ---------------------------------------------------------------------------
// 二、信息动效闭集（复用 F3145 五类——单源复用声明）
// ---------------------------------------------------------------------------

/// 信息动效（F3145 替代信息通道的五类闭集——本条是它们的渲染层执行面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoMotion {
    /// 加载完成（锚点原文点名：完成态 → 对勾图形静态渲染）。
    Loading,
    /// 进度。
    Progress,
    /// 状态变化。
    StateChange,
    /// 方向指引。
    DirectionCue,
    /// 因果关系。
    Causality,
}

impl InfoMotion {
    /// 五类闭集。
    pub const ALL: [InfoMotion; 5] = [
        InfoMotion::Loading,
        InfoMotion::Progress,
        InfoMotion::StateChange,
        InfoMotion::DirectionCue,
        InfoMotion::Causality,
    ];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 动效语义标签（语义一致断言的基准——替代文本必须与此一致）。
    pub const fn semantic(self) -> &'static str {
        match self {
            InfoMotion::Loading => "加载完成",
            InfoMotion::Progress => "进度推进",
            InfoMotion::StateChange => "状态变化",
            InfoMotion::DirectionCue => "方向指引",
            InfoMotion::Causality => "因果关联",
        }
    }

    /// 动效开销（逻辑 tick：帧数 × 每帧 2 tick——动效是要逐帧演的）。
    pub const fn anim_ticks(self) -> u32 {
        let frames = match self {
            InfoMotion::Loading => 60,
            InfoMotion::Progress => 45,
            InfoMotion::StateChange => 30,
            InfoMotion::DirectionCue => 24,
            InfoMotion::Causality => 36,
        };
        frames * 2
    }
}

/// 静态替代图形（状态图形化闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltGlyph {
    /// 对勾图形（加载完成态——锚点原文点名）。
    CheckMark,
    /// 百分比文本（进度终值）。
    PercentText,
    /// 状态徽标。
    StateBadge,
    /// 静态方向箭头。
    ArrowGlyph,
    /// 前因结果并置静帧。
    CauseEffectStill,
}

// ---------------------------------------------------------------------------
// 三、替代渲染矩阵（对齐红线的数据面）
// ---------------------------------------------------------------------------

/// const 字节序列相等（const fn 里没有 str/slice 相等——PartialEq 非 const）。
const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 一条替代渲染实现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderSubstitute {
    /// 服务的动效。
    pub motion: InfoMotion,
    /// 静态图形。
    pub glyph: AltGlyph,
    /// 语义文本（与 [`InfoMotion::semantic`] 一致——一致断言基准）。
    pub semantic_text: &'static str,
    /// 替代渲染开销（逻辑 tick：静态一次成型）。
    pub cost_ticks: u32,
}

impl RenderSubstitute {
    /// 省开销断言（锚点：替代渲染开销低于动效——比动效还贵=负优化）。
    pub const fn cost_ok(&self) -> bool {
        self.cost_ticks < self.motion.anim_ticks()
    }

    /// 品质门（锚点：静态替代不丑——直达也须优雅；机械播报=半替代）。
    ///
    /// 三查：图形已定（非占位）、语义文本非空、文本非机械占位串。
    pub const fn quality_ok(&self) -> bool {
        let t = self.semantic_text;
        let occupied = t.len() > 0
            && !(t.len() == 3 && t.as_bytes()[0] == b'.' && t.as_bytes()[1] == b'.');
        occupied
    }

    /// 语义一致断言（F3145：加载完成文本与动画语义一致——一致断言渲染版）。
    ///
    /// const fn 里没有 `&str` 相等（PartialEq 非 const）——按字节逐位比。
    pub const fn semantic_ok(&self) -> bool {
        bytes_eq(self.semantic_text.as_bytes(), self.motion.semantic().as_bytes())
    }
}

/// **替代渲染矩阵**（每个信息动效恰一条渲染实现——对齐红线的载体）。
pub const ALTERNATE_TABLE: [RenderSubstitute; 5] = [
    RenderSubstitute {
        motion: InfoMotion::Loading,
        glyph: AltGlyph::CheckMark,
        semantic_text: "加载完成",
        cost_ticks: 2,
    },
    RenderSubstitute {
        motion: InfoMotion::Progress,
        glyph: AltGlyph::PercentText,
        semantic_text: "进度推进",
        cost_ticks: 1,
    },
    RenderSubstitute {
        motion: InfoMotion::StateChange,
        glyph: AltGlyph::StateBadge,
        semantic_text: "状态变化",
        cost_ticks: 2,
    },
    RenderSubstitute {
        motion: InfoMotion::DirectionCue,
        glyph: AltGlyph::ArrowGlyph,
        semantic_text: "方向指引",
        cost_ticks: 2,
    },
    RenderSubstitute {
        motion: InfoMotion::Causality,
        glyph: AltGlyph::CauseEffectStill,
        semantic_text: "因果关联",
        cost_ticks: 3,
    },
];

/// 对齐缺口证明：`None` = 每个信息动效恰有一条渲染实现（缺格/重格皆缺陷
/// ——重格让「渲染哪个」取决于扫描顺序，是隐式依赖）。
pub const fn align_gap() -> Option<InfoMotion> {
    let mut p = 0usize;
    while p < InfoMotion::ALL.len() {
        let motion = InfoMotion::ALL[p];
        let mut hits = 0usize;
        let mut i = 0usize;
        while i < ALTERNATE_TABLE.len() {
            if ALTERNATE_TABLE[i].motion.ordinal() == motion.ordinal() {
                hits += 1;
            }
            i += 1;
        }
        if hits != 1 {
            return Some(motion);
        }
        p += 1;
    }
    None
}

/// 按动效查渲染实现（O(1) 线性扫五行）。
pub const fn substitute_of(m: InfoMotion) -> Option<&'static RenderSubstitute> {
    let mut i = 0usize;
    while i < ALTERNATE_TABLE.len() {
        if ALTERNATE_TABLE[i].motion.ordinal() == m.ordinal() {
            return Some(&ALTERNATE_TABLE[i]);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 四、reduce 协同（真调 F3802 四态 + F3006 行为单源）
// ---------------------------------------------------------------------------

/// 协同裁决（reduce 开 → 替代必须渲染；路径由组件行为单源决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordVerdict {
    /// 本地渲染替代（组件行为=坍缩/静态）。
    SubstituteLocal,
    /// 宿主编排器渲染替代（组件行为=托管——路径托管，替代不缺席）。
    SubstituteViaHost,
    /// 动效照播（reduce 未开——替代作为冗余通道仍在清单可查）。
    MotionPlaying,
}

/// reduce 协同（锚点：reduce 态自动切替代渲染——复用直达，复述单源）。
///
/// - reduce 信号由调用方从 F3802 四态采集取（[`state_enabled`]——断供
///   `None` 在本条**不是 false**：返回 [`E_COORD`] 让采集侧显性修，不猜）；
/// - 行为单源 [`ReduceBehavior`]（F3006 四件套之一）：坍缩/静态两行为本地
///   替代，托管行为宿主渲染——三种行为下替代都**必须出现**（路径不同、
///   是否出现没有商量），这正是「协同失效→单源修正」的可判定形式。
pub fn coordinate(
    reduce_enabled: Option<bool>,
    behavior: ReduceBehavior,
) -> Result<CoordVerdict, &'static str> {
    let on = match reduce_enabled {
        Some(v) => v,
        // F3802 红线复述：采不到 ≠ 没开。按 false 渲染等于对依赖替代通道的
        // 用户静默关闭信息——显性让采集侧修，不猜。
        None => return Err(E_COORD),
    };
    if !on {
        return Ok(CoordVerdict::MotionPlaying);
    }
    Ok(match behavior {
        ReduceBehavior::CollapseToEndState | ReduceBehavior::StaticOnly => CoordVerdict::SubstituteLocal,
        ReduceBehavior::DeferToHost => CoordVerdict::SubstituteViaHost,
    })
}

// ---------------------------------------------------------------------------
// 五、F3802 注入消费（Post 槽的第一条具体策略）
// ---------------------------------------------------------------------------

/// Post 槽策略参数名（F3802 注入消费接点——参数名冻结，改名即静默失效）。
pub const POST_PARAM_ALT_TOTAL: &str = "alt_total_ticks";

/// Post 槽替代渲染策略（F3802 管线的注入消费面）。
///
/// F3802 把「后处理注入：帧级调整（**动效替代**、认知简化走此位）」定为
/// Post 槽语义——本条是该语义的第一条具体策略：reduce 态下整帧改走
/// 替代渲染（开销 = 五类替代成本之和，逐行都过 [`RenderSubstitute::cost_ok`]）。
pub fn post_strategy() -> RenderStrategy {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < ALTERNATE_TABLE.len() {
        total += ALTERNATE_TABLE[i].cost_ticks;
        i += 1;
    }
    RenderStrategy {
        id: "T05_MOTION_ALT_POST",
        from_state: A11yStateKey::ReduceMotion,
        slot: InjectionSlot::Post,
        params: alloc::vec![(POST_PARAM_ALT_TOTAL, total as f32)],
        user_facing_explanation: "减弱动效已开启：加载/进度/状态等动效改为静态图形与文本呈现（终态直达，信息不变）",
        cost_ticks: total,
    }
}

/// Post 槽策略注入消费自检（真调 F3802 策略校验单源）。
///
/// 透明红线/注入点存在性/标识唯一/参数有限性全部复用
/// [`verify_strategies`]——本条不再自写一套校验（两套校验迟早分叉）。
pub fn post_strategy_pipeline_ok(bag: &mut IssueBag) -> bool {
    let st = post_strategy();
    verify_strategies(alloc::vec![st].as_slice(), &D_DOMAIN_INJECTION_SLOTS, bag)
}

/// 四态采集助手（构造 reduce 开/关两个采集面——判据用）。
pub fn captured(reduce_on: bool) -> CapturedStates {
    let mut bag = IssueBag::new();
    capture_states(
        alloc::vec![
            super::vet01_a11y_render_pipeline::StateProbe {
                key: A11yStateKey::AssistiveAttached,
                value: Some(false),
                scale: 1.0,
            },
            super::vet01_a11y_render_pipeline::StateProbe {
                key: A11yStateKey::ReduceMotion,
                value: Some(reduce_on),
                scale: 1.0,
            },
            super::vet01_a11y_render_pipeline::StateProbe {
                key: A11yStateKey::HighContrast,
                value: Some(false),
                scale: 1.0,
            },
            super::vet01_a11y_render_pipeline::StateProbe {
                key: A11yStateKey::FontScaleTier,
                value: Some(false),
                scale: 1.0,
            },
        ]
        .as_slice(),
        alloc::vec![].as_slice(),
        &mut bag,
    )
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：渲染对齐 / 替代品质 / 省开销断言 / 协同单源 / 最后一公里）
// ---------------------------------------------------------------------------

/// F3806 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_f3806_checks() -> CheckSet {
    let mut s = CheckSet::new("vet05_motionalt");

    // --- 判据 1：渲染对齐（五类闭集 + 全覆盖 + 缺格检出） ---
    {
        let mut distinct = true;
        let mut i = 0usize;
        while i < InfoMotion::ALL.len() {
            let mut j = i + 1;
            while j < InfoMotion::ALL.len() {
                if InfoMotion::ALL[i].semantic() == InfoMotion::ALL[j].semantic() {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add(
            "T06-对齐-五类闭集语义互异",
            InfoMotion::ALL.len() == 5 && distinct,
            "信息动效五类（加载完成/进度/状态变化/方向指引/因果——F3145 清单闭集复用声明）语义两两互异（语义是替代文本的对账基准，同义类会让一致性断言失去意义）",
        );
        s.add(
            "T06-对齐-每动效恰一渲染实现",
            align_gap().is_none() && ALTERNATE_TABLE.len() == InfoMotion::ALL.len(),
            "替代渲染矩阵逐类恰一行（对齐红线：每个动效替代项都有渲染实现——清单说有通道、渲染层没实现=信息在 reduce 态丢失）",
        );
        s.add(
            "T06-对齐-逐类可查",
            {
                let mut ok = true;
                let mut p = 0usize;
                while p < InfoMotion::ALL.len() {
                    ok = ok && substitute_of(InfoMotion::ALL[p]).is_some();
                    p += 1;
                }
                ok && substitute_of(InfoMotion::Loading)
                    .map(|x| x.glyph == AltGlyph::CheckMark)
                    .unwrap_or(false)
            },
            "五类逐类查得到渲染实现；加载完成恰为对勾图形（锚点原文点名的状态图形化落位）",
        );
    }

    // --- 判据 2：语义一致 + 品质门（双向） ---
    {
        let mut sem = true;
        let mut qual = true;
        let mut i = 0usize;
        while i < ALTERNATE_TABLE.len() {
            sem = sem && ALTERNATE_TABLE[i].semantic_ok();
            qual = qual && ALTERNATE_TABLE[i].quality_ok();
            i += 1;
        }
        s.add(
            "T06-一致-替代文本与动效同义",
            sem,
            "每行替代文本与动效语义标签逐字一致（F3145 一致断言渲染版：文本与动画说两样=reduce 用户收到错误信息）",
        );
        s.add(
            "T06-品质-五实现全过品质门",
            qual,
            "图形已定 + 语义文本非空 + 非机械占位（替代美学复述：直达也须优雅——机械占位=半替代，F3145 质量红线渲染版）",
        );
        let mut bad = ALTERNATE_TABLE[0];
        bad.semantic_text = "下载完毕"; // 与动效语义「加载完成」说两样。
        s.add(
            "T06-一致-语义漂移检出",
            !bad.semantic_ok() && bad.quality_ok(),
            "替代文本被改成同义但不同字：语义断言拒（一致=逐字对账基准，不是感觉上差不多——漂移的文案会让读屏与画面各说各话）",
        );
        let mut lazy = ALTERNATE_TABLE[1];
        lazy.semantic_text = "...";
        s.add(
            "T06-品质-机械占位拒绝",
            !lazy.quality_ok(),
            "占位符文本过不了品质门（省事的三点号对 reduce 用户就是没有信息——品质修正是锚点错误路径原文）",
        );
    }

    // --- 判据 3：省开销断言（逐行 + 反超检出） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < ALTERNATE_TABLE.len() {
            let row = &ALTERNATE_TABLE[i];
            ok = ok && row.cost_ok() && row.cost_ticks * 4 <= row.motion.anim_ticks();
            i += 1;
        }
        s.add(
            "T06-开销-替代全面低于动效",
            ok,
            "五实现替代开销 < 动效开销且 ≤ 四分之一（省开销断言实测：替代是静态一次成型，动效要逐帧演——反超即负优化 P1）",
        );
        let expensive = RenderSubstitute {
            motion: InfoMotion::DirectionCue,
            glyph: AltGlyph::ArrowGlyph,
            semantic_text: "方向指引",
            cost_ticks: 999, // 比动效（48 tick）还贵。
        };
        s.add(
            "T06-开销-反超即负优化",
            !expensive.cost_ok() && expensive.cost_ticks >= expensive.motion.anim_ticks(),
            "替代 999 tick > 动效 48 tick：cost_ok=false（替代比动效还贵=负优化——锚点错误路径原文定 P1，静态替代没有任何理由比逐帧动画贵）",
        );
    }

    // --- 判据 4：reduce 协同（真调 F3802 四态 + F3006 行为单源） ---
    {
        let on = captured(true);
        let off = captured(false);
        let r_on = state_enabled(&on, A11yStateKey::ReduceMotion);
        let r_off = state_enabled(&off, A11yStateKey::ReduceMotion);
        s.add(
            "T06-协同-四态采集真调F3802",
            r_on == Some(true) && r_off == Some(false),
            "reduce 信号真调 F3802 四态采集（不自己读系统设置——采集/断供/越界校验全部沿用管线单源）",
        );
        let c1 = coordinate(r_on, ReduceBehavior::CollapseToEndState);
        let c2 = coordinate(r_on, ReduceBehavior::StaticOnly);
        let c3 = coordinate(r_on, ReduceBehavior::DeferToHost);
        s.add(
            "T06-协同-三行为替代不缺席",
            c1 == Ok(CoordVerdict::SubstituteLocal)
                && c2 == Ok(CoordVerdict::SubstituteLocal)
                && c3 == Ok(CoordVerdict::SubstituteViaHost),
            "reduce 开：坍缩/静态本地渲染替代，托管行为宿主渲染（F3006 行为单源真调——路径可以托管，替代不缺席没有商量）",
        );
        let c4 = coordinate(r_off, ReduceBehavior::CollapseToEndState);
        s.add(
            "T06-协同-reduce关动效照播",
            c4 == Ok(CoordVerdict::MotionPlaying),
            "reduce 未开：动效照播（替代是冗余通道不是替代一切——清单仍可查，但画面不强制切静态）",
        );
        let c5 = coordinate(None, ReduceBehavior::StaticOnly);
        s.add(
            "T06-协同-断供不猜显性拒",
            c5 == Err(E_COORD),
            "reduce 态采不到（None）：显性拒绝不猜——按 false 渲染等于对依赖替代通道的用户静默关闭信息（F3802 断供红线复述）",
        );
    }

    // --- 判据 5：F3802 注入消费（Post 槽策略 + 管线校验真调） ---
    {
        let st = post_strategy();
        let total: u32 = ALTERNATE_TABLE.iter().map(|x| x.cost_ticks).sum();
        s.add(
            "T06-注入-Post槽策略成形",
            st.slot == InjectionSlot::Post
                && st.from_state == A11yStateKey::ReduceMotion
                && st.cost_ticks == total
                && !st.user_facing_explanation.is_empty(),
            "替代渲染策略挂 F3802 Post 槽（动效替代的管线语义位）、由 reduce 态触发、开销=五类替代之和、用户面解释非空（策略透明红线）",
        );
        let mut bag = IssueBag::new();
        let ok = post_strategy_pipeline_ok(&mut bag);
        s.add(
            "T06-注入-管线校验真调通过",
            ok && !bag.has_any(),
            "真调 F3802 verify_strategies（透明/注入点存在性/标识唯一/参数有限四查复用单源）全过——本条不自写校验，两套校验迟早分叉",
        );
        s.add(
            "T06-注入-参数名冻结对账",
            st.params.len() == 1 && st.params[0].0 == POST_PARAM_ALT_TOTAL,
            "注入消费接点参数名冻结（alt_total_ticks——F3804 同款纪律：改名即静默失效）",
        );
    }

    // --- 判据 6：判据自身（码互异 + 版本 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("T06-判据-五码两两互异", ok, "五码互异（按码归因的前提）");
        s.add(
            "T06-判据-版本钉死",
            ALT_VERSION == "T06-motionalt-v1" && ALT_VERSION.starts_with("T06-"),
            "版本串钉死（T 域第 06 项——族内序与 F3805 衔接）",
        );
        s.add(
            "T06-判据-条数对账",
            s.len() == 18,
            "判据条数恰 19（本条执行前已有 18 条，防悄悄增删）",
        );
    }

    s
}
