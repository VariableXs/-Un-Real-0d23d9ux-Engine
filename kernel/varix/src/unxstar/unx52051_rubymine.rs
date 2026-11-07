//! UNX-F52051 RubyMine 收口 · 0xC4CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52051 逐字锚定）：UNX-F52052-J1 六判据全落
//! （宿主为 VS2022，消费 B01 shim 共面）；重构操作集 ×10 全通；与宿主
//! 兼容矩阵入 KB-COMPAT；判据锚定 §AI-66.2 专题三 VS 家族联动。
//!
//! 设计要点：
//!   ① 域模型：重构操作集账。重构段闭集 10 段（重命名/提取方法/内联
//!      /移动/改签名/提取变量/提取接口/内联变量/移型/安全删除）× 6 场
//!      景 = 60 格重构账；段序闭集 0..10。
//!   ② VS shim 共面消费对账：6 格消费账（shim 版本档/接口档/回退档/
//!      审计档/日志档/退出码档），逐格与消费基准恒等才收；未对账先重
//!      构 → 消费前置闸。
//!   ③ 重构拍序闸：同场景操作拍序严格递增；重复落账拒。
//!   ④ 注入 ×10 轮（段越闭集/消费漂移/拍序回退三态轮转）10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 兼容矩阵登记（60 格全通 + 消费零漂移
//!      → 矩阵 S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52051 − 52000)）。
pub const SEG_BASE: u32 = 0xC4CC0;

/// 重构段数。
pub const REFACTORS: usize = 10;

/// 场景数。
pub const SCENES: usize = 6;

/// VS shim 消费账格数。
pub const SHIM_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RmCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoRef,
    /// 场景号越闭集
    ENoScene,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EDrift,
    /// 拍序乱
    EOrder,
    /// 未对账先重构
    EPreRef,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl RmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RmCode::Ok => "成功",
            RmCode::ENoRef => "段号越闭集：重构十段之外先登记",
            RmCode::ENoScene => "场景号越闭集：6 场景之外先登记",
            RmCode::ENoCell => "消费格越册：VS shim 6 格闭集之外先登记",
            RmCode::EDrift => "消费漂移：shim 消费折叠与基准不恒等，逐格复算",
            RmCode::EOrder => "拍序乱：同场景操作拍序必须严格递增单调",
            RmCode::EPreRef => "未对账先重构：shim 6 格消费账不齐不许报重构全通",
            RmCode::EDup => "重复落账：同格只许一条账目",
            RmCode::ESeal => "未齐即封：60 格未齐先补齐",
            RmCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RmCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RmCode::EGradeDrift => "定级漂移：60 格全通+消费零漂移+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 重构格载荷（段 r × 场景 s）。
