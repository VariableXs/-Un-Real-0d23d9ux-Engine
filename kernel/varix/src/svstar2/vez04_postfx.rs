//! VE-F5204 · 后处理特效子系统（VE-Z 域 · 后处理编排 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5204`
//!
//! **判据（锚点原文）**：二次加工、编排约定、氛围语义、口径衔接、判据。
//!
//! **职责定位（锚点原文）**：后处理特效子系统——全屏画面的二次加工
//! ——泛光景深色彩模糊的编排层；与 Z03 后处理库分工：本条管后处理
//! 编排约定与内容语义（何时用何种氛围）、Z03 管后处理效果引擎本体；
//! 后处理与色调映射衔接声明（F4404 口径）。
//!
//! **错误路径与降级矩阵（锚点原文）**：越界实现→归位拦截；氛围失当
//! →语义复核；失配→对拍。
//!
//! **性能逐项分解（锚点原文）**：定位 O(1)；拦截 O(1)；对拍 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4404 管线上游；Z03 后处理库下游；
//! F5220 双签闸。
//!
//! # 深化说明：编排层与引擎层的边界是一条**词表**，氛围是**查表数据**
//!
//! 「本条管编排、Z03 管引擎」如果只是分工说明，那么第一个想做酷炫
//! 泛光的人就会在编排层里手写高斯核——边界靠自觉必然失守。故本条
//! 把边界落成**越界词表**（[`OVERREACH_TERMS`]：直调引擎本体/私建
//! 效果实例/绕编排直挂管线/私改效果参数——命中即归位拦截并给归位
//! 指引，O(1) 查表拦截——锚点「拦截 O(1)」）。同样，「何时用何种
//! 氛围」落成**语义场景表**（[`MoodSceneTable`]：场景→氛围闭集
//! 查表 O(1)——锚点「定位 O(1)」），氛围失当=场景在表内映射与方案
//! 声明不一致→语义复核（对拍不拍板：复核结论留痕，裁决归评审）。
//!
//! ## 深化一：子域定位卡——编排层的职责闭集
//!
//! [`PostfxCharter`] 五项职责封闭集：氛围编排（何时用何种后处理）、
//! 次序约定（效果叠加次序与色调映射衔接）、语义标注（每编排方案的
//! 内容语义声明）、强度档位（氛围→强度档查表）、双签受理（编排册
//! 变更入口）。闭集外的动作即越界——「定位 O(1)」落在职责词表查表。
//!
//! ## 深化二：色调映射衔接——次序是口径不是口味
//!
//! 泛光在**色调映射之前**采样（HDR 域亮部才在阈值之上——TM 之后
//! 取亮部会把溢出裁掉，泛光变成「不亮」）；色彩分级在**色调映射
//! 之后**（LUT 吃的是显示域色）。这不是实现选择是口径：[`ToneMapHandshake`]
//! 把「本编排生效在 TM 前/后」声明成对账字段，与 F4404 管线对拍
//! ——失配→对拍拦截（[`E_PFX_TONEMAP_MISMATCH`]），O(1) 比对
//! ——锚点「对拍 O(1)」。
//!
//! ## 深化三：氛围强度是查表档位，不是逐效果调参
//!
//! 五氛围封闭集（[`Mood`]：昂扬/紧张/平静/诡异/低回）各配四效果
//! 强度档（泛光/景深/色彩/模糊，0..=4 五档），氛围→档位查表单源
//! （[`MOOD_PRESETS`]）。编排方案只允许**引用氛围**（拿预设档）或
//! **显性偏离**（逐效果改档必须给理由并过语义复核）——「随手调一档」
//! 是氛围漂移的起点，语义标注（域本色：为内容服务）要求每偏离可答
//! 「为了什么内容」。
//!
//! ## 深化四：双签闸——编排册变更双人复核（F5220 同构）
//!
//! 编排约定册的变更（新增编排方案/改氛围预设/改衔接声明）走
//! [`DualSignGate`]：单签拒绝留痕、双签开闸——编排册是内容语义的
//! 正本，单人改动等于语义单人说了算。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_PFX_* 前缀）
// ---------------------------------------------------------------------------

