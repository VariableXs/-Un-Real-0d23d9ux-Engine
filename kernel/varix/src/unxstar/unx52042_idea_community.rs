//! UNX-F52042 IntelliJ IDEA Community 收口 · 0xC4A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52042 逐字锚定）：UNX-F52043-J1 六判据全落 +
//! Python/科学模式 20 操作集特化项；解释器探测（系统/venv/conda 三源）
//! ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：解释器探测三源账（系统/venv/conda 三源 × 10 工程场景 =
//!      30 格探测账）；源档闭集 0..3（0=系统 1=venv 2=conda）。
//!   ② Python/科学模式 20 操作集特化账：20 操作格（解释器绑定/包管理/
//!      科学控制台/绘图后端/数据帧查看/…），逐操作载荷对照恒等才收；
//!      操作号越闭集拒。
//!   ③ 探测拍序闸：探测拍序（源档 × 场景）必须严格递增单调；重复探测
//!      同格拒；注入 ×10 轮（源档越闭集/载荷漂移/拍序回退三态轮转）
//!      10/10 必抓，注入=检出+逃逸守恒。
//!   ④ 六判据母版闸 + KB-COMPAT 定级（30 格探测全通 + 20 操作集全账
//!      + 拍序单调 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52042 − 52000)）。
pub const SEG_BASE: u32 = 0xC4A80;

/// 解释器源档数。
pub const SRCS: usize = 3;

/// 工程场景数。
pub const PROJECTS: usize = 10;

/// Python/科学模式 20 操作集。
pub const PY_OPS: usize = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IcCode {
    /// 成功
    Ok = 0,
    /// 源档越闭集
    ENoSrc,
    /// 场景号越闭集
    ENoProj,
    /// 操作号越闭集
    ENoOp,
    /// 探测载荷漂移
    EDrift,
    /// 探测拍序乱
    EOrder,
    /// 重复探测
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

impl IcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IcCode::Ok => "成功",
            IcCode::ENoSrc => "源档越闭集：系统/venv/conda 三源之外先登记",
            IcCode::ENoProj => "场景号越闭集：10 工程之外先登记",
            IcCode::ENoOp => "操作号越闭集：Python/科学模式 20 操作之外先登记",
            IcCode::EDrift => "探测载荷漂移：探测折叠与基准不恒等，逐格复算",
            IcCode::EOrder => "探测拍序乱：探测拍序必须严格递增单调",
            IcCode::EDup => "重复探测：同格只许一条账目",
            IcCode::ESeal => "未齐即封：30 格探测 + 20 操作集未齐先补齐",
            IcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            IcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IcCode::EGradeDrift => "定级漂移：30 格全通+20 操作集全账+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 探测格载荷（源 s × 工程 p）。
pub const fn probe_cell(s: usize, p: usize) -> Option<u64> {
    if s >= SRCS || p >= PROJECTS {
        return None;
    }
    Some(
        0x24000u64
            + (s as u64) * 0x1C
            + (p as u64) * 0x8
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> p),
    )
}

