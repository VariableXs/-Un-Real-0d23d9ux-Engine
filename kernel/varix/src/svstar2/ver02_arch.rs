//! VE-F3601 · R 域开工与域号 ADR 声明（VE-R 域 · 创作生态域 · R01 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3601`
//!
//! **判据（锚点原文）**：跳段 ADR、同步更新、五板块、四域分工、收敛复述、判据。
//!
//! **职责定位（锚点原文）**：R 域开工与域号 ADR 声明——R 域开工（R=创作生态域：
//! 让用户与第三方在引擎之上创造（Minecraft 式生长的终极承载——十四章执法域）；
//! 域号 ADR（R 域条目号 F3601 起跳（F3401-F3600 段为 E 域扩展预留——域号跳段
//! 声明 ADR：跳段原因（E 域二期扩展预留+未来域对齐——ADR 登记（域号跳段不加
//! 混淆：台账与映射表同步更新（同步更新红线：跳段不登记=后续域混乱（跳段登记
//! 红线）；域使命（创作资产模型/工作流/工作台/编辑器/工坊五板块十项映射；
//! 与 E/P/S 域关系（E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义——四域
//! 分工声明）。
//!
//! **数据结构（锚点原文·家族格式）**：ADR 登记册；五板块架构；十项映射；四域分工表。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：跳段未登记→阻断开工（红线实测）；
//! 映射缺项→补齐；域间分工分歧→对拍。
//!
//! **性能逐项分解（锚点原文·家族格式）**：登记 O(1)；映射 O(1)；分工 O(1)。
//!
//! **跨批对接点（锚点原文·家族格式）**：E/P/S 对端声明；F3201 Q 管线（创作资产
//! 走管线——收敛复述）；Q 域移交包接收。
//!
//! **无障碍与隐私（锚点原文）**：创作无障碍双维（创作工具无障碍+创作内容无障碍
//! ——F3611/F3650 前向）；无隐私面。
//!
//! # 〇、模块命名与`ver01` 的关系（防撞名，先说清）
//!
//! 仓内已有 [`vep01_arch`](super::vep01_arch)（P 域 F3001）、`veq01_pipeline`
//! （Q 域 F3201）、`vee01_arch`（E 域 F0801 文字渲染）等域开工模块，另有
//! **`ver01_arch`（VE-F3401 · E 域令牌运行时架构）**。它占用`ver01` 这个名字，
//! 但它是 **E 域**（主题与个性化引擎 F3401-F3600）的开工项，不是本项。
//! 因此本项取 `ver02_*`——序号递增避让，不改动他人模块名（源码只增不减不移）。
//!
//! # 一、域号跳段：本项存在的第一理由（锚点「域号跳段声明 ADR」）
//!
//! 锚点写「R 域条目号 F3601 起跳（F3401-F3600 段为 E 域扩展预留）」。这句话
//! 本身是一个 **ADR（架构决策记录）**，而不是一句编号说明——它决定三件事，任何
//! 一件登记错位，后续 200 项都会建在错地基上：
//!
//! 1. **R 域起点 = F3601**（不是开篇域号总表写的 F3401）；
//! 2. **F3401-F3600 整段让给 E 域二期扩展**（不是给 R 域）；
//! 3. **R 域之后的号段归属**（F3601+ 是 R；原 S 域顺延至 F3801+）。
//!
//! ## 1.1 为什么必须登记（锚点：跳段不登记=后续域混乱）
//!
//! 总纲开篇的**全域地图**（锚点第 100 行「域 | 序号区间 | 域名」表）写：
//! `VE-R | F3401-F3600 | 主题与个性化引擎`、`VE-S | F3601-F3800 | 无障碍渲染`。
//! 而本项锚点写「R 域条目号 F3601 起跳（F3401-F3600 段为 E 域扩展预留）」。
//!
//! **两处对同一段号给出了不同归属**——域号表把 F3401-F3600 记为 R、
//! F3601-F3800 记为 S；本项锚点把 F3401-F3600 记为 E（二期预留）、F3601+ 记为
//! R（创作生态）。这不是笔误，是**册内两套编号并存**的历史事实：
//!
//! - 早期按字母顺序排定：R=主题与个性化（F3401-F3600）、S=无障碍渲染（F3601-F3800）；
//! - 后期**主题与个性化被改判为 E 域**（册内 E01 批次段明写「VE-E 域开工
//!   （主题与个性化引擎，F3401-F3600）」）；创作生态另起 R 域占 F3601+；
//!   原 S 域顺延至 F3801+（册内「VE-F3801 · S 域开工与无障碍渲染总架构」可证）。
//!
//! ## 1.2 本项的裁决（不静默、不装作没看见）
//!
//! 本项**不做静默取舍**：既不假装域号表不存在，也不假装锚点没写。做法是
//! **两套编号同时登记**，并给出**判定优先级**：
//!
//! - [`NumberBand`] 是号段一等公民：每个号段带 [`BandAuthority`]（谁裁定）
//!   与 [`BandBasis`]（依据什么）；
//! - 冲突号段在 [`JumpLedger`] 里以 [`BandVerdict`] **显式记录裁决与理由**，
//!   不靠「取其一」把另一半信息丢掉；
//! - [`JumpLedger::preflight`] 是**开工硬门**：未裁决冲突非零即
//!   [`E_CONFLICT_UNADJUDICATED`]阻断（锚点：跳段未登记→阻断开工·红线实测）。
//!
//! 这样后来人读到哪套编号都能查到裁决依据，而不会看到一份自相矛盾的台账。
//!
//! ## 1.3 同步更新红线（锚点：台账与映射表同步更新）
//!
//! 跳段一旦登记，**两处必须同时改**：域号台账（[`JumpLedger`]）与十项映射表
//! （[`CreationBoard`] 的落点）。只改一处就是埋雷——台账说 R 从 F3601 起、
//! 映射表还按 F3401 落点，后续条目会建到 E 域段里去。本项把这件事做成
//! **可机检的**：[`JumpLedger::sync_report`] 逐项核对，
//! [`DomainNumberMap::check_sync`] 是硬门（不同步即 [`E_SYNC_MISMATCH`]）。
//!
//! # 二、五板块 vs 十项映射（锚点计数歧义与裁决）
//!
//! 锚点写「域使命（创作资产模型/工作流/工作台/编辑器/工坊**五板块十项映射**）」。
//! 五板块由锚点逐个点名，**五项确定**；「十项映射」未逐个点名，因此本项的
//! 十项**从锚点自身与册内实情推出**，推出后写进 [`CreationConcern`] 并由
//! [`check_concern_coverage`] 机检**十项 10/10 齐备且无空落点**——十项不是
//! 凑数，每项都指向**册内真实条目号**，且每项必须写清**取这一项的依据**
//! （[`CreationConcern::basis`]，缺依据即 [`E_CONCERN_NO_BASIS`]）。
//!
//! 十项取法（每板块两项，理由见 `basis`）：创作资产模型分「资产模型本体」与
//! 「导入导出」（F3603/F3648）——模型是内型、迁出迁入是外形，只立模型会让
//! 第三方资产进不来；工作流分「工作流引擎」与「验证发布」（F3604/F3627）——
//! 编排与把关是两种失败模式；工作台分「预览运行时」与「调试器」
//! （F3606/F3613）——所见即所得与问题定位互为入口与出口；编辑器分「主题编辑器」
//! 与「编辑器无障碍」（F3621/F3630）——工具本身要能被无障碍用户操作；
//! 工坊分「壁纸工坊」与「图标工坊」（F3641/F3645）——两个最大的视觉创作面。
//!
//! # 三、四域分工（锚点：E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义）
//!
//! 锚点逐域点明了四域分工，本项把它做成 [`DivisionTable`]：**每项能力有且只有
//! 一个属主域**，属主重复即 [`E_DIVISION_DUPLICATE`]（域间分工分歧→对拍），
//! 无主即 [`E_DIVISION_NO_OWNER`]。四域语义边界：
//!
//! - **E 供引擎事实**：令牌与主题包的运行时事实（E01 组起）——R 域只消费不复述；
//! - **R 供创作界面**：编辑器/工坊/工作流/工作台的界面与流程本体；
//! - **P 供动效语言**：动效库与转场（F3001 起）——R 域动效一律走 P 域语言；
//! - **S 供语义**：无障碍渲染语义与像素层执行（F3801 起）——R 域不重述渲染语义。
//!
//! # 四、收敛复述（锚点：创作资产走 F3201 Q 管线——收敛复述）
//!
//! 锚点要求复述 Q 域收敛红线：**消费域不各自加载资源，一律走 Q 管线**。R 域
//! 是消费域，因此创作资产（含用户自制主题/壁纸/组件）**必须走 F3201 六段管线**。
//! 本项用 [`ConvergenceLedger`] 固化：七类创作资产各声明它走管线的哪一段，
//! 绕管线的检出即 [`E_PIPELINE_BYPASS`] 阻断。这是**复述**不是另立标准——
//! 六段签名与段序归 F3201，本项只登记「本域不得绕」。
//!
//! # 五、前向义务：不许把待兑现项伪装成已完成
//!
//! 锚点写「创作无障碍双维（创作工具无障碍+创作内容无障碍——**F3611/F3650 前向**）」。
//! 「前向」意味着**本体在下游条目**，本项只能立**双维声明**与**待兑现位**。
//! 若把前向项标成已兑现，就是**用声明冒充实现**——比不声明更坏，因为它会让
//! 验收以为无障碍已覆盖。故 [`ForwardObligation`] 带 `settled` 位，
//! [`ForwardLedger::audit`] 对「声称已兑现但无证据条目号」报 [`E_FORWARD_FAKED`]。
//!
//! # 六、本项的边界（不越界施工，遵守「只做领到的任务」）
//!
//! VE-F3601 是**域开工与总架构**。它交付：域号跳段 ADR 与两套编号的显式裁决、
//! 同步更新的机检硬门、五板块与十项映射、四域分工表、收敛复述登记、禁扩面裁决、
//! 前向义务双维声明。它**不代做**后续 199 项的引擎本体，分工在册
//! （见 [`DOWNSTREAM_OWNERSHIP`]）：
//!
//! - F3602 拥有创作生态三层与五段签名**本体**；本项只登记「创作资产走 Q 管线」
//!   这条复述位与五段流程的**段名**，不写段的实现；
//! - F3603 拥有创作资产七要素模型**本体**（ID/类型/内容/元数据/版本/来源/许可）；
//!   本项只声明该能力属主是 R 域，不写字段定义；
//! - F3604 拥有创作工作流 DAG **本体**（复用 F3005 编排模式）；本项不写节点算法；
//! - F3606/F3613 拥有预览运行时与调试器本体；本项不写刷新与定位算法；
//! - F3607 拥有资产验证五段**本体**；本项不写校验规则；
//! - F3611/F3650 拥有创作无障碍双维**本体**（工具/内容）；本项只立双维**声明**
//!   与前向登记位；
//! - F3620 拥有 R01 组收口双签；本项自检是**开工级**，不代做组级收口。
//!
//! # 七、复杂度口径（锚点：登记 O(1)；映射 O(1)；分工 O(1)）
//!
//! 声明层：域号裁决/映射/分工/收敛登记全在建期固化，**帧路径零开销**。三个
//! O(1) 分别是：
//!
//! - **登记 O(1)**：[`AdrLedger::register`] 追加即得号（不重扫）；
//! - **映射 O(1)**：[`DomainNumberMap::band_of`] 按闭区间定位（段表定长
//!   [`BAND_COUNT`]=3），**不做「遍历全部条目找归属」**——那才是真的线性；
//! - **分工 O(1)**：[`DivisionTable::owner_of`] 按能力序号直接索引。
//!
//! 但诚实地说：机检侧 `check_sync` 是 O(段数)、`check_concern_coverage` 是
//! O(十项)、`audit_boards` 是 O(板块数 × 落点数)——它们**只在开工与收口时跑**，
//! 不在帧路径。这条区分写进 [`COMPLEXITY_DOC`]，不让「全是 O(1)」变成空话。

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与版本
// ---------------------------------------------------------------------------

/// 总纲版本。
pub const ARCH_VERSION: &str = "R01-arch-v1";
/// 段间接口冻结版本。
pub const INTERFACE_VERSION: &str = "R01-iface-v1";
/// 域号台账版本（跳段登记的台账版本号——台账与映射表同步更新的凭据）。
pub const LEDGER_VERSION: &str = "R01-ledger-v1";

/// 板块数（锚点：五板块）。
pub const BOARD_COUNT: usize = 5;
/// 映射项数（锚点：十项映射）。
pub const CONCERN_COUNT: usize = 10;
/// 分工域数（锚点：四域分工——E/R/P/S）。
pub const DIVISION_COUNT: usize = 4;
/// 判据项数（锚点判据六项）。
pub const CRITERION_COUNT: usize = 6;
/// 禁扩面条数。
pub const BOUNDARY_COUNT: usize = 7;
/// 收敛登记类目数（创作资产七类；本体归 F3603，本项只登记走管线）。
pub const CONVERGENCE_KINDS: usize = 7;
/// 前向义务上限。
pub const MAX_FORWARD: usize = 16;
/// ADR 上限。
pub const MAX_ADRS: usize = 32;
/// 冲突裁决上限（两套编号并存的裁决条目）。
pub const MAX_CONFLICTS: usize = 8;
/// 每板块映射项上限。
pub const MAX_ITEMS_PER_BOARD: usize = 4;
/// 分工能力上限。
pub const MAX_CAPABILITIES: usize = 32;
/// 号段表定长（三段：E 域二期预留 / R 创作生态 / S 无障碍渲染）。
///
/// 定长化才能兑现锚点「映射 O(1)」——否则查表长度随域数漂移，O(1) 只是话术。
pub const BAND_COUNT: usize = 3;

/// 复杂度口径（**把「建期跑」与「帧路径跑」分开报，不笼统报**）。
pub const COMPLEXITY_DOC: &str = "\
R 域为声明层：域号裁决/映射/分工/收敛登记全部在建期固化，帧路径零开销。
帧路径相关口径（锚点逐项）：
  登记 O(1)：ADR 追加即得号，不重扫既有记录；
  映射 O(1)：按号段闭区间定位（段表定长 3），不做遍历全条目查归属；
  分工 O(1)：按能力序号直接索引，不做线性查表。
机检侧例外（诚实标注）：check_sync O(段数=3)、check_concern_coverage O(十项)、
audit_boards O(板块数 × 落点数) —— 这些只在开工/收口时跑，不在帧路径。
把机检成本混进\"全是 O(1)\"是自欺：声明层的诚实是分开报，不是笼统报。";

// ---------------------------------------------------------------------------
// 二、错误码（全部 &'static str，零分配可比对）
// ---------------------------------------------------------------------------