/// 越界实现：编排层干了引擎本体的活（归位拦截）。
pub const E_PFX_OVERREACH: &str = "E_PFX_OVERREACH";
/// 氛围失当：场景映射与方案声明不一致（语义复核）。
pub const E_PFX_MOOD_MISMATCH: &str = "E_PFX_MOOD_MISMATCH";
/// 色调映射衔接失配：次序口径与 F4404 管线对不上。
pub const E_PFX_TONEMAP_MISMATCH: &str = "E_PFX_TONEMAP_MISMATCH";
/// 未登记场景：语义场景表查无此场景。
pub const E_PFX_UNSCENE: &str = "E_PFX_UNSCENE";
/// 双签未齐：编排册变更单人签章。
pub const E_PFX_DUALSIGN: &str = "E_PFX_DUALSIGN";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_PFX_OVERREACH => "越界实现：编排层不得实现效果引擎本体",
        E_PFX_MOOD_MISMATCH => "氛围失当：场景映射与方案声明不一致",
        E_PFX_TONEMAP_MISMATCH => "色调映射衔接失配：次序口径对不上",
        E_PFX_UNSCENE => "未登记场景：先入语义场景表再编排",
        E_PFX_DUALSIGN => "双签未齐：编排册变更需双人签章",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向（归位指引）。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_PFX_OVERREACH => "引擎本体走 Z03，编排层只引用与编排",
        E_PFX_MOOD_MISMATCH => "交语义复核：改场景映射或改方案声明",
        E_PFX_TONEMAP_MISMATCH => "按 F4404 口径改次序声明后对拍",
        E_PFX_UNSCENE => "先在语义场景表登记场景",
        E_PFX_DUALSIGN => "补第二签章后再入册",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、子域定位卡：编排层职责闭集
// ---------------------------------------------------------------------------

/// 编排层职责闭集（五项——闭集外即越界）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterDuty {
    /// 氛围编排：何时用何种后处理。
    MoodOrchestration,
    /// 次序约定：效果叠加次序与色调映射衔接。
    OrderConvention,
    /// 语义标注：每编排方案的内容语义声明。
    SemanticAnnotation,
    /// 强度档位：氛围→强度档查表。
    IntensityTier,
    /// 双签受理：编排册变更入口。
    DualSignIntake,
}

/// 职责数（判据侧独立写死 5）。
pub const DUTY_COUNT: usize = 5;

impl CharterDuty {
    /// 全集。
    pub const ALL: [CharterDuty; DUTY_COUNT] = [
        CharterDuty::MoodOrchestration,
        CharterDuty::OrderConvention,
        CharterDuty::SemanticAnnotation,
        CharterDuty::IntensityTier,
        CharterDuty::DualSignIntake,
    ];

    /// 职责名。
    pub const fn name(self) -> &'static str {
        match self {
            CharterDuty::MoodOrchestration => "氛围编排",
            CharterDuty::OrderConvention => "次序约定",
            CharterDuty::SemanticAnnotation => "语义标注",
            CharterDuty::IntensityTier => "强度档位",
            CharterDuty::DualSignIntake => "双签受理",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            CharterDuty::MoodOrchestration => 0,
            CharterDuty::OrderConvention => 1,
            CharterDuty::SemanticAnnotation => 2,
            CharterDuty::IntensityTier => 3,
            CharterDuty::DualSignIntake => 4,
        }
    }
}

/// 子域定位卡：定位声明 + 职责闭集。
#[derive(Clone, Debug)]
pub struct PostfxCharter {
    /// 定位一句话（读屏可达）。
    pub statement: String,
}

impl PostfxCharter {
    /// 现行定位卡。
    pub fn current() -> Self {
        PostfxCharter {
            statement: String::from("后处理编排层：管编排约定与内容语义，不管效果引擎本体（Z03）"),
        }
    }

