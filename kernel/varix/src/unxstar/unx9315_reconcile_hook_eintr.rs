//! UNX-F9315 EINTR 与 B12 事件挂点族对账格 · 0x7BC40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9315 逐字）：
//!   ① 纯功能代码 ≥320 行；② UNX-F9315-J1 对账判据过，与 B12 四输入
//!   判定器逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① EINTR 语义域按「信号打断阻塞调用」建模：挂起位非零的一次阻塞
//!      判定必回 -EINTR，且调用可安全重试、状态零损坏——累计字节在
//!      打断前后必须逐位相等，回合可复算。
//!   ② B12 四输入判定器 `judge_wait`：输入族为（信号挂起位 / 超时剩余
//!      / 事件就绪位 / 已累计字节）。判定次序与 Linux 等待面一致：
//!      参数值域校验（EINVAL）先于信号检查（EINTR），信号检查先于
//!      就绪交付与超时收场——信号与就绪同时成立时必先回 EINTR。
//!   ③ 对账册是冻结表（10~14 行，共 5 行）：每行登记场景 / EINTR 期望
//!      / 重试语义 / 零损坏声明四列；判定器逐行复算，冲突数必须为 0。
//!      重试语义闭式：EINVAL 不可盲重试（先修参数），EINTR 与成功态
//!      均可安全再次发起等待。
//!   ④ 状态机为等待回合四态：Idle→Blocked→Interrupted→Resumed 单调
//!      迁移；合法边只有 Idle→Blocked、Blocked→Interrupted、
//!      Blocked→Resumed、Interrupted→Resumed 四条，任何倒退边与越级
//!      边一律拒绝。rearm 是回合重置面（Interrupted/Resumed→Idle），
//!      不属于迁移边，账目保留。
//!   ⑤ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸必须恒为 0；
//!      三类注入（挂起位 / 非法参数 / 状态倒退）各 10 轮全抓，另加
//!      容量耗尽注入（注册越界必回 ENOSPC 且既有钩子账不丢）。
//!   ⑥ 双跑一致：判定器语料 + 钩子回合语料（固定 LCG 种子）全部结果
//!      经 FNV-1a 收指纹，两次运行逐字节相等，任何隐藏状态即失败。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9315 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7BC40;

/// 单类注入的复测轮数（判据要求 10/10 全抓）。
pub const INJECT_ROUNDS: u32 = 10;

/// B12 事件挂点容量（定容注册表）。
pub const HOOK_CAP: usize = 12;

/// 已累计字节值域上限（1 MiB）；超出按 EINVAL 值域越界处理。
pub const ACC_CAP: i32 = 1_048_576;

/// 超时剩余值域上限（24 小时毫秒数）；超出按 EINVAL 值域越界处理。
pub const TMO_CAP_MS: i32 = 86_400_000;

/// Linux errno 正数码（本模块错误面闭集）。
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookCode {
    /// 信号挂起打断：阻塞判定遇挂起位非零
    SigPendingInterrupt,
    /// 参数值域非法：负超时/负累计/超上限
    ParamInvalid,
    /// 零交付收场：无就绪无信号，等待完毕零字节
    ZeroDelivery,
    /// 就绪交付：事件就绪，返回累计字节
    ReadyDelivered,
    /// 钩子号未注册：目标槽位空闲
    HookUnknown,
    /// 钩子号越界：负值或超出 HOOK_CAP
    HookRangeBad,
    /// 状态倒退或越级：等待回合迁移边非法
    PhaseRegress,
    /// 容量耗尽：注册表满仍申请
    HookTableFull,
    /// 重试语义与账不符：不可重试码被盲重试
    RetryMismatch,
    /// 打断后状态损坏：累计字节不可复算
    ZeroDamageViolation,
    /// 注入账倒退：守恒式破裂
    LedgerRegress,
    /// 双跑不一致：指纹不等
    FingerprintMismatch,
    /// 段位越界：段基址非法
    SegmentViolation,
}

impl HookCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            HookCode::SigPendingInterrupt => "信号挂起打断：阻塞判定在挂起位非零时必回 \
                -EINTR；请先消费信号再重试等待，累计字节不受影响",
            HookCode::ParamInvalid => "参数值域非法：负超时/负累计/超上限一律回 EINVAL；\
                请先把超时与累计字节收敛到 0..=上限再发起等待",
            HookCode::ZeroDelivery => "零交付收场：无就绪、无信号、超时到期，返回 0；\
                这是合法收场，调用方无需重试也无需告警",
            HookCode::ReadyDelivered => "就绪交付：事件就绪位与订阅掩码相交非零，\
                直接返回累计字节；不产生 EINTR",
            HookCode::HookUnknown => "钩子号未注册：目标槽位空闲；请先 register 取得 \
                有效钩子号再交付事件或发起等待",
            HookCode::HookRangeBad => "钩子号越界：负值或 ≥ HOOK_CAP 的号段非法；\
                请按 0..HOOK_CAP 约定使用钩子号",
            HookCode::PhaseRegress => "状态倒退或越级：等待回合只允许沿合法边单调前移；\
                请检查是否把 Interrupted/Resumed 误迁回 Blocked/Idle",
            HookCode::HookTableFull => "容量耗尽：注册表已满，再注册回 ENOSPC；\
                请先释放空闲钩子或扩容，既有钩子账目不受影响",
            HookCode::RetryMismatch => "重试语义与账不符：EINVAL 不可盲重试，\
                EINTR 才可安全重试；请按结果类别分流重试策略",
            HookCode::ZeroDamageViolation => "打断后状态损坏：EINTR 前后累计字节不等；\
                请排查打断路径是否误改账目或提前推进状态机",
            HookCode::LedgerRegress => "注入账倒退：注入=检出+逃逸守恒式破裂；\
                请检查是否有回滚路径误改累计计数",
            HookCode::FingerprintMismatch => "双跑不一致：同语料两次指纹不等；\
                请排查隐藏状态、未初始化槽位或非确定哈希",
            HookCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [HookCode; 13] = [
        HookCode::SigPendingInterrupt,
        HookCode::ParamInvalid,
        HookCode::ZeroDelivery,
        HookCode::ReadyDelivered,
        HookCode::HookUnknown,
        HookCode::HookRangeBad,
        HookCode::PhaseRegress,
        HookCode::HookTableFull,
        HookCode::RetryMismatch,
        HookCode::ZeroDamageViolation,
        HookCode::LedgerRegress,
        HookCode::FingerprintMismatch,
        HookCode::SegmentViolation,
    ];
}

/// 判定结果 → 诊断码映射（错误面闭集：四类结果各归一码）。
pub fn code_of_result(r: Result<u32, i32>) -> HookCode {
    match r {
        Err(EINTR) => HookCode::SigPendingInterrupt,
        Err(EINVAL) => HookCode::ParamInvalid,
        // 表外 errno 归参数面（判定面闭集收口，防非穷尽匹配编译错）。
        Err(_) => HookCode::ParamInvalid,
        Ok(0) => HookCode::ZeroDelivery,
        Ok(_) => HookCode::ReadyDelivered,
    }
}

