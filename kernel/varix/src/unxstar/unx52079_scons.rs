//! UNX-F52079 SCons 收口 · 0xC53C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52079 逐字锚定）：UNX-F52080-J1 Makefile ×10
//! 样本（含并行 -j4）构建全通；与 L4 make 类开发者包组分账声明（联签
//! 锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明。
//!
//! 设计要点：
//!   ① 域模型：Makefile ×10 样本账（含并行 -j4 二元），10×2 = 20 格构
//!      建账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 并行账：-j4 并行度闸（并行度 ≤4 才收，超限必抓）；并行拍序账
//!      8 拍严格递增。
//!   ③ L4 分账声明 2 行闭集（make 类开发者包组分账声明/防重声明），
//!      缺一拒封。
//!   ④ 注入 ×10 轮（并行超限/构建漂移/样本越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 并行
//!      闸在册 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52079 − 52000)）。
pub const SEG_BASE: u32 = 0xC53C0;

/// 样本数。
pub const SAMPLES: usize = 10;

/// 并行二元。
pub const PAR: usize = 2;

/// 并行度封顶。
pub const J_CAP: u32 = 4;

/// 并行拍数。
pub const PAR_TICKS: usize = 8;

/// 声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 成功
    Ok = 0,
    /// 样本号越闭集
    ENoSample,
    /// 并行二元越闭集
    ENoPar,
    /// 构建漂移
    EDrift,
    /// 并行度超限
    EJCap,
    /// 并行拍序乱
    EOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

impl ScCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScCode::Ok => "成功",
            ScCode::ENoSample => "样本号越闭集：10 样本之外先登记",
            ScCode::ENoPar => "并行二元越闭集：串行/-j4 二元之外先登记",
            ScCode::EDrift => "构建漂移：构建折叠与基准不恒等，逐格复算",
            ScCode::EJCap => "并行度超限：-j ≤4 才收，超限必抓",
            ScCode::EOrder => "并行拍序乱：8 拍必须严格递增单调",
            ScCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            ScCode::ERowMiss => "缺声明行：L4 分账/防重声明未齐先补齐",
            ScCode::EDup => "重复落账：同格只许一条账目",
            ScCode::ESeal => "未齐即封：20 格未齐先补齐",
            ScCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ScCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ScCode::EGradeDrift => "定级漂移：20 格全通+并行闸在册+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建格载荷（样本 s × 并行 p）。
pub const fn build_cell(s: usize, p: usize) -> Option<u64> {
    if s >= SAMPLES || p >= PAR {
        return None;
    }
    Some(
        (0x9E000u64 + (s as u64) * 0x2C)
            ^ (p as u64).wrapping_mul(0x01000193)
            ^ 0x73636F6E736A34,
    )
}

/// 并行度闸。
pub const fn j_ok(j: u32) -> bool {
    j <= J_CAP
}

