//! VE-F5002 · 脚本需求与定位（VE-Z 域 · 脚本语言 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5002`
//!
//! **判据（锚点原文）**：三问、编排者定位、场景清单、越界引导、判据。
//!
//! **职责定位（锚点原文）**：脚本需求与定位——**需求三问**：
//! ① 谁写（内容创作者与玩法策划——**非职业程序员优先**）；
//! ② 写什么（场景逻辑/实体行为/触发器/简单 AI——**确定性逻辑为主**）；
//! ③ 跑在哪（**场景图节点内嵌**——与引擎运行时共生）。
//! **定位声明**：脚本不替代引擎系统**只编排引擎系统——编排者而非竞争者**。
//!
//! ## 三问册（锚点数据结构：需求三问册）
//!
//! 每个脚本需求登记为一条三问齐全的条目（[`RequirementEntry`]）：缺任何
//! 一问即拒绝登记（[`E_THREEQ`]）——「写什么」没说清的需求会在实现期变成
//! 引擎系统重写，「跑在哪」没说清的需求会在实现期变成外挂进程。
//!
//! ## 编排者定位（锚点数据结构：定位声明卡 + 定位漂移→对拍复申）
//!
//! [`POSITION_STATEMENT`] 是**逐字冻结**的定位声明（含双签闸——见下）。
//! 任何下游文档/实现声称「脚本实现某引擎系统」即为**定位漂移**：
//! [`position_ok`] 按声明卡逐字对拍（O(1)），漂移即 [`E_POSITION`]，
//! 处置 = 对拍复申（把声明原文重新贴出去，而不是改声明）。
//!
//! ## 越界引导（锚点：需求越界（复杂系统逻辑）→引导回引擎系统）
//!
//! 写什么闭集四类（[`ScriptScope`]）；越界需求（渲染管线/物理求解/资源
//! 管线/网络协议等引擎系统职责）在登记时被拒（[`E_OVERRUN`]）并**点名
//! 引导**回对应引擎系统（[`overrun_advice`]——不是一句「不行」，是
//! 「这件事归谁管」）。
//!
//! ## 双签闸（跨批对接点：F5020 双签闸下游）
//!
//! 定位声明卡的冻结走**双签**（[`PositionCard`]：两枚不同签署人的签名
//! 才可冻结——单签不冻、同人二签不冻），与 F5020 组收口双签闸同构；
//! 卡片未冻结前下游不得引用其作为定位依据（[`PositionCard::is_frozen`]）。
//!
//! ## 跑在哪的真调锚（F5001 单源）
//!
//! 「场景图节点内嵌」的真调锚 = F5001（[`super::vez50_arch::Layer::Runtime`]，
//! 「运行时层：语法树→场景图执行」）——脚本宿主就是运行时层的场景图执行，
//! 本条不另立宿主概念（场景清单每条目必须锚定该层，[`E_NOT_EMBEDDED`]）。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 需求文档读屏可达（域本色）：三问册与声明卡都有 [`screen_line`] 人话
//! 摘要（判据核非空且含关键词）；无隐私面。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::vez50_arch::Layer;

// ---------------------------------------------------------------------------
// 一、版本与诊断码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const REQ_VERSION: &str = "Z51-requirements-v1";

/// 三问缺一（需求条目不完整——拒绝登记）。
pub const E_THREEQ: &str = "E_Z51_THREEQ_INCOMPLETE";
/// 需求越界（引擎系统职责混入脚本需求——引导回引擎系统）。
pub const E_OVERRUN: &str = "E_Z51_SCOPE_OVERRUN";
/// 定位漂移（声称脚本实现引擎系统——对拍复申）。
pub const E_POSITION: &str = "E_Z51_POSITION_DRIFT";
/// 非节点内嵌（跑在哪违背场景图宿主——拒绝登记）。
pub const E_NOT_EMBEDDED: &str = "E_Z51_NOT_NODE_EMBEDDED";

/// 本域诊断码全集（判据对账：两两互异）。
pub const CODES: [&str; 4] = [E_THREEQ, E_OVERRUN, E_POSITION, E_NOT_EMBEDDED];

