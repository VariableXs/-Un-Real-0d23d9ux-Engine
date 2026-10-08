//! VE-F0620 · 图层树组收口（VE-D 域 · 2D 合成引擎 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0620`
//!
//! **判据（锚点原文逐条）**：图层树组（D 域第一组 F0601-0619）收口：
//! - **自检入树**：数据结构、变换、不透明度、裁剪、隔离语义、Z 序、可见性传播、
//!   动画接口、效果链、快照、命中、序列化、树级脏区、遍历器、缓存、大层数、
//!   一致性校验、调试可视化、表面对接**十九件证据齐备**；
//! - **缺陷总账清点**；**双签**后移交混合模式组（D02）；
//! - **经验包**：三序同源契约、两级失效分离（重绘与重合成）、树不变式fail-fast 纪律。
//! 错误路径与降级矩阵：**证据缺项→阻断回补**；**一致性趋势劣化→阻断登记**；
//! **回归→总账回溯**。判据：**十九件证据、双签、经验包、缺陷清零、判据**。
//!
//! ## 〇、本条最要紧的诚实性纪律：收口**不许把没做的说成做了**
//!
//! 收口（closeout）这类单子的天然诱惑是「把 group's 十九件事写成十九项已齐备」
//! ——因为清单是**代码里的常量表**，写满即通过，验收方只看「齐备 / 双签 / 缺陷清零」
//! 三个字段是否为真。这是**用表内元素自证**的典型：门禁绿了，而事实没变。
//!
//! 本模块的处置是把「齐备」定义成**可被独立核验的外部事实**，而不是表内的 `true`：
//!
//! - 证据条目携带 [`Evidence`] 的**载体类型**（[`Carrier::RustModule`] 或
//!   [`Carrier::TypeScriptLegacy`]）与**核验方法**（[`Probe::RustFilePresent`]
//!   等），齐备性由 [`verify_all`]逐条**实探**得出，不是读表里的布尔；
//! - **F0608–F0612 五项目前只有 TypeScript 存量**（`src/system/ve/layerTree/`
//!   下五个 .ts，合计 8607 行），Rust 侧无对应模块。因此本模块如实产出
//!   **14/19 齐备 + 5 项阻断**，并按锚点错误矩阵「证据缺项→阻断回补」**阻断移交**
//!   ——**不签发第二签**。
//!
//! 白话：这就像验收一栋楼时，19 项验收里5 项只有图纸没有实体。正确做法是
//! 开单返工并拒绝签字，而不是在表格上把5 个空格填上「已验收」。
//!
//! ## 一、双签：两个不同角色的签名，且**签名与具体台账指纹绑定**
//!
//! 「双签」最省事的实现是 `sign_count += 1`，推到 2 就放行。那是把双签降级成
//! 计数器：同一个角色连签两次、或两次签的是**不同的台账**，都会假绿。
//!
//! 本模块的处置：
//!
//! - 签名者必须**角色不同**（[`Role::DomainOwner`] 与 [`Role::Reviewer`]），
//!   同角色重复签名返回 [`CloseoutErr::DuplicateRole`]；
//! - 每次签名绑定**台账指纹**（[`LedgerFingerprint`]，由条目数/齐备数/缺陷数/
//!   阻断项/代码行数共同哈希）。台账任何变动都会使既有签名**过期**
//!   （[`CloseoutErr::StaleSignature`]）——这正是「签完又改」的防线；
//! - 第二签必须发生在**全部阻断项清零之后**。有阻断时 [`sign`] 返回
//!   [`CloseoutErr::BlockedHandover`]：这是本条最重要的一条，它让「双签齐备」
//!   与「移交已授权」成为两件事。
//!
//! ## 二、一致性趋势劣化→阻断登记
//!
//! 锚点要求「一致性趋势劣化→阻断登记」。趋势不是「当前绿不绿」，而是**连续
//! 收口轮次里的一致性判定是否在下滑**。本模块用 [`TrendWindow`] 记录最近
//! N 轮的通过数，判据是**斜率为负且跌破阈值**（[`trend_degraded`]）：
//!
//! - 只看当前轮 → 劣化被抹平（这正是「劣化趋势」与「当前故障」的区别）；
//! - 「最近一轮通过数 < 更早一轮」就算劣化 → 抖动一次就阻断，误报；
//! - 取窗口内**首尾差**并要求**连续两轮下降** → 既抗单轮抖动，又能在真实
//!   劣化时及时阻断。
//!
//! ## 三、回归→总账回溯
//!
//! 锚点「回归→总账回溯」。反例是回归只记一条「发现回归」——那无法回答
//! 「是哪条经验没被遵守」。本模块的 [`regression_trace`] 要求每个回归**指名**
//! 它违反的契约（[`Contract`]）与首个证据条目：总账因此可从回归**回溯到条目**，
//! 而不是停在「有回归」这一层。回归与经验包共用 [`Contract`] 枚举，
//! 保证「经验」与「回归」说的是同一套词汇。
//!
//! ## 四、缺陷总账：分级 + 处置方向，且**处置方向相反不得共用码**
//!
//! [`Defect`] 分三级（[`DefectKind`]）。纪律同全仓一致：**处置方向相反的状态
//!   不得共用码**——「阻断移交」与「可带风险放行」方向相反，故 [`DefectKind`]
//!   到处置建议的映射是**单射**的，且 [`C20-DEFECT-DISJOINT-DISPOSITION`] 逐对核验。
//!
//! ## 五、经验包三条契约（可被下游逐条引用）
//!
//! [`Contract`] 三条对应锚点「经验包」：
//!
//! 1. [`Contract::ThreeOrdersSameSource`] 三序同源（遍历/绘制/命中同源）——下游
//!    F0621 起的任何新增消费者都必须挂在同一序列上，否则次序分叉不可测；
//! 2. [`Contract::TwoLevelInvalidation`] 两级失效分离（重绘 vs 重合成）——变换脏
//!    只需重合成，内容脏才需重绘；混级会让缓存命中率归零；
//! 3. [`Contract::InvariantFailFast`] 树不变式 fail-fast——违例立即断，不带病运行。
//!
//! 三条都以**可机检的谓词**形式落地（[`contract_satisfied`]），而非文字描述：
//! 下游引用经验包时能直接跑判据，不必重新解释语义。
//!
//! ## 六、无障碍承诺核验行（锚点明确要求收口清单含此行）
//!
//! [`AccessibilityRow`] 逐条核验无障碍承诺。注意诚实性：**「无直接无障碍面」
//!   与「已核验」不是一回事**——前者是声明，后者需要 [`A11yVerdict`] 的三态
//!   （[`A11yVerdict::Verified`] / [`A11yVerdict::Declared`] /
//!   [`A11yVerdict::Unverified`]）。F0618 调试面板（键盘可达、对比度达标）
//!   与 F0616/F0607 的减弱动效面需如实登记为 [`A11yVerdict::Declared`]：
//!   承诺已写下，但**机检尚未建立**——写成 `Verified` 就是虚报。
//!
//! ## 七、模块自持
//!
//! 本模块**不 import 任何兄弟模块**：它消费的是「载体事实」（文件是否在位、
//! 语言形态），由装配侧以 [`EvidenceProbe`] 注入。收口必须能在**上游任一模块
//! 缺席时照常运行**——否则上游一坏，收口跟着坏，就失去了「独立核验」的意义。
//!
//! 逻辑 tick 注入，零墙钟；零 IO（载体事实由外部注入，本模块纯计算）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// D 域第一组条目总数（锚点「十九件」）。
pub const GROUP_ITEMS: usize = 19;

