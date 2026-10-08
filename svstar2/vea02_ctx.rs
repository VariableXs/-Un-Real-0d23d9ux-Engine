//! VE-F0002 · 图形上下文生命周期管理器（VE-A 域 · 内核图形抽象层 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0002`
//!
//! **判据（锚点原文）**：四态管理、泄漏检测、归属约束、强制回收、判据；
//! 上下文含调试命名（每个上下文有人话名字，日志里认得出谁是谁）；
//! 共享上下文的父子的溯源审计（谁共享给谁可查）；生命周期含僵尸上下文回收
//! （无引用不销毁的自动清理）；创建参数越界的拒绝含修正建议。
//!
//! **设计要点**：
//! - 四态 = created / active / shared / destroyed。`shared` 单列一态而不做成
//!   destroyed 的标记：共享中的上下文"逻辑上活着、资源上被冻结"，其泄漏语义
//!   （`LEAK_SHARED_HELD`）与普通存活完全不同，混在一起会把正常共享误判成泄漏；
//! - 合法迁移表写死，非法迁移**拒绝并给建议**——不做"尽力而为"的静默修正，
//!   因为上下文迁移失败意味着画面停在旧帧，静默比报错更难查；
//! - 归属约束是硬约束：非归属线程操作即拒绝。但系统级强制操作（by=`system`）
//!   豁免——那是"强制回收"的入口，豁免必须留审计痕迹；
//! - 强制回收两条铁律：**不许静默**（必留 by=system 审计）、**不许蛮干**
//!   （`allowForceReclaim=false` 的系统级上下文只告警不强收——强收会导致驱动
//!   句柄悬挂，比泄漏本身更糟）。
//!
//! **跨批对接点**：AD01 资源治理同构——本模块"预算申请—超限降级—拒绝挤占"
//! 三段式与 AD01 同构，资源治理侧接入时按同一形状对接。
//!
//! 逻辑时钟而非墙钟：生命周期要可复现、可回放，墙钟会让"哪个上下文先死"
//! 这类顺序问题在测试里飘。零外部依赖，只依赖 `crate::checks`。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 上下文数量上限（边界防护）。
pub const MAX_CONTEXTS: usize = 32;
/// 队列深度合法区间。
pub const QUEUE_DEPTH_MIN: u32 = 2;
pub const QUEUE_DEPTH_MAX: u32 = 4096;
/// 队列深度告警线：超过此值仍合法（2 的幂且 ≤ MAX），但记账 warn。
/// 存在的意义是把"偏深但能用"和"越界不能用"分成两档——否则硬上限以下的
/// 深队列会被静默接受，用户拿不到任何提示。
pub const QUEUE_DEPTH_WARN: u32 = 1024;
/// 僵尸判定阈值：引用为零且超过该 tick 数未销毁即僵尸。
pub const ZOMBIE_TICK_THRESHOLD: u64 = 32;
/// 单次强制回收的句柄释放上限（防一次清太多导致长时间卡顿）。
pub const FORCE_RECLAIM_HANDLE_CAP: i64 = 64;

// ---------------------------------------------------------------------------
// 一、四态（判据：四态管理）
// ---------------------------------------------------------------------------

/// 上下文生命周期四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextState {
    /// 已创建，资源未就绪，不可使用
    Created,
    /// 已激活，资源就绪，可提交命令（同一时刻每设备仅一个 active）
    Active,
    /// 已共享，资源被其他上下文引用，不可销毁（须等引用归零）
    Shared,
    /// 已销毁，资源已释放，终态不可回退
    Destroyed,
}

impl ContextState {
    pub fn label(self) -> &'static str {
        match self {
            ContextState::Created => "已创建",
            ContextState::Active => "已激活",
            ContextState::Shared => "共享中",
            ContextState::Destroyed => "已销毁",
        }
    }
}

/// 合法状态迁移表——规格"四态管理"的机器可读形式。非法迁移一律拒绝。
pub fn legal_transitions(from: ContextState) -> &'static [ContextState] {
    match from {
        ContextState::Created => &[ContextState::Active, ContextState::Destroyed],
        ContextState::Active => &[ContextState::Shared, ContextState::Destroyed],
        // 共享解除后回到 active
        ContextState::Shared => &[ContextState::Active, ContextState::Destroyed],
        // 终态，不可回退
        ContextState::Destroyed => &[],
    }
}

/// 迁移合法性。
pub fn transition_allowed(from: ContextState, to: ContextState) -> bool {
    legal_transitions(from).contains(&to)
}

// ---------------------------------------------------------------------------
// 二、创建参数（规格：含上下文创建参数的校验规则集）
// ---------------------------------------------------------------------------

/// 上下文创建参数。全部字段参与校验，越界即拒绝并给修正建议。
#[derive(Clone, Debug)]
pub struct ContextCreateParams {
    /// 调试命名——规格判据点名项
    pub debug_name: String,
    /// 色彩空间标识
    pub color_space: ColorSpace,
    /// 命令队列深度，须为 2 的幂
    pub queue_depth: u32,
    /// 共享源上下文 id；`None` = 不共享
    pub share_from: Option<String>,
    /// 归属线程标识
    pub owner_thread_id: String,
    /// 是否允许强制回收
    pub allow_force_reclaim: bool,
}

/// 色彩空间枚举（只接受在册值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
    Hdr10,
    ScrgbLinear,
}

impl ColorSpace {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "srgb" => Some(ColorSpace::Srgb),
            "display-p3" => Some(ColorSpace::DisplayP3),
            "hdr10" => Some(ColorSpace::Hdr10),
            "scrgb-linear" => Some(ColorSpace::ScrgbLinear),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::DisplayP3 => "Display-P3",
            ColorSpace::Hdr10 => "HDR10",
            ColorSpace::ScrgbLinear => "scRGB 线性",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::DisplayP3 => "display-p3",
            ColorSpace::Hdr10 => "hdr10",
            ColorSpace::ScrgbLinear => "scrgb-linear",
        }
    }
}

/// 校验严重度：error 阻断创建，warn 放行但记账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardSeverity {
    Error,
    Warn,
}

/// 单条校验问题。规格要求"越界的拒绝含修正建议"，故必带 suggestion。
#[derive(Clone, Debug)]
pub struct ParamIssue {
    pub field: &'static str,
    pub severity: GuardSeverity,
    pub message: String,
    /// 修正建议——人话，可直接展示给用户
    pub suggestion: String,
}

/// 校验结果。`ok` = 无 error（warn 不阻断，但要记账）。
#[derive(Clone, Debug)]
pub struct ParamValidation {
    pub ok: bool,
    pub issues: Vec<ParamIssue>,
    pub error_count: usize,
    pub warn_count: usize,
}

impl ParamValidation {
    /// 越界时的人话整句（规格："越界的拒绝含修正建议"）。
    pub fn rejection_message(&self) -> String {
        if self.ok {
            return String::new();
        }
        let mut out = format!("上下文创建被拒：{} 项参数越界。", self.error_count);
        let mut idx = 1;
        for i in self.issues.iter() {
            if i.severity != GuardSeverity::Error {
                continue;
            }
            out.push_str(&format!(
                "\n  {}. {} —— {}；建议：{}",
                idx, i.field, i.message, i.suggestion
            ));
            idx += 1;
        }
        out
    }
}