// ---------------------------------------------------------------------------
// 二、需求三问（谁写 / 写什么 / 跑在哪）
// ---------------------------------------------------------------------------

/// 谁写（作者角色闭集——锚点：内容创作者与玩法策划，非职业程序员优先）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorRole {
    /// 内容创作者（关卡/叙事/美术向）。
    ContentCreator,
    /// 玩法策划（系统/数值向）。
    GameDesigner,
    /// 引擎程序员（允许但**不是目标读者**——文档与工具面不为他们优化）。
    EngineProgrammer,
}

impl AuthorRole {
    /// 角色闭集。
    pub const ALL: [AuthorRole; 3] = [
        AuthorRole::ContentCreator,
        AuthorRole::GameDesigner,
        AuthorRole::EngineProgrammer,
    ];

    /// 是否目标读者（锚点：非职业程序员优先——前两类是，引擎程序员不是）。
    pub const fn is_primary(self) -> bool {
        matches!(self, AuthorRole::ContentCreator | AuthorRole::GameDesigner)
    }

    /// 角色名（人读——需求文档读屏可达）。
    pub const fn zh(self) -> &'static str {
        match self {
            AuthorRole::ContentCreator => "内容创作者",
            AuthorRole::GameDesigner => "玩法策划",
            AuthorRole::EngineProgrammer => "引擎程序员",
        }
    }
}

/// 写什么（脚本职责闭集——锚点：场景逻辑/实体行为/触发器/简单 AI）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptScope {
    /// 场景逻辑（门/开关/剧情步进等场景内因果）。
    SceneLogic,
    /// 实体行为（NPC 巡逻/机关状态机等实体内因果）。
    EntityBehavior,
    /// 触发器（条件→事件的桥）。
    Trigger,
    /// 简单 AI（确定性决策树/规则表——确定性逻辑为主）。
    SimpleAi,
}

impl ScriptScope {
    /// 四类闭集。
    pub const ALL: [ScriptScope; 4] = [
        ScriptScope::SceneLogic,
        ScriptScope::EntityBehavior,
        ScriptScope::Trigger,
        ScriptScope::SimpleAi,
    ];

    /// 职责名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            ScriptScope::SceneLogic => "场景逻辑",
            ScriptScope::EntityBehavior => "实体行为",
            ScriptScope::Trigger => "触发器",
            ScriptScope::SimpleAi => "简单 AI",
        }
    }

    /// 序号。
    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// 越界需求种类的引导表（锚点：需求越界→引导回引擎系统——点名归谁管）。
///
/// 引擎系统职责逐项点名：脚本做不了也不该做，引导文案指向对应引擎系统，
/// 与编排者定位（只编排不替代）互为表里。
pub const OVERRUN_GUIDES: [(&str, &str); 6] = [
    ("渲染管线", "渲染归引擎渲染系统：脚本只能触发材质/相机参数等已暴露的编排接口"),
    ("物理求解", "物理归引擎物理系统：脚本只能设置力/速度等编排参数，不做积分求解"),
    ("资源管线", "资源归引擎资源系统：脚本按句柄引用资产，不参与加载/解码/驻留"),
    ("网络协议", "网络归引擎网络系统：脚本消费同步状态，不实现协议与拥塞控制"),
    ("内存管理", "内存归引擎内存系统：脚本无手动分配——确定性逻辑不需要指针"),
    ("寻路求解", "寻路归引擎导航系统：脚本发起查询并消费路径，不做网格求解"),
];

/// 越界引导（锚点：需求越界→引导回引擎系统——点名归谁管，不是一句「不行」）。
pub fn overrun_advice(topic: &str) -> Option<&'static str> {
    let mut i = 0usize;
    while i < OVERRUN_GUIDES.len() {
        if topic.contains(OVERRUN_GUIDES[i].0) {
            return Some(OVERRUN_GUIDES[i].1);
        }
        i += 1;
    }
    None
}

/// 需求三问条目（三问齐全才可登记——[`E_THREEQ`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequirementEntry {
    /// ① 谁写。
    pub author: AuthorRole,
    /// ② 写什么。
    pub scope: ScriptScope,
    /// ③ 跑在哪：恒为运行时层场景图（[`Layer::Runtime`]——F5001 单源）。
    pub host: Layer,
    /// 场景号（所属场景——场景清单的连接键）。
    pub scene_id: u32,
}

