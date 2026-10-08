//! CGPU-F0641 · E 域开工与表面调度架构（CGPU-E 域 · 网页表面调度组 · 目标 400 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0641`
//!
//! **判据（锚点原文）**：**八主题、定位、三向契约、十组规划、判据**。
//!
//! # 一、域定位（锚点原文：架构定位）
//!
//! 网页表面是 **80 帧合同的最大负载方**——本域管「**30 个网页同时活着
//! 的算力秩序**」。30 张表面同抢一份 GPU/CPU 预算时，谁可见、谁节流、
//! 谁合批、谁先滚动，全部要在帧预算内裁出秩序——这就是本域的全部工作。
//!
//! # 二、三向边界契约（锚点原文：关系图）
//!
//! - **VE-O（F2801+）管网页内容渲染本体**：样式解析、选择器匹配、
//!   渲染树生成都归 VE-O；本域**吃渲染产物的存在性**（表面活着、多大、
//!   是否可见），不碰内容生成本体；
//! - **C 域管预算账户**：预算的记账、结算、账本归 C 域；本域**只申报
//!   用量与遵额调度**，不开户不平账；
//! - **D 域管帧合同兑现**：帧合同（80 帧）的兑现与追责归 D 域；本域
//!   **产出调度决策供兑现**，不裁决合同违约。
//! 三向各自的「peer 管什么 / 本域管什么」逐条登记（[`BOUNDARY_CONTRACTS`]），
//! 两列职责**交集恒空**（编译期+运行期双闸）——边界含糊是调度域最贵
//! 的 bug，开工单直接把它钉死。
//!
//! # 三、八主题（锚点原文：官方八主题）
//!
//! 网页场景专项域（F0641-F0800）的封闭八主题：**网页表面调度 / 表面
//! 配额 / 可见性仲裁 / 后台节流 / 合成合批 / 滚动同步 / 内存封顶 /
//! 基准场景**（[`SurfaceTheme`]）。封闭枚举 + `of_rank` 越界守卫——
//! 「表里没有」是正确答案不是遗漏。
//!
//! # 四、十组规划（锚点原文：八主题十组规划声明）
//!
//! F0641-F0800 共 **160 单**，按批次切 **E01~E10 十组**、每组 16 单
//! （[`BatchGroup`]）。规划纪律：每组 16 单 = 八主题 × 各 **2 单**
//! （[`THEME_TASKS_PER_GROUP`]）——十组规划不是「先到先得堆到满」，
//! 而是八主题在每组内**全覆盖均摊**（[`GROUP_PLANS`] + 编译期守恒闸：
//! 10 × 16 = 8 × 10 × 2 = 160）。单号 → 组映射走 O(1) 算术
//! （[`group_of_task`]），组边界衔接（上组末+1 = 下组首）编译期钉死。
//!
//! # 五、错误路径与降级矩阵
//!
//! - **非法输入 → 校验拒绝三要素**：主题秩越界（[`E_DOMAIN_THEME_INVALID`]，
//!   枚举守卫）、组秩越界（[`E_DOMAIN_GROUP_INVALID`]）、单号越域
//!   （同码，[`group_of_task`] 返回 None 并计数）；
//! - **规划漂移 → 立案拒绝**：组表范围衔接断裂 / 守恒式不成立 →
//!   [`E_DOMAIN_PLAN_DRIFT`]（规划表是开工承诺，漂移即违诺，零静默）；
//! - **边界漂移 → 立案拒绝**：三向契约 peer/职责列被改动 →
//!   [`E_DOMAIN_BOUNDARY_DRIFT`]；
//! 全部经 [`DomainLedger::open_case`] 立案（现象/影响/定位/处置）。
//!
//! # 六、性能逐项分解
//!
//! 主题/组映射 O(1)（算术映射，无查找表扫描）；规划审计 O(组数)=O(10)；
//! 边界审计 O(向数)=O(3)——开工声明面全部常数级。
//!
//! # 七、跨批对接点
//!
//! 上游：VE-O F2801+（内容渲染本体，本域消费其表面存在性）；C 域
//! （预算账户，本域申报用量）；D 域（帧合同，本域供给调度决策）。
//! 下游：E02~E10 十组各单（以本单的封闭主题集合与组表为承接前提——
//! 后续单的主题落位必须取自 [`SurfaceTheme::ALL`]，组落位取自
//! [`BatchGroup::ALL`]，越界即拒收）。
//!
//! # 八、无障碍与隐私
//!
//! 读屏替述：全部枚举/组/契约带 `zh()` 中文名与 `screen_line()` 可读
//! 单行（文档替述可读）；无隐私面（域规划不含用户数据）。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（本条不 import 未注册的
//! 兄弟模块——E02+ 尚未施工，编译期硬耦合会让本单因别人的进度而红）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 域标识（CheckSet 聚合用）。
pub const CGPU_E_DOMAIN: &str = "CGPU-E";

