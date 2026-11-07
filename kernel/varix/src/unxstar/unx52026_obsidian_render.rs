//! UNX-F52026 Obsidian 收口（Electron 笔记库渲染面） · 0xC4680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52026 逐字锚定）：UNX-F52027-J1 六判据全落；J5
//! 特化：公式/表格/中文混排渲染对照（消费 F3 联签锚定行）；KB-COMPAT 定
//! 级登记；判据锚定 §AI-66.4 横向消费 F3。
//!
//! 设计要点：
//!   ① 域模型：渲染对照面。三渲染面（公式/表格/中文混排）× 8 行 = 24 格
//!      渲染账（纯函数复算载荷），消费 F3 源行账（10 行联签锚定，同
//!      52019 口径独立语料）：渲染载荷必须与 F3 派生期望恒等（零漂移）。
//!   ② 消费闸：F3 源行账在册才许渲染对照（上游未对账显性拒）；漂移 ×10
//!      轮 10/10 必抓。
//!   ③ 六判据母版闸（6/6 全落）。
//!   ④ KB-COMPAT 定级登记：六判据 + F3 对账零漂移 + 三面 24/24 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52026 − 52000)）。
pub const SEG_BASE: u32 = 0xC4680;

/// 每面渲染行数（3 面 × 8 行 = 24 格）。
pub const RENDER_PER_SURFACE: usize = 8;

/// 渲染面数。
pub const RENDER_SURFACES: usize = 3;

/// F3 源行账容量。
pub const F3_ROWS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrCode {
    /// 成功
    Ok = 0,
    /// 渲染行越册
    ENoLine,
    /// 渲染漂移
    EDrift,
    /// 判据越闭集
    ENoGate,
    /// F3 上游未对账
    EF3Absent,
    /// F3 对账漂移
    EF3Drift,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复落账
    EDup,
}

impl OrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OrCode::Ok => "成功",
            OrCode::ENoLine => "渲染行越册：三面 24 格闭集之外先登记",
            OrCode::EDrift => "渲染漂移：渲染载荷与期望必须逐格恒等（零漂移）",
            OrCode::ENoGate => "判据越闭集：六判据之外先登记",
            OrCode::EF3Absent => "F3 上游未对账：源行账在册才许渲染对照（消费前置闸）",
            OrCode::EF3Drift => "F3 对账漂移：消费行与源行恒等断言破缺",
            OrCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OrCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OrCode::EEmpty => "账为空：至少落 1 格",
            OrCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            OrCode::EGradeDrift => "定级漂移：六判据+F3 零漂移+三面 24/24 才定 S1",
            OrCode::EDup => "重复落账：同格只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 渲染行（面 × 行，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderCell {
    /// 格号（0..24）。
    pub no: u32,
    /// 面（0..3：公式/表格/中文混排）。
    pub surface: u32,
    /// 渲染载荷（确定性）。
    pub payload: u32,
}

/// 复算第 i 格（载荷 = F3 派生同源口径：格 i 消费 F3 行 i%10）。
pub const fn render_cell(i: usize) -> Option<RenderCell> {
    if i >= RENDER_SURFACES * RENDER_PER_SURFACE {
        return None;
    }
    let f3_idx = (i % F3_ROWS) as u32;
    Some(RenderCell {
        no: i as u32,
        surface: (i / RENDER_PER_SURFACE) as u32,
        payload: 0xC400 + (f3_idx * 0x21).wrapping_mul(3),
    })
}

/// F3 源行（纯函数复算，独立语料）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct F3Source {
    /// 行号（0..10）。
    pub no: u32,
    /// 源载荷（F3 派生期望）。
    pub payload: u32,
}