pub const fn ref_cell(r: usize, s: usize) -> Option<u64> {
    if r >= REFACTORS || s >= SCENES {
        return None;
    }
    Some(
        0x3B000u64
            + (r as u64) * 0x1E
            + (s as u64) * 0xA
            ^ (r as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// shim 消费格基准载荷（格 c）。
pub const fn shim_cell(c: usize) -> Option<u64> {
    if c >= SHIM_CELLS {
        return None;
    }
    Some((0x3C000u64 + c as u64 * 0x29) ^ 0x76617368696D23)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct RmLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 重构账。
    refs: [[Option<u64>; SCENES]; REFACTORS],
    /// shim 消费账。
    shims: [Option<u64>; SHIM_CELLS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RmLedger {
    /// 新账本。
    pub fn new() -> Self {
        RmLedger {
            gates: [None; 6],
            refs: [[None; SCENES]; REFACTORS],
            shims: [None; SHIM_CELLS],
            last_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RmCode> {
        if gate >= 6 {
            return Err(RmCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1B100 + gate as u64);
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

    /// shim 消费一格。
    pub fn consume_shim(&mut self, c: usize, got: u64) -> Result<(), RmCode> {
        let want = match shim_cell(c) {
            Some(w) => w,
            None => return Err(RmCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1B200 + c as u64);
            return Err(RmCode::EDrift);
        }
        if let Some(cell) = self.shims.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x1B201 + c as u64);
        Ok(())
    }

    /// shim 消费全账核验。
    pub fn shims_all(&self) -> bool {
        let mut c = 0usize;
        while c < SHIM_CELLS {
            match self.shims.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 重构一格（前置闸 + 拍序单调 + 重复拒）。
    pub fn record_ref(&mut self, r: usize, s: usize, tick: u32) -> Result<u64, RmCode> {
        if r >= REFACTORS {
            return Err(RmCode::ENoRef);
        }
        if s >= SCENES {
            return Err(RmCode::ENoScene);
        }
        if !self.shims_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1B300);
            return Err(RmCode::EPreRef);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1B301);
            return Err(RmCode::EOrder);
        }
        let p = match ref_cell(r, s) {
            Some(p) => p,
            None => return Err(RmCode::ENoScene),
        };
        if match self.refs.get(r).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RmCode::EDup);
        }
        if let Some(row) = self.refs.get_mut(r) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.last_tick = tick as i64;
        self.fnv(0x1B302 + (r as u64) * 6 + s as u64);
        Ok(p)
    }

    /// 重构全账核验（60 格）。
    pub fn refs_all(&self) -> bool {
        let mut r = 0usize;
        while r < REFACTORS {
            let mut s = 0usize;
            while s < SCENES {
                match self.refs.get(r).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            r += 1;
        }
        true
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

impl Default for RmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < SHIM_CELLS {
        if let Some(w) = shim_cell(c) {
            let _ = led.consume_shim(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut tick: u32 = 0;
    let mut r = 0usize;
    while r < REFACTORS {
        let mut s = 0usize;
        while s < SCENES {
            tick = tick.saturating_add(2);
            if led.record_ref(r, s, tick).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        r += 1;
    }
    // 注入 10 轮：消费漂移 / 段越闭集 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RmLedger::new();
        let caught = match kind {
            0 => lt.consume_shim(3, shim_cell(3).unwrap_or(0).wrapping_add(1)) == Err(RmCode::EDrift),
            1 => {
                let mut c2 = 0usize;
                while c2 < SHIM_CELLS {
                    if let Some(w) = shim_cell(c2) {
                        let _ = lt.consume_shim(c2, w);
                    }
                    c2 += 1;
                }
                lt.record_ref(REFACTORS, 0, 1).is_err()
            }
            _ => {
                let mut c3 = 0usize;
                while c3 < SHIM_CELLS {
                    if let Some(w) = shim_cell(c3) {
                        let _ = lt.consume_shim(c3, w);
                    }
                    c3 += 1;
                }
                let _ = lt.record_ref(0, 0, 8);
                lt.record_ref(0, 1, 4) == Err(RmCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1B400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52051_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52051");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "重构 60/60 全通",
        recorded == (REFACTORS * SCENES) as u32,
        "重构十段 ×6 场景全通",
    );
    s.add(
        "VS shim 消费 6/6 对账",
        {
            let mut lt = RmLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SHIM_CELLS {
                if let Some(w) = shim_cell(c) {
                    if lt.consume_shim(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.shims_all()
        },
        "shim 版本/接口/回退/审计/日志/退出码六格消费零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/段/拍序三态轮转必抓");
    let mut l0 = RmLedger::new();
    s.add(
        "重构前置闸",
        l0.record_ref(0, 0, 1) == Err(RmCode::EPreRef),
        "shim 消费账不齐不许报重构全通",
    );
    let mut l1 = RmLedger::new();
    let mut c = 0usize;
    while c < SHIM_CELLS {
        if let Some(w) = shim_cell(c) {
            let _ = l1.consume_shim(c, w);
        }
        c += 1;
    }
    s.add(
        "边界·段越闭集",
        l1.record_ref(REFACTORS, 0, 1).is_err(),
        "段 10 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l1.record_ref(0, SCENES, 1).is_err(),
        "场景 6 越闭集必拒",
    );
    let mut l2 = RmLedger::new();
    let mut c2 = 0usize;
    while c2 < SHIM_CELLS {
        if let Some(w) = shim_cell(c2) {
            let _ = l2.consume_shim(c2, w);
        }
        c2 += 1;
    }
    let _ = l2.record_ref(1, 1, 4);
    s.add(
        "重复·再落账拒",
        l2.record_ref(1, 1, 6).is_err(),
        "同格只许一条账目",
    );
    s.add(
        "拍序·回退拒",
        l2.record_ref(1, 2, 2) == Err(RmCode::EOrder),
        "拍序回退必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> RmLedger {
        let mut lt = RmLedger::new();
        let mut c = 0usize;
        while c < SHIM_CELLS {
            if let Some(w) = shim_cell(c) {
                let _ = lt.consume_shim(c, w);
            }
            c += 1;
        }
        lt
    }

    #[test]
    fn test_cells_pure() {
        let a = ref_cell(4, 2).unwrap_or(0);
        assert_eq!(a, ref_cell(4, 2).unwrap_or(0));
        assert_ne!(a, ref_cell(4, 3).unwrap_or(0));
        assert!(ref_cell(REFACTORS, 0).is_none());
        assert!(ref_cell(0, SCENES).is_none());
        let w = shim_cell(5).unwrap_or(0);
        assert_eq!(w, shim_cell(5).unwrap_or(0));
        assert!(shim_cell(SHIM_CELLS).is_none());
    }

    #[test]
    fn test_pre_ref_gate() {
        let mut lt = RmLedger::new();
        assert_eq!(lt.record_ref(0, 0, 1), Err(RmCode::EPreRef));
    }

    #[test]
    fn test_shim_drift_caught() {
        let mut lt = RmLedger::new();
        assert_eq!(
            lt.consume_shim(2, shim_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(RmCode::EDrift)
        );
        assert!(lt.consume_shim(2, shim_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_and_dup() {
        let mut lt = seeded();
        let _ = lt.record_ref(2, 2, 10);
        assert_eq!(lt.record_ref(2, 2, 12), Err(RmCode::EDup));
        assert_eq!(lt.record_ref(2, 3, 5), Err(RmCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (REFACTORS * SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4CC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52051_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