/// 跳段未登记（锚点红线：跳段不登记=后续域混乱）。
pub const E_JUMP_UNREGISTERED: &str = "E_JUMP_UNREGISTERED";
/// 跳段冲突未裁决。
pub const E_CONFLICT_UNADJUDICATED: &str = "E_CONFLICT_UNADJUDICATED";
/// 台账与映射表不同步（同步更新红线）。
pub const E_SYNC_MISMATCH: &str = "E_SYNC_MISMATCH";
/// 号段重叠。
pub const E_BAND_OVERLAP: &str = "E_BAND_OVERLAP";
/// 号段无主（号不在任何已登记段内）。
pub const E_BAND_NO_OWNER: &str = "E_BAND_NO_OWNER";
/// 号段格式非法。
pub const E_BAND_MALFORMED: &str = "E_BAND_MALFORMED";
/// 号段区间倒置（起号 ≥ 止号）。
pub const E_BAND_INVERTED: &str = "E_BAND_INVERTED";
/// 号段表已满。
pub const E_BAND_CAP: &str = "E_BAND_CAP";
/// ADR 无标题。
pub const E_ADR_NO_TITLE: &str = "E_ADR_NO_TITLE";
/// ADR 无决策。
pub const E_ADR_NO_DECISION: &str = "E_ADR_NO_DECISION";
/// ADR 无否决记录。
pub const E_ADR_NO_REJECTED: &str = "E_ADR_NO_REJECTED";
/// ADR 无新版本。
pub const E_ADR_NO_TO_VERSION: &str = "E_ADR_NO_TO_VERSION";
/// ADR 升版矛盾（声明升版但新旧同号）。
pub const E_ADR_BUMP_NO_FROM: &str = "E_ADR_BUMP_NO_FROM";
/// ADR 账满。
pub const E_ADR_CAP: &str = "E_ADR_CAP";
/// 板块数不符。
pub const E_BOARD_COUNT: &str = "E_BOARD_COUNT";
/// 板块序错。
pub const E_BOARD_ORDER: &str = "E_BOARD_ORDER";
/// 板块码往返失败。
pub const E_BOARD_CODE_ROUNDTRIP: &str = "E_BOARD_CODE_ROUNDTRIP";
/// 映射项缺失。
pub const E_CONCERN_MISSING: &str = "E_CONCERN_MISSING";
/// 映射项落点为空。
pub const E_CONCERN_EMPTY: &str = "E_CONCERN_EMPTY";
/// 映射项落点格式非法。
pub const E_CONCERN_MALFORMED: &str = "E_CONCERN_MALFORMED";
/// 映射项板块错配。
pub const E_CONCERN_BOARD_MISMATCH: &str = "E_CONCERN_BOARD_MISMATCH";
/// 映射项落点溢出。
pub const E_CONCERN_OVERFLOW: &str = "E_CONCERN_OVERFLOW";
/// 映射项依据缺失（每项须写清「为什么是这一项」）。
pub const E_CONCERN_NO_BASIS: &str = "E_CONCERN_NO_BASIS";
/// 分工重复（两域都声称拥有同一能力）。
pub const E_DIVISION_DUPLICATE: &str = "E_DIVISION_DUPLICATE";
/// 分工无主（能力无人认领）。
pub const E_DIVISION_NO_OWNER: &str = "E_DIVISION_NO_OWNER";
/// 分工域不合法（超出 E/R/P/S 四域）。
pub const E_DIVISION_UNKNOWN_DOMAIN: &str = "E_DIVISION_UNKNOWN_DOMAIN";
/// 分工表溢出。
pub const E_DIVISION_CAP: &str = "E_DIVISION_CAP";
/// 分工与板块冲突（R 域板块的能力被判给了非 R 域）。
pub const E_DIVISION_BOARD_CONFLICT: &str = "E_DIVISION_BOARD_CONFLICT";
/// 分工条目五要素不全。
pub const E_DIVISION_INCOMPLETE: &str = "E_DIVISION_INCOMPLETE";
/// 绕管线私加载（收敛红线）。
pub const E_PIPELINE_BYPASS: &str = "E_PIPELINE_BYPASS";
/// 收敛登记缺类目。
pub const E_CONVERGENCE_KIND_MISSING: &str = "E_CONVERGENCE_KIND_MISSING";
/// 收敛登记段序错。
pub const E_CONVERGENCE_ORDER: &str = "E_CONVERGENCE_ORDER";
/// 收敛登记类目不匹配。
pub const E_CONVERGENCE_KIND_UNKNOWN: &str = "E_CONVERGENCE_KIND_UNKNOWN";
/// 收敛登记类目重复。
pub const E_CONVERGENCE_KIND_DUP: &str = "E_CONVERGENCE_KIND_DUP";
/// 收敛登记账满。
pub const E_CONVERGENCE_CAP: &str = "E_CONVERGENCE_CAP";
/// 前向义务无主责条目。
pub const E_FORWARD_NO_OWNER: &str = "E_FORWARD_NO_OWNER";
/// 前向义务伪装成已完成（红线：前向项不许假装兑现）。
pub const E_FORWARD_FAKED: &str = "E_FORWARD_FAKED";
/// 前向义务账满。
pub const E_FORWARD_CAP: &str = "E_FORWARD_CAP";
/// 禁扩面越界。
pub const E_BOUNDARY_OVERREACH: &str = "E_BOUNDARY_OVERREACH";
/// 能力无名。
pub const E_CAPABILITY_NO_NAME: &str = "E_CAPABILITY_NO_NAME";
/// 能力描述缺失。
pub const E_CAPABILITY_NO_DESC: &str = "E_CAPABILITY_NO_DESC";
/// 能力重复登记（同名能力两次入册）。
pub const E_CAPABILITY_DUP: &str = "E_CAPABILITY_DUP";

// ---------------------------------------------------------------------------
// 三、错误与严重度（五元组：错误必带「怎么办」与「找谁」）
// ---------------------------------------------------------------------------

/// R 域错误（五元组：码/现象/原因/下一步/责任方）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么（静态：现象文案是有限集）。
    pub what: &'static str,
    /// 为什么（动态：带具体号段/条目号才有排查价值）。
    pub why: String,
    /// 下一步（**必填**——拒绝必须给路）。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl CreationError {
    /// 构造（五元组齐发，构造点强制写全）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        CreationError {
            code,
            what,
            why: why.to_string(),
            next: next.to_string(),
            who: who.to_string(),
        }
    }

    /// 五元组齐备性（`next` 为空即不合格——拒绝不给路等于踢皮球）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.what.trim().is_empty()
            && !self.why.trim().is_empty()
            && !self.next.trim().is_empty()
            && !self.who.trim().is_empty()
    }

    /// 读屏可读完整错误（现象/原因/怎么办/找谁四要素齐发）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 阻断：不修不得开工。
    Blocking,
    /// 警告：可开工但须限期修。
    Warning,
}

impl Severity {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Severity::Blocking => "阻断",
            Severity::Warning => "警告",
        }
    }
}

/// 契约问题（五元组：码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub root_cause: String,
    /// 建议。
    pub advice: &'static str,
    /// 严重度。
    pub severity: Severity,
}

impl ContractIssue {
    /// 读屏单行（问题要能念给用户听——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "契约问题[{}·{}]：{}；根因 {}；建议 {}",
            self.severity.zh(),
            self.code,
            self.symptom,
            self.root_cause,
            self.advice
        )
    }
}
// ---------------------------------------------------------------------------
// 四、域号台账（锚点：域号跳段声明 ADR 的第一等公民）
// ---------------------------------------------------------------------------

/// 号段裁定权威（**谁说了算**——两套编号并存时必须记权威，否则各写各的）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BandAuthority {
    /// 册内条目锚点裁定（条目级锚点是最终权威）。
    Anchor,
    /// 开篇全域地图裁定（域号总表）。
    DomainMap,
}

impl BandAuthority {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BandAuthority::Anchor => "条目锚点",
            BandAuthority::DomainMap => "开篇域号总表",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            BandAuthority::Anchor => "R01-AUTH-ANCHOR",
            BandAuthority::DomainMap => "R01-AUTH-MAP",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<BandAuthority> {
        match code {
            "R01-AUTH-ANCHOR" => Some(BandAuthority::Anchor),
            "R01-AUTH-MAP" => Some(BandAuthority::DomainMap),
            _ => None,
        }
    }
}

/// 号段归属依据（**依据什么**——把「为什么这么裁」落成可核字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandBasis {
    /// 锚点「R 域条目号 F3601 起跳」。
    AnchorF3601Start,
    /// 册内 E01 批次段「VE-E 域开工（主题与个性化引擎，F3401-F3600）」。
    EBatchF3401ToE,
    /// 开篇域号总表 `VE-S | F3601-F3800 | 无障碍渲染`。
    MapS3601To3800,
    /// 册内「VE-F3801 · S 域开工与无障碍渲染总架构」。
    MapS3801Start,
}

impl BandBasis {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BandBasis::AnchorF3601Start => "锚点：R 域条目号 F3601 起跳",
            BandBasis::EBatchF3401ToE => "册内 E01 批次段：F3401-F3600 判为 E 域",
            BandBasis::MapS3601To3800 => "开篇域号总表：F3601-F3800 记为 S 域",
            BandBasis::MapS3801Start => "册内 F3801 起 S 域开工",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            BandBasis::AnchorF3601Start => "R01-BASIS-ANCHOR3601",
            BandBasis::EBatchF3401ToE => "R01-BASIS-E3401",
            BandBasis::MapS3601To3800 => "R01-BASIS-MAPS",
            BandBasis::MapS3801Start => "R01-BASIS-S3801",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<BandBasis> {
        match code {
            "R01-BASIS-ANCHOR3601" => Some(BandBasis::AnchorF3601Start),
            "R01-BASIS-E3401" => Some(BandBasis::EBatchF3401ToE),
            "R01-BASIS-MAPS" => Some(BandBasis::MapS3601To3800),
            "R01-BASIS-S3801" => Some(BandBasis::MapS3801Start),
            _ => None,
        }
    }
}

/// 号段裁决状态（**两套编号撞在同一段号时怎么落**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandVerdict {
    /// 锚点优先：条目级锚点覆盖开篇总表。
    AnchorWins,
    /// 已查清并存不矛盾（总表记的是旧名，新锚点换了属主但号段未变）。
    Reconciled,
    /// 未裁决（**红线状态**：不许开工）。
    Unadjudicated,
}

impl BandVerdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BandVerdict::AnchorWins => "锚点优先",
            BandVerdict::Reconciled => "已查清并存不矛盾",
            BandVerdict::Unadjudicated => "未裁决",
        }
    }

    /// 是否为红线条目（未裁决即阻断）。
    pub fn is_red(self) -> bool {
        matches!(self, BandVerdict::Unadjudicated)
    }
}

/// 号段（闭区间 `[lo, hi]`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberBand {
    /// 归属域。
    pub domain: DivisionDomain,
    /// 起号（含）。
    pub lo: u32,
    /// 止号（含）。
    pub hi: u32,
    /// 裁定权威。
    pub authority: BandAuthority,
    /// 归属依据。
    pub basis: BandBasis,
    /// 裁决状态。
    pub verdict: BandVerdict,
}

impl NumberBand {
    /// 新建号段（校验失败返回错误，不静默接受倒置区间）。
    ///
    /// 跨度上限取 1000：单域 200 项，两倍余量留给「总表与锚点各占一段」的
    /// 合并写法；超了几乎必是有人把整册当一段塞进来。
    pub fn new(
        domain: DivisionDomain,
        lo: u32,
        hi: u32,
        authority: BandAuthority,
        basis: BandBasis,
        verdict: BandVerdict,
    ) -> Result<NumberBand, CreationError> {
        if lo >= hi {
            return Err(CreationError::new(
                E_BAND_INVERTED,
                "号段登记被拒：区间倒置",
                &format!("号段 {}-{}：起号 {} 不小于止号 {}", lo, hi, lo, hi),
                "闭区间要求 lo < hi；单条目号段请用相邻两号表达（如 3801-3802）",
                "R 域域号台账维护方",
            ));
        }
        if (hi - lo) > 1000 {
            return Err(CreationError::new(
                E_BAND_MALFORMED,
                "号段登记被拒：跨度异常",
                &format!(
                    "号段 {}-{} 跨度 {}，超过单域 200 项的两倍余量",
                    lo,
                    hi,
                    hi - lo
                ),
                "拆成多段登记；单域不应超过 200 项号段",
                "R 域域号台账维护方",
            ));
        }
        Ok(NumberBand {
            domain,
            lo,
            hi,
            authority,
            basis,
            verdict,
        })
    }

    /// 是否含号（闭区间）。
    pub fn contains(&self, item: u32) -> bool {
        item >= self.lo && item <= self.hi
    }

    /// 跨度（项数）。
    pub fn span(&self) -> u32 {
        self.hi - self.lo + 1
    }

    /// 与另一段是否重叠（复杂度 O(1)：闭区间相交一次比较）。
    pub fn overlaps(&self, other: &NumberBand) -> bool {
        self.lo <= other.hi && other.lo <= self.hi
    }

    /// 裁决是否已落地（未裁决即红线条目）。
    pub fn is_settled(&self) -> bool {
        !self.verdict.is_red()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "号段 F{}-F{} 归{}（裁定权威：{}；依据：{}；裁决：{}）",
            self.lo,
            self.hi,
            self.domain.zh(),
            self.authority.zh(),
            self.basis.zh(),
            self.verdict.zh()
        )
    }
}

/// 域号表（[`NumberBand`] 的有序集合，定长 [`BAND_COUNT`]）。
#[derive(Clone, Debug)]
pub struct DomainNumberMap {
    bands: Vec<NumberBand>,
}

impl DomainNumberMap {
    /// 空号表。
    pub fn new() -> Self {
        DomainNumberMap { bands: Vec::new() }
    }

    /// 登记号段（拒绝重叠、倒置与超容）。
    pub fn register(&mut self, band: NumberBand) -> Result<usize, CreationError> {
        if self.bands.len() >= BAND_COUNT {
            return Err(CreationError::new(
                E_BAND_CAP,
                "号段登记被拒：号段表已满",
                &format!("号段表 {} 段达到上限 {}", self.bands.len(), BAND_COUNT),
                "先核对是否重复登记；确需扩容再提升 BAND_COUNT",
                "R 域域号台账维护方",
            ));
        }
        for b in self.bands.iter() {
            if b.overlaps(&band) {
                return Err(CreationError::new(
                    E_BAND_OVERLAP,
                    "号段登记被拒：与已登记段重叠",
                    &format!(
                        "新区间 F{}-F{} 与{} 域 F{}-F{} 重叠",
                        band.lo,
                        band.hi,
                        b.domain.zh(),
                        b.lo,
                        b.hi
                    ),
                    "两套编号并存时用 BandVerdict 显式裁决，不要靠重叠号段表达分歧",
                    "R 域域号台账维护方",
                ));
            }
        }
        // 保持按 lo 升序——查表与对账都依赖有序。
        let mut idx = 0usize;
        while idx < self.bands.len() && self.bands[idx].lo < band.lo {
            idx += 1;
        }
        self.bands.insert(idx, band);
        Ok(idx)
    }

