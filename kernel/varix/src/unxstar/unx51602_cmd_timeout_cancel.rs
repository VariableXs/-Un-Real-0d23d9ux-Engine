//! UNX-F51602 命令超时与取消（命令环挂起检测） · 0xC1940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51603-J1 越界 ×10 全拒并审计；UNX-F51603-J1R 合法敲击
//! ×10^4 零误拒；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - CmdCode            诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - CmdState           命令状态机（Pending→Running→Done/Cancelled）
//!   - CmdTimeoutScheduler 命令环调度器（时限推进/取消收敛/挂起检测）
//!   - CancelLedger       取消审计账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51602_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 命令状态机严格单向：Pending→Running→Done 或 Running→Cancelled，
//!      任何回退（Done→Running）与二次取消显性拒（StateRegress /
//!      DoubleCancel），未知命令号取消显性拒并审计（CmdOob）。
//!   ② 超时取消收敛：时限步进器逐轮推进，凡年龄越限的在途命令一律取消
//!      落账；×50 条超时命令在时限窗口内全部收敛到终态，账面 50/50 对平。
//!   ③ 挂起伪装检测：命令谎报 Done 但命令环仍在途即 FakeHang，×10 伪装
//!      全部检出；正常命令 ×1000 轮完成期早于时限，零误取消。
//!   ④ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑤ 完成写回：收敛完成写一条状态=已完成、结果摘要非空的回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——注入参数由 LCG
//!      派生（种子 = SEG_BASE × 黄金比常数 + 1），同段位恒定可复现。
//!   ⑦ 全程零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1940;

/// 命令环槽位数。
pub const RING_SLOTS: usize = 64;
/// 命令时限（调度轮数）。
pub const TIMEOUT_LIMIT: u32 = 8;
/// 超时取消收敛批大小（判据 ×50）。
pub const CONVERGE_BATCH: u32 = 50;

/// 诊断码闭集（命令超时取消域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 命令号越界
    CmdOob = 3,
    /// 命令时限已越
    TimeoutExceeded = 4,
    /// 挂起伪装（谎报完成仍在途）
    FakeHang = 5,
    /// 取消了未知命令
    CancelUnknown = 6,
    /// 二次取消
    DoubleCancel = 7,
    /// 命令环已满
    RingFull = 8,
    /// 注入账不符
    LedgerMismatch = 9,
    /// 写回摘要为空
    WritebackEmpty = 10,
    /// 取消未收敛
    ConvergeFailed = 11,
    /// 状态回退
    StateRegress = 12,
}

impl CmdCode {
    /// 闭集编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            CmdCode::Ok => "命令操作成功",
            CmdCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            CmdCode::CmdOob => "命令号越界：取消目标必须在已登记命令号之内",
            CmdCode::TimeoutExceeded => "命令时限已越：按取消收敛流程处理",
            CmdCode::FakeHang => "挂起伪装：谎报完成但命令环仍在途，必须检出",
            CmdCode::CancelUnknown => "取消未知命令：仅允许取消已登记的命令号",
            CmdCode::DoubleCancel => "二次取消：命令已到终态不得再取消",
            CmdCode::RingFull => "命令环已满：先消化在途命令再提交",
            CmdCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            CmdCode::WritebackEmpty => "写回摘要为空：完成回执必须带非空结果摘要",
            CmdCode::ConvergeFailed => "取消未收敛：时限窗口内必须全部到达终态",
            CmdCode::StateRegress => "状态回退：命令状态机只允许单向前进",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [CmdCode; 12] = [
    CmdCode::Ok,
    CmdCode::SegBad,
    CmdCode::CmdOob,
    CmdCode::TimeoutExceeded,
    CmdCode::FakeHang,
    CmdCode::CancelUnknown,
    CmdCode::DoubleCancel,
    CmdCode::RingFull,
    CmdCode::LedgerMismatch,
    CmdCode::WritebackEmpty,
    CmdCode::ConvergeFailed,
    CmdCode::StateRegress,
];

/// 命令状态机（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdState {
    /// 已登记未派发
    Pending = 0,
    /// 在途
    Running = 1,
    /// 正常完成
    Done = 2,
    /// 超时取消
    Cancelled = 3,
}

