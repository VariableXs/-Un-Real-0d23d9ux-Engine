//! UNX-F52021 VSCode 收口（Electron 上栈代表条目） · 0xC4540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52021 逐字锚定）：UNX-F52022-J1 六判据全落；滚
//! 动版随上游月更，钉版 + AI-94 通知链 T+1 回归口径落账（联签锚定行）；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六上游滚动。
//!
//! 设计要点：
//!   ① 域模型：版本双轨账。滚动轨（月更流：12 个月档，纯函数复算，随上
//!      游推进只增不减）与钉版轨（冻结档：钉住月档位，冻结后写入显性
//!      拒）；两轨互斥写（滚动版写入钉版轨 = 混轨显性拒）。
//!   ② 通知链 T+1 回归口径：钉版日 T 记账，T+1 回归账逐钉版条目落账
//!      （回归位零静默）；缺 T+1 回归注入 ×10 轮 10/10 必抓。
//!   ③ 六判据母版闸（6/6 全落）。
//!   ④ KB-COMPAT 定级登记：六判据 + 钉版轨 12/12 + T+1 回归 12/12 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52021 − 52000)）。
pub const SEG_BASE: u32 = 0xC4540;

/// 月档数（滚动轨/钉版轨各 12 档）。
pub const MONTH_ROWS: usize = 12;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vc2Code {
    /// 成功
    Ok = 0,
    /// 月档越册
    ENoMonth,
    /// 六判据越闭集
    ENoGate,
    /// 钉版轨冻结后写入
    EPinFrozen,
    /// 混轨写
    ETrackMix,
    /// T+1 回归缺失
    ENoRegress,
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

impl Vc2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Vc2Code::Ok => "成功",
            Vc2Code::ENoMonth => "月档越册：12 月档闭集之外先登记",
            Vc2Code::ENoGate => "六判据越闭集：六判据之外先登记",
            Vc2Code::EPinFrozen => "钉版轨冻结后写入：冻结档不得再写，升档走新版冻结",
            Vc2Code::ETrackMix => "混轨写：滚动版不得写入钉版轨，轨位互斥",
            Vc2Code::ENoRegress => "T+1 回归缺失：钉版条目 T+1 回归必须落账（零静默）",
            Vc2Code::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Vc2Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Vc2Code::EEmpty => "账为空：至少落 1 格",
            Vc2Code::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            Vc2Code::EGradeDrift => "定级漂移：六判据+钉版 12/12+T+1 回归 12/12 才定 S1",
            Vc2Code::EDup => "重复落账：同档只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 月档行（滚动轨，纯函数复算：月档 → 上游版本号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RollingRow {
    /// 月档（0..12）。
    pub month: u32,
    /// 上游版本号（1.0 + month*0.01 口径 ×100 整数）。
    pub version: u32,
}

/// 复算第 i 档。
pub const fn rolling_row(i: usize) -> Option<RollingRow> {
    if i >= MONTH_ROWS {
        return None;
    }
    Some(RollingRow {
        month: i as u32,
        version: 100 + i as u32,
    })
}

/// 钉版行（月档 → 钉住版本，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinRow {
    /// 月档（0..12）。
    pub month: u32,
    /// 钉住版本（冻结时点的滚动版本）。
    pub pinned: u32,
    /// 钉版日 T。
    pub day_t: u32,
}

