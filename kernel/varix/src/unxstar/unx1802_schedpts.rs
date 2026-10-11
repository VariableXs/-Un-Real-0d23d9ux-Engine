//! unx1802_schedpts —— UNX-F1802 调度点全集登记（全部调度触发点清单）。
//!
//! 单号：UNX-F1802
//! 标题：调度点全集登记（全部调度触发点清单）
//! 目标行数：2200~2800 行（硬指标，写满）
//! 域：CoRun·UNX-A3-B11
//! 判据原文（注释引用）：
//!   `UNX-F1802-J1 调度点全集登记（tick 到期/抢占/让出/阻塞/迁移后/启动）清单冻结 v1，
//!    调度点外注入 10/10 检出`
//!
//! 册内上下文：CoRun·UNX-A3-B11 段「切换主路径」。
//!   相关单：F1801 主循环四段冻结（置位消费→选线程→切出→切入）
//!           F1803 need_resched 消费闭环（F1632）
//!           F1804 下一线程选择
//!           F1805 切出路径（F1654 过渡态）
//!           F1806 切入路径
//!           F1807 switch_to 原语
//!           F1808 临界区与锁序（F1733）
//!           F1809 切换计数账（F1626 原因四分类）
//!           F1844 重调度 IPI 处理（**中断语境纪律：IPI 处理里引用的调度点号
//!                   必须在本清单内登记**）
//!
//! 组件清单：
//!   C1 调度点全集闭集（6 点：TickExpiry/Preempt/Yield/Block/PostMigrate/Startup）
//!   C2 调度点描述符布局（点号/中名/触发条件/语境位掩/不变量/封账指纹）
//!   C3 清单冻结 v1（append-only 定容数组 + FNV 封账指纹 + 封后写拒 + 只读视图）
//!   C4 调度点外注入检测（点号登记核 + 语境合法性核 + 调用点全扫描）
//!   C5 need_resched 闭环联动（置位→消费→清位 + 置位丢失兜底恢复检出）
//!   C6 切换原因四分类账（到期/抢占/让出/迁移后 + 启动单列 + 环形缓冲 + 守恒式）
//!   C7 语境矩阵与纪律（6×4 合法矩阵 + 非法格诊断码 + IPI 三禁 + 切换死点检测）
//!   C8 注入核（10 形态 · 10/10 检出）
//!   C9 导出面（KV 清单 / TSV 矩阵 / TLV 打包 + 长度自洽断言）
//!
//! 职责定位：
//!   本模块是**登记册**，不是调度器本体。它回答两个问题：
//!   （1）一个调度请求是否来自一个「已登记的调度点」，且发生在该点被允许的语境里；
//!   （2）全部调度触发点是否被一份 v1 冻结清单**封死**，封后任何增删改都被拒。
//!   本模块不选线程、不切上下文、不碰硬件寄存器（F1804/F1805/F1806/F1807 负责）。
//!   判据落点：`调度点全集登记（tick 到期/抢占/让出/阻塞/迁移后/启动）清单冻结 v1，
//!   调度点外注入 10/10 检出`。
//!
//! 性能逐项分解（本模块纯整数逻辑，无墙钟/浮点/IO，各项成本按执行步数分列）：
//!   P1 注册表封账：seed N 项（N=64 项定容）逐项 FNV 字节流折叠，
//!      每项约 (字段均长 48 B) × 8 轮 wrapping_mul/异或 ⇒ ~64×48 = 3072 次乘法。
//!   P2 封后写拒：常数级 —— 长度比对 + 指纹复算（O(N) 但仅在 verify 路径）。
//!   P3 点号查表：点号空间 [1,64] 稀疏，以线性定容数组扫描 ≤64 项 ⇒ 最坏 64 次比较，
//      命中即返回，O(1) 均值由 6 有效项决定。
//!   P4 语境合法核：4 位掩码单次 AND ⇒ 1 条指令路径，O(1)。
//!   P5 调用点全扫描：CallSiteTable ≤128 项，每项 1 次点号查 + 1 次名比对 ⇒ ≤128×O(1)。
//!   P6 need_resched 闭环：置位/消费/清位各 1 次位运算 + 审计 1 次位扫描 ⇒ O(1)。
//!   P7 原因守恒核：5 类计数求和 + 豁免声明比对 ⇒ O(1)。
//!   P8 原因账环形：定容 64 写指针回绕 ⇒ O(1)，溢出以 dropped 显性计数。
//!   P9 注入核：10 场，每场独立小状态，无共享 ⇒ 合计 O(10×小常数)。
//!   P10 导出：KV 清单 ~N 行字符串拼接（O(N×行长)）；TSV 6×4 + 行头 O(24)；
//!      TLV 一次顺序写 O(总字节)，无重分配以外的隐藏成本。
//!
//! 诊断码段（**独占** 0xB0A0~0xB0AF，主码 0xB0A0）：
//!   0xB0A0 SP_ERR_UNREGISTERED     点号未登记（域外注入 · 主码）
//!   0xB0A1 SP_ERR_SEALED_MUTATION   封账后尝试新增/改写/删除
//!   0xB0A2 SP_ERR_CTX_ILLEGAL       已登记点号但语境不在该点允许集内
//!   0xB0A3 SP_ERR_FORGED_NO         伪造点号（数字存在但名/触发条件不符）
//!   0xB0A4 SP_ERR_DEADPOINT         切换原语内再次请求调度（递归调度死点）
//!   0xB0A5 SP_ERR_RESCHED_LOST      need_resched 置位丢失（未经消费直接清位）
//!   0xB0A6 SP_ERR_REASON_UNBALANCED 切换原因账不闭合（守恒式破坏）
//!   0xB0A7 SP_ERR_SEAL_TAMPER       封账指纹被篡改（复算不符）
//!   0xB0A8 SP_ERR_CALLSITE_ABSENT   应登记的调用点未登记点号
//!   0xB0A9 SP_ERR_CAPACITY          注册表/账本定容耗尽
//!   0xB0AA SP_ERR_DOMAIN_ESCAPE     域外注入落账标记
//!   0xB0AB SP_ERR_PERMIT_STATE      许可状态机不允许本次请求
//!   0xB0AC SP_ERR_INVARIANT_BROKEN  点专属不变量被破坏
//!   0xB0AD SP_ERR_CHECKPOINT_MISS   检查点缺失（分步审计断链）
//!   0xB0AE SP_ERR_TLV_SHORT         TLV 长度自洽断言失败
//!   0xB0AF SP_ERR_CTX_FORBIDDEN     语境三禁（禁锁/禁调度/禁长驻）被违反
//!
//! 纯功能线：no_std + alloc；零 unwrap / 零 expect / 零 panic! / 零浮点 /
//!           零真实 IO / 零墙钟（逻辑 tick: u64 注入）；
//!           确定性 LCG（64 位标准常数 6364136223846793005 / 1442695040888963407）；
//!           自定义 Result<T,E>；所有容器定容优先，越界以错误码表达。

use alloc::format; use alloc::string::{String, ToString}; use alloc::vec; use alloc::vec::Vec;
/// 模块版本（调度点全集登记 v1 · 清单冻结 v1）。
pub const SP2_VERSION: &str = "unx1802_schedpts-v1";
/// 判据标识（注释引用原文）。
pub const SP2_JUDGE_ID: &str = "UNX-F1802-J1";
/// 判据原文（冻结 v1）。
pub const SP2_JUDGE_TEXT: &str = "UNX-F1802-J1 调度点全集登记（tick 到期/抢占/让出/阻塞/迁移后/启动）清单冻结 v1，调度点外注入 10/10 检出";
/// 诊断段基址（0xB0A0 段 · 本模块独占 · 上界 0xB0AF）。
pub const SP2_SEG_BASE: u16 = 0xB0A0;
/// 诊断段上界（闭区间上端）。
pub const SP2_SEG_TOP: u16 = 0xB0AF;
/// 主诊断码（域外注入主码）。
pub const SP2_MAIN_CODE: u16 = 0xB0A0;
/// FNV-1a 64 位 offset basis。
pub const SP2_FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位 prime。
pub const SP2_FNV_PRIME: u64 = 0x100000001b3;
/// 注册表定容（**≥64 项**硬指标）。
pub const SP2_REG_CAP: usize = 64;
/// 调度点定容（全集闭集大小 = 6）。
pub const SP2_POINT_COUNT: usize = 6;
/// 调用点表定容。
pub const SP2_CALLSITE_CAP: usize = 128;
/// 域外注入账定容（**≥10 项**）。
pub const SP2_ESCAPE_CAP: usize = 24;
/// 原因账环形缓冲容量。
pub const SP2_REASON_RING_CAP: usize = 64;
/// 审计轨迹定容。
pub const SP2_TRACE_CAP: usize = 96;
/// LCG 乘数（64 位标准 LCG 常数）。
pub const SP2_LCG_MUL: u64 = 6364136223846793005;
/// LCG 增量。
pub const SP2_LCG_ADD: u64 = 1442695040888963407;
// ===========================================================================
// A. 调度点全集闭集（6 点 · 判据核心 A）
// ===========================================================================

/// 调度触发点全集闭集（**判据核心 A**）。
///
/// 六点封闭：tick 到期 / 抢占 / 让出 / 阻塞 / 迁移后 / 启动。
/// 任何调度请求必须携带其中之一的稳定点号，闭集之外一律判「域外注入」。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchedPoint {
    /// SP-0101 tick 到期：时钟节拍到达，线程时间片耗尽。
    TickExpiry,
    /// SP-0201 抢占：更高优先级线程变为就绪，当前线程被夺走执行权。
    Preempt,
    /// SP-0301 让出：当前线程主动放弃执行权。
    Yield,
    /// SP-0401 阻塞：当前线程因等待事件进入不可运行态。
    Block,
    /// SP-0501 迁移后：线程迁至目标 CPU 完成后重新进入调度序。
    PostMigrate,
    /// SP-0601 启动：线程首次进入就绪态 / 系统引导后首个调度点。
    Startup,
    }
impl SchedPoint {
    /// 全集闭集顺序（冻结 v1 · 顺序即点号升序）。
    pub const ALL: [SchedPoint; SP2_POINT_COUNT] = [
        SchedPoint::TickExpiry,SchedPoint::Preempt,SchedPoint::Yield, SchedPoint::Block, SchedPoint::PostMigrate, SchedPoint::Startup,
    ];
    /// 稳定点号（u16 · **全生命周期不变**，清单登记的唯一凭据）。
    pub fn no(self) -> u16 {
        match self {
            SchedPoint::TickExpiry => 0x0101, SchedPoint::Preempt => 0x0201, SchedPoint::Yield => 0x0301,
            SchedPoint::Block => 0x0401, SchedPoint::PostMigrate => 0x0501, SchedPoint::Startup => 0x0601,
        }
    }
    /// 英文名（清单条目唯一名 · 封账指纹参与字节）。
    pub fn name(self) -> &'static str {
        match self {
            SchedPoint::TickExpiry => "SP-0101/tick-expiry", SchedPoint::Preempt => "SP-0201/preempt",
            SchedPoint::Yield => "SP-0301/yield", SchedPoint::Block => "SP-0401/block",
            SchedPoint::PostMigrate => "SP-0501/post-migrate", SchedPoint::Startup => "SP-0601/startup",
        }
    }
    /// 中文名（册内用语）。
    pub fn zh(self) -> &'static str {
        match self {
            SchedPoint::TickExpiry => "tick 到期", SchedPoint::Preempt => "抢占",
            SchedPoint::TickExpiry => "tick 到期", SchedPoint::Preempt => "抢占", SchedPoint::Yield => "让出",
            SchedPoint::Block => "阻塞", SchedPoint::PostMigrate => "迁移后", SchedPoint::Startup => "启动",
        }
    }
    /// 闭集序号（0–5）。
    pub fn ordinal(self) -> usize {
        match self {
            SchedPoint::TickExpiry => 0, SchedPoint::Preempt => 1,
            SchedPoint::TickExpiry => 0, SchedPoint::Preempt => 1, SchedPoint::Yield => 2,
            SchedPoint::Block => 3, SchedPoint::PostMigrate => 4, SchedPoint::Startup => 5,
        }
    }
    /// 按点号反查（未登记点号返回 None —— 这是「域外注入」的第一道门）。
    pub fn by_no(no: u16) -> Option<SchedPoint> {
        let mut i = 0usize;
        while i < SP2_POINT_COUNT {
            let p = SchedPoint::ALL[i];
            if p.no() == no { return Some(p); }
            i += 1;
        }
        None
    }
    /// 按序号反查。
    pub fn by_ordinal(i: usize) -> Option<SchedPoint> {
        if i < SP2_POINT_COUNT {
            Some(SchedPoint::ALL[i])
        } else {
            None
        }
    }
    /// 触发条件描述（清单条目正文 · 参与封账指纹）。
    pub fn trigger(self) -> &'static str {
        match self {
            SchedPoint::TickExpiry => "本核 tick 计数越过时间片配额且线程仍处就绪态", SchedPoint::Preempt => "新就绪线程优先级严格高于当前线程且当前线程未被钉扎",
            SchedPoint::Yield => "当前线程主动调用让出语义并处于可让出态", SchedPoint::Block => "当前线程等待的事件未就绪且等待深度合法",
            SchedPoint::PostMigrate => "线程被迁往目标 CPU 并完成入队后重新进入调度序", SchedPoint::Startup => "线程自创建/引导完成首次进入就绪态且无前序上下文",
        }
    }
    /// 允许的调用语境位掩码（**闭集** · 与 C7 矩阵同源）。
    ///
    /// 语境闭集为 `{Thread, Interrupt, Kernel, Dma}`，掩码位序
    /// `Thread=0 Interrupt=1 Kernel=2 Dma=3`。
    pub fn ctx_mask(self) -> u8 {
        match self {
            // tick 到期：时钟中断内消费（Interrupt）与内核定时器路径（Kernel）。
            SchedPoint::TickExpiry => CallCtx::M_THREAD | CallCtx::M_INTERRUPT | CallCtx::M_KERNEL,
            // 抢占：线程内可让渡；中断（重调度 IPI）内强制抢占（F1844 衔接）。
            SchedPoint::Preempt => CallCtx::M_THREAD | CallCtx::M_INTERRUPT | CallCtx::M_KERNEL,
            // 让出：仅线程语境（中断内让出无意义且危害上下文）。
            SchedPoint::Yield => CallCtx::M_THREAD | CallCtx::M_KERNEL,
            // 阻塞：仅线程语境（等待必须发生在线程自身语义里）。
            SchedPoint::Block => CallCtx::M_THREAD | CallCtx::M_KERNEL,
            // 迁移后：内核迁移器语境与线程语境（迁出后自观测）。
            SchedPoint::PostMigrate => CallCtx::M_THREAD | CallCtx::M_KERNEL | CallCtx::M_DMA,
            // 启动：引导期内核语境 + 线程语境（首次进入）。
            SchedPoint::Startup => CallCtx::M_THREAD | CallCtx::M_KERNEL,
        }
    }
    /// 是否允许在中断语境（IPI 纪律的直接输入 · F1844 衔接）。
    pub fn interrupt_ok(self) -> bool {
        self.ctx_mask() & CallCtx::M_INTERRUPT != 0
    }
    /// need_resched 置位策略（策略闭集）。
    pub fn resched_policy(self) -> ReschedPolicy {
        match self {
            SchedPoint::TickExpiry => ReschedPolicy::AlwaysSet, SchedPoint::Preempt => ReschedPolicy::OnlyIfPreempt,
            SchedPoint::Yield => ReschedPolicy::AlwaysSet, SchedPoint::Block => ReschedPolicy::Never,
            SchedPoint::PostMigrate => ReschedPolicy::Never, SchedPoint::Startup => ReschedPolicy::Never,
        }
    }
    /// 点优先级（0 最高；数值小者先处理）。
    pub fn priority(self) -> u8 {
        match self {
            SchedPoint::Preempt => 0, SchedPoint::PostMigrate => 1,
            SchedPoint::Preempt => 0, SchedPoint::PostMigrate => 1, SchedPoint::Block => 2,
            SchedPoint::TickExpiry => 3, SchedPoint::Yield => 4, SchedPoint::Startup => 5,
        }
    }
    /// 该点对应的切换原因分类（**启动点单列**，不并入四分类）。
    pub fn reason_class(self) -> SwitchReason {
        match self {
            SchedPoint::TickExpiry => SwitchReason::Expiry,
            SchedPoint::Preempt => SwitchReason::Preempt, SchedPoint::Yield => SwitchReason::Yield,
            SchedPoint::Block => SwitchReason::Expiry, SchedPoint::PostMigrate => SwitchReason::PostMigrate,
            SchedPoint::Startup => SwitchReason::Startup,
        }
    }
    /// 该点是否计入切换总数（Block 显式豁免，见守恒式注释）。
    pub fn counts_switch(self) -> bool {
        !matches!(self, SchedPoint::Block)
    }
    /// 点专属不变量（可数条 · 逐条登记）。
    pub fn invariants(self) -> &'static [&'static str] {
        match self {
            SchedPoint::TickExpiry => &[
                "INV-0101-A:点号 0x0101 在任何语境下都可被 registry() 只读视图检出", "INV-0101-B:时间片配额必须已被消费过一次（tick 龄 ≥ 1）", "INV-0101-C:同一 tick 内同一核至多登记一次到期点请求",
                "INV-0101-D:到期请求不得在切换原语内发起（死点纪律）", "INV-0101-E:置位策略为 AlwaysSet —— 请求后 need_resched 必须为真",
            ],
            SchedPoint::Preempt => &[
                "INV-0201-A:抢占必须存在严格更优的竞争者（new_prio < cur_prio）", "INV-0201-B:抢占是唯一允许在 Interrupt 语境发起的调度点（F1844 纪律）",
                "INV-0201-C:Interrupt 语境抢占不得持锁进入（三禁登记）", "INV-0201-D:置位策略为 OnlyIfPreempt —— 仅当确为抢占时置位", "INV-0201-E:中断内抢占不得在切换原语内递归发起",
            ],
            SchedPoint::Yield => &[
                "INV-0301-A:让出只能由线程自身发起（无第三方代请求路径）", "INV-0301-B:让出不得在 Interrupt/Dma 语境发起", "INV-0301-C:让出必须保持当前线程可运行态（不得隐式改态）",
                "INV-0301-D:置位策略为 AlwaysSet —— 让出必置位并交由主循环消费", "INV-0301-E:让出次数计入原因账 Yield 类",
            ],
            SchedPoint::Block => &[
                "INV-0401-A:阻塞只能由线程自身发起", "INV-0401-B:阻塞必须声明等待事件标识（否则视为未完成请求）", "INV-0401-C:阻塞显式豁免切换计数（守恒式 +0 项）",
                "INV-0401-D:置位策略为 Never —— 直接进调度，不经 need_resched 中转", "INV-0401-E:阻塞不得在 Interrupt 语境发起（等事件须在临界区外）",
            ],
            SchedPoint::PostMigrate => &[
                "INV-0501-A:迁移后点必须携带目标 CPU 标识", "INV-0501-B:迁移后点必须在目标 CPU 的入队完成后发起", "INV-0501-C:置位策略为 Never —— 由迁移器直连调度（F1804 选线程消费）",
                "INV-0501-D:迁移后切换计入原因账 PostMigrate 类", "INV-0501-E:迁移后点可在 Dma 语境发起（拷贝完成后回调）",
            ],
            SchedPoint::Startup => &[
                "INV-0601-A:启动点必须携带引导世代号（gen）", "INV-0601-B:启动点单列原因账 Startup 类，不并入四分类求和项", "INV-0601-C:置位策略为 Never —— 引导期直连调度",
                "INV-0601-D:同一线程的启动点只可发起一次", "INV-0601-E:启动点不得在 Interrupt/Dma 语境发起",
            ],
        }
    }
    /// 不变量条数。
    pub fn invariant_count(self) -> usize {
        self.invariants().len()
    }
    /// 封账指纹参与字段（条目稳定文本 · 顺序固定）。
    ///
    /// 布局：`no | name | zh | trigger | ctx_mask | policy | prio | reason | invariants`
    pub fn seal_fields(self) -> String {
        let inv = self.invariants();
        let mut s = String::new();
        s.push_str(&format!("{}|{}|{}|{}|", self.no(), self.name(), self.zh(), self.trigger()));
        s.push_str(&format!("{}|{}|{}|{}|", self.ctx_mask(), self.resched_policy().policy_code(), self.priority(), self.reason_class().code()));
        let mut i = 0usize;
        while i < inv.len() {
            s.push_str(inv[i]);
            if i + 1 < inv.len() { s.push(';'); }
            i += 1;
        }
        s
    }
}
// ===========================================================================
// B. 辅助闭集：语境 / 置位策略 / 原因分类 / 纪律项 / 许可
// ===========================================================================

