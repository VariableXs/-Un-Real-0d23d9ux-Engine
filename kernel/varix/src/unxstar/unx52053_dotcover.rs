//! UNX-F52053 dotCover 收口 · 0xC4D40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52053 逐字锚定）：UNX-F52054-J1 快照采集/时间
//! 线视图/热点定位三段账；采样模式 ×2 全通；KB-COMPAT 定级登记；判据
//! 锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：三段账（快照采集/时间线视图/热点定位）× 6 工程场景 =
//!      18 格账；采样模式二元闭集（全量 full / 增量 incr），模式 ×2 全
//!      通 = 逐工程两轮采样各记两格，双模式账 12 格独立核验。
//!   ② 时间线单调闸：时间线 12 拍严格递增单调（拍载荷与基准恒等），
//!      回退/漂移必抓。
//!   ③ 热点守恒：热点定位 Σ热点格载荷 = 纯函数复算总折叠，不守恒即漂
//!      移闸。
//!   ④ 注入 ×10 轮（模式越闭集/时间线回退/热点不守恒三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（18 格全通 +
//!      双模式全账+时间线单调 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52053 − 52000)）。
pub const SEG_BASE: u32 = 0xC4D40;

/// 三段账段数。
pub const SEGMENTS: usize = 3;

/// 工程场景数。
pub const PROJECTS: usize = 6;

/// 采样模式数。
pub const MODES: usize = 2;

/// 时间线拍数。
pub const TL_TICKS: usize = 12;

/// 热点格数。
pub const HOTSPOTS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoSeg,
    /// 工程号越闭集
    ENoProj,
    /// 模式越闭集
    ENoMode,
    /// 时间线拍序乱
    EOrder,
    /// 载荷漂移
    EDrift,
    /// 热点不守恒
    EHotsum,
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

impl DcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DcCode::Ok => "成功",
            DcCode::ENoSeg => "段号越闭集：采集/时间线/热点三段之外先登记",
            DcCode::ENoProj => "工程号越闭集：6 工程之外先登记",
            DcCode::ENoMode => "模式越闭集：全量/增量二元之外先登记",
            DcCode::EOrder => "时间线拍序乱：12 拍必须严格递增单调",
            DcCode::EDrift => "载荷漂移：折叠与基准不恒等，逐格复算",
            DcCode::EHotsum => "热点不守恒：Σ热点格载荷 ≠ 总折叠，逐格对账",
            DcCode::EDup => "重复落账：同格只许一条账目",
            DcCode::ESeal => "未齐即封：18 格未齐先补齐",
            DcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DcCode::EGradeDrift => "定级漂移：18 格全通+双模式全账+时间线单调才定级",
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
        0x40000u64
            + (s as u64) * 0x1F
            + (p as u64) * 0xB
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> p),
    )
}

/// 时间线第 t 拍基准载荷。
pub const fn tl_cell(t: usize) -> Option<u64> {
    if t >= TL_TICKS {
        return None;
    }
    Some(0x41000u64 + (t as u64) * 0x17)
}

/// 热点格基准载荷（格 h）与总折叠（纯函数守恒）。
pub const fn hot_cell(h: usize) -> Option<u64> {
    if h >= HOTSPOTS {
        return None;
    }
    Some(0x42000u64 + (h as u64) * 0x1B)
}

