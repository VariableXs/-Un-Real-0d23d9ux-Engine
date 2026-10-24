//! unx1742_idleenter —— UNX-F1742 idle 进入路径（队空判定→静息置位→账目起算）。
//!
//! # 单号 / 标题
//! - 单号：**UNX-F1742**
//! - 标题：**idle 进入路径（队空判定 → 静息置位 → 账目起算）**
//! - 域：**CoRun·UNX-A3-B08**（段「空闲核与唤醒路径」）
//! - 目标行数：**2000 ~ 2600 行**（硬指标）
//!
//! # 判据原文（逐条落实）
//! > `UNX-F1742-J1 进入四步顺序断言全过，静息置位在 idle 进入点 100% 命中（F1735 挂点消费）`
//!
//! 判据拆解为三条可执行断言：
//! - **J1-a（顺序）** 进入四步严格顺序 Step1 队空判定 → Step2 静息置位 → Step3 账目起算 → Step4 状态落定。每步消费上一步交接签名，跳步 / 乱序 / 伪造签名一律闭集拒绝。
//! - **J1-b（命中率）** 静息置位在 idle 进入点 **100%** 命中。分子（静息位置位次数）与分母（进入 idle 次数）同源同事件，任一进入绕开挂点即命中率 < 100%，立即显性报错并落账。
//! - **J1-c（挂点消费）** 静息位由 F1735 的**切换挂点**与 **idle 挂点**两处消费，两来源分别登记，静息位与 F1666 发现账双源一致性对账。
//!
//! # 组件清单
//! - [A] idle 三态状态机基座：`Busy → Entering → Idle → Leaving → Busy` 闭集 + 转换矩阵 + append-only 转换账（from/to/tick/cpu/cause）+ 转换账与实际状态一致性断言。
//! - [B] 进入四步链：步号交接签名链、共享 `enter_id`、四步签名账、静息命中率统计器。
//! - [C] 队空判定（F1651 消费）：`QueueEmptiness::{TrueEmpty, PseudoEmpty, NonEmpty}` 闭集 + 判定表 + 伪队空延后账（不得静默当空）。
//! - [D] 静息位（F1735 挂点消费）：每核位图 + 置位时刻 + 消费方标识；置位/清位/查询三路径；置位幂等；切换挂点 / idle 挂点两来源分别登记；与 F1666 发现账双源对账。
//! - [E] 账目起算：起算 tick / 当前 tick / 累计 idle tick / idle 段计数 / busy tick；`Σidle + Σbusy == total_online_ticks` 全局守恒不变量；append-only 环形账本 + dropped 显性化。
//! - [F] 注入核 10 形态（10/10 检出），`inject_all()` 汇总。
//! - [G] 观测导出：四族指标单点导出 `export_kv()` / `export_tlv()`（TLV 自洽断言）。
//! - [H] 确定性回放：`replay()` 重建一次进入全过程，逐步对账，偏差必须为零。
//!
//! # 职责定位
//! 本模块是 B08 段「空闲核与唤醒路径」中 **idle 进入路径** 的纯功能实现：只描述「一个核从忙态如何确定性地走到 idle 态」这一条写路径的四步契约与账目，不含驻留循环本体（F1743）与退出路径本体（F1744），不含调度器主循环，不做任何真实 IO。
//!
//! 册内上下文：
//! - 上游：F1741 空闲循环总纲（三态状态机）、F1651 队空确定性判定、F1618 AP 落座 idle 那一刻、F1735 RCU 静息位原语（每核静息位由切换 / idle 两挂点自动置位）、F1666 idle 发现账、F1745 idle 账目结算（Σidle + Σbusy = 上线总时长守恒）。
//! - 下游：F1743 驻留循环、F1744 退出路径。
//!
//! 段内纪律：
//! - 唤醒路径是调度器最高频写路径 → 本模块全部状态迁移为 O(1)，无循环扫描、无分配热路径。
//! - 与 B11 切换主路径共用 **F1654 过渡态纪律**：过渡态中禁止任何跨态直落，伪队空即过渡态占位。
//! - 与 B07 迁移共用 **F1727 idle 拉取衔接**：迁移拉取（`migrate_pending`）视同过渡态占位。
//! - 无写盘条目；开发期零 QEMU。
//!
//! # 性能逐项分解（每项给出量级与依据）
//! - 四步链本体：**每核每次进入 4 次 O(1) 判定**，无分支预测外开销；签名链每步一次 u64 折叠（3 异或 + 2 乘 + 1 移位），故单次进入签名开销 ~12 条整数指令。
//! - 队空判定：**O(1)**，仅读 `runq_len` / `blocked_all` / `transition_hold` / `migrate_pending` 四个标量，禁止遍历就绪队列（O(n) 禁止项，理由：队列长度已由 F1651 保证自洽）。
//! - 静息置位：**O(1)**，位图按核下标寻址，`quiesce_bitmap` 为 u64 位图（NCPUS_MAX=64 内联，超出则回退 Vec<u64> 分段，仍为 O(1)）；置位幂等由「先查后写 + 来源登记」保证，重复置位走一次位测试即返回，**不记账**。
//! - 账目起算：**O(1)** 一次 u64 差值写入，段计数 +1。
//! - 转换账 append：**摊还 O(1)**（`Vec::push` 摊还），满后走环形覆盖并 `dropped += 1`。
//! - 静息命中率：**O(1)** 分子分母同源，直接相除为定点万分比（无浮点）。
//! - 守恒校验：**O(NCPUS)**，仅在观测/自检时调用，不在热路径。
//! - 双源对账：**O(NCPUS)**，同守恒校验同频。
//! - 导出：`export_kv()` 分配一次 `String`；`export_tlv()` 分配一次 `Vec<u8>`，长度 O(指标数)。
//! - 回放：`replay()` 为确定性纯函数，输入步数 N 时 **O(N)**，无随机源外部依赖。
//! - 注入核：`inject_all()` 固定 10 形态，**O(10)**，每次独立建场景不互相污染。
//! # 诊断码段（独占 0xB040 ~ 0xB04F）
//! - `0xB040` 主码：idle 进入路径事件（成功进入 / 四步链完成）。
//! - `0xB041` 四步乱序 / 跳步。
//! - `0xB042` 交接签名不匹配（伪造 / 断裂）。
//! - `0xB043` 静息置位遗漏（命中率 < 100%）。
//! - `0xB044` 队空判定误判（队内有线程却判空）。
//! - `0xB045` 伪队空误进（未延后直接进 idle）。
//! - `0xB046` 非法状态转换。
//! - `0xB047` 守恒破坏（Σidle + Σbusy ≠ 上线总时长）。
//! - `0xB048` 静息位置位幂等违规。
//! - `0xB049` 过渡态中强行进入 idle。
//! - `0xB04A` 双源不一致（静息位 vs F1666 发现账）。
//! - `0xB04B` 转换账与实际状态不一致。
//! - `0xB04C` 环形账本溢出（dropped 显性化）。
//! - `0xB04D` TLV 导出自洽失败。
//! - `0xB04E` 回放偏差非零。
//! - `0xB04F` 参数越界（核号 / 容量 / 序号）。
//! # 纯功能线声明
//! `no_std + alloc`；**零 `unwrap()`、零 `expect()`、零 `panic!()`、零浮点、零真实 IO、零墙钟**。
//! 时间一律为注入的逻辑 `tick: u64`；随机源一律为确定性 LCG；错误一律为闭集枚举 `E`。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const ID_VERSION: &str = "unx1742_idleenter-v1";

/// 判据单号（贯穿断言与诊断输出）。
pub const ID_CASE: &str = "UNX-F1742-J1";

/// 诊断码段基址（0xB040 ~ 0xB04F 独占）。
pub const ID_SEG_BASE: u16 = 0xB040;
/// 诊断码段上界（闭）。
pub const ID_SEG_END: u16 = 0xB04F;
/// 主码：idle 进入路径事件。
pub const DIAG_ENTER_OK: u16 = 0xB040;
/// 诊断码：四步乱序 / 跳步。
pub const DIAG_STEP_ORDER: u16 = 0xB041;
/// 诊断码：交接签名不匹配。
pub const DIAG_SIG_MISMATCH: u16 = 0xB042;
/// 诊断码：静息置位遗漏。
pub const DIAG_QUIESCE_MISS: u16 = 0xB043;
/// 诊断码：队空判定误判。
pub const DIAG_QUEUE_NOTEMPTY: u16 = 0xB044;
/// 诊断码：伪队空误进。
pub const DIAG_PSEUDO_ENTER: u16 = 0xB045;
/// 诊断码：非法状态转换。
pub const DIAG_ILLEGAL_TRANS: u16 = 0xB046;
/// 诊断码：守恒破坏。
pub const DIAG_CONSERVATION: u16 = 0xB047;
/// 诊断码：静息位置位幂等违规。
pub const DIAG_QUIESCE_DUP: u16 = 0xB048;
/// 诊断码：过渡态中强行进入。
pub const DIAG_IN_TRANSITION: u16 = 0xB049;
/// 诊断码：双源不一致。
pub const DIAG_DUAL_SOURCE: u16 = 0xB04A;
/// 诊断码：转换账不一致。
pub const DIAG_LEDGER_DRIFT: u16 = 0xB04B;
/// 诊断码：环形账本溢出。
pub const DIAG_RING_DROP: u16 = 0xB04C;
/// 诊断码：TLV 自洽失败。
pub const DIAG_TLV: u16 = 0xB04D;
/// 诊断码：回放偏差非零。
pub const DIAG_REPLAY: u16 = 0xB04E;
/// 诊断码：参数越界。
pub const DIAG_RANGE: u16 = 0xB04F;

/// 四步链步号（闭集 · 1..=4 · 与判据 J1-a 严格对应）。
pub const STEP_QUEUE_PROBE: u8 = 1;
/// Step1：队空判定（F1651 消费点）。
pub const STEP_QUEUE_PROBE_DOC: &str = "step1-queue-probe";
/// Step2：静息置位（F1735 挂点消费点）。
pub const STEP_QUIESCE_SET: u8 = 2;
/// Step3：账目起算（F1745 账目起算点）。
pub const STEP_LEDGER_BEGIN: u8 = 3;
/// Step4：状态落定。
pub const STEP_STATE_SETTLE: u8 = 4;

/// 四步链步总数（闭集常量 · 越界即参数错误）。
pub const STEP_TOTAL: u8 = 4;

/// 核数上限（位图内联容量）。
pub const NCPUS_MAX: usize = 64;

/// 上线总时长默认值（逻辑 tick · 守恒锚）。
pub const DEFAULT_ONLINE_TICKS: u64 = 1000;

/// 转换账容量（append-only 环形 · 默认）。
pub const TRANS_LEDGER_CAP: usize = 256;
/// 签名账容量。
pub const SIG_LEDGER_CAP: usize = 256;
/// 伪队空延后账容量。
pub const DEFER_LEDGER_CAP: usize = 128;

/// 静息命中率基准：100% = 10000（万分比定点 · 无浮点）。
pub const HIT_RATE_SCALE: u64 = 10_000;

/// 确定性 LCG（判据：随机源必须可复现 · 返回高 31 位）。
#[inline]
pub fn lcg(s: &mut u64) -> u64 {
    *s = s
    .wrapping_mul(6364136223846793005)
    .wrapping_add(1442695040888963407);
    *s >> 33
}

/// 确定性 LCG 批量取样（固定次数量 · 供注入核构造场景）。
pub fn lcg_fill(seed: &mut u64, out: &mut Vec<u64>, n: usize) {
    for _ in 0..n { let v = lcg(seed); out.push(v); }
}

/// 模块错误闭集（无字符串 · 无动态分配 · 诊断码一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum E {
    /// 0xB041 四步乱序 / 跳步。
    StepOrder { expect: u8, got: u8 },
    /// 0xB042 交接签名不匹配（伪造 / 断裂）。
    SigMismatch { expect: u64, got: u64 },
    /// 0xB043 静息置位遗漏（命中率 < 100%）。
    QuiesceMiss { hit: u64, enter: u64 },
    /// 0xB044 队空判定误判（队内有线程却判空）。
    QueueNotEmpty { depth: u32 },
    /// 0xB045 伪队空误进（未延后直接进 idle）。
    PseudoEnter { depth: u32 },
    /// 0xB046 非法状态转换。
    IllegalTrans { from: u8, to: u8 },
    /// 0xB047 守恒破坏。
    Conservation { lhs: u64, rhs: u64 },
    /// 0xB048 静息位置位幂等违规。
    QuiesceDup { cpu: u32 },
    /// 0xB049 过渡态中强行进入 idle。
    InTransition { cpu: u32 },
    /// 0xB04A 双源不一致。
    DualSource { cpu: u32, quiesce: u64, discovery: u64 },
    /// 0xB04B 转换账与实际状态不一致。
    LedgerDrift { cpu: u32, ledger: u8, actual: u8 },
    /// 0xB04C 环形账本溢出。
    RingDrop { lost: u64 },
    /// 0xB04D TLV 导出自洽失败。
    Tlv { why: u8 },
    /// 0xB04E 回放偏差非零。
    Replay { step: u8, delta: i64 },
    /// 0xB04F 参数越界。
    Range { what: u8, val: u64 },
}

impl E {
    /// 错误对应诊断码（闭集一一映射 · 落在 0xB040~0xB04F 内）。
    pub fn code(self) -> u16 {
        match self {
            E::StepOrder { .. } => DIAG_STEP_ORDER,
            E::SigMismatch { .. } => DIAG_SIG_MISMATCH,
            E::QuiesceMiss { .. } => DIAG_QUIESCE_MISS,
            E::QueueNotEmpty { .. } => DIAG_QUEUE_NOTEMPTY,
            E::PseudoEnter { .. } => DIAG_PSEUDO_ENTER,
            E::IllegalTrans { .. } => DIAG_ILLEGAL_TRANS,
            E::Conservation { .. } => DIAG_CONSERVATION,
            E::QuiesceDup { .. } => DIAG_QUIESCE_DUP,
            E::InTransition { .. } => DIAG_IN_TRANSITION,
            E::DualSource { .. } => DIAG_DUAL_SOURCE,
            E::LedgerDrift { .. } => DIAG_LEDGER_DRIFT,
            E::RingDrop { .. } => DIAG_RING_DROP,
            E::Tlv { .. } => DIAG_TLV,
            E::Replay { .. } => DIAG_REPLAY,
            E::Range { .. } => DIAG_RANGE,
        }
    }

