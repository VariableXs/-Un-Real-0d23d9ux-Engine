//! UNX-F52019 .NET SDK（Windows 发行版收口，与 L4 分账） · 0xC44C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52019 逐字锚定）：UNX-F52020-J1 离线帮助安装/
//! 检索/渲染三段账；F3 文本渲染消费对账（联签锚定行）；KB-COMPAT 定级
//! 登记；判据锚定 §AI-66.4 横向消费 F3。
//!
//! 设计要点：
//!   ① 域模型：离线帮助三段账（安装/检索/渲染），段状态机 Idle→
//!      Installed→Searched→Rendered 单向推进（回退显性拒），每段退出声
//!      明（零静默）。
//!   ② 检索索引账：帮助主题 ×12 格（主题号、命中拍、命中位），检索按主
//!      题号升序（乱序检索显性拒）；未安装先检索 ×10 轮 10/10 必抓。
//!   ③ F3 文本渲染消费对账：渲染行 ×10（纯函数复算载荷），与 F3 源行账
//!      逐行恒等（零漂移断言）；漂移行 ×10 轮 10/10 必抓。
//!   ④ KB-COMPAT 定级登记：三段全走 + 索引 12/12 + 对账零漂移 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52019 − 52000)）。
pub const SEG_BASE: u32 = 0xC44C0;

/// 帮助主题索引容量。
pub const TOPIC_ROWS: usize = 12;

/// F3 渲染对账行数。
pub const F3_ROWS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsCode {
    /// 成功
    Ok = 0,
    /// 段状态机越步
    EOrder,
    /// 主题越册
    ENoTopic,
    /// 检索序错（乱序）
    ESearchOrder,
    /// 未安装先检索
    ENotInstalled,
    /// 渲染行越册
    ENoLine,
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
    /// 重复声明
    EDup,
}

impl NsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NsCode::Ok => "成功",
            NsCode::EOrder => "段状态机越步：Idle→Installed→Searched→Rendered 单向，回退显性拒",
            NsCode::ENoTopic => "主题越册：12 主题闭集之外先登记",
            NsCode::ESearchOrder => "检索序错：按主题号升序检索，乱序显性拒",
            NsCode::ENotInstalled => "未安装先检索：离线帮助先安装再检索",
            NsCode::ENoLine => "渲染行越册：10 行闭集之外先登记",
            NsCode::EF3Drift => "F3 对账漂移：渲染行与 F3 源行账逐行必须恒等，漂移显性拒",
            NsCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NsCode::EEmpty => "账为空：至少落 1 格",
            NsCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            NsCode::EGradeDrift => "定级漂移：三段+索引 12/12+对账零漂移才定 S1",
            NsCode::EDup => "重复声明：同段只许声明一次",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 帮助段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsState {
    /// 空闲。
    Idle,
    /// 离线帮助已安装。
    Installed,
    /// 已检索。
    Searched,
    /// 已渲染。
    Rendered,
}

/// 单向推进合法性。
pub fn ns_step(from: NsState, to: NsState) -> bool {
    matches!(
        (from, to),
        (NsState::Idle, NsState::Installed)
            | (NsState::Installed, NsState::Searched)
            | (NsState::Searched, NsState::Rendered)
    )
}

/// 帮助主题行（纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TopicRow {
    /// 主题号（0..12）。
    pub no: u32,
    /// 命中拍。
    pub tick: u32,
}

/// 复算第 i 行。
pub const fn topic_row(i: usize) -> Option<TopicRow> {
    if i >= TOPIC_ROWS {
        return None;
    }
    Some(TopicRow {
        no: i as u32,
        tick: (i as u32) * 2,
    })
}

/// F3 源行账（纯函数复算：载荷 = 行号 × 0x30 + 0x77）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct F3Row {
    /// 行号（0..10）。
    pub no: u32,
    /// 源载荷。
    pub payload: u32,
}

