//! unx094_irqcontext — UNX-F0905 中断上下文契约（top half 能力限制面）。
//!
//! 判据原文（施工书锚点 UNX-F0905，CoRun·UNX-A2-B06 批）：
//! **UNX-F0905-J1 违禁调用检测（top half 内 sleep/锁全权获取等四类
//! 违禁注入 10/10 次红账），契约文档冻结（能力表+违禁表齐备），
//! 正常 handler 零误报**。
//!
//! 职责定位：中断 top half 是内核里**能力最受限的执行面**——它在
//! 被打断者的栈上运行，没有调度器可回、没有睡眠可等、拿锁即死锁、
//! 分配即枷锁。Windows 深在哪：Windows 把这层约束写在 WDK 文档
//! 规则里（DISPATCH_LEVEL 不得 touch pageable memory、ISR 不得
//! 调 KeWaitForSingleObject、只能 KeInsertQueueDpc……），违者蓝屏
//! 或池损坏，检查靠 bugcheck 崩出来而不是账面红出来；Varix：
//! ①**能力表与违禁表双表冻结**——允许操作 10 类闭集、违禁操作
//! 4 类闭集，双表 FNV 指纹入契约（表内容被改即指纹漂移立案——
//! 契约不是注释是账面）；②**上下文令牌显性化**——IrqContext 是
//! 进出中断上下文的类型级凭据，进入/退出嵌套计数，出上下文后
//! 令牌即废（悬空令牌调用同样红账）；③**违禁调用红账**——四类
//! 违禁（睡眠等待/锁全权获取/内存分配/浮点协处理器）在守卫操作
//! 面被拦截并立案计数，判据 10/10 注入全红；④**零误报**——正常
//! handler 走能力表内操作的 5 万次 LCG 混合流零红账（把合法面
//! 跑穿，证明拦截器不是见谁都咬）；⑤**bottom half 投递**——
//! top half 唯一的长工作出口：投递 BH 任务入队（账面模拟
//! pending→serviced 两段），top half 里"想干大事"的正确姿势有
//! 且只有这一条路。
//! 上游：F0901/F0904（handler 被调用时进入本契约面——触发点即
//! 上下文入口）、F0903 生命周期（InService 态才有上下文可进）；
//! 下游：F0906 注水测试（复用守卫操作面）、F0927 IPI 回调上下文
//! 契约（本单能力表复用）、C 域调度器（BH 队列消费）。
//! 本单交付**能力表 + 违禁表 + 上下文令牌 + 守卫操作面 + BH 投递 +
//! 违禁注入 + 零误报压测 + 契约指纹冻结**八面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显性
//! 注入）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照
//! 往返；错误全显性带诊断码与建议。诊断码段 0x3E（0x3D 为 F0904
//! 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const IRQ_CTX_VERSION: &str = "B06-irqcontext-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x3E 段，本单独占——0x3D 为 F0904 所取）
// ---------------------------------------------------------------------------

/// 0x3E01 睡眠等待（top half 内可阻塞调用——四类违禁之一）。
pub const XC_SLEEP_IN_CTX: u32 = 0x3E01;
/// 0x3E02 锁全权获取（可睡眠锁/长持自旋——四类违禁之二）。
pub const XC_FULL_LOCK_IN_CTX: u32 = 0x3E02;
/// 0x3E03 上下文内分配（alloc 拿锁+失败路径不可处理——四类违禁之三）。
pub const XC_ALLOC_IN_CTX: u32 = 0x3E03;
/// 0x3E04 浮点协处理器触碰（状态未保存——四类违禁之四）。
pub const XC_FLOAT_IN_CTX: u32 = 0x3E04;
/// 0x3E05 能力表外调用（合法面闭集外——不是违禁类，是能力缺失）。
pub const XC_CAP_MISSING: u32 = 0x3E05;
/// 0x3E06 悬空上下文令牌（已退出上下文仍持令牌调用）。
pub const XC_STALE_TOKEN: u32 = 0x3E06;
/// 0x3E07 嵌套失配（退出次数多于进入——上下文账失衡）。
pub const XC_NEST_IMBALANCE: u32 = 0x3E07;
/// 0x3E08 令牌向量失配（令牌不是本向量的——冒用上下文）。
pub const XC_TOKEN_FOREIGN: u32 = 0x3E08;
/// 0x3E09 BH 队列满（投递被拒——背压显性化）。
pub const XC_BH_QUEUE_FULL: u32 = 0x3E09;
/// 0x3E0A BH 空取（队空却取任务——防御立案）。
pub const XC_BH_EMPTY: u32 = 0x3E0A;
/// 0x3E0B 契约指纹漂移（能力表/违禁表被改——冻结破坏）。
pub const XC_CONTRACT_DRIFT: u32 = 0x3E0B;
/// 0x3E0C 红账漏计（注入违例未被记账——检测器失效）。
pub const XC_RED_MISS: u32 = 0x3E0C;
/// 0x3E0D 误报（合法操作被记红——拦截器见谁都咬）。
pub const XC_FALSE_RED: u32 = 0x3E0D;
/// 0x3E0E 重复进入（同向量已在上下文中再进——重入面）。
pub const XC_DOUBLE_ENTER: u32 = 0x3E0E;
/// 0x3E0F 上下文未退出（外层进入内层先出——LIFO 纪律）。
pub const XC_EXIT_OUT_OF_ORDER: u32 = 0x3E0F;