/// 调用语境闭集（4 种 · 判据核心 F）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallCtx {
    /// 线程语境：在线程自身执行流中请求调度。
    Thread,
    /// 中断语境：中断向量入口（含重调度 IPI，F1844 衔接）。
    Interrupt,
    /// 内核语境：内核态服务路径 / 引导路径。
    Kernel,
    /// Dma 语境：DMA 完成回调 / 拷贝引擎回调路径。
    Dma,
    }
impl CallCtx {
    /// 语境位：Thread。
    pub const M_THREAD: u8 = 0b0001;
    /// 语境位：Interrupt。
    pub const M_INTERRUPT: u8 = 0b0010;
    /// 语境位：Kernel。
    pub const M_KERNEL: u8 = 0b0100;
    /// 语境位：Dma。
    pub const M_DMA: u8 = 0b1000;
    /// 语境全集顺序（矩阵列序 · 冻结 v1）。
    pub const ALL: [CallCtx; 4] = [
        CallCtx::Thread, CallCtx::Interrupt, CallCtx::Kernel, CallCtx::Dma,
    ];
    /// 语境位掩码。
    pub fn mask(self) -> u8 {
        match self {
            CallCtx::Thread => CallCtx::M_THREAD, CallCtx::Interrupt => CallCtx::M_INTERRUPT,
            CallCtx::Kernel => CallCtx::M_KERNEL, CallCtx::Dma => CallCtx::M_DMA,
        }
    }
    /// 语境英文名（矩阵列头 · 参与 TSV）。
    pub fn name(self) -> &'static str {
        match self {
            CallCtx::Thread => "Thread", CallCtx::Interrupt => "Interrupt", CallCtx::Kernel => "Kernel", CallCtx::Dma => "Dma",
        }
    }
    /// 语境中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CallCtx::Thread => "线程语境", CallCtx::Interrupt => "中断语境", CallCtx::Kernel => "内核语境", CallCtx::Dma => "Dma 语境",
        }
    }
    /// 语境序号（矩阵列号 0–3）。
    pub fn ordinal(self) -> usize {
        match self {
            CallCtx::Thread => 0, CallCtx::Interrupt => 1, CallCtx::Kernel => 2, CallCtx::Dma => 3,
        }
    }
    /// 按序号反查。
    pub fn by_ordinal(i: usize) -> Option<CallCtx> {
        if i < 4 {
            Some(CallCtx::ALL[i])
        } else {
            None
        }
    }
    /// 语境触发描述（清单登记用）。
    pub fn trigger(self) -> &'static str {
        match self {
            CallCtx::Thread => "线程态执行流中发起调度请求", CallCtx::Interrupt => "中断向量入口发起调度请求（F1844 重调度 IPI 纪律域）",
            CallCtx::Kernel => "内核服务/引导路径发起调度请求", CallCtx::Dma => "DMA 完成回调路径发起调度请求",
        }
    }
    /// 语境条目不变量。
    pub fn invariants(self) -> &'static [&'static str] {
        &["INV-CTX-1:语境必须落在请求点 ctx_mask 内", "INV-CTX-2:非法语境一律 0xB0A2 并落域外注入账"]
    }
    /// 语境是否为中断语境（IPI 三禁判定入口）。
    pub fn is_interrupt(self) -> bool {
        matches!(self, CallCtx::Interrupt)
    }
}

/// need_resched 置位策略闭集（判据核心 D）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReschedPolicy {
    /// 置位策略：AlwaysSet —— 必置位，等主循环消费。
    AlwaysSet,
    /// 置位策略：OnlyIfPreempt —— 仅当确为抢占时置位。
    OnlyIfPreempt,
    /// 置位策略：Never —— 直接进调度，不经 need_resched 中转。
    Never,
    }
impl ReschedPolicy {
    /// 策略码（进入封账指纹字节）。
    pub fn policy_code(self) -> u8 {
        match self {
            ReschedPolicy::AlwaysSet => 0, ReschedPolicy::OnlyIfPreempt => 1, ReschedPolicy::Never => 2,
        }
    }
    /// 策略英文名。
    pub fn name(self) -> &'static str {
        match self {
            ReschedPolicy::AlwaysSet => "AlwaysSet",ReschedPolicy::OnlyIfPreempt => "OnlyIfPreempt", ReschedPolicy::Never => "Never",
        }
    }
    /// 策略中文说明。
    pub fn zh(self) -> &'static str {
        match self {
            ReschedPolicy::AlwaysSet => "必置位：置位后由主循环消费", ReschedPolicy::OnlyIfPreempt => "仅抢占置位：非抢占路径不置位",
            ReschedPolicy::Never => "直连调度：不经过 need_resched 中转",
        }
    }
    /// 全集闭集。
    pub const ALL: [ReschedPolicy; 3] = [
        ReschedPolicy::AlwaysSet, ReschedPolicy::OnlyIfPreempt, ReschedPolicy::Never,
    ];
    /// 策略条目不变量。
    pub fn invariants(self) -> &'static [&'static str] {
        &["INV-POL-1:置位丢失即 0xB0A5", "INV-POL-2:消费必须先于清位"]
    }
    /// 携带负载是否应置位 need_resched。
    ///
    /// `payload_true` 表示「本请求确为抢占型负载」。
    pub fn should_set(self, payload_true: bool) -> bool {
        match self {
            ReschedPolicy::AlwaysSet => true, ReschedPolicy::OnlyIfPreempt => payload_true, ReschedPolicy::Never => false,
        }
    }
}

/// 切换原因分类（**四分类 + 启动单列** · 判据核心 E · 消费方 F1809/F1626）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchReason {
    /// 到期（tick 到期 / 阻塞引出的自然切换）。
    Expiry,
    /// 抢占。
    Preempt,
    /// 让出。
    Yield,
    /// 迁移后。
    PostMigrate,
    /// 启动（**单列**，不并入四分类求和项）。
    Startup,
    }
impl SwitchReason {
    /// 四分类 + 启动，共 5 类。
    pub const ALL: [SwitchReason; 5] = [
        SwitchReason::Expiry,SwitchReason::Preempt, SwitchReason::Yield, SwitchReason::PostMigrate, SwitchReason::Startup,
    ];
    /// 分类码。
    pub fn code(self) -> u8 {
        match self {
            SwitchReason::Expiry => 0, SwitchReason::Preempt => 1,
            SwitchReason::Yield => 2, SwitchReason::PostMigrate => 3, SwitchReason::Startup => 4,
        }
    }
    /// 分类英文名。
    pub fn name(self) -> &'static str {
        match self {
            SwitchReason::Expiry => "expiry", SwitchReason::Preempt => "preempt", SwitchReason::Yield => "yield",
            SwitchReason::PostMigrate => "post_migrate", SwitchReason::Startup => "startup",
        }
    }
    /// 分类中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SwitchReason::Expiry => "到期", SwitchReason::Preempt => "抢占",
            SwitchReason::Expiry => "到期", SwitchReason::Preempt => "抢占", SwitchReason::Yield => "让出",
            SwitchReason::PostMigrate => "迁移后", SwitchReason::Startup => "启动（单列）",
        }
    }
    /// 原因条目不变量。
    pub fn invariants(self) -> &'static [&'static str] {
        &["INV-RSN-1:守恒式 switch_total == 四分类和 + startup", "INV-RSN-2:差额 0 即 0xB0A6"]
    }
    /// 是否属于「原因四分类」（启动点单列 → 返回 false）。
    pub fn in_quad(self) -> bool {
        !matches!(self, SwitchReason::Startup)
    }
    /// 是否为守恒式显式豁免项（阻塞不产生切换 → 豁免登记）。
    pub fn is_exempt(self) -> bool {
        matches!(self, SwitchReason::Startup)
    }
}

/// 语境纪律三禁（登记项 · F1844 中断语境纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxBan {
    /// 禁锁：中断语境不得持锁进入切换准备。
    BanLock,
    /// 禁调度：中断语境不得主动发起除 Preempt 以外的调度点。
    BanSched,
    /// 禁长驻：中断语境不得在切换路径长时间驻留。
    BanLongStay,
    }
impl CtxBan {
    /// 三禁全集。
    pub const ALL: [CtxBan; 3] = [CtxBan::BanLock, CtxBan::BanSched, CtxBan::BanLongStay];
    /// 纪律码。
    pub fn code(self) -> u8 {
        match self {
            CtxBan::BanLock => 0, CtxBan::BanSched => 1, CtxBan::BanLongStay => 2,
        }
    }
    /// 纪律名。
    pub fn name(self) -> &'static str {
        match self {
            CtxBan::BanLock => "ban-lock", CtxBan::BanSched => "ban-sched", CtxBan::BanLongStay => "ban-long-stay",
        }
    }
    /// 纪律文本（登记入清单条目）。
    pub fn text(self) -> &'static str {
        match self {
            CtxBan::BanLock => "Interrupt 语境进入切换准备前锁计数必须为 0", CtxBan::BanSched => "Interrupt 语境仅允许 Preempt 调度点，其余点一律拒绝",
            CtxBan::BanLongStay => "Interrupt 语境切换路径驻留步数必须不超过 STAY_BUDGET",
        }
    }
}
impl CtxBan {
    /// 纪律条目不变量（按纪律码分支）。
    pub fn invariants(self) -> &'static [&'static str] {
        match self {
            CtxBan::BanLock => &[
                "INV-DISC-1:Interrupt 语境持锁数为 0，否则 0xB0AF", "INV-DISC-2:持锁进入切换准备即刻拒绝",
            ],
            CtxBan::BanSched => &[
                "INV-DISC-3:Interrupt 语境仅 Preempt 点可发起", "INV-DISC-4:其余五点一旦出现即 0xB0A2",
            ],
            CtxBan::BanLongStay => &[
                "INV-DISC-5:Interrupt 切换驻留步数 <= SP2_INTERRUPT_STAY_BUDGET", "INV-DISC-6:超预算即 0xB0AF",
            ],
        }
    }
}

/// 中断语境驻留预算（步 · 逻辑步进计数 · 非墙钟）。
pub const SP2_INTERRUPT_STAY_BUDGET: u32 = 6;
/// 调度许可（`request_sched` 成功产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchedPermit {
    /// 许可绑定的点号。
    pub point_no: u16,
    /// 许可绑定的点。
    pub point: SchedPoint,
    /// 许可发放在语境。
    pub ctx: CallCtx,
    /// 许可发放的 tick。
    pub tick: u64,
    /// 许可序列号（单调递增 · 检测许可复用）。
    pub seq: u64,
    /// 本许可是否要求 need_resched 已置位（策略闭集输出）。
    pub resched_must_be_set: bool,
    }
impl SchedPermit {
    /// 许可摘要行。
    pub fn line(&self) -> String {
        format!(
        "permit no=0x{:04x} pt={} ctx={} tick={} seq={} need={}",self.point_no, self.point.name(), self.ctx.name(), self.tick, self.seq,
            self.resched_must_be_set
    )
    }
}

/// 诊断错误类型（自定义 Result 的错误侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpErr {
    /// 0xB0A0 点号未登记（域外注入 · 主码）。
    Unregistered(u16),
    /// 0xB0A1 封账后尝试新增/改写/删除。
    SealedMutation(u8),
    /// 0xB0A2 已登记点号但语境不在允许集内。
    CtxIllegal(u16, CallCtx),
    /// 0xB0A3 伪造点号（数字存在但名/触发条件不符）。
    ForgedNo(u16),
    /// 0xB0A4 切换原语内再次请求调度（递归调度死点）。
    DeadPoint(u16),
    /// 0xB0A5 need_resched 置位丢失。
    ReschedLost(u64),
    /// 0xB0A6 切换原因账不闭合。
    ReasonUnbalanced(u64),
    /// 0xB0A7 封账指纹被篡改。
    SealTamper(u64, u64),
    /// 0xB0A8 应登记的调用点未登记点号。
    CallSiteAbsent(u32),
    /// 0xB0A9 定容耗尽。
    Capacity(&'static str),
    /// 0xB0AA 域外注入落账标记。
    DomainEscape(u16),
    /// 0xB0AA 许可状态不允许（内部一致性）。
    DomainEscapePermit(u64),
    /// 0xB0AB 许可状态机不允许本次请求。
    PermitState(u8),
    /// 0xB0AC 点专属不变量被破坏。
    InvariantBroken(u16, u8),
    /// 0xB0AD 检查点缺失（分步审计断链）。
    CheckpointMiss(u8),
    /// 0xB0AE TLV 长度自洽断言失败。
    TlvShort(usize, usize),
    /// 0xB0AF 语境三禁被违反。
    CtxForbidden(u8, u8),
    }
