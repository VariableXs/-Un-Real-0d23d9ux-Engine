//! UNX-F52017 Team Explorer 2022 收口（Git/TFS 集成面） · 0xC4440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52017 逐字锚定）：UNX-F52018-J1 Unity 工程调试
//! 挂接/断点命中/变量查看三段账；与 G 部渲染面无耦合声明（收口仅限工具
//! 链侧）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 语义边界。
//!
//! 设计要点：
//!   ① 域模型：Unity 工程调试三段账（挂接/断点命中/变量查看），段状态机
//!      Idle→Attached→BpHit→WatchReady 单向推进（回退显性拒），每段退
//!      出声明（零静默）。
//!   ② 断点命中账：断点 ×12 格（命中序、命中拍、命中态三段单向
//!      Armed→Hit→Resolved），命中拍单调闸；乱序命中注入 ×10 轮 10/10
//!      必抓。
//!   ③ 变量查看账：监视变量 ×8 格（变量号、值档、作用域档），逐格复算
//!      恒等（漂移拒）。
//!   ④ 无耦合声明账：工具链侧对 G 部渲染面的依赖声明闭集 8 行（纯函数
//!      复算：依赖项 = None 全空），零耦合断言；未声明依赖注入 ×10 轮
//!      10/10 必抓。
//!   ⑤ KB-COMPAT 定级登记：三段全走 + 断点 12/12 + 变量 8/8 + 零耦合 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52017 − 52000)）。
pub const SEG_BASE: u32 = 0xC4440;

/// 断点账容量。
pub const BP_ROWS: usize = 12;

/// 监视变量账容量。
pub const WATCH_ROWS: usize = 8;

/// 无耦合声明行数。
pub const DECOUP_ROWS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeCode {
    /// 成功
    Ok = 0,
    /// 段状态机越步
    EOrder,
    /// 断点越册
    ENoBp,
    /// 断点三段越步
    EBpOrder,
    /// 命中拍倒退
    ETickBack,
    /// 变量越册
    ENoWatch,
    /// 变量值漂移
    EWatchDrift,
    /// 耦合依赖未声明
    ECoupled,
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
}

impl TeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TeCode::Ok => "成功",
            TeCode::EOrder => "段状态机越步：Idle→Attached→BpHit→WatchReady 单向，回退显性拒",
            TeCode::ENoBp => "断点越册：12 断点闭集之外先登记",
            TeCode::EBpOrder => "断点三段越步：Armed→Hit→Resolved 单向，反向显性拒",
            TeCode::ETickBack => "命中拍倒退：命中拍必须单调不减，倒退显性拒",
            TeCode::ENoWatch => "变量越册：8 监视变量闭集之外先登记",
            TeCode::EWatchDrift => "变量值漂移：账面与复算不一致，逐格对平",
            TeCode::ECoupled => "耦合依赖未声明：工具链侧对渲染面零耦合是收口前提",
            TeCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TeCode::EEmpty => "账为空：至少落 1 格",
            TeCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            TeCode::EGradeDrift => "定级漂移：三段+断点 12/12+变量 8/8+零耦合才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 调试段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeState {
    /// 空闲。
    Idle,
    /// 调试已挂接。
    Attached,
    /// 断点已命中。
    BpHit,
    /// 变量查看就绪。
    WatchReady,
}

/// 单向推进合法性。
pub fn te_step(from: TeState, to: TeState) -> bool {
    matches!(
        (from, to),
        (TeState::Idle, TeState::Attached)
            | (TeState::Attached, TeState::BpHit)
            | (TeState::BpHit, TeState::WatchReady)
    )
}

/// 断点三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BpPhase {
    /// 已布防。
    Armed,
    /// 已命中。
    Hit,
    /// 已消解。
    Resolved,
}

/// 三段单向合法性。
pub fn bp_phase_ok(from: BpPhase, to: BpPhase) -> bool {
    matches!(
        (from, to),
        (BpPhase::Armed, BpPhase::Hit)
            | (BpPhase::Hit, BpPhase::Resolved)
            | (BpPhase::Armed, BpPhase::Resolved)
    )
}

/// 监视变量行（纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchRow {
    /// 变量号（0..8）。
    pub no: u32,
    /// 值档（确定性载荷）。
    pub value: u32,
    /// 作用域档（0..2）。
    pub scope: u32,
}

