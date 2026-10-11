//! VE-F5208 · 特效组合与叠加（VE-Z 域 · 组合面 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5208`
//!
//! **判据（锚点原文）**：叠加消解、场景组合包、组合预算、降档建议、判据。
//!
//! **职责定位（锚点原文）**：特效组合与叠加——组合三件：特效叠加模型
//! （多特效共存叠加规则与冲突消解——同屏密度节制律执行）/组合预设
//! （成套特效组合（战斗/氛围/演出场景包））/组合性能预算（组合总开销
//! 预算——组合不超支）。
//!
//! **数据结构（锚点原文）**：叠加规则模型；组合预设包；组合预算器。
//!
//! **错误路径与降级矩阵（锚点原文）**：叠加冲突→消解规则裁决；组合
//! 超支→自动降档建议；私组绕预算→拦截。
//!
//! **性能逐项分解（锚点原文）**：叠加 O(特效数)；降档 O(1)；拦截 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4330 降级口径参照上游；F5216 性能消费；
//! F5213 测试。F5207 参数系统被本组合消费（组合预设的微调面）。
//!
//! **无障碍与隐私（锚点原文）**：节制律执行面（域本色）；组合报告读屏
//! 可达。
//!
//! # 深化说明：组合不是堆叠——每一层共存都有规则裁决，每一微秒开销
//! ## 都有预算看管，每一次超支都有让位名单
//!
//! 「多特效共存」如果只靠自觉不冲突，同屏第十七个粒子发射器就会把帧
//! 时间吃穿——节制律必须落成**机械闸**：同屏密度上限 16（F5202 节制律
//! 16/17 恰边界同口径），第 17 枚拒入 [`E_CB_DENSITY`]，告警是 O(1) 计
//! 数器（锚点「拦截 O(1)」）；「组合不超支」如果只做拦截不做建议，超支
//! 就变成死局——预算器超支时按超支比例查**档位表**（锚点「降档 O(1)」
//! ——比例对拍 O(1)）出**让位名单**：低权重先让（内容权重是作者声明
//! 的价值序，预算紧张时先牺牲轻的），名单可执行可验证（应用后不超支
//! ——建议不是安慰剂）。
//!
//! ## 深化一：叠加方式是查表规则不是临场商量——四类四方式闭集单源
//!
//! Z 域四子系统（F5201 责任互斥口径）各有**默认叠加方式**：粒子
//! Additive（不同源可共存——烟与火花同屏是常态）、后处理 Replace
//! （同屏一档——两套色调映射没有意义）、环境 Replace（同屏一相）、
//! 转场 Exclusive（互斥——一天一相一过渡）。规则表单源（
//! [`OverlayRules`]），查表 O(1)（四项线性——类闭集定长）；未登记类
//! 查无规则 → 冲突不可消解 → 显性拒绝 [`E_CB_CONFLICT`]——没有规则
//! 的共存就是没有裁决的冲突。
//!
//! ## 深化二：冲突消解是三态裁决不是布尔——加入/顶替/互斥拒绝各是一态
//!
//! 同类两卡相遇按叠加方式裁决（[`StackResolver::push`]）：Additive 直
//! 接入屏；Replace 后到优先（新档替换旧档——旧名从屏上消失，裁决可
//! 问询）；Exclusive 先到优先（后到被拒，裁决 [`Verdict::RejectedExclusive`]
//! 是**正常裁决不是错误**——互斥类的第二枚本来就不该上屏）。裁决三态
//! 全可观测，消解不静默。
//!
//! ## 深化三：预算器是唯一入屏闸——私组绕预算拦截在票面上
//!
//! 特效入屏必过预算闸（[`ComboBudget::admit`]）：**无票请求**（绕过编
//! 排直接挂屏）→ [`E_CB_OVERREACH`] 拦截计数——两翼是唯一形参来源的
//! 同纪律，预算器是唯一开销账房；**超支请求** → [`E_CB_OVER_BUDGET`]
//! 拒入**不记账**（账实守恒——失败的准入不留幽灵开销）；组合编排层
//! （[`ComboStage::mount`]）预算闸过后叠加闸拒入（互斥/密度）→ **回滚
//! 退账**（错误路径「降级与回滚处置」——账上 spent 只含真在屏成员）。
//!
//! ## 深化四：降档是档位表不是猜——比例查表 O(1)，名单按权重升序
//!
//! 超支比例定档（[`tier_of`]，全整数无除法浮点）：≤9/8 轻剪（让位
//! ceil(n/4)）、≤5/4 半剪（让位 ceil(n/2)）、更深深剪（全撤——超支过
//! 深保帧率优先）。让位名单取**权重升序头部**（成员入屏即按权重升序
//! 归位——插入 O(n) 累计叠加 O(特效数)，锚点「叠加 O(特效数)」口径；
//! 降档判定 O(1)+取名单 O(k)）。预算收缩复检（[`ComboBudget::replan`]）
//! 复用同一档位表——cap 调小与整包试装走同一把尺。
//!
//! ## 深化五：场景组合包开箱即用且自洽——三包总开销全在子预算内
//!
//! 战斗/氛围/演出三包（[`PACK_KINDS`] 闭集）各带**开箱预设**（
//! [`ComboPack::battle_default`] 等——成套组合的「成套」面）：每包成员
//! 总开销 ≤ [`COMBO_BUDGET_US`]（包设计自洽——包自己能装下自己，判据
//! 侧对拍）。每成员可挂 F5207 参数微调（[`ComboPack::attach_tweak`]）
//! ——微调 resolve 后交校验器钳制（**微调不豁免钳制**——越界的微调照
//! 样钳+提示，F5207 消费接缝）。
//!
//! ## 深化六：组合报告读屏可达——每行一个语义句，行数不超预算
//!
//! [`combo_report`] 固定 4 行（≤6 行读屏预算，F5202 清单口径同款）：
//! 在屏数/开销对预算/状态句（预算内或超支+档位+让位名额）/密度句——
//! 读屏玩家听到的是「超支 30 微秒，建议半剪：让位 2 名」不是一堆裸数
//! 字（组合报告读屏可达——域本色）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vez07_params::{PresetPack, Validator};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_CB_* 前缀）
// ---------------------------------------------------------------------------