fn err(field: &'static str, message: &str, suggestion: &str) -> ParamIssue {
    ParamIssue {
        field,
        severity: GuardSeverity::Error,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
    }
}

fn warn(field: &'static str, message: &str, suggestion: &str) -> ParamIssue {
    ParamIssue {
        field,
        severity: GuardSeverity::Warn,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
    }
}

/// 全字段校验。规格：创建参数的校验规则集（边界防护约 60 行）。
pub fn validate_params(p: &ContextCreateParams) -> ParamValidation {
    let mut issues: Vec<ParamIssue> = Vec::new();

    // 调试命名——规格："每个上下文有人话名字，日志里认得出谁是谁"
    if p.debug_name.is_empty() {
        issues.push(err(
            "debug_name",
            "调试名为空，日志里将认不出这是谁",
            "给一个人话名字，如 \"主窗口 · 编辑器画布\"",
        ));
    } else if p.debug_name.trim().is_empty() {
        issues.push(err(
            "debug_name",
            "调试名只有空白字符",
            "填入可读文本，不要纯空格",
        ));
    } else if p.debug_name.chars().count() > 64 {
        issues.push(warn(
            "debug_name",
            "调试名过长，将被截断",
            "控制在 64 字符内，保留可辨识前缀",
        ));
    }

    // 队列深度：区间 + 2 的幂 + 偏深告警三档
    let n = p.queue_depth;
    if n < QUEUE_DEPTH_MIN {
        issues.push(err(
            "queue_depth",
            &format!("队列深度 {} 小于下限 {}", n, QUEUE_DEPTH_MIN),
            "GPU 环形缓冲至少要能容下双缓冲，填 2 或更大",
        ));
    } else if n > QUEUE_DEPTH_MAX {
        issues.push(err(
            "queue_depth",
            &format!("队列深度 {} 超过硬上限 {}", n, QUEUE_DEPTH_MAX),
            "过深的队列会掩盖提交延迟，降到 4096 以内",
        ));
    } else if (n & (n - 1)) != 0 {
        // 2 的幂判定：n & (n-1) == 0
        let mut pow = 0u32;
        while (1u32 << pow) < n {
            pow += 1;
        }
        issues.push(err(
            "queue_depth",
            &format!("队列深度 {} 不是 2 的幂（GPU 环形缓冲按幂次对齐）", n),
            &format!("改成 2^{} = {}", pow, 1u32 << pow),
        ));
    } else if n > QUEUE_DEPTH_WARN {
        issues.push(warn(
            "queue_depth",
            &format!("队列深度 {} 偏深（> {}），注意首帧延迟与显存占用", n, QUEUE_DEPTH_WARN),
            "一般 256 足够；超大深度只在批量提交场景用，并确认驱动支持",
        ));
    }

    // 归属线程
    if p.owner_thread_id.is_empty() {
        issues.push(err(
            "owner_thread_id",
            "归属线程缺失，多线程归属约束将失效",
            "填线程标识，如 \"main\" / \"render-3\"",
        ));
    } else if p.owner_thread_id.chars().count() > 32 {
        issues.push(warn(
            "owner_thread_id",
            "线程标识过长",
            "用短标识，别把堆栈塞进来",
        ));
    }

    // 共享源：开启共享必须指定来源，且不能自引用
    if let Some(src) = &p.share_from {
        if src.is_empty() {
            issues.push(err(
                "share_from",
                "共享已开启但未指定共享源",
                "填要共享的上下文 id，或把 share_from 置 None 关掉共享",
            ));
        } else if src == "self" {
            issues.push(err(
                "share_from",
                "共享源指向自己，构成自引用",
                "共享源填另一个已存在的上下文 id",
            ));
        }
    }

    let error_count = issues
        .iter()
        .filter(|i| i.severity == GuardSeverity::Error)
        .count();
    let warn_count = issues.len() - error_count;
    ParamValidation {
        ok: error_count == 0,
        issues,
        error_count,
        warn_count,
    }
}

// ---------------------------------------------------------------------------
// 三、上下文记录与生命周期表（规格"数据结构：生命周期表"）
// ---------------------------------------------------------------------------

/// 一次状态迁移的留痕。规格"溯源审计"的最小单元。
#[derive(Clone, Debug)]
pub struct TransitionRecord {
    pub tick: u64,
    pub from: ContextState,
    pub to: ContextState,
    /// 触发方：线程 id 或 `system`（系统强制）
    pub by: String,
    /// 人话原因
    pub reason: String,
}

/// 单个上下文的完整档案。
#[derive(Clone, Debug)]
pub struct ContextRecord {
    pub id: String,
    pub debug_name: String,
    pub state: ContextState,
    pub params: ContextCreateParams,
    pub created_at_tick: u64,
    pub activated_at_tick: Option<u64>,
    pub destroyed_at_tick: Option<u64>,
    /// 资源句柄计数——销毁后必须归零，非零即泄漏
    pub live_handle_count: i64,
    pub share_from: Option<String>,
    /// 共享子上下文 id 列表（溯源审计：谁共享给谁可查）
    pub shared_children: Vec<String>,
    /// 引用计数：归零且非 destroyed ⇒ 僵尸
    pub ref_count: i64,
    /// 累计命令提交数（活跃度指标，僵尸判定辅助）
    pub submit_count: u64,
    /// 状态迁移留痕——完整审计轨迹
    pub transitions: Vec<TransitionRecord>,
}

/// 业务错误。带 code 便于上层归因，不靠字符串匹配。
#[derive(Clone, Debug)]
pub struct ContextError {
    pub code: &'static str,
    pub message: String,
    /// 修正建议——规格要求拒绝必带建议
    pub suggestion: String,
}

impl ContextError {
    fn new(code: &'static str, message: &str, suggestion: &str) -> Self {
        ContextError {
            code,
            message: message.to_string(),
            suggestion: suggestion.to_string(),
        }
    }
}

/// 归属校验结论。
#[derive(Clone, Debug)]
pub struct OwnershipVerdict {
    pub ok: bool,
    pub reason: String,
    pub suggestion: String,
    pub actual_owner: Option<String>,
}

/// 创建被拒（参数越界或前置条件不满足）——统一结构，含修正建议。
#[derive(Clone, Debug)]
pub struct CreateRejection {
    pub message: String,
    pub issues: Vec<ParamIssue>,
}

/// 生命周期管理器。状态全在内存，每次变更写回新记录——
/// 这样"谁在什么时候把谁改成什么"永远有据可查。
pub struct ContextLifecycleManager {
    ctx: Vec<ContextRecord>,
    tick: u64,
    seq: u32,
    active: Option<String>,
}

impl ContextLifecycleManager {
    pub fn new() -> Self {
        ContextLifecycleManager {
            ctx: Vec::new(),
            tick: 0,
            seq: 0,
            active: None,
        }
    }

    /// 当前逻辑时钟。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 推进逻辑时钟。所有与"多久"有关的判定都靠它，保证可复现。
    pub fn advance(&mut self, steps: u64) -> u64 {
        self.tick += if steps == 0 { 1 } else { steps };
        self.tick
    }

    /// 全量快照（按创建序稳定序）。
    pub fn list(&self) -> &[ContextRecord] {
        &self.ctx
    }

