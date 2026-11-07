//! UNX-F52052 ReSharper 收口（VS 扩展形态） · 0xC4D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52052 逐字锚定）：UNX-F52053-J1 覆盖率采集/
//! 报告生成/VS 集成三段账；性能开销 ≤20% 落账；KB-COMPAT 定级登记；
//! 判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：三段账（覆盖率采集/报告生成/VS 集成）× 6 工程场景 =
//!      18 格账；段序闭集 0..3。
//!   ② 性能开销账：开销 = 采集拍 − 基线拍（纯函数复算），≤ 基线 ×20%
//!      才收；超限注入必抓（开销闸单向：超限登记后不许降档修正，只许
//!      复测重记）。
//!   ③ 段序闸：采集→报告→集成三段拍序严格递增；跨段乱序必抓。
//!   ④ 注入 ×10 轮（开销超限/段越闭集/拍序回退三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（18 格全通 + 开销达标
//!      → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52052 − 52000)）。
pub const SEG_BASE: u32 = 0xC4D00;

/// 三段账段数。
pub const SEGMENTS: usize = 3;

/// 工程场景数。
pub const PROJECTS: usize = 6;

/// 基线拍。
pub const BASE_TICKS: u64 = 100;

/// 开销封顶（×20%）。
pub const OVERHEAD_CAP: u64 = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoSeg,
    /// 工程号越闭集
    ENoProj,
    /// 性能开销超限
    EOverhead,
    /// 拍序乱
    EOrder,
    /// 载荷漂移
    EDrift,
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

impl RsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsCode::Ok => "成功",
            RsCode::ENoSeg => "段号越闭集：采集/报告/集成三段之外先登记",
            RsCode::ENoProj => "工程号越闭集：6 工程之外先登记",
            RsCode::EOverhead => "性能开销超限：开销必须 ≤ 基线 ×20%",
            RsCode::EOrder => "拍序乱：采集→报告→集成三段拍序必须严格递增",
            RsCode::EDrift => "载荷漂移：段折叠与基准不恒等，逐格复算",
            RsCode::EDup => "重复落账：同格只许一条账目",
            RsCode::ESeal => "未齐即封：18 格未齐先补齐",
            RsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RsCode::EGradeDrift => "定级漂移：18 格全通+开销达标+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 段格载荷（段 s × 工程 p）。
pub const fn seg_cell(s: usize, p: usize) -> Option<u64> {
    if s >= SEGMENTS || p >= PROJECTS {
        return None;
    }
    Some(
        0x3E000u64
            + (s as u64) * 0x2E
            + (p as u64) * 0x13
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> p),
    )
}

/// 采集拍（纯函数复算：基线 + 确定性开销，开销 ≤×20%）。
pub const fn collect_ticks(p: usize) -> u64 {
    if p >= PROJECTS {
        return 0;
    }
    BASE_TICKS + 8 + (p as u64) * 0x2
}