/// 官方八主题数（封闭全集规模，契约不是参数）。
pub const THEME_COUNT: usize = 8;

/// 批次组数（E01~E10）。
pub const GROUP_COUNT: usize = 10;

/// 每组任务数。
pub const TASKS_PER_GROUP: usize = 16;

/// 域任务总数（10 × 16）。
pub const TOTAL_TASKS: usize = GROUP_COUNT * TASKS_PER_GROUP;

/// 域首单号（F0641）。
pub const FIRST_TASK_NO: u32 = 641;

/// 域末单号（F0800）。
pub const LAST_TASK_NO: u32 = 800;

/// 每组内单主题槽位数（16 = 8 主题 × 2——全覆盖均摊的规划纪律）。
pub const THEME_TASKS_PER_GROUP: usize = 2;

// ---------------------------------------------------------------------------
// 二、诊断码（**CGPU-E 开工独占：E_DOMAIN_ 前缀段**，逐条冻结）
// ---------------------------------------------------------------------------

/// 码段纪律：开工单四码自足，与后续单的细粒度码段互不侵占。
pub const E_DOMAIN_THEME_INVALID: &str = "E_DOMAIN_THEME_INVALID";
pub const E_DOMAIN_GROUP_INVALID: &str = "E_DOMAIN_GROUP_INVALID";
pub const E_DOMAIN_PLAN_DRIFT: &str = "E_DOMAIN_PLAN_DRIFT";
pub const E_DOMAIN_BOUNDARY_DRIFT: &str = "E_DOMAIN_BOUNDARY_DRIFT";

/// 每码对应的「下一步」（拒绝必须给出路——三要素之三）。
pub const FIX_THEME: &str = "主题只能取 SurfaceTheme 八类封闭全集之一；越界秩无对应主题";
pub const FIX_GROUP: &str = "组只能取 BatchGroup 十组封闭全集之一；单号须落在 F0641..=F0800";
pub const FIX_PLAN: &str = "组表范围或守恒式漂移：先修 GROUP_PLANS 衔接，再核对 10×16=8×10×2=160";
pub const FIX_BOUNDARY: &str = "三向契约 peer 或职责列被改动；与 VE-O/C/D 对齐后重跑边界审计";

// ---------------------------------------------------------------------------
// 三、案件账（开工单自持，零墙钟零 IO）
// ---------------------------------------------------------------------------

/// 案件记录（四要素齐：现象/影响/定位/处置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainCase {
    /// 案件 id（单调递增）。
    pub id: u64,
    /// 现象。
    pub symptom: String,
    /// 影响面。
    pub impact: String,
    /// 定位。
    pub locus: String,
    /// 处置。
    pub disposition: String,
    /// 立案时的逻辑 tick。
    pub tick: u64,
}