/// 叠加规则缺失：未登记特效类，冲突不可消解。
pub const E_CB_CONFLICT: &str = "E_CB_CONFLICT";
/// 同屏密度超节制上限（16——F5202 节制律口径）。
pub const E_CB_DENSITY: &str = "E_CB_DENSITY";
/// 组合超支：准入被拒且不记账。
pub const E_CB_OVER_BUDGET: &str = "E_CB_OVER_BUDGET";
/// 组合包非法：场景类未登记/成员重名/成员超限。
pub const E_CB_BAD_PACK: &str = "E_CB_BAD_PACK";
/// 私组绕预算：无票请求入屏，拦截。
pub const E_CB_OVERREACH: &str = "E_CB_OVERREACH";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_CB_CONFLICT => "叠加规则缺失：未登记特效类，冲突不可消解",
        E_CB_DENSITY => "同屏密度超节制上限：第 17 枚拒入",
        E_CB_OVER_BUDGET => "组合超支：准入被拒且不记账",
        E_CB_BAD_PACK => "组合包非法：类未登记/成员重名/成员超限",
        E_CB_OVERREACH => "私组绕预算：无票请求入屏被拦截",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_CB_CONFLICT => "先登记特效类叠加规则再组合",
        E_CB_DENSITY => "撤销低权重在屏特效或分帧呈现",
        E_CB_OVER_BUDGET => "按降档建议让位或缩减组合规模",
        E_CB_BAD_PACK => "核对场景包类型与成员唯一性",
        E_CB_OVERREACH => "走组合编排入口取预算凭据",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、叠加规则模型：四类四方式闭集单源
// ---------------------------------------------------------------------------

/// 叠加方式闭集：同类两特效相遇时的共存规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    /// 可叠加：多枚共存（不同源粒子同屏是常态）。
    Additive,
    /// 替换：后到顶先到（同屏一档——两套色调映射没有意义）。
    Replace,
    /// 互斥：同类只容一，后到被拒（先到优先——一天一相一过渡）。
    Exclusive,
}

/// Z 域四子系统类闭集（F5201 责任互斥同口径）。
pub const FX_CLASSES: [&str; 4] = ["particles", "postfx", "envfx", "transition"];

/// 同屏密度上限（F5202 节制律 16/17 恰边界口径——第 17 枚拒入）。
pub const DENSITY_CAP: usize = 16;

/// 单特效叠加卡：入组合的资格证（名/类/方式/权重/开销五元组）。
#[derive(Clone, Debug)]
pub struct OverlayCard {
    /// 特效名（组合内唯一键）。
    pub name: String,
    /// 所属类（必属 [`FX_CLASSES`] 闭集）。
    pub class: String,
    /// 叠加方式（与类同源的规则面——规则表是单源，卡上的方式是快照）。
    pub blend: BlendMode,
    /// 内容权重（作者声明的价值序——降档时低权重先让位）。
    pub weight: u32,
    /// 开销估值（微秒——预算账的唯一货币）。
    pub cost_us: u32,
}

/// 叠加规则模型：类→默认方式单源表 + 同屏密度上限。
#[derive(Clone, Debug)]
pub struct OverlayRules {
    table: Vec<(&'static str, BlendMode)>,
    /// 同屏密度上限（节制律执行面）。
    pub density_cap: usize,
}

impl OverlayRules {
    /// 默认规则表：四类四方式（Z 域四子系统同源口径）。
    pub fn new() -> Self {
        OverlayRules {
            table: vec![
                ("particles", BlendMode::Additive),
                ("postfx", BlendMode::Replace),
                ("envfx", BlendMode::Replace),
                ("transition", BlendMode::Exclusive),
            ],
            density_cap: DENSITY_CAP,
        }
    }

    /// 查叠加方式：O(1)（类闭集定长四项线性）。查无 → None（冲突不可消解）。
    pub fn blend_of(&self, class: &str) -> Option<BlendMode> {
        self.table.iter().find(|(c, _)| *c == class).map(|(_, b)| *b)
    }

    /// 规则条数（判据侧对拍面）。
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// 空表判据。
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、叠加冲突消解器：三态裁决（加入/顶替/互斥拒绝）
// ---------------------------------------------------------------------------

/// 裁决三态：全可观测，消解不静默。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 直接入屏。
    Added,
    /// 后到顶先到（携带被顶替的旧名——裁决可问询）。
    Replaced(String),
    /// 互斥类后到被拒（正常裁决不是错误）。
    RejectedExclusive,
}

/// 叠加冲突消解器：规则表 + 当前同屏成员账。
#[derive(Clone, Debug)]
pub struct StackResolver {
    rules: OverlayRules,
    live: Vec<OverlayCard>,
}

impl StackResolver {
    /// 以规则表建消解器。
    pub fn new(rules: OverlayRules) -> Self {
        StackResolver { rules, live: Vec::new() }
    }