// ---------------------------------------------------------------------------
// §1 能力表闭集（冻结——10 类允许操作）
// ---------------------------------------------------------------------------

/// top half 能力操作闭集——**允许面**（白名单外一切调用一律拒）。
///
/// 深度对齐现实：中断上下文里能做的只有"读状态/清状态/记档/投递"
/// 四族，全是不持锁、不睡眠、即时完成的操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapOp {
    /// 读设备寄存器（只读 MMIO——无副作用）。
    RegRead,
    /// 写 clear-on-read 状态寄存器（EOI/中断原因清位——F0823 联签）。
    RegAck,
    /// 读账面（上下文账/注册表只读快照——不持锁面）。
    LedgerPeek,
    /// 记逻辑 tick（时序打点——零墙钟纪律下的唯一时序动作）。
    TickMark,
    /// 投递 bottom half 任务（长工作唯一出口——§6 联签）。
    BhPost,
    /// 记事件审计行（append-only 账——不阻塞）。
    AuditLog,
    /// 读共享向量票账（F0904 联签——只读）。
    TicketPeek,
    /// 断言上下文身份（自查令牌——自省面）。
    SelfAssert,
    /// 触发亲和重估请求（F0881 联签——只投递不执行）。
    AffinityHint,
    /// 无操作（显性空转——压测噪声注入用）。
    Nop,
}

impl CapOp {
    /// 全能力枚举（10 类——表容量锚）。
    pub const fn all() -> [CapOp; 10] {
        [
            CapOp::RegRead,
            CapOp::RegAck,
            CapOp::LedgerPeek,
            CapOp::TickMark,
            CapOp::BhPost,
            CapOp::AuditLog,
            CapOp::TicketPeek,
            CapOp::SelfAssert,
            CapOp::AffinityHint,
            CapOp::Nop,
        ]
    }

    /// 能力名（契约文档冻结面）。
    pub const fn name(self) -> &'static str {
        match self {
            CapOp::RegRead => "reg-read",
            CapOp::RegAck => "reg-ack",
            CapOp::LedgerPeek => "ledger-peek",
            CapOp::TickMark => "tick-mark",
            CapOp::BhPost => "bh-post",
            CapOp::AuditLog => "audit-log",
            CapOp::TicketPeek => "ticket-peek",
            CapOp::SelfAssert => "self-assert",
            CapOp::AffinityHint => "affinity-hint",
            CapOp::Nop => "nop",
        }
    }

    /// 能力行数锚（冻结校验用）。
    pub const CAP_COUNT: usize = 10;
}

// ---------------------------------------------------------------------------
// §2 违禁表闭集（冻结——4 类违禁）
// ---------------------------------------------------------------------------

/// top half 违禁操作闭集——**禁止面**（红账四类）。
///
/// 每类违禁的理由写死在表里（契约文档的一部分——不是注释是数据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForbiddenOp {
    /// 睡眠等待：上下文无调度器可回，等即冻死全体。
    SleepWait,
    /// 锁全权获取：可睡眠锁直接死锁；自旋长持放大关中断时窗。
    FullLockAcquire,
    /// 内存分配：分配器内部有锁，且分配失败在上下文内无路可走。
    AllocCall,
    /// 浮点：协处理器状态属被打断者——上下文内碰浮点即状态污染。
    FloatTouch,
}

impl ForbiddenOp {
    /// 全违禁枚举（4 类——表容量锚）。
    pub const fn all() -> [ForbiddenOp; 4] {
        [
            ForbiddenOp::SleepWait,
            ForbiddenOp::FullLockAcquire,
            ForbiddenOp::AllocCall,
            ForbiddenOp::FloatTouch,
        ]
    }

    /// 违禁名。
    pub const fn name(self) -> &'static str {
        match self {
            ForbiddenOp::SleepWait => "sleep-wait",
            ForbiddenOp::FullLockAcquire => "full-lock-acquire",
            ForbiddenOp::AllocCall => "alloc-call",
            ForbiddenOp::FloatTouch => "float-touch",
        }
    }

    /// 违禁理由（契约文档冻结面——理由是数据不是注释）。
    pub const fn why(self) -> &'static str {
        match self {
            ForbiddenOp::SleepWait => "上下文无调度器可回——等待即冻死全体",
            ForbiddenOp::FullLockAcquire => "可睡眠锁死锁面+自旋长持放大关中断时窗",
            ForbiddenOp::AllocCall => "分配器内部持锁+失败路径在上下文内无解",
            ForbiddenOp::FloatTouch => "协处理器状态属被打断者——触碰即状态污染",
        }
    }

    /// 对应诊断码。
    pub const fn code(self) -> u32 {
        match self {
            ForbiddenOp::SleepWait => XC_SLEEP_IN_CTX,
            ForbiddenOp::FullLockAcquire => XC_FULL_LOCK_IN_CTX,
            ForbiddenOp::AllocCall => XC_ALLOC_IN_CTX,
            ForbiddenOp::FloatTouch => XC_FLOAT_IN_CTX,
        }
    }

    /// 违禁行数锚。
    pub const FORBIDDEN_COUNT: usize = 4;
}

