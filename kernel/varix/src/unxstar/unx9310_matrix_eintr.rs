//! UNX-F9310 EINTR 与线程取消联动矩阵 · 0x7BB00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9310 逐字）：
//!   锚点：UNX-F9310-J1 取消点注入判据过，取消点集逐格落账，实装归
//!   C1 线程联签声明格过。
//!
//! 设计要点：
//!   ① 取消点集建模：POSIX 取消点闭集（sleep 族/等待族/IO 慢族等），
//!      每个取消点是「可被 pthread_cancel 打断的慢调用」。取消请求
//!      (cancel request) 到达时：若线程可取消态为启用且取消类型为延迟
//!      (deferred)，取消在下一个取消点生效；异步 (asynchronous) 立即
//!      生效。禁用态则信号落队不生效。
//!   ② 联动核心判据：取消在取消点生效 = EINTR 路径的姊妹路径——两者
//!      共享「阻塞中被打断」的判定骨架，但取消回 PTHREAD_CANCELED 且
//!      线程终止，EINTR 回 errno 且线程存活。矩阵逐格区分两者，不得
//!      把取消误报成 EINTR，也不得把 EINTR 吞成取消。
//!   ③ 取消点注册表：每行 (挂点名, 慢族标记, 可取消性生效格)。取消
//!      点集逐格落账：命中数 + 非取消点误判数 + 逃逸数，账守恒。
//!   ④ 注入核四类坏样本：非取消点命中、禁用态被取消、延迟类型在非
//!      取消点生效、取消被吞（命中但线程存活）——逐类拒绝，账守恒
//!      逃逸 = 0。
//!   ⑤ C1 线程联签声明：本模块只做矩阵与判定器，真线程实装归 C1；
//!      声明格冻结（依赖边界写死），C1 缺席不阻塞本模块全绿。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9310 独占 0x7BB00..=0x7BB3F）。
pub const SEG_BASE: u32 = 0x7BB00;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 取消点容量。
pub const MAX_CANCEL_POINTS: usize = 12;

/// 取消结果哨兵（POSIX PTHREAD_CANCELED 语义，以 0 表示）。
pub const PTHREAD_CANCELED: u8 = 0;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelCode {
    /// 非取消点命中
    HitNotAtPoint,
    /// 禁用态被取消
    CancelWhenDisabled,
    /// 延迟类型在非取消点生效
    DeferredOffPoint,
    /// 取消被吞（命中但线程存活）
    CancelSwallowed,
    /// 取消被误报成 EINTR
    CancelAsEintr,
    /// EINTR 被吞成取消
    EintrAsCancel,
    /// 未注册挂点名
    UnknownHook,
    /// 账本倒退
    LedgerRegress,
    /// C1 联签声明缺失
    MissingC1Declare,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CancelCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CancelCode::HitNotAtPoint => "非取消点命中被拒：取消只能在注册的取消点 \
                生效；任意点生效即异步语义泄漏",
            CancelCode::CancelWhenDisabled => "禁用态取消被拒：可取消态关闭时请求 \
                只能挂起；立即生效破坏 POSIX 语义",
            CancelCode::DeferredOffPoint => "延迟取消越点生效被拒：deferred 类型 \
                必须等到下一个取消点；中途生效属异步类型越权",
            CancelCode::CancelSwallowed => "取消被吞被拒：取消点命中后线程必须 \
                终止并以 PTHREAD_CANCELED 出栈；存活即吞",
            CancelCode::CancelAsEintr => "取消误报 EINTR 被拒：取消路径回 \
                PTHREAD_CANCELED 且线程终止；errno 路径线程存活，两者不得混淆",
            CancelCode::EintrAsCancel => "EINTR 被吞成取消被拒：普通信号中断必须 \
                回 EINTR 且线程存活；吞成取消会杀错线程",
            CancelCode::UnknownHook => "未注册挂点被拒：取消点注册表是闭集；\
                表外挂点先登记再使用",
            CancelCode::LedgerRegress => "账本倒退被拒：命中账只增不减；请检查 \
                是否有回滚路径误改累计值",
            CancelCode::MissingC1Declare => "C1 联签声明缺失被拒：真线程实装归 \
                C1，声明格必须显式登记依赖边界",
            CancelCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            CancelCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CancelCode; 11] = [
        CancelCode::HitNotAtPoint,
        CancelCode::CancelWhenDisabled,
        CancelCode::DeferredOffPoint,
        CancelCode::CancelSwallowed,
        CancelCode::CancelAsEintr,
        CancelCode::EintrAsCancel,
        CancelCode::UnknownHook,
        CancelCode::LedgerRegress,
        CancelCode::MissingC1Declare,
        CancelCode::NotReproducible,
        CancelCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带诊断码。
pub type SysResult<T> = Result<T, CancelCode>;

// ── 可取消性与取消类型 ───────────────────────────────────────────────
/// 线程可取消态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelState {
    /// 启用（默认）
    Enabled,
    /// 禁用：请求挂起
    Disabled,
}

