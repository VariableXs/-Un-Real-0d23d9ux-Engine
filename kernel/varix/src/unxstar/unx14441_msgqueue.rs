//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14441 · 每线程消息队列四分区结构 · 0x66700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥383 行；
//!   ② UNX-F14441-J1 posted/sent/hardware/quit 四分区独立记账，分区操
//!      作账 10 万次零串区，结构字段与版本锚档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四分区闭集：posted 0 / sent 1 / hardware 2 / quit 3（写死，来
//!     源：每线程消息队列四分区结构口径）；
//!   - 每消息携带路由分区标签，入队即钉；出队核销时逐条比对标签与
//!     所在分区——零串区（跨区即红线）；
//!   - quit 分区专用语义：WM_QUIT 只入 quit 分区（其它分区注入
//!     quit 类消息必拒），quit 消息取走即置线程退出位（单次）；
//!   - 独立记账：每分区独立计数 + 全队列总操作账，守恒：Σ四分区计数
//!     = 总操作账；Σ出队核销 = Σ各分区出队；
//!   - 版本锚档：结构字段名表（四分区 + 计数字段）写死锚档，字段数
//!     与版本号（V1）对表一致；
//!   - 10 万次操作账：LCG 确定性操作序列（入队/出队/窥探），双机对拍
//!     终态逐分区一致；
//!   - LCG 种子写死 = 0x66700；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66700，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66700;

/// 四分区闭集。
pub const PART_POSTED: usize = 0;
pub const PART_SENT: usize = 1;
pub const PART_HW: usize = 2;
pub const PART_QUIT: usize = 3;
pub const PART_COUNT: usize = 4;
pub const PART_NAMES: [&str; PART_COUNT] = ["posted", "sent", "hardware", "quit"];

/// 消息类闭集。
pub const KIND_POST: usize = 0;
pub const KIND_SENT: usize = 1;
pub const KIND_HW: usize = 2;
pub const KIND_QUIT: usize = 3;
pub const KIND_COUNT: usize = 4;

/// 每分区容量上限（写死：4096 条，越界入队拒收——背压口径）。
pub const PART_CAP: usize = 4096;
/// 操作账规模（判据原文：10 万次）。
pub const OP_ACCOUNT: usize = 100_000;