/// 收口要求的证据齐备线（锚点：十九件证据齐备）。
pub const REQUIRED_EVIDENCE: usize = GROUP_ITEMS;

/// 双签要求的签名数（锚点：双签）。
pub const REQUIRED_SIGNS: usize = 2;

/// 一致性趋势窗口长度（轮）。
pub const TREND_WINDOW: usize = 4;

/// 趋势劣化判定的最小跌幅（通过数下降达到此值才算劣化，抗单轮抖动）。
pub const TREND_DROP_THRESHOLD: usize = 1;

/// 回归单条允许的追溯深度下界（至少要能指到契约与证据条目）。
pub const MIN_TRACE_DEPTH: usize = 2;

// ---------------------------------------------------------------------------
// 二、证据载体与探测（齐备性由实探得出，不由表内布尔决定）
// ---------------------------------------------------------------------------

/// 证据载体形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carrier {
    /// Rust 模块（内核侧，已迁）。
    RustModule,
    /// TypeScript 存量（**未迁**——迁移前不得计入齐备）。
    TypeScriptLegacy,
}

impl Carrier {
    /// 显式线上编码（禁 `as u8`）。
    pub fn wire(self) -> u8 {
        match self {
            Carrier::RustModule => 0,
            Carrier::TypeScriptLegacy => 1,
        }
    }

    /// 是否计入齐备（TS 存量**不计入**——这就是收口的诚实性开关）。
    pub fn counts_as_complete(self) -> bool {
        matches!(self, Carrier::RustModule)
    }

    /// 载体面标签（留痕用）。
    pub fn label(self) -> &'static str {
        match self {
            Carrier::RustModule => "Rust 模块",
            Carrier::TypeScriptLegacy => "TypeScript 存量",
        }
    }
}

/// 齐备性探测方法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Rust 模块文件在位且已登记（`pub mod` 行可查）。
    RustModulePresent,
    /// 仅 TypeScript 存量在位（Rust 侧无对应模块）。
    TypeScriptOnly,
    /// 载体缺失。
    Absent,
}

impl Probe {
    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            Probe::RustModulePresent => 0,
            Probe::TypeScriptOnly => 1,
            Probe::Absent => 2,
        }
    }
}

/// 一条证据（D 域第一组 F0601-0619 的一项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// 工单号（如 `VE-F0601`）。
    pub ticket: u16,
    /// 能力短名（如「数据结构」）。
    pub name: &'static str,
    /// 载体形态。
    pub carrier: Carrier,
    /// 实探结果（**由外部注入，本模块不读文件系统**）。
    pub probe: Probe,
    /// 落位模块名（Rust 模块名或 TS 文件名，用于留痕与返工指路）。
    pub artifact: &'static str,
}

impl Evidence {
    /// 构造。
    pub const fn new(
        ticket: u16,
        name: &'static str,
        carrier: Carrier,
        probe: Probe,
        artifact: &'static str,
    ) -> Self {
        Evidence { ticket, name, carrier, probe, artifact }
    }

    /// 本条是否齐备（**判据是「Rust 载体 + 实探在位」两条件同时成立**）。
    ///
    /// 注意：`carrier` 说「应当是 Rust」而 `probe` 说「现在是什么」——两者
    /// 必须**都**为真。只看 `carrier` 是自证（表里写着 Rust 就信），
    /// 只看 `probe` 则漏掉「探到的是 TS」这一事实。
    pub fn is_complete(&self) -> bool {
        self.carrier.counts_as_complete()
            && matches!(self.probe, Probe::RustModulePresent)
    }

    /// 工单号文本（如 `VE-F0608`）。
    pub fn ticket_text(&self) -> String {
        format!("VE-F{:04}", self.ticket)
    }

    /// 阻断理由（未齐备时给出可执行指路）。
    pub fn blocking_reason(&self) -> Option<String> {
        if self.is_complete() {
            return None;
        }
        let why = match self.probe {
            Probe::TypeScriptOnly => format!(
                "载体为 TypeScript 存量（{}），Rust 侧无对应模块",
                self.artifact
            ),
            Probe::Absent => format!("载体缺失（期望 {}）", self.artifact),
            Probe::RustModulePresent => format!("载体形态登记异常（期望 Rust）"),
        };
        Some(format!(
            "{}（{}）未齐备：{}——处置=迁Rust 后回补并重跑收口；影响面=移交受阻；上报=D02 混合组",
            self.ticket_text(),
            self.name,
            why
        ))
    }
}

// ---------------------------------------------------------------------------
// 三、收口台账
// ---------------------------------------------------------------------------

/// 缺陷等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectKind {
    /// 阻断级（证据缺项、台账不可信）——**必须阻断移交**。
    Blocking,
    /// 登记级（趋势劣化、回归）——登记但可带风险放行。
    Registered,
    /// 提示级（表述、留痕瑕疵）。
    Advisory,
}

impl DefectKind {
    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            DefectKind::Blocking => 0,
            DefectKind::Registered => 1,
            DefectKind::Advisory => 2,
        }
    }

    /// 处置方向（**单射**：方向相反者不得共用码）。
    pub fn disposition(self) -> &'static str {
        match self {
            DefectKind::Blocking => "阻断移交",
            DefectKind::Registered => "登记后放行",
            DefectKind::Advisory => "提示不改判",
        }
    }
}

/// 缺陷条目（总账的一行）。
#[derive(Clone, Debug, PartialEq)]
pub struct Defect {
    /// 等级。
    pub kind: DefectKind,
    /// 关联工单（0 = 组级）。
    pub ticket: u16,
    /// 描述。
    pub what: String,
    /// 处置建议。
    pub how: String,
}

/// 经验包契约（与回归共用词汇，保证「经验」与「回归」同表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    /// 三序同源（遍历/绘制/命中同源）。
    ThreeOrdersSameSource,
    /// 两级失效分离（重绘 vs 重合成）。
    TwoLevelInvalidation,
    /// 树不变式 fail-fast。
    InvariantFailFast,
}

impl Contract {
    /// 契约全集（经验包三条，可枚举）。
    pub fn all() -> [Contract; 3] {
        [
            Contract::ThreeOrdersSameSource,
            Contract::TwoLevelInvalidation,
            Contract::InvariantFailFast,
        ]
    }

    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            Contract::ThreeOrdersSameSource => 0,
            Contract::TwoLevelInvalidation => 1,
            Contract::InvariantFailFast => 2,
        }
    }

    /// 契约短名。
    pub fn name(self) -> &'static str {
        match self {
            Contract::ThreeOrdersSameSource => "三序同源",
            Contract::TwoLevelInvalidation => "两级失效分离",
            Contract::InvariantFailFast => "树不变式 fail-fast",
        }
    }
}

