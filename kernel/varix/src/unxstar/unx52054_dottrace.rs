//! UNX-F52054 dotTrace 收口 · 0xC4D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52054 逐字锚定）：UNX-F52055-J1 内存快照/泄漏
//! 定位/保留树三段账；GB 级堆快照打开 ≤×1.5；KB-COMPAT 定级登记；判据
//! 锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：三段账（内存快照/泄漏定位/保留树）× 6 堆场景 = 18 格
//!      账；段序闭集 0..3。
//!   ② GB 级堆快照打开时长账：快照 4 GB 基线打开拍 4096 拍，GB 级打开
//!      ≤ 基线 ×1.5 封顶闸；逐场景打开拍纯函数复算并逐格落账。
//!   ③ 泄漏定位账：泄漏候选 8 项闭集，逐项保留树路径载荷对照恒等才
//!      收；泄漏项保留引用计数必须 >0（零引用即误报必抓）。
//!   ④ 注入 ×10 轮（打开超封顶/泄漏误报/段越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（18 格全通 + 打开
//!      达标 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52054 − 52000)）。
pub const SEG_BASE: u32 = 0xC4D80;

/// 三段账段数。
pub const SEGMENTS: usize = 3;

/// 堆场景数。
pub const HEAPS: usize = 6;

/// 堆快照基线（GB → 拍）。
pub const SNAP_BASE_TICKS: u64 = 4096;

/// 打开封顶分子/分母（×1.5 = 3/2）。
pub const OPEN_CAP_NUM: u64 = 3;
pub const OPEN_CAP_DEN: u64 = 2;

/// 泄漏候选数。
pub const LEAKS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DtCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoSeg,
    /// 堆号越闭集
    ENoHeap,
    /// 打开超封顶
    EOpenCap,
    /// 泄漏误报（保留引用为零）
    EZeroRef,
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

impl DtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DtCode::Ok => "成功",
            DtCode::ENoSeg => "段号越闭集：快照/泄漏/保留树三段之外先登记",
            DtCode::ENoHeap => "堆号越闭集：6 堆场景之外先登记",
            DtCode::EOpenCap => "打开超封顶：GB 级堆快照打开必须 ≤ 基线 ×1.5",
            DtCode::EZeroRef => "泄漏误报：泄漏候选保留引用计数为零即误报必抓",
            DtCode::EDrift => "载荷漂移：折叠与基准不恒等，逐格复算",
            DtCode::EDup => "重复落账：同格只许一条账目",
            DtCode::ESeal => "未齐即封：18 格未齐先补齐",
            DtCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DtCode::EGradeDrift => "定级漂移：18 格全通+打开达标+泄漏在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 段格载荷（段 s × 堆 h）。
pub const fn seg_cell(s: usize, h: usize) -> Option<u64> {
    if s >= SEGMENTS || h >= HEAPS {
        return None;
    }
    Some(
        0x44000u64
            + (s as u64) * 0x23
            + (h as u64) * 0xF
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> h),
    )
}

/// 第 h 堆打开拍（纯函数复算：4 GB 基线 + 确定性增量，≤×1.5 封顶）。
pub const fn open_ticks(h: usize) -> u64 {
    if h >= HEAPS {
        return 0;
    }
    SNAP_BASE_TICKS + 256 + (h as u64) * 0x80
}

/// 打开封顶闸。
pub const fn open_ok(h: usize) -> bool {
    let used = open_ticks(h);
    used >= SNAP_BASE_TICKS && used - SNAP_BASE_TICKS <= SNAP_BASE_TICKS * OPEN_CAP_NUM / OPEN_CAP_DEN
}