    /// 号段数。
    pub fn len(&self) -> usize {
        self.bands.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &NumberBand> {
        self.bands.iter()
    }

    /// 查号归属（复杂度 O(段数)，段表定长 3 → 常数级）。
    ///
    /// 返回 `Ok(None)` 表示**查无此号**（不在任何段内），与「查到了但被裁决
    /// 拦住」区分开：前者是号段表缺漏，后者是红线未清。两种故障混成一个
    /// `None` 会让缺漏被当成"红线未清"，整改方向就错了。
    pub fn band_of(&self, item: u32) -> Result<Option<&NumberBand>, CreationError> {
        for b in self.bands.iter() {
            if b.contains(item) {
                if b.verdict.is_red() {
                    return Err(CreationError::new(
                        E_CONFLICT_UNADJUDICATED,
                        "号归属查询被拒：该段裁决未落地",
                        &format!("F{} 落在未裁决号段 F{}-F{}", item, b.lo, b.hi),
                        "先在 JumpLedger::record_conflict 登记裁决与理由，再开工",
                        "R 域域号台账维护方",
                    ));
                }
                return Ok(Some(b));
            }
        }
        Ok(None)
    }

    /// 查号归属的域（查无或被拦均返回错误——不静默返回「无人负责」）。
    pub fn domain_of(&self, item: u32) -> Result<DivisionDomain, CreationError> {
        match self.band_of(item)? {
            Some(b) => Ok(b.domain),
            None => Err(CreationError::new(
                E_BAND_NO_OWNER,
                "号归属查询被拒：查无此号",
                &format!("F{} 不属于任何已登记号段", item),
                "补登该号所在号段；号段表外的条目号意味着跳段未登记",
                "R 域域号台账维护方",
            )),
        }
    }

    /// R 域起始号（锚点：R 域条目号 F3601 起跳）。
    pub fn r_start(&self) -> Result<u32, CreationError> {
        for b in self.bands.iter() {
            if b.domain == DivisionDomain::Creation && b.is_settled() {
                return Ok(b.lo);
            }
        }
        Err(CreationError::new(
            E_JUMP_UNREGISTERED,
            "R 域起点未登记",
            "号段表里没有已裁决的 R（创作生态）域号段",
            "按锚点「R 域条目号 F3601 起跳」登记 F3601-F3800 段",
            "R 域域号台账维护方",
        ))
    }

    /// 同步核对（见 [`SyncReport`]）。
    pub fn check_sync(&self, board: &MappingTable) -> SyncReport {
        SyncReport::compute(self, board)
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!("域号台账：{} 段。", self.bands.len());
        for b in self.bands.iter() {
            s.push_str(&format!("{} ", b.screen_line()));
        }
        s.push_str(&format!(
            "未裁决段 {} 条（红线须清零）。",
            self.bands.iter().filter(|b| !b.is_settled()).count()
        ));
        s
    }
}

impl Default for DomainNumberMap {
    fn default() -> Self {
        Self::new()
    }
}

/// 同步核对报告（**同步更新红线的可机检凭据**）。
#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    /// 台账侧 R 起始号（0 表示未登记）。
    pub ledger_r_start: u32,
    /// 台账侧 R 段止号。
    pub ledger_r_end: u32,
    /// 映射表侧期望起始号（锚点常量）。
    pub board_r_start: u32,
    /// 映射表落点覆盖的最大条目号。
    pub board_max_item: u32,
    /// 映射表落点覆盖的最小条目号（0 表示无有效落点）。
    pub board_min_item: u32,
    /// 落点总条数。
    pub item_count: usize,
    /// 越出台账 R 段的落点条目号（**逐个列出，不只报"有越段"**）。
    pub out_of_band: Vec<u32>,
    /// 未裁决号段数。
    pub unsettled_bands: usize,
}

impl SyncReport {
    /// 逐项核对（O(段数 + 落点数)，只在开工/收口跑）。
    pub fn compute(map: &DomainNumberMap, board: &MappingTable) -> SyncReport {
        let mut rep = SyncReport {
            unsettled_bands: map.bands.iter().filter(|b| !b.is_settled()).count(),
            ..SyncReport::default()
        };
        for b in map.iter() {
            if b.domain == DivisionDomain::Creation && b.is_settled() {
                rep.ledger_r_start = b.lo;
                rep.ledger_r_end = b.hi;
                break;
            }
        }
        rep.board_r_start = CreationBoard::R_ANCHOR_START;
        let mut max_item = 0u32;
        let mut min_item = u32::MAX;
        let mut n = 0usize;
        let lo = rep.ledger_r_start;
        let hi = rep.ledger_r_end;
        for c in CreationConcern::ALL.iter() {
            for it in board.items_of(*c) {
                if let Some(num) = parse_item_num(&it) {
                    if num > max_item {
                        max_item = num;
                    }
                    if num < min_item {
                        min_item = num;
                    }
                    n += 1;
                    // 段边界已知（lo>0）时逐个核对越段落点。
                    // 只看max 会漏掉「多数落在段内、少数越段」的情形——
                    // 那正是同步红线最该抓的情况：有人把某一项挂到 E 域段上，
                    // 而其余九项仍正常，max 看不出问题。
                    if lo > 0 && hi > 0 && (num < lo || num > hi) {
                        rep.out_of_band.push(num);
                    }
                }
            }
        }
        rep.board_max_item = max_item;
        rep.board_min_item = if min_item == u32::MAX { 0 } else { min_item };
        rep.item_count = n;
        rep
    }

    /// 是否同步（起点一致 + 未裁决清零 + 落点非空 + 落点落在台账段内）。
    pub fn is_synced(&self) -> bool {
        self.desync_reason().is_none()
    }

    /// 不同步的原因（`None` 表示同步）。
    ///
    /// **顺序有意**：先报红线（未裁决），再报缺登记，再报起点不符——
    /// 因为红线的整改动作与其他项不同（要的是裁决，不是改号）。
    pub fn desync_reason(&self) -> Option<&'static str> {
        if self.unsettled_bands > 0 {
            return Some("台账存在未裁决号段——冲突未落地即不许开工");
        }
        if self.ledger_r_start == 0 {
            return Some("台账未登记 R 域起始号");
        }
        if self.ledger_r_start != self.board_r_start {
            return Some("台账 R 起始号与映射表期望起点不一致");
        }
        if self.item_count == 0 {
            return Some("映射表无落点——空表与台账不可能同步");
        }
        // 越段落点**逐个报出**（不止"有越段"）——只报布尔会让人不知道是哪几项。
        if !self.out_of_band.is_empty() {
            return Some("映射表存在越出台账 R 段的落点");
        }
        None
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        match self.desync_reason() {
            None => format!(
                "台账与映射表同步：R 起点 F{}，段止 F{}，落点 {} 条（最大 F{}）",
                self.ledger_r_start, self.ledger_r_end, self.item_count, self.board_max_item
            ),
            Some(r) => format!(
                "台账与映射表不同步：{}；台账起点 F{}，落点 {} 条（范围 F{}-F{}，越段 {:?}）",
                r,
                self.ledger_r_start,
                self.item_count,
                self.board_min_item,
                self.board_max_item,
                self.out_of_band
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、跳段裁决账（锚点：跳段未登记→阻断开工·红线实测）
// ---------------------------------------------------------------------------

/// 冲突裁决记录（**一条冲突一记录，不合并**——合并就等于把分歧藏起来）。
#[derive(Clone, Debug)]
pub struct ConflictRecord {
    /// 冲突号（单调）。
    pub id: u64,
    /// 争议号段起号。
    pub lo: u32,
    /// 争议号段止号。
    pub hi: u32,
    /// 开篇总表的说法。
    pub map_claim: String,
    /// 条目锚点的说法。
    pub anchor_claim: String,
    /// 裁决结论。
    pub verdict: BandVerdict,
    /// 裁决理由（**必填**：无理由的裁决等于没裁决）。
    pub reason: String,
    /// 同步动作（裁决后要改哪两处——锚点：台账与映射表同步更新）。
    pub sync_action: String,
}

impl ConflictRecord {
    /// 四要素齐备性（理由与同步动作缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.map_claim.trim().is_empty()
            && !self.anchor_claim.trim().is_empty()
            && !self.reason.trim().is_empty()
            && !self.sync_action.trim().is_empty()
    }

    /// 是否为红线条目（未裁决）。
    pub fn is_red(&self) -> bool {
        self.verdict.is_red()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "冲突 #{} F{}-F{}：总表称「{}」，锚点称「{}」；裁决 {}；理由 {}；同步 {}",
            self.id,
            self.lo,
            self.hi,
            self.map_claim,
            self.anchor_claim,
            self.verdict.zh(),
            self.reason,
            self.sync_action
        )
    }
}

/// 跳段裁决账。
#[derive(Clone, Debug)]
pub struct JumpLedger {
    /// 域号表。
    pub map: DomainNumberMap,
    conflicts: Vec<ConflictRecord>,
    next_id: u64,
}

impl JumpLedger {
    /// 空账。
    pub fn new() -> Self {
        JumpLedger {
            map: DomainNumberMap::new(),
            conflicts: Vec::new(),
            next_id: 1,
        }
    }

    /// 登记号段（转发 [`DomainNumberMap::register`]）。
    pub fn register_band(&mut self, band: NumberBand) -> Result<usize, CreationError> {
        self.map.register(band)
    }

    /// 登记一条冲突裁决（四项拒绝：两套说法缺一 / 理由空 / 同步动作空 / 账满）。
    ///
    /// 注意**未裁决也允许登记**——红线条目必须能表达，否则就只剩「不写」，
    /// 而「不写」会被 [`JumpLedger::preflight`] 单独捞出来当缺项报，
    /// 反而看不出「这里本来有争议」。
    pub fn record_conflict(&mut self, mut rec: ConflictRecord) -> Result<u64, CreationError> {
        if rec.map_claim.trim().is_empty() || rec.anchor_claim.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_DECISION,
                "冲突登记被拒：两套说法缺一",
                &format!(
                    "冲突 F{}-F{}：总表说法「{}」锚点说法「{}」",
                    rec.lo, rec.hi, rec.map_claim, rec.anchor_claim
                ),
                "两套编号都要写出来——只写一套等于把另一套信息丢掉",
                "R 域域号台账维护方",
            ));
        }
        if rec.reason.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_REJECTED,
                "冲突登记被拒：无裁决理由",
                &format!("冲突 F{}-F{} 没有写裁决理由", rec.lo, rec.hi),
                "写清为什么这么裁（条目锚点为何优先），否则后来人会重提旧议",
                "R 域域号台账维护方",
            ));
        }
        if rec.sync_action.trim().is_empty() {
            return Err(CreationError::new(
                E_SYNC_MISMATCH,
                "冲突登记被拒：无同步动作",
                &format!("冲突 F{}-F{} 裁了但没写要同步改哪两处", rec.lo, rec.hi),
                "写明台账与映射表各自要怎么改——锚点：跳段不登记=后续域混乱",
                "R 域域号台账维护方",
            ));
        }
        if self.conflicts.len() >= MAX_CONFLICTS {
            return Err(CreationError::new(
                E_ADR_CAP,
                "冲突登记被拒：裁决账已满",
                &format!("裁决账 {} 条达到上限 {}", self.conflicts.len(), MAX_CONFLICTS),
                "先归档旧裁决，或按需提升 MAX_CONFLICTS",
                "R 域域号台账维护方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        rec.id = id;
        self.conflicts.push(rec);
        Ok(id)
    }

    /// 冲突总数。
    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// 未裁决冲突数（**红线：须为 0 才许开工**）。
    pub fn unadjudicated_count(&self) -> usize {
        self.conflicts.iter().filter(|c| c.is_red()).count()
    }

    /// 未裁决号段数。
    pub fn unsettled_band_count(&self) -> usize {
        self.map.bands.iter().filter(|b| !b.is_settled()).count()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ConflictRecord> {
        self.conflicts.iter()
    }

    /// 不完整裁决数（自检用）。
    pub fn incomplete_count(&self) -> usize {
        self.conflicts.iter().filter(|c| !c.is_complete()).count()
    }

    /// 开工前置闸（复杂度 O(段数 + 冲突数)，**只在开工时跑**）。
    ///
    /// 四条判定：**未裁决段 → 未裁决冲突 → R 起点已登记 → R 起点符锚点**。
    ///
    /// **判定顺序有讲究**：未裁决必须排在 `r_start()` 之前。因为
    /// [`DomainNumberMap::r_start`] 只承认**已裁决**的 R 段，若把顺序倒过来，
    /// 「登记了 R 段但忘了裁决」会报成 `E_JUMP_UNREGISTERED`（看起来像压根没
    /// 登记），而整改动作其实是「补裁决」。报错要指向真正的病因，否则
    /// 处理的人会去重登号段——重登并不能让未裁决的段变成已裁决。
    pub fn preflight(&self) -> Result<(), CreationError> {
        let unsettled_bands = self.unsettled_band_count();
        if unsettled_bands > 0 {
            return Err(CreationError::new(
                E_CONFLICT_UNADJUDICATED,
                "开工前置未过：存在未裁决号段",
                &format!("号段表有 {} 段未裁决", unsettled_bands),
                "逐段登记裁决与理由（JumpLedger::record_conflict），清零后再开工",
                "R 域域号台账维护方",
            ));
        }
        let r_start = self.map.r_start()?;
        if r_start != CreationBoard::R_ANCHOR_START {
            return Err(CreationError::new(
                E_JUMP_UNREGISTERED,
                "开工前置未过：R 域起点与锚点不符",
                &format!(
                    "台账 R 起点 F{}，锚点要求 F{}",
                    r_start,
                    CreationBoard::R_ANCHOR_START
                ),
                "按锚点「R 域条目号 F3601 起跳」改正号段起点",
                "R 域域号台账维护方",
            ));
        }
        let unadj = self.unadjudicated_count();
        if unadj > 0 {
            return Err(CreationError::new(
                E_CONFLICT_UNADJUDICATED,
                "开工前置未过：存在未裁决冲突",
                &format!("裁决账有 {} 条标为未裁决", unadj),
                "锚点红线：跳段不登记=后续域混乱。未裁决冲突须先落地",
                "R 域域号台账维护方",
            ));
        }
        Ok(())
    }

    /// 同步核对。
    pub fn sync_report(&self, board: &MappingTable) -> SyncReport {
        self.map.check_sync(board)
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "跳段裁决账：{} 条（未裁决 {}）；",
            self.conflict_count(),
            self.unadjudicated_count()
        );
        for c in self.conflicts.iter() {
            s.push_str(&format!("{} ", c.screen_line()));
        }
        s.push_str(&format!("{} ", self.map.screen_text()));
        s
    }
}

impl Default for JumpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、ADR 登记册（锚点：ADR 登记）
// ---------------------------------------------------------------------------

/// ADR 关联范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdrScope {
    /// 域号跳段（域号 ADR）。
    NumberJump,
    /// 五板块架构。
    Boards,
    /// 四域分工。
    Division,
    /// 收敛复述。
    Convergence,
}

impl AdrScope {
    /// 全部 scope（顺序即 rank）。
    pub const ALL: [AdrScope; 4] = [
        AdrScope::NumberJump,
        AdrScope::Boards,
        AdrScope::Division,
        AdrScope::Convergence,
    ];

    /// 序号（单源派生——`ALL` 的下标即 rank）。
    pub fn rank(self) -> u8 {
        match self {
            AdrScope::NumberJump => 0,
            AdrScope::Boards => 1,
            AdrScope::Division => 2,
            AdrScope::Convergence => 3,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AdrScope::NumberJump => "域号跳段",
            AdrScope::Boards => "五板块架构",
            AdrScope::Division => "四域分工",
            AdrScope::Convergence => "收敛复述",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            AdrScope::NumberJump => "R01-S0",
            AdrScope::Boards => "R01-S1",
            AdrScope::Division => "R01-S2",
            AdrScope::Convergence => "R01-S3",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<AdrScope> {
        match code {
            "R01-S0" => Some(AdrScope::NumberJump),
            "R01-S1" => Some(AdrScope::Boards),
            "R01-S2" => Some(AdrScope::Division),
            "R01-S3" => Some(AdrScope::Convergence),
            _ => None,
        }
    }
}

/// ADR 范围序（单源）。
pub const ADR_SCOPE_ORDER: [AdrScope; 4] = AdrScope::ALL;

/// ADR 记录。
#[derive(Clone, Debug)]
pub struct AdrRecord {
    /// ADR 号（单调）。
    pub id: u64,
    /// 关联范围。
    pub scope: AdrScope,
    /// 决策标题。
    pub title: String,
    /// 决策内容（为什么这么定）。
    pub decision: String,
    /// 被否决方案与否决理由（**必填**：不写否决理由的 ADR 无法复核）。
    pub rejected: String,
    /// 旧版本（新建为空串）。
    pub from_version: String,
    /// 新版本。
    pub to_version: String,
    /// 逻辑 tick。
    pub tick: u64,
}

impl AdrRecord {
    /// 四要素齐备性自检（缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.title.trim().is_empty()
            && !self.decision.trim().is_empty()
            && !self.rejected.trim().is_empty()
            && !self.to_version.trim().is_empty()
    }

    /// 是否为升版记录。
    pub fn is_version_bump(&self) -> bool {
        !self.from_version.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "ADR #{}（{} · {}→{}）：{}；决策 {}；否决 {}",
            self.id,
            self.scope.zh(),
            if self.from_version.is_empty() {
                "新建"
            } else {
                self.from_version.as_str()
            },
            self.to_version,
            self.title,
            self.decision,
            self.rejected
        )
    }
}