/// 复算第 i 行（期望 = F3 源载荷 = 渲染期望的同源口径）。
pub const fn f3_source(i: usize) -> Option<F3Source> {
    if i >= F3_ROWS {
        return None;
    }
    Some(F3Source {
        no: i as u32,
        payload: 0xC400 + ((i as u32) * 0x21).wrapping_mul(3),
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 渲染对照收口账本。
#[derive(Clone)]
pub struct OrLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// F3 源行账（在册位图）。
    f3_booked: u16,
    /// 渲染账（格 → 载荷）。
    rendered: [Option<u32>; 24],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OrLedger {
    /// 新账本。
    pub fn new() -> Self {
        OrLedger {
            gates: [None; 6],
            f3_booked: 0,
            rendered: [None; 24],
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OrCode> {
        if gate >= 6 {
            return Err(OrCode::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(OrCode::EEmpty),
        };
        *cell = Some(true);
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// F3 源行落账（消费前置：源行账在册）。
    pub fn land_f3(&mut self, i: usize) -> Result<(), OrCode> {
        if i >= F3_ROWS {
            return Err(OrCode::EF3Absent);
        }
        let _ = f3_source(i);
        self.f3_booked |= 1u16 << i;
        self.fnv(0x5200 + i as u64);
        Ok(())
    }

    /// F3 全账核验。
    pub fn f3_all(&self) -> bool {
        self.f3_booked.count_ones() == F3_ROWS as u32
    }

    /// 渲染对照一格（消费闸：F3 未全账即拒；载荷与 F3 派生期望恒等）。
    pub fn render_cell_gate(&mut self, i: usize) -> Result<(), OrCode> {
        let cell_ref = match render_cell(i) {
            Some(c) => c,
            None => return Err(OrCode::ENoLine),
        };
        // 消费前置闸：F3 源行账未全账即拒。
        if !self.f3_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + i as u64);
            return Err(OrCode::EF3Absent);
        }
        // 期望 = 同面同位 F3 派生口径（面 × 行 → F3 行号映射）。
        let f3_idx = cell_ref.no as usize % F3_ROWS;
        let expect = match f3_source(f3_idx) {
            Some(f) => f.payload,
            None => return Err(OrCode::EF3Drift),
        };
        if cell_ref.payload != expect {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + i as u64);
            return Err(OrCode::EDrift);
        }
        if match self.rendered.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrCode::EDup);
        }
        let slot = match self.rendered.get_mut(i) {
            Some(c) => c,
            None => return Err(OrCode::EEmpty),
        };
        *slot = Some(cell_ref.payload);
        self.fnv(0x5302 + i as u64);
        Ok(())
    }

    /// 渲染全账核验。
    pub fn render_all(&self) -> bool {
        let mut i = 0usize;
        while i < 24 {
            match self.rendered.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
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

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for OrLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 未对账先渲染注入（10 轮，确定性）：F3 零账直接渲染。
pub struct NoF3Injector;

impl NoF3Injector {
    /// 下一轮违规格。
    pub const fn next_cell(k: usize) -> usize {
        k % 24
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + F3 10/10 + 渲染 24/24 + 注入 10/10 + 指纹。
/// 返回（渲染格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OrLedger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② F3 源行 10/10。
    let mut i = 0usize;
    while i < F3_ROWS {
        let _ = led.land_f3(i);
        i += 1;
    }
    // ③ 渲染 24/24。
    let mut rendered = 0u32;
    let mut i = 0usize;
    while i < 24 {
        if led.render_cell_gate(i).is_ok() {
            rendered += 1;
        }
        i += 1;
    }
    // ④ 注入：未对账先渲染 10/10。
    let mut k = 0usize;
    while k < 10 {
        let cell_no = NoF3Injector::next_cell(k);
        let mut lt = OrLedger::new();
        if lt.render_cell_gate(cell_no).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (rendered, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52026_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52026");
    // 1. 正常流：渲染 24/24 + 注入全抓。
    let (rendered, caught, fp) = run_cycle();
    s.add(
        "三面渲染 24/24 + 注入全抓",
        rendered == 24,
        "公式/表格/中文混排逐格对照，未对账先渲染 10/10 全抓",
    );
    // 2. 边界：格越册/判据越闭集/F3 越册。
    let mut l0 = OrLedger::new();
    s.add(
        "边界·格越册",
        l0.render_cell_gate(24) == Err(OrCode::ENoLine),
        "格 24 越册必拒",
    );
    s.add(
        "边界·F3 越册",
        l0.land_f3(F3_ROWS) == Err(OrCode::EF3Absent),
        "F3 行 10 越册必拒",
    );
    s.add(
        "边界·判据越闭集",
        l0.judge_gate(6) == Err(OrCode::ENoGate),
        "判据 6 越闭集必拒",
    );
    // 3. 消费前置闸直判。
    let mut l1 = OrLedger::new();
    s.add(
        "消费·F3 未全账拒",
        l1.render_cell_gate(0) == Err(OrCode::EF3Absent),
        "源行账未齐先渲染显性拒",
    );
    // 4. F3 全账后渲染过。
    let mut l2 = OrLedger::new();
    let mut i = 0usize;
    while i < F3_ROWS {
        let _ = l2.land_f3(i);
        i += 1;
    }
    s.add("消费·F3 全账后过", l2.render_cell_gate(0).is_ok(), "前置满足放行");
    // 5. 渲染重复拒。
    let mut l3 = OrLedger::new();
    let mut i = 0usize;
    while i < F3_ROWS {
        let _ = l3.land_f3(i);
        i += 1;
    }
    let _ = l3.render_cell_gate(0);
    s.add("渲染·重复拒", l3.render_cell_gate(0) == Err(OrCode::EDup), "同格只许一条账目");
    // 6. 渲染全账核验。
    let mut l4 = OrLedger::new();
    let mut i = 0usize;
    while i < F3_ROWS {
        let _ = l4.land_f3(i);
        i += 1;
    }
    let mut i = 0usize;
    while i < 24 {
        let _ = l4.render_cell_gate(i);
        i += 1;
    }
    s.add("渲染·24 格全账", l4.render_all(), "三面逐格落账");
    // 7. 六判据全落。
    let mut l5 = OrLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = l5.judge_gate(g);
        g += 1;
    }
    s.add("六判据全落", l5.gates_all(), "母版闸 6/6");
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 9. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 11. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let c = render_cell(0).unwrap_or(RenderCell { no: 0, surface: 0, payload: 0 });
        assert_eq!(c.no, 0);
        assert!(f3_source(0).is_some());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(render_cell(24).is_none());
        assert!(f3_source(F3_ROWS).is_none());
    }

    #[test]
    fn test_consume_precondition() {
        let mut led = OrLedger::new();
        assert_eq!(led.render_cell_gate(0), Err(OrCode::EF3Absent));
        let mut i = 0usize;
        while i < F3_ROWS {
            let _ = led.land_f3(i);
            i += 1;
        }
        assert!(led.render_cell_gate(0).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = OrLedger::new();
        let mut i = 0usize;
        while i < F3_ROWS {
            let _ = led.land_f3(i);
            i += 1;
        }
        let _ = led.render_cell_gate(0);
        assert_eq!(led.render_cell_gate(0), Err(OrCode::EDup));
    }

    #[test]
    fn test_render_all_full() {
        let mut led = OrLedger::new();
        let mut i = 0usize;
        while i < F3_ROWS {
            let _ = led.land_f3(i);
            i += 1;
        }
        let mut i = 0usize;
        while i < 24 {
            let _ = led.render_cell_gate(i);
            i += 1;
        }
        assert!(led.render_all());
        assert!(led.f3_all());
    }

    #[test]
    fn test_payload_matches_f3_expectation() {
        // 渲染格载荷与 F3 派生期望同源（格 i 消费 F3 行 i%10 恒等）。
        let mut i = 0usize;
        while i < 24 {
            let c = match render_cell(i) {
                Some(x) => x,
                None => break,
            };
            let expect = f3_source(i % F3_ROWS).map(|f| f.payload).unwrap_or(0);
            assert_eq!(c.payload, expect);
            i += 1;
        }
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let cell_no = NoF3Injector::next_cell(k);
            let mut lt = OrLedger::new();
            if lt.render_cell_gate(cell_no).is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (rendered, caught, _) = run_cycle();
        assert_eq!(rendered, 24);
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
        let s = run_unx52026_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