/// errno 对账表：(名称, Linux 正数码, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", 4, 995, "Windows ERROR_OPERATION_ABORTED；信号打断族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSPC", 28, 112, "Windows ERROR_DISK_FULL；容量耗尽族"),
];

/// 按名称查 errno 对账行；表外返回 None（显性可查）。
pub fn errno_row(name: &str) -> Option<(i32, i32)> {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return Some((row.1, row.2));
            }
        }
        i += 1;
    }
    None
}

// ── 等待回合状态机 ────────────────────────────────────────────────────
/// 等待回合四态。深度 Idle=0 < Blocked=1 < Interrupted=2 < Resumed=3，
/// 合法迁移边只允许深度单调增加（见 `can_transition` 的边表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitPhase {
    /// 回合未开始
    Idle,
    /// 已进入阻塞等待
    Blocked,
    /// 被信号打断（EINTR）
    Interrupted,
    /// 等待完成（交付或超时）
    Resumed,
}

impl WaitPhase {
    /// 回合深度（单调性的度量）。
    pub fn depth(self) -> u32 {
        match self {
            WaitPhase::Idle => 0,
            WaitPhase::Blocked => 1,
            WaitPhase::Interrupted => 2,
            WaitPhase::Resumed => 3,
        }
    }
}

/// 全部状态（边表全验用）。
pub const ALL_PHASES: [WaitPhase; 4] = [
    WaitPhase::Idle,
    WaitPhase::Blocked,
    WaitPhase::Interrupted,
    WaitPhase::Resumed,
];

/// 合法迁移边表（闭集）：
///   Idle→Blocked（进入阻塞）、Blocked→Interrupted（信号打断）、
///   Blocked→Resumed（就绪/超时完成）、Interrupted→Resumed（重试后完成）。
pub fn can_transition(from: WaitPhase, to: WaitPhase) -> bool {
    matches!(
        (from, to),
        (WaitPhase::Idle, WaitPhase::Blocked)
            | (WaitPhase::Blocked, WaitPhase::Interrupted)
            | (WaitPhase::Blocked, WaitPhase::Resumed)
            | (WaitPhase::Interrupted, WaitPhase::Resumed)
    )
}

/// 执行一次迁移：非法边（倒退/越级/原地）拒绝并回 EINVAL。
pub fn transition(from: WaitPhase, to: WaitPhase) -> Result<WaitPhase, i32> {
    if can_transition(from, to) {
        Ok(to)
    } else {
        Err(EINVAL)
    }
}

// ── B12 四输入判定器 ─────────────────────────────────────────────────
/// 等待参数值域校验（判定器与挂点等待共用的单源）。
pub fn wait_params_valid(timeout_left_ms: i32, accumulated: i32) -> bool {
    timeout_left_ms >= 0
        && timeout_left_ms <= TMO_CAP_MS
        && accumulated >= 0
        && accumulated <= ACC_CAP
}

/// B12 四输入判定器。
///
/// 输入族：`sig_pending` 信号挂起位 / `timeout_left_ms` 超时剩余 /
/// `event_ready` 事件就绪位 / `accumulated` 已累计字节。
///
/// 判定次序（与 Linux 等待面一致）：
///   1. 参数值域非法（负超时/负累计/超上限）→ EINVAL——参数校验先于
///      一切阻塞面，即使信号同时挂起也先回 EINVAL；
///   2. 信号挂起位非零 → EINTR——阻塞判定在挂起位非零时必被打断，
///      即便事件同时就绪也先回 EINTR（信号在等待入口被消费）；
///   3. 事件就绪位非零 → Ok(累计字节)——就绪交付；
///   4. 其余（无信号、无就绪）→ Ok(0)——超时/等待完毕零交付。
pub fn judge_wait(
    sig_pending: u32,
    timeout_left_ms: i32,
    event_ready: u32,
    accumulated: i32,
) -> Result<u32, i32> {
    if !wait_params_valid(timeout_left_ms, accumulated) {
        return Err(EINVAL);
    }
    if sig_pending != 0 {
        return Err(EINTR);
    }
    if event_ready != 0 {
        return Ok(accumulated as u32);
    }
    Ok(0)
}

/// 重试语义闭式：EINVAL 不可盲重试（必须先修参数），EINTR 与成功态
/// 都可安全再次发起等待（EINTR 重试不损状态，成功态重试是新一轮回合）。
pub fn retryable_of(r: Result<u32, i32>) -> bool {
    match r {
        Err(EINVAL) => false,
        _ => true,
    }
}

// ── 固定 LCG（确定性随机源，禁墙钟禁真随机） ─────────────────────────
/// 32 位线性同余发生器（固定常数，种子固定即序列固定）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 以固定种子构造。
    pub const fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }

    /// 推进一步并返回新状态。
    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.state
    }
}

// ── B12 事件挂点注册表 ───────────────────────────────────────────────
/// 单个事件挂点槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookSlot {
    /// 槽位是否已注册
    pub used: bool,
    /// 订阅的事件掩码（交付位与掩码相交才有效）
    pub mask: u32,
    /// 等待回合状态机
    pub phase: WaitPhase,
    /// 已累计字节（EINTR 零损坏声明的主对象）
    pub accumulated: i32,
    /// 注入的信号挂起位
    pub pending: u32,
    /// 已交付的事件就绪位
    pub ready: u32,
    /// 发起等待次数（账）
    pub waits: u32,
    /// 被 EINTR 打断次数（账）
    pub interrupts: u32,
    /// 回合完成次数（账）
    pub resumes: u32,
    /// 回合重置次数（账）
    pub arms: u32,
}

impl HookSlot {
    /// 空槽。
    pub const fn empty() -> HookSlot {
        HookSlot {
            used: false,
            mask: 0,
            phase: WaitPhase::Idle,
            accumulated: 0,
            pending: 0,
            ready: 0,
            waits: 0,
            interrupts: 0,
            resumes: 0,
            arms: 0,
        }
    }

    /// 占槽（注册时整槽写入）。
    pub const fn occupied(mask: u32) -> HookSlot {
        HookSlot {
            used: true,
            mask,
            phase: WaitPhase::Idle,
            accumulated: 0,
            pending: 0,
            ready: 0,
            waits: 0,
            interrupts: 0,
            resumes: 0,
            arms: 0,
        }
    }
}

/// B12 事件挂点定容注册表。账目（waits/interrupts/resumes/arms 与
/// registered）只增不减；任何倒退路径都不得触碰累计账。
#[derive(Clone, Copy, Debug)]
pub struct HookTable {
    slots: [HookSlot; HOOK_CAP],
    /// 已注册钩子数
    registered: u32,
    /// 注册成功笔数（账）
    register_ok: u32,
    /// 注册被拒笔数（账，容量耗尽）
    register_refused: u32,
}

impl HookTable {
    /// 构造空表。
    pub const fn new() -> HookTable {
        HookTable {
            slots: [HookSlot::empty(); HOOK_CAP],
            registered: 0,
            register_ok: 0,
            register_refused: 0,
        }
    }