/// ADR 账。
#[derive(Clone, Debug)]
pub struct AdrLedger {
    records: Vec<AdrRecord>,
    next_id: u64,
}

impl AdrLedger {
    /// 空账。
    pub fn new() -> Self {
        AdrLedger {
            records: Vec::new(),
            next_id: 1,
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &AdrRecord> {
        self.records.iter()
    }

    /// 登记一条 ADR（复杂度 **O(1)**：追加即得号，不重扫）。
    ///
    /// 五项拒绝：标题/决策/否决理由/新版本为空；声明升版但新旧同号；账满。
    pub fn register(&mut self, mut rec: AdrRecord) -> Result<u64, CreationError> {
        if rec.title.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_TITLE,
                "ADR 登记被拒：无标题",
                "无标题的 ADR 无法被检索，等于把决策藏起来",
                "写清决策标题（改了什么）",
                "R 域架构维护方",
            ));
        }
        if rec.decision.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_DECISION,
                "ADR 登记被拒：无决策内容",
                &format!("ADR「{}」没写决策内容，后来人无法判断实现是否符合本决策", rec.title),
                "写清决策内容（为什么这么定）",
                "R 域架构维护方",
            ));
        }
        if rec.rejected.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_REJECTED,
                "ADR 登记被拒：无否决记录",
                &format!(
                    "ADR「{}」没写否决方案与理由；不写否决理由的 ADR 无法复核，\
                     半年后有人会把否决方案再提一遍",
                    rec.title
                ),
                "写清被否决的方案与否决理由",
                "R 域架构维护方",
            ));
        }
        if rec.to_version.trim().is_empty() {
            return Err(CreationError::new(
                E_ADR_NO_TO_VERSION,
                "ADR 登记被拒：无新版本号",
                &format!("ADR「{}」没写新接口版本，下游无从判断该按哪版编", rec.title),
                "填入新接口版本号（如 R01-iface-v2）",
                "R 域架构维护方",
            ));
        }
        if !rec.from_version.trim().is_empty() && rec.from_version == rec.to_version {
            rec.from_version = String::new();
            return Err(CreationError::new(
                E_ADR_BUMP_NO_FROM,
                "ADR 登记被拒：升版信息矛盾",
                &format!(
                    "ADR「{}」声明从 {} 升到 {}，新旧版本相同",
                    rec.title, rec.from_version, rec.to_version
                ),
                "若确为新建则清空旧版本；若确为升版则填不同的新版本号",
                "R 域架构维护方",
            ));
        }
        if self.records.len() >= MAX_ADRS {
            return Err(CreationError::new(
                E_ADR_CAP,
                "ADR 登记被拒：账已满",
                &format!("ADR 账 {} 条达到上限 {}", self.records.len(), MAX_ADRS),
                "先归档旧 ADR，或按需提升 MAX_ADRS",
                "R 域架构维护方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        rec.id = id;
        self.records.push(rec);
        Ok(id)
    }

    /// 某范围的 ADR 数。
    pub fn count_for(&self, scope: AdrScope) -> usize {
        self.records.iter().filter(|r| r.scope == scope).count()
    }

    /// 不完整 ADR 数。
    pub fn incomplete_count(&self) -> usize {
        self.records.iter().filter(|r| !r.is_complete()).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "ADR 账：{} 条（跳段 {} / 板块 {} / 分工 {} / 收敛 {}），不完整 {} 条。",
            self.len(),
            self.count_for(AdrScope::NumberJump),
            self.count_for(AdrScope::Boards),
            self.count_for(AdrScope::Division),
            self.count_for(AdrScope::Convergence),
            self.incomplete_count()
        );
        for r in self.records.iter() {
            s.push_str(&format!("{} ", r.screen_line()));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 七、五板块架构（锚点：创作资产模型/工作流/工作台/编辑器/工坊）
// ---------------------------------------------------------------------------

/// 创作生态五板块。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CreationBoard {
    /// 板块一：创作资产模型（资产七要素与类型注册的内型）。
    AssetModel,
    /// 板块二：工作流（创作任务编排与断点续作）。
    Workflow,
    /// 板块三：工作台（预览运行时与调试器）。
    Workbench,
    /// 板块四：编辑器（主题编辑器与其无障碍）。
    Editor,
    /// 板块五：工坊（壁纸与图标两大视觉创作面）。
    Workshop,
}

impl CreationBoard {
    /// 锚点写死的 R 域起始号（**单一事实源**：台账与映射表都以此为准）。
    ///
    /// 放在枚举上而非映射表上，因为它是**域的号段事实**而不是表的属性——
    /// 放错位置会让人以为改表就能改号段。
    pub const R_ANCHOR_START: u32 = 3601;

    /// 全部板块（顺序即 rank，锚点逐个点名的五项）。
    pub const ALL: [CreationBoard; 5] = [
        CreationBoard::AssetModel,
        CreationBoard::Workflow,
        CreationBoard::Workbench,
        CreationBoard::Editor,
        CreationBoard::Workshop,
    ];

    /// 序号（单源派生）。
    pub fn rank(self) -> u8 {
        match self {
            CreationBoard::AssetModel => 0,
            CreationBoard::Workflow => 1,
            CreationBoard::Workbench => 2,
            CreationBoard::Editor => 3,
            CreationBoard::Workshop => 4,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CreationBoard::AssetModel => "创作资产模型",
            CreationBoard::Workflow => "工作流",
            CreationBoard::Workbench => "工作台",
            CreationBoard::Editor => "编辑器",
            CreationBoard::Workshop => "工坊",
        }
    }

    /// 一句话职责（**职责边界写清，才不会下游互相抢活**）。
    pub fn duty(self) -> &'static str {
        match self {
            CreationBoard::AssetModel => "定义创作资产的内型：七要素模型与七类注册",
            CreationBoard::Workflow => "编排创作任务：步骤 DAG 与断点续作",
            CreationBoard::Workbench => "承载创作期反馈：所见即所得预览与问题定位",
            CreationBoard::Editor => "主题创作界面本体：可视化编辑与其无障碍",
            CreationBoard::Workshop => "视觉创作面：壁纸与图标两大工坊",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            CreationBoard::AssetModel => "R01-B0",
            CreationBoard::Workflow => "R01-B1",
            CreationBoard::Workbench => "R01-B2",
            CreationBoard::Editor => "R01-B3",
            CreationBoard::Workshop => "R01-B4",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<CreationBoard> {
        match code {
            "R01-B0" => Some(CreationBoard::AssetModel),
            "R01-B1" => Some(CreationBoard::Workflow),
            "R01-B2" => Some(CreationBoard::Workbench),
            "R01-B3" => Some(CreationBoard::Editor),
            "R01-B4" => Some(CreationBoard::Workshop),
            _ => None,
        }
    }

    /// 本板块的映射项（每板块恰两项——十项映射的取法见头注§2）。
    pub fn concerns(self) -> [CreationConcern; 2] {
        match self {
            CreationBoard::AssetModel => [CreationConcern::AssetModel, CreationConcern::AssetIO],
            CreationBoard::Workflow => [CreationConcern::Workflow, CreationConcern::ValidatePublish],
            CreationBoard::Workbench => [CreationConcern::Preview, CreationConcern::Debug],
            CreationBoard::Editor => [CreationConcern::Editor, CreationConcern::EditorA11y],
            CreationBoard::Workshop => [CreationConcern::Wallpaper, CreationConcern::Icon],
        }
    }
}

/// 板块序（单源）。
pub const BOARD_ORDER: [CreationBoard; BOARD_COUNT] = CreationBoard::ALL;

// ---------------------------------------------------------------------------
// 八、十项映射（锚点：五板块十项映射）
// ---------------------------------------------------------------------------

/// 创作映射项（十项——锚点未逐项点名，取法见头注 §2，每项须带依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CreationConcern {
    /// 1 资产模型本体（F3603）。
    AssetModel,
    /// 2 资产导入导出（F3648）。
    AssetIO,
    /// 3 工作流引擎（F3604）。
    Workflow,
    /// 4 验证与发布（F3627）。
    ValidatePublish,
    /// 5 预览运行时（F3606）。
    Preview,
    /// 6 调试器（F3613）。
    Debug,
    /// 7 主题编辑器（F3621）。
    Editor,
    /// 8 编辑器无障碍（F3630）。
    EditorA11y,
    /// 9 壁纸工坊（F3641）。
    Wallpaper,
    /// 10 图标工坊（F3645）。
    Icon,
}

impl CreationConcern {
    /// 全部映射项（顺序即 rank，**恰十项**）。
    pub const ALL: [CreationConcern; CONCERN_COUNT] = [
        CreationConcern::AssetModel,
        CreationConcern::AssetIO,
        CreationConcern::Workflow,
        CreationConcern::ValidatePublish,
        CreationConcern::Preview,
        CreationConcern::Debug,
        CreationConcern::Editor,
        CreationConcern::EditorA11y,
        CreationConcern::Wallpaper,
        CreationConcern::Icon,
    ];

    /// 序号（单源派生）。
    pub fn rank(self) -> u8 {
        match self {
            CreationConcern::AssetModel => 0,
            CreationConcern::AssetIO => 1,
            CreationConcern::Workflow => 2,
            CreationConcern::ValidatePublish => 3,
            CreationConcern::Preview => 4,
            CreationConcern::Debug => 5,
            CreationConcern::Editor => 6,
            CreationConcern::EditorA11y => 7,
            CreationConcern::Wallpaper => 8,
            CreationConcern::Icon => 9,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CreationConcern::AssetModel => "资产模型本体",
            CreationConcern::AssetIO => "资产导入导出",
            CreationConcern::Workflow => "工作流引擎",
            CreationConcern::ValidatePublish => "验证与发布",
            CreationConcern::Preview => "预览运行时",
            CreationConcern::Debug => "创作调试器",
            CreationConcern::Editor => "主题编辑器",
            CreationConcern::EditorA11y => "编辑器无障碍",
            CreationConcern::Wallpaper => "壁纸工坊",
            CreationConcern::Icon => "图标工坊",
        }
    }

    /// 所属板块。
    pub fn board(self) -> CreationBoard {
        match self {
            CreationConcern::AssetModel | CreationConcern::AssetIO => CreationBoard::AssetModel,
            CreationConcern::Workflow | CreationConcern::ValidatePublish => CreationBoard::Workflow,
            CreationConcern::Preview | CreationConcern::Debug => CreationBoard::Workbench,
            CreationConcern::Editor | CreationConcern::EditorA11y => CreationBoard::Editor,
            CreationConcern::Wallpaper | CreationConcern::Icon => CreationBoard::Workshop,
        }
    }

    /// 册内落点条目号（**必须是真实存在的条目号**——机检逐项校验格式与号段）。
    pub fn landing(self) -> &'static str {
        match self {
            CreationConcern::AssetModel => "VE-F3603",
            CreationConcern::AssetIO => "VE-F3648",
            CreationConcern::Workflow => "VE-F3604",
            CreationConcern::ValidatePublish => "VE-F3627",
            CreationConcern::Preview => "VE-F3606",
            CreationConcern::Debug => "VE-F3613",
            CreationConcern::Editor => "VE-F3621",
            CreationConcern::EditorA11y => "VE-F3630",
            CreationConcern::Wallpaper => "VE-F3641",
            CreationConcern::Icon => "VE-F3645",
        }
    }

    /// 取这一项的依据（**必填**：无依据的映射项等于凑数）。
    pub fn basis(self) -> &'static str {
        match self {
            CreationConcern::AssetModel => "模型是内型：七要素与七类注册不立，第三方资产无处安放",
            CreationConcern::AssetIO => "迁出迁入是外形：只立模型则资产进不来也出不去",
            CreationConcern::Workflow => "编排与断点续作是创作长任务的骨架，缺它则创作不可中断",
            CreationConcern::ValidatePublish => "把关与编排是两种失败模式：编排错要重排，把关错要拦住",
            CreationConcern::Preview => "所见即所得是创作信任基础，预览与实况分歧会直接毁掉信任",
            CreationConcern::Debug => "定位精度是调试器立身之本，只报错不定位等于失职",
            CreationConcern::Editor => "编辑器是 R 域面向用户的主界面本体",
            CreationConcern::EditorA11y => "工具本身要能被无障碍用户操作，否则无障碍只是产物侧的事",
            CreationConcern::Wallpaper => "壁纸是最大的视觉创作面，也是既有引擎能力的直接延伸",
            CreationConcern::Icon => "图标是第二大的视觉创作面，与壁纸共享导出链但受众不同",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            CreationConcern::AssetModel => "R01-M0",
            CreationConcern::AssetIO => "R01-M1",
            CreationConcern::Workflow => "R01-M2",
            CreationConcern::ValidatePublish => "R01-M3",
            CreationConcern::Preview => "R01-M4",
            CreationConcern::Debug => "R01-M5",
            CreationConcern::Editor => "R01-M6",
            CreationConcern::EditorA11y => "R01-M7",
            CreationConcern::Wallpaper => "R01-M8",
            CreationConcern::Icon => "R01-M9",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<CreationConcern> {
        match code {
            "R01-M0" => Some(CreationConcern::AssetModel),
            "R01-M1" => Some(CreationConcern::AssetIO),
            "R01-M2" => Some(CreationConcern::Workflow),
            "R01-M3" => Some(CreationConcern::ValidatePublish),
            "R01-M4" => Some(CreationConcern::Preview),
            "R01-M5" => Some(CreationConcern::Debug),
            "R01-M6" => Some(CreationConcern::Editor),
            "R01-M7" => Some(CreationConcern::EditorA11y),
            "R01-M8" => Some(CreationConcern::Wallpaper),
            "R01-M9" => Some(CreationConcern::Icon),
            _ => None,
        }
    }
}

/// 映射项序（单源）。
pub const CONCERN_ORDER: [CreationConcern; CONCERN_COUNT] = CreationConcern::ALL;

/// 映射落点表（**可替换**——标准表是正样本，机检也扫替换后的表）。
#[derive(Clone, Debug, Default)]
pub struct MappingTable {
    /// 逐项落点条目号（与 [`CreationConcern::ALL`] 同序）。
    landings: Vec<String>,
    /// 逐项依据（与落点同序；缺依据即 [`E_CONCERN_NO_BASIS`]）。
    basis: Vec<String>,
}

impl MappingTable {
    /// 标准映射表（十项落点齐备 + 十项依据齐备）。
    pub fn standard() -> Self {
        let mut landings = Vec::with_capacity(CONCERN_COUNT);
        let mut basis = Vec::with_capacity(CONCERN_COUNT);
        for c in CreationConcern::ALL.iter() {
            landings.push(c.landing().to_string());
            basis.push(c.basis().to_string());
        }
        MappingTable { landings, basis }
    }

    /// 某项的落点（复杂度 **O(1)**：按 rank 直接索引）。
    ///
    /// 返回 `Ok(None)` 表示查无此项；`Err` 表示索引位与枚举不同步——
    /// **两种故障必须可区分**，否则表错位会被当成"没这项"而放过。
    pub fn landing_of(&self, c: CreationConcern) -> Result<Option<String>, CreationError> {
        let idx = c.rank() as usize;
        match self.landings.get(idx) {
            Some(v) => Ok(Some(v.clone())),
            None => Ok(None),
        }
    }

    /// 某项的落点（借用版，避免机检里的无谓分配）。
    pub fn landing_ref(&self, c: CreationConcern) -> Option<&String> {
        self.landings.get(c.rank() as usize)
    }

    /// 某项的依据（借用版）。
    pub fn basis_ref(&self, c: CreationConcern) -> Option<&String> {
        self.basis.get(c.rank() as usize)
    }

    /// 某项的全部落点（**当前每项恰一条**，保留 Vec 以便将来一项多落点）。
    pub fn items_of(&self, c: CreationConcern) -> Vec<&String> {
        match self.landings.get(c.rank() as usize) {
            Some(v) if !v.trim().is_empty() => alloc::vec![v],
            _ => Vec::new(),
        }
    }

    /// 覆盖某项落点（机检与演练用；不做静默拒绝）。
    pub fn set_landing(&mut self, c: CreationConcern, item: &str) {
        let idx = c.rank() as usize;
        if self.landings.len() <= idx {
            self.landings.resize(CONCERN_COUNT, String::new());
            self.basis.resize(CONCERN_COUNT, String::new());
        }
        self.landings[idx] = item.to_string();
    }

    /// 覆盖某项依据。
    pub fn set_basis(&mut self, c: CreationConcern, b: &str) {
        let idx = c.rank() as usize;
        if self.basis.len() <= idx {
            self.landings.resize(CONCERN_COUNT, String::new());
            self.basis.resize(CONCERN_COUNT, String::new());
        }
        self.basis[idx] = b.to_string();
    }

    /// 落点表长度。
    pub fn len(&self) -> usize {
        self.landings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.landings.is_empty()
    }

    /// 读屏摘要（**十项逐项念出**——只报"十项齐备"等于没报）。
    pub fn screen_text(&self) -> String {
        let mut s = format!("五板块十项映射（{} 项）：", self.landings.len());
        for c in CreationConcern::ALL.iter() {
            s.push_str(&format!(
                "{}（{}，落点 {}）；",
                c.zh(),
                c.code(),
                self.landing_ref(*c)
                    .map(|v| v.as_str())
                    .unwrap_or("<缺>")
            ));
        }
        s.push_str(&format!(
            "R 域锚点起始号 F{}。",
            CreationBoard::R_ANCHOR_START
        ));
        s
    }
}

// ---------------------------------------------------------------------------
// 九、四域分工（锚点：E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义）
// ---------------------------------------------------------------------------

/// 分工域（锚点点名的四域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DivisionDomain {
    /// E 域：引擎事实（令牌与主题包的运行时）。
    Engine,
    /// R 域：创作界面（本域）。
    Creation,
    /// P 域：动效语言。
    Motion,
    /// S 域：语义（无障碍渲染语义与像素层执行）。
    Semantic,
}

impl DivisionDomain {
    /// 全部域（顺序即 rank，锚点四域）。
    pub const ALL: [DivisionDomain; DIVISION_COUNT] = [
        DivisionDomain::Engine,
        DivisionDomain::Creation,
        DivisionDomain::Motion,
        DivisionDomain::Semantic,
    ];

    /// 序号（单源派生）。
    pub fn rank(self) -> u8 {
        match self {
            DivisionDomain::Engine => 0,
            DivisionDomain::Creation => 1,
            DivisionDomain::Motion => 2,
            DivisionDomain::Semantic => 3,
        }
    }

    /// 单字母码（册内表记法）。
    pub fn letter(self) -> &'static str {
        match self {
            DivisionDomain::Engine => "E",
            DivisionDomain::Creation => "R",
            DivisionDomain::Motion => "P",
            DivisionDomain::Semantic => "S",
        }
    }

    /// 中文名（**按锚点原话**：「E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义」）。
    pub fn zh(self) -> &'static str {
        match self {
            DivisionDomain::Engine => "E 引擎事实域",
            DivisionDomain::Creation => "R 创作界面域",
            DivisionDomain::Motion => "P 动效语言域",
            DivisionDomain::Semantic => "S 语义域",
        }
    }

    /// 本域供给什么（锚点原话的四句分工）。
    pub fn supplies(self) -> &'static str {
        match self {
            DivisionDomain::Engine => "供引擎事实：令牌与主题包的运行时事实",
            DivisionDomain::Creation => "供创作界面：编辑器/工坊/工作流/工作台的界面与流程本体",
            DivisionDomain::Motion => "供动效语言：动效库与转场编排（R 域动效一律走此语言）",
            DivisionDomain::Semantic => "供语义：无障碍渲染语义与像素层执行（R 域不重述渲染语义）",
        }
    }

    /// 供给关键词（锚点原话里的中文词——供`supplies` 自检逐域核对）。
    pub fn supply_key(self) -> &'static str {
        match self {
            DivisionDomain::Engine => "供引擎事实",
            DivisionDomain::Creation => "供创作界面",
            DivisionDomain::Motion => "供动效语言",
            DivisionDomain::Semantic => "供语义",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            DivisionDomain::Engine => "R01-D-E",
            DivisionDomain::Creation => "R01-D-R",
            DivisionDomain::Motion => "R01-D-P",
            DivisionDomain::Semantic => "R01-D-S",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<DivisionDomain> {
        match code {
            "R01-D-E" => Some(DivisionDomain::Engine),
            "R01-D-R" => Some(DivisionDomain::Creation),
            "R01-D-P" => Some(DivisionDomain::Motion),
            "R01-D-S" => Some(DivisionDomain::Semantic),
            _ => None,
        }
    }
}

