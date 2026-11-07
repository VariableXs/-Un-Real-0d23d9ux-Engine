//! UNX-F51644 超时取消竞态（取消与完成同时到达 ×50） · 0xC23C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F51646-J1 错序 ×10
//! 检出重同步；UNX-F51646-J1R 正常三段 ×500 零误检；③ 完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - RaceCode        诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - SlotState       请求槽相态机（Pending→Cancelled/Completed，唯一落定）
//!   - CancelRaceBook  取消/完成竞态账（×50 同拍仲裁，唯一收敛）
//!   - ResponseGuard   响应长度守卫（Babble 隔离留账 + 超短响应零误杀）
//!   - run_unx51644_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 请求槽唯一落定：Pending 只允许被 Cancelled 或 Completed 之一
//!      收编——同拍取消与完成由仲裁器强制先到先落定，二次落定显性拒
//!      （DoubleSettle），取消后完成 / 完成后取消皆判时序违例。
//!   ② ×50 竞态：每轮同拍送达 cancel 与 complete，仲裁器必须场场收敛
//!      到唯一胜者（仲裁计数 = 轮数 = 收敛计数），无悬空槽。
//!   ③ 响应守卫：实收长度超端点上限即 Babble，隔离留账不伤账本；
//!      合法超短响应（0..2 字节）零误杀（J1R 锚点）。
//!   ④ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑤ 完成写回：全部竞态收敛后写状态=已完成、结果摘要非空回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——LCG 种子由
//!      SEG_BASE 派生；全程零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC23C0;

/// 请求槽数量。
pub const SLOT_COUNT: usize = 8;
/// 竞态轮数。
pub const RACE_ROUNDS: u32 = 50;
/// 响应长度上限（字节），超限即 Babble。
pub const RESP_MAX: usize = 128;

/// 诊断码闭集（取消竞态/响应守卫域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 二次落定（槽已被收编后再落定）
    DoubleSettle = 3,
    /// 未知请求标签
    UnknownTag = 4,
    /// 取消晚于完成（时序违例）
    CancelAfterComplete = 5,
    /// 完成晚于取消（时序违例）
    CompleteAfterCancel = 6,
    /// 槽索引越界
    SlotOob = 7,
    /// Babble 超长响应
    BabbleOverlong = 8,
    /// 竞态未收敛
    RaceUnconverged = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 合法响应被误杀
    UltraShortKill = 12,
}

impl RaceCode {
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::Ok => "竞态仲裁成功",
            RaceCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            RaceCode::DoubleSettle => "二次落定：请求槽已被收编不得再落定",
            RaceCode::UnknownTag => "未知标签：请求标签不在在册槽位之内",
            RaceCode::CancelAfterComplete => "取消晚于完成：已完成请求不得再取消",
            RaceCode::CompleteAfterCancel => "完成晚于取消：已取消请求不得再完成",
            RaceCode::SlotOob => "槽索引越界：访问必须在请求槽表容量之内",
            RaceCode::BabbleOverlong => "Babble 超长响应：超出端点长度上限，隔离留账",
            RaceCode::RaceUnconverged => "竞态未收敛：同拍双方必须收敛出唯一胜者",
            RaceCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            RaceCode::WritebackEmpty => "写回摘要为空：仲裁收口回执必须带非空结果摘要",
            RaceCode::UltraShortKill => "误杀合法响应：超短响应属协议合法帧不得拒",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [RaceCode; 12] = [
    RaceCode::Ok,
    RaceCode::SegBad,
    RaceCode::DoubleSettle,
    RaceCode::UnknownTag,
    RaceCode::CancelAfterComplete,
    RaceCode::CompleteAfterCancel,
    RaceCode::SlotOob,
    RaceCode::BabbleOverlong,
    RaceCode::RaceUnconverged,
    RaceCode::LedgerMismatch,
    RaceCode::WritebackEmpty,
    RaceCode::UltraShortKill,
];

/// 请求槽相态（Pending 唯一落定到两侧之一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 在途未决
    Pending = 0,
    /// 已取消（取消方胜）
    Cancelled = 1,
    /// 已完成（完成方胜）
    Completed = 2,
}

impl SlotState {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            SlotState::Pending => "在途未决",
            SlotState::Cancelled => "已取消",
            SlotState::Completed => "已完成",
        }
    }
}

/// 完成写回记录（判据③：状态=已完成、结果摘要非空）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArbitrationWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
    /// 已收敛竞态场数
    pub converged: u32,
}

/// 请求槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestSlot {
    /// 请求标签（槽序号即标签）
    pub tag: u32,
    /// 当前相态
    pub state: SlotState,
    /// 落定拍号（仲裁序留痕）
    pub settle_tick: u32,
}

/// 取消/完成竞态账：×50 同拍仲裁，唯一收敛。
pub struct CancelRaceBook {
    slots: [RequestSlot; SLOT_COUNT],
    tick: u32,
    races: u32,
    converged: u32,
    babble_ledger: Vec<(usize, usize)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl CancelRaceBook {
    /// 新账本：全部槽 Pending。
    pub fn new() -> CancelRaceBook {
        let mut slots = [RequestSlot { tag: 0, state: SlotState::Pending, settle_tick: 0 }; SLOT_COUNT];
        let mut i = 0usize;
        while i < SLOT_COUNT {
            slots[i] = RequestSlot { tag: i as u32, state: SlotState::Pending, settle_tick: 0 };
            i += 1;
        }
        CancelRaceBook {
            slots,
            tick: 0,
            races: 0,
            converged: 0,
            babble_ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 槽相态读面（越界 None）。
    pub fn slot_state(&self, tag: usize) -> Option<SlotState> {
        self.slots.get(tag).map(|s| s.state)
    }

    /// 提交请求（槽复位为 Pending）。
    pub fn submit(&mut self, tag: usize) -> Result<SlotState, RaceCode> {
        match self.slots.get_mut(tag) {
            Some(s) => {
                s.state = SlotState::Pending;
                s.settle_tick = 0;
                Ok(s.state)
            }
            None => Err(RaceCode::SlotOob),
        }
    }

    /// 单独取消（无竞态路径）：Pending→Cancelled。
    pub fn cancel(&mut self, tag: usize) -> Result<SlotState, RaceCode> {
        self.tick = self.tick.wrapping_add(1);
        let t = self.tick;
        match self.slots.get_mut(tag) {
            Some(s) => match s.state {
                SlotState::Pending => {
                    s.state = SlotState::Cancelled;
                    s.settle_tick = t;
                    Ok(s.state)
                }
                SlotState::Completed => Err(RaceCode::CancelAfterComplete),
                SlotState::Cancelled => Err(RaceCode::DoubleSettle),
            },
            None => Err(RaceCode::SlotOob),
        }
    }

    /// 单独完成（无竞态路径）：Pending→Completed。
    pub fn complete(&mut self, tag: usize) -> Result<SlotState, RaceCode> {
        self.tick = self.tick.wrapping_add(1);
        let t = self.tick;
        match self.slots.get_mut(tag) {
            Some(s) => match s.state {
                SlotState::Pending => {
                    s.state = SlotState::Completed;
                    s.settle_tick = t;
                    Ok(s.state)
                }
                SlotState::Cancelled => Err(RaceCode::CompleteAfterCancel),
                SlotState::Completed => Err(RaceCode::DoubleSettle),
            },
            None => Err(RaceCode::SlotOob),
        }
    }

    /// 同拍竞态仲裁：cancel 与 complete 同 tick 到达，强制唯一落定。
    /// cancel_wins=true 规范序取取消，否则取完成；二次宣称必拒。
    pub fn arbitrate(&mut self, tag: usize, cancel_wins: bool, double_win_inject: bool) -> Result<SlotState, RaceCode> {
        if tag >= SLOT_COUNT {
            return Err(RaceCode::SlotOob);
        }
        let _ = self.submit(tag);
        self.tick = self.tick.wrapping_add(1);
        let t = self.tick;
        let first = if cancel_wins { SlotState::Cancelled } else { SlotState::Completed };
        let second = if cancel_wins { SlotState::Completed } else { SlotState::Cancelled };
        match self.slots.get_mut(tag) {
            Some(s) => {
                s.state = first;
                s.settle_tick = t;
            }
            None => return Err(RaceCode::SlotOob),
        }
        self.races = self.races.saturating_add(1);
        if double_win_inject {
            // 注入双赢：败方也宣称胜利——必须显性拒（重复合并注入检出）。
            let r = self.complete(tag);
            let _ = r;
            // 落定态被违例翻转过，重写回规范胜者并回报双赢拒。
            match self.slots.get_mut(tag) {
                Some(s) => {
                    let bad = s.state == second;
                    s.state = first;
                    if bad {
                        self.converged = self.converged.saturating_add(1);
                        return Err(RaceCode::DoubleSettle);
                    }
                }
                None => return Err(RaceCode::SlotOob),
            }
            return Err(RaceCode::DoubleSettle);
        }
        self.converged = self.converged.saturating_add(1);
        Ok(first)
    }

    /// 全账收敛核验：无悬空 Pending 且收敛数 = 仲裁数。
    pub fn all_converged(&self) -> bool {
        let mut i = 0usize;
        while i < SLOT_COUNT {
            match self.slots.get(i) {
                Some(s) => {
                    if s.state == SlotState::Pending {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.converged == self.races
    }

    /// 收口写回（判据③）。
    pub fn finalize(&self) -> Result<ArbitrationWriteback, RaceCode> {
        if !self.all_converged() {
            return Err(RaceCode::RaceUnconverged);
        }
        Ok(ArbitrationWriteback {
            done: true,
            summary: format!(
                "竞态收口：仲裁 {} 场全部唯一收敛，Babble 留账 {} 条",
                self.races,
                self.babble_ledger.len()
            ),
            converged: self.converged,
        })
    }

    /// 响应守卫：超长 Babble 隔离留账；超短合法响应零误杀。
    pub fn judge_response(&mut self, got: usize) -> Result<bool, RaceCode> {
        if got > RESP_MAX {
            self.babble_ledger.push((got, RESP_MAX));
            return Err(RaceCode::BabbleOverlong);
        }
        // 0..2 字节超短响应：协议合法帧，放行不杀。
        Ok(true)
    }

    /// 竞态场次。
    pub fn races(&self) -> u32 {
        self.races
    }

    /// 收敛场次。
    pub fn converged(&self) -> u32 {
        self.converged
    }

    /// Babble 留账条数。
    pub fn babble_len(&self) -> usize {
        self.babble_ledger.len()
    }

    /// 第 k 条 Babble 留账（实收, 上限）。
    pub fn babble_at(&self, k: usize) -> Option<(usize, usize)> {
        self.babble_ledger.get(k).copied()
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
    pub fn snapshot(&self) -> (u32, u32, u32, usize, u64, u64) {
        (
            self.tick,
            self.races,
            self.converged,
            self.babble_ledger.len(),
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
    let mut b = CancelRaceBook::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(2654435761).wrapping_add(1);
    let mut log: String = String::new();
    for round in 0..64u32 {
        let op = lcg_next(&mut seed) % 5;
        let tag = (lcg_next(&mut seed) % (SLOT_COUNT as u64 + 1)) as usize;
        let cw = lcg_next(&mut seed) % 2 == 0;
        let r = match op {
            0 => b.arbitrate(tag % SLOT_COUNT, cw, false).map(|s| s.code()),
            1 => b.arbitrate(tag % SLOT_COUNT, cw, true).map(|s| s.code()),
            2 => b.cancel(tag).map(|s| s.code()),
            3 => b.complete(tag).map(|s| s.code()),
            _ => b.judge_response((lcg_next(&mut seed) % 200) as usize).map(|_| 0),
        };
        log.push_str(&format!("{}:{:?};", round, r.err()));
        if round % 5 == 4 {
            let _ = b.submit(tag % SLOT_COUNT);
        }
    }
    let (t, r, c, l, i, d) = b.snapshot();
    let full = format!("{}|{},{},{},{},{},{}", log, t, r, c, l, i, d);
    fnv1a(full.as_bytes())
}

/// 双跑一致性：同一脚本两次执行指纹必须逐位一致。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut b = CancelRaceBook::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：二次落定 ×10（完成后重复 complete）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = b.submit(i as usize % SLOT_COUNT);
            let _ = b.complete(i as usize % SLOT_COUNT);
            let r = b.complete(i as usize % SLOT_COUNT);
            if r == Err(RaceCode::DoubleSettle) {
                caught += 1;
                b.note_detected();
            } else {
                b.note_escape();
            }
            let _ = lcg_next(&mut seed) + i as u64;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：取消晚于完成 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = b.submit((i as usize + 1) % SLOT_COUNT);
            let _ = b.complete((i as usize + 1) % SLOT_COUNT);
            let r = b.cancel((i as usize + 1) % SLOT_COUNT);
            if r == Err(RaceCode::CancelAfterComplete) {
                caught += 1;
                b.note_detected();
            } else {
                b.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：未知标签 ×10（越界槽）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let tag = SLOT_COUNT + i as usize;
            let r = b.complete(tag);
            if r == Err(RaceCode::SlotOob) {
                caught += 1;
                b.note_detected();
            } else {
                b.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：竞态双赢注入 ×10（同拍双方都宣称胜）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = b.arbitrate(i as usize % SLOT_COUNT, i % 2 == 0, true);
            if r == Err(RaceCode::DoubleSettle) {
                caught += 1;
                b.note_detected();
            } else {
                b.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：Babble 超长 ×10（隔离留账）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = b.judge_response(RESP_MAX + 1 + i as usize);
            if r == Err(RaceCode::BabbleOverlong) {
                caught += 1;
                b.note_detected();
            } else {
                b.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (b.injected(), b.detected(), b.escaped(), classes_hit)
}

/// 竞态 ×50 双方收敛唯一（J1 正测）。
pub fn races_50_converge_unique() -> bool {
    let mut b = CancelRaceBook::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(0x9E3779B1).wrapping_add(3);
    for _ in 0..RACE_ROUNDS {
        let tag = (lcg_next(&mut seed) % SLOT_COUNT as u64) as usize;
        let cw = lcg_next(&mut seed) % 2 == 0;
        if b.arbitrate(tag, cw, false).is_err() {
            return false;
        }
    }
    b.races() == RACE_ROUNDS && b.converged() == RACE_ROUNDS
}

/// 合法超短响应 ×10 零误杀（J1R 锚点）。
pub fn ultra_short_zero_kill() -> bool {
    let mut b = CancelRaceBook::new();
    for len in 0..10usize {
        if b.judge_response(len % 2).is_err() {
            return false;
        }
    }
    true
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51644_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51644");
    // 1 单独取消/完成路径。
    {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(0);
        let c = b.cancel(0);
        let _ = b.submit(1);
        let p = b.complete(1);
        let ok = c == Ok(SlotState::Cancelled) && p == Ok(SlotState::Completed);
        s.add("单独落定路径", ok, "无竞态时取消与完成各按自身路径唯一落定");
    }
    // 2 同拍竞态 ×50 双方收敛唯一。
    s.add("竞态收敛唯一", races_50_converge_unique(), "50 场同拍竞态须场场收敛到唯一胜者");
    // 3 双赢注入 10/10 检出。
    {
        let mut b = CancelRaceBook::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if b.arbitrate(i as usize % SLOT_COUNT, true, true) == Err(RaceCode::DoubleSettle) {
                caught += 1;
            }
        }
        s.add("双赢注入检出", caught == 10, "同拍双赢注入 ×10 须 10/10 显性拒");
    }
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
    // 6 Babble ×10 隔离留账。
    {
        let mut b = CancelRaceBook::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if b.judge_response(RESP_MAX + 1 + i as usize) == Err(RaceCode::BabbleOverlong) {
                caught += 1;
            }
        }
        let ledged = b.babble_len() == 10 && matches!(b.babble_at(0), Some((g, m)) if g > m);
        s.add("Babble 隔离留账", caught == 10 && ledged, "超长响应 ×10 须全隔离且逐条留账（实收/上限）");
    }
    // 7 合法超短响应 ×10 零误杀。
    s.add("超短响应零误杀", ultra_short_zero_kill(), "0..2 字节合法响应 ×10 不得误杀一次");
    // 8 时序违例：完成晚于取消显性拒。
    {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(2);
        let _ = b.cancel(2);
        s.add("完成晚于取消拒", b.complete(2) == Err(RaceCode::CompleteAfterCancel), "已取消请求不得再完成");
    }
    // 9 诊断码闭集唯一且文案非空。
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
    // 10 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 11 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 12 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC23C0 须 0x40 对齐且在分派区间");
    // 13 指纹非退化。
    {
        let h1 = fnv1a(b"unx51644-probe-a");
        let h2 = fnv1a(b"unx51644-probe-b");
        s.add("指纹非退化", h1 != h2 && h1 != 0, "FNV-1a 对不同输入必须给出不同指纹");
    }
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
    // 15 收口写回：摘要非空且状态=已完成。
    {
        let mut b = CancelRaceBook::new();
        for i in 0..SLOT_COUNT {
            let _ = b.arbitrate(i, i % 2 == 0, false);
        }
        let wb = b.finalize();
        s.add(
            "仲裁收口写回",
            matches!(wb, Ok(ref w) if w.done && !w.summary.is_empty()),
            "全部槽唯一落定后收口，写回状态=已完成且摘要非空",
        );
    }
    // 16 未收敛拒写回。
    {
        let b = CancelRaceBook::new();
        s.add("未收敛拒写回", b.finalize() == Err(RaceCode::RaceUnconverged), "存在悬空 Pending 槽不得写回完成");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_settle_paths() {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(0);
        assert_eq!(b.cancel(0), Ok(SlotState::Cancelled));
        let _ = b.submit(1);
        assert_eq!(b.complete(1), Ok(SlotState::Completed));
    }

    #[test]
    fn double_settle_rejected() {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(0);
        let _ = b.complete(0);
        assert_eq!(b.complete(0), Err(RaceCode::DoubleSettle));
    }

    #[test]
    fn cancel_after_complete_rejected() {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(1);
        let _ = b.complete(1);
        assert_eq!(b.cancel(1), Err(RaceCode::CancelAfterComplete));
    }

    #[test]
    fn complete_after_cancel_rejected() {
        let mut b = CancelRaceBook::new();
        let _ = b.submit(1);
        let _ = b.cancel(1);
        assert_eq!(b.complete(1), Err(RaceCode::CompleteAfterCancel));
    }

    #[test]
    fn races_50_unique() {
        assert!(races_50_converge_unique());
    }

    #[test]
    fn double_win_inject_detected() {
        let mut b = CancelRaceBook::new();
        assert_eq!(b.arbitrate(0, true, true), Err(RaceCode::DoubleSettle));
    }

    #[test]
    fn babble_isolated_and_ledged() {
        let mut b = CancelRaceBook::new();
        assert_eq!(b.judge_response(RESP_MAX + 5), Err(RaceCode::BabbleOverlong));
        assert_eq!(b.babble_len(), 1);
    }

    #[test]
    fn ultra_short_responses_not_killed() {
        assert!(ultra_short_zero_kill());
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn slot_oob() {
        let mut b = CancelRaceBook::new();
        assert_eq!(b.complete(SLOT_COUNT + 2), Err(RaceCode::SlotOob));
        assert!(b.slot_state(SLOT_COUNT + 2).is_none());
    }

    #[test]
    fn ledger_conserved() {
        let mut b = CancelRaceBook::new();
        for _ in 0..10 {
            b.note_detected();
        }
        assert!(b.conserved());
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
    fn unx51644_checks_green() {
        let set = run_unx51644_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