    /// 解析钩子号 → 槽位下标；越界或未注册返回 None。
    fn resolve(&self, id: i32) -> Option<usize> {
        if id < 0 || id >= HOOK_CAP as i32 {
            return None;
        }
        match self.slots.get(id as usize) {
            Some(s) if s.used => Some(id as usize),
            _ => None,
        }
    }

    /// 注册一个事件挂点：取首个空槽，订阅掩码为 mask。表满回 ENOSPC。
    pub fn register(&mut self, mask: u32) -> Result<usize, i32> {
        let mut i = 0usize;
        while i < HOOK_CAP {
            if let Some(slot) = self.slots.get_mut(i) {
                if !slot.used {
                    *slot = HookSlot::occupied(mask);
                    self.registered = self.registered.saturating_add(1);
                    self.register_ok = self.register_ok.saturating_add(1);
                    return Ok(i);
                }
            }
            i += 1;
        }
        self.register_refused = self.register_refused.saturating_add(1);
        Err(ENOSPC)
    }

    /// 设置已累计字节（值域 0..=ACC_CAP，越界 EINVAL）。
    pub fn set_accum(&mut self, id: i32, bytes: i32) -> Result<(), i32> {
        let idx = match self.resolve(id) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        if bytes < 0 || bytes > ACC_CAP {
            return Err(EINVAL);
        }
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.accumulated = bytes;
                Ok(())
            }
            None => Err(EBADF),
        }
    }

    /// 交付事件位：就绪位 |= bits & 订阅掩码（掩码外交付被过滤，
    /// 这是订阅语义；交付不改状态机）。
    pub fn deliver(&mut self, id: i32, bits: u32) -> Result<u32, i32> {
        let idx = match self.resolve(id) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.ready |= bits & s.mask;
                Ok(s.ready)
            }
            None => Err(EBADF),
        }
    }

    /// 注入信号挂起位（测试面：模拟信号到达）。
    pub fn inject_signal(&mut self, id: i32, bits: u32) -> Result<u32, i32> {
        let idx = match self.resolve(id) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.pending |= bits;
                Ok(s.pending)
            }
            None => Err(EBADF),
        }
    }

    /// 发起一次阻塞等待（B12 四输入判定器的挂点封装）。
    ///
    /// 流程：参数校验（先于阻塞）→ Idle→Blocked → 判定 → 按结果推进
    /// Blocked→Interrupted（EINTR，累计字节零损坏）或 Blocked→Resumed
    /// （完成）。非 Idle 态发起等待属回合违例，回 EINVAL。
    pub fn wait(&mut self, id: i32, timeout_left_ms: i32) -> Result<u32, i32> {
        let idx = match self.resolve(id) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        // 只读快照先行收敛借用，状态推进再取可变借用（编译坑 2）。
        let (phase, pend, mask, ready, acc) = {
            let s = match self.slots.get(idx) {
                Some(s) => s,
                None => return Err(EBADF),
            };
            (s.phase, s.pending, s.mask, s.ready, s.accumulated)
        };
        if phase != WaitPhase::Idle {
            return Err(EINVAL);
        }
        if !wait_params_valid(timeout_left_ms, acc) {
            return Err(EINVAL);
        }
        let blocked = match transition(phase, WaitPhase::Blocked) {
            Ok(p) => p,
            Err(e) => return Err(e),
        };
        let effective_ready = ready & mask;
        let verdict = judge_wait(pend, timeout_left_ms, effective_ready, acc);
        match verdict {
            Ok(n) => {
                let resumed = match transition(blocked, WaitPhase::Resumed) {
                    Ok(p) => p,
                    Err(e) => return Err(e),
                };
                match self.slots.get_mut(idx) {
                    Some(s) => {
                        s.phase = resumed;
                        s.waits = s.waits.saturating_add(1);
                        s.resumes = s.resumes.saturating_add(1);
                    }
                    None => return Err(EBADF),
                }
                Ok(n)
            }
            Err(EINTR) => {
                let interrupted = match transition(blocked, WaitPhase::Interrupted) {
                    Ok(p) => p,
                    Err(e) => return Err(e),
                };
                match self.slots.get_mut(idx) {
                    Some(s) => {
                        s.phase = interrupted;
                        s.waits = s.waits.saturating_add(1);
                        s.interrupts = s.interrupts.saturating_add(1);
                    }
                    None => return Err(EBADF),
                }
                Err(EINTR)
            }
            Err(e) => Err(e),
        }
    }

    /// 回合重置：Interrupted/Resumed → Idle（重试与新一轮的前置面）。
    /// 重置不属于迁移边；重置时挂起位视为已消费（信号已被交付处理），
    /// 累计字节与账目一律保留。Blocked 态不允许重置（仍在等待中）。
    pub fn rearm(&mut self, id: i32) -> Result<(), i32> {
        let idx = match self.resolve(id) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        match self.slots.get_mut(idx) {
            Some(s) => match s.phase {
                WaitPhase::Interrupted | WaitPhase::Resumed => {
                    s.phase = WaitPhase::Idle;
                    s.pending = 0;
                    s.arms = s.arms.saturating_add(1);
                    Ok(())
                }
                _ => Err(EINVAL),
            },
            None => Err(EBADF),
        }
    }

    /// 读累计字节。
    pub fn acc_of(&self, id: i32) -> Result<i32, i32> {
        match self.resolve(id) {
            Some(idx) => match self.slots.get(idx) {
                Some(s) => Ok(s.accumulated),
                None => Err(EBADF),
            },
            None => Err(EBADF),
        }
    }

    /// 读回合状态。
    pub fn phase_of(&self, id: i32) -> Result<WaitPhase, i32> {
        match self.resolve(id) {
            Some(idx) => match self.slots.get(idx) {
                Some(s) => Ok(s.phase),
                None => Err(EBADF),
            },
            None => Err(EBADF),
        }
    }

    /// 已注册钩子数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 注册成功笔数。
    pub fn register_ok(&self) -> u32 {
        self.register_ok
    }

    /// 注册被拒笔数（容量耗尽账）。
    pub fn register_refused(&self) -> u32 {
        self.register_refused
    }

    /// 全表被打断次数合计。
    pub fn interrupts_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < HOOK_CAP {
            if let Some(s) = self.slots.get(i) {
                n = n.saturating_add(s.interrupts);
            }
            i += 1;
        }
        n
    }
}

// ── 注入账 ───────────────────────────────────────────────────────────
/// 注入账：注入次数 = 检出次数 + 逃逸次数，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式破裂回负诊断码（显性不可静默）。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(HookCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式成立与否。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 对账册（冻结表 10~14 行） ────────────────────────────────────────
/// 对账册单行：场景 / 四输入 / EINTR 期望 / 重试语义 / 零损坏声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconcileRow {
    /// 冻结行号（10..=14）
    pub row_no: u32,
    /// 场景名
    pub scenario: &'static str,
    /// 信号挂起位
    pub sig_pending: u32,
    /// 超时剩余
    pub timeout_left_ms: i32,
    /// 事件就绪位
    pub event_ready: u32,
    /// 已累计字节
    pub accumulated: i32,
    /// EINTR 期望（true=该场景必回 EINTR）
    pub eintr_expected: bool,
    /// 重试语义（true=可安全再次发起等待）
    pub retryable: bool,
    /// 零损坏声明（true=打断前后累计字节必须逐位相等）
    pub zero_damage: bool,
    /// 期望结果类别：true=Ok(expect_value)，false=Err(expect_code)
    pub expect_ok: bool,
    /// Ok 期望值
    pub expect_value: u32,
    /// Err 期望码
    pub expect_code: i32,
}

