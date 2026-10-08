//! VE-F3201 · Q 域开工与管线总架构（VE-Q 域 · 资源管线域开工条 · 批次 Q01 首项）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3201`
//!
//! **判据（锚点原文）**：单向委托、六段签名、收敛红线、透传不失真、十项映射、判据。
//!
//! **职责定位（锚点原文）**：Q 域使命（Q=资源管线域：一切资源（纹理/模型/音频/
//! 字体/样式/场景）的统一加载/流送/缓存/生命周期权威——F/Q 分工（F 域=编解码
//! 格式专家（解码单源），Q 域=调度与生命周期专家（管线权威）：Q 调度 F 解码——
//! 单向委托声明）；官方主题资源范围映射（模型/引用/句柄/类型/加载/依赖/并发/
//! 状态/失败/取消/版本/校验/寻址/打包/解码对接/消费协议/调试/预算——十项映射）；
//! 架构六段（寻址→请求→调度→加载→校验→交付句柄——各段签名冻结 v1）；与各消费
//! 域契约（O/N/G/H/I/P 八域消费侧统一接口——多对一收敛红线：消费域不各自加载，
//! 一律走 Q 管线（绕管线私加载=分叉缺陷）。
//!
//! **数据结构（锚点原文）**：六段接口；十项映射；消费收敛契约。
//!
//! **错误路径与降级矩阵（锚点原文）**：段间契约分歧→对拍拦截；私加载检出→缺陷
//! 立案（收敛红线实测）；解码失败→F 域错误码透传+三要素（透传不失真红线）。
//!
//! **性能逐项分解（锚点原文）**：六段流水 O(1) 每段摊销；句柄交付 O(1)；映射 O(1)。
//!
//! **跨批对接点（锚点原文）**：F 域解码接口（冻结接收）；八消费域契约前向；
//! F3216 协议细化。
//!
//! **无障碍与隐私（锚点原文）**：资源 URI 不含用户内容（内容走数据域不走资源
//! 域——归属红线）；无隐私面。
//!
//! **落位与迁移纪律（Variable 2026-10-07 指令）**：本条为**开工条**，纯契约层，
//! 零 IO、零 GPU、零 DOM、无全局可变状态。原`src/system/ve/qDomain/f3201-*.ts`
//! 为 TypeScript 存量实现，已按「TypeScript 存量全面迁移」指令用本Rust 模块
//! 逐条重新实现（迁移对照见 [`MIGRATION_FROM_TS`]）。功能代码全部为 Rust，落在
//! 内核 crate 内，不引入任何前端语言实现。
//!
//! **本项的边界（不越界施工，遵守"只做领到的任务"）**：
//! VE-F3201 是**开工条**——它把「资源怎么被寻址、被调度、被交付、谁负责解码、
//! 消费方怎么接」写成**可机检的契约**，不代做各段的引擎本体。分工在册（见
//! [`DOWNSTREAM_OWNERSHIP`]）：
//! - F3202 拥有**资源实体与引用图**；本条只立「未登记资源即拒绝」的契约；
//! - F3203 拥有**句柄引用计数与分代GC**；本条只立「句柄不透明、释放权唯一」的
//!   契约，句柄本体是 [`ResourceHandle`] 这层壳；
//! - F3205 拥有**优先级队列与请求合并**；本条只立「缓存键在 request 段定死」
//!   的契约；
//! - F3207 拥有**并发双池与背压**；本条只立「并发合并在 schedule 段」的契约；
//! - F3213 拥有**URI 规范化与寻址缓存**；本条只立**方案权限闸与归属红线**
//!   （闸门必须先立，细则后补——见 [`SCHEME_PERMISSION`] 头注）；
//! - F3215 拥有**Q→F 委托协议 v2（流式/进度/取消点）**；本条只立 v1 的委托
//!   方向与错误码透传契约。
//!
//! **设计要点**：
//! - **单向委托是硬红线，不是口号**：[`check_delegation_direction`] 把调用记录
//!   与禁止表双向比对，检出 F→Q 反向依赖即失败。红线两侧都过[`strip_args`]
//!   规范化——禁止表写`F->Q:acquireRefCount(key)`，现场记录可能是无参形态，
//!   两侧不同规范化则`includes` 永不相等，红线**一次都不会触发**，看着有
//!   红线实则没有；
//! - **六段签名冻结 v1**：每段的入参/出参/允许失败码写进 [`STAGE_SPECS`] 数据，
//!   [`audit_stage_contract`] 把实际观察到的失败码与白名单逐段对拍——改签名
//!   是可机检的事实，而不是靠人读 diff；
//! - **收敛红线是可执行形态**：[`acquire_from_pipeline`] 是**唯一**能造句柄的
//!   出口（成功路径无分支，杜绝各处手工造句柄），[`detect_private_load`] 把
//!   绕管线私加载立案为 `PRIVLOAD-<域>-<序号>`；
//! - **透传不失真**：[`verify_passthrough`] 三条判据（原码与F 域登记一致 /
//!   `mutated` 标志为假 / 补充三要素齐备）——透传不等于甩锅，Q 域仍须负责
//!   「怎么办」这一段；
//! - **归属红线的两道判定分离**：非query 段用**形态特征**判定（空白/尖括号等），
//!   query 段用**键名集合**判定（[`CONTENT_QUERY_KEYS`] 反例 /
//!   [`SAFE_QUERY_KEYS`] 白名单）。子串词表挡不住变体（`?title=` vs`&title=`），
//!   故query 段按键名判定并对键名做百分号解码（`%74itle` 不得绕过）；
//! - **越界 `..` 是吸收态**：一旦跳出authority 根，后续 `..` 无法再回到根内，
//!   故 `escaped_root` 置位后**不再清除**——否则 `/../../etc/passwd` 的前两个
//!   `..` 互相抵消，穿越检测形同虚设。
//!
//! **零外部依赖**，只依赖 `alloc` 与 `crate::checks`（自检侧）。
//! 确定性：逻辑 tick 注入、零墙钟、零 IO，回归可复现（对拍红线）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断基础设施（零静默第一层）
// ---------------------------------------------------------------------------

/// 诊断码：每种失败独立可检索。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagCode {
    /// 六段之一未注册（阶段名不在冻结清单里）。
    StageUnregistered,
    /// 六段顺序被改（阶段次序不满足冻结签名要求）。
    StageOrderViolated,
    /// 资源类型不在十项映射的覆盖集内。
    ResourceTypeUnmapped,
    /// 资源 URI 含用户内容（违反归属红线）。
    UriContainsUserContent,
    /// URI 结构非法（缺 scheme / 段数不对）。
    UriMalformed,
    /// F→Q 反向依赖被检出（违反单向委托）。
    DelegationDirectionViolated,
    /// 解码错误码被改写（违反透传不失真）。
    ErrorCodeMutated,
    /// 检出消费域私加载（违反收敛红线）。
    PrivateLoadDetected,
    /// 段间契约分歧（对拍不通过）。
    StageContractDiverged,
    /// 预算超限。
    BudgetExceeded,
    /// 引用环（资源依赖成环，会让加载死锁）。
    DependencyCycle,
    /// 读盘超时。
    IoTimeout,
    /// 资源不存在（包内索引未命中 / 文件缺失）。
    IoNotFound,
    /// 读盘失败（介质错误、权限不足）。
    IoFailed,
    /// 源不可信（来源不在白名单，或签名校验不通过——安全前置闸）。
    SourceUntrusted,
    /// 内容完整性校验不符（哈希与期望不符）。
    HashMismatch,
    /// F 域解码失败（原码透传，此码仅标记「已进入解码」这一段）。
    DecodeFailed,
    /// 请求被取消（与失败语义分离：`cancelled`≠`failed`）。
    Cancelled,
    /// 非法参数。
    ValueInvalid,
}

impl DiagCode {
    /// 诊断码的稳定字符串（门禁比对与跨语言对拍用）。
    pub fn code(self) -> &'static str {
        match self {
            DiagCode::StageUnregistered => "STAGE_UNREGISTERED",
            DiagCode::StageOrderViolated => "STAGE_ORDER_VIOLATED",
            DiagCode::ResourceTypeUnmapped => "RESOURCE_TYPE_UNMAPPED",
            DiagCode::UriContainsUserContent => "URI_CONTAINS_USER_CONTENT",
            DiagCode::UriMalformed => "URI_MALFORMED",
            DiagCode::DelegationDirectionViolated => "DELEGATION_DIRECTION_VIOLATED",
            DiagCode::ErrorCodeMutated => "ERROR_CODE_MUTATED",
            DiagCode::PrivateLoadDetected => "PRIVATE_LOAD_DETECTED",
            DiagCode::StageContractDiverged => "STAGE_CONTRACT_DIVERGED",
            DiagCode::BudgetExceeded => "BUDGET_EXCEEDED",
            DiagCode::DependencyCycle => "DEPENDENCY_CYCLE",
            DiagCode::IoTimeout => "IO_TIMEOUT",
            DiagCode::IoNotFound => "IO_NOT_FOUND",
            DiagCode::IoFailed => "IO_FAILED",
            DiagCode::SourceUntrusted => "SOURCE_UNTRUSTED",
            DiagCode::HashMismatch => "HASH_MISMATCH",
            DiagCode::DecodeFailed => "DECODE_FAILED",
            DiagCode::Cancelled => "CANCELLED",
            DiagCode::ValueInvalid => "VALUE_INVALID",
        }
    }

    /// 由稳定字符串反查诊断码（对拍与台账核验用；未登记返回 `None`）。
    ///
    /// 这条反查是「白名单不得过窄」的自检基础：若某码无法由字符串反查，
    /// 则它在跨语言对拍中不可寻址，等于该失败码在契约层不存在。
    pub fn from_code(s: &str) -> Option<DiagCode> {
        Some(match s {
            "STAGE_UNREGISTERED" => DiagCode::StageUnregistered,
            "STAGE_ORDER_VIOLATED" => DiagCode::StageOrderViolated,
            "RESOURCE_TYPE_UNMAPPED" => DiagCode::ResourceTypeUnmapped,
            "URI_CONTAINS_USER_CONTENT" => DiagCode::UriContainsUserContent,
            "URI_MALFORMED" => DiagCode::UriMalformed,
            "DELEGATION_DIRECTION_VIOLATED" => DiagCode::DelegationDirectionViolated,
            "ERROR_CODE_MUTATED" => DiagCode::ErrorCodeMutated,
            "PRIVATE_LOAD_DETECTED" => DiagCode::PrivateLoadDetected,
            "STAGE_CONTRACT_DIVERGED" => DiagCode::StageContractDiverged,
            "BUDGET_EXCEEDED" => DiagCode::BudgetExceeded,
            "DEPENDENCY_CYCLE" => DiagCode::DependencyCycle,
            "IO_TIMEOUT" => DiagCode::IoTimeout,
            "IO_NOT_FOUND" => DiagCode::IoNotFound,
            "IO_FAILED" => DiagCode::IoFailed,
            "SOURCE_UNTRUSTED" => DiagCode::SourceUntrusted,
            "HASH_MISMATCH" => DiagCode::HashMismatch,
            "DECODE_FAILED" => DiagCode::DecodeFailed,
            "CANCELLED" => DiagCode::Cancelled,
            "VALUE_INVALID" => DiagCode::ValueInvalid,
            _ => return None,
        })
    }

    /// 诊断码全集（遍历与自检用，避免手写清单与枚举漂移）。
    pub const ALL: [DiagCode; 18] = [
        DiagCode::StageUnregistered,
        DiagCode::StageOrderViolated,
        DiagCode::ResourceTypeUnmapped,
        DiagCode::UriContainsUserContent,
        DiagCode::UriMalformed,
        DiagCode::DelegationDirectionViolated,
        DiagCode::ErrorCodeMutated,
        DiagCode::PrivateLoadDetected,
        DiagCode::StageContractDiverged,
        DiagCode::BudgetExceeded,
        DiagCode::DependencyCycle,
        DiagCode::IoTimeout,
        DiagCode::IoNotFound,
        DiagCode::IoFailed,
        DiagCode::SourceUntrusted,
        DiagCode::HashMismatch,
        DiagCode::DecodeFailed,
        DiagCode::Cancelled,
    ];
}

/// 一条诊断：发生了什么（人话）、影响什么、下一步怎么办。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 现象（人话）。
    pub message: String,
    /// 下一步（处置建议——非空是硬要求，空建议等于把问题推给调用方猜）。
    pub hint: String,
}

impl Diagnostic {
    /// 读屏可读单行（无障碍：诊断要能念出来，且必须带出「怎么办」）。
    pub fn screen_line(&self) -> String {
        format!(
            "诊断 {}：{}；处置建议：{}",
            self.code.code(),
            self.message,
            self.hint
        )
    }
}

/// 结果判别：成功必带 `value`，失败必带 `code`/`message`/`hint`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// 成功（`diagnostics` 可携带非致命告警）。
    Ok {
        /// 产出值。
        value: T,
        /// 非致命告警（成功路径也允许有告警——如启动预载的可延后失败）。
        diagnostics: Vec<Diagnostic>,
    },
    /// 失败（三要素齐备，零静默）。
    Err {
        /// 诊断码。
        code: DiagCode,
        /// 现象。
        message: String,
        /// 下一步。
        hint: String,
        /// 诊断明细（失败必至少一条）。
        diagnostics: Vec<Diagnostic>,
    },
}

/// 失败载荷：`?` 桥接在 `Result` 侧承载的形态。
///
/// 独立成类型而非复用 `Outcome<()>`，是为了避开 blanket impl 与 core 的
/// `impl<T> From<T> for T`（`T = ()` 时两者冲突，编译直接拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// 诊断码。
    pub code: DiagCode,
    /// 现象。
    pub message: String,
    /// 下一步。
    pub hint: String,
    /// 诊断明细。
    pub diagnostics: Vec<Diagnostic>,
}

impl Failure {
    /// 还原为无值失败（供需要 `Outcome<()>` 的场合）。
    pub fn to_outcome(self) -> Outcome<()> {
        Outcome::Err {
            code: self.code,
            message: self.message,
            hint: self.hint,
            diagnostics: self.diagnostics,
        }
    }
}

impl<T> From<Failure> for Outcome<T> {
    fn from(f: Failure) -> Self {
        Outcome::Err {
            code: f.code,
            message: f.message,
            hint: f.hint,
            diagnostics: f.diagnostics,
        }
    }
}

impl<T> Outcome<T> {
    /// 成功构造。
    pub fn ok(value: T) -> Self {
        Outcome::Ok {
            value,
            diagnostics: Vec::new(),
        }
    }

    /// 成功构造并携带告警。
    pub fn ok_with(value: T, diagnostics: Vec<Diagnostic>) -> Self {
        Outcome::Ok { value, diagnostics }
    }

    /// 失败构造：三要素齐备。
    pub fn err(code: DiagCode, message: &str, hint: &str) -> Self {
        Outcome::Err {
            code,
            message: message.to_string(),
            hint: hint.to_string(),
            diagnostics: alloc::vec![Diagnostic {
                code,
                message: message.to_string(),
                hint: hint.to_string(),
            }],
        }
    }

    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 是否失败。
    pub fn is_err(&self) -> bool {
        matches!(self, Outcome::Err { .. })
    }

    /// 取产出（失败时给出显式兜底值而非 panic——守卫不得成为崩溃源）。
    ///
    /// 按值消费本结果（`self`），故不对 `T` 提出 `Clone` 约束——内核侧
    /// 大量产出类型（如含 `Vec` 的报告）不实现 `Clone`，强加约束会逼调用方
    /// 做无谓的深拷贝。
    pub fn value_or(self, fallback: T) -> T {
        match self {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { .. } => fallback,
        }
    }

    /// 失败穿透：保持失败内容不变，把成功值类型从 `T` 换成 `U`。
    ///
    /// 用于「上一层成功、下一层可能失败」的链式闸门：上一层拿到值继续走，
    /// 下一层失败时原样把失败交回去（诊断明细一条不少）。这是本域不依赖
    /// `Try` trait 也能写出可读链式闸门的手段。
    ///
    /// 成功侧刻意**不**静默通过：若有人把成功值丢在这里，返回的是一条显式
    /// 失败（形态错误），而不是假装成功的空值——静默通过会让下游拿到无值却
    /// 以为成功，缺陷延后到集成期才暴露。
    pub fn map_empty<U>(self) -> Outcome<U> {
        match self {
            Outcome::Ok { .. } => Outcome::err(
                DiagCode::ValueInvalid,
                "成功值被穿透丢弃（形态错误）",
                "此处应先取出成功值再进入下一道闸；请勿对成功的 Outcome 调用 map_empty",
            ),
            Outcome::Err {
                code,
                message,
                hint,
                diagnostics,
            } => Outcome::Err {
                code,
                message,
                hint,
                diagnostics,
            },
        }
    }