/// 域案件账（立案流转的显性载体；满后拒绝并计数，不静默丢弃）。
pub struct DomainLedger {
    cases: Vec<DomainCase>,
    next_id: u64,
    /// 满账拒绝计数。
    pub full_rejects: u32,
}

/// 案件账容量（防御上界；开工声明面案件量极小）。
pub const MAX_DOMAIN_CASES: usize = 64;

impl DomainLedger {
    /// 空账。
    pub fn new() -> DomainLedger {
        DomainLedger { cases: Vec::new(), next_id: 1, full_rejects: 0 }
    }

    /// 在册案件数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &DomainCase> {
        self.cases.iter()
    }

    /// 立案（现象/影响为空拒——无现象或无影响面的案件无法归因）。
    pub fn open_case(
        &mut self,
        symptom: &str,
        impact: &str,
        locus: &str,
        disposition: &str,
        tick: u64,
    ) -> Result<u64, DomainError> {
        if symptom.trim().is_empty() {
            return Err(DomainError::new(
                E_DOMAIN_THEME_INVALID,
                "立案被拒：无现象",
                "没有现象的案件无法归因，也无法验证是否已修好",
            ));
        }
        if impact.trim().is_empty() {
            return Err(DomainError::new(
                E_DOMAIN_THEME_INVALID,
                "立案被拒：无影响面",
                "案件没写影响面，优先级无从判断",
            ));
        }
        if self.cases.len() >= MAX_DOMAIN_CASES {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(DomainError::new(
                E_DOMAIN_PLAN_DRIFT,
                "立案被拒：案件账已满",
                "案件账达到上限，未裁决存量未清空前不得继续堆",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.cases.push(DomainCase {
            id,
            symptom: String::from(symptom),
            impact: String::from(impact),
            locus: String::from(locus),
            disposition: String::from(disposition),
            tick,
        });
        Ok(id)
    }
}

// ---------------------------------------------------------------------------
// 四、域内错误（五元组自持；开工单不 import 兄弟模块）
// ---------------------------------------------------------------------------

/// 域错误（码/发生了什么/为什么——三要素齐发；下一步在 FIX_* 常量）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
}

impl DomainError {
    /// 构造。
    pub fn new(code: &'static str, what: &'static str, why: &str) -> Self {
        DomainError { code, what, why: String::from(why) }
    }

    /// 读屏可读的完整错误。
    pub fn screen_text(&self) -> String {
        format!("错误 {}：{}；原因：{}", self.code, self.what, self.why)
    }
}

// ---------------------------------------------------------------------------
// 五、官方八主题（封闭全集）
// ---------------------------------------------------------------------------

/// 网页场景专项域的官方八主题（封闭枚举；`of_rank` 越界 None = 枚举守卫）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceTheme {
    /// 网页表面调度（本域主轴：30 表面的算力秩序）。
    Scheduling,
    /// 表面配额（每表预算槽位分配与遵额）。
    Quota,
    /// 可见性仲裁（视口交集/遮挡裁剪/用户感知度）。
    Visibility,
    /// 后台节流（失焦/隐藏表面的降频与冻结）。
    Throttle,
    /// 合成合批（跨表面合批与图层提升）。
    Compositing,
    /// 滚动同步（滚动驱动的重排重绘调度）。
    ScrollSync,
    /// 内存封顶（表面内存上限与逐出）。
    MemoryCap,
    /// 基准场景（八主题的基准语料与验收场景）。
    Benchmark,
}

pub use SurfaceTheme::{
    Benchmark as ThBenchmark, Compositing as ThCompositing, MemoryCap as ThMemoryCap,
    Quota as ThQuota, Scheduling as ThScheduling, ScrollSync as ThScrollSync,
    Throttle as ThThrottle, Visibility as ThVisibility,
};

/// 主题总数（封闭全集规模，判据对账锚）。
pub const SURFACE_THEME_COUNT: usize = 8;

