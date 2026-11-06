//! UNX-F9382 futex 四操作挂点联签（WAIT/WAKE/FD/REQUEUE 协议） · 0x7CD00段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9382-J1 四操作挂点联签判据过，C4 futex 四操作协议逐操作
//!       对接可观测，挂点单源格过。
//!
//! 设计要点：
//!   ① futex 空间按等待队列状态机建模：WAIT 入队、WAKE 出队、REQUEUE
//!      迁移、FD 附句柄；四操作全部经过唯一实现（单源），hook 只是
//!      在实现前后记账，不得复制第二套语义。
//!   ② 挂点等价账是本模块核心：每个操作跑「直连路径」与「hook 路径」
//!      两次（同初态、同参数），结果必须逐字段相等；等价账只增不减，
//!      出现一次不等即挂点语义漂移，显性拒绝。
//!   ③ 注入核覆盖四类坏样本：WAIT 值不匹配（EAGAIN 且零入队）、WAKE
//!      超发（唤醒数 ≤ 队列深度）、REQUEUE 自迁移（EINVAL）、未知操作
//!      码（ENOSYS），逐类 10/10 检出且队列账守恒。
//!   ④ 队列账守恒式：残留 = 入队 − 唤醒（REQUEUE 换键不出队），任何
//!      操作后复算成立；EAGAIN 路径零入队（不产生幽灵等待者）。
//!   ⑤ 诊断码闭集：SEG_BASE + 变体序号，段内 +0x01..+0x0B；双跑指纹
//!      FNV-1a 覆盖四操作全语料。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9382 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CD00;

/// 队列容量上限（幽灵等待者防线）。
pub const QUEUE_CAP: usize = 32;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// 操作码：FUTEX_WAIT。
pub const OP_WAIT: u32 = 0;
/// 操作码：FUTEX_WAKE。
pub const OP_WAKE: u32 = 1;
/// 操作码：FUTEX_FD。
pub const OP_FD: u32 = 2;
/// 操作码：FUTEX_REQUEUE。
pub const OP_REQUEUE: u32 = 3;

/// Linux errno 正值口径。
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；值不匹配重试族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSYS", -38, 1, "Windows ERROR_NOT_SUPPORTED；未实装操作族"),
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；等待中断族"),
    ("ETIMEDOUT", -110, 1460, "Windows ERROR_TIMEOUT；等待超时族"),
];

/// 按 errno 正数查负值；表外返回 0。
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
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FxCode {
    /// WAIT 值不匹配（Linux EAGAIN 语义）
    ValMismatch,
    /// 队列已满仍入队
    QueueFull,
    /// 唤醒超发
    WakeOverflow,
    /// REQUEUE 自迁移
    RequeueSelf,
    /// 未知操作码
    UnknownOp,
    /// hook 路径与直连路径不等价
    HookDiverged,
    /// 队列账不守恒
    QueueUnconserved,
    /// 账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl FxCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FxCode::ValMismatch => "WAIT 值不匹配被拒：*uaddr 与期望值不等时必须\
                回 EAGAIN 且零入队；请先重读 uaddr 再等待",
            FxCode::QueueFull => "队列已满被拒：等待队列不得超过容量上限；\
                请先唤醒或迁移再入队",
            FxCode::WakeOverflow => "唤醒超发被拒：唤醒数不得超过队列深度；\
                超发即丢账，请按队列实深截断",
            FxCode::RequeueSelf => "REQUEUE 自迁移被拒：源键与目标键相同必须回 \
                EINVAL；自迁移会重复计数",
            FxCode::UnknownOp => "未知操作码被拒：四操作闭集外一律回 ENOSYS；\
                新操作须先走宪章登记",
            FxCode::HookDiverged => "hook 路径与直连路径不等价：同初态同参数两次\
                结果必须逐字段一致；请勿在 hook 内复制语义",
            FxCode::QueueUnconserved => "队列账不守恒：残留必须等于入队减唤醒\
                （REQUEUE 只换键不出队）；请检查操作是否漏记或重复记账",
            FxCode::LedgerRegress => "账倒退被拒：操作/等价计数只增不减；请检查\
                回滚路径是否误改累计值",
            FxCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            FxCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            FxCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [FxCode; 11] = [
        FxCode::ValMismatch,
        FxCode::QueueFull,
        FxCode::WakeOverflow,
        FxCode::RequeueSelf,
        FxCode::UnknownOp,
        FxCode::HookDiverged,
        FxCode::QueueUnconserved,
        FxCode::LedgerRegress,
        FxCode::NotReproducible,
        FxCode::SegmentViolation,
        FxCode::EmptyDetail,
    ];
}