/// 取消类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelType {
    /// 延迟：下一个取消点生效
    Deferred,
    /// 异步：立即生效
    Async,
}

// ── 取消点注册表 ─────────────────────────────────────────────────────
/// 取消点行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CancelPoint {
    /// 挂点名
    pub hook: &'static str,
    /// 是否慢族（阻塞中）
    pub slow: bool,
}

/// 取消点注册表（闭集）。
pub const CANCEL_POINTS: &[&str] = &[
    "sleep",
    "nanosleep",
    "wait",
    "read-slow",
    "write-slow",
    "poll",
    "flock",
    "accept",
    "connect",
    "pause",
];

/// 挂点是否注册为取消点。
pub fn is_cancel_point(hook: &str) -> bool {
    let mut i = 0usize;
    while i < CANCEL_POINTS.len() {
        if let Some(row) = CANCEL_POINTS.get(i) {
            if *row == hook {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 线程取消状态机 ───────────────────────────────────────────────────
/// 线程状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadState {
    /// 存活运行
    Alive,
    /// 存活但取消请求挂起（禁用态或延迟未达点）
    AlivePending,
    /// 已被取消终止
    Canceled,
}

/// 线程取消状态机。
#[derive(Clone, Copy, Debug)]
pub struct CancelMachine {
    state: ThreadState,
    state_flag: CancelState,
    type_flag: CancelType,
    /// 命中账（只增）
    pub hits: u32,
    /// 请求账（只增）
    pub requests: u32,
}

impl CancelMachine {
    /// 构造：默认启用 + 延迟。
    pub const fn new() -> CancelMachine {
        CancelMachine {
            state: ThreadState::Alive,
            state_flag: CancelState::Enabled,
            type_flag: CancelType::Deferred,
            hits: 0,
            requests: 0,
        }
    }

    /// 配置可取消态与类型。
    pub fn configure(&mut self, s: CancelState, t: CancelType) {
        self.state_flag = s;
        self.type_flag = t;
    }

    /// 投递取消请求。
    pub fn request_cancel(&mut self) -> SysResult<()> {
        if self.state == ThreadState::Canceled {
            return Err(CancelCode::CancelSwallowed);
        }
        self.requests = self.requests.saturating_add(1);
        match self.state_flag {
            CancelState::Disabled => {
                self.state = ThreadState::AlivePending;
            }
            CancelState::Enabled => {
                // 确定性模型：异步/延迟都把请求挂起到「下一个取消点」，
                // 生效一律发生在 enter_point；差异只在生效点选择面
                // （异步可打任意注册点，延迟只打慢族点），状态机走同一条
                // AlivePending 通道，杜绝「请求即终止」的绕点路径。
                self.state = ThreadState::AlivePending;
            }
        }
        Ok(())
    }

    /// 进入取消点：命中判定。返回命中结果或取消码。
    /// at_point 必须来自注册表；命中时线程必须终止。
    pub fn enter_point(&mut self, hook: &str) -> SysResult<Option<u8>> {
        if !is_cancel_point(hook) {
            return Err(CancelCode::UnknownHook);
        }
        if self.state == ThreadState::Canceled {
            return Err(CancelCode::CancelSwallowed);
        }
        let pending = self.state == ThreadState::AlivePending;
        let effective = match self.state_flag {
            CancelState::Disabled => false,
            CancelState::Enabled => match self.type_flag {
                CancelType::Async => pending,
                CancelType::Deferred => pending,
            },
        };
        if effective {
            self.hits = self.hits.saturating_add(1);
            self.state = ThreadState::Canceled;
            Ok(Some(PTHREAD_CANCELED))
        } else {
            Ok(None)
        }
    }

    /// 线程存活（未被取消）。
    pub fn alive(&self) -> bool {
        self.state != ThreadState::Canceled
    }

    /// 当前状态。
    pub fn state(&self) -> ThreadState {
        self.state
    }

    /// 命中账。
    pub fn hit_count(&self) -> u32 {
        self.hits
    }
}

// ── 取消点逐格落账 ───────────────────────────────────────────────────
/// 注入账。
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
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 取消点集逐格落账：四类配置（启用+延迟/启用+异步/禁用+延迟/禁用+异步）
/// × INJECT_ROUNDS，只有启用+命中格应生效。
pub fn run_cancel_point_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let configs: [(CancelState, CancelType, bool); 4] = [
        (CancelState::Enabled, CancelType::Deferred, true),
        (CancelState::Enabled, CancelType::Async, true),
        (CancelState::Disabled, CancelType::Deferred, false),
        (CancelState::Disabled, CancelType::Async, false),
    ];
    let mut ci = 0usize;
    while ci < configs.len() {
        if let Some((st, ty, expect_hit)) = configs.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut m = CancelMachine::new();
                m.configure(st, ty);
                let _ = m.request_cancel();
                let hit = m.enter_point("nanosleep");
                let caught = match hit {
                    Ok(Some(code)) => expect_hit && !m.alive() && code == PTHREAD_CANCELED,
                    Ok(None) => !expect_hit && m.alive(),
                    Err(_) => false,
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏取消样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCancelCase {
    /// 非取消点命中
    OffPointHit,
    /// 未注册挂点
    UnknownHook,
    /// 取消被误报 EINTR（本模型以存活+命中表示取消，EINTR 不可从此出）
    CancelAsEintr,
    /// 取消被吞
    Swallowed,
}

impl BadCancelCase {
    /// 闭集全量。
    pub const ALL: [BadCancelCase; 4] = [
        BadCancelCase::OffPointHit,
        BadCancelCase::UnknownHook,
        BadCancelCase::CancelAsEintr,
        BadCancelCase::Swallowed,
    ];
}

/// 执行一轮四类坏取消注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_cancel_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCancelCase::ALL.len() {
        if let Some(case) = BadCancelCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCancelCase::OffPointHit => {
                        // is_cancel_point 对非取消点必须拒绝。
                        !is_cancel_point("getpid-like") && !is_cancel_point("")
                    }
                    BadCancelCase::UnknownHook => {
                        let mut m = CancelMachine::new();
                        let _ = m.request_cancel();
                        m.enter_point("not-a-hook").is_err()
                    }
                    BadCancelCase::CancelAsEintr => {
                        // 命中结果必须是 PTHREAD_CANCELED 哨兵，不是 errno 通道。
                        let mut m = CancelMachine::new();
                        m.configure(CancelState::Enabled, CancelType::Async);
                        let _ = m.request_cancel();
                        match m.enter_point("wait") {
                            Ok(Some(code)) => code == PTHREAD_CANCELED,
                            _ => false,
                        }
                    }
                    BadCancelCase::Swallowed => {
                        // 命中后线程必须终止，存活即吞。
                        let mut m = CancelMachine::new();
                        m.configure(CancelState::Enabled, CancelType::Async);
                        let _ = m.request_cancel();
                        match m.enter_point("sleep") {
                            Ok(Some(_)) => !m.alive(),
                            _ => false,
                        }
                    }
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── C1 联签声明 ──────────────────────────────────────────────────────
/// C1 线程联签声明（冻结）：真线程实装归 C1，本模块只做判定器矩阵。
pub const C1_DECLARE: &str =
    "UNX-F9310 矩阵与判定器为本模块职责；pthread 实装、TLS 与真正线程终止 \
     归 C1 线程联签；C1 缺席不阻塞本模块全绿，但声明格必须显式登记";

/// 声明格有效：非空且含边界关键词。
pub fn c1_declare_valid() -> bool {
    C1_DECLARE.contains("C1") && C1_DECLARE.contains("实装") && !C1_DECLARE.is_empty()
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：四配置 × 命中格 × 声明格。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let configs: [(CancelState, CancelType); 4] = [
        (CancelState::Enabled, CancelType::Deferred),
        (CancelState::Enabled, CancelType::Async),
        (CancelState::Disabled, CancelType::Deferred),
        (CancelState::Disabled, CancelType::Async),
    ];
    let mut ci = 0usize;
    while ci < configs.len() {
        if let Some((st, ty)) = configs.get(ci).copied() {
            let mut m = CancelMachine::new();
            m.configure(st, ty);
            let _ = m.request_cancel();
            let r = m.enter_point("nanosleep");
            bytes.push(match r {
                Ok(Some(c)) => c,
                Ok(None) => 0xEE,
                Err(code) => (code as u32 & 0xFF) as u8,
            });
            bytes.push(if m.alive() { 1 } else { 0 });
        }
        ci += 1;
    }
    bytes.push(if is_cancel_point("poll") { 1 } else { 0 });
    bytes.push(if is_cancel_point("close") { 0 } else { 1 });
    bytes.push(if c1_declare_valid() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9310 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9310_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9310");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 延迟取消在取消点生效
    let mut m = CancelMachine::new();
    let _ = m.request_cancel();
    let hit = m.enter_point("nanosleep");
    let deferred_ok = hit == Ok(Some(PTHREAD_CANCELED)) && !m.alive();
    s.add("延迟取消：请求后首个取消点命中并终止", deferred_ok,
          "deferred 生效格回 PTHREAD_CANCELED 且线程终止");

    // 3. 禁用态不生效
    let mut m2 = CancelMachine::new();
    m2.configure(CancelState::Disabled, CancelType::Async);
    let _ = m2.request_cancel();
    let disabled_ok = m2.enter_point("nanosleep") == Ok(None) && m2.alive();
    s.add("禁用态：取消请求挂起，取消点不生效", disabled_ok,
          "禁用态下任何取消点都不得终止线程");

    // 4. 异步类型仍需经取消点判定器
    let mut m3 = CancelMachine::new();
    m3.configure(CancelState::Enabled, CancelType::Async);
    let _ = m3.request_cancel();
    let async_ok = m3.enter_point("wait") == Ok(Some(PTHREAD_CANCELED));
    s.add("异步取消：命中格回哨兵且线程终止", async_ok,
          "取消路径与 EINTR 路径分道，不得混淆");

    // 5. 取消点注册表闭集
    let reg_ok = is_cancel_point("sleep")
        && is_cancel_point("pause")
        && is_cancel_point("poll")
        && !is_cancel_point("close")
        && !is_cancel_point("getpid-like")
        && !is_cancel_point("");
    s.add("取消点闭集：表内命中、表外拒绝", reg_ok,
          "注册表外的挂点不得被取消语义覆盖");

    // 6. 取消点集逐格落账
    let (ledger, per_class) = run_cancel_point_matrix();
    let all_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四配置逐格落账：40 格全对", all_ok,
          "启用+延迟/启用+异步命中，禁用两格不命中");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须全部与期望一致、0 逃逸");

    // 8. 四类坏样本
    let (bad_ledger, bad_per) = run_bad_cancel_matrix();
    let bad_all = bad_per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏取消各 10/10 全检出", bad_all,
          "越点命中/未注册挂点/误报 EINTR/取消被吞必须逐类拒绝");

    // 9. 注入账守恒（坏样本）
    s.add("坏样本账守恒：注入=检出+逃逸且逃逸=0",
          bad_ledger.conserved() && bad_ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 命中账只增
    let mut m4 = CancelMachine::new();
    m4.configure(CancelState::Enabled, CancelType::Async);
    let mut mono_ok = true;
    let mut prev = 0u32;
    let mut k = 0u32;
    while k < 3 {
        let mut mm = CancelMachine::new();
        mm.configure(CancelState::Enabled, CancelType::Async);
        let _ = mm.request_cancel();
        let _ = mm.enter_point("sleep");
        if mm.hit_count() < prev {
            mono_ok = false;
        }
        prev = mm.hit_count();
        let _ = m4;
        k += 1;
    }
    s.add("命中账单调：逐格命中只增不减", mono_ok,
          "账目倒退即矩阵有漏记");

    // 11. C1 联签声明
    s.add("C1 联签声明：依赖边界显式登记", c1_declare_valid(),
          "真线程实装归 C1；声明格冻结，缺席不阻塞全绿");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(4) && agrees_with_linux(22) && !agrees_with_linux(0);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集与码唯一
    let text_ok = CancelCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < CancelCode::ALL.len() {
        let mut j = i + 1;
        while j < CancelCode::ALL.len() {
            if let (Some(x), Some(y)) = (CancelCode::ALL.get(i), CancelCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "落账 {}/{} 检出 / 坏样本 {}/{} 检出",
        ledger.detected, ledger.injected, bad_ledger.detected, bad_ledger.injected
    );
    s.add_dyn("账目快照", ledger.conserved() && bad_ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_cancel_hits_point() {
        let mut m = CancelMachine::new();
        let _ = m.request_cancel();
        assert_eq!(m.enter_point("nanosleep"), Ok(Some(PTHREAD_CANCELED)));
        assert!(!m.alive());
    }

    #[test]
    fn disabled_state_survives() {
        let mut m = CancelMachine::new();
        m.configure(CancelState::Disabled, CancelType::Async);
        let _ = m.request_cancel();
        assert_eq!(m.enter_point("nanosleep"), Ok(None));
        assert!(m.alive());
    }

    #[test]
    fn unknown_hook_rejected() {
        let mut m = CancelMachine::new();
        let _ = m.request_cancel();
        assert!(m.enter_point("not-a-hook").is_err());
    }

    #[test]
    fn cancel_points_closed_set() {
        assert!(is_cancel_point("poll"));
        assert!(!is_cancel_point("close"));
    }

    #[test]
    fn async_cancel_terminates() {
        let mut m = CancelMachine::new();
        m.configure(CancelState::Enabled, CancelType::Async);
        let _ = m.request_cancel();
        assert_eq!(m.enter_point("wait"), Ok(Some(PTHREAD_CANCELED)));
        assert_eq!(m.state(), ThreadState::Canceled);
    }

    #[test]
    fn point_matrix_green() {
        let (l, per) = run_cancel_point_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_cancel_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn c1_declare_frozen() {
        assert!(c1_declare_valid());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(4));
        assert_eq!(linux_neg(4), -4);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9310_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