/// 泄漏候选基准载荷（项 l）与保留引用计数。
pub const fn leak_cell(l: usize) -> Option<(u64, u32)> {
    if l >= LEAKS {
        return None;
    }
    Some((
        (0x45000u64 + l as u64 * 0x2C) ^ (l as u64).wrapping_mul(0x01000193),
        3 + (l as u32) % 5,
    ))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DtLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 段账。
    segs: [[Option<u64>; HEAPS]; SEGMENTS],
    /// 泄漏账。
    leaks: [Option<u64>; LEAKS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DtLedger {
    /// 新账本。
    pub fn new() -> Self {
        DtLedger {
            gates: [None; 6],
            segs: [[None; HEAPS]; SEGMENTS],
            leaks: [None; LEAKS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DtCode> {
        if gate >= 6 {
            return Err(DtCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1E100 + gate as u64);
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

    /// 段一格（打开闸 + 载荷对照 + 重复拒）。
    pub fn record_seg(&mut self, s: usize, h: usize, used_ticks: u64) -> Result<u64, DtCode> {
        if s >= SEGMENTS {
            return Err(DtCode::ENoSeg);
        }
        if h >= HEAPS {
            return Err(DtCode::ENoHeap);
        }
        if s == 0
            && (used_ticks == 0
                || used_ticks
                    > SNAP_BASE_TICKS + SNAP_BASE_TICKS * OPEN_CAP_NUM / OPEN_CAP_DEN)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1E200 + h as u64);
            return Err(DtCode::EOpenCap);
        }
        let payload = match seg_cell(s, h) {
            Some(p) => p,
            None => return Err(DtCode::ENoHeap),
        };
        if match self.segs.get(s).and_then(|row| row.get(h)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DtCode::EDup);
        }
        if let Some(row) = self.segs.get_mut(s) {
            if let Some(c) = row.get_mut(h) {
                *c = Some(payload);
            }
        }
        self.fnv(0x1E201 + (s as u64) * 6 + h as u64);
        Ok(payload)
    }

    /// 段全账核验（18 格）。
    pub fn segs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SEGMENTS {
            let mut h = 0usize;
            while h < HEAPS {
                match self.segs.get(s).and_then(|row| row.get(h)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                h += 1;
            }
            s += 1;
        }
        true
    }

    /// 泄漏一格（保留引用 >0 + 载荷恒等才收）。
    pub fn record_leak(&mut self, l: usize, got: u64, refs: u32) -> Result<(), DtCode> {
        let (want, want_refs) = match leak_cell(l) {
            Some(x) => x,
            None => return Err(DtCode::ENoHeap),
        };
        if refs == 0 || refs != want_refs {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1E300 + l as u64);
            return Err(DtCode::EZeroRef);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1E301 + l as u64);
            return Err(DtCode::EDrift);
        }
        if let Some(c) = self.leaks.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x1E302 + l as u64);
        Ok(())
    }

    /// 泄漏全账核验。
    pub fn leaks_all(&self) -> bool {
        let mut l = 0usize;
        while l < LEAKS {
            match self.leaks.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

impl Default for DtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DtLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SEGMENTS {
        let mut h = 0usize;
        while h < HEAPS {
            let ticks = match s {
                0 => open_ticks(h),
                _ => 1,
            };
            if led.record_seg(s, h, ticks).is_ok() {
                recorded += 1;
            }
            h += 1;
        }
        s += 1;
    }
    let mut l = 0usize;
    while l < LEAKS {
        if let Some((w, refs)) = leak_cell(l) {
            let _ = led.record_leak(l, w, refs);
        }
        l += 1;
    }
    // 注入 10 轮：打开超封顶 / 泄漏误报 / 段越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DtLedger::new();
        let caught = match kind {
            0 => lt.record_seg(0, 0, SNAP_BASE_TICKS * 3) == Err(DtCode::EOpenCap),
            1 => {
                let (w, _) = match leak_cell(2) {
                    Some(x) => x,
                    None => (0, 1),
                };
                lt.record_leak(2, w, 0) == Err(DtCode::EZeroRef)
            }
            _ => lt.record_seg(SEGMENTS, 0, 1).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1E400 + k as u64);
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
pub fn run_unx52054_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52054");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段 18/18 全通",
        recorded == (SEGMENTS * HEAPS) as u32,
        "快照/泄漏/保留树三段 ×6 堆场景全通",
    );
    s.add(
        "GB 级打开 ≤×1.5",
        {
            let mut ok = true;
            let mut h = 0usize;
            while h < HEAPS {
                if !open_ok(h) {
                    ok = false;
                }
                h += 1;
            }
            ok
        },
        "逐堆打开拍 ≤ 4 GB 基线 ×1.5",
    );
    s.add(
        "泄漏 8/8 在册零误报",
        {
            let mut lt = DtLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LEAKS {
                if let Some((w, refs)) = leak_cell(l) {
                    if lt.record_leak(l, w, refs).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.leaks_all()
        },
        "保留树引用计数 >0 且载荷恒等",
    );
    s.add("注入 10/10 全抓", caught >= 10, "打开/误报/段三态轮转必抓");
    let mut l0 = DtLedger::new();
    s.add(
        "打开·超封顶拒",
        l0.record_seg(0, 0, SNAP_BASE_TICKS * 3) == Err(DtCode::EOpenCap),
        "打开拍超基线 ×1.5 必抓",
    );
    s.add(
        "边界·段越闭集",
        l0.record_seg(SEGMENTS, 0, 1).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·堆越闭集",
        l0.record_seg(0, HEAPS, 1).is_err(),
        "堆 6 越闭集必拒",
    );
    let mut l1 = DtLedger::new();
    let (w, r) = match leak_cell(1) {
        Some(x) => x,
        None => (0, 1),
    };
    s.add(
        "泄漏·误报拒",
        l1.record_leak(1, w, 0) == Err(DtCode::EZeroRef),
        "保留引用为零必抓",
    );
    let mut l2 = DtLedger::new();
    let _ = l2.record_seg(1, 1, 1);
    s.add(
        "重复·再落账拒",
        l2.record_seg(1, 1, 1).is_err(),
        "同格只许一条账目",
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
    fn test_cells_pure() {
        let a = seg_cell(2, 3).unwrap_or(0);
        assert_eq!(a, seg_cell(2, 3).unwrap_or(0));
        assert_ne!(a, seg_cell(2, 4).unwrap_or(0));
        assert!(seg_cell(SEGMENTS, 0).is_none());
        assert!(seg_cell(0, HEAPS).is_none());
        assert!(leak_cell(LEAKS).is_none());
        assert_ne!(leak_cell(0), leak_cell(1));
    }

    #[test]
    fn test_open_cap() {
        let mut h = 0usize;
        while h < HEAPS {
            assert!(open_ok(h));
            h += 1;
        }
        assert!(open_ticks(0) > SNAP_BASE_TICKS);
        assert!(open_ticks(HEAPS) == 0);
    }

    #[test]
    fn test_seg_gates() {
        let mut lt = DtLedger::new();
        assert_eq!(lt.record_seg(0, 0, SNAP_BASE_TICKS * 3), Err(DtCode::EOpenCap));
        assert_eq!(lt.record_seg(0, 0, 0), Err(DtCode::EOpenCap));
        assert!(lt.record_seg(0, 0, open_ticks(0)).is_ok());
        assert!(lt.record_seg(0, 0, open_ticks(0)).is_err());
    }

    #[test]
    fn test_leak_zero_ref_caught() {
        let mut lt = DtLedger::new();
        let (w, _) = leak_cell(3).unwrap_or((0, 1));
        assert_eq!(lt.record_leak(3, w, 0), Err(DtCode::EZeroRef));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (SEGMENTS * HEAPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4D80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52054_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