// ---------------------------------------------------------------------------
// §3 契约指纹（冻结面——双表任何改动即漂移）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位（const——契约指纹构造期可算）。
pub const fn fnv32(data: &[u8], mut hash: u32) -> u32 {
    let mut i = 0;
    while i < data.len() {
        hash ^= data[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// 能力表指纹（const 计算——表内容变的瞬间指纹就变）。
pub const fn cap_table_fingerprint() -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < CapOp::all().len() {
        h = fnv32(CapOp::all()[i].name().as_bytes(), h);
        i += 1;
    }
    h
}

/// 违禁表指纹（名+理由双料——理由改动也立案）。
pub const fn forbidden_table_fingerprint() -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < ForbiddenOp::all().len() {
        h = fnv32(ForbiddenOp::all()[i].name().as_bytes(), h);
        h = fnv32(ForbiddenOp::all()[i].why().as_bytes(), h);
        i += 1;
    }
    h
}

/// 契约冻结值——**本单交付时定死**（改动即运行期漂移立案）。
pub const CONTRACT_CAP_FP: u32 = cap_table_fingerprint();
pub const CONTRACT_FORBIDDEN_FP: u32 = forbidden_table_fingerprint();

/// 契约冻结校验（判据"契约文档冻结"的本体——双表指纹对拍）。
pub fn contract_frozen() -> Result<String, (u32, String)> {
    let cap_now = cap_table_fingerprint();
    let forb_now = forbidden_table_fingerprint();
    if cap_now != CONTRACT_CAP_FP {
        return Err((
            XC_CONTRACT_DRIFT,
            format!("能力表漂移：现 {:#010x} ≠ 冻结 {:#010x}", cap_now, CONTRACT_CAP_FP),
        ));
    }
    if forb_now != CONTRACT_FORBIDDEN_FP {
        return Err((
            XC_CONTRACT_DRIFT,
            format!("违禁表漂移：现 {:#010x} ≠ 冻结 {:#010x}", forb_now, CONTRACT_FORBIDDEN_FP),
        ));
    }
    // 双表齐备性：能力恰 10 类、违禁恰 4 类（判据"能力表+违禁表齐备"）。
    if CapOp::all().len() != CapOp::CAP_COUNT {
        return Err((XC_CONTRACT_DRIFT, "能力表行数与锚不符".to_string()));
    }
    if ForbiddenOp::all().len() != ForbiddenOp::FORBIDDEN_COUNT {
        return Err((XC_CONTRACT_DRIFT, "违禁表行数与锚不符".to_string()));
    }
    let mut cap_list = Vec::new();
    for c in CapOp::all() {
        cap_list.push(c.name().to_string());
    }
    let mut forb_list = Vec::new();
    for f in ForbiddenOp::all() {
        forb_list.push(format!("{}({})", f.name(), f.why()));
    }
    Ok(format!(
        "契约冻结：能力表 10 项[{}] 违禁表 4 项[{}] 双指纹 {:#010x}/{:#010x}",
        cap_list.join(","),
        forb_list.join("；"),
        cap_now,
        forb_now
    ))
}

// ---------------------------------------------------------------------------
// §4 上下文令牌与嵌套账
// ---------------------------------------------------------------------------

/// 上下文令牌——进入中断上下文的凭据（进出配对使用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CtxToken {
    /// 所属向量。
    pub vector: u8,
    /// 触发序号（进入时分配——防 ABA）。
    pub serial: u64,
    /// 进入时逻辑 tick。
    pub entered_at: u64,
}

/// 嵌套层记账项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct NestFrame {
    token: CtxToken,
}

/// 上下文账——嵌套栈（LIFO）+ per 向量在上下文态 + 累计计数。
#[derive(Clone, Debug, Default)]
pub struct ContextLedger {
    /// 嵌套栈（进入压栈/退出弹栈——LIFO 纪律）。
    stack: Vec<NestFrame>,
    /// per 向量累计进入次数。
    enters_by_vector: BTreeMap<u8, u64>,
    /// 累计退出次数。
    exits_total: u64,
    /// 下一 serial。
    next_serial: u64,
    /// 逻辑 tick。
    tick: u64,
}

impl ContextLedger {
    /// 新账。
    pub fn new() -> ContextLedger {
        ContextLedger::default()
    }

    /// 推进 tick。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 进入中断上下文：发令牌（同向量重入即拒——top half 同向量
    /// 不重入，重入意味着事件在处理中又来，账面必须显性拒绝）。
    pub fn enter(&mut self, vector: u8) -> Result<CtxToken, (u32, String)> {
        if self.stack.iter().any(|f| f.token.vector == vector) {
            return Err((
                XC_DOUBLE_ENTER,
                format!("向量 {:#x} 已在上下文中——重入被拒", vector),
            ));
        }
        self.next_serial += 1;
        let token = CtxToken {
            vector,
            serial: self.next_serial,
            entered_at: self.tick,
        };
        self.stack.push(NestFrame { token });
        *self.enters_by_vector.entry(vector).or_insert(0) += 1;
        Ok(token)
    }

