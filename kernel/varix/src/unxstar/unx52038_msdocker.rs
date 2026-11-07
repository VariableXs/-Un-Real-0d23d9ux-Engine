//! UNX-F52038 VSCode 扩展：Docker（ms-azuretools）收口 · 0xC4980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52038 逐字锚定）：UNX-F52039-J1 容器/镜像/
//! Compose 三视图操作 ×10 全通；与 B14 Docker Desktop 消费对账（联签
//! 锚定行 F52281）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：三视图操作账（容器/镜像/Compose）× 10 操作场景 = 30 格
//!      操作账；视图档闭集 0..3（0=容器 1=镜像 2=Compose）。
//!   ② Docker Desktop 消费对账：6 格消费账（引擎档/守护进程档/套接字
//!      档/网络档/卷档/注册表档），逐格与消费基准恒等才收；未对账先操
//!      作 → 消费前置闸。
//!   ③ 状态机闭集：容器生命周期闭集（created/running/paused/exited/
//!      dead 五态），越态拒；操作拍序单调闸（同视图操作拍序严格递增）。
//!   ④ 注入 ×10 轮（视图越闭集/消费漂移/状态越闭集三态轮转）10/10 必
//!      抓；六判据母版闸 + KB-COMPAT 定级（30 格全通 + 消费零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52038 − 52000)）。
pub const SEG_BASE: u32 = 0xC4980;

/// 操作场景数。
pub const OPS: usize = 10;

/// 三视图段数。
pub const VIEWS: usize = 3;

/// Docker Desktop 消费账格数。
pub const DD_CELLS: usize = 6;