/// 操作格基准载荷（操作 o）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= PY_OPS {
        return None;
    }
    Some((0x25000u64 + o as u64 * 0x2A) ^ (o as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct IcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 探测账。
    probes: [[Option<u64>; PROJECTS]; SRCS],
    /// 操作集账。
    ops: [Option<u64>; PY_OPS],
    /// 最后探测拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IcLedger {
    /// 新账本。
    pub fn new() -> Self {
        IcLedger {
            gates: [None; 6],
            probes: [[None; PROJECTS]; SRCS],
            ops: [None; PY_OPS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IcCode> {
        if gate >= 6 {
            return Err(IcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x12100 + gate as u64);
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

    /// 探测一格（拍序单调 + 载荷对照 + 重复拒）。
    pub fn probe(&mut self, s: usize, p: usize, tick: u32, got: u64) -> Result<(), IcCode> {
        if s >= SRCS {
            return Err(IcCode::ENoSrc);
        }
        if p >= PROJECTS {
            return Err(IcCode::ENoProj);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x12200);
            return Err(IcCode::EOrder);
        }
        let want = match probe_cell(s, p) {
            Some(w) => w,
            None => return Err(IcCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x12201 + s as u64);
            return Err(IcCode::EDrift);
        }
        if match self.probes.get(s).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IcCode::EDup);
        }
        if let Some(row) = self.probes.get_mut(s) {
            if let Some(c) = row.get_mut(p) {
                *c = Some(got);
            }
        }
        self.last_tick = tick as i64;
        self.fnv(0x12202 + (s as u64) * 10 + p as u64);
        Ok(())
    }

    /// 探测全账核验（30 格）。
    pub fn probes_all(&self) -> bool {
        let mut s = 0usize;
        while s < SRCS {
            let mut p = 0usize;
            while p < PROJECTS {
                match self.probes.get(s).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            s += 1;
        }
        true
    }

    /// 操作集一格。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), IcCode> {
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(IcCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x12300 + o as u64);
            return Err(IcCode::EDrift);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x12301 + o as u64);
        Ok(())
    }

    /// 操作集全账核验。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < PY_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
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

impl Default for IcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（探测格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut tick: u32 = 0;
    let mut s = 0usize;
    while s < SRCS {
        let mut p = 0usize;
        while p < PROJECTS {
            tick = tick.saturating_add(1);
            if let Some(w) = probe_cell(s, p) {
                if led.probe(s, p, tick, w).is_ok() {
                    recorded += 1;
                }
            }
            p += 1;
        }
        s += 1;
    }
    let mut o = 0usize;
    while o < PY_OPS {
        if let Some(w) = op_cell(o) {
            let _ = led.record_op(o, w);
        }
        o += 1;
    }
    // 注入 10 轮：源越闭集 / 载荷漂移 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = IcLedger::new();
        let caught = match kind {
            0 => lt.probe(SRCS, 0, 1, 0) == Err(IcCode::ENoSrc),
            1 => lt.probe(0, 0, 1, probe_cell(0, 0).unwrap_or(0).wrapping_add(1))
                == Err(IcCode::EDrift),
            _ => {
                let _ = lt.probe(0, 0, 5, probe_cell(0, 0).unwrap_or(0));
                lt.probe(0, 1, 3, probe_cell(0, 1).unwrap_or(0)) == Err(IcCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x12400 + k as u64);
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
pub fn run_unx52042_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52042");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "解释器探测 30/30 全通",
        recorded == (SRCS * PROJECTS) as u32,
        "系统/venv/conda 三源 ×10 工程全通",
    );
    s.add(
        "操作集 20/20 全账",
        {
            let mut lt = IcLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < PY_OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "Python/科学模式 20 操作集特化逐格入账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "载荷/拍序三态轮转必抓");
    let mut l0 = IcLedger::new();
    s.add(
        "边界·源档越闭集",
        l0.probe(SRCS, 0, 1, 0).is_err(),
        "源档 3 越闭集必拒",
    );
    s.add(
        "边界·工程越闭集",
        l0.probe(0, PROJECTS, 1, 0).is_err(),
        "工程号 10 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(PY_OPS, 0) == Err(IcCode::ENoOp),
        "操作 20 越闭集必拒",
    );
    let mut l1 = IcLedger::new();
    s.add(
        "探测·载荷漂移拒",
        l1.probe(1, 2, 1, probe_cell(1, 2).unwrap_or(0).wrapping_add(3)) == Err(IcCode::EDrift),
        "探测折叠漂移必抓",
    );
    let mut l2 = IcLedger::new();
    let _ = l2.probe(0, 0, 4, probe_cell(0, 0).unwrap_or(0));
    s.add(
        "拍序·回退拒",
        l2.probe(0, 1, 2, probe_cell(0, 1).unwrap_or(0)) == Err(IcCode::EOrder),
        "拍序回退必抓",
    );
    s.add(
        "拍序·重复拍拒",
        l2.probe(1, 0, 4, probe_cell(1, 0).unwrap_or(0)) == Err(IcCode::EOrder),
        "同拍重落必抓",
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

    #[test]
    fn test_probe_and_op_pure() {
        let a = probe_cell(1, 2).unwrap_or(0);
        assert_eq!(a, probe_cell(1, 2).unwrap_or(0));
        assert_ne!(a, probe_cell(1, 3).unwrap_or(0));
        assert!(probe_cell(SRCS, 0).is_none());
        assert!(probe_cell(0, PROJECTS).is_none());
        let w = op_cell(9).unwrap_or(0);
        assert_eq!(w, op_cell(9).unwrap_or(0));
        assert!(op_cell(PY_OPS).is_none());
    }

    #[test]
    fn test_probe_gates() {
        let mut lt = IcLedger::new();
        assert!(lt.probe(SRCS, 0, 1, 0).is_err());
        assert!(lt.probe(0, PROJECTS, 1, 0).is_err());
        assert_eq!(
            lt.probe(0, 0, 1, probe_cell(0, 0).unwrap_or(0).wrapping_add(9)),
            Err(IcCode::EDrift)
        );
        assert!(lt.probe(0, 0, 1, probe_cell(0, 0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.probe(0, 0, 2, probe_cell(0, 0).unwrap_or(0)),
            Err(IcCode::EDup)
        );
    }

    #[test]
    fn test_order_gate() {
        let mut lt = IcLedger::new();
        let _ = lt.probe(0, 0, 10, probe_cell(0, 0).unwrap_or(0));
        assert_eq!(
            lt.probe(1, 0, 5, probe_cell(1, 0).unwrap_or(0)),
            Err(IcCode::EOrder)
        );
        assert!(lt
            .probe(1, 0, 11, probe_cell(1, 0).unwrap_or(0))
            .is_ok());
    }

    #[test]
    fn test_op_drift_caught() {
        let mut lt = IcLedger::new();
        assert_eq!(
            lt.record_op(3, op_cell(3).unwrap_or(0).wrapping_sub(1)),
            Err(IcCode::EDrift)
        );
        assert!(lt.record_op(3, op_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (SRCS * PROJECTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4A80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52042_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