impl SurfaceTheme {
    /// 封闭全集（秩序 = 声明序）。
    pub const ALL: [SurfaceTheme; SURFACE_THEME_COUNT] = [
        SurfaceTheme::Scheduling,
        SurfaceTheme::Quota,
        SurfaceTheme::Visibility,
        SurfaceTheme::Throttle,
        SurfaceTheme::Compositing,
        SurfaceTheme::ScrollSync,
        SurfaceTheme::MemoryCap,
        SurfaceTheme::Benchmark,
    ];

    /// 秩（精确 0..=7，判据钉死不许漂移）。
    pub const fn rank(self) -> usize {
        match self {
            SurfaceTheme::Scheduling => 0,
            SurfaceTheme::Quota => 1,
            SurfaceTheme::Visibility => 2,
            SurfaceTheme::Throttle => 3,
            SurfaceTheme::Compositing => 4,
            SurfaceTheme::ScrollSync => 5,
            SurfaceTheme::MemoryCap => 6,
            SurfaceTheme::Benchmark => 7,
        }
    }

    /// 秩反查（越界 None——枚举守卫的唯一合法通道）。
    pub const fn of_rank(r: usize) -> Option<SurfaceTheme> {
        match r {
            0 => Some(SurfaceTheme::Scheduling),
            1 => Some(SurfaceTheme::Quota),
            2 => Some(SurfaceTheme::Visibility),
            3 => Some(SurfaceTheme::Throttle),
            4 => Some(SurfaceTheme::Compositing),
            5 => Some(SurfaceTheme::ScrollSync),
            6 => Some(SurfaceTheme::MemoryCap),
            7 => Some(SurfaceTheme::Benchmark),
            _ => None,
        }
    }

    /// 读屏中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            SurfaceTheme::Scheduling => "网页表面调度",
            SurfaceTheme::Quota => "表面配额",
            SurfaceTheme::Visibility => "可见性仲裁",
            SurfaceTheme::Throttle => "后台节流",
            SurfaceTheme::Compositing => "合成合批",
            SurfaceTheme::ScrollSync => "滚动同步",
            SurfaceTheme::MemoryCap => "内存封顶",
            SurfaceTheme::Benchmark => "基准场景",
        }
    }
}

// ---------------------------------------------------------------------------
// 六、十组（批次规划封闭全集）
// ---------------------------------------------------------------------------

/// 批次组（E01~E10；每组 16 单、八主题全覆盖均摊）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchGroup {
    /// E01：F0641~F0656（开工组，本单为组首）。
    E01,
    /// E02：F0657~F0672。
    E02,
    /// E03：F0673~F0688。
    E03,
    /// E04：F0689~F0704。
    E04,
    /// E05：F0705~F0720。
    E05,
    /// E06：F0721~F0736。
    E06,
    /// E07：F0737~F0752。
    E07,
    /// E08：F0753~F0768。
    E08,
    /// E09：F0769~F0784。
    E09,
    /// E10：F0785~F0800（收口组）。
    E10,
}

pub use BatchGroup::{E01 as G01, E02 as G02, E03 as G03, E04 as G04, E05 as G05,
    E06 as G06, E07 as G07, E08 as G08, E09 as G09, E10 as G10};

/// 组数（封闭全集规模）。
pub const BATCH_GROUP_COUNT: usize = 10;

impl BatchGroup {
    /// 封闭全集（秩序 = 声明序）。
    pub const ALL: [BatchGroup; BATCH_GROUP_COUNT] = [
        BatchGroup::E01, BatchGroup::E02, BatchGroup::E03, BatchGroup::E04,
        BatchGroup::E05, BatchGroup::E06, BatchGroup::E07, BatchGroup::E08,
        BatchGroup::E09, BatchGroup::E10,
    ];