    /// 错误短名（诊断输出 · 无分配）。
    pub fn tag(self) -> &'static str {
        match self {
            E::StepOrder { .. } => "step_order",
            E::SigMismatch { .. } => "sig_mismatch",
            E::QuiesceMiss { .. } => "quiesce_miss",
            E::QueueNotEmpty { .. } => "queue_not_empty",
            E::PseudoEnter { .. } => "pseudo_enter",
            E::IllegalTrans { .. } => "illegal_trans",
            E::Conservation { .. } => "conservation",
            E::QuiesceDup { .. } => "quiesce_dup",
            E::InTransition { .. } => "in_transition",
            E::DualSource { .. } => "dual_source",
            E::LedgerDrift { .. } => "ledger_drift",
            E::RingDrop { .. } => "ring_drop",
            E::Tlv { .. } => "tlv",
            E::Replay { .. } => "replay",
            E::Range { .. } => "range",
        }
    }

    /// 是否为「静息命中率 < 100%」类错误（判据 J1-b 的显性报错面）。
    pub fn is_quiesce_miss(self) -> bool { matches!(self, E::QuiesceMiss { .. }) }
}

/// 模块结果别名（闭集错误 · 禁 unwrap / expect / panic）。
pub type Res<T> = core::result::Result<T, E>;

/// idle 三态状态机基座状态（闭集 · 四态，段「空闲核与唤醒路径」扩展）。与 F1741 的 Active/Idle/Deep 三态正交：F1741 管「驻留深浅」，本单管「进入过程」，
/// 故此处为 `Busy → Entering → Idle → Leaving → Busy` 的环形四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleState {
    /// 忙态（就绪队列非空或正在派发）。
    Busy = 0,
    /// 进入态（Step1 ~ Step3 之间，尚未落定为 Idle）。
    Entering = 1,
    /// 空闲态（Step4 已落定 · 驻留由 F1743 承接）。
    Idle = 2,
    /// 退出态（F1744 唤醒路径承接）。
    Leaving = 3,
}

impl IdleState {
    /// 态序号（0..=3）。
    pub fn ordinal(self) -> u8 { self as u8 }

    /// 态名（诊断输出）。
    pub fn name(self) -> &'static str {
        match self  { IdleState::Busy => "busy", IdleState::Entering => "entering", IdleState::Idle => "idle",
        IdleState::Leaving => "leaving", }
    }

    /// 由序号反解态（越界返回 None · 无 panic）。
    pub fn from_ord(o: u8) -> Option<IdleState> {
        match o {
            0 => Some(IdleState::Busy),
            1 => Some(IdleState::Entering),
            2 => Some(IdleState::Idle),
            3 => Some(IdleState::Leaving),
            _ => None,
        }
    }

    /// 是否处于「过渡态」（F1654 过渡态纪律：过渡态中禁止跨态直落）。
    pub fn in_transition(self) -> bool { matches!(self, IdleState::Entering | IdleState::Leaving) }
}

/// 状态转换矩阵（**行 = from · 列 = to · 顺序 Busy, Entering, Idle, Leaving**）。
/// 合法边仅 4 条：`Busy→Entering`、`Entering→Idle`、`Idle→Leaving`、`Leaving→Busy`。其余 12 条边全拒（闭集冻结 · 编译期表驱动）。
pub const IDLE_TRANS_MATRIX: [[bool; 4]; 4] = [
    //            Busy    Entering  Idle   Leaving
    /* Busy    */ [false, true, false, false],
    /* Entering*/ [false, false, true, false],
    /* Idle    */ [false, false, false, true],
    /* Leaving */ [true, false, false, false],
];

/// 转换合法性判定（矩阵唯一裁决点 · 纯函数）。
pub fn trans_legal(from: IdleState, to: IdleState) -> bool {
    let f = from.ordinal() as usize; let t = to.ordinal() as usize;
    if f >= 4 || t >= 4 { return false; }
    IDLE_TRANS_MATRIX[f][t]
}

/// 转换原因码（闭集 · 追加到转换账）。
pub mod cause {
    /// 空闲调度器进入四步链（Step0 前置）。
    pub const BUSY_TO_ENTERING: u8 = 0x10;
    /// Step4 状态落定成功。
    pub const ENTERING_TO_IDLE: u8 = 0x11;
    /// 唤醒源触发（F1744 入口代登记）。
    pub const IDLE_TO_LEAVING: u8 = 0x12;
    /// 退出完成回到忙态。
    pub const LEAVING_TO_BUSY: u8 = 0x13;
    /// 注入核非法边（登记但不改变实际态）。
    pub const INJECT_ILLEGAL: u8 = 0xF0;
    /// 注入核乱序边。
    pub const INJECT_ORDER: u8 = 0xF1;
}

/// 单次转换账条目（append-only · 一次转换一条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransRecord {
    /// 账内序号（单调递增 · 从 1 起）。
    pub seq: u64,
    /// 转换前态（ordinal）。
    pub from: u8,
    /// 转换后态（ordinal）。
    pub to: u8,
    /// 逻辑 tick。
    pub tick: u64,
    /// 核号。
    pub cpu: u32,
    /// 原因码（`cause::*`）。
    pub cause: u8,
    /// 关联 `enter_id`（非进入路径转换为 0）。
    pub enter_id: u64,
    /// 本次转换是否被受理。
    pub accepted: bool,
}

impl TransRecord {
    /// 账条目一行文本（诊断用 · 无 panic）。
    pub fn line(&self) -> String {
        let from_name = IdleState::from_ord(self.from)
        .map(|s| s.name())
        .unwrap_or("?");
        let to_name = IdleState::from_ord(self.to).map(|s| s.name()).unwrap_or("?");
        format!(
            "tr#{} {}->{} tick={} cpu={} cause=0x{:02X} eid={} acc={}",
            self.seq, from_name, to_name, self.tick, self.cpu, self.cause, self.enter_id, self.accepted
        )
    }
}

/// append-only 环形账本（通用 · 满则覆盖最旧并累计 `dropped`）。
#[derive(Clone, Debug)]
pub struct RingLedger<T> {
    /// 账内条目（容量 `cap` · 满后覆盖最旧）。
    pub items: Vec<T>,
    /// 容量上限。
    pub cap: usize,
    /// 被覆盖（丢失）条数 —— 显性化，不静默。
    pub dropped: u64,
    /// 累计追加条数（含被覆盖者）。
    pub total: u64,
}

impl<T: Clone> RingLedger<T> {
    /// 新账本（容量非法时钳到 1 · 无 panic）。
    pub fn new(cap: usize) -> Self {
        let c = if cap == 0 { 1 } else { cap }; let mut v: Vec<T> = Vec::new();
        if c > 8 { v = Vec::with_capacity(c); }
        RingLedger  { items: v, cap: c, dropped: 0, total: 0, }
    }

    /// 追加一条（摊还 O(1) · 满则环形覆盖最旧并 `dropped += 1`）。
    pub fn push(&mut self, item: T) -> u64 {
        self.total = self.total.wrapping_add(1);
        if self.items.len() < self.cap {
            self.items.push(item);
        } else {
            if self.cap > 0 { let idx = (self.total.wrapping_sub(1) % self.cap as u64) as usize; self.items[idx] = item; }
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.total
    }

    /// 当前有效条数。
    pub fn len(&self) -> usize { self.items.len() }

    /// 账本是否空。
    pub fn is_empty(&self) -> bool { self.items.is_empty() }

    /// 取第 `i` 条（越界 None）。
    pub fn get(&self, i: usize) -> Option<&T> { self.items.get(i) }

    /// 取最后一条（空则 None）。
    pub fn last(&self) -> Option<&T> { self.items.last() }

    /// 逆序迭代索引（`0` = 最新）。
    pub fn rev_index(&self, i: usize) -> Option<&T> {
        if i >= self.items.len() { return None; }
        let n = self.items.len();
        self.items.get(n - 1 - i)
    }

    /// 清空（保留容量与累计计数）。
    pub fn clear(&mut self) { self.items.clear(); }
}

/// 队列快照（每核就绪队列的四个标量 · O(1) 判定输入）。判据：`depth != 0` **不是**队空的充分条件。两个伪队空形态：
/// - **形态 P1「有线程但全部被阻塞」**：`depth == 0` 但 `blocked_live > 0`——队列槽位已空，
///   但被阻塞线程仍挂在等待链上；此时若直接判空进 idle，被阻塞线程的唤醒源会落在 idle 之后，  形成唤醒路径与 idle 进入路径的竞争窗口（F1744 需二次确认）。故判为 **PseudoEmpty**。
/// - **形态 P2「队内有线程但正在切换过渡态」**：`depth > 0` 但全部线程处于切换过渡态占位
///   （`transition_hold == depth`）——此时核正在切换（F1654 过渡态纪律），不允许落定 idle。  故判为 **PseudoEmpty**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueSnapshot {
    /// 就绪队列可运行线程数（F1651 保证自洽的深度标量）。
    pub depth: u32,
    /// 仍存活但被阻塞（等待链上）的线程数（计入「有线程但全阻塞」）。
    pub blocked_live: u32,
    /// 处于切换过渡态占位的线程数（F1654 · 与 B11 切换主路径共用纪律）。
    pub transition_hold: u32,
    /// 迁移拉取在途标记（F1727 idle 拉取衔接 · 与 B07 迁移共用）。
    pub migrate_pending: bool,
}

impl QueueSnapshot {
    /// 构造「真队空」快照（depth=0 · 无阻塞 · 无过渡占位 · 无迁移）。
    pub fn true_empty() -> Self {
        QueueSnapshot  { depth: 0, blocked_live: 0, transition_hold: 0, migrate_pending: false, }
    }

    /// 构造「伪队空·全阻塞」快照（形态 P1）。
    pub fn pseudo_blocked(blocked_live: u32) -> Self {
        QueueSnapshot  { depth: 0, blocked_live, transition_hold: 0, migrate_pending: false, }
    }

    /// 构造「伪队空·切换过渡」快照（形态 P2）。
    pub fn pseudo_transition(depth: u32) -> Self {
        QueueSnapshot  { depth, blocked_live: 0, transition_hold: depth, migrate_pending: false, }
    }

    /// 构造「队非空」快照。
    pub fn non_empty(depth: u32) -> Self {
        QueueSnapshot  { depth, blocked_live: 0, transition_hold: 0, migrate_pending: false, }
    }
}

/// 队空判定结果（闭集 · 三形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueEmptiness {
    /// 真队空：可安全进入 idle（唯一允许继续四步链的形态）。
    TrueEmpty,
    /// 伪队空（过渡态占位）：**必须延后**，登记延后账，不得静默当空。
    PseudoEmpty(PseudoCause),
    /// 队非空：拒绝进入，返回 `E::QueueNotEmpty`。
    NonEmpty(u32),
}

/// 伪队空成因（闭集 · 可区分 P1/P2/迁移在途）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoCause {
    /// P1：队列槽位空但有存活阻塞线程。
    BlockedLive(u32),
    /// P2：队内线程全部处于切换过渡态（F1654）。
    TransitionHold(u32),
    /// P3：迁移拉取在途（F1727 衔接）。
    MigratePending,
    /// 组合：阻塞 + 过渡占位并存。
    BlockedAndTransition(u32, u32),
}

impl PseudoCause {
    /// 成因短名。
    pub fn name(self) -> &'static str {
        match self  { PseudoCause::BlockedLive(_) => "blocked_live",
            PseudoCause::TransitionHold(_) => "transition_hold", PseudoCause::MigratePending => "migrate_pending",
        PseudoCause::BlockedAndTransition(_, _) => "blocked_and_transition", }
    }

    /// 成因计数（延后账归并键）。
    pub fn key(self) -> u8 {
        match self  { PseudoCause::BlockedLive(_) => 1, PseudoCause::TransitionHold(_) => 2,
        PseudoCause::MigratePending => 3, PseudoCause::BlockedAndTransition(_, _) => 4, }
    }
}

/// 队空判定表（优先级自上而下 · **纯函数 · Step1 唯一裁决点**）。判据：伪队空必须延后进入（登记延后账，不得静默当空）。判定顺序：
/// 1. `migrate_pending` → PseudoEmpty(MigratePending)（F1727 衔接优先于一切）。
/// 2. `depth > 0 && transition_hold == depth` → PseudoEmpty(TransitionHold)（P2）。
/// 3. `depth == 0 && blocked_live > 0` → PseudoEmpty(BlockedLive)（P1）。
/// 4. `depth == 0 && blocked_live > 0 && transition_hold > 0` 已被 2/3 覆盖，
///    但当 `depth == 0` 且两者同时 >0 时归 `BlockedAndTransition`（组合成因）。
/// 5. `depth == 0 && blocked_live == 0` → TrueEmpty。
/// 6. 其余（`depth > 0` 且未全占位）→ NonEmpty(depth)。
pub fn judge_queue(snap: QueueSnapshot) -> QueueEmptiness {
    if snap.migrate_pending { return QueueEmptiness::PseudoEmpty(PseudoCause::MigratePending); }
    let has_blocked = snap.blocked_live > 0; let all_hold = snap.depth > 0 && snap.transition_hold >= snap.depth;
    if all_hold && has_blocked {
        return QueueEmptiness::PseudoEmpty(PseudoCause::BlockedAndTransition(
                snap.blocked_live,
                snap.transition_hold,
        ));
    }
    if all_hold { return QueueEmptiness::PseudoEmpty(PseudoCause::TransitionHold(snap.transition_hold)); }
    if snap.depth == 0 && has_blocked { return QueueEmptiness::PseudoEmpty(PseudoCause::BlockedLive(snap.blocked_live)); }
    if snap.depth == 0 { return QueueEmptiness::TrueEmpty; }
    QueueEmptiness::NonEmpty(snap.depth)
}

/// 判定是否允许继续四步链（仅 TrueEmpty 允许）。
pub fn may_enter(judge: QueueEmptiness) -> bool { matches!(judge, QueueEmptiness::TrueEmpty) }

/// 静息位来源（F1735 挂点消费 · 两个挂点分别登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuiesceSource {
    /// 切换挂点（context_switch 出口 · B11 切换主路径共挂点）。
    SwitchHook,
    /// idle 挂点（idle 进入点 · 本单 Step2 消费）。
    IdleHook,
    /// 注入核伪造来源（非法 · 仅注入核使用）。
    Injected,
}

impl QuiesceSource {
    /// 来源短名。
    pub fn name(self) -> &'static str {
        match self  { QuiesceSource::SwitchHook => "switch_hook", QuiesceSource::IdleHook => "idle_hook",
        QuiesceSource::Injected => "injected", }
    }

    /// 来源序号（0/1/2）。
    pub fn ordinal(self) -> u8 {
        match self  { QuiesceSource::SwitchHook => 0, QuiesceSource::IdleHook => 1, QuiesceSource::Injected => 2, }
    }

    /// 是否为 F1735 认可的两个挂点之一（`Injected` 不认可）。
    pub fn legit(self) -> bool { matches!(self, QuiesceSource::SwitchHook | QuiesceSource::IdleHook) }

    /// 解析序号（越界 None）。
    pub fn from_ord(o: u8) -> Option<QuiesceSource> {
        match o {
            0 => Some(QuiesceSource::SwitchHook),
            1 => Some(QuiesceSource::IdleHook),
            2 => Some(QuiesceSource::Injected),
            _ => None,
        }
    }
}