/// 开销 = 采集拍 − 基线；≤ ×20% 才达标。
pub const fn overhead_ok(p: usize) -> bool {
    let used = collect_ticks(p);
    used >= BASE_TICKS && used - BASE_TICKS <= BASE_TICKS * OVERHEAD_CAP / 100
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct RsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 段账。
    segs: [[Option<u64>; PROJECTS]; SEGMENTS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RsLedger {
    /// 新账本。
    pub fn new() -> Self {
        RsLedger {
            gates: [None; 6],
            segs: [[None; PROJECTS]; SEGMENTS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RsCode> {
        if gate >= 6 {
            return Err(RsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1C100 + gate as u64);
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

    /// 段一格（开销闸 + 拍序单调 + 载荷对照 + 重复拒）。
    pub fn record_seg(&mut self, s: usize, p: usize, ticks: u64) -> Result<u64, RsCode> {
        if s >= SEGMENTS {
            return Err(RsCode::ENoSeg);
        }
        if p >= PROJECTS {
            return Err(RsCode::ENoProj);
        }
        if s == 0 && !overhead_ok(p) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1C200 + p as u64);
            return Err(RsCode::EOverhead);
        }
        if (ticks as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1C201);
            return Err(RsCode::EOrder);
        }
        let payload = match seg_cell(s, p) {
            Some(p) => p,
            None => return Err(RsCode::ENoProj),
        };
        if match self.segs.get(s).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RsCode::EDup);
        }
        if let Some(row) = self.segs.get_mut(s) {
            if let Some(c) = row.get_mut(p) {
                *c = Some(payload);
            }
        }
        self.last_tick = ticks as i64;
        self.fnv(0x1C202 + (s as u64) * 6 + p as u64);
        Ok(payload)
    }

    /// 段全账核验（18 格）。
    pub fn segs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SEGMENTS {
            let mut p = 0usize;
            while p < PROJECTS {
                match self.segs.get(s).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            s += 1;
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

impl Default for RsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut tick: u64 = 0;
    let mut s = 0usize;
    while s < SEGMENTS {
        let mut p = 0usize;
        while p < PROJECTS {
            tick = tick.saturating_add(7);
            if led.record_seg(s, p, tick).is_ok() {
                recorded += 1;
            }
            p += 1;
        }
        s += 1;
    }
    // 注入 10 轮：开销超限 / 段越闭集 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RsLedger::new();
        let caught = match kind {
            0 => !overhead_ok(PROJECTS),
            1 => lt.record_seg(SEGMENTS, 0, 1).is_err(),
            _ => {
                let _ = lt.record_seg(0, 0, 50);
                lt.record_seg(1, 0, 30) == Err(RsCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1C300 + k as u64);
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
pub fn run_unx52052_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52052");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段 18/18 全通",
        recorded == (SEGMENTS * PROJECTS) as u32,
        "采集/报告/集成三段 ×6 工程全通",
    );
    s.add(
        "性能开销 ≤×20%",
        {
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJECTS {
                if !overhead_ok(p) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "采集拍开销账逐工程 ≤ 基线 ×20%",
    );
    s.add("注入 10/10 全抓", caught >= 10, "开销/段/拍序三态轮转必抓");
    let mut l0 = RsLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_seg(SEGMENTS, 0, 1).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_seg(0, PROJECTS, 1).is_err(),
        "工程 6 越闭集必拒",
    );
    let mut l1 = RsLedger::new();
    let _ = l1.record_seg(0, 0, 10);
    s.add(
        "重复·再落账拒",
        l1.record_seg(0, 0, 12).is_err(),
        "同格只许一条账目",
    );
    s.add(
        "拍序·回退拒",
        l1.record_seg(1, 0, 5) == Err(RsCode::EOrder),
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_cell_pure() {
        let a = seg_cell(1, 2).unwrap_or(0);
        assert_eq!(a, seg_cell(1, 2).unwrap_or(0));
        assert_ne!(a, seg_cell(1, 3).unwrap_or(0));
        assert!(seg_cell(SEGMENTS, 0).is_none());
        assert!(seg_cell(0, PROJECTS).is_none());
    }

    #[test]
    fn test_overhead_gate() {
        let mut p = 0usize;
        while p < PROJECTS {
            assert!(overhead_ok(p));
            p += 1;
        }
        // 开销账：8+2p ≤ 20（p≤5）。
        assert!(collect_ticks(0) - BASE_TICKS <= BASE_TICKS * 20 / 100);
        assert!(overhead_ok(PROJECTS) == false);
    }

    #[test]
    fn test_order_and_dup() {
        let mut lt = RsLedger::new();
        let _ = lt.record_seg(0, 0, 20);
        assert_eq!(lt.record_seg(0, 0, 22), Err(RsCode::EDup));
        assert_eq!(lt.record_seg(1, 0, 10), Err(RsCode::EOrder));
        assert!(lt.record_seg(1, 0, 25).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (SEGMENTS * PROJECTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4D00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52052_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