/// 分工域序（单源）。
pub const DIVISION_ORDER: [DivisionDomain; DIVISION_COUNT] = DivisionDomain::ALL;

/// 分工条目（**能力 → 唯一属主域**）。
#[derive(Clone, Debug)]
pub struct DivisionEntry {
    /// 能力名（唯一键）。
    pub capability: String,
    /// 能力说明（**必填**：说不清的能力会两边都认领）。
    pub desc: String,
    /// 属主域。
    pub owner: DivisionDomain,
    /// 册内依据条目号（可选但建议；无依据的分工是口头约定）。
    pub basis_item: String,
}

impl DivisionEntry {
    /// 四要素齐备性（无名/无说明即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.capability.trim().is_empty()
            && !self.desc.trim().is_empty()
            && !self.owner.code().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}）→ {} 域；说明 {}；依据 {}",
            self.capability,
            self.code(),
            self.owner.letter(),
            self.desc,
            if self.basis_item.trim().is_empty() {
                "未标"
            } else {
                self.basis_item.as_str()
            }
        )
    }

    /// 能力码（**由能力名实算哈希派生**——不能自己带码，改名即断链）。
    pub fn code(&self) -> String {
        format!("R01-CAP-{:04}", (fnv1a64(self.capability.as_bytes()) & 0xffff) as u16)
    }
}

/// 分工表。
#[derive(Clone, Debug, Default)]
pub struct DivisionTable {
    entries: Vec<DivisionEntry>,
}

impl DivisionTable {
    /// 空表。
    pub fn new() -> Self {
        DivisionTable {
            entries: Vec::new(),
        }
    }

    /// 标准分工表（八项能力，覆盖四域）。
    ///
    /// 八项的取法：每域两项。E 两项（令牌事实/主题包）守着"引擎事实"；
    /// R 两项（编辑器本体/工坊本体）守着"创作界面"；P 两项（动效库/转场编排）
    /// 守着"动效语言"；S 两项（渲染语义/像素层执行）守着"语义"。
    pub fn standard() -> Self {
        let seeds: [(DivisionDomain, &str, &str, &str); 8] = [
            (
                DivisionDomain::Engine,
                "令牌运行时",
                "令牌解析与四级覆盖：主题的运行时表达是引擎事实，R 域只消费",
                "VE-F3401",
            ),
            (
                DivisionDomain::Engine,
                "主题包生态",
                "主题包的装载与一致性：包格式与校验属引擎事实，不属创作界面",
                "VE-F3461",
            ),
            (
                DivisionDomain::Creation,
                "编辑器界面本体",
                "主题编辑器的可视化界面与编辑操作：这是创作界面的本体",
                "VE-F3621",
            ),
            (
                DivisionDomain::Creation,
                "工坊界面本体",
                "壁纸与图标工坊界面：视觉创作面的交互本体",
                "VE-F3641",
            ),
            (
                DivisionDomain::Motion,
                "动效库",
                "动效词汇与微交互：R 域动效一律走P 域语言，不自造动效集",
                "VE-F3002",
            ),
            (
                DivisionDomain::Motion,
                "转场编排",
                "转场与编排器：创作流程的过场动效同样走 P 域编排",
                "VE-F3005",
            ),
            (
                DivisionDomain::Semantic,
                "无障碍渲染语义",
                "高对比/焦点强化等渲染语义定义：语义层产出定义，不产出界面",
                "VE-F3801",
            ),
            (
                DivisionDomain::Semantic,
                "像素层执行",
                "语义到像素的最后一公里执行：R 域不重述渲染执行",
                "VE-F3803",
            ),
        ];
        let mut t = DivisionTable::new();
        for (owner, cap, desc, basis) in seeds.iter() {
            // 正样本不该失败；若失败说明种子表自身有错，让它显式崩出来。
            t.register(DivisionEntry {
                capability: cap.to_string(),
                desc: desc.to_string(),
                owner: *owner,
                basis_item: basis.to_string(),
            })
            .expect("标准分工表种子自身合法");
        }
        t
    }

    /// 登记一条分工（拒绝重名、超容）。
    pub fn register(&mut self, e: DivisionEntry) -> Result<usize, CreationError> {
        if e.capability.trim().is_empty() {
            return Err(CreationError::new(
                E_CAPABILITY_NO_NAME,
                "分工登记被拒：能力无名",
                "无名能力无法被检索，等于这条分工不存在",
                "给能力起一个唯一名（建议用「XX 本体/XX 语义」这类可判定形式）",
                "R 域分工表维护方",
            ));
        }
        if e.desc.trim().is_empty() {
            return Err(CreationError::new(
                E_CAPABILITY_NO_DESC,
                "分工登记被拒：无能力说明",
                &format!("能力「{}」没写说明，说不清的能力会两边都认领", e.capability),
                "写清这项能力到底管什么，越具体越不会撞车",
                "R 域分工表维护方",
            ));
        }
        if self.entries.len() >= MAX_CAPABILITIES {
            return Err(CreationError::new(
                E_DIVISION_CAP,
                "分工登记被拒：表已满",
                &format!("分工表 {} 条达到上限 {}", self.entries.len(), MAX_CAPABILITIES),
                "先合并冗余能力，或按需提升 MAX_CAPABILITIES",
                "R 域分工表维护方",
            ));
        }
        for x in self.entries.iter() {
            if x.capability == e.capability {
                return Err(CreationError::new(
                    E_CAPABILITY_DUP,
                    "分工登记被拒：能力重名",
                    &format!("能力「{}」已由{} 域认领", e.capability, x.owner.letter()),
                    "改名或合并；同名重复入册会让「谁拥有」失去唯一答案",
                    "R 域分工表维护方",
                ));
            }
        }
        let idx = self.entries.len();
        self.entries.push(e);
        Ok(idx)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &DivisionEntry> {
        self.entries.iter()
    }

    /// 查能力属主（复杂度 O(条目数)，能力数定长 [`MAX_CAPABILITIES`]）。
    ///
    /// 查无主返回 [`E_DIVISION_NO_OWNER`] 而非 `None`——
    /// 「没人认领」是需要处置的缺陷，不是合法状态。
    pub fn owner_of(&self, capability: &str) -> Result<DivisionDomain, CreationError> {
        let mut found: Option<DivisionDomain> = None;
        for e in self.entries.iter() {
            if e.capability == capability {
                if found.is_some() {
                    return Err(CreationError::new(
                        E_DIVISION_DUPLICATE,
                        "分工查询被拒：能力被多域认领",
                        &format!("能力「{}」出现在多条分工里，属主不唯一", capability),
                        "按锚点「四域分工」收敛到唯一属主：E供事实/R供界面/P供动效/S供语义",
                        "R 域分工表维护方",
                    ));
                }
                found = Some(e.owner);
            }
        }
        found.ok_or_else(|| {
            CreationError::new(
                E_DIVISION_NO_OWNER,
                "分工查询被拒：能力无人认领",
                &format!("能力「{}」在分工表里没有任何属主", capability),
                "补登该能力的属主域；无主能力是最容易两边都做的空洞",
                "R 域分工表维护方",
            )
        })
    }

    /// 某域的能力数。
    pub fn count_for(&self, d: DivisionDomain) -> usize {
        self.entries.iter().filter(|e| e.owner == d).count()
    }

    /// 不完整条目数。
    pub fn incomplete_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_complete()).count()
    }

    /// 四域覆盖审计（**每域至少一项**）。
    ///
    /// 返回缺失的域码列表；空列表即四域全覆盖。
    pub fn audit_coverage(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for d in DIVISION_ORDER.iter() {
            if self.count_for(*d) == 0 {
                missing.push(d.code());
            }
        }
        missing
    }

    /// 读屏摘要（逐条念出）。
    pub fn screen_text(&self) -> String {
        let mut s = format!("四域分工表：{} 条。", self.entries.len());
        for d in DIVISION_ORDER.iter() {
            s.push_str(&format!(
                "{}（{}，{} 项）；",
                d.zh(),
                d.letter(),
                self.count_for(*d)
            ));
        }
        for e in self.entries.iter() {
            s.push_str(&format!("{} ", e.screen_line()));
        }
        s.push_str(&format!(
            "不完整 {} 条。",
            self.incomplete_count()
        ));
        s
    }
}

// ---------------------------------------------------------------------------
// 十、收敛复述（锚点：创作资产走 F3201 Q 管线——收敛复述）
// ---------------------------------------------------------------------------

/// 创作资产类目（七类，本体归 F3603；本项只登记"必须走 Q 管线"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetKind {
    /// 主题。
    Theme,
    /// 皮肤。
    Skin,
    /// 壁纸。
    Wallpaper,
    /// 图标。
    Icon,
    /// 组件。
    Component,
    /// 模板。
    Template,
    /// 脚本。
    Script,
}

impl AssetKind {
    /// 全部类目（**恰七类**，与 F3603 锚点「七类创作资产」对齐）。
    pub const ALL: [AssetKind; CONVERGENCE_KINDS] = [
        AssetKind::Theme,
        AssetKind::Skin,
        AssetKind::Wallpaper,
        AssetKind::Icon,
        AssetKind::Component,
        AssetKind::Template,
        AssetKind::Script,
    ];