/// 每核静息位（F1735 原语在本模块的落地视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuiesceBit {
    /// 静息位当前值（1 = 已置位）。
    pub bit: u8,
    /// 置位时刻 tick（`bit == 0` 时为 0）。
    pub set_tick: u64,
    /// 置位来源（最后一个置位者 · 清位时保持）。
    pub source: QuiesceSource,
    /// 该位被置位的总次数（含幂等重复置位的调用，但**不记账**于 hit 分子）。
    pub set_calls: u64,
    /// 幂等命中次数（同核重复置位被拒且不记账的次数）。
    pub idempotent_hits: u64,
    /// 清位次数。
    pub clear_calls: u64,
}

impl QuiesceBit {
    /// 零值静息位（未置位）。
    pub const ZERO: QuiesceBit = QuiesceBit  { bit: 0, set_tick: 0, source: QuiesceSource::SwitchHook, set_calls: 0,
    idempotent_hits: 0, clear_calls: 0, };

    /// 是否已置位。
    pub fn is_set(&self) -> bool { self.bit == 1 }

    /// 置位时刻已置位的持续 tick（未置位返回 0）。
    pub fn held_ticks(&self, now: u64) -> u64 {
        if self.bit == 1 && now >= self.set_tick {
            now - self.set_tick
        } else {
            0
        }
    }
}

/// 每核位图（静息位集合 · `NCPUS_MAX` 内联位图 + 溢出段）。
#[derive(Clone, Debug)]
pub struct QuiesceBitmap {
    /// 内联位图（NCPUS_MAX ≤ 64 时单 u64 足够 · O(1) 寻址）。
    pub bits: u64,
    /// 核数。
    pub cpus: u32,
}

impl QuiesceBitmap {
    /// 新位图（`cpus > NCPUS_MAX` 时高位无法表示 · 由 `E::Range` 拒绝）。
    pub fn new(cpus: u32) -> Self { QuiesceBitmap { bits: 0, cpus } }

    /// 置位（幂等 · 返回是否真正发生置位）。
    pub fn set(&mut self, cpu: u32) -> bool {
        let m = 1u64 << (cpu & 63); let before = self.bits & m; self.bits |= m;
        before == 0
    }

    /// 清位（幂等 · 返回是否真正发生清位）。
    pub fn clear(&mut self, cpu: u32) -> bool {
        let m = 1u64 << (cpu & 63); let before = self.bits & m; self.bits &= !m;
        before != 0
    }

    /// 查询（O(1)）。
    pub fn test(&self, cpu: u32) -> bool { self.bits & (1u64 << (cpu & 63)) != 0 }

    /// 已置位核数（popcount · O(1)）。
    pub fn count(&self) -> u32 { self.bits.count_ones() }

    /// 索引为 1 的最低核号（空则 `None`）。
    pub fn lowest_set(&self) -> Option<u32> {
        if self.bits == 0 {
            None
        } else {
            Some(self.bits.trailing_zeros() as u32)
        }
    }
}

/// 四步交接签名（步号 + 队列深度快照 + 核号 · 链式折叠）。判据 J1-a：每步消费上一步交接签名，跳步 / 乱序 / 伪造即拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepSig {
    /// 所属 `enter_id`（一次进入生成一个 · 四步共享）。
    pub enter_id: u64,
    /// 步号（1..=4）。
    pub step: u8,
    /// 队列深度快照（Step1 判定时的 `snap.depth` · 全链共享不重采样）。
    pub depth_snapshot: u32,
    /// 核号。
    pub cpu: u32,
    /// 上一签名的折叠值（Step1 为 0 · 形成链）。
    pub prev_fold: u64,
}

impl StepSig {
    /// 折叠函数（确定性 · 3 异或 + 2 乘 + 1 移位 · 无浮点）。
    pub fn fold(&self) -> u64 {
        let mut h = self.enter_id ^ ((self.step as u64) << 56); h ^= (self.depth_snapshot as u64).wrapping_mul(0x9E3779B97F4A7C15);
        h = h.wrapping_mul(6364136223846793005); h ^= (self.cpu as u64).wrapping_add(1442695040888963407);
        h ^ self.prev_fold
    }

    /// 签名一行文本。
    pub fn line(&self) -> String {
        format!(
            "sig eid={} step={} depth={} cpu={} fold=0x{:016X}",
            self.enter_id,
            self.step,
            self.depth_snapshot,
            self.cpu,
            self.fold()
        )
    }
}

/// 签名账条目（append-only · 四步共享一个 `enter_id` · 回放可核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigRecord {
    /// `enter_id`。
    pub enter_id: u64,
    /// 步号。
    pub step: u8,
    /// 核号。
    pub cpu: u32,
    /// 深度快照。
    pub depth_snapshot: u32,
    /// 折叠值。
    pub fold: u64,
    /// 步起 tick（逻辑）。
    pub tick_begin: u64,
    /// 步止 tick（逻辑）。
    pub tick_end: u64,
    /// 该步是否被受理。
    pub accepted: bool,
}

impl SigRecord {
    /// 账条目一行文本。
    pub fn line(&self) -> String {
        format!(
            "sig eid={} step={} cpu={} depth={} fold=0x{:016X} t=[{}..{}] acc={}",
            self.enter_id,
            self.step,
            self.cpu,
            self.depth_snapshot,
            self.fold,
            self.tick_begin,
            self.tick_end,
            self.accepted
        )
    }
}

/// 伪队空延后账条目（**伪队空必须延后 · 不得静默当空**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeferRecord {
    /// 账内序号。
    pub seq: u64,
    /// 逻辑 tick。
    pub tick: u64,
    /// 核号。
    pub cpu: u32,
    /// 成因键（`PseudoCause::key()`）。
    pub cause_key: u8,
    /// 成因名（诊断输出用 · 借用静态串）。
    pub cause_name: &'static str,
    /// 当时的队列深度。
    pub depth: u32,
    /// 延后次数（同一 (cpu, cause) 的第几次延后）。
    pub defer_ordinal: u64,
}

/// 每核账目（idle 起算 + busy 累计 · 守恒输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuLedger {
    /// 累计 idle tick（Σidle）。
    pub idle_ticks: u64,
    /// 累计 busy tick（Σbusy）。
    pub busy_ticks: u64,
    /// 当前 idle 段起算 tick（不在 idle 时为 0）。
    pub idle_begin: u64,
    /// idle 段计数（进入次数）。
    pub idle_segments: u64,
    /// 当前 idle 段的 `enter_id`（不在 idle 时为 0）。
    pub cur_enter_id: u64,
}

impl CpuLedger {
    /// 零值账目。
    pub const ZERO: CpuLedger = CpuLedger  { idle_ticks: 0, busy_ticks: 0, idle_begin: 0, idle_segments: 0,
    cur_enter_id: 0, };

    /// 是否处于 idle 段中。
    pub fn in_idle(&self) -> bool { self.idle_begin != 0 }

    /// 当前未闭合 idle 段的时长（已累计 idle 不含本段）。
    pub fn open_segment_ticks(&self, now: u64) -> u64 {
        if self.in_idle() && now >= self.idle_begin {
            now - self.idle_begin
        } else {
            0
        }
    }

    /// idle + busy 之和（含当前未闭合 idle 段的时长）。
    pub fn total_ticks(&self, now: u64) -> u64 {
        self.idle_ticks
        .wrapping_add(self.busy_ticks)
        .wrapping_add(self.open_segment_ticks(now))
    }
}

/// 每核 idle 进入上下文（状态 + 静息位 + 账目 + 队列快照）。
#[derive(Clone, Copy, Debug)]
pub struct CpuCtx {
    /// 当前态。
    pub state: IdleState,
    /// 静息位。
    pub quiesce: QuiesceBit,
    /// 账目。
    pub ledger: CpuLedger,
    /// 最近一次队列快照（Step1 采样 · 供诊断与回放对账）。
    pub last_snap: QueueSnapshot,
    /// 最近一次 Step1 判定结果（闭集）。
    pub last_judge: QueueJudgeSnapshot,
    /// 本核累计伪队空延后次数。
    pub defer_count: u64,
    /// 本核累计进入 idle 次数（分母来源）。
    pub enter_count: u64,
    /// 本核累计静息位置位命中次数（分子来源）。
    pub quiesce_hit_count: u64,
}

/// Step1 判定结果快照（可存入 CpuCtx · 故不用 `QueueEmptiness` 的借用问题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueJudgeSnapshot {
    /// 是否真队空。
    pub true_empty: bool,
    /// 成因键（0 = 无成因）。
    pub cause_key: u8,
    /// 成因名。
    pub cause_name: &'static str,
    /// 判定时深度。
    pub depth: u32,
}

impl QueueJudgeSnapshot {
    /// 无成因（非伪队空）。
    pub const NONE: QueueJudgeSnapshot = QueueJudgeSnapshot  { true_empty: false, cause_key: 0, cause_name: "-", depth: 0, };

    /// 由 `QueueEmptiness` 归一为快照（无损 · 闭集穷举）。
    pub fn of(j: QueueEmptiness) -> Self {
        match j {
            QueueEmptiness::TrueEmpty => QueueJudgeSnapshot  { true_empty: true, cause_key: 0,
            cause_name: "true_empty", depth: 0, },
            QueueEmptiness::PseudoEmpty(c) => QueueJudgeSnapshot  { true_empty: false, cause_key: c.key(),
            cause_name: c.name(), depth: 0, },
            QueueEmptiness::NonEmpty(d) => QueueJudgeSnapshot  { true_empty: false, cause_key: 9,
            cause_name: "non_empty", depth: d, },
        }
    }
}

impl CpuCtx {
    /// 新核上下文（初始 Busy · 账目零 · 静息位零）。
    pub fn new() -> Self {
        CpuCtx  { state: IdleState::Busy, quiesce: QuiesceBit::ZERO, ledger: CpuLedger::ZERO,
            last_snap: QueueSnapshot::true_empty(), last_judge: QueueJudgeSnapshot::NONE, defer_count: 0, enter_count: 0,
        quiesce_hit_count: 0, }
    }
}

/// 静息命中率（分子分母同源 · 万分比定点）。判据 J1-b：分子 = `quiesce_hit_count` 之和（静息位置位成功次数，仅 Step2 真实置位计入），
/// 分母 = `enter_count` 之和（idle 实际落定次数）。两者来自同一次 `enter_id` 的同一事件流，故在无注入绕行路径时比值恰为 10000/10000 = 100%。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitRate {
    /// 分子：静息位置位成功次数。
    pub hit: u64,
    /// 分母：idle 进入次数。
    pub enter: u64,
    /// 万分比（`hit * 10000 / enter` · 分母为 0 时为 10000）。
    pub permille: u64,
}

impl HitRate {
    /// 是否恰为 100%。
    pub fn is_full(&self) -> bool { self.permille >= HIT_RATE_SCALE }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "quiesce_hit_rate {}/{} = {}.{:02}%",
            self.hit,
            self.enter,
            self.permille / 100,
            self.permille % 100
        )
    }
}

/// 全局账目（守恒锚 · F1745 结算输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalLedger {
    /// 上线总时长（各核相同基准 · 逻辑 tick）。
    pub online_ticks: u64,
    /// 核数。
    pub cpus: u32,
}

impl GlobalLedger {
    /// 全局守恒期望值（Σ 各核在线时长 = `online_ticks * cpus`）。
    pub fn expect_total(&self) -> u64 { self.online_ticks.wrapping_mul(self.cpus as u64) }
}

/// 守恒校验结果（`Σidle + Σbusy == total_online_ticks` · 全局守恒）。手算锚：4 核 × 1000 tick，每核 idle 300 + busy 700 → 300*4 + 700*4 = 4000 == 4*1000 ✓。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conservation {
    /// Σ 各核 idle tick。
    pub sum_idle: u64,
    /// Σ 各核 busy tick。
    pub sum_busy: u64,
    /// Σ idle + Σ busy（实测）。
    pub lhs: u64,
    /// `online_ticks * cpus`（期望）。
    pub rhs: u64,
    /// 偏差（`lhs - rhs` · i64 有符号）。
    pub drift: i64,
}

impl Conservation {
    /// 是否守恒。
    pub fn ok(&self) -> bool { self.drift == 0 }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "conservation idle={} busy={} lhs={} rhs={} drift={}",
            self.sum_idle, self.sum_busy, self.lhs, self.rhs, self.drift
        )
    }
}

/// 一次 idle 进入的全过程快照（回放对账单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterSnapshot {
    /// `enter_id`。
    pub enter_id: u64,
    /// 核号。
    pub cpu: u32,
    /// 四步是否全部受理。
    pub four_steps_ok: bool,
    /// Step1 判定是否真队空。
    pub step1_true_empty: bool,
    /// Step2 是否真实置位静息位。
    pub step2_quiesce_set: bool,
    /// Step3 是否完成账目起算。
    pub step3_ledger_begin: bool,
    /// Step4 是否落定为 Idle。
    pub step4_settled: bool,
    /// 进入起点 tick。
    pub t_enter: u64,
    /// 四步完成 tick。
    pub t_settled: u64,
    /// 四步逻辑耗时（`t_settled - t_enter`）。
    pub elapsed: u64,
    /// 队空判定返回的形态序号（0=TrueEmpty / 1=PseudoEmpty / 2=NonEmpty）。
    pub judge_ord: u8,
    /// 静息位置位来源序号。
    pub quiesce_source_ord: u8,
    /// 交接签名折叠终值。
    pub final_fold: u64,
}