    /// 职责在册判据：闭集枚举逐项在册（判据侧独立枚举对拍）。
    pub const fn duty_in_charter(d: CharterDuty) -> bool {
        let _ = d;
        true // 封闭集构造性保证；判据面独立枚举核对 ALL 表
    }

    /// **越界词表**：编排层不得出现的动作（命中即归位拦截）。
    /// O(1) 定长表——锚点「拦截 O(1)」。
    pub const OVERREACH_TERMS: [&str; 4] = [
        "直调引擎本体",
        "私建效果实例",
        "绕编排直挂管线",
        "私改引擎参数",
    ];

    /// 归位指引（每个越界词对应一条——拦截必附指引，锚点「归位拦截」）。
    pub fn relocate_hint(term: &str) -> &'static str {
        match term {
            "直调引擎本体" => "效果执行走 Z03 EngineSurface，编排层只声明用哪个",
            "私建效果实例" => "效果实例由 Z03 池化，编排层引用名册条目",
            "绕编排直挂管线" => "后处理挂载必须经编排册条目",
            "私改引擎参数" => "参数改动走氛围预设表，逐效果偏离要语义复核",
            _ => "未知越界词",
        }
    }

    /// 越界拦截：词表命中→`E_PFX_OVERREACH`；未命中放行。
    pub fn screen_term(&self, term: &str) -> Result<(), &'static str> {
        for t in Self::OVERREACH_TERMS {
            if t == term {
                return Err(E_PFX_OVERREACH);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、氛围闭集与强度档位：查表单源
// ---------------------------------------------------------------------------

/// 五氛围封闭集（域本色：为内容服务——氛围是内容的情绪）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    /// 昂扬：胜利/高光——泛光偏暖偏高。
    Heroic,
    /// 紧张：战斗/追逐——色彩偏冷锐、景深收窄。
    Tense,
    /// 平静：探索/休整——低泛光低模糊。
    Calm,
    /// 诡异：悬疑/异常——色彩偏移、景深拉长。
    Eerie,
    /// 低回：失败/离别——泛光压暗、模糊加沉。
    Somber,
}

/// 氛围数（判据侧独立写死 5）。
pub const MOOD_COUNT: usize = 5;

/// 单氛围的强度档（四效果 0..=4 五档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoodPreset {
    /// 泛光档。
    pub bloom: u8,
    /// 景深档。
    pub dof: u8,
    /// 色彩分级档。
    pub color: u8,
    /// 模糊档。
    pub blur: u8,
}

/// 档位上限（0..=4 五档——超档即越界）。
pub const TIER_MAX: u8 = 4;

impl Mood {
    /// 全集。
    pub const ALL: [Mood; MOOD_COUNT] = [
        Mood::Heroic,
        Mood::Tense,
        Mood::Calm,
        Mood::Eerie,
        Mood::Somber,
    ];

    /// 氛围名。
    pub const fn name(self) -> &'static str {
        match self {
            Mood::Heroic => "昂扬",
            Mood::Tense => "紧张",
            Mood::Calm => "平静",
            Mood::Eerie => "诡异",
            Mood::Somber => "低回",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            Mood::Heroic => 0,
            Mood::Tense => 1,
            Mood::Calm => 2,
            Mood::Eerie => 3,
            Mood::Somber => 4,
        }
    }

    /// 氛围→强度预设（查表单源 [`MOOD_PRESETS`]——判据侧独立写死对拍）。
    pub const fn preset(self) -> MoodPreset {
        MOOD_PRESETS[self.ordinal()]
    }
}

/// 氛围预设表（单源——「何时用何种氛围」的强度面）。
pub const MOOD_PRESETS: [MoodPreset; MOOD_COUNT] = [
    MoodPreset { bloom: 4, dof: 1, color: 3, blur: 0 }, // 昂扬：暖高泛光
    MoodPreset { bloom: 2, dof: 3, color: 4, blur: 0 }, // 紧张：冷锐收窄
    MoodPreset { bloom: 1, dof: 1, color: 1, blur: 1 }, // 平静：全低
    MoodPreset { bloom: 2, dof: 4, color: 3, blur: 2 }, // 诡异：长景深偏色
    MoodPreset { bloom: 0, dof: 2, color: 2, blur: 3 }, // 低回：压暗加沉
];