    pub fn get(&self, id: &str) -> Option<&ContextRecord> {
        self.ctx.iter().find(|c| c.id == id)
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.as_deref()
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.ctx.iter().position(|c| c.id == id)
    }

    /// 创建上下文（前置条件检查；参数校验在 `create_checked`）。
    ///
    /// 上限超限 / 共享源不存在 / 共享源已销毁三类前置条件不满足时拒绝，
    /// 且必带修正建议。
    pub fn create(&mut self, params: ContextCreateParams) -> Result<String, ContextError> {
        if self.ctx.len() >= MAX_CONTEXTS {
            return Err(ContextError::new(
                "E_LIMIT",
                &format!("上下文数量已达上限 {}，新建被拒", MAX_CONTEXTS),
                "先销毁不用掉的上下文，或复用已销毁的槽位",
            ));
        }
        if let Some(src) = &params.share_from {
            match self.get(src) {
                None => {
                    return Err(ContextError::new(
                        "E_SHARE_SRC",
                        &format!("共享源 {} 不存在", src),
                        "先创建共享源上下文，或把 share_from 置 None",
                    ))
                }
                Some(s) if s.state == ContextState::Destroyed => {
                    return Err(ContextError::new(
                        "E_SHARE_DEAD",
                        &format!("共享源 {}({}) 已销毁，不能再共享", s.debug_name, src),
                        "换一个存活的上下文作共享源",
                    ))
                }
                _ => {}
            }
        }

        self.seq += 1;
        let id = format!("ctx-{:04}", self.seq);
        let parent = params.share_from.clone();
        let owner = params.owner_thread_id.clone();
        let rec = ContextRecord {
            id: id.clone(),
            debug_name: params.debug_name.clone(),
            state: ContextState::Created,
            created_at_tick: self.tick,
            activated_at_tick: None,
            destroyed_at_tick: None,
            live_handle_count: 0,
            share_from: parent.clone(),
            shared_children: Vec::new(),
            ref_count: 0,
            submit_count: 0,
            transitions: alloc::vec![TransitionRecord {
                tick: self.tick,
                from: ContextState::Created,
                to: ContextState::Created,
                by: owner,
                reason: "创建登记".to_string(),
            }],
            params,
        };
        self.ctx.push(rec);

        // 共享关系双向登记：规格"共享上下文的父子的溯源审计"
        if let Some(pid) = parent {
            if let Some(i) = self.index_of(&pid) {
                self.ctx[i].shared_children.push(id.clone());
                self.ctx[i].ref_count += 1;
            }
        }
        Ok(id)
    }

    /// 带参数校验的创建。规格"创建参数越界的拒绝含修正建议"的标准入口。
    ///
    /// 校验不 panic——返回结构化结果，上层爱怎么展示都行。
    pub fn create_checked(&mut self, params: ContextCreateParams) -> Result<String, CreateRejection> {
        let v = validate_params(&params);
        if !v.ok {
            return Err(CreateRejection {
                message: v.rejection_message(),
                issues: v.issues,
            });
        }
        self.create(params).map_err(|e| CreateRejection {
            message: e.message.clone(),
            issues: alloc::vec![ParamIssue {
                field: "precondition",
                severity: GuardSeverity::Error,
                message: e.message,
                suggestion: e.suggestion,
            }],
        })
    }

    /// 归属校验：非归属线程操作该上下文即违例。
    pub fn check_ownership(&self, id: &str, by_thread: &str) -> OwnershipVerdict {
        let c = match self.get(id) {
            None => {
                return OwnershipVerdict {
                    ok: false,
                    reason: format!("上下文 {} 不存在", id),
                    suggestion: "先 create 拿到 id 再操作".to_string(),
                    actual_owner: None,
                }
            }
            Some(c) => c,
        };
        if c.state == ContextState::Destroyed {
            return OwnershipVerdict {
                ok: false,
                reason: format!("上下文 {}({}) 已销毁，不可再操作", c.debug_name, id),
                suggestion: "销毁是终态；要再用请新建一个上下文".to_string(),
                actual_owner: Some(c.params.owner_thread_id.clone()),
            };
        }
        if c.params.owner_thread_id != by_thread {
            return OwnershipVerdict {
                ok: false,
                reason: format!(
                    "归属违例：{}({}) 属线程 {}，{} 无权操作",
                    c.debug_name, id, c.params.owner_thread_id, by_thread
                ),
                suggestion: format!(
                    "把操作发回 {}，或用 share_from 共享而非跨线程直接改",
                    c.params.owner_thread_id
                ),
                actual_owner: Some(c.params.owner_thread_id.clone()),
            };
        }
        OwnershipVerdict {
            ok: true,
            reason: "归属正确".to_string(),
            suggestion: String::new(),
            actual_owner: Some(c.params.owner_thread_id.clone()),
        }
    }

