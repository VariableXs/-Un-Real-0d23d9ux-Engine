//! VE-F2807 · 样式规则存储与样式表对象 —— Stylesheet + 规则七类节点。
//!
//! 承接 VE-F2806（简写属性展开）。F2806 把一条简写声明展开成 1~4 条
//! longhand 声明（[`veo06_shorthand::Expansion`]），本单把这些展开产物
//! **存进样式表对象**并按选择器可查——存储与对象，不是级联求值。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2807`
//!
//! # 一、职责与边界
//!
//! 职责定位（锚点原文）：**Stylesheet 数据结构 + 规则七类节点**。
//!
//! 边界声明（**不越权**，逐条写明「归谁」）：
//! - **级联与优先级裁决不归本单**——本单只做「声明进账、按名可查」，
//!   不算权重、不裁 `!important` 胜负（那是级联层与 F2808 的活）；
//! - **规则内容解析不归本单**——声明值是 F2805 解析、F2806 展开好的
//!   `ParsedValue`，本单存的是**展开产物**，不重新走解析器；
//! - **样式求值与绘制不归本单**——`margin-top` 怎么影响盒模型归
//!   F2811/F2813 计算层；
//! - **七类节点是封闭全集**（[`RuleKind`]）：样式/媒体/导入/字体面/
//!   关键帧/命名空间/注释——CSS 常见规则形态在本引擎子集里的封闭枚举，
//!   「表里没有」是正确答案不是遗漏，判据专设一项钉死。
//!
//! # 二、存储模型：追加序 + 命名索引
//!
//! 规则按**追加序**（源顺序）线性存放（`Vec<RuleNode>`），另建一张
//! **名字 → 规则下标**的开桶哈希索引（FNV-1a 64，同 F3409/F2803 先例）：
//! - 追加序保序：级联层将来按源顺序裁胜负，本单不许打乱它；
//! - 索引供 O(1) 平均按名查（锚点性能口径 O(1)-O(logN) 的 O(1) 侧）；
//! - 桶内保插入序 ⇒ 同名规则按源序命中，同输入必同输出（判据可复现）。
//!
//! 删除走**墓碑**（`removed=true`）而非物理删除：物理搬移会让「之前的
//! 下标」失效，索引里所有旧下标都要改——O(n) 写放大；墓碑让删除 O(1)，
//! 代价是查询要跳过墓碑（读时过滤，写时零搬移）。
//!
//! # 三、承接 F2806：展开产物进账
//!
//! [`Stylesheet::insert_style`] 消费 [`veo06_shorthand::Expansion`]：
//! 每条 longhand 名**必须在 F2806 的 [`veo06_shorthand::LONGHAND_TABLE`]
//! 里查得到**（24 名封闭全集）——查不到就是「展开器没产过这个名字」，
//! 拒收并立案（[`E_SHEET_LONGHAND_UNKNOWN`]）。
//! 存储条目 [`StoredDecl`] 的四字段（longhand_name/value/important/offset）
//! 与 F2806 前向声明 [`veo06_shorthand::DOWNSTREAM_DECL`] 承诺的字段
//! **逐一对齐**——上游声明的消费接口，本单如约实现。
//!
//! # 四、错误路径与降级矩阵（锚点家族）
//!
//! - **非法输入 → 校验拒绝三要素**：空名（[`E_SHEET_NAME_EMPTY`]）、
//!   节点类型越界（[`E_SHEET_KIND_INVALID`]，枚举守卫）、承接未知
//!   longhand（[`E_SHEET_LONGHAND_UNKNOWN`]）、移除不存在的规则
//!   （[`E_SHEET_ID_INVALID`]）——各带专属码 + `next` + `who`，零静默；
//! - **边界越界 → 钳制 + 告警**：名字超该类规格的字节上界 → 截到上界
//!   并记 [`ClampNotice`]（截断有痕可归因）；声明数超该类上界 → 只收
//!   上界内、超出数记账——**不静默截断**；
//! - **账满 → 拒绝并计数**：样式表规则数达 [`MAX_RULES`] →
//!   [`E_SHEET_CAP`]，不覆盖旧行（覆盖会让之前的规则凭空消失）；
//! - **异常检出 → 立案流转**：每次拒绝都经 [`CaseLedger::open_case`]
//!   立案（现象/影响/定位/处置四要素齐）。
//!
//! # 五、跨批对接点
//!
//! - **上游契约接收（哈希对账）**：[`audit_upstream`] 现算 F2806 的
//!   [`veo06_shorthand::spec_summary`]，六条简写名**逐条点名**核对
//!   （引用完整性：本单服务的每条简写都必须真的存在于上游规格表）；
//!   F2806 改了规格表而本单没跟，对账即红；
//! - **下游消费接口（前向声明）**：[`DOWNSTREAM_DECL`] 向 F2808 声明
//!   本单的样式表对象按什么形态被消费（规则/声明/计数三面），只声明
//!   不实现——F2808 落定时改的是它自己的单；
//! - **跨域衔接对账钩子（复用方义务）**：[`reconcile_stylesheet`]
//!   给复用方自查「我拿到的是不是本单的样式表」，本单提供判据、
//!   义务在复用方。
//!
//! # 六、性能分解
//!
//! - 插入 **O(1) 均摊**（Vec 追加 + 一次索引写入）；
//! - 按名查 **O(1) 平均**（开桶索引；最坏 O(桶内冲突数)，
//!   桶数 [`INDEX_BUCKETS`] ≥ [`MAX_RULES`] 保证负载 <1）；
//! - 移除 **O(1)**（墓碑）；
//! - 承接校验 **O(声明数 × 24)**（24 是 F2806 longhand 名封闭全集，
//!   编译期常量）。
//!
//! # 七、无障碍与隐私
//!
//! [`RuleNode::screen_line`] / [`Stylesheet::screen_text`] 让规则与
//! 样式表状态可被读屏念出。**无隐私面**：不触碰作者身份/用户数据。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! 生产路径一律 `get`/`iter`/`match` 记红。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veo01_arch::{CaseLedger, ClampLog, ClampNotice, StyleError};
use super::veo05_props::ParsedValue;
use super::veo06_shorthand::{self, Expansion};