    /// 序号（单源派生）。
    pub fn rank(self) -> u8 {
        match self {
            AssetKind::Theme => 0,
            AssetKind::Skin => 1,
            AssetKind::Wallpaper => 2,
            AssetKind::Icon => 3,
            AssetKind::Component => 4,
            AssetKind::Template => 5,
            AssetKind::Script => 6,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AssetKind::Theme => "主题",
            AssetKind::Skin => "皮肤",
            AssetKind::Wallpaper => "壁纸",
            AssetKind::Icon => "图标",
            AssetKind::Component => "组件",
            AssetKind::Template => "模板",
            AssetKind::Script => "脚本",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            AssetKind::Theme => "R01-K0",
            AssetKind::Skin => "R01-K1",
            AssetKind::Wallpaper => "R01-K2",
            AssetKind::Icon => "R01-K3",
            AssetKind::Component => "R01-K4",
            AssetKind::Template => "R01-K5",
            AssetKind::Script => "R01-K6",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<AssetKind> {
        match code {
            "R01-K0" => Some(AssetKind::Theme),
            "R01-K1" => Some(AssetKind::Skin),
            "R01-K2" => Some(AssetKind::Wallpaper),
            "R01-K3" => Some(AssetKind::Icon),
            "R01-K4" => Some(AssetKind::Component),
            "R01-K5" => Some(AssetKind::Template),
            "R01-K6" => Some(AssetKind::Script),
            _ => None,
        }
    }

    /// 是否为代码类资产（**须沙箱**——F3602 沙箱隔离复述位）。
    pub fn is_code_like(self) -> bool {
        matches!(self, AssetKind::Script | AssetKind::Component)
    }
}

/// 类目序（单源）。
pub const ASSET_KIND_ORDER: [AssetKind; CONVERGENCE_KINDS] = AssetKind::ALL;

/// 收敛登记条目（一类资产一条）。
#[derive(Clone, Debug)]
pub struct ConvergenceRecord {
    /// 资产类目。
    pub kind: AssetKind,
    /// 走的 Q 管线段（`寻址→请求→调度→加载→校验→交付句柄` 之一）。
    pub stage: &'static str,
    /// 是否声明绕过管线（**必须全为 false**——绕管线的检出即 P0）。
    pub bypassed: bool,
    /// 册内依据（收敛复述的对端）。
    pub basis: String,
}

impl ConvergenceRecord {
    /// 段名是否合法（复述 F3201 六段段名）。
    pub fn stage_ok(&self) -> bool {
        Q_STAGES.contains(&self.stage)
    }

    /// 是否合规（走合法段且未绕管线）。
    pub fn is_converged(&self) -> bool {
        !self.bypassed && self.stage_ok()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}类创作资产走 Q 管线「{}」段{}；依据 {}",
            self.kind.zh(),
            self.stage,
            if self.bypassed { "【绕管线红线】" } else { "" },
            self.basis
        )
    }
}

/// Q 管线六段段名（**复述 F3201，不另立标准**）。
///
/// 六段与段序归 F3201；本项只把这六个名字抄下来当校验集——
/// 抄写而非重定义，是为了避免"两处六段写法不一致"的收敛破口。
pub const Q_STAGES: [&str; 6] = ["寻址", "请求", "调度", "加载", "校验", "交付句柄"];

/// 收敛登记账（七类齐备 + 段序合法 + 零绕管线）。
#[derive(Clone, Debug, Default)]
pub struct ConvergenceLedger {
    records: Vec<ConvergenceRecord>,
}

impl ConvergenceLedger {
    /// 空账。
    pub fn new() -> Self {
        ConvergenceLedger {
            records: Vec::new(),
        }
    }

    /// 标准收敛账（七类各走其应走之段）。
    ///
    /// 段序分配的理由：主题/皮肤/壁纸/图标是**成品资产**（走加载+校验）；
    /// 组件/模板/脚本是**创作中间物**（走请求+调度，交由加载段处理）。
    /// 具体段序归 F3201/F3602 落地，本项只保证**每类都有明确归口**。
    pub fn standard() -> Self {
        let seeds: [(AssetKind, &'static str, &str); CONVERGENCE_KINDS] = [
            (AssetKind::Theme, "校验", "VE-F3201 收敛红线：消费域不各自加载"),
            (AssetKind::Skin, "校验", "VE-F3201 收敛红线：消费域不各自加载"),
            (AssetKind::Wallpaper, "加载", "VE-F3605 创作资产委托 Q 管线转换"),
            (AssetKind::Icon, "加载", "VE-F3645 图标工坊产出走同一管线"),
            (AssetKind::Component, "请求", "VE-F3607 验证器经管线取依赖"),
            (AssetKind::Template, "调度", "VE-F3625 模板实例化经管线调度依赖"),
            (AssetKind::Script, "调度", "VE-F3612 脚本类资产沙箱执行前置调度"),
        ];
        let mut l = ConvergenceLedger::new();
        for (kind, stage, basis) in seeds.iter() {
            l.register(ConvergenceRecord {
                kind: *kind,
                stage,
                bypassed: false,
                basis: basis.to_string(),
            })
            .expect("标准收敛账种子自身合法");
        }
        l
    }

    /// 登记一条（**绕管线直接被拒**，不给"先记下来以后改"的机会）。
    pub fn register(&mut self, r: ConvergenceRecord) -> Result<usize, CreationError> {
        if r.bypassed {
            return Err(CreationError::new(
                E_PIPELINE_BYPASS,
                "收敛登记被拒：声明绕过 Q 管线",
                &format!("{}类创作资产被声明为绕过 Q 管线", r.kind.zh()),
                "创作资产是资源，必须走 F3201 六段管线；绕管线即多源加载分叉",
                "R 域收敛登记维护方",
            ));
        }
        if !r.stage_ok() {
            return Err(CreationError::new(
                E_CONVERGENCE_ORDER,
                "收敛登记被拒：段名不合法",
                &format!(
                    "{}类声明走「{}」段，但 Q 管线只有 {:?}",
                    r.kind.zh(),
                    r.stage,
                    Q_STAGES
                ),
                "段名必须取自 F3201 六段；自造段名等于另立管线",
                "R 域收敛登记维护方",
            ));
        }
        for x in self.records.iter() {
            if x.kind == r.kind {
                return Err(CreationError::new(
                    E_CONVERGENCE_KIND_DUP,
                    "收敛登记被拒：类目重复",
                    &format!("{}类已有登记（走「{}」段）", r.kind.zh(), x.stage),
                    "一类一条；重复登记会让归口变得不可判定",
                    "R 域收敛登记维护方",
                ));
            }
        }
        if self.records.len() >= CONVERGENCE_KINDS {
            return Err(CreationError::new(
                E_CONVERGENCE_CAP,
                "收敛登记被拒：账已满",
                &format!("收敛账 {} 条达到上限 {}", self.records.len(), CONVERGENCE_KINDS),
                "七类齐备即止；多登说明有人把类目拆细了，应改类目定义而非加条目",
                "R 域收敛登记维护方",
            ));
        }
        let idx = self.records.len();
        self.records.push(r);
        Ok(idx)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ConvergenceRecord> {
        self.records.iter()
    }

    /// 某类目的登记。
    pub fn record_of(&self, k: AssetKind) -> Option<&ConvergenceRecord> {
        self.records.iter().find(|r| r.kind == k)
    }

    /// 缺登记的类目（**空列表即七类齐备**）。
    pub fn missing_kinds(&self) -> Vec<AssetKind> {
        let mut missing = Vec::new();
        for k in ASSET_KIND_ORDER.iter() {
            if self.record_of(*k).is_none() {
                missing.push(*k);
            }
        }
        missing
    }

    /// 不合规条数（绕管线或段名非法——绕管线不可能入账，故实际只查段序）。
    pub fn divergent_count(&self) -> usize {
        self.records.iter().filter(|r| !r.is_converged()).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "收敛登记账（七类创作资产走 F3201 Q 管线）：{}/{} 类已登记，不合规 {}。",
            self.len(),
            CONVERGENCE_KINDS,
            self.divergent_count()
        );
        for r in self.records.iter() {
            s.push_str(&format!("{} ", r.screen_line()));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 十一、禁扩面（防抢活：越界必须能指出「这事该谁做」）
// ---------------------------------------------------------------------------

/// 禁扩面条目（R 域不许做的事，**每条都要给出去处**）。
pub const BOUNDARY_EXCLUSIONS: [(&str, &str); BOUNDARY_COUNT] = [
    (
        "R-OWN-TOKEN-RUNTIME",
        "自建令牌解析与四级覆盖——令牌运行时归 VE-E，R 域只消费已解析令牌",
    ),
    (
        "R-RELOAD-BYPASS",
        "创作资产绕过 Q 管线自行加载——加载/流送/生命周期归 F3201，绕管线即多源分叉",
    ),
    (
        "R-OWN-ANIM",
        "自建动效库与转场编排器——动效语言归 VE-P，R 域动效一律走 P 域词汇与编排",
    ),
    (
        "R-RECAST-A11Y-PIXEL",
        "重述无障碍渲染语义或像素层执行——语义归 VE-S，R 域只提需求不做渲染",
    ),
    (
        "R-OWN-LAYOUT",
        "自建控件布局算法——布局归VE-N，编辑器只消费布局结果",
    ),
    (
        "R-SANDBOX-BYPASS",
        "让代码类创作资产（脚本/组件）免沙箱执行——沙箱隔离是生态底线，不因是自家资产而豁免",
    ),
    (
        "R-OWN-PREVIEW-PIPELINE",
        "自建预览渲染管线——预览走轻量通道但管线本体归 Q 域，R 域只做编辑态投影",
    ),
];

/// 禁扩面的归属去处（越界拒绝的「下一步」内容——**给路，不只是拒绝**）。
pub fn boundary_advice(code: &str) -> &'static str {
    match code {
        "R-OWN-TOKEN-RUNTIME" => "把令牌需求写成消费声明交 VE-E；R 域不持有解析器",
        "R-RELOAD-BYPASS" => "把加载请求交 F3201 六段管线；R 域只持句柄",
        "R-OWN-ANIM" => "把动效需求提给 VE-P；R 域不定义缓动与转场原语",
        "R-RECAST-A11Y-PIXEL" => "把渲染语义需求交 VE-S；R 域只声明用户可及性要求",
        "R-OWN-LAYOUT" => "把布局需求提给 VE-N；编辑器只消费测量结果",
        "R-SANDBOX-BYPASS" => "代码类资产一律入沙箱；同待遇不因资产归属而豁免",
        "R-OWN-PREVIEW-PIPELINE" => "预览走轻量通道但仍归 Q 域管线；R 域只做编辑态数据投影",
        _ => "先查 BOUNDARY_EXCLUSIONS 确认此事归属，再决定找哪个域",
    }
}

/// 禁扩面校验（复杂度 O(禁扩面条数)= 7）。
///
/// 传入一条「想做的事」，逐条比对 [`BOUNDARY_EXCLUSIONS`]。命中即
/// [`E_BOUNDARY_OVERREACH`]，并把该禁扩面的**归属去处**一并给出——
/// 越界拒绝必须告诉对方「这事该谁做」，否则下次还会有人试。
pub fn check_no_overreach(intent: &str) -> Result<&'static str, CreationError> {
    for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
        if intent.contains(desc) || intent.contains(code) {
            return Err(CreationError::new(
                E_BOUNDARY_OVERREACH,
                "越界被拒：此事不归 R 域",
                &format!("「{}」命中禁扩面 {}：{}", intent, code, desc),
                boundary_advice(code),
                "R 域架构维护方",
            ));
        }
    }
    Ok("在边界内")
}

// ---------------------------------------------------------------------------
// 十二、前向义务（锚点：F3611/F3650 前向——不许把待兑现伪装成已完成）
// ---------------------------------------------------------------------------

/// 前向义务（**本体在下游条目**，本项只立声明与待兑现位）。
#[derive(Clone, Debug)]
pub struct ForwardObligation {
    /// 义务名。
    pub name: String,
    /// 承诺内容。
    pub promise: String,
    /// 主责条目号（本体归它）。
    pub owner_item: String,
    /// 是否已兑现（**默认 false**；无证据不得置 true）。
    pub settled: bool,
    /// 兑现证据（`settled` 为 true 时必填——口头不算证据）。
    pub evidence: String,
}

impl ForwardObligation {
    /// 四要素齐备性。
    pub fn is_complete(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.promise.trim().is_empty()
            && !self.owner_item.trim().is_empty()
    }

    /// 是否**自洽**（声称已兑现却无证据 = 伪装）。
    pub fn is_faked(&self) -> bool {
        self.settled && self.evidence.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（主责 {}）：{}；状态 {}",
            self.name,
            self.owner_item,
            self.promise,
            if self.settled {
                "已兑现"
            } else {
                "待兑现（前向）"
            }
        )
    }
}

/// 前向义务账。
#[derive(Clone, Debug, Default)]
pub struct ForwardLedger {
    items: Vec<ForwardObligation>,
}

impl ForwardLedger {
    /// 空账。
    pub fn new() -> Self {
        ForwardLedger { items: Vec::new() }
    }

    /// 标准账（**双维无障碍声明**，本体归 F3611/F3650）。
    pub fn standard() -> Self {
        let mut l = ForwardLedger::new();
        // 双维声明：工具侧与内容侧分开——合并成"创作无障碍"一句会让两侧都以为对方做了。
        let seeds: [(&str, &str, &str); 2] = [
            (
                "创作工具无障碍",
                "编辑器与工坊本身要能被无障碍用户操作：键盘全通/读屏/对比度",
                "VE-F3611",
            ),
            (
                "创作内容无障碍",
                "创作产出的内容要满足无障碍基线：对比度/语义标注（降级显性不阻断）",
                "VE-F3650",
            ),
        ];
        for (name, promise, owner) in seeds.iter() {
            l.register(ForwardObligation {
                name: name.to_string(),
                promise: promise.to_string(),
                owner_item: owner.to_string(),
                settled: false,
                evidence: String::new(),
            })
            .expect("标准前向义务种子自身合法");
        }
        l
    }

    /// 登记一条（拒绝无主责条目、超容）。
    pub fn register(&mut self, o: ForwardObligation) -> Result<usize, CreationError> {
        if o.owner_item.trim().is_empty() {
            return Err(CreationError::new(
                E_FORWARD_NO_OWNER,
                "前向义务登记被拒：无主责条目",
                &format!("义务「{}」没有主责条目号", o.name),
                "前向义务必须写清本体归哪个条目，否则永远没人兑现",
                "R 域架构维护方",
            ));
        }
        if self.items.len() >= MAX_FORWARD {
            return Err(CreationError::new(
                E_FORWARD_CAP,
                "前向义务登记被拒：账已满",
                &format!("前向义务账 {} 条达到上限 {}", self.items.len(), MAX_FORWARD),
                "先归档已兑现项，或按需提升 MAX_FORWARD",
                "R 域架构维护方",
            ));
        }
        let idx = self.items.len();
        self.items.push(o);
        Ok(idx)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ForwardObligation> {
        self.items.iter()
    }

    /// 标记第 `idx` 项已兑现（**必须同时给证据**——无证据的兑现一律被拒）。
    ///
    /// 为什么不给"先标兑现、证据后补"的口子：那会让 `settled` 位在一天内
    /// 变成口头承诺，而下游收口会拿它当"已覆盖"的依据。把口子堵死，代价是
    /// 补证据麻烦一点；收益是没人能靠改一个 bool 骗过无障碍双维。
    pub fn mark_settled(
        &mut self,
        idx: usize,
        evidence: &str,
    ) -> Result<(), CreationError> {
        let Some(item) = self.items.get_mut(idx) else {
            return Err(CreationError::new(
                E_FORWARD_NO_OWNER,
                "标记兑现失败：序号越界",
                &format!("前向义务账共 {} 条，收到序号 {}", self.items.len(), idx),
                "按实际序号重试；越界说明账已变",
                "R 域架构维护方",
            ));
        };
        if evidence.trim().is_empty() {
            return Err(CreationError::new(
                E_FORWARD_FAKED,
                "标记兑现失败：证据为空",
                &format!("义务「{}」被标为已兑现却没给证据", item.name),
                "兑现须附证据（自检报告/条目号/验收记录）；无证据的兑现会被 audit 判伪装",
                "R 域架构维护方",
            ));
        }
        item.evidence = evidence.trim().to_string();
        item.settled = true;
        Ok(())
    }

    /// 附证据（**不自动改settled**——补证据与宣告兑现是两件事，分开才可审**）。
    pub fn attach_evidence(&mut self, idx: usize, evidence: &str) {
        if let Some(item) = self.items.get_mut(idx) {
            item.evidence = evidence.trim().to_string();
        }
    }

    /// 无证据强标已兑现（**仅供自检演练注入反例**；返回是否成功）。
    ///
    /// 这是刻意的反例注入口：真实代码路径走 [`ForwardLedger::mark_settled`]，
    /// 它要求证据非空。要证明 `faked_count` 不是恒零，就必须有一条
    /// **能造出「已兑现 + 无证据」状态**的受控入口，否则该自检无法证伪。
    /// 命名与文档都在明说它是演练入口，不给"顺手在生产代码里用一下"。
    pub fn mark_settled_without_evidence(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(item) => {
                item.settled = true;
                item.evidence = String::new();
                true
            }
            None => false,
        }
    }