    /// 转成 `Result`，使 `?` 能在返回 `Result` 的函数里传播失败。
    ///
    /// 为何需要它：`Try` trait 在 stable 上不可实现，故本域用
    /// `into_result()?` 这一行桥接，而不是把 `Outcome` 改回 `Result`——
    /// `Result` 的 `Err(String)` 装不下三要素，零静默纪律会被迫退化。
    /// 桥接后失败侧仍是完整的 `Outcome<()>`（含全部诊断明细），信息不丢。
    pub fn into_result(self) -> Result<T, Failure> {
        match self {
            Outcome::Ok { value, .. } => Ok(value),
            Outcome::Err {
                code,
                message,
                hint,
                diagnostics,
            } => Err(Failure {
                code,
                message,
                hint,
                diagnostics,
            }),
        }
    }

    /// 只读取引用（成功时；失败时给出显式兜底引用）。
    ///
    /// 与 [`Outcome::value_or`] 分开：需要「看但不移交所有权」时用本方法，
    /// 避免为取一个字段而克隆整份报告。
    pub fn value_ref(&self) -> Option<&T> {
        match self {
            Outcome::Ok { value, .. } => Some(value),
            Outcome::Err { .. } => None,
        }
    }

    /// 取诊断码（成功时为 `None`）。
    pub fn code(&self) -> Option<DiagCode> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err { code, .. } => Some(*code),
        }
    }

    /// 取诊断明细（两种形态都给得出——调用方不必先判形态）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Outcome::Ok { diagnostics, .. } => diagnostics.as_slice(),
            Outcome::Err { diagnostics, .. } => diagnostics.as_slice(),
        }
    }

    /// 读屏可读的失败全貌（成功时给的是告警摘要）。
    pub fn screen_text(&self) -> String {
        match self {
            Outcome::Ok { value: _, diagnostics } => {
                if diagnostics.is_empty() {
                    "成功，无告警。".to_string()
                } else {
                    format!("成功，附带 {} 条告警。", diagnostics.len())
                }
            }
            Outcome::Err {
                code,
                message,
                hint,
                ..
            } => format!("失败 {}：{}；处置建议：{}", code.code(), message, hint),
        }
    }
}

/// 诊断聚合器。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 空聚合器。
    pub fn new() -> Self {
        DiagBag { items: Vec::new() }
    }

    /// 追加一条诊断（空 message/hint 会被补上占位——不留空建议）。
    pub fn push(&mut self, code: DiagCode, message: &str, hint: &str) {
        self.items.push(Diagnostic {
            code,
            message: if message.trim().is_empty() {
                "（未提供描述）".to_string()
            } else {
                message.to_string()
            },
            hint: if hint.trim().is_empty() {
                "（未提供处置建议）".to_string()
            } else {
                hint.to_string()
            },
        });
    }

    /// 批量追加。
    pub fn push_all(&mut self, ds: &[Diagnostic]) {
        self.items.extend_from_slice(ds);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取全部诊断。
    pub fn all(&self) -> Vec<Diagnostic> {
        self.items.clone()
    }

    /// 按码过滤。
    pub fn by_code(&self, code: DiagCode) -> Vec<Diagnostic> {
        self.items.iter().filter(|d| d.code == code).cloned().collect()
    }

    /// 读屏摘要（逐条念出——诊断不该只留在日志里）。
    pub fn screen_text(&self) -> String {
        if self.items.is_empty() {
            return "诊断袋为空。".to_string();
        }
        let mut out = format!("诊断袋共 {} 条：", self.items.len());
        for (i, d) in self.items.iter().enumerate() {
            if i > 0 {
                out.push('；');
            }
            out.push_str(&d.screen_line());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 二、域身份与十项资源范围映射（判据「十项映射」）
// ---------------------------------------------------------------------------

/// 域号段下界。
pub const DOMAIN_ITEM_LO: u32 = 3201;
/// 域号段上界。
pub const DOMAIN_ITEM_HI: u32 = 3400;
/// 域内条目总数。
pub const DOMAIN_ITEM_COUNT: u32 = 200;
/// 开工条自身条目号。
pub const DOMAIN_KICKOFF_ITEM: u32 = 3201;
/// 架构版本（六段签名冻结版本）。
pub const ARCH_VERSION: &str = "Q01-pipeline-v1";

/// Q 域官方十主题（主题注册表：让「新增资源能力归哪条」有唯一答案）。
///
/// 与 [`ResourceKind`] 的区别（易混处，故写明）：`ResourceKind` 是**资源本身
/// 的种类**（纹理/模型/音频…），进十项映射的左列；`QDomainTopic` 是**管线能力
/// 的主题**（加载/调度/解码对接/缓存/生命周期…），进主题注册表。两者正交：
/// 「字体」是一种资源（`ResourceKind`），它经由「缓存与预算」主题被管。
pub const Q_DOMAIN_TEN_TOPICS: [QDomainTopic; 10] = [
    QDomainTopic::Addressing,
    QDomainTopic::Request,
    QDomainTopic::Scheduling,
    QDomainTopic::State,
    QDomainTopic::Failure,
    QDomainTopic::DecodeBridge,
    QDomainTopic::Packing,
    QDomainTopic::Cache,
    QDomainTopic::Lifecycle,
    QDomainTopic::Consumption,
];

/// 管线能力主题。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QDomainTopic {
    /// 资源寻址。
    Addressing,
    /// 加载请求。
    Request,
    /// 并发调度。
    Scheduling,
    /// 加载状态。
    State,
    /// 失败与降级。
    Failure,
    /// 解码对接。
    DecodeBridge,
    /// 打包与索引。
    Packing,
    /// 缓存与预算。
    Cache,
    /// 生命周期。
    Lifecycle,
    /// 消费契约。
    Consumption,
}

impl QDomainTopic {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            QDomainTopic::Addressing => "资源寻址",
            QDomainTopic::Request => "加载请求",
            QDomainTopic::Scheduling => "并发调度",
            QDomainTopic::State => "加载状态",
            QDomainTopic::Failure => "失败与降级",
            QDomainTopic::DecodeBridge => "解码对接",
            QDomainTopic::Packing => "打包与索引",
            QDomainTopic::Cache => "缓存与预算",
            QDomainTopic::Lifecycle => "生命周期",
            QDomainTopic::Consumption => "消费契约",
        }
    }

    /// 英文标识。
    pub fn en(self) -> &'static str {
        match self {
            QDomainTopic::Addressing => "addressing",
            QDomainTopic::Request => "request",
            QDomainTopic::Scheduling => "scheduling",
            QDomainTopic::State => "state",
            QDomainTopic::Failure => "failure",
            QDomainTopic::DecodeBridge => "decode-bridge",
            QDomainTopic::Packing => "packing",
            QDomainTopic::Cache => "cache",
            QDomainTopic::Lifecycle => "lifecycle",
            QDomainTopic::Consumption => "consumption",
        }
    }

    /// 该主题占用的功能号区间（号段必须无缝且不重叠）。
    pub fn span(self) -> TopicSpan {
        let lo = DOMAIN_ITEM_LO + (self.ordinal() as u32) * 20;
        TopicSpan {
            lo,
            hi: lo + 19,
            item_count: 20,
        }
    }

    /// 主题序位（0..10，对应号段顺序）。
    pub fn ordinal(self) -> u8 {
        match self {
            QDomainTopic::Addressing => 0,
            QDomainTopic::Request => 1,
            QDomainTopic::Scheduling => 2,
            QDomainTopic::State => 3,
            QDomainTopic::Failure => 4,
            QDomainTopic::DecodeBridge => 5,
            QDomainTopic::Packing => 6,
            QDomainTopic::Cache => 7,
            QDomainTopic::Lifecycle => 8,
            QDomainTopic::Consumption => 9,
        }
    }
}

/// 主题号段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopicSpan {
    /// 号段下界（含）。
    pub lo: u32,
    /// 号段上界（含）。
    pub hi: u32,
    /// 条目数。
    pub item_count: u32,
}

impl TopicSpan {
    /// 该号段是否覆盖某条目号。
    pub fn covers(&self, item: u32) -> bool {
        item >= self.lo && item <= self.hi
    }

    /// 读屏可读单行。
    pub fn screen_line(&self, topic: QDomainTopic) -> String {
        format!(
            "主题「{}」占功能号 {}–{}（{} 条）",
            topic.zh(),
            self.lo,
            self.hi,
            self.item_count
        )
    }
}

/// 资源类型（十项映射左列；闭集——新增须走扩展点，不得改枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKind {
    /// 纹理。
    Texture,
    /// 模型。
    Model,
    /// 几何。
    Geometry,
    /// 音频。
    Audio,
    /// 字体。
    Font,
    /// 样式。
    Style,
    /// 场景。
    Scene,
    /// 着色器。
    Shader,
    /// 动画。
    Animation,
    /// 流媒体。
    Stream,
}

impl ResourceKind {
    /// 英文标识（同时是 URI 路径的类型段——寻址的机器可读形态）。
    pub fn en(self) -> &'static str {
        match self {
            ResourceKind::Texture => "texture",
            ResourceKind::Model => "model",
            ResourceKind::Geometry => "geometry",
            ResourceKind::Audio => "audio",
            ResourceKind::Font => "font",
            ResourceKind::Style => "style",
            ResourceKind::Scene => "scene",
            ResourceKind::Shader => "shader",
            ResourceKind::Animation => "animation",
            ResourceKind::Stream => "stream",
        }
    }

    /// 由英文标识反查（URI 类型段解析用；未登记即 `None` → 拒绝）。
    pub fn from_en(s: &str) -> Option<ResourceKind> {
        Some(match s {
            "texture" => ResourceKind::Texture,
            "model" => ResourceKind::Model,
            "geometry" => ResourceKind::Geometry,
            "audio" => ResourceKind::Audio,
            "font" => ResourceKind::Font,
            "style" => ResourceKind::Style,
            "scene" => ResourceKind::Scene,
            "shader" => ResourceKind::Shader,
            "animation" => ResourceKind::Animation,
            "stream" => ResourceKind::Stream,
            _ => return None,
        })
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ResourceKind::Texture => "纹理",
            ResourceKind::Model => "模型",
            ResourceKind::Geometry => "几何",
            ResourceKind::Audio => "音频",
            ResourceKind::Font => "字体",
            ResourceKind::Style => "样式",
            ResourceKind::Scene => "场景",
            ResourceKind::Shader => "着色器",
            ResourceKind::Animation => "动画",
            ResourceKind::Stream => "流媒体",
        }
    }

    /// 该类资源是否可流式交付（大资源走流送而非整体加载）。
    pub fn is_streamable(self) -> bool {
        matches!(
            self,
            ResourceKind::Texture
                | ResourceKind::Model
                | ResourceKind::Geometry
                | ResourceKind::Audio
                | ResourceKind::Animation
                | ResourceKind::Stream
        )
    }

    /// 生命周期粒度：常驻 / 按场景 / 按帧。决定缓存淘汰策略。
    pub fn lifetime(self) -> Lifetime {
        match self {
            ResourceKind::Font
            | ResourceKind::Style
            | ResourceKind::Scene
            | ResourceKind::Shader => Lifetime::Resident,
            ResourceKind::Texture
            | ResourceKind::Model
            | ResourceKind::Geometry
            | ResourceKind::Audio
            | ResourceKind::Animation => Lifetime::SceneScoped,
            ResourceKind::Stream => Lifetime::FrameScoped,
        }
    }
}

/// 生命周期粒度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    /// 常驻（永不淘汰）。
    Resident,
    /// 按场景（场景切换时可淘汰）。
    SceneScoped,
    /// 按帧（帧级淘汰是常态）。
    FrameScoped,
}

impl Lifetime {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Lifetime::Resident => "常驻",
            Lifetime::SceneScoped => "按场景",
            Lifetime::FrameScoped => "按帧",
        }
    }
}

/// 资源映射（十项映射的右列：调度侧关注点）。
#[derive(Clone, Copy, Debug)]
pub struct ResourceMapping {
    /// 资源类型。
    pub kind: ResourceKind,
    /// 中文名。
    pub label: &'static str,
    /// 谁负责解码（F 域的哪一域，或「无编解码——Q 域直接产出」）。
    pub decoded_by: &'static str,
    /// 生命周期粒度。
    pub lifetime: Lifetime,
    /// 是否可被流式交付。
    pub streamable: bool,
    /// 缓存键须包含的字段（多一项即可能漏命中，故显式声明）。
    pub cache_key_fields: &'static [&'static str],
    /// 解码失败时是否可降级到占位。
    pub fallback_allowed: bool,
    /// 一句话说明本项的调度特殊性。
    pub note: &'static str,
}

/// 十项映射表：把官方资源范围逐类映射到调度侧关注点。
///
/// 「十项映射」的价值在于把「这一类资源在管线里怎么活」写成数据而非口头
/// 约定——新增资源类型时必须在此登记，否则它就不受管（等于出现了第二个生命
/// 周期权威）。
pub const RESOURCE_MAPPINGS: [ResourceMapping; 10] = [
    ResourceMapping {
        kind: ResourceKind::Texture,
        label: "纹理",
        decoded_by: "F 域（F1001-F1200 图像编解码矩阵）",
        lifetime: Lifetime::SceneScoped,
        streamable: true,
        cache_key_fields: &["uri", "mipCount", "colorSpace", "srgb"],
        fallback_allowed: true,
        note: "纹理是流送与显存双重压力源，缓存键须含色彩空间——同图不同色彩空间不是同一纹理",
    },
    ResourceMapping {
        kind: ResourceKind::Model,
        label: "模型",
        decoded_by: "F 域（网格容器解码）",
        lifetime: Lifetime::SceneScoped,
        streamable: true,
        cache_key_fields: &["uri", "lodBias"],
        fallback_allowed: false,
        note: "模型加载失败不得降级为占位——占位模型进渲染管线会被误当真实资产，比缺失更难查",
    },
    ResourceMapping {
        kind: ResourceKind::Geometry,
        label: "几何",
        decoded_by: "I 域（vmesh 容器 F1602）",
        lifetime: Lifetime::SceneScoped,
        streamable: true,
        cache_key_fields: &["uri", "lodLevel", "quantBits"],
        fallback_allowed: false,
        note: "几何由 I 域格式层提供、Q 域只调度；缓存键须含 LOD 级与量化位宽，否则不同精度会互相顶替",
    },
    ResourceMapping {
        kind: ResourceKind::Audio,
        label: "音频",
        decoded_by: "F 域（音频编解码）",
        lifetime: Lifetime::SceneScoped,
        streamable: true,
        cache_key_fields: &["uri", "sampleRate", "channels"],
        fallback_allowed: true,
        note: "音频有实时性约束：解码超时比解码失败更糟，超时须走降级并上报而非阻塞",
    },
    ResourceMapping {
        kind: ResourceKind::Font,
        label: "字体",
        decoded_by: "F 域（字体解析 F0800 段）",
        lifetime: Lifetime::Resident,
        streamable: false,
        cache_key_fields: &["uri", "faceIndex", "axisRange"],
        fallback_allowed: false,
        note: "字体常驻且三层引用计数（族/字体/实例）；缺字回退由字体域负责，Q 域不得替换字形",
    },
    ResourceMapping {
        kind: ResourceKind::Style,
        label: "样式",
        decoded_by: "无编解码——Q 域直接产出（CSS 子集由 O 域消费）",
        lifetime: Lifetime::Resident,
        streamable: false,
        cache_key_fields: &["uri", "mediaQuery"],
        fallback_allowed: true,
        note: "样式体积极小而访问极频繁，常驻且永不淘汰——把它放进 LRU 只会制造抖动",
    },
    ResourceMapping {
        kind: ResourceKind::Scene,
        label: "场景",
        decoded_by: "无编解码——Q 域直接产出（VE-Scene 由 Z 域消费）",
        lifetime: Lifetime::Resident,
        streamable: false,
        cache_key_fields: &["uri", "sceneVersion"],
        fallback_allowed: false,
        note: "场景是依赖图的根：它加载失败等于整场景加载失败，不存在「降级加载场景」",
    },
    ResourceMapping {
        kind: ResourceKind::Shader,
        label: "着色器",
        decoded_by: "C 域（F0401-F0600 着色器系统编译）",
        lifetime: Lifetime::Resident,
        streamable: false,
        cache_key_fields: &["uri", "compileTarget", "variantHash"],
        fallback_allowed: false,
        note: "着色器编译昂贵，缓存键须含变体哈希；编译失败须回退默认着色器而非空材质",
    },
    ResourceMapping {
        kind: ResourceKind::Animation,
        label: "动画",
        decoded_by: "M 域（F2401-F2600 动画系统解析）",
        lifetime: Lifetime::SceneScoped,
        streamable: true,
        cache_key_fields: &["uri", "clipName"],
        fallback_allowed: true,
        note: "动画按剪辑名分粒度缓存：整段加载会让一个长动画的首次播放延迟不可接受",
    },
    ResourceMapping {
        kind: ResourceKind::Stream,
        label: "流媒体",
        decoded_by: "G 域（F1201-F1400 视频引擎解封装）",
        lifetime: Lifetime::FrameScoped,
        streamable: true,
        cache_key_fields: &["uri", "qualityLevel", "segmentIndex"],
        fallback_allowed: true,
        note: "流媒体是唯一 frame-scoped 类：帧级淘汰是常态而非异常，缓存策略与其他类不可共用",
    },
];