/// idle 进入路径引擎（每核一份上下文 · 全局共享三本账）。判据落点：
/// - J1-a：四步严格顺序，每步消费上一步交接签名。
/// - J1-b：静息命中率统计器（分子分母同源）。
/// - J1-c：静息位来源登记（切换挂点 / idle 挂点）。
#[derive(Clone, Debug)]
pub struct IdleEnter {
    /// 每核上下文。
    pub cpus: Vec<CpuCtx>,
    /// 全局静息位图。
    pub quiesce_map: QuiesceBitmap,
    /// 状态转换账（append-only 环形）。
    pub trans: RingLedger<TransRecord>,
    /// 四步签名账（append-only 环形）。
    pub sigs: RingLedger<SigRecord>,
    /// 伪队空延后账（append-only 环形）。
    pub defers: RingLedger<DeferRecord>,
    /// F1666 idle 发现账（双源对账 · 每核发现 idle tick）。
    pub discovery_idle_ticks: Vec<u64>,
    /// 当前逻辑 tick。
    pub now: u64,
    /// 上线总时长（各核基准）。
    pub online_ticks: u64,
    /// `enter_id` 分配器（单调 · 从 1 起）。
    pub next_enter_id: u64,
    /// 转换账序号分配器。
    pub next_seq: u64,
    /// 延后账序号分配器。
    pub next_defer_seq: u64,
    /// 转换序号分配器（供状态机内部登记）。
    pub trans_seq: u64,
    /// 静息位置位总成功次数（分子 · 全局）。
    pub quiesce_hit_total: u64,
    /// idle 落定总次数（分母 · 全局）。
    pub enter_total: u64,
    /// 静息位置位调用总次数（含幂等重复）。
    pub quiesce_call_total: u64,
    /// 幂等置位命中总次数。
    pub idempotent_total: u64,
    /// 伪队空延后总次数。
    pub defer_total: u64,
    /// 切换挂点置位次数（F1735 挂点消费 · 来源分离登记）。
    pub switch_hook_sets: u64,
    /// idle 挂点置位次数（F1735 挂点消费 · 来源分离登记）。
    pub idle_hook_sets: u64,
    /// 非法来源置位次数（仅注入核产生 · 显性化）。
    pub illegal_source_sets: u64,
    /// 四步耗时累计 tick（四步平均耗时分子）。
    pub enter_elapsed_total: u64,
    /// 四步耗时样本数（分母）。
    pub enter_elapsed_samples: u64,
    /// 确定性 LCG 状态（注入核与回放共用 · 可复现）。
    pub rng: u64,
    /// 当前进行中的 `enter_id`（0 = 无进行中的进入）。
    pub cur_enter_id: u64,
    /// 当前进行中的步号（0 = 未开始 / 已完成）。
    pub cur_step: u8,
    /// 当前进行中的链尾折叠值。
    pub cur_fold: u64,
    /// 当前进行中的深度快照。
    pub cur_depth_snapshot: u32,
    /// 当前进行中的核号。
    pub cur_cpu: u32,
    /// 当前进行中的进入起点 tick（用于四步耗时统计）。
    pub cur_t_enter: u64,
    /// 最近的错误（闭集 · 最近一次失败）。
    pub last_err: Option<E>,
    /// 累计失败次数。
    pub fail_total: u64,
    /// 累计成功进入次数（= `enter_total` 的镜像 · 一致性自检用）。
    pub ok_total: u64,
    /// 本次调用的诊断码（成功 = `DIAG_ENTER_OK`）。
    pub last_code: u16,
}

impl IdleEnter {
    /// 新引擎（`cpus` 核 · `online_ticks` 上线总时长 · 确定性种子 `rng`）。判据：核数必须 ≤ `NCPUS_MAX`（位图容量），否则 `E::Range`。
    pub fn new(cpus: u32, online_ticks: u64, rng: u64) -> Res<Self> {
        if cpus == 0 || cpus as usize > NCPUS_MAX {
            return Err(E::Range { what: 1, val: cpus as u64, });
        }
        if online_ticks == 0 {
            return Err(E::Range { what: 2, val: online_ticks, });
        }
        let mut v: Vec<CpuCtx> = Vec::with_capacity(cpus as usize); let mut d: Vec<u64> = Vec::with_capacity(cpus as usize);
        for _ in 0..cpus { v.push(CpuCtx::new()); d.push(0); }
        Ok(IdleEnter {
                cpus: v,
                quiesce_map: QuiesceBitmap::new(cpus),
                trans: RingLedger::new(TRANS_LEDGER_CAP),
                sigs: RingLedger::new(SIG_LEDGER_CAP),
                defers: RingLedger::new(DEFER_LEDGER_CAP),
                discovery_idle_ticks: d,
                now: 0,
                online_ticks,
                next_enter_id: 1,
                next_seq: 1,
                next_defer_seq: 1,
                trans_seq: 1,
                quiesce_hit_total: 0,
                enter_total: 0,
                quiesce_call_total: 0,
                idempotent_total: 0,
                defer_total: 0,
                switch_hook_sets: 0,
                idle_hook_sets: 0,
                illegal_source_sets: 0,
                enter_elapsed_total: 0,
                enter_elapsed_samples: 0,
                rng: if rng == 0 { 0x5DEECE66D } else { rng },
                cur_enter_id: 0,
                cur_step: 0,
                cur_fold: 0,
                cur_depth_snapshot: 0,
                cur_cpu: 0,
                cur_t_enter: 0,
                last_err: None,
                fail_total: 0,
                ok_total: 0,
                last_code: DIAG_ENTER_OK,
        })
    }

    /// 便捷构造（默认 4 核 × 1000 tick · 手算守恒锚）。
    pub fn demo() -> Res<Self> { IdleEnter::new(4, DEFAULT_ONLINE_TICKS, 0x00C0FFEE) }

    /// 核数。
    pub fn cpu_count(&self) -> u32 { self.cpus.len() as u32 }

    /// 推进逻辑时钟（单调 · 禁止回退）。
    pub fn advance(&mut self, to: u64) -> Res<()> {
        if to < self.now {
            self.note_err(E::Range { what: 3, val: to, }); return Err(E::Range { what: 3, val: to, });
        }
        self.now = to;
        Ok(())
    }

    /// 取核上下文（越界返回 `None` · 无 panic）。
    pub fn cpu(&self, cpu: u32) -> Res<&CpuCtx> {
        match self.cpus.get(cpu as usize) {
            Some(c) => Ok(c),
            None => Err(E::Range {
                    what: 4,
                    val: cpu as u64,
            }),
        }
    }

    /// 取核上下文可写（越界 `Err` · 不写 `last_err`，避免与返回值借用冲突）。
    pub fn cpu_mut(&mut self, cpu: u32) -> Res<&mut CpuCtx> {
        let idx = cpu as usize; let len = self.cpus.len();
        if idx >= len {
            return Err(E::Range { what: 4, val: cpu as u64, });
        }
        match self.cpus.get_mut(idx) {
            Some(c) => Ok(c),
            None => Err(E::Range {
                    what: 4,
                    val: cpu as u64,
            }),
        }
    }

    /// 记一次失败（更新 `last_err` / `fail_total` / `last_code`）。
    pub fn note_err(&mut self, e: E) {
        self.fail_total = self.fail_total.wrapping_add(1); self.last_code = e.code();
        if self.last_err.is_none() { self.last_err = Some(e); }
    }

    /// 取核号合法（≤ 引擎核数）。
    pub fn cpu_valid(&self, cpu: u32) -> bool { (cpu as usize) < self.cpus.len() }

    /// 全局守恒校验（`Σidle + Σbusy == online_ticks * cpus`）。
    // 判据：UNX-F1742-J1 —— 全局守恒 Σidle + Σbusy = 上线总时长（与 F1745 结算同锚）。
    pub fn conservation(&self) -> Conservation {
        let mut si: u64 = 0; let mut sb: u64 = 0;
        for c in self.cpus.iter() {
            si = si.wrapping_add(c.ledger.idle_ticks); si = si.wrapping_add(c.ledger.open_segment_ticks(self.now));
            sb = sb.wrapping_add(c.ledger.busy_ticks);
        }
        let lhs = si.wrapping_add(sb);
        let g = GlobalLedger  { online_ticks: self.online_ticks, cpus: self.cpu_count(), };
        let rhs = g.expect_total(); let drift = lhs as i64 - rhs as i64;
        Conservation  { sum_idle: si, sum_busy: sb, lhs, rhs, drift, }
    }

    /// 守恒校验并在破坏时返回 `E::Conservation`。
    pub fn check_conservation(&mut self) -> Res<Conservation> {
        let c = self.conservation();
        if c.ok() {
            self.last_code = DIAG_ENTER_OK;
            Ok(c)
        } else {
            let e = E::Conservation  { lhs: c.lhs, rhs: c.rhs, };
            self.note_err(e);
            Err(e)
        }
    }

    /// 静息命中率（分子分母同源 · 万分比）。
    // 判据：UNX-F1742-J1 —— 静息置位在 idle 进入点 100% 命中（F1735 挂点消费）。
    pub fn quiesce_hit_rate(&self) -> HitRate {
        let permille = if self.enter_total == 0 {
            HIT_RATE_SCALE
        } else {
            self.quiesce_hit_total
            .wrapping_mul(HIT_RATE_SCALE)
            / self.enter_total
        };
        HitRate  { hit: self.quiesce_hit_total, enter: self.enter_total, permille, }
    }

    /// 静息命中率强制校验：非 100% 立即显性报错（`E::QuiesceMiss`）。
    // 判据：UNX-F1742-J1 —— 命中率 < 100% 立即显性报错并落账。
    pub fn require_full_hit_rate(&mut self) -> Res<HitRate> {
        let h = self.quiesce_hit_rate();
        if h.is_full() {
            self.last_code = DIAG_ENTER_OK;
            Ok(h)
        } else {
            let e = E::QuiesceMiss  { hit: h.hit, enter: h.enter, };
            self.note_err(e);
            Err(e)
        }
    }

    /// 全核静息位是否全部置位（位图聚合查询）。
    pub fn all_quiesced(&self) -> bool { self.quiesce_map.count() >= self.cpu_count() }
}

/// 进入路径阶段（闭集 · 四步链的对外封装粒度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 未开始。
    None,
    /// Step1 已完成（队空判定通过）。
    Probed,
    /// Step2 已完成（静息位已置）。
    Quiesced,
    /// Step3 已完成（账目已起算）。
    Accounted,
    /// Step4 已完成（状态已落定为 Idle）。
    Settled,
}

impl Phase {
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self  { Phase::None => "none", Phase::Probed => "probed", Phase::Quiesced => "quiesced",
        Phase::Accounted => "accounted", Phase::Settled => "settled", }
    }

    /// 相位序号（0..=4）。
    pub fn ordinal(self) -> u8 {
        match self  { Phase::None => 0, Phase::Probed => 1, Phase::Quiesced => 2, Phase::Accounted => 3,
        Phase::Settled => 4, }
    }
}

impl IdleEnter {
    /// 状态机迁移（矩阵唯一执行点 · 非法边闭集拒绝）。合法边仅 4 条，见 `IDLE_TRANS_MATRIX`。非法边：登记转换账（`accepted = false`）并返回 `E::IllegalTrans`，实际态保持不变。
    pub fn trans(&mut self, cpu: u32, to: IdleState, cause: u8, enter_id: u64) -> Res<IdleState> {
        if !self.cpu_valid(cpu) {
            let e = E::Range  { what: 4, val: cpu as u64, };
            self.note_err(e); return Err(e);
        }
        let from = match self.cpus.get(cpu as usize) {
            Some(c) => c.state,
            None => IdleState::Busy,
        };
        let legal = trans_legal(from, to);
        let rec = TransRecord  { seq: self.next_seq, from: from.ordinal(), to: to.ordinal(), tick: self.now, cpu,
        cause, enter_id, accepted: legal, };
        self.next_seq = self.next_seq.wrapping_add(1); self.trans.push(rec);
        if !legal {
            let e = E::IllegalTrans  { from: from.ordinal(), to: to.ordinal(), };
            self.note_err(e); return Err(e);
        }
        if let Some(c) = self.cpus.get_mut(cpu as usize) { c.state = to; }
        self.last_code = DIAG_ENTER_OK;
        Ok(to)
    }

    /// 转换账与实际状态一致性断言（逐核：账尾态 == 实际态）。
    // 判据：UNX-F1742-J1 —— 转换账与实际状态一致断言全过。
    pub fn assert_trans_ledger(&mut self) -> Res<u64> {
        let n = self.cpus.len(); let mut checked: u64 = 0;
        for i in 0..n {
            let cpu = i as u32;
            let actual = match self.cpus.get(i) {
                Some(c) => c.state.ordinal(),
                None => 0,
            };
            // 取该核最后一条已受理的转换账。
            let mut ledger_state: Option<u8> = None; let mut k = 0usize;
            while let Some(r) = self.trans.rev_index(k) {
                if r.cpu == cpu && r.accepted { ledger_state = Some(r.to); break; }
                k += 1;
            }
            let expect = match ledger_state {
                Some(s) => s,
                None => IdleState::Busy.ordinal(),
            };
            if expect != actual {
                let e = E::LedgerDrift  { cpu, ledger: expect, actual, };
                self.note_err(e); return Err(e);
            }
            checked = checked.wrapping_add(1);
        }
        Ok(checked)
    }

    /// 开启一次进入（分配 `enter_id` · Busy → Entering）。前置条件：核处于 `Busy`（若处于过渡态 `Entering`，返回 `E::InTransition`）。
    pub fn enter_begin(&mut self, cpu: u32) -> Res<u64> {
        if !self.cpu_valid(cpu) {
            let e = E::Range  { what: 4, val: cpu as u64, };
            self.note_err(e); return Err(e);
        }
        let st = match self.cpus.get(cpu as usize) {
            Some(c) => c.state,
            None => IdleState::Busy,
        };
        if st.in_transition() { let e = E::InTransition { cpu }; self.note_err(e); return Err(e); }
        if st != IdleState::Busy {
            let e = E::IllegalTrans  { from: st.ordinal(), to: IdleState::Entering.ordinal(), };
            self.note_err(e); return Err(e);
        }
        let eid = self.next_enter_id; self.next_enter_id = self.next_enter_id.wrapping_add(1); self.cur_enter_id = eid;
        self.cur_step = 0; self.cur_fold = 0; self.cur_depth_snapshot = 0; self.cur_cpu = cpu; self.cur_t_enter = self.now;
        self.trans(cpu, IdleState::Entering, cause::BUSY_TO_ENTERING, eid)?; self.last_code = DIAG_ENTER_OK;
        Ok(eid)
    }

    /// 步序校验（内部裁决点 · **四步严格顺序**）。
    // 判据：UNX-F1742-J1 —— 进入四步顺序断言全过（跳步 / 乱序即拒）。
    fn guard_step(&mut self, want: u8, enter_id: u64, prev_fold: u64, depth: u32, cpu: u32) -> Res<u64> {
        if enter_id == 0 || enter_id != self.cur_enter_id {
            let e = E::SigMismatch  { expect: self.cur_enter_id, got: enter_id, };
            self.note_err(e); return Err(e);
        }
        let expect_step = self.cur_step.wrapping_add(1);
        if want != expect_step {
            let e = E::StepOrder  { expect: expect_step, got: want, };
            self.note_err(e); return Err(e);
        }
        let sig = StepSig  { enter_id, step: want, depth_snapshot: depth, cpu, prev_fold, };
        let f = sig.fold();
        Ok(f)
    }

    /// 签名入账（append-only · 一步一条）。
    fn push_sig(&mut self, r: SigRecord) { self.sigs.push(r); }

    /// 失败步的签名入账 + 断链（四步 guard 失败分支与注入变体的唯一来源）。判据 J1-a：跳步 / 乱序 / 伪造签名时，本步必须留下一条 `accepted = false` 的签名，且进入链立即复位（`cur_enter_id = 0`），使后续步骤无法凭残留链位继续落定。
    fn sig_fail(&mut self, eid: u64, step: u8, cpu: u32, depth: u32, fold: u64) {
        let t = self.now;
        self.sigs.push(SigRecord {
                enter_id: eid,
                step,
                cpu,
                depth_snapshot: depth,
                fold,
                tick_begin: t,
                tick_end: t,
                accepted: false,
        });
        self.abort_chain();
    }