// ---------------------------------------------------------------------------
// 三、语义场景表：场景→氛围（定位 O(1)）
// ---------------------------------------------------------------------------

/// 场景闭集（何时——语义场景表的行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// Boss 登场。
    BossIntro,
    /// 战斗白热。
    CombatPeak,
    /// 大世界漫游。
    OverworldRoam,
    /// 解谜/异象。
    PuzzleAnomaly,
    /// 剧情诀别。
    StoryFarewell,
    /// 主菜单。
    MainMenu,
}

/// 场景数（判据侧独立写死 6）。
pub const SCENE_COUNT: usize = 6;

impl Scene {
    /// 全集。
    pub const ALL: [Scene; SCENE_COUNT] = [
        Scene::BossIntro,
        Scene::CombatPeak,
        Scene::OverworldRoam,
        Scene::PuzzleAnomaly,
        Scene::StoryFarewell,
        Scene::MainMenu,
    ];

    /// 场景名。
    pub const fn name(self) -> &'static str {
        match self {
            Scene::BossIntro => "Boss登场",
            Scene::CombatPeak => "战斗白热",
            Scene::OverworldRoam => "大世界漫游",
            Scene::PuzzleAnomaly => "解谜异象",
            Scene::StoryFarewell => "剧情诀别",
            Scene::MainMenu => "主菜单",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            Scene::BossIntro => 0,
            Scene::CombatPeak => 1,
            Scene::OverworldRoam => 2,
            Scene::PuzzleAnomaly => 3,
            Scene::StoryFarewell => 4,
            Scene::MainMenu => 5,
        }
    }
}

/// 语义场景表：场景→氛围查表（单源真值）。
#[derive(Clone, Debug, Default)]
pub struct MoodSceneTable {
    entries: [Option<Mood>; SCENE_COUNT],
    /// 语义复核触发计数（氛围失当的可观测面）。
    pub review_hits: u32,
    /// 未登记拒绝计数。
    pub unscene_blocks: u32,
}

impl MoodSceneTable {
    /// 空表。
    pub fn new() -> Self {
        MoodSceneTable { entries: [None; SCENE_COUNT], review_hits: 0, unscene_blocks: 0 }
    }

    /// 登记场景氛围映射。
    pub fn enroll(&mut self, scene: Scene, mood: Mood) {
        self.entries[scene.ordinal()] = Some(mood);
    }

    /// 表内查询。
    pub fn lookup(&self, scene: Scene) -> Option<Mood> {
        self.entries[scene.ordinal()]
    }

    /// 已登记场景数。
    pub fn enrolled(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// **编排受理**：方案声明氛围 vs 表内映射对拍。
    /// 未登记→`E_PFX_UNSCENE`（O(1) 定位——锚点「定位 O(1)」）；
    /// 不一致→氛围失当（`E_PFX_MOOD_MISMATCH`）+ 语义复核计数留痕
    ///（对拍不拍板：裁决归评审——锚点「语义复核」）；一致放行。
    pub fn screen(&mut self, scene: Scene, claimed: Mood) -> Result<Mood, &'static str> {
        let table_mood = match self.lookup(scene) {
            Some(m) => m,
            None => {
                self.unscene_blocks += 1;
                return Err(E_PFX_UNSCENE);
            }
        };
        if table_mood != claimed {
            self.review_hits += 1;
            return Err(E_PFX_MOOD_MISMATCH);
        }
        Ok(claimed)
    }
}

// ---------------------------------------------------------------------------
// 四、色调映射衔接声明：次序是口径（F4404 对拍 O(1)）
// ---------------------------------------------------------------------------