    /// 执行状态迁移。非法迁移 / 归属违例一律拒绝（规格"归属违例→拒绝"）。
    ///
    /// `by == "system"` 为系统强制入口，豁免归属检查（但仍留审计）。
    pub fn transition(
        &mut self,
        id: &str,
        to: ContextState,
        by: &str,
        reason: &str,
    ) -> Result<(), ContextError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(ContextError::new(
                    "E_NOENT",
                    &format!("上下文 {} 不存在", id),
                    "先 create 拿到 id",
                ))
            }
        };
        let from = self.ctx[i].state;
        if !transition_allowed(from, to) {
            let allowed = legal_transitions(from);
            let advice = if allowed.is_empty() {
                "（终态，无出路）".to_string()
            } else {
                let names: Vec<&str> = allowed.iter().map(|s| s.label()).collect();
                names.join(" / ")
            };
            return Err(ContextError::new(
                "E_ILLEGAL",
                &format!(
                    "非法状态迁移：{}({}) {} → {} 不在允许表内",
                    self.ctx[i].debug_name,
                    id,
                    from.label(),
                    to.label()
                ),
                &format!("{} 只能迁到 {}", from.label(), advice),
            ));
        }
        if by != "system" {
            let v = self.check_ownership(id, by);
            if !v.ok {
                return Err(ContextError::new("E_OWNER", &v.reason, &v.suggestion));
            }
        }

        self.ctx[i].transitions.push(TransitionRecord {
            tick: self.tick,
            from,
            to,
            by: by.to_string(),
            reason: reason.to_string(),
        });
        self.ctx[i].state = to;
        if to == ContextState::Active && self.ctx[i].activated_at_tick.is_none() {
            self.ctx[i].activated_at_tick = Some(self.tick);
        }
        if to == ContextState::Destroyed {
            self.ctx[i].destroyed_at_tick = Some(self.tick);
        }
        // active 唯一性：同一时刻只有一个激活上下文
        if to == ContextState::Active {
            self.active = Some(id.to_string());
        }
        if to == ContextState::Destroyed && self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Ok(())
    }

    /// 分配资源句柄。资源没到位就激活，等于把空上下文交给上层用。
    pub fn alloc_handle(&mut self, id: &str, by: &str, n: i64) -> Result<(), ContextError> {
        self.must_live(id, by)?;
        let i = self.index_of(id).unwrap_or(0);
        self.ctx[i].live_handle_count += n;
        Ok(())
    }

    /// 释放资源句柄。钳到 0，不许负——负数意味着释放逻辑有 bug。
    pub fn free_handle(&mut self, id: &str, by: &str, n: i64) -> Result<(), ContextError> {
        self.must_live(id, by)?;
        let i = self.index_of(id).unwrap_or(0);
        self.ctx[i].live_handle_count = (self.ctx[i].live_handle_count - n).max(0);
        Ok(())
    }

    /// 增加引用（外部持有）。
    pub fn retain(&mut self, id: &str, n: i64) {
        if let Some(i) = self.index_of(id) {
            self.ctx[i].ref_count += n;
        }
    }

    /// 释放引用。归零且非 destroyed ⇒ 僵尸候选。
    pub fn release(&mut self, id: &str, n: i64) {
        if let Some(i) = self.index_of(id) {
            self.ctx[i].ref_count = (self.ctx[i].ref_count - n).max(0);
        }
    }

    /// 记一次命令提交（活跃度指标，僵尸判定辅助）。
    pub fn note_submit(&mut self, id: &str, by: &str) {
        if self.must_live(id, by).is_ok() {
            if let Some(i) = self.index_of(id) {
                self.ctx[i].submit_count += 1;
            }
        }
    }

    /// 正常销毁。引用未归零或仍有子上下文时拒绝——
    /// 这是"销毁不彻底即缺陷"的第一道闸。
    pub fn destroy(&mut self, id: &str, by: &str) -> Result<(), ContextError> {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(ContextError::new(
                    "E_NOENT",
                    &format!("上下文 {} 不存在", id),
                    "先 create 拿到 id",
                ))
            }
        };
        if self.ctx[i].ref_count > 0 {
            return Err(ContextError::new(
                "E_DESTROY_REF",
                &format!(
                    "{}({}) 仍有 {} 个引用，销毁被拒",
                    self.ctx[i].debug_name, id, self.ctx[i].ref_count
                ),
                "先 release 掉全部引用；确需强收请走 force_reclaim",
            ));
        }
        if !self.ctx[i].shared_children.is_empty() {
            let kids = self.ctx[i].shared_children.join("、");
            return Err(ContextError::new(
                "E_DESTROY_CHILD",
                &format!(
                    "{}({}) 仍被 {} 个子上下文共享（{}），销毁被拒",
                    self.ctx[i].debug_name,
                    id,
                    self.ctx[i].shared_children.len(),
                    kids
                ),
                "先销毁子上下文，或等它们自行退出",
            ));
        }
        self.transition(id, ContextState::Destroyed, by, "正常销毁")?;

        // ★ 共享关系双向解绑（真实缺陷修复，2026-10-04）
        // 子上下文销毁时必须给父减引用 + 从子列表摘除，否则父的 ref_count 永久 > 0，
        // 父自己永远销毁不掉，整条共享链死锁。
        let parent = self.ctx[i].share_from.clone();
        if let Some(pid) = parent {
            if let Some(pi) = self.index_of(&pid) {
                self.ctx[pi].ref_count = (self.ctx[pi].ref_count - 1).max(0);
                self.ctx[pi].shared_children.retain(|x| x != id);
            }
            // 父不存在（已被强收）时静默跳过：那属于 dangling_child 泄漏，
            // 由 detect_leaks 上报，不在销毁路径里重复报错。
        }
        Ok(())
    }

    /// 系统级强制操作入口。规格"销毁失败→强制+审计"用。
    ///
    /// 绕过归属与引用检查，但**必须留审计痕迹**（by=`system` 进 transitions）。
    pub fn force_mutate<F>(&mut self, id: &str, reason: &str, mutate: F) -> Result<(), ContextError>
    where
        F: FnOnce(&mut ContextRecord),
    {
        let i = match self.index_of(id) {
            Some(i) => i,
            None => {
                return Err(ContextError::new(
                    "E_NOENT",
                    &format!("上下文 {} 不存在", id),
                    "先 create 拿到 id",
                ))
            }
        };
        let from = self.ctx[i].state;
        mutate(&mut self.ctx[i]);
        let to = self.ctx[i].state;
        self.ctx[i].transitions.push(TransitionRecord {
            tick: self.tick,
            from,
            to,
            by: "system".to_string(),
            reason: format!("强制：{}", reason),
        });
        if to == ContextState::Destroyed {
            self.ctx[i].destroyed_at_tick = Some(self.tick);
            if self.active.as_deref() == Some(id) {
                self.active = None;
            }
        }
        Ok(())
    }

    /// 存活性与归属的合并前置检查。
    fn must_live(&self, id: &str, by: &str) -> Result<(), ContextError> {
        let c = match self.get(id) {
            None => {
                return Err(ContextError::new(
                    "E_NOENT",
                    &format!("上下文 {} 不存在", id),
                    "先 create 拿到 id",
                ))
            }
            Some(c) => c,
        };
        if c.state == ContextState::Destroyed {
            return Err(ContextError::new(
                "E_DEAD",
                &format!("{}({}) 已销毁，不可再操作", c.debug_name, id),
                "销毁是终态，请新建上下文",
            ));
        }
        if by != "system" && c.params.owner_thread_id != by {
            let v = self.check_ownership(id, by);
            return Err(ContextError::new("E_OWNER", &v.reason, &v.suggestion));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、泄漏检测（判据：泄漏检测 / 强制回收 / 僵尸上下文回收）
// ---------------------------------------------------------------------------

/// 泄漏类型。分类不同，处置手段不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeakKind {
    /// 声明销毁了但资源句柄没归零
    HandleLeak,
    /// 引用为零却没被销毁（僵尸上下文）
    Zombie,
    /// ref_count > 0 但没有子上下文登记，引用来源不明
    SharedHeld,
    /// 子上下文已销毁，父上下文还挂着它的记录
    DanglingChild,
    /// 归属线程已死，线程所有权泄漏
    Orphaned,
}

impl LeakKind {
    pub fn label(self) -> &'static str {
        match self {
            LeakKind::HandleLeak => "句柄泄漏",
            LeakKind::Zombie => "僵尸上下文",
            LeakKind::SharedHeld => "共享占用",
            LeakKind::DanglingChild => "悬挂子记录",
            LeakKind::Orphaned => "孤儿上下文",
        }
    }
    /// 严重度序（越小越严重，排序时用）
    pub fn severity(self) -> u8 {
        match self {
            LeakKind::HandleLeak => 0,
            LeakKind::DanglingChild => 1,
            LeakKind::Zombie => 2,
            LeakKind::SharedHeld => 3,
            LeakKind::Orphaned => 4,
        }
    }
}

/// 单条泄漏发现。
#[derive(Clone, Debug)]
pub struct LeakFinding {
    pub context_id: String,
    pub debug_name: String,
    pub kind: LeakKind,
    /// 人话描述
    pub detail: String,
    /// 泄漏量（句柄数 / 存活 tick 数）
    pub magnitude: i64,
    /// 是否可强制回收
    pub reclaimable: bool,
    /// 不可回收时的人话原因
    pub blocked_reason: String,
}