// ── futex 空间状态机 ──────────────────────────────────────────────────
/// 等待者：uaddr 键 + 线程标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiter {
    /// uaddr 键
    pub key: u64,
    /// 线程标识
    pub tid: u32,
}

/// futex 空间：四操作共享的等待队列状态机。
#[derive(Clone, Debug)]
pub struct FutexSpace {
    waiters: Vec<Waiter>,
    /// 累计入队数（账只增）
    waits: u32,
    /// 累计唤醒数
    wakes: u32,
    /// 累计迁移数
    requeues: u32,
    /// EAGAIN 拒绝数
    eagains: u32,
}

impl FutexSpace {
    /// 构造空空间。
    pub fn new() -> FutexSpace {
        FutexSpace { waiters: Vec::new(), waits: 0, wakes: 0, requeues: 0, eagains: 0 }
    }

    /// WAIT：值不匹配回 EAGAIN 零入队；否则入队（容量满显性拒绝）。
    pub fn op_wait(&mut self, key: u64, expect: u32, cur: u32, tid: u32) -> Result<(), i32> {
        if expect != cur {
            self.eagains = self.eagains.saturating_add(1);
            return Err(EAGAIN);
        }
        if self.waiters.len() >= QUEUE_CAP {
            return Err(EINVAL);
        }
        self.waiters.push(Waiter { key, tid });
        self.waits = self.waits.saturating_add(1);
        Ok(())
    }

    /// WAKE：唤醒至多 n 个同键等待者，返回实唤醒数（不得超发）。
    pub fn op_wake(&mut self, key: u64, n: u32) -> Result<u32, i32> {
        if n == 0 || key == 0 {
            return Err(EINVAL);
        }
        let mut woken = 0u32;
        let mut i = 0usize;
        while i < self.waiters.len() && woken < n {
            let keep = match self.waiters.get(i) {
                Some(w) => w.key != key,
                None => true,
            };
            if !keep {
                let _ = self.waiters.remove(i);
                woken = woken.saturating_add(1);
                // 移除后不前进 i，继续检查当前位置
                continue;
            }
            i += 1;
        }
        self.wakes = self.wakes.saturating_add(woken);
        Ok(woken)
    }

    /// REQUEUE：把至多 n 个等待者从 key 迁到 key2，返回迁移数。
    pub fn op_requeue(&mut self, key: u64, n: u32, key2: u64) -> Result<u32, i32> {
        if key == 0 || key2 == 0 {
            return Err(EFAULT);
        }
        if key == key2 {
            return Err(EINVAL);
        }
        let mut moved = 0u32;
        let mut i = 0usize;
        while i < self.waiters.len() && moved < n {
            if let Some(w) = self.waiters.get_mut(i) {
                if w.key == key {
                    w.key = key2;
                    moved = moved.saturating_add(1);
                }
            }
            i += 1;
        }
        self.requeues = self.requeues.saturating_add(moved);
        Ok(moved)
    }

    /// FD：为键附一个句柄标识（键非 0 合法；键 0 回 EINVAL）。
    pub fn op_fd(&self, key: u64) -> Result<u32, i32> {
        if key == 0 {
            return Err(EINVAL);
        }
        // 句柄 = 键低 32 位 + 打开标记（纯函数，无副作用）
        Ok(((key & 0xFFFF_FFFF) | 0x8000_0000) as u32)
    }

    /// 统一操作入口（单源）：四操作只此一路分发。
    pub fn op(&mut self, op: u32, key: u64, expect: u32, cur: u32, n: u32, key2: u64, tid: u32) -> Result<i64, i32> {
        match op {
            OP_WAIT => self.op_wait(key, expect, cur, tid).map(|()| 0),
            OP_WAKE => self.op_wake(key, n).map(|v| v as i64),
            OP_FD => self.op_fd(key).map(|v| v as i64),
            OP_REQUEUE => self.op_requeue(key, n, key2).map(|v| v as i64),
            _ => Err(ENOSYS),
        }
    }

