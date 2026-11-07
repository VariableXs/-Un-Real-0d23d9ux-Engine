//! UNX-F51650 B23 恢复收敛总判据（×10 族错误恢复 ≤3s） · 0xC2540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F51651-J1 断言 ×50
//! 全过；UNX-F51651-J1R 链损坏注入 10/10 检出；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - ConsistCode     诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - EntryState      环项相态机（Live→Cancelled/Completed 唯一落定）
//!   - PostCancelRing  取消后环一致性引擎（取消/完成落定 + 链校验）
//!   - CorruptLedger   链损坏留账（注入=检出+逃逸，逃逸恒 0）
//!   - run_unx51650_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 取消后环一致性总闸：每次取消（或完成）落定后必须全链自检——
//!      深度记账 = 在环活项数、期位序列连续、无取消残留载荷、无悬空
//!      项；任何一项不符即链损坏，检出留账。
//!   ② 断言 ×50 全过（J1）：50 轮「入环→取消→自检→推进」性质断言，
//!      逐轮核验取消后环一致性（深度/期位/残留四要素）。
//!   ③ 链损坏注入 10/10 检出（J1R）：期位翻转、深度说谎、双取消、
//!      取消已完成项、越界取消逐类注入，一致性核验器必须 10/10 拦截。
//!   ④ 环项唯一落定：Live 只允许落定到 Cancelled 或 Completed 之一，
//!      二次落定显性拒；取消后载荷必须清零（无残留）。
//!   ⑤ 完成写回：一致性总闸收口写状态=已完成、结果摘要非空回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——LCG 种子由
//!      SEG_BASE 派生；全程零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2540;

/// 环容量。
pub const RING_CAP: usize = 12;
/// 断言扫描轮数（J1：×50 全过）。
pub const ASSERT_ROUNDS: u32 = 50;

/// 诊断码闭集（取消后环一致性域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsistCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 双重取消
    DoubleCancel = 3,
    /// 取消已完成项
    CancelAfterComplete = 4,
    /// 取消索引越界
    CancelOob = 5,
    /// 期位损坏（链损坏形态一）
    CycleCorrupt = 6,
    /// 深度说谎（链损坏形态二）
    DepthLie = 7,
    /// 取消残留载荷（链损坏形态三）
    CancelResidue = 8,
    /// 悬空项（有期位无载荷）
    OrphanEntry = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 一致性未过即宣称收口
    ConsistSkip = 12,
}

impl ConsistCode {
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            ConsistCode::Ok => "环一致性核验通过",
            ConsistCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            ConsistCode::DoubleCancel => "双重取消：环项已落定 Cancelled 不得再取消",
            ConsistCode::CancelAfterComplete => "取消已完成项：已落定 Completed 不得再取消",
            ConsistCode::CancelOob => "取消越界：环项索引必须在容量之内",
            ConsistCode::CycleCorrupt => "期位损坏：期位序列与建模值不符，判链损坏",
            ConsistCode::DepthLie => "深度说谎：深度记账与在环活项数不符",
            ConsistCode::CancelResidue => "取消残留：已取消环项载荷未清零",
            ConsistCode::OrphanEntry => "悬空项：期位在案但载荷缺失，判链损坏",
            ConsistCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            ConsistCode::WritebackEmpty => "写回摘要为空：一致性收口回执必须带非空结果摘要",
            ConsistCode::ConsistSkip => "跳过自检：一致性未过不得宣称收口",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [ConsistCode; 12] = [
    ConsistCode::Ok,
    ConsistCode::SegBad,
    ConsistCode::DoubleCancel,
    ConsistCode::CancelAfterComplete,
    ConsistCode::CancelOob,
    ConsistCode::CycleCorrupt,
    ConsistCode::DepthLie,
    ConsistCode::CancelResidue,
    ConsistCode::OrphanEntry,
    ConsistCode::LedgerMismatch,
    ConsistCode::WritebackEmpty,
    ConsistCode::ConsistSkip,
];

/// 环项状态（Live 唯一落定到两侧之一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 在环
    Live = 0,
    /// 已取消（载荷清零）
    Cancelled = 1,
    /// 已完成
    Completed = 2,
}

impl EntryState {
    /// 状态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文状态名。
    pub fn name(self) -> &'static str {
        match self {
            EntryState::Live => "在环",
            EntryState::Cancelled => "已取消",
            EntryState::Completed => "已完成",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConsistWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
    /// 核验过的环项数
    pub verified: u32,
}

/// 环项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 载荷（取消后必须清零）
    pub payload: u64,
    /// 期位（建模值 = 槽序号奇偶翻转基准）
    pub cycle: u64,
    /// 状态
    pub state: EntryState,
}

/// 取消后环一致性引擎。
pub struct PostCancelRing {
    entries: [RingEntry; RING_CAP],
    depth: u32,
    cancels: u32,
    completes: u32,
    corrupts: u32,
    ledger: Vec<(u32, String)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PostCancelRing {
    /// 新环：全空、期位基线建立。
    pub fn new() -> PostCancelRing {
        let mut entries = [RingEntry { payload: 0, cycle: 0, state: EntryState::Live }; RING_CAP];
        let mut i = 0usize;
        while i < RING_CAP {
            // 空槽以 Cancelled+零载荷 建模（可复用空闲位）。
            entries[i] = RingEntry { payload: 0, cycle: (i as u64) ^ 0xA5, state: EntryState::Cancelled };
            i += 1;
        }
        PostCancelRing {
            entries,
            depth: 0,
            cancels: 0,
            completes: 0,
            corrupts: 0,
            ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 入环一个活项。
    pub fn post(&mut self, idx: usize, payload: u64) -> Result<(), ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                // 只允许向空闲槽（Cancelled+零载荷）入环。
                if e.state != EntryState::Cancelled || e.payload != 0 {
                    return Err(ConsistCode::DoubleCancel);
                }
                e.payload = payload;
                e.state = EntryState::Live;
                self.depth = self.depth.saturating_add(1);
                Ok(())
            }
            None => Err(ConsistCode::CancelOob),
        }
    }

    /// 环项状态读面（越界 None）。
    pub fn entry_state(&self, idx: usize) -> Option<EntryState> {
        self.entries.get(idx).map(|e| e.state)
    }

    /// 取消：Live→Cancelled，载荷清零；二次取消/取消已完成项显性拒。
    pub fn cancel(&mut self, idx: usize) -> Result<EntryState, ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => match e.state {
                EntryState::Live => {
                    e.state = EntryState::Cancelled;
                    e.payload = 0;
                    self.depth = self.depth.saturating_sub(1);
                    self.cancels = self.cancels.saturating_add(1);
                    Ok(e.state)
                }
                EntryState::Cancelled => Err(ConsistCode::DoubleCancel),
                EntryState::Completed => Err(ConsistCode::CancelAfterComplete),
            },
            None => Err(ConsistCode::CancelOob),
        }
    }

    /// 完成：Live→Completed，载荷保留供上层取件。
    pub fn complete(&mut self, idx: usize) -> Result<EntryState, ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => match e.state {
                EntryState::Live => {
                    e.state = EntryState::Completed;
                    self.depth = self.depth.saturating_sub(1);
                    self.completes = self.completes.saturating_add(1);
                    Ok(e.state)
                }
                EntryState::Cancelled => Err(ConsistCode::CancelResidue),
                EntryState::Completed => Err(ConsistCode::DoubleCancel),
            },
            None => Err(ConsistCode::CancelOob),
        }
    }

    /// 释放已完成槽回空闲位（Completed→Cancelled 零载荷），供环项复用。
    pub fn release(&mut self, idx: usize) -> Result<EntryState, ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                if e.state != EntryState::Completed {
                    return Err(ConsistCode::ConsistSkip);
                }
                e.payload = 0;
                e.state = EntryState::Cancelled;
                Ok(e.state)
            }
            None => Err(ConsistCode::CancelOob),
        }
    }

    /// 一致性全链自检：深度/期位/残留/悬空四要素。
    pub fn verify(&mut self) -> Result<u32, ConsistCode> {
        // 要素一：深度记账 = 在环活项数。
        let mut live = 0u32;
        let mut i = 0usize;
        while i < RING_CAP {
            if let Some(e) = self.entries.get(i) {
                if e.state == EntryState::Live {
                    live += 1;
                }
            }
            i += 1;
        }
        if self.depth != live {
            self.corrupts = self.corrupts.saturating_add(1);
            self.ledger.push((ConsistCode::DepthLie as u32, String::from("深度说谎：记账深度与活项数不符，判链损坏")));
            return Err(ConsistCode::DepthLie);
        }
        // 要素二/三/四：期位、取消残留、悬空。
        let mut i = 0usize;
        while i < RING_CAP {
            let (cycle_ok, residue, orphan) = match self.entries.get(i) {
                Some(e) => (e.cycle == (i as u64) ^ 0xA5, e.state == EntryState::Cancelled && e.payload != 0, e.state == EntryState::Live && e.payload == 0),
                None => (true, false, false),
            };
            if !cycle_ok {
                self.corrupts = self.corrupts.saturating_add(1);
                self.ledger.push((ConsistCode::CycleCorrupt as u32, String::from("期位损坏：期位与建模基线不符，判链损坏")));
                return Err(ConsistCode::CycleCorrupt);
            }
            if residue {
                self.corrupts = self.corrupts.saturating_add(1);
                self.ledger.push((ConsistCode::CancelResidue as u32, String::from("取消残留：已取消项载荷未清零，判链损坏")));
                return Err(ConsistCode::CancelResidue);
            }
            if orphan {
                self.corrupts = self.corrupts.saturating_add(1);
                self.ledger.push((ConsistCode::OrphanEntry as u32, String::from("悬空项：活项无载荷，判链损坏")));
                return Err(ConsistCode::OrphanEntry);
            }
            i += 1;
        }
        Ok(RING_CAP as u32)
    }

    /// 一致性收口写回：自检必须先过（跳过自检显性拒）。
    pub fn finalize(&mut self) -> Result<ConsistWriteback, ConsistCode> {
        let verified = self.verify()?;
        Ok(ConsistWriteback {
            done: true,
            summary: format!(
                "一致性收口：核验 {} 槽全过，取消 {} 项 / 完成 {} 项，链损坏检出 {} 次",
                verified, self.cancels, self.completes, self.corrupts
            ),
            verified,
        })
    }

    /// 链损坏留账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 第 k 条留账。
    pub fn ledger_at(&self, k: usize) -> Option<&(u32, String)> {
        self.ledger.get(k)
    }

    /// 链损坏检出计数。
    pub fn corrupts(&self) -> u32 {
        self.corrupts
    }

    /// 取消计数。
    pub fn cancels(&self) -> u32 {
        self.cancels
    }

    /// 深度读面。
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// 注入期位翻转（链损坏注入核专用探针）。
    pub fn inject_cycle_flip(&mut self, idx: usize) -> Result<(), ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.cycle = e.cycle.wrapping_add(1);
                Ok(())
            }
            None => Err(ConsistCode::CancelOob),
        }
    }

    /// 注入深度说谎（链损坏注入核专用探针）。
    pub fn inject_depth_lie(&mut self, delta: u32) {
        self.depth = self.depth.wrapping_add(delta);
    }

    /// 注入取消残留（链损坏注入核专用探针）。
    pub fn inject_residue(&mut self, idx: usize) -> Result<(), ConsistCode> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                if e.state == EntryState::Cancelled {
                    e.payload = 0xDEAD;
                    Ok(())
                } else {
                    Err(ConsistCode::CancelResidue)
                }
            }
            None => Err(ConsistCode::CancelOob),
        }
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

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u32, u32, u32, u32, usize, u64, u64) {
        (
            self.depth,
            self.cancels,
            self.completes,
            self.corrupts,
            self.ledger.len(),
            self.injected,
            self.detected,
        )
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

/// 引擎指纹：对确定性脚本做 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let mut r = PostCancelRing::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(2654435761).wrapping_add(1);
    let mut log: String = String::new();
    for round in 0..64u32 {
        let op = lcg_next(&mut seed) % 5;
        let idx = (lcg_next(&mut seed) % (RING_CAP as u64 + 1)) as usize;
        let payload = lcg_next(&mut seed);
        let run = match op {
            0 => r.post(idx % RING_CAP, payload).map(|_| 0u32),
            1 => r.cancel(idx % RING_CAP).map(|s| s.code()),
            2 => r.complete(idx % RING_CAP).map(|s| s.code()),
            3 => r.verify(),
            _ => r.finalize().map(|w| w.verified),
        };
        log.push_str(&format!("{}:{:?};", round, run.err()));
    }
    let (d, c, p, x, l, i, det) = r.snapshot();
    let full = format!("{}|{},{},{},{},{},{},{}", log, d, c, p, x, l, i, det);
    fnv1a(full.as_bytes())
}

/// 双跑一致性：同一脚本两次执行指纹必须逐位一致。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut r = PostCancelRing::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：双重取消 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let idx = i as usize % RING_CAP;
            let _ = r.post(idx, 0x100 + i as u64);
            let _ = r.cancel(idx);
            let run = r.cancel(idx);
            if run == Err(ConsistCode::DoubleCancel) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
            let _ = lcg_next(&mut seed) + i as u64;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：取消已完成项 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let idx = (i as usize + 1) % RING_CAP;
            let _ = r.post(idx, 0x200 + i as u64);
            let _ = r.complete(idx);
            let run = r.cancel(idx);
            if run == Err(ConsistCode::CancelAfterComplete) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：期位翻转 ×10（verify 10/10 拦截）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let idx = (i as usize + 2) % RING_CAP;
            let _ = r.inject_cycle_flip(idx);
            let run = r.verify();
            if run == Err(ConsistCode::CycleCorrupt) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
            // 复原期位。
            if let Some(e) = r.entries.get_mut(idx) {
                e.cycle = (idx as u64) ^ 0xA5;
            }
            r.corrupts = r.corrupts.saturating_sub(1);
            let _ = r.ledger.pop();
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：深度说谎 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            r.inject_depth_lie(1);
            let run = r.verify();
            if run == Err(ConsistCode::DepthLie) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
            r.inject_depth_lie(u32::MAX); // -1 回滚
            r.corrupts = r.corrupts.saturating_sub(1);
            let _ = r.ledger.pop();
            let _ = i;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：取消越界 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let run = r.cancel(RING_CAP + i as usize);
            if run == Err(ConsistCode::CancelOob) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (r.injected(), r.detected(), r.escaped(), classes_hit)
}

/// 断言 ×50 全过（J1）：入环→取消→自检→收口逐轮性质断言。
pub fn assertions_50_all_pass() -> bool {
    let mut r = PostCancelRing::new();
    for round in 0..ASSERT_ROUNDS {
        let a = round as usize % RING_CAP;
        let b = (round as usize + 1) % RING_CAP;
        // 断言1：入环成功。
        if r.post(a, 0x7000 + round as u64).is_err() {
            return false;
        }
        // 断言2：取消后深度归位。
        let d0 = r.depth();
        if r.cancel(a).is_err() || r.depth() != d0 - 1 {
            return false;
        }
        // 断言3：取消后全链自检通过（四要素）。
        if r.verify() != Ok(RING_CAP as u32) {
            return false;
        }
        // 断言4：取消后同槽可复用（无残留）。
        if r.post(a, 0x7100 + round as u64).is_err() {
            return false;
        }
        // 断言5：完成落定后环仍一致。
        if r.complete(a).is_err() {
            return false;
        }
        if r.verify() != Ok(RING_CAP as u32) {
            return false;
        }
        // 断言6：释放后槽位复用就绪。
        if r.release(a) != Ok(EntryState::Cancelled) {
            return false;
        }
        let _ = b;
    }
    r.finalize().is_ok()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51650_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51650");
    // 1 入环/取消/完成基本链。
    {
        let mut r = PostCancelRing::new();
        let p = r.post(0, 0x11);
        let c = r.cancel(0);
        let st = r.entry_state(0);
        s.add("取消落定链", p.is_ok() && c == Ok(EntryState::Cancelled) && st == Some(EntryState::Cancelled), "取消后环项落定 Cancelled 且载荷清零");
    }
    // 2 断言 ×50 全过。
    s.add("断言五十轮全过", assertions_50_all_pass(), "50 轮入环→取消→自检→落定性质断言须全过");
    // 3 双重取消 10/10 检出。
    {
        let mut r = PostCancelRing::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let idx = i as usize % RING_CAP;
            let _ = r.post(idx, 0x100 + i as u64);
            let _ = r.cancel(idx);
            if r.cancel(idx) == Err(ConsistCode::DoubleCancel) {
                caught += 1;
            }
        }
        s.add("双取消十连检出", caught == 10, "双重取消 ×10 须 10/10 显性拒");
    }
    // 4 链损坏注入（期位翻转）10/10 检出。
    {
        let mut r = PostCancelRing::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let idx = i as usize % RING_CAP;
            let _ = r.inject_cycle_flip(idx);
            if r.verify() == Err(ConsistCode::CycleCorrupt) {
                caught += 1;
            }
            if let Some(e) = r.entries.get_mut(idx) {
                e.cycle = (idx as u64) ^ 0xA5;
            }
            r.corrupts = r.corrupts.saturating_sub(1);
            let _ = r.ledger.pop();
        }
        s.add("期位损坏检出", caught == 10, "期位翻转注入 ×10 须 10/10 拦截");
    }
    // 5 深度说谎 10/10 检出。
    {
        let mut r = PostCancelRing::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            r.inject_depth_lie(1);
            if r.verify() == Err(ConsistCode::DepthLie) {
                caught += 1;
            }
            r.inject_depth_lie(u32::MAX);
            r.corrupts = r.corrupts.saturating_sub(1);
            let _ = r.ledger.pop();
        }
        s.add("深度说谎检出", caught == 10, "深度记账篡改 ×10 须 10/10 拦截");
    }
    // 6 注入核：5 类 ×10 全检出、逃逸 0、类齐。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 7 注入账守恒。
    {
        let (inj, det, esc) = {
            let (i, d, x, _) = injection_core();
            (i, d, x)
        };
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 8 一致性收口写回合规。
    {
        let mut r = PostCancelRing::new();
        let _ = r.post(0, 0x21);
        let _ = r.cancel(0);
        let w = r.finalize();
        s.add(
            "收口写回合规",
            matches!(w, Ok(ref x) if x.done && !x.summary.is_empty() && x.verified == RING_CAP as u32),
            "自检全过后收口写回须状态=已完成且摘要非空",
        );
    }
    // 9 越界取消显性拒。
    {
        let mut r = PostCancelRing::new();
        s.add("越界取消拒", r.cancel(RING_CAP + 3) == Err(ConsistCode::CancelOob), "越界环项取消必须显性拒");
    }
    // 10 链损坏留账齐全。
    {
        let mut r = PostCancelRing::new();
        let _ = r.inject_cycle_flip(0);
        let _ = r.verify();
        let ok = match r.ledger_at(0) {
            Some((code, txt)) => *code == ConsistCode::CycleCorrupt as u32 && !txt.is_empty(),
            None => false,
        };
        s.add("链损坏留账齐全", ok, "链损坏必须落账且文本非空（零静默）");
    }
    // 11 诊断码闭集唯一且文案非空。
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
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC2540 须 0x40 对齐且在分派区间");
    // 15 指纹非退化。
    {
        let h1 = fnv1a(b"unx51650-probe-a");
        let h2 = fnv1a(b"unx51650-probe-b");
        s.add("指纹非退化", h1 != h2 && h1 != 0, "FNV-1a 对不同输入必须给出不同指纹");
    }
    // 16 LCG 确定性。
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
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_cancel_chain() {
        let mut r = PostCancelRing::new();
        assert!(r.post(0, 0x11).is_ok());
        assert_eq!(r.cancel(0), Ok(EntryState::Cancelled));
        assert_eq!(r.depth(), 0);
    }

    #[test]
    fn post_complete_chain() {
        let mut r = PostCancelRing::new();
        assert!(r.post(1, 0x12).is_ok());
        assert_eq!(r.complete(1), Ok(EntryState::Completed));
        assert_eq!(r.cancel(1), Err(ConsistCode::CancelAfterComplete));
    }

    #[test]
    fn double_cancel_rejected() {
        let mut r = PostCancelRing::new();
        let _ = r.post(0, 0x13);
        let _ = r.cancel(0);
        assert_eq!(r.cancel(0), Err(ConsistCode::DoubleCancel));
    }

    #[test]
    fn cancel_oob_rejected() {
        let mut r = PostCancelRing::new();
        assert_eq!(r.cancel(RING_CAP + 2), Err(ConsistCode::CancelOob));
        assert!(r.entry_state(RING_CAP + 2).is_none());
    }

    #[test]
    fn verify_clean_ring() {
        let mut r = PostCancelRing::new();
        assert_eq!(r.verify(), Ok(RING_CAP as u32));
    }

    #[test]
    fn cycle_flip_detected() {
        let mut r = PostCancelRing::new();
        let _ = r.inject_cycle_flip(3);
        assert_eq!(r.verify(), Err(ConsistCode::CycleCorrupt));
    }

    #[test]
    fn depth_lie_detected() {
        let mut r = PostCancelRing::new();
        r.inject_depth_lie(2);
        assert_eq!(r.verify(), Err(ConsistCode::DepthLie));
    }

    #[test]
    fn assertions_50() {
        assert!(assertions_50_all_pass());
    }

    #[test]
    fn finalize_writeback() {
        let mut r = PostCancelRing::new();
        let _ = r.post(0, 0x14);
        let _ = r.cancel(0);
        let w = r.finalize();
        assert!(w.is_ok());
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn ledger_conserved() {
        let mut r = PostCancelRing::new();
        for _ in 0..10 {
            r.note_detected();
        }
        assert!(r.conserved());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(engine_fingerprint(), engine_fingerprint());
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
    fn unx51650_checks_green() {
        let set = run_unx51650_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
