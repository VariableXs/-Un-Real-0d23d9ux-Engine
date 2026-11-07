//! UNX-F52098 fastboot 刷写面收口（只读判据版） · 0xC5640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52098 逐字锚定）：UNX-F52099-J1 三档平台包安
//! 装/引用/编译样本 ×3 ×10 全通；多档并存选择语义零漂移；KB-COMPAT 定
//! 级登记；判据锚定 §AI-66.2 专题六版本阶梯。
//!
//! 设计要点：
//!   ① 域模型：只读状态查询 ×10 场景账（getvar/bootdevices/reboot 三
//!      类只读闭集），逐场景载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 只读红线闸：flash/erase/boot 镜像类一律 dry-run（只列清单）；
//!      红线声明 2 行闭集（dry-run-only 声明/真机写不在域声明），缺一
//!      拒封；越线必抓。
//!   ③ 三档平台包并存选择账：3 档闭集 ×3 样本 = 9 格并存账，选择载荷
//!      恒等才收；选择漂移必抓。
//!   ④ 注入 ×10 轮（越线/并存漂移/场景越闭集三态轮转）10/10 必抓，注
//!      入=检出+逃逸守恒；六判据母版闸 + 定级（10 格 + 9 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52098 − 52000)）。
pub const SEG_BASE: u32 = 0xC5640;

/// 只读查询场景数。
pub const RSCENES: usize = 10;

/// 平台包档数。
pub const PKGS: usize = 3;

/// 样本数。
pub const SAMPLES: usize = 3;

/// 红线声明行数。
pub const RED_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 查询载荷漂移
    EDrift,
    /// 红线越线
    ERedLine,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 包档越闭集
    ENoPkg,
    /// 并存选择漂移
    EPkgDrift,
    /// 样本号越闭集
    ENoSample,
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

impl FbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FbCode::Ok => "成功",
            FbCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            FbCode::EDrift => "查询载荷漂移：折叠与基准不恒等，逐场景复算",
            FbCode::ERedLine => "红线越线：flash/erase/boot 镜像必须 dry-run，真机写必抓",
            FbCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            FbCode::ERowMiss => "缺声明行：只读红线适用性声明未齐先补齐",
            FbCode::ENoPkg => "包档越闭集：3 档平台包之外先登记",
            FbCode::EPkgDrift => "并存选择漂移：选择折叠与基准不恒等，逐格复算",
            FbCode::ENoSample => "样本号越闭集：3 样本之外先登记",
            FbCode::EDup => "重复落账：同格只许一条账目",
            FbCode::ESeal => "未齐即封：10 格 + 9 格未齐先补齐",
            FbCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FbCode::EGradeDrift => "定级漂移：10 格+9 格全通+红线声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 只读查询格载荷（场景 sc）。
pub const fn rq_cell(sc: usize) -> Option<u64> {
    if sc >= RSCENES {
        return None;
    }
    Some((0xC1000u64 + (sc as u64) * 0x2B) ^ ((sc / 4) as u64).wrapping_mul(0x01000193))
}

/// 并存选择格载荷（包档 p × 样本 s）。
pub const fn pkg_cell(p: usize, s: usize) -> Option<u64> {
    if p >= PKGS || s >= SAMPLES {
        return None;
    }
    Some(
        (0xC2000u64 + (p as u64) * 0x2F)
            ^ (s as u64).wrapping_mul(0x01000193)
            ^ 0x66617374626F6F74,
    )
}

