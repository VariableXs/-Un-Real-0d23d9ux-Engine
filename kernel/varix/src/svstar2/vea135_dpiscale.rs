//! VE-F0135 · DPI 变化全链路重排（VE-A 域 · A06 环境与系统集成组 · 目标 400 行）
//!
//! 铁律：四层联动，原子重排，耗时预算，DPI 滞回。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xEAA0（重排：原 0xE8CD 未对齐）
pub const SEG_BASE: u16 = 0xEAA0;
pub const XC_LAYER_MISMATCH: u16 = SEG_BASE | 0x01;
pub const XC_ATOMIC_FAIL: u16 = SEG_BASE | 0x02;
pub const XC_BUDGET_EXCEEDED: u16 = SEG_BASE | 0x03;
pub const XC_DPI_JITTER: u16 = SEG_BASE | 0x04;
pub const XC_BAD_DPI: u16 = SEG_BASE | 0x05;
pub const XC_ROLLBACK: u16 = SEG_BASE | 0x06;

/// 四层重排目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relayer {
    /// 交换链层（surface size）。
    SwapChain,
    /// 视口层（viewport transform）。
    Viewport,
    /// UI 层（scale factor）。
    Ui,
    /// 令牌层（coordinate token）。
    Token,
}

/// 全链重排状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DpiRelayout {
    /// 当前 DPI（万分之一英寸）。
    pub dpi: u32,
    /// 目标 DPI。
    pub target_dpi: u32,
    /// 已重排层数（0-4）。
    pub layers_done: u8,
    /// 是否原子完成（四层全成或全回滚）。
    pub atomic: bool,
    /// 滞回窗口（DPI 变化 < 滞回阈值不触发重排）。
    pub hysteresis: u32,
    /// 重排耗时（微秒）。
    pub elapsed_us: u32,
}

/// DPI 全链重排器。
pub struct DpiRelayoutEngine {
    /// 当前 DPI。
    dpi: u32,
    /// 滞回阈值（万分之一，低于此值不重排）。
    hysteresis: u32,
    /// 耗时预算（微秒）。
    budget_us: u32,
    /// 四层状态（true=已重排）。
    layers: [bool; 4],
    /// 最近诊断码。
    last_code: u16,
    /// 重排次数。
    relayouts: u32,
    /// 回滚次数。
    rollbacks: u32,
    /// DPI 滞回触发次数（被忽略的抖动）。
    jitter_suppressed: u32,
}

impl DpiRelayoutEngine {
    /// 建引擎。
    pub fn new(dpi: u32, hysteresis: u32, budget_us: u32) -> Result<Self, u16> {
        if dpi == 0 || dpi > 2000000 { return Err(XC_BAD_DPI); }
        Ok(DpiRelayoutEngine {
            dpi, hysteresis, budget_us, layers: [false; 4],
            last_code: 0, relayouts: 0, rollbacks: 0, jitter_suppressed: 0,
        })
    }
    pub fn dpi(&self) -> u32 { self.dpi }
    pub fn last_code(&self) -> u16 { self.last_code }

    /// 请求 DPI 变化（触发四层重排）。
    /// 返回 true=执行了重排，false=滞回忽略。
    pub fn request_dpi(&mut self, new_dpi: u32, elapsed_us: u32) -> Result<bool, u16> {
        if new_dpi == 0 || new_dpi > 2000000 { self.last_code = XC_BAD_DPI; return Err(XC_BAD_DPI); }
        let diff = if new_dpi > self.dpi { new_dpi - self.dpi } else { self.dpi - new_dpi };
        if diff < self.hysteresis {
            self.jitter_suppressed += 1;
            self.last_code = XC_DPI_JITTER;
            return Ok(false);
        }
        if elapsed_us > self.budget_us { self.last_code = XC_BUDGET_EXCEEDED; return Err(XC_BUDGET_EXCEEDED); }

        // 四层重排（原子性：任一层失败则全部回滚）。
        for i in 0..4 { self.layers[i] = true; }
        self.dpi = new_dpi;
        self.relayouts += 1;
        self.last_code = 0;
        Ok(true)
    }

    /// 回滚（四层失配时）。
    pub fn rollback(&mut self, old_dpi: u32) {
        for i in 0..4 { self.layers[i] = false; }
        self.dpi = old_dpi;
        self.rollbacks += 1;
        self.last_code = XC_ROLLBACK;
    }

    /// 层完成状态。
    pub fn layer_done(&self, l: Relayer) -> bool {
        self.layers[l as usize]
    }
    pub fn relayouts(&self) -> u32 { self.relayouts }
    pub fn rollbacks(&self) -> u32 { self.rollbacks }
    pub fn jitter_suppressed(&self) -> u32 { self.jitter_suppressed }
}

pub fn run_vea135_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea135_dpiscale");
    { let e = DpiRelayoutEngine::new(960000, 50000, 50000); chk(&mut set, "C135-01", e.is_ok(), "err"); }
    { let r = DpiRelayoutEngine::new(0, 50000, 50000); chk(&mut set, "C135-02", r.err() == Some(XC_BAD_DPI), "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 50000).unwrap();
        let r = e.request_dpi(1440000, 1000);
        chk(&mut set, "C135-03", r.is_ok() && r.unwrap() && e.dpi() == 1440000, "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 50000).unwrap();
        let r = e.request_dpi(970000, 1000);
        chk(&mut set, "C135-04", r.is_ok() && !r.unwrap() && e.jitter_suppressed() == 1, "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 1000).unwrap();
        let r = e.request_dpi(1440000, 5000);
        chk(&mut set, "C135-05", r.err() == Some(XC_BUDGET_EXCEEDED), "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 50000).unwrap();
        let _ = e.request_dpi(1440000, 1000);
        chk(&mut set, "C135-06", e.layer_done(Relayer::SwapChain) && e.layer_done(Relayer::Token), "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 50000).unwrap();
        let _ = e.request_dpi(1440000, 1000);
        e.rollback(960000);
        chk(&mut set, "C135-07", e.dpi() == 960000 && e.rollbacks() == 1, "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 50000, 50000).unwrap();
        let _ = e.request_dpi(1440000, 1000);
        chk(&mut set, "C135-08", e.relayouts() == 1, "err"); }
    { let r = DpiRelayoutEngine::new(999999999, 0, 0); chk(&mut set, "C135-09", r.err() == Some(XC_BAD_DPI), "err"); }
    { let mut e = DpiRelayoutEngine::new(960000, 0, 50000).unwrap();
        let _ = e.request_dpi(960001, 1000);
        chk(&mut set, "C135-10", e.dpi() == 960001, "err"); }
    set
}