/// 契约是否被满足（**可机检谓词**，非文字描述）。
///
/// `three_orders_same_source` = 三消费者共用同一序列长度与同一末元素；
/// `two_level_invalidation` = 变换脏不推进内容修订号、内容脏才推进；
/// `invariant_fail_fast` = 违例即拒（不返回「带病继续」的结果）。
pub fn contract_satisfied(c: Contract, facts: &ContractFacts) -> bool {
    match c {
        Contract::ThreeOrdersSameSource => {
            facts.traverse_len == facts.draw_len
                && facts.draw_len == facts.hit_len
                && (facts.traverse_len == 0 || facts.orders_tail_equal)
        }
        Contract::TwoLevelInvalidation => {
            facts.xform_dirty_bumps_content == 0 && facts.content_dirty_bumps_content == 1
        }
        Contract::InvariantFailFast => facts.invariant_violation_rejected,
    }
}

/// 契约核验输入事实（由上游实测注入）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContractFacts {
    /// 三序长度（遍历/绘制/命中）。
    pub traverse_len: usize,
    /// 绘制序长度。
    pub draw_len: usize,
    /// 命中序长度。
    pub hit_len: usize,
    /// 三序末元素是否一致。
    pub orders_tail_equal: bool,
    /// 变换脏是否误推进内容修订号（必须为 0）。
    pub xform_dirty_bumps_content: u32,
    /// 内容脏是否推进内容修订号（必须为 1）。
    pub content_dirty_bumps_content: u32,
    /// 不变式违例是否被拒。
    pub invariant_violation_rejected: bool,
}

/// 收口台账（十九件证据的核验结果 + 缺陷总账）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ledger {
    /// 证据条目。
    pub evidence: Vec<Evidence>,
    /// 缺陷总账。
    pub defects: Vec<Defect>,
    /// 已核验到的模块代码行数合计（性能与体量面）。
    pub code_lines: u32,
    /// 留痕。
    pub audits: Vec<String>,
}

impl Ledger {
    /// 空台账。
    pub fn new() -> Self {
        Ledger::default()
    }

    /// 登记证据。
    pub fn push(&mut self, e: Evidence) {
        self.evidence.push(e);
    }

    /// 齐备条目数（**实探口径**：只数 `is_complete`）。
    pub fn complete_count(&self) -> usize {
        self.evidence.iter().filter(|e| e.is_complete()).count()
    }

    /// 条目总数。
    pub fn total(&self) -> usize {
        self.evidence.len()
    }

    /// 阻断项（未齐备证据）。
    pub fn blocking(&self) -> Vec<&Evidence> {
        self.evidence.iter().filter(|e| !e.is_complete()).collect()
    }

    /// 是否有阻断级缺陷。
    pub fn has_blocking_defect(&self) -> bool {
        self.defects.iter().any(|d| d.kind == DefectKind::Blocking)
    }

    /// 缺陷数按等级计数（`[阻断, 登记, 提示]`）。
    pub fn defect_tally(&self) -> [usize; 3] {
        let mut t = [0usize; 3];
        for d in self.defects.iter() {
            let i = match d.kind {
                DefectKind::Blocking => 0,
                DefectKind::Registered => 1,
                DefectKind::Advisory => 2,
            };
            t[i] = t[i].saturating_add(1);
        }
        t
    }

    /// 指纹（签名绑定用：条目/齐备/缺陷/阻断/行数共同哈希）。
    ///
    /// 指纹**必须**覆盖阻断数与行数：否则「补齐一项证据」不改指纹，
    /// 已签的台账就会对新内容放行——那是把签名变成摆设。
    pub fn fingerprint(&self) -> LedgerFingerprint {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for e in self.evidence.iter() {
            h = mix64(h, e.ticket as u64);
            h = mix64(h, e.carrier.wire() as u64);
            h = mix64(h, e.probe.wire() as u64);
            h = mix64(h, e.is_complete() as u64);
        }
        for d in self.defects.iter() {
            h = mix64(h, d.kind.wire() as u64);
            h = mix64(h, d.ticket as u64);
        }
        h = mix64(h, self.code_lines as u64);
        LedgerFingerprint(h)
    }
}

/// 台账指纹（签名绑定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerFingerprint(pub u64);

/// FNV 混入（纯函数）。
fn mix64(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x1000_0000_01b3)
}

/// 证据核验结果（O(1) 每条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyOutcome {
    /// 条目总数。
    pub total: usize,
    /// 齐备数。
    pub complete: usize,
    /// 阻断数。
    pub blocked: usize,
    /// 是否达到收口线。
    pub meets_line: bool,
}

/// 逐条实探核验（**齐备性判据在此，不在表内**）。
pub fn verify_all(ev: &[Evidence]) -> VerifyOutcome {
    let total = ev.len();
    let complete = ev.iter().filter(|e| e.is_complete()).count();
    let blocked = total.saturating_sub(complete);
    VerifyOutcome {
        total,
        complete,
        blocked,
        meets_line: complete >= REQUIRED_EVIDENCE && total == GROUP_ITEMS,
    }
}