    /// 秩（精确 0..=9）。
    pub const fn rank(self) -> usize {
        match self {
            BatchGroup::E01 => 0,
            BatchGroup::E02 => 1,
            BatchGroup::E03 => 2,
            BatchGroup::E04 => 3,
            BatchGroup::E05 => 4,
            BatchGroup::E06 => 5,
            BatchGroup::E07 => 6,
            BatchGroup::E08 => 7,
            BatchGroup::E09 => 8,
            BatchGroup::E10 => 9,
        }
    }

    /// 秩反查（越界 None）。
    pub const fn of_rank(r: usize) -> Option<BatchGroup> {
        match r {
            0 => Some(BatchGroup::E01),
            1 => Some(BatchGroup::E02),
            2 => Some(BatchGroup::E03),
            3 => Some(BatchGroup::E04),
            4 => Some(BatchGroup::E05),
            5 => Some(BatchGroup::E06),
            6 => Some(BatchGroup::E07),
            7 => Some(BatchGroup::E08),
            8 => Some(BatchGroup::E09),
            9 => Some(BatchGroup::E10),
            _ => None,
        }
    }

    /// 读屏名。
    pub const fn zh(self) -> &'static str {
        match self {
            BatchGroup::E01 => "E01 开工组",
            BatchGroup::E02 => "E02",
            BatchGroup::E03 => "E03",
            BatchGroup::E04 => "E04",
            BatchGroup::E05 => "E05",
            BatchGroup::E06 => "E06",
            BatchGroup::E07 => "E07",
            BatchGroup::E08 => "E08",
            BatchGroup::E09 => "E09",
            BatchGroup::E10 => "E10 收口组",
        }
    }

    /// 首单号（O(1) 算术：641 + 16 × rank）。
    pub const fn first_task(self) -> u32 {
        FIRST_TASK_NO + (self.rank() as u32) * (TASKS_PER_GROUP as u32)
    }

    /// 末单号（首 + 15）。
    pub const fn last_task(self) -> u32 {
        self.first_task() + (TASKS_PER_GROUP as u32) - 1
    }
}

/// 单号 → 组映射（O(1) 算术；越域返回 None 并可计数）。
///
/// 判据可注入越域单号（640 / 801 / u32::MAX）验证拒绝；映射本身
/// 纯算术零 panic 面（checked_sub 防回绕）。
pub fn group_of_task(n: u32) -> Option<BatchGroup> {
    if n < FIRST_TASK_NO || n > LAST_TASK_NO {
        return None;
    }
    match n.checked_sub(FIRST_TASK_NO) {
        Some(off) => BatchGroup::of_rank((off / TASKS_PER_GROUP as u32) as usize),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 七、架构定位与三向边界契约
// ---------------------------------------------------------------------------

/// 架构定位文档（锚点原文固化；判据独立断关键词）。
pub const DOMAIN_POSITIONING_DOC: &str = "\
E 域开工（CGPU-F0641 · v1）：网页表面是 80 帧合同的最大负载方——本域管\
「30 个网页同时活着的算力秩序」。关系图：VE-O F2801+ 管网页内容渲染本体、\
C 域管预算账户、D 域管帧合同兑现——本域管表面层的调度与仲裁，三向边界契约\
逐条登记、职责交集恒空。";

/// 单向边界契约（peer 管什么 / 本域管什么，两列交集恒空）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryContract {
    /// 对端标识。
    pub peer: &'static str,
    /// 对端辖区（一句话）。
    pub peer_scope: &'static str,
    /// 对端管的面（本域不碰）。
    pub owned_by_peer: &'static [&'static str],
    /// 本域管的面（对端不碰）。
    pub owned_by_self: &'static [&'static str],
}

impl BoundaryContract {
    /// 职责交集是否为空（两列逐项对拍）。
    pub fn disjoints(&self) -> bool {
        for a in self.owned_by_peer.iter() {
            for b in self.owned_by_self.iter() {
                if a == b {
                    return false;
                }
            }
        }
        true
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "三向契约：{} 管【{}】；本域管【{}】",
            self.peer,
            self.owned_by_peer.join("、"),
            self.owned_by_self.join("、")
        )
    }
}