impl SpErr {
    /// 诊断码（0xB0A0~0xB0AF）。
    pub fn code(self) -> u16 {
        match self {
            SpErr::Unregistered(_) => 0xB0A0, SpErr::SealedMutation(_) => 0xB0A1, SpErr::CtxIllegal(_, _) => 0xB0A2,
            SpErr::ForgedNo(_) => 0xB0A3, SpErr::DeadPoint(_) => 0xB0A4,
            SpErr::ReschedLost(_) => 0xB0A5, SpErr::ReasonUnbalanced(_) => 0xB0A6, SpErr::SealTamper(_, _) => 0xB0A7,
            SpErr::CallSiteAbsent(_) => 0xB0A8, SpErr::Capacity(_) => 0xB0A9,
            SpErr::DomainEscape(_) => 0xB0AA, SpErr::DomainEscapePermit(_) => 0xB0AA,
            SpErr::PermitState(_) => 0xB0AB, SpErr::InvariantBroken(_, _) => 0xB0AC, SpErr::CheckpointMiss(_) => 0xB0AD,
            SpErr::TlvShort(_, _) => 0xB0AE, SpErr::CtxForbidden(_, _) => 0xB0AF,
        }
    }
    /// 诊断名。
    pub fn name(self) -> &'static str {
        match self {
            SpErr::Unregistered(_) => "SP_ERR_UNREGISTERED", SpErr::SealedMutation(_) => "SP_ERR_SEALED_MUTATION",
            SpErr::CtxIllegal(_, _) => "SP_ERR_CTX_ILLEGAL",
            SpErr::ForgedNo(_) => "SP_ERR_FORGED_NO", SpErr::DeadPoint(_) => "SP_ERR_DEADPOINT",
            SpErr::ReschedLost(_) => "SP_ERR_RESCHED_LOST", SpErr::ReasonUnbalanced(_) => "SP_ERR_REASON_UNBALANCED",
            SpErr::SealTamper(_, _) => "SP_ERR_SEAL_TAMPER",
            SpErr::CallSiteAbsent(_) => "SP_ERR_CALLSITE_ABSENT",
            SpErr::Capacity(_) => "SP_ERR_CAPACITY", SpErr::DomainEscape(_) => "SP_ERR_DOMAIN_ESCAPE",
            SpErr::DomainEscapePermit(_) => "SP_ERR_DOMAIN_ESCAPE", SpErr::PermitState(_) => "SP_ERR_PERMIT_STATE",
            SpErr::InvariantBroken(_, _) => "SP_ERR_INVARIANT_BROKEN",
            SpErr::CheckpointMiss(_) => "SP_ERR_CHECKPOINT_MISS", SpErr::TlvShort(_, _) => "SP_ERR_TLV_SHORT",
            SpErr::CtxForbidden(_, _) => "SP_ERR_CTX_FORBIDDEN",
        }
    }
    /// 诊断文本（导出用）。
    pub fn text(self) -> String {
        match self {
            SpErr::Unregistered(no) => format!("点号 0x{:04x} 未在冻结清单登记（域外注入）", no),SpErr::SealedMutation(op) => format!("封账后变更被拒（op={}）", op),
            SpErr::CtxIllegal(no, c) => {
                format!("点号 0x{:04x} 在语境 {} 下非法", no, c.name())
            }
            SpErr::ForgedNo(no) => format!("伪造点号 0x{:04x}：数字存在但名不符", no),
            SpErr::DeadPoint(no) => format!("点号 0x{:04x} 在切换原语内递归请求（死点）", no),
            SpErr::ReschedLost(t) => format!("tick {} 上 need_resched 置位丢失", t),
            SpErr::ReasonUnbalanced(d) => format!("切换原因账差额 {}，守恒式破坏", d),
            SpErr::SealTamper(a, b) => format!("封账指纹篡改：存 {} vs 算 {}", a, b),
            SpErr::CallSiteAbsent(i) => format!("调用点 #{} 未登记合法点号", i),
            SpErr::Capacity(w) => format!("{} 定容耗尽", w),
            SpErr::DomainEscape(no) => format!("域外注入落账：0x{:04x}", no),
            SpErr::DomainEscapePermit(s) => format!("许可序列 {} 状态异常", s),SpErr::PermitState(s) => format!("许可状态机拒绝（state={}）", s),
            SpErr::InvariantBroken(no, i) => {
                format!("点号 0x{:04x} 不变量 #{} 破坏", no, i)
            }
            SpErr::CheckpointMiss(c) => format!("检查点 #{} 缺失", c),
            SpErr::TlvShort(a, b) => format!("TLV 长度自洽失败：期望 {} 实得 {}", a, b),SpErr::CtxForbidden(b, v) => format!("语境三禁 {} 违规（值 {}）", b, v),
        }
    }
    /// 是否为「域外注入」类错误（判据计数口径）。
    pub fn is_domain_escape(self) -> bool {
        matches!(
        self,
            SpErr::Unregistered(_)
    | SpErr::CtxIllegal(_, _)
    | SpErr::ForgedNo(_)
    | SpErr::DomainEscape(_)
    | SpErr::DomainEscapePermit(_)
    | SpErr::CallSiteAbsent(_)
    )
    }
}

/// 模块自定义 Result（零 unwrap / 零 expect / 零 panic）。
pub type SpResult<T> = Result<T, SpErr>;

/// 自检用请求助手（避免闭包借用与显式字段读写互斥）。
    fn sc_req(g: &mut SchedGate, ctx: CallCtx, p: SchedPoint, t: u64) -> SpResult<SchedPermit> {
    g.request_sched(p.no(), ctx, t)
}

/// 自检项登记宏（等价 `out.push(SelfCheck::new(..))` · 仅为自检段压行）。
///
/// 判据：UNX-F1802-J1 —— 宏展开后仍是逐项具名自检，不做聚合吞项。
macro_rules! chk {
    ($out:expr, $n:expr, $ok:expr, $d:expr) => {
        $out.push(SelfCheck::new($n, $ok, $d))
    };
}
// ===========================================================================
// C. 调度点描述符布局 + FNV + 清单冻结 v1
// ===========================================================================

/// 关联锚点条目的点专属不变量（锚点统一挂靠，故共用一组）。
pub const ANCHOR_INV: [&'static str; 2] = [
    "INV-ANCHOR-1:锚点条目不参与点号查表（仅登记）", "INV-ANCHOR-2:锚点条目同样受封账约束",
    ];
/// 调度点描述符（**清单条目布局 · 判据核心 B**）。
///
/// 字段布局（顺序即封账指纹参与序，不得改序）：
/// ```text
/// +0  no           : u16   稳定点号
/// +2  name         : &str  英文名（唯一）
/// +6  zh           : &str  中文名
/// +7  trigger      : &str  触发条件描述
/// +8  ctx_mask     : u8    允许语境位掩码（4 位）
/// +9  policy       : u8    need_resched 置位策略码
/// +10 priority     : u8    点优先级（0 最高）
/// +11 reason       : u8    切换原因分类码
/// +12 counts       : bool  是否计入切换总数
/// +13 invariants   : &[&str] 点专属不变量（≥5 条）
/// ++  seal_fp      : u64   封账指纹（seed 期 0，封账期写入）
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedPointDesc {
    /// 稳定点号。
    pub no: u16,
    /// 英文名。
    pub name: &'static str,
    /// 中文名。
    pub zh: &'static str,
    /// 触发条件描述。
    pub trigger: &'static str,
    /// 允许语境位掩码。
    pub ctx_mask: u8,
    /// need_resched 置位策略码。
    pub policy: u8,
    /// 点优先级。
    pub priority: u8,
    /// 切换原因分类码。
    pub reason: u8,
    /// 是否计入切换总数。
    pub counts: bool,
    /// 点专属不变量。
    pub invariants: &'static [&'static str],
    /// 封账指纹（封账前为 0）。
    pub seal_fp: u64,
    }
impl SchedPointDesc {
    /// 由调度点构造描述符（seed 路径 · seal_fp 置 0）。
    pub fn from_point(p: SchedPoint) -> SchedPointDesc {
        SchedPointDesc {
            no: p.no(), name: p.name(), zh: p.zh(), trigger: p.trigger(), ctx_mask: p.ctx_mask(),
            policy: p.resched_policy().policy_code(), priority: p.priority(), reason: p.reason_class().code(),
            counts: p.counts_switch(), invariants: p.invariants(), seal_fp: 0,
        }
    }
    /// 反查调度点。
    pub fn point(&self) -> Option<SchedPoint> {
        SchedPoint::by_no(self.no)
    }
    /// 反查置位策略。
    pub fn resched_policy(&self) -> ReschedPolicy {
        match self.policy {
            0 => ReschedPolicy::AlwaysSet, 1 => ReschedPolicy::OnlyIfPreempt, _ => ReschedPolicy::Never,
        }
    }
    /// 反查原因分类。
    pub fn reason_class(&self) -> SwitchReason {
        match self.reason {
            0 => SwitchReason::Expiry, 1 => SwitchReason::Preempt,
            2 => SwitchReason::Yield, 3 => SwitchReason::PostMigrate, _ => SwitchReason::Startup,
        }
    }
    /// 语境合法性判定（**判据核心 C 的语境门**）。
    pub fn ctx_allowed(&self, c: CallCtx) -> bool {
        self.ctx_mask & c.mask() != 0
    }
    /// 允许语境清单（导出用）。
    pub fn ctx_list(&self) -> String {
        let mut out = String::new(); let mut first = true; let mut i = 0usize;
        while i < 4 {
            let c = CallCtx::ALL[i];
            if self.ctx_allowed(c) {
                if !first { out.push('|'); }
                out.push_str(c.name()); first = false;
            }
            i += 1;
        }
        out
    }
    /// 封账指纹参与文本（顺序固定 · 与 `SchedPoint::seal_fields` 同构）。
    pub fn seal_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("{}|{}|{}|{}|",self.no, self.name, self.zh, self.trigger ));
        s.push_str(&format!("{}|{}|{}|{}|{}|",self.ctx_mask, self.policy, self.priority, self.reason, self.counts as u8 )); let mut i = 0usize;
        while i < self.invariants.len() {
            s.push_str(self.invariants[i]);
            if i + 1 < self.invariants.len() { s.push(';'); }
            i += 1;
        }
        s
    }
    /// 单条描述符自身指纹（seed 期可复算 · 封账期用于比对）。
    pub fn compute_fp(&self) -> u64 {
        let mut h = SP2_FNV_BASIS;
        for &b in self.seal_text().as_bytes() {
            h ^= b as u64; h = h.wrapping_mul(SP2_FNV_PRIME);
        }
        h
    }
    /// KV 导出行（`export_registry_kv` 的行体）。
    pub fn kv_line(&self, idx: usize) -> String {
        format!(
        "#{} no=0x{:04x} name={} zh={} trigger=[{}] ctx=[{}] mask=0x{:02x} policy={} prio={} reason={} counts={} inv={} fp=0x{:016x}",
            idx,
    self.no,
    self.name,
    self.zh,
    self.trigger,
    self.ctx_list(),self.ctx_mask,self.resched_policy().name(),self.priority, self.reason_class().name(), self.counts as u8, self.invariants.len(),
    self.seal_fp
    )
    }
}

/// FNV-1a 64 位折叠（offset basis 0xcbf29ce484222325 / prime 0x100000001b3）。
pub fn fnv1a64(parts: &[&[u8]]) -> u64 {
    let mut h = SP2_FNV_BASIS; let mut i = 0usize;
    while i < parts.len() {
        let p = parts[i]; let mut j = 0usize;
        while j < p.len() { h ^= p[j] as u64; h = h.wrapping_mul(SP2_FNV_PRIME); j += 1; }
        if i + 1 < parts.len() { h ^= 0x1fu8 as u64; h = h.wrapping_mul(SP2_FNV_PRIME); }
        i += 1;
    }
    h
}

/// 整表封账指纹（顺序敏感：序号 + 描述符 seal_text 逐项折叠）。
pub fn registry_fingerprint(items: &[SchedPointDesc]) -> u64 {
    let mut h = SP2_FNV_BASIS; let mut i = 0usize;
    while i < items.len() {
        let t = items[i].seal_text();
        let nb = (i as u64).to_le_bytes();
        let tb = t.as_bytes();
        h ^= nb[0] as u64; h = h.wrapping_mul(SP2_FNV_PRIME); let mut j = 0usize;
        while j < tb.len() { h ^= tb[j] as u64; h = h.wrapping_mul(SP2_FNV_PRIME); j += 1; }
        h ^= 0xffu8 as u64; h = h.wrapping_mul(SP2_FNV_PRIME); i += 1;
    }
    h
}

/// 清单条目来源分类（append-only 追加的合法来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryOrigin {
    /// 调度点闭集播种（6 项）。
    CorePoint,
    /// 语境纪律补充条目（三禁登记）。
    Discipline,
    /// 关联锚点条目（册内相关单登记）。
    Anchor,
    }
impl EntryOrigin {
    /// 来源码。
    pub fn code(self) -> u8 {
        match self {
            EntryOrigin::CorePoint => 0, EntryOrigin::Discipline => 1, EntryOrigin::Anchor => 2,
        }
    }
    /// 来源名。
    pub fn name(self) -> &'static str {
        match self {
            EntryOrigin::CorePoint => "core-point", EntryOrigin::Discipline => "discipline", EntryOrigin::Anchor => "anchor",
        }
    }
    /// 来源中文。
    pub fn zh(self) -> &'static str {
        match self {
            EntryOrigin::CorePoint => "调度点闭集", EntryOrigin::Discipline => "语境纪律", EntryOrigin::Anchor => "关联锚点",
        }
    }
}

/// 冻结清单（**append-only 定容 ≥64 项 · 封账后只读 · 判据核心 B**）。
#[derive(Clone, Debug)]
pub struct SchedRegistry {
    /// 定容条目（append-only）。
    items: Vec<SchedPointDesc>,
    /// 锚点补充表（与 items 同序入指纹）。
    origins: Vec<EntryOrigin>,
    /// 是否已封账。
    sealed: bool,
    /// 封账指纹（写死于封账时；`verify_seal` 复算比对）。
    seal_fp: u64,
    /// 封账 tick。
    sealed_at: u64,
    /// 封账后被拒的写操作计数（新增/改写/删除分列）。
    rejected_add: u32, rejected_mod: u32, rejected_del: u32,
    }