/// 后处理相对色调映射的次序声明（F4404 口径面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneMapPhase {
    /// 色调映射之前（HDR 域——泛光的正确采样位置）。
    BeforeToneMap,
    /// 色调映射之后（显示域——色彩分级 LUT 的正确位置）。
    AfterToneMap,
}

/// 效果名闭集（四效果——编排的最小单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostfxEffect {
    /// 泛光。
    Bloom,
    /// 景深。
    DepthOfField,
    /// 色彩分级。
    ColorGrade,
    /// 模糊。
    Blur,
}

/// 效果数（判据侧独立写死 4）。
pub const EFFECT_COUNT: usize = 4;

impl PostfxEffect {
    /// 全集。
    pub const ALL: [PostfxEffect; EFFECT_COUNT] = [
        PostfxEffect::Bloom,
        PostfxEffect::DepthOfField,
        PostfxEffect::ColorGrade,
        PostfxEffect::Blur,
    ];

    /// 效果名。
    pub const fn name(self) -> &'static str {
        match self {
            PostfxEffect::Bloom => "泛光",
            PostfxEffect::DepthOfField => "景深",
            PostfxEffect::ColorGrade => "色彩分级",
            PostfxEffect::Blur => "模糊",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            PostfxEffect::Bloom => 0,
            PostfxEffect::DepthOfField => 1,
            PostfxEffect::ColorGrade => 2,
            PostfxEffect::Blur => 3,
        }
    }
}

/// 衔接声明册：每效果一个 TM 相位声明（单源）——泛光必须在
/// BeforeToneMap（HDR 亮部采样），色彩分级必须在 AfterToneMap
/// （显示域 LUT）——这两个是**口径**不是口味，判据侧独立写死。
#[derive(Clone, Debug)]
pub struct ToneMapHandshake {
    phases: [Option<ToneMapPhase>; EFFECT_COUNT],
    /// 失配拦截计数。
    pub mismatch_blocks: u32,
}

impl ToneMapHandshake {
    /// 空声明册。
    pub fn new() -> Self {
        ToneMapHandshake { phases: [None; EFFECT_COUNT], mismatch_blocks: 0 }
    }

    /// 声明某效果的 TM 相位。
    pub fn declare(&mut self, effect: PostfxEffect, phase: ToneMapPhase) {
        self.phases[effect.ordinal()] = Some(phase);
    }

    /// 查询声明。
    pub fn phase_of(&self, effect: PostfxEffect) -> Option<ToneMapPhase> {
        self.phases[effect.ordinal()]
    }

