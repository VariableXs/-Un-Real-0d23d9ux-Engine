//! VE-F2808 · @import 与样式表依赖图 —— @import 解析/循环检测/拓扑序加载/深度限制。
//!
//! 承接 VE-F2807（样式规则存储与样式表对象）。F2807 把规则存进
//! [`veo07_rules::Stylesheet`] 并按名可查，同时以前向声明
//! `DOWNSTREAM_DECL { peer: "VE-F2808", faces: [...] }` 承诺本单消费其
//! **源序规则 / 在账计数**两面子集；本单把「谁 import 谁」建成一张
//! **依赖图**，提供循环检测与拓扑序加载计划——不是级联求值。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2808`
//!
//! # 一、职责与边界
//!
//! 职责定位（锚点原文）：**@import 解析/循环检测/拓扑序加载/深度限制**。
//!
//! 边界声明（**不越权**，逐条写明「归谁」）：
//! - **@import 词法扫描不归本单**——`@import` 语句从源文本里剥出来是
//!   F2804/F2807 词法与规则层的活，本单吃的是「父表名/子表名/形态/
//!   媒体参数」四元组，不重扫源文本；
//! - **级联与优先级裁决不归本单**——拓扑序只定「加载先后」，不定
//!   同名规则谁胜谁负（F2809 失效调度与层叠层的活）；
//! - **网络/IO 取表不归本单**——本单图里的节点是**已注册的样式表名**，
//!   取表动作由装载器在拓扑序驱动下另行执行，本单零 IO；
//! - **形态是封闭全集**（[`ImportForm`]）：Plain（纯名）/ Media（带
//!   媒体参数）/ Layer（层叠层包裹）——本引擎子集里 @import 的三种
//!   合法形态，封闭枚举，「表里没有」是正确答案不是遗漏。
//!
//! # 二、图模型：节点=样式表，边=@import 语句
//!
//! - 节点（[`ImportNode`]）：一张已注册样式表，`depth` 记依赖链深度
//!   （根表 0，每被 import 一层 +1）；删除走墓碑（承 F2807 同款语义：
//!   物理搬移会令哈希索引全体失效，墓碑让删除 O(1) 读时过滤）；
//! - 边（[`ImportEdge`]）：一条 @import 语句，`from` 依赖 `to`
//!   （子表必须先于父表加载）；
//! - 索引：名字 → 节点下标，直接**复用 F2807 的
//!   [`veo07_rules::NameIndex`]**（开桶哈希、桶内保插入序、FNV-1a 64）
//!   ——单源复用，不另造一套哈希。
//!
//! # 三、循环检测与拓扑序：Kahn 单源两用
//!
//! 循环检测与拓扑序用**同一套 Kahn 入度消去**（单源两用，判据可对拍）：
//! - 入度 = 该表的未消去依赖数；入度 0 的表（无依赖）先入队；
//! - 消去产物即**拓扑序加载计划**（依赖先于依赖者）；
//! - 若消去停止时仍有节点未出队，剩余节点**恰为环成员**——
//!   循环检测不另写一遍 DFS，判据侧用独立第二套实现对拍（性质断言：
//!   集合相同 + 每条边都满足「先于」），不逐位比序（拓扑序不唯一）。
//!
//! 复杂度入域账本：单边注册/单点查询 O(1) 均摊；整图拓扑序
//! O(V+E)（V≤256、E≤1024 上界内实测微秒级），锚点家族口径
//! 「核心逻辑 O(1)-O(logN)」按**单操作**计量达标。
//!
//! # 四、深度限制：单边 O(1) 恰阈
//!
//! 依赖链深度上界 [`MAX_IMPORT_DEPTH`]（16）：加边时
//! `child.depth = max(child.depth, parent.depth + 1)`，一行算出、
//! 越界即拒（[`E_IMPORT_DEPTH`]）——不必整图重扫。恰阈（16 可收、
//! 17 拒）由判据双向钉死。
//!
//! # 五、错误路径与降级矩阵（锚点家族）
//!
//! - **非法输入 → 校验拒绝三要素**：空名（[`E_IMPORT_NAME_EMPTY`]）、
//!   端点未注册（[`E_IMPORT_UNKNOWN`]）、自环（[`E_IMPORT_SELF_LOOP`]）、
//!   重边（[`E_IMPORT_DUP`]）、形态秩越界（[`E_IMPORT_FORM_INVALID`]，
//!   枚举守卫）——各带专属码 + `next` + `who`，零静默；
//! - **边界越界 → 钳制 + 告警**：表名/参数超该形态规格上界 →
//!   字符边界安全截断 + [`ClampNotice`] 记账；媒体参数收上界内、
//!   超出**计数不静默**；
//! - **容量 → 拒绝并计数**：节点满（[`E_IMPORT_CAP`]）/ 边满（同码）
//!   / 深度越界（[`E_IMPORT_DEPTH`]），不覆盖旧行；
//! - **异常检出 → 立案流转**：循环检出（[`E_IMPORT_CYCLE`]）与每次
//!   拒绝都经 [`CaseLedger::open_case`] 立案（现象/影响/定位/处置）。
//!
//! # 六、跨批对接点
//!
//! - **上游契约接收（哈希对账——家族）**：[`audit_upstream`] 现算
//!   F2807 摘要并对拍 [`veo07_rules::DOWNSTREAM_DECL`]（peer 必须是
//!   本单、三消费面必须齐）；[`upstream_missing_for`] 支持判据注入
//!   劣化摘要验证检出能力，不改真常量；
//! - **下游消费接口（前向声明——家族）**：拓扑序加载计划交
//!   VE-F2809（样式失效与重计算调度）消费，[`DOWNSTREAM_DECL`]
//!   声明三个面（拓扑序/边表/深度剖面），本单只声明不实现消费；
//! - **跨域衔接对账钩子（复用方义务——家族）**：
//!   [`reconcile_import_graph`] 供复用方自查点名表都在图上且非墓碑。