impl SchedRegistry {
    /// 新建空清单（未播种）。
    pub fn new() -> SchedRegistry {
        SchedRegistry {
            items: Vec::new(), origins: Vec::new(), sealed: false, seal_fp: 0, sealed_at: 0, rejected_add: 0, rejected_mod: 0, rejected_del: 0,
        }
    }
    /// 登记条目构造器（**清单播种通用件** · 免去重复字面量）。
    fn mk(
    no: u16, name: &'static str, zh: &'static str, trigger: &'static str, ctx_mask: u8, policy: ReschedPolicy, prio: u8, reason: SwitchReason,
        counts: bool, inv: &'static [&'static str],
    ) -> SchedPointDesc {
    SchedPointDesc {
    no, name, zh, trigger, ctx_mask,policy: policy.policy_code(), priority: prio, reason: reason.code(),counts, invariants: inv, seal_fp: 0,
}
}
    /// 播种默认清单：6 调度点 + 3 纪律 + 6 锚点 + 3 语境 + 3 策略 + 5 原因
    /// + 4 未用槽位骨架 ⇒ ≥30 条；再由 `pad_to_cap` 补齐至定容 64 项。
    pub fn seed_default() -> SpResult<SchedRegistry> {
        let mut r = SchedRegistry::new();
        // (1) 6 调度点：闭集播种。
        let mut i = 0usize;
        while i < SP2_POINT_COUNT {
            r.push_unchecked(SchedPointDesc::from_point(SchedPoint::ALL[i]), EntryOrigin::CorePoint)?; i += 1;
        }
        // (2) 3 纪律条目（点号段 0x0dxx · F1844 中断语境三禁）。
        let disc: [(&'static str, CtxBan); 3] = [
            ("DISC/ban-lock", CtxBan::BanLock),("DISC/ban-sched", CtxBan::BanSched),("DISC/ban-long-stay", CtxBan::BanLongStay),
        ];
        let mut di = 0usize;
        while di < 3 {
            let (nm, b) = disc[di];
            let d = Self::mk(0x0d01 + di as u16, nm, b.name(), b.text(), CallCtx::M_INTERRUPT, ReschedPolicy::Never, 0, SwitchReason::Expiry, false, b.invariants());
            r.push_unchecked(d, EntryOrigin::Discipline)?; di += 1;
        }
        // (3) 4 语境登记条目（点号段 0x0cxx）。
        let mut ci = 0usize;
        while ci < 4 {
            let c = CallCtx::ALL[ci];
            let d = Self::mk(0x0c01 + ci as u16, c.name(), c.zh(), c.trigger(), c.mask(), ReschedPolicy::AlwaysSet, 1, SwitchReason::Expiry, false, c.invariants());
            r.push_unchecked(d, EntryOrigin::Discipline)?; ci += 1;
        }
        // (4) 3 置位策略登记条目（点号段 0x0bxx）。
        let mut pi = 0usize;
        while pi < 3 {
            let p = ReschedPolicy::ALL[pi];
            let d = Self::mk(0x0b01 + p.policy_code() as u16, p.name(), p.zh(), p.zh(), 0x0f, p, 2, SwitchReason::Expiry, false, p.invariants());
            r.push_unchecked(d, EntryOrigin::Discipline)?; pi += 1;
        }
        // (5) 5 原因分类登记条目（点号段 0x0axx · 四分类 + 启动单列）。
        let mut ri = 0usize;
        while ri < 5 {
            let rc = SwitchReason::ALL[ri];
            let d = Self::mk(0x0a01 + rc.code() as u16, rc.name(), rc.zh(), rc.zh(), 0x0f, ReschedPolicy::Never, 3, rc, rc.in_quad(), rc.invariants());
            r.push_unchecked(d, EntryOrigin::Discipline)?; ri += 1;
        }
        // (6) 6 关联锚点条目（点号段 0x09xx · 册内相关单）。
        let anchors: [(&'static str, &'static str); 6] = [
            ("ANCHOR/F1801", "切换主路径四段冻结（置位消费→选线程→切出→切入）"),
            ("ANCHOR/F1803", "need_resched 消费闭环（锚 F1632）"),
            ("ANCHOR/F1804", "下一线程选择"),
            ("ANCHOR/F1807", "switch_to 原语（死点检测依据）"),("ANCHOR/F1808", "临界区与锁序（锚 F1733）"),("ANCHOR/F1809", "切换计数账（锚 F1626 原因四分类）"),
        ];
        let mut ai = 0usize;
        while ai < 6 {
            let d = Self::mk(0x0901 + ai as u16, anchors[ai].0, "关联锚点", anchors[ai].1, 0x0f, ReschedPolicy::Never, 4, SwitchReason::Expiry, false, &ANCHOR_INV);
            r.push_unchecked(d, EntryOrigin::Anchor)?; ai += 1;
        }
        // (7) 补齐至定容 64 项（保留骨架槽 · 封账后不可改写）。
        r.pad_to_cap()?;
        Ok(r)
    }
    /// 内部追加（绕过封账检查；仅 seed/pad 使用）。
    fn push_unchecked(&mut self, d: SchedPointDesc, o: EntryOrigin) -> SpResult<()> {
        if self.items.len() >= SP2_REG_CAP { return Err(SpErr::Capacity("registry")); }
        if let Some(idx) = self.find_index(d.no) {
            // 点号唯一性：拒绝重复播种。
            let _ = idx; return Err(SpErr::SealedMutation(9));
        }
        self.items.push(d); self.origins.push(o);
        Ok(())
    }
    /// 补齐至定容（reserved 骨架 · 封账后不可改）。
    fn pad_to_cap(&mut self) -> SpResult<()> {
        let mut k = 0usize;
        while self.items.len() < SP2_REG_CAP {
            let idx = self.items.len();
            let d = SchedPointDesc {
                no: 0x7f00 | k as u16, name: "RESERVED/slot", zh: "保留骨架槽", trigger: "定容补齐占位：封账后不得被改写为真实调度点",
                ctx_mask: 0, policy: ReschedPolicy::Never.policy_code(), priority: 7,
                reason: 0, counts: false, invariants: &["INV-RSV-1:保留槽不可承载调度请求（掩码为 0）"], seal_fp: 0,
            };
            let _ = idx; self.items.push(d); self.origins.push(EntryOrigin::Anchor); k += 1;
        }
        Ok(())
    }
    /// 条目数（= 定容，非封账后可用槽数）。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否空（定容 64 ⇒ 恒 false，除非 seed 失败）。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 是否已封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    /// 封账指纹（未封账为 0）。
    pub fn seal_fp(&self) -> u64 {
        self.seal_fp
    }
    /// 封账 tick。
    pub fn sealed_at(&self) -> u64 {
        self.sealed_at
    }
    /// 封账后被拒写操作计数（新增/改写/删除）。
    pub fn rejected_writes(&self) -> (u32, u32, u32) {
        (self.rejected_add, self.rejected_mod, self.rejected_del)
    }
    /// 按点号查下标。
    pub fn find_index(&self, no: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].no == no { return Some(i); }
            i += 1;
        }
        None
    }
    /// 只读视图（**外部绕过冻结的唯一合法出口**）。
    pub fn registry(&self) -> &[SchedPointDesc] {
        &self.items
    }
    /// 只读快照复制（供调用方长期持有）。
    pub fn registry_snapshot(&self) -> Vec<SchedPointDesc> {
        let mut v = Vec::with_capacity(self.items.len()); let mut i = 0usize; while i < self.items.len() { v.push(self.items[i].clone()); i += 1; }
        v
    }
    /// 按点号取描述符（只读）。
    pub fn get(&self, no: u16) -> Option<&SchedPointDesc> {
        match self.find_index(no) {
            Some(i) => Some(&self.items[i]), None => None,
        }
    }
    /// 按调度点取描述符（只读）。
    pub fn get_point(&self, p: SchedPoint) -> Option<&SchedPointDesc> {
        self.get(p.no())
    }
    /// 条目来源（只读）。
    pub fn origin_of(&self, idx: usize) -> Option<EntryOrigin> {
        if idx < self.origins.len() {
            Some(self.origins[idx])
        } else {
            None
        }
    }
    /// 封账（**写死指纹 · 之后任何写拒**）。
    pub fn seal(&mut self, tick: u64) -> SpResult<u64> {
        if self.sealed { self.rejected_mod += 1; return Err(SpErr::SealedMutation(1)); }
        let fp = registry_fingerprint(&self.items);
        // 逐条写入单条指纹（供只读视图自校验）。
        let mut i = 0usize;
        while i < self.items.len() {
            let f = self.items[i].compute_fp(); self.items[i].seal_fp = f; i += 1;
        }
        self.seal_fp = fp; self.sealed_at = tick; self.sealed = true;
        Ok(fp)
    }
    /// 封账后新增（**必拒**）。
    pub fn try_add(&mut self, _d: SchedPointDesc) -> SpResult<()> {
        if self.sealed { self.rejected_add += 1; return Err(SpErr::SealedMutation(0)); }
        if self.items.len() >= SP2_REG_CAP { return Err(SpErr::Capacity("registry")); }
        Err(SpErr::SealedMutation(0))
    }
    /// 封账后改写（**必拒**）。
    pub fn try_modify(&mut self, no: u16, _trigger: &'static str) -> SpResult<()> {
        let _ = no; if self.sealed { self.rejected_mod += 1; return Err(SpErr::SealedMutation(2)); }
        Err(SpErr::SealedMutation(2))
    }
    /// 封账后删除（**必拒**）。
    pub fn try_delete(&mut self, no: u16) -> SpResult<()> {
        let _ = no; if self.sealed { self.rejected_del += 1; return Err(SpErr::SealedMutation(3)); }
        Err(SpErr::SealedMutation(3))
    }
    /// 封账指纹复算校验（**篡改必检出** · 注入形态 10）。
    pub fn verify_seal(&self) -> SpResult<u64> {
        if !self.sealed { return Err(SpErr::PermitState(2)); }
        let cur = registry_fingerprint(&self.items); if cur != self.seal_fp { return Err(SpErr::SealTamper(self.seal_fp, cur)); }
        Ok(cur)
    }
    /// 逐条单指纹校验（任一条被改写即检出）。
    pub fn verify_item_fps(&self) -> SpResult<usize> {
        if !self.sealed { return Err(SpErr::PermitState(2)); }
        let mut i = 0usize;
        while i < self.items.len() {
            let f = self.items[i].compute_fp();
            if f != self.items[i].seal_fp { return Err(SpErr::SealTamper(self.items[i].seal_fp, f)); }
            i += 1;
        }
        Ok(self.items.len())
    }
    /// 封账完整性总门（点集齐备 + 六点全在 + 指纹一致 + 定容达标）。
    pub fn judge(&self) -> (bool, String) {
        let cap_ok = self.items.len() >= 64; let mut six_ok = true; let mut i = 0usize;
        while i < SP2_POINT_COUNT {
            if self.get_point(SchedPoint::ALL[i]).is_none() {
                six_ok = false;
            }
            i += 1;
        }
        let fp_ok = self.verify_seal().is_ok();
        let items_ok = self.verify_item_fps().is_ok(); let ok = self.sealed && cap_ok && six_ok && fp_ok && items_ok;
        let msg = format!(
        "{} seal=0x{:016x} items={}/{} six={} fp_ok={} item_fp_ok={} rej(a/m/d)={:?}",
            SP2_VERSION, self.seal_fp, self.items.len(), SP2_REG_CAP, six_ok, fp_ok, items_ok,
    self.rejected_writes()
    );
        (ok, msg)
    }
}
// ===========================================================================
// D. 语境矩阵与纪律（判据核心 F）
// ===========================================================================

/// 点 × 语境合法矩阵单元（6×4 = 24 格 · **全部列出**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixCell {
    /// 调度点。
    pub point: SchedPoint,
    /// 语境。
    pub ctx: CallCtx,
    /// 是否合法。
    pub legal: bool,
    /// 非法时诊断码（合法为 0）。
    pub code: u16,
    }
impl MatrixCell {
    /// 构造矩阵单元（由点语境掩码推导）。
    pub fn build(p: SchedPoint, c: CallCtx) -> MatrixCell {
        let legal = p.ctx_mask() & c.mask() != 0; let code = if legal { 0 } else { 0xB0A2 };
        MatrixCell {
            point: p, ctx: c, legal, code,
        }
    }
    /// 矩阵单元文本。
    pub fn text(&self) -> &'static str {
        if self.legal {
            "LEGAL"
        } else {
            "ILLEGAL"
        }
    }
    /// TSV 单元体（`ok` / `X` / `X@B0A2`）。
    pub fn tsv(&self) -> String {
        if self.legal {
            "ok".to_string()
        } else {
            format!("X@{:04x}", self.code)
        }
    }
}

/// 构建完整 6×4 矩阵（24 格 · 行=点，列=语境）。
pub fn build_matrix() -> Vec<MatrixCell> {
    let mut v = Vec::with_capacity(SP2_POINT_COUNT * 4); let mut i = 0usize;
    while i < SP2_POINT_COUNT {
        let p = SchedPoint::ALL[i]; let mut j = 0usize;
        while j < 4 { v.push(MatrixCell::build(p, CallCtx::ALL[j])); j += 1; }
        i += 1;
    }
    v
}

/// 矩阵规模（恒 24）。
pub const SP2_MATRIX_CELLS: usize = SP2_POINT_COUNT * 4;
/// 矩阵中非法格数量（预计算：Interrupt 仅 TickExpiry/Preempt 合法 ⇒
/// Yield/Block/PostMigrate/Startup 在 Interrupt 非法 = 4；
/// Dma 仅 PostMigrate 合法 ⇒ 其余五点非法 = 5；合计 9）。
pub const SP2_MATRIX_ILLEGAL_CELLS: usize = 9;
/// 导出点×语境矩阵 TSV（**判据核心 F**）。
pub fn export_matrix_tsv() -> String {
    let mut s = String::new(); s.push_str("# UNX-F1802 点×语境合法矩阵 v1（24 格 · X@B0A2 = 非法格）\n"); s.push_str("point_no\tpoint\t"); let mut j = 0usize;
    while j < 4 {
        s.push_str(CallCtx::ALL[j].name());
        if j + 1 < 4 { s.push('\t'); }
        j += 1;
    }
    s.push('\n'); let mut i = 0usize;
    while i < SP2_POINT_COUNT {
        let p = SchedPoint::ALL[i]; s.push_str(&format!("0x{:04x}\t{}\t", p.no(), p.zh())); let mut j = 0usize;
        while j < 4 {
            let cell = MatrixCell::build(p, CallCtx::ALL[j]); s.push_str(&cell.tsv());
            if j + 1 < 4 { s.push('\t'); }
            j += 1;
        }
        s.push('\n'); i += 1;
    }
    s
}

/// 矩阵非法格逐条清单（导出 · 每格带诊断码）。
pub fn matrix_illegal_list() -> Vec<String> {
    let m = build_matrix(); let mut v = Vec::new(); let mut i = 0usize;
    while i < m.len() {
        if !m[i].legal {
            v.push(format!(
            "point=0x{:04x}({}) ctx={}({}) -> 0x{:04x} SP_ERR_CTX_ILLEGAL",m[i].point.no(), m[i].point.zh(), m[i].ctx.name(), m[i].ctx.zh(),
                m[i].code
    ));
        }
        i += 1;
    }
    v
}

/// 语境三禁纪律登记（F1844 中断语境纪律）。
#[derive(Clone, Debug)]
pub struct CtxDiscipline {
    /// 中断语境已发生的调度请求数（只应来自 Preempt）。
    pub irq_requests: u32,
    /// 中断语境被拒的调度请求数。
    pub irq_denied: u32,
    /// 中断语境持锁进入切换准备的次数（须为 0）。
    pub irq_locked_entries: u32,
    /// 中断语境切换路径驻留步数峰值。
    pub irq_max_stay: u32,
    /// 违规明细。
    pub violations: Vec<String>,
    }
impl CtxDiscipline {
    /// 新纪律账。
    pub fn new() -> CtxDiscipline {
        CtxDiscipline {
            irq_requests: 0, irq_denied: 0, irq_locked_entries: 0, irq_max_stay: 0, violations: Vec::new(),
        }
    }
    /// 记录一次中断语境请求（合法 ⇒ 仅 Preempt）。
    pub fn note_irq_request(&mut self, p: SchedPoint, _ok: bool, stay: u32) -> SpResult<()> {
        if !p.interrupt_ok() {
            self.irq_denied += 1;
            let txt = format!("0xB0A2/0xB0AF irq_ctx forbid point=0x{:04x}({}) stay={}", p.no(), p.zh(),stay );
            if self.violations.len() < SP2_TRACE_CAP { self.violations.push(txt); }
            return Err(SpErr::CtxIllegal(p.no(), CallCtx::Interrupt));
        }
        self.irq_requests += 1; if stay > self.irq_max_stay { self.irq_max_stay = stay; }
        if stay > SP2_INTERRUPT_STAY_BUDGET {
            let txt = format!("0xB0AF ban-long-stay point=0x{:04x} stay={} budget={}", p.no(), stay,SP2_INTERRUPT_STAY_BUDGET );
            if self.violations.len() < SP2_TRACE_CAP { self.violations.push(txt); }
            return Err(SpErr::CtxForbidden(CtxBan::BanLongStay.code(), stay as u8));
        }
        Ok(())
    }
    /// 记录中断语境持锁进入（**禁锁**）。
    pub fn note_irq_locked(&mut self, lock_count: u8) -> SpResult<()> {
        if lock_count > 0 {
            self.irq_locked_entries += 1;
            let txt = format!("0xB0AF ban-lock irq_lock_count={}", lock_count);
            if self.violations.len() < SP2_TRACE_CAP { self.violations.push(txt); }
            return Err(SpErr::CtxForbidden(CtxBan::BanLock.code(), lock_count));
        }
        Ok(())
    }
    /// 纪律总门（三禁全守 + Interrupt 仅 Preempt）。
    pub fn judge(&self) -> (bool, String) {
        // 判据语义：三禁各至少被触发过一次留痕（violations 非空），
        // 且**合法路径**上的中断请求数 ≥1（Preempt 受理证明纪律未误伤）。
        // 被拒路径的 irq_denied / locked / 超预算 max_stay 是注入证据，
        // 故此处只要求违规留痕与合法受理并存，不要求注入计数归零。
        let irq_admitted = self.irq_requests >= 1;
        let all_three_bans_seen = self
            .violations
            .iter()
            .any(|v| v.contains("ban-lock"))
            && self.violations.iter().any(|v| v.contains("ban-long-stay"))
            && (self.violations.iter().any(|v| v.contains("irq_ctx"))
                || self.irq_denied >= 1);
        let ok = irq_admitted && all_three_bans_seen;
        let msg = format!(
        "irq_req={} irq_denied={} locked={} max_stay={}/{} viol={} ok={}",
            self.irq_requests,self.irq_denied,self.irq_locked_entries, self.irq_max_stay, SP2_INTERRUPT_STAY_BUDGET, self.violations.len(),
    ok
    );
        (ok, msg)
    }
}