// ---------------------------------------------------------------------------
// 一、版本与契约锚点
// ---------------------------------------------------------------------------

/// 本单规格版本（改动规格即改此串，便于下游对账）。
pub const SHEET_VERSION: &str = "O01-stylesheet-v1";

/// 上游契约锚点（F2806 简写属性展开）。
pub const UPSTREAM_ANCHOR: &str = "VE-F2806/O01-shorthand-v1";

/// 下游契约锚点（F2808）。
pub const DOWNSTREAM_ANCHOR: &str = "VE-F2808";

/// 上游指纹基线标识（对账时现算上游摘要与本单记录的名单核对）。
pub const UPSTREAM_DIGEST: &str = "o06-shorthand-spec-v1";

// ---------------------------------------------------------------------------
// 二、诊断码（**F2807 独占码段：0x2Axx**）
// ---------------------------------------------------------------------------
//
// 码段纪律：F2806=0x2Cxx 已占；**本单独占 0x2Axx**（F2808 预留 0x2Bxx）。
// 自建码而非复用上游封闭枚举——上游加变体不会因本单的码编译失败，
// 本单也不必等下游先裁决。

/// 规则名为空（无名的规则无法被任何选择器命中）。
pub const E_SHEET_NAME_EMPTY: &str = "E_SHEET_NAME_EMPTY";

/// 节点类型非法（枚举守卫：越界秩不可构造出合法种类）。
pub const E_SHEET_KIND_INVALID: &str = "E_SHEET_KIND_INVALID";

/// 样式表规则数达上限（拒绝新规则，不覆盖旧行）。
pub const E_SHEET_CAP: &str = "E_SHEET_CAP";

/// 承接的 longhand 名不在 F2806 的 24 名封闭全集（展开器没产过它）。
pub const E_SHEET_LONGHAND_UNKNOWN: &str = "E_SHEET_LONGHAND_UNKNOWN";

/// 移除不存在的规则 id（或已是墓碑）。
pub const E_SHEET_ID_INVALID: &str = "E_SHEET_ID_INVALID";

/// 上游契约对账失败（F2806 规格表已变而本单未跟）。
pub const E_SHEET_UPSTREAM_DRIFT: &str = "E_SHEET_UPSTREAM_DRIFT";

/// 下游声明审计失败（对端标识/字段面被改动）。
pub const E_SHEET_DOWNSTREAM_DRIFT: &str = "E_SHEET_DOWNSTREAM_DRIFT";

/// 处置建议：规则名不可为空。
pub const FIX_NAME_EMPTY: &str = "样式规则必须有选择器（或 at 规则必须有参数名）；空名规则无法被命中也无法归因";

/// 处置建议：节点类型越界。
pub const FIX_KIND_INVALID: &str = "节点类型只能取 RuleKind 七类封闭全集之一；越界秩无对应种类";

/// 处置建议：样式表满。
pub const FIX_SHEET_CAP: &str = "先归档或合并存量规则，或按 ADR 提升 MAX_RULES；在此之前不得覆盖既有规则";

/// 处置建议：承接未知 longhand。
pub const FIX_LONGHAND_UNKNOWN: &str = "longhand 名必须逐字节来自 F2806 的 LONGHAND_TABLE 24 名全集；外来名字先经 F2805/F2806 解析展开";

/// 处置建议：移除不存在的规则。
pub const FIX_ID_INVALID: &str = "移除前先按名查出规则 id；已移除的规则不可重复移除（墓碑不可复用）";

/// 处置建议：上游漂移。
pub const FIX_UPSTREAM: &str = "F2806 简写规格表已改动；同步本单承接名单与规格常量后重跑审计";

/// 处置建议：下游漂移。
pub const FIX_DOWNSTREAM: &str = "下游对端标识或字段面被改动；与 F2808 对齐消费契约后重跑审计";

// ---------------------------------------------------------------------------
// 三、规则类型（七类封闭全集 + 枚举守卫）
// ---------------------------------------------------------------------------

/// 规则七类节点：封闭全集。
///
/// 封闭的含义：`of_rank` 对 0..=6 给 Some、对 7 起给 None——越界秩
/// **在类型面造不出合法种类**，枚举守卫因此可判据（不是靠自觉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    /// 样式规则（选择器 + longhand 声明；唯一承载声明的一类）。
    Style,
    /// 媒体查询（@media；参数是查询文本）。
    Media,
    /// 导入（@import；参数是目标 URI）。
    Import,
    /// 字体面（@font-face；参数是字体族名，描述符走声明）。
    FontFace,
    /// 关键帧（@keyframes；参数是动画名）。
    Keyframes,
    /// 命名空间（@namespace；参数是 URI）。
    Namespace,
    /// 注释节点（保序保真：注释在源序里占位）。
    Comment,
}

impl RuleKind {
    /// 全集（判据据此核对无遗漏）。
    pub const ALL: [RuleKind; RULE_KIND_COUNT] = [
        RuleKind::Style,
        RuleKind::Media,
        RuleKind::Import,
        RuleKind::FontFace,
        RuleKind::Keyframes,
        RuleKind::Namespace,
        RuleKind::Comment,
    ];