impl ResourceMapping {
    /// 读屏可读单行（无障碍：映射表要能念出四要素）。
    pub fn screen_line(&self) -> String {
        format!(
            "资源类型「{}」（{}）：解码归{}，生命周期{}，{}流式，缓存键含{}，失败{}降级。备注：{}",
            self.label,
            self.kind.en(),
            self.decoded_by,
            self.lifetime.zh(),
            if self.streamable {
                "可"
            } else {
                "不可"
            },
            self.cache_key_fields.join("+"),
            if self.fallback_allowed {
                "可"
            } else {
                "不"
            },
            self.note
        )
    }
}

/// 查资源映射；未登记即显性失败（「不受管 = 第二生命周期权威」是缺陷，不是例外）。
pub fn lookup_mapping(kind: ResourceKind) -> Outcome<ResourceMapping> {
    match RESOURCE_MAPPINGS.iter().find(|m| m.kind == kind) {
        Some(m) => Outcome::ok(*m),
        None => Outcome::err(
            DiagCode::ResourceTypeUnmapped,
            "资源类型不在十项映射内",
            "十项映射是资源类型的闭集：新增类型须在 RESOURCE_MAPPINGS 登记\
             （解码归属、生命周期粒度、缓存键字段、失败处置四要素齐备），否则它不受管",
        ),
    }
}

/// 由 URI 类型段反查映射（未登记段即 `Err`——不受管不可能悄悄发生）。
pub fn lookup_mapping_by_str(kind_seg: &str) -> Outcome<ResourceMapping> {
    match ResourceKind::from_en(kind_seg) {
        Some(k) => lookup_mapping(k),
        None => {
            if kind_seg.is_empty() {
                Outcome::err(
                    DiagCode::ResourceTypeUnmapped,
                    "资源 URI 的类型段为空",
                    "URI 须形如 ve-asset://<authority>/<kind>/...；类型段缺失则资源不受管",
                )
            } else {
                let kinds: Vec<&str> = RESOURCE_MAPPINGS.iter().map(|m| m.kind.en()).collect();
                Outcome::err(
                    DiagCode::ResourceTypeUnmapped,
                    &format!("资源类型段「{}」不在十项映射内", kind_seg),
                    &format!(
                        "十项映射是闭集，现有：{}。新增类型须在 RESOURCE_MAPPINGS 登记\
                         （解码归属、生命周期粒度、缓存键字段、失败处置四要素齐备）",
                        kinds.join("、")
                    ),
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、跨域边界表与能力裁决（能力有唯一属主）
// ---------------------------------------------------------------------------

/// 能力键（机检键）：一个能力有且只有一个属主域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedCapability {
    /// 机检键。
    pub key: &'static str,
    /// 中文描述。
    pub label: &'static str,
}

/// 跨域边界条目。
#[derive(Clone, Copy, Debug)]
pub struct DomainBoundary {
    /// 域标识。
    pub id: &'static str,
    /// 域中文名。
    pub name: &'static str,
    /// 号段下界。
    pub lo: u32,
    /// 号段上界。
    pub hi: u32,
    /// 锚点。
    pub anchor: &'static str,
    /// 负责的能力（机检键→唯一属主域）。
    pub owns: &'static [OwnedCapability],
    /// 禁做项（越界判据）。
    pub must_not: &'static [&'static str],
}

/// 跨域边界表：Q 域与相邻域的能力归属。
///
/// 为何必须有这张表（无它则边界形同虚设）：跨域争议最终只有两种问法——
/// 「这个能力归谁」和「谁被允许做这件事」。没有这张表时，两问都靠口口相传，
/// 于是出现「所有申请判成无主」的假象：看着像边界严，实则边界不存在。
///
/// `owns` 必须是 [`OwnedCapability`] 结构体（历史教训，勿回退）：把机检键与
/// 中文描述合成一个字符串（如 `owns: ["resource-load(资源加载)"]`）会让能力
/// 反查全失配——所有申请都判成「无主」，且这个失配不会报错，只会静默退化。
pub const DOMAIN_BOUNDARY_TABLE: [DomainBoundary; 5] = [
    DomainBoundary {
        id: "VE-Q",
        name: "Q 域 · 资源管线",
        lo: DOMAIN_ITEM_LO,
        hi: DOMAIN_ITEM_HI,
        anchor: "VE-F3201 Q 域开工与管线总架构（本条）",
        owns: &[
            OwnedCapability {
                key: "resource-addressing",
                label: "资源 URI 寻址与规范化",
            },
            OwnedCapability {
                key: "resource-load-pipeline",
                label: "六段加载流水编排",
            },
            OwnedCapability {
                key: "resource-scheduling",
                label: "并发调度与请求合并",
            },
            OwnedCapability {
                key: "resource-handle-lifecycle",
                label: "句柄生命周期与 GC",
            },
            OwnedCapability {
                key: "resource-type-registry",
                label: "资源类型注册表",
            },
            OwnedCapability {
                key: "resource-cache-budget",
                label: "缓存预算与淘汰",
            },
            OwnedCapability {
                key: "resource-pack-index",
                label: "资源包格式与索引",
            },
            OwnedCapability {
                key: "resource-failure-degrade",
                label: "失败分类、重试与降级",
            },
            // 仲裁位能力：写入方式受限、须走 ADR，但**有明确属主**。
            // 仲裁不是「无人负责」——把 property-write 之类登记在此，是为了让
            // 写路径有唯一裁决点，而不是让它变成三不管地带。
            OwnedCapability {
                key: "property-write",
                label: "属性写入仲裁（受限写，须走 ADR）",
            },
            OwnedCapability {
                key: "resource-diagnostics",
                label: "资源域诊断与错误码透传",
            },
        ],
        must_not: &[
            "不得实现格式解码逻辑（属 F 域编解码专家，Q 域只调度）",
            "不得自行发起对消费域的主动推送（消费侧由各域订阅 Q 域事件）",
            "不得在 URI 中承载用户内容（内容走数据域）",
            "不得绕过 F 域直接把字节当解码产物交付（解码单源红线）",
        ],
    },
    DomainBoundary {
        id: "VE-F",
        name: "F 域 · 编解码格式专家",
        lo: 1001,
        hi: 1200,
        anchor: "VE 册 F1001-F1200 编解码格式专家",
        owns: &[
            OwnedCapability {
                key: "format-decode",
                label: "字节流→单源真相的解码",
            },
            OwnedCapability {
                key: "format-parse-safety",
                label: "解析器深度安全（恶意输入防护）",
            },
            OwnedCapability {
                key: "decoder-error-codes",
                label: "格式专属错误码定义",
            },
        ],
        must_not: &[
            "不得持有 Q 域调度状态（引用计数、加载队列——否则形成域环）",
            "不得反向调用 Q 域发起加载",
            "不得改写自身错误码后再上抛（透传不失真红线）",
        ],
    },
    DomainBoundary {
        id: "VE-O",
        name: "O 域 · CSS/HTML 表面",
        lo: 2801,
        hi: 3000,
        anchor: "VE-F2801 O 域开工与样式引擎架构",
        owns: &[
            OwnedCapability {
                key: "style-resolution",
                label: "样式解析与层叠",
            },
            OwnedCapability {
                key: "layout-box",
                label: "盒模型与布局",
            },
        ],
        must_not: &[
            "不得自行读盘取纹理/字体（必须经 Q 域 acquire）",
            "不得缓存资源句柄的副本（缓存权归 Q 域）",
        ],
    },
    DomainBoundary {
        id: "VE-N",
        name: "N 域 · UI 框架内核",
        lo: 2601,
        hi: 2800,
        anchor: "VE-F2601 N 域开工与 UI 内核总架构",
        owns: &[
            OwnedCapability {
                key: "control-tree-structure",
                label: "控件树结构与三不变量",
            },
            OwnedCapability {
                key: "measure-layout",
                label: "测量与排列两阶段布局",
            },
            OwnedCapability {
                key: "focus-chain",
                label: "焦点链与 Tab 顺序",
            },
        ],
        must_not: &[
            "不得自行读盘取资源（收敛红线：一切经 Q 域管线）",
            "不得绕过 Q 域直接调 F 域解码",
        ],
    },
    DomainBoundary {
        id: "VE-I",
        name: "I 域 · 3D 管线",
        lo: 1601,
        hi: 1800,
        anchor: "VE-F1601 I 域 3D 管线架构",
        owns: &[
            OwnedCapability {
                key: "mesh-pipeline",
                label: "网格处理与顶点管线",
            },
            OwnedCapability {
                key: "gpu-dispatch",
                label: "GPU 分派与着色",
            },
        ],
        must_not: &["不得自行加载模型/纹理（须经 Q 域 acquire，句柄只读不解引用）"],
    },
];

/// 能力裁决结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityVerdict {
    /// 本域自有，放行。
    Owned {
        /// 属主域。
        owner: &'static str,
        /// 属主标签。
        label: &'static str,
    },
    /// 能力属他域，应改向属主申请。
    Redirected {
        /// 属主域。
        owner: &'static str,
        /// 属主标签。
        label: &'static str,
    },
}

impl CapabilityVerdict {
    /// 状态码（`owned` / `redirected`）。
    pub fn status(&self) -> &'static str {
        match self {
            CapabilityVerdict::Owned { .. } => "owned",
            CapabilityVerdict::Redirected { .. } => "redirected",
        }
    }

    /// 处置建议（人话，可直接贴进缺陷单）。
    pub fn remediation(&self, requester: &str) -> String {
        match self {
            CapabilityVerdict::Owned { owner, label } => {
                format!("本域（{}）自有能力「{}」，放行", owner, label)
            }
            CapabilityVerdict::Redirected { owner, label } => format!(
                "该能力属 {}（{}）；{} 应改为向 {} 申请，不得自行实现——两份实现意味着两份生命周期",
                owner, label, requester, owner
            ),
        }
    }

    /// 读屏可读单行。
    pub fn screen_line(&self, requester: &str) -> String {
        format!("能力裁决：{}；{}", self.status(), self.remediation(requester))
    }
}

/// 能力反查：由机检键求唯一属主域。
///
/// 返回 `None` 而非"查不到"含糊表达：归属不明是**一种判定结果**（需立案），
/// 不是「不在范围内」。把二者混同会让「无人认领」看起来像「不在范围内」。
pub fn resolve_capability_owner(capability_key: &str) -> Option<(/* owner */ &'static str, /* label */ &'static str)> {
    for d in DOMAIN_BOUNDARY_TABLE.iter() {
        for c in d.owns.iter() {
            if c.key == capability_key {
                return Some((d.id, c.label));
            }
        }
    }
    None
}

/// 能力裁决：能力有唯一属主；他域申请即导流，无主即立案。
pub fn adjudicate_capability(capability_key: &str, requester_domain: &str) -> Outcome<CapabilityVerdict> {
    match resolve_capability_owner(capability_key) {
        None => Outcome::err(
            DiagCode::ValueInvalid,
            &format!("能力「{}」在跨域边界表中无属主（申请方 {}）", capability_key, requester_domain),
            "这与「无人负责」不同：无人负责是治理缺口，须补登记。请在 DOMAIN_BOUNDARY_TABLE 的\
             对应域 owns 中登记该能力（owns 必须是 {key,label} 结构体，合成字符串会导致反查全失配）",
        ),
        Some((owner, label)) => {
            if owner == requester_domain {
                Outcome::ok(CapabilityVerdict::Owned { owner, label })
            } else {
                Outcome::ok(CapabilityVerdict::Redirected { owner, label })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、上游协议接收台账（跨域约定不烂在口头）
// ---------------------------------------------------------------------------

/// 上游协议台账条目。
#[derive(Clone, Copy, Debug)]
pub struct UpstreamProtocolEntry {
    /// 协议机检键。
    pub key: &'static str,
    /// 发起方域（`"*"` 表示任意消费域）。
    pub from: &'static str,
    /// 协议签名（冻结 v1，改签名须走 ADR）。
    pub signature: &'static str,
    /// Q 域侧的接收函数名。
    pub handler: &'static str,
    /// 无障碍替述：本协议出错时，无障碍层如何向用户播报。
    pub alt_text: &'static str,
}

/// 上游协议接收台账：Q 域从外部收到的一切请求面，均须登记可查。
///
/// 登记项即断言：台账里声明的每条协议，其接收函数都必须真实存在且被登记
/// （见 [`audit_upstream_ledger`]）。台账若与实现脱节，它就从「守约记录」
/// 退化为「一份没人看的文档」——比没有台账更糟，因为它看起来在守约。
pub const UPSTREAM_LEDGER: [UpstreamProtocolEntry; 4] = [
    UpstreamProtocolEntry {
        key: "consumer-acquire",
        from: "*",
        signature: "acquireFromPipeline(canonicalUri: string, consumerDomain: string) -> ResourceHandle",
        handler: "acquireFromPipeline",
        alt_text: "资源加载失败时向屏幕阅读器播报「素材未能载入，已用替代素材」，不暴露本地路径",
    },
    UpstreamProtocolEntry {
        key: "consumer-release",
        from: "*",
        signature: "releaseHandle(handle: ResourceHandle) -> void",
        handler: "releaseHandle",
        alt_text: "释放资源不产生播报（非错误路径），但释放失败会累积内存，须记入诊断而非静默",
    },
    UpstreamProtocolEntry {
        key: "decoder-delegate",
        from: "VE-Q",
        signature: "decode(bytes, kind) -> DecodeFailureEnvelope | DecodeProduct",
        handler: "delegateDecode",
        alt_text: "素材格式无法解析时播报「该素材格式不支持」，并报出格式名而非文件路径",
    },
    UpstreamProtocolEntry {
        key: "domain-warmup",
        from: "engine-boot",
        signature: "warmPipeline(entries) -> WarmupReport",
        handler: "warmPipeline",
        alt_text: "预载失败不面向用户播报（启动期无用户交互），但必须记入启动诊断",
    },
];

/// 台账审计：台账声明的每条协议，其 handler 必须已登记于 [`HANDLER_REGISTRY`]。
///
/// ESM/Rust 都无法在编译期枚举导出表，故以显式注册表登记各 handler，新增
/// 协议时必须在此登记——**漏登记 = 审计失败**（这正是我们要的失败方向）。
pub const HANDLER_REGISTRY: [&str; 4] = [
    "acquireFromPipeline",
    "releaseHandle",
    "delegateDecode",
    "warmPipeline",
];

/// 台账审计：检出「台账声明了但未登记实现」的协议。
pub fn audit_upstream_ledger() -> Outcome<Vec<DiagCode>> {
    let mut bag = DiagBag::new();
    for e in UPSTREAM_LEDGER.iter() {
        if !HANDLER_REGISTRY.contains(&e.handler) {
            bag.push(
                DiagCode::StageContractDiverged,
                &format!("协议「{}」声明的 handler「{}」未登记实现", e.key, e.handler),
                "台账与实现脱节会让「守约记录」退化为没人看的文档；请在 HANDLER_REGISTRY 登记该 handler",
            );
        }
        if e.alt_text.trim().is_empty() {
            bag.push(
                DiagCode::StageContractDiverged,
                &format!("协议「{}」缺无障碍替述", e.key),
                "每条协议出错时都要有面向用户的播报口径；缺替述等于让无障碍层在错误时失语",
            );
        }
    }
    if !bag.is_empty() {
        let codes: Vec<DiagCode> = bag.all().iter().map(|d| d.code).collect();
        return Outcome::Err {
            code: DiagCode::StageContractDiverged,
            message: format!("上游台账审计失败（{} 处）", codes.len()),
            hint: "补齐 handler 登记与无障碍替述".to_string(),
            diagnostics: bag.all(),
        };
    }
    Outcome::ok_with(Vec::new(), bag.all())
}

// ---------------------------------------------------------------------------
// 五、无障碍与隐私声明
// ---------------------------------------------------------------------------

/// 面向用户的错误文案原则。
pub const A11Y_ERROR_PRINCIPLE: &str =
    "错误文案须包含「受影响的对象」与「可采取的动作」；不暴露本地路径、内部错误码、URI 原文";

/// 隐私声明：Q 域无隐私面。
pub const PRIVACY_NOTE: &str =
    "Q 域无隐私面——URI 只承载资源标识，用户内容一律走数据域（归属红线的隐私侧）";

/// 失败必可感知。
pub const FAILURE_VISIBILITY: &str =
    "任何终态失败必须同时产出诊断与可播报文案，禁止无声失败（无障碍的基本要求）";

/// 资源 URI 各段的长度上限（防御性上限：超长 URI 必是误用而非真实资源）。
pub const URI_SEGMENT_MAX: usize = 512;

/// URI 总长上限（四段合计）。
pub const URI_MAX_LEN: usize = URI_SEGMENT_MAX * 4;

/// 人话内容信号（出现在 URI 的**非query 段**即违规）。
///
/// 列的是**形态特征**而非具体词表——词表永远列不全，故形态判定为主、query
/// 键判定为辅（见 [`CONTENT_QUERY_KEYS`]）。这里刻意不含任何 `?key=` 形式的
/// 条目：query 段的判定交给键名解析，不靠子串匹配。
///
/// 历史缺陷（已修，勿回退）：形态表曾含 `"?q="` 与 `"&title="` 两条 query
/// 形态条目，但漏了单问号起首的 `"?title="`；由于判定用的是子串包含，攻击者
/// 只要换一个 query 键即可绕过。教训：**子串词表挡不住变体**，故 query 段
/// 改为按键名集合判定。
pub const USER_CONTENT_MARKERS: [&str; 7] = [" ", "\t", "\n", "\r", "\u{feff}", "<", ">"];

/// 内容型 query 键反例表：这些键名一旦出现在 query 中即视为「把用户内容塞进
/// 了 URI」，无论其值是编码还是明文。
///
/// 判定按**键名**而非整串，故 `?title=`、`&title=`、`?TItle=` 全部命中，
/// 不存在「换个前缀就绕过」的可能。这是相对旧实现的实质修正。
///
/// 保留白名单：内容散落在 URI 里已不可回收，故宁可误伤——合法资源请求用
/// `?v=<哈希>` 这类**标识型**参数（见 [`SAFE_QUERY_KEYS`]），不该用
/// `?title=` 这类**内容型**参数。
pub const CONTENT_QUERY_KEYS: [&str; 21] = [
    "q", "title", "text", "content", "data", "name", "label", "caption", "desc",
    "description", "body", "message", "msg", "comment", "note", "author", "user",
    "username", "prompt", "search", "query",
];

/// 合法（标识型）query 键：版本号、内容哈希、切片区间、画质档等。
///
/// 这些键的值是**标识符**而非内容，进 URI 不违反归属红线。
pub const SAFE_QUERY_KEYS: [&str; 21] = [
    "v", "hash", "rev", "rev2", "ver", "version", "lod", "mip", "slice", "offset", "length",
    "range", "quality", "tier", "level", "w", "h", "dpr", "frame", "id", "uid",
];

/// scheme 权限档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemePermission {
    /// 放行。
    Allowed,
    /// 默认拒绝（须显式授权）。
    DeniedByDefault,
}

/// scheme 权限矩阵（F3213 细化前的地基，锚点「权限红线」的最小实现）。
///
/// 为何放在 f3201 而非等到 f3213：Q 域是**所有**资源进入系统的第一道闸，
/// 消费域与 F 域拿到的每一个 URI 都先过这里。若 scheme 管控等到 f3213 才开始，
/// 在 f3201~f3212 这十余项功能开发期间，远程 scheme 会被无差别放行——闸门必须
/// 先立，细则后补。
///
/// 四类 scheme：
/// - `ve-asset`：内部资源包（Q 域自管线打包产物）；
/// - `embedded`：内嵌资源（编入可执行文件的只读资源）；
/// - `local`：本地文件路径（用户显式授权的目录内）；
/// - `remote`：远程资源（**默认不授权**，须显式开启）。
///
/// `remote` 的处置刻意是「默认拒绝」而非「默认放行」：远程资源可被中间人
/// 替换内容，且其可用性不受本机控制。确需远程资源时由受信域显式开启
/// （[`grant_remote_scheme`]），这样「谁开了远程」这件事本身可审计。
pub const SCHEME_PERMISSION: [(&str, SchemePermission); 4] = [
    ("ve-asset", SchemePermission::Allowed),
    ("embedded", SchemePermission::Allowed),
    ("local", SchemePermission::Allowed),
    ("remote", SchemePermission::DeniedByDefault),
];

/// 远程 scheme 授权表（默认空——远程须显式开启才可审计）。
#[derive(Clone, Debug, Default)]
pub struct RemoteGrantTable {
    grants: Vec<String>,
}

impl RemoteGrantTable {
    /// 空授权表（默认拒绝，不是默认放行）。
    pub fn new() -> Self {
        RemoteGrantTable { grants: Vec::new() }
    }

    /// 显式授权某域使用 `remote` scheme（重复授权幂等）。
    pub fn grant(&mut self, consumer_domain: &str) -> Outcome<Vec<String>> {
        if consumer_domain.trim().is_empty() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                "远程 scheme 授权被拒：域标识为空",
                "授权须指名道姓——匿名授权等于没有授权，「谁开了远程」将无法审计",
            );
        }
        if !self.grants.iter().any(|g| g == consumer_domain) {
            self.grants.push(consumer_domain.to_string());
        }
        let mut sorted = self.grants.clone();
        sorted.sort();
        Outcome::ok(sorted)
    }

    /// 撤销某域的远程授权。
    pub fn revoke(&mut self, consumer_domain: &str) -> Outcome<Vec<String>> {
        self.grants.retain(|g| g != consumer_domain);
        let mut sorted = self.grants.clone();
        sorted.sort();
        Outcome::ok(sorted)
    }

    /// 已授权域清单（确定性排序——回归可对拍）。
    pub fn list(&self) -> Vec<String> {
        let mut sorted = self.grants.clone();
        sorted.sort();
        sorted
    }

    /// 是否已授权。
    pub fn is_granted(&self, consumer_domain: &str) -> bool {
        self.grants.iter().any(|g| g == consumer_domain)
    }
}

/// scheme 权限判定（封闭默认：未登记 scheme 一律拒绝）。
pub fn check_scheme_permission(
    scheme: &str,
    consumer_domain: Option<&str>,
    grants: &RemoteGrantTable,
) -> Outcome<()> {
    let Some((_, perm)) = SCHEME_PERMISSION.iter().find(|(s, _)| *s == scheme) else {
        let known: Vec<&str> = SCHEME_PERMISSION.iter().map(|(s, _)| *s).collect();
        return Outcome::err(
            DiagCode::UriMalformed,
            &format!("scheme「{}」不在权限矩阵内", scheme),
            &format!(
                "封闭默认：未登记的 scheme 一律拒绝（理由是它可能是拼错的合法 scheme，\
                 也可能是攻击者自造的后门）。现有 scheme：{}",
                known.join("、")
            ),
        );
    };
    match perm {
        SchemePermission::Allowed => Outcome::ok(()),
        SchemePermission::DeniedByDefault => {
            // remote：须指名域显式授权。启动期（consumer_domain = None）无人可
            // 审计，故一律拒绝——「启动期无消费域」不等于「启动期可放行远程」。
            match consumer_domain {
                None => Outcome::err(
                    DiagCode::SourceUntrusted,
                    "启动期预载不得使用 remote scheme",
                    "启动期没有可审计的请求方，远程资源可被中间人替换；\
                     请改用 ve-asset/embedded/local，或由受信域在运行期显式授权后请求",
                ),
                Some(d) if grants.is_granted(d) => Outcome::ok(()),
                Some(d) => Outcome::err(
                    DiagCode::SourceUntrusted,
                    &format!("域 {} 未获 remote scheme 授权", d),
                    "远程资源默认拒绝：内容可被中间人替换，可用性不受本机控制。\
                     确需远程时请由受信域显式开启（grant_remote_scheme），这样「谁开了远程」可审计",
                ),
            }
        }
    }
}

/// 归一化结果：分段 + 越界标志。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedPath {
    /// 归一化后的分段。
    pub segments: Vec<String>,
    /// 是否曾跳出 authority 根（吸收态：置位后不再清除）。
    pub escaped_root: bool,
}

/// 路径归一化：折叠重复斜杠、解析 `.` 与 `..`。
///
/// 返回分段结果而非单一字符串，是为了让**越界 `..` 可被上层检出**——若把越界
/// `..` 折叠成绝对路径后返回，`../../etc/passwd` 会被归一成 `/etc/passwd` 静默
/// 放行（读到了与请求完全不同的资源却无任何报错）。故越界一律保留为标志位，
/// 由 [`parse_resource_uri`] 的红线闸拒绝。
pub fn normalize_path(p: &str) -> NormalizedPath {
    // 先切掉 query（归一化只管 path 段）。
    let before_query = match p.find('?') {
        Some(i) => &p[..i],
        None => p,
    };
    let mut out: Vec<String> = Vec::new();
    // 越界标志一旦置位就**不再清除**：历史缺陷是 `..` 的越界形态被压进 out 后，
    // 后续的 `..` 会把它 pop 掉——`/../../etc/passwd` 前两个 `..` 互相抵消，
    // 最终 segments 变成干净的 ["etc","passwd"]，穿越检测形同虚设。
    // 正确语义：一旦跳出根，后续所有 `..` 都无法再回到根内，故越界是吸收态。
    let mut escaped_root = false;
    for seg in before_query.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if out.is_empty() {
                escaped_root = true;
                continue; // 不入 out——越界事实由标志位承载，不参与后续折叠
            }
            out.pop();
            continue;
        }
        out.push(seg.to_string());
    }
    NormalizedPath {
        segments: out,
        escaped_root,
    }
}