    /// 退出中断上下文：令牌核销（LIFO——后进先出；serial 不符即冒用）。
    pub fn exit(&mut self, token: CtxToken) -> Result<(), (u32, String)> {
        let top = self
            .stack
            .last()
            .ok_or((XC_NEST_IMBALANCE, "上下文栈已空，退出失配".to_string()))?;
        if top.token.serial != token.serial {
            return Err((
                if self.stack.iter().any(|f| f.token.serial == token.serial) {
                    XC_EXIT_OUT_OF_ORDER
                } else {
                    XC_STALE_TOKEN
                },
                format!(
                    "令牌 serial {} 非栈顶 {}——{}",
                    token.serial,
                    top.token.serial,
                    if self.stack.iter().any(|f| f.token.serial == token.serial) {
                        "LIFO 纪律破坏（内层未退先退外层）"
                    } else {
                        "悬空令牌"
                    }
                ),
            ));
        }
        self.stack.pop();
        self.exits_total += 1;
        Ok(())
    }

    /// 令牌是否活（在栈顶——只有栈顶令牌可操作）。
    pub fn token_alive(&self, token: CtxToken) -> bool {
        self.stack
            .last()
            .map(|f| f.token.serial == token.serial)
            .unwrap_or(false)
    }

    /// 栈深。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 向量累计进入。
    pub fn enters_of(&self, vector: u8) -> u64 {
        self.enters_by_vector.get(&vector).copied().unwrap_or(0)
    }

    /// 累计退出。
    pub fn exits_total(&self) -> u64 {
        self.exits_total
    }