    /// **入屏裁决**：未登记类→`E_CB_CONFLICT`（冲突不可消解）；密度
    /// 满→`E_CB_DENSITY`（第 17 枚拒入）；否则按叠加方式三态裁决。
    pub fn push(&mut self, card: OverlayCard) -> Result<Verdict, &'static str> {
        let blend = match self.rules.blend_of(&card.class) {
            Some(b) => b,
            None => return Err(E_CB_CONFLICT),
        };
        if self.live.len() >= self.rules.density_cap {
            return Err(E_CB_DENSITY);
        }
        match blend {
            BlendMode::Additive => {
                self.live.push(card);
                Ok(Verdict::Added)
            }
            BlendMode::Exclusive => {
                if self.live.iter().any(|c| c.class == card.class) {
                    Ok(Verdict::RejectedExclusive)
                } else {
                    self.live.push(card);
                    Ok(Verdict::Added)
                }
            }
            BlendMode::Replace => {
                let mut replaced: Option<String> = None;
                self.live.retain(|c| {
                    if c.class == card.class {
                        replaced = Some(c.name.clone());
                        false
                    } else {
                        true
                    }
                });
                self.live.push(card);
                match replaced {
                    Some(old) => Ok(Verdict::Replaced(old)),
                    None => Ok(Verdict::Added),
                }
            }
        }
    }

    /// 在屏成员数。
    pub fn live_len(&self) -> usize {
        self.live.len()
    }

    /// 在屏成员名册快照。
    pub fn live_names(&self) -> Vec<String> {
        self.live.iter().map(|c| c.name.clone()).collect()
    }

    /// 某名是否在屏。
    pub fn is_live(&self, name: &str) -> bool {
        self.live.iter().any(|c| c.name == name)
    }

    /// 密度上限（报告面）。
    pub fn density_cap(&self) -> usize {
        self.rules.density_cap
    }
}

// ---------------------------------------------------------------------------
// 三、组合预算器：唯一入屏闸（票据/超支/回滚/降档四面）
// ---------------------------------------------------------------------------

/// 组合特效子预算（微秒）。F5801 全核总预算 400us；组合特效子预算
/// 120us（F4330 降级口径参照——子预算含于总预算，组合不越权吃满全帧）。
pub const COMBO_BUDGET_US: u32 = 120;

/// 组合包成员上限（包完整性边界——成套组合不是无底清单）。
pub const MAX_PACK_MEMBERS: usize = 8;

/// 降档档位（超支比例查表产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 未超支：不降。
    None,
    /// 轻剪：让位 ceil(n/4)。
    Light,
    /// 半剪：让位 ceil(n/2)。
    Half,
    /// 深剪：全撤（超支过深保帧率优先）。
    Deep,
}

impl Tier {
    /// 档位标签（读屏报告用——中文单字面语义句）。
    pub fn label(&self) -> &'static str {
        match self {
            Tier::None => "不降",
            Tier::Light => "轻剪",
            Tier::Half => "半剪",
            Tier::Deep => "深剪",
        }
    }

    /// 档位序（0..=3，判据侧对拍面）。
    pub fn rank(&self) -> u8 {
        match self {
            Tier::None => 0,
            Tier::Light => 1,
            Tier::Half => 2,
            Tier::Deep => 3,
        }
    }
}

/// **档位判定**：全整数无除法浮点——spent ≤ cap 不降；≤9/8 轻剪；
/// ≤5/4 半剪；更深深剪。O(1)（两次乘法比较——锚点「降档 O(1)」）。
pub fn tier_of(spent: u32, cap: u32) -> Tier {
    if spent <= cap {
        return Tier::None;
    }
    if spent * 8 <= cap * 9 {
        Tier::Light
    } else if spent * 4 <= cap * 5 {
        Tier::Half
    } else {
        Tier::Deep
    }
}

/// 组合预算器：唯一开销账房 + 唯一入屏闸。
#[derive(Clone, Debug)]
pub struct ComboBudget {
    /// 预算上限（微秒）。
    pub cap_us: u32,
    members: Vec<OverlayCard>,
    spent_us: u32,
    /// 私组绕预算拦截计数（O(1) 告警器——锚点「拦截 O(1)」）。
    pub bypass_blocks: u32,
    /// 超支拒入计数。
    pub over_budget_blocks: u32,
}

impl ComboBudget {
    /// 以上限建账房。
    pub fn new(cap_us: u32) -> Self {
        ComboBudget {
            cap_us,
            members: Vec::new(),
            spent_us: 0,
            bypass_blocks: 0,
            over_budget_blocks: 0,
        }
    }