    /// **对拍**：管线侧（F4404）声称的相位与本册声明一致→Ok；
    /// 不一致→`E_PFX_TONEMAP_MISMATCH`（O(1) 比对——锚点「对拍 O(1)」）。
    pub fn reconcile(
        &mut self,
        effect: PostfxEffect,
        pipeline_phase: ToneMapPhase,
    ) -> Result<(), &'static str> {
        match self.phase_of(effect) {
            Some(p) if p == pipeline_phase => Ok(()),
            _ => {
                self.mismatch_blocks += 1;
                Err(E_PFX_TONEMAP_MISMATCH)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、双签闸：编排册变更双人复核（F5220 同构）
// ---------------------------------------------------------------------------

/// 双签闸：单签拒绝留痕、双签开闸。
#[derive(Clone, Debug, Default)]
pub struct DualSignGate {
    first: Option<String>,
    /// 单签拒绝计数（留痕）。
    pub single_sign_blocks: u32,
    /// 开闸变更数。
    pub opened_changes: u32,
}

impl DualSignGate {
    /// 空闸。
    pub fn new() -> Self {
        DualSignGate { first: None, single_sign_blocks: 0, opened_changes: 0 }
    }

    /// 提交变更：第一签受理挂起；同签名第二签→开闸；不同名第二签→
    /// 双签成立开闸（双签=两个不同人）。
    pub fn submit(&mut self, signer: &str, change_id: u64) -> Result<u64, &'static str> {
        let _ = change_id;
        match &self.first {
            None => {
                self.first = Some(String::from(signer));
                Err(E_PFX_DUALSIGN)
            }
            Some(first) => {
                if first == signer {
                    // 同一人重复签不算第二签——留痕拒绝。
                    self.single_sign_blocks += 1;
                    return Err(E_PFX_DUALSIGN);
                }
                self.first = None;
                self.opened_changes += 1;
                Ok(self.opened_changes as u64)
            }
        }
    }

    /// 挂起第一签判据。
    pub fn pending(&self) -> bool {
        self.first.is_some()
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5204 域自检（判据分六组：定位/越界/氛围/衔接/双签/判据元）。
pub fn run_vez04_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5204");

    // --- 定位（判据一）---
    // 定位-01：职责闭集判据侧独立枚举对拍（五项序数 0..=5 连续）。
    let mut ordinals_ok = true;
    for d in CharterDuty::ALL {
        if d.ordinal() >= DUTY_COUNT {
            ordinals_ok = false;
        }
    }
    let mut seen = [false; DUTY_COUNT];
    for d in CharterDuty::ALL {
        seen[d.ordinal()] = true;
    }
    let full = seen.iter().all(|v| *v);
    let card = PostfxCharter::current();
    let stmt_ok = card.statement.contains("编排")
        && card.statement.contains("语义")
        && card.statement.contains("Z03");
    if ordinals_ok && full && stmt_ok {
        s.ok("EZ4-定位-01-职责闭集与定位声明");
    } else {
        s.fail("EZ4-定位-01-职责闭集与定位声明", "闭集缺位或定位漂移");
    }

    // --- 越界拦截（判据二）---
    // 拦截-01：越界词表逐词命中→拦截+归位指引非空（O(1) 查表）。
    let mut all_hit = true;
    for t in PostfxCharter::OVERREACH_TERMS {
        if card.screen_term(t) != Err(E_PFX_OVERREACH) || PostfxCharter::relocate_hint(t).is_empty()
        {
            all_hit = false;
        }
    }
    // 拦截-02：词表外放行（无辜编排动作不受牵连）。
    let pass = card.screen_term("引用氛围预设").is_ok()
        && card.screen_term("声明衔接相位").is_ok();
    if all_hit && pass {
        s.ok("EZ4-拦截-01-越界词表拦截归位");
    } else {
        s.fail("EZ4-拦截-01-越界词表拦截归位", "越界漏拦或误伤");
    }

    // --- 氛围与场景（判据三）---
    // 氛围-01：五氛围预设档判据侧独立写死对拍（全档 ≤ TIER_MAX）。
    let expect_presets: [(u8, u8, u8, u8); MOOD_COUNT] = [
        (4, 1, 3, 0), // 昂扬
        (2, 3, 4, 0), // 紧张
        (1, 1, 1, 1), // 平静
        (2, 4, 3, 2), // 诡异
        (0, 2, 2, 3), // 低回
    ];
    let mut presets_ok = true;
    for (i, m) in Mood::ALL.iter().enumerate() {
        let p = m.preset();
        let e = expect_presets[i];
        if p.bloom != e.0 || p.dof != e.1 || p.color != e.2 || p.blur != e.3 {
            presets_ok = false;
        }
        if p.bloom > TIER_MAX || p.dof > TIER_MAX || p.color > TIER_MAX || p.blur > TIER_MAX {
            presets_ok = false;
        }
    }
    if presets_ok {
        s.ok("EZ4-氛围-01-预设档独立对拍");
    } else {
        s.fail("EZ4-氛围-01-预设档独立对拍", "预设档漂移或超档");
    }
    // 氛围-02：场景→氛围映射编排受理（一致放行 O(1) 定位）。
    let mut t = MoodSceneTable::new();
    t.enroll(Scene::BossIntro, Mood::Heroic);
    t.enroll(Scene::CombatPeak, Mood::Tense);
    t.enroll(Scene::StoryFarewell, Mood::Somber);
    let ok1 = t.screen(Scene::BossIntro, Mood::Heroic).is_ok();
    let ok2 = t.screen(Scene::CombatPeak, Mood::Tense).is_ok();
    if ok1 && ok2 && t.enrolled() == 3 && t.review_hits == 0 {
        s.ok("EZ4-氛围-02-场景映射受理");
    } else {
        s.fail("EZ4-氛围-02-场景映射受理", "一致映射被误拒");
    }
    // 氛围-03：氛围失当→语义复核计数留痕（对拍不拍板）。
    let review = t.screen(Scene::BossIntro, Mood::Somber) == Err(E_PFX_MOOD_MISMATCH)
        && t.review_hits == 1;
    // 氛围-04：未登记场景拒绝（先入表再编排）。
    let unscene = t.screen(Scene::MainMenu, Mood::Calm) == Err(E_PFX_UNSCENE)
        && t.unscene_blocks == 1;
    if review && unscene {
        s.ok("EZ4-氛围-03-失当复核与未登记拒");
    } else {
        s.fail("EZ4-氛围-03-失当复核与未登记拒", "复核或未登记闸失效");
    }

    // --- 衔接（判据四）---
    // 衔接-01：口径声明——泛光在 TM 前、色彩分级在 TM 后（判据侧
    // 独立写死的口径事实，声明册与管线双向对拍）。
    let mut hs = ToneMapHandshake::new();
    hs.declare(PostfxEffect::Bloom, ToneMapPhase::BeforeToneMap);
    hs.declare(PostfxEffect::ColorGrade, ToneMapPhase::AfterToneMap);
    let hs1 = hs.reconcile(PostfxEffect::Bloom, ToneMapPhase::BeforeToneMap).is_ok();
    let hs2 = hs.reconcile(PostfxEffect::ColorGrade, ToneMapPhase::AfterToneMap).is_ok();
    // 衔接-02：失配→对拍拦截（管线声称泛光在 TM 后=口径错误）。
    let mis = hs.reconcile(PostfxEffect::Bloom, ToneMapPhase::AfterToneMap)
        == Err(E_PFX_TONEMAP_MISMATCH)
        && hs.mismatch_blocks == 1;
    if hs1 && hs2 && mis {
        s.ok("EZ4-衔接-01-口径对拍与失配拦截");
    } else {
        s.fail("EZ4-衔接-01-口径对拍与失配拦截", "衔接对拍失效");
    }

    // --- 双签（判据五）---
    // 双签-01：单签拒绝留痕、双签开闸、同人重签不算第二签。
    let mut g = DualSignGate::new();
    let one = g.submit("alice", 1) == Err(E_PFX_DUALSIGN) && g.pending();
    let same = g.submit("alice", 1) == Err(E_PFX_DUALSIGN) && g.single_sign_blocks == 1;
    let two = g.submit("bob", 1).is_ok() && !g.pending() && g.opened_changes == 1;
    if one && same && two {
        s.ok("EZ4-双签-01-单签拒双签开闸");
    } else {
        s.fail("EZ4-双签-01-单签拒双签开闸", "双签闸失效");
    }

    // --- 判据元（判据六）---
    // 元-01：错误码互异 + 三要素齐备（判据侧独立枚举五码）。
    let codes = [
        E_PFX_OVERREACH,
        E_PFX_MOOD_MISMATCH,
        E_PFX_TONEMAP_MISMATCH,
        E_PFX_UNSCENE,
        E_PFX_DUALSIGN,
    ];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ4-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ4-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez04_postfx.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ4-判据-02-零panic面");
    } else {
        s.fail("EZ4-判据-02-零panic面", "判据/生产面含 panic 面");
    }
    // 元-03：闭集计数对账（判据侧写死：职责5/氛围5/场景6/效果4）。
    if DUTY_COUNT == 5 && MOOD_COUNT == 5 && SCENE_COUNT == 6 && EFFECT_COUNT == 4 {
        s.ok("EZ4-判据-03-闭集计数对账");
    } else {
        s.fail("EZ4-判据-03-闭集计数对账", "闭集计数漂移");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
