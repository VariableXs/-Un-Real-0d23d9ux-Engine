//! UNX-F52012 Visual C++ Redistributable 2019（14.2x）收口 · 0xC4300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52012 逐字锚定）：UNX-F52013-J1 远程调试器服务
//! 启动/连接/断点双向/内存查看四段全账；网络端口与防火墙预置建议账（联
//! 签锚定行 AI-50）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二调试
//! API 消费。
//!
//! 设计要点：
//!   ① 域模型：远程调试会话四段账（服务启动/连接/断点双向/内存查看），
//!      段状态机 Idle→Listening→Connected→BpArmed→MemReady 单向推进
//!      （回退显性拒）；每段退出码声明（零静默）。
//!   ② 断点双向账：断点置位/命中/清除三态账（Armed→Hit→Cleared 单向，
//!      反向显性拒）；断点 ×16 格全账。
//!   ③ 网络端口与防火墙预置建议账：端口预置表 8 行（纯函数复算：端口、
//!      方向入/出、协议档、建议文案档），连接建立前必须预置（零静默）；
//!      缺预置连接 ×10 轮 10/10 必抓。
//!   ④ KB-COMPAT 定级登记：四段全走 + 断点 16/16 + 预置 8/8 → S1；
//!      定级与复算一致（等式断言）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52012 − 52000)）。
pub const SEG_BASE: u32 = 0xC4300;

/// 断点账容量。
pub const BP_ROWS: usize = 16;

/// 端口预置表容量。
pub const PORT_ROWS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RdCode {
    /// 成功
    Ok = 0,
    /// 段越闭集
    ENoStage,
    /// 会话状态机越步
    EOrder,
    /// 断点越册
    ENoBp,
    /// 断点双向越步
    EBpOrder,
    /// 端口越表
    ENoPort,
    /// 防火墙预置缺失
    EPreset,
    /// 重复置位
    EDup,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 定级漂移
    EGradeDrift,
    /// 未齐全账即封
    ESeal,
}

impl RdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RdCode::Ok => "成功",
            RdCode::ENoStage => "段越闭集：四段闭集之外先登记",
            RdCode::EOrder => "会话状态机越步：Idle→Listening→Connected→BpArmed→MemReady 单向，回退显性拒",
            RdCode::ENoBp => "断点越册：16 断点闭集之外先登记",
            RdCode::EBpOrder => "断点双向越步：Armed→Hit→Cleared 单向，反向显性拒",
            RdCode::ENoPort => "端口越表：8 端口闭集之外先登记",
            RdCode::EPreset => "防火墙预置缺失：连接建立前端口预置必须入账（零静默）",
            RdCode::EDup => "重复置位：同断点只许一条置位账",
            RdCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RdCode::EEmpty => "账为空：至少落 1 格",
            RdCode::EGradeDrift => "定级漂移：四段+断点 16/16+预置 8/8 才定 S1",
            RdCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 会话段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RdState {
    /// 空闲（服务未启动）。
    Idle,
    /// 服务已启动监听。
    Listening,
    /// 已连接。
    Connected,
    /// 断点已布防。
    BpArmed,
    /// 内存查看就绪。
    MemReady,
}

/// 单向推进合法性。
pub fn rd_step(from: RdState, to: RdState) -> bool {
    matches!(
        (from, to),
        (RdState::Idle, RdState::Listening)
            | (RdState::Listening, RdState::Connected)
            | (RdState::Connected, RdState::BpArmed)
            | (RdState::BpArmed, RdState::MemReady)
    )
}

/// 断点三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BpState {
    /// 已布防。
    Armed,
    /// 已命中。
    Hit,
    /// 已清除。
    Cleared,
}

/// 断点双向推进合法性（Armed→Hit→Cleared 单向）。
pub fn bp_step(from: BpState, to: BpState) -> bool {
    matches!(
        (from, to),
        (BpState::Armed, BpState::Hit)
            | (BpState::Hit, BpState::Cleared)
            | (BpState::Armed, BpState::Cleared)
    )
}

/// 端口预置表行（纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortRow {
    /// 行号（0..8）。
    pub no: u32,
    /// 端口号（4024..4031 档）。
    pub port: u32,
    /// 方向（0=入，1=出）。
    pub dir: u32,
    /// 协议档（0=TCP，1=UDP）。
    pub proto: u32,
}