    /// **准入闸**：先验票（无票→`E_CB_OVERREACH`，拦截 O(1)——无票连
    /// 账都不该碰）；再算账（超支→`E_CB_OVER_BUDGET`，**不记账**——
    /// 账实守恒）；过闸成员按权重升序归位（插入 O(n) 累计叠加
    /// O(特效数)——降档让位名单直接取头部）。
    pub fn admit(&mut self, card: OverlayCard, has_ticket: bool) -> Result<(), &'static str> {
        if !has_ticket {
            self.bypass_blocks += 1;
            return Err(E_CB_OVERREACH);
        }
        if self.spent_us + card.cost_us > self.cap_us {
            self.over_budget_blocks += 1;
            return Err(E_CB_OVER_BUDGET);
        }
        self.spent_us += card.cost_us;
        let mut pos = self.members.len();
        for (i, m) in self.members.iter().enumerate() {
            if m.weight >= card.weight {
                pos = i;
                break;
            }
        }
        self.members.insert(pos, card);
        Ok(())
    }

    /// **退账回滚**：指定成员出屏退开销（返回释放微秒）。mount 叠加闸
    /// 拒入时调用——账上 spent 只含真在屏成员。
    pub fn refund(&mut self, name: &str) -> u32 {
        let mut freed = 0u32;
        self.members.retain(|c| {
            if c.name == name {
                freed += c.cost_us;
                false
            } else {
                true
            }
        });
        self.spent_us -= freed;
        freed
    }

    /// 已入账开销。
    pub fn spent(&self) -> u32 {
        self.spent_us
    }

    /// 在账成员数。
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// 在账成员名册（权重升序）。
    pub fn member_names(&self) -> Vec<String> {
        self.members.iter().map(|m| m.name.clone()).collect()
    }

    /// **降档建议**：档位判定 O(1)（比例查表），让位名单取权重升序
    /// 头部 O(k)——低权重先让（作者声明的价值序，预算紧张先牺牲轻的）。
    /// 返回（档位，让位名单）。
    pub fn downgrade_plan(&self) -> (Tier, Vec<String>) {
        let tier = tier_of(self.spent_us, self.cap_us);
        let n = self.members.len();
        let k = match tier {
            Tier::None => 0usize,
            Tier::Light => (n + 3) / 4,
            Tier::Half => (n + 1) / 2,
            Tier::Deep => n,
        };
        let plan = self.members.iter().take(k).map(|m| m.name.clone()).collect();
        (tier, plan)
    }

    /// **预算收缩复检**：cap 调小后已屏成员可能超支——出档位与让位
    /// 建议（不改账，建议面；与整包试装走同一把档位尺）。
    pub fn replan(&mut self, new_cap: u32) -> (Tier, Vec<String>) {
        self.cap_us = new_cap;
        self.downgrade_plan()
    }

    /// **执行让位**：名单成员逐个退账出屏（返回共释放微秒——建议可
    /// 执行可验证，不是安慰剂）。
    pub fn apply_downgrade(&mut self, names: &[String]) -> u32 {
        let mut freed = 0u32;
        for n in names {
            freed += self.refund(n);
        }
        freed
    }

    /// 清账复用（判据面：清册归零）。
    pub fn reset(&mut self) {
        self.members.clear();
        self.spent_us = 0;
    }
}

// ---------------------------------------------------------------------------
// 四、场景组合包：三类闭集 + 成员完整性 + F5207 微调消费
// ---------------------------------------------------------------------------

/// 场景组合包三类闭集（战斗/氛围/演出）。
pub const PACK_KINDS: [&str; 3] = ["battle", "ambience", "cinematic"];

/// 组合预设包：成套特效组合（成员清单 + 每成员参数微调账）。
#[derive(Clone, Debug)]
pub struct ComboPack {
    /// 包名。
    pub name: String,
    /// 场景类（必属 [`PACK_KINDS`] 闭集）。
    pub kind: String,
    /// 成员清单（名唯一 + 类属闭集 + 上限 8）。
    pub members: Vec<OverlayCard>,
    /// 微调账：成员名 → F5207 预设包（微调层，resolve 后交钳制）。
    pub tweaks: Vec<(String, PresetPack)>,
}