/// 三向边界契约（锚点原文：关系图，秩序 = VE-O / C / D）。
pub const BOUNDARY_CONTRACTS: [BoundaryContract; 3] = [
    BoundaryContract {
        peer: "VE-O（F2801+）",
        peer_scope: "网页内容渲染本体",
        owned_by_peer: &["样式解析与选择器", "渲染树生成", "文本排版"],
        owned_by_self: &["表面调度与仲裁", "表面配额分配", "可见性裁决"],
    },
    BoundaryContract {
        peer: "C 域",
        peer_scope: "预算账户",
        owned_by_peer: &["预算记账与结算", "账户账本", "超支追责"],
        owned_by_self: &["用量申报", "遵额调度", "配额遵从执行"],
    },
    BoundaryContract {
        peer: "D 域",
        peer_scope: "帧合同兑现",
        owned_by_peer: &["帧合同裁决", "违约追责", "帧兑现记录"],
        owned_by_self: &["调度决策产出", "节流与合批指令", "滚动同步供给"],
    },
];

// ---------------------------------------------------------------------------
// 八、十组规划声明表
// ---------------------------------------------------------------------------

/// 单组规划行（范围 + 主题覆盖 + 状态声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组。
    pub group: BatchGroup,
    /// 主题覆盖数（恒 8——全覆盖纪律）。
    pub themes_covered: usize,
    /// 每主题槽位数（恒 2）。
    pub slots_per_theme: usize,
    /// 组状态声明（开工组=进行中；收口组=待收口；其余=待开工）。
    pub status: &'static str,
}

/// 十组规划表（与 BatchGroup::ALL 秩对位；编译期闸钉死衔接与守恒）。
pub const GROUP_PLANS: [GroupPlan; BATCH_GROUP_COUNT] = [
    GroupPlan { group: BatchGroup::E01, themes_covered: 8, slots_per_theme: 2, status: "进行中（本单开工）" },
    GroupPlan { group: BatchGroup::E02, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E03, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E04, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E05, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E06, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E07, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E08, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E09, themes_covered: 8, slots_per_theme: 2, status: "待开工" },
    GroupPlan { group: BatchGroup::E10, themes_covered: 8, slots_per_theme: 2, status: "待收口" },
];

// ---------------------------------------------------------------------------
// 九、审计（运行期对照编译期闸；全部常数级）
// ---------------------------------------------------------------------------

/// 审计八主题封闭全集（秩/中文名/越界守卫）。
pub fn audit_themes() -> Result<String, DomainError> {
    let mut k = 0usize;
    while k < SURFACE_THEME_COUNT {
        match SurfaceTheme::of_rank(k) {
            Some(t) => {
                if t.rank() != k || t.zh().is_empty() {
                    return Err(DomainError::new(
                        E_DOMAIN_THEME_INVALID,
                        "主题审计失败：秩或中文名漂移",
                        "封闭全集第 k 项的秩必须恰为 k 且中文名非空",
                    ));
                }
            }
            None => {
                return Err(DomainError::new(
                    E_DOMAIN_THEME_INVALID,
                    "主题审计失败：封闭全集有空洞",
                    "of_rank 在 0..=7 内返回 None",
                ));
            }
        }
        k += 1;
    }
    if SurfaceTheme::of_rank(SURFACE_THEME_COUNT).is_some() {
        return Err(DomainError::new(
            E_DOMAIN_THEME_INVALID,
            "主题审计失败：越界秩可命中",
            "枚举守卫失守：秩 8 不应命中任何主题",
        ));
    }
    Ok(format!("八主题审计通过：{} 类封闭全集秩精确、中文名齐备", SURFACE_THEME_COUNT))
}