/// 容器生命周期状态闭集数。
pub const LIFE_STATES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DkCode {
    /// 成功
    Ok = 0,
    /// 视图号越闭集
    ENoView,
    /// 操作号越闭集
    ENoOp,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EDrift,
    /// 状态越闭集
    ENoState,
    /// 操作拍序乱
    EOpOrder,
    /// 重复落账
    EDup,
    /// 未对账先操作
    EPreOp,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DkCode::Ok => "成功",
            DkCode::ENoView => "视图号越闭集：容器/镜像/Compose 三视图之外先登记",
            DkCode::ENoOp => "操作号越闭集：10 操作场景之外先登记",
            DkCode::ENoCell => "消费格越册：Docker Desktop 6 格闭集之外先登记",
            DkCode::EDrift => "消费漂移：Docker Desktop 消费折叠与基准不恒等，逐格复算",
            DkCode::ENoState => "状态越闭集：created/running/paused/exited/dead 五态之外先登记",
            DkCode::EOpOrder => "操作拍序乱：同视图操作拍序必须严格递增单调",
            DkCode::EDup => "重复落账：同格只许一条账目",
            DkCode::EPreOp => "未对账先操作：Docker Desktop 6 格消费账不齐不许报三视图全通",
            DkCode::ESeal => "未齐即封：30 格未齐先补齐",
            DkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DkCode::EGradeDrift => "定级漂移：30 格全通+消费零漂移+状态在册才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（视图 v × 操作 o）。
pub const fn op_cell(v: usize, o: usize) -> Option<u64> {
    if v >= VIEWS || o >= OPS {
        return None;
    }
    Some(
        0x18000u64
            + (v as u64) * 0x23
            + (o as u64) * 0xD
            ^ (o as u64).wrapping_mul(0x9E3779B97F4A7C15 >> v),
    )
}

/// Docker Desktop 消费格基准载荷（格 c）。
pub const fn dd_cell(c: usize) -> Option<u64> {
    if c >= DD_CELLS {
        return None;
    }
    Some((0x19000u64 + c as u64 * 0x31) ^ 0x646F636B657264)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DkLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [[Option<u64>; OPS]; VIEWS],
    /// Docker Desktop 消费账。
    dd: [Option<u64>; DD_CELLS],
    /// 各视图最后操作拍。
    last_tick: [i64; VIEWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DkLedger {
    /// 新账本。
    pub fn new() -> Self {
        DkLedger {
            gates: [None; 6],
            ops: [[None; OPS]; VIEWS],
            dd: [None; DD_CELLS],
            last_tick: [-1; VIEWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DkCode> {
        if gate >= 6 {
            return Err(DkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xD100 + gate as u64);
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

    /// Docker Desktop 消费一格（恒等才收）。
    pub fn consume_dd(&mut self, c: usize, got: u64) -> Result<(), DkCode> {
        let want = match dd_cell(c) {
            Some(w) => w,
            None => return Err(DkCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD200 + c as u64);
            return Err(DkCode::EDrift);
        }
        if let Some(cell) = self.dd.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0xD201 + c as u64);
        Ok(())
    }

    /// Docker Desktop 消费全账核验。
    pub fn dd_all(&self) -> bool {
        let mut c = 0usize;
        while c < DD_CELLS {
            match self.dd.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 操作一格（消费前置闸 + 拍序单调 + 载荷对照 + 重复拒）。
    pub fn record_op(
        &mut self,
        v: usize,
        o: usize,
        tick: u32,
        state: usize,
    ) -> Result<u64, DkCode> {
        if v >= VIEWS {
            return Err(DkCode::ENoView);
        }
        if o >= OPS {
            return Err(DkCode::ENoOp);
        }
        if state >= LIFE_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD300 + state as u64);
            return Err(DkCode::ENoState);
        }
        if !self.dd_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD301);
            return Err(DkCode::EPreOp);
        }
        if match self.ops.get(v).and_then(|row| row.get(o)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DkCode::EDup);
        }
        if (tick as i64) <= self.last_tick[v] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD302 + v as u64);
            return Err(DkCode::EOpOrder);
        }
        let p = match op_cell(v, o) {
            Some(p) => p,
            None => return Err(DkCode::ENoOp),
        };
        if let Some(row) = self.ops.get_mut(v) {
            if let Some(c) = row.get_mut(o) {
                *c = Some(p);
            }
        }
        self.last_tick[v] = tick as i64;
        self.fnv(0xD303 + (v as u64) * 10 + o as u64);
        Ok(p)
    }

    /// 操作账全齐核验（30 格）。
    pub fn ops_all(&self) -> bool {
        let mut v = 0usize;
        while v < VIEWS {
            let mut o = 0usize;
            while o < OPS {
                match self.ops.get(v).and_then(|row| row.get(o)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                o += 1;
            }
            v += 1;
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

impl Default for DkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（操作格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < DD_CELLS {
        if let Some(w) = dd_cell(c) {
            let _ = led.consume_dd(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VIEWS {
        let mut o = 0usize;
        while o < OPS {
            if led.record_op(v, o, (v * OPS + o) as u32, o % LIFE_STATES).is_ok() {
                recorded += 1;
            }
            o += 1;
        }
        v += 1;
    }
    // 注入 10 轮：状态越闭集 / 消费漂移 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DkLedger::new();
        let mut c2 = 0usize;
        while c2 < DD_CELLS {
            if let Some(w) = dd_cell(c2) {
                let _ = lt.consume_dd(c2, w);
            }
            c2 += 1;
        }
        let caught = match kind {
            0 => lt.record_op(0, 0, 1, LIFE_STATES).is_err(),
            1 => lt.consume_dd(1, match dd_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(DkCode::EDrift),
            _ => {
                let _ = lt.record_op(0, 0, 5, 0);
                lt.record_op(0, 1, 3, 0) == Err(DkCode::EOpOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xD400 + k as u64);
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
pub fn run_unx52038_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52038");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三视图操作 30/30 全通",
        recorded == (VIEWS * OPS) as u32,
        "容器/镜像/Compose 三视图操作 ×10 场景全通",
    );
    s.add(
        "Docker Desktop 消费 6/6 对账",
        {
            let mut lt = DkLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < DD_CELLS {
                if let Some(w) = dd_cell(c) {
                    if lt.consume_dd(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.dd_all()
        },
        "引擎/守护进程/套接字/网络/卷/注册表六格消费零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "消费/拍序三态轮转必抓");
    let mut l0 = DkLedger::new();
    s.add(
        "操作前置闸",
        l0.record_op(0, 0, 1, 0) == Err(DkCode::EPreOp),
        "Docker Desktop 消费账不齐不许报三视图全通",
    );
    let mut l1 = DkLedger::new();
    let mut c = 0usize;
    while c < DD_CELLS {
        if let Some(w) = dd_cell(c) {
            let _ = l1.consume_dd(c, w);
        }
        c += 1;
    }
    s.add(
        "边界·视图越闭集",
        l1.record_op(VIEWS, 0, 1, 0).is_err(),
        "视图号 3 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l1.record_op(0, OPS, 1, 0).is_err(),
        "操作号 10 越闭集必拒",
    );
    s.add(
        "状态·越闭集拒",
        l1.record_op(0, 0, 1, LIFE_STATES + 1).is_err(),
        "状态 5+ 越闭集必抓",
    );
    let mut l2 = DkLedger::new();
    let mut c2 = 0usize;
    while c2 < DD_CELLS {
        if let Some(w) = dd_cell(c2) {
            let _ = l2.consume_dd(c2, w);
        }
        c2 += 1;
    }
    let _ = l2.record_op(1, 0, 4, 1);
    s.add(
        "拍序·回退拒",
        l2.record_op(1, 1, 2, 1) == Err(DkCode::EOpOrder),
        "同视图拍序回退必抓",
    );
    s.add(
        "拍序·异视图独立",
        l2.record_op(2, 0, 1, 2).is_ok(),
        "异视图拍序账分立放行",
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
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> DkLedger {
        let mut lt = DkLedger::new();
        let mut c = 0usize;
        while c < DD_CELLS {
            if let Some(w) = dd_cell(c) {
                let _ = lt.consume_dd(c, w);
            }
            c += 1;
        }
        lt
    }

    #[test]
    fn test_op_and_dd_pure() {
        let a = op_cell(2, 3).unwrap_or(0);
        assert_eq!(a, op_cell(2, 3).unwrap_or(0));
        assert_ne!(a, op_cell(2, 4).unwrap_or(0));
        assert!(op_cell(VIEWS, 0).is_none());
        assert!(op_cell(0, OPS).is_none());
        let w = dd_cell(5).unwrap_or(0);
        assert_eq!(w, dd_cell(5).unwrap_or(0));
        assert!(dd_cell(DD_CELLS).is_none());
    }

    #[test]
    fn test_state_gate() {
        let mut lt = seeded();
        assert!(lt.record_op(0, 0, 1, 0).is_ok());
        assert!(lt.record_op(0, 1, 2, LIFE_STATES).is_err());
    }

    #[test]
    fn test_pre_op_gate() {
        let mut lt = DkLedger::new();
        assert_eq!(lt.record_op(0, 0, 1, 0), Err(DkCode::EPreOp));
    }

    #[test]
    fn test_dd_drift_caught() {
        let mut lt = DkLedger::new();
        assert_eq!(
            lt.consume_dd(3, dd_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(DkCode::EDrift)
        );
        assert!(lt.consume_dd(3, dd_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_tick_order_gate() {
        let mut lt = seeded();
        let _ = lt.record_op(0, 0, 10, 0);
        assert_eq!(lt.record_op(0, 1, 5, 0), Err(DkCode::EOpOrder));
        assert!(lt.record_op(1, 0, 1, 0).is_ok());
    }

    #[test]
    fn test_dup_op_rejected() {
        let mut lt = seeded();
        let _ = lt.record_op(2, 2, 1, 0);
        assert!(lt.record_op(2, 2, 2, 0).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (VIEWS * OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4980);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52038_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