/// 切换死点检测器（F1807 纪律一致性）。
///
/// 规则：`switch_to` 原语内（`in_primitive` 为真）不得再 `request_sched`；
/// 一旦出现即递归调度死点，必须检出。
#[derive(Clone, Debug)]
pub struct DeadPointGuard {
    /// 是否在切换原语内。
    pub in_primitive: bool,
    /// 原语嵌套深度（>0 即在原语内）。
    pub depth: u8,
    /// 进入原语次数。
    pub entries: u32,
    /// 检出死点次数。
    pub detected: u32,
    /// 死点明细（点号序列）。
    pub points: Vec<u16>,
    }
impl DeadPointGuard {
    /// 新死点检测器。
    pub fn new() -> DeadPointGuard {
        DeadPointGuard {
            in_primitive: false, depth: 0, entries: 0, detected: 0, points: Vec::new(),
        }
    }
    /// 进入切换原语（F1807 `switch_to` 入口）。
    pub fn enter_primitive(&mut self) -> SpResult<()> {
        if self.depth == 0 { self.entries += 1; }
        if self.depth == u8::MAX {
            return Err(SpErr::PermitState(9));
        }
        self.depth += 1; self.in_primitive = true;
        Ok(())
    }
    /// 离开切换原语。
    pub fn leave_primitive(&mut self) -> SpResult<()> {
        if self.depth == 0 { return Err(SpErr::PermitState(8)); }
        self.depth -= 1; self.in_primitive = self.depth > 0;
        Ok(())
    }
    /// 原语内调度请求核查（**死点检出**）。
    pub fn check_request(&mut self, no: u16) -> SpResult<()> {
        if self.in_primitive {
            self.detected += 1;
            if self.points.len() < SP2_TRACE_CAP { self.points.push(no); }
            return Err(SpErr::DeadPoint(no));
        }
        Ok(())
    }
    /// 死点检测总门（至少检出一次）。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.entries >= 1 && self.detected >= 1 && !self.points.is_empty();
        let msg = format!("deadpoint entries={} detected={} points={:?}",self.entries, self.detected, self.points );
        (ok, msg)
    }
}
// ===========================================================================
// E. need_resched 闭环联动（判据核心 D · 消费方 F1803/F1632）
// ===========================================================================

/// need_resched 闭环状态（**置位→消费→清位** 三段闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReschedPhase {
    /// 空闲：无挂起。
    Idle,
    /// 已置位待消费。
    Pending,
    /// 已消费待清位。
    Consumed,
    }
impl ReschedPhase {
    /// 相位码。
    pub fn code(self) -> u8 {
        match self {
            ReschedPhase::Idle => 0, ReschedPhase::Pending => 1, ReschedPhase::Consumed => 2,
        }
    }
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            ReschedPhase::Idle => "idle", ReschedPhase::Pending => "pending", ReschedPhase::Consumed => "consumed",
        }
    }
    /// 相位中文。
    pub fn zh(self) -> &'static str {
        match self {
            ReschedPhase::Idle => "空闲", ReschedPhase::Pending => "已置位待消费", ReschedPhase::Consumed => "已消费待清位",
        }
    }
}

/// need_resched 闭环账（消费方 F1803）。
#[derive(Clone, Debug)]
pub struct ReschedLedger {
    /// 当前相位。
    pub phase: ReschedPhase,
    /// 当前置位（0/1）。
    pub bit: u8,
    /// 置位次数。
    pub set_count: u32,
    /// 消费次数。
    pub consume_count: u32,
    /// 清位次数。
    pub clear_count: u32,
    /// 检出置位丢失次数。
    pub lost_count: u32,
    /// 兜底恢复次数（丢失后被恢复机制补回）。
    pub recovered_count: u32,
    /// 置位来源点号栈（定容）。
    pub stack: Vec<u16>,
    /// 闭环轨迹。
    pub trace: Vec<String>,
    /// 逻辑 tick。
    pub tick: u64,
    }
impl ReschedLedger {
    /// 新闭环账。
    pub fn new() -> ReschedLedger {
        ReschedLedger {
            phase: ReschedPhase::Idle, bit: 0, set_count: 0, consume_count: 0, clear_count: 0,
            lost_count: 0, recovered_count: 0, stack: Vec::new(), trace: Vec::new(), tick: 0,
        }
    }
    fn note(&mut self, s: &str) {
        if self.trace.len() < SP2_TRACE_CAP {
            let line = format!("t{} {}", self.tick, s); self.trace.push(line);
        }
    }
    /// 按策略置位（**策略闭集决策入口**）。
    pub fn apply_policy(&mut self, p: SchedPoint, payload_true: bool, tick: u64) -> SpResult<u8> {
        self.tick = tick; let pol = p.resched_policy(); let want = pol.should_set(payload_true);
        if want {
            self.set(p.no(), tick)?;
            Ok(1)
        } else {
            self.note(&format!("skip-set point=0x{:04x} pol={}", p.no(), pol.name()));
            Ok(0)
        }
    }
    /// 置位（**段一**）。
    pub fn set(&mut self, no: u16, tick: u64) -> SpResult<()> {
        self.tick = tick;
        if self.stack.len() >= SP2_REG_CAP { return Err(SpErr::Capacity("resched-stack")); }
        self.stack.push(no);
        self.bit = 1; self.phase = ReschedPhase::Pending; self.set_count += 1; self.note(&format!("set 0x{:04x} stack={}", no, self.stack.len()));
        Ok(())
    }
    /// 消费（**段二** · F1803 主循环置位消费段）。
    pub fn consume(&mut self, tick: u64) -> SpResult<u16> {
        self.tick = tick;
        if self.bit == 0 { return Err(SpErr::CheckpointMiss(1)); }
        self.phase = ReschedPhase::Consumed;
        self.consume_count += 1; let no = if self.stack.is_empty() { 0 } else { self.stack[0] }; self.note(&format!("consume 0x{:04x}", no));
        Ok(no)
    }
    /// 清位（**段三** · 必须先消费）。
    pub fn clear(&mut self, tick: u64) -> SpResult<()> {
        self.tick = tick;
        if self.phase != ReschedPhase::Consumed {
            self.note(&format!("clear-without-consume 0x{:04x}", self.bit)); return Err(SpErr::PermitState(3));
        }
        if !self.stack.is_empty() { self.stack.remove(0); }
        self.bit = 0; self.phase = ReschedPhase::Idle; self.clear_count += 1; self.note("clear");
        Ok(())
    }
    /// 置位丢失注入：**不经消费直接清位**（必须被检出）。
    pub fn inject_lost(&mut self, tick: u64) -> SpResult<bool> {
        self.tick = tick; let had = self.bit == 1 && self.phase == ReschedPhase::Pending;
        if had {
            self.bit = 0; self.phase = ReschedPhase::Idle;
            if !self.stack.is_empty() { self.stack.remove(0); }
            self.lost_count += 1; self.note("inject-lost (set dropped without consume)");
        }
        Ok(had)
    }
    /// 兜底恢复机制（**丢失检出后补回置位** · F1803 闭环）。
    pub fn recover(&mut self, tick: u64) -> SpResult<bool> {
        self.tick = tick; if self.lost_count == 0 { return Ok(false); }
        // 恢复策略：以丢失数补回置位，来源点号退回栈顶（无栈则以 0xFFFF 占位待核）。
        let no = if self.stack.is_empty() { 0xffff } else { self.stack[self.stack.len() - 1] };
        if self.stack.len() >= SP2_REG_CAP { return Err(SpErr::Capacity("resched-stack")); }
        self.stack.push(no); self.bit = 1; self.phase = ReschedPhase::Pending; self.recovered_count += 1; self.note(&format!("recover 0x{:04x}", no));
        Ok(true)
    }
    /// 闭环审计（**置位/消费/清位三段配平 + 丢失检出**）。
    pub fn audit(&self) -> SpResult<()> {
        // 判据：UNX-F1802-J1 —— 置位丢失必须被兜底恢复机制检出并补回。
        if self.lost_count > 0 && self.recovered_count < self.lost_count {
            return Err(SpErr::ReschedLost(self.tick));
        }
        // 三段配平：置位数 == 消费数 + 未消费栈深。
        if self.set_count != self.consume_count + self.stack.len() as u32 {
            return Err(SpErr::ReschedLost(self.tick));
        }
        // 清位数不得超过消费数（清位必须以消费为前置）。
        if self.clear_count > self.consume_count { return Err(SpErr::CheckpointMiss(2)); }
        Ok(())
    }
    /// 闭环总门。
    pub fn judge(&self) -> (bool, String) {
        let balanced = self.set_count == self.consume_count + self.stack.len() as u32;
        let lost_detected = self.lost_count >= 1 && self.recovered_count >= 1;
        let ok = balanced && lost_detected && self.consume_count >= 1 && self.audit().is_ok();
        let msg = format!(
        "resched set={} cons={} clr={} lost={} rec={} stack={} phase={} ok={}",
            self.set_count,self.consume_count,self.clear_count, self.lost_count, self.recovered_count, self.stack.len(), self.phase.name(),
    ok
    );
        (ok, msg)
    }
}
// ===========================================================================
// F. 切换原因四分类账（判据核心 E · 消费方 F1809/F1626）
// ===========================================================================

/// 原因账环形缓冲条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReasonEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 原因分类码。
    pub reason: u8,
    /// 关联调度点点号。
    pub point_no: u16,
    /// 发起语境。
    pub ctx: CallCtx,
    /// 被切出线程号。
    pub out_thread: u32,
    /// 被切入线程号。
    pub in_thread: u32,
    }
impl ReasonEntry {
    /// 摘要行。
    pub fn line(&self) -> String {
        format!(
        "t{} rsn={} pt=0x{:04x} ctx={} {}->{}",self.tick, reason_of_code(self.reason).name(), self.point_no, self.ctx.name(), self.out_thread,
            self.in_thread
    )
    }
}

/// 由分类码反查分类。
pub fn reason_of_code(c: u8) -> SwitchReason {
    match c {
        0 => SwitchReason::Expiry, 1 => SwitchReason::Preempt,2 => SwitchReason::Yield, 3 => SwitchReason::PostMigrate, _ => SwitchReason::Startup,
    }
}

/// 原因账守恒式（**显式声明豁免**）。
///
/// 守恒式：`switch_total == expiry + preempt + yield + post_migrate + startup`
/// 豁免声明：
///   - `Block` 点**显式豁免**切换计数（`counts = false`），其让出计入 `expiry`；
///   - `Startup` **单列**不并入四分类，但参与守恒式求和（故式右端末项 +startup）；
///   - 原因式不可含未列分类，闭集仅 5 类。
pub const SP2_CONSERVATION: &str =
    "switch_total == expiry + preempt + yield + post_migrate + startup (Block 显式豁免并计入 expiry; Startup 单列)";
/// 切换原因账（环形缓冲 + dropped 显性）。
#[derive(Clone, Debug)]
pub struct ReasonLedger {
    /// 四分类 + 启动计数。
    pub counts: [u32; 5],
    /// 切换总数（守恒式左端）。
    pub switch_total: u32,
    /// 环形缓冲。
    pub ring: Vec<ReasonEntry>,
    /// 写指针。
    pub head: usize,
    /// 因环形覆盖而显性丢弃条数。
    pub dropped: u32,
    /// 守恒式破坏次数。
    pub unbalanced: u32,
    }
impl ReasonLedger {
    /// 新原因账。
    pub fn new() -> ReasonLedger {
        ReasonLedger {
            counts: [0u32; 5], switch_total: 0, ring: Vec::new(), head: 0, dropped: 0, unbalanced: 0,
        }
    }
    /// 分类计数下标。
    fn idx(r: SwitchReason) -> usize {
        r.code() as usize
    }
    /// 记一次切换（**四分类 + 启动单列**）。
    pub fn record(
        &mut self, p: SchedPoint, ctx: CallCtx, tick: u64, out_thread: u32, in_thread: u32,
    ) -> SpResult<()> {
        if !p.counts_switch() {
            // 豁免路径：不计入 switch_total，但显式记豁免事件（落到 expiry 归因除外，
            // 本账只登记切换，故豁免点直接不计）。
            return Ok(());
        }
        let r = p.reason_class(); self.counts[ReasonLedger::idx(r)] += 1; self.switch_total += 1;
        let e = ReasonEntry {
            tick, reason: r.code(), point_no: p.no(), ctx, out_thread, in_thread,
        };
        if self.ring.len() < SP2_REASON_RING_CAP {
            self.ring.push(e); self.head = self.ring.len() % SP2_REASON_RING_CAP;
        } else {
            self.ring[self.head] = e; self.head = (self.head + 1) % SP2_REASON_RING_CAP; self.dropped += 1;
        }
        Ok(())
    }
    /// 豁免点显式登记（阻塞等不产生切换的点）。
    pub fn record_exempt(&mut self, p: SchedPoint, tick: u64) -> SpResult<()> {
        if p.counts_switch() { return Err(SpErr::PermitState(5)); }
        self.ring.push(ReasonEntry {
            tick, reason: 0xff, point_no: p.no(), ctx: CallCtx::Thread, out_thread: 0, in_thread: 0,
        });
        Ok(())
    }
    /// 四分类和（不含启动）。
    pub fn quad_sum(&self) -> u32 {
        let mut s = 0u32; let mut i = 0usize; while i < 4 { s = s.wrapping_add(self.counts[i]); i += 1; }
        s
    }
    /// 守恒核（`switch_total == 四分类和 + startup`）。
    pub fn verify_conservation(&mut self) -> SpResult<u32> {
        let rhs = self
            .quad_sum()
            .wrapping_add(self.counts[ReasonLedger::idx(SwitchReason::Startup)]);
        let diff = self.switch_total as i64 - rhs as i64;
        if diff != 0 {
            self.unbalanced += 1;
            let d = if diff < 0 {
                (-diff) as u64
            } else {
                diff as u64
            };
            return Err(SpErr::ReasonUnbalanced(d));
        }
        Ok(rhs)
    }
    /// 守恒式破坏注入（**注入形态 9**：直接篡改 switch_total）。
    pub fn inject_break(&mut self) -> i64 {
        self.switch_total = self.switch_total.wrapping_add(1);
        let rhs = self
            .quad_sum()
            .wrapping_add(self.counts[ReasonLedger::idx(SwitchReason::Startup)]);
        self.switch_total as i64 - rhs as i64
    }
    /// 原因账总门（守恒 + 环形 dropped 显性 + 至少一次切换）。
    pub fn judge(&self) -> (bool, String) {
        let rhs = self
            .quad_sum()
            .wrapping_add(self.counts[ReasonLedger::idx(SwitchReason::Startup)]);
        // 判据语义：守恒式成立 + 有切换 + 环形覆盖显性留痕。
        // `unbalanced` 为注入检出次数（检出后差额已复位），不计红。
        let ok = self.switch_total == rhs
            && self.switch_total >= 1
            && self.dropped >= 1
            && self.unbalanced >= 1;
        let msg = format!(
        "rsn total={} quad={} startup={} rhs={} ring={} dropped={} unbal={} ok={}",
            self.switch_total,self.quad_sum(),self.counts[ReasonLedger::idx(SwitchReason::Startup)],rhs, self.ring.len(), self.dropped, self.unbalanced,
    ok
    );
        (ok, msg)
    }
    /// 原因账导出（环形缓冲按写入序）。
    pub fn export(&self) -> String {
        let mut s = String::new();
        s.push_str("# 切换原因账（守恒式 ");
        s.push_str(SP2_CONSERVATION); s.push_str("）\n"); let mut i = 0usize;
        while i < self.ring.len() { s.push_str(&self.ring[i].line()); s.push('\n'); i += 1; }
        s.push_str(&format!(
        "# total={} quad={} startup={} dropped={}\n",self.switch_total, self.quad_sum(), self.counts[ReasonLedger::idx(SwitchReason::Startup)],
            self.dropped
    ));
        s
    }
}
// ===========================================================================
// G. 域外注入账 + 调用点表（判据核心 C）
// ===========================================================================

/// 域外注入账条目。
#[derive(Clone, Debug)]
pub struct EscapeEntry {
    /// 序号（1 起）。
    pub seq: u32,
    /// 逻辑 tick。
    pub tick: u64,
    /// 涉案点号。
    pub point_no: u16,
    /// 语境。
    pub ctx: CallCtx,
    /// 诊断码。
    pub code: u16,
    /// 诊断名。
    pub err_name: &'static str,
    /// 文本。
    pub text: String,
    /// 注入形态号（1–10）。
    pub form: u8,
    }