use crate::svstar2::veo01_arch::{CaseLedger, ClampLog, ClampNotice, StyleError};
use crate::svstar2::veo07_rules::{self, NameIndex, Stylesheet};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与锚点
// ---------------------------------------------------------------------------

/// 本单版本（域账本锚）。
pub const IMPORT_VERSION: &str = "O01-import-graph-v1";

/// 上游锚点（承 F2807 样式表对象）。
pub const UPSTREAM_ANCHOR: &str = "VE-F2807/O01-stylesheet-v1";

/// 下游锚点（拓扑序加载计划交 F2809 消费）。
pub const DOWNSTREAM_ANCHOR: &str = "VE-F2809";

// ---------------------------------------------------------------------------
// 二、诊断码（**F2808 独占码段：0x2Bxx**，F2807 注释预留段，逐条冻结）
// ---------------------------------------------------------------------------

/// 码段纪律：F2806=0x2Cxx、F2807=0x2Axx 已占；**本单独占 0x2Bxx**。
pub const E_IMPORT_NAME_EMPTY: &str = "E_IMPORT_NAME_EMPTY";
pub const E_IMPORT_UNKNOWN: &str = "E_IMPORT_UNKNOWN";
pub const E_IMPORT_SELF_LOOP: &str = "E_IMPORT_SELF_LOOP";
pub const E_IMPORT_CYCLE: &str = "E_IMPORT_CYCLE";
pub const E_IMPORT_DEPTH: &str = "E_IMPORT_DEPTH";
pub const E_IMPORT_DUP: &str = "E_IMPORT_DUP";
pub const E_IMPORT_CAP: &str = "E_IMPORT_CAP";
pub const E_IMPORT_FORM_INVALID: &str = "E_IMPORT_FORM_INVALID";
pub const E_IMPORT_UPSTREAM_DRIFT: &str = "E_IMPORT_UPSTREAM_DRIFT";
pub const E_IMPORT_DOWNSTREAM_DRIFT: &str = "E_IMPORT_DOWNSTREAM_DRIFT";

/// 每码对应的「下一步」（拒绝必须给出路——三要素之三）。
pub const FIX_NAME_EMPTY: &str = "import 两端必须是已注册的非空样式表名；先 register_sheet 再 link_import";
pub const FIX_UNKNOWN: &str = "先按名在样式表与依赖图注册该表；图只连已注册节点，不做隐式建点";
pub const FIX_SELF_LOOP: &str = "自 import 恒等空操作，直接删掉该条 @import；若确需拆表请改两表名";
pub const FIX_CYCLE: &str = "按 detect_cycles 列出的环成员拆环：抽公共规则进新表或改单向依赖";
pub const FIX_DEPTH: &str = "依赖链超过深度上界；拍平中间层或按 ADR 提升 MAX_IMPORT_DEPTH";
pub const FIX_DUP: &str = "同 (from,to) 只允许一条边；幂等场景请先 has_edge 查重再决定是否再连";
pub const FIX_CAP: &str = "图容量达到上界；先归档拆除无用节点/边，或按 ADR 提升 MAX_NODES/MAX_EDGES";
pub const FIX_FORM: &str = "形态只能取 ImportForm 三类封闭全集之一；越界秩无对应形态";
pub const FIX_UPSTREAM: &str = "F2807 样式表契约已改动；同步本单承接面与锚点常量后重跑审计";
pub const FIX_DOWNSTREAM: &str = "下游对端标识或消费面被改动；与 F2809 对齐加载计划契约后重跑审计";

// ---------------------------------------------------------------------------
// 三、形态封闭全集与规格表（家族格式：逐条规格公开/参数域钳制/枚举守卫）
// ---------------------------------------------------------------------------

/// @import 形态封闭全集（三类；`of_rank` 越界返回 None = 枚举守卫）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportForm {
    /// 纯名引入：`@import "theme";`。
    Plain,
    /// 带媒体参数：`@import "print" screen and (min-width: 800px);`。
    Media,
    /// 层叠层包裹：`@import "legacy" layer(base);`。
    Layer,
}

pub use ImportForm::{Layer, Media, Plain};

/// 形态总数（封闭全集规模，判据对账锚）。
pub const IMPORT_FORM_COUNT: usize = 3;

impl ImportForm {
    /// 封闭全集（秩序 = 声明序）。
    pub const ALL: [ImportForm; IMPORT_FORM_COUNT] = [Plain, Media, Layer];