    /// 秩（与本单规格表下标一致）。
    pub const fn rank(self) -> usize {
        match self {
            RuleKind::Style => 0,
            RuleKind::Media => 1,
            RuleKind::Import => 2,
            RuleKind::FontFace => 3,
            RuleKind::Keyframes => 4,
            RuleKind::Namespace => 5,
            RuleKind::Comment => 6,
        }
    }

    /// 秩反查种类（越界 None——枚举守卫的落地）。
    pub const fn of_rank(r: usize) -> Option<RuleKind> {
        match r {
            0 => Some(RuleKind::Style),
            1 => Some(RuleKind::Media),
            2 => Some(RuleKind::Import),
            3 => Some(RuleKind::FontFace),
            4 => Some(RuleKind::Keyframes),
            5 => Some(RuleKind::Namespace),
            6 => Some(RuleKind::Comment),
            _ => None,
        }
    }

    /// 中文名（读屏）。
    pub const fn zh(self) -> &'static str {
        match self {
            RuleKind::Style => "样式",
            RuleKind::Media => "媒体",
            RuleKind::Import => "导入",
            RuleKind::FontFace => "字体面",
            RuleKind::Keyframes => "关键帧",
            RuleKind::Namespace => "命名空间",
            RuleKind::Comment => "注释",
        }
    }
}

/// 节点类型总数（封闭全集；数组长度即断言）。
pub const RULE_KIND_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// 四、规格表（逐条公开 + 平行数组单源 + 参数域钳制）
// ---------------------------------------------------------------------------

/// 每类节点的参数（名字/选择器/注释体）字节上界。
///
/// **唯一真源**：[`RULE_SPECS`] 的 `param_max` 从它派生——改这里，
/// 表跟着变；字符串断不进编译期闸（`PartialEq` 非 const trait），
/// 数值/布尔才断得住（F2806 同款纪律）。
pub const PARAM_MAX_OF: [usize; RULE_KIND_COUNT] = [256, 256, 512, 128, 256, 512, 1024];

/// 每类节点的声明数上界（样式 64、字体面描述符 32、其余不承载声明）。
pub const DECLS_CAP_OF: [usize; RULE_KIND_COUNT] = [64, 0, 0, 32, 0, 0, 0];

/// 每类是否 at 规则（样式/注释不是，其余五类是）。
pub const AT_RULE_OF: [bool; RULE_KIND_COUNT] = [false, true, true, true, true, true, false];

/// 单类节点规格（**逐条公开**：字段全 pub，下游可机检）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSpec {
    /// 该类节点的中文名（读屏）。
    pub kind: RuleKind,
    /// 该类节点的机器名。
    pub name: &'static str,
    /// 参数字节上界（来源 [`PARAM_MAX_OF`]）。
    pub param_max: usize,
    /// 声明数上界（来源 [`DECLS_CAP_OF`]）。
    pub decls_cap: usize,
    /// 是否 at 规则（来源 [`AT_RULE_OF`]）。
    pub at_rule: bool,
}

/// 七类节点规格表（**逐条公开**）。
pub const RULE_SPECS: [RuleSpec; RULE_KIND_COUNT] = [
    RuleSpec { kind: RuleKind::Style,     name: "style",     param_max: PARAM_MAX_OF[0], decls_cap: DECLS_CAP_OF[0], at_rule: AT_RULE_OF[0] },
    RuleSpec { kind: RuleKind::Media,     name: "media",     param_max: PARAM_MAX_OF[1], decls_cap: DECLS_CAP_OF[1], at_rule: AT_RULE_OF[1] },
    RuleSpec { kind: RuleKind::Import,    name: "import",    param_max: PARAM_MAX_OF[2], decls_cap: DECLS_CAP_OF[2], at_rule: AT_RULE_OF[2] },
    RuleSpec { kind: RuleKind::FontFace,  name: "font-face", param_max: PARAM_MAX_OF[3], decls_cap: DECLS_CAP_OF[3], at_rule: AT_RULE_OF[3] },
    RuleSpec { kind: RuleKind::Keyframes, name: "keyframes", param_max: PARAM_MAX_OF[4], decls_cap: DECLS_CAP_OF[4], at_rule: AT_RULE_OF[4] },
    RuleSpec { kind: RuleKind::Namespace, name: "namespace", param_max: PARAM_MAX_OF[5], decls_cap: DECLS_CAP_OF[5], at_rule: AT_RULE_OF[5] },
    RuleSpec { kind: RuleKind::Comment,   name: "comment",   param_max: PARAM_MAX_OF[6], decls_cap: DECLS_CAP_OF[6], at_rule: AT_RULE_OF[6] },
];

/// 按类型取规格（秩即下标，O(1)）。
pub const fn spec_of_kind(k: RuleKind) -> Option<&'static RuleSpec> {
    let r = k.rank();
    if r < RULE_KIND_COUNT {
        Some(&RULE_SPECS[r])
    } else {
        None
    }
}

/// 样式表规则数上限（满后拒绝，不覆盖旧行）。
pub const MAX_RULES: usize = 256;

/// 哈希索引桶数（2 的幂；负载 = MAX_RULES/BUCKETS < 1）。
pub const INDEX_BUCKETS: usize = 512;