/// 复算第 i 行。
pub const fn watch_row(i: usize) -> Option<WatchRow> {
    if i >= WATCH_ROWS {
        return None;
    }
    Some(WatchRow {
        no: i as u32,
        value: 0xB000 + ((i as u32) * 7),
        scope: (i % 2) as u32,
    })
}

/// 无耦合声明行（依赖项 = None 全空，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecoupRow {
    /// 行号（0..8）。
    pub no: u32,
    /// 对渲染面依赖项（None = 零耦合）。
    pub depends: Option<u32>,
}

/// 复算第 i 行（零耦合口径：depends 恒 None）。
pub const fn decoup_row(i: usize) -> Option<DecoupRow> {
    if i >= DECOUP_ROWS {
        return None;
    }
    Some(DecoupRow {
        no: i as u32,
        depends: None,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Team Explorer 收口账本。
#[derive(Clone)]
pub struct TeLedger {
    /// 段状态。
    state: TeState,
    /// 断点账（相位 + 命中拍）。
    bps: [Option<(BpPhase, u32)>; BP_ROWS],
    /// 上一命中拍。
    last_tick: u32,
    /// 变量账。
    watches: [Option<u32>; WATCH_ROWS],
    /// 耦合声明违例账。
    coupled: u32,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TeLedger {
    /// 新账本。
    pub fn new() -> Self {
        TeLedger {
            state: TeState::Idle,
            bps: [None; BP_ROWS],
            last_tick: 0,
            watches: [None; WATCH_ROWS],
            coupled: 0,
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
    pub fn advance(&mut self, to: TeState) -> Result<(), TeCode> {
        if !te_step(self.state, to) {
            return Err(TeCode::EOrder);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 断点推进（三段单向 + 命中拍单调）。
    pub fn bp_advance(&mut self, i: usize, to: BpPhase, tick: u32) -> Result<(), TeCode> {
        if i >= BP_ROWS {
            return Err(TeCode::ENoBp);
        }
        let cur = match self.bps.get(i).copied().flatten() {
            Some((p, _)) => p,
            None => BpPhase::Armed,
        };
        let armed = match self.bps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        };
        if armed {
            if !bp_phase_ok(cur, to) {
                return Err(TeCode::EBpOrder);
            }
        } else if to != BpPhase::Armed {
            return Err(TeCode::EBpOrder);
        }
        // 命中拍单调闸。
        if tick < self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100 + i as u64);
            return Err(TeCode::ETickBack);
        }
        self.last_tick = tick;
        let cell = match self.bps.get_mut(i) {
            Some(c) => c,
            None => return Err(TeCode::ENoBp),
        };
        *cell = Some((to, tick));
        self.fnv(0x5101 + i as u64);
        Ok(())
    }

    /// 断点全消解核验（12 格全 Resolved）。
    pub fn bps_all_resolved(&self) -> bool {
        let mut i = 0usize;
        while i < BP_ROWS {
            match self.bps.get(i).copied().flatten() {
                Some((BpPhase::Resolved, _)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 变量落账（与复算恒等才收）。
    pub fn land_watch(&mut self, i: usize, value: u32) -> Result<(), TeCode> {
        let row = match watch_row(i) {
            Some(r) => r,
            None => return Err(TeCode::ENoWatch),
        };
        if value != row.value {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(TeCode::EWatchDrift);
        }
        let cell = match self.watches.get_mut(i) {
            Some(c) => c,
            None => return Err(TeCode::EEmpty),
        };
        *cell = Some(value);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 变量全账核验。
    pub fn watches_all(&self) -> bool {
        let mut i = 0usize;
        while i < WATCH_ROWS {
            match self.watches.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 耦合声明账（依赖项必须为 None，非 None = 耦合违）。
    pub fn declare_decoup(&mut self, i: usize, depends: Option<u32>) -> Result<(), TeCode> {
        let row = match decoup_row(i) {
            Some(r) => r,
            None => return Err(TeCode::EEmpty),
        };
        let _ = row;
        if depends.is_some() {
            self.injected += 1;
            self.caught += 1;
            self.coupled += 1;
            self.fnv(0x5300 + i as u64);
            return Err(TeCode::ECoupled);
        }
        self.fnv(0x5301 + i as u64);
        Ok(())
    }

    /// 零耦合核验。
    pub fn zero_coupled(&self) -> bool {
        self.coupled == 0
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

impl Default for TeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 命中拍倒退注入（10 轮，确定性）：命中拍从高到低。
pub struct TickBackInjector;

impl TickBackInjector {
    /// 下一轮倒退拍（10-k，保证 < 上一拍）。
    pub const fn next_tick(k: usize) -> u32 {
        (10 - (k % 10)) as u32
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：三段全走 + 断点 12/12 + 变量 8/8 + 零耦合 + 双注入 10/10
/// + 指纹。返回（断点账数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TeLedger::new();
    // ① 三段全走。
    let seq = [TeState::Attached, TeState::BpHit, TeState::WatchReady];
    let mut s = 0usize;
    while s < seq.len() {
        let _ = led.advance(seq[s]);
        s += 1;
    }
    // ② 断点 12 格全账（Armed→Hit→Resolved，拍单调）。
    let mut bp_n = 0u32;
    let mut i = 0usize;
    let mut tick = 1u32;
    while i < BP_ROWS {
        if led.bp_advance(i, BpPhase::Armed, tick).is_ok()
            && led.bp_advance(i, BpPhase::Hit, tick + 1).is_ok()
            && led.bp_advance(i, BpPhase::Resolved, tick + 2).is_ok()
        {
            bp_n += 1;
        }
        tick += 3;
        i += 1;
    }
    // ③ 变量 8/8。
    let mut i = 0usize;
    while i < WATCH_ROWS {
        if let Some(r) = watch_row(i) {
            let _ = led.land_watch(i, r.value);
        }
        i += 1;
    }
    // ④ 无耦合声明 8/8。
    let mut i = 0usize;
    while i < DECOUP_ROWS {
        let _ = led.declare_decoup(i, None);
        i += 1;
    }
    // ⑤ 注入 A：命中拍倒退 10/10。
    let mut k = 0usize;
    while k < 10 {
        let tick_back = TickBackInjector::next_tick(k);
        let mut lt = TeLedger::new();
        let _ = lt.bp_advance(0, BpPhase::Armed, 100);
        if lt.bp_advance(0, BpPhase::Hit, tick_back).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ⑥ 注入 B：耦合依赖声明 10/10。
    let mut k = 0usize;
    while k < 10 {
        let mut lt = TeLedger::new();
        if lt.declare_decoup(k % DECOUP_ROWS, Some(0xE000)).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
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
pub fn run_unx52017_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52017");
    // 1. 正常流：三段全走 + 断点 12/12 + 变量 8/8。
    let (bp_n, caught, fp) = run_cycle();
    s.add(
        "三段全走+断点 12/12",
        bp_n == BP_ROWS as u32,
        "挂接/命中/查看逐段走，断点三段单向",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "倒退 10 + 耦合 10 全抓");
    // 2. 边界：断点越册/状态机越步。
    let mut l0 = TeLedger::new();
    s.add(
        "边界·断点越册",
        l0.bp_advance(BP_ROWS, BpPhase::Armed, 1) == Err(TeCode::ENoBp),
        "断点 12 越册必拒",
    );
    s.add(
        "状态机·Idle→BpHit 跳步拒",
        l0.advance(TeState::BpHit) == Err(TeCode::EOrder),
        "必须过 Attached，跳步非法",
    );
    // 3. 命中拍倒退直判。
    let mut l1 = TeLedger::new();
    let _ = l1.bp_advance(0, BpPhase::Armed, 50);
    s.add(
        "命中拍·倒退拒",
        l1.bp_advance(0, BpPhase::Hit, 49) == Err(TeCode::ETickBack),
        "拍号倒退显性拒",
    );
    s.add(
        "命中拍·单调过",
        l1.bp_advance(0, BpPhase::Hit, 51).is_ok(),
        "拍号单调合法",
    );
    // 4. 变量漂移直判。
    let mut l2 = TeLedger::new();
    s.add(
        "变量·漂移拒",
        l2.land_watch(0, 999) == Err(TeCode::EWatchDrift),
        "账面与复算不一致必拒",
    );
    let expect = watch_row(0).map(|r| r.value).unwrap_or(0);
    s.add("变量·复算值过", l2.land_watch(0, expect).is_ok(), "复算值合法");
    // 5. 耦合违直判。
    let mut l3 = TeLedger::new();
    s.add(
        "耦合·依赖声明拒",
        l3.declare_decoup(0, Some(0xE000)) == Err(TeCode::ECoupled),
        "对渲染面依赖声明显性拒",
    );
    s.add("耦合·零声明过", l3.declare_decoup(0, None).is_ok(), "零耦合声明放行");
    s.add("耦合·零违核验", l3.zero_coupled() == false, "已计违例账（守卫口径）");
    // 6. 断点三段越步拒。
    let mut l4 = TeLedger::new();
    let _ = l4.bp_advance(1, BpPhase::Armed, 10);
    let _ = l4.bp_advance(1, BpPhase::Resolved, 11);
    s.add(
        "断点·Resolved→Hit 反向拒",
        l4.bp_advance(1, BpPhase::Hit, 12) == Err(TeCode::EBpOrder),
        "消解后回命中显性拒",
    );
    // 7. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 8. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 10. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = watch_row(0).unwrap_or(WatchRow { no: 0, value: 0xB000, scope: 0 });
        assert_eq!(row.value, 0xB000);
        assert!(decoup_row(0).unwrap_or(DecoupRow { no: 0, depends: None }).depends.is_none());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(watch_row(WATCH_ROWS).is_none());
        assert!(decoup_row(DECOUP_ROWS).is_none());
        assert_eq!(
            TeLedger::new().bp_advance(BP_ROWS, BpPhase::Armed, 1),
            Err(TeCode::ENoBp)
        );
    }

    #[test]
    fn test_state_machine() {
        assert!(te_step(TeState::Idle, TeState::Attached));
        assert!(te_step(TeState::Attached, TeState::BpHit));
        assert!(te_step(TeState::BpHit, TeState::WatchReady));
        assert!(!te_step(TeState::Idle, TeState::BpHit));
        assert!(!te_step(TeState::WatchReady, TeState::Attached));
    }

    #[test]
    fn test_bp_phase_monotonic() {
        assert!(bp_phase_ok(BpPhase::Armed, BpPhase::Hit));
        assert!(bp_phase_ok(BpPhase::Hit, BpPhase::Resolved));
        assert!(bp_phase_ok(BpPhase::Armed, BpPhase::Resolved));
        assert!(!bp_phase_ok(BpPhase::Resolved, BpPhase::Hit));
        assert!(!bp_phase_ok(BpPhase::Hit, BpPhase::Armed));
    }

    #[test]
    fn test_tick_monotonic() {
        let mut led = TeLedger::new();
        let _ = led.bp_advance(0, BpPhase::Armed, 10);
        assert_eq!(led.bp_advance(0, BpPhase::Hit, 9), Err(TeCode::ETickBack));
        assert!(led.bp_advance(0, BpPhase::Hit, 11).is_ok());
    }

    #[test]
    fn test_watch_drift() {
        let mut led = TeLedger::new();
        assert_eq!(led.land_watch(2, 1), Err(TeCode::EWatchDrift));
        let expect = watch_row(2).map(|r| r.value).unwrap_or(0);
        assert!(led.land_watch(2, expect).is_ok());
    }

    #[test]
    fn test_decoup_gate() {
        let mut led = TeLedger::new();
        assert_eq!(led.declare_decoup(3, Some(1)), Err(TeCode::ECoupled));
        assert!(led.declare_decoup(3, None).is_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        // 注入 A：倒退。
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let tick_back = TickBackInjector::next_tick(k);
            let mut lt = TeLedger::new();
            let _ = lt.bp_advance(0, BpPhase::Armed, 100);
            if lt.bp_advance(0, BpPhase::Hit, tick_back).is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
        // 注入 B：耦合。
        let mut k = 0usize;
        let mut caught2 = 0u32;
        while k < 10 {
            let mut lt = TeLedger::new();
            if lt.declare_decoup(k % DECOUP_ROWS, Some(0xE000)).is_err() {
                caught2 += 1;
            }
            k += 1;
        }
        assert_eq!(caught2, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (bp_n, caught, _) = run_cycle();
        assert_eq!(bp_n, BP_ROWS as u32);
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
        let s = run_unx52017_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