    /// 秩（精确 0..=2，判据钉死不许漂移）。
    pub const fn rank(self) -> usize {
        match self {
            Plain => 0,
            Media => 1,
            Layer => 2,
        }
    }

    /// 秩反查（越界 None——枚举守卫的唯一合法通道）。
    pub const fn of_rank(r: usize) -> Option<ImportForm> {
        match r {
            0 => Some(Plain),
            1 => Some(Media),
            2 => Some(Layer),
            _ => None,
        }
    }

    /// 读屏中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Plain => "纯名",
            Media => "媒体",
            Layer => "层叠层",
        }
    }
}

/// 单形态规格（参数域逐条公开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormSpec {
    /// 形态。
    pub form: ImportForm,
    /// 目标表名上界（字节；越界钳制）。
    pub name_cap: usize,
    /// 媒体/层名参数上界（字节；Plain 恒 0）。
    pub param_cap: usize,
}

/// 规格表（与封闭全集逐位对位；编译期闸钉死长度与语义）。
pub const FORM_SPECS: [FormSpec; IMPORT_FORM_COUNT] = [
    FormSpec { form: Plain, name_cap: 128, param_cap: 0 },
    FormSpec { form: Media, name_cap: 128, param_cap: 256 },
    FormSpec { form: Layer, name_cap: 128, param_cap: 64 },
];

/// 按形态取规格（越界秩返回 None；const 期 match 索引——slice::get
/// 在本工具链尚未 const 稳定，禁用）。
pub const fn spec_of_form(f: ImportForm) -> Option<&'static FormSpec> {
    match f.rank() {
        0 => Some(&FORM_SPECS[0]),
        1 => Some(&FORM_SPECS[1]),
        2 => Some(&FORM_SPECS[2]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 四、容量与索引参数
// ---------------------------------------------------------------------------

/// 节点（样式表）上限。
pub const MAX_NODES: usize = 256;
/// 边（@import 语句）上限。
pub const MAX_EDGES: usize = 1024;
/// 依赖链深度上界（恰阈判据：16 可收、17 拒）。
pub const MAX_IMPORT_DEPTH: u32 = 16;
/// 单节点出边（该表 import 他表）上限。
pub const EDGES_PER_NODE: usize = 32;
/// 哈希桶数（复用 F2807 NameIndex；须 ≥ 节点上限且 2 的幂）。
pub const INDEX_BUCKETS: usize = 512;

// ---------------------------------------------------------------------------
// 五、节点与边
// ---------------------------------------------------------------------------

/// 依赖图节点：一张已注册样式表。
#[derive(Clone, Debug, PartialEq)]
pub struct ImportNode {
    /// 节点 id（单调递增，全图唯一）。
    pub id: u64,
    /// 样式表名（注册唯一键）。
    pub name: String,
    /// 依赖链深度（根表 0；被 import 一层 +1；上界 MAX_IMPORT_DEPTH）。
    pub depth: u32,
    /// 墓碑（拆除留痕，读时过滤）。
    pub removed: bool,
}

impl ImportNode {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "#{}「{}」深度{}{}",
            self.id,
            self.name,
            self.depth,
            if self.removed { "〔已拆除〕" } else { "" }
        )
    }
}

/// 依赖图边：一条 @import 语句（from 依赖 to）。
#[derive(Clone, Debug, PartialEq)]
pub struct ImportEdge {
    /// 父表下标（依赖方）。
    pub from: u32,
    /// 子表下标（被依赖方，先加载）。
    pub to: u32,
    /// 形态。
    pub form: ImportForm,
    /// 媒体/层参数（Plain 恒空串）。
    pub param: String,
    /// 源偏移。
    pub offset: u32,
}

impl ImportEdge {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "「{}」⇽「{}」（{}{}，偏移 {}）",
            self.from,
            self.to,
            self.form.zh(),
            if self.param.is_empty() {
                String::new()
            } else {
                format!("，参数「{}」", self.param)
            },
            self.offset
        )
    }
}

// ---------------------------------------------------------------------------
// 六、统计（失败显性化）
// ---------------------------------------------------------------------------

/// 依赖图操作统计。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportStats {
    /// 注册尝试数。
    pub attempts: u32,
    /// 注册成功数。
    pub accepted: u32,
    /// 被拒数（空名/重名/未注册/自环/重边/容量/深度/形态越界）。
    pub rejected: u32,
    /// 钳制次数（名字/参数截断）。
    pub clamped: u32,
    /// 在账边数。
    pub edges: u32,
    /// 环检出次数。
    pub cycles: u32,
}

impl ImportStats {
    /// 守恒：成功 + 被拒 == 尝试。
    pub fn conserves(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.attempts
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "依赖图：注册尝试 {} 次，接受 {}，拒绝 {}，钳制 {}，在账边 {} 条，环检出 {} 次",
            self.attempts, self.accepted, self.rejected, self.clamped, self.edges, self.cycles
        )
    }
}

// ---------------------------------------------------------------------------
// 七、依赖图对象
// ---------------------------------------------------------------------------