/// 百分号解码（仅 `%XX` 形态；非十六进制原样保留）。
///
/// 用途单一：把 query **键名**还原后再判定，避免 `%74itle`（`title` 的编码
/// 形式）绕过内容键判定。
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8 as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// 十六进制字符值（`0-9a-fA-F`）。
fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a') as u32 + 10),
        b'A'..=b'F' => Some((b - b'A') as u32 + 10),
        _ => None,
    }
}

/// 解析后的资源 URI。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceUri {
    /// scheme（小写）。
    pub scheme: String,
    /// authority（小写）。
    pub authority: String,
    /// 归一化路径（以 `/` 开头）。
    pub path: String,
    /// 归一化后的唯一字符串形式（缓存键与去重的依据）。
    pub canonical: String,
}

impl ResourceUri {
    /// 路径首段（资源类型段）。
    pub fn kind_segment(&self) -> &str {
        self.path
            .split('/')
            .find(|s| !s.is_empty())
            .unwrap_or("")
    }

    /// 读屏可读单行（无障碍：URI 要能念，且不含路径细节以免泄露）。
    pub fn screen_line(&self) -> String {
        format!(
            "资源 {} 类型段 {}，来源 {}（路径细节不播报）",
            self.kind_segment(),
            self.kind_segment(),
            self.scheme
        )
    }
}

/// 资源 URI 解析：四道闸（长度/内容形态/结构 + query 键/权限/路径穿越）。
///
/// 顺序刻意如此：先拦长度与内容（最便宜且能挡掉绝大多数误用），再解结构、
/// 判 query 键，最后判权限与穿越——让恶意输入在越早的闸被拦下。
pub fn parse_resource_uri(
    raw: &str,
    consumer_domain: Option<&str>,
    grants: &RemoteGrantTable,
) -> Outcome<ResourceUri> {
    // 闸① 长度：超长 URI 必是误把内容塞进了 URI。
    if raw.is_empty() || raw.len() > URI_MAX_LEN {
        return Outcome::err(
            DiagCode::UriMalformed,
            &format!(
                "资源 URI 长度为 {}，超出合法范围 [1, {}]",
                raw.len(),
                URI_MAX_LEN
            ),
            "URI 过长通常是误把内容塞进了 URI（违反归属红线）；请把内容移入数据域，用独立 ID 引用",
        );
    }
    // 闸② 内容形态信号（空白/尖括号/BOM）。
    for marker in USER_CONTENT_MARKERS.iter() {
        if raw.contains(marker) {
            return Outcome::err(
                DiagCode::UriContainsUserContent,
                &format!(
                    "资源 URI 含疑似用户内容信号 {:?}：{}",
                    marker,
                    preview(raw)
                ),
                "归属红线：URI 只表达「资源在哪」，不表达「资源是什么」。\
                 用户产生的内容（标题、文本、聊天）必须走数据域并用独立 ID 引用，\
                 因为 URI 会进日志与遥测——内容一旦写进去就无法回收",
            );
        }
    }
    // 结构：scheme://authority/path。
    let sep = match raw.find("://") {
        Some(i) if i > 0 => i,
        _ => {
            return Outcome::err(
                DiagCode::UriMalformed,
                &format!("资源 URI 缺 scheme 分隔符：{}", preview(raw)),
                "URI 须形如 scheme://authority/path（如 ve-asset://app/texture/base.png）",
            )
        }
    };
    let scheme = raw[..sep].to_lowercase();
    let rest = &raw[sep + 3..];
    let (authority_raw, raw_path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let authority = authority_raw.to_lowercase();
    if authority.is_empty() {
        return Outcome::err(
            DiagCode::UriMalformed,
            &format!("资源 URI 缺 authority：{}", preview(raw)),
            "URI 须形如 scheme://authority/path；authority 标资源来源（app / pkg / host）",
        );
    }

    // 闸③ query 段按**键名**判定内容泄漏。
    // 只对 query 段做此判定——path 段里的 `content=` 是合法的文件名形态。
    if let Some(qmark) = raw_path.find('?') {
        let query = &raw_path[qmark + 1..];
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let eq = pair.find('=');
            let raw_key = match eq {
                Some(i) => &pair[..i],
                None => pair,
            };
            // 键名去百分号编码，避免 `%74itle=x` 绕过；再小写，避开大小写变体。
            let key = percent_decode(raw_key).to_lowercase();
            if CONTENT_QUERY_KEYS.contains(&key.as_str()) {
                return Outcome::err(
                    DiagCode::UriContainsUserContent,
                    &format!(
                        "资源 URI 的 query 含内容型键「{}」：{}",
                        key,
                        preview(raw)
                    ),
                    "归属红线：query 参数只承载标识（版本、哈希、切片区间），不承载内容。\
                     合法键见 SAFE_QUERY_KEYS（v/hash/lod/slice…）。若确需按标题检索，\
                     请走数据域查询接口拿资源 ID，再用该 ID 拼 ve-asset URI",
                );
            }
            if !SAFE_QUERY_KEYS.contains(&key.as_str()) {
                return Outcome::err(
                    DiagCode::UriMalformed,
                    &format!("资源 URI 的 query 含未登记键「{}」", key),
                    "query 键必须在 SAFE_QUERY_KEYS（标识型）内登记后才可使用；\
                     内容型键被 CONTENT_QUERY_KEYS 明确拒绝。这样每个 query 键都有明确归属，\
                     不会在无人察觉的情况下逐渐长成内容通道",
                );
            }
        }
    }

    // 闸④ scheme 权限矩阵（未登记/未授权一律拒绝——封闭默认）。
    // 显式传播而非 `?`：`Outcome` 未实现 `Try`（它是本域的诊断载体而非
    // 异常通道——用 `?` 会把「失败即异常」的语义偷渡进契约层）。
    if let Outcome::Err {
        code,
        message,
        hint,
        diagnostics,
    } = check_scheme_permission(&scheme, consumer_domain, grants)
    {
        return Outcome::Err {
            code,
            message,
            hint,
            diagnostics,
        };
    }

    // 路径穿越：`normalize_path` 只做折叠，越界事实由 `escaped_root` 承载
    // （它不做授权判断）；授权判断在这里做——两者分层，避免归一化函数兼职
    // 安全决策。
    let normalized = normalize_path(raw_path);
    if normalized.escaped_root || normalized.segments.iter().any(|s| s == "..") {
        return Outcome::err(
            DiagCode::UriMalformed,
            &format!("资源 URI 试图跳出资源根：{}", preview(raw)),
            "路径穿越红线：`..` 不得越过 authority 根。归一化已把 `a/../b` 折叠成 `b`\
             （合法），但越界形态（开头即 `..` 或折叠后仍有残留 `..`）必须拒绝而非静默截断——\
             静默截断会让 `pkg/../../secret` 被当成 `pkg/secret` 加载，读到错误资源且无任何报错",
        );
    }
    let path = format!("/{}", normalized.segments.join("/"));
    Outcome::ok(ResourceUri {
        canonical: format!("{}://{}{}", scheme, authority, path),
        scheme,
        authority,
        path,
    })
}

/// URI 预览（截断到 80 字符——错误信息里不回显完整 URI，避免日志泄露）。
fn preview(raw: &str) -> String {
    if raw.chars().count() <= 80 {
        raw.to_string()
    } else {
        let head: String = raw.chars().take(80).collect();
        format!("{}…", head)
    }
}

/// 由 authority 与路径拼规范 URI（避免各域各拼一套，导致缓存键分裂）。
///
/// scheme 恒取 `ve-asset` 而非资源类型：资源类型（纹理/网格/音频…）是**寻址
/// 结果**，不是 URI 的分段维度。早期用 `texture://` 拼 scheme，等于把「十项
/// 映射」的资源类型混进 URI 空间——后果是每新增一类资源就等于新增一批
/// scheme，而 scheme 是安全边界，新增边界必须走 [`SCHEME_PERMISSION`] 登记；
/// 资源类型不该有security 含义（`texture://` 与 `audio://` 的权限并无差别）。
pub fn build_uri(
    authority: &str,
    path: &str,
    grants: &RemoteGrantTable,
) -> Outcome<ResourceUri> {
    let p = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    parse_resource_uri(&format!("ve-asset://{}{}", authority, p), None, grants)
}

// ---------------------------------------------------------------------------
// 六、六段流水线签名冻结 v1（判据「六段签名」）
// ---------------------------------------------------------------------------

/// 六段阶段名（闭集；顺序即语义，不可调换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StageId {
    /// 寻址。
    Address,
    /// 请求。
    Request,
    /// 调度。
    Schedule,
    /// 加载。
    Load,
    /// 校验。
    Verify,
    /// 交付句柄。
    Deliver,
}