    /// 伪装项（声称已兑现但无证据）。
    pub fn faked_count(&self) -> usize {
        self.items.iter().filter(|o| o.is_faked()).count()
    }

    /// 待兑现项数。
    pub fn pending_count(&self) -> usize {
        self.items.iter().filter(|o| !o.settled).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "前向义务账：{} 条（待兑现 {}，伪装兑现 {}）。",
            self.len(),
            self.pending_count(),
            self.faked_count()
        );
        for o in self.items.iter() {
            s.push_str(&format!("{} ", o.screen_line()));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 十三、判据六项（锚点判据：跳段 ADR / 同步更新 / 五板块 / 四域分工 / 收敛复述 / 判据）
// ---------------------------------------------------------------------------

/// 判据项。锚点判据列六项，本总纲把它们变成**可被逐条断言的枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：跳段 ADR（域号跳段声明且冲突已裁决）。
    JumpAdr,
    /// 判据二：同步更新（台账与映射表逐项核对通过）。
    SyncUpdate,
    /// 判据三：五板块（板块不多不少、序递增、码往返）。
    FiveBoards,
    /// 判据四：四域分工（每能力唯一属主、四域全覆盖）。
    Division,
    /// 判据五：收敛复述（七类资产走 Q 管线、零绕管线）。
    Convergence,
    /// 判据六：判据自身可追溯（承诺可被人话复述）。
    CriterionTrace,
}

impl Criterion {
    /// 全部判据（顺序即 rank）。
    pub const ALL: [Criterion; CRITERION_COUNT] = [
        Criterion::JumpAdr,
        Criterion::SyncUpdate,
        Criterion::FiveBoards,
        Criterion::Division,
        Criterion::Convergence,
        Criterion::CriterionTrace,
    ];

    /// 序号（单源派生）。
    pub fn rank(self) -> u8 {
        match self {
            Criterion::JumpAdr => 0,
            Criterion::SyncUpdate => 1,
            Criterion::FiveBoards => 2,
            Criterion::Division => 3,
            Criterion::Convergence => 4,
            Criterion::CriterionTrace => 5,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::JumpAdr => "跳段 ADR",
            Criterion::SyncUpdate => "同步更新",
            Criterion::FiveBoards => "五板块",
            Criterion::Division => "四域分工",
            Criterion::Convergence => "收敛复述",
            Criterion::CriterionTrace => "判据自证",
        }
    }

    /// 承诺（判据要能用人话讲清，否则没法验收）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::JumpAdr => "域号跳段有ADR 登记，两套编号的冲突逐条裁决且未裁决清零",
            Criterion::SyncUpdate => "域号台账与十项映射表逐项核对一致，不同步即阻断开工",
            Criterion::FiveBoards => "五板块不多不少、序递增、码往返，五板块职责各有边界",
            Criterion::Division => "每项能力有且只有一个属主域，E/R/P/S 四域全覆盖",
            Criterion::Convergence => "七类创作资产各有管线归口，零绕管线，段名取自 F3201 六段",
            Criterion::CriterionTrace => "六项判据各有可复述的承诺，读屏能一条条念出来",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::JumpAdr => "R01-C0",
            Criterion::SyncUpdate => "R01-C1",
            Criterion::FiveBoards => "R01-C2",
            Criterion::Division => "R01-C3",
            Criterion::Convergence => "R01-C4",
            Criterion::CriterionTrace => "R01-C5",
        }
    }

    /// 按码反查。
    pub fn from_code(code: &str) -> Option<Criterion> {
        match code {
            "R01-C0" => Some(Criterion::JumpAdr),
            "R01-C1" => Some(Criterion::SyncUpdate),
            "R01-C2" => Some(Criterion::FiveBoards),
            "R01-C3" => Some(Criterion::Division),
            "R01-C4" => Some(Criterion::Convergence),
            "R01-C5" => Some(Criterion::CriterionTrace),
            _ => None,
        }
    }
}

/// 判据序（单源）。
pub const CRITERIA: [Criterion; CRITERION_COUNT] = Criterion::ALL;

// ---------------------------------------------------------------------------
// 十四、下游归属表（防止抢活与漏活）
// ---------------------------------------------------------------------------

/// 下游条目归属（**谁拥有什么**——本项只立总纲，不代做后续 199 项）。
///
/// 这张表的作用是**防止抢活**：域开工最常见的失败不是做不出来，而是总纲
/// 顺手把下游的活也干了，然后下游条目开工时发现「已经有人做过了，但没人
/// 知道在哪、依据是什么」。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 12] = [
    ("VE-F3602", "创作生态总架构：三层（工具/资产/分发）与五段签名冻结 v1"),
    ("VE-F3603", "创作资产模型：七要素模型与七类资产注册表本体"),
    ("VE-F3604", "创作工作流引擎：步骤 DAG、状态机与断点续作"),
    ("VE-F3605", "创作与 E 域引擎对接：转换委托协议与预览隔离声明"),
    ("VE-F3606", "创作预览运行时：沙箱预览环境、保真对拍与100ms 实时断言"),
    ("VE-F3607", "创作资产验证器：schema/引用闭合/许可/无障碍/安全五段"),
    ("VE-F3611", "创作工具无障碍：编辑器键盘全通/读屏/对比度与辅助提示器本体"),
    ("VE-F3613", "创作调试器：三维体检、错误定位与修复建议"),
    ("VE-F3616", "创作与市场分发：上架流水、收益明细与合规分发"),
    ("VE-F3621", "主题编辑器总架构：五段单源声明与三层编辑对象"),
    ("VE-F3650", "视觉创作无障碍：创作内容侧无障碍基线与分级处置"),
    ("VE-F3620", "R01 组收口双签：接口总账与四联动核验"),
];

// ---------------------------------------------------------------------------
// 十五、条目号与哈希工具
// ---------------------------------------------------------------------------

/// 条目号格式校验（`VE-F` + 四位数字）。
pub fn is_valid_item_id(item: &str) -> bool {
    let Some(rest) = item.strip_prefix("VE-F") else {
        return false;
    };
    rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit())
}

/// 解析条目号（`VE-F3603` → `Some(3603)`；格式非法返回 `None`）。
pub fn parse_item_num(item: &str) -> Option<u32> {
    if !is_valid_item_id(item) {
        return None;
    }
    let rest = &item[4..];
    let mut n: u32 = 0;
    for b in rest.bytes() {
        n = n.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(n)
}

/// FNV-1a 64 位偏移基。
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位哈希（单遍累积）。
///
/// 选它不用更强哈希是因为**对账要的是确定性而非抗攻击**——两侧算同一样东西
/// 必须得到同一个数，而抗碰撞不是这一层的诉求。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV64_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
    }
    h
}

/// FNV-1a 64 位十六进制（16 位小写，定宽——宽度不定就没法字符串比对）。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

// ---------------------------------------------------------------------------
// 十六、R 域总纲本体
// ---------------------------------------------------------------------------

/// 板块审计结果（**逐项列缺，不只报"过/不过"**）。
#[derive(Clone, Debug, Default)]
pub struct BoardAudit {
    /// 缺登记的板块。
    pub missing: Vec<CreationBoard>,
    /// 落点为空的映射项。
    pub empty: Vec<CreationConcern>,
    /// 落点格式非法的映射项。
    pub malformed: Vec<CreationConcern>,
    /// 板块错配的映射项。
    pub board_mismatch: Vec<CreationConcern>,
    /// 落点溢出的映射项。
    pub overflow: Vec<CreationConcern>,
    /// 依据缺失的映射项。
    pub no_basis: Vec<CreationConcern>,
    /// 落点越出 R 域号段的映射项（域号台账联动）。
    pub out_of_band: Vec<CreationConcern>,
}

impl BoardAudit {
    /// 是否全绿（**六条同时为空**）。
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty()
            && self.empty.is_empty()
            && self.malformed.is_empty()
            && self.board_mismatch.is_empty()
            && self.overflow.is_empty()
            && self.no_basis.is_empty()
            && self.out_of_band.is_empty()
    }

    /// 问题总数（**报数量而非布尔**，便于定位严重度）。
    pub fn issue_count(&self) -> usize {
        self.missing.len()
            + self.empty.len()
            + self.malformed.len()
            + self.board_mismatch.len()
            + self.overflow.len()
            + self.no_basis.len()
            + self.out_of_band.len()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "板块审计：缺板块 {} / 空落点 {} / 格式非法 {} / 板块错配 {} / 溢出 {} / 缺依据 {} / 越号段 {}，合计 {}",
            self.missing.len(),
            self.empty.len(),
            self.malformed.len(),
            self.board_mismatch.len(),
            self.overflow.len(),
            self.no_basis.len(),
            self.out_of_band.len(),
            self.issue_count()
        )
    }
}

/// R 域开工总纲。
#[derive(Clone, Debug)]
pub struct CreationArchitecture {
    /// 总纲版本。
    pub version: &'static str,
    /// 段间接口冻结版本。
    pub interface_version: &'static str,
    /// 域号台账（跳段裁决）。
    pub jump: JumpLedger,
    /// 十项映射表。
    pub mapping: MappingTable,
    /// 四域分工表。
    pub division: DivisionTable,
    /// 收敛登记账。
    pub convergence: ConvergenceLedger,
    /// 前向义务账。
    pub forward: ForwardLedger,
    /// ADR 账。
    pub adrs: AdrLedger,
}

impl CreationArchitecture {
    /// 标准总纲（**台账已裁决、映射齐备、分工全覆盖、收敛七类齐备**）。
    ///
    /// 注意这里**没有伪造"前向义务已兑现"**：双维无障碍的 `settled` 为false，
    /// 因为本体在 F3611/F3650。宣称已兑现就是用声明冒充实现。
    pub fn standard() -> Self {
        CreationArchitecture {
            version: ARCH_VERSION,
            interface_version: INTERFACE_VERSION,
            jump: standard_jump_ledger(),
            mapping: MappingTable::standard(),
            division: DivisionTable::standard(),
            convergence: ConvergenceLedger::standard(),
            forward: ForwardLedger::standard(),
            adrs: standard_adrs(),
        }
    }

    /// 空总纲（**全空，用于反例演练**）。
    pub fn empty() -> Self {
        CreationArchitecture {
            version: ARCH_VERSION,
            interface_version: INTERFACE_VERSION,
            jump: JumpLedger::new(),
            mapping: MappingTable::default(),
            division: DivisionTable::new(),
            convergence: ConvergenceLedger::new(),
            forward: ForwardLedger::new(),
            adrs: AdrLedger::new(),
        }
    }

    /// 开工前置（**五道闸全过才算可开工**）。
    ///
    /// 顺序有意：域号 → 映射 → 分工 → 收敛 → 前向。先查地基（域号），
    /// 因为地基错时后面几项的报错都会指向错误的位置。
    pub fn preflight(&self) -> Result<(), CreationError> {
        self.jump.preflight()?;
        let audit = self.audit_boards();
        if !audit.missing.is_empty() {
            return Err(CreationError::new(
                E_BOARD_COUNT,
                "开工前置未过：板块不齐",
                &format!("缺 {} 个板块", audit.missing.len()),
                "按锚点五板块补齐：资产模型/工作流/工作台/编辑器/工坊",
                "R 域架构维护方",
            ));
        }
        if !audit.empty.is_empty() {
            return Err(CreationError::new(
                E_CONCERN_EMPTY,
                "开工前置未过：映射项落点为空",
                &format!("{} 项映射无落点", audit.empty.len()),
                "为每项映射补册内条目号——十项映射不是凑数",
                "R 域架构维护方",
            ));
        }
        if !audit.out_of_band.is_empty() {
            return Err(CreationError::new(
                E_SYNC_MISMATCH,
                "开工前置未过：映射落点越出 R 域号段",
                &format!("{} 项落点不在 F{} 起段内", audit.out_of_band.len(), CreationBoard::R_ANCHOR_START),
                "按锚点「R 域条目号 F3601 起跳」改正落点，或先改台账并登记裁决",
                "R 域架构维护方",
            ));
        }
        let missing = self.division.audit_coverage();
        if !missing.is_empty() {
            return Err(CreationError::new(
                E_DIVISION_NO_OWNER,
                "开工前置未过：分工有域未覆盖",
                &format!("{} 个域无任何能力", missing.len()),
                "四域分工要求 E/R/P/S 各有职责，空域意味着边界没画完",
                "R 域架构维护方",
            ));
        }
        let kinds = self.convergence.missing_kinds();
        if !kinds.is_empty() {
            return Err(CreationError::new(
                E_CONVERGENCE_KIND_MISSING,
                "开工前置未过：收敛登记缺类目",
                &format!("{} 类创作资产无管线归口", kinds.len()),
                "七类创作资产逐一登记归口段，段名取自 F3201 六段",
                "R 域架构维护方",
            ));
        }
        Ok(())
    }

    /// 板块与映射审计（复杂度 O(板块数 + 十项× 落点数)，**只在开工/收口跑**）。
    ///
    /// 七条判定：板块齐备、落点非空、格式合法、板块一致、落点不溢出、
    /// 依据齐备、**落点在 R 域号段内**（最后一条把映射表与台账真正焊在一起——
    /// 只查落点非空的话，映射表写满 F3401段的条目号也能"通过"，
    /// 而那正是同步更新红线要防的事）。
    pub fn audit_boards(&self) -> BoardAudit {
        let mut a = BoardAudit::default();
        for b in BOARD_ORDER.iter() {
            if !self.mapping_has_board(*b) {
                a.missing.push(*b);
            }
        }
        for c in CreationConcern::ALL.iter() {
            let items = self.mapping.items_of(*c);
            if items.is_empty() {
                a.empty.push(*c);
                continue;
            }
            if items.len() > MAX_ITEMS_PER_BOARD {
                a.overflow.push(*c);
            }
            let mut bad_format = false;
            let mut out_of_band = false;
            for it in items.iter() {
                match parse_item_num(it) {
                    None => bad_format = true,
                    Some(n) => {
                        if n < CreationBoard::R_ANCHOR_START {
                            out_of_band = true;
                        }
                    }
                }
            }
            if bad_format {
                a.malformed.push(*c);
            }
            if out_of_band {
                a.out_of_band.push(*c);
            }
            if !self.concern_board_matches(*c) {
                a.board_mismatch.push(*c);
            }
            match self.mapping.basis_ref(*c) {
                None => a.no_basis.push(*c),
                Some(b) => {
                    if b.trim().is_empty() {
                        a.no_basis.push(*c);
                    }
                }
            }
        }
        a
    }