/// @import 依赖图（建图 + 循环检测 + 拓扑序加载计划）。
pub struct ImportGraph {
    /// 节点（注册序存放）。
    pub nodes: Vec<ImportNode>,
    /// 边（追加序存放）。
    pub edges: Vec<ImportEdge>,
    /// 名字 → 节点下标索引（复用 F2807 NameIndex，O(1) 平均）。
    pub index: NameIndex,
    /// 每节点出边计数（EDGES_PER_NODE 闸，下标对位 nodes）。
    out_degree: Vec<u32>,
    /// 钳制告警账（承 F2801/F2807 家族）。
    pub clamps: ClampLog,
    /// 案件账（承 F2801/F2807 家族）。
    pub cases: CaseLedger,
    /// 统计。
    pub stats: ImportStats,
    /// 节点 id 发放器。
    next_id: u64,
    /// 逻辑 tick（唯一时间源，零墙钟）。
    pub tick: u64,
}

impl ImportGraph {
    /// 空依赖图。
    pub fn new() -> ImportGraph {
        ImportGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            index: NameIndex::new(),
            out_degree: Vec::new(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            stats: ImportStats::default(),
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
    fn note_clamp(&mut self, field: &str, original: usize, clamped: usize, high: usize) {
        let _ = self.clamps.push(ClampNotice {
            field: String::from(field),
            original: original as f64,
            clamped: clamped as f64,
            low: 0.0,
            high: high as f64,
            tick: self.tick,
        });
        self.stats.clamped = self.stats.clamped.saturating_add(1);
    }

    /// 字符边界安全截断（承 F2807 同款逐字符收法，禁裸字节切片）。
    fn clamp_str(&mut self, field: &str, s: &str, cap: usize) -> String {
        if s.len() <= cap {
            return String::from(s);
        }
        let mut end = cap;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let clamped = String::from(&s[..end]);
        self.note_clamp(field, s.len(), clamped.len(), cap);
        clamped
    }

    /// 按名查在账节点下标（墓碑与未注册统一返回 None）。
    pub fn live_idx(&self, name: &str) -> Option<usize> {
        let hits = self.index.get(name, |i| self.nodes.get(i as usize).map(|n| n.name.as_str()));
        let mut found = None;
        let mut k = 0usize;
        while k < hits.len() {
            let i = hits[k] as usize;
            if let Some(n) = self.nodes.get(i) {
                if !n.removed {
                    found = Some(i);
                }
            }
            k += 1;
        }
        found
    }

    /// 该 (from,to) 边是否已存在（重边闸，O(出度) 线性扫）。
    pub fn has_edge(&self, from: usize, to: usize) -> bool {
        let mut hit = false;
        let mut k = 0usize;
        while k < self.edges.len() {
            let e = &self.edges[k];
            if e.from as usize == from && e.to as usize == to {
                hit = true;
            }
            k += 1;
        }
        hit
    }

    /// **注册一张样式表**（建点；O(1) 均摊）。
    ///
    /// 空名拒 / 名超上界钳制 / 节点满拒 / 重名（在账）拒。
    pub fn register_sheet(&mut self, name: &str) -> Result<u64, StyleError> {
        self.stats.attempts = self.stats.attempts.saturating_add(1);
        // 闸 1：空名拒（非法输入 → 拒绝三要素）。
        if name.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_NAME_EMPTY,
                "注册被拒：样式表名为空",
                "空名无法被 @import 引用，也无法在索引里命中",
                FIX_NAME_EMPTY,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "register_sheet/empty-name");
            return Err(err);
        }
        // 闸 2：重名拒（在账节点唯一键；墓碑不复用）。
        if self.live_idx(name).is_some() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_DUP,
                "注册被拒：样式表名已在图上",
                "同名双节点会让依赖边指代不明，环检测与拓扑序失去唯一语义",
                FIX_DUP,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "register_sheet/dup-name");
            return Err(err);
        }
        // 闸 3：容量拒（满后不覆盖，旧行不凭空消失）。
        if self.nodes.len() >= MAX_NODES {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_CAP,
                "注册被拒：依赖图节点已满",
                "节点数达到上界，继续塞入会挤出既有节点",
                FIX_CAP,
                "O 域组件负责人",
            );
            self.file_case(&err, "register_sheet/nodes-cap");
            return Err(err);
        }
        // 闸 4：名字超上界 → 钳制 + 告警（不拒——越界家族）。
        let clamped = self.clamp_str("sheet:name", name, 128);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let idx = self.nodes.len() as u32;
        self.nodes.push(ImportNode {
            id,
            name: clamped,
            depth: 0,
            removed: false,
        });
        self.out_degree.push(0);
        self.index.insert(name, idx);
        self.stats.accepted = self.stats.accepted.saturating_add(1);
        Ok(id)
    }

    /// **连一条 @import 边**（父表依赖子表；子表先加载）。
    ///
    /// 闸序：端点在图（[`E_IMPORT_UNKNOWN`]）→ 自环（[`E_IMPORT_SELF_LOOP`]）
    /// → 重边（[`E_IMPORT_DUP`]）→ 出边容量（[`E_IMPORT_CAP`]）→
    /// 深度恰阈（[`E_IMPORT_DEPTH`]）；参数超形态规格上界 → 钳制 + 告警。
    pub fn link_import(
        &mut self,
        parent: &str,
        child: &str,
        form: ImportForm,
        param: &str,
        offset: u32,
    ) -> Result<u64, StyleError> {
        self.stats.attempts = self.stats.attempts.saturating_add(1);
        // 闸 0：形态守卫（枚举越界秩不可经 of_rank 造假，防御性复检）。
        if ImportForm::of_rank(form.rank()) != Some(form) {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_FORM_INVALID,
                "连边被拒：形态秩与封闭全集对不上",
                "形态枚举被越权构造（秩不在 0..=2）",
                FIX_FORM,
                "O 域组件负责人",
            );
            self.file_case(&err, "link_import/form-rank");
            return Err(err);
        }
        // 闸 1：两端必须已注册（空名也落在未注册里，但空名单列码更可归因）。
        if parent.is_empty() || child.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_NAME_EMPTY,
                "连边被拒：端点名为空",
                "空名无法定位任何节点",
                FIX_NAME_EMPTY,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "link_import/empty-endpoint");
            return Err(err);
        }
        let from = match self.live_idx(parent) {
            Some(i) => i,
            None => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_IMPORT_UNKNOWN,
                    "连边被拒：父表未注册",
                    "依赖图只连已注册节点，不做隐式建点",
                    FIX_UNKNOWN,
                    "复用方（装载器/上游规则层）",
                );
                self.file_case(&err, "link_import/unknown-parent");
                return Err(err);
            }
        };
        let to = match self.live_idx(child) {
            Some(i) => i,
            None => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_IMPORT_UNKNOWN,
                    "连边被拒：子表未注册",
                    "依赖图只连已注册节点，不做隐式建点",
                    FIX_UNKNOWN,
                    "复用方（装载器/上游规则层）",
                );
                self.file_case(&err, "link_import/unknown-child");
                return Err(err);
            }
        };
        // 闸 2：自环拒。
        if from == to {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_SELF_LOOP,
                "连边被拒：表 import 自身",
                "自 import 恒等空操作，却会把拓扑序打成环",
                FIX_SELF_LOOP,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "link_import/self-loop");
            return Err(err);
        }
        // 闸 3：重边拒。
        if self.has_edge(from, to) {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_DUP,
                "连边被拒：同向边已存在",
                "同 (from,to) 双边会让入度消去重复计数，拓扑序失真",
                FIX_DUP,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "link_import/dup-edge");
            return Err(err);
        }
        // 闸 4：出边容量拒。
        let od = self.out_degree[from] as usize;
        if od >= EDGES_PER_NODE {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_CAP,
                "连边被拒：该表出边已达上界",
                "单表 import 数超过 EDGES_PER_NODE，入度账继续堆积会失真",
                FIX_CAP,
                "O 域组件负责人",
            );
            self.file_case(&err, "link_import/out-degree-cap");
            return Err(err);
        }
        // 闸 5：全图边容量拒。
        if self.edges.len() >= MAX_EDGES {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_CAP,
                "连边被拒：依赖图边已满",
                "边数达到上界，继续塞入会挤出既有边",
                FIX_CAP,
                "O 域组件负责人",
            );
            self.file_case(&err, "link_import/edges-cap");
            return Err(err);
        }
        // 闸 6：深度恰阈拒（单边 O(1)：child.depth 将达 parent.depth+1）。
        let parent_depth = self.nodes[from].depth;
        if parent_depth.saturating_add(1) > MAX_IMPORT_DEPTH {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_DEPTH,
                "连边被拒：依赖链深度越界",
                "子表深度将超过 MAX_IMPORT_DEPTH，链式 import 过深",
                FIX_DEPTH,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "link_import/depth-cap");
            return Err(err);
        }
        // 越界 → 钳制：参数按形态规格上界收（Plain 恒收成空串语义由
        // 规格 param_cap=0 兑现——钳到 0 即空）。
        let param_cap = match spec_of_form(form) {
            Some(sp) => sp.param_cap,
            None => 0,
        };
        let clamped_param = self.clamp_str("import:param", param, param_cap);
        // 提交：边入账 + 子表深度抬升 + 出度计数。
        let eid = self.edges.len() as u64;
        self.edges.push(ImportEdge {
            from: from as u32,
            to: to as u32,
            form,
            param: clamped_param,
            offset,
        });
        let child_depth = self.nodes[to].depth.max(parent_depth.saturating_add(1));
        self.nodes[to].depth = child_depth;
        self.out_degree[from] = self.out_degree[from].saturating_add(1);
        self.stats.edges = self.stats.edges.saturating_add(1);
        Ok(eid)
    }

    /// **循环检测**（Kahn 剩余 = 环成员；零 panic 面）。
    ///
    /// 边语义：from 依赖 to ⇒ **to 必须先于 from 加载**。入度记在
    /// from 上（依赖者的未消去依赖数）；入度 0 = 无依赖 = 先出队。
    /// 出队 cur 后消去它的所有「被依赖边」（e.to == cur）。
    /// 返回在账环成员名清单（Kahn 剩余的精确语义：环上成员 +
    /// 被环拖住的下游依赖者）。
    pub fn detect_cycles(&self) -> Vec<String> {
        let n = self.nodes.len();
        let mut indeg = vec![0u32; n];
        let mut k = 0usize;
        while k < self.edges.len() {
            let e = &self.edges[k];
            if (e.from as usize) < n {
                indeg[e.from as usize] = indeg[e.from as usize].saturating_add(1);
            }
            k += 1;
        }
        let mut queue: Vec<usize> = Vec::new();
        let mut head = 0usize;
        let mut i = 0usize;
        while i < n {
            if indeg[i] == 0 && !self.nodes[i].removed {
                queue.push(i);
            }
            i += 1;
        }
        while head < queue.len() {
            let cur = queue[head];
            head += 1;
            let mut j = 0usize;
            while j < self.edges.len() {
                let e = &self.edges[j];
                if e.to as usize == cur && (e.from as usize) < n {
                    let f = e.from as usize;
                    indeg[f] = indeg[f].saturating_sub(1);
                    if indeg[f] == 0 && !self.nodes[f].removed {
                        queue.push(f);
                    }
                }
                j += 1;
            }
        }
        // 剩余 = 环成员（或被环拖住的下游）。
        let mut leftover: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < n {
            let mut drained = false;
            let mut q = 0usize;
            while q < queue.len() {
                if queue[q] == i {
                    drained = true;
                }
                q += 1;
            }
            if !drained && !self.nodes[i].removed {
                leftover.push(String::from(self.nodes[i].name.as_str()));
            }
            i += 1;
        }
        leftover
    }

    /// **拓扑序加载计划**（依赖先于依赖者；O(V+E)）。
    ///
    /// 有环 → [`E_IMPORT_CYCLE`]（拒绝整图出序，绝不输出部分序——
    /// 部分序会让装载器以看似合法的顺序加载坏图）并立案流转。
    pub fn topo_order(&mut self) -> Result<Vec<String>, StyleError> {
        self.tick = self.tick.saturating_add(1);
        let leftover = self.detect_cycles();
        if !leftover.is_empty() {
            self.stats.cycles = self.stats.cycles.saturating_add(1);
            let err = StyleError::new(
                E_IMPORT_CYCLE,
                "拓扑序被拒：依赖图存在循环 import",
                "环上表互相依赖，任何全序都无法满足「依赖先于依赖者」",
                FIX_CYCLE,
                "复用方（装载器/上游规则层）",
            );
            self.file_case(&err, "topo_order/cycle");
            return Err(err);
        }
        let n = self.nodes.len();
        let mut indeg = vec![0u32; n];
        let mut k = 0usize;
        while k < self.edges.len() {
            let e = &self.edges[k];
            if (e.from as usize) < n {
                indeg[e.from as usize] = indeg[e.from as usize].saturating_add(1);
            }
            k += 1;
        }
        let mut queue: Vec<usize> = Vec::new();
        let mut head = 0usize;
        let mut i = 0usize;
        while i < n {
            if indeg[i] == 0 && !self.nodes[i].removed {
                queue.push(i);
            }
            i += 1;
        }
        while head < queue.len() {
            let cur = queue[head];
            head += 1;
            let mut j = 0usize;
            while j < self.edges.len() {
                let e = &self.edges[j];
                if e.to as usize == cur && (e.from as usize) < n {
                    let f = e.from as usize;
                    indeg[f] = indeg[f].saturating_sub(1);
                    if indeg[f] == 0 && !self.nodes[f].removed {
                        queue.push(f);
                    }
                }
                j += 1;
            }
        }
        let mut out: Vec<String> = Vec::new();
        let mut q = 0usize;
        while q < queue.len() {
            out.push(String::from(self.nodes[queue[q]].name.as_str()));
            q += 1;
        }
        Ok(out)
    }

    /// 下游消费面之二：**在账边表快照**（from 名/to 名/形态/参数）。
    pub fn import_edges(&self) -> Vec<(String, String, ImportForm, String)> {
        let mut out: Vec<(String, String, ImportForm, String)> = Vec::new();
        let mut k = 0usize;
        while k < self.edges.len() {
            let e = &self.edges[k];
            let f = self.nodes.get(e.from as usize).map(|n| n.name.as_str());
            let t = self.nodes.get(e.to as usize).map(|n| n.name.as_str());
            if let (Some(f), Some(t)) = (f, t) {
                out.push((
                    String::from(f),
                    String::from(t),
                    e.form,
                    String::from(e.param.as_str()),
                ));
            }
            k += 1;
        }
        out
    }

    /// 下游消费面之三：**深度剖面**（最深依赖链 + 达到该深度的表数）。
    pub fn depth_profile(&self) -> (u32, usize) {
        let mut max_d = 0u32;
        let mut count = 0usize;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if !self.nodes[i].removed {
                let d = self.nodes[i].depth;
                if d > max_d {
                    max_d = d;
                    count = 1;
                } else if d == max_d {
                    count += 1;
                }
            }
            i += 1;
        }
        (max_d, count)
    }

    /// 读屏摘要（整图）。
    pub fn screen_text(&self) -> String {
        let (max_d, cnt) = self.depth_profile();
        format!(
            "依赖图：在账表 {} 张、在账边 {} 条，最深链 {} 层（{} 张表触底），环检出 {} 次",
            self.nodes.len(), self.edges.len(), max_d, cnt, self.stats.cycles
        )
    }
}