impl StageId {
    /// 英文标识。
    pub fn en(self) -> &'static str {
        match self {
            StageId::Address => "address",
            StageId::Request => "request",
            StageId::Schedule => "schedule",
            StageId::Load => "load",
            StageId::Verify => "verify",
            StageId::Deliver => "deliver",
        }
    }

    /// 中文名（诊断文案与文档引用同一事实源）。
    pub fn zh(self) -> &'static str {
        match self {
            StageId::Address => "寻址",
            StageId::Request => "请求",
            StageId::Schedule => "调度",
            StageId::Load => "加载",
            StageId::Verify => "校验",
            StageId::Deliver => "交付句柄",
        }
    }

    /// 由英文标识反查（未注册即 `None` → [`lookup_stage`] 报错）。
    pub fn from_en(s: &str) -> Option<StageId> {
        Some(match s {
            "address" => StageId::Address,
            "request" => StageId::Request,
            "schedule" => StageId::Schedule,
            "load" => StageId::Load,
            "verify" => StageId::Verify,
            "deliver" => StageId::Deliver,
            _ => return None,
        })
    }
}

/// 六段的规范次序（冻结 v1 的唯一事实源）。
pub const STAGE_ORDER: [StageId; 6] = [
    StageId::Address,
    StageId::Request,
    StageId::Schedule,
    StageId::Load,
    StageId::Verify,
    StageId::Deliver,
];

/// 一个阶段的冻结签名规格（v1）。
///
/// 「签名冻结」的含义：每段的入参、出参、失败语义一经发布即不可改。改签名 =
/// 破坏性变更，必须走 ADR（F1598变更纪律）。故本条把签名写进数据，让「签名
/// 是否被改」成为可机检的事，而不是靠人读 diff。
#[derive(Clone, Copy, Debug)]
pub struct StageSpec {
    /// 阶段标识。
    pub id: StageId,
    /// 序位（0..6）。
    pub ordinal: u8,
    /// 入参名（按序）。
    pub inputs: &'static [&'static str],
    /// 出参名（按序）。
    pub outputs: &'static [&'static str],
    /// 本段允许的失败码（不在表内的失败码即越权产出，属契约分歧）。
    pub allowed_failures: &'static [DiagCode],
    /// 摊销复杂度声明。
    pub amortized: &'static str,
    /// 一句话说明本段存在的排他性理由。
    pub rationale: &'static str,
}

/// 六段签名规格表（冻结 v1）。
pub const STAGE_SPECS: [StageSpec; 6] = [
    StageSpec {
        id: StageId::Address,
        ordinal: 0,
        inputs: &["rawUri"],
        outputs: &["canonicalUri", "resourceKind"],
        allowed_failures: &[
            DiagCode::UriMalformed,
            DiagCode::UriContainsUserContent,
            DiagCode::ResourceTypeUnmapped,
            DiagCode::IoNotFound,
        ],
        amortized: "O(1)",
        rationale: "先归一 URI 才能判定「是不是同一资源」；不先归一则去重失效，同一资源被加载多次",
    },
    StageSpec {
        id: StageId::Request,
        ordinal: 1,
        inputs: &["canonicalUri", "resourceKind", "priority"],
        outputs: &["requestId", "cacheKey"],
        allowed_failures: &[
            DiagCode::ValueInvalid,
            DiagCode::DependencyCycle,
            DiagCode::BudgetExceeded,
        ],
        amortized: "O(1)",
        rationale: "缓存键必须在此一次性定死；放到后面各段各自算键会出现两套键空间，命中率恒低",
    },
    StageSpec {
        id: StageId::Schedule,
        ordinal: 2,
        inputs: &["requestId", "cacheKey", "concurrentLimit"],
        outputs: &["slot", "coalesced"],
        allowed_failures: &[
            DiagCode::BudgetExceeded,
            DiagCode::DependencyCycle,
            DiagCode::Cancelled,
        ],
        amortized: "O(1)",
        rationale: "并发合并（同一缓存键的并发请求合并为一个 IO）必须在此做，否则缓存击穿风暴",
    },
    StageSpec {
        id: StageId::Load,
        ordinal: 3,
        inputs: &["slot", "canonicalUri"],
        outputs: &["rawBytes", "decoderId"],
        allowed_failures: &[
            DiagCode::IoTimeout,
            DiagCode::IoNotFound,
            DiagCode::IoFailed,
            DiagCode::SourceUntrusted,
            DiagCode::Cancelled,
            DiagCode::ValueInvalid,
        ],
        amortized: "O(1)",
        rationale: "本段只管搬运字节，不解释字节；解释归 F 域解码器（单向委托）",
    },
    StageSpec {
        id: StageId::Verify,
        ordinal: 4,
        inputs: &["rawBytes", "expectedHash"],
        outputs: &["verifiedBytes"],
        allowed_failures: &[
            DiagCode::HashMismatch,
            DiagCode::SourceUntrusted,
            DiagCode::DecodeFailed,
            DiagCode::StageContractDiverged,
        ],
        amortized: "O(1)",
        rationale: "不校验就交付 = 让损坏资源进管线；半个网格比明确失败危害大得多",
    },
    StageSpec {
        id: StageId::Deliver,
        ordinal: 5,
        inputs: &["verifiedBytes", "decoderId", "cacheKey"],
        outputs: &["handle"],
        allowed_failures: &[
            DiagCode::ErrorCodeMutated,
            DiagCode::BudgetExceeded,
            DiagCode::Cancelled,
        ],
        amortized: "O(1)",
        rationale: "句柄是前五段的全部产出，也是消费方唯一入口；交付之后的生命周期归 Q 域",
    },
];

impl StageSpec {
    /// 读屏可读单行（无障碍：契约表要能念出六段全链）。
    pub fn screen_line(&self) -> String {
        format!(
            "第 {} 段「{}」：吃{}，吐{}；允许失败 {}；复杂度 {}；理由：{}",
            self.ordinal,
            self.label(),
            self.inputs.join("+"),
            self.outputs.join("+"),
            self.allowed_failures
                .iter()
                .map(|c| c.code())
                .collect::<Vec<_>>()
                .join("、"),
            self.amortized,
            self.rationale
        )
    }

    /// 阶段中文名。
    pub fn label(&self) -> &'static str {
        self.id.zh()
    }

    /// 该段是否允许产出某失败码。
    pub fn allows(&self, code: DiagCode) -> bool {
        self.allowed_failures.contains(&code)
    }
}

/// 查阶段规格；未注册即显性失败。
pub fn lookup_stage(id: &str) -> Outcome<StageSpec> {
    match StageId::from_en(id) {
        None => Outcome::err(
            DiagCode::StageUnregistered,
            &format!("阶段 {} 不在六段流水线冻结清单内", id),
            &format!(
                "六段固定为：{}；新增阶段属破坏性变更，须走 ADR（F1598）而非悄悄插入",
                stage_order_line()
            ),
        ),
        Some(sid) => match STAGE_SPECS.iter().find(|s| s.id == sid) {
            Some(s) => Outcome::ok(*s),
            None => Outcome::err(
                DiagCode::StageUnregistered,
                &format!("阶段 {} 未在 STAGE_SPECS 中登记", id),
                "六段签名表与阶段枚举必须一一对应；缺登记即契约断链",
            ),
        },
    }
}

/// 六段次序的人话串。
pub fn stage_order_line() -> String {
    STAGE_ORDER
        .iter()
        .map(|s| s.zh())
        .collect::<Vec<_>>()
        .join(" → ")
}

/// 六段顺序校验：实际次序必须严格等于冻结次序。
///
/// 不接受「子集」或「超集」——跳过某段正是契约分歧的形态（跳过 verify 意味
/// 着损坏资源可直接交付）。
pub fn check_stage_order(actual: &[StageId]) -> Outcome<Vec<StageId>> {
    if actual.len() != STAGE_ORDER.len() {
        return Outcome::err(
            DiagCode::StageOrderViolated,
            &format!(
                "六段流水走了 {} 段，应为 {} 段（顺序：{}）",
                actual.len(),
                STAGE_ORDER.len(),
                stage_order_line()
            ),
            "跳过某段即契约分歧：跳过 verify 意味着损坏资源可直接交付。\
             请补齐六段，或按 ADR 正式变更冻结签名",
        );
    }
    for (i, got) in actual.iter().enumerate() {
        let want = STAGE_ORDER[i];
        if *got != want {
            return Outcome::err(
                DiagCode::StageOrderViolated,
                &format!(
                    "第 {} 段实为「{}」，冻结次序要求「{}」",
                    i,
                    got.zh(),
                    want.zh()
                ),
                &format!(
                    "六段顺序不可调换：完整次序为 {}。\
                     调换顺序会改变语义（如先交付后校验等于交付未校验资源）",
                    stage_order_line()
                ),
            );
        }
    }
    Outcome::ok(actual.to_vec())
}

/// 段失败码归属校验：给定阶段与失败码，判定该码在此段是否合法。
pub fn check_stage_failure(stage: StageId, code: DiagCode) -> Outcome<StageSpec> {
    match STAGE_SPECS.iter().find(|s| s.id == stage) {
        None => Outcome::err(
            DiagCode::StageUnregistered,
            &format!("阶段 {} 未在冻结清单内", stage.en()),
            "先登记阶段，再判失败码归属",
        ),
        Some(spec) => {
            if spec.allows(code) {
                Outcome::ok(*spec)
            } else {
                Outcome::err(
                    DiagCode::StageContractDiverged,
                    &format!(
                        "段「{}」不允许产出 {}（该段白名单：{}）",
                        spec.label(),
                        code.code(),
                        spec.allowed_failures
                            .iter()
                            .map(|c| c.code())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                    "契约分歧意味着错误在这一段被换了新码抛出，根因信息在中途丢失。\
                     请让失败码沿原码上抛；若确需新增失败码，先改 STAGE_SPECS 再改实现（顺序不可反）",
                )
            }
        }
    }
}

/// 段观察记录（实际执行的一次失败）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageObservation {
    /// 阶段。
    pub stage: StageId,
    /// 实际观察到的失败码（无失败则 `None`）。
    pub observed_failure: Option<DiagCode>,
}

/// 段间契约对拍：把一次实际执行各段的实际失败码与冻结签名逐段比对。
///
/// 这是「段间契约分歧 → 对拍拦截」的落点：契约写在数据里（[`STAGE_SPECS`]），
/// 实际行为经参数传入，比对不通过即拦截——而不是等集成期靠人发现。
pub fn audit_stage_contract(observations: &[StageObservation]) -> Outcome<Vec<DiagCode>> {
    let mut bad: Vec<DiagCode> = Vec::new();
    let mut bag = DiagBag::new();
    for o in observations.iter() {
        let Some(spec) = STAGE_SPECS.iter().find(|s| s.id == o.stage) else {
            bag.push(
                DiagCode::StageUnregistered,
                &format!("观察记录含未注册阶段 {}", o.stage.en()),
                "阶段未注册则无白名单可比对；先补 STAGE_SPECS 再对拍",
            );
            continue;
        };
        let Some(code) = o.observed_failure else {
            continue;
        };
        if !spec.allows(code) {
            bad.push(code);
            bag.push(
                DiagCode::StageContractDiverged,
                &format!(
                    "段「{}」实际产出 {}，不在冻结白名单（{}）内",
                    spec.label(),
                    code.code(),
                    spec.allowed_failures
                        .iter()
                        .map(|c| c.code())
                        .collect::<Vec<_>>()
                        .join("、")
                ),
                "契约分歧意味着错误在这一段被换了新码抛出，根因信息在中途丢失。\
                 请让失败码沿原码上抛；若确需新增失败码，先改 STAGE_SPECS 再改实现（顺序不可反）",
            );
        }
    }
    if !bad.is_empty() {
        return Outcome::Err {
            code: DiagCode::StageContractDiverged,
            message: format!("段间契约分歧 {} 处", bad.len()),
            hint: "请让失败码沿原码上抛，并先改冻结签名再改实现".to_string(),
            diagnostics: bag.all(),
        };
    }
    Outcome::ok_with(bad, bag.all())
}
// ---------------------------------------------------------------------------
// 七、单向委托：Q 调度、F 解码（判据「单向委托」）
// ---------------------------------------------------------------------------

/// 允许的 Q→F 委托（正向）。
pub const DELEGATION_ALLOWED: [&str; 3] = [
    "Q->F:requestDecode(rawBytes, decoderId) -> decodedOrError",
    "Q->F:queryDecoderCapabilities(kind) -> DecoderCaps",
    "Q->F:releaseDecoder(decoderId)",
];

/// 明确禁止的反向依赖：F 域**不得**调用的 Q 域能力（任何一条出现在调用现场
/// 即判越权）。
///
/// 表项登记为**裸签名**（不带 `F->Q:` 前缀）：比对时调用记录会拼上调用方前缀
/// 成为 `F->acquireRefCount`，若表项自带前缀则两侧形态不同、永不相等——
/// 于是这条红线**一次都不会触发**，看上去有红线实则没有（真实缺陷，
/// 独立探针 `veq01_delegation_redline_actually_fires` 专盯此项）。
pub const DELEGATION_FORBIDDEN: [&str; 4] = [
    "acquireRefCount(key)",
    "releaseRefCount(key)",
    "loadResource(uri)",
    "enqueueRequest(uri)",
];

/// 委托调用方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegateSide {
    /// Q 域（调度方）。
    Q,
    /// F 域（解码方）。
    F,
}

impl DelegateSide {
    /// 单字母标识（拼调用键用）。
    pub fn tag(self) -> &'static str {
        match self {
            DelegateSide::Q => "Q",
            DelegateSide::F => "F",
        }
    }
}

/// 一次委托调用的记录（供对拍与审计）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegationCall {
    /// 调用方。
    pub from: DelegateSide,
    /// 签名（可带实参形态）。
    pub signature: String,
}

/// 剥掉签名末尾的参数列表：`acquireRefCount(key)` → `acquireRefCount`。
///
/// 禁止表键统一登记为「带参形态」，调用点记录则可能带实参也可能不带。两侧都
/// 过这道规范化，`contains` 才可靠。
///
/// 历史缺陷（勿回退）：只在一侧规范化时，禁止项表写的是
/// `F->Q:acquireRefCount(key)`，而运行时拼出的是无参形态
/// `F->Q:acquireRefCount`，直接比较永不相等——于是单向委托检测**一次都不会
/// 触发**，看起来有红线实则没有。教训：白名单键必须两侧同规范化。
pub fn strip_args(signature: &str) -> String {
    let close = match signature.rfind(')') {
        Some(i) => i,
        None => return signature.to_string(),
    };
    // 末尾括号之前必须还有左括号，否则形如 `foo)` 的畸形串会被误剥。
    match signature[..close].find('(') {
        Some(open) => signature[..open].trim().to_string(),
        None => signature.to_string(),
    }
}

/// 单向委托检查：给定一批调用记录，检出 F→Q 的反向依赖。
///
/// 检查方式刻意做成「键规范化 + 黑名单逐条比对」而非类型系统约束——因为跨域
/// 调用常发生在动态分发点上（解码器是插件），类型层面拦不住。字符串匹配虽
/// 粗糙，但足以在开发期把「谁调了不该调的」摆在明面上。
pub fn check_delegation_direction(calls: &[DelegationCall]) -> Outcome<Vec<String>> {
    let mut bag = DiagBag::new();
    let mut violations: Vec<String> = Vec::new();
    for c in calls.iter() {
        // 只有 F 发起的调用才可能命中禁止表（禁止项全部是「F 不得调 Q 的能力」）。
        // 两侧都过 `strip_args`：表项写 `acquireRefCount(key)`，现场可能记成
        // `acquireRefCount`，不统一形态则比较永不相等。
        if c.from != DelegateSide::F {
            continue;
        }
        let bare = strip_args(&c.signature);
        if DELEGATION_FORBIDDEN.iter().any(|f| strip_args(f) == bare) {
            violations.push(format!("{}->{}", c.from.tag(), bare));
        }
    }
    if !violations.is_empty() {
        bag.push(
            DiagCode::DelegationDirectionViolated,
            &format!(
                "检出 F→Q 反向依赖 {} 处：{}",
                violations.len(),
                violations.join("、")
            ),
            "单向委托红线：Q 调度、F 解码。F 域一旦反向持有调度状态（引用计数、加载队列），\
             两域即形成环，任何一方都无法独立测试，且释放时机脱离 Q 域预算——\
             请把这类状态上移到 Q 域，由 Q 域在解码完成后接管生命周期",
        );
        return Outcome::Err {
            code: DiagCode::DelegationDirectionViolated,
            message: format!("检出 F→Q 反向依赖：{}", violations.join("、")),
            hint: "把状态上移到 Q 域，遵守 Q 调度 F 解码的单向委托".to_string(),
            diagnostics: bag.all(),
        };
    }
    Outcome::ok_with(violations, bag.all())
}

// ---------------------------------------------------------------------------
// 八、消费收敛红线：全部消费域一律走 Q 管线（判据「收敛红线」）
// ---------------------------------------------------------------------------