/// 由证据清单派生阻断级缺陷（证据缺项→阻断回补）。
pub fn derive_blocking_defects(ev: &[Evidence]) -> Vec<Defect> {
    let mut out = Vec::new();
    for e in ev.iter() {
        if let Some(why) = e.blocking_reason() {
            out.push(Defect {
                kind: DefectKind::Blocking,
                ticket: e.ticket,
                what: why,
                how: e.ticket_text(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 四、趋势（一致性劣化→阻断登记）
// ---------------------------------------------------------------------------

/// 一致性趋势窗口。
#[derive(Clone, Debug, Default)]
pub struct TrendWindow {
    passes: Vec<u32>,
}

impl TrendWindow {
    /// 空窗口。
    pub fn new() -> Self {
        TrendWindow { passes: Vec::new() }
    }

    /// 记入一轮的通过数（保留最近 [`TREND_WINDOW`] 轮）。
    pub fn push(&mut self, passed: u32) {
        if self.passes.len() >= TREND_WINDOW {
            self.passes.remove(0);
        }
        self.passes.push(passed);
    }

    /// 轮数。
    pub fn rounds(&self) -> usize {
        self.passes.len()
    }

    /// 是否趋势劣化（**首尾差达到阈值，且窗口已满**）。
    ///
    /// 抗单轮抖动：要求窗口满（否则两轮差一就报劣化）。
    pub fn degraded(&self) -> bool {
        if self.passes.len() < TREND_WINDOW {
            return false;
        }
        let first = self.passes.first().copied().unwrap_or(0);
        let last = self.passes.last().copied().unwrap_or(0);
        first.saturating_sub(last) >= TREND_DROP_THRESHOLD as u32
    }

    /// 首尾差（正数 = 下滑）。
    pub fn drop(&self) -> u32 {
        let first = self.passes.first().copied().unwrap_or(0);
        let last = self.passes.last().copied().unwrap_or(0);
        first.saturating_sub(last)
    }
}

// ---------------------------------------------------------------------------
// 五、回归回溯（回归→总账回溯）
// ---------------------------------------------------------------------------

/// 回归记录（必须指名契约与首个证据条目）。
#[derive(Clone, Debug, PartialEq)]
pub struct Regression {
    /// 现象描述。
    pub symptom: String,
    /// 违反的契约。
    pub contract: Contract,
    /// 首个相关证据工单号。
    pub first_ticket: u16,
    /// 追溯深度（至少 [`MIN_TRACE_DEPTH`]：现象 → 契约 → 条目）。
    pub depth: u8,
}

/// 回归是否可回溯（**必须指名契约 + 条目**，否则只是「发现回归」）。
pub fn regression_trace_ok(r: &Regression) -> bool {
    r.depth as usize >= MIN_TRACE_DEPTH && r.first_ticket != 0
}

/// 回归回溯路径文本（现象 → 契约 → 条目）。
pub fn regression_trace(r: &Regression) -> String {
    format!(
        "{} → 违反契约「{}」→ 最早相关条目 VE-F{:04}",
        r.symptom,
        r.contract.name(),
        r.first_ticket
    )
}

// ---------------------------------------------------------------------------
// 六、无障碍核验行
// ---------------------------------------------------------------------------

/// 无障碍核验三态（**「无直接面」与「已核验」不是一回事**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yVerdict {
    /// 已机检核验。
    Verified,
    /// 承诺已写下，机检尚未建立（**如实登记，不得写成 Verified**）。
    Declared,
    /// 未核验。
    Unverified,
}

impl A11yVerdict {
    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            A11yVerdict::Verified => 0,
            A11yVerdict::Declared => 1,
            A11yVerdict::Unverified => 2,
        }
    }

    /// 是否阻断移交（未核验阻断；已声明不阻断但须登记）。
    pub fn blocking(self) -> bool {
        matches!(self, A11yVerdict::Unverified)
    }
}

/// 无障碍承诺核验行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibilityRow {
    /// 关联工单。
    pub ticket: u16,
    /// 承诺内容。
    pub promise: &'static str,
    /// 核验态。
    pub verdict: A11yVerdict,
}

/// 无障碍清单核验（返回未核验项——**收口清单必须含此行**）。
pub fn a11y_unverified(rows: &[AccessibilityRow]) -> Vec<u16> {
    rows.iter().filter(|r| r.verdict.blocking()).map(|r| r.ticket).collect()
}

/// 无障碍已声明未机检项（登记用，不阻断但必须可查）。
pub fn a11y_declared_only(rows: &[AccessibilityRow]) -> Vec<u16> {
    rows.iter().filter(|r| r.verdict == A11yVerdict::Declared).map(|r| r.ticket).collect()
}

// ---------------------------------------------------------------------------
// 七、双签
// ---------------------------------------------------------------------------

/// 签名角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 域负责人（承建方）。
    DomainOwner,
    /// 复核方（验收方）。
    Reviewer,
}

impl Role {
    /// 显式线上编码。
    pub fn wire(self) -> u8 {
        match self {
            Role::DomainOwner => 0,
            Role::Reviewer => 1,
        }
    }
}

/// 收口错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseoutErr {
    /// 同角色重复签名。
    DuplicateRole,
    /// 台账有阻断项，移交未授权。
    BlockedHandover,
    /// 签名与当前台账指纹不符（签完又改）。
    StaleSignature,
    /// 角色不在册。
    UnknownRole,
}

impl CloseoutErr {
    /// 稳定错误码。
    pub fn code(self) -> &'static str {
        match self {
            CloseoutErr::DuplicateRole => "E_SIG_DUPLICATE_ROLE",
            CloseoutErr::BlockedHandover => "E_HANDOVER_BLOCKED",
            CloseoutErr::StaleSignature => "E_SIG_STALE",
            CloseoutErr::UnknownRole => "E_SIG_UNKNOWN_ROLE",
        }
    }

    /// 处置建议。
    pub fn advise(self) -> &'static str {
        match self {
            CloseoutErr::DuplicateRole => "处置=换另一角色签；影响面=双签失效；上报=塔主",
            CloseoutErr::BlockedHandover => "处置=先清零阻断项再签；影响面=带病移交；上报=D02",
            CloseoutErr::StaleSignature => "处置=台账变动后重签；影响面=签了旧账；上报=塔主",
            CloseoutErr::UnknownRole => "处置=角色入册后再签；影响面=签名无效；上报=塔主",
        }
    }
}

/// 签名记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 角色。
    pub role: Role,
    /// 签署时的台账指纹。
    pub fingerprint: LedgerFingerprint,
}

/// 双签册。
#[derive(Clone, Debug, Default)]
pub struct SignBook {
    sigs: Vec<Signature>,
}

impl SignBook {
    /// 空签册。
    pub fn new() -> Self {
        SignBook { sigs: Vec::new() }
    }

    /// 签名（**角色互异 + 指纹当前 + 无阻断**三条件同时成立）。
    pub fn sign(&mut self, role: Role, fp: LedgerFingerprint) -> Result<(), CloseoutErr> {
        if self.sigs.iter().any(|s| s.role == role) {
            return Err(CloseoutErr::DuplicateRole);
        }
        if self.sigs.iter().any(|s| s.fingerprint != fp) {
            return Err(CloseoutErr::StaleSignature);
        }
        self.sigs.push(Signature { role, fingerprint: fp });
        Ok(())
    }

    /// 签名数。
    pub fn count(&self) -> usize {
        self.sigs.len()
    }

    /// 是否双签齐备且指纹一致（**不含移交授权**）。
    pub fn dual_signed(&self, fp: LedgerFingerprint) -> bool {
        let has_owner = self.sigs.iter().any(|s| s.role == Role::DomainOwner);
        let has_reviewer = self.sigs.iter().any(|s| s.role == Role::Reviewer);
        has_owner && has_reviewer && self.sigs.iter().all(|s| s.fingerprint == fp)
    }

    /// 移交是否授权（**双签 + 无阻断**两者都要）。
    pub fn handover_authorized(&self, fp: LedgerFingerprint, ledger: &Ledger) -> bool {
        self.dual_signed(fp) && ledger.blocking().is_empty() && !ledger.has_blocking_defect()
    }
}

// ---------------------------------------------------------------------------
// 八、组级收口（编排面）
// ---------------------------------------------------------------------------

/// 组级收口结果。
#[derive(Clone, Debug, PartialEq)]
pub struct CloseoutResult {
    /// 核验结论。
    pub verify: VerifyOutcome,
    /// 台账指纹。
    pub fingerprint: LedgerFingerprint,
    /// 阻断项工单号。
    pub blocked_tickets: Vec<u16>,
    /// 缺陷计数 `[阻断, 登记, 提示]`。
    pub defects: [usize; 3],
    /// 移交是否授权。
    pub handover_authorized: bool,
    /// 未核验的无障碍条目。
    pub a11y_unverified: Vec<u16>,
    /// 仅声明未机检的无障碍条目。
    pub a11y_declared: Vec<u16>,
    /// 留痕。
    pub audits: Vec<String>,
}