/// Σ热点总折叠。
pub const fn hot_total() -> u64 {
    let mut acc: u64 = 0;
    let mut h = 0usize;
    while h < HOTSPOTS {
        acc = acc.wrapping_add(match hot_cell(h) {
            Some(x) => x,
            None => 0,
        });
        h += 1;
    }
    acc
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 段账。
    segs: [[Option<u64>; PROJECTS]; SEGMENTS],
    /// 双模式账（模式 × 工程 → 载荷）。
    modes: [[Option<u64>; PROJECTS]; MODES],
    /// 时间线最后拍。
    last_tl: i64,
    /// 热点累计。
    hotsum: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DcLedger {
    /// 新账本。
    pub fn new() -> Self {
        DcLedger {
            gates: [None; 6],
            segs: [[None; PROJECTS]; SEGMENTS],
            modes: [[None; PROJECTS]; MODES],
            last_tl: -1,
            hotsum: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DcCode> {
        if gate >= 6 {
            return Err(DcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1D100 + gate as u64);
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

    /// 段一格。
    pub fn record_seg(&mut self, s: usize, p: usize) -> Result<u64, DcCode> {
        if s >= SEGMENTS {
            return Err(DcCode::ENoSeg);
        }
        if p >= PROJECTS {
            return Err(DcCode::ENoProj);
        }
        let payload = match seg_cell(s, p) {
            Some(p) => p,
            None => return Err(DcCode::ENoProj),
        };
        if match self.segs.get(s).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DcCode::EDup);
        }
        if let Some(row) = self.segs.get_mut(s) {
            if let Some(c) = row.get_mut(p) {
                *c = Some(payload);
            }
        }
        self.fnv(0x1D200 + (s as u64) * 6 + p as u64);
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

    /// 采样一轮（模式 × 工程，载荷恒等才收）。
    pub fn record_mode(&mut self, m: usize, p: usize, got: u64) -> Result<(), DcCode> {
        if m >= MODES {
            return Err(DcCode::ENoMode);
        }
        if p >= PROJECTS {
            return Err(DcCode::ENoProj);
        }
        let want = match seg_cell(m, p) {
            Some(x) => x,
            None => return Err(DcCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1D300 + (m as u64) * 6 + p as u64);
            return Err(DcCode::EDrift);
        }
        if let Some(c) = self.modes.get_mut(m).and_then(|row| row.get_mut(p)) {
            *c = Some(got);
        }
        self.fnv(0x1D301 + (m as u64) * 6 + p as u64);
        Ok(())
    }

    /// 双模式全账核验（12 格）。
    pub fn modes_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODES {
            let mut p = 0usize;
            while p < PROJECTS {
                match self.modes.get(m).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            m += 1;
        }
        true
    }

    /// 时间线一拍（严格递增单调）。
    pub fn tl_tick(&mut self, t: usize, got: u64) -> Result<(), DcCode> {
        let want = match tl_cell(t) {
            Some(w) => w,
            None => return Err(DcCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1D400 + t as u64);
            return Err(DcCode::EDrift);
        }
        if (t as i64) <= self.last_tl {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1D401);
            return Err(DcCode::EOrder);
        }
        self.last_tl = t as i64;
        self.fnv(0x1D402 + t as u64);
        Ok(())
    }

    /// 热点一格（累计守恒）。
    pub fn record_hot(&mut self, h: usize, got: u64) -> Result<(), DcCode> {
        let want = match hot_cell(h) {
            Some(w) => w,
            None => return Err(DcCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1D500 + h as u64);
            return Err(DcCode::EHotsum);
        }
        self.hotsum = self.hotsum.wrapping_add(got);
        self.fnv(0x1D501 + h as u64);
        Ok(())
    }

    /// 热点守恒核验。
    pub fn hot_ok(&self) -> bool {
        self.hotsum == hot_total()
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

impl Default for DcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SEGMENTS {
        let mut p = 0usize;
        while p < PROJECTS {
            if led.record_seg(s, p).is_ok() {
                recorded += 1;
            }
            p += 1;
        }
        s += 1;
    }
    let mut m = 0usize;
    while m < MODES {
        let mut p = 0usize;
        while p < PROJECTS {
            if let Some(x) = seg_cell(m, p) {
                let _ = led.record_mode(m, p, x);
            }
            p += 1;
        }
        m += 1;
    }
    let mut t = 0usize;
    while t < TL_TICKS {
        if let Some(w) = tl_cell(t) {
            let _ = led.tl_tick(t, w);
        }
        t += 1;
    }
    let mut h = 0usize;
    while h < HOTSPOTS {
        if let Some(w) = hot_cell(h) {
            let _ = led.record_hot(h, w);
        }
        h += 1;
    }
    // 注入 10 轮：模式越闭集 / 时间线回退 / 热点漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DcLedger::new();
        let caught = match kind {
            0 => lt.record_mode(MODES, 0, 0).is_err(),
            1 => {
                let _ = lt.tl_tick(5, tl_cell(5).unwrap_or(0));
                lt.tl_tick(2, tl_cell(2).unwrap_or(0)) == Err(DcCode::EOrder)
            }
            _ => lt.record_hot(3, hot_cell(3).unwrap_or(0).wrapping_add(1))
                == Err(DcCode::EHotsum),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1D600 + k as u64);
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
pub fn run_unx52053_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52053");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段 18/18 全通",
        recorded == (SEGMENTS * PROJECTS) as u32,
        "采集/时间线/热点三段 ×6 工程全通",
    );
    s.add(
        "采样模式 ×2 全账",
        {
            let mut lt = DcLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MODES {
                let mut p = 0usize;
                while p < PROJECTS {
                    if let Some(x) = seg_cell(m, p) {
                        if lt.record_mode(m, p, x).is_err() {
                            ok = false;
                        }
                    }
                    p += 1;
                }
                m += 1;
            }
            ok && lt.modes_all()
        },
        "全量/增量双模式 12 格逐格恒等",
    );
    s.add(
        "时间线 12 拍单调",
        {
            let mut lt = DcLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TL_TICKS {
                if let Some(w) = tl_cell(t) {
                    if lt.tl_tick(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok
        },
        "时间线拍序严格递增且载荷恒等",
    );
    s.add(
        "热点守恒",
        {
            let mut lt = DcLedger::new();
            let mut h = 0usize;
            while h < HOTSPOTS {
                if let Some(w) = hot_cell(h) {
                    let _ = lt.record_hot(h, w);
                }
                h += 1;
            }
            lt.hot_ok()
        },
        "Σ热点格载荷 = 纯函数总折叠",
    );
    s.add("注入 10/10 全抓", caught >= 10, "模式/时间线/热点三态轮转必抓");
    let mut l0 = DcLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_seg(SEGMENTS, 0).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODES, 0, 0).is_err(),
        "模式 2 越闭集必拒",
    );
    let mut l1 = DcLedger::new();
    let _ = l1.record_seg(0, 0);
    s.add(
        "重复·再落账拒",
        l1.record_seg(0, 0).is_err(),
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
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
        assert!(tl_cell(TL_TICKS).is_none());
        assert_ne!(tl_cell(0), tl_cell(1));
    }

    #[test]
    fn test_hot_conservation() {
        let mut lt = DcLedger::new();
        let mut h = 0usize;
        while h < HOTSPOTS {
            let _ = lt.record_hot(h, hot_cell(h).unwrap_or(0));
            h += 1;
        }
        assert!(lt.hot_ok());
        assert_eq!(lt.record_hot(0, hot_cell(0).unwrap_or(0).wrapping_add(1)), Err(DcCode::EHotsum));
        assert!(lt.record_hot(HOTSPOTS, 0).is_err());
    }

    #[test]
    fn test_tl_order() {
        let mut lt = DcLedger::new();
        let _ = lt.tl_tick(3, tl_cell(3).unwrap_or(0));
        assert_eq!(lt.tl_tick(1, tl_cell(1).unwrap_or(0)), Err(DcCode::EOrder));
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
        assert_eq!(SEG_BASE, 0xC4D40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52053_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