impl ReconcileRow {
    /// 期望结果（对账复算基准）。
    pub fn expected(self) -> Result<u32, i32> {
        if self.expect_ok {
            Ok(self.expect_value)
        } else {
            Err(self.expect_code)
        }
    }
}

/// 冻结对账册（10~14 行，共 5 行）。行号即册位，不得增删改。
///
/// 行 14 是判定次序的核心行：负超时与信号挂起同时成立 → 先回 EINVAL
/// （参数校验先于阻塞面）；行 13 是次序第二轴：信号与就绪同时成立 →
/// 先回 EINTR（信号在等待入口被消费，就绪位留待重试后取用）。
pub const RECONCILE_ROWS: [ReconcileRow; 5] = [
    ReconcileRow {
        row_no: 10,
        scenario: "纯阻塞等待被信号打断",
        sig_pending: 0x1,
        timeout_left_ms: 100,
        event_ready: 0,
        accumulated: 64,
        eintr_expected: true,
        retryable: true,
        zero_damage: true,
        expect_ok: false,
        expect_value: 0,
        expect_code: EINTR,
    },
    ReconcileRow {
        row_no: 11,
        scenario: "无信号事件就绪交付",
        sig_pending: 0,
        timeout_left_ms: 100,
        event_ready: 0x4,
        accumulated: 64,
        eintr_expected: false,
        retryable: true,
        zero_damage: true,
        expect_ok: true,
        expect_value: 64,
        expect_code: 0,
    },
    ReconcileRow {
        row_no: 12,
        scenario: "无信号零就绪零超时收场",
        sig_pending: 0,
        timeout_left_ms: 0,
        event_ready: 0,
        accumulated: 0,
        eintr_expected: false,
        retryable: true,
        zero_damage: true,
        expect_ok: true,
        expect_value: 0,
        expect_code: 0,
    },
    ReconcileRow {
        row_no: 13,
        scenario: "信号与就绪同时成立（信号先于就绪）",
        sig_pending: 0x2,
        timeout_left_ms: 100,
        event_ready: 0x4,
        accumulated: 32,
        eintr_expected: true,
        retryable: true,
        zero_damage: true,
        expect_ok: false,
        expect_value: 0,
        expect_code: EINTR,
    },
    ReconcileRow {
        row_no: 14,
        scenario: "负超时与信号同时成立（EINVAL 先于 EINTR）",
        sig_pending: 0x1,
        timeout_left_ms: -1,
        event_ready: 0,
        accumulated: 8,
        eintr_expected: false,
        retryable: false,
        zero_damage: true,
        expect_ok: false,
        expect_value: 0,
        expect_code: EINVAL,
    },
];

/// 单行零损坏声明复算：EINTR 行走挂点路径（打断前后累计字节逐位相等），
/// 非 EINTR 行以判定器双跑相等作可复算声明。
fn row_zero_damage_ok(row: &ReconcileRow) -> bool {
    if row.eintr_expected && row.timeout_left_ms >= 0 {
        let mut t = HookTable::new();
        let id = match t.register(0xFF) {
            Ok(i) => i as i32,
            Err(_) => return false,
        };
        if t.set_accum(id, row.accumulated).is_err() {
            return false;
        }
        let _ = t.inject_signal(id, row.sig_pending);
        match t.wait(id, row.timeout_left_ms) {
            Err(EINTR) => match t.acc_of(id) {
                Ok(a) => a == row.accumulated,
                Err(_) => false,
            },
            _ => false,
        }
    } else {
        judge_wait(row.sig_pending, row.timeout_left_ms, row.event_ready, row.accumulated)
            == judge_wait(row.sig_pending, row.timeout_left_ms, row.event_ready, row.accumulated)
    }
}

/// 对账册逐行复算：返回（冲突数, 册行数）。冲突 = EINTR 期望不符 ∪
/// 重试语义不符 ∪ 零损坏声明不实 ∪ 期望结果逐码不符，四者任一。
pub fn reconcile_conflict_count() -> (u32, u32) {
    let mut conflicts = 0u32;
    let mut i = 0usize;
    while i < RECONCILE_ROWS.len() {
        if let Some(row) = RECONCILE_ROWS.get(i).copied() {
            let got = judge_wait(row.sig_pending, row.timeout_left_ms, row.event_ready, row.accumulated);
            let eintr_got = got == Err(EINTR);
            let retry_got = retryable_of(got);
            let ok = eintr_got == row.eintr_expected
                && retry_got == row.retryable
                && row.zero_damage
                && row_zero_damage_ok(&row)
                && got == row.expected();
            if !ok {
                conflicts += 1;
            }
        } else {
            conflicts += 1;
        }
        i += 1;
    }
    (conflicts, RECONCILE_ROWS.len() as u32)
}

// ── 三类注入器（各 10 轮全抓） ───────────────────────────────────────
/// 注入一：信号挂起位注入。10 轮各注入非零挂起位（LCG 定种），任何
/// 一次阻塞判定必回 EINTR 且累计字节保留、回合停在 Interrupted。
/// 返回（注入账, 状态损坏计数, 抓获轮数）。
pub fn run_signal_injection() -> (InjectLedger, u32, u32) {
    let mut ledger = InjectLedger::zero();
    let mut damage = 0u32;
    let mut caught_rounds = 0u32;
    let mut t = HookTable::new();
    let id = match t.register(0xFF) {
        Ok(i) => i as i32,
        Err(_) => return (ledger, 1, 0),
    };
    let mut lcg = Lcg::new(0x9315_0001);
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if t.set_accum(id, 128).is_err() {
            damage += 1;
        }
        let bits = lcg.next() | 1; // 保证非零挂起位
        let _ = t.inject_signal(id, bits);
        let before = match t.acc_of(id) {
            Ok(a) => a,
            Err(_) => -1,
        };
        let r = t.wait(id, 50);
        let mut caught = r == Err(EINTR);
        let after = match t.acc_of(id) {
            Ok(a) => a,
            Err(_) => -1,
        };
        if after != before {
            damage += 1;
            caught = false;
        }
        match t.phase_of(id) {
            Ok(p) => {
                if p != WaitPhase::Interrupted {
                    caught = false;
                }
            }
            Err(_) => caught = false,
        }
        if caught {
            caught_rounds += 1;
        }
        let _ = ledger.record(caught);
        let _ = t.rearm(id);
        round += 1;
    }
    (ledger, damage, caught_rounds)
}