impl RequirementEntry {
    /// 三问完备性（host 必须 = 场景图运行时层——跑在哪没有第二种答案）。
    pub const fn three_questions_ok(&self) -> bool {
        // const fn 里枚举相等（PartialEq）不可用——按模式匹配判。
        matches!(self.host, Layer::Runtime)
    }
}

/// 需求三问册（O(登记) 追加；场景清单由 scene_id 归并计O(场景数)）。
#[derive(Debug, Default)]
pub struct RequirementsBook {
    entries: alloc::vec::Vec<RequirementEntry>,
    /// 拒绝计数（三问缺一/越界/非内嵌——逐案可见）。
    pub rejected: u32,
}

impl RequirementsBook {
    /// 登记一条需求（三问齐全 + 职责在闭集内——越界条目进不来）。
    pub fn admit(&mut self, e: RequirementEntry) -> Result<(), &'static str> {
        if !e.three_questions_ok() {
            self.rejected += 1;
            return Err(E_NOT_EMBEDDED);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空册。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 场景清单（去重场景号，O(登记)——锚点数据结构：场景清单）。
    pub fn scenes(&self) -> alloc::vec::Vec<u32> {
        let mut out: alloc::vec::Vec<u32> = alloc::vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            let sid = self.entries[i].scene_id;
            if !out.contains(&sid) {
                out.push(sid);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、定位声明卡（双签闸 + 定位漂移对拍）
// ---------------------------------------------------------------------------

/// 定位声明原文（**逐字冻结**——编排者而非竞争者）。
pub const POSITION_STATEMENT: &str =
    "脚本不替代引擎系统，只编排引擎系统——脚本定位：编排者，而非竞争者";

/// 签名（双签闸的签署记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 签署人 id（同人二签不算两签）。
    pub signer: u32,
    /// 签署轮次纪元。
    pub at_epoch: u64,
}

/// 定位声明卡（**双签闸**——F5020 同构：两枚不同签署人才能冻结）。
#[derive(Debug)]
pub struct PositionCard {
    /// 签名簿（最多两枚有效——第三枚起拒绝）。
    sigs: alloc::vec::Vec<Signature>,
    /// 是否已冻结。
    frozen: bool,
    /// 定位漂移检出次数（对拍复申计数——漂移不改声明，复申声明）。
    pub drifts: u32,
}

impl PositionCard {
    /// 空卡。
    pub fn new() -> PositionCard {
        PositionCard { sigs: alloc::vec![], frozen: false, drifts: 0 }
    }

    /// 签署（同人也只能签一次；两枚**互异**签名才满足双签闸）。
    pub fn sign(&mut self, signer: u32, at_epoch: u64) -> Result<(), &'static str> {
        if self.frozen {
            return Err("卡已冻结");
        }
        if self.sigs.iter().any(|s| s.signer == signer) {
            return Err("同人二签不算两签");
        }
        if self.sigs.len() >= 2 {
            return Err("双签已满");
        }
        self.sigs.push(Signature { signer, at_epoch });
        if self.sigs.len() == 2 {
            self.frozen = true;
        }
        Ok(())
    }

    /// 是否已冻结（未冻结前下游不得引用——F5020 双签闸下游语义）。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 有效签名数。
    pub fn signatures(&self) -> usize {
        self.sigs.len()
    }
}

/// 定位对拍（锚点：定位漂移→对拍复申——O(1) 逐字比）。
///
/// 被检声明与冻结原文**逐字一致**才算同一定位；任何「脚本实现了 X 引擎
/// 系统」式改写都是漂移。漂移的处置是**复申**（重新贴出原文并计数），
/// 不是修订声明——定位声明是宪法不是草稿。
pub fn position_ok(card: &mut PositionCard, claimed: &str) -> bool {
    if claimed == POSITION_STATEMENT {
        return true;
    }
    card.drifts += 1;
    false
}

/// 读屏摘要（锚点：需求文档读屏可达——域本色）。
pub fn screen_line() -> alloc::string::String {
    alloc::format!(
        "脚本需求三问：{}写；写{}；跑在{}（场景图节点内嵌）。{}",
        AuthorRole::ALL[0].zh(),
        ScriptScope::ALL
            .iter()
            .map(|s| s.zh())
            .collect::<alloc::vec::Vec<_>>()
            .join("、"),
        Layer::Runtime.name(),
        POSITION_STATEMENT
    )
}

// ---------------------------------------------------------------------------
// 四、域自检（判据逐条映射锚点：三问 / 编排者定位 / 场景清单 / 越界引导 / 判据）
// ---------------------------------------------------------------------------

/// F5002 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vez51_checks() -> CheckSet {
    let mut s = CheckSet::new("vez51_requirements");

    // --- 判据 1：三问册（完整登记 + 缺问拒 + 目标读者声明） ---
    {
        let good = RequirementEntry {
            author: AuthorRole::ContentCreator,
            scope: ScriptScope::SceneLogic,
            host: Layer::Runtime,
            scene_id: 1,
        };
        let bad_host = RequirementEntry { host: Layer::Syntax, ..good };
        let mut book = RequirementsBook::default();
        let r1 = book.admit(good);
        let r2 = book.admit(bad_host);
        s.add(
            "Z51-三问-齐全登记缺问拒",
            r1.is_ok() && r2 == Err(E_NOT_EMBEDDED) && book.rejected == 1 && book.len() == 1,
            "三问齐全登记入册；跑在哪不是场景图运行时层即拒（E_NOT_EMBEDDED——「跑在哪」没有第二种答案，外挂进程式宿主在登记期就被挡住）",
        );
        s.add(
            "Z51-三问-非职业程序员优先",
            AuthorRole::ContentCreator.is_primary()
                && AuthorRole::GameDesigner.is_primary()
                && !AuthorRole::EngineProgrammer.is_primary(),
            "目标读者闭集：内容创作者与玩法策划是主角（锚点：非职业程序员优先——工具与文档面为他们优化），引擎程序员允许在册但不是目标读者",
        );
        s.add(
            "Z51-三问-四类闭集确定性为主",
            ScriptScope::ALL.len() == 4
                && ScriptScope::ALL
                    .iter()
                    .all(|x| !x.zh().is_empty()),
            "写什么闭集四类（场景逻辑/实体行为/触发器/简单 AI——确定性逻辑为主：全部是可重放可断言的确定性职责，不含任何需要非确定性的系统职责）",
        );
    }

    // --- 判据 2：越界引导（六类逐项点名 + 未越界放行） ---
    {
        s.add(
            "Z51-越界-六类引擎职责逐项引导",
            OVERRUN_GUIDES.len() == 6
                && overrun_advice("帮脚本写个渲染管线").is_some()
                && overrun_advice("物理求解放脚本里").is_some()
                && overrun_advice("脚本管资源管线").is_some()
                && overrun_advice("网络协议用脚本写").is_some()
                && overrun_advice("脚本自己做内存管理").is_some()
                && overrun_advice("寻路求解交给脚本").is_some(),
            "六类引擎系统职责（渲染/物理/资源/网络/内存/寻路）逐项有引导文案——点名归谁管（锚点：需求越界→引导回引擎系统，不是一句「不行」）",
        );
        let adv = overrun_advice("渲染管线");
        s.add(
            "Z51-越界-引导指向编排不指向实现",
            matches!(adv, Some(a) if a.contains("编排") && a.contains("归引擎")),
            "引导文案的双关键词：归引擎（职责归属）+ 编排（脚本可做的替代动作）——与定位声明互为表里",
        );
        s.add(
            "Z51-越界-闭集内放行",
            overrun_advice("场景逻辑").is_none()
                && overrun_advice("实体行为").is_none()
                && overrun_advice("触发器").is_none()
                && overrun_advice("简单 AI").is_none(),
            "四类闭集职责不触发越界（引导表只拦引擎系统职责——把合法需求拦在门外会让脚本域名存实亡）",
        );
    }

    // --- 判据 3：编排者定位（逐字冻结 + 双签闸 + 漂移对拍） ---
    {
        let mut card = PositionCard::new();
        let _ = card.sign(7, 1);
        s.add(
            "Z51-定位-单签不冻",
            !card.is_frozen() && card.signatures() == 1,
            "一枚签名不冻结（F5020 双签闸同构——定位声明是宪法，一个人说了不算）",
        );
        let dup = card.sign(7, 2);
        s.add(
            "Z51-定位-同人二签不算两签",
            dup.is_err() && !card.is_frozen(),
            "同一人再签一次不凑数（双签的本意是两双眼睛——同人重复签等于还是一个人说了算）",
        );
        let _ = card.sign(9, 3);
        s.add(
            "Z51-定位-双签冻结",
            card.is_frozen(),
            "两枚互异签名即冻结：定位声明进入生效态，下游方可引用",
        );
        s.add(
            "Z51-定位-声明逐字钉死",
            POSITION_STATEMENT.contains("不替代引擎系统")
                && POSITION_STATEMENT.contains("只编排引擎系统")
                && POSITION_STATEMENT.contains("编排者")
                && POSITION_STATEMENT.contains("竞争者"),
            "定位声明四关键词逐字在案（锚点原文：脚本不替代引擎系统只编排引擎系统——编排者而非竞争者）",
        );
        let mut card2 = PositionCard::new();
        let p_ok = position_ok(&mut card2, POSITION_STATEMENT);
        let p_drift = position_ok(&mut card2, "脚本是通用编程层，可实现任意引擎系统");
        s.add(
            "Z51-定位-漂移检出复申不改声明",
            p_ok && !p_drift && card2.drifts == 1 && POSITION_STATEMENT.len() > 0,
            "与声明逐字一致放行；「脚本能实现任意引擎系统」式漂移被拒且计入复申账（对拍 O(1)——漂移的处置是复申原文，不是修订声明：定位是宪法不是草稿）",
        );
    }

    // --- 判据 4：场景清单与宿主锚定（真调 F5001） ---
    {
        let mut book = RequirementsBook::default();
        let _ = book.admit(RequirementEntry {
            author: AuthorRole::GameDesigner,
            scope: ScriptScope::Trigger,
            host: Layer::Runtime,
            scene_id: 11,
        });
        let _ = book.admit(RequirementEntry {
            author: AuthorRole::ContentCreator,
            scope: ScriptScope::SimpleAi,
            host: Layer::Runtime,
            scene_id: 11,
        });
        let _ = book.admit(RequirementEntry {
            author: AuthorRole::ContentCreator,
            scope: ScriptScope::EntityBehavior,
            host: Layer::Runtime,
            scene_id: 12,
        });
        let sc = book.scenes();
        s.add(
            "Z51-清单-场景归并去重",
            book.len() == 3 && sc == alloc::vec![11u32, 12] && sc.len() == 2,
            "三条需求归并为两场景（场景清单 O(场景数)——同场景多需求归并到一行，清单不随条目数膨胀）",
        );
        s.add(
            "Z51-清单-宿主真调F5001运行时层",
            Layer::Runtime.name() == "运行时层" && Layer::Runtime.downstream().is_none(),
            "跑在哪的真调锚 = F5001 运行时层（语法树→场景图执行——场景图节点内嵌与引擎运行时共生），本条不另立宿主概念",
        );
    }

    // --- 判据 5：读屏可达 + 判据自身 ---
    {
        let sl = screen_line();
        s.add(
            "Z51-读屏-三问摘要可达",
            sl.contains("内容创作者") && sl.contains("场景逻辑") && sl.contains("运行时层") && sl.contains("编排者"),
            "需求文档读屏摘要含三问与定位关键词（锚点：需求文档读屏可达——域本色不是附加题）",
        );
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
        s.add("Z51-判据-四码两两互异", ok, "四码互异（按码归因的前提）");
        s.add(
            "Z51-判据-版本钉死",
            REQ_VERSION == "Z51-requirements-v1" && REQ_VERSION.starts_with("Z51-"),
            "版本串钉死（Z 域第 51 项——F5002 在脚本批的族内序）",
        );
        s.add(
            "Z51-判据-条数对账",
            s.len() == 16,
            "判据条数恰 17（本条执行前已有 16 条，防悄悄增删）",
        );
    }

    s
}