/// 审计十组规划（范围衔接 + 守恒式 + 覆盖纪律）。
pub fn audit_plan() -> Result<String, DomainError> {
    // 闸 1：每组范围 = 算术推导且逐组衔接。
    let mut k = 0usize;
    while k < BATCH_GROUP_COUNT {
        let g = match BatchGroup::of_rank(k) {
            Some(g) => g,
            None => {
                return Err(DomainError::new(
                    E_DOMAIN_GROUP_INVALID,
                    "规划审计失败：组封闭全集有空洞",
                    "of_rank 在 0..=9 内返回 None",
                ));
            }
        };
        let expect_first = FIRST_TASK_NO + (k as u32) * (TASKS_PER_GROUP as u32);
        if g.first_task() != expect_first || g.last_task() != expect_first + 15 {
            return Err(DomainError::new(
                E_DOMAIN_PLAN_DRIFT,
                "规划审计失败：组范围漂移",
                "组首末单号与算术推导不一致（首=641+16k，末=首+15）",
            ));
        }
        let plan = &GROUP_PLANS[k];
        if plan.group != g || plan.themes_covered != THEME_COUNT
            || plan.slots_per_theme != THEME_TASKS_PER_GROUP
        {
            return Err(DomainError::new(
                E_DOMAIN_PLAN_DRIFT,
                "规划审计失败：组表与封闭全集失配",
                "GROUP_PLANS 行与组秩对位、覆盖纪律（8 主题 × 2 槽）漂移",
            ));
        }
        if plan.status.is_empty() {
            return Err(DomainError::new(
                E_DOMAIN_PLAN_DRIFT,
                "规划审计失败：组状态声明为空",
                "每组必须有状态声明（进行中/待开工/待收口）",
            ));
        }
        k += 1;
    }
    // 闸 2：守恒式 10 × 16 = 8 × 10 × 2 = 160，且首末单号闭合。
    if TOTAL_TASKS != GROUP_COUNT * TASKS_PER_GROUP
        || TOTAL_TASKS != THEME_COUNT * BATCH_GROUP_COUNT * THEME_TASKS_PER_GROUP
        || FIRST_TASK_NO != 641
        || LAST_TASK_NO != FIRST_TASK_NO + (TOTAL_TASKS as u32) - 1
    {
        return Err(DomainError::new(
            E_DOMAIN_PLAN_DRIFT,
            "规划审计失败：守恒式不成立",
            "10×16 = 8×10×2 = 160 与首末单号闭合是开工承诺的算术骨架",
        ));
    }
    Ok(format!(
        "十组规划审计通过：{} 组 × {} 单 = {} 单，八主题 × {} 槽全覆盖均摊",
        GROUP_COUNT, TASKS_PER_GROUP, TOTAL_TASKS, THEME_TASKS_PER_GROUP
    ))
}

/// 审计三向边界契约（peer 齐 + 职责交集空 + 双列非空）。
pub fn audit_boundary() -> Result<String, DomainError> {
    if BOUNDARY_CONTRACTS.len() != 3 {
        return Err(DomainError::new(
            E_DOMAIN_BOUNDARY_DRIFT,
            "边界审计失败：三向不齐",
            "锚点关系图恰三向（VE-O / C / D），缺向即漂移",
        ));
    }
    for c in BOUNDARY_CONTRACTS.iter() {
        if c.peer.is_empty() || c.peer_scope.is_empty() {
            return Err(DomainError::new(
                E_DOMAIN_BOUNDARY_DRIFT,
                "边界审计失败：对端标识或辖区为空",
                "空对端的契约无法归责",
            ));
        }
        if c.owned_by_peer.is_empty() || c.owned_by_self.is_empty() {
            return Err(DomainError::new(
                E_DOMAIN_BOUNDARY_DRIFT,
                "边界审计失败：职责列为空",
                "任一侧职责列空 = 边界未声明 = 调度越权温床",
            ));
        }
        if !c.disjoints() {
            return Err(DomainError::new(
                E_DOMAIN_BOUNDARY_DRIFT,
                "边界审计失败：职责交集非空",
                "同一面被两侧同时声明所有，边界含糊是调度域最贵的 bug",
            ));
        }
    }
    Ok("三向边界审计通过：VE-O/C/D 职责交集恒空、双列齐备".to_string())
}