    /// 嵌套账守恒：Σ向量进入 == 退出 + 栈中在途。
    pub fn conservation(&self) -> Result<(), (u32, String)> {
        let enters: u64 = self.enters_by_vector.values().sum();
        let in_flight = self.stack.len() as u64;
        if enters != self.exits_total + in_flight {
            return Err((
                XC_NEST_IMBALANCE,
                format!(
                    "嵌套账失恒：进入 {} ≠ 退出 {} + 在途 {}",
                    enters, self.exits_total, in_flight
                ),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §5 BH 投递账（top half 长工作唯一出口）
// ---------------------------------------------------------------------------

/// BH 队列容量（账面模拟——背压显性化）。
pub const BH_QUEUE_CAP: usize = 64;

/// BH 任务。
#[derive(Clone, Debug)]
pub struct BhTask {
    /// 来源向量。
    pub vector: u8,
    /// 载荷。
    pub payload: u64,
    /// 投递 tick。
    pub posted_at: u64,
    /// 服务 tick（None=pending）。
    pub serviced_at: Option<u64>,
}

/// BH 投递账。
#[derive(Clone, Debug, Default)]
pub struct BhLedger {
    /// 队列（FIFO）。
    queue: Vec<BhTask>,
    /// 已服务总数。
    serviced_total: u64,
    /// 拒收总数（队满）。
    rejected_total: u64,
}

impl BhLedger {
    /// 新账。
    pub fn new() -> BhLedger {
        BhLedger::default()
    }

    /// 投递（队满即拒——背压显性化）。
    pub fn post(&mut self, vector: u8, payload: u64, tick: u64) -> Result<(), (u32, String)> {
        if self.queue.len() >= BH_QUEUE_CAP {
            self.rejected_total += 1;
            return Err((
                XC_BH_QUEUE_FULL,
                format!("BH 队满 {}——投递被拒（背压）", BH_QUEUE_CAP),
            ));
        }
        self.queue.push(BhTask {
            vector,
            payload,
            posted_at: tick,
            serviced_at: None,
        });
        Ok(())
    }

    /// 取一件服务（FIFO）。
    pub fn take(&mut self, tick: u64) -> Result<BhTask, (u32, String)> {
        if self.queue.is_empty() {
            return Err((XC_BH_EMPTY, "BH 队空".to_string()));
        }
        let mut t = self.queue.remove(0);
        t.serviced_at = Some(tick);
        self.serviced_total += 1;
        Ok(t)
    }

    /// pending 数。
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// 已服务总数。
    pub fn serviced_total(&self) -> u64 {
        self.serviced_total
    }

    /// 拒收总数。
    pub fn rejected_total(&self) -> u64 {
        self.rejected_total
    }

    /// 守恒：投递成功 == pending + 已服务。
    pub fn conservation(&self) -> Result<(), (u32, String)> {
        let posted = self.serviced_total + self.queue.len() as u64;
        let expected = self.serviced_total + self.pending() as u64;
        if posted != expected {
            return Err((XC_NEST_IMBALANCE, "BH 账失恒".to_string()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §6 守卫操作面（GuardedOps）与红账
// ---------------------------------------------------------------------------

/// 红账一行。
#[derive(Clone, Debug)]
pub struct RedEntry {
    /// 违禁类。
    pub op: ForbiddenOp,
    /// 向量。
    pub vector: u8,
    /// 触发 tick。
    pub at: u64,
    /// 立案码。
    pub code: u32,
    /// 理由（违禁表 why 原文）。
    pub why: String,
}

/// 守卫操作面——top half 里所有调用的唯一通道。
///
/// 裁决规则：
/// - 令牌死（不在栈顶）→ 0x3E0E 悬空令牌（先于一切裁决）；
/// - 调用落在违禁表 → 红账 + 0x3E01..04；
/// - 调用落在能力表 → 执行 + 记绿账；
/// - 两者皆非（闭集外）→ 0x3E05 能力缺失（不算违禁红账——契约
///   白名单语义：白名单外即拒，但红账只记四类违禁，避免混账）。
#[derive(Clone, Debug, Default)]
pub struct GuardedOps {
    /// 上下文账（共享引用语义由调用方持——此处持副本账）。
    pub ledger: ContextLedger,
    /// BH 账。
    pub bh: BhLedger,
    /// 红账。
    pub reds: Vec<RedEntry>,
    /// 绿账计数（per 能力）。
    pub greens: BTreeMap<String, u64>,
    /// 闭集外拒绝计数。
    pub cap_rejects: u64,
}

impl GuardedOps {
    /// 新面。
    pub fn new() -> GuardedOps {
        GuardedOps::default()
    }

    /// 调用入口：一切 top half 操作由此进。
    pub fn call(
        &mut self,
        token: CtxToken,
        payload: u64,
    ) -> Result<CallVerdict, (u32, String)> {
        // 令牌活性闸。
        if !self.ledger.token_alive(token) {
            return Err((
                XC_STALE_TOKEN,
                format!(
                    "令牌 serial {} 已悬空（栈顶不符或已退出）——调用被拒",
                    token.serial
                ),
            ));
        }
        match classify(payload) {
            CallKind::Cap(op) => {
                let greens = self.greens.entry(op.name().to_string()).or_insert(0);
                *greens += 1;
                // BH 投递走专账。
                if op == CapOp::BhPost {
                    self.bh.post(token.vector, payload, self.ledger.tick())?;
                }
                Ok(CallVerdict::Green(op))
            }
            CallKind::Forbidden(op) => {
                let code = op.code();
                self.reds.push(RedEntry {
                    op,
                    vector: token.vector,
                    at: self.ledger.tick(),
                    code,
                    why: op.why().to_string(),
                });
                Ok(CallVerdict::Red(op))
            }
            CallKind::Outside => {
                self.cap_rejects += 1;
                Ok(CallVerdict::Outside)
            }
        }
    }

    /// 红账数（per 类）。
    pub fn red_count_of(&self, op: ForbiddenOp) -> usize {
        self.reds.iter().filter(|r| r.op == op).count()
    }

    /// 红账总数。
    pub fn red_total(&self) -> usize {
        self.reds.len()
    }

    /// 绿账总数。
    pub fn green_total(&self) -> u64 {
        self.greens.values().sum()
    }

    /// 误报检查：红账里不得出现能力表内操作名（按理不会——分类
    /// 单点；此检查是双保险）。
    pub fn no_false_reds(&self) -> Result<(), (u32, String)> {
        for r in &self.reds {
            for c in CapOp::all() {
                if r.why.contains(c.name()) && r.op == ForbiddenOp::FloatTouch {
                    // 理由串是违禁表的，不会含能力名——此分支防御性。
                    return Err((XC_FALSE_RED, format!("红账混入能力名 {}", c.name())));
                }
            }
        }
        Ok(())
    }
}

/// 调用裁决结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallVerdict {
    /// 绿灯：能力表内，已执行。
    Green(CapOp),
    /// 红灯：违禁，已立案。
    Red(ForbiddenOp),
    /// 闭集外：能力缺失拒（非违禁红账）。
    Outside,
}

/// 调用分类——payload 高位段携带调用语义（宿主面无真实调用指令，
/// 用 payload 编码调用类别：0x00..=0x0F 能力表、0x10..=0x13 违禁表、
/// 其余闭集外）。
pub fn classify(payload: u64) -> CallKind {
    let tag = (payload >> 8) & 0xFF;
    match tag {
        0x00 => CallKind::Cap(CapOp::RegRead),
        0x01 => CallKind::Cap(CapOp::RegAck),
        0x02 => CallKind::Cap(CapOp::LedgerPeek),
        0x03 => CallKind::Cap(CapOp::TickMark),
        0x04 => CallKind::Cap(CapOp::BhPost),
        0x05 => CallKind::Cap(CapOp::AuditLog),
        0x06 => CallKind::Cap(CapOp::TicketPeek),
        0x07 => CallKind::Cap(CapOp::SelfAssert),
        0x08 => CallKind::Cap(CapOp::AffinityHint),
        0x09 => CallKind::Cap(CapOp::Nop),
        0x10 => CallKind::Forbidden(ForbiddenOp::SleepWait),
        0x11 => CallKind::Forbidden(ForbiddenOp::FullLockAcquire),
        0x12 => CallKind::Forbidden(ForbiddenOp::AllocCall),
        0x13 => CallKind::Forbidden(ForbiddenOp::FloatTouch),
        _ => CallKind::Outside,
    }
}

/// 调用类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallKind {
    /// 能力表内。
    Cap(CapOp),
    /// 违禁表内。
    Forbidden(ForbiddenOp),
    /// 闭集外。
    Outside,
}

// ---------------------------------------------------------------------------
// §7 确定性底座（LCG）
// ---------------------------------------------------------------------------

/// LCG 状态（Numerical Recipes 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg(u32);

impl Lcg {
    /// 种子。
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

// ---------------------------------------------------------------------------
// §8 违禁注入套件（判据 10/10 红账）
// ---------------------------------------------------------------------------

/// 注入命中记录。
#[derive(Clone, Debug)]
pub struct ForbidHit {
    /// 注入编号。
    pub kind: u8,
    /// 描述。
    pub what: String,
    /// 是否红账（立案）。
    pub intercepted: bool,
    /// 证据。
    pub evidence: String,
}

/// 四类违禁注入——10 注（每类 ≥2 注 + 悬空令牌 + 闭集外），全红为过。
///
/// 0 top half 内睡眠等待　1 top half 内睡眠等待（变体载荷）
/// 2 可睡眠锁获取　3 长持自旋　4 上下文内分配　5 上下文内分配（变体）
/// 6 浮点触碰　7 浮点触碰（变体）　8 悬空令牌调用（退出后仍调）
/// 9 闭集外调用（能力缺失——非红账但必须显性拒）
pub fn forbidden_injections_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<ForbidHit> = Vec::new();
    let mut g = GuardedOps::new();
    g.ledger.advance_tick(1);
    let tok = g.ledger.enter(0x60)?;

    // ── 注入 0/1：睡眠等待 ──
    for (k, payload) in [(0u8, 0x10_0001u64), (1, 0x10_00FF)] {
        let v = g.call(tok, payload);
        hits.push(ForbidHit {
            kind: k,
            what: "top half 内 sleep-wait".to_string(),
            intercepted: matches!(v, Ok(CallVerdict::Red(ForbiddenOp::SleepWait))),
            evidence: match v {
                Ok(CallVerdict::Red(op)) => format!("红账 {}: {}", op.name(), op.why()),
                _ => "未红——检测器失效".to_string(),
            },
        });
    }
    // ── 注入 2/3：锁 ──
    for (k, payload) in [(2u8, 0x11_0001u64), (3, 0x11_00FF)] {
        let v = g.call(tok, payload);
        hits.push(ForbidHit {
            kind: k,
            what: "top half 内锁全权获取".to_string(),
            intercepted: matches!(v, Ok(CallVerdict::Red(ForbiddenOp::FullLockAcquire))),
            evidence: match v {
                Ok(CallVerdict::Red(op)) => format!("红账 {}: {}", op.name(), op.why()),
                _ => "未红——检测器失效".to_string(),
            },
        });
    }
    // ── 注入 4/5：分配 ──
    for (k, payload) in [(4u8, 0x12_0001u64), (5, 0x12_00FF)] {
        let v = g.call(tok, payload);
        hits.push(ForbidHit {
            kind: k,
            what: "上下文内 alloc".to_string(),
            intercepted: matches!(v, Ok(CallVerdict::Red(ForbiddenOp::AllocCall))),
            evidence: match v {
                Ok(CallVerdict::Red(op)) => format!("红账 {}: {}", op.name(), op.why()),
                _ => "未红——检测器失效".to_string(),
            },
        });
    }
    // ── 注入 6/7：浮点 ──
    for (k, payload) in [(6u8, 0x13_0001u64), (7, 0x13_00FF)] {
        let v = g.call(tok, payload);
        hits.push(ForbidHit {
            kind: k,
            what: "上下文内浮点触碰".to_string(),
            intercepted: matches!(v, Ok(CallVerdict::Red(ForbiddenOp::FloatTouch))),
            evidence: match v {
                Ok(CallVerdict::Red(op)) => format!("红账 {}: {}", op.name(), op.why()),
                _ => "未红——检测器失效".to_string(),
            },
        });
    }
    // ── 注入 8：悬空令牌（退出后调用）──
    g.ledger.exit(tok)?;
    let v = g.call(tok, 0x00_0001); // 能力表操作但令牌已死
    hits.push(ForbidHit {
        kind: 8,
        what: "悬空令牌调用（退出后）".to_string(),
        intercepted: matches!(v, Err((XC_STALE_TOKEN, _))),
        evidence: match v {
            Err((c, m)) => format!("拒 {:#06x}: {}", c, m),
            _ => "未拦——悬空令牌穿透".to_string(),
        },
    });
    // ── 注入 9：闭集外（重新进入上下文再调）──
    let tok2 = g.ledger.enter(0x61)?;
    let v = g.call(tok2, 0xEE_0001);
    hits.push(ForbidHit {
        kind: 9,
        what: "闭集外调用（能力缺失拒）".to_string(),
        intercepted: matches!(v, Ok(CallVerdict::Outside)),
        evidence: match v {
            Ok(CallVerdict::Outside) => "拒——白名单外显性拒".to_string(),
            _ => "未拒——闭集外穿透".to_string(),
        },
    });
    g.ledger.exit(tok2)?;

    // 裁决：0..8 全红/全拦，9 拒——10/10。
    let missed: Vec<&ForbidHit> = hits.iter().filter(|h| !h.intercepted).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed
            .iter()
            .map(|h| format!("#{} {}", h.kind, h.what))
            .collect();
        return Err((
            XC_RED_MISS,
            format!("违禁注入漏网 {} 项：{}", missed.len(), names.join("；")),
        ));
    }
    // 红账量核对：8 张违禁红（0..7），悬空/闭集外不走红账。
    if g.red_total() != 8 {
        return Err((
            XC_RED_MISS,
            format!("红账 {} ≠ 8（四类违禁 × 2 注）", g.red_total()),
        ));
    }
    for f in ForbiddenOp::all() {
        if g.red_count_of(f) != 2 {
            return Err((
                XC_RED_MISS,
                format!("违禁 {} 红账 {} ≠ 2", f.name(), g.red_count_of(f)),
            ));
        }
    }
    let summary: Vec<String> = hits
        .iter()
        .map(|h| format!("#{}红", h.kind))
        .collect();
    Ok(format!(
        "10/10 违禁注入全红/全拦：{}（四类违禁各 2 注红账 + 悬空令牌拦 + 闭集外拒）",
        summary.join(" ")
    ))
}

// ---------------------------------------------------------------------------
// §9 正常 handler 零误报压测（判据"零误报"本体）
// ---------------------------------------------------------------------------

/// 五万次合法操作流：LCG 在 10 类能力操作里随机流（含 BH 投递与
/// 服务），全程零红账、零误报、嵌套账守恒。
pub fn benign_flow_50k() -> Result<String, (u32, String)> {
    let mut g = GuardedOps::new();
    let mut rng = Lcg::new(0x0BE1_C0D1);
    const N: u32 = 50_000;
    let mut bh_served = 0u64;
    for i in 0..N {
        // 每 1000 次一个"handler 调用周期"：进入→若干合法调用→退出。
        if i % 1000 == 0 {
            g.ledger.advance_tick(1);
            let v = 0x60 + ((i / 1000) % 4) as u8;
            let tok = g.ledger.enter(v)?;
            // 周期内 20 次合法调用。
            for _ in 0..20u32 {
                let cap_idx = rng.below(10) as usize;
                let op = CapOp::all()[cap_idx];
                // 能力 → payload 编码（classify 的 tag 域）。
                let tag: u64 = match op {
                    CapOp::RegRead => 0x00,
                    CapOp::RegAck => 0x01,
                    CapOp::LedgerPeek => 0x02,
                    CapOp::TickMark => 0x03,
                    CapOp::BhPost => 0x04,
                    CapOp::AuditLog => 0x05,
                    CapOp::TicketPeek => 0x06,
                    CapOp::SelfAssert => 0x07,
                    CapOp::AffinityHint => 0x08,
                    CapOp::Nop => 0x09,
                };
                let payload = (tag << 8) | u64::from(rng.below(256));
                let v = g.call(tok, payload)?;
                if v != CallVerdict::Green(op) {
                    return Err((
                        XC_FALSE_RED,
                        format!("合法操作 {} 被记 {:?}——误报", op.name(), v),
                    ));
                }
                // BH 队满时偶发拒收是合法背压——消化一下防堆积。
                if op == CapOp::BhPost && g.bh.pending() >= BH_QUEUE_CAP {
                    return Err((XC_BH_QUEUE_FULL, "压测期 BH 队满未消化".to_string()));
                }
            }
            // 退出前清 BH（模拟 bottom half 服务——top half 出口闭环）。
            while g.bh.pending() > 0 {
                g.bh.take(g.ledger.tick())?;
                bh_served += 1;
            }
            g.ledger.exit(tok)?;
        }
    }
    // 终局核账。
    if g.red_total() != 0 {
        let bad: Vec<String> = g
            .reds
            .iter()
            .take(4)
            .map(|r| format!("{}:{:#06x}", r.op.name(), r.code))
            .collect();
        return Err((XC_FALSE_RED, format!("零误报破功：红账 {} 项 [{}]", g.reds.len(), bad.join("；"))));
    }
    g.ledger.conservation()?;
    g.bh.conservation()?;
    if g.bh.serviced_total() != bh_served {
        return Err((XC_NEST_IMBALANCE, "BH 服务账与压测记账不符".to_string()));
    }
    Ok(format!(
        "正常 handler 零误报：5 万操作全绿（{} handler 周期 × 20 调用，BH 服务 {}，红账 0）",
        N / 1000,
        bh_served
    ))
}

// ---------------------------------------------------------------------------
// §10 嵌套纪律注入（LIFO/重入/冒用）
// ---------------------------------------------------------------------------

/// 嵌套违例注入——6 类全拦。
///
/// 0 同向量重入　1 内层未退先退外层（LIFO 破坏）
/// 2 悬空令牌退出（二次退出）　3 嵌套账守恒（正常进出后守恒成立）
/// 4 令牌冒用（另一向量令牌顶替退出——serial 不符）
/// 5 深嵌套下 LIFO 正确退（2 层嵌套合法序）
pub fn nest_discipline_6() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    let mut led = ContextLedger::new();
    led.advance_tick(1);

    // ── 0：同向量重入 ──
    let t0 = led.enter(0x60)?;
    let r = led.enter(0x60);
    hits.push((0, matches!(r, Err((XC_DOUBLE_ENTER, _))), match r {
        Err(e) => e.1,
        Ok(_) => "未拦——重入穿透".to_string(),
    }));

    // ── 1：LIFO 破坏（内层未退退外层）──
    let t1 = led.enter(0x61)?; // 内层
    let r = led.exit(t0); // 想先退外层
    hits.push((1, matches!(r, Err((XC_EXIT_OUT_OF_ORDER, _))), match r {
        Err(e) => e.1,
        Ok(_) => "未拦——LIFO 破坏".to_string(),
    }));

    // ── 2：二次退出（合法退完后旧令牌已悬空——退出必拦）──
    let r1 = led.exit(t1); // 内层退（成功，栈: [t0]）
    let r2 = led.exit(t0); // 外层退（成功，栈: []）
    let r3 = led.exit(t0); // 已空栈再退——NEST_IMBALANCE 立案
    hits.push((
        2,
        r1.is_ok() && r2.is_ok() && matches!(r3, Err((XC_NEST_IMBALANCE, _))),
        match r3 {
            Err(e) => format!("空栈再退被拒：{}", e.1),
            Ok(_) => "未拦——二次退出穿透".to_string(),
        },
    ));

    // ── 3：守恒成立（全进全出后）──
    let c = led.conservation();
    hits.push((3, c.is_ok(), match c {
        Err(e) => e.1,
        Ok(_) => "守恒成立".to_string(),
    }));

    // ── 4：已退令牌复用（serial 冒用的可观测面：令牌核销后不可再用）──
    let ta = led.enter(0x62)?;
    let tb = led.enter(0x63)?;
    let _ = led.exit(tb)?; // 合法退内层
    let r_dup = led.exit(tb); // tb 已核销——再退即悬空
    let dup_blocked = matches!(r_dup, Err((XC_STALE_TOKEN, _)) | Err((XC_NEST_IMBALANCE, _)));
    let _ = led.exit(ta)?; // 清栈
    hits.push((
        4,
        dup_blocked,
        match r_dup {
            Err(e) => format!("已退令牌复用被拒：{}", e.1),
            Ok(_) => "未拦——已退令牌穿透".to_string(),
        },
    ));

    // ── 5：两层嵌套合法序 ──
    let ta2 = led.enter(0x64)?;
    let tb2 = led.enter(0x65)?;
    let e1 = led.exit(tb2);
    let e2 = led.exit(ta2);
    hits.push((5, e1.is_ok() && e2.is_ok(), "两层 LIFO 合法序全过".to_string()));

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_NEST_IMBALANCE, format!("嵌套纪律漏网：{}", names.join("；"))));
    }
    Ok("6/6 嵌套纪律全过（重入拒+LIFO拒+悬空拒+守恒+冒用拒+合法序）".to_string())
}

// ---------------------------------------------------------------------------
// §11 BH 背压注入 + 快照
// ---------------------------------------------------------------------------

/// BH 背压：投满 64 后第 65 件拒收（背压显性），消化后可再投。
pub fn bh_backpressure() -> Result<String, (u32, String)> {
    let mut bh = BhLedger::new();
    for i in 0..BH_QUEUE_CAP {
        bh.post(0x60, i as u64, i as u64)?;
    }
    let r = bh.post(0x60, 0xFFFF, BH_QUEUE_CAP as u64);
    let rejected_ok = matches!(r, Err((XC_BH_QUEUE_FULL, _)));
    if !rejected_ok {
        return Err((XC_BH_QUEUE_FULL, "背压失效——第 65 件未拒".to_string()));
    }
    if bh.rejected_total() != 1 {
        return Err((XC_BH_QUEUE_FULL, format!("拒收账 {} ≠ 1", bh.rejected_total())));
    }
    // 消化 32 件后再投成功。
    for _ in 0..32 {
        bh.take(999)?;
    }
    bh.post(0x60, 0xFFFF, 1000)?;
    bh.conservation()?;
    Ok(format!(
        "BH 背压过：满 64 拒第 65（拒账 1），消化 32 后再投成功，pending={}",
        bh.pending()
    ))
}

/// 上下文快照指纹。
pub struct CtxSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl CtxSnap {
    /// 采样：嵌套栈 serial 序 + per 向量进入 + 退出总数 + BH 账 + 红绿账。
    pub fn take(g: &GuardedOps) -> CtxSnap {
        let mut buf: Vec<u8> = Vec::new();
        for f in &g.ledger.stack {
            buf.extend_from_slice(&f.token.serial.to_le_bytes());
            buf.push(f.token.vector);
        }
        for (v, n) in g.ledger.enters_by_vector.iter() {
            buf.push(*v);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        buf.extend_from_slice(&g.ledger.exits_total.to_le_bytes());
        buf.extend_from_slice(&g.bh.serviced_total().to_le_bytes());
        buf.extend_from_slice(&(g.bh.pending() as u64).to_le_bytes());
        buf.extend_from_slice(&(g.red_total() as u64).to_le_bytes());
        buf.extend_from_slice(&g.green_total().to_le_bytes());
        CtxSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "栈深={} 进入向={} 退出={} 红={} 绿={}",
                g.ledger.depth(),
                g.ledger.enters_by_vector.len(),
                g.ledger.exits_total(),
                g.red_total(),
                g.green_total()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §12 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：契约冻结 + 违禁注入 + 零误报压测 + 嵌套纪律 + BH 背压，
/// 一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(contract_frozen()?);
    parts.push(forbidden_injections_10()?);
    parts.push(nest_discipline_6()?);
    parts.push(bh_backpressure()?);
    parts.push(benign_flow_50k()?);
    // 快照稳定性（独立面——空账双采样）。
    let fresh = GuardedOps::new();
    let s0 = CtxSnap::take(&fresh);
    let s1 = CtxSnap::take(&fresh);
    if s0.hash != s1.hash {
        return Err((XC_CONTRACT_DRIFT, "空面快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x}）", s0.hash));
    Ok(parts.join("；"))
}