/// 消费域标识。
///
/// 锚点正文称「O/N/G/H/I/P 八域消费侧统一接口」，其中 O/N/G/H/I/P 为已具名
/// 登记的六域；「八域」是总称口径（含未在锚点列名者）。本枚举只登记锚点
/// 具名的六域——**未登记域一律拒绝**，边界外的生命周期管不到。登记扩充须走
/// ADR（改域集合属消费收敛契约变更）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsumerDomain {
    /// O 域· CSS/HTML 表面。
    O,
    /// N 域 · UI 框架内核。
    N,
    /// G 域 · 视频引擎。
    G,
    /// H 域 · 音频服务。
    H,
    /// I 域 · 3D 管线。
    I,
    /// P 域 · 动效与交互。
    P,
}

impl ConsumerDomain {
    /// 全集（确定性次序）。
    pub const ALL: [ConsumerDomain; 6] = [
        ConsumerDomain::O,
        ConsumerDomain::N,
        ConsumerDomain::G,
        ConsumerDomain::H,
        ConsumerDomain::I,
        ConsumerDomain::P,
    ];

    /// 域标识串。
    pub fn id(self) -> &'static str {
        match self {
            ConsumerDomain::O => "VE-O",
            ConsumerDomain::N => "VE-N",
            ConsumerDomain::G => "VE-G",
            ConsumerDomain::H => "VE-H",
            ConsumerDomain::I => "VE-I",
            ConsumerDomain::P => "VE-P",
        }
    }

    /// 中文名（收敛红线报错与看板引用同一事实源）。
    pub fn label(self) -> &'static str {
        match self {
            ConsumerDomain::O => "CSS/HTML 表面",
            ConsumerDomain::N => "UI 框架内核",
            ConsumerDomain::G => "视频引擎",
            ConsumerDomain::H => "音频服务",
            ConsumerDomain::I => "3D 管线",
            ConsumerDomain::P => "动效与交互",
        }
    }

    /// 由域标识串反查（未登记域 → `None`，即拒绝）。
    pub fn from_id(s: &str) -> Option<ConsumerDomain> {
        ConsumerDomain::ALL.iter().copied().find(|d| d.id() == s)
    }

    /// 该域典型消费的资源类型（消费模式适配的输入）。
    pub fn typical_kinds(self) -> &'static [ResourceKind] {
        match self {
            ConsumerDomain::O => &[ResourceKind::Style, ResourceKind::Texture, ResourceKind::Font],
            ConsumerDomain::N => &[ResourceKind::Font, ResourceKind::Style],
            ConsumerDomain::G => &[ResourceKind::Stream, ResourceKind::Audio],
            ConsumerDomain::H => &[ResourceKind::Audio],
            ConsumerDomain::I => &[
                ResourceKind::Model,
                ResourceKind::Geometry,
                ResourceKind::Texture,
                ResourceKind::Shader,
            ],
            ConsumerDomain::P => &[ResourceKind::Animation, ResourceKind::Scene],
        }
    }

    /// 读屏可读单行。
    pub fn screen_line(self) -> String {
        format!(
            "消费域 {}（{}），典型消费资源 {}",
            self.id(),
            self.label(),
            self.typical_kinds()
                .iter()
                .map(|k| k.zh())
                .collect::<Vec<_>>()
                .join("、")
        )
    }
}

/// 消费域标识全集串（进错误信息，让拒绝自解释）。
pub fn consumer_domain_line() -> String {
    ConsumerDomain::ALL
        .iter()
        .map(|d| d.id())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 消费侧统一接口签名（各域拿到的是**同一个**函数——收敛红线的可执行形态）。
pub const CONSUMER_INTERFACE: &str =
    "Q->Consumer: acquire(canonicalUri, consumerDomain) -> ResourceHandle";

/// 资源句柄：不透明值（消费方持有不解引用——不解引用才能保证 O(1) 与生命
/// 周期唯一）。
///
/// 句柄**不得携带资源数组指针**，否则交付退化为 O(资源大小)（见
/// [`PERF_BUDGET`] 的 deliver 段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceHandle {
    /// 句柄内含的规范化缓存键（仅 Q 域内部使用；消费方不解引用）。
    pub cache_key: String,
    /// 句柄所指资源类型。
    pub kind: ResourceKind,
    /// 交付时的状态快照（消费方可查，用于「为什么还没好」的透明化）。
    pub state: HandleState,
}

/// 句柄状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleState {
    /// 就绪。
    Ready,
    /// 在途。
    Pending,
    /// 失败（失败详情走透传信封，见 [`DecodeFailureEnvelope`]）。
    Failed,
}

impl HandleState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            HandleState::Ready => "就绪",
            HandleState::Pending => "在途",
            HandleState::Failed => "失败",
        }
    }
}

/// 构建 ready 句柄（成功路径唯一出口——故成功无分支，杜绝各处手工造句柄）。
fn ready_handle(cache_key: &str, kind: ResourceKind) -> ResourceHandle {
    ResourceHandle {
        cache_key: cache_key.to_string(),
        kind,
        state: HandleState::Ready,
    }
}

impl ResourceHandle {
    /// 读屏可读单行（无障碍：句柄要能念，且不暴露路径）。
    pub fn screen_line(&self) -> String {
        format!(
            "资源句柄：类型 {}，状态 {}（句柄不可解引用，细节由 Q 域管线日志提供）",
            self.kind.zh(),
            self.state.zh()
        )
    }
}

/// 消费侧统一入口：任意消费域按 canonical URI 获取资源句柄（收敛红线）。
///
/// 这是 `consumer-acquire` 协议的实现。消费域拿到的是**同一个函数**——收敛
/// 红线的可执行形态不是「大家约定都走这里」，而是「只有这里能造句柄」。
///
/// 四道闸（与 address 段同一套红线，此处复用而非重写——重写就会出现两套口径）：
/// ① 消费域是否合法（未登记域不得发起管线请求）；
/// ② URI 归属红线（不含用户内容）+ 结构与穿越；
/// ③ scheme 权限（含 remote 显式授权）；
/// ④ 资源类型是否已登记（不受管 = 第二个生命周期权威）。
pub fn acquire_from_pipeline(
    canonical_uri: &str,
    consumer_domain: &str,
    grants: &RemoteGrantTable,
) -> Outcome<ResourceHandle> {
    let Some(dom) = ConsumerDomain::from_id(consumer_domain) else {
        return Outcome::err(
            DiagCode::PrivateLoadDetected,
            &format!("未登记的消费域「{}」试图发起管线请求", consumer_domain),
            &format!(
                "收敛红线：只有 {} 这些消费域可走 Q 域管线；未登记域接入意味着\
                 管线边界外还有一个我们管不到的生命周期。请先把该域登记进 ConsumerDomain，\
                 或改由已登记域代为请求",
                consumer_domain_line()
            ),
        );
    };
    // 两道闸用显式 `match` 传播而非 `?`：本函数返回 `Outcome`（不是
    // `Result`），`?` 需要 `Try` trait，而它在 stable 上不可实现。
    let uri = match parse_resource_uri(canonical_uri, Some(dom.id()), grants) {
        Outcome::Ok { value, diagnostics } => Outcome::Ok { value, diagnostics },
        e @ Outcome::Err { .. } => return e.map_empty(),
    };
    let uri = match uri {
        Outcome::Ok { value, .. } => value,
        e @ Outcome::Err { .. } => return e.map_empty(),
    };
    let mapping = match lookup_mapping_by_str(uri.kind_segment()) {
        Outcome::Ok { value, diagnostics } => Outcome::Ok { value, diagnostics },
        e @ Outcome::Err { .. } => return e.map_empty(),
    };
    let mapping = match mapping {
        Outcome::Ok { value, .. } => value,
        e @ Outcome::Err { .. } => return e.map_empty(),
    };
    Outcome::ok(ready_handle(&uri.canonical, mapping.kind))
}

/// 句柄释放（`consumer-release` 协议的实现）。
///
/// 为何空实现也要成函数：释放权必须**只有一处**。若各域自己管释放时机，就
/// 回到「两份生命周期」的老问题。开工条阶段尚无引用计数（F3203 承接），但
/// 接口先冻结——接口晚冻结会让已写好的调用点各自发明签名。
pub fn release_handle(handle: &ResourceHandle, consumer_domain: &str) -> Outcome<bool> {
    if ConsumerDomain::from_id(consumer_domain).is_none() {
        return Outcome::err(
            DiagCode::PrivateLoadDetected,
            &format!("未登记的消费域「{}」试图释放句柄", consumer_domain),
            "句柄的交付与释放必须成对地经 Q 域，否则无法保证引用计数平衡",
        );
    }
    if handle.cache_key.trim().is_empty() {
        return Outcome::err(
            DiagCode::ValueInvalid,
            "释放的句柄缓存键为空（伪造句柄）",
            "句柄只能由 acquire_from_pipeline 产生；构造不出来的句柄说明调用方绕过了管线（收敛红线）",
        );
    }
    Outcome::ok(true)
}

/// 私加载现场记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateLoadSighting {
    /// 消费域。
    pub domain: ConsumerDomain,
    /// 现场描述：消费域声称自己做了什么。
    pub action: String,
    /// 是否声明自己走了 Q 管线（`true` = 合法）。
    pub via_pipeline: bool,
}

/// 私加载立案条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateLoadCase {
    /// 消费域。
    pub domain: ConsumerDomain,
    /// 动作。
    pub action: String,
    /// 立案号（`PRIVLOAD-<域>-<序号>`，序号从 1 起，确定性）。
    pub case_id: String,
}

impl PrivateLoadCase {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "分叉缺陷立案 {}：{} 域（{}）声称 {}",
            self.case_id,
            self.domain.id(),
            self.domain.label(),
            self.action
        )
    }
}

/// 私加载立案结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateLoadVerdict {
    /// 立案明细。
    pub cases: Vec<PrivateLoadCase>,
    /// 检出的私加载数（> 0 即为分叉缺陷）。
    pub count: usize,
    /// 人话汇总（可直接进缺陷账本）。
    pub summary: String,
}

/// 私加载检出与立案。收敛红线的**可执行**形态。
///
/// 收敛红线（多对一）：全部消费域**一律走 Q 管线**。理由是资源在系统里必须
/// 只有一个生命周期权威；一旦某域私加载，同一资源就有两份缓存、两个释放时机，
/// Q 域的预算统计从此失真。
pub fn detect_private_load(sightings: &[PrivateLoadSighting]) -> PrivateLoadVerdict {
    let mut cases: Vec<PrivateLoadCase> = Vec::new();
    let mut seq = 0usize;
    for s in sightings.iter() {
        if s.via_pipeline {
            continue;
        }
        seq += 1;
        cases.push(PrivateLoadCase {
            domain: s.domain,
            action: s.action.clone(),
            case_id: format!("PRIVLOAD-{}-{}", s.domain.id(), seq),
        });
    }
    let summary = if cases.is_empty() {
        "收敛红线实测：未检出私加载，消费域取资源全部经 Q 管线".to_string()
    } else {
        format!(
            "收敛红线实测：检出 {} 处私加载（{}）——每处都形成第二个生命周期权威，已立案待修",
            cases.len(),
            cases
                .iter()
                .map(|c| format!("{}:{}", c.domain.label(), c.action))
                .collect::<Vec<_>>()
                .join("；")
        )
    };
    PrivateLoadVerdict {
        count: cases.len(),
        cases,
        summary,
    }
}

// ---------------------------------------------------------------------------
// 九、透传不失真：F 域错误码原样上抛（判据「透传不失真」）
// ---------------------------------------------------------------------------

/// 成功哨兵：本次委托不存在失败码（区别于「失败但码为空」——后者是失真）。
pub const DECODE_OK_SENTINEL: &str = "F_DECODE_OK";

/// 解码失败的上抛结构：**原码 + 补充三要素**。
///
/// 不重写原码的理由（F 域错误码携带格式专属信息）：F 域的码能告诉用户
/// 「PNG 的第 3 块 CRC 不符」，而通用码只能说「加载失败」。Q 域补上下文（哪个
/// URI、哪个段、怎么办），但不删不改 F 域的原码——于是用户看到的是
/// 「加载失败（原始原因：PNG_CHUNK_CRC_MISMATCH，具体处置：…）」。
///
/// `mutated` 字段是自查用的：若调用方把原码换掉，此处置为 `true`，由
/// [`verify_passthrough`] 机检拦下。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeFailureEnvelope {
    /// F 域原始错误码，原样透传。
    pub decoder_code: String,
    /// Q 域补充的上下文之一：哪个资源。
    pub uri: String,
    /// Q 域补充的上下文之二：哪个段。
    pub stage: StageId,
    /// Q 域补充的上下文之三：怎么办。
    pub hint: String,
    /// 原码是否被改写（`true` = 违反透传不失真红线）。
    pub mutated: bool,
}

impl DecodeFailureEnvelope {
    /// 读屏可读单行（无障碍：错误要能念，且不暴露路径）。
    pub fn screen_line(&self) -> String {
        format!(
            "素材加载失败：原始原因码 {}，失败于{}段，处置建议：{}（不播报本地路径）",
            self.decoder_code,
            self.stage.zh(),
            self.hint
        )
    }
}

/// F 域解码产物的最小形态（真实产物形态由 F 域定义，Q 域只认这层壳）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeProduct {
    /// 解码器标识（来自十项映射的 `decoded_by`）。
    pub decoder_id: String,
    /// 解码后的资源标识（Q 域据此定缓存键）。
    pub product_id: String,
    /// 解码产物的字节数（预算记账用）。
    pub byte_length: usize,
    /// F 域登记的失败码（成功时为 [`DECODE_OK_SENTINEL`]）。
    ///
    /// 为何成功也要带这个字段：它让透传校验成为**纯数据比对**而非依赖日志。
    /// 失败时上抛的信封里 `decoder_code` 必须等于此处的登记值；成功时带上
    /// 登记值，使得「本次委托对应的 F 域错误码表」始终随产物流动——产物被日志
    /// 吞掉时，事后仍可据此重建「当时该走哪条错误码」。
    pub registered_code: String,
}

impl DecodeProduct {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "解码产物：解码器 {}，字节数 {}，登记码 {}",
            self.decoder_id, self.byte_length, self.registered_code
        )
    }
}

/// Q→F 解码委托（`decoder-delegate` 协议的实现）。
///
/// 单向委托的**正向**一半：Q 域交出字节流与类型，F 域返回产物或错误码。反向
/// 一半（F 域回头调 Q 域的调度能力）由 [`check_delegation_direction`] 拦截。
///
/// 关键设计：错误路径只做信封封装，**不改码**。此函数里出现的任何
/// `registered_code` 赋值都是原码拷贝或成功哨兵；一旦有人在这里写死
/// `registered_code: "LOAD_FAILED"` 这类常量，透传不失真红线即被破坏——故自检
/// 里有一条用例专门盯这一点。
pub fn delegate_decode(
    byte_length: usize,
    kind: ResourceKind,
    registered_code: &str,
) -> Outcome<DecodeProduct> {
    if byte_length == 0 {
        return Outcome::err(
            DiagCode::IoNotFound,
            "解码委托收到空字节流",
            "空字节流不是有效输入：说明上游取字节环节已失败却仍在往下走。\
             请在 load 段拦截空内容，不要让 F 域去面对它",
        );
    }
    let mapping = match lookup_mapping(kind) {
        Outcome::Ok { value, .. } => value,
        e @ Outcome::Err { .. } => return e.map_empty(),
    };
    Outcome::ok(DecodeProduct {
        decoder_id: mapping.decoded_by.to_string(),
        product_id: format!("product:{}:{}", kind.en(), byte_length),
        byte_length,
        registered_code: if registered_code.is_empty() {
            DECODE_OK_SENTINEL.to_string()
        } else {
            registered_code.to_string()
        },
    })
}

/// 透传校验：给定一个上抛信封与 F 域登记的原码，检查是否失真。
///
/// 三条判据（缺一即失真）：
/// ① 原码非空且与 F 域登记一致——被改写或折叠成通用错误即失真；
/// ② `mutated` 标志为 `false`；
/// ③ 补充三要素齐备（uri/hint）——透传不等于甩锅，Q 域仍须负责「怎么办」
/// 这一段。
pub fn verify_passthrough(
    env: &DecodeFailureEnvelope,
    registered_decoder_code: &str,
) -> Outcome<DecodeFailureEnvelope> {
    if env.decoder_code != registered_decoder_code {
        return Outcome::err(
            DiagCode::ErrorCodeMutated,
            &format!(
                "解码错误码被改写：信封码 {} ≠ F 域登记码 {}",
                env.decoder_code, registered_decoder_code
            ),
            "透传不失真红线：F 域错误码携带格式专属信息，改写它会让消费方失去定位能力。\
             请原样透传原码，并把 Q 域上下文补在 hint 里——补上下文可以，改码不行",
        );
    }
    if env.mutated {
        return Outcome::err(
            DiagCode::ErrorCodeMutated,
            "解码错误信封自带 mutated=true 标志（原码在流转中被改写）",
            "请检查中间层是否把错误码统一包装成了通用错误；透传时只允许补字段不允许改值",
        );
    }
    if env.uri.trim().is_empty() || env.hint.trim().is_empty() {
        return Outcome::err(
            DiagCode::ErrorCodeMutated,
            "解码错误信封缺少 uri 或 hint（透传不等于甩锅）",
            "Q 域须在透传原码的同时补充「哪个资源、怎么办」；否则用户只看到格式错误却无法行动",
        );
    }
    Outcome::ok(env.clone())
}