    /// 队列深度。
    pub fn depth(&self) -> usize {
        self.waiters.len()
    }

    /// 队列账守恒：残留 = 入队 − 唤醒（REQUEUE 只换键不出队，不影响深度）。
    pub fn conserved(&self) -> bool {
        let residual = self.waits.saturating_sub(self.wakes);
        residual as usize == self.waiters.len()
    }

    /// 键命中数。
    pub fn count_key(&self, key: u64) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.waiters.len() {
            if let Some(w) = self.waiters.get(i) {
                if w.key == key {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 挂点等价账 ────────────────────────────────────────────────────────
/// hook 等价账：每个操作双路径对拍一笔，只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EquivLedger {
    /// 已对拍笔数
    pub compared: u32,
    /// 不等价笔数（必须为 0）
    pub diverged: u32,
}

impl EquivLedger {
    const fn zero() -> EquivLedger {
        EquivLedger { compared: 0, diverged: 0 }
    }

    /// 记一笔对拍结果（账只增）。
    pub fn record(&mut self, equal: bool) {
        self.compared = self.compared.saturating_add(1);
        if !equal {
            self.diverged = self.diverged.saturating_add(1);
        }
    }

    /// 单源成立：全部对拍等价。
    pub fn single_source(&self) -> bool {
        self.compared > 0 && self.diverged == 0
    }
}

/// hook 路径：在单源 op 前后记账，返回值与直连路径逐字段一致。
pub fn hooked_op(space: &mut FutexSpace, op: u32, key: u64, expect: u32, cur: u32, n: u32, key2: u64, tid: u32) -> Result<i64, i32> {
    let depth_before = space.depth();
    let r = space.op(op, key, expect, cur, n, key2, tid);
    // hook 侧仅观测：入队成功深度 +1，其余深度不变（FD 不入队）
    let depth_after = space.depth();
    match r {
        Ok(v) => {
            if op == OP_WAIT && depth_after != depth_before + 1 {
                return Err(EINVAL);
            }
            Ok(v)
        }
        Err(e) => Err(e),
    }
}

/// 四操作逐操作等价对拍：同初态复制两份，直连 vs hook 逐字段比较。
pub fn run_equivalence_suite() -> EquivLedger {
    let mut led = EquivLedger::zero();
    // 语料：四操作 × 三键位
    let keys = [0x1000u64, 0x2000, 0x3000];
    let mut ki = 0usize;
    while ki < keys.len() {
        let key = match keys.get(ki) {
            Some(k) => *k,
            None => break,
        };
        // WAIT 直连 vs hook
        let mut a = FutexSpace::new();
        let mut b = FutexSpace::new();
        let ra = a.op(OP_WAIT, key, 5, 5, 0, 0, 701);
        let rb = hooked_op(&mut b, OP_WAIT, key, 5, 5, 0, 0, 701);
        led.record(ra == rb && a.depth() == b.depth() && a.conserved() && b.conserved());
        // WAKE 直连 vs hook（先各补两个等待者）
        let mut a2 = FutexSpace::new();
        let mut b2 = FutexSpace::new();
        let _ = a2.op_wait(key, 5, 5, 801);
        let _ = a2.op_wait(key, 5, 5, 802);
        let _ = b2.op_wait(key, 5, 5, 801);
        let _ = b2.op_wait(key, 5, 5, 802);
        let ra2 = a2.op(OP_WAKE, key, 0, 0, 1, 0, 0);
        let rb2 = hooked_op(&mut b2, OP_WAKE, key, 0, 0, 1, 0, 0);
        led.record(ra2 == rb2 && a2.depth() == b2.depth() && a2.conserved() && b2.conserved());
        // REQUEUE 直连 vs hook
        let mut a3 = FutexSpace::new();
        let mut b3 = FutexSpace::new();
        let _ = a3.op_wait(key, 5, 5, 901);
        let _ = b3.op_wait(key, 5, 5, 901);
        let ra3 = a3.op(OP_REQUEUE, key, 0, 0, 1, key + 0x100, 0);
        let rb3 = hooked_op(&mut b3, OP_REQUEUE, key, 0, 0, 1, key + 0x100, 0);
        led.record(ra3 == rb3 && a3.depth() == b3.depth() && a3.conserved() && b3.conserved());
        // FD 直连 vs hook（FD 无队列副作用）
        let mut a4 = FutexSpace::new();
        let mut b4 = FutexSpace::new();
        let ra4 = a4.op(OP_FD, key, 0, 0, 0, 0, 0);
        let rb4 = hooked_op(&mut b4, OP_FD, key, 0, 0, 0, 0, 0);
        led.record(ra4 == rb4 && a4.depth() == b4.depth());
        ki += 1;
    }
    led
}

// ── 注入账与注入核 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
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

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadOpCase {
    /// WAIT 值不匹配（应 EAGAIN 零入队）
    WaitMismatch,
    /// WAKE 超发（应按实深截断）
    WakeOverflow,
    /// REQUEUE 自迁移（应 EINVAL）
    RequeueSelf,
    /// 未知操作码（应 ENOSYS）
    UnknownOp,
}

impl BadOpCase {
    /// 闭集全量。
    pub const ALL: [BadOpCase; 4] = [
        BadOpCase::WaitMismatch,
        BadOpCase::WakeOverflow,
        BadOpCase::RequeueSelf,
        BadOpCase::UnknownOp,
    ];
}

/// 注入核：四类坏样本各注入 INJECT_ROUNDS 次，逐类断言显性拒绝且账守恒。
pub fn run_op_injection() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadOpCase::ALL.len() {
        let case = match BadOpCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut sp = FutexSpace::new();
            let caught = match case {
                BadOpCase::WaitMismatch => {
                    let before = sp.depth();
                    let r = sp.op(OP_WAIT, 0x10, 7, 8, 0, 0, 111);
                    r == Err(EAGAIN) && sp.depth() == before && sp.conserved()
                }
                BadOpCase::WakeOverflow => {
                    let _ = sp.op_wait(0x20, 3, 3, 222);
                    let r = sp.op(OP_WAKE, 0x20, 0, 0, 9, 0, 0);
                    // 超发被截断为实深 1，且队列账守恒
                    r == Ok(1) && sp.depth() == 0 && sp.conserved()
                }
                BadOpCase::RequeueSelf => {
                    let _ = sp.op_wait(0x30, 1, 1, 333);
                    let before = sp.count_key(0x30);
                    let r = sp.op(OP_REQUEUE, 0x30, 0, 0, 1, 0x30, 0);
                    r == Err(EINVAL) && sp.count_key(0x30) == before
                }
                BadOpCase::UnknownOp => {
                    let r = sp.op(99, 0x40, 1, 1, 0, 0, 444);
                    r == Err(ENOSYS) && sp.depth() == 0 && sp.conserved()
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
        ci += 1;
    }
    (ledger, per_class)
}

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
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

/// 规范语料指纹：四操作全流程 + 等价对拍 + 注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut sp = FutexSpace::new();
    // WAIT ×2 → WAKE ×1 → REQUEUE ×1 → FD
    let r1 = sp.op(OP_WAIT, 0xA0, 5, 5, 0, 0, 1);
    let r2 = sp.op(OP_WAIT, 0xA0, 5, 5, 0, 0, 2);
    let r3 = sp.op(OP_WAKE, 0xA0, 0, 0, 1, 0, 0);
    let r4 = sp.op(OP_REQUEUE, 0xA0, 0, 0, 8, 0xB0, 0);
    let r5 = sp.op(OP_FD, 0xB0, 0, 0, 0, 0, 0);
    for r in [r1, r2, r3, r4, r5].iter() {
        match r {
            Ok(v) => {
                bytes.push((*v as u64 & 0xFF) as u8);
            }
            Err(e) => bytes.push((*e & 0xFF) as u8),
        }
    }
    bytes.push(sp.depth() as u8);
    bytes.push(sp.conserved() as u8);
    bytes.push(sp.count_key(0xB0) as u8);
    // EAGAIN 路径
    let r6 = sp.op(OP_WAIT, 0xA0, 5, 6, 0, 0, 3);
    bytes.push(match r6 {
        Err(EAGAIN) => 1,
        _ => 0,
    });
    // 等价对拍账
    let eq = run_equivalence_suite();
    bytes.push(eq.compared as u8);
    bytes.push(eq.diverged as u8);
    // 注入账
    let (inj, per) = run_op_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9382 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9382_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9382");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 四操作协议正向流：WAIT→WAKE→REQUEUE→FD 全链
    let mut sp = FutexSpace::new();
    let w1 = sp.op(OP_WAIT, 0x100, 5, 5, 0, 0, 10);
    let w2 = sp.op(OP_WAIT, 0x100, 5, 5, 0, 0, 11);
    let wk = sp.op(OP_WAKE, 0x100, 0, 0, 1, 0, 0);
    let rq = sp.op(OP_REQUEUE, 0x100, 0, 0, 8, 0x200, 0);
    let fd = sp.op(OP_FD, 0x200, 0, 0, 0, 0, 0);
    let chain_ok = w1.is_ok() && w2.is_ok() && wk == Ok(1)
        && rq == Ok(1) && fd.is_ok() && sp.conserved();
    s.add("四操作正向流：全链成功且队列账守恒", chain_ok,
          "WAIT/WAKE/REQUEUE/FD 逐操作对接可观测");

    // 3. 挂点单源：四操作等价对拍全等
    let eq = run_equivalence_suite();
    s.add("挂点单源：等价对拍全等且账只增",
          eq.single_source() && eq.compared == 12,
          "3 键位 × 4 操作 = 12 笔对拍零漂移");

    // 4. WAKE 语义：只唤醒目标键
    let mut sp2 = FutexSpace::new();
    let _ = sp2.op_wait(0x300, 1, 1, 21);
    let _ = sp2.op_wait(0x301, 1, 1, 22);
    let wake_sel = match sp2.op_wake(0x300, 5) {
        Ok(n) => n,
        Err(_) => 99,
    };
    s.add("WAKE 语义：异键等待者不被误唤醒",
          wake_sel == 1 && sp2.depth() == 1 && sp2.count_key(0x301) == 1,
          "唤醒按键过滤，不得串键");

    // 5. REQUEUE 语义：迁移后目标键可 WAKE
    let mut sp3 = FutexSpace::new();
    let _ = sp3.op_wait(0x400, 1, 1, 31);
    let _ = sp3.op_requeue(0x400, 5, 0x500);
    let wake_moved = match sp3.op_wake(0x500, 5) {
        Ok(n) => n,
        Err(_) => 99,
    };
    s.add("REQUEUE 语义：迁移后新键可唤醒", wake_moved == 1 && sp3.depth() == 0,
          "迁移保持等待者身份，只换键位");

    // 6. 注入核：四类坏样本各 10/10 检出
    let (inj, per_class) = run_op_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 检出", all_caught,
          "值不匹配/超发/自迁移/未知操作逐类拒绝");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. EAGAIN 零入队（幽灵等待者防线）
    let mut sp4 = FutexSpace::new();
    let before = sp4.depth();
    let r_eagain = sp4.op_wait(0x600, 9, 8, 41);
    s.add("EAGAIN 路径：零入队且账守恒",
          r_eagain == Err(EAGAIN) && sp4.depth() == before && sp4.conserved(),
          "值不匹配不得产生等待者");

    // 9. 容量边界：满容 WAIT 显性 EINVAL
    let mut sp5 = FutexSpace::new();
    let mut fill = 0usize;
    let mut fill_ok = true;
    while fill < QUEUE_CAP {
        if sp5.op_wait(0x700, 1, 1, (fill as u32) + 1).is_err() {
            fill_ok = false;
        }
        fill += 1;
    }
    let full_rej = sp5.op_wait(0x700, 1, 1, 999);
    s.add("容量边界：满容后 WAIT 显性拒绝",
          fill_ok && full_rej == Err(EINVAL) && sp5.depth() == QUEUE_CAP,
          "容量满必须拒绝而非静默覆盖");

    // 10. 未知操作码 ENOSYS
    let mut sp6 = FutexSpace::new();
    s.add("未知操作码：闭集外一律 ENOSYS",
          sp6.op(42, 0x800, 1, 1, 0, 0, 0) == Err(ENOSYS),
          "四操作闭集外不得误入其他语义");

    // 11. FD 无副作用：不改变队列
    let mut sp7 = FutexSpace::new();
    let _ = sp7.op_wait(0x900, 1, 1, 51);
    let d_before = sp7.depth();
    let fd_r = sp7.op_fd(0x900);
    s.add("FD 语义：附句柄零队列副作用",
          fd_r.is_ok() && sp7.depth() == d_before,
          "FD 只取句柄视图，不入队不出队");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EAGAIN) && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL) && agrees_with_linux(ENOSYS)
        && !agrees_with_linux(0) && !agrees_with_linux(999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = FxCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < FxCode::ALL.len() {
        let mut j = i + 1;
        while j < FxCode::ALL.len() {
            if let (Some(x), Some(y)) = (FxCode::ALL.get(i), FxCode::ALL.get(j)) {
                if x.code() == y.code() {
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
    let sp_text = format!(
        "入队 {} / 唤醒 {} / 迁移 {} / EAGAIN {} / 等价 {} 笔",
        sp.waits, sp.wakes, sp.requeues, sp.eagains, eq.compared
    );
    s.add_dyn("操作账快照", sp.conserved() && eq.single_source(), &sp_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_wake_roundtrip() {
        let mut sp = FutexSpace::new();
        assert!(sp.op_wait(1, 5, 5, 100).is_ok());
        assert_eq!(sp.depth(), 1);
        assert_eq!(sp.op_wake(1, 1), Ok(1));
        assert_eq!(sp.depth(), 0);
        assert!(sp.conserved());
    }

    #[test]
    fn wait_mismatch_eagain_zero_enqueue() {
        let mut sp = FutexSpace::new();
        assert_eq!(sp.op_wait(1, 5, 6, 100), Err(EAGAIN));
        assert_eq!(sp.depth(), 0);
    }

    #[test]
    fn wake_never_overflow() {
        let mut sp = FutexSpace::new();
        let _ = sp.op_wait(2, 1, 1, 1);
        assert_eq!(sp.op_wake(2, 10), Ok(1));
    }

    #[test]
    fn requeue_moves_waiters() {
        let mut sp = FutexSpace::new();
        let _ = sp.op_wait(3, 1, 1, 1);
        let _ = sp.op_wait(3, 1, 1, 2);
        assert_eq!(sp.op_requeue(3, 2, 4), Ok(2));
        assert_eq!(sp.count_key(3), 0);
        assert_eq!(sp.count_key(4), 2);
    }

    #[test]
    fn requeue_self_einval() {
        let mut sp = FutexSpace::new();
        assert_eq!(sp.op_requeue(3, 1, 3), Err(EINVAL));
    }

    #[test]
    fn unknown_op_enosys() {
        let mut sp = FutexSpace::new();
        assert_eq!(sp.op(77, 1, 1, 1, 0, 0, 0), Err(ENOSYS));
    }

    #[test]
    fn fd_handle_stable() {
        let sp = FutexSpace::new();
        let h1 = sp.op_fd(0x1234);
        let h2 = sp.op_fd(0x1234);
        assert_eq!(h1, h2);
        assert_eq!(sp.op_fd(0), Err(EINVAL));
    }

    #[test]
    fn equivalence_suite_single_source() {
        let eq = run_equivalence_suite();
        assert!(eq.single_source());
        assert_eq!(eq.compared, 12);
        assert_eq!(eq.diverged, 0);
    }

    #[test]
    fn op_injection_matrix_green() {
        let (l, per) = run_op_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn queue_cap_enforced() {
        let mut sp = FutexSpace::new();
        let mut i = 0;
        while i < QUEUE_CAP {
            assert!(sp.op_wait(9, 1, 1, i as u32 + 1).is_ok());
            i += 1;
        }
        assert_eq!(sp.op_wait(9, 1, 1, 9999), Err(EINVAL));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9382_checks_green() {
        let s = run_unx9382_checks();
        assert!(s.all_passed(), "UNX-F9382 red: {}", s.red_items_text());
    }
}