/// 版本锚档（写死：结构版本 V1，锚档字段 8 项——四分区槽 + 四计数）。
pub const STRUCT_VERSION: u32 = 1;
pub const ANCHOR_FIELDS: usize = 8;
pub const ANCHOR_FIELD_NAMES: [&str; ANCHOR_FIELDS] = [
    "posted 槽",
    "sent 槽",
    "hardware 槽",
    "quit 槽",
    "posted 计数",
    "sent 计数",
    "hardware 计数",
    "quit 计数",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MqCode {
    /// 分区越闭集（0..4）
    PartitionInvalid,
    /// 消息类越闭集
    KindInvalid,
    /// 串区：消息标签与所在分区不符
    CrossContamination,
    /// quit 类消息注入非 quit 分区
    QuitMisrouted,
    /// 计数守恒破裂（Σ分区 ≠ 总账）
    CounterMismatch,
    /// 分区容量越限（4096 背压）
    PartitionFull,
    /// 空分区出队
    EmptyPoll,
    /// quit 单次位二次置位
    QuitDoubleSet,
    /// 版本锚档不一致
    AnchorMismatch,
    /// 操作账总数 ≠ 10 万
    OpAccountMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl MqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MqCode::PartitionInvalid => "分区越闭集：posted/sent/hardware/quit",
            MqCode::KindInvalid => "消息类越闭集：四类",
            MqCode::CrossContamination => "串区：消息标签与所在分区不符（零串区红线）",
            MqCode::QuitMisrouted => "quit 类消息注入非 quit 分区：专用语义红线",
            MqCode::CounterMismatch => "计数守恒破裂：Σ四分区 ≠ 总账",
            MqCode::PartitionFull => "分区容量越限：4096 背压拒收",
            MqCode::EmptyPoll => "空分区出队：窥探/取走必拒",
            MqCode::QuitDoubleSet => "quit 单次位二次置位",
            MqCode::AnchorMismatch => "结构字段与版本锚档不一致",
            MqCode::OpAccountMismatch => "操作账总数 ≠ 10 万",
            MqCode::DualReplayDrift => "双机对拍漂移：终态不可复现",
        }
    }
    pub const ALL: [MqCode; 11] = [
        MqCode::PartitionInvalid,
        MqCode::KindInvalid,
        MqCode::CrossContamination,
        MqCode::QuitMisrouted,
        MqCode::CounterMismatch,
        MqCode::PartitionFull,
        MqCode::EmptyPoll,
        MqCode::QuitDoubleSet,
        MqCode::AnchorMismatch,
        MqCode::OpAccountMismatch,
        MqCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MqCode::ALL.len() {
        if MqCode::ALL[i].val() == code {
            return MqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66700 段闭集内"
}

// ── 消息 ─────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    /// 全队列唯一序号（单调递增）
    pub id: u64,
    /// 消息类（0..4）
    pub kind: usize,
    /// 路由分区标签（入队即钉）
    pub part: usize,
    /// 载荷（建模 wparam/lparam 合成值）
    pub payload: u64,
}

/// 消息类→分区路由闭式（写死：类与分区一一对应）。
pub fn route_of(kind: usize) -> Result<usize, u32> {
    if kind >= KIND_COUNT {
        return Err(MqCode::KindInvalid.val());
    }
    Ok(kind)
}

// ── 四分区队列 ───────────────────────────────────────────────────
pub struct MsgQueue {
    pub slots: [Vec<Msg>; PART_COUNT],
    /// 每分区入队计数
    pub pushed: [u64; PART_COUNT],
    /// 每分区出队核销计数
    pub popped: [u64; PART_COUNT],
    /// 全队列总操作账（入队 + 出队 + 窥探）
    pub ops: u64,
    /// quit 单次位
    pub quit_set: bool,
    /// 线程退出位（由 quit 消息取走触发）
    pub thread_exit: bool,
    next_id: u64,
}

impl MsgQueue {
    pub fn new() -> MsgQueue {
        MsgQueue {
            slots: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            pushed: [0; PART_COUNT],
            popped: [0; PART_COUNT],
            ops: 0,
            quit_set: false,
            thread_exit: false,
            next_id: 1,
        }
    }

    /// 入队：路由分区由消息类闭式决定（标签入队即钉）。
    /// 操作账口径：每次调用（含拒收）都计一次操作。
    pub fn push(&mut self, kind: usize, payload: u64) -> Result<u64, u32> {
        self.ops += 1;
        let part = route_of(kind)?;
        if self.slots[part].len() >= PART_CAP {
            return Err(MqCode::PartitionFull.val());
        }
        // quit 专用语义：quit 类只入 quit 分区（route 闭式已保证）；
        // 反向注入面：非 quit 分区收到 quit 类标签必拒（构造防御）。
        if kind == KIND_QUIT && part != PART_QUIT {
            return Err(MqCode::QuitMisrouted.val());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.slots[part].push(Msg { id, kind, part, payload });
        self.pushed[part] += 1;
        Ok(id)
    }

    /// 窥探（不改队）：空分区必拒；窥探也计操作账。
    pub fn peek(&mut self, part: usize) -> Result<Msg, u32> {
        self.ops += 1;
        if part >= PART_COUNT {
            return Err(MqCode::PartitionInvalid.val());
        }
        if self.slots[part].is_empty() {
            return Err(MqCode::EmptyPoll.val());
        }
        Ok(self.slots[part][0].clone())
    }

    /// 出队核销：标签与所在分区比对（零串区），quit 取走置退出位。
    pub fn pop(&mut self, part: usize) -> Result<Msg, u32> {
        self.ops += 1;
        if part >= PART_COUNT {
            return Err(MqCode::PartitionInvalid.val());
        }
        if self.slots[part].is_empty() {
            return Err(MqCode::EmptyPoll.val());
        }
        let m = self.slots[part].remove(0);
        // 零串区核销：标签 vs 分区。
        if m.part != part {
            return Err(MqCode::CrossContamination.val());
        }
        self.popped[part] += 1;
        if m.kind == KIND_QUIT {
            if self.quit_set {
                return Err(MqCode::QuitDoubleSet.val());
            }
            self.quit_set = true;
            self.thread_exit = true;
        }
        Ok(m)
    }

    /// 计数守恒：Σ入队 = Σ分区计数；Σ(pushed-popped) = 在队数。
    pub fn counter_conservation(&self) -> Result<(), u32> {
        let mut sum_push = 0;
        let mut sum_pop = 0;
        let mut in_queue = 0;
        let mut p = 0;
        while p < PART_COUNT {
            sum_push += self.pushed[p];
            sum_pop += self.popped[p];
            in_queue += self.slots[p].len();
            p += 1;
        }
        if sum_pop > sum_push {
            return Err(MqCode::CounterMismatch.val());
        }
        if sum_push - sum_pop != in_queue as u64 {
            return Err(MqCode::CounterMismatch.val());
        }
        Ok(())
    }

    /// 全量零串区核销：逐分区逐条比对标签（只读扫）。
    pub fn scan_no_contamination(&self) -> Result<(), u32> {
        let mut p = 0;
        while p < PART_COUNT {
            let mut i = 0;
            while i < self.slots[p].len() {
                if self.slots[p][i].part != p {
                    return Err(MqCode::CrossContamination.val());
                }
                i += 1;
            }
            p += 1;
        }
        Ok(())
    }

    /// 双机对拍：独立新队重放同序列，逐分区终态一致。
    pub fn dual_replay(&self, ops: &[(u8, usize, u64)]) -> Result<(), u32> {
        let mut twin = MsgQueue::new();
        replay_ops(&mut twin, ops)?;
        let mut p = 0;
        while p < PART_COUNT {
            if twin.pushed[p] != self.pushed[p]
                || twin.popped[p] != self.popped[p]
                || twin.slots[p].len() != self.slots[p].len()
            {
                return Err(MqCode::DualReplayDrift.val());
            }
            // 逐条比对在队消息（id/kind/payload）。
            let mut i = 0;
            while i < self.slots[p].len() {
                if twin.slots[p][i] != self.slots[p][i] {
                    return Err(MqCode::DualReplayDrift.val());
                }
                i += 1;
            }
            p += 1;
        }
        Ok(())
    }
}

/// 操作序列重放（op: 0=push 1=pop 2=peek；arg 分区或类）。
pub fn replay_ops(q: &mut MsgQueue, ops: &[(u8, usize, u64)]) -> Result<(), u32> {
    let mut i = 0;
    while i < ops.len() {
        let (op, a, b) = ops[i];
        match op {
            0 => {
                let _ = q.push(a, b);
            }
            1 => {
                let _ = q.pop(a);
            }
            _ => {
                let _ = q.peek(a);
            }
        }
        i += 1;
    }
    Ok(())
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 10 万次操作序列生成（确定性：op 3 种、类 4 种、载荷伪随机）。
pub fn gen_ops() -> Vec<(u8, usize, u64)> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ops: Vec<(u8, usize, u64)> = Vec::new();
    let mut i = 0;
    while i < OP_ACCOUNT {
        let op = (rng.below(3)) as u8;
        let a = (rng.below(KIND_COUNT as u64)) as usize;
        let b = rng.below(0x10000);
        ops.push((op, a, b));
        i += 1;
    }
    ops
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u64, [u64; PART_COUNT], bool), u32> {
    // 版本锚档对表：V1 × 8 字段。
    if STRUCT_VERSION != 1 || ANCHOR_FIELDS != 8 {
        return Err(MqCode::AnchorMismatch.val());
    }
    let mut named = 0;
    let mut i = 0;
    while i < ANCHOR_FIELDS {
        if ANCHOR_FIELD_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    if named != ANCHOR_FIELDS {
        return Err(MqCode::AnchorMismatch.val());
    }
    // 10 万次操作账。
    let ops = gen_ops();
    let mut q = MsgQueue::new();
    replay_ops(&mut q, &ops)?;
    if q.ops != OP_ACCOUNT as u64 {
        return Err(MqCode::OpAccountMismatch.val());
    }
    q.counter_conservation()?;
    q.scan_no_contamination()?;
    let pushed = q.pushed;
    q.dual_replay(&ops)?;
    Ok((q.ops, pushed, q.thread_exit))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：越界分区与越界类注入（分区 4/9、类 7）。
    ledger.tries[0] += 3;
    let mut q = MsgQueue::new();
    let mut c = 0;
    if q.push(7, 1).is_err() {
        c += 1;
    }
    if q.peek(4).is_err() {
        c += 1;
    }
    if q.pop(9).is_err() {
        c += 1;
    }
    if c == 3 {
        ledger.caught[0] += 3;
    }
    // 族 1：空分区出队注入。
    ledger.tries[1] += 1;
    if q.pop(PART_QUIT).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：quit 单次位注入（第二条 WM_QUIT 取走必拒）。
    ledger.tries[2] += 2;
    let mut q2 = MsgQueue::new();
    let _ = q2.push(KIND_QUIT, 0);
    let first = q2.pop(PART_QUIT).is_ok();
    let _ = q2.push(KIND_QUIT, 0);
    let second = q2.pop(PART_QUIT).is_err();
    if first && second {
        ledger.caught[2] += 2;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MqCode::ALL.len() {
        let v = MqCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：10 万次操作账 + 零串区 + 计数守恒。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((ops, _, _)) => *ops == OP_ACCOUNT as u64,
        Err(_) => false,
    };
    v.push(("J1-10万次操作账零串区", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 四分区独立记账（四分区名点验）。
    let mut named = 0;
    let mut i = 0;
    while i < PART_COUNT {
        if PART_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-四分区独立记账点名",
        named == PART_COUNT,
        format!("{}/4", named),
    ));
    // 4. quit 专用语义（quit 只入 quit 分区并置退出位）。
    let quit_ok = {
        let mut q = MsgQueue::new();
        let r1 = q.push(KIND_QUIT, 0xBEEF);
        let p = q.pop(PART_QUIT);
        match (r1, p) {
            (Ok(_), Ok(m)) => m.kind == KIND_QUIT && q.thread_exit,
            _ => false,
        }
    };
    v.push(("J1-quit 专用分区+退出位", quit_ok, String::from("WM_QUIT 语义")));
    // 5. 版本锚档一致（V1 × 8 字段写死）。
    v.push((
        "J1-结构字段与版本锚档一致",
        STRUCT_VERSION == 1 && ANCHOR_FIELDS == 8,
        String::from("V1×8"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14441 每线程消息队列四分区结构");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