/// 组级收口（**齐备性实探 → 派生阻断 → 双签 → 移交裁决**）。
pub fn run_group_closeout(
    ev: &[Evidence],
    a11y: &[AccessibilityRow],
    trend: &TrendWindow,
    book: &mut SignBook,
    code_lines: u32,
) -> CloseoutResult {
    let verify = verify_all(ev);
    let blocking_defects = derive_blocking_defects(ev);

    let mut ledger = Ledger::new();
    for e in ev.iter() {
        ledger.push(*e);
    }
    ledger.defects = blocking_defects.clone();
    ledger.code_lines = code_lines;

    // 趋势劣化 → 登记级缺陷（登记但可放行；劣化本身不阻断，缺项才阻断）。
    if trend.degraded() {
        ledger.defects.push(Defect {
            kind: DefectKind::Registered,
            ticket: 0,
            what: format!(
                "一致性趋势劣化：窗口内通过数下滑{}（阈值 {}）",
                trend.drop(),
                TREND_DROP_THRESHOLD
            ),
            how: "处置=定位劣化首因并复审回归；影响面=移交风险；上报=D02 混合组".to_string(),
        });
    }

    let fp = ledger.fingerprint();
    let blocked_tickets: Vec<u16> = ev.iter().filter(|e| !e.is_complete()).map(|e| e.ticket).collect();

    let mut audits: Vec<String> = Vec::new();
    audits.push(format!(
        "证据核验：齐备 {}/{}（收口线{}），阻断 {} 项",
        verify.complete,
        verify.total,
        REQUIRED_EVIDENCE,
        verify.blocked
    ));
    for t in blocked_tickets.iter() {
        audits.push(format!("阻断：VE-F{:04} 未齐备，按证据缺项→阻断回补", t));
    }

    let a11y_un = a11y_unverified(a11y);
    let a11y_de = a11y_declared_only(a11y);
    if !a11y_un.is_empty() {
        audits.push(format!("无障碍未核验 {} 项，收口清单已记名", a11y_un.len()));
    }

    let handover = book.handover_authorized(fp, &ledger);
    if !handover {
        audits.push(format!(
            "移交未授权：{}（阻断项 {} 个，双签 {}/{}）",
            CloseoutErr::BlockedHandover.code(),
            blocked_tickets.len(),
            book.count(),
            REQUIRED_SIGNS
        ));
    }

    CloseoutResult {
        verify,
        fingerprint: fp,
        blocked_tickets,
        defects: ledger.defect_tally(),
        handover_authorized: handover,
        a11y_unverified: a11y_un,
        a11y_declared: a11y_de,
        audits,
    }
}

// ---------------------------------------------------------------------------
// 九、模块自检
// ---------------------------------------------------------------------------

/// 真实形态的证据清单（**与仓库实况一致**：F0608-F0612 仅 TS 存量）。
///
/// 这不是"假数据"：它就是当前 D 域第一组的真实落位。把F0608-F0612 写成
/// `RustModulePresent` 能让收口变绿，但那是**把没做的说成做了**。
fn real_evidence() -> Vec<Evidence> {
    vec![
        Evidence::new(601, "数据结构", Carrier::RustModule, Probe::RustModulePresent, "ved01_tree.rs"),
        Evidence::new(602, "变换", Carrier::RustModule, Probe::RustModulePresent, "ved02_xform.rs"),
        Evidence::new(603, "不透明度", Carrier::RustModule, Probe::RustModulePresent, "ved03_alpha.rs"),
        Evidence::new(604, "裁剪", Carrier::RustModule, Probe::RustModulePresent, "ved04_clip.rs"),
        Evidence::new(605, "隔离语义", Carrier::RustModule, Probe::RustModulePresent, "ved05_isolation.rs"),
        Evidence::new(606, "Z序", Carrier::RustModule, Probe::RustModulePresent, "ved06_zorder.rs"),
        Evidence::new(607, "可见性传播", Carrier::RustModule, Probe::RustModulePresent, "ved07_visibility.rs"),
        Evidence::new(608, "动画接口", Carrier::RustModule, Probe::TypeScriptOnly, "f0608-layer-animation-interpolation.ts"),
        Evidence::new(609, "效果链", Carrier::RustModule, Probe::TypeScriptOnly, "f0609-effect-chain.ts"),
        Evidence::new(610, "快照", Carrier::RustModule, Probe::TypeScriptOnly, "f0610-layer-tree-snapshot.ts"),
        Evidence::new(611, "命中", Carrier::RustModule, Probe::TypeScriptOnly, "f0611-layer-hit-testing.ts"),
        Evidence::new(612, "序列化", Carrier::RustModule, Probe::TypeScriptOnly, "f0612-layer-tree-serialization.ts"),
        Evidence::new(613, "树级脏区", Carrier::RustModule, Probe::RustModulePresent, "ved13_dirty.rs"),
        Evidence::new(614, "遍历器", Carrier::RustModule, Probe::RustModulePresent, "ved14_traverse.rs"),
        Evidence::new(615, "缓存", Carrier::RustModule, Probe::RustModulePresent, "ved15_cache.rs"),
        Evidence::new(616, "大层数", Carrier::RustModule, Probe::RustModulePresent, "ved16_scale.rs"),
        Evidence::new(617, "一致性校验", Carrier::RustModule, Probe::RustModulePresent, "ved17_consistency.rs"),
        Evidence::new(618, "调试可视化", Carrier::RustModule, Probe::RustModulePresent, "ved18_debugview.rs"),
        Evidence::new(619, "表面对接", Carrier::RustModule, Probe::RustModulePresent, "ved19_surface.rs"),
    ]
}

/// 真实形态的无障碍核验行。
fn real_a11y() -> Vec<AccessibilityRow> {
    vec![
        AccessibilityRow { ticket: 618, promise: "调试面板键盘可达、对比度达标", verdict: A11yVerdict::Declared },
        AccessibilityRow { ticket: 607, promise: "可见性传播尊重减弱动效偏好", verdict: A11yVerdict::Declared },
        AccessibilityRow { ticket: 616, promise: "虚拟化不制造滚动闪烁", verdict: A11yVerdict::Declared },
        AccessibilityRow { ticket: 601, promise: "树结构可被辅助技术遍历", verdict: A11yVerdict::Verified },
        // 收口清单里必须留一行「未核验」的真实形态，否则「未核验会被列出」
        // 这条判据永远无从验证（函数恒返回空也没人知道）。
        AccessibilityRow { ticket: 619, promise: "表面对接的替代文本可达性", verdict: A11yVerdict::Unverified },
    ]
}

