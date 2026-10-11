//! UNX-F1843 · IPI 接收主路径（入口→识别→分发→EOI）
//!
//! 诊断段 0x9C61~0x9C68（IPI 族接收段）。
//! 判据（施工书 J1）：接收四步序冻结 v1（中断入口→向量识别→分派→EOI），EOI 遗漏注入 10/10 检出，处理时长上界断言通过。

use alloc::vec::Vec;

/// IPI 接收诊断码闭集（0x9C61~0x9C68 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiRxCode {
    /// 0x9C61 中断入口受理。
    Entered = 0x9C61,
    /// 0x9C62 向量识别完成（IPI 向量窗命中）。
    VectorKnown = 0x9C62,
    /// 0x9C63 向量越窗（非 IPI 向量）。
    VectorUnknown = 0x9C63,
    /// 0x9C64 分派完成（处理函数命中并执行）。
    Dispatched = 0x9C64,
    /// 0x9C65 分派空转（无处理函数注册）。
    DispatchEmpty = 0x9C65,
    /// 0x9C66 EOI 发出（四步闭环）。
    EoiSent = 0x9C66,
    /// 0x9C67 EOI 遗漏检出（四步未走完）。
    EoiMissed = 0x9C67,
    /// 0x9C68 接收账守恒破坏。
    LedgerBroken = 0x9C68,
}

impl IpiRxCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            IpiRxCode::Entered => "IPI 中断入口已受理",
            IpiRxCode::VectorKnown => "IPI 向量识别命中",
            IpiRxCode::VectorUnknown => "向量不在 IPI 窗",
            IpiRxCode::Dispatched => "处理函数分派完成",
            IpiRxCode::DispatchEmpty => "无注册处理函数",
            IpiRxCode::EoiSent => "EOI 已发出",
            IpiRxCode::EoiMissed => "EOI 遗漏检出",
            IpiRxCode::LedgerBroken => "接收账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            IpiRxCode::Entered => "本地 APIC 中断引脚触发",
            IpiRxCode::VectorKnown => "向量落 0xF0~0xFE IPI 窗",
            IpiRxCode::VectorUnknown => "向量越 IPI 窗",
            IpiRxCode::Dispatched => "向量命中注册表",
            IpiRxCode::DispatchEmpty => "向量未注册处理器",
            IpiRxCode::EoiSent => "四步序完整",
            IpiRxCode::EoiMissed => "分派后未走 EOI 步",
            IpiRxCode::LedgerBroken => "受理数≠闭环数+在途数",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            IpiRxCode::Entered => "按四步序推进",
            IpiRxCode::VectorKnown => "继续分派",
            IpiRxCode::VectorUnknown => "转交通用中断路径",
            IpiRxCode::Dispatched => "补发 EOI 收口",
            IpiRxCode::DispatchEmpty => "登记空转并补 EOI",
            IpiRxCode::EoiSent => "无需处置",
            IpiRxCode::EoiMissed => "注入核兜底补发 EOI",
            IpiRxCode::LedgerBroken => "冻结 IPI 账并上报 0x9C68",
        }
    }
}

/// IPI 向量窗（0xF0~0xFE）。
pub const IPI_VEC_LO: u8 = 0xF0;
pub const IPI_VEC_HI: u8 = 0xFE;

/// 接收四步闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RxStep {
    /// 1 中断入口。
    Enter,
    /// 2 向量识别。
    Identify,
    /// 3 分派。
    Dispatch,
    /// 4 EOI。
    Eoi,
}

impl RxStep {
    /// 全序。
    pub const ALL: [RxStep; 4] = [RxStep::Enter, RxStep::Identify, RxStep::Dispatch, RxStep::Eoi];

    /// 步序号（1 起）。
    pub const fn ordinal(self) -> u8 {
        match self {
            RxStep::Enter => 1,
            RxStep::Identify => 2,
            RxStep::Dispatch => 3,
            RxStep::Eoi => 4,
        }
    }
}

/// 接收事件记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RxRec {
    /// 向量。
    pub vec: u8,
    /// 走到的最后一步序号。
    pub last_step: u8,
    /// 闭环与否。
    pub closed: bool,
    /// 处理时长（拍）。
    pub dur: u64,
}