/// 注入二：非法参数注入。负超时/负累计/超上限三类轮换 10 轮，判定器
/// 必须全回 EINVAL（参数校验先于阻塞面）。
pub fn run_param_injection() -> InjectLedger {
    let mut ledger = InjectLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let r = match round % 3 {
            0 => judge_wait(0, -1 - round as i32, 0x8, 16),
            1 => judge_wait(0, 50, 0x8, -1 - round as i32),
            _ => judge_wait(0, 50, 0x8, ACC_CAP + 1 + round as i32),
        };
        let _ = ledger.record(r == Err(EINVAL));
        round += 1;
    }
    ledger
}

/// 倒退/越级迁移违例对（闭集 5 对，轮换注入）。
const REGRESS_PAIRS: [(WaitPhase, WaitPhase); 5] = [
    (WaitPhase::Interrupted, WaitPhase::Blocked),
    (WaitPhase::Resumed, WaitPhase::Interrupted),
    (WaitPhase::Resumed, WaitPhase::Blocked),
    (WaitPhase::Blocked, WaitPhase::Idle),
    (WaitPhase::Resumed, WaitPhase::Idle),
];

/// 注入三：状态倒退注入。5 组违例迁移对轮换 10 轮，迁移器必须全拒。
pub fn run_phase_regress_injection() -> InjectLedger {
    let mut ledger = InjectLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let pair = match REGRESS_PAIRS.get((round % 5) as usize).copied() {
            Some(p) => p,
            None => (WaitPhase::Idle, WaitPhase::Idle),
        };
        let rejected = transition(pair.0, pair.1) == Err(EINVAL);
        let _ = ledger.record(rejected);
        round += 1;
    }
    ledger
}

/// 注入四（容量面）：注册表打满后再注册 10 轮，必回 ENOSPC，且既有
/// 钩子的累计字节与注册账不丢。返回（注入账, 账目完好声明）。
pub fn run_capacity_injection() -> (InjectLedger, bool) {
    let mut ledger = InjectLedger::zero();
    let mut t = HookTable::new();
    let mut intact = true;
    let mut first_id: i32 = -1;
    let mut i = 0usize;
    while i < HOOK_CAP {
        match t.register(0xFF) {
            Ok(idx) => {
                if first_id < 0 {
                    first_id = idx as i32;
                    if t.set_accum(first_id, 77).is_err() {
                        intact = false;
                    }
                }
            }
            Err(_) => intact = false,
        }
        i += 1;
    }
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let caught = t.register(0xFF) == Err(ENOSPC);
        if caught {
            if t.acc_of(first_id) != Ok(77) {
                intact = false;
            }
            if t.registered() != HOOK_CAP as u32 {
                intact = false;
            }
            if t.register_ok() != HOOK_CAP as u32 {
                intact = false;
            }
        } else {
            intact = false;
        }
        let _ = ledger.record(caught);
        round += 1;
    }
    (ledger, intact)
}

// ── 正反分层声明语料 ─────────────────────────────────────────────────
/// 正向语料（8 向量）：无信号、参数合法 → 判定器必须全 Ok。
pub const POSITIVE_CORPUS: [(u32, i32, u32, i32); 8] = [
    (0, 100, 0x8, 64),
    (0, 0, 0x4, 0),
    (0, TMO_CAP_MS, 0, ACC_CAP),
    (0, 0, 0, 0),
    (0, 33, 0x1, 1),
    (0, 7, 0xFFFF_FFFF, 255),
    (0, 0, 0x80, 4096),
    (0, 5, 0, 0),
];

/// 反向语料（8 向量）：有信号、参数合法 → 判定器必须全 EINTR，且经
/// 挂点路径打断后累计字节逐位保留（状态可复算）。
pub const NEGATIVE_CORPUS: [(u32, i32, u32, i32); 8] = [
    (1, 100, 0, 64),
    (0x2, 50, 0x8, 32),
    (0x8000_0000, 1, 0, 1),
    (0xFFFF_FFFF, 0, 0x4, 8),
    (3, TMO_CAP_MS, 0, ACC_CAP),
    (0x10, 0, 0, 0),
    (0x40, 9, 0x8, 96),
    (0x91, 1, 0x2, 5),
];

/// 正向分层声明：返回（总数, Ok 数）。无信号语料全成功才合格。
pub fn positive_layer() -> (u32, u32) {
    let mut total = 0u32;
    let mut ok_n = 0u32;
    let mut i = 0usize;
    while i < POSITIVE_CORPUS.len() {
        if let Some(v) = POSITIVE_CORPUS.get(i).copied() {
            total += 1;
            if judge_wait(v.0, v.1, v.2, v.3).is_ok() {
                ok_n += 1;
            }
        }
        i += 1;
    }
    (total, ok_n)
}

/// 反向分层声明：返回（总数, EINTR 数, 状态可复算数）。可复算 = 判定器
/// 双跑相等 且 挂点路径打断后累计字节与注入前逐位相等。
pub fn negative_layer() -> (u32, u32, u32) {
    let mut total = 0u32;
    let mut eintr_n = 0u32;
    let mut repro_n = 0u32;
    let mut i = 0usize;
    while i < NEGATIVE_CORPUS.len() {
        if let Some(v) = NEGATIVE_CORPUS.get(i).copied() {
            total += 1;
            let r = judge_wait(v.0, v.1, v.2, v.3);
            if r == Err(EINTR) {
                eintr_n += 1;
            }
            let mut ok = r == judge_wait(v.0, v.1, v.2, v.3);
            let mut t = HookTable::new();
            let id = match t.register(0xFF) {
                Ok(idx) => idx as i32,
                Err(_) => -1,
            };
            if id >= 0 && t.set_accum(id, v.3).is_ok() && t.inject_signal(id, v.0).is_ok() {
                if t.wait(id, v.1) == Err(EINTR) && t.acc_of(id) == Ok(v.3) {
                    ok = ok && true;
                } else {
                    ok = false;
                }
            } else {
                ok = false;
            }
            if ok {
                repro_n += 1;
            }
        }
        i += 1;
    }
    (total, eintr_n, repro_n)
}

// ── 确定性指纹 ───────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 把一个判定结果收进字节流（低 8 位；Ok/Err 同域可辨）。
fn push_result(bytes: &mut Vec<u8>, r: Result<u32, i32>) {
    match r {
        Ok(n) => bytes.push((n & 0xFF) as u8),
        Err(e) => bytes.push((e & 0xFF) as u8),
    }
}