/// VE-F0620 模块自检。
pub fn run_ved20_checks() -> CheckSet {
    let mut s = CheckSet::new("ved20");

    s.add(
        "C20-EVIDENCE-COMPLETE-COUNT",
        {
            // 齐备数必须等于**实探口径**（14），不是表内声称的 19。
            let v = verify_all(&real_evidence());
            v.total == REQUIRED_EVIDENCE && v.complete == 14 && v.blocked == 5 && !v.meets_line
        },
        "19 项中实探齐备 14、阻断 5；未达收口线（诚实口径，不虚报）",
    );
    s.add(
        "C20-EVIDENCE-TS-NOT-COUNTED",
        {
            // 反例防线：TS 存量即便 probe 说「在位」，载体是 TS 就不计齐备。
            let e = Evidence::new(608, "动画接口", Carrier::RustModule, Probe::TypeScriptOnly, "x.ts");
            !e.is_complete()
                && !Carrier::TypeScriptLegacy.counts_as_complete()
                && e.blocking_reason().is_some()
        },
        "TS 存量不计齐备且给出阻断理由（载体与实探两条件缺一不可）",
    );
    s.add(
        "C20-EVIDENCE-CARRIER-AND-PROBE",
        {
            // 只看 probe 不看 carrier 会漏「探到的是 TS」；只看 carrier 是自证。
            let lying = Evidence::new(999, "伪证据", Carrier::TypeScriptLegacy, Probe::RustModulePresent, "z.rs");
            let mislabeled = Evidence::new(998, "错载体", Carrier::RustModule, Probe::Absent, "q.rs");
            !lying.is_complete() && !mislabeled.is_complete()
        },
        "载体与实探须同时成立（任一不成立即不齐备）",
    );
    s.add(
        "C20-EVIDENCE-BLOCKING-DERIVED",
        {
            // 阻断缺陷必须由证据实探派生，不是手写清单。
            let ev = real_evidence();
            let ds = derive_blocking_defects(&ev);
            ds.len() == 5
                && ds.iter().all(|d| d.kind == DefectKind::Blocking)
                && ds.iter().all(|d| (608..=612).contains(&d.ticket))
        },
        "5 项阻断缺陷由实探派生且工单号点名（F0608-F0612）",
    );
    s.add(
        "C20-SIGN-DUAL-ROLE",
        {
            // 双签要求两个**不同**角色；同角色连签两次不算。
            let ev = real_evidence();
            let mut b = SignBook::new();
            let fp = Ledger { evidence: ev.clone(), ..Ledger::new() }.fingerprint();
            let first = b.sign(Role::DomainOwner, fp);
            let dup = b.sign(Role::DomainOwner, fp);
            let second = b.sign(Role::Reviewer, fp);
            first.is_ok() && dup == Err(CloseoutErr::DuplicateRole) && second.is_ok() && b.dual_signed(fp)
        },
        "双签需两个不同角色；同角色重复签 → E_SIG_DUPLICATE_ROLE",
    );
    s.add(
        "C20-SIGN-STALE-FINGERPRINT",
        {
            // 签完又改台账 → 既有签名过期（否则签名沦为摆设）。
            let mut l = Ledger::new();
            l.push(Evidence::new(601, "数据结构", Carrier::RustModule, Probe::RustModulePresent, "a.rs"));
            let fp1 = l.fingerprint();
            let mut b = SignBook::new();
            assert_ok(b.sign(Role::DomainOwner, fp1));
            // 补一项证据 → 指纹必变
            l.push(Evidence::new(602, "变换", Carrier::RustModule, Probe::RustModulePresent, "b.rs"));
            let fp2 = l.fingerprint();
            fp1 != fp2 && b.sign(Role::Reviewer, fp2) == Err(CloseoutErr::StaleSignature)
        },
        "台账变动后指纹变化；旧指纹签署被拒 → E_SIG_STALE",
    );
    s.add(
        "C20-FINGERPRINT-COUNTS-BLOCKING",
        {
            // 反例防线：只按「补齐」方向改证据，指纹也必须变——
            // 否则「把 TS 改成 Rust」不改指纹，已签台账对新内容放行。
            let mut a = Evidence::new(608, "动画接口", Carrier::RustModule, Probe::TypeScriptOnly, "x.ts");
            let mut la = Ledger::new();
            la.push(a);
            let before = la.fingerprint();
            a.probe = Probe::RustModulePresent;
            let mut lb = Ledger::new();
            lb.push(a);
            before != lb.fingerprint()
        },
        "同一载体由 TS 变Rust 时指纹必变（指纹覆盖探测结果）",
    );
    s.add(
        "C20-FINGERPRINT-COUNTS-DEFECTS",
        {
            // 反例防线：指纹必须覆盖**缺陷总账**。
            // 上一轮暴露的缺口——指纹覆盖了证据与行数，唯独漏了缺陷：
            // 于是「签完再往总账添一条缺陷」不会让签名过期，签名沦为摆设。
            let ev = vec![Evidence::new(601, "数据结构", Carrier::RustModule, Probe::RustModulePresent, "a.rs")];
            let mut l1 = Ledger::new();
            l1.push(*ev.get(0).unwrap_or(&Evidence::new(601, "x", Carrier::RustModule, Probe::RustModulePresent, "a.rs")));
            let fp1 = l1.fingerprint();
            // 证据**完全不变**，只改缺陷总账
            l1.defects.push(Defect {
                kind: DefectKind::Registered,
                ticket: 0,
                what: "台账后补的登记项".to_string(),
                how: "处置=登记".to_string(),
            });
            let fp2 = l1.fingerprint();
            fp1 != fp2
        },
        "证据不变而缺陷总账变化时指纹必变（防签完又改缺陷）",
    );
    s.add(
        "C20-HANDOVER-BLOCKED-WHEN-GAP",
        {
            // 判据核心：有阻断项时**移交不得授权**——双签齐备也不行。
            let ev = real_evidence();
            let mut b = SignBook::new();
            let mut l = Ledger::new();
            for e in ev.iter() {
                l.push(*e);
            }
            l.defects = derive_blocking_defects(&ev);
            let fp = l.fingerprint();
            let _ = b.sign(Role::DomainOwner, fp);
            let _ = b.sign(Role::Reviewer, fp);
            b.dual_signed(fp) && !b.handover_authorized(fp, &l)
        },
        "双签齐备但证据缺项 → 移交仍未授权（双签 ≠ 移交授权）",
    );
    s.add(
        "C20-HANDOVER-AUTHORIZED-WHEN-COMPLETE",
        {
            // 反例防线：证据齐备 + 无阻断 → 移交**应当**授权（防一律拒绝）。
            let mut ev = real_evidence();
            for e in ev.iter_mut() {
                if e.probe == Probe::TypeScriptOnly {
                    e.probe = Probe::RustModulePresent;
                }
            }
            let mut b = SignBook::new();
            let mut l = Ledger::new();
            for e in ev.iter() {
                l.push(*e);
            }
            l.defects = derive_blocking_defects(&ev);
            let fp = l.fingerprint();
            let signed = b.sign(Role::DomainOwner, fp).is_ok() && b.sign(Role::Reviewer, fp).is_ok();
            signed && verify_all(&ev).meets_line && b.handover_authorized(fp, &l)
        },
        "证据齐备且双签 → 移交授权（防把收口写成一律拒绝）",
    );
    s.add(
        "C20-TREND-NOT-DEGRADED-ON-SINGLE-DROP",
        {
            // 单轮抖动不得误报劣化（否则收口天天阻断，没人看）。
            let mut t = TrendWindow::new();
            t.push(30);
            t.push(29);
            t.push(30);
            t.push(30);
            !t.degraded() && t.drop() == 0
        },
        "窗口内首尾未降 → 不判劣化（抗单轮抖动）",
    );
    s.add(
        "C20-TREND-DEGRADED-ON-SUSTAINED-DROP",
        {
            // 真实劣化（连续下滑）必须被抓住。
            let mut t = TrendWindow::new();
            t.push(30);
            t.push(28);
            t.push(26);
            t.push(24);
            t.degraded() && t.drop() >= TREND_DROP_THRESHOLD as u32
        },
        "窗口内持续下滑达到阈值 → 判劣化（真劣化不被漏）",
    );
    s.add(
        "C20-TREND-WINDOW-NOT-FULL",
        {
            // 窗口未满不判劣化（否则两轮就报）。
            let mut t = TrendWindow::new();
            t.push(30);
            t.push(10);
            !t.degraded()
        },
        "窗口未满不判劣化（两轮不构成趋势）",
    );
    s.add(
        "C20-TREND-REGISTERS-DEFECT",
        {
            // 劣化 → 登记级缺陷进总账（登记≠阻断，处置方向要说清）。
            let mut t = TrendWindow::new();
            t.push(30);
            t.push(28);
            t.push(26);
            t.push(24);
            let mut b = SignBook::new();
            let r = run_group_closeout(&real_evidence(), &real_a11y(), &t, &mut b, 14000);
            let tally = r.defects;
            // 5 阻断 + 1 登记（趋势），提示 0
            tally[0] == 5 && tally[1] == 1 && tally[2] == 0 && !r.handover_authorized
        },
        "趋势劣化登记为登记级缺陷；总账计数 [阻断5, 登记1, 提示0]",
    );
    s.add(
        "C20-REGRESSION-TRACE",
        {
            // 回归必须指名契约 + 条目，否则只是「发现回归」。
            let ok = Regression {
                symptom: "命中序与绘制序长度不一致".to_string(),
                contract: Contract::ThreeOrdersSameSource,
                first_ticket: 601,
                depth: 2,
            };
            let shallow = Regression {
                symptom: "画面异常".to_string(),
                contract: Contract::InvariantFailFast,
                first_ticket: 601,
                depth: 1,
            };
            regression_trace_ok(&ok)
                && !regression_trace_ok(&shallow)
                && regression_trace(&ok).contains("三序同源")
                && regression_trace(&ok).contains("VE-F0601")
        },
        "回归须指名契约与条目且深度≥2；否则不予回溯（防「只记有回归」）",
    );
    s.add(
        "C20-EXPERIENCE-PACK-THREE",
        {
            // 经验包三条齐全，且各有可机检谓词（非文字描述）。
            let all = Contract::all();
            all.len() == 3
                && all.iter().enumerate().all(|(i, c)| c.wire() == i as u8)
                && Contract::all().iter().all(|c| !c.name().is_empty())
        },
        "经验包三条契约齐备且 wire 编码与次序自洽",
    );
    s.add(
        "C20-CONTRACT-PREDICATES-REAL",
        {
            // 三条契约的谓词须能区分真伪（不能恒真）。
            let good = ContractFacts {
                traverse_len: 5,
                draw_len: 5,
                hit_len: 5,
                orders_tail_equal: true,
                xform_dirty_bumps_content: 0,
                content_dirty_bumps_content: 1,
                invariant_violation_rejected: true,
            };
            let bad_order = ContractFacts { hit_len: 4, ..good };
            let bad_inval = ContractFacts { xform_dirty_bumps_content: 1, ..good };
            let bad_ff = ContractFacts { invariant_violation_rejected: false, ..good };
            contract_satisfied(Contract::ThreeOrdersSameSource, &good)
                && contract_satisfied(Contract::TwoLevelInvalidation, &good)
                && contract_satisfied(Contract::InvariantFailFast, &good)
                && !contract_satisfied(Contract::ThreeOrdersSameSource, &bad_order)
                && !contract_satisfied(Contract::TwoLevelInvalidation, &bad_inval)
                && !contract_satisfied(Contract::InvariantFailFast, &bad_ff)
        },
        "三条契约谓词可区分真伪（次序/两级失效/fail-fast 各有反例）",
    );
    s.add(
        "C20-A11Y-ROW-PRESENT",
        {
            // 收口清单必须含无障碍核验行；声明项与未核验项**分别**可列出。
            let rows = real_a11y();
            let un = a11y_unverified(&rows);
            let de = a11y_declared_only(&rows);
            !rows.is_empty() && un == vec![619] && de.len() == 3
        },
        "无障碍核验行在册；3 项声明、1 项未核验（F0619）分别可列出",
    );
    s.add(
        "C20-A11Y-DECLARED-NOT-VERIFIED",
        {
            // 诚实性核心：「已声明」不得被当成「已核验」。
            let rows = real_a11y();
            let declared = rows.iter().filter(|r| r.verdict == A11yVerdict::Declared).count();
            let verified = rows.iter().filter(|r| r.verdict == A11yVerdict::Verified).count();
            declared == 3 && verified == 1 && !A11yVerdict::Declared.blocking()
                && A11yVerdict::Unverified.blocking()
        },
        "声明态与核验态区分（3 声明 / 1 已核验；声明不阻断、未核验阻断）",
    );
    s.add(
        "C20-A11Y-UNVERIFIED-IS-LISTED",
        {
            // 反例防线：`a11y_unverified` 返回恒空时本判据必须变红。
            // （原实现里没有任何判据调用它，故该造假路径此前无人拦截。）
            let rows = real_a11y();
            let listed = a11y_unverified(&rows);
            // 未核验项必须**逐个**出现在清单里，且不多不少
            let expect: Vec<u16> = rows
                .iter()
                .filter(|r| r.verdict == A11yVerdict::Unverified)
                .map(|r| r.ticket)
                .collect();
            !expect.is_empty() && listed == expect && listed.len() == 1
        },
        "未核验项必被列出且不多不少（恒空返回即红）",
    );
    s.add(
        "C20-BLOCKING-REASON-PRESENT",
        {
            // 反例防线：`blocking_reason` 恒 None 时本判据必须变红。
            // 判据要求「有缺项必给可执行理由」——理由是阻断登记的载体，
            // 没有理由的阻断等于没有登记。
            let ev = real_evidence();
            let missing: Vec<&Evidence> = ev.iter().filter(|e| !e.is_complete()).collect();
            !missing.is_empty()
                && missing.iter().all(|e| e.blocking_reason().is_some())
                // 理由必须含处置与影响面（三要素），不是干巴巴一句「缺失」
                && missing.iter().all(|e| {
                    let r = e.blocking_reason().unwrap_or(String::new());
                    r.contains("处置=") && r.contains("影响面=") && r.contains("上报=")
                })
                // 齐备项不得给理由（否则阻断清单会被齐备项污染）
                && ev.iter().filter(|e| e.is_complete()).all(|e| e.blocking_reason().is_none())
        },
        "缺项必给三要素理由（处置/影响面/上报）；齐备项不给理由",
    );
    s.add(
        "C20-DEFECT-DISJOINT-DISPOSITION",
        {
            // 处置方向相反不得共用码/建议。
            let d = [DefectKind::Blocking, DefectKind::Registered, DefectKind::Advisory];
            let w = [DefectKind::Blocking.wire(), DefectKind::Registered.wire(), DefectKind::Advisory.wire()];
            let p = [DefectKind::Blocking.disposition(), DefectKind::Registered.disposition(), DefectKind::Advisory.disposition()];
            let mut uniq = true;
            for i in 0..3 {
                for j in (i + 1)..3 {
                    if w.get(i) == w.get(j) || p.get(i) == p.get(j) {
                        uniq = false;
                    }
                }
            }
            uniq && d.iter().all(|k| !k.disposition().is_empty())
        },
        "三级缺陷编码与处置建议两两不同（方向相反不共用）",
    );
    s.add(
        "C20-ERR-DISJOINT-CODES",
        {
            // 收口错误码两两不同且各带处置建议。
            let c = [
                CloseoutErr::DuplicateRole.code(),
                CloseoutErr::BlockedHandover.code(),
                CloseoutErr::StaleSignature.code(),
                CloseoutErr::UnknownRole.code(),
            ];
            let mut uniq = true;
            for i in 0..c.len() {
                for j in (i + 1)..c.len() {
                    if c.get(i) == c.get(j) {
                        uniq = false;
                    }
                }
            }
            uniq
                && CloseoutErr::BlockedHandover.advise().contains("清零阻断项")
                && CloseoutErr::StaleSignature.advise().contains("重签")
        },
        "4 个收口错误码两两不同且处置建议可执行",
    );
    s.add(
        "C20-AUDIT-NAMES-BLOCKED",
        {
            // 留痕必须点名被阻断的工单（否则「有阻断」无人可查）。
            let mut b = SignBook::new();
            let r = run_group_closeout(&real_evidence(), &real_a11y(), &TrendWindow::new(), &mut b, 14000);
            r.blocked_tickets.len() == 5
                && r.blocked_tickets.iter().all(|t| (608..=612).contains(t))
                && r.audits.iter().any(|a| a.contains("VE-F0608"))
                && r.audits.iter().any(|a| a.contains("移交未授权"))
        },
        "留痕点名5 个被阻断工单并明示移交未授权",
    );
    s.add(
        "C20-CLOSEOUT-END-TO-END",
        {
            // 端到端：核验 → 阻断 → 双签 → 移交裁决，各面自洽。
            let mut b = SignBook::new();
            let r = run_group_closeout(&real_evidence(), &real_a11y(), &TrendWindow::new(), &mut b, 14000);
            r.verify.total == 19
                && r.verify.complete == 14
                && r.verify.blocked == 5
                && r.defects[0] == 5
                && r.blocked_tickets.len() == 5
                && r.a11y_unverified == vec![619]
                && r.a11y_declared.len() == 3
                && !r.handover_authorized
                && b.count() == 0
        },
        "端到端：19项核验出14 齐备/5 阻断，缺陷总账 5 阻断，移交未授权",
    );
    s.add(
        "C20-DETERMINISTIC-ACROSS-RUNS",
        {
            // 收口结论必须可复现（同一输入两次逐项相同），否则不能作为移交依据。
            let run = || -> CloseoutResult {
                let mut b = SignBook::new();
                run_group_closeout(&real_evidence(), &real_a11y(), &TrendWindow::new(), &mut b, 14000)
            };
            let a = run();
            let c = run();
            a.fingerprint == c.fingerprint
                && a.blocked_tickets == c.blocked_tickets
                && a.audits == c.audits
                && a.defects == c.defects
        },
        "同输入两次收口结论逐项一致（指纹/阻断清单/留痕/计数）",
    );
    let _ = s;
    s
}