// ---------------------------------------------------------------------------
// 十、启动预载（区分致命与可延后，不混成一个数字）
// ---------------------------------------------------------------------------

/// 预载条目（`domain-warmup` 协议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarmupEntry {
    /// 资源 URI。
    pub uri: String,
    /// 资源类型。
    pub kind: ResourceKind,
    /// 预载失败时是否致命（启动期关键资源为 `true`）。
    pub critical: bool,
}

/// 预载报告：区分「致命失败」与「可延后失败」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarmupReport {
    /// 请求预载数。
    pub requested: usize,
    /// 成功预载数。
    pub warmed: usize,
    /// 可延后失败数。
    pub deferred: usize,
    /// 致命失败 URI 清单。
    pub critical_failures: Vec<String>,
}

impl WarmupReport {
    /// 读屏摘要（启动期无用户交互，但诊断须留痕）。
    pub fn screen_line(&self) -> String {
        format!(
            "启动预载：请求 {}，成功 {}，可延后失败 {}，致命失败 {}",
            self.requested,
            self.warmed,
            self.deferred,
            self.critical_failures.len()
        )
    }
}

/// 启动期预载（`domain-warmup` 协议的实现）。
///
/// 为何区分 `critical` 与非 `critical`：把所有预载失败同等对待，会让一次可选
/// 素材缺失导致启动失败——这是常见的过度失败；而把关键素材缺失也当「可延后」，
/// 则问题被推迟到用户看到空画面时才暴露。故按 `critical` 分流是必需的，不是
/// 可选的精细化。
pub fn warm_pipeline(
    entries: &[WarmupEntry],
    grants: &RemoteGrantTable,
) -> Outcome<WarmupReport> {
    let mut bag = DiagBag::new();
    let mut critical_failures: Vec<String> = Vec::new();
    let mut warmed = 0usize;
    let mut deferred = 0usize;
    for e in entries.iter() {
        // 启动期无消费域；remote 一律拒绝（无人可审计）。
        let uri = parse_resource_uri(&e.uri, None, grants);
        let mapping = lookup_mapping(e.kind);
        // 两道校验各自独立：先看寻址，再看类型映射，任一失败即记为预载失败。
        // 不把两个 `Outcome` 配成元组去解构——那要求 `Outcome::Err` 恰好三个
        // 字段，而它有四个（多了 `diagnostics`），解构即错。
        let mut fail_detail: Option<DiagCode> = None;
        if let Some(code) = uri.code() {
            bag.push_all(uri.diagnostics());
            fail_detail = Some(code);
        } else if let Some(code) = mapping.code() {
            bag.push_all(mapping.diagnostics());
            fail_detail = Some(code);
        }
        match fail_detail {
            None => warmed += 1,
            Some(_) => {
                if e.critical {
                    critical_failures.push(e.uri.clone());
                } else {
                    deferred += 1;
                }
            }
        }
    }
    Outcome::ok_with(
        WarmupReport {
            requested: entries.len(),
            warmed,
            deferred,
            critical_failures,
        },
        bag.all(),
    )
}

// ---------------------------------------------------------------------------
// 十一、依赖图与环检测（F3202/F3206 的数据基础与前置闸）
// ---------------------------------------------------------------------------

/// 资源依赖图的一条边（`from` 依赖 `to`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyEdge {
    /// 依赖方。
    pub from: String,
    /// 被依赖方。
    pub to: String,
}

/// 依赖环检测（迭代式 DFS + 回边判定，复杂度 O(V+E)）。
///
/// 为何开工条就带环检测：环 = 加载死锁（A 等 B 的同时 B 等 A），且它在集成期
/// 才暴露时定位成本极高。此处提供前置闸，深度算法本体（拓扑序、深度钳制）
/// 归 F3206。
///
/// 用**迭代式** DFS 而非递归：递归深度随依赖链线性增长，深链图会吃掉内核栈
/// （内核栈预算不可被业务逻辑侵占）。
pub fn detect_dependency_cycle(edges: &[DependencyEdge]) -> Outcome<Vec<String>> {
    // 邻接表（保持插入序，确定性——回归可对拍）。
    let mut nodes: Vec<String> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();
    let index_of = |nodes: &mut Vec<String>,
                        adj: &mut Vec<Vec<usize>>,
                        name: &str|
     -> usize {
        match nodes.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                nodes.push(name.to_string());
                adj.push(Vec::new());
                nodes.len() - 1
            }
        }
    };
    for e in edges.iter() {
        let from = index_of(&mut nodes, &mut adj, &e.from);
        let to = index_of(&mut nodes, &mut adj, &e.to);
        adj[from].push(to);
    }

    // 0=未访问 1=在栈 2=已完成
    let mut state: Vec<u8> = vec![0; nodes.len()];
    let mut path: Vec<usize> = Vec::new();
    let mut cycle: Option<Vec<String>> = None;

    for start in 0..nodes.len() {
        if state[start] != 0 || cycle.is_some() {
            continue;
        }
        state[start] = 1;
        path.push(start);
        // 栈帧：(节点, 已处理到的子节点下标)
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        while let Some((node, child)) = stack.pop() {
            if child < adj[node].len() {
                // 先把本帧的进度推进，再看子节点。
                stack.push((node, child + 1));
                let next = adj[node][child];
                if state[next] == 1 {
                    // 命中回边：收集环上节点（从该节点起按入栈顺序取）。
                    let start_at = match path.iter().position(|p| *p == next) {
                        Some(i) => i,
                        // 理论上 `next` 必在 path 中（state==1 即在栈）；
                        // 取 0仅为极端畸形图（自指已出栈）的兜底，不 panic。
                        None => 0,
                    };
                    let mut cyc: Vec<String> =
                        path[start_at..].iter().map(|i| nodes[*i].clone()).collect();
                    cyc.push(nodes[next].clone());
                    cycle = Some(cyc);
                    break;
                }
                if state[next] == 0 {
                    state[next] = 1;
                    path.push(next);
                    stack.push((next, 0));
                }
            } else {
                // 子节点处理完毕，出栈。
                state[node] = 2;
                if path.last() == Some(&node) {
                    path.pop();
                }
            }
        }
        if cycle.is_some() {
            break;
        }
    }

    match cycle {
        Some(cyc) => Outcome::err(
            DiagCode::DependencyCycle,
            &format!("资源依赖成环：{}", cyc.join(" → ")),
            "加载依赖成环会让加载死锁（A 等 B 的同时 B 等 A）。\
             请打破环——通常是把某个资源改为「后加载的弱依赖」（不阻塞主资源交付）",
        ),
        None => Outcome::ok(Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// 十二、性能预算（诚实声明复杂度，不谎报 O(1)）
// ---------------------------------------------------------------------------

/// 复杂度预算项。
#[derive(Clone, Copy, Debug)]
pub struct PerfBudgetItem {
    /// 项名。
    pub item: &'static str,
    /// 关于输入规模的渐近行为。
    pub complexity: &'static str,
    /// 为何在真实区间内是常数级（`None` = 明说它不是常数级）。
    pub bounded_by: Option<&'static str>,
    /// 备注。
    pub note: &'static str,
}

/// 六段流水 + 句柄交付 + 映射查找的复杂度预算。
///
/// 判据不是「都写 O(1)」——那会诱使把 O(n) 谎报为 O(1)。真正要保证的是：每项
/// 都诚实声明自己是什么复杂度，且复杂度要么是常数级、要么明确写出它为什么
/// 会变成常数级（`bounded_by` 非空），要么明说它不是常数级（`bounded_by`
/// 为 `None`）。
pub const PERF_BUDGET: [PerfBudgetItem; 8] = [
    PerfBudgetItem {
        item: "六段流水整体",
        complexity: "O(1) 每段摊销",
        bounded_by: Some("每段只做常数次表查找与字段拷贝，无按输入规模增长的循环"),
        note: "分段是为了可测，不是为了慢",
    },
    PerfBudgetItem {
        item: "URI 归一化（address 段）",
        complexity: "O(n) n=URI 长度",
        bounded_by: Some("n 已被 URI_MAX_LEN 硬上限钳住（同时是防误用闸门）"),
        note: "对输入规模是线性扫描，但输入规模被硬上限钳住，故真实区间内为常数级。\
              若取消该上限并接受任意长 URI，此项预算必须重评为真 O(n)",
    },
    PerfBudgetItem {
        item: "缓存键构造（request 段）",
        complexity: "O(1)",
        bounded_by: Some("缓存键字段数由映射表固定（非按需拼接）"),
        note: "字段数固定故与 URI 长度无关",
    },
    PerfBudgetItem {
        item: "并发合并判定（schedule 段）",
        complexity: "O(1)",
        bounded_by: Some("哈希表按缓存键查找，不遍历在途请求列表"),
        note: "遍历在途列表是 O(并发数)，是本段最容易被无意写坏的形态",
    },
    PerfBudgetItem {
        item: "句柄交付（deliver 段）",
        complexity: "O(1)",
        bounded_by: Some("句柄为不透明值，消费方只持有不解引用"),
        note: "句柄不得携带资源数组指针，否则退化为 O(资源大小)",
    },
    PerfBudgetItem {
        item: "资源映射查找",
        complexity: "O(1)",
        bounded_by: Some("十项映射为定长表"),
        note: "超过十项须重新评估是否仍是 O(1) 可接受（定长表的前提是定长）",
    },
    PerfBudgetItem {
        item: "query 键权限判定（address 段）",
        complexity: "O(k) k=query 键数",
        bounded_by: Some("键数受 URI 长度上限钳住"),
        note: "每键一次集合查找，集合为常量规模",
    },
    PerfBudgetItem {
        item: "依赖环检测",
        complexity: "O(V+E)",
        bounded_by: None,
        note: "此项不属常数级预算，故单列——把它混进 PERF_BUDGET 会让常量级预算的断言失去意义",
    },
];

/// 复杂度预算机检：断言每项都诚实声明复杂度与上界来源。
pub fn audit_perf_budget() -> Outcome<Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    for b in PERF_BUDGET.iter() {
        if b.complexity.trim().is_empty() {
            bad.push(format!("项「{}」未声明复杂度", b.item));
            continue;
        }
        if b.complexity.contains("O(1)") && b.bounded_by.is_none() {
            bad.push(format!(
                "项「{}」自称 O(1) 但未注明为何在真实区间内为常数级",
                b.item
            ));
        }
    }
    if !bad.is_empty() {
        return Outcome::err(
            DiagCode::BudgetExceeded,
            &format!("复杂度预算审计失败（{} 处）", bad.len()),
            &bad.join("；"),
        );
    }
    Outcome::ok(PERF_BUDGET.iter().map(|b| b.item.to_string()).collect())
}

// ---------------------------------------------------------------------------
// 十三、开工门禁与下游归属
// ---------------------------------------------------------------------------

/// 开工门禁条目。
#[derive(Clone, Copy, Debug)]
pub struct GateCriterion {
    /// 门禁号。
    pub id: &'static str,
    /// 门禁描述。
    pub criterion: &'static str,
    /// 映射到哪条判据。
    pub maps_to: &'static str,
}

/// 开工门禁清单（逐条对应判据，可机检）。
pub const KICKOFF_GATES: [GateCriterion; 5] = [
    GateCriterion {
        id: "G1",
        criterion: "十项资源映射齐备，每项含解码归属、生命周期、缓存键与失败处置",
        maps_to: "十项映射",
    },
    GateCriterion {
        id: "G2",
        criterion: "六段签名冻结 v1，顺序校验通过，失败码白名单与实际行为一致",
        maps_to: "六段签名",
    },
    GateCriterion {
        id: "G3",
        criterion: "F→Q 反向依赖检出可用（单向委托红线可执行）",
        maps_to: "单向委托",
    },
    GateCriterion {
        id: "G4",
        criterion: "消费域统一接口，私加载检出与立案可用（收敛红线可执行）",
        maps_to: "收敛红线",
    },
    GateCriterion {
        id: "G5",
        criterion: "解码错误原码透传且补三要素，失真可机检",
        maps_to: "透传不失真",
    },
];

/// 下游归属表（防止开工条被当成万能筐，也防止各组互相抢活）。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 8] = [
    ("VE-F3202", "资源模型与引用图（悬空检测、环检测数据基础）"),
    ("VE-F3203", "句柄引用计数与分代 GC（句柄本体行为）"),
    ("VE-F3205", "优先级队列与请求合并（队列本体）"),
    ("VE-F3206", "依赖解析与加载图（拓扑排序、深度钳制）"),
    ("VE-F3207", "并发双池、老化与背压（调度器本体）"),
    ("VE-F3211", "资源版本与热更新（哈希指纹与原子切换）"),
    ("VE-F3213", "URI 规范化细则与寻址缓存（scheme 权限闸已在本条立）"),
    ("VE-F3215", "Q→F 委托协议 v2（流式解码、进度回调、解码取消点）"),
];

/// TS→Rust 迁移对照（Variable 2026-10-07「TypeScript 存量全面迁移」指令）。
///
/// 本条把原 `src/system/ve/qDomain/f3201-q-domain-pipeline-architecture.ts`
/// 的全部契约面逐条用 Rust 重新实现，落位于内核 crate。迁移范围仅限本任务
/// 涉及的模块，未触碰范围外的 TS 文件。
pub const MIGRATION_FROM_TS: [(&str, &str); 13] = [
    ("DiagCode/Diagnostic/Outcome/DiagBag", "§一 诊断基础设施（Rust enum + DiagBag）"),
    ("DOMAIN", "§二 域身份常量（DOMAIN_ITEM_LO/HI 等）"),
    ("Q_DOMAIN_TEN_TOPICS/Q_TOPIC_SPANS", "§二 QDomainTopic + TopicSpan"),
    (
        "ResourceKind/ResourceMapping/RESOURCE_MAPPINGS/lookupMapping",
        "§二 ResourceKind + ResourceMapping 表",
    ),
    (
        "OwnedCapability/DomainBoundary/DOMAIN_BOUNDARY_TABLE",
        "§三 OwnedCapability + DomainBoundary",
    ),
    ("resolveCapabilityOwner/adjudicateCapability", "§三 能力反查与裁决"),
    ("UpstreamProtocolEntry/UPSTREAM_LEDGER", "§四 上游协议接收台账"),
    ("ACCESSIBILITY_NOTE", "§五 A11Y_ERROR_PRINCIPLE 等三条"),
    (
        "ResourceUri/SCHEME_PERMISSION/parseResourceUri/normalizePath/buildUri",
        "§五 URI 四道闸 + 归一化",
    ),
    ("StageId/StageSpec/STAGE_SPECS/lookupStage", "§六 六段签名冻结 v1"),
    ("checkStageOrder/checkStageFailure/auditStageContract", "§六 顺序校验与段契约对拍"),
    (
        "ResourceHandle/acquireFromPipeline/releaseHandle/detectPrivateLoad",
        "§八 消费侧接收面、句柄与私加载立案",
    ),
    (
        "DelegationCall/checkDelegationDirection/stripArgs/DecodeProduct/\
         DecodeFailureEnvelope/verifyPassthrough/delegateDecode/warmPipeline/\
         detectDependencyCycle/PERF_BUDGET",
        "§七~§十二 单向委托、透传不失真、预载、环检测、复杂度预算",
    ),
];

// ---------------------------------------------------------------------------
// 十四、架构总纲本体（契约自检与读屏替代）
// ---------------------------------------------------------------------------

/// 字符串是否含调用方前缀（`->`）。
fn has_arrow_prefix(s: &str) -> bool {
    s.contains("->")
}

/// 契约问题（码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub cause: String,
    /// 建议。
    pub advice: String,
    /// 严重度：`blocker` 阻断 / `warn` 告警。
    pub severity: &'static str,
}

impl ContractIssue {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "契约问题[{}/{}]：{}；根因：{}；建议：{}",
            self.code, self.severity, self.symptom, self.cause, self.advice
        )
    }
}

/// 管线总纲本体。
#[derive(Clone, Debug)]
pub struct PipelineArchitecture {
    /// 架构版本。
    pub version: &'static str,
    /// 远程 scheme 授权表。
    pub remote_grants: RemoteGrantTable,
}

impl Default for PipelineArchitecture {
    fn default() -> Self {
        Self::standard()
    }
}

impl PipelineArchitecture {
    /// 标准总纲（远程授权表为空——远程默认拒绝，不是默认放行）。
    pub fn standard() -> Self {
        PipelineArchitecture {
            version: ARCH_VERSION,
            remote_grants: RemoteGrantTable::new(),
        }
    }

    /// 只读访问远程授权表。
    pub fn grants(&self) -> &RemoteGrantTable {
        &self.remote_grants
    }

    /// 可变访问远程授权表（授权/撤销走 [`RemoteGrantTable`] 的显式方法）。
    pub fn grants_mut(&mut self) -> &mut RemoteGrantTable {
        &mut self.remote_grants
    }