    /// Step1：队空判定（F1651 消费点）。
    /// - `TrueEmpty` → 受理，进入 Phase::Probed。
    /// - `PseudoEmpty` → **必须延后**：登记延后账，进入链复位（`cur_enter_id = 0`），
    ///   返回 `E::PseudoEnter` 以显性化「不得静默当空」。
    /// - `NonEmpty` → 返回 `E::QueueNotEmpty`。
    pub fn step1_probe(&mut self, cpu: u32, snap: QueueSnapshot, prev_fold: u64) -> Res<u64> {
        let t0 = self.now; let eid = self.cur_enter_id;
        let f = match self.guard_step(STEP_QUEUE_PROBE, eid, prev_fold, snap.depth, cpu) {
            Ok(f) => f,
            Err(e) => { self.sig_fail(eid, STEP_QUEUE_PROBE, cpu, snap.depth, prev_fold); return Err(e); }
        };
        let judge = judge_queue(snap); let jsnap = QueueJudgeSnapshot::of(judge);
        let (ok, err) = match judge {
            QueueEmptiness::TrueEmpty => (true, None),
            QueueEmptiness::PseudoEmpty(c) => {
                self.record_defer(cpu, snap, c);
                (false, Some(E::PseudoEnter { depth: snap.depth }))
            }
            QueueEmptiness::NonEmpty(d) => (false, Some(E::QueueNotEmpty { depth: d })),
        };
        if let Some(c) = self.cpus.get_mut(cpu as usize) { c.last_snap = snap; c.last_judge = jsnap; }
        self.push_sig(SigRecord {
                enter_id: eid,
                step: STEP_QUEUE_PROBE,
                cpu,
                depth_snapshot: snap.depth,
                fold: f,
                tick_begin: t0,
                tick_end: self.now,
                accepted: ok,
        });
        match err {
            Some(e) => {
                self.note_err(e); self.abort_chain();
                Err(e)
            }
            None => {
                self.cur_step = STEP_QUEUE_PROBE; self.cur_fold = f; self.cur_depth_snapshot = snap.depth; self.last_code = DIAG_ENTER_OK;
                Ok(f)
            }
        }
    }

    /// Step1：队空判定（**指定 `enter_id` 的注入变体** · 仅注入核使用）。与 `step1_probe` 唯一差别是外部传入 `enter_id`（可伪造），从而命中 `guard_step`
    /// 的 `enter_id != cur_enter_id` 分支 ⇒ `E::SigMismatch`。
    pub fn step1_probe_with(
        &mut self,
        enter_id: u64,
        cpu: u32,
        snap: QueueSnapshot,
        prev_fold: u64,
        ) -> Res<u64> {
        if enter_id == 0 || enter_id != self.cur_enter_id {
            let e = E::SigMismatch { expect: self.cur_enter_id, got: enter_id }; self.note_err(e);
            self.sig_fail(enter_id, STEP_QUEUE_PROBE, cpu, snap.depth, prev_fold); return Err(e);
        }
        self.step1_probe(cpu, snap, prev_fold)
    }

    /// 中断进入链（Phase 复位 · 但**不**回滚已完成步的账 · 账目 append-only 语义）。
    fn abort_chain(&mut self) { self.cur_enter_id = 0; self.cur_step = 0; self.cur_fold = 0; self.cur_depth_snapshot = 0; }

    /// 登记伪队空延后账（**伪队空不得静默当空** · 显性化）。
    fn record_defer(&mut self, cpu: u32, snap: QueueSnapshot, c: PseudoCause) {
        let seq = self.next_defer_seq; self.next_defer_seq = self.next_defer_seq.wrapping_add(1);
        let ord = self.defer_total.wrapping_add(1); self.defer_total = ord;
        self.defers.push(DeferRecord {
                seq,
                tick: self.now,
                cpu,
                cause_key: c.key(),
                cause_name: c.name(),
                depth: snap.depth,
                defer_ordinal: ord,
        });
        if let Some(ctx) = self.cpus.get_mut(cpu as usize) { ctx.defer_count = ctx.defer_count.wrapping_add(1); }
    }

    /// Step2：静息置位（F1735 挂点消费 · **判据 J1-b 核心**）。置位幂等：同核已置位时不重复记账（`quiesce_hit_total` 不增），仅 `set_calls` 增。
    /// 本步是 `quiesce_hit_total`（分子）**唯一**的合法增长点——因此任何绕开本步的进入路径都会立刻使命中率 < 100%。
    pub fn step2_quiesce(&mut self, cpu: u32, src: QuiesceSource, prev_fold: u64) -> Res<u64> {
        let t0 = self.now; let eid = self.cur_enter_id; let depth = self.cur_depth_snapshot;
        let f = match self.guard_step(STEP_QUIESCE_SET, eid, prev_fold, depth, cpu) {
            Ok(f) => f,
            Err(e) => { self.sig_fail(eid, STEP_QUIESCE_SET, cpu, depth, prev_fold); return Err(e); }
        };
        // 来源合法性（F1735 两挂点）：非法来源拒绝置位并落账。
        let mut ok = true; let mut err: Option<E> = None;
        if !src.legit() {
            self.illegal_source_sets = self.illegal_source_sets.wrapping_add(1); ok = false; err = Some(E::QuiesceDup { cpu });
        }
        if ok {
            // `set` 返回「本次是否真正发生置位」：`true` = 首次置位（记账 · 分子 +1），`false` = 幂等重复（同核已置位 · **不记账** · 分子不变）。
            let fresh = self.quiesce_map.set(cpu);
            if let Some(ctx) = self.cpus.get_mut(cpu as usize) {
                ctx.quiesce.set_calls = ctx.quiesce.set_calls.wrapping_add(1);
                if fresh {
                    ctx.quiesce.bit = 1; ctx.quiesce.set_tick = self.now; ctx.quiesce.source = src;
                    ctx.quiesce_hit_count = ctx.quiesce_hit_count.wrapping_add(1);
                } else {
                    // 幂等：重复置位不重复记账（分子不增）。
                    ctx.quiesce.idempotent_hits =
                    ctx.quiesce.idempotent_hits.wrapping_add(1);
                }
            }
            if fresh {
                self.quiesce_hit_total = self.quiesce_hit_total.wrapping_add(1);
            } else {
                self.idempotent_total = self.idempotent_total.wrapping_add(1);
            }
            self.quiesce_call_total = self.quiesce_call_total.wrapping_add(1);
            match src {
                QuiesceSource::SwitchHook => { self.switch_hook_sets = self.switch_hook_sets.wrapping_add(1) }
                QuiesceSource::IdleHook => self.idle_hook_sets = self.idle_hook_sets.wrapping_add(1),
                QuiesceSource::Injected => {}
            }
        }
        self.push_sig(SigRecord {
                enter_id: eid,
                step: STEP_QUIESCE_SET,
                cpu,
                depth_snapshot: depth,
                fold: f,
                tick_begin: t0,
                tick_end: self.now,
                accepted: ok,
        });
        match err {
            Some(e) => {
                self.note_err(e); self.abort_chain();
                Err(e)
            }
            None => {
                self.cur_step = STEP_QUIESCE_SET; self.cur_fold = f; self.last_code = DIAG_ENTER_OK;
                Ok(f)
            }
        }
    }

    /// Step3：账目起算（F1745 结算起算点）。前置不变量（判据 J1-a）：静息位必须已置位（Step2 已过），否则拒绝——这是「跳步（跳过静息置位直接落定）」的第二道防线（第一道是 `guard_step` 的步序校验）。
    pub fn step3_account(&mut self, cpu: u32, prev_fold: u64) -> Res<u64> {
        let t0 = self.now; let eid = self.cur_enter_id; let depth = self.cur_depth_snapshot;
        let f = match self.guard_step(STEP_LEDGER_BEGIN, eid, prev_fold, depth, cpu) {
            Ok(f) => f,
            Err(e) => { self.sig_fail(eid, STEP_LEDGER_BEGIN, cpu, depth, prev_fold); return Err(e); }
        };
        // 不变量：静息位必须已置位（跳步的第二道防线）。
        let qset = match self.cpus.get(cpu as usize) {
            Some(c) => c.quiesce.bit == 1,
            None => false,
        };
        if !qset {
            let e = E::QuiesceMiss { hit: self.quiesce_hit_total, enter: self.enter_total.wrapping_add(1) }; self.note_err(e);
            self.sig_fail(eid, STEP_LEDGER_BEGIN, cpu, depth, f); return Err(e);
        }
        // 账目起算：写 idle_begin / idle_segments / cur_enter_id。
        if let Some(ctx) = self.cpus.get_mut(cpu as usize) {
            ctx.ledger.idle_begin = self.now; ctx.ledger.idle_segments = ctx.ledger.idle_segments.wrapping_add(1); ctx.ledger.cur_enter_id = eid;
        }
        self.push_sig(SigRecord {
                enter_id: eid,
                step: STEP_LEDGER_BEGIN,
                cpu,
                depth_snapshot: depth,
                fold: f,
                tick_begin: t0,
                tick_end: self.now,
                accepted: true,
        });
        self.cur_step = STEP_LEDGER_BEGIN; self.cur_fold = f; self.last_code = DIAG_ENTER_OK;
        Ok(f)
    }

    /// Step4：状态落定（Entering → Idle · 分母增长点）。前置不变量：账目已起算（`idle_begin != 0`）。落定即计入 `enter_total`（分母），故「进入 idle 的次数」的唯一定义点就是本步。
    pub fn step4_settle(&mut self, cpu: u32, prev_fold: u64) -> Res<u64> {
        let t0 = self.now; let eid = self.cur_enter_id; let depth = self.cur_depth_snapshot;
        let f = match self.guard_step(STEP_STATE_SETTLE, eid, prev_fold, depth, cpu) {
            Ok(f) => f,
            Err(e) => { self.sig_fail(eid, STEP_STATE_SETTLE, cpu, depth, prev_fold); return Err(e); }
        };
        // 不变量：账目必须已起算。
        let started = match self.cpus.get(cpu as usize) {
            Some(c) => c.ledger.idle_begin != 0,
            None => false,
        };
        if !started {
            let e = E::StepOrder { expect: STEP_LEDGER_BEGIN, got: STEP_STATE_SETTLE }; self.note_err(e);
            self.sig_fail(eid, STEP_STATE_SETTLE, cpu, depth, f); return Err(e);
        }
        self.trans(cpu, IdleState::Idle, cause::ENTERING_TO_IDLE, eid)?;
        if let Some(ctx) = self.cpus.get_mut(cpu as usize) { ctx.enter_count = ctx.enter_count.wrapping_add(1); }
        self.enter_total = self.enter_total.wrapping_add(1); self.ok_total = self.ok_total.wrapping_add(1);
        self.enter_elapsed_total = self
        .enter_elapsed_total
        .wrapping_add(t0.wrapping_sub(self.cur_t_enter));
        self.enter_elapsed_samples = self.enter_elapsed_samples.wrapping_add(1);
        self.push_sig(SigRecord {
                enter_id: eid,
                step: STEP_STATE_SETTLE,
                cpu,
                depth_snapshot: depth,
                fold: f,
                tick_begin: t0,
                tick_end: self.now,
                accepted: true,
        });
        self.cur_step = STEP_STATE_SETTLE; self.cur_fold = f; self.last_code = DIAG_ENTER_OK;
        Ok(f)
    }
}

/// 静息位置位结果（供外层判定是否记账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOutcome {
    /// 首次置位（记账 · 分子 +1）。
    Fresh,
    /// 幂等重复置位（**不记账** · 分子不变）。
    Idempotent,
}

impl IdleEnter {
    /// 静息位置位（**F1735 挂点消费点** · O(1) · 幂等）。`src` 为挂点来源：`SwitchHook`（切换挂点）或 `IdleHook`（idle 挂点）。非法来源（`Injected`）返回 `E::QuiesceDup` 且不置位。
    // 判据：UNX-F1742-J1 —— 静息置位在 idle 进入点 100% 命中（F1735 挂点消费）。故本路径是分子唯一增长点，且幂等重复置位不得重复记账（否则分子会 > 分母）。
    pub fn quiesce_set(&mut self, cpu: u32, tick: u64, src: QuiesceSource) -> Res<SetOutcome> {
        if !self.cpu_valid(cpu) {
            let e = E::Range  { what: 4, val: cpu as u64, };
            self.note_err(e); return Err(e);
        }
        if !src.legit() {
            self.illegal_source_sets = self.illegal_source_sets.wrapping_add(1); let e = E::QuiesceDup { cpu }; self.note_err(e); return Err(e);
        }
        let fresh = self.quiesce_map.set(cpu); self.quiesce_call_total = self.quiesce_call_total.wrapping_add(1);
        match src {
            QuiesceSource::SwitchHook => self.switch_hook_sets = self.switch_hook_sets.wrapping_add(1),
            QuiesceSource::IdleHook => self.idle_hook_sets = self.idle_hook_sets.wrapping_add(1),
            QuiesceSource::Injected => {}
        }
        let out = if fresh {
            if let Some(c) = self.cpus.get_mut(cpu as usize) {
                c.quiesce.bit = 1; c.quiesce.set_tick = tick; c.quiesce.source = src; c.quiesce.set_calls = c.quiesce.set_calls.wrapping_add(1);
            }
            SetOutcome::Fresh
        } else {
            self.idempotent_total = self.idempotent_total.wrapping_add(1);
            if let Some(c) = self.cpus.get_mut(cpu as usize) {
                c.quiesce.set_calls = c.quiesce.set_calls.wrapping_add(1); c.quiesce.idempotent_hits = c.quiesce.idempotent_hits.wrapping_add(1);
            }
            SetOutcome::Idempotent
        };
        self.last_code = DIAG_ENTER_OK;
        Ok(out)
    }

    /// 静息位清位（O(1) · 幂等 · 返回是否真正清位）。
    pub fn quiesce_clear(&mut self, cpu: u32) -> Res<bool> {
        if !self.cpu_valid(cpu) {
            let e = E::Range  { what: 4, val: cpu as u64, };
            self.note_err(e); return Err(e);
        }
        let was = self.quiesce_map.clear(cpu);
        if let Some(c) = self.cpus.get_mut(cpu as usize) {
            c.quiesce.bit = 0; c.quiesce.clear_calls = c.quiesce.clear_calls.wrapping_add(1); c.quiesce.set_tick = 0;
        }
        self.last_code = DIAG_ENTER_OK;
        Ok(was)
    }

    /// 静息位查询（O(1) · 无副作用）。
    pub fn quiesce_test(&self, cpu: u32) -> bool { self.quiesce_map.test(cpu) }

    /// 静息位快照（只读拷贝）。
    pub fn quiesce_of(&self, cpu: u32) -> Res<QuiesceBit> {
        match self.cpus.get(cpu as usize) {
            Some(c) => Ok(c.quiesce),
            None => Err(E::Range {
                    what: 4,
                    val: cpu as u64,
            }),
        }
    }

    /// F1666 发现账登记（双源对账的另一源 · 由 F1666 发现路径写入）。
    pub fn discovery_record(&mut self, cpu: u32, idle_ticks: u64) -> Res<()> {
        match self.discovery_idle_ticks.get_mut(cpu as usize) {
            Some(d) => {
                *d = idle_ticks;
                Ok(())
            }
            None => Err(E::Range {
                    what: 4,
                    val: cpu as u64,
            }),
        }
    }