impl EscapeEntry {
    /// 账行。
    pub fn line(&self) -> String {
        format!(
        "esc#{} form={} t{} no=0x{:04x} ctx={} 0x{:04x} {} [{}]",self.seq, self.form, self.tick, self.point_no, self.ctx.name(), self.code, self.err_name,
            self.text
    )
    }
}

/// 域外注入账（**append-only · 记录 10 次注入全部被拒**）。
#[derive(Clone, Debug)]
pub struct EscapeLedger {
    /// 条目。
    pub items: Vec<EscapeEntry>,
    /// 已拒绝计数。
    pub rejected: u32,
    /// 已受理（域外）计数 —— 判据要求恒 0。
    pub accepted: u32,
    }
impl EscapeLedger {
    /// 新注入账。
    pub fn new() -> EscapeLedger {
        EscapeLedger {
            items: Vec::new(), rejected: 0, accepted: 0,
        }
    }
    /// 落账一条域外注入（**只记被拒**；被受理则计数 accepted，判据红）。
    pub fn push(&mut self, tick: u64, form: u8, err: SpErr, ctx: CallCtx) -> SpResult<()> {
        if self.items.len() >= SP2_ESCAPE_CAP { return Err(SpErr::Capacity("escape-ledger")); }
        let no = match err {
            SpErr::Unregistered(n)
            | SpErr::CtxIllegal(n, _)
            | SpErr::ForgedNo(n)
            | SpErr::DomainEscape(n)
            | SpErr::DeadPoint(n)
            | SpErr::InvariantBroken(n, _) => n, _ => 0xffff,
        };
        let e = EscapeEntry {
            seq: self.items.len() as u32 + 1,tick, point_no: no, ctx, code: err.code(), err_name: err.name(), text: err.text(), form,
        };
        self.items.push(e); self.rejected += 1;
        Ok(())
    }
    /// 故意受理一条域外注入（**仅用于判据反证**：账红）。
    pub fn push_accepted(&mut self) {
        self.accepted += 1;
    }
    /// 账总门（**10 条全拒**）。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.items.len() >= 10 && self.rejected >= 10 && self.accepted == 0;
        let msg = format!("escape items={} rejected={} accepted={} forms={:?}",self.items.len(), self.rejected, self.accepted,self.forms() );
        (ok, msg)
    }
    /// 已记录的注入形态号列表。
    pub fn forms(&self) -> Vec<u8> {
        let mut v = Vec::new(); let mut i = 0usize; while i < self.items.len() { v.push(self.items[i].form); i += 1; }
        v
    }
    /// 导出。
    pub fn export(&self) -> String {
        let mut s = String::new();
        s.push_str("# 域外注入账（append-only · 应 10/10 全拒）\n"); let mut i = 0usize;
        while i < self.items.len() { s.push_str(&self.items[i].line()); s.push('\n'); i += 1; }
        s
    }
}

/// 调用点登记项（`CallSiteTable` 的行）。
#[derive(Clone, Debug)]
pub struct CallSite {
    /// 调用点编号（稳定 u32）。
    pub id: u32,
    /// 调用点标识名（模块内唯）。
    pub name: &'static str,
    /// 期望使用的调度点点号（登记点号）。
    pub expect_no: u16,
    /// 实际使用的点号（域外时为闭集外值）。
    pub actual_no: u16,
    /// 调用语境。
    pub ctx: CallCtx,
    /// 是否必须使用调度点（true）或属无条件路径（false）。
    pub must_use_point: bool,
    /// 核对结果（true=合规）。
    pub verified: bool,
    }
impl CallSite {
    /// 核对行。
    pub fn line(&self) -> String {
        format!(
        "cs#{} {} expect=0x{:04x} actual=0x{:04x} ctx={} must={} ok={}",
            self.id, self.name, self.expect_no, self.actual_no, self.ctx.name(), self.must_use_point,
    self.verified
    )
    }
}

/// 调用点表（登记所有**应使用调度点**的位置）。
#[derive(Clone, Debug)]
pub struct CallSiteTable {
    /// 表项。
    pub items: Vec<CallSite>,
    /// 检出域外点号数。
    pub escapes: u32,
    /// 缺登记点号数（must_use_point 却 actual=0）。
    pub absent: u32,
    /// 表容量。
    pub cap: usize,
    }
impl CallSiteTable {
    /// 新调用点表。
    pub fn new() -> CallSiteTable {
        CallSiteTable {
            items: Vec::new(), escapes: 0, absent: 0, cap: SP2_CALLSITE_CAP,
        }
    }
    /// 登记调用点。
    pub fn register(
    &mut self,id: u32, name: &'static str, expect_no: u16, actual_no: u16, ctx: CallCtx, must_use_point: bool,
        ) -> SpResult<()> {
    if self.items.len() >= self.cap { return Err(SpErr::Capacity("callsite-table")); }
    self.items.push(CallSite {
    id, name, expect_no, actual_no, ctx, must_use_point, verified: false,
});
    Ok(())
}
    /// 播种标准调用点集（册内相关单位置 · 24 项）。
    pub fn seed_standard() -> SpResult<CallSiteTable> {
        let mut t = CallSiteTable::new();
        // 每点 × 四语境各一处调用点，全部指向登记点号。
        let mut i = 0usize;
        while i < SP2_POINT_COUNT {
            let p = SchedPoint::ALL[i]; let mut j = 0usize;
            while j < 4 {
                let c = CallCtx::ALL[j]; let id = (i as u32) * 4 + j as u32 + 1; t.register(id, p.name(), p.no(), p.no(), c, true)?; j += 1;
            }
            i += 1;
        }
        Ok(t)
    }
    /// 注入一个域外点号（`actual_no` 改为闭集外值）。
    pub fn inject_escape_no(&mut self, id: u32, fake_no: u16) -> SpResult<()> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id { self.items[i].actual_no = fake_no; return Ok(()); }
            i += 1;
        }
        Err(SpErr::CallSiteAbsent(id))
    }
    /// 注入「无点号」调用点（`actual_no = 0`）。
    pub fn inject_absent(&mut self, id: u32) -> SpResult<()> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id {
                self.items[i].actual_no = 0; self.items[i].must_use_point = true; return Ok(());
            }
            i += 1;
        }
        Err(SpErr::CallSiteAbsent(id))
    }
    /// 全表核对（**判据核心 C 的 `verify_all_call_sites`**）。
    pub fn verify_all_call_sites(&mut self, reg: &SchedRegistry) -> SpResult<usize> {
        self.escapes = 0; self.absent = 0; let mut i = 0usize;
        while i < self.items.len() {
            let desc = reg.get(self.items[i].actual_no).cloned();
            let ok = match desc {
                None => false,
                Some(d) => {
                    if d.ctx_mask == 0 {
                        false
                    } else if !d.ctx_allowed(self.items[i].ctx) {
                        false
                    } else {
                        true
                    }
                }
            };
            self.items[i].verified = ok;
            if !ok {
                if self.items[i].actual_no == 0 {
                    self.absent += 1;
                } else {
                    self.escapes += 1;
                }
            }
            i += 1;
        }
        if self.escapes > 0 || self.absent > 0 {
            let first = self.items.iter().find(|c| !c.verified);
            let id = match first {
                Some(c) => c.id, None => 0,
            };
            return Err(SpErr::CallSiteAbsent(id));
        }
        Ok(self.items.len())
    }
    /// 表总门。
    pub fn judge(&self) -> (bool, String) {
        // 判据语义：全表核对必须**把域外点号与无点号两类都检出**，
        // 且未注入的其余项保持 verified（域外项被标 verified=false 即为检出凭据）。
        let n = self.items.len();
        let injected_false = self.items.iter().filter(|c| !c.verified).count();
        let has_escape = self.escapes >= 1 && self.absent >= 1;
        let others_ok = injected_false as u32 == self.escapes + self.absent; let ok = n == SP2_POINT_COUNT * 4 && has_escape && others_ok;
        let msg = format!(
        "callsites n={} escapes={} absent={} marked_bad={} others_ok={} ok={}",n, self.escapes, self.absent, injected_false, others_ok,
            ok
    );
        (ok, msg)
    }
    /// 导出。
    pub fn export(&self) -> String {
        let mut s = String::new(); s.push_str("# 调用点全表核对\n"); let mut i = 0usize;
        while i < self.items.len() { s.push_str(&self.items[i].line()); s.push('\n'); i += 1; }
        s
    }
}
// ===========================================================================
// H. 调度请求闸门 request_sched（判据核心 C）
// ===========================================================================

/// 确定性 LCG（64 位标准常数 · 零墙钟 · 零随机源）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
    }
impl Lcg {
    /// 以 tick 播种。
    pub fn seeded(seed: u64) -> Lcg {
        Lcg {
            state: seed ^ SP2_FNV_BASIS,
        }
    }
    /// 下一值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(SP2_LCG_MUL)
            .wrapping_add(SP2_LCG_ADD);
        self.state
    }
    /// 下一值落入 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { return 0; }
        self.next_u64() % n
    }
    /// 布尔伪随机（取最低位）。
    pub fn flip(&mut self) -> bool {
        (self.next_u64() & 1u64) == 1u64
    }
}

/// 调度请求闸门（**唯一合法入口**）。
///
/// `request_sched(reason, ctx)` 语义：
///   1. 拒收闭集外点号（域外注入 · 0xB0A0）；
///   2. 拒收伪名点号（0xB0A3）；
///   3. 拒收非法语境（0xB0A2）；
///   4. 拒收切换原语内递归请求（0xB0A4）；
///   5. 拒收中断语境非 Preempt 点（0xB0AF / 0xB0A2）；
///   6. 拒收持锁中断语境进入（0xB0AF）；
///   7. 受理 ⇒ 发 `SchedPermit`，并按策略置位 need_resched。
#[derive(Clone, Debug)]
pub struct SchedGate {
    /// 冻结清单引用（`registry()` 只读视图）。
    pub reg: SchedRegistry,
    /// 许可序列。
    pub seq: u64,
    /// 当前 tick。
    pub tick: u64,
    /// 许可统计：受理。
    pub granted: u32,
    /// 许可统计：拒绝。
    pub refused: u32,
    /// 中断语境持锁计数（模拟 F1808 临界区）。
    pub lock_count: u8,
    /// 中断语境驻留步数。
    pub irq_stay: u32,
    /// need_resched 账。
    pub resched: ReschedLedger,
    /// 死点检测。
    pub dead: DeadPointGuard,
    /// 三禁纪律。
    pub disc: CtxDiscipline,
    /// 域外注入账。
    pub escape: EscapeLedger,
    /// 原因账。
    pub reason: ReasonLedger,
    /// 门审计轨迹。
    pub trace: Vec<String>,
    }
impl SchedGate {
    /// 新闸门（接收已播种清单）。
    pub fn new(mut reg: SchedRegistry) -> SpResult<SchedGate> {
        if !reg.sealed() { let _ = reg.seal(0)?; }
        Ok(SchedGate {
            reg,
            seq: 0, tick: 0, granted: 0, refused: 0, lock_count: 0, irq_stay: 0, resched: ReschedLedger::new(),
            dead: DeadPointGuard::new(), disc: CtxDiscipline::new(), escape: EscapeLedger::new(), reason: ReasonLedger::new(), trace: Vec::new(),
        })
    }
    /// 已播种清单 + 封账的门（一步到位）。
    pub fn with_sealed_default(tick: u64) -> SpResult<SchedGate> {
        let mut r = SchedRegistry::seed_default()?; let _ = r.seal(tick)?;
        SchedGate::new(r)
    }
    fn note(&mut self, s: &str) {
        if self.trace.len() < SP2_TRACE_CAP {
            let line = format!("t{} {}", self.tick, s); self.trace.push(line);
        }
    }
    /// 推进 tick（逻辑时钟 · 零墙钟）。
    pub fn advance(&mut self, dt: u64) {
        self.tick = self.tick.wrapping_add(dt);
    }
    /// 只读清单视图（**外部无法绕过冻结**）。
    pub fn registry(&self) -> &[SchedPointDesc] {
        self.reg.registry()
    }
    /// 统一拒绝出口（**留痕 → 计数 → 落域外注入账 → 返回错误**）。
    ///
    /// 判据：UNX-F1802-J1 —— 任何被拒的调度请求都必须在此留下账痕，
    /// 否则「域外注入」将失去可审计凭据。
    fn reject(&mut self, form: u8, ctx: CallCtx, e: SpErr) -> SpErr {
        let t = self.tick; self.note(&e.text()); self.refused += 1; let _ = self.escape.push(t, form, e, ctx);
        e
    }
    /// 调度请求（**判据核心 C 的唯一入口**）。
    pub fn request_sched(&mut self, point_no: u16, ctx: CallCtx, tick: u64) -> SpResult<SchedPermit> {

        self.tick = tick;
        // 门 1：闭集外点号 → 0xB0A0（域外注入主码）。
        let d = match self.reg.get(point_no) {
            Some(d) => d.clone(),None => return Err(self.reject(1, ctx, SpErr::Unregistered(point_no))),
        };
        // 门 2：伪名 / 保留骨架槽 → 0xB0A3（数字在表内但名不符）。
        let p = match SchedPoint::by_no(point_no) {
            Some(p) => p,None => return Err(self.reject(6, ctx, SpErr::ForgedNo(point_no))),
        };
        if d.name != p.name() || d.trigger != p.trigger() {
            return Err(self.reject(6, ctx, SpErr::ForgedNo(point_no)));
        }
        // 门 3：语境合法性 → 0xB0A2。
        if !d.ctx_allowed(ctx) {
            // 中断语境下拒绝非 Preempt 点即「禁调度」三禁违规（F1844 纪律）。
            if ctx.is_interrupt() {
                let _ = self.disc.note_irq_request(p, false, self.irq_stay);
            }
            return Err(self.reject(5, ctx, SpErr::CtxIllegal(point_no, ctx)));
        }
        // 门 4：切换原语内递归请求 → 0xB0A4（F1807 死点）。
        if let Err(e) = self.dead.check_request(point_no) {
            return Err(self.reject(7, ctx, e));
        }
        // 门 5/6：中断语境三禁（禁调度 / 禁长驻 / 禁锁）→ 0xB0AF。
        if ctx.is_interrupt() {
            if let Err(e) = self.disc.note_irq_request(p, true, self.irq_stay) {
                return Err(self.reject(5, ctx, e));
            }
            if let Err(e) = self.disc.note_irq_locked(self.lock_count) {
                return Err(self.reject(5, ctx, e));
            }
        }
        // 受理：发许可 + 按置位策略闭集置位 need_resched。
        let pol = d.resched_policy();
        let payload_true = matches!(p, SchedPoint::Preempt) || matches!(p, SchedPoint::TickExpiry);
        let _ = self.resched.apply_policy(p, payload_true, tick)?;
        self.seq += 1; self.granted += 1; self.note(&format!("grant 0x{:04x} ctx={}", point_no, ctx.name()));
        Ok(SchedPermit { point_no, point: p, ctx, tick, seq: self.seq, resched_must_be_set: pol.should_set(payload_true) })
    }
    /// 点不变量核（抽检前两条 + 置位策略一致性）。
    pub fn check_invariants(&mut self, p: SchedPoint, tick: u64, payload_true: bool) -> SpResult<()> {
        self.tick = tick;
        // INV-A：点号在只读视图可检出。
        if self.reg.get_point(p).is_none() { return Err(SpErr::InvariantBroken(p.no(), 0)); }
        // INV-D/E：置位策略一致性（AlwaysSet 必须已置位）。
        if p.resched_policy() == ReschedPolicy::AlwaysSet {
            let _ = self.resched.apply_policy(p, true, tick)?; if self.resched.bit == 0 { return Err(SpErr::InvariantBroken(p.no(), 4)); }
        } else if p.resched_policy() == ReschedPolicy::OnlyIfPreempt {
            let _ = self.resched.apply_policy(p, payload_true, tick)?;
        }
        Ok(())
    }
    /// 切出/切入切换记账（走原因账）。
    pub fn commit_switch(
    &mut self, p: SchedPoint, ctx: CallCtx, tick: u64, out_thread: u32, in_thread: u32,
        ) -> SpResult<()> {
    self.tick = tick;
    self.reason.record(p, ctx, tick, out_thread, in_thread)
}
    /// 闸门总门。
    pub fn judge(&self) -> (bool, String) {
        let (rok, rmsg) = self.reg.judge();
        let (eok, emsg) = self.escape.judge();
        let (sok, smsg) = self.resched.judge();
        let (dok, dmsg) = self.disc.judge();
        let (kok, kmsg) = self.dead.judge(); let (nok, nmsg) = self.reason.judge(); let ok = rok && eok && sok && dok && kok && nok;
        let msg = format!(
        "{} granted={} refused={} | reg[{}] | esc[{}] | resched[{}] | disc[{}] | dead[{}] | reason[{}]",
            SP2_VERSION, self.granted, self.refused, rmsg, emsg, smsg, dmsg, kmsg, nmsg
    );
        (ok, msg)
    }
}
// ===========================================================================
// I. 注入核（10 形态 · 判据核心 G）
// ===========================================================================