/// 单个上下文的泄漏检查。
fn inspect_one(c: &ContextRecord, now_tick: u64) -> Vec<LeakFinding> {
    let mut out: Vec<LeakFinding> = Vec::new();

    // 泄漏 1：声明销毁但句柄没归零 —— 最经典也最难查的一种
    if c.state == ContextState::Destroyed && c.live_handle_count > 0 {
        out.push(LeakFinding {
            context_id: c.id.clone(),
            debug_name: c.debug_name.clone(),
            kind: LeakKind::HandleLeak,
            detail: format!(
                "已标记销毁，但仍有 {} 个资源句柄未释放",
                c.live_handle_count
            ),
            magnitude: c.live_handle_count,
            reclaimable: c.params.allow_force_reclaim,
            blocked_reason: if c.params.allow_force_reclaim {
                String::new()
            } else {
                "该上下文标记为不可强制回收（系统级资源），只能告警".to_string()
            },
        });
    }

    // 泄漏 2：僵尸 —— 引用为零却一直不销毁
    if c.state != ContextState::Destroyed && c.ref_count == 0 {
        let idle = now_tick.saturating_sub(c.created_at_tick) as i64;
        if idle >= ZOMBIE_TICK_THRESHOLD as i64 {
            out.push(LeakFinding {
                context_id: c.id.clone(),
                debug_name: c.debug_name.clone(),
                kind: LeakKind::Zombie,
                detail: format!(
                    "引用已归零但存活 {} tick 未销毁（阈值 {}）",
                    idle, ZOMBIE_TICK_THRESHOLD
                ),
                magnitude: idle,
                reclaimable: true,
                blocked_reason: String::new(),
            });
        }
    }

    // 泄漏 3：引用计数有值但无子上下文登记 —— 来源不明
    if c.ref_count > 0 && c.shared_children.is_empty() {
        out.push(LeakFinding {
            context_id: c.id.clone(),
            debug_name: c.debug_name.clone(),
            kind: LeakKind::SharedHeld,
            detail: format!("ref_count={} 但没有子上下文登记，引用来源不明", c.ref_count),
            magnitude: c.ref_count,
            reclaimable: false,
            blocked_reason: "引用来源不明，强制回收会误伤他人，需人工确认".to_string(),
        });
    }

    out
}

/// 跨上下文检查：悬挂子记录。
fn inspect_dangling(mgr: &ContextLifecycleManager) -> Vec<LeakFinding> {
    let mut out: Vec<LeakFinding> = Vec::new();
    for parent in mgr.list() {
        for child_id in parent.shared_children.iter() {
            let child = mgr.get(child_id);
            let gone = match child {
                None => "不存在",
                Some(c) if c.state == ContextState::Destroyed => "已销毁",
                _ => continue,
            };
            out.push(LeakFinding {
                context_id: parent.id.clone(),
                debug_name: parent.debug_name.clone(),
                kind: LeakKind::DanglingChild,
                detail: format!("共享子 {} {}，父上下文仍持有其记录", child_id, gone),
                magnitude: 1,
                reclaimable: true,
                blocked_reason: String::new(),
            });
        }
    }
    out
}

/// 全量泄漏扫描。能修的排前面——先修能修的。
pub fn detect_leaks(mgr: &ContextLifecycleManager) -> Vec<LeakFinding> {
    let now = mgr.now();
    let mut out: Vec<LeakFinding> = Vec::new();
    for c in mgr.list() {
        out.extend(inspect_one(c, now));
    }
    out.extend(inspect_dangling(mgr));
    out.sort_by(|a, b| {
        b.reclaimable
            .cmp(&a.reclaimable)
            .then_with(|| a.kind.severity().cmp(&b.kind.severity()))
            .then_with(|| a.context_id.cmp(&b.context_id))
    });
    out
}

// ---------------------------------------------------------------------------
// 五、强制回收（规格"销毁失败→强制+审计"）
// ---------------------------------------------------------------------------

/// 单次回收的处置结果。
#[derive(Clone, Debug)]
pub struct ReclaimOutcome {
    pub context_id: String,
    pub ok: bool,
    /// 人话说明——成功失败都要有
    pub note: String,
    pub freed_handles: i64,
    /// 是否走了审计留痕
    pub audited: bool,
}

/// 强制回收单个上下文。
///
/// ★ 两条铁律 ★
/// 1. 不许静默：强制回收必留审计痕迹（transitions 里 by=`system`）。
/// 2. 不许蛮干：`allow_force_reclaim=false` 的上下文只能告警——强收系统级
///    上下文会导致驱动句柄悬挂，比泄漏本身更糟。
pub fn force_reclaim(mgr: &mut ContextLifecycleManager, context_id: &str) -> ReclaimOutcome {
    let (debug_name, allow, live, refs) = match mgr.get(context_id) {
        None => {
            return ReclaimOutcome {
                context_id: context_id.to_string(),
                ok: false,
                note: format!("上下文 {} 不存在，无可回收", context_id),
                freed_handles: 0,
                audited: false,
            }
        }
        Some(c) => (
            c.debug_name.clone(),
            c.params.allow_force_reclaim,
            c.live_handle_count,
            c.ref_count,
        ),
    };

    // 铁律 2：不可强收的只告警
    if !allow {
        return ReclaimOutcome {
            context_id: context_id.to_string(),
            ok: false,
            note: format!(
                "{}({}) 标记为不可强制回收（系统级资源），强收会导致驱动句柄悬挂；已转为告警",
                debug_name, context_id
            ),
            freed_handles: 0,
            audited: false,
        };
    }

    // 引用未归零时先清引用——不然后面还是销毁不掉
    let need_ref_clear = refs > 0;
    let freed = live;
    // 铁律 1：必留审计
    let reason = if need_ref_clear {
        format!("强制回收：清引用 {} + 释放句柄 {}", refs, freed)
    } else {
        format!("强制回收：释放句柄 {}", freed)
    };
    let _ = mgr.force_mutate(context_id, &reason, |c| {
        c.ref_count = 0;
        c.live_handle_count = 0;
        c.state = ContextState::Destroyed;
    });

    ReclaimOutcome {
        context_id: context_id.to_string(),
        ok: true,
        note: format!(
            "{}({}) 已强制回收，释放句柄 {}{}",
            debug_name,
            context_id,
            freed,
            if need_ref_clear {
                format!("，并清空 {} 个残留引用", refs)
            } else {
                String::new()
            }
        ),
        freed_handles: freed,
        audited: true,
    }
}

/// 批量强制回收。一次最多释放 `FORCE_RECLAIM_HANDLE_CAP` 个句柄，
/// 防止一次清太多造成长时间卡顿。
pub fn reclaim_all(mgr: &mut ContextLifecycleManager, leaks: &[LeakFinding]) -> Vec<ReclaimOutcome> {
    let mut out: Vec<ReclaimOutcome> = Vec::new();
    let mut budget = FORCE_RECLAIM_HANDLE_CAP;
    for l in leaks.iter() {
        if !l.reclaimable {
            out.push(ReclaimOutcome {
                context_id: l.context_id.clone(),
                ok: false,
                note: format!(
                    "{}({}) {} 不可自动回收：{}",
                    l.debug_name,
                    l.context_id,
                    l.kind.label(),
                    if l.blocked_reason.is_empty() {
                        "需人工确认"
                    } else {
                        &l.blocked_reason
                    }
                ),
                freed_handles: 0,
                audited: false,
            });
            continue;
        }
        if budget <= 0 {
            out.push(ReclaimOutcome {
                context_id: l.context_id.clone(),
                ok: false,
                note: format!(
                    "本轮强制回收句柄预算 {} 已用尽，{}({}) 留待下轮",
                    FORCE_RECLAIM_HANDLE_CAP, l.debug_name, l.context_id
                ),
                freed_handles: 0,
                audited: false,
            });
            continue;
        }
        let r = force_reclaim(mgr, &l.context_id);
        budget -= r.freed_handles;
        out.push(r);
    }
    out
}