// ---------------------------------------------------------------------------
// 八、跨批对接点
// ---------------------------------------------------------------------------

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

/// 对账核心（**判据可注入**）：给定上游摘要，返回本单点名却缺失的
/// 锚串个数。锚串取自 F2807 `domain_summary()` 的真实产物
/// （"O01-stylesheet-v1｜七类节点｜规则上限 256｜…"），判据侧另写
/// 独立期望清单对拍（防同源恒绿）。
pub fn upstream_missing_for(summary: &str) -> usize {
    let peers = ["O01-stylesheet-v1", "七类节点", "256"];
    let mut missing = 0usize;
    let mut k = 0usize;
    while k < peers.len() {
        if !summary.contains(peers[k]) {
            missing += 1;
        }
        k += 1;
    }
    missing
}

/// 上游契约接收与哈希对账（**运行期现算**，不抄常量）。
///
/// 双闸：① F2807 摘要含本单点名锚串；② F2807 的下游前向声明
/// （`DOWNSTREAM_DECL`）peer 恰是本单、三消费面齐——上游承诺的
/// 消费接口本单如约兑现。
pub fn audit_upstream(sheet_summary: &str) -> UpstreamContract {
    let missing = upstream_missing_for(sheet_summary);
    let decl_ok = veo07_rules::DOWNSTREAM_DECL.peer == "VE-F2808"
        && veo07_rules::DOWNSTREAM_DECL.faces.len() >= 3;
    UpstreamContract {
        peer: "VE-F2807",
        anchor: String::from(UPSTREAM_ANCHOR),
        summary_len: sheet_summary.len() as u32,
        missing,
        matched: missing == 0 && decl_ok && !sheet_summary.is_empty(),
    }
}