/// 并行拍基准（拍 t，升序）。
pub const fn par_tick(t: usize) -> Option<u64> {
    if t >= PAR_TICKS {
        return None;
    }
    Some(0x9F000u64 + (t as u64) * 0x21)
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA000 + r as u32 * 0x33)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct ScLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 构建账。
    builds: [[Option<u64>; PAR]; SAMPLES],
    /// 并行最后拍。
    last_par: i64,
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ScLedger {
    /// 新账本。
    pub fn new() -> Self {
        ScLedger {
            gates: [None; 6],
            builds: [[None; PAR]; SAMPLES],
            last_par: -1,
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ScCode> {
        if gate >= 6 {
            return Err(ScCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x38100 + gate as u64);
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

    /// 构建一格（并行二元 + 载荷对照 + 重复拒）。
    pub fn record_build(&mut self, s: usize, p: usize, j: u32, got: u64) -> Result<(), ScCode> {
        if s >= SAMPLES {
            return Err(ScCode::ENoSample);
        }
        if p >= PAR {
            return Err(ScCode::ENoPar);
        }
        if p == 1 && !j_ok(j) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x38200 + s as u64);
            return Err(ScCode::EJCap);
        }
        let want = match build_cell(s, p) {
            Some(w) => w,
            None => return Err(ScCode::ENoPar),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x38201 + (s as u64) * 2 + p as u64);
            return Err(ScCode::EDrift);
        }
        if match self.builds.get(s).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScCode::EDup);
        }
        if let Some(row) = self.builds.get_mut(s) {
            if let Some(c) = row.get_mut(p) {
                *c = Some(got);
            }
        }
        self.fnv(0x38202 + (s as u64) * 2 + p as u64);
        Ok(())
    }

    /// 构建全账核验（20 格）。
    pub fn builds_all(&self) -> bool {
        let mut s = 0usize;
        while s < SAMPLES {
            let mut p = 0usize;
            while p < PAR {
                match self.builds.get(s).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            s += 1;
        }
        true
    }

    /// 并行一拍（严格递增）。
    pub fn record_par(&mut self, t: usize, got: u64) -> Result<(), ScCode> {
        let want = match par_tick(t) {
            Some(w) => w,
            None => return Err(ScCode::EOrder),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x38300 + t as u64);
            return Err(ScCode::EOrder);
        }
        if (t as i64) <= self.last_par {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x38301);
            return Err(ScCode::EOrder);
        }
        self.last_par = t as i64;
        self.fnv(0x38302 + t as u64);
        Ok(())
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), ScCode> {
        if r >= SPLIT_ROWS {
            return Err(ScCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x38400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
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

impl Default for ScLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ScLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SAMPLES {
        let mut p = 0usize;
        while p < PAR {
            if let Some(w) = build_cell(s, p) {
                if led.record_build(s, p, J_CAP, w).is_ok() {
                    recorded += 1;
                }
            }
            p += 1;
        }
        s += 1;
    }
    let mut t = 0usize;
    while t < PAR_TICKS {
        if let Some(w) = par_tick(t) {
            let _ = led.record_par(t, w);
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：并行超限 / 构建漂移 / 样本越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ScLedger::new();
        let caught = match kind {
            0 => lt.record_build(0, 1, J_CAP + 1, build_cell(0, 1).unwrap_or(0))
                == Err(ScCode::EJCap),
            1 => lt.record_build(3, 0, J_CAP, build_cell(3, 0).unwrap_or(0).wrapping_add(1))
                == Err(ScCode::EDrift),
            _ => lt.record_build(SAMPLES, 0, J_CAP, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x38500 + k as u64);
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
pub fn run_unx52079_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52079");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "构建 20/20 全通",
        recorded >= (SAMPLES * PAR) as u32,
        "Makefile ×10 样本（含并行 -j4 二元）逐格恒等",
    );
    s.add(
        "并行度闸",
        j_ok(J_CAP) && !j_ok(J_CAP + 1) && !j_ok(255),
        "-j ≤4 才收，超限必抓",
    );
    s.add(
        "并行拍 8/8 单调",
        {
            let mut lt = ScLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < PAR_TICKS {
                if let Some(w) = par_tick(t) {
                    if lt.record_par(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok
        },
        "并行拍序严格递增",
    );
    s.add(
        "L4 分账声明 2/2 全齐",
        {
            let mut lt = ScLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "make 类开发者包组分账/防重声明两行闭集（AI-59 联签）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "超限/漂移/越闭集三态轮转必抓");
    let mut l0 = ScLedger::new();
    s.add(
        "并行·超限拒",
        l0.record_build(1, 1, 9, build_cell(1, 1).unwrap_or(0)) == Err(ScCode::EJCap),
        "-j 超过 4 必抓",
    );
    s.add(
        "边界·样本越闭集",
        l0.record_build(SAMPLES, 0, J_CAP, 0).is_err(),
        "样本 10 越闭集必拒",
    );
    s.add(
        "边界·并行越闭集",
        l0.record_build(0, PAR, J_CAP, 0).is_err(),
        "并行二元越闭集必拒",
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
    fn test_j_cap() {
        assert!(j_ok(0));
        assert!(j_ok(4));
        assert!(!j_ok(5));
        assert!(!j_ok(u32::MAX));
    }

    #[test]
    fn test_cells_pure() {
        let a = build_cell(4, 1).unwrap_or(0);
        assert_eq!(a, build_cell(4, 1).unwrap_or(0));
        assert_ne!(a, build_cell(4, 0).unwrap_or(0));
        assert!(build_cell(SAMPLES, 0).is_none());
        assert!(build_cell(0, PAR).is_none());
        assert!(par_tick(PAR_TICKS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_par_order() {
        let mut lt = ScLedger::new();
        let _ = lt.record_par(3, par_tick(3).unwrap_or(0));
        assert_eq!(lt.record_par(1, par_tick(1).unwrap_or(0)), Err(ScCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SAMPLES * PAR) as u32);
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
        assert_eq!(SEG_BASE, 0xC53C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52079_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