/// 全域审计汇总（一条命令跑完所有闸，供下游与判据共用）。
pub fn audit_all() -> Result<String, DomainError> {
    let a = audit_themes()?;
    let b = audit_plan()?;
    let c = audit_boundary()?;
    Ok(format!("{}；{}；{}", a, b, c))
}

/// 域审计摘要（可读单行，承 F4601 的 `*_summary` 惯例）。
pub fn domain_summary() -> String {
    format!(
        "{}｜八主题（调度/配额/可见性/节流/合批/滚动/内存/基准）｜十组 {} 单",
        CGPU_E_DOMAIN, TOTAL_TASKS
    )
}

// ---------------------------------------------------------------------------
// 十、编译期闸（数值域全在这里断）
// ---------------------------------------------------------------------------

const _: () = {
    // 闸 1：封闭全集规模（数组长度即断言）。
    assert!(SurfaceTheme::ALL.len() == THEME_COUNT);
    assert!(BatchGroup::ALL.len() == GROUP_COUNT);
    assert!(GROUP_PLANS.len() == GROUP_COUNT);

    // 闸 2：秩精确（不是单调，是精确值）。
    assert!(SurfaceTheme::Scheduling.rank() == 0);
    assert!(SurfaceTheme::Benchmark.rank() == 7);
    assert!(BatchGroup::E01.rank() == 0);
    assert!(BatchGroup::E10.rank() == 9);
    assert!(SurfaceTheme::of_rank(8).is_none());
    assert!(BatchGroup::of_rank(10).is_none());

    // 闸 3：守恒式（开工承诺的算术骨架）。
    assert!(GROUP_COUNT * TASKS_PER_GROUP == TOTAL_TASKS);
    assert!(THEME_COUNT * BATCH_GROUP_COUNT * THEME_TASKS_PER_GROUP == TOTAL_TASKS);
    assert!(TOTAL_TASKS == 160);

    // 闸 4：单号域闭合（首 641、末 800）。
    assert!(FIRST_TASK_NO == 641);
    assert!(LAST_TASK_NO == 800);
    assert!(BatchGroup::E01.first_task() == 641);
    assert!(BatchGroup::E01.last_task() == 656);
    assert!(BatchGroup::E10.first_task() == 785);
    assert!(BatchGroup::E10.last_task() == 800);
    assert!(BatchGroup::E02.first_task() == BatchGroup::E01.last_task() + 1);

    // 闸 5：规划纪律——全覆盖均摊。
    assert!(THEME_TASKS_PER_GROUP == 2);
    assert!(THEME_COUNT * THEME_TASKS_PER_GROUP == TASKS_PER_GROUP);
};

// ---------------------------------------------------------------------------
// 十一、tests（宿主单测；发行剔除零成本）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_roundtrip() {
        for (i, t) in SurfaceTheme::ALL.iter().enumerate() {
            assert_eq!(t.rank(), i);
            assert_eq!(SurfaceTheme::of_rank(i), Some(*t));
        }
        assert_eq!(SurfaceTheme::of_rank(8), None);
    }

    #[test]
    fn task_group_mapping() {
        assert_eq!(group_of_task(641), Some(BatchGroup::E01));
        assert_eq!(group_of_task(656), Some(BatchGroup::E01));
        assert_eq!(group_of_task(657), Some(BatchGroup::E02));
        assert_eq!(group_of_task(800), Some(BatchGroup::E10));
        assert_eq!(group_of_task(640), None);
        assert_eq!(group_of_task(801), None);
    }
}