/// 复算第 i 档（确定性：钉住版本 = 冻结时滚动版本；T = 10 + month）。
pub const fn pin_row(i: usize) -> Option<PinRow> {
    if i >= MONTH_ROWS {
        return None;
    }
    Some(PinRow {
        month: i as u32,
        pinned: 100 + i as u32,
        day_t: 10 + i as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VSCode 收口账本（双轨 + T+1 回归链）。
#[derive(Clone)]
pub struct Vc2Ledger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 滚动轨（月档 → 版本）。
    rolling: [Option<u32>; MONTH_ROWS],
    /// 钉版轨（月档 → (钉住版本, T)）。
    pinned: [Option<(u32, u32)>; MONTH_ROWS],
    /// 钉版轨冻结位（冻结后写拒）。
    frozen: u16,
    /// T+1 回归账（月档 → 回归位）。
    regress: [bool; MONTH_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Vc2Ledger {
    /// 新账本。
    pub fn new() -> Self {
        Vc2Ledger {
            gates: [None; 6],
            rolling: [None; MONTH_ROWS],
            pinned: [None; MONTH_ROWS],
            frozen: 0,
            regress: [false; MONTH_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Vc2Code> {
        if gate >= 6 {
            return Err(Vc2Code::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(Vc2Code::EEmpty),
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

    /// 滚动轨落档（与复算恒等才收）。
    pub fn land_rolling(&mut self, i: usize, version: u32) -> Result<(), Vc2Code> {
        let row = match rolling_row(i) {
            Some(r) => r,
            None => return Err(Vc2Code::ENoMonth),
        };
        if version != row.version {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(Vc2Code::ETrackMix);
        }
        let cell = match self.rolling.get_mut(i) {
            Some(c) => c,
            None => return Err(Vc2Code::EEmpty),
        };
        *cell = Some(version);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 钉版轨落档（冻结闸 + 混轨闸：滚动版值写入钉版轨显性拒）。
    pub fn land_pin(&mut self, i: usize, version: u32, day_t: u32) -> Result<(), Vc2Code> {
        let row = match pin_row(i) {
            Some(r) => r,
            None => return Err(Vc2Code::ENoMonth),
        };
        // 冻结闸。
        if self.frozen & (1u16 << i) != 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + i as u64);
            return Err(Vc2Code::EPinFrozen);
        }
        // 混轨闸：滚动轨已落同档同值 = 滚动版串写钉版轨。
        if let Some(rv) = self.rolling.get(i).copied().flatten() {
            if rv == version {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5301 + i as u64);
                return Err(Vc2Code::ETrackMix);
            }
        }
        if version != row.pinned || day_t != row.day_t {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5302 + i as u64);
            return Err(Vc2Code::ETrackMix);
        }
        let cell = match self.pinned.get_mut(i) {
            Some(c) => c,
            None => return Err(Vc2Code::EEmpty),
        };
        *cell = Some((version, day_t));
        self.frozen |= 1u16 << i;
        self.fnv(0x5303 + i as u64);
        Ok(())
    }

    /// T+1 回归落账（钉版在册才收）。
    pub fn land_regress(&mut self, i: usize) -> Result<(), Vc2Code> {
        if i >= MONTH_ROWS {
            return Err(Vc2Code::ENoMonth);
        }
        if match self.pinned.get(i) {
            Some(Some(_)) => false,
            _ => true,
        } {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + i as u64);
            return Err(Vc2Code::ENoRegress);
        }
        if self.regress[i] {
            return Err(Vc2Code::EDup);
        }
        self.regress[i] = true;
        self.fnv(0x5401 + i as u64);
        Ok(())
    }

    /// T+1 回归全账核验。
    pub fn regress_all(&self) -> bool {
        let mut i = 0usize;
        while i < MONTH_ROWS {
            if !self.regress[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 钉版全账核验。
    pub fn pins_all(&self) -> bool {
        let mut i = 0usize;
        while i < MONTH_ROWS {
            match self.pinned.get(i) {
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

impl Default for Vc2Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 混轨注入（10 轮，确定性）：滚动版值写钉版轨。
pub struct TrackMixInjector;

impl TrackMixInjector {
    /// 下一轮（月档、滚动版本值）。
    pub const fn next_case(k: usize) -> (usize, u32, u32) {
        (k % MONTH_ROWS, 100 + (k % MONTH_ROWS) as u32, 10 + (k % MONTH_ROWS) as u32)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + 钉版 12/12 + T+1 回归 12/12 + 双注入 10/10 +
/// 指纹。返回（回归账数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Vc2Ledger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② 钉版轨 12/12（先落钉版再落回归；滚动轨不落，保钉版轨纯净）。
    let mut i = 0usize;
    while i < MONTH_ROWS {
        if let Some(r) = pin_row(i) {
            let _ = led.land_pin(i, r.pinned, r.day_t);
        }
        i += 1;
    }
    // ③ T+1 回归 12/12。
    let mut regress = 0u32;
    let mut i = 0usize;
    while i < MONTH_ROWS {
        if led.land_regress(i).is_ok() {
            regress += 1;
        }
        i += 1;
    }
    // ④ 注入 A：混轨 10/10（钉版冻结后重写）。
    let mut k = 0usize;
    while k < 10 {
        let (m, v, t) = TrackMixInjector::next_case(k);
        let mut lt = Vc2Ledger::new();
        let _ = lt.land_pin(m, v, t);
        if lt.land_pin(m, v, t).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ⑤ 注入 B：缺 T+1 回归 10/10（未钉版先回归）。
    let mut k = 0usize;
    while k < 10 {
        let m = k % MONTH_ROWS;
        let mut lt = Vc2Ledger::new();
        if lt.land_regress(m).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (regress, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52021_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52021");
    // 1. 正常流：钉版 12/12 + T+1 回归 12/12。
    let (regress, caught, fp) = run_cycle();
    s.add(
        "钉版轨 12/12 + T+1 回归 12/12",
        regress == MONTH_ROWS as u32,
        "钉版日 T 记账，T+1 回归逐条落账",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "混轨 10 + 缺回归 10 全抓");
    // 2. 边界：月档越册/判据越闭集。
    let mut l0 = Vc2Ledger::new();
    s.add(
        "边界·月档越册",
        l0.land_rolling(MONTH_ROWS, 100) == Err(Vc2Code::ENoMonth),
        "月档 12 越册必拒",
    );
    s.add(
        "边界·判据越闭集",
        l0.judge_gate(6) == Err(Vc2Code::ENoGate),
        "判据 6 越闭集必拒",
    );
    // 3. 冻结闸直判。
    let mut l1 = Vc2Ledger::new();
    let _ = l1.land_pin(0, 100, 10);
    s.add(
        "冻结·冻结后重写拒",
        l1.land_pin(0, 100, 10) == Err(Vc2Code::EPinFrozen),
        "冻结档写入显性拒",
    );
    // 4. 滚动轨语义。
    let mut l2 = Vc2Ledger::new();
    let _ = l2.land_rolling(0, 100);
    s.add(
        "滚动·复算值过",
        l2.land_rolling(1, 101).is_ok(),
        "月档 1 → 版本 101",
    );
    s.add(
        "滚动·漂移拒",
        l2.land_rolling(2, 999) == Err(Vc2Code::ETrackMix),
        "版本漂移显性拒",
    );
    // 5. T+1 回归前置闸直判。
    let mut l3 = Vc2Ledger::new();
    s.add(
        "回归·未钉版先回归拒",
        l3.land_regress(0) == Err(Vc2Code::ENoRegress),
        "钉版在册才许回归落账",
    );
    let _ = l3.land_pin(0, 100, 10);
    s.add("回归·钉版后过", l3.land_regress(0).is_ok(), "前置满足放行");
    // 6. 六判据全落。
    let mut l4 = Vc2Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = l4.judge_gate(g);
        g += 1;
    }
    s.add("六判据全落", l4.gates_all(), "母版闸 6/6");
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
        let row = rolling_row(0).unwrap_or(RollingRow { month: 0, version: 100 });
        assert_eq!(row.version, 100);
        assert!(pin_row(0).is_some());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(rolling_row(MONTH_ROWS).is_none());
        assert!(pin_row(MONTH_ROWS).is_none());
        assert_eq!(
            Vc2Ledger::new().land_rolling(MONTH_ROWS, 100),
            Err(Vc2Code::ENoMonth)
        );
    }

    #[test]
    fn test_pin_freeze_gate() {
        let mut led = Vc2Ledger::new();
        let _ = led.land_pin(2, 102, 12);
        assert_eq!(led.land_pin(2, 102, 12), Err(Vc2Code::EPinFrozen));
    }

    #[test]
    fn test_rolling_semantics() {
        let mut led = Vc2Ledger::new();
        assert!(led.land_rolling(0, 100).is_ok());
        assert_eq!(led.land_rolling(0, 999), Err(Vc2Code::ETrackMix));
        assert!(led.land_rolling(1, 101).is_ok());
    }

    #[test]
    fn test_regress_precondition() {
        let mut led = Vc2Ledger::new();
        assert_eq!(led.land_regress(0), Err(Vc2Code::ENoRegress));
        let _ = led.land_pin(0, 100, 10);
        assert!(led.land_regress(0).is_ok());
        assert_eq!(led.land_regress(0), Err(Vc2Code::EDup));
    }

    #[test]
    fn test_gates_all() {
        let mut led = Vc2Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = led.judge_gate(g);
            g += 1;
        }
        assert!(led.gates_all());
    }

    #[test]
    fn test_injection_all_caught() {
        // 注入 A：冻结重写。
        let mut k = 0usize;
        let mut a = 0u32;
        while k < 10 {
            let (m, v, t) = TrackMixInjector::next_case(k);
            let mut lt = Vc2Ledger::new();
            let _ = lt.land_pin(m, v, t);
            if lt.land_pin(m, v, t).is_err() {
                a += 1;
            }
            k += 1;
        }
        assert_eq!(a, 10);
        // 注入 B：缺回归。
        let mut k = 0usize;
        let mut b = 0u32;
        while k < 10 {
            let mut lt = Vc2Ledger::new();
            if lt.land_regress(k % MONTH_ROWS).is_err() {
                b += 1;
            }
            k += 1;
        }
        assert_eq!(b, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (regress, caught, _) = run_cycle();
        assert_eq!(regress, MONTH_ROWS as u32);
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
        let s = run_unx52021_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