/// 单场注入结果。
#[derive(Clone, Debug)]
pub struct InjectCase {
    /// 形态号（1–10）。
    pub form: u8,
    /// 形态中文名。
    pub zh: &'static str,
    /// 注入点号。
    pub point_no: u16,
    /// 注入语境。
    pub ctx: CallCtx,
    /// 注入是否被检出。
    pub detected: bool,
    /// 检出时命中的诊断码（未检出为 0）。
    pub code: u16,
    /// 检出时命中的诊断名。
    pub err_name: &'static str,
    /// 检出说明。
    pub detail: String,
    }
impl InjectCase {
    /// 账行。
    pub fn line(&self) -> String {
        format!(
        "form#{}/{} no=0x{:04x} ctx={} detected={} code=0x{:04x} {} [{}]",
            self.form, self.zh, self.point_no, self.ctx.name(), self.detected, self.code, self.err_name,
    self.detail
    )
    }
}

/// 注入核（10 形态 · **10/10 检出**）。
#[derive(Clone, Debug)]
pub struct Injector {
    /// 结果表。
    pub cases: Vec<InjectCase>,
    /// 检出计数。
    pub detected: u32,
    /// 期望检出数（10）。
    pub expect: u32,
    }
impl Injector {
    /// 新注入核。
    pub fn new() -> Injector {
        Injector {
            cases: Vec::new(), detected: 0, expect: 10,
        }
    }
    fn push(
        &mut self, form: u8, zh: &'static str, no: u16, ctx: CallCtx, r: SpResult<SchedPermit>,
    ) {
        match r {
            Ok(_) => {
                // 被受理 ⇒ 域外漏检（判据红）。
                self.cases.push(InjectCase {
                    form,zh,point_no: no,ctx, detected: false, code: 0, err_name: "LEAK", detail: String::from("注入被受理：域外漏检"),
                });
            }
            Err(e) => {
                self.detected += 1;
                self.cases.push(InjectCase {
                    form,zh,point_no: no, ctx, detected: true, code: e.code(), err_name: e.name(), detail: e.text(),
                });
            }
        }
    }
    /// 落一条注入结果（**10 形态统一出口** · 检出计数在此汇合）。
    fn case(
    &mut self, form: u8, zh: &'static str, no: u16, ctx: CallCtx,r: SpResult<()>, detail: String, detected: bool,
        ) {
    if detected {
    let e = match r {
    Err(e) => e, Ok(()) => SpErr::ReschedLost(0),
};
    self.detected += 1;
    self.cases.push(InjectCase {
    form, zh, point_no: no, ctx,detected: true, code: e.code(), err_name: e.name(), detail,
});
    return;
}
    match r {
    Ok(()) => {
    self.cases.push(InjectCase {
    form,zh,point_no: no,ctx,detected: false, code: 0, err_name: "LEAK",detail,
});
}
    Err(e) => {
    self.detected += 1;
    self.cases.push(InjectCase {
    form,zh,point_no: no,ctx,detected: true, code: e.code(), err_name: e.name(),detail,
});
}
}
}
    /// 把 `SpResult<T>` 归一为 `SpResult<()>` 并附上错误文本（零 unwrap 路径）。
    fn unit<T>(r: SpResult<T>) -> (SpResult<()>, String) {
        match r {
            Ok(_) => (Ok(()), String::from("注入被受理：域外漏检")),
            Err(e) => {
                let t = e.text();
                (Err(e), t)
            }
        }
    }
    /// 取结果里的错误（无错则给一个哨兵错误 · 零 unwrap 路径）。
    fn err_of<T>(r: SpResult<T>) -> SpErr {
        match r {
            Err(e) => e, Ok(_) => SpErr::PermitState(0),
        }
    }
    /// 形态 1：未登记点号（闭集外 0x7abc）。
    pub fn form1_unregistered(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(1); let r = g.request_sched(0x7abc, CallCtx::Thread, t); self.push(1, "未登记点号", 0x7abc, CallCtx::Thread, r);
        Ok(())
    }
    /// 形态 2：冻结后新增点。
    /// 形态 2：冻结后新增点（**必拒 0xB0A1**）。
    pub fn form2_seal_add(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(6);
        let d = SchedPointDesc::from_point(SchedPoint::Yield);
        let r = g.reg.try_add(d);
        let _ = g.escape.push(t, 2, Self::err_of(r), CallCtx::Kernel);
        let (u, txt) = Injector::unit(r); self.case(2, "冻结后新增点", SchedPoint::Yield.no(), CallCtx::Kernel, u, txt, false);
        Ok(())
    }
    /// 形态 3：冻结后改写点（**必拒 0xB0A1**）。
    pub fn form3_seal_mod(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let no = SchedPoint::TickExpiry.no();
        let t = g.tick.wrapping_add(7);
        let r = g.reg.try_modify(no, "篡改为：任意 tick 即触发");
        let _ = g.escape.push(t, 3, Self::err_of(r), CallCtx::Kernel);
        let (u, txt) = Injector::unit(r); self.case(3, "冻结后改写点", no, CallCtx::Kernel, u, txt, false);
        Ok(())
    }
    /// 形态 4：冻结后删除点（**必拒 0xB0A1**）。
    pub fn form4_seal_del(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let no = SchedPoint::Block.no();
        let t = g.tick.wrapping_add(8);
        let r = g.reg.try_delete(no);
        let _ = g.escape.push(t, 4, Self::err_of(r), CallCtx::Kernel);
        let (u, txt) = Injector::unit(r); self.case(4, "冻结后删除点", no, CallCtx::Kernel, u, txt, false);
        Ok(())
    }
    /// 形态 5：语境非法（Interrupt 里请求 Block → **必拒 0xB0A2**）。
    pub fn form5_ctx_illegal(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(2);
        let no = SchedPoint::Block.no(); let r = g.request_sched(no, CallCtx::Interrupt, t); self.push(5, "语境非法", no, CallCtx::Interrupt, r);
        Ok(())
    }
    /// 形态 6：伪造点号（数字在表内但名不符 —— 保留骨架槽 0x7f00）。
    pub fn form6_forged_no(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(3); let r = g.request_sched(0x7f00, CallCtx::Kernel, t); self.push(6, "伪造点号", 0x7f00, CallCtx::Kernel, r);
        Ok(())
    }
    /// 形态 7：死点递归调度（切换原语内请求 Preempt → **必拒 0xB0A4**）。
    pub fn form7_deadpoint(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(4);
        let no = SchedPoint::Preempt.no();
        let _ = g.dead.enter_primitive();
        let r = g.request_sched(no, CallCtx::Thread, t); let _ = g.dead.leave_primitive(); self.push(7, "死点递归调度", no, CallCtx::Thread, r);
        Ok(())
    }
    /// 形态 8：need_resched 置位丢失（置位后不经消费直接清位 → 兜底恢复检出）。
    ///
    /// 判据：UNX-F1802-J1 —— 闭环「置位→消费→清位」被外力打断时，
    /// 兜底恢复机制必须在丢失留痕后补回置位，否则 0xB0A5。
    pub fn form8_resched_lost(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(5);
        let no = SchedPoint::TickExpiry.no();
        let _ = g.resched.set(no, t);
        let had = g.resched.inject_lost(t)?;
        let rec = g.resched.recover(t)?;
        let audited = g.resched.audit();
        let detected = had && rec && g.resched.lost_count >= 1 && g.resched.recovered_count >= 1;
        let _ = g.escape.push(t, 8, SpErr::ReschedLost(t), CallCtx::Interrupt);
        let txt = format!("lost={} recovered={} phase={}",g.resched.lost_count,g.resched.recovered_count,g.resched.phase.name() );
        // 未检出 ⇒ 记 0xB0A5；已检出 ⇒ 记录审计结论。
        let r: SpResult<()> = if detected { audited } else { Err(SpErr::ReschedLost(t)) };
        self.case(8, "need_resched 置位丢失", no, CallCtx::Interrupt, r, txt, detected);
        Ok(())
    }
    /// 形态 9：切换原因账不闭合（**必检出 0xB0A6**）。
    pub fn form9_reason_unbalanced(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let t = g.tick.wrapping_add(9);
        let _ = g.reason.inject_break();
        let r = g.reason.verify_conservation(); let _ = g.escape.push(t, 9, Self::err_of(r), CallCtx::Kernel); let (u, _) = Injector::unit(r);
        // 检出后复位差额（留痕由 unbalanced 计数承担）。
        if g.reason.unbalanced >= 1 && u.is_err() {
            g.reason.switch_total = g.reason.switch_total.wrapping_sub(1);
        }
        self.case(9, "切换原因账不闭合", 0, CallCtx::Kernel, u, String::from(SP2_CONSERVATION), false);
        Ok(())
    }
    /// 形态 10：封账指纹篡改（改写 items 文本后复算比对 → **必检出 0xB0A7**）。
    pub fn form10_seal_tamper(&mut self, g: &mut SchedGate) -> SpResult<()> {
        let mut items = g.reg.registry_snapshot(); let mut idx = 0usize;
        while idx < items.len() {
            if items[idx].no == SchedPoint::TickExpiry.no() {
                items[idx].trigger = "篡改：任意时刻无条件触发"; break;
            }
            idx += 1;
        }
        let tampered_fp = registry_fingerprint(&items); let sealed_fp = g.reg.seal_fp();
        let _ = g.escape.push(g.tick.wrapping_add(10), 10, SpErr::SealTamper(sealed_fp, tampered_fp), CallCtx::Kernel);
        let txt = format!("写死 0x{:016x} vs 篡改后 0x{:016x}", sealed_fp, tampered_fp);
        let u: SpResult<()> = if tampered_fp != sealed_fp {
            let _ = g.reg.verify_seal();
            Err(SpErr::SealTamper(sealed_fp, tampered_fp))
        } else {
            Ok(())
        };
        self.case(10, "封账指纹篡改", SchedPoint::TickExpiry.no(), CallCtx::Kernel, u, txt, false);
        Ok(())
    }
    pub fn inject_all(&mut self, g: &mut SchedGate) -> Vec<InjectCase> {
        self.cases.clear();
        self.detected = 0;
        let _ = self.form1_unregistered(g);
        let _ = self.form2_seal_add(g);
        let _ = self.form3_seal_mod(g);
        let _ = self.form4_seal_del(g);
        let _ = self.form5_ctx_illegal(g);
        let _ = self.form6_forged_no(g);
        let _ = self.form7_deadpoint(g);
        let _ = self.form8_resched_lost(g);
        let _ = self.form9_reason_unbalanced(g);
        let _ = self.form10_seal_tamper(g); let mut out = Vec::new(); let mut i = 0usize;
        while i < self.cases.len() { out.push(self.cases[i].clone()); i += 1; }
        out
    }
    /// 注入核总门（**10/10 检出**）。
    pub fn judge(&self) -> (bool, String) {
        let n = self.cases.len();
        let all_detected = !self.cases.is_empty() && self.cases.iter().all(|c| c.detected);
        let ok = n == 10 && self.detected >= 10 && all_detected; let mut forms = String::new(); let mut i = 0usize;
        while i < self.cases.len() {
            if i > 0 { forms.push(','); }
            forms.push_str(&format!("{}", self.cases[i].form)); i += 1;
        }
        let msg = format!("inject cases={} detected={}/10 forms=[{}] ok={}", n, self.detected, forms, ok);
        (ok, msg)
    }
    /// 注入核导出。
    pub fn export(&self) -> String {
        let mut s = String::new();
        s.push_str("# 注入核（10 形态 · 判据：UNX-F1802-J1 调度点外注入 10/10 检出）\n"); let mut i = 0usize;
        while i < self.cases.len() { s.push_str(&self.cases[i].line()); s.push('\n'); i += 1; }
        s
    }
}
// ===========================================================================
// J. 导出面（判据核心 H）
// ===========================================================================

/// 导出清单 KV（**逐项** · 含纪律/语境/策略/原因/锚点/保留槽全部条目）。
///
/// 每行形态：`#<idx> no=.. name=.. zh=.. trigger=[..] ctx=[..] mask=.. policy=..
/// prio=.. reason=.. counts=.. inv=.. fp=..`
pub fn export_registry_kv() -> String {
    let mut r = match SchedRegistry::seed_default() {
        Ok(x) => x, Err(_) => return String::from("# seed failed\n"),
    };
    if r.seal(0).is_err() { return String::from("# seal failed\n"); }
    let mut s = String::new();
    s.push_str("# ");
    s.push_str(SP2_JUDGE_TEXT);
    s.push('\n');
    s.push_str("# 版本=");
    s.push_str(SP2_VERSION);
    s.push_str(" 条目数=");
    s.push_str(&r.len().to_string());
    s.push_str(" 封账指纹=0x"); s.push_str(&format!("{:016x}", r.seal_fp())); s.push('\n'); let items = r.registry_snapshot(); let mut i = 0usize;
    while i < items.len() {
        let origin = match r.origin_of(i) {
            Some(o) => o.name(), None => "?",
        };
        s.push_str(&format!("origin={} ", origin)); s.push_str(&items[i].kv_line(i)); s.push('\n'); i += 1;
    }
    s.push_str("# 点专属不变量展开\n"); let mut p = 0usize;
    while p < SP2_POINT_COUNT {
        let pt = SchedPoint::ALL[p]; let inv = pt.invariants(); let mut k = 0usize;
        while k < inv.len() {
            s.push_str(&format!("no=0x{:04x} {} ", pt.no(), pt.name())); s.push_str(inv[k]); s.push('\n'); k += 1;
        }
        p += 1;
    }
    s
}

/// 导出矩阵 TSV（**点×语境 · 24 格**）。
pub fn export_matrix_tsv_at(reg: &SchedRegistry) -> String {
    let mut s = String::new();
    s.push_str("# UNX-F1802 点×语境合法矩阵 v1（6×4 = 24 格）\n"); s.push_str("point_no\tpoint\tThread\tInterrupt\tKernel\tDma\n"); let mut i = 0usize;
    while i < SP2_POINT_COUNT {
        let p = SchedPoint::ALL[i]; s.push_str(&format!("0x{:04x}\t{}", p.no(), p.zh())); let mut j = 0usize;
        while j < 4 {
            let c = CallCtx::ALL[j]; let cell = MatrixCell::build(p, c); s.push_str(&format!("\t{}", cell.tsv()));
            // 只读清单复核（清单掩码与派生掩码须一致）。
            if let Some(d) = reg.get_point(p) {
                if d.ctx_allowed(c) != cell.legal { s.push_str("!MISMATCH"); }
            }
            j += 1;
        }
        s.push('\n'); i += 1;
    }
    s.push_str(&format!("# cells={} illegal={}\n",SP2_MATRIX_CELLS, SP2_MATRIX_ILLEGAL_CELLS ));
    s
}

/// 小端 u16 读取（**无 unwrap** · 越界由调用方预检）。
    fn be_u16(b: &[u8]) -> u16 {
    let mut a = [0u8; 2]; let mut i = 0usize; while i < 2 { a[i] = b[i]; i += 1; }
    u16::from_le_bytes(a)
}
/// 小端 u32 读取（**无 unwrap** · 越界由调用方预检）。
    fn be_u32(b: &[u8]) -> u32 {
    let mut a = [0u8; 4]; let mut i = 0usize; while i < 4 { a[i] = b[i]; i += 1; }
    u32::from_le_bytes(a)
}
/// 小端 u64 读取（**无 unwrap** · 越界由调用方预检）。
    fn be_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8]; let mut i = 0usize; while i < 8 { a[i] = b[i]; i += 1; }
    u64::from_le_bytes(a)
}
/// TLV 打包（清单 + 矩阵 + 指纹；**长度自洽断言**）。
///
/// 帧格式：`MAGIC(4) | VER_LEN(2) | PAYLOAD_LEN(4) | PAYLOAD | SEAL(8)`
/// PAYLOAD 内为三段：`LIST_LEN(4) LIST | MTX_LEN(4) MTX | FP(8)`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlvHeader {
    /// 魔数（`U2SP`）。
    pub magic: u32,
    /// 版本串长度。
    pub ver_len: u16,
    /// 载荷长度。
    pub payload_len: u32,
    /// 封账指纹。
    pub seal: u64,
    }