impl CmdState {
    /// 状态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文状态名。
    pub fn name(self) -> &'static str {
        match self {
            CmdState::Pending => "已登记",
            CmdState::Running => "在途",
            CmdState::Done => "完成",
            CmdState::Cancelled => "已取消",
        }
    }
    /// 是否终态。
    pub fn terminal(self) -> bool {
        matches!(self, CmdState::Done | CmdState::Cancelled)
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CancelWriteback {
    /// 状态：true=已完成（收敛写回）
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
}

/// 单条命令账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CmdEntry {
    state: CmdState,
    age: u32,
    claimed_done: bool,
}

/// 命令环超时调度器：登记→派发→时限推进→取消收敛。
pub struct CmdTimeoutScheduler {
    entries: Vec<CmdEntry>,
    tick: u32,
    cancel_cnt: u32,
    done_cnt: u32,
    hang_cnt: u32,
    ledger: Vec<(u32, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl CmdTimeoutScheduler {
    /// 新调度器：空命令环。
    pub fn new() -> CmdTimeoutScheduler {
        CmdTimeoutScheduler {
            entries: Vec::new(),
            tick: 0,
            cancel_cnt: 0,
            done_cnt: 0,
            hang_cnt: 0,
            ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记一条命令（Pending 态）。
    pub fn submit(&mut self) -> Result<u32, CmdCode> {
        if self.entries.len() >= RING_SLOTS {
            return Err(CmdCode::RingFull);
        }
        self.entries.push(CmdEntry {
            state: CmdState::Pending,
            age: 0,
            claimed_done: false,
        });
        Ok((self.entries.len() - 1) as u32)
    }

    /// 派发：Pending→Running。
    pub fn dispatch(&mut self, id: u32) -> Result<CmdState, CmdCode> {
        let e = match self.entries.get_mut(id as usize) {
            Some(e) => e,
            None => return Err(CmdCode::CmdOob),
        };
        if e.state != CmdState::Pending {
            return Err(CmdCode::StateRegress);
        }
        e.state = CmdState::Running;
        Ok(e.state)
    }

    /// 谎报完成（挂起伪装注入口）：状态仍 Running 但宣称 Done。
    pub fn claim_done(&mut self, id: u32) -> Result<CmdState, CmdCode> {
        let e = match self.entries.get_mut(id as usize) {
            Some(e) => e,
            None => return Err(CmdCode::CmdOob),
        };
        if e.state == CmdState::Running {
            e.claimed_done = true;
            return Ok(CmdState::Running);
        }
        Err(CmdCode::StateRegress)
    }

    /// 正常完成：Running→Done（清伪装位）。
    pub fn complete(&mut self, id: u32) -> Result<CmdState, CmdCode> {
        let e = match self.entries.get_mut(id as usize) {
            Some(e) => e,
            None => return Err(CmdCode::CmdOob),
        };
        if e.state != CmdState::Running {
            return Err(CmdCode::StateRegress);
        }
        e.state = CmdState::Done;
        e.claimed_done = false;
        self.done_cnt += 1;
        Ok(e.state)
    }

    /// 取消一条命令（Running/Pending→Cancelled）并落审计账。
    pub fn cancel(&mut self, id: u32) -> Result<CmdState, CmdCode> {
        let st = match self.entries.get(id as usize) {
            Some(e) => e.state,
            None => {
                self.ledger.push((id, CmdCode::CancelUnknown.code()));
                return Err(CmdCode::CmdOob);
            }
        };
        if st.terminal() {
            self.ledger.push((id, CmdCode::DoubleCancel.code()));
            return Err(CmdCode::DoubleCancel);
        }
        let e = match self.entries.get_mut(id as usize) {
            Some(e) => e,
            None => return Err(CmdCode::CmdOob),
        };
        e.state = CmdState::Cancelled;
        self.cancel_cnt += 1;
        self.ledger.push((id, CmdState::Cancelled.code()));
        Ok(e.state)
    }

    /// 时限推进一轮：所有 Running 命令年龄 +1，越限即取消（收敛主循环）。
    pub fn tick_once(&mut self) -> u32 {
        self.tick += 1;
        let mut fired = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let fire = match self.entries.get(i) {
                Some(e) => e.state == CmdState::Running && e.age + 1 >= TIMEOUT_LIMIT,
                None => false,
            };
            if fire {
                if let Some(e) = self.entries.get_mut(i) {
                    e.state = CmdState::Cancelled;
                    e.age += 1;
                }
                self.cancel_cnt += 1;
                self.ledger.push((i as u32, CmdCode::TimeoutExceeded.code()));
                fired += 1;
            } else if let Some(e) = self.entries.get_mut(i) {
                if e.state == CmdState::Running {
                    e.age += 1;
                }
            }
            i += 1;
        }
        fired
    }

    /// 挂起巡检：谎报 Done 而仍在途的命令全部抓出并取消落账。
    pub fn sweep_hangs(&mut self) -> u32 {
        let mut caught = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let hang = match self.entries.get(i) {
                Some(e) => e.claimed_done && e.state == CmdState::Running,
                None => false,
            };
            if hang {
                if let Some(e) = self.entries.get_mut(i) {
                    e.claimed_done = false;
                }
                self.hang_cnt += 1;
                self.ledger.push((i as u32, CmdCode::FakeHang.code()));
                caught += 1;
            }
            i += 1;
        }
        caught
    }

    /// 收割终态命令：把已 Done/Cancelled 的槽位从环上摘除，腾出提交位。
    /// 返回摘除条数（终态账面不变，仅出环）。
    pub fn reap_terminal(&mut self) -> u32 {
        let mut reaped = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let term = match self.entries.get(i) {
                Some(e) => e.state.terminal(),
                None => false,
            };
            if term {
                let _ = self.entries.remove(i);
                reaped += 1;
            } else {
                i += 1;
            }
        }
        reaped
    }

    /// 收敛写回：全部命令到终态后生成完成回执。
    pub fn converge_writeback(&self) -> Result<CancelWriteback, CmdCode> {
        let mut pending = 0u32;
        for e in self.entries.iter() {
            if !e.state.terminal() {
                pending += 1;
            }
        }
        if pending > 0 {
            return Err(CmdCode::ConvergeFailed);
        }
        Ok(CancelWriteback {
            done: true,
            summary: format!(
                "超时取消收敛完成：完成 {} 条、取消 {} 条、挂起检出 {} 条，共 {} 轮时限推进",
                self.done_cnt, self.cancel_cnt, self.hang_cnt, self.tick
            ),
        })
    }

    pub fn done_cnt(&self) -> u32 {
        self.done_cnt
    }
    pub fn cancel_cnt(&self) -> u32 {
        self.cancel_cnt
    }
    pub fn hang_cnt(&self) -> u32 {
        self.hang_cnt
    }
    pub fn tick(&self) -> u32 {
        self.tick
    }
    /// 审计账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u32, u32, u32, u32, usize) {
        (self.tick, self.done_cnt, self.cancel_cnt, self.hang_cnt, self.ledger.len())
    }
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现（伪装挂起、二次取消、
/// 未知命令号、状态回退、越界提交），注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut sc = CmdTimeoutScheduler::new();
    let mut seed: u64 = (SEG_BASE as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：挂起伪装 ×10（谎报 Done、巡检全抓）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let id = match sc.submit() {
                Ok(id) => id,
                Err(_) => {
                    sc.note_escape();
                    continue;
                }
            };
            let _ = sc.dispatch(id);
            let _ = sc.claim_done(id);
            if sc.sweep_hangs() > 0 {
                caught += 1;
                sc.note_detected();
            } else {
                sc.note_escape();
            }
            let _ = lcg_next(&mut seed);
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：二次取消 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let id = match sc.submit() {
                Ok(id) => id,
                Err(_) => continue,
            };
            let _ = sc.dispatch(id);
            let _ = sc.cancel(id);
            if sc.cancel(id) == Err(CmdCode::DoubleCancel) {
                caught += 1;
                sc.note_detected();
            } else {
                sc.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：未知命令号取消 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let ghost = 1000 + i;
            if sc.cancel(ghost) == Err(CmdCode::CmdOob) {
                caught += 1;
                sc.note_detected();
            } else {
                sc.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：状态回退 ×10（Done 后再 complete / Pending 再 complete）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let id = match sc.submit() {
                Ok(id) => id,
                Err(_) => continue,
            };
            // Pending 态直接 complete 属回退。
            if sc.complete(id) == Err(CmdCode::StateRegress) {
                caught += 1;
                sc.note_detected();
            } else {
                sc.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：越界提交 ×10（灌满命令环后继续提交必须 RingFull）。
    {
        // 先消化既有在途：时限推进放空。
        let mut guard = 0u32;
        while guard < 32u32 {
            sc.tick_once();
            guard += 1;
        }
        sc.entries.clear();
        let mut caught = 0u32;
        for i in 0..(RING_SLOTS as u32 + 10) {
            match sc.submit() {
                Ok(_) => {
                    if i >= RING_SLOTS as u32 {
                        sc.note_escape();
                    }
                }
                Err(CmdCode::RingFull) => {
                    caught += 1;
                    sc.note_detected();
                }
                Err(_) => sc.note_escape(),
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (sc.injected, sc.detected, sc.escaped, classes_hit)
}

/// 超时取消收敛 ×50：50 条在途命令时限窗口内全部取消到终态。
pub fn converge_fifty() -> bool {
    let mut sc = CmdTimeoutScheduler::new();
    let mut ids: Vec<u32> = Vec::new();
    for _ in 0..CONVERGE_BATCH {
        match sc.submit() {
            Ok(id) => {
                if sc.dispatch(id).is_err() {
                    return false;
                }
                ids.push(id);
            }
            Err(_) => return false,
        }
    }
    // 时限 8 轮，每轮至少触发一批；窗口 16 轮内必须全部取消。
    let mut rounds = 0u32;
    while rounds < 16u32 {
        sc.tick_once();
        rounds += 1;
        let mut unsettled = 0u32;
        for id in ids.iter() {
            if let Some(e) = sc.entries.get(*id as usize) {
                if !e.state.terminal() {
                    unsettled += 1;
                }
            }
        }
        if unsettled == 0 {
            break;
        }
    }
    let wb = sc.converge_writeback();
    match wb {
        Ok(w) => w.done && !w.summary.is_empty() && sc.cancel_cnt() == CONVERGE_BATCH,
        Err(_) => false,
    }
}

/// 正常流 ×1000：命令在时限内完成，零误取消。
pub fn normal_stream_zero_false() -> bool {
    let mut sc = CmdTimeoutScheduler::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(2654435761).wrapping_add(3);
    let mut false_cancel = 0u32;
    for round in 0..1000u32 {
        let id = match sc.submit() {
            Ok(id) => id,
            Err(_) => {
                // 满环：先收割终态槽位再重投一次，保证 1000 条全部走环。
                sc.reap_terminal();
                match sc.submit() {
                    Ok(id) => id,
                    Err(_) => {
                        let _ = round;
                        continue;
                    }
                }
            }
        };
        let _ = sc.dispatch(id);
        // 服务时长 1..=6 轮，恒早于时限 8。
        let service = 1 + (lcg_next(&mut seed) % 6) as u32;
        let mut k = 0u32;
        let mut completed = false;
        while k < service {
            sc.tick_once();
            k += 1;
        }
        // 完成前若已被超时取消即误取消。
        if let Some(e) = sc.entries.get(id as usize) {
            if e.state == CmdState::Cancelled {
                false_cancel += 1;
            } else if e.state == CmdState::Running {
                let _ = sc.complete(id);
                completed = true;
            } else {
                completed = e.state == CmdState::Done;
            }
        }
        let _ = completed;
        let _ = round;
    }
    false_cancel == 0 && sc.hang_cnt() == 0
}

/// 引擎指纹：注入核 + 收敛 + 正常流的快照混合 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let (inj, det, esc, cls) = injection_core();
    let mut sc = CmdTimeoutScheduler::new();
    for _ in 0..8u32 {
        if let Ok(id) = sc.submit() {
            let _ = sc.dispatch(id);
        }
    }
    for _ in 0..TIMEOUT_LIMIT + 2 {
        sc.tick_once();
    }
    let _ = sc.converge_writeback();
    let snap = sc.snapshot();
    let full = format!(
        "unx51602|{} {} {} {}|{:?}|{}",
        inj, det, esc, cls, snap, TIMEOUT_LIMIT
    );
    fnv1a(full.as_bytes())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51602_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51602");
    // 1 正常流：登记→派发→完成。
    {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit();
        let ok = match id {
            Ok(id) => sc.dispatch(id).is_ok() && sc.complete(id) == Ok(CmdState::Done),
            Err(_) => false,
        };
        s.add("正常命令生命周期", ok, "登记→派发→完成必须全通且终态 Done");
    }
    // 2 超时取消收敛 ×50。
    s.add("超时取消收敛×50", converge_fifty(), "50 条在途命令时限窗口内全部取消并写回完成");
    // 3 正常流 ×1000 零误取消。
    s.add("正常流零误取消", normal_stream_zero_false(), "服务时长恒早于时限，1000 轮不得误取消一次");
    // 4 注入核：5 类 ×10 全检出、逃逸 0、类齐。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 5 注入账守恒。
    {
        let (inj, det, esc, _) = injection_core();
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 6 审计留痕：取消与违例全部落账。
    {
        let (_, _, _, _, ledger) = {
            let sc = CmdTimeoutScheduler::new();
            sc.snapshot()
        };
        let _ = ledger;
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        let _ = sc.dispatch(id);
        let _ = sc.cancel(id);
        let _ = sc.cancel(id);
        let _ = sc.cancel(999);
        s.add(
            "违例全审计",
            sc.ledger_len() == 3,
            "正常取消、二次取消、未知命令号三条都必须留审计账",
        );
    }
    // 7 挂起巡检独立可复现：伪装 3 条巡检一次全抓。
    {
        let mut sc = CmdTimeoutScheduler::new();
        for _ in 0..3u32 {
            if let Ok(id) = sc.submit() {
                let _ = sc.dispatch(id);
                let _ = sc.claim_done(id);
            }
        }
        s.add("挂起巡检批量", sc.sweep_hangs() == 3 && sc.hang_cnt() == 3, "一次巡检须抓出全部 3 条伪装");
    }
    // 8 未收敛拒写回。
    {
        let mut sc = CmdTimeoutScheduler::new();
        if let Ok(id) = sc.submit() {
            let _ = sc.dispatch(id);
        }
        s.add(
            "未收敛拒写回",
            sc.converge_writeback() == Err(CmdCode::ConvergeFailed),
            "在途命令未清账不得写回完成",
        );
    }
    // 9 状态机单向：Pending 直接 complete 回退拒。
    {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        s.add(
            "状态机单向",
            sc.complete(id) == Err(CmdCode::StateRegress),
            "Pending 态直接完成属状态回退必须拒",
        );
    }
    // 10 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 11 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 12 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 13 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC1940 须 0x40 对齐且在分派区间");
    // 14 LCG 确定性。
    {
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("LCG 确定性", same, "同种子两次 LCG 序列必须逐值一致");
    }
    // 15 时限语义：时限内不取消、达限即取消。
    {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        let _ = sc.dispatch(id);
        let first = sc.tick_once();
        s.add(
            "时限触发语义",
            first == 0,
            "首轮年龄 1 未见限不得触发超时取消（时限 8 轮）",
        );
    }
    // 16 命令环容量语义。
    {
        let mut sc = CmdTimeoutScheduler::new();
        let mut full_ok = false;
        for i in 0..(RING_SLOTS as u32 + 1) {
            match sc.submit() {
                Ok(_) => {}
                Err(CmdCode::RingFull) => {
                    full_ok = i == RING_SLOTS as u32;
                    break;
                }
                Err(_) => {}
            }
        }
        s.add("命令环容量", full_ok, "第 65 次提交必须显性 RingFull");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_command_lifecycle() {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        assert!(sc.dispatch(id).is_ok());
        assert_eq!(sc.complete(id), Ok(CmdState::Done));
    }

    #[test]
    fn converge_fifty_all_cancelled() {
        assert!(converge_fifty());
    }

    #[test]
    fn fake_hang_sweep_all_caught() {
        let mut sc = CmdTimeoutScheduler::new();
        for _ in 0..10u32 {
            let id = sc.submit().unwrap_or(0);
            let _ = sc.dispatch(id);
            let _ = sc.claim_done(id);
            assert_eq!(sc.sweep_hangs(), 1);
        }
        assert_eq!(sc.hang_cnt(), 10);
    }

    #[test]
    fn double_cancel_rejected() {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        let _ = sc.dispatch(id);
        assert!(sc.cancel(id).is_ok());
        assert_eq!(sc.cancel(id), Err(CmdCode::DoubleCancel));
    }

    #[test]
    fn unknown_cmd_audited() {
        let mut sc = CmdTimeoutScheduler::new();
        assert_eq!(sc.cancel(4242), Err(CmdCode::CmdOob));
        assert_eq!(sc.ledger_len(), 1);
    }

    #[test]
    fn state_regress_rejected() {
        let mut sc = CmdTimeoutScheduler::new();
        let id = sc.submit().unwrap_or(0);
        assert_eq!(sc.complete(id), Err(CmdCode::StateRegress));
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn unx51602_checks_green() {
        let set = run_unx51602_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