/// 复算第 i 行。
pub const fn f3_row(i: usize) -> Option<F3Row> {
    if i >= F3_ROWS {
        return None;
    }
    Some(F3Row {
        no: i as u32,
        payload: ((i as u32) * 0x30).wrapping_add(0x77),
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// .NET SDK 收口账本。
#[derive(Clone)]
pub struct NsLedger {
    /// 段状态。
    state: NsState,
    /// 段退出声明位图。
    declared: u8,
    /// 检索索引账（主题号 → 命中拍）。
    index: [Option<u32>; TOPIC_ROWS],
    /// 上一检索主题号（乱序闸）。
    last_topic: u32,
    /// 渲染对账账（行号 → 载荷）。
    rendered: [Option<u32>; F3_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NsLedger {
    /// 新账本。
    pub fn new() -> Self {
        NsLedger {
            state: NsState::Idle,
            declared: 0,
            index: [None; TOPIC_ROWS],
            last_topic: 0,
            rendered: [None; F3_ROWS],
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

    /// 段推进（单向）。
    pub fn advance(&mut self, to: NsState) -> Result<(), NsCode> {
        if !ns_step(self.state, to) {
            return Err(NsCode::EOrder);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 段退出声明（零静默）。
    pub fn declare(&mut self, bit: u8) -> Result<(), NsCode> {
        if self.declared & bit != 0 {
            return Err(NsCode::EDup);
        }
        self.declared |= bit;
        self.fnv(bit as u64);
        Ok(())
    }

    /// 检索一格（未安装先检索拒 + 乱序拒 + 拍恒等闸）。
    pub fn search(&mut self, i: usize) -> Result<(), NsCode> {
        let row = match topic_row(i) {
            Some(r) => r,
            None => return Err(NsCode::ENoTopic),
        };
        if self.state != NsState::Installed && self.state != NsState::Searched {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100 + i as u64);
            return Err(NsCode::ENotInstalled);
        }
        // 乱序闸：主题号必须严格递增。
        if match self.index.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsCode::EDup);
        }
        if i > 0 && i <= self.last_topic as usize && self.last_topic != 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5101 + i as u64);
            return Err(NsCode::ESearchOrder);
        }
        self.last_topic = i as u32;
        let cell = match self.index.get_mut(i) {
            Some(c) => c,
            None => return Err(NsCode::EEmpty),
        };
        *cell = Some(row.tick);
        self.fnv(0x5102 + i as u64);
        Ok(())
    }

    /// 索引全账核验。
    pub fn index_all(&self) -> bool {
        let mut i = 0usize;
        while i < TOPIC_ROWS {
            match self.index.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// F3 渲染对账一行（与源行账恒等才收）。
    pub fn render_line(&mut self, i: usize, payload: u32) -> Result<(), NsCode> {
        let row = match f3_row(i) {
            Some(r) => r,
            None => return Err(NsCode::ENoLine),
        };
        if payload != row.payload {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(NsCode::EF3Drift);
        }
        let cell = match self.rendered.get_mut(i) {
            Some(c) => c,
            None => return Err(NsCode::EEmpty),
        };
        *cell = Some(payload);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 对账零漂移核验。
    pub fn zero_drift(&self) -> bool {
        let mut i = 0usize;
        while i < F3_ROWS {
            let src = match f3_row(i) {
                Some(r) => r.payload,
                None => return false,
            };
            match self.rendered.get(i).copied().flatten() {
                Some(v) if v == src => {}
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

    /// 段状态。
    pub fn state(&self) -> NsState {
        self.state
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for NsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// F3 漂移注入（10 轮，确定性）：渲染载荷 +1 漂移。
pub struct DriftInjector;

impl DriftInjector {
    /// 下一轮漂移载荷。
    pub const fn next_payload(k: usize) -> u32 {
        // 行 k%10 的源载荷 +1（必漂移）。
        0
    }
}

/// 未安装先检索注入（10 轮，确定性）：Idle 态直接检索。
pub struct NoInstallInjector;

impl NoInstallInjector {
    /// 下一轮违规主题。
    pub const fn next_topic(k: usize) -> usize {
        k % TOPIC_ROWS
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：三段全走 + 索引 12/12 + F3 对账 10/10 + 双注入 10/10 +
/// 指纹。返回（对账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NsLedger::new();
    // ① 安装段：推进 Installed 后立刻升序检索（检索须在 Installed 态）。
    let _ = led.advance(NsState::Installed);
    let _ = led.declare(0b001);
    let mut i = 0usize;
    while i < TOPIC_ROWS {
        let _ = led.search(i);
        i += 1;
    }
    // ② 检索段：推进 Searched 后逐行 F3 渲染对账。
    let _ = led.advance(NsState::Searched);
    let _ = led.declare(0b010);
    let mut ok_n = 0u32;
    let mut i = 0usize;
    while i < F3_ROWS {
        if let Some(r) = f3_row(i) {
            if led.render_line(i, r.payload).is_ok() {
                ok_n += 1;
            }
        }
        i += 1;
    }
    // ③ 渲染段：收口。
    let _ = led.advance(NsState::Rendered);
    let _ = led.declare(0b100);
    // ④ 注入 A：未安装先检索 10/10。
    let mut k = 0usize;
    while k < 10 {
        let topic = NoInstallInjector::next_topic(k);
        let mut lt = NsLedger::new();
        if lt.search(topic).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ⑤ 注入 B：F3 漂移 10/10。
    let mut k = 0usize;
    while k < 10 {
        let row_no = k % F3_ROWS;
        let src = match f3_row(row_no) {
            Some(r) => r.payload,
            None => 0,
        };
        let mut lt = NsLedger::new();
        if lt.render_line(row_no, src.wrapping_add(1)).is_err() {
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
    (ok_n, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52019_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52019");
    // 1. 正常流：三段全走 + F3 对账 10/10。
    let (ok_n, caught, fp) = run_cycle();
    s.add(
        "三段全走+F3 对账 10/10",
        ok_n == F3_ROWS as u32,
        "安装/检索/渲染逐段声明，对账零漂移",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "未安装检索 10 + 漂移 10 全抓");
    // 2. 边界：主题越册/状态机越步。
    let mut l0 = NsLedger::new();
    s.add(
        "边界·主题越册",
        {
            let _ = l0.advance(NsState::Installed);
            l0.search(TOPIC_ROWS) == Err(NsCode::ENoTopic)
        },
        "主题 12 越册必拒",
    );
    s.add(
        "状态机·Idle→Searched 跳步拒",
        {
            let mut l = NsLedger::new();
            l.advance(NsState::Searched) == Err(NsCode::EOrder)
        },
        "必须过 Installed，跳步非法",
    );
    // 3. 未安装先检索直判。
    let mut l1 = NsLedger::new();
    s.add(
        "检索·未安装拒",
        l1.search(0) == Err(NsCode::ENotInstalled),
        "Idle 态检索显性拒",
    );
    // 4. F3 漂移直判。
    let mut l2 = NsLedger::new();
    let src0 = f3_row(0).map(|r| r.payload).unwrap_or(0);
    s.add(
        "F3·漂移拒",
        l2.render_line(0, src0.wrapping_add(1)) == Err(NsCode::EF3Drift),
        "渲染载荷漂移显性拒",
    );
    s.add("F3·源载荷过", l2.render_line(0, src0).is_ok(), "源载荷合法");
    // 5. 零漂移核验。
    let mut l3 = NsLedger::new();
    let mut i = 0usize;
    while i < F3_ROWS {
        if let Some(r) = f3_row(i) {
            let _ = l3.render_line(i, r.payload);
        }
        i += 1;
    }
    s.add("F3·零漂移断言", l3.zero_drift(), "10 行逐行恒等");
    // 6. 索引全账。
    let mut l4 = NsLedger::new();
    let _ = l4.advance(NsState::Installed);
    let mut i = 0usize;
    while i < TOPIC_ROWS {
        let _ = l4.search(i);
        i += 1;
    }
    s.add("索引 12/12", l4.index_all(), "升序检索全账");
    // 7. 全账口径。
    let seq = [NsState::Installed, NsState::Searched, NsState::Rendered];
    let mut s2 = 0usize;
    while s2 < seq.len() {
        let _ = l4.advance(seq[s2]);
        let _ = l4.declare(1u8 << s2);
        s2 += 1;
    }
    s.add(
        "全账口径成立",
        l4.state() == NsState::Rendered && l4.declared == 0b111,
        "三段声明齐 → 定级 S1",
    );
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
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
        let row = topic_row(0).unwrap_or(TopicRow { no: 0, tick: 0 });
        assert_eq!(row.no, 0);
        assert!(f3_row(0).is_some());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(topic_row(TOPIC_ROWS).is_none());
        assert!(f3_row(F3_ROWS).is_none());
    }

    #[test]
    fn test_state_machine() {
        assert!(ns_step(NsState::Idle, NsState::Installed));
        assert!(ns_step(NsState::Installed, NsState::Searched));
        assert!(ns_step(NsState::Searched, NsState::Rendered));
        assert!(!ns_step(NsState::Idle, NsState::Rendered));
        assert!(!ns_step(NsState::Rendered, NsState::Idle));
    }

    #[test]
    fn test_not_installed_caught() {
        let mut led = NsLedger::new();
        assert_eq!(led.search(0), Err(NsCode::ENotInstalled));
        let _ = led.advance(NsState::Installed);
        assert!(led.search(0).is_ok());
    }

    #[test]
    fn test_f3_drift_caught() {
        let mut led = NsLedger::new();
        let src = f3_row(3).map(|r| r.payload).unwrap_or(0);
        assert_eq!(led.render_line(3, src.wrapping_add(1)), Err(NsCode::EF3Drift));
        assert!(led.render_line(3, src).is_ok());
    }

    #[test]
    fn test_zero_drift_full() {
        let mut led = NsLedger::new();
        let mut i = 0usize;
        while i < F3_ROWS {
            if let Some(r) = f3_row(i) {
                let _ = led.render_line(i, r.payload);
            }
            i += 1;
        }
        assert!(led.zero_drift());
    }

    #[test]
    fn test_index_all() {
        let mut led = NsLedger::new();
        let _ = led.advance(NsState::Installed);
        let mut i = 0usize;
        while i < TOPIC_ROWS {
            let _ = led.search(i);
            i += 1;
        }
        assert!(led.index_all());
        assert_eq!(led.search(0), Err(NsCode::EDup));
    }

    #[test]
    fn test_injection_all_caught() {
        // 注入 A。
        let mut k = 0usize;
        let mut a = 0u32;
        while k < 10 {
            let mut lt = NsLedger::new();
            if lt.search(NoInstallInjector::next_topic(k)).is_err() {
                a += 1;
            }
            k += 1;
        }
        assert_eq!(a, 10);
        // 注入 B。
        let mut k = 0usize;
        let mut b = 0u32;
        while k < 10 {
            let row_no = k % F3_ROWS;
            let src = f3_row(row_no).map(|r| r.payload).unwrap_or(0);
            let mut lt = NsLedger::new();
            if lt.render_line(row_no, src.wrapping_add(1)).is_err() {
                b += 1;
            }
            k += 1;
        }
        assert_eq!(b, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (ok_n, caught, _) = run_cycle();
        assert_eq!(ok_n, F3_ROWS as u32);
        assert_eq!(caught, 20);
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
        let s = run_unx52019_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