impl ComboPack {
    /// 建包：场景类未登记 → `E_CB_BAD_PACK`。
    pub fn new(name: &str, kind: &str) -> Result<Self, &'static str> {
        if !PACK_KINDS.contains(&kind) {
            return Err(E_CB_BAD_PACK);
        }
        Ok(ComboPack {
            name: String::from(name),
            kind: String::from(kind),
            members: Vec::new(),
            tweaks: Vec::new(),
        })
    }

    /// 加成员：类未登记 → `E_CB_CONFLICT`；重名/超限 → `E_CB_BAD_PACK`
    /// （包完整性三闸）。
    pub fn add_member(&mut self, card: OverlayCard) -> Result<(), &'static str> {
        if !FX_CLASSES.contains(&card.class.as_str()) {
            return Err(E_CB_CONFLICT);
        }
        if self.members.iter().any(|m| m.name == card.name) {
            return Err(E_CB_BAD_PACK);
        }
        if self.members.len() >= MAX_PACK_MEMBERS {
            return Err(E_CB_BAD_PACK);
        }
        self.members.push(card);
        Ok(())
    }

    /// 包成员开销合计（手算锚对拍面）。
    pub fn total_cost(&self) -> u32 {
        self.members.iter().map(|m| m.cost_us).sum()
    }

    /// 挂 F5207 参数微调：成员不存在 → `E_CB_BAD_PACK`（微调只落在
    /// 真成员上——幽灵微调不收）。
    pub fn attach_tweak(&mut self, member: &str, pack: PresetPack) -> Result<(), &'static str> {
        if !self.members.iter().any(|m| m.name == member) {
            return Err(E_CB_BAD_PACK);
        }
        match self.tweaks.iter_mut().find(|(n, _)| n == member) {
            Some((_, p)) => *p = pack,
            None => self.tweaks.push((String::from(member), pack)),
        }
        Ok(())
    }

    /// **微调值集**（F5207 消费面）：resolve 出名值对——交校验器钳制
    /// （微调不豁免钳制；钳制在消费方 [`Validator`] 完成——接缝清晰）。
    pub fn tweak_values(&self, member: &str) -> Option<Vec<(String, i64)>> {
        self.tweaks.iter().find(|(n, _)| n == member).map(|(_, p)| p.resolve())
    }

    /// 开箱预设·战斗包：打击火花+屏幕震动+命中顿帧（总开销 75us 自洽）。
    pub fn battle_default() -> Self {
        let mut bp = match ComboPack::new("战斗-默认", "battle") {
            Ok(p) => p,
            Err(_) => ComboPack {
                name: String::from("战斗-默认"),
                kind: String::from("battle"),
                members: Vec::new(),
                tweaks: Vec::new(),
            },
        };
        let _ = bp.add_member(card("hit_spark", "particles", BlendMode::Additive, 8, 30));
        let _ = bp.add_member(card("screen_shake", "postfx", BlendMode::Replace, 6, 25));
        let _ = bp.add_member(card("hit_pause", "transition", BlendMode::Exclusive, 4, 20));
        bp
    }

    /// 开箱预设·氛围包：落雨+雾光+尘埃（总开销 80us 自洽）。
    pub fn ambience_default() -> Self {
        let mut ap = match ComboPack::new("氛围-默认", "ambience") {
            Ok(p) => p,
            Err(_) => ComboPack {
                name: String::from("氛围-默认"),
                kind: String::from("ambience"),
                members: Vec::new(),
                tweaks: Vec::new(),
            },
        };
        let _ = ap.add_member(card("rain", "envfx", BlendMode::Replace, 7, 35));
        let _ = ap.add_member(card("fog_glow", "postfx", BlendMode::Replace, 5, 30));
        let _ = ap.add_member(card("dust", "particles", BlendMode::Additive, 3, 15));
        ap
    }

    /// 开箱预设·演出包：黑场过渡+景深（总开销 70us 自洽）。
    pub fn cinematic_default() -> Self {
        let mut cp = match ComboPack::new("演出-默认", "cinematic") {
            Ok(p) => p,
            Err(_) => ComboPack {
                name: String::from("演出-默认"),
                kind: String::from("cinematic"),
                members: Vec::new(),
                tweaks: Vec::new(),
            },
        };
        let _ = cp.add_member(card("fade_black", "transition", BlendMode::Exclusive, 7, 30));
        let _ = cp.add_member(card("dof", "postfx", BlendMode::Replace, 6, 40));
        cp
    }
}

/// 卡片构造辅助（判据与默认包共用）。
pub fn card(name: &str, class: &str, blend: BlendMode, weight: u32, cost_us: u32) -> OverlayCard {
    OverlayCard {
        name: String::from(name),
        class: String::from(class),
        blend,
        weight,
        cost_us,
    }
}

// ---------------------------------------------------------------------------
// 五、组合总编排：预算闸→叠加闸→入屏（含回滚处置）
// ---------------------------------------------------------------------------

/// 入屏结果三态（裁决不是错误——互斥拒绝也是合法结局）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountOutcome {
    /// 直接入屏。
    Mounted,
    /// 顶替了旧成员（携带旧名）。
    Replaced(String),
    /// 互斥类后到被拒（预算已回滚退账——账实一致）。
    RejectedExclusive,
}

/// 组合总编排：预算闸→叠加闸双闸串联，**叠加闸拒入必回滚退账**（错误
/// 路径「降级与回滚处置」——账上 spent 只含真在屏成员）。组合三件的
/// 编排入口：绕过编排直接挂屏=无票，被预算闸拦截。
#[derive(Clone, Debug)]
pub struct ComboStage {
    /// 预算闸（开销账房）。
    pub budget: ComboBudget,
    /// 叠加闸（冲突消解器）。
    pub stack: StackResolver,
}

impl ComboStage {
    /// 以预算上限+规则表建编排。
    pub fn new(cap_us: u32, rules: OverlayRules) -> Self {
        ComboStage {
            budget: ComboBudget::new(cap_us),
            stack: StackResolver::new(rules),
        }
    }