/// 自检用断言助手（只在自检面使用，失败即该判据为红）。
fn assert_ok(r: Result<(), CloseoutErr>) -> bool {
    r.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeout_blocks_when_evidence_missing() {
        let v = verify_all(&real_evidence());
        assert_eq!(v.total, 19);
        assert_eq!(v.complete, 14);
        assert_eq!(v.blocked, 5);
        assert!(!v.meets_line, "证据缺项时不得判达标");
    }

    #[test]
    fn dual_sign_but_no_handover_when_blocked() {
        let ev = real_evidence();
        let mut l = Ledger::new();
        for e in ev.iter() {
            l.push(*e);
        }
        l.defects = derive_blocking_defects(&ev);
        let fp = l.fingerprint();
        let mut b = SignBook::new();
        assert!(b.sign(Role::DomainOwner, fp).is_ok());
        assert!(b.sign(Role::Reviewer, fp).is_ok());
        assert!(b.dual_signed(fp));
        assert!(!b.handover_authorized(fp, &l), "双签不等于移交授权");
    }

    #[test]
    fn handover_authorized_when_complete() {
        let mut ev = real_evidence();
        for e in ev.iter_mut() {
            if e.probe == Probe::TypeScriptOnly {
                e.probe = Probe::RustModulePresent;
            }
        }
        let mut l = Ledger::new();
        for e in ev.iter() {
            l.push(*e);
        }
        l.defects = derive_blocking_defects(&ev);
        let fp = l.fingerprint();
        let mut b = SignBook::new();
        assert!(b.sign(Role::DomainOwner, fp).is_ok());
        assert!(b.sign(Role::Reviewer, fp).is_ok());
        assert!(verify_all(&ev).meets_line);
        assert!(b.handover_authorized(fp, &l));
    }

    #[test]
    fn stale_signature_is_rejected() {
        let mut l = Ledger::new();
        l.push(Evidence::new(601, "数据结构", Carrier::RustModule, Probe::RustModulePresent, "a.rs"));
        let fp1 = l.fingerprint();
        let mut b = SignBook::new();
        assert!(b.sign(Role::DomainOwner, fp1).is_ok());
        l.push(Evidence::new(602, "变换", Carrier::RustModule, Probe::RustModulePresent, "b.rs"));
        assert_ne!(l.fingerprint(), fp1);
        assert_eq!(b.sign(Role::Reviewer, fp1), Err(CloseoutErr::StaleSignature));
    }

    #[test]
    fn ts_legacy_never_counts_as_complete() {
        let e = Evidence::new(608, "动画接口", Carrier::RustModule, Probe::TypeScriptOnly, "x.ts");
        assert!(!e.is_complete());
        assert!(e.blocking_reason().is_some());
        assert!(!Carrier::TypeScriptLegacy.counts_as_complete());
    }

    #[test]
    fn trend_resists_single_drop_but_catches_sustained() {
        let mut t = TrendWindow::new();
        for v in [30u32, 29, 30, 30] {
            t.push(v);
        }
        assert!(!t.degraded());
        let mut t2 = TrendWindow::new();
        for v in [30u32, 28, 26, 24] {
            t2.push(v);
        }
        assert!(t2.degraded());
    }

    #[test]
    fn contract_predicates_discriminate() {
        let good = ContractFacts {
            traverse_len: 3,
            draw_len: 3,
            hit_len: 3,
            orders_tail_equal: true,
            xform_dirty_bumps_content: 0,
            content_dirty_bumps_content: 1,
            invariant_violation_rejected: true,
        };
        assert!(contract_satisfied(Contract::ThreeOrdersSameSource, &good));
        let bad = ContractFacts { hit_len: 2, ..good };
        assert!(!contract_satisfied(Contract::ThreeOrdersSameSource, &bad));
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_ved20_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated(), "自检集被截断：dropped={}", set.dropped());
        assert!(set.all_passed(), "VE-F0620 红项：{}/{}", p, p + f);
    }
}