    /// 某板块是否有任何映射项登记（**内部用：判缺板块**）。
    fn mapping_has_board(&self, b: CreationBoard) -> bool {
        b.concerns()
            .iter()
            .any(|c| !self.mapping.items_of(*c).is_empty())
    }

    /// 某映射项的板块归属是否**双向自洽**（**内部用**）。
    ///
    /// 查的是两条**分别写就**的声明是否一致：[`CreationConcern::board`] 反查出
    /// 的板块，其 [`CreationBoard::concerns`] 必须把本项列进去。一处写在
    /// `CreationConcern`、另一处写在 `CreationBoard::concerns`，改一处忘另一处
    /// 会真的失配——所以这不是恒真断言。
    ///
    /// 反例可证伪：若把 `CreationConcern::EditorA11y::board()` 改成 `Workshop`
    /// 而忘了改 `Workshop::concerns()`，本函数即返回 false。
    fn concern_board_matches(&self, c: CreationConcern) -> bool {
        c.board().concerns().contains(&c)
    }

    /// 映射覆盖自检（复杂度 O(十项)）：十项齐备且无空落点。
    pub fn check_concern_coverage(&self) -> BoardAudit {
        self.audit_boards()
    }

    /// 契约问题全检（复杂度 O(段数 + 能力数 + 类目数)）。
    pub fn check_contracts(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        // 域号：未裁决段。
        if self.jump.unsettled_band_count() > 0 {
            issues.push(ContractIssue {
                code: E_CONFLICT_UNADJUDICATED,
                symptom: self.jump.map.screen_text(),
                root_cause: "号段存在未裁决条目——两套编号的冲突没有落地".to_string(),
                advice: "逐段登记裁决与理由（JumpLedger::record_conflict）",
                severity: Severity::Blocking,
            });
        }
        // 域号：R 起点与锚点不符。
        if let Ok(start) = self.jump.map.r_start() {
            if start != CreationBoard::R_ANCHOR_START {
                issues.push(ContractIssue {
                    code: E_JUMP_UNREGISTERED,
                    symptom: format!("台账 R 起点 F{}", start),
                    root_cause: "号段起点与锚点「R 域条目号 F3601 起跳」不符".to_string(),
                    advice: "按锚点改正起点，或登记一条说明偏离的 ADR",
                    severity: Severity::Blocking,
                });
            }
        } else {
            issues.push(ContractIssue {
                code: E_JUMP_UNREGISTERED,
                symptom: "R 域起点未登记".to_string(),
                root_cause: "号段表里没有已裁决的 R 域号段".to_string(),
                advice: "登记 F3601-F3800 段（裁定权威=条目锚点）",
                severity: Severity::Blocking,
            });
        }
        // 同步：台账与映射表。
        let sync = self.jump.sync_report(&self.mapping);
        if !sync.is_synced() {
            issues.push(ContractIssue {
                code: E_SYNC_MISMATCH,
                symptom: sync.screen_line(),
                root_cause: sync
                    .desync_reason()
                    .unwrap_or("未知")
                    .to_string(),
                advice: "按同步更新红线同时改台账与映射表，改完重跑 check_sync",
                severity: Severity::Blocking,
            });
        }
        // 板块与映射。
        let audit = self.audit_boards();
        if !audit.is_clean() {
            issues.push(ContractIssue {
                code: E_BOARD_COUNT,
                symptom: audit.screen_text(),
                root_cause: "五板块十项映射存在缺项/空落点/错配/越号段".to_string(),
                advice: "按锚点五板块十项补齐，落点须落在 R 域号段内",
                severity: Severity::Blocking,
            });
        }
        // 分工：域覆盖。
        let missing = self.division.audit_coverage();
        if !missing.is_empty() {
            issues.push(ContractIssue {
                code: E_DIVISION_NO_OWNER,
                symptom: format!("{} 个域无能力登记", missing.len()),
                root_cause: "四域分工未覆盖全——空域意味着边界没画完".to_string(),
                advice: "为每个域至少登记一项能力（锚点：E/R/P/S 各有供给）",
                severity: Severity::Blocking,
            });
        }
        // 分工：重复属主。
        for cap in standard_capabilities().iter() {
            if self.division.owner_of(cap).is_err() {
                issues.push(ContractIssue {
                    code: E_DIVISION_NO_OWNER,
                    symptom: format!("能力「{}」无唯一属主", cap),
                    root_cause: "分工表缺该能力或存在重名".to_string(),
                    advice: "为该能力登记唯一属主域",
                    severity: Severity::Blocking,
                });
            }
        }
        // 收敛：缺类目。
        let kinds = self.convergence.missing_kinds();
        if !kinds.is_empty() {
            issues.push(ContractIssue {
                code: E_CONVERGENCE_KIND_MISSING,
                symptom: format!("缺 {} 类：{:?}", kinds.len(), kinds),
                root_cause: "七类创作资产未全部登记管线归口".to_string(),
                advice: "补登记，段名取自 F3201 六段",
                severity: Severity::Blocking,
            });
        }
        // 前向：伪装兑现。
        if self.forward.faked_count() > 0 {
            issues.push(ContractIssue {
                code: E_FORWARD_FAKED,
                symptom: format!("{} 项声称已兑现但无证据", self.forward.faked_count()),
                root_cause: "前向义务被标为已兑现但证据为空——用声明冒充实现".to_string(),
                advice: "补证据，或改回待兑现（前向项本体在F3611/F3650）",
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// 零运行时开销自检（**本域是声明层，帧路径不得出现本域代码**）。
    pub fn is_zero_runtime_cost(&self) -> bool {
        // 声明层的全部结构都是建期固化的数据；判据是「没有任何待执行动作」。
        // 这里用可机检的形式表达：台账、映射、分工、收敛、前向五账都无待办。
        self.jump.unsettled_band_count() == 0
            && self.audit_boards().is_clean()
            && self.division.audit_coverage().is_empty()
            && self.convergence.missing_kinds().is_empty()
    }

    /// 读屏替述（无障碍替述：**覆盖全部六项判据**）。
    ///
    /// 替述与总纲同源生成——另写一份的风险是漂移，而漂移的无障碍文档比没有
    /// 更坏：它会让用户以为已经有保障。
    pub fn architecture_narration(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "VE-R 域创作生态总架构，版本 {}，段间接口冻结 {}，域号台账版本 {}。\n",
            self.version, self.interface_version, LEDGER_VERSION
        ));
        s.push_str("本域使命：让用户与第三方在引擎之上创造。判据共六项：");
        for c in CRITERIA.iter() {
            s.push_str(&format!("{}（{}）——{}；", c.zh(), c.code(), c.promise()));
        }
        s.push('\n');
        s.push_str(&format!("{}\n", self.jump.screen_text()));
        s.push_str(&format!("{}\n", self.jump.sync_report(&self.mapping).screen_line()));
        s.push_str("五板块：");
        for b in BOARD_ORDER.iter() {
            s.push_str(&format!(
                "{}（{}，{}）→ {} 项；",
                b.zh(),
                b.code(),
                b.duty(),
                b.concerns().len()
            ));
        }
        s.push_str(&format!("\n{}\n", self.mapping.screen_text()));
        s.push_str(&format!("{}\n", self.division.screen_text()));
        s.push_str(&format!("{}\n", self.convergence.screen_text()));
        s.push_str(&format!("{}\n", self.forward.screen_text()));
        s.push_str(&format!("{}\n", self.jump.conflict_note()));
        s.push_str("禁扩面七条：");
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            s.push_str(&format!("{}（{}）；", code, desc));
        }
        s.push_str(&format!(
            "\n复杂度口径：{}\n",
            COMPLEXITY_DOC
        ));
        s
    }
}

/// 标准跳段裁决账（**三段齐备 + 两条冲突已裁决**——这是本项的正样本）。
///
/// 三段（依锚点与册内实情，非臆造）：
/// - `F3401-F3600` → E 域（册内 E01 批次段明写「VE-E 域开工（主题与个性化
///   引擎，F3401-F3600）」）；
/// - `F3601-F3800` → R 域（**本项锚点原文**：R 域条目号 F3601 起跳）；
/// - `F3801-F4000` → S 域（册内「VE-F3801 · S 域开工与无障碍渲染总架构」起）。
///
/// 两条冲突：
/// 1. F3401-F3600：开篇总表说 R 域，E01 批次段说 E 域 → 锚点优先，判 E；
/// 2. F3601-F3800：开篇总表说 S 域，本项锚点说 R 域 → 锚点优先，判 R。
pub fn standard_jump_ledger() -> JumpLedger {
    let mut l = JumpLedger::new();
    let bands = [
        (
            DivisionDomain::Engine,
            3401u32,
            3600u32,
            BandAuthority::Anchor,
            BandBasis::EBatchF3401ToE,
        ),
        (
            DivisionDomain::Creation,
            3601,
            3800,
            BandAuthority::Anchor,
            BandBasis::AnchorF3601Start,
        ),
        (
            DivisionDomain::Semantic,
            3801,
            4000,
            BandAuthority::Anchor,
            BandBasis::MapS3801Start,
        ),
    ];
    for (d, lo, hi, auth, basis) in bands.iter() {
        l.register_band(
            NumberBand::new(*d, *lo, *hi, *auth, *basis, BandVerdict::AnchorWins)
                .expect("标准号段本身合法"),
        )
        .expect("标准号段之间不重叠");
    }
    let conflicts = [
        ConflictRecord {
            id: 0,
            lo: 3401,
            hi: 3600,
            map_claim: "开篇域号总表：VE-R 主题与个性化 F3401-F3600".to_string(),
            anchor_claim: "册内 E01 批次段：VE-E 主题与个性化 F3401-F3600".to_string(),
            verdict: BandVerdict::AnchorWins,
            reason: "条目级批次段晚于开篇总表，且E01 批次段把该段全部条目（F3401 起）\
                     都编排为 E 域令牌运行时组——总表的R 是改判前的旧名"
                .to_string(),
            sync_action: "台账登记 F3401-F3600 归 E 域；映射表内所有 F34xx 落点须迁至 E 域条目，\
                          R 域映射一律从 F3601 起"
                .to_string(),
        },
        ConflictRecord {
            id: 0,
            lo: 3601,
            hi: 3800,
            map_claim: "开篇域号总表：VE-S 无障碍渲染 F3601-F3800".to_string(),
            anchor_claim: "本项锚点：R 域条目号 F3601 起跳（R=创作生态域）".to_string(),
            verdict: BandVerdict::AnchorWins,
            reason: "本项锚点是R 域开工的直接依据；无障碍渲染已顺延至 F3801 起\
                     （册内「VE-F3801 · S 域开工与无障碍渲染总架构」可证），\
                     故总表的 S 段是改判前的旧名"
                .to_string(),
            sync_action: "台账登记 F3601-F3800 归 R 域；S 域起点同步改为 F3801；\
                          映射表十项落点全部落在 F3601-F3800 内"
                .to_string(),
        },
    ];
    for c in conflicts.iter() {
        l.record_conflict(ConflictRecord {
            id: 0,
            lo: c.lo,
            hi: c.hi,
            map_claim: c.map_claim.clone(),
            anchor_claim: c.anchor_claim.clone(),
            verdict: c.verdict,
            reason: c.reason.clone(),
            sync_action: c.sync_action.clone(),
        })
        .expect("标准冲突裁决四要素齐备");
    }
    l
}

/// 标准 ADR 账（**四条覆盖四范围**——跳段/板块/分工/收敛各一条）。
pub fn standard_adrs() -> AdrLedger {
    let mut l = AdrLedger::new();
    let seeds: [(AdrScope, &str, &str, &str, &str); 4] = [
        (
            AdrScope::NumberJump,
            "R 域条目号自 F3601 起跳，F3401-F3600 让给 E 域二期扩展",
            "两套编号并存会让后续条目建到错号段上；条目级锚点晚于开篇总表，故锚点优先。\
             R 域起点定为 F3601，S 域顺延至 F3801",
            "否决「沿用总表让 R 域从 F3401 起」：该段已被E01 批次段编排为 E 域令牌运行时组，\
             沿用会让R 域与 E 域抢同一批条目号",
            LEDGER_VERSION,
        ),
        (
            AdrScope::Boards,
            "五板块各两项映射，十项落点全部落在 R 域号段内",
            "锚点只点名五板块未点名十项，故十项按每板块两项取；\
             取项依据逐条落进 CreationConcern::basis，可被复核",
            "否决「十项按锚点括号内顺序平铺」：锚点未给括号内顺序，平铺等于自造顺序并冒充锚点",
            INTERFACE_VERSION,
        ),
        (
            AdrScope::Division,
            "四域分工：E 供引擎事实/R 供创作界面/P 供动效语言/S 供语义",
            "每项能力有且只有一个属主域，属主重复即分工分歧，须对拍收敛",
            "否决「按需动态认领」：动态认领让同一能力在不同批次落到不同域，\
             跨批次无法对账",
            INTERFACE_VERSION,
        ),
        (
            AdrScope::Convergence,
            "七类创作资产一律走 F3201 六段管线，R 域不私建加载",
            "消费域各自加载是多源分叉的起点；复述 Q 域收敛红线而非另立标准",
            "否决「创作资产走创作侧专用加载器」：专用加载器与 Q 管线并存会让同一资产两套缓存",
            INTERFACE_VERSION,
        ),
    ];
    for (scope, title, decision, rejected, to_version) in seeds.iter() {
        l.register(AdrRecord {
            id: 0,
            scope: *scope,
            title: title.to_string(),
            decision: decision.to_string(),
            rejected: rejected.to_string(),
            from_version: String::new(),
            to_version: to_version.to_string(),
            tick: 1,
        })
        .expect("标准 ADR 四要素齐备");
    }
    l
}

/// 标准能力名清单（**分工覆盖审计的输入**——与 [`DivisionTable::standard`] 同源）。
pub fn standard_capabilities() -> Vec<&'static str> {
    vec![
        "令牌运行时",
        "主题包生态",
        "编辑器界面本体",
        "工坊界面本体",
        "动效库",
        "转场编排",
        "无障碍渲染语义",
        "像素层执行",
    ]
}

impl JumpLedger {
    /// 裁决账的读屏补充（**逐条念出两套说法的分歧**）。
    pub fn conflict_note(&self) -> String {
        let mut s = format!("域号跳段说明：共登记 {} 条冲突裁决。", self.conflict_count());
        for c in self.iter() {
            s.push_str(&format!(
                "F{}-F{}：总表与锚点说法不同，裁决为{}，已按「{}」同步台账与映射表；",
                c.lo, c.hi, c.verdict.zh(), c.sync_action
            ));
        }
        s.push_str("理由：条目级锚点晚于开篇域号总表，以锚点为准。");
        s
    }
}

// ---------------------------------------------------------------------------
// 十七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F3601 域自检（判据逐条映射见 `ver02_checks.rs`）。
///
/// 聚合表经本模块取自检（与 [`ver01_arch`](super::ver01_arch) 同一约定）：
/// 这样聚合面只认「域开工模块」，不认检查模块，检查实现挪位不影响总表。
pub fn run_ver02_checks() -> CheckSet {
    super::ver02_checks::run_ver02_checks()
}