    /// **入组合**：预算闸（私组/超支拦截直接冒泡）→ 叠加闸（互斥拒绝
    /// 回滚退账；密度/未知类拒绝同样回滚——账实守恒）。
    pub fn mount(&mut self, card: OverlayCard, has_ticket: bool) -> Result<MountOutcome, &'static str> {
        let name = card.name.clone();
        self.budget.admit(card.clone(), has_ticket)?;
        match self.stack.push(card) {
            Ok(Verdict::Added) => Ok(MountOutcome::Mounted),
            Ok(Verdict::Replaced(old)) => Ok(MountOutcome::Replaced(old)),
            Ok(Verdict::RejectedExclusive) => {
                self.budget.refund(&name);
                Ok(MountOutcome::RejectedExclusive)
            }
            Err(e) => {
                self.budget.refund(&name);
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、组合报告：读屏可达——每行一个语义句，固定 4 行不超读屏预算
// ---------------------------------------------------------------------------

/// **组合报告**（读屏可达——域本色）：固定 4 行（≤6 行读屏预算，
/// F5202 清单口径同款）：在屏句/开销句/状态句（预算内或超支+档位+
/// 让位名额）/密度句。读屏玩家听到「超支 30 微秒，建议半剪：让位
/// 2 名」不是一堆裸数字。
pub fn combo_report(pack_name: &str, budget: &ComboBudget, stack: &StackResolver) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("组合 {}：特效 {} 枚在屏", pack_name, stack.live_len()));
    lines.push(format!("开销 {} 微秒，预算 {} 微秒", budget.spent(), budget.cap_us));
    if budget.spent() > budget.cap_us {
        let (tier, plan) = budget.downgrade_plan();
        lines.push(format!(
            "超支 {} 微秒，建议 {}：让位 {} 名",
            budget.spent() - budget.cap_us,
            tier.label(),
            plan.len()
        ));
    } else {
        lines.push(String::from("状态：预算内"));
    }
    lines.push(format!("同屏密度 {}/{}", stack.live_len(), stack.density_cap()));
    lines
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5208 域自检（判据 15 条七组：叠加消解/场景组合包/微调消费/组合预算/降档建议/读屏报告/判据元）。
pub fn run_vez08_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5208");

    // --- 叠加消解（判据一）---
    // 消-01：叠加方式表四类闭集对拍 + 未知类查无（冲突不可消解）。
    let rules = OverlayRules::new();
    let t1 = rules.blend_of("particles") == Some(BlendMode::Additive);
    let t2 = rules.blend_of("postfx") == Some(BlendMode::Replace);
    let t3 = rules.blend_of("envfx") == Some(BlendMode::Replace);
    let t4 = rules.blend_of("transition") == Some(BlendMode::Exclusive);
    let unk = rules.blend_of("ghost") == None;
    if t1 && t2 && t3 && t4 && unk && rules.len() == 4 && !rules.is_empty() {
        s.ok("EZ8-消-01-叠加表四类闭集对拍");
    } else {
        s.fail("EZ8-消-01-叠加表四类闭集对拍", "规则表漂移");
    }
    // 消-02：三态裁决——互斥先到优先 / 替换后到优先（旧名可问询）。
    let mut st = StackResolver::new(OverlayRules::new());
    let v1 = st.push(card("day", "transition", BlendMode::Exclusive, 5, 10));
    let ok1 = v1 == Ok(Verdict::Added) && st.is_live("day");
    let v2 = st.push(card("night", "transition", BlendMode::Exclusive, 5, 10));
    let ok2 = v2 == Ok(Verdict::RejectedExclusive) && !st.is_live("night");
    let v3 = st.push(card("bloom1", "postfx", BlendMode::Replace, 3, 8));
    let ok3 = v3 == Ok(Verdict::Added);
    let v4 = st.push(card("bloom2", "postfx", BlendMode::Replace, 4, 8));
    let ok4 = match v4 {
        Ok(Verdict::Replaced(ref old)) => old == "bloom1",
        _ => false,
    } && st.is_live("bloom2")
        && !st.is_live("bloom1");
    if ok1 && ok2 && ok3 && ok4 {
        s.ok("EZ8-消-02-互斥与替换三态裁决");
    } else {
        s.fail("EZ8-消-02-互斥与替换三态裁决", "消解裁决失效");
    }
    // 消-03：同屏密度 16 恰边界——第 16 枚入、第 17 枚拒（节制律执行面）。
    let mut st16 = StackResolver::new(OverlayRules::new());
    let mut i = 0usize;
    let mut all_added = true;
    while i < 16 {
        let name = format!("p{}", i);
        match st16.push(card(&name, "particles", BlendMode::Additive, 1, 1)) {
            Ok(Verdict::Added) => {}
            _ => all_added = false,
        }
        i += 1;
    }
    let v17 = st16.push(card("p17", "particles", BlendMode::Additive, 1, 1));
    let capped = all_added && v17 == Err(E_CB_DENSITY) && st16.live_len() == 16;
    if capped {
        s.ok("EZ8-消-03-密度16恰边界");
    } else {
        s.fail("EZ8-消-03-密度16恰边界", "节制闸漏");
    }

    // --- 场景组合包（判据二）---
    // 包-01：三类闭集 + 非法 kind 拒 + 成员重名拒 + 成员超限拒。
    let ok_b = ComboPack::new("a", "battle").is_ok();
    let ok_a = ComboPack::new("b", "ambience").is_ok();
    let ok_c = ComboPack::new("c", "cinematic").is_ok();
    let bad = ComboPack::new("x", "nightmare").is_err();
    let mut dup_ok = false;
    if let Ok(mut pk) = ComboPack::new("包", "battle") {
        let m1 = pk.add_member(card("s1", "particles", BlendMode::Additive, 5, 10));
        let m2 = pk.add_member(card("s1", "particles", BlendMode::Additive, 5, 10));
        dup_ok = m1.is_ok() && m2 == Err(E_CB_BAD_PACK);
    }
    let mut cap_ok = false;
    if let Ok(mut pk) = ComboPack::new("满", "battle") {
        let mut j = 0usize;
        let mut all_ok = true;
        while j < MAX_PACK_MEMBERS {
            let name = format!("m{}", j);
            if pk.add_member(card(&name, "particles", BlendMode::Additive, 1, 5)).is_err() {
                all_ok = false;
            }
            j += 1;
        }
        cap_ok = all_ok
            && pk.add_member(card("over", "particles", BlendMode::Additive, 1, 5))
                == Err(E_CB_BAD_PACK);
    }
    if ok_b && ok_a && ok_c && bad && dup_ok && cap_ok {
        s.ok("EZ8-包-01-三类闭集与完整性闸");
    } else {
        s.fail("EZ8-包-01-三类闭集与完整性闸", "包闸漏");
    }
    // 包-02：成员类校验（未知类拒）+ 开销合计手算锚（35+15=50）。
    let mut cost_ok = false;
    if let Ok(mut pk) = ComboPack::new("账", "ambience") {
        let unknown = pk.add_member(card("g", "ghost", BlendMode::Additive, 1, 1)) == Err(E_CB_CONFLICT);
        let m1 = pk.add_member(card("r", "envfx", BlendMode::Replace, 7, 35));
        let m2 = pk.add_member(card("d", "particles", BlendMode::Additive, 3, 15));
        cost_ok = unknown && m1.is_ok() && m2.is_ok() && pk.total_cost() == 50;
    }
    if cost_ok {
        s.ok("EZ8-包-02-类校验与开销合计");
    } else {
        s.fail("EZ8-包-02-类校验与开销合计", "包账失效");
    }

    // --- F5207 微调消费（判据三）---
    // 包-03：挂微调→resolve→校验器钳制（微调 250 钳到 100——微调不豁免钳制）。
    let mut tweak_ok = false;
    if let Ok(mut pk) = ComboPack::new("调", "battle") {
        let _ = pk.add_member(card("bloom1", "postfx", BlendMode::Replace, 6, 25));
        let mut vd = Validator::new();
        let d = vd.book.define("intensity", 0, 100, 40, "特效强度");
        let mut tw = PresetPack::new("微调");
        tw.put_override("intensity", 250);
        let att = pk.attach_tweak("bloom1", tw);
        let ghost = pk.attach_tweak("ghost", PresetPack::new("g")) == Err(E_CB_BAD_PACK);
        let vals = pk.tweak_values("bloom1");
        let clamped_ok = match (d, att, vals) {
            (Ok(()), Ok(()), Some(vs)) => match vd.validate_all(&vs) {
                Ok(out) => out.len() == 1 && out[0].1 == 100,
                Err(_) => false,
            },
            _ => false,
        };
        tweak_ok = ghost && clamped_ok && vd.clamped_count == 1;
    }
    if tweak_ok {
        s.ok("EZ8-包-03-F5207微调消费钳制");
    } else {
        s.fail("EZ8-包-03-F5207微调消费钳制", "消费接缝失效");
    }

    // --- 组合预算（判据四）---
    // 算-01：预算内准入 + 超支拒不记账 + 恰满含端点 + 拦截计数。
    let mut b = ComboBudget::new(120);
    let a1 = b.admit(card("m1", "particles", BlendMode::Additive, 5, 80), true);
    let a2 = b.admit(card("m2", "particles", BlendMode::Additive, 4, 80), true);
    let a3 = b.admit(card("m3", "particles", BlendMode::Additive, 3, 40), true);
    let budget_ok = a1.is_ok()
        && a2 == Err(E_CB_OVER_BUDGET)
        && a3.is_ok()
        && b.spent() == 120
        && b.member_count() == 2
        && b.over_budget_blocks == 1;
    if budget_ok {
        s.ok("EZ8-算-01-预算准入与账实守恒");
    } else {
        s.fail("EZ8-算-01-预算准入与账实守恒", "预算闸失效");
    }
    // 算-02：私组绕预算拦截——无票拒不记账，拦截 O(1) 计数。
    let mut b2 = ComboBudget::new(120);
    let r1 = b2.admit(card("x", "particles", BlendMode::Additive, 5, 10), false);
    let bypass_ok = r1 == Err(E_CB_OVERREACH)
        && b2.bypass_blocks == 1
        && b2.spent() == 0
        && b2.member_count() == 0;
    if bypass_ok {
        s.ok("EZ8-算-02-私组绕预算拦截");
    } else {
        s.fail("EZ8-算-02-私组绕预算拦截", "票据闸漏");
    }
    // 算-03：mount 回滚退账（互斥拒绝→spent 归零账实一致）+ 清账复用。
    let mut stage = ComboStage::new(120, OverlayRules::new());
    let mo1 = stage.mount(card("day", "transition", BlendMode::Exclusive, 5, 30), true);
    let mo2 = stage.mount(card("night", "transition", BlendMode::Exclusive, 5, 30), true);
    let rollback_ok = mo1 == Ok(MountOutcome::Mounted)
        && mo2 == Ok(MountOutcome::RejectedExclusive)
        && stage.budget.spent() == 30
        && stage.budget.member_count() == 1
        && stage.stack.is_live("day")
        && !stage.stack.is_live("night");
    let mut b3 = ComboBudget::new(120);
    let _ = b3.admit(card("q", "particles", BlendMode::Additive, 5, 60), true);
    b3.reset();
    let reset_ok = b3.spent() == 0
        && b3.member_count() == 0
        && b3.admit(card("q2", "particles", BlendMode::Additive, 5, 60), true).is_ok();
    if rollback_ok && reset_ok {
        s.ok("EZ8-算-03-回滚退账与清账复用");
    } else {
        s.fail("EZ8-算-03-回滚退账与清账复用", "回滚或清账失效");
    }

    // --- 降档建议（判据五）---
    // 降-01：档位表手算锚（未超/轻剪/半剪含端点/深剪）+ 低权重先让位。
    let t0 = tier_of(120, 120) == Tier::None;
    let t1 = tier_of(130, 120) == Tier::Light;
    let t2 = tier_of(150, 120) == Tier::Half;
    let t3 = tier_of(160, 120) == Tier::Deep;
    let mut b4 = ComboBudget::new(200);
    let _ = b4.admit(card("w40", "particles", BlendMode::Additive, 40, 30), true);
    let _ = b4.admit(card("w10", "particles", BlendMode::Additive, 10, 30), true);
    let _ = b4.admit(card("w30", "particles", BlendMode::Additive, 30, 30), true);
    let _ = b4.admit(card("w20", "particles", BlendMode::Additive, 20, 30), true);
    let (tn, pn) = b4.downgrade_plan();
    let order_ok = tn == Tier::None && pn.is_empty();
    let (t5, plan5) = b4.replan(90);
    let deep_ok = t5 == Tier::Deep
        && plan5.len() == 4
        && plan5[0] == "w10"
        && plan5[1] == "w20"
        && plan5[2] == "w30"
        && plan5[3] == "w40";
    let mut b5 = ComboBudget::new(120);
    let _ = b5.admit(card("w40", "particles", BlendMode::Additive, 40, 30), true);
    let _ = b5.admit(card("w10", "particles", BlendMode::Additive, 10, 30), true);
    let _ = b5.admit(card("w30", "particles", BlendMode::Additive, 30, 30), true);
    let _ = b5.admit(card("w20", "particles", BlendMode::Additive, 20, 30), true);
    let (t6, plan6) = b5.replan(100);
    let half_ok = t6 == Tier::Half && plan6.len() == 2 && plan6[0] == "w10" && plan6[1] == "w20";
    if t0 && t1 && t2 && t3 && order_ok && deep_ok && half_ok {
        s.ok("EZ8-降-01-档位对拍低权重先让");
    } else {
        s.fail("EZ8-降-01-档位对拍低权重先让", "档位或权重序漂移");
    }
    // 降-02：建议可执行——应用让位名单后不再超支（Half 让 2 释放 60 / Deep 全撤归零）。
    let mut b6 = ComboBudget::new(120);
    let _ = b6.admit(card("w40", "particles", BlendMode::Additive, 40, 30), true);
    let _ = b6.admit(card("w10", "particles", BlendMode::Additive, 10, 30), true);
    let _ = b6.admit(card("w30", "particles", BlendMode::Additive, 30, 30), true);
    let _ = b6.admit(card("w20", "particles", BlendMode::Additive, 20, 30), true);
    let (t7, plan7) = b6.replan(100);
    let freed = b6.apply_downgrade(&plan7);
    let exec_ok = t7 == Tier::Half && freed == 60 && b6.spent() == 60 && b6.member_count() == 2
        && b6.spent() <= b6.cap_us;
    let mut b7 = ComboBudget::new(120);
    let _ = b7.admit(card("w40", "particles", BlendMode::Additive, 40, 30), true);
    let _ = b7.admit(card("w10", "particles", BlendMode::Additive, 10, 30), true);
    let _ = b7.admit(card("w30", "particles", BlendMode::Additive, 30, 30), true);
    let _ = b7.admit(card("w20", "particles", BlendMode::Additive, 20, 30), true);
    let (t8, plan8) = b7.replan(70);
    let freed2 = b7.apply_downgrade(&plan8);
    let deep_exec_ok = t8 == Tier::Deep && freed2 == 120 && b7.spent() == 0 && b7.member_count() == 0;
    if exec_ok && deep_exec_ok {
        s.ok("EZ8-降-02-建议可执行不再超支");
    } else {
        s.fail("EZ8-降-02-建议可执行不再超支", "让位建议不可执行");
    }

    // --- 读屏报告（判据六）---
    // 报-01：固定 4 行全非空 + 预算内状态句 + 超支形态（档位入句）。
    let mut stage2 = ComboStage::new(120, OverlayRules::new());
    let _ = stage2.mount(card("rain", "envfx", BlendMode::Replace, 7, 35), true);
    let _ = stage2.mount(card("dust", "particles", BlendMode::Additive, 3, 15), true);
    let rep = combo_report("氛围包", &stage2.budget, &stage2.stack);
    let rep_ok = rep.len() == 4
        && rep.iter().all(|l| !l.trim().is_empty())
        && rep[2].contains("预算内")
        && rep[3].contains("同屏密度");
    let (t9, _) = stage2.budget.replan(40);
    let rep2 = combo_report("氛围包", &stage2.budget, &stage2.stack);
    let over_ok = t9 == Tier::Half && rep2.len() == 4 && rep2[2].contains("超支") && rep2[2].contains("半剪");
    if rep_ok && over_ok {
        s.ok("EZ8-报-01-读屏报告行预算");
    } else {
        s.fail("EZ8-报-01-读屏报告行预算", "报告读屏面失效");
    }

    // --- 判据元（判据七）---
    // 元-01：五码互异 + 三要素齐备（判据侧独立枚举）。
    let codes = [E_CB_CONFLICT, E_CB_DENSITY, E_CB_OVER_BUDGET, E_CB_BAD_PACK, E_CB_OVERREACH];
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
        s.ok("EZ8-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ8-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez08_combo.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ8-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ8-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：开箱三包自洽——成员数/场景类/总开销全在子预算内（包自己装得下自己）。
    let bp = ComboPack::battle_default();
    let ap = ComboPack::ambience_default();
    let cp = ComboPack::cinematic_default();
    let self_ok = bp.total_cost() == 75
        && ap.total_cost() == 80
        && cp.total_cost() == 70
        && bp.total_cost() <= COMBO_BUDGET_US
        && ap.total_cost() <= COMBO_BUDGET_US
        && cp.total_cost() <= COMBO_BUDGET_US
        && bp.members.len() == 3
        && ap.members.len() == 3
        && cp.members.len() == 2
        && bp.kind == "battle"
        && ap.kind == "ambience"
        && cp.kind == "cinematic";
    if self_ok {
        s.ok("EZ8-判据-03-开箱三包自洽");
    } else {
        s.fail("EZ8-判据-03-开箱三包自洽", "默认包漂移或超支");
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