/// 下游消费接口的正式声明（**只声明，不实现**；F2809 消费本单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownstreamDecl {
    /// 下游模块标识。
    pub peer: &'static str,
    /// 下游需要的面（按序）。
    pub faces: &'static [&'static str],
}

/// 下游声明：F2809 按这三个面消费依赖图。
pub const DOWNSTREAM_DECL: DownstreamDecl = DownstreamDecl {
    peer: "VE-F2809",
    faces: &["topo_order", "import_edges", "depth_profile"],
};

/// 下游声明审计（只查声明完整性，不实现消费逻辑）。
pub fn audit_downstream() -> Result<&'static str, StyleError> {
    if DOWNSTREAM_DECL.peer != DOWNSTREAM_ANCHOR {
        return Err(StyleError::new(
            E_IMPORT_DOWNSTREAM_DRIFT,
            "下游声明审计失败：对端标识不符",
            "本单的加载计划交 F2809 消费；对端标识被改动",
            FIX_DOWNSTREAM,
            "O 域组件负责人",
        ));
    }
    if DOWNSTREAM_DECL.faces.len() < 3 {
        return Err(StyleError::new(
            E_IMPORT_DOWNSTREAM_DRIFT,
            "下游声明审计失败：消费面不足",
            "F2809 至少需要 拓扑序/边表/深度剖面 三个消费面",
            FIX_DOWNSTREAM,
            "O 域组件负责人",
        ));
    }
    Ok("下游消费接口前向声明完整（F2809 取 topo_order/import_edges/depth_profile）")
}