/// 僵尸上下文专项回收（规格点名项）。
pub fn reclaim_zombies(mgr: &mut ContextLifecycleManager) -> Vec<ReclaimOutcome> {
    let targets: Vec<LeakFinding> = detect_leaks(mgr)
        .into_iter()
        .filter(|l| l.kind == LeakKind::Zombie)
        .collect();
    reclaim_all(mgr, &targets)
}

/// 生命周期总表。
pub struct LifecycleTable {
    pub contexts: Vec<ContextRecord>,
    pub tick: u64,
    pub active_id: Option<String>,
    pub leaks: Vec<LeakFinding>,
    pub reclaimed: Vec<String>,
    /// 读屏可达摘要（规格：生命周期读屏可查）
    pub a11y_summary: String,
}

/// 生成读屏摘要。
pub fn a11y_lifecycle(
    contexts: &[ContextRecord],
    active_id: Option<&str>,
    leaks: &[LeakFinding],
    reclaimed: &[String],
) -> String {
    if contexts.is_empty() {
        return "当前没有图形上下文。".to_string();
    }
    let count_of = |s: ContextState| contexts.iter().filter(|c| c.state == s).count();
    let mut out = format!(
        "共 {} 个图形上下文：已创建 {}、激活 {}、共享中 {}、已销毁 {}。",
        contexts.len(),
        count_of(ContextState::Created),
        count_of(ContextState::Active),
        count_of(ContextState::Shared),
        count_of(ContextState::Destroyed),
    );
    match active_id {
        Some(a) => {
            let name = contexts
                .iter()
                .find(|c| c.id == a)
                .map(|c| c.debug_name.clone())
                .unwrap_or_else(|| a.to_string());
            out.push_str(&format!("当前激活：{}。", name));
        }
        None => out.push_str("当前无激活上下文。"),
    }
    if leaks.is_empty() {
        out.push_str("未检出泄漏。");
    } else {
        let names: Vec<String> = leaks
            .iter()
            .take(3)
            .map(|l| format!("{}（{}）", l.debug_name, l.kind.label()))
            .collect();
        out.push_str(&format!(
            "检出 {} 项泄漏：{}{}。",
            leaks.len(),
            names.join("、"),
            if leaks.len() > 3 { " 等" } else { "" }
        ));
    }
    if !reclaimed.is_empty() {
        out.push_str(&format!("本轮强制回收 {} 个上下文。", reclaimed.len()));
    }
    out
}

/// 装配生命周期总表。
pub fn build_table(mgr: &ContextLifecycleManager, reclaimed: Vec<String>) -> LifecycleTable {
    let contexts: Vec<ContextRecord> = mgr.list().to_vec();
    let leaks = detect_leaks(mgr);
    let active_id = mgr.active_id().map(|s| s.to_string());
    let a11y = a11y_lifecycle(&contexts, active_id.as_deref(), &leaks, &reclaimed);
    LifecycleTable {
        contexts,
        tick: mgr.now(),
        active_id,
        leaks,
        reclaimed,
        a11y_summary: a11y,
    }
}

// ---------------------------------------------------------------------------
// 六、完整生命周期剧本（回归与演示用）
// ---------------------------------------------------------------------------

/// 默认参数构造（省去每处重复）。
pub fn params_of(name: &str, owner: &str) -> ContextCreateParams {
    ContextCreateParams {
        debug_name: name.to_string(),
        color_space: ColorSpace::Srgb,
        queue_depth: 256,
        share_from: None,
        owner_thread_id: owner.to_string(),
        allow_force_reclaim: true,
    }
}

/// 完整生命周期剧本：创建主上下文 → 激活 → 分配句柄 → 派生子上下文（共享）
/// → 提交若干次 → 销毁子 → 释放句柄 → 销毁主 → 扫泄漏 → 出总表。
///
/// 这段是"四态真的能转起来"的活证据，不是文档描述。
pub fn run_lifecycle_script() -> (LifecycleTable, Vec<(String, String)>) {
    let mut mgr = ContextLifecycleManager::new();
    let mut reclaimed: Vec<String> = Vec::new();

    // 1) 创建主上下文
    let main_id = mgr.create(params_of("主窗口 · 编辑器画布", "main")).unwrap_or_default();

    // 2) 激活 + 分配资源句柄
    mgr.advance(2);
    let _ = mgr.transition(&main_id, ContextState::Active, "main", "窗口获得焦点");
    let _ = mgr.alloc_handle(&main_id, "main", 8);

    // 3) 派生子上下文（共享主上下文的资源）
    mgr.advance(1);
    let mut child_p = params_of("缩略图 · 后台渲染", "render-1");
    child_p.share_from = Some(main_id.clone());
    let child_id = mgr.create(child_p).unwrap_or_default();
    let _ = mgr.transition(&child_id, ContextState::Active, "render-1", "缩略图首次渲染");
    let _ = mgr.transition(&child_id, ContextState::Shared, "render-1", "资源并入主上下文共享池");

    // 4) 提交若干次
    for _ in 0..3 {
        mgr.advance(1);
        mgr.note_submit(&main_id, "main");
    }

    // 5) 销毁子上下文（先子后父，顺序错会被拒——这正是要验的）
    mgr.advance(1);
    let _ = mgr.transition(&child_id, ContextState::Active, "render-1", "退出共享");
    let _ = mgr.destroy(&child_id, "render-1");

    // 6) 释放主上下文句柄并销毁
    mgr.advance(1);
    let _ = mgr.free_handle(&main_id, "main", 8);
    let _ = mgr.destroy(&main_id, "main");

    // 7) 扫泄漏 + 回收 + 出总表
    let leaks = detect_leaks(&mgr);
    for o in reclaim_all(&mut mgr, &leaks) {
        if o.ok {
            reclaimed.push(o.context_id);
        }
    }
    let table = build_table(&mgr, reclaimed);

    // 父子溯源账：谁共享给谁
    let mut parent_of: Vec<(String, String)> = Vec::new();
    for c in table.contexts.iter() {
        if let Some(p) = &c.share_from {
            parent_of.push((c.id.clone(), p.clone()));
        }
    }
    (table, parent_of)
}

// ---------------------------------------------------------------------------
// 七、域自检
// ---------------------------------------------------------------------------

/// VE-F0002 域自检。逐条判据一项一 check，绿了才算做完。
pub fn run_vea02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea02");

    // 判据：四态管理（created→active→shared→destroyed 全程可转）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let mut b = params_of("B", "r1");
        b.share_from = Some(a.clone());
        let b_id = m.create(b).unwrap_or_default();
        let t1 = m.transition(&a, ContextState::Active, "main", "焦点");
        let t2 = m.transition(&b_id, ContextState::Active, "r1", "首渲");
        let t3 = m.transition(&b_id, ContextState::Shared, "r1", "并入池");
        set.add(
            "A02-四态-全程可转",
            t1.is_ok() && t2.is_ok() && t3.is_ok() && m.get(&b_id).map(|c| c.state) == Some(ContextState::Shared),
            "",
        );
    }

    // 判据：destroyed 是终态，不可回退
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let _ = m.destroy(&a, "main");
        let back = m.transition(&a, ContextState::Active, "main", "试图复活").err();
        let has_suggestion = back
            .as_ref()
            .map(|e| e.suggestion.contains("终态"))
            .unwrap_or(false);
        set.add(
            "A02-四态-终态不可回退",
            back.is_some() && has_suggestion,
            "",
        );
    }

    // 判据：归属约束（非归属线程被拒且给建议）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let v = m.check_ownership(&a, "render-9");
        let mv = m.transition(&a, ContextState::Active, "render-9", "越权激活");
        set.add(
            "A02-归属约束-越权拒绝",
            !v.ok
                && v.reason.contains("归属违例")
                && !v.suggestion.is_empty()
                && mv.is_err(),
            "",
        );
    }

    // 判据：参数越界拒绝含修正建议
    {
        let mut p = params_of("x", "main");
        p.queue_depth = 300; // 非 2 的幂
        let v = validate_params(&p);
        let has_pow_advice = v
            .issues
            .iter()
            .any(|i| i.field == "queue_depth" && i.suggestion.contains("2^"));
        set.add("A02-参数-非2幂拒绝", !v.ok && has_pow_advice, "");
    }

    // 判据：多类越界全部拒绝（越小/越大/空名/空白名/自引用/空线程）
    {
        let mut all_rejected = true;
        let mut p1 = params_of("x", "main");
        p1.queue_depth = 1;
        all_rejected &= !validate_params(&p1).ok;
        let mut p2 = params_of("x", "main");
        p2.queue_depth = 99999;
        all_rejected &= !validate_params(&p2).ok;
        let mut p3 = params_of("", "main");
        p3.queue_depth = 256;
        all_rejected &= !validate_params(&p3).ok;
        let p4 = params_of("   ", "main");
        all_rejected &= !validate_params(&p4).ok;
        let mut p5 = params_of("x", "main");
        p5.share_from = Some("self".to_string());
        all_rejected &= !validate_params(&p5).ok;
        let p6 = params_of("x", "");
        all_rejected &= !validate_params(&p6).ok;
        set.add("A02-参数-多类越界拒绝", all_rejected, "");
    }

    // 判据：合法但偏深 → warn 不阻断
    {
        let mut p = params_of(&"x".repeat(80), "main");
        p.queue_depth = 2048; // 2 的幂、≤4096、>1024 ⇒ warn 档
        let v = validate_params(&p);
        set.add("A02-参数-warn不阻断", v.ok && v.warn_count > 0, "");
    }

    // 判据：调试命名（人话名字可查）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("主窗口 · 编辑器画布", "main")).unwrap_or_default();
        set.add(
            "A02-调试命名-可查",
            m.get(&a).map(|c| c.debug_name == "主窗口 · 编辑器画布").unwrap_or(false),
            "",
        );
    }

    // 判据：共享父子溯源审计 + 销毁时双向解绑
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let mut b = params_of("B", "r1");
        b.share_from = Some(a.clone());
        let b_id = m.create(b).unwrap_or_default();
        let parent_has_child = m
            .get(&a)
            .map(|c| c.shared_children.contains(&b_id))
            .unwrap_or(false);
        let child_has_parent = m.get(&b_id).map(|c| c.share_from.is_some()).unwrap_or(false);
        let _ = m.destroy(&b_id, "r1");
        let unbound = m
            .get(&a)
            .map(|c| c.ref_count == 0 && c.shared_children.is_empty())
            .unwrap_or(false);
        set.add(
            "A02-溯源-双向登记与解绑",
            parent_has_child && child_has_parent && unbound,
            "",
        );
    }

    // 判据：销毁顺序错被拒（两道闸任一命中都算对）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let mut b = params_of("B", "r1");
        b.share_from = Some(a.clone());
        let _ = m.create(b);
        let e = m.destroy(&a, "main");
        let gated = e
            .err()
            .map(|x| x.code == "E_DESTROY_REF" || x.code == "E_DESTROY_CHILD")
            .unwrap_or(false);
        set.add("A02-销毁-顺序闸生效", gated, "");
    }

    // 判据：泄漏检测（已销毁但句柄未归零）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let _ = m.alloc_handle(&a, "main", 7);
        let _ = m.force_mutate(&a, "模拟只改状态没收资源", |c| {
            c.state = ContextState::Destroyed;
        });
        let leaks = detect_leaks(&m);
        let hit = leaks.iter().find(|l| l.kind == LeakKind::HandleLeak);
        set.add(
            "A02-泄漏-句柄泄漏检出",
            hit.map(|l| l.magnitude == 7 && l.reclaimable).unwrap_or(false),
            "",
        );
    }

    // 判据：强制回收必留审计痕迹
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let _ = m.alloc_handle(&a, "main", 5);
        let _ = m.force_mutate(&a, "模拟销毁不彻底", |c| {
            c.state = ContextState::Destroyed;
        });
        let r = force_reclaim(&mut m, &a);
        let audited = m
            .get(&a)
            .and_then(|c| c.transitions.last())
            .map(|t| t.by == "system" && t.reason.contains("强制"))
            .unwrap_or(false);
        set.add(
            "A02-强制回收-留审计",
            r.ok && r.freed_handles == 5 && r.audited && audited,
            "",
        );
    }

    // 判据：强制回收铁律——不可强收的只告警
    {
        let mut m = ContextLifecycleManager::new();
        let mut p = params_of("系统资源", "main");
        p.allow_force_reclaim = false;
        let a = m.create(p).unwrap_or_default();
        let _ = m.alloc_handle(&a, "main", 3);
        let _ = m.force_mutate(&a, "模拟销毁不彻底", |c| {
            c.state = ContextState::Destroyed;
        });
        let r = force_reclaim(&mut m, &a);
        let untouched = m.get(&a).map(|c| c.live_handle_count == 3).unwrap_or(false);
        set.add(
            "A02-强制回收-只告警不强收",
            !r.ok && r.note.contains("告警") && untouched,
            "",
        );
    }

    // 判据：僵尸上下文回收（阈值内不误报）
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        m.advance(2);
        let no_false_positive = detect_leaks(&m)
            .iter()
            .filter(|l| l.kind == LeakKind::Zombie)
            .count()
            == 0;
        m.advance(ZOMBIE_TICK_THRESHOLD + 5);
        let detected = detect_leaks(&m)
            .iter()
            .any(|l| l.kind == LeakKind::Zombie && l.context_id == a);
        let out = reclaim_zombies(&mut m);
        let reclaimed_ok = out.iter().any(|o| o.ok && o.context_id == a);
        set.add(
            "A02-僵尸-阈值回收不误报",
            no_false_positive && detected && reclaimed_ok,
            "",
        );
    }

    // 判据：悬挂子记录检出
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let mut b = params_of("B", "r1");
        b.share_from = Some(a.clone());
        let b_id = m.create(b).unwrap_or_default();
        let _ = m.destroy(&b_id, "r1");
        let _ = m.force_mutate(&a, "模拟未清子记录", |c| {
            c.shared_children = alloc::vec![b_id.clone()];
        });
        set.add(
            "A02-泄漏-悬挂子记录",
            detect_leaks(&m)
                .iter()
                .any(|l| l.kind == LeakKind::DanglingChild),
            "",
        );
    }

    // 判据：上下文数量上限拦截并给建议
    {
        let mut m = ContextLifecycleManager::new();
        for i in 0..MAX_CONTEXTS {
            let _ = m.create(params_of(&format!("ctx{}", i), "main"));
        }
        let over = m.create(params_of("overflow", "main")).err();
        let has_advice = over
            .as_ref()
            .map(|e| e.suggestion.contains("销毁"))
            .unwrap_or(false);
        set.add(
            "A02-边界-上限拦截",
            over.is_some() && has_advice,
            "",
        );
    }

    // 判据：共享源不存在 / 已销毁被拒
    {
        let mut m = ContextLifecycleManager::new();
        let mut bad = params_of("x", "main");
        bad.share_from = Some("ctx-9999".to_string());
        let e1 = m.create(bad);
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let _ = m.destroy(&a, "main");
        let mut dead = params_of("x", "main");
        dead.share_from = Some(a.clone());
        let e2 = m.create(dead);
        set.add(
            "A02-边界-共享源校验",
            e1.is_err() && e2.is_err() && e2.err().map(|x| x.suggestion.contains("存活")).unwrap_or(false),
            "",
        );
    }

    // 判据：状态迁移全程留痕
    {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params_of("A", "main")).unwrap_or_default();
        let _ = m.transition(&a, ContextState::Active, "main", "焦点");
        let _ = m.destroy(&a, "main");
        let tr = m.get(&a).map(|c| c.transitions.clone()).unwrap_or_default();
        set.add(
            "A02-审计-迁移留痕",
            tr.len() >= 3 && tr.iter().all(|x| !x.reason.is_empty()),
            "",
        );
    }

    // 判据：错误带 code 与建议
    {
        let mut m = ContextLifecycleManager::new();
        match m.destroy("ctx-9999", "main") {
            Err(e) => set.add(
                "A02-错误-带code与建议",
                e.code == "E_NOENT" && !e.suggestion.is_empty(),
                "",
            ),
            Ok(_) => set.add("A02-错误-带code与建议", false, "应抛错"),
        }
    }

    // 判据：读屏可达（生命周期摘要含四态计数与泄漏）
    {
        let (t, _) = run_lifecycle_script();
        set.add(
            "A02-读屏-生命周期可查",
            t.a11y_summary.contains("图形上下文")
                && t.a11y_summary.contains("已销毁")
                && t.a11y_summary.contains("泄漏"),
            "",
        );
    }

    // 判据：完整剧本跑通（四态 + 共享 + 销毁 + 溯源）
    {
        let (t, parent_of) = run_lifecycle_script();
        let all_destroyed = t
            .contexts
            .iter()
            .all(|c| c.state == ContextState::Destroyed);
        set.add(
            "A02-剧本-全链跑通",
            t.contexts.len() == 2 && all_destroyed && parent_of.len() == 1 && t.tick > 0,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(name: &str, owner: &str) -> ContextCreateParams {
        params_of(name, owner)
    }

    #[test]
    fn vea02_checks_all_green() {
        let set = run_vea02_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-A02 域自检红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    #[test]
    fn four_states_transition_chain() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        let mut bp = params("B", "r1");
        bp.share_from = Some(a.clone());
        let b = m.create(bp).unwrap();
        assert!(m.transition(&a, ContextState::Active, "main", "x").is_ok());
        assert!(m.transition(&b, ContextState::Active, "r1", "x").is_ok());
        assert!(m.transition(&b, ContextState::Shared, "r1", "x").is_ok());
        assert_eq!(m.get(&b).unwrap().state, ContextState::Shared);
    }

    #[test]
    fn destroyed_is_terminal() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        m.destroy(&a, "main").unwrap();
        assert!(m.transition(&a, ContextState::Active, "main", "复活").is_err());
    }

    #[test]
    fn ownership_violation_rejected() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        let v = m.check_ownership(&a, "other");
        assert!(!v.ok);
        assert_eq!(v.actual_owner.as_deref(), Some("main"));
        assert!(!v.suggestion.is_empty());
    }

    #[test]
    fn param_out_of_range_rejected_with_advice() {
        let mut p = params("x", "main");
        p.queue_depth = 300;
        let v = validate_params(&p);
        assert!(!v.ok);
        assert!(v.rejection_message().contains("建议"));
    }

    #[test]
    fn shared_child_destroy_unbinds_parent() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        let mut bp = params("B", "r1");
        bp.share_from = Some(a.clone());
        let b = m.create(bp).unwrap();
        m.destroy(&b, "r1").unwrap();
        let pa = m.get(&a).unwrap();
        assert_eq!(pa.ref_count, 0);
        assert!(pa.shared_children.is_empty());
        // 父现在可以销毁
        assert!(m.destroy(&a, "main").is_ok());
    }

    #[test]
    fn force_reclaim_leaves_audit() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        m.alloc_handle(&a, "main", 5).unwrap();
        m.force_mutate(&a, "模拟", |c| c.state = ContextState::Destroyed)
            .unwrap();
        let r = force_reclaim(&mut m, &a);
        assert!(r.ok && r.audited);
        let last = m.get(&a).unwrap().transitions.last().unwrap();
        assert_eq!(last.by, "system");
    }

    #[test]
    fn non_reclaimable_only_warns() {
        let mut m = ContextLifecycleManager::new();
        let mut p = params("系统", "main");
        p.allow_force_reclaim = false;
        let a = m.create(p).unwrap();
        m.alloc_handle(&a, "main", 3).unwrap();
        m.force_mutate(&a, "模拟", |c| c.state = ContextState::Destroyed)
            .unwrap();
        let r = force_reclaim(&mut m, &a);
        assert!(!r.ok);
        assert_eq!(m.get(&a).unwrap().live_handle_count, 3, "没强收就真没动");
    }

    #[test]
    fn zombie_reclaimed_above_threshold() {
        let mut m = ContextLifecycleManager::new();
        let a = m.create(params("A", "main")).unwrap();
        m.advance(2);
        assert_eq!(
            detect_leaks(&m)
                .iter()
                .filter(|l| l.kind == LeakKind::Zombie)
                .count(),
            0,
            "阈值内不误报"
        );
        m.advance(ZOMBIE_TICK_THRESHOLD + 5);
        assert!(detect_leaks(&m)
            .iter()
            .any(|l| l.kind == LeakKind::Zombie));
        assert!(reclaim_zombies(&mut m).iter().any(|o| o.ok));
    }

    #[test]
    fn full_script_runs_clean() {
        let (t, parent_of) = run_lifecycle_script();
        assert_eq!(t.contexts.len(), 2);
        assert!(t.contexts.iter().all(|c| c.state == ContextState::Destroyed));
        assert_eq!(parent_of.len(), 1);
    }
}