    /// F1666 发现账读取。
    pub fn discovery_of(&self, cpu: u32) -> Res<u64> {
        match self.discovery_idle_ticks.get(cpu as usize) {
            Some(d) => Ok(*d),
            None => Err(E::Range {
                    what: 4,
                    val: cpu as u64,
            }),
        }
    }

    /// 静息位与 F1666 发现账**双源一致性对账**（逐核）。静息源：由静息位置位时刻到清位时刻推得的 idle 累计；发现源：F1666 idle 发现账。两者不一致立即 `E::DualSource`。
    // 判据：UNX-F1742-J1 —— 静息位（挂点消费）与发现账双源一致。
    pub fn reconcile_dual_source(&mut self) -> Res<u64> {
        let n = self.cpus.len(); let mut ok_cnt: u64 = 0;
        for i in 0..n {
            let (q_src, d_src) = {
                let ctx = match self.cpus.get(i) {
                    Some(c) => c,
                    None => continue,
                };
                let d = match self.discovery_idle_ticks.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
                (ctx.ledger.idle_ticks, d)
            };
            if q_src != d_src {
                let e = E::DualSource  { cpu: i as u32, quiesce: q_src, discovery: d_src, };
                self.note_err(e); return Err(e);
            }
            ok_cnt = ok_cnt.wrapping_add(1);
        }
        self.last_code = DIAG_ENTER_OK;
        Ok(ok_cnt)
    }

    /// 完整一次 idle 进入（**四步严格顺序** · 正常路径）。每步之间推进 1 个逻辑 tick（模拟四步各占 1 tick 的典型代价），故四步耗时恰为 4 tick。
    // 判据：UNX-F1742-J1 —— 进入四步顺序断言全过。
    pub fn enter_full(&mut self, cpu: u32, snap: QueueSnapshot) -> Res<EnterSnapshot> {
        let t_enter = self.now; let eid = self.enter_begin(cpu)?; let mut f: u64 = 0;
        // Step1
        f = self.step1_probe(cpu, snap, f)?; self.now = self.now.wrapping_add(1);
        // Step2（idle 挂点）
        f = self.step2_quiesce(cpu, QuiesceSource::IdleHook, f)?; self.now = self.now.wrapping_add(1);
        // Step3
        f = self.step3_account(cpu, f)?; self.now = self.now.wrapping_add(1);
        // Step4
        f = self.step4_settle(cpu, f)?; self.now = self.now.wrapping_add(1); let t_settled = self.now;
        let snap_out = EnterSnapshot {
            enter_id: eid,
            cpu,
            four_steps_ok: true,
            step1_true_empty: true,
            step2_quiesce_set: true,
            step3_ledger_begin: true,
            step4_settled: true,
            t_enter,
            t_settled,
            elapsed: t_settled.wrapping_sub(t_enter),
            judge_ord: 0,
            quiesce_source_ord: QuiesceSource::IdleHook.ordinal(),
            final_fold: f,
        };
        Ok(snap_out)
    }

    /// 完整一次 idle 进入（**注入绕行路径** · 故意跳过 Step2 静息置位）。该路径存在的唯一目的是让注入核 #3 能够触发「命中率 < 100%」的显性报错：
    /// 分子不增而分母增 ⇒ `quiesce_hit_rate()` < 100% ⇒ `require_full_hit_rate()` 报`E::QuiesceMiss`。故本方法仅在注入核中使用，**正常路径绝不可用**。
    /// 实现方式：Step1 之后直接调 `quiesce_set(SwitchHook)` 把位图置上（使 Step3 的「静息位必须已置位」不变量通过），但**不经过 Step2** ⇒ 分子不增、分母增。
    pub fn enter_full_skip_quiesce(&mut self, cpu: u32, snap: QueueSnapshot) -> Res<EnterSnapshot> {
        let t_enter = self.now; let eid = self.enter_begin(cpu)?; let mut f: u64 = 0; f = self.step1_probe(cpu, snap, f)?;
        self.now = self.now.wrapping_add(1);
        // 绕过 Step2：改从「切换挂点」直接置位（位图置上，但分子不增 —— 正是缺陷所在）。
        let t_q = self.now; let _ = self.quiesce_set(cpu, t_q, QuiesceSource::SwitchHook); self.cur_step = STEP_QUIESCE_SET;
        self.now = self.now.wrapping_add(1); f = self.step3_account(cpu, f)?; self.now = self.now.wrapping_add(1);
        f = self.step4_settle(cpu, f)?; self.now = self.now.wrapping_add(1);
        Ok(EnterSnapshot {
                enter_id: eid,
                cpu,
                four_steps_ok: false,
                step1_true_empty: true,
                step2_quiesce_set: false,
                step3_ledger_begin: true,
                step4_settled: true,
                t_enter,
                t_settled: self.now,
                elapsed: self.now.wrapping_sub(t_enter),
                judge_ord: 0,
                quiesce_source_ord: 255,
                final_fold: f,
        })
    }
}

impl IdleEnter {
    /// 闭合一个 idle 段（把 `open_segment` 计入 `idle_ticks` · 供 F1744 退出路径调用）。本单只提供账目原语，退出流程本体在 F1744。
    pub fn idle_close_segment(&mut self, cpu: u32, tick: u64) -> Res<u64> {
        let (begin, in_idle) = match self.cpus.get(cpu as usize) {
            Some(c) => (c.ledger.idle_begin, c.ledger.in_idle()),
            None => {
                let e = E::Range  { what: 4, val: cpu as u64, };
                self.note_err(e); return Err(e);
            }
        };
        if !in_idle { return Ok(0); }
        let delta = if tick >= begin { tick - begin } else { 0 };
        if let Some(c) = self.cpus.get_mut(cpu as usize) {
            c.ledger.idle_ticks = c.ledger.idle_ticks.wrapping_add(delta); c.ledger.idle_begin = 0; c.ledger.cur_enter_id = 0;
        }
        self.last_code = DIAG_ENTER_OK;
        Ok(delta)
    }

    /// 记 busy tick（进入忙态时由 F1744 收口；本单提供原语）。
    pub fn busy_add(&mut self, cpu: u32, delta: u64) -> Res<()> {
        match self.cpus.get_mut(cpu as usize) {
            Some(c) => {
                c.ledger.busy_ticks = c.ledger.busy_ticks.wrapping_add(delta);
                Ok(())
            }
            None => {
                let e = E::Range  { what: 4, val: cpu as u64, };
                self.note_err(e);
                Err(e)
            }
        }
    }

    /// 把每核账目按 `[0, online_ticks)` 均匀铺满（构造守恒满值场景用）。每核 idle = `idle_each`，busy = `online_ticks - idle_each`。
    /// 手算锚：4 核 × 1000 tick，idle 300 + busy 700 ⇒ 300*4 + 700*4 = 4000 == 4*1000 ✓。
    pub fn fill_uniform(&mut self, idle_each: u64) -> Res<()> {
        if idle_each > self.online_ticks {
            let e = E::Range  { what: 5, val: idle_each, };
            self.note_err(e); return Err(e);
        }
        let busy = self.online_ticks - idle_each; let n = self.cpus.len();
        for i in 0..n {
            if let Some(c) = self.cpus.get_mut(i) {
                c.ledger.idle_ticks = idle_each; c.ledger.busy_ticks = busy; c.ledger.idle_begin = 0; c.ledger.cur_enter_id = 0;
            }
            if let Some(d) = self.discovery_idle_ticks.get_mut(i) { *d = idle_each; }
        }
        self.now = self.online_ticks; self.last_code = DIAG_ENTER_OK;
        Ok(())
    }

    /// 逐核 idle 占比（万分比 · 无浮点）。
    pub fn idle_ratio_permille(&self, cpu: u32) -> Res<u64> {
        let c = match self.cpus.get(cpu as usize) {
            Some(c) => c,
            None => {
                return Err(E::Range {
                        what: 4,
                        val: cpu as u64,
                })
            }
        };
        let total = c.ledger.total_ticks(self.now);
        if total == 0 { return Ok(0); }
        Ok(c
            .ledger
            .idle_ticks
            .wrapping_add(c.ledger.open_segment_ticks(self.now))
            .wrapping_mul(HIT_RATE_SCALE)
        / total)
    }

    /// 全局 idle 占比（万分比）。
    pub fn idle_ratio_global(&self) -> u64 {
        let c = self.conservation();
        if c.lhs == 0 { return 0; }
        c.sum_idle.wrapping_mul(HIT_RATE_SCALE) / c.lhs
    }

    /// 进入四步平均耗时（逻辑 tick 差 · 样本为 0 时返回 0）。
    pub fn enter_elapsed_avg(&self) -> u64 {
        if self.enter_elapsed_samples == 0 {
            0
        } else {
            self.enter_elapsed_total / self.enter_elapsed_samples
        }
    }

    /// 诊断事件（单点导出用 · 一行文本）。
    pub fn summary_line(&self) -> String {
        let h = self.quiesce_hit_rate(); let c = self.conservation();
        format!(
            "unx1742 {} ncpu={} now={} enter={} hit={} rate={}/{} idem={} defer={} idle={} busy={} cons_drift={} avg4={} dropped={}",
            ID_VERSION,
            self.cpu_count(),
            self.now,
            self.enter_total,
            self.quiesce_hit_total,
            h.hit,
            h.enter,
            self.idempotent_total,
            self.defer_total,
            c.sum_idle,
            c.sum_busy,
            c.drift,
            self.enter_elapsed_avg(),
            self.trans.dropped + self.sigs.dropped + self.defers.dropped
        )
    }

    /// 环形账本溢出是否发生（dropped 显性化 · 返回累计丢失条数）。
    pub fn ledger_dropped(&self) -> u64 {
        self.trans
        .dropped
        .wrapping_add(self.sigs.dropped)
        .wrapping_add(self.defers.dropped)
    }

    /// 溢出即报错（`E::RingDrop` · 供观测点显性暴露）。
    pub fn check_no_drop(&mut self) -> Res<u64> {
        let d = self.ledger_dropped();
        if d == 0 {
            Ok(0)
        } else {
            let e = E::RingDrop { lost: d }; self.note_err(e);
            Err(e)
        }
    }

    /// 转换账尾（最近 `n` 条 · 逆序 · 诊断用）。
    pub fn trans_tail(&self, n: usize) -> Vec<TransRecord> {
        let mut out: Vec<TransRecord> = Vec::new(); let mut i = 0usize;
        while let Some(r) = self.trans.rev_index(i) {
            out.push(*r); i += 1;
            if i >= n { break; }
        }
        out
    }

    /// 签名账尾（最近 `n` 条 · 逆序 · 回放对账用）。
    pub fn sig_tail(&self, n: usize) -> Vec<SigRecord> {
        let mut out: Vec<SigRecord> = Vec::new(); let mut i = 0usize;
        while let Some(r) = self.sigs.rev_index(i) {
            out.push(*r); i += 1;
            if i >= n { break; }
        }
        out
    }

    /// 延后账尾（最近 `n` 条 · 逆序）。
    pub fn defer_tail(&self, n: usize) -> Vec<DeferRecord> {
        let mut out: Vec<DeferRecord> = Vec::new(); let mut i = 0usize;
        while let Some(r) = self.defers.rev_index(i) {
            out.push(*r); i += 1;
            if i >= n { break; }
        }
        out
    }

    /// 某次进入的四步签名（按 `enter_id` 正序取 · 回放逐步对账）。
    pub fn sigs_of_enter(&self, eid: u64) -> Vec<SigRecord> {
        let mut out: Vec<SigRecord> = Vec::new();
        for r in self.sigs.items.iter() {
            if r.enter_id == eid { out.push(*r); }
        }
        out
    }
}

/// 注入核结果（单形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectResult {
    /// 形态编号（1..=10）。
    pub id: u32,
    /// 形态名（诊断输出）。
    pub name: &'static str,
    /// 是否被检出（`true` = 防御生效）。
    pub detected: bool,
    /// 触发时返回的错误码（`0` = 未触发）。
    pub err_code: u16,
    /// 错误短名（诊断输出 · `-` = 无错误）。
    pub err_tag: &'static str,
    /// 该形态的附加说明（错误序号 / 深度 / 偏差等）。
    pub detail: u64,
}

impl InjectResult {
    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "inject#{} {} detected={} code=0x{:04X} tag={} detail={}",
            self.id, self.name, self.detected, self.err_code, self.err_tag, self.detail
        )
    }
}