/// 跨域衔接对账钩子（**复用方义务**：本单提供判据，义务在复用方）。
///
/// 复用方拿到一组「样式表名」后应当调这个自查：每条名都在图上
/// 在账（非墓碑、未拆除）。
pub fn reconcile_import_graph(g: &ImportGraph, names: &[&str]) -> Result<usize, StyleError> {
    if names.is_empty() {
        return Err(StyleError::new(
            E_IMPORT_NAME_EMPTY,
            "对账失败：名字清单为空",
            "空清单的对账是空转，复用方应当至少点一张表",
            FIX_NAME_EMPTY,
            "复用方（下游模块）",
        ));
    }
    let mut total = 0usize;
    let mut k = 0usize;
    while k < names.len() {
        let n = names[k];
        if n.is_empty() {
            return Err(StyleError::new(
                E_IMPORT_NAME_EMPTY,
                "对账失败：清单含空名",
                "空名无法定位任何节点",
                FIX_NAME_EMPTY,
                "复用方（下游模块）",
            ));
        }
        if g.live_idx(n).is_none() {
            return Err(StyleError::new(
                E_IMPORT_UNKNOWN,
                "对账失败：点名在依赖图无在账节点",
                "该表未注册或已拆除（墓碑不可复用）",
                FIX_UNKNOWN,
                "复用方（下游模块）",
            ));
        }
        total += 1;
        k += 1;
    }
    Ok(total)
}