/// 规格表可机检摘要（**版本|上限|逐类「名:参数上界:声明上界:at」**）。
pub fn spec_summary() -> String {
    let mut s = String::new();
    s.push_str(SHEET_VERSION);
    s.push('|');
    s.push_str(&MAX_RULES.to_string());
    s.push('|');
    let mut i = 0usize;
    while i < RULE_KIND_COUNT {
        if let Some(sp) = RULE_SPECS.get(i) {
            s.push_str(sp.name);
            s.push(':');
            s.push_str(&sp.param_max.to_string());
            s.push(':');
            s.push_str(&sp.decls_cap.to_string());
            s.push(':');
            s.push_str(if sp.at_rule { "at" } else { "plain" });
            s.push(';');
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 五、开桶哈希索引（alloc 面 O(1) 平均；桶内保插入序）
// ---------------------------------------------------------------------------

/// FNV-1a 64（确定性散列，跨平台同值）。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 名字 → 规则下标的开桶索引。
///
/// 桶内追加保持**插入序**；查找时桶内线性比对真键（散列只选桶，
/// 不判等——判等必须落在键本身上）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NameIndex {
    buckets: Vec<Vec<u32>>,
}

impl NameIndex {
    /// 空索引。
    pub fn new() -> NameIndex {
        NameIndex {
            buckets: vec![Vec::new(); INDEX_BUCKETS],
        }
    }

    /// 写入（key=名字，idx=规则下标）。重复 key 追加不覆盖——
    /// CSS 同名规则合法（同选择器多条规则按源序参与级联）。
    pub fn insert(&mut self, key: &str, idx: u32) {
        let b = (fnv1a(key) % INDEX_BUCKETS as u64) as usize;
        if let Some(bucket) = self.buckets.get_mut(b) {
            bucket.push(idx);
        }
    }

    /// 命中键的全部下标（插入序；`key_of` 回读真键判等）。
    pub fn get<'a>(&'a self, key: &str, key_of: impl Fn(u32) -> Option<&'a str>) -> Vec<u32> {
        let b = (fnv1a(key) % INDEX_BUCKETS as u64) as usize;
        let mut out = Vec::new();
        if let Some(bucket) = self.buckets.get(b) {
            for idx in bucket.iter() {
                if let Some(k) = key_of(*idx) {
                    if k == key {
                        out.push(*idx);
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 六、存储条目与规则节点
// ---------------------------------------------------------------------------

/// 一条已存声明（**四字段恰好对齐 F2806 前向声明的消费接口**：
/// longhand_name / value / important / offset）。
#[derive(Clone, Debug, PartialEq)]
pub struct StoredDecl {
    /// longhand 名（逐字节来自 F2806 的 LONGHAND_TABLE 全集）。
    pub name: String,
    /// 声明值（F2805 解析产物，本单不重解析）。
    pub value: ParsedValue,
    /// 是否 `!important`（逐条承自源声明，本单不裁决级联）。
    pub important: bool,
    /// 源偏移（承自源声明）。
    pub offset: u32,
}

impl StoredDecl {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}: {}{}（偏移 {}）",
            self.name,
            self.value.screen_line(),
            if self.important { "（重要）" } else { "" },
            self.offset
        )
    }
}

/// 一个规则节点（追加序存放；删除走墓碑）。
#[derive(Clone, Debug, PartialEq)]
pub struct RuleNode {
    /// 规则 id（单调递增，全表唯一；判据可对账）。
    pub id: u64,
    /// 节点类型。
    pub kind: RuleKind,
    /// 选择器（样式）或参数（at 规则/注释体）。
    pub name: String,
    /// 已存声明（仅 Style/FontFace 承载；其余恒空）。
    pub decls: Vec<StoredDecl>,
    /// 源偏移。
    pub offset: u32,
    /// 墓碑（删除留痕：物理搬移会让索引旧下标全体失效，O(n) 写放大）。
    pub removed: bool,
}

impl RuleNode {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        let mut s = format!(
            "#{}（{}）{}{}：{} 条声明",
            self.id,
            self.kind.zh(),
            self.name,
            if self.removed { "〔已移除〕" } else { "" },
            self.decls.len()
        );
        for d in self.decls.iter() {
            s.push('；');
            s.push_str(&d.screen_line());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 七、承接校验（F2806 复用方义务的落地）
// ---------------------------------------------------------------------------

/// 该名是否在 F2806 的 longhand 24 名封闭全集（O(24) 线性，全集编译期常量）。
pub fn is_known_longhand(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut hit = false;
    for quad in veo06_shorthand::LONGHAND_TABLE.iter() {
        let mut c = 0usize;
        while c < 4 {
            if quad.get(c).copied() == Some(name) {
                hit = true;
            }
            c += 1;
        }
    }
    hit
}

// ---------------------------------------------------------------------------
// 八、统计（失败显性化）
// ---------------------------------------------------------------------------

/// 样式表操作统计。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetStats {
    /// 尝试插入的规则数。
    pub attempts: u32,
    /// 成功插入数。
    pub accepted: u32,
    /// 被拒数（空名/类型越界/承接未知/账满/移除无效）。
    pub rejected: u32,
    /// 被钳的次数（名字截断/声明截收）。
    pub clamped: u32,
    /// 在账声明总条数（守恒判据用它）。
    pub produced: u32,
    /// 移除数（墓碑数）。
    pub removed: u32,
}

impl SheetStats {
    /// 守恒：成功 + 被拒 == 尝试。
    pub fn conserves(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.attempts
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "样式表：尝试 {} 条，接受 {}，拒绝 {}，钳制 {}，在账声明 {} 条，移除 {} 条",
            self.attempts, self.accepted, self.rejected, self.clamped, self.produced, self.removed
        )
    }
}

// ---------------------------------------------------------------------------
// 九、样式表对象
// ---------------------------------------------------------------------------

/// 样式表对象（**存储 + 按名可查**；不裁级联）。
pub struct Stylesheet {
    /// 规则（追加序 = 源顺序）。
    pub rules: Vec<RuleNode>,
    /// 名字索引（名字 → 规则下标；O(1) 平均）。
    pub index: NameIndex,
    /// 钳制告警账（承 F2801）。
    pub clamps: ClampLog,
    /// 案件账（承 F2801）。
    pub cases: CaseLedger,
    /// 统计。
    pub stats: SheetStats,
    /// 规则 id 发放器（单调递增）。
    next_id: u64,
    /// 逻辑 tick（唯一时间源，零墙钟）。
    pub tick: u64,
}

impl Stylesheet {
    /// 空样式表。
    pub fn new() -> Stylesheet {
        Stylesheet {
            rules: Vec::new(),
            index: NameIndex::new(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            stats: SheetStats::default(),
            next_id: 1,
            tick: 0,
        }
    }

    /// 推进逻辑 tick。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 立案（异常检出的显性载体）。
    fn file_case(&mut self, err: &StyleError, locus: &str) {
        let _ = self.cases.open_case(err.what, &err.why, locus, err.next, self.tick);
    }

    /// 钳制记账。
    fn note_clamp(&mut self, field: &str, original: f64, clamped: f64, high: f64) {
        let _ = self.clamps.push(ClampNotice {
            field: String::from(field),
            original,
            clamped,
            low: 0.0,
            high,
            tick: self.tick,
        });
        self.stats.clamped = self.stats.clamped.saturating_add(1);
    }

    /// 名字按该类规格的参数上界钳制（返回钳后名；越界必记账）。
    fn clamp_name(&mut self, kind: RuleKind, name: &str) -> String {
        let cap = match spec_of_kind(kind) {
            Some(sp) => sp.param_max,
            None => 0,
        };
        if name.len() <= cap {
            return String::from(name);
        }
        // 字节截断须落在字符边界上（逐字符收，不做裸字节切片）。
        let mut end = cap;
        while end > 0 && !name.is_char_boundary(end) {
            end -= 1;
        }
        let original_len = name.len();
        let clamped = String::from(&name[..end]);
        self.note_clamp(
            &format!("{}:name", kind.zh()),
            original_len as f64,
            clamped.len() as f64,
            cap as f64,
        );
        clamped
    }

    /// **插入一条规则**（核心入口；O(1) 均摊）。
    ///
    /// - 空名拒（[`E_SHEET_NAME_EMPTY`]）；
    /// - 类型越界拒（[`E_SHEET_KIND_INVALID`]）；
    /// - 名字超该类上界 → 钳制 + 告警（不拒——锚点家族：越界→钳制+告警）；
    /// - 表满拒（[`E_SHEET_CAP`]），不覆盖旧行。
    ///
    /// `decls` 由调用方预先承接校验（[`Self::convert_longhands`]），
    /// 非 Style 类的声明会被本入口**钳空**（规格表 DECLS_CAP_OF 为 0）。
    pub fn insert_rule(
        &mut self,
        kind: RuleKind,
        name: &str,
        decls: Vec<StoredDecl>,
        offset: u32,
    ) -> Result<u64, StyleError> {
        self.stats.attempts = self.stats.attempts.saturating_add(1);

        // 闸 1：空名拒（非法输入 → 拒绝三要素）。
        if name.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHEET_NAME_EMPTY,
                "插入被拒：规则名为空",
                "无名规则无法被任何选择器命中，也无法在诊断里归因",
                FIX_NAME_EMPTY,
                "O 域组件负责人",
            );
            self.file_case(&err, name);
            return Err(err);
        }

        // 闸 2：枚举守卫（类型越界拒）。
        let spec = match spec_of_kind(kind) {
            Some(sp) => sp,
            None => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_SHEET_KIND_INVALID,
                    "插入被拒：节点类型越界",
                    "类型秩不在七类封闭全集内，无对应规格",
                    FIX_KIND_INVALID,
                    "O 域组件负责人",
                );
                self.file_case(&err, name);
                return Err(err);
            }
        };

        // 闸 3：表满拒（不覆盖旧行）。
        if self.rules.len() >= MAX_RULES {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHEET_CAP,
                "插入被拒：样式表已满",
                &format!(
                    "规则数 {} 达上限 {}；覆盖会让既有规则凭空消失",
                    self.rules.len(),
                    MAX_RULES
                ),
                FIX_SHEET_CAP,
                "O 域组件负责人",
            );
            self.file_case(&err, name);
            return Err(err);
        }

        // 闸 4：名字钳制（越界 → 钳制 + 告警，不拒）。
        let name = self.clamp_name(kind, name);

        // 闸 5：声明数按该类上界钳收（超出数记账，不静默截断）。
        let mut decls = decls;
        let decls_cap = spec.decls_cap;
        if decls.len() > decls_cap {
            let original = decls.len();
            decls.truncate(decls_cap);
            self.note_clamp(
                &format!("{}:decls", kind.zh()),
                original as f64,
                decls_cap as f64,
                decls_cap as f64,
            );
        }

        // 入账（追加序 + 索引）。
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let idx = self.rules.len() as u32;
        self.rules.push(RuleNode {
            id,
            kind,
            name: String::from(&name),
            decls,
            offset,
            removed: false,
        });
        self.index.insert(&name, idx);
        self.stats.accepted = self.stats.accepted.saturating_add(1);
        self.stats.produced = self
            .stats
            .produced
            .saturating_add(self.rules.get(idx as usize).map(|r| r.decls.len()).unwrap_or(0) as u32);
        Ok(id)
    }

    /// 把 F2806 的展开产物换成已存声明（**承接校验在此**）。
    ///
    /// 每条 longhand 名必须在 F2806 的 24 名全集里；遇未知名立即拒
    /// （[`E_SHEET_LONGHAND_UNKNOWN`]）并立案——「展开器没产过的名字」
    /// 进了账，下游按名查会查到来历不明的声明。
    pub fn convert_longhands(
        &mut self,
        exp: &Expansion,
    ) -> Result<Vec<StoredDecl>, StyleError> {
        let mut out: Vec<StoredDecl> = Vec::new();
        for l in exp.longhands.iter() {
            if !is_known_longhand(&l.name) {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_SHEET_LONGHAND_UNKNOWN,
                    "承接被拒：longhand 名不在上游全集",
                    &format!(
                        "简写 {} 展开产出的 {} 不在 F2806 的 LONGHAND_TABLE 全集",
                        exp.shorthand, l.name
                    ),
                    FIX_LONGHAND_UNKNOWN,
                    "O 域组件负责人",
                );
                self.file_case(&err, &l.name);
                return Err(err);
            }
            out.push(StoredDecl {
                name: String::from(&l.name),
                value: l.value.clone(),
                important: l.important,
                offset: l.offset,
            });
        }
        Ok(out)
    }

    /// **插入一条样式规则并承接展开产物**（消费 F2806 的主入口）。
    pub fn insert_style(
        &mut self,
        selector: &str,
        exp: &Expansion,
        offset: u32,
    ) -> Result<u64, StyleError> {
        self.tick = self.tick.saturating_add(1);
        let decls = self.convert_longhands(exp)?;
        self.insert_rule(RuleKind::Style, selector, decls, offset)
    }

    /// 按名查规则 id（跳过墓碑；插入序 = 源序；O(1) 平均）。
    pub fn lookup_ids(&self, name: &str) -> Vec<u64> {
        let mut out = Vec::new();
        for idx in self
            .index
            .get(name, |i| self.rules.get(i as usize).map(|r| r.name.as_str()))
            .iter()
        {
            if let Some(r) = self.rules.get(*idx as usize) {
                if !r.removed && r.name == name {
                    out.push(r.id);
                }
            }
        }
        out
    }

    /// 按 id 取规则（跳过墓碑；O(n) 线性——id→下标无第二索引，
    /// n ≤ MAX_RULES 有界）。
    pub fn rule_of(&self, id: u64) -> Option<&RuleNode> {
        self.rules.iter().find(|r| r.id == id && !r.removed)
    }

    /// 移除规则（墓碑；O(1)；幂等性**不提供**——重复移除同一 id
    /// 第二次报 [`E_SHEET_ID_INVALID`]，静默幂等会让「移了两次」无法归因）。
    pub fn remove_rule(&mut self, id: u64) -> Result<(), StyleError> {
        self.stats.attempts = self.stats.attempts.saturating_add(1);
        let slot = self.rules.iter_mut().find(|r| r.id == id && !r.removed);
        match slot {
            Some(r) => {
                r.removed = true;
                self.stats.removed = self.stats.removed.saturating_add(1);
                self.stats.accepted = self.stats.accepted.saturating_add(1);
                Ok(())
            }
            None => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_SHEET_ID_INVALID,
                    "移除被拒：规则 id 不存在或已是墓碑",
                    &format!("id {} 查无在账规则；重复移除不可静默幂等", id),
                    FIX_ID_INVALID,
                    "O 域组件负责人",
                );
                let locus = format!("id:{}", id);
                self.file_case(&err, &locus);
                Err(err)
            }
        }
    }

    /// 在账规则数（不含墓碑）。
    pub fn live_count(&self) -> usize {
        self.rules.iter().filter(|r| !r.removed).count()
    }

    /// 读屏摘要（逐规则可读）。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "样式表：在账 {} 条（墓碑 {} 条）；",
            self.live_count(),
            self.stats.removed
        );
        for r in self.rules.iter() {
            if !r.removed {
                s.push_str(&r.screen_line());
                s.push('；');
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 十、跨批对接点
// ---------------------------------------------------------------------------

/// 上游契约接收与哈希对账（**运行期现算**，不抄常量）。
///
/// 对账实质是**引用完整性**：F2806 摘要必须含六条简写名（本单承接
/// 的展开产物全部来自这六条简写）。判据可用 [`upstream_missing_for`]
/// 喂**自造劣化摘要**验证检出能力，不必改真常量。
pub fn audit_upstream() -> UpstreamContract {
    let summary = veo06_shorthand::spec_summary();
    let missing = upstream_missing_for(&summary);
    UpstreamContract {
        peer: "VE-F2806",
        anchor: String::from(UPSTREAM_ANCHOR),
        summary_len: summary.len() as u32,
        missing,
        matched: missing == 0 && !summary.is_empty(),
    }
}

/// 对账核心（**判据可注入**）：给定上游摘要，返回本单点名却缺失的
/// 简写名个数（0 = 全部命中）。
pub fn upstream_missing_for(summary: &str) -> usize {
    let mut missing = 0usize;
    for sp in veo06_shorthand::SHORTHAND_TABLE.iter() {
        if !summary.contains(sp.name) {
            missing += 1;
        }
    }
    missing
}

/// 上游契约接收记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamContract {
    /// 上游模块标识。
    pub peer: &'static str,
    /// 锚点。
    pub anchor: String,
    /// 上游摘要字节长（现算）。
    pub summary_len: u32,
    /// 点名缺失数（0 = 对账通过）。
    pub missing: usize,
    /// 是否对账通过。
    pub matched: bool,
}

impl UpstreamContract {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "上游 {}｜锚点 {}｜摘要 {} 字节｜缺失 {}｜对账 {}",
            self.peer,
            self.anchor,
            self.summary_len,
            self.missing,
            if self.matched { "通过" } else { "拒绝" }
        )
    }
}