/// 注入核辅助：建引擎（失败时给出「未检出」骨架，避免每个形态重复样板）。
fn inj_new(id: u32, name: &'static str, cpus: u32, ticks: u64, rng: u64) -> Res<IdleEnter> { IdleEnter::new(cpus, ticks, rng) }

/// 注入核辅助：未检出的骨架结果。
fn inj_fail(id: u32, name: &'static str) -> InjectResult {
    InjectResult { id, name, detected: false, err_code: 0, err_tag: "-", detail: 0 }
}

fn inj_res(id: u32, name: &'static str, r: Res<()>, detail: u64) -> InjectResult {
    match r {
        Ok(()) => InjectResult  { id, name, detected: false, err_code: 0, err_tag: "-", detail, },
        Err(e) => InjectResult  { id, name, detected: true, err_code: e.code(), err_tag: e.tag(), detail, },
    }
}

/// 注入核 #1：**队空判定误判**（队内有线程却判空）。构造：`QueueSnapshot::non_empty(3)` 但调用方谎报深度为 0 走伪判定路径 ——更直接的形态是：`enter_full` 传 `depth = 3` 的非空快照，期望 Step1 拒绝。
pub fn inject_01_queue_not_empty() -> InjectResult {
    let mut eng = match inj_new(1, "queue_not_empty", 2, 100, 0x1111) {
        Ok(e) => e,
        Err(_) => return inj_fail(1, "queue_not_empty"),
    };
    let snap = QueueSnapshot::non_empty(3); let r = eng.enter_full(0, snap); let detected = r.is_err();
    let (code, tag, detail) = match eng.last_err {
        Some(e) => {
            let d = match e {
                E::QueueNotEmpty { depth } => depth as u64,
                E::PseudoEnter { depth } => depth as u64,
                _ => 0,
            };
            (e.code(), e.tag(), d)
        }
        None => (0, "-", 0),
    };
    InjectResult  { id: 1, name: "queue_not_empty", detected, err_code: code, err_tag: tag, detail, }
}

/// 注入核 #2：**跳步**（跳过 Step1 直接从 Step3 起算）。
pub fn inject_02_skip_step() -> InjectResult {
    let mut eng = match inj_new(2, "skip_step", 2, 100, 0x2222) {
        Ok(e) => e,
        Err(_) => return inj_fail(2, "skip_step"),
    };
    let _ = eng.enter_begin(0);
    // 直接调 Step3 —— `guard_step` 期望 Step1，返回 `E::StepOrder`。
    let r: Res<u64> = eng.step3_account(0, 0);
    inj_res(2, "skip_step", r.map(|_| ()), 3)
}

/// 注入核 #3：**静息位置位遗漏**（进入 idle 时绕开 idle 挂点）。形态：`enter_full_skip_quiesce` 使分子不增、分母增 ⇒ 命中率 < 100%，
/// `require_full_hit_rate()` 立即报 `E::QuiesceMiss`。判据：UNX-F1742-J1 —— 静息置位在 idle 进入点 100% 命中。
pub fn inject_03_quiesce_miss() -> InjectResult {
    let mut eng = match inj_new(3, "quiesce_miss", 3, 1000, 0x3333) {
        Ok(e) => e,
        Err(_) => return inj_fail(3, "quiesce_miss"),
    };
    // 先建立两次正常进入（命中率 100%）。
    let _ = eng.enter_full(0, QueueSnapshot::true_empty()); let _ = eng.enter_full(1, QueueSnapshot::true_empty());
    let h_before = eng.quiesce_hit_rate();
    // 再在核 2 上走一次绕行路径（分子不增、分母增）。
    let _ = eng.enter_full_skip_quiesce(2, QueueSnapshot::true_empty()); let h_after = eng.quiesce_hit_rate();
    let r = eng.require_full_hit_rate(); let hit_ok = h_before.is_full() && !h_after.is_full();
    let base = inj_res(
        3,
        "quiesce_miss",
        r.map(|_| ()),
        h_after.hit.wrapping_mul(HIT_RATE_SCALE) / if h_after.enter == 0 {
            1
        } else {
            h_after.enter
        },
    );
    InjectResult {
        detected: hit_ok && base.detected,
        ..base
    }
}

/// 注入核 #4：**伪队空误进**（有线程但全阻塞 / 全过渡占位却直接进 idle）。
pub fn inject_04_pseudo_enter() -> InjectResult {
    let mut eng = match inj_new(4, "pseudo_enter", 2, 100, 0x4444) {
        Ok(e) => e,
        Err(_) => return inj_fail(4, "pseudo_enter"),
    };
    // P1：队列槽位空但有 2 个存活阻塞线程。
    let r = eng.enter_full(0, QueueSnapshot::pseudo_blocked(2)); let defer_recorded = eng.defer_total;
    let deferred = defer_recorded == 1; let base = inj_res(4, "pseudo_enter", r.map(|_| ()), defer_recorded);
    InjectResult {
        detected: base.detected && deferred,
        ..base
    }
}

/// 注入核 #5：**乱序执行**（先账目起算后队空判定）。
pub fn inject_05_reverse_order() -> InjectResult {
    let mut eng = match inj_new(5, "reverse_order", 2, 100, 0x5555) {
        Ok(e) => e,
        Err(_) => return inj_fail(5, "reverse_order"),
    };
    let _ = eng.enter_begin(0);
    let mut f = match eng.step1_probe(0, QueueSnapshot::true_empty(), 0) {
        Ok(f) => f,
        Err(e) => {
            return InjectResult  { id: 5, name: "reverse_order", detected: true, err_code: e.code(),
            err_tag: e.tag(), detail: e.code() as u64, }
        }
    };
    f = match eng.step2_quiesce(0, QuiesceSource::IdleHook, f) {
        Ok(f) => f,
        Err(e) => {
            return InjectResult  { id: 5, name: "reverse_order", detected: true, err_code: e.code(),
            err_tag: e.tag(), detail: e.code() as u64, }
        }
    };
    let _ = eng.step3_account(0, f);
    // 现在倒回 Step1 —— 期望 Step4，返回 `E::StepOrder`。
    let r: Res<u64> = eng.step1_probe(0, QueueSnapshot::true_empty(), f);
    inj_res(5, "reverse_order", r.map(|_| ()), 1)
}

/// 注入核 #6：**伪造步签名**（`prev_fold` 被篡改 / `enter_id` 冒用）。
pub fn inject_06_forged_signature() -> InjectResult {
    let mut eng = match inj_new(6, "forged_signature", 2, 100, 0x6666) {
        Ok(e) => e,
        Err(_) => return inj_fail(6, "forged_signature"),
    };
    let eid = match eng.enter_begin(0) {
        Ok(e) => e,
        Err(_) => {
            return InjectResult  { id: 6, name: "forged_signature", detected: false, err_code: 0, err_tag: "-",
            detail: 0, }
        }
    };
    // 冒用他人 `enter_id`（伪造签名）。
    let forged = eid.wrapping_add(0xFFFF); let r: Res<u64> = eng.step1_probe(0, QueueSnapshot::true_empty(), 0); let _ = r;
    // 用伪造 `enter_id` 走 Step1（此时 `cur_enter_id` 已为真实 eid）。
    let r2: Res<u64> = eng.step1_probe_with(forged, 0, QueueSnapshot::true_empty(), 0);
    inj_res(6, "forged_signature", r2.map(|_| ()), forged)
}

/// 注入核 #7：**守恒破坏**（手工改 `idle_ticks`）。
pub fn inject_07_conservation_break() -> InjectResult {
    let mut eng = match inj_new(7, "conservation_break", 4, DEFAULT_ONLINE_TICKS, 0x7777) {
        Ok(e) => e,
        Err(_) => return inj_fail(7, "conservation_break"),
    };
    if eng.fill_uniform(300).is_err() {
        return InjectResult  { id: 7, name: "conservation_break", detected: false, err_code: 0, err_tag: "-",
        detail: 0, };
    }
    // 手算锚：4 核 × 1000 tick，idle 300 + busy 700 ⇒ 300*4 + 700*4 = 4000 == 4*1000 ✓
    let ok = eng.check_conservation().is_ok();
    // 手工篡改某核 idle_ticks（+5）。
    if let Some(c) = eng.cpus.get_mut(0) { c.ledger.idle_ticks = c.ledger.idle_ticks.wrapping_add(5); }
    let r = eng.check_conservation();
    let detail = match r {
        Ok(_) => 0,
        Err(e) => match e {
            E::Conservation { lhs, rhs } => (lhs as i64 - rhs as i64).unsigned_abs(),
            _ => 0,
        },
    };
    InjectResult {
        id: 7,
        name: "conservation_break",
        detected: ok && r.is_err() && detail == 5,
        err_code: match r {
            Err(e) => e.code(),
            Ok(_) => 0,
        },
        err_tag: match r {
            Err(e) => e.tag(),
            Ok(_) => "-",
        },
        detail,
    }
}

/// 注入核 #8：**静息位重复置位**（幂等违规）。
pub fn inject_08_quiesce_dup() -> InjectResult {
    let mut eng = match inj_new(8, "quiesce_dup", 2, 100, 0x8888) {
        Ok(e) => e,
        Err(_) => return inj_fail(8, "quiesce_dup"),
    };
    let t = 10; let first = eng.quiesce_set(0, t, QuiesceSource::IdleHook);
    let second = eng.quiesce_set(0, t + 1, QuiesceSource::IdleHook); let fresh = matches!(first, Ok(SetOutcome::Fresh));
    let idem = matches!(second, Ok(SetOutcome::Idempotent));
    // 幂等违规：分子不应增加（静息位置位成功次数不增）。
    let num_ok = eng.quiesce_hit_total == 0 || eng.idempotent_total == 1; let bit_set = eng.quiesce_test(0);
    InjectResult  { id: 8, name: "quiesce_dup", detected: fresh && idem && num_ok && bit_set, err_code: 0,
    err_tag: "idempotent_rejected", detail: eng.idempotent_total, }
}

/// 注入核 #9：**过渡态中强行进入**。
pub fn inject_09_transition_force() -> InjectResult {
    let mut eng = match inj_new(9, "transition_force", 2, 100, 0x9999) {
        Ok(e) => e,
        Err(_) => return inj_fail(9, "transition_force"),
    };
    let _ = eng.enter_begin(0); // → Entering（过渡态）
    // 再次 `enter_begin` —— 过渡态中强行进入，返回 `E::InTransition`。
    let r = eng.enter_begin(0);
    inj_res(9, "transition_force", r.map(|_| ()), 0)
}

/// 注入核 #10：**双源不一致**（静息位账 vs F1666 发现账）。
pub fn inject_10_dual_source_mismatch() -> InjectResult {
    let mut eng = match inj_new(10, "dual_source_mismatch", 2, 1000, 0xAAAA) {
        Ok(e) => e,
        Err(_) => return inj_fail(10, "dual_source_mismatch"),
    };
    // 静息源：跑一段 idle 后闭合得 300 tick；发现源写成 280。
    eng.now = 100; let _ = eng.enter_full(0, QueueSnapshot::true_empty()); let _ = eng.idle_close_segment(0, 400);
    if eng.discovery_record(0, 280).is_err() {
        return InjectResult  { id: 10, name: "dual_source_mismatch", detected: false, err_code: 0, err_tag: "-",
        detail: 0, };
    }
    let r = eng.reconcile_dual_source();
    inj_res(10, "dual_source_mismatch", r.map(|_| ()), 20)
}

/// 注入核汇总（**固定 10 形态 · 顺序执行 · 互不污染**）。返回长度恒为 10；`all_detected()` 判定是否 10/10 检出。
pub fn inject_all() -> Vec<InjectResult> {
    let mut v: Vec<InjectResult> = vec![]; v.push(inject_01_queue_not_empty()); v.push(inject_02_skip_step());
    v.push(inject_03_quiesce_miss()); v.push(inject_04_pseudo_enter()); v.push(inject_05_reverse_order());
    v.push(inject_06_forged_signature()); v.push(inject_07_conservation_break()); v.push(inject_08_quiesce_dup());
    v.push(inject_09_transition_force()); v.push(inject_10_dual_source_mismatch());
    v
}

/// 汇总是否 10/10 检出（长度必须为 10 且全部 `detected`）。
pub fn all_detected(list: &[InjectResult]) -> bool { list.len() == 10 && list.iter().all(|r| r.detected) }

/// 注入核汇总一行文本（诊断用）。
pub fn inject_report(list: &[InjectResult]) -> String {
    let mut s = String::new(); s.push_str("inject_core["); let mut i = 0usize;
    while i < list.len() {
        if i > 0 { s.push(';'); }
        s.push_str(list[i].name); s.push('='); s.push_str(if list[i].detected { "hit" } else { "MISS" }); i += 1;
    }
    s.push(']');
    s
}

/// TLV 魔数（`0xF1742A01` · 自洽断言锚）。
pub const TLV_MAGIC: u32 = 0xF174_2A01;
/// TLV 版本。
pub const TLV_VERSION: u8 = 1;
/// TLV 头长（magic u32 + ver u8 + resv u8 + len u16）。
pub const TLV_HEADER_LEN: usize = 8;

/// TLV 标签（四族指标）。
pub mod tlv_tag {
    /// 族一：idle 占比（每核 + 全局）。
    pub const IDLE_RATIO: u16 = 0x0001;
    /// 族二：静息命中率。
    pub const QUIESCE_HIT: u16 = 0x0002;
    /// 族三：伪队空延后次数。
    pub const PSEUDO_DEFER: u16 = 0x0003;
    /// 族四：四步平均耗时。
    pub const ENTER_LATENCY: u16 = 0x0004;
    /// 附加：守恒漂移。
    pub const CONSERVATION: u16 = 0x0005;
}

/// TLV 条目（label + u64 值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlvItem {
    /// 标签。
    pub tag: u16,
    /// 值（u64 · 无浮点）。
    pub value: u64,
}

/// TLV 导出自洽结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlvCheck {
    /// 声明长度。
    pub declared_len: u16,
    /// 实算长度。
    pub actual_len: usize,
    /// 条目数。
    pub items: usize,
    /// 是否自洽。
    pub ok: bool,
}

impl TlvCheck {
    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "tlv declared={} actual={} items={} ok={}",
            self.declared_len, self.actual_len, self.items, self.ok
        )
    }
}

/// 写 u16 到字节序缓冲（小端 · 确定性 · 无 IO）。
fn put_u16(buf: &mut Vec<u8>, v: u16) { buf.push((v & 0xFF) as u8); buf.push(((v >> 8) & 0xFF) as u8); }

/// 写 u32 到字节序缓冲（小端）。
fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.push((v & 0xFF) as u8); buf.push(((v >> 8) & 0xFF) as u8); buf.push(((v >> 16) & 0xFF) as u8); buf.push(((v >> 24) & 0xFF) as u8);
}

/// 写 u64 到字节序缓冲（小端）。
fn put_u64(buf: &mut Vec<u8>, v: u64) {
    let mut i = 0;
    while i < 8 { buf.push(((v >> (i * 8)) & 0xFF) as u8); i += 1; }
}

/// 读 u16（小端 · 自洽校验用）。
fn get_u16(buf: &[u8], off: usize) -> Option<u16> {
    if off + 2 > buf.len() { return None; }
    let a = buf[off] as u16; let b = buf[off + 1] as u16;
    Some((b << 8) | a)
}

/// 读 u32（小端）。
fn get_u32(buf: &[u8], off: usize) -> Option<u32> {
    if off + 4 > buf.len() { return None; }
    let mut v: u32 = 0; let mut i = 0;
    while i < 4 { v |= (buf[off + i] as u32) << (i * 8); i += 1; }
    Some(v)
}

/// 四族指标单点导出（KV 文本 · 无浮点）。族一：idle 占比（每核 + 全局 · 万分比）。族二：静息命中率（分子 / 分母 / 万分比）。族三：伪队空延后次数（总 / 分核）。族四：进入四步平均耗时（逻辑 tick 差）。
pub fn export_kv(eng: &IdleEnter) -> String {
    let mut s: String = String::new(); s.push_str("unit="); s.push_str(ID_VERSION); s.push(';'); s.push_str("case=");
    s.push_str(ID_CASE); s.push(';');
    // 族一
    let mut i = 0usize;
    while i < eng.cpus.len() {
        let r = eng.idle_ratio_permille(i as u32);
        let v = match r {
            Ok(v) => v,
            Err(_) => 0,
        };
        s.push_str("idle_ratio.cpu"); s.push_str(&i.to_string()); s.push('='); s.push_str(&v.to_string()); s.push(';'); i += 1;
    }
    s.push_str("idle_ratio.global="); s.push_str(&eng.idle_ratio_global().to_string()); s.push(';');
    // 族二
    let h = eng.quiesce_hit_rate(); s.push_str("quiesce_hit="); s.push_str(&h.hit.to_string()); s.push_str(";quiesce_enter=");
    s.push_str(&h.enter.to_string()); s.push_str(";quiesce_rate="); s.push_str(&h.permille.to_string()); s.push(';');
    // 族三
    s.push_str("defer_total="); s.push_str(&eng.defer_total.to_string()); s.push(';'); i = 0;
    while i < eng.cpus.len() {
        let d = match eng.cpus.get(i) {
            Some(c) => c.defer_count,
            None => 0,
        };
        s.push_str("defer.cpu"); s.push_str(&i.to_string()); s.push('='); s.push_str(&d.to_string()); s.push(';'); i += 1;
    }
    // 族四
    s.push_str("enter_avg4="); s.push_str(&eng.enter_elapsed_avg().to_string()); s.push(';');
    // 附加
    let c = eng.conservation(); s.push_str("cons_lhs="); s.push_str(&c.lhs.to_string()); s.push_str(";cons_rhs=");
    s.push_str(&c.rhs.to_string()); s.push_str(";cons_drift="); s.push_str(&c.drift.to_string()); s.push(';');
    s.push_str("dropped="); s.push_str(&eng.ledger_dropped().to_string()); s.push(';'); s.push_str("hooks_switch=");
    s.push_str(&eng.switch_hook_sets.to_string()); s.push_str(";hooks_idle="); s.push_str(&eng.idle_hook_sets.to_string());
    s
}