/// 规格表审计（运行期对照编译期闸）。
pub fn audit_spec_table() -> Result<String, StyleError> {
    if FORM_SPECS.len() != IMPORT_FORM_COUNT {
        return Err(StyleError::new(
            E_IMPORT_FORM_INVALID,
            "规格表审计失败：条数与封闭全集不一致",
            "规格表与形态枚举必须逐位对位",
            FIX_FORM,
            "O 域组件负责人",
        ));
    }
    let mut k = 0usize;
    while k < FORM_SPECS.len() {
        let sp = &FORM_SPECS[k];
        if sp.form.rank() != k {
            return Err(StyleError::new(
                E_IMPORT_FORM_INVALID,
                "规格表审计失败：秩漂移",
                "规格表第 k 行的形态秩必须恰为 k",
                FIX_FORM,
                "O 域组件负责人",
            ));
        }
        if sp.name_cap == 0 || sp.name_cap > 1024 {
            return Err(StyleError::new(
                E_IMPORT_CAP,
                "规格表审计失败：名字上界越域",
                "名字上界必须在 1..=1024 内",
                FIX_CAP,
                "O 域组件负责人",
            ));
        }
        k += 1;
    }
    if INDEX_BUCKETS < MAX_NODES || !INDEX_BUCKETS.is_power_of_two() {
        return Err(StyleError::new(
            E_IMPORT_CAP,
            "规格表审计失败：索引桶数不足",
            "桶数必须 ≥ 节点上限且为 2 的幂（否则取模偏斜）",
            FIX_CAP,
            "O 域组件负责人",
        ));
    }
    Ok(format!(
        "依赖图规格表审计通过：{} 形态 × 参数上界逐类齐备，节点上限 {}，边上限 {}，深度上界 {}",
        IMPORT_FORM_COUNT, MAX_NODES, MAX_EDGES, MAX_IMPORT_DEPTH
    ))
}

/// 全域审计汇总（一条命令跑完所有闸，供下游与判据共用）。
pub fn audit_all(sheet: &Stylesheet) -> Result<String, StyleError> {
    let a = audit_spec_table()?;
    // 上游摘要现场取两路：F2807 版本摘要（规格面）+ 本样式表实例
    // 读屏（实例面）——两路拼接后再对账，规格漂移与实例漂移都逃不掉。
    let mut summary = veo07_rules::domain_summary();
    summary.push('；');
    summary.push_str(&sheet.screen_text());
    let b = audit_upstream(&summary);
    if !b.matched {
        return Err(StyleError::new(
            E_IMPORT_UPSTREAM_DRIFT,
            "上游对账失败",
            "F2807 摘要缺本单点名锚串，或其下游声明对端/消费面漂移",
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
        "{}｜三形态（纯名/媒体/层叠层）｜节点上限 {}｜边上限 {}｜深度上界 {}",
        IMPORT_VERSION, MAX_NODES, MAX_EDGES, MAX_IMPORT_DEPTH
    )
}

// ---------------------------------------------------------------------------
// 九、编译期闸（数值域全在这里断）
// ---------------------------------------------------------------------------

const _: () = {
    // 闸 1：规格表与封闭全集逐位对位（数组长度即断言）。
    assert!(FORM_SPECS.len() == IMPORT_FORM_COUNT);

    // 闸 2：秩精确 0..=2（不是单调，是精确值；const 上下文用 matches! 断）。
    assert!(Plain.rank() == 0);
    assert!(Media.rank() == 1);
    assert!(Layer.rank() == 2);
    assert!(matches!(ImportForm::of_rank(0), Some(Plain)));
    assert!(matches!(ImportForm::of_rank(1), Some(Media)));
    assert!(matches!(ImportForm::of_rank(2), Some(Layer)));
    assert!(ImportForm::of_rank(3).is_none());

    // 闸 3：参数上界语义——Plain 无参数、媒体最宽、层叠层窄于媒体。
    assert!(FORM_SPECS[0].param_cap == 0);
    assert!(FORM_SPECS[1].param_cap > FORM_SPECS[2].param_cap);
    assert!(FORM_SPECS[1].param_cap <= 1024);
    assert!(FORM_SPECS[2].param_cap >= 1);

    // 闸 4：容量纪律——桶数为 2 的幂且 ≥ 节点上限；各上界有界。
    assert!(INDEX_BUCKETS.is_power_of_two());
    assert!(INDEX_BUCKETS >= MAX_NODES);
    assert!(MAX_NODES <= 1024);
    assert!(MAX_EDGES <= 4096);
    assert!(EDGES_PER_NODE >= 1 && EDGES_PER_NODE <= 256);

    // 闸 5：深度上界域（1..=64；恰阈判据锚定 16）。
    assert!(MAX_IMPORT_DEPTH >= 1 && MAX_IMPORT_DEPTH <= 64);
    assert!(MAX_IMPORT_DEPTH == 16);
};

// ---------------------------------------------------------------------------
// 十、tests（宿主单测；发行剔除零成本）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_roundtrip() {
        for (i, f) in ImportForm::ALL.iter().enumerate() {
            assert_eq!(f.rank(), i);
            assert_eq!(ImportForm::of_rank(i), Some(*f));
        }
        assert_eq!(ImportForm::of_rank(3), None);
    }

    #[test]
    fn self_loop_rejected() {
        let mut g = ImportGraph::new();
        let _ = g.register_sheet("a");
        assert!(g.link_import("a", "a", Plain, "", 0).is_err());
    }
}