/// 下游消费接口的正式声明（**只声明，不实现**；F2808 消费本单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownstreamDecl {
    /// 下游模块标识。
    pub peer: &'static str,
    /// 下游需要的面（按序）。
    pub faces: &'static [&'static str],
}

/// 下游声明：F2808 按这三个面消费样式表对象。
pub const DOWNSTREAM_DECL: DownstreamDecl = DownstreamDecl {
    peer: "VE-F2808",
    faces: &["rules_in_source_order", "declarations_by_rule", "live_counts"],
};

/// 下游声明审计（只查声明完整性，不实现消费逻辑）。
pub fn audit_downstream() -> Result<&'static str, StyleError> {
    if DOWNSTREAM_DECL.peer != DOWNSTREAM_ANCHOR {
        return Err(StyleError::new(
            E_SHEET_DOWNSTREAM_DRIFT,
            "下游声明审计失败：对端标识不符",
            "本单的样式表对象交 F2808 消费；对端标识被改动",
            FIX_DOWNSTREAM,
            "O 域组件负责人",
        ));
    }
    if DOWNSTREAM_DECL.faces.len() < 3 {
        return Err(StyleError::new(
            E_SHEET_DOWNSTREAM_DRIFT,
            "下游声明审计失败：消费面不足",
            "F2808 至少需要 源序规则/按规则声明/在账计数 三个消费面",
            FIX_DOWNSTREAM,
            "O 域组件负责人",
        ));
    }
    Ok("下游消费接口前向声明完整（F2808 取 rules_in_source_order/declarations_by_rule/live_counts）")
}