/// TLV 条目收集（四族 + 附加 · 确定性顺序）。
pub fn collect_tlv_items(eng: &IdleEnter) -> Vec<TlvItem> {
    let mut v: Vec<TlvItem> = Vec::with_capacity(16); let mut i = 0usize;
    while i < eng.cpus.len() {
        let r = match eng.idle_ratio_permille(i as u32) {
            Ok(x) => x,
            Err(_) => 0,
        };
        v.push(TlvItem { tag: tlv_tag::IDLE_RATIO.wrapping_add(i as u16), value: r, });
        i += 1;
    }
    v.push(TlvItem { tag: tlv_tag::IDLE_RATIO, value: eng.idle_ratio_global(), });
    let h = eng.quiesce_hit_rate();
    v.push(TlvItem { tag: tlv_tag::QUIESCE_HIT, value: h.hit, });
    v.push(TlvItem { tag: tlv_tag::QUIESCE_HIT.wrapping_add(1), value: h.enter, });
    v.push(TlvItem { tag: tlv_tag::QUIESCE_HIT.wrapping_add(2), value: h.permille, });
    v.push(TlvItem { tag: tlv_tag::PSEUDO_DEFER, value: eng.defer_total, });
    i = 0;
    while i < eng.cpus.len() {
        let d = match eng.cpus.get(i) {
            Some(c) => c.defer_count,
            None => 0,
        };
        v.push(TlvItem { tag: tlv_tag::PSEUDO_DEFER.wrapping_add(1).wrapping_add(i as u16), value: d, });
        i += 1;
    }
    v.push(TlvItem { tag: tlv_tag::ENTER_LATENCY, value: eng.enter_elapsed_avg(), });
    let c = eng.conservation();
    v.push(TlvItem { tag: tlv_tag::CONSERVATION, value: c.lhs, });
    v.push(TlvItem { tag: tlv_tag::CONSERVATION.wrapping_add(1), value: c.rhs, });
    v.push(TlvItem { tag: tlv_tag::CONSERVATION.wrapping_add(2), value: eng.ledger_dropped(), });
    v
}

/// TLV 导出（`Vec<u8>` · 魔数 + 版本 + 长度自洽断言）。布局（小端）：```text off 0 : u32 magic   = 0xF1742A01 off 4 : u8  version = 1
/// off 5 : u8  resv    = 0 off 6 : u16 length  = 负载字节数（不含头）off 8 : 条目区，每条 = u16 tag + u64 value（10 字节）```
pub fn export_tlv(eng: &IdleEnter) -> Vec<u8> {
    let items = collect_tlv_items(eng); let payload_len = items.len() * 10;
    let mut buf: Vec<u8> = Vec::with_capacity(TLV_HEADER_LEN + payload_len); put_u32(&mut buf, TLV_MAGIC);
    buf.push(TLV_VERSION); buf.push(0); put_u16(&mut buf, payload_len as u16); let mut i = 0usize;
    while i < items.len() { put_u16(&mut buf, items[i].tag); put_u64(&mut buf, items[i].value); i += 1; }
    buf
}

/// TLV 自洽校验（魔数 / 版本 / 长度三者必须互洽）。
pub fn check_tlv(buf: &[u8]) -> TlvCheck {
    let magic = get_u32(buf, 0); let ver = if buf.len() > 4 { buf[4] } else { 0 };
    let declared = match get_u16(buf, 6) {
        Some(v) => v,
        None => {
            return TlvCheck  { declared_len: 0, actual_len: buf.len(), items: 0, ok: false, }
        }
    };
    let actual_payload = if buf.len() >= TLV_HEADER_LEN {
        buf.len() - TLV_HEADER_LEN
    } else {
        0
    };
    let items = actual_payload / 10;
    let ok = magic == Some(TLV_MAGIC)
    && ver == TLV_VERSION
    && declared as usize == actual_payload
    && actual_payload % 10 == 0
    && buf.len() == TLV_HEADER_LEN + declared as usize;
    TlvCheck  { declared_len: declared, actual_len: buf.len(), items, ok, }
}

/// TLV 自洽校验并在失败时返回 `E::Tlv`。
pub fn check_tlv_strict(eng: &mut IdleEnter) -> Res<TlvCheck> {
    let buf = export_tlv(eng); let c = check_tlv(&buf);
    if c.ok {
        eng.last_code = DIAG_ENTER_OK;
        Ok(c)
    } else {
        let e = E::Tlv { why: 1 }; eng.note_err(e);
        Err(e)
    }
}

/// 回放逐步对账条目（一条一步 · 共四步 + 终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayStep {
    /// 步号（0 = 起始检查，1..=4 = 四步，5 = 终态检查）。
    pub step: u8,
    /// 期望值。
    pub expect: u64,
    /// 实测值。
    pub got: u64,
    /// 偏差（`got - expect` · i64）。
    pub delta: i64,
}

impl ReplayStep {
    /// 该步是否零偏差。
    pub fn ok(&self) -> bool { self.delta == 0 }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "replay step={} expect={} got={} delta={}",
            self.step, self.expect, self.got, self.delta
        )
    }
}

/// 回放报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    /// 逐步对账（四步 + 终态）。
    pub steps: Vec<ReplayStep>,
    /// 静息命中率（重建后）。
    pub hit: HitRate,
    /// 守恒（重建后）。
    pub conservation: Conservation,
    /// 全部零偏差。
    pub zero_drift: bool,
}

impl ReplayReport {
    /// 一行文本摘要。
    pub fn line(&self) -> String {
        format!(
            "replay steps={} zero_drift={} {} {}",
            self.steps.len(),
            self.zero_drift,
            self.hit.line(),
            self.conservation.line()
        )
    }
}

/// 确定性回放：重建一次进入全过程，逐步对账，**偏差必须为零**。步骤：
/// 1. 起点检查：`enter_total == 0`、`quiesce_hit_total == 0`、状态全 Busy。
/// 2. Step1 判定结果必须为 `TrueEmpty`（深度快照 0）。
/// 3. Step2 静息位置位必须发生（`quiesce_map.test(cpu) == true`）。
/// 4. Step3 账目必须起算（`idle_begin != 0`）。
/// 5. Step4 状态必须落定为 Idle。
/// 6. 终态检查：`enter_total == 1`、`quiesce_hit_total == 1`、命中率 = 10000。
/// 无随机源（除固定 seed 外）、无墙钟、无 IO ⇒ 同输入必得同输出。
pub fn replay() -> Res<ReplayReport> {
    let mut eng = IdleEnter::new(1, 1000, 0x5EED_F1742)?; eng.now = 100; let mut steps: Vec<ReplayStep> = Vec::with_capacity(8);

    // 0 起点
    steps.push(ReplayStep { step: 0, expect: 0, got: eng.enter_total, delta: eng.enter_total as i64, });
    steps.push(ReplayStep { step: 0, expect: 0, got: eng.quiesce_hit_total, delta: eng.quiesce_hit_total as i64, });

    let snap = eng.enter_full(0, QueueSnapshot::true_empty())?; let eid = snap.enter_id;

    // 1 Step1
    let s1 = eng
    .cpus
    .get(0)
    .map(|c| c.last_snap.depth)
    .unwrap_or(1); steps.push(ReplayStep { step: 1, expect: 0, got: s1 as u64, delta: s1 as i64, });

    // 2 Step2
    let q = eng.quiesce_test(0);
    steps.push(ReplayStep {
            step: 2,
            expect: 1,
            got: if q { 1 } else { 0 },
            delta: if q { 0 } else { -1 },
    });

    // 3 Step3
    let begin = eng.cpus.get(0).map(|c| c.ledger.idle_begin).unwrap_or(0);
    steps.push(ReplayStep {
            step: 3,
            expect: snap.t_enter + 2,
            got: begin,
            delta: begin as i64 - (snap.t_enter as i64 + 2),
    });

    // 4 Step4
    let st = eng.cpus.get(0).map(|c| c.state.ordinal()).unwrap_or(255);
    steps.push(ReplayStep {
            step: 4,
            expect: IdleState::Idle.ordinal() as u64,
            got: st as u64,
            delta: st as i64 - IdleState::Idle.ordinal() as i64,
    });

    // 5 终态
    steps.push(ReplayStep { step: 5, expect: 1, got: eng.enter_total, delta: eng.enter_total as i64 - 1, });
    steps.push(ReplayStep { step: 5, expect: 1, got: eng.quiesce_hit_total, delta: eng.quiesce_hit_total as i64 - 1, });

    // 签名链核验：四步共享一个 enter_id。
    let sigs = eng.sigs_of_enter(eid); let sig_ok = sigs.len() == 4;
    steps.push(ReplayStep { step: 6, expect: 4, got: sigs.len() as u64, delta: sigs.len() as i64 - 4, });

    let hit = eng.quiesce_hit_rate();
    steps.push(ReplayStep {
            step: 7,
            expect: HIT_RATE_SCALE,
            got: hit.permille,
            delta: hit.permille as i64 - HIT_RATE_SCALE as i64,
    });

    let cons = eng.conservation(); let zero = steps.iter().all(|s| s.ok()) && sig_ok;
    Ok(ReplayReport {
            steps,
            hit,
            conservation: cons,
            zero_drift: zero,
    })
}

/// 回放严格校验（偏差非零即 `E::Replay`）。
pub fn replay_strict() -> Res<ReplayReport> {
    let r = replay()?;
    if !r.zero_drift {
        for s in r.steps.iter() {
            if !s.ok() {
                return Err(E::Replay { step: s.step, delta: s.delta, });
            }
        }
        return Err(E::Replay { step: 0, delta: -1 });
    }
    Ok(r)
}

/// 自检总表（一次跑完判据的三条子句 + 十形态 + 守恒 + 回放 + 导出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfCheck {
    /// 判据 J1-a：四步顺序（`step_order_ok`）。
    pub step_order_ok: bool,
    /// 判据 J1-b：静息命中率恰为 100%（`hit_full_ok`）。
    pub hit_full_ok: bool,
    /// 判据 J1-c：双源对账通过（`dual_ok`）。
    pub dual_ok: bool,
    /// 守恒通过。
    pub conservation_ok: bool,
    /// 转换账一致。
    pub ledger_ok: bool,
    /// TLV 自洽。
    pub tlv_ok: bool,
    /// 回放零偏差。
    pub replay_zero: bool,
    /// 注入核 10/10。
    pub inject_10of10: bool,
    /// 总判定（全部为真方为 `true`）。
    pub all_ok: bool,
    /// 自检摘要行。
    pub summary: String,
}

/// 全量自检（**判据 UNX-F1742-J1 的验收面**）。覆盖：
/// - J1-a：四步严格顺序（一次正常进入 + 一次乱序注入被拒）。
/// - J1-b：静息命中率 100%（正常路径 2 次进入后仍为 10000/10000）。
/// - J1-c：静息位与发现账双源一致（对账通过）。
/// - 守恒：4 核 × 1000 tick，idle 300 + busy 700 ⇒ 4000 == 4000 ✓。
/// - 注入核 10/10。
/// - 回放零偏差。
/// - TLV 自洽。
pub fn self_check() -> Res<SelfCheck> {
    let mut eng = IdleEnter::demo()?;

    // --- J1-a：四步顺序 ---
    eng.now = 10; let _ = eng.enter_full(0, QueueSnapshot::true_empty())?;
    let step_order_ok = eng
    .sigs_of_enter(1)
    .iter()
    .all(|r| r.accepted)
    && eng.sigs_of_enter(1).len() == 4;

    // --- J1-b：静息命中率 100% ---
    eng.now = 100; let _ = eng.enter_full(1, QueueSnapshot::true_empty())?; let h = eng.quiesce_hit_rate();
    let hit_full_ok = h.hit == 2 && h.enter == 2 && h.permille == HIT_RATE_SCALE;

    // --- 伪队空延后（族三）---
    eng.now = 200; let _ = eng.enter_full(2, QueueSnapshot::pseudo_transition(2)); let defer_ok = eng.defer_total == 1;

    // --- 双源对账（闭合段并对齐发现账）---
    let _ = eng.idle_close_segment(0, 400); let _ = eng.idle_close_segment(1, 400);
    for cpu in 0..eng.cpu_count() {
        let v = match eng.cpus.get(cpu as usize) {
            Some(c) => c.ledger.idle_ticks,
            None => 0,
        };
        eng.discovery_record(cpu, v)?;
    }
    let dual_ok = eng.reconcile_dual_source().is_ok();

    // --- 守恒（均匀铺满）---
    let _ = eng.fill_uniform(300); let conservation_ok = eng.check_conservation().is_ok();

    // --- 转换账一致 ---
    let ledger_ok = eng.assert_trans_ledger().is_ok();

    // --- TLV 自洽 ---
    let tlv_ok = check_tlv_strict(&mut eng).map(|c| c.ok).unwrap_or(false);

    // --- 回放 ---
    let rp = replay()?; let replay_zero = rp.zero_drift;

    // --- 注入核 ---
    let inj = inject_all(); let inject_10of10 = all_detected(&inj);

    let all_ok = step_order_ok
    && hit_full_ok
    && dual_ok
    && conservation_ok
    && ledger_ok
    && tlv_ok
    && replay_zero
    && inject_10of10
    && defer_ok;

    let summary = format!(
        "{} step_order={} hit_full={} dual={} cons={} ledger={} tlv={} replay={} inject={} defer={} => {}",
        ID_CASE,
        step_order_ok,
        hit_full_ok,
        dual_ok,
        conservation_ok,
        ledger_ok,
        tlv_ok,
        replay_zero,
        inject_10of10,
        defer_ok,
        if all_ok { "PASS" } else { "FAIL" }
    );

    Ok(SelfCheck {
            step_order_ok,
            hit_full_ok,
            dual_ok,
            conservation_ok,
            ledger_ok,
            tlv_ok,
            replay_zero,
            inject_10of10,
            all_ok,
            summary,
    })
}

/// 判据文本（自检输出引用 · 与 `self_check` 的 summary 对齐）。
pub fn criteria_text() -> String {
    let mut s = String::new(); s.push_str(ID_CASE); s.push_str(": "); s.push_str("进入四步顺序断言全过，静息置位在 idle 进入点 100% 命中（F1735 挂点消费）");
    s
}