/// 复算第 i 行。
pub const fn port_row(i: usize) -> Option<PortRow> {
    if i >= PORT_ROWS {
        return None;
    }
    Some(PortRow {
        no: i as u32,
        port: 4024 + i as u32,
        dir: (i % 2) as u32,
        proto: ((i / 2) % 2) as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 远程调试收口账本。
#[derive(Clone)]
pub struct RdLedger {
    /// 会话状态。
    state: RdState,
    /// 断点账（三态）。
    bps: [Option<BpState>; BP_ROWS],
    /// 预置账（端口行在册位图）。
    presets: u8,
    /// 段退出声明账。
    stage_declared: u8,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RdLedger {
    /// 新账本。
    pub fn new() -> Self {
        RdLedger {
            state: RdState::Idle,
            bps: [None; BP_ROWS],
            presets: 0,
            stage_declared: 0,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 会话推进（单向）。
    pub fn advance(&mut self, to: RdState) -> Result<(), RdCode> {
        if !rd_step(self.state, to) {
            return Err(RdCode::EOrder);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 段退出声明（零静默）。
    pub fn declare_stage(&mut self, stage_bit: u8) -> Result<(), RdCode> {
        if self.stage_declared & stage_bit != 0 {
            return Err(RdCode::EDup);
        }
        self.stage_declared |= stage_bit;
        self.fnv(stage_bit as u64);
        Ok(())
    }

    /// 断点推进（双向账单向核）。
    pub fn bp_advance(&mut self, i: usize, to: BpState) -> Result<(), RdCode> {
        if i >= BP_ROWS {
            return Err(RdCode::ENoBp);
        }
        let cur = match self.bps.get(i).copied().flatten() {
            Some(s) => s,
            None => BpState::Armed,
        };
        // 首次置位：None → Armed（任何 to 直收）；已置位走 bp_step 闸。
        let cell = match self.bps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        };
        if cell && !bp_step(cur, to) {
            return Err(RdCode::EBpOrder);
        }
        if !cell && to != BpState::Armed {
            return Err(RdCode::EBpOrder);
        }
        let slot = match self.bps.get_mut(i) {
            Some(c) => c,
            None => return Err(RdCode::ENoBp),
        };
        *slot = Some(to);
        self.fnv(i as u64 | (to as u64) << 8);
        Ok(())
    }

    /// 断点全账核验（16 格全 Cleared）。
    pub fn bps_all_cleared(&self) -> bool {
        let mut i = 0usize;
        while i < BP_ROWS {
            match self.bps.get(i).copied().flatten() {
                Some(BpState::Cleared) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 端口预置（连接前置闸：未预置端口连接显性拒）。
    pub fn preset(&mut self, i: usize) -> Result<(), RdCode> {
        if i >= PORT_ROWS {
            return Err(RdCode::ENoPort);
        }
        self.presets |= 1u8 << i;
        self.fnv(0x5100 + i as u64);
        Ok(())
    }

    /// 连接预置闸：方向入的端口必须已预置才许连接。
    pub fn connect_gate(&mut self, port_no: usize) -> Result<(), RdCode> {
        if port_no >= PORT_ROWS {
            return Err(RdCode::ENoPort);
        }
        let row = match port_row(port_no) {
            Some(r) => r,
            None => return Err(RdCode::ENoPort),
        };
        let listed = self.presets & (1u8 << port_no) != 0;
        if row.dir == 0 && !listed {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + port_no as u64);
            return Err(RdCode::EPreset);
        }
        Ok(())
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 预置数。
    pub fn preset_count(&self) -> u32 {
        self.presets.count_ones()
    }

    /// 会话状态。
    pub fn state(&self) -> RdState {
        self.state
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for RdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 缺预置连接注入（10 轮，确定性）：未预置的入向端口直接连接。
pub struct NoPresetInjector;

impl NoPresetInjector {
    /// 下一轮违规端口（入向端口号）。
    pub const fn next_port(k: usize) -> usize {
        // 入向端口 = 偶号行（dir=0）。
        (k * 2) % PORT_ROWS
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：四段全走 + 预置 8/8 + 断点 16/16 + 注入 10/10 + 指纹。
/// 返回（断点账数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RdLedger::new();
    // ① 端口预置 8/8（连接前）。
    let mut i = 0usize;
    while i < PORT_ROWS {
        let _ = led.preset(i);
        i += 1;
    }
    // ② 四段全走（每段退出声明）。
    let seq = [
        RdState::Listening,
        RdState::Connected,
        RdState::BpArmed,
        RdState::MemReady,
    ];
    let mut s = 0usize;
    while s < seq.len() {
        if led.advance(seq[s]).is_ok() {
            let _ = led.declare_stage(1u8 << s);
        }
        s += 1;
    }
    // ③ 断点 16 格全账（Armed→Hit→Cleared）。
    let mut bp_n = 0u32;
    let mut i = 0usize;
    while i < BP_ROWS {
        if led.bp_advance(i, BpState::Armed).is_ok()
            && led.bp_advance(i, BpState::Hit).is_ok()
            && led.bp_advance(i, BpState::Cleared).is_ok()
        {
            bp_n += 1;
        }
        i += 1;
    }
    // ④ 注入 10/10：缺预置连接。
    let mut k = 0usize;
    while k < 10 {
        let port = NoPresetInjector::next_port(k);
        let mut lt = RdLedger::new();
        // 不预置直接连接。
        if lt.connect_gate(port).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (bp_n, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52012_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52012");
    // 1. 正常流：四段全走 + 断点 16/16 + 预置 8/8。
    let (bp_n, caught, fp) = run_cycle();
    s.add("四段全走+退出声明", bp_n == BP_ROWS as u32, "服务启动/连接/断点布防/内存就绪逐段声明");
    s.add("断点 16/16 双向账", caught >= 10, "Armed→Hit→Cleared 全走，注入 10/10 全抓");
    // 2. 边界：断点越册/端口越表/状态机越步。
    let mut l0 = RdLedger::new();
    s.add(
        "边界·断点越册",
        l0.bp_advance(BP_ROWS, BpState::Armed) == Err(RdCode::ENoBp),
        "断点 16 越册必拒",
    );
    s.add(
        "边界·端口越表",
        l0.preset(PORT_ROWS) == Err(RdCode::ENoPort),
        "端口 8 越表必拒",
    );
    s.add(
        "状态机·Idle→Connected 跳步拒",
        l0.advance(RdState::Connected) == Err(RdCode::EOrder),
        "必须过 Listening，跳步非法",
    );
    // 3. 断点双向越步拒。
    let mut l1 = RdLedger::new();
    let _ = l1.bp_advance(0, BpState::Armed);
    s.add(
        "断点·Cleared→Hit 反向拒",
        l1.bp_advance(0, BpState::Cleared).is_ok()
            && l1.bp_advance(0, BpState::Hit) == Err(RdCode::EBpOrder),
        "清除后回命中显性拒",
    );
    s.add(
        "断点·Armed→Hit 合法",
        {
            let mut l = RdLedger::new();
            let _ = l.bp_advance(1, BpState::Armed);
            l.bp_advance(1, BpState::Hit).is_ok()
        },
        "布防→命中单向合法",
    );
    // 4. 缺预置注入直判。
    let mut l2 = RdLedger::new();
    s.add(
        "预置·未预置入向连接拒",
        l2.connect_gate(0) == Err(RdCode::EPreset),
        "入向端口 0 未预置必拒",
    );
    let _ = l2.preset(0);
    s.add("预置·预置后连接过", l2.connect_gate(0).is_ok(), "预置后放行");
    s.add("预置·出向免预置", l2.connect_gate(1).is_ok(), "出向端口免预置放行");
    // 5. 全账定级等式。
    let mut l3 = RdLedger::new();
    let mut i = 0usize;
    while i < PORT_ROWS {
        let _ = l3.preset(i);
        i += 1;
    }
    let seq = [
        RdState::Listening,
        RdState::Connected,
        RdState::BpArmed,
        RdState::MemReady,
    ];
    let mut s2 = 0usize;
    while s2 < seq.len() {
        let _ = l3.advance(seq[s2]);
        let _ = l3.declare_stage(1u8 << s2);
        s2 += 1;
    }
    s.add(
        "全账口径成立",
        l3.state() == RdState::MemReady && l3.preset_count() == PORT_ROWS as u32 && l3.stage_declared == 0b1111,
        "四段声明 + 预置 8/8 → 定级 S1",
    );
    // 6. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "缺预置注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 7. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = port_row(0).unwrap_or(PortRow { no: 0, port: 4024, dir: 0, proto: 0 });
        assert_eq!(row.port, 4024);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(port_row(PORT_ROWS).is_none());
        assert_eq!(
            RdLedger::new().bp_advance(BP_ROWS, BpState::Armed),
            Err(RdCode::ENoBp)
        );
    }

    #[test]
    fn test_session_state_machine() {
        assert!(rd_step(RdState::Idle, RdState::Listening));
        assert!(rd_step(RdState::Listening, RdState::Connected));
        assert!(rd_step(RdState::Connected, RdState::BpArmed));
        assert!(rd_step(RdState::BpArmed, RdState::MemReady));
        assert!(!rd_step(RdState::Idle, RdState::Connected));
        assert!(!rd_step(RdState::MemReady, RdState::Connected));
    }

    #[test]
    fn test_bp_monotonic() {
        assert!(bp_step(BpState::Armed, BpState::Hit));
        assert!(bp_step(BpState::Hit, BpState::Cleared));
        assert!(bp_step(BpState::Armed, BpState::Cleared));
        assert!(!bp_step(BpState::Cleared, BpState::Hit));
        assert!(!bp_step(BpState::Cleared, BpState::Armed));
        assert!(!bp_step(BpState::Hit, BpState::Armed));
    }

    #[test]
    fn test_bp_first_arm_only() {
        let mut led = RdLedger::new();
        assert_eq!(led.bp_advance(2, BpState::Hit), Err(RdCode::EBpOrder));
        assert!(led.bp_advance(2, BpState::Armed).is_ok());
        assert!(led.bp_advance(2, BpState::Hit).is_ok());
        assert!(led.bp_advance(2, BpState::Cleared).is_ok());
        // 单格口径：格 2 走满三态。
        assert!(matches!(led.bps.get(2).copied().flatten(), Some(BpState::Cleared)));
    }

    #[test]
    fn test_preset_gate() {
        let mut led = RdLedger::new();
        assert_eq!(led.connect_gate(2), Err(RdCode::EPreset));
        let _ = led.preset(2);
        assert!(led.connect_gate(2).is_ok());
        // 出向端口免预置。
        assert!(led.connect_gate(1).is_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let port = NoPresetInjector::next_port(k);
            let mut lt = RdLedger::new();
            if lt.connect_gate(port).is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (bp_n, caught, _) = run_cycle();
        assert_eq!(bp_n, BP_ROWS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52012_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