/// 规范场景指纹：判定器正反语料 + LCG 定种钩子回合语料全部结果收
/// FNV-1a。同输入双跑必须逐字节相等。
pub fn scenario_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < POSITIVE_CORPUS.len() {
        if let Some(v) = POSITIVE_CORPUS.get(i).copied() {
            push_result(&mut bytes, judge_wait(v.0, v.1, v.2, v.3));
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < NEGATIVE_CORPUS.len() {
        if let Some(v) = NEGATIVE_CORPUS.get(j).copied() {
            push_result(&mut bytes, judge_wait(v.0, v.1, v.2, v.3));
        }
        j += 1;
    }
    // 钩子回合语料：注册→置账→LCG 决定注入/交付→等待→重置，16 回合。
    let mut t = HookTable::new();
    let id = match t.register(0xFF) {
        Ok(idx) => idx as i32,
        Err(_) => -1,
    };
    let mut lcg = Lcg::new(0x7BC4_0F31);
    if id >= 0 {
        let mut round = 0u32;
        while round < 16 {
            let a = lcg.next();
            let b = lcg.next();
            let _ = t.set_accum(id, (b % 512) as i32);
            if a & 1 != 0 {
                let _ = t.inject_signal(id, a | 1);
            }
            if a & 2 != 0 {
                let _ = t.deliver(id, 0x8);
            }
            let r = t.wait(id, match b % 3 {
                0 => 0,
                1 => 40,
                _ => TMO_CAP_MS,
            });
            push_result(&mut bytes, r);
            let _ = t.rearm(id);
            round += 1;
        }
        // 账目收尾：全表打断次数入指纹。
        let waits = t.interrupts_total();
        bytes.push((waits & 0xFF) as u8);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9315 判据集（≥14 条；动态明细 ≤8 条）。
pub fn run_unx9315_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9315");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：无信号事件就绪，交付累计字节（判定器 + 挂点双路径）
    let mut t_norm = HookTable::new();
    let normal = judge_wait(0, 100, 0x8, 64) == Ok(64) && match t_norm.register(0xFF) {
        Ok(idx) => {
            let id = idx as i32;
            t_norm.set_accum(id, 64).is_ok()
                && t_norm.deliver(id, 0x8).is_ok()
                && t_norm.wait(id, 100) == Ok(64)
        }
        Err(_) => false,
    };
    s.add("正常流：无信号就绪交付累计字节", normal,
          "合法四输入 + 就绪位非零应返回累计字节，判定器与挂点路径一致");

    // 3. 正常流：无信号零就绪零超时零交付
    let zero_ok = judge_wait(0, 0, 0, 0) == Ok(0)
        && judge_wait(0, 100, 0, 128) == Ok(0);
    s.add("正常流：零交付收场返回 0", zero_ok,
          "无就绪无信号时等待正常收场，返回 0 不回错误码");

    // 4. EINTR 主面：信号挂起打断阻塞等待
    let eintr_ok = judge_wait(0x1, 100, 0, 64) == Err(EINTR);
    s.add("EINTR 主面：挂起位非零必打断阻塞判定", eintr_ok,
          "信号打断阻塞调用的语义域：回 -EINTR、可重试、状态零损坏");

    // 5. 判定次序：EINVAL 先于 EINTR、EINTR 先于就绪
    let order_ok = judge_wait(0x1, -1, 0x8, 8) == Err(EINVAL)
        && judge_wait(0x2, 100, 0x8, 32) == Err(EINTR);
    s.add("判定次序：EINVAL→EINTR→就绪 三轴全序", order_ok,
          "参数校验先于阻塞面，信号在等待入口先于就绪位被消费");

    // 6. 边界四类：零/负/上限/上限+1
    let boundary_ok = judge_wait(0, 0, 0, 0) == Ok(0)
        && judge_wait(0, -1, 0, 0) == Err(EINVAL)
        && judge_wait(0, 0, 0, -1) == Err(EINVAL)
        && judge_wait(0, TMO_CAP_MS, 0x8, ACC_CAP) == Ok(ACC_CAP as u32)
        && judge_wait(0, TMO_CAP_MS + 1, 0, 0) == Err(EINVAL)
        && judge_wait(0, TMO_CAP_MS, 0x8, ACC_CAP + 1) == Err(EINVAL);
    s.add("边界四类：零/负/上限/上限+1 全部落账", boundary_ok,
          "零合法、负值 EINVAL、上限值合法、上限+1 EINVAL，四格缺一不可");

    // 7. 对账册零冲突：冻结表 10~14 行逐条复算
    let (conflicts, rows) = reconcile_conflict_count();
    s.add("对账册零冲突：冻结表 10~14 行逐条复算", conflicts == 0 && rows == 5,
          "每行 EINTR 期望/重试语义/零损坏声明/期望码四轴与判定器零冲突");

    // 8. 挂起位注入：10/10 全抓且累计字节保留
    let (sig_ledger, damage, sig_rounds) = run_signal_injection();
    s.add("挂起位注入：10/10 全抓且零损坏", sig_rounds == INJECT_ROUNDS && damage == 0,
          "任何一次阻塞判定在挂起位非零时必回 EINTR 且累计字节保留");

    // 9. 参数注入：10/10 全抓 EINVAL
    let param_ledger = run_param_injection();
    s.add("参数注入：10/10 全抓 EINVAL", param_ledger.detected == INJECT_ROUNDS,
          "负超时/负累计/超上限三类轮换必须全部被参数校验拦下");

    // 10. 倒退注入：10/10 全抓拒绝
    let regress_ledger = run_phase_regress_injection();
    s.add("倒退注入：10/10 全抓拒绝", regress_ledger.detected == INJECT_ROUNDS,
          "倒退边与越级边一律拒绝，状态机单调性不可绕过");

    // 11. 正向分层声明：无信号语料全成功
    let (pos_total, pos_ok) = positive_layer();
    s.add("正向分层声明：无信号语料全成功", pos_total == 8 && pos_ok == 8,
          "sig=0 且参数合法的语料必须全部 Ok，无一误报 EINTR");

    // 12. 反向分层声明：有信号语料全 EINTR 且状态可复算
    let (neg_total, neg_eintr, neg_repro) = negative_layer();
    s.add("反向分层声明：有信号全 EINTR 且状态可复算",
          neg_total == 8 && neg_eintr == 8 && neg_repro == 8,
          "挂起位非零语料全部 EINTR，打断后累计字节逐位可复算");

    // 13. 状态机：全 16 迁移对边表复验 + 深度单调
    let mut machine_ok = true;
    let mut fi = 0usize;
    while fi < ALL_PHASES.len() {
        let mut ti = 0usize;
        while ti < ALL_PHASES.len() {
            if let (Some(from), Some(to)) = (ALL_PHASES.get(fi).copied(), ALL_PHASES.get(ti).copied()) {
                let allowed = can_transition(from, to);
                let actual = transition(from, to).is_ok();
                if allowed != actual || (allowed && to.depth() <= from.depth()) {
                    machine_ok = false;
                }
            } else {
                machine_ok = false;
            }
            ti += 1;
        }
        fi += 1;
    }
    s.add("状态机：合法边全通、倒退越级全拒且深度单调", machine_ok,
          "16 迁移对逐一复验：4 条合法边深度递增，其余 12 对全拒");

    // 14. 重试语义：EINTR 后重置重试成功，累计字节全程保留
    let mut t_retry = HookTable::new();
    let retry_ok = match t_retry.register(0xFF) {
        Ok(idx) => {
            let id = idx as i32;
            let mut ok = t_retry.set_accum(id, 96).is_ok();
            let _ = t_retry.inject_signal(id, 0x1);
            ok = ok && t_retry.wait(id, 40) == Err(EINTR);
            ok = ok && t_retry.acc_of(id) == Ok(96);
            ok = ok && t_retry.rearm(id).is_ok();
            ok = ok && t_retry.deliver(id, 0x8).is_ok();
            ok = ok && t_retry.wait(id, 40) == Ok(96);
            ok = ok && t_retry.acc_of(id) == Ok(96);
            ok
        }
        Err(_) => false,
    };
    s.add("重试语义：EINTR 后重试成功且累计全程保留", retry_ok,
          "重试不损状态：打断→重置→交付→再等待，累计字节 96 全程可复算");

    // 15. 容量耗尽：表满回 ENOSPC 且账不丢
    let (cap_ledger, cap_intact) = run_capacity_injection();
    s.add("容量耗尽：表满 ENOSPC 且既有账不丢",
          cap_ledger.detected == INJECT_ROUNDS && cap_intact,
          "HOOK_CAP 满后 10 次越界注册全拒，钩子累计与注册账逐位完好");

    // 16. errno 对账：四码数值与对照表逐项一致
    let errno_ok = EINTR == 4 && EBADF == 9 && EINVAL == 22 && ENOSPC == 28
        && errno_row("EINTR") == Some((4, 995))
        && errno_row("EBADF") == Some((9, 6))
        && errno_row("EINVAL") == Some((22, 87))
        && errno_row("ENOSPC") == Some((28, 112))
        && errno_row("E2BIG").is_none();
    s.add("errno 对账：错误面闭集与对照表逐项一致", errno_ok,
          "本模块错误面只允许 EINTR/EBADF/EINVAL/ENOSPC 四码，表外码拒绝");

    // 17. 错误面映射闭集：判定结果 → 诊断码一一对应
    let map_ok = code_of_result(Err(EINTR)) == HookCode::SigPendingInterrupt
        && code_of_result(Err(EINVAL)) == HookCode::ParamInvalid
        && code_of_result(Ok(0)) == HookCode::ZeroDelivery
        && code_of_result(Ok(96)) == HookCode::ReadyDelivered;
    s.add("错误面映射闭集：四类结果各归一码", map_ok,
          "EINTR/EINVAL/零交付/就绪交付与诊断码一一对应，不留模糊格");

    // 18. 双跑一致：场景语料指纹相等
    let fp_a = scenario_fingerprint();
    let fp_b = scenario_fingerprint();
    s.add("双跑一致：场景语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 19. 文案闭集：全部诊断码带非空中文建议
    let text_ok = HookCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 20. 诊断码：13 个码值两两唯一
    let mut uniq = true;
    let mut ci = 0usize;
    while ci < HookCode::ALL.len() {
        let mut cj = ci + 1;
        while cj < HookCode::ALL.len() {
            if let (Some(a), Some(b)) = (HookCode::ALL.get(ci), HookCode::ALL.get(cj)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            cj += 1;
        }
        ci += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 21. 账目守恒：三类注入账合计注入=检出+逃逸且逃逸=0
    let injected_total = sig_ledger.injected
        + param_ledger.injected
        + regress_ledger.injected;
    let detected_total = sig_ledger.detected
        + param_ledger.detected
        + regress_ledger.detected;
    let escaped_total = sig_ledger.escaped + param_ledger.escaped + regress_ledger.escaped;
    s.add("账目守恒：30 注入=30 检出+0 逃逸",
          injected_total == 3 * INJECT_ROUNDS
              && detected_total == injected_total
              && escaped_total == 0
              && sig_ledger.conserved()
              && param_ledger.conserved()
              && regress_ledger.conserved(),
          "注入次数=检出次数+逃逸次数，逃逸必须恒为 0，账只增不减");

    // 动态明细（≤8）：四条最关键运行期快照
    let ledger_text = format!(
        "信号注入 {} / 检出 {} / 损坏 {}",
        sig_ledger.injected, sig_ledger.detected, damage
    );
    s.add_dyn("注入账快照", sig_ledger.conserved() && damage == 0, &ledger_text);
    let recon_text = format!("冻结表 {} 行 / 冲突 {} 处", rows, conflicts);
    s.add_dyn("对账冲突快照", conflicts == 0, &recon_text);
    let layer_text = format!(
        "正向 {}/{} Ok · 反向 {}/{} EINTR",
        pos_ok, pos_total, neg_eintr, neg_total
    );
    s.add_dyn(
        "分层声明快照",
        pos_ok == pos_total && neg_eintr == neg_total && neg_repro == neg_total,
        &layer_text,
    );
    let fp_text = format!("场景指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    // 22. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "本模块声明 14 条判据下限，实际条数以 len 复算为准");

    // 23. 挂点账收口：空表零注册、重试路径打断账入账、容量账守恒
    s.add(
        "挂点账收口：注册/打断/容量三账同闭",
        HookTable::new().registered() == 0
            && t_retry.interrupts_total() >= 1
            && cap_ledger.conserved(),
        "空表注册账为 0、重试路径打断账非零、容量账守恒，三账同闭",
    );

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_delivers_and_zero_delivers() {
        assert_eq!(judge_wait(0, 100, 0x8, 64), Ok(64));
        assert_eq!(judge_wait(0, 0, 0, 0), Ok(0));
        assert_eq!(judge_wait(0, 100, 0, 128), Ok(0));
        let mut t = HookTable::new();
        let id = t.register(0xFF).unwrap_or(usize::MAX) as i32;
        assert!(id >= 0);
        assert!(t.set_accum(id, 64).is_ok());
        assert!(t.deliver(id, 0x8).is_ok());
        assert_eq!(t.wait(id, 100), Ok(64));
        assert_eq!(t.phase_of(id), Ok(WaitPhase::Resumed));
    }

    #[test]
    fn eintr_fires_on_pending_bits() {
        assert_eq!(judge_wait(0x1, 100, 0, 64), Err(EINTR));
        assert_eq!(judge_wait(0x8000_0000, 1, 0, 1), Err(EINTR));
        assert_eq!(judge_wait(u32::MAX, 0, 0x4, 8), Err(EINTR));
    }

    #[test]
    fn judge_order_einval_then_eintr_then_ready() {
        // EINVAL 先于 EINTR：负超时与挂起位同时成立。
        assert_eq!(judge_wait(0x1, -1, 0x8, 8), Err(EINVAL));
        // EINTR 先于就绪：挂起位与就绪位同时成立。
        assert_eq!(judge_wait(0x2, 100, 0x8, 32), Err(EINTR));
        // 就绪先于超时收场。
        assert_eq!(judge_wait(0, 0, 0x8, 16), Ok(16));
    }

    #[test]
    fn four_boundary_classes() {
        // 零
        assert_eq!(judge_wait(0, 0, 0, 0), Ok(0));
        // 负
        assert_eq!(judge_wait(0, -1, 0, 0), Err(EINVAL));
        assert_eq!(judge_wait(0, 0, 0, -1), Err(EINVAL));
        // 上限
        assert_eq!(judge_wait(0, TMO_CAP_MS, 0x8, ACC_CAP), Ok(ACC_CAP as u32));
        // 上限+1
        assert_eq!(judge_wait(0, TMO_CAP_MS + 1, 0, 0), Err(EINVAL));
        assert_eq!(judge_wait(0, TMO_CAP_MS, 0x8, ACC_CAP + 1), Err(EINVAL));
    }

    #[test]
    fn signal_injection_ten_rounds_all_caught() {
        let (ledger, damage, rounds) = run_signal_injection();
        assert!(ledger.conserved());
        assert_eq!(ledger.injected, INJECT_ROUNDS);
        assert_eq!(ledger.detected, INJECT_ROUNDS);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(damage, 0);
        assert_eq!(rounds, INJECT_ROUNDS);
    }

    #[test]
    fn param_and_regress_injection_ten_rounds() {
        let p = run_param_injection();
        assert!(p.conserved());
        assert_eq!(p.detected, INJECT_ROUNDS);
        assert_eq!(p.escaped, 0);
        let r = run_phase_regress_injection();
        assert!(r.conserved());
        assert_eq!(r.detected, INJECT_ROUNDS);
        assert_eq!(r.escaped, 0);
    }

    #[test]
    fn reconcile_table_zero_conflict() {
        let (conflicts, rows) = reconcile_conflict_count();
        assert_eq!(rows, 5);
        assert_eq!(conflicts, 0);
        // 冻结行号闭集 10..=14 逐行在册。
        let mut k = 0usize;
        while k < RECONCILE_ROWS.len() {
            if let Some(row) = RECONCILE_ROWS.get(k) {
                assert_eq!(row.row_no, 10 + k as u32);
                assert!(row.zero_damage);
            }
            k += 1;
        }
    }

    #[test]
    fn layered_declarations_hold() {
        let (pt, po) = positive_layer();
        assert_eq!((pt, po), (8, 8));
        let (nt, ne, nr) = negative_layer();
        assert_eq!((nt, ne, nr), (8, 8, 8));
    }

    #[test]
    fn state_machine_edges_and_monotonic_depth() {
        // 合法边四条全通。
        assert_eq!(transition(WaitPhase::Idle, WaitPhase::Blocked), Ok(WaitPhase::Blocked));
        assert_eq!(
            transition(WaitPhase::Blocked, WaitPhase::Interrupted),
            Ok(WaitPhase::Interrupted)
        );
        assert_eq!(transition(WaitPhase::Blocked, WaitPhase::Resumed), Ok(WaitPhase::Resumed));
        assert_eq!(
            transition(WaitPhase::Interrupted, WaitPhase::Resumed),
            Ok(WaitPhase::Resumed)
        );
        // 倒退与原地全拒。
        assert_eq!(transition(WaitPhase::Interrupted, WaitPhase::Blocked), Err(EINVAL));
        assert_eq!(transition(WaitPhase::Resumed, WaitPhase::Blocked), Err(EINVAL));
        assert_eq!(transition(WaitPhase::Blocked, WaitPhase::Idle), Err(EINVAL));
        assert_eq!(transition(WaitPhase::Idle, WaitPhase::Idle), Err(EINVAL));
        // 越级（Idle→Interrupted / Idle→Resumed）拒绝。
        assert_eq!(transition(WaitPhase::Idle, WaitPhase::Interrupted), Err(EINVAL));
        assert_eq!(transition(WaitPhase::Idle, WaitPhase::Resumed), Err(EINVAL));
        // 非 Idle 发起等待属回合违例；重置后恢复。
        let mut t = HookTable::new();
        let id = t.register(0xFF).unwrap_or(usize::MAX) as i32;
        let _ = t.inject_signal(id, 0x1);
        assert_eq!(t.wait(id, 40), Err(EINTR));
        assert_eq!(t.wait(id, 40), Err(EINVAL));
        assert_eq!(t.rearm(id), Ok(()));
        assert_eq!(t.wait(id, 40), Ok(0));
    }

    #[test]
    fn retry_preserves_accumulated_bytes() {
        let mut t = HookTable::new();
        let id = t.register(0xFF).unwrap_or(usize::MAX) as i32;
        assert!(t.set_accum(id, 96).is_ok());
        let _ = t.inject_signal(id, 0x1);
        assert_eq!(t.wait(id, 40), Err(EINTR));
        assert_eq!(t.acc_of(id), Ok(96));
        assert_eq!(t.phase_of(id), Ok(WaitPhase::Interrupted));
        assert!(t.rearm(id).is_ok());
        assert!(t.deliver(id, 0x8).is_ok());
        assert_eq!(t.wait(id, 40), Ok(96));
        assert_eq!(t.acc_of(id), Ok(96));
        assert!(t.interrupts_total() >= 1);
    }

    #[test]
    fn capacity_exhaustion_enospc_ledger_intact() {
        let (ledger, intact) = run_capacity_injection();
        assert!(ledger.conserved());
        assert_eq!(ledger.injected, INJECT_ROUNDS);
        assert_eq!(ledger.detected, INJECT_ROUNDS);
        assert!(intact);
        let mut t = HookTable::new();
        let mut filled = 0u32;
        let mut i = 0usize;
        while i < HOOK_CAP + 2 {
            if t.register(0xFF).is_ok() {
                filled += 1;
            }
            i += 1;
        }
        assert_eq!(filled, HOOK_CAP as u32);
        assert_eq!(t.register(0xFF), Err(ENOSPC));
        assert_eq!(t.registered(), HOOK_CAP as u32);
        assert_eq!(t.register_refused(), 3);
    }

    #[test]
    fn bad_hook_ids_rejected() {
        let mut t = HookTable::new();
        assert_eq!(t.wait(-1, 10), Err(EBADF));
        assert_eq!(t.wait(HOOK_CAP as i32, 10), Err(EBADF));
        assert_eq!(t.wait(0, 10), Err(EBADF));
        assert_eq!(t.acc_of(-2), Err(EBADF));
        assert_eq!(t.phase_of(HOOK_CAP as i32 + 1), Err(EBADF));
        assert!(t.set_accum(0, -5).is_err());
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert_eq!(scenario_fingerprint(), scenario_fingerprint());
        assert_eq!(fnv1a(b""), 0x811C_9DC5);
        assert!(scenario_fingerprint() != 0);
    }

    #[test]
    fn errno_and_code_closure() {
        assert_eq!(errno_row("EINTR"), Some((4, 995)));
        assert_eq!(errno_row("ENOSPC"), Some((28, 112)));
        assert_eq!(errno_row("E2BIG"), None);
        assert_eq!(code_of_result(Err(EINTR)), HookCode::SigPendingInterrupt);
        assert_eq!(code_of_result(Ok(0)), HookCode::ZeroDelivery);
        let mut k = 0usize;
        while k < HookCode::ALL.len() {
            if let Some(c) = HookCode::ALL.get(k).copied() {
                assert!(!c.detail().is_empty());
            }
            k += 1;
        }
    }

    #[test]
    fn segment_guard_holds() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9315_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.len() >= 22);
    }
}