    /// 契约自检：开工条的声明不许断链、不许挂名、不许越界。
    ///
    /// 检出六类问题（开工条自己先做到可追溯，否则凭什么要求别人）：
    /// - `CONTRACT_MISSING`：项在册但实现缺失；
    /// - `CONTRACT_EMPTY`：字段留空（挂名）；
    /// - `CONTRACT_DUP`：同一项重复在册（唯一性）；
    /// - `CONTRACT_ORPHAN`：号段有洞或重叠（条目悬空 / 归属歧义）；
    /// - `CONTRACT_UNMAPPED`：枚举与数据表漂移（不可反查）。
    pub fn check_contracts(&self) -> Vec<ContractIssue> {
        let mut issues: Vec<ContractIssue> = Vec::new();

        // ① 十项映射：十类齐备、字段非空、类型可反查。
        if RESOURCE_MAPPINGS.len() != 10 {
            issues.push(ContractIssue {
                code: "CONTRACT_MISSING",
                symptom: format!("十项映射在册 {} 项，应为 10 项", RESOURCE_MAPPINGS.len()),
                cause: "映射表被增删——十项是官方资源范围的闭集".to_string(),
                advice: "恢复为十类；新增类型须走扩展点并重新评估定长表前提".to_string(),
                severity: "blocker",
            });
        }
        for m in RESOURCE_MAPPINGS.iter() {
            if m.decoded_by.trim().is_empty()
                || m.label.trim().is_empty()
                || m.note.trim().is_empty()
                || m.cache_key_fields.is_empty()
            {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("资源类型 {} 的映射有字段留空（挂名）", m.kind.en()),
                    cause: "映射字段留空——解码归属/缓存键/说明缺一即无法调度".to_string(),
                    advice: format!("补全 {} 的映射四要素", m.kind.en()),
                    severity: "blocker",
                });
            }
            if ResourceKind::from_en(m.kind.en()).is_none() {
                issues.push(ContractIssue {
                    code: "CONTRACT_UNMAPPED",
                    symptom: format!("资源类型 {} 无法由标识反查", m.kind.en()),
                    cause: "枚举与映射表漂移——URI 类型段将无法解析到该类".to_string(),
                    advice: format!("补 ResourceKind::from_en 的「{}」分支", m.kind.en()),
                    severity: "blocker",
                });
            }
            // `streamable` 与 `lifetime` 的一致性：frame-scoped 必须可流式
            // （帧级淘汰的资源若不可流送，则每帧都要整体重取——预算必然爆）。
            if m.lifetime == Lifetime::FrameScoped && !m.streamable {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("资源类型 {} 是按帧粒度却声明不可流式", m.kind.en()),
                    cause: "粒度与流送能力矛盾——按帧资源不可流送等于每帧整体重取".to_string(),
                    advice: format!("修正 {} 的 streamable 或 lifetime", m.kind.en()),
                    severity: "warn",
                });
            }
        }

        // ② 六段：六段齐备、契约字段非空、枚举可反查、失败码可反查。
        if STAGE_SPECS.len() != STAGE_ORDER.len() {
            issues.push(ContractIssue {
                code: "CONTRACT_MISSING",
                symptom: format!(
                    "六段规格在册 {} 段，应为 {} 段",
                    STAGE_SPECS.len(),
                    STAGE_ORDER.len()
                ),
                cause: "阶段表与阶段枚举数量不一致".to_string(),
                advice: "恢复为寻址→请求→调度→加载→校验→交付六段".to_string(),
                severity: "blocker",
            });
        }
        for s in STAGE_SPECS.iter() {
            if s.inputs.is_empty()
                || s.outputs.is_empty()
                || s.allowed_failures.is_empty()
                || s.rationale.trim().is_empty()
                || s.amortized.trim().is_empty()
            {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("段「{}」的签名有字段留空（挂名）", s.label()),
                    cause: "签名字段留空——冻结契约不允许残缺".to_string(),
                    advice: format!("补全段「{}」的入参/出参/失败码/理由", s.label()),
                    severity: "blocker",
                });
            }
            if StageId::from_en(s.id.en()).is_none() {
                issues.push(ContractIssue {
                    code: "CONTRACT_MISSING",
                    symptom: format!("段 {} 无法由标识反查", s.id.en()),
                    cause: "枚举与规格表漂移——对拍将找不到该段白名单".to_string(),
                    advice: format!("补 StageId::from_en 的「{}」分支", s.id.en()),
                    severity: "blocker",
                });
            }
            for c in s.allowed_failures.iter() {
                if DiagCode::from_code(c.code()).is_none() {
                    issues.push(ContractIssue {
                        code: "CONTRACT_UNMAPPED",
                        symptom: format!(
                            "段「{}」白名单含无法反查的诊断码 {}",
                            s.label(),
                            c.code()
                        ),
                        cause: "诊断码字符串与枚举漂移——跨语言对拍不可寻址".to_string(),
                        advice: "补 DiagCode::from_code 分支".to_string(),
                        severity: "blocker",
                    });
                }
            }
        }

        // ③ 主题号段：无缝且不重叠（条目悬空 / 归属歧义）。
        for t in Q_DOMAIN_TEN_TOPICS.iter() {
            let span = t.span();
            if span.hi < span.lo || span.item_count == 0 {
                issues.push(ContractIssue {
                    code: "CONTRACT_ORPHAN",
                    symptom: format!("主题「{}」号段非法（{}–{}）", t.zh(), span.lo, span.hi),
                    cause: "号段下界大于上界或条目数为零".to_string(),
                    advice: format!("修正主题「{}」的号段划分", t.zh()),
                    severity: "blocker",
                });
            }
            if span.hi > DOMAIN_ITEM_HI {
                issues.push(ContractIssue {
                    code: "CONTRACT_ORPHAN",
                    symptom: format!(
                        "主题「{}」号段上界 {} 越过域上界 {}",
                        t.zh(),
                        span.hi,
                        DOMAIN_ITEM_HI
                    ),
                    cause: "号段越界——该主题的部分条目落在 Q 域之外".to_string(),
                    advice: format!("收窄主题「{}」的号段", t.zh()),
                    severity: "blocker",
                });
            }
        }
        for i in 0..Q_DOMAIN_TEN_TOPICS.len() {
            for j in (i + 1)..Q_DOMAIN_TEN_TOPICS.len() {
                let a = Q_DOMAIN_TEN_TOPICS[i].span();
                let b = Q_DOMAIN_TEN_TOPICS[j].span();
                if a.lo <= b.hi && b.lo <= a.hi {
                    issues.push(ContractIssue {
                        code: "CONTRACT_ORPHAN",
                        symptom: format!(
                            "主题「{}」与「{}」号段重叠（{}–{} vs {}–{}）",
                            Q_DOMAIN_TEN_TOPICS[i].zh(),
                            Q_DOMAIN_TEN_TOPICS[j].zh(),
                            a.lo,
                            a.hi,
                            b.lo,
                            b.hi
                        ),
                        cause: "号段重叠——两个主题抢同一条目".to_string(),
                        advice: "重新划分号段使其无缝不重叠".to_string(),
                        severity: "blocker",
                    });
                }
                // 「有洞」只对**相邻**主题判定（按 ordinal 连号比较）：
                // 非相邻主题之间天然隔着中间主题的整段，若对所有主题对都比
                // 「b.lo == a.hi + 1」，则每对非相邻主题都被误判成洞——真实
                // 缺陷是判定式本身写错，不是号段划分错。重叠仍须比所有对。
                let adjacent = Q_DOMAIN_TEN_TOPICS[j].ordinal()
                    == Q_DOMAIN_TEN_TOPICS[i].ordinal().saturating_add(1);
                if adjacent && b.lo != a.hi + 1 {
                    issues.push(ContractIssue {
                        code: "CONTRACT_ORPHAN",
                        symptom: format!(
                            "主题「{}」（止于 {}）与「{}」（起于 {}）之间有洞",
                            Q_DOMAIN_TEN_TOPICS[i].zh(),
                            a.hi,
                            Q_DOMAIN_TEN_TOPICS[j].zh(),
                            b.lo
                        ),
                        cause: "号段有洞——某个主题的实现无处落号（条目悬空）".to_string(),
                        advice: "补齐号段空洞".to_string(),
                        severity: "blocker",
                    });
                }
            }
        }

        // ③b 禁止表形态自检：表项必须是裸签名（带 `F->Q:` 前缀会让红线永不触发）。
    for f in DELEGATION_FORBIDDEN.iter() {
        if has_arrow_prefix(f) {
            issues.push(ContractIssue {
                code: "CONTRACT_EMPTY",
                symptom: format!("禁止表项「{}」自带调用方前缀", f),
                cause: "表项带前缀而比对键也带前缀（或反之）——两侧形态不一致则红线永不触发，\
                        看上去有红线实则没有"
                    .to_string(),
                advice: "禁止表登记为裸签名，前缀由比对侧统一拼接".to_string(),
                severity: "blocker",
            });
        }
    }

    // ④ 边界表：域号段不重叠、每域 owns 与 must_not 非空。
        for d in DOMAIN_BOUNDARY_TABLE.iter() {
            if d.owns.is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("域 {} 的 owns 为空（挂名域）", d.id),
                    cause: "域不负责任何能力——它就是边界外的第二权威".to_string(),
                    advice: format!("为域 {} 登记至少一项能力", d.id),
                    severity: "blocker",
                });
            }
            if d.must_not.is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("域 {} 的 must_not 为空（无禁做项等于无边界）", d.id),
                    cause: "只写「我做什么」不写「我不做什么」，边界无法判定".to_string(),
                    advice: format!("为域 {} 补禁做项", d.id),
                    severity: "blocker",
                });
            }
            if d.hi < d.lo {
                issues.push(ContractIssue {
                    code: "CONTRACT_ORPHAN",
                    symptom: format!("域 {} 号段非法（{}–{}）", d.id, d.lo, d.hi),
                    cause: "域号段下界大于上界".to_string(),
                    advice: format!("修正域 {} 的号段", d.id),
                    severity: "blocker",
                });
            }
        }
        for i in 0..DOMAIN_BOUNDARY_TABLE.len() {
            for j in (i + 1)..DOMAIN_BOUNDARY_TABLE.len() {
                let a = &DOMAIN_BOUNDARY_TABLE[i];
                let b = &DOMAIN_BOUNDARY_TABLE[j];
                if a.lo <= b.hi && b.lo <= a.hi {
                    issues.push(ContractIssue {
                        code: "CONTRACT_ORPHAN",
                        symptom: format!(
                            "域 {} 与 {} 号段重叠（{}–{} vs {}–{}）",
                            a.id, b.id, a.lo, a.hi, b.lo, b.hi
                        ),
                        cause: "域号段重叠——归属歧义".to_string(),
                        advice: "重新划分域号段".to_string(),
                        severity: "blocker",
                    });
                }
                // 能力键全局唯一（一个能力两个属主 = 裁决不可判）。
                for ca in a.owns.iter() {
                    for cb in b.owns.iter() {
                        if ca.key == cb.key {
                            issues.push(ContractIssue {
                                code: "CONTRACT_DUP",
                                symptom: format!(
                                    "能力「{}」同时登记在 {} 与 {}（两个属主）",
                                    ca.key, a.id, b.id
                                ),
                                cause: "能力键未全局唯一——裁决结果依赖查表顺序".to_string(),
                                advice: format!("把能力「{}」收敛到唯一属主域", ca.key),
                                severity: "blocker",
                            });
                        }
                    }
                }
            }
        }

        // ⑤ 台账：每条协议有 handler 登记与无障碍替述。
        for e in UPSTREAM_LEDGER.iter() {
            if !HANDLER_REGISTRY.contains(&e.handler) {
                issues.push(ContractIssue {
                    code: "CONTRACT_MISSING",
                    symptom: format!("协议「{}」的 handler「{}」未登记", e.key, e.handler),
                    cause: "台账与实现脱节——退化为没人看的文档".to_string(),
                    advice: "在 HANDLER_REGISTRY 登记该 handler".to_string(),
                    severity: "blocker",
                });
            }
            if e.alt_text.trim().is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("协议「{}」缺无障碍替述", e.key),
                    cause: "错误时无播报口径——无障碍层失语".to_string(),
                    advice: "补 alt_text".to_string(),
                    severity: "blocker",
                });
            }
            if e.signature.trim().is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("协议「{}」缺签名", e.key),
                    cause: "无签名的协议无法对拍".to_string(),
                    advice: "补 signature".to_string(),
                    severity: "blocker",
                });
            }
        }

        // ⑥ 性能预算：自称 O(1) 者必须注明为何是常数级。
        for b in PERF_BUDGET.iter() {
            if b.complexity.contains("O(1)") && b.bounded_by.is_none() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("预算项「{}」自称 O(1) 但未注明上界来源", b.item),
                    cause: "把 O(n) 谎报为 O(1) 会让常量级断言失去意义".to_string(),
                    advice: "注明 bounded_by 或改报真实复杂度".to_string(),
                    severity: "warn",
                });
            }
            if b.note.trim().is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("预算项「{}」缺备注", b.item),
                    cause: "无备注的预算项在半年后无人能解释为何这么写".to_string(),
                    advice: format!("补「{}」的备注", b.item),
                    severity: "warn",
                });
            }
        }

        // ⑦ 门禁：五条判据齐备。
        if KICKOFF_GATES.len() != 5 {
            issues.push(ContractIssue {
                code: "CONTRACT_MISSING",
                symptom: format!("开工门禁在册 {} 条，应为 5 条", KICKOFF_GATES.len()),
                cause: "门禁条数与判据条数不一致".to_string(),
                advice: "补齐五条门禁".to_string(),
                severity: "blocker",
            });
        }
        for g in KICKOFF_GATES.iter() {
            if g.criterion.trim().is_empty() || g.maps_to.trim().is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("门禁 {} 有字段留空", g.id),
                    cause: "门禁字段留空——门禁无法判定".to_string(),
                    advice: format!("补全门禁 {}", g.id),
                    severity: "blocker",
                });
            }
        }

        issues
    }

    /// 无障碍：架构图的读屏替代（判据点名项）。
    ///
    /// 架构图对读屏用户是不可达的——本函数产出**线性文字版**：六段是什么、
    /// 数据怎么流、红线怎么守、失败了会怎样，全部念得出来。文字版与图版同源
    /// （同一份 [`STAGE_SPECS`] / [`RESOURCE_MAPPINGS`]），不另写一份以免漂移。
    pub fn architecture_narration(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "资源管线总架构 {}，覆盖功能号 {}–{}，共六段流水线。",
            self.version, DOMAIN_ITEM_LO, DOMAIN_ITEM_HI
        ));
        out.push_str(&format!("六段数据流顺序：{}。", stage_order_line()));
        out.push_str("逐段说明：");
        for s in STAGE_SPECS.iter() {
            out.push_str(&format!(
                "第 {} 段，{}，吃{}，吐{}；允许失败 {}；复杂度 {}；存在理由：{}。",
                s.ordinal,
                s.label(),
                s.inputs.join("+"),
                s.outputs.join("+"),
                s.allowed_failures
                    .iter()
                    .map(|c| c.code())
                    .collect::<Vec<_>>()
                    .join("、"),
                s.amortized,
                s.rationale
            ));
        }
        out.push_str("十项资源映射：");
        for m in RESOURCE_MAPPINGS.iter() {
            out.push_str(&format!("{}。", m.screen_line()));
        }
        out.push_str("十个能力主题号段：");
        for t in Q_DOMAIN_TEN_TOPICS.iter() {
            out.push_str(&format!("{}。", t.span().screen_line(*t)));
        }
        out.push_str("消费侧统一接口：");
        out.push_str(&format!("{}。", CONSUMER_INTERFACE));
        out.push_str("已登记消费域：");
        for d in ConsumerDomain::ALL.iter() {
            out.push_str(&format!("{}。", d.screen_line()));
        }
        out.push_str(&format!(
            "单向委托：允许 {}；禁止 {}。",
            DELEGATION_ALLOWED.join("、"),
            DELEGATION_FORBIDDEN.join("、")
        ));
        out.push_str(&format!(
            "开工门禁五条：{}。",
            KICKOFF_GATES
                .iter()
                .map(|g| format!("{} 对应判据「{}」：{}", g.id, g.maps_to, g.criterion))
                .collect::<Vec<_>>()
                .join("；")
        ));
        out.push_str(&format!("无障碍原则：{}。", A11Y_ERROR_PRINCIPLE));
        out.push_str(&format!("隐私声明：{}。", PRIVACY_NOTE));
        out.push_str(&format!(
            "远程授权现状：{}。",
            if self.remote_grants.list().is_empty() {
                "无（远程默认拒绝）".to_string()
            } else {
                self.remote_grants.list().join("、")
            }
        ));
        out
    }

    /// 读屏摘要（一行版）。
    pub fn screen_text(&self) -> String {
        format!(
            "Q 域管线 {}：六段 {}；十项映射 {} 类；消费域 {} 个；主题 {} 条；远程授权 {}",
            self.version,
            stage_order_line(),
            RESOURCE_MAPPINGS.len(),
            ConsumerDomain::ALL.len(),
            Q_DOMAIN_TEN_TOPICS.len(),
            self.remote_grants.list().len()
        )
    }
}

// ---------------------------------------------------------------------------
// 十五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F3201 域自检（判据逐条映射见 `veq01_checks.rs`）。
pub fn run_veq01_checks() -> crate::checks::CheckSet {
    super::veq01_checks::run_veq01_checks()
}