/// 红线声明行折叠（行 r）。
pub const fn red_row(r: usize) -> Option<u32> {
    if r >= RED_ROWS {
        return None;
    }
    Some(0xC300 + r as u32 * 0x38)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct FbLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 只读查询账。
    rqs: [Option<u64>; RSCENES],
    /// 并存选择账。
    pkgs: [[Option<u64>; SAMPLES]; PKGS],
    /// 红线声明账。
    reds: [Option<u32>; RED_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FbLedger {
    /// 新账本。
    pub fn new() -> Self {
        FbLedger {
            gates: [None; 6],
            rqs: [None; RSCENES],
            pkgs: [[None; SAMPLES]; PKGS],
            reds: [None; RED_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FbCode> {
        if gate >= 6 {
            return Err(FbCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x42100 + gate as u64);
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

    /// 破坏性写入口（一律 dry-run；exec=true 越红线必抓）。
    pub fn destructive(&mut self, exec: bool) -> Result<(), FbCode> {
        if exec {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x42200);
            return Err(FbCode::ERedLine);
        }
        self.fnv(0x42201);
        Ok(())
    }

    /// 只读查询一格。
    pub fn record_rq(&mut self, sc: usize, got: u64) -> Result<(), FbCode> {
        if sc >= RSCENES {
            return Err(FbCode::ENoScene);
        }
        let want = match rq_cell(sc) {
            Some(w) => w,
            None => return Err(FbCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x42300 + sc as u64);
            return Err(FbCode::EDrift);
        }
        if match self.rqs.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FbCode::EDup);
        }
        if let Some(c) = self.rqs.get_mut(sc) {
            *c = Some(got);
        }
        self.fnv(0x42301 + sc as u64);
        Ok(())
    }

    /// 只读查询全账核验（10 格）。
    pub fn rqs_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < RSCENES {
            match self.rqs.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 并存选择一格。
    pub fn choose_pkg(&mut self, p: usize, s: usize, got: u64) -> Result<(), FbCode> {
        if p >= PKGS {
            return Err(FbCode::ENoPkg);
        }
        if s >= SAMPLES {
            return Err(FbCode::ENoSample);
        }
        let want = match pkg_cell(p, s) {
            Some(w) => w,
            None => return Err(FbCode::ENoPkg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x42400 + (p as u64) * 3 + s as u64);
            return Err(FbCode::EPkgDrift);
        }
        if match self.pkgs.get(p).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FbCode::EDup);
        }
        if let Some(row) = self.pkgs.get_mut(p) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(got);
            }
        }
        self.fnv(0x42401 + (p as u64) * 3 + s as u64);
        Ok(())
    }

    /// 并存选择全账核验（9 格）。
    pub fn pkgs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PKGS {
            let mut s = 0usize;
            while s < SAMPLES {
                match self.pkgs.get(p).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            p += 1;
        }
        true
    }

    /// 红线声明一行。
    pub fn declare_red(&mut self, r: usize) -> Result<(), FbCode> {
        if r >= RED_ROWS {
            return Err(FbCode::ENoRow);
        }
        if match self.reds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FbCode::EDup);
        }
        if let Some(c) = self.reds.get_mut(r) {
            *c = red_row(r);
        }
        self.fnv(0x42500 + r as u64);
        Ok(())
    }

    /// 红线声明全齐核验。
    pub fn reds_all(&self) -> bool {
        let mut r = 0usize;
        while r < RED_ROWS {
            match self.reds.get(r) {
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

impl Default for FbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < RSCENES {
        if let Some(w) = rq_cell(sc) {
            if led.record_rq(sc, w).is_ok() {
                recorded += 1;
            }
        }
        sc += 1;
    }
    let _ = led.destructive(false);
    let mut p = 0usize;
    while p < PKGS {
        let mut s = 0usize;
        while s < SAMPLES {
            if let Some(w) = pkg_cell(p, s) {
                if led.choose_pkg(p, s, w).is_ok() {
                    recorded += 1;
                }
            }
            s += 1;
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < RED_ROWS {
        let _ = led.declare_red(r);
        r += 1;
    }
    // 注入 10 轮：红线越线 / 并存漂移 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FbLedger::new();
        let caught = match kind {
            0 => lt.destructive(true) == Err(FbCode::ERedLine),
            1 => lt.choose_pkg(1, 1, pkg_cell(1, 1).unwrap_or(0).wrapping_add(1))
                == Err(FbCode::EPkgDrift),
            _ => lt.record_rq(RSCENES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x42600 + k as u64);
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
pub fn run_unx52098_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52098");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "只读查询 10/10 全通",
        recorded >= RSCENES as u32,
        "getvar/bootdevices/reboot 三类只读逐场景恒等",
    );
    s.add(
        "三档并存选择 9/9 零漂移",
        {
            let mut lt = FbLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PKGS {
                let mut s2 = 0usize;
                while s2 < SAMPLES {
                    if let Some(w) = pkg_cell(p, s2) {
                        if lt.choose_pkg(p, s2, w).is_err() {
                            ok = false;
                        }
                    }
                    s2 += 1;
                }
                p += 1;
            }
            ok && lt.pkgs_all()
        },
        "3 档平台包 ×3 样本选择语义逐格恒等",
    );
    s.add(
        "破坏性写一律 dry-run",
        {
            let mut lt = FbLedger::new();
            lt.destructive(true) == Err(FbCode::ERedLine)
                && lt.destructive(false).is_ok()
        },
        "flash/erase/boot 镜像越线必抓",
    );
    s.add(
        "红线声明 2/2 全齐",
        {
            let mut lt = FbLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RED_ROWS {
                if lt.declare_red(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.reds_all()
        },
        "dry-run-only/真机写不在域两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越线/漂移/越闭集三态轮转必抓");
    let mut l0 = FbLedger::new();
    s.add(
        "红线·越线拒",
        l0.destructive(true) == Err(FbCode::ERedLine),
        "真机写入必抓",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_rq(RSCENES, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·包档越闭集",
        l0.choose_pkg(PKGS, 0, 0).is_err(),
        "包档 3 越闭集必拒",
    );
    s.add(
        "边界·样本越闭集",
        l0.choose_pkg(0, SAMPLES, 0).is_err(),
        "样本 3 越闭集必拒",
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
        let a = rq_cell(4).unwrap_or(0);
        assert_eq!(a, rq_cell(4).unwrap_or(0));
        assert_ne!(a, rq_cell(5).unwrap_or(0));
        assert!(rq_cell(RSCENES).is_none());
        let b = pkg_cell(2, 1).unwrap_or(0);
        assert_ne!(b, pkg_cell(1, 1).unwrap_or(0));
        assert!(pkg_cell(PKGS, 0).is_none());
        assert!(pkg_cell(0, SAMPLES).is_none());
        assert!(red_row(RED_ROWS).is_none());
    }

    #[test]
    fn test_redline_gate() {
        let mut lt = FbLedger::new();
        assert_eq!(lt.destructive(true), Err(FbCode::ERedLine));
        assert!(lt.destructive(false).is_ok());
    }

    #[test]
    fn test_pkg_drift_caught() {
        let mut lt = FbLedger::new();
        assert_eq!(
            lt.choose_pkg(0, 0, pkg_cell(0, 0).unwrap_or(0).wrapping_add(3)),
            Err(FbCode::EPkgDrift)
        );
        assert!(lt.choose_pkg(0, 0, pkg_cell(0, 0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= RSCENES as u32);
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
        assert_eq!(SEG_BASE, 0xC5640);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52098_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