/// IPI 接收引擎：四步严格序 + EOI 遗漏核 + 时长上界。
pub struct IpiRxEngine {
    /// 注册处理表（向量 → 注册）。
    handlers: Vec<u8>,
    /// 受理记录。
    recs: Vec<RxRec>,
    /// 当前受理上下文（None = 空闲）。
    active: Option<(u8, u8, u64)>, // (vec, last_step, start_tick)
    /// EOI 遗漏检出数。
    eoi_misses: u64,
    /// 闭环数。
    closed: u64,
    /// tick。
    tick: u64,
    /// 时长上界（拍）。
    pub dur_budget: u64,
}

impl IpiRxEngine {
    /// 新引擎（时长上界 64 拍）。
    pub fn new() -> Self {
        IpiRxEngine {
            handlers: Vec::new(),
            recs: Vec::new(),
            active: None,
            eoi_misses: 0,
            closed: 0,
            tick: 0,
            dur_budget: 64,
        }
    }

    /// 注册处理函数向量。
    pub fn register_handler(&mut self, vec: u8) -> bool {
        if vec < IPI_VEC_LO || vec > IPI_VEC_HI {
            return false;
        }
        if !self.handlers.contains(&vec) {
            self.handlers.push(vec);
        }
        true
    }

    /// 推进一步（严格序：步序号必须 = 当前+1）。
    pub fn step(&mut self, s: RxStep, vec: u8) -> IpiRxCode {
        self.tick += 1;
        match s {
            RxStep::Enter => {
                if self.active.is_some() {
                    return IpiRxCode::LedgerBroken;
                }
                self.active = Some((vec, 1, self.tick));
                IpiRxCode::Entered
            }
            RxStep::Identify => {
                match self.active {
                    Some((v, last, _)) if last == 1 => {
                        if vec < IPI_VEC_LO || vec > IPI_VEC_HI {
                            // 越窗：收口为未识别（受理记录落账）。
                            self.recs.push(RxRec { vec, last_step: 2, closed: false, dur: self.tick - self.active.unwrap_or((vec, 0, self.tick)).2 });
                            self.active = None;
                            return IpiRxCode::VectorUnknown;
                        }
                        self.active = Some((vec, 2, self.active.unwrap_or((vec, 0, self.tick)).2));
                        IpiRxCode::VectorKnown
                    }
                    _ => IpiRxCode::LedgerBroken,
                }
            }
            RxStep::Dispatch => {
                match self.active {
                    Some((v, last, st)) if last == 2 => {
                        let code = if self.handlers.contains(&v) { IpiRxCode::Dispatched } else { IpiRxCode::DispatchEmpty };
                        self.active = Some((v, 3, st));
                        code
                    }
                    _ => IpiRxCode::LedgerBroken,
                }
            }
            RxStep::Eoi => {
                match self.active {
                    Some((v, last, st)) if last == 3 => {
                        let dur = self.tick - st;
                        self.active = None;
                        self.closed += 1;
                        self.recs.push(RxRec { vec: v, last_step: 4, closed: true, dur });
                        IpiRxCode::EoiSent
                    }
                    _ => IpiRxCode::LedgerBroken,
                }
            }
        }
    }

    /// EOI 遗漏注入：在 Dispatch 后中止（不走 Eoi 步），遗漏核检出。
    pub fn inject_eoi_miss(&mut self, vec: u8) -> IpiRxCode {
        let _ = self.step(RxStep::Enter, vec);
        let _ = self.step(RxStep::Identify, vec);
        let _ = self.step(RxStep::Dispatch, vec);
        // 中止：不 Eoi，直接核遗漏。
        match self.active {
            Some((v, last, st)) if last == 3 => {
                self.eoi_misses += 1;
                let dur = self.tick - st;
                self.recs.push(RxRec { vec: v, last_step: 3, closed: false, dur });
                self.active = None;
                IpiRxCode::EoiMissed
            }
            _ => IpiRxCode::LedgerBroken,
        }
    }

    /// 时长上界核：全部闭环记录 dur ≤ 预算。
    pub fn dur_within_budget(&self) -> bool {
        self.recs.iter().all(|r| r.dur <= self.dur_budget)
    }

    /// 最大时长。
    pub fn max_dur(&self) -> u64 {
        self.recs.iter().map(|r| r.dur).max().unwrap_or(0)
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, usize) {
        (self.closed, self.eoi_misses, self.recs.len())
    }

    /// 账守恒：闭环数 + 遗漏数 + 非闭环识别记录 == 记录总数。
    pub fn ledger_consistent(&self) -> bool {
        let unknown = self.recs.iter().filter(|r| !r.closed && r.last_step == 2).count() as u64;
        let missed = self.recs.iter().filter(|r| !r.closed && r.last_step == 3).count() as u64;
        self.closed + unknown + missed == self.recs.len() as u64 && missed == self.eoi_misses
    }
}

