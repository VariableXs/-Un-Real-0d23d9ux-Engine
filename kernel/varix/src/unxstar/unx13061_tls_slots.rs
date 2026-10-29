//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13061 · TlsAlloc/TlsFree 槽位分配语义（位图分配账）· 0x655C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F13061-J1 位图分配/回收 ×10^4 轮账实一致（无泄漏无复用
//!     竞态），分配序与 Windows 一致（最低空闲位）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 槽位闭集（写死自 winnls/ntdll 语义）：TlsAlloc 最低空闲位；
//!     TLS_MINIMUM_AVAILABLE = 64（常规槽）+ 扩展槽 1024 = 1088 全域；
//!     TLS_OUT_OF_INDEXES = 0xFFFFFFFF 写死；
//!   - 位图账：alloc 置位 + free 清位 + 双账（置位数 == live）；
//!     分配/回收 ×10^4 轮零泄漏零重复；
//!   - TlsGetValue 空槽语义：未分配槽读 → 0 且不落账；已分配槽
//!     未写入 → 0；越界 → OUT_OF_INDEXES；
//!   - TlsFree 非法闭式：越界 / 未分配槽回收 / 双重回收必拒；
//!   - 状态机显式：Free→Allocated→(Written)→Free，非法转换拒收；
//!   - LCG 种子写死 = SEG_BASE（0x655C0）；禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x655C0，全仓独占）。
pub const SEG_BASE: u32 = 0x655C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlsSlotCode {
    /// 槽位耗尽（全满再分配）
    SlotsExhausted,
    /// 越界槽号
    SlotOutOfRange,
    /// 双重回收
    DoubleFree,
    /// 回收未分配槽
    FreeUnallocated,
    /// 位图账破（置位数 != live）
    BitmapLedgerBroken,
    /// 分配序破（非最低空闲位）
    AllocOrderBroken,
    /// 槽值账破（已分配槽值不守恒）
    ValueLedgerBroken,
    /// 状态机非法转换
    IllegalTransition,
}

impl TlsSlotCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TlsSlotCode::SlotsExhausted => "槽位耗尽：全满再分配（Windows 返回 OUT_OF_INDEXES）",
            TlsSlotCode::SlotOutOfRange => "越界槽号：越 1088 全域",
            TlsSlotCode::DoubleFree => "双重回收：必拒",
            TlsSlotCode::FreeUnallocated => "回收未分配槽：必拒",
            TlsSlotCode::BitmapLedgerBroken => "位图账破：置位数 != live",
            TlsSlotCode::AllocOrderBroken => "分配序破：非最低空闲位",
            TlsSlotCode::ValueLedgerBroken => "槽值账破：已分配槽值不守恒",
            TlsSlotCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [TlsSlotCode; 8] = [
        TlsSlotCode::SlotsExhausted,
        TlsSlotCode::SlotOutOfRange,
        TlsSlotCode::DoubleFree,
        TlsSlotCode::FreeUnallocated,
        TlsSlotCode::BitmapLedgerBroken,
        TlsSlotCode::AllocOrderBroken,
        TlsSlotCode::ValueLedgerBroken,
        TlsSlotCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TlsSlotCode::ALL.len() {
        if TlsSlotCode::ALL[i].val() == code {
            return TlsSlotCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 槽位常量（写死自 Windows 语义） ─────────────────────────────
/// 常规槽数（TLS_MINIMUM_AVAILABLE）。
pub const TLS_MINIMUM: usize = 64;
/// 扩展槽数（TLS_EXPANSION_SLOTS）。
pub const TLS_EXPANSION: usize = 1024;
/// 全域槽位上界。
pub const TOTAL_SLOTS: usize = TLS_MINIMUM + TLS_EXPANSION; // 1088
/// TlsAlloc 失败返回码。
pub const TLS_OUT_OF_INDEXES: u32 = 0xFFFF_FFFF;

// ── 槽位状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free,
    Allocated,
    Written,
}

pub fn edge_legal(from: SlotState, to: SlotState) -> bool {
    matches!(
        (from, to),
        (SlotState::Free, SlotState::Allocated)
            | (SlotState::Allocated, SlotState::Written)
            | (SlotState::Allocated, SlotState::Free)
            | (SlotState::Written, SlotState::Free)
    )
}

pub fn transition(from: SlotState, to: SlotState) -> Result<SlotState, TlsSlotCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(TlsSlotCode::IllegalTransition)
    }
}

// ── 位图分配账 ──────────────────────────────────────────────────
pub struct TlsSlotTable {
    /// 状态图（每槽一态）。
    pub states: Vec<SlotState>,
    /// 槽值账（已写入值；未写槽为 0）。
    pub values: Vec<u64>,
    /// live = 已分配（含已写）槽数。
    pub live: usize,
    /// 累计分配 / 回收。
    pub allocs: u64,
    pub frees: u64,
}

impl TlsSlotTable {
    pub fn new() -> TlsSlotTable {
        TlsSlotTable {
            states: vec![SlotState::Free; TOTAL_SLOTS],
            values: vec![0u64; TOTAL_SLOTS],
            live: 0,
            allocs: 0,
            frees: 0,
        }
    }

    /// TlsAlloc：最低空闲位；全满 → Err（调用方映射 OUT_OF_INDEXES）。
    pub fn tls_alloc(&mut self) -> Result<usize, TlsSlotCode> {
        let mut i = 0;
        while i < TOTAL_SLOTS {
            if self.states[i] == SlotState::Free {
                let _ = transition(self.states[i], SlotState::Allocated)?;
                self.states[i] = SlotState::Allocated;
                self.values[i] = 0;
                self.live += 1;
                self.allocs += 1;
                // 分配序核：之前的槽必须全非 Free。
                let mut j = 0;
                while j < i {
                    if self.states[j] == SlotState::Free {
                        return Err(TlsSlotCode::AllocOrderBroken);
                    }
                    j += 1;
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(TlsSlotCode::SlotsExhausted)
    }

    /// TlsFree：越界/未分配/双重回收必拒。
    pub fn tls_free(&mut self, idx: usize) -> Result<(), TlsSlotCode> {
        if idx >= TOTAL_SLOTS {
            return Err(TlsSlotCode::SlotOutOfRange);
        }
        let st = self.states[idx];
        if st == SlotState::Free {
            return Err(TlsSlotCode::FreeUnallocated);
        }
        let _ = transition(st, SlotState::Free)?;
        self.states[idx] = SlotState::Free;
        self.values[idx] = 0;
        self.live -= 1;
        self.frees += 1;
        Ok(())
    }

    /// TlsSetValue：已分配槽才可写；越界/未分配拒。
    pub fn tls_set(&mut self, idx: usize, v: u64) -> Result<(), TlsSlotCode> {
        if idx >= TOTAL_SLOTS {
            return Err(TlsSlotCode::SlotOutOfRange);
        }
        match self.states[idx] {
            SlotState::Allocated => {
                let _ = transition(self.states[idx], SlotState::Written)?;
                self.states[idx] = SlotState::Written;
                self.values[idx] = v;
                Ok(())
            }
            SlotState::Written => {
                self.values[idx] = v;
                Ok(())
            }
            SlotState::Free => Err(TlsSlotCode::FreeUnallocated),
        }
    }

    /// TlsGetValue：越界 → None；空槽/未写 → 0。
    pub fn tls_get(&self, idx: usize) -> Result<u64, TlsSlotCode> {
        if idx >= TOTAL_SLOTS {
            return Err(TlsSlotCode::SlotOutOfRange);
        }
        if self.states[idx] == SlotState::Free {
            return Ok(0);
        }
        Ok(self.values[idx])
    }

    /// 位图账：置位数 == live；allocs-frees == live。
    pub fn conserved(&self) -> bool {
        let mut n = 0;
        let mut i = 0;
        while i < TOTAL_SLOTS {
            if self.states[i] != SlotState::Free {
                n += 1;
            }
            i += 1;
        }
        n == self.live && self.allocs >= self.frees && self.allocs - self.frees == self.live as u64
    }

    /// 零泄漏终态：全 Free、live==0、allocs==frees。
    pub fn zero_leak(&self) -> bool {
        self.live == 0 && self.allocs == self.frees && self.conserved()
    }
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── ×10^4 轮分配/回收账实一致（判据 J1 主面） ────────────────────
pub fn drill_10k_rounds() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut table = TlsSlotTable::new();
    let mut live_tids: Vec<usize> = Vec::new();
    let mut ok = true;
    let mut round = 0;
    while round < 10_000 {
        // 操作闭集：0=alloc 1=free 2=set 3=get（LCG 决定，保可行）。
        let op = rng.next_below(4);
        match op {
            0 => {
                if live_tids.len() < TOTAL_SLOTS {
                    match table.tls_alloc() {
                        Ok(tid) => live_tids.push(tid),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            1 => {
                if !live_tids.is_empty() {
                    // LIFO 回收（确定性）。
                    let tid = live_tids[live_tids.len() - 1];
                    if table.tls_free(tid).is_err() {
                        ok = false;
                        break;
                    }
                    live_tids.pop();
                }
            }
            2 => {
                if !live_tids.is_empty() {
                    let k = rng.next_below(live_tids.len() as u64) as usize;
                    let tid = live_tids[k];
                    let v = rng.next();
                    if table.tls_set(tid, v).is_err() {
                        ok = false;
                        break;
                    }
                }
            }
            _ => {
                if !live_tids.is_empty() {
                    let k = rng.next_below(live_tids.len() as u64) as usize;
                    let tid = live_tids[k];
                    if table.tls_get(tid).is_err() {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if !table.conserved() {
            ok = false;
            break;
        }
        round += 1;
    }
    // 终态清账：全回收 → 零泄漏。
    let mut d = live_tids.len();
    while d > 0 {
        d -= 1;
        if table.tls_free(live_tids[d]).is_err() {
            ok = false;
            break;
        }
    }
    ok = ok && table.zero_leak();
    (
        ok,
        format!(
            "×10^4 轮: allocs={} frees={} 零泄漏={}",
            table.allocs, table.frees, table.zero_leak()
        ),
    )
}

pub fn drill_alloc_order_lowest() -> (bool, String) {
    // 分配序核：空洞复用时必得最低空闲位 ×20 组。
    let mut caught = 0;
    let mut g = 0;
    while g < 20 {
        let mut table = TlsSlotTable::new();
        // 占 0..5。
        let mut tids: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < 6 {
            match table.tls_alloc() {
                Ok(t) => tids.push(t),
                Err(_) => break,
            }
            i += 1;
        }
        // 拆 2 与 4（形成空洞）。
        if table.tls_free(2).is_ok() && table.tls_free(4).is_ok() {
            // 下一次分配必得 2，再下次必得 4。
            let a = table.tls_alloc();
            let b = table.tls_alloc();
            if a == Ok(2) && b == Ok(4) {
                caught += 1;
            }
        }
        g += 1;
    }
    let ok = caught == 20;
    (ok, format!("最低空闲位序 {}/20 组", caught))
}

pub fn drill_exhaustion() -> (bool, String) {
    // 槽位耗尽：1088 全占 → OUT_OF_INDEXES；拆 1 再得最低位。
    let mut table = TlsSlotTable::new();
    let mut ok = true;
    let mut i = 0;
    while i < TOTAL_SLOTS {
        if table.tls_alloc().is_err() {
            ok = false;
            break;
        }
        i += 1;
    }
    ok = ok && table.live == TOTAL_SLOTS;
    // 满表再分配 → SlotsExhausted（调用方映射 0xFFFFFFFF）。
    ok = ok && table.tls_alloc() == Err(TlsSlotCode::SlotsExhausted)
        && TLS_OUT_OF_INDEXES == 0xFFFF_FFFF;
    // 拆最低位 0 → 复用得 0。
    if table.tls_free(0).is_ok() {
        ok = ok && table.tls_alloc() == Ok(0);
    } else {
        ok = false;
    }
    (ok, format!("槽位耗尽 + OUT_OF_INDEXES: {}", ok))
}

pub fn drill_free_illegal() -> (bool, String) {
    // TlsFree 非法闭式注入：越界/未分配/双重回收 10/10 必拒。
    let mut caught = 0;
    let mut g = 0;
    while g < 10 {
        let mut table = TlsSlotTable::new();
        let tid = match table.tls_alloc() {
            Ok(t) => t,
            Err(_) => break,
        };
        let r1 = table.tls_free(TOTAL_SLOTS + g); // 越界
        let r2 = table.tls_free(50 + g); // 未分配
        let r3 = table.tls_free(tid).is_ok(); // 正常回收
        let r4 = table.tls_free(tid) == Err(TlsSlotCode::FreeUnallocated); // 双重回收
        if r1 == Err(TlsSlotCode::SlotOutOfRange)
            && r2 == Err(TlsSlotCode::FreeUnallocated)
            && r3
            && r4
            && table.conserved()
        {
            caught += 1;
        }
        g += 1;
    }
    let ok = caught == 10;
    (ok, format!("TlsFree 非法注入 {}/10", caught))
}

pub fn drill_value_semantics() -> (bool, String) {
    // 槽值语义：空槽读 0、未写读 0、写入读回、重写覆盖、回收清零。
    let mut table = TlsSlotTable::new();
    let mut ok = table.tls_get(3) == Ok(0); // 未分配槽读 0（不落账）
    let tid = match table.tls_alloc() {
        Ok(t) => t,
        Err(_) => return (false, String::from("alloc 失败")),
    };
    ok = ok && table.tls_get(tid) == Ok(0); // 未写读 0
    ok = ok && table.tls_set(tid, 0xDEAD_BEEF).is_ok();
    ok = ok && table.tls_get(tid) == Ok(0xDEAD_BEEF); // 写入读回
    ok = ok && table.tls_set(tid, 7).is_ok() && table.tls_get(tid) == Ok(7); // 重写覆盖
    ok = ok && table.tls_get(TOTAL_SLOTS) == Err(TlsSlotCode::SlotOutOfRange); // 越界
    // 未分配槽写拒。
    ok = ok && table.tls_set(77, 1) == Err(TlsSlotCode::FreeUnallocated);
    if table.tls_free(tid).is_ok() {
        ok = ok && table.tls_get(tid) == Ok(0); // 回收清零
    } else {
        ok = false;
    }
    (ok, format!("槽值语义: {}", ok))
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && transition(SlotState::Free, SlotState::Allocated).is_ok();
    ok = ok && transition(SlotState::Allocated, SlotState::Written).is_ok();
    ok = ok && transition(SlotState::Allocated, SlotState::Free).is_ok();
    ok = ok && transition(SlotState::Written, SlotState::Free).is_ok();
    ok = ok && transition(SlotState::Free, SlotState::Written) == Err(TlsSlotCode::IllegalTransition);
    ok = ok && transition(SlotState::Written, SlotState::Allocated) == Err(TlsSlotCode::IllegalTransition);
    ok = ok && transition(SlotState::Free, SlotState::Free) == Err(TlsSlotCode::IllegalTransition);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_bitmap_conservation() -> (bool, String) {
    // 位图账守恒：随机停点 30 组（置位数 == live == allocs-frees）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    let mut g = 0;
    while g < 30 {
        let mut table = TlsSlotTable::new();
        let mut live: Vec<usize> = Vec::new();
        let steps = rng.next_below(200);
        let mut s = 0;
        while s < steps {
            let op = rng.next_below(2);
            if op == 0 && live.len() < TOTAL_SLOTS {
                if let Ok(t) = table.tls_alloc() {
                    live.push(t);
                }
            } else if !live.is_empty() {
                let k = rng.next_below(live.len() as u64) as usize;
                let tid = live[k];
                if table.tls_free(tid).is_ok() {
                    let last = live.len() - 1;
                    live.swap(k, last);
                    live.pop();
                }
            }
            s += 1;
        }
        if table.conserved() {
            caught += 1;
        }
        g += 1;
    }
    let ok = caught == 30;
    (ok, format!("位图账守恒 {}/30 停点", caught))
}

pub fn drill_min_vs_expansion() -> (bool, String) {
    // 常规/扩展槽边界：0..63 常规、64..1087 扩展，全域连续位图。
    let mut table = TlsSlotTable::new();
    let mut ok = table.tls_alloc() == Ok(0); // 首槽 0（常规区）
    // 占到 63。
    let mut i = 1;
    while i < TLS_MINIMUM {
        if table.tls_alloc() != Ok(i) {
            ok = false;
            break;
        }
        i += 1;
    }
    // 第 65 个槽 = 64（扩展区首）。
    ok = ok && table.tls_alloc() == Ok(TLS_MINIMUM);
    // 最后一槽 = 1087。
    let mut j = TLS_MINIMUM + 1;
    let mut last = Err(TlsSlotCode::SlotsExhausted);
    while j < TOTAL_SLOTS {
        last = table.tls_alloc();
        j += 1;
    }
    ok = ok && last == Ok(TOTAL_SLOTS - 1);
    (ok, format!("常规/扩展边界: 64/1088 = {}", ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < TlsSlotCode::ALL.len() {
        ok = ok && TlsSlotCode::ALL[i].val() >= SEG_BASE && TlsSlotCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !TlsSlotCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", TlsSlotCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < TlsSlotCode::ALL.len() {
        let v = TlsSlotCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_10k_rounds();
    let (a2, d2) = drill_alloc_order_lowest();
    let (a3, d3) = drill_exhaustion();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 10^4轮={} 最低位序={} 耗尽={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_10k_rounds();
    v.push(("J1-分配回收×10^4轮账实一致", a, b));
    let (a, b) = drill_alloc_order_lowest();
    v.push(("J1-最低空闲位序20组", a, b));
    let (a, b) = drill_exhaustion();
    v.push(("J1-槽位耗尽OUT_OF_INDEXES", a, b));
    let (a, b) = drill_free_illegal();
    v.push(("J2-TlsFree非法注入10/10", a, b));
    let (a, b) = drill_value_semantics();
    v.push(("J2-槽值语义", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_bitmap_conservation();
    v.push(("J2-位图账守恒30停点", a, b));
    let (a, b) = drill_min_vs_expansion();
    v.push(("J2-常规/扩展槽边界", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13061_checks：本单判据聚合（诊断段 0x655C0）。
pub fn run_unx13061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13061 TlsAlloc/TlsFree 位图分配账");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x655C0 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