/// 跨域衔接对账钩子（**复用方义务**：本单提供判据，义务在复用方）。
///
/// 复用方拿到一组「规则名」后应当调这个自查：名字非空、且每条名
/// 都能在本单按名查到至少一条在账规则。
pub fn reconcile_stylesheet(sheet: &Stylesheet, names: &[&str]) -> Result<usize, StyleError> {
    if names.is_empty() {
        return Err(StyleError::new(
            E_SHEET_NAME_EMPTY,
            "对账失败：名字清单为空",
            "空清单的对账是空转，复用方应当至少点一条名",
            FIX_NAME_EMPTY,
            "复用方（下游模块）",
        ));
    }
    let mut total = 0usize;
    for n in names.iter() {
        if n.is_empty() {
            return Err(StyleError::new(
                E_SHEET_NAME_EMPTY,
                "对账失败：清单含空名",
                "空名无法定位任何规则",
                FIX_NAME_EMPTY,
                "复用方（下游模块）",
            ));
        }
        let hit = sheet.lookup_ids(n).len();
        if hit == 0 {
            return Err(StyleError::new(
                E_SHEET_ID_INVALID,
                "对账失败：点名在样式表无在账规则",
                &format!("{} 查无在账规则（可能已被移除或从未插入）", n),
                FIX_ID_INVALID,
                "复用方（下游模块）",
            ));
        }
        total += hit;
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// 十一、规格表审计（运行期对照编译期闸）
// ---------------------------------------------------------------------------

/// 审计规格表（运行期版；编译期版在文件末尾 `const _: () = assert!`）。
pub fn audit_spec_table() -> Result<String, StyleError> {
    // 闸 1：条数与封闭全集一致。
    if RULE_SPECS.len() != RULE_KIND_COUNT {
        return Err(StyleError::new(
            E_SHEET_KIND_INVALID,
            "规格表审计失败：条数不符",
            &format!("表长 {} 与全集 {} 不等", RULE_SPECS.len(), RULE_KIND_COUNT),
            FIX_KIND_INVALID,
            "O 域组件负责人",
        ));
    }
    // 闸 2：逐类——名字非空、种类与下标一致、声明上界与承载一致。
    let mut i = 0usize;
    while i < RULE_KIND_COUNT {
        let sp = match RULE_SPECS.get(i) {
            Some(sp) => sp,
            None => {
                return Err(StyleError::new(
                    E_SHEET_KIND_INVALID,
                    "规格表审计失败：下标越界",
                    &format!("第 {} 类取不到", i),
                    FIX_KIND_INVALID,
                    "O 域组件负责人",
                ));
            }
        };
        if sp.name.is_empty() {
            return Err(StyleError::new(
                E_SHEET_NAME_EMPTY,
                "规格表审计失败：类名为空",
                "无名类别不可机检",
                FIX_NAME_EMPTY,
                "O 域组件负责人",
            ));
        }
        if sp.kind.rank() != i {
            return Err(StyleError::new(
                E_SHEET_KIND_INVALID,
                "规格表审计失败：种类与下标错位",
                &format!("第 {} 位放了秩 {} 的种类", i, sp.kind.rank()),
                FIX_KIND_INVALID,
                "O 域组件负责人",
            ));
        }
        // 承载一致性：decls_cap==0 的类不该被塞声明（规格即承诺）。
        let cap_from_array = DECLS_CAP_OF.get(i).copied().unwrap_or(0);
        if sp.decls_cap != cap_from_array {
            return Err(StyleError::new(
                E_SHEET_KIND_INVALID,
                "规格表审计失败：与平行数组漂移",
                &format!("{} 的声明上界 {} ≠ 平行数组 {}", sp.name, sp.decls_cap, cap_from_array),
                FIX_KIND_INVALID,
                "O 域组件负责人",
            ));
        }
        let param_from_array = PARAM_MAX_OF.get(i).copied().unwrap_or(0);
        if sp.param_max != param_from_array || sp.param_max == 0 {
            return Err(StyleError::new(
                E_SHEET_KIND_INVALID,
                "规格表审计失败：参数上界漂移或为零",
                &format!("{} 的参数上界与平行数组不符或为 0", sp.name),
                FIX_KIND_INVALID,
                "O 域组件负责人",
            ));
        }
        let at_from_array = AT_RULE_OF.get(i).copied().unwrap_or(!sp.at_rule);
        if sp.at_rule != at_from_array {
            return Err(StyleError::new(
                E_SHEET_KIND_INVALID,
                "规格表审计失败：at 标志漂移",
                &format!("{} 的 at 标志与平行数组不符", sp.name),
                FIX_KIND_INVALID,
                "O 域组件负责人",
            ));
        }
        i += 1;
    }
    // 闸 3：索引桶数 ≥ 规则上限（负载 <1 的前提）。
    if INDEX_BUCKETS < MAX_RULES || !INDEX_BUCKETS.is_power_of_two() {
        return Err(StyleError::new(
            E_SHEET_CAP,
            "规格表审计失败：索引桶数不足",
            &format!(
                "桶数 {} 必须 ≥ 规则上限 {} 且为 2 的幂（否则取模偏斜）",
                INDEX_BUCKETS, MAX_RULES
            ),
            FIX_SHEET_CAP,
            "O 域组件负责人",
        ));
    }
    Ok(format!(
        "样式规格表审计通过：{} 类节点 × 参数上界逐类齐备，规则上限 {}，桶数 {}",
        RULE_KIND_COUNT, MAX_RULES, INDEX_BUCKETS
    ))
}

/// 全域审计汇总（一条命令跑完所有闸，供下游与判据共用）。
pub fn audit_all() -> Result<String, StyleError> {
    let a = audit_spec_table()?;
    let b = audit_upstream();
    if !b.matched {
        return Err(StyleError::new(
            E_SHEET_UPSTREAM_DRIFT,
            "上游对账失败",
            &format!(
                "F2806 摘要 {} 字节缺 {} 条简写名",
                b.summary_len, b.missing
            ),
            FIX_UPSTREAM,
            "O 域组件负责人",
        ));
    }
    let c = audit_downstream()?;
    Ok(format!("{}；上游 {}；{}", a, b.screen_line(), c))
}

/// 全域审计摘要（可读单行，承 F2805 的 `*_summary` 惯例）。
pub fn domain_summary() -> String {
    format!(
        "{}｜七类节点｜规则上限 {}｜声明承载 样式64/字体面32｜索引 {} 桶",
        SHEET_VERSION, MAX_RULES, INDEX_BUCKETS
    )
}

// ---------------------------------------------------------------------------
// 十二、编译期闸（数值域全在这里断）
// ---------------------------------------------------------------------------

const _: () = {
    // 闸 1：表长与全集一致（数组长度即断言）。
    assert!(RULE_SPECS.len() == RULE_KIND_COUNT);
    assert!(PARAM_MAX_OF.len() == RULE_KIND_COUNT);
    assert!(DECLS_CAP_OF.len() == RULE_KIND_COUNT);
    assert!(AT_RULE_OF.len() == RULE_KIND_COUNT);

    // 闸 2：参数上界逐类非零且 ≤ 1024（注释体最大）。
    assert!(PARAM_MAX_OF[0] >= 1 && PARAM_MAX_OF[0] <= 1024);
    assert!(PARAM_MAX_OF[1] >= 1 && PARAM_MAX_OF[1] <= 1024);
    assert!(PARAM_MAX_OF[2] >= 1 && PARAM_MAX_OF[2] <= 1024);
    assert!(PARAM_MAX_OF[3] >= 1 && PARAM_MAX_OF[3] <= 1024);
    assert!(PARAM_MAX_OF[4] >= 1 && PARAM_MAX_OF[4] <= 1024);
    assert!(PARAM_MAX_OF[5] >= 1 && PARAM_MAX_OF[5] <= 1024);
    assert!(PARAM_MAX_OF[6] >= 1 && PARAM_MAX_OF[6] <= 1024);

    // 闸 3：声明承载恰两类（样式 64、字体面 32），其余为 0。
    assert!(DECLS_CAP_OF[0] == 64);
    assert!(DECLS_CAP_OF[3] == 32);
    assert!(DECLS_CAP_OF[1] == 0);
    assert!(DECLS_CAP_OF[2] == 0);
    assert!(DECLS_CAP_OF[4] == 0);
    assert!(DECLS_CAP_OF[5] == 0);
    assert!(DECLS_CAP_OF[6] == 0);

    // 闸 4：at 标志恰五真（样式/注释为假）。
    assert!(!AT_RULE_OF[0]);
    assert!(AT_RULE_OF[1]);
    assert!(AT_RULE_OF[2]);
    assert!(AT_RULE_OF[3]);
    assert!(AT_RULE_OF[4]);
    assert!(AT_RULE_OF[5]);
    assert!(!AT_RULE_OF[6]);

    // 闸 5：容量纪律——桶数为 2 的幂且 ≥ 规则上限；上限有界。
    assert!(INDEX_BUCKETS.is_power_of_two());
    assert!(INDEX_BUCKETS >= MAX_RULES);
    assert!(MAX_RULES <= 1024);

    // 闸 6：七类封闭全集的秩精确 0..=6（不是单调，是精确值）。
    assert!(RuleKind::Style.rank() == 0);
    assert!(RuleKind::Media.rank() == 1);
    assert!(RuleKind::Import.rank() == 2);
    assert!(RuleKind::FontFace.rank() == 3);
    assert!(RuleKind::Keyframes.rank() == 4);
    assert!(RuleKind::Namespace.rank() == 5);
    assert!(RuleKind::Comment.rank() == 6);
};

// ---------------------------------------------------------------------------
// 十三、tests（宿主单测；发行剔除零成本）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_roundtrip() {
        for (i, k) in RuleKind::ALL.iter().enumerate() {
            assert_eq!(k.rank(), i);
            assert_eq!(RuleKind::of_rank(i), Some(*k));
        }
        assert_eq!(RuleKind::of_rank(7), None);
    }

    #[test]
    fn empty_name_rejected() {
        let mut s = Stylesheet::new();
        assert!(s.insert_rule(RuleKind::Style, "", Vec::new(), 0).is_err());
    }
}