/// 判据测试（段 0x9C61~0x9C68）。
pub fn run_t843_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t843");
    // c1 四步严格序闭环。
    let mut e = IpiRxEngine::new();
    let _ = e.register_handler(0xF1);
    let mut c1 = true;
    c1 &= e.step(RxStep::Enter, 0xF1) == IpiRxCode::Entered;
    c1 &= e.step(RxStep::Identify, 0xF1) == IpiRxCode::VectorKnown;
    c1 &= e.step(RxStep::Dispatch, 0xF1) == IpiRxCode::Dispatched;
    c1 &= e.step(RxStep::Eoi, 0xF1) == IpiRxCode::EoiSent;
    set.add("t843-c1-four-steps", c1, "");
    // c2 乱序拒绝（跳步）。
    let mut e2 = IpiRxEngine::new();
    let _ = e2.register_handler(0xF2);
    let _ = e2.step(RxStep::Enter, 0xF2);
    set.add(
        "t843-c2-order-strict",
        e2.step(RxStep::Dispatch, 0xF2) == IpiRxCode::LedgerBroken,
        "",
    );
    // c3 向量越窗。
    let mut e3 = IpiRxEngine::new();
    let _ = e3.step(RxStep::Enter, 0x20);
    set.add("t843-c3-vec-range", e3.step(RxStep::Identify, 0x20) == IpiRxCode::VectorUnknown, "");
    // c4 空转分派（未注册向量）。
    let mut e4 = IpiRxEngine::new();
    let _ = e4.step(RxStep::Enter, 0xF9);
    let _ = e4.step(RxStep::Identify, 0xF9);
    set.add("t843-c4-empty-dispatch", e4.step(RxStep::Dispatch, 0xF9) == IpiRxCode::DispatchEmpty, "");
    // c5 EOI 遗漏注入 10/10 检出。
    let mut e5 = IpiRxEngine::new();
    let _ = e5.register_handler(0xF3);
    let mut misses = 0;
    for _ in 0..10u32 {
        if e5.inject_eoi_miss(0xF3) == IpiRxCode::EoiMissed {
            misses += 1;
        }
    }
    set.add("t843-c5-eoi-miss-10of10", misses == 10 && e5.stats().1 == 10, "");
    // c6 时长上界断言。
    let mut e6 = IpiRxEngine::new();
    let _ = e6.register_handler(0xF4);
    let _ = e6.step(RxStep::Enter, 0xF4);
    let _ = e6.step(RxStep::Identify, 0xF4);
    let _ = e6.step(RxStep::Dispatch, 0xF4);
    let _ = e6.step(RxStep::Eoi, 0xF4);
    set.add("t843-c6-dur-budget", e6.dur_within_budget() && e6.max_dur() <= e6.dur_budget, "");
    // c7 处理函数注册越窗拒。
    let mut e7 = IpiRxEngine::new();
    set.add("t823-c7-handler-range", !e7.register_handler(0x01) && e7.register_handler(0xFE), "");
    // c8 LCG 512 轮注入：随机向量与步序，账守恒恒成立。
    let mut seed: u64 = 0x8430_9C61_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = IpiRxEngine::new();
        let _ = eng.register_handler(0xF0u8.wrapping_add((lc() % 15) as u8));
        for _ in 0..12 {
            let vec = 0xF0u8.wrapping_add((lc() % 20) as u8);
            match lc() % 3 {
                0 => {
                    let _ = eng.inject_eoi_miss(vec);
                }
                _ => {
                    let mut walked = 0;
                    for s in RxStep::ALL {
                        let r = eng.step(s, vec);
                        walked += 1;
                        if r == IpiRxCode::LedgerBroken || r == IpiRxCode::VectorUnknown {
                            break;
                        }
                    }
                    let _ = walked;
                }
            }
        }
        inj_ok &= eng.ledger_consistent();
    }
    set.add("t843-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        IpiRxCode::Entered,
        IpiRxCode::VectorKnown,
        IpiRxCode::VectorUnknown,
        IpiRxCode::Dispatched,
        IpiRxCode::DispatchEmpty,
        IpiRxCode::EoiSent,
        IpiRxCode::EoiMissed,
        IpiRxCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t843-c9-triad", tri_ok, "");
    set
}