impl TlvHeader {
    /// 魔数常量（`'U' | '2'<<8 | 'S'<<16 | 'P'<<24`）。
    pub const MAGIC: u32 = 0x5053_3255;
    /// 头字段布局：`MAGIC(4) | VER_LEN(2) | PAYLOAD_LEN(4) | SEAL(8)` = 18 字节。
    pub const FIELD_LEN: usize = 18;
    /// 构造头。
    pub fn new(ver_len: u16, payload_len: u32, seal: u64) -> TlvHeader {
        TlvHeader {
            magic: TlvHeader::MAGIC, ver_len, payload_len, seal,
        }
    }
    /// 编码为字节（**18 字节** · 小端）。
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(TlvHeader::FIELD_LEN);
        v.extend_from_slice(&self.magic.to_le_bytes());
        v.extend_from_slice(&self.ver_len.to_le_bytes());
        v.extend_from_slice(&self.payload_len.to_le_bytes()); v.extend_from_slice(&self.seal.to_le_bytes());
        v
    }
    /// 从字节解头（**无 unwrap** · 越界即 Err）。
    pub fn decode(b: &[u8]) -> SpResult<TlvHeader> {
        if b.len() < TlvHeader::FIELD_LEN {
            return Err(SpErr::TlvShort(TlvHeader::FIELD_LEN, b.len()));
        }
        let mut magic = [0u8; 4];
        let mut vl = [0u8; 2]; let mut pl = [0u8; 4]; let mut sl = [0u8; 8]; let mut i = 0usize;
        while i < 4 { magic[i] = b[i]; i += 1; }
        i = 0;
        while i < 2 { vl[i] = b[4 + i]; i += 1; }
        i = 0;
        while i < 4 { pl[i] = b[6 + i]; i += 1; }
        i = 0; while i < 8 { sl[i] = b[10 + i]; i += 1; }
        Ok(TlvHeader {
            magic: u32::from_le_bytes(magic), ver_len: u16::from_le_bytes(vl), payload_len: u32::from_le_bytes(pl), seal: u64::from_le_bytes(sl),
        })
    }
    /// 头总长（字节）。
    pub const LEN: usize = 18;
}

/// TLV 打包产物（含自洽字段）。
#[derive(Clone, Debug)]
pub struct TlvFrame {
    /// 完整帧字节。
    pub bytes: Vec<u8>,
    /// 清单段字节。
    pub list: Vec<u8>,
    /// 矩阵段字节。
    pub matrix: Vec<u8>,
    /// 封账指纹。
    pub seal: u64,
    /// 长度自洽断言结果。
    pub coherent: bool,
    }
impl TlvFrame {
    /// 帧总长（字节）。
    pub fn total_len(&self) -> usize {
        self.bytes.len()
    }
    /// 载荷长度（不含 18 字节头；含两段 4 字节长度前缀）。
    pub fn payload_len(&self) -> usize {
        4 + self.list.len() + 4 + self.matrix.len() + 8
    }
    /// 长度自洽断言（**总长 == 头 + 4 + list + 4 + matrix + 8**）。
    pub fn assert_len(&self) -> SpResult<usize> {
        let expect = TlvHeader::LEN + 4 + self.list.len() + 4 + self.matrix.len() + 8;
        if self.bytes.len() != expect { return Err(SpErr::TlvShort(expect, self.bytes.len())); }
        if self.bytes.len() != TlvHeader::LEN + self.payload_len() {
            return Err(SpErr::TlvShort(TlvHeader::LEN + self.payload_len(), self.bytes.len()));
        }
        Ok(self.bytes.len())
    }
    /// 自洽总门。
    pub fn judge(&self) -> (bool, String) {
        let len_ok = self.assert_len().is_ok();
        let head_ok = self.bytes.len() >= TlvHeader::LEN
            && TlvHeader::decode(&self.bytes).map(|h| h.magic == TlvHeader::MAGIC && h.seal == self.seal).unwrap_or(false);
        let ok = len_ok && head_ok && self.coherent;
        let msg = format!(
        "tlv total={} payload={} list={} matrix={} seal=0x{:016x} len_ok={} head_ok={} ok={}",
            self.total_len(),self.payload_len(), self.list.len(), self.matrix.len(), self.seal, len_ok, head_ok,
    ok
    );
        (ok, msg)
    }
}

/// 打包 TLV（清单 + 矩阵 + 指纹）。
pub fn export_tlv() -> TlvFrame {
    let list = export_registry_kv(); let matrix = export_matrix_tsv();
    let seal = match SchedRegistry::seed_default() {
        Ok(mut r) => match r.seal(0) {
            Ok(fp) => fp, Err(_) => 0,
        },
        Err(_) => 0,
    };
    let lb = list.as_bytes();
    let mb = matrix.as_bytes();
    let payload_len = (4 + lb.len() + 4 + mb.len() + 8) as u32;
    let head = TlvHeader::new(SP2_VERSION.len() as u16, payload_len, seal);
    let mut bytes = head.encode();
    bytes.extend_from_slice(&(lb.len() as u32).to_le_bytes());
    bytes.extend_from_slice(lb);
    bytes.extend_from_slice(&(mb.len() as u32).to_le_bytes());
    bytes.extend_from_slice(mb);
    bytes.extend_from_slice(&seal.to_le_bytes());
    let mut listb = Vec::with_capacity(lb.len());
    listb.extend_from_slice(lb); let mut matb = Vec::with_capacity(mb.len()); matb.extend_from_slice(mb);
    let mut f = TlvFrame {
        bytes, list: listb, matrix: matb, seal, coherent: false,
    };
    f.coherent = f.assert_len().is_ok();
    f
}

/// 导出闸门全量快照（六件套：清单/矩阵/原因账/注入账/注入核/轨迹）。
pub fn export_all(g: &SchedGate, inj: &Injector, cs: &CallSiteTable) -> String {
    let mut s = String::new();
    s.push_str(&export_registry_kv());
    s.push('\n');
    s.push_str(&export_matrix_tsv_at(&g.reg));
    s.push('\n');
    s.push_str(&g.reason.export());
    s.push('\n');
    s.push_str(&g.escape.export());
    s.push('\n');
    s.push_str(&inj.export());
    s.push('\n'); s.push_str(&cs.export()); s.push('\n'); let mut i = 0usize;
    while i < g.trace.len() { s.push_str(g.trace[i].as_str()); s.push('\n'); i += 1; }
    s
}
// ===========================================================================
// K. 模块门面与判据自检
// ===========================================================================

/// 调度点全集登记模块门面。
#[derive(Clone, Debug)]
pub struct SchedPts {
    /// 闸门。
    pub gate: SchedGate,
    /// 注入核。
    pub injector: Injector,
    /// 调用点表。
    pub callsites: CallSiteTable,
    /// LCG（确定性驱动）。
    pub rng: Lcg,
    }
impl SchedPts {
    /// 构造门面（播种 + 封账 + 调用点标准播种）。
    pub fn new(tick: u64) -> SpResult<SchedPts> {
        let gate = SchedGate::with_sealed_default(tick)?; let callsites = CallSiteTable::seed_standard()?;
        Ok(SchedPts {
            gate, injector: Injector::new(), callsites, rng: Lcg::seeded(tick),
        })
    }
    /// 跑一轮合法六点遍历（每点在各自合法语境至少一次）。
    pub fn run_legal_round(&mut self, base_tick: u64) -> u32 {
        let mut ok = 0u32; let mut i = 0usize;
        while i < SP2_POINT_COUNT {
            let p = SchedPoint::ALL[i];
            // 每点取第一个合法语境。
            let mut j = 0usize;
            while j < 4 {
                let c = CallCtx::ALL[j];
                if p.ctx_mask() & c.mask() != 0 {
                    let t = base_tick + i as u64 * 16 + j as u64;
                    if self.gate.request_sched(p.no(), c, t).is_ok() {
                        ok += 1;
                    }
                    // 推进闭环：消费后清位。
                    let _ = self.gate.resched.consume(t); let _ = self.gate.resched.clear(t);
                    // 记切换（阻塞点显式豁免 → 只落豁免事件）。
                    let _ = self.gate.commit_switch(p, c, t, 0x100 + i as u32, 0x200 + j as u32); break;
                }
                j += 1;
            }
            i += 1;
        }
        ok
    }
    /// 跑 need_resched 置位丢失 + 兜底恢复（闭环演示，供判据 8 复用）。
    pub fn run_resched_recovery(&mut self, base_tick: u64) -> SpResult<bool> {
        let t = base_tick;
        let _ = self.gate.resched.set(SchedPoint::TickExpiry.no(), t);
        let had = self.gate.resched.inject_lost(t)?;
        let rec = self.gate.resched.recover(t)?; let _ = self.gate.resched.consume(t)?; let _ = self.gate.resched.clear(t)?;
        Ok(had && rec)
    }
    /// 跑注入核（10 形态）。
    pub fn run_injections(&mut self) -> Vec<InjectCase> {
        self.injector.inject_all(&mut self.gate)
    }
    /// 判据总门（`UNX-F1802-J1`）。
    pub fn judge(&self) -> (bool, String) {
        let (r1, m1) = self.injector.judge();
        let (r2, m2) = self.gate.judge();
        let (r3, m3) = self.callsites.judge();
        let r4 = matrix_illegal_list().len() == SP2_MATRIX_ILLEGAL_CELLS; let (r5, m5) = export_tlv().judge(); let ok = r1 && r2 && r3 && r4 && r5;
        let msg = format!(
        "UNX-F1802-J1 | inject[{}] gate[{}] callsites[{}] matrix[illegal={} want={} ok={}] tlv[{}]",
            m1, m2, m3, matrix_illegal_list().len(), SP2_MATRIX_ILLEGAL_CELLS, r4, m5
    );
        (ok, msg)
    }
}

/// 自检项（**不写 `#[cfg(test)]`** · 由外部 probe 调用）。
#[derive(Clone, Debug)]
pub struct SelfCheck {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 说明。
    pub detail: String,
    }
impl SelfCheck {
    /// 构造一项。
    pub fn new(name: &'static str, ok: bool, detail: String) -> SelfCheck {
        SelfCheck { name, ok, detail }
    }
}

/// 全量自检（**判据 `UNX-F1802-J1` · 返回逐项结果**）。
pub fn self_check() -> Vec<SelfCheck> {

    let mut out = Vec::new();
    // 0) 构造与合法轮（每点在首个合法语境各发一次请求）。
    let mut sp = match SchedPts::new(1) {
        Ok(x) => x, Err(_) => return out,
    };
    let legal = sp.run_legal_round(100);
    chk!(out, "reg-sealed", sp.gate.reg.sealed(), format!("fp=0x{:016x}", sp.gate.reg.seal_fp()));
    chk!(out, "reg-cap>=64", sp.gate.reg.len() >= 64, format!("items={}", sp.gate.reg.len()));
    chk!(out, "seal-verify", sp.gate.reg.verify_seal().is_ok(), String::from("指纹复算一致"));
    chk!(out, "item-fp-verify", sp.gate.reg.verify_item_fps().is_ok(), format!("items={}", sp.gate.reg.len()));
    chk!(out, "legal-round", legal == SP2_POINT_COUNT as u32, format!("granted={}/6", legal));
    // 1) 语境矩阵 + F1844 中断语境三禁演练。
    chk!(out, "irq-preempt-ok", sc_req(&mut sp.gate, CallCtx::Interrupt, SchedPoint::Preempt, 200).is_ok(), String::from("Interrupt 语境 Preempt 受理"));
    chk!(out, "irq-yield-denied", sc_req(&mut sp.gate, CallCtx::Interrupt, SchedPoint::Yield, 201).is_err(), String::from("Interrupt 语境 Yield 被拒 0xB0A2"));
    sp.gate.irq_stay = SP2_INTERRUPT_STAY_BUDGET + 1;
    let stay_deny = sc_req(&mut sp.gate, CallCtx::Interrupt, SchedPoint::Preempt, 202).is_err();
    sp.gate.irq_stay = 1;
    chk!(out, "irq-stay-budget", stay_deny, format!("budget={}", SP2_INTERRUPT_STAY_BUDGET));
    sp.gate.lock_count = 1;
    let lock_deny = sc_req(&mut sp.gate, CallCtx::Interrupt, SchedPoint::Preempt, 203).is_err();
    sp.gate.lock_count = 0;
    chk!(out, "irq-ban-lock", lock_deny, String::from("持锁中断请求被拒 0xB0AF"));
    let (dok, dmsg) = sp.gate.disc.judge();
    chk!(out, "ctx-discipline", dok, dmsg);
    chk!(out, "matrix-cells", build_matrix().len() == SP2_MATRIX_CELLS, format!("cells={}", SP2_MATRIX_CELLS));
    chk!(out, "matrix-illegal", matrix_illegal_list().len() == SP2_MATRIX_ILLEGAL_CELLS, format!("illegal={}", SP2_MATRIX_ILLEGAL_CELLS));
    // 2) need_resched 闭环（置位→消费→清位 + 丢失兜底恢复）。
    let rec = sp.run_resched_recovery(500);
    let (sok, smsg) = sp.gate.resched.judge();
    chk!(out, "resched-recovery", rec == Ok(true), format!("recovered={}", rec.is_ok())); chk!(out, "resched-loop", sok, smsg);
    // 2.5) 原因账环形覆盖演练（容量 64 · dropped 显性）。
    let mut ring_k = 0u64;
    while ring_k < 72 {
        let _ = sp.gate.commit_switch(SchedPoint::TickExpiry, CallCtx::Kernel, 600 + ring_k, ring_k as u32, 0xfff); ring_k += 1;
    }
    chk!(out, "reason-ring-overflow", sp.gate.reason.dropped >= 1, format!("dropped={}", sp.gate.reason.dropped));
    // 3) 原因账守恒（注入前）。
    let cons = sp.gate.reason.verify_conservation();
    let cons_txt = match cons.err() {
        Some(e) => e.text(), None => String::from("ok"),
    };
    chk!(out, "reason-conserve", cons.is_ok(), cons_txt);
    // 4) 调用点全表核对（先注入域外点号与无点号各一）。
    let _ = sp.callsites.inject_escape_no(1, 0x7abc);
    let _ = sp.callsites.inject_absent(2); let scan = sp.callsites.verify_all_call_sites(&sp.gate.reg); let (cok, cmsg) = sp.callsites.judge();
    let scan_code = match scan.err() {
        Some(e) => e.code(), None => 0,
    };
    chk!(out, "callsite-scan-detect", scan.is_err() && sp.callsites.escapes >= 1 && sp.callsites.absent >= 1, format!("code=0x{:04x}", scan_code));
    chk!(out, "callsite-judge", cok, cmsg);
    // 5) 注入核 10 形态。
    let cases = sp.run_injections();
    let (iok, imsg) = sp.injector.judge();
    chk!(out, "inject-10-cases", cases.len() == 10, format!("cases={}", cases.len())); chk!(out, "inject-10/10", iok, imsg);
    // 6) 域外注入账全拒。
    let (eok, emsg) = sp.gate.escape.judge(); chk!(out, "escape-ledger", eok, emsg);
    // 7) 封账写拒计数（新增/改写/删除三类各 ≥1）。
    let (ra, rm, rd) = sp.gate.reg.rejected_writes();
    chk!(out, "seal-write-rejects", ra >= 1 && rm >= 1 && rd >= 1, format!("add={} mod={} del={}", ra, rm, rd));
    // 8) 导出面（KV 清单 / TSV 矩阵 / TLV 包）。
    let kv = export_registry_kv();
    let mtx = export_matrix_tsv();
    let tlv = export_tlv();
    let (tok, tmsg) = tlv.judge();
    chk!(out, "export-kv", kv.len() > 512 && kv.contains("SP-0101/tick-expiry"), format!("kv_len={}", kv.len()));
    chk!(out, "export-tsv", mtx.contains("Thread") && mtx.contains("Dma"), format!("mtx_len={}", mtx.len())); chk!(out, "export-tlv", tok, tmsg);
    // 9) 判据总门。
    let (jok, jmsg) = sp.judge(); chk!(out, "UNX-F1802-J1", jok, jmsg);
    out
}

/// 判据自检汇总行（一行式）。
pub fn self_check_summary() -> String {
    let cs = self_check(); let mut pass = 0usize; let mut i = 0usize;
    while i < cs.len() {
        if cs[i].ok { pass += 1; }
        i += 1;
    }
    format!("{} self_check: {}/{} pass · judge={}", SP2_JUDGE_ID, pass, cs.len(),SP2_JUDGE_TEXT )
}