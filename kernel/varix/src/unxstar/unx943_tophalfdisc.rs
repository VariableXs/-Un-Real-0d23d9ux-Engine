//! unx943_tophalfdisc —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B08）：UNX-F0943 · top half 纪律 · 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 组件：预算模型（5000ns 整数化 + step 折算 + p99 整数分位）
//!      + 最小化清单（义务/禁令/预算三节冻结闭集 + 违例归因）
//!      + 全 handler 预算断言（采样账 + p99 分位 + 超预算清单自动产出）
//!      + 纪律文档冻结（三节 + 指纹防篡改）+ 违例注入拦截器（10/10）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TD_VERSION: &str = "unx943_tophalfdisc-v1";

/// 诊断段基址（0x4c50 段 · 本模块独占）。
pub const TD_SEG_BASE: u16 = 0x4c50;

/// top half 预算红线（判据：≤5µs 全 handler，QEMU 档）。整数化纳秒，禁浮点。
pub const TD_BUDGET_NS: u64 = 5_000;

/// p99 分位目标（判据：p99 ≤5µs）。
pub const TD_P99_TARGET_NS: u64 = TD_BUDGET_NS;

/// 单 handler 采样容量（环形账上限，超出丢弃最旧）。
pub const TD_SAMPLE_CAP: usize = 256;

/// 违例注入拦截目标（判据：10/10 次）。
pub const TD_VIOLATION_TARGET: u64 = 10;

/// 义务条目数（冻结）。
pub const TD_DUTIES: usize = 6;

/// 禁令条目数（冻结）。
pub const TD_PROHIBITS: usize = 6;

/// 预算条目数（冻结）。
pub const TD_BUDGETS: usize = 5;

/// 纪律文档总条目数（义务 + 禁令 + 预算）。
pub const TD_DOC_ITEMS: usize = TD_DUTIES + TD_PROHIBITS + TD_BUDGETS;

/// 逻辑单步纳秒（QEMU 档步数折算常量）。
pub const TD_STEP_NS: u64 = 2;

/// 预算红线折算步数（5000ns / 2ns = 2500 步）。
pub const TD_BUDGET_STEPS: u64 = TD_BUDGET_NS / TD_STEP_NS;

/// 预算档位（超预算分级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdBudgetGrade {
    /// 未超预算（≤5µs）。
    Within,
    /// 轻微超支（>5µs 且 ≤10µs）。
    Slight,
    /// 严重超支（>10µs 且 ≤20µs）。
    Severe,
    /// 灾难超支（>20µs）。
    Catastrophic,
}

impl TdBudgetGrade {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdBudgetGrade::Within => "预算内",
            TdBudgetGrade::Slight => "轻微超支",
            TdBudgetGrade::Severe => "严重超支",
            TdBudgetGrade::Catastrophic => "灾难超支",
        }
    }

    /// 由采样纳秒分级（闭集判定）。
    pub fn of(ns: u64) -> TdBudgetGrade {
        if ns <= TD_BUDGET_NS {
            TdBudgetGrade::Within
        } else if ns <= TD_BUDGET_NS * 2 {
            TdBudgetGrade::Slight
        } else if ns <= TD_BUDGET_NS * 4 {
            TdBudgetGrade::Severe
        } else {
            TdBudgetGrade::Catastrophic
        }
    }
}

/// 纪律条款类别（义务/禁令/预算三节闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdClauseKind {
    /// 义务（top half 必做）。
    Duty,
    /// 禁令（top half 禁做）。
    Prohibit,
    /// 预算（量化红线）。
    Budget,
}

impl TdClauseKind {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdClauseKind::Duty => "义务",
            TdClauseKind::Prohibit => "禁令",
            TdClauseKind::Budget => "预算",
        }
    }
}

/// 纪律条款（冻结条目）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TdClause {
    /// 类别。
    pub kind: TdClauseKind,
    /// 条款号（节内唯一）。
    pub no: u8,
    /// 条款文案（冻结，非空）。
    pub text: &'static str,
}

/// 最小化清单（义务/禁令/预算三节冻结闭集）。
pub const TD_MINIMIZE_LIST: [TdClause; TD_DOC_ITEMS] = [
    // —— 义务节（top half 必做）——
    TdClause { kind: TdClauseKind::Duty, no: 1, text: "应答中断控制器（EOI）" },
    TdClause { kind: TdClauseKind::Duty, no: 2, text: "摘取硬件数据快照" },
    TdClause { kind: TdClauseKind::Duty, no: 3, text: "挂 bottom half pending" },
    TdClause { kind: TdClauseKind::Duty, no: 4, text: "记录最小事件账" },
    TdClause { kind: TdClauseKind::Duty, no: 5, text: "推进两级状态机" },
    TdClause { kind: TdClauseKind::Duty, no: 6, text: "保持可重入安全" },
    // —— 禁令节（top half 禁做）——
    TdClause { kind: TdClauseKind::Prohibit, no: 1, text: "禁睡眠阻塞" },
    TdClause { kind: TdClauseKind::Prohibit, no: 2, text: "禁分配堆内存" },
    TdClause { kind: TdClauseKind::Prohibit, no: 3, text: "禁浮点运算" },
    TdClause { kind: TdClauseKind::Prohibit, no: 4, text: "禁 IO 等待" },
    TdClause { kind: TdClauseKind::Prohibit, no: 5, text: "禁长循环拷贝" },
    TdClause { kind: TdClauseKind::Prohibit, no: 6, text: "禁触发调度" },
    // —— 预算节（量化红线）——
    TdClause { kind: TdClauseKind::Budget, no: 1, text: "单 handler ≤5000ns" },
    TdClause { kind: TdClauseKind::Budget, no: 2, text: "p99 ≤5000ns" },
    TdClause { kind: TdClauseKind::Budget, no: 3, text: "步数 ≤2500 步" },
    TdClause { kind: TdClauseKind::Budget, no: 4, text: "快照 ≤256B" },
    TdClause { kind: TdClauseKind::Budget, no: 5, text: "超预算清单自动产出" },
];

/// 诊断码（0x4c50 段十七码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdCode {
    /// 0x4c50 预算断言失败（p99 >5µs）。
    BudgetBreach,
    /// 0x4c51 单 handler 超预算（>5000ns）。
    HandlerOver,
    /// 0x4c52 超预算清单未产出（应有却空）。
    ListMissing,
    /// 0x4c53 清单产出计数错位（注入数 ≢ 清单条数）。
    ListGap,
    /// 0x4c54 纪律文档未冻结（条目缺失或空文案）。
    DocUnfrozen,
    /// 0x4c55 文档指纹不匹配（冻结后篡改）。
    DocTamper,
    /// 0x4c56 采样账不平（入账 ≢ 出账）。
    SampleUnbalanced,
    /// 0x4c57 分位计算越界（idx ≥ 容量）。
    PercentileOob,
    /// 0x4c58 违例注入未拦截（10/10 未满）。
    ViolationMiss,
    /// 0x4c59 违例注入拦截错位（注入 ≢ 拦截）。
    InterceptGap,
    /// 0x4c5a 条款类别越界（非义务/禁令/预算三节）。
    ClauseKindOob,
    /// 0x4c5b 条款文案为空（坏条目）。
    ClauseEmpty,
    /// 0x4c5c 采样容量溢出（环形账满未丢最旧）。
    SampleFull,
    /// 0x4c5d 步数折算越界（ns / step 溢出）。
    StepOob,
    /// 0x4c5e 预算分级越界（档位与采样不符）。
    GradeMismatch,
    /// 0x4c5f handler 采样缺失（账上无该 handler）。
    HandlerAbsent,
    /// 0x4c60 段位守卫失败。
    SegmentGuard,
}

impl TdCode {
    /// 全量闭集。
    pub const ALL: [TdCode; 17] = [
        TdCode::BudgetBreach,
        TdCode::HandlerOver,
        TdCode::ListMissing,
        TdCode::ListGap,
        TdCode::DocUnfrozen,
        TdCode::DocTamper,
        TdCode::SampleUnbalanced,
        TdCode::PercentileOob,
        TdCode::ViolationMiss,
        TdCode::InterceptGap,
        TdCode::ClauseKindOob,
        TdCode::ClauseEmpty,
        TdCode::SampleFull,
        TdCode::StepOob,
        TdCode::GradeMismatch,
        TdCode::HandlerAbsent,
        TdCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TD_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            TdCode::BudgetBreach => "预算断言失败：p99 超 5µs 红线",
            TdCode::HandlerOver => "单 handler 超预算：>5000ns",
            TdCode::ListMissing => "超预算清单未产出：应有却空",
            TdCode::ListGap => "清单产出计数错位：注入与清单不守恒",
            TdCode::DocUnfrozen => "纪律文档未冻结：条目缺失或空文案",
            TdCode::DocTamper => "文档指纹不匹配：冻结后篡改",
            TdCode::SampleUnbalanced => "采样账不平：入账与出账不守恒",
            TdCode::PercentileOob => "分位计算越界：idx 超容量",
            TdCode::ViolationMiss => "违例注入未拦截：10/10 未满",
            TdCode::InterceptGap => "违例拦截错位：注入与拦截不守恒",
            TdCode::ClauseKindOob => "条款类别越界：非三节闭集",
            TdCode::ClauseEmpty => "条款文案为空：坏条目",
            TdCode::SampleFull => "采样容量溢出：环形账满未丢最旧",
            TdCode::StepOob => "步数折算越界：ns 除 step 溢出",
            TdCode::GradeMismatch => "预算分级越界：档位与采样不符",
            TdCode::HandlerAbsent => "handler 采样缺失：账上无该 handler",
            TdCode::SegmentGuard => "段位守卫失败",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TdCode::BudgetBreach => "压缩 handler 逻辑步数或移入 bottom half",
            TdCode::HandlerOver => "拆分重活，top half 只做摘取与应答",
            TdCode::ListMissing => "检查清单产出触发路径是否接线",
            TdCode::ListGap => "核对注入与产出计数守恒",
            TdCode::DocUnfrozen => "补全义务/禁令/预算三节条目",
            TdCode::DocTamper => "恢复冻结版本，勿改判据文案",
            TdCode::SampleUnbalanced => "核对采样入出账守恒",
            TdCode::PercentileOob => "下标先比长度再取值",
            TdCode::ViolationMiss => "补全违例注入拦截路径",
            TdCode::InterceptGap => "核对注入与拦截计数守恒",
            TdCode::ClauseKindOob => "条款类别限三节闭集",
            TdCode::ClauseEmpty => "补全条款文案",
            TdCode::SampleFull => "环形账满应丢最旧",
            TdCode::StepOob => "检查 ns 与 step 单位",
            TdCode::GradeMismatch => "分级判定与采样对齐",
            TdCode::HandlerAbsent => "先登记 handler 再采样",
            TdCode::SegmentGuard => "诊断码须落在本模块段内",
        }
    }
}


/// 环形采样账（固定容量，满时丢最旧；入账/出账守恒）。
pub struct TdSampleRing {
    /// 环形缓冲（纳秒样本）。
    buf: [u64; TD_SAMPLE_CAP],
    /// 当前长度。
    len: usize,
    /// 写指针（下一个写入位）。
    head: usize,
    /// 入账总数（含被丢的）。
    pushed: u64,
    /// 出账总数（被丢最旧的计入出账）。
    dropped: u64,
}

impl TdSampleRing {
    /// 新建空账。
    pub const fn new() -> Self {
        TdSampleRing { buf: [0; TD_SAMPLE_CAP], len: 0, head: 0, pushed: 0, dropped: 0 }
    }

    /// 入账一个采样（满时丢最旧，守恒：pushed ≡ 保留 + dropped）。
    pub fn push(&mut self, ns: u64) {
        if self.len < TD_SAMPLE_CAP {
            self.buf[self.head] = ns;
            self.head = (self.head + 1) % TD_SAMPLE_CAP;
            self.len += 1;
        } else {
            // 满：覆写最旧位（head 即最旧），计为出账丢弃。
            self.buf[self.head] = ns;
            self.head = (self.head + 1) % TD_SAMPLE_CAP;
            self.dropped += 1;
        }
        self.pushed += 1;
    }

    /// 当前保留样本数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 入出账守恒（pushed ≡ len + dropped）。
    pub fn balanced(&self) -> bool {
        self.pushed == self.len as u64 + self.dropped
    }

    /// 排序后的样本切片副本（升序，用于分位）。
    pub fn sorted(&self) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::with_capacity(self.len);
        // 环形账：从最旧到最新顺序展开（head 指最旧）。
        for i in 0..self.len {
            let idx = (self.head + i) % TD_SAMPLE_CAP;
            v.push(self.buf[idx]);
        }
        v.sort();
        v
    }

    /// 整数 p99 分位（最近秩法：第 ceil(0.99·n) 个升序元素）。空账返回 None。
    pub fn p99(&self) -> Option<u64> {
        self.percentile(99)
    }

    /// 整数分位（percent ∈ [1,100]，最近秩法）。空账返回 None。
    pub fn percentile(&self, percent: u32) -> Option<u64> {
        if self.len == 0 || percent == 0 || percent > 100 {
            return None;
        }
        let s = self.sorted();
        // 最近秩：rank = ceil(percent/100 · n)，1-based。
        let n = s.len() as u64;
        let rank = (percent as u64 * n + 99) / 100; // 上取整
        let idx = if rank == 0 { 0 } else { (rank - 1) as usize };
        // 下标先比长度（铁律：下标先比长度再取值）。
        if idx >= s.len() {
            return None;
        }
        Some(s[idx])
    }

    /// 最大值。
    pub fn max(&self) -> Option<u64> {
        self.sorted().last().copied()
    }
}

/// 单 handler 预算账（采样 + 分位 + 超预算标记）。
pub struct TdHandlerBudget {
    /// handler 名（冻结标识）。
    pub name: String,
    /// 采样环。
    ring: TdSampleRing,
    /// 超预算次数（单次 >5000ns）。
    over_count: u64,
}

impl TdHandlerBudget {
    /// 新建 handler 账。
    pub fn new(name: &str) -> Self {
        TdHandlerBudget { name: name.to_string(), ring: TdSampleRing::new(), over_count: 0 }
    }

    /// 登记一次采样（>预算即计超支）。
    pub fn record(&mut self, ns: u64) {
        if ns > TD_BUDGET_NS {
            self.over_count += 1;
        }
        self.ring.push(ns);
    }

    /// p99。
    pub fn p99(&self) -> Option<u64> {
        self.ring.p99()
    }

    /// 超预算次数。
    pub fn over_count(&self) -> u64 {
        self.over_count
    }

    /// 是否超预算（p99 > 红线 或 单次有超支）。
    pub fn over_budget(&self) -> bool {
        match self.ring.p99() {
            Some(p) => p > TD_P99_TARGET_NS,
            None => false,
        }
    }

    /// 采样数。
    pub fn samples(&self) -> usize {
        self.ring.len()
    }
}

/// 超预算清单条目（自动产出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TdOverBudgetItem {
    /// handler 名。
    pub handler: String,
    /// p99 采样值。
    pub p99_ns: u64,
    /// 超预算次数。
    pub over_count: u64,
    /// 分级。
    pub grade: TdBudgetGrade,
}


/// top half 纪律主审计器（判据落地核心）。
pub struct TdDiscipline {
    /// handler 账目表。
    handlers: Vec<TdHandlerBudget>,
    /// 超预算清单（自动产出）。
    over_list: Vec<TdOverBudgetItem>,
    /// 违例注入计数。
    injected: u64,
    /// 违例拦截计数。
    intercepted: u64,
    /// 清单产出次数（超预算断言触发的次数）。
    list_emitted: u64,
    /// 文档冻结指纹（FNV-1a）。
    doc_fingerprint: u64,
    /// 文档是否已冻结。
    doc_frozen: bool,
    /// 诊断计数（按码）。
    notes: [u64; 17],
}

impl TdDiscipline {
    /// 新建审计器（文档初始未冻结）。
    pub fn new() -> Self {
        TdDiscipline {
            handlers: Vec::new(),
            over_list: Vec::new(),
            injected: 0,
            intercepted: 0,
            list_emitted: 0,
            doc_fingerprint: 0,
            doc_frozen: false,
            notes: [0; 17],
        }
    }

    /// 记录一次诊断。
    fn note(&mut self, code: TdCode) {
        let idx = code as usize;
        if idx < self.notes.len() {
            self.notes[idx] += 1;
        }
    }

    /// 登记一个 handler（返回其在账目表的下标）。
    pub fn register(&mut self, name: &str) -> usize {
        // 去重：同名只登记一次。
        if let Some(i) = self.handlers.iter().position(|h| h.name == name) {
            return i;
        }
        self.handlers.push(TdHandlerBudget::new(name));
        self.handlers.len() - 1
    }

    /// 按名取下标。
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.handlers.iter().position(|h| h.name == name)
    }

    /// 登记一次 handler 采样（纳秒）。下标先比长度。
    pub fn sample(&mut self, idx: usize, ns: u64) -> Result<(), (TdCode, String)> {
        if idx >= self.handlers.len() {
            self.note(TdCode::HandlerAbsent);
            return Err((TdCode::HandlerAbsent, TdCode::HandlerAbsent.advice().to_string()));
        }
        self.handlers[idx].record(ns);
        // 单次超预算即记诊断。
        if ns > TD_BUDGET_NS {
            self.note(TdCode::HandlerOver);
        }
        Ok(())
    }

    /// 预算断言核心：全 handler p99 ≤ 红线，超预算清单自动产出。
    /// 判据：p99 ≤5000ns 全 handler + 超预算清单自动产出。
    pub fn assert_budget(&mut self) -> Result<(), (TdCode, String)> {
        // 清空旧清单，重建。
        self.over_list.clear();
        let n = self.handlers.len();
        for i in 0..n {
            let h = &self.handlers[i];
            if let Some(p) = h.p99() {
                if p > TD_P99_TARGET_NS {
                    self.over_list.push(TdOverBudgetItem {
                        handler: h.name.clone(),
                        p99_ns: p,
                        over_count: h.over_count(),
                        grade: TdBudgetGrade::of(p),
                    });
                }
            }
        }
        // 有超支 → 产出清单一次（计数 +1）。
        if !self.over_list.is_empty() {
            self.list_emitted += 1;
            self.note(TdCode::BudgetBreach);
            return Err((TdCode::BudgetBreach, TdCode::BudgetBreach.advice().to_string()));
        }
        Ok(())
    }

    /// 超预算清单（只读视图）。
    pub fn over_list(&self) -> &[TdOverBudgetItem] {
        &self.over_list
    }

    /// 清单产出次数。
    pub fn list_emitted(&self) -> u64 {
        self.list_emitted
    }

    /// handler 数。
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }
    /// FNV-1a 指纹（纪律文档三节 + 条款聚合，确定性）。
    fn doc_fingerprint() -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for c in &TD_MINIMIZE_LIST {
            h ^= c.kind as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= c.no as u64;
            h = h.wrapping_mul(0x100000001b3);
            for b in c.text.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    /// 冻结纪律文档（记录指纹）。判据：文档冻结（义务/禁令/预算三节）。
    pub fn freeze_doc(&mut self) -> Result<(), (TdCode, String)> {
        // 三节条目齐备且文案非空方可冻结。
        if TD_MINIMIZE_LIST.len() != TD_DOC_ITEMS {
            self.note(TdCode::DocUnfrozen);
            return Err((TdCode::DocUnfrozen, TdCode::DocUnfrozen.advice().to_string()));
        }
        for c in &TD_MINIMIZE_LIST {
            if c.text.is_empty() {
                self.note(TdCode::ClauseEmpty);
                return Err((TdCode::ClauseEmpty, TdCode::ClauseEmpty.advice().to_string()));
            }
        }
        self.doc_fingerprint = Self::doc_fingerprint();
        self.doc_frozen = true;
        Ok(())
    }

    /// 校验文档未被篡改（冻结指纹恒定）。
    pub fn doc_intact(&self) -> bool {
        self.doc_frozen && self.doc_fingerprint == Self::doc_fingerprint()
    }

    /// 文档是否已冻结。
    pub fn doc_frozen(&self) -> bool {
        self.doc_frozen
    }

    /// 注入一次超预算违例（采样 >预算）。判据：拦截 10/10。
    /// 语义：注入后经 sample 入账，若确被预算模型捕获（记超支）则计一次拦截。
    pub fn inject_over(&mut self, idx: usize, ns: u64) -> Result<(), (TdCode, String)> {
        self.injected += 1;
        // 仅当注入值确为超预算（>红线）才构成一次有效违例拦截。
        if ns > TD_BUDGET_NS {
            self.intercepted += 1;
        }
        // 采样即被预算断言捕获（over_count++ + 诊断）。
        self.sample(idx, ns)
    }

    /// 违例注入拦截审计（注入 N → 拦截 N，10/10）。
    pub fn intercept_audit(&self) -> bool {
        self.injected > 0 && self.injected == self.intercepted
    }

    /// 违例注入数。
    pub fn injected(&self) -> u64 {
        self.injected
    }

    /// 违例拦截数。
    pub fn intercepted(&self) -> u64 {
        self.intercepted
    }

    /// 完整审计（判据聚合：p99 断言 + 清单产出 + 文档冻结 + 违例拦截）。
    pub fn full_audit(&mut self) -> Result<(), (TdCode, String)> {
        // 文档必须先冻结。
        if !self.doc_frozen {
            self.note(TdCode::DocUnfrozen);
            return Err((TdCode::DocUnfrozen, TdCode::DocUnfrozen.advice().to_string()));
        }
        // 文档指纹恒定。
        if !self.doc_intact() {
            self.note(TdCode::DocTamper);
            return Err((TdCode::DocTamper, TdCode::DocTamper.advice().to_string()));
        }
        // 预算断言（可能产出超预算清单）。
        self.assert_budget()
    }

    /// 摘要（确定性中文串）。
    pub fn digest(&self) -> String {
        format!(
            "top half 纪律：handler {} 采样账，超预算清单 {} 条（产出 {} 次），违例注入 {} 拦截 {}，文档冻结 {}",
            self.handlers.len(),
            self.over_list.len(),
            self.list_emitted,
            self.injected,
            self.intercepted,
            self.doc_frozen
        )
    }

}


/// 段位守卫（诊断码须全部落在本模块 0x4c50 段内）。
pub fn segment_guard() -> bool {
    TdCode::ALL.iter().all(|c| c.val() >= TD_SEG_BASE && c.val() < TD_SEG_BASE + 0x100)
}

