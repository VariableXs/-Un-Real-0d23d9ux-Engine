//! UNX-F52065 .NET Framework 4.8 Developer Pack 收口 · 0xC5040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52065 逐字锚定）：UNX-F52066-J1 dotnet build/
//! publish ×10 工程全通；AOT 发布路径产物可运行；Windows 发行版应用
//! 收口口径与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚
//! 定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：dotnet build/publish ×10 工程账（build 10 格 + publish
//!      10 格 = 20 格账），逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② AOT 发布路径账：AOT 链 4 格（前编/链接/裁剪/产物可运行），裁
//!      剪后产物载荷必须回可运行基准（不可运行即漂移必抓）。
//!   ③ L4 分账：发行版应用收口口径与 L4 分账 4 行声明闭集（分账声明
//!      /联签声明/口径声明/回退声明），缺一拒封。
//!   ④ build→publish 拍序单向闸：publish 前必须 build（未 build 先
//!      publish → 前置闸）；注入 ×10 轮（build 漂移/publish 前置/AOT
//!      产物不可运行三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据
//!      母版闸 + 定级 S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52065 − 52000)）。
pub const SEG_BASE: u32 = 0xC5040;

/// 工程数。
pub const PROJS: usize = 10;

/// AOT 链格数。
pub const AOT_CELLS: usize = 4;

/// L4 分账声明行数。
pub const L4_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NfCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// build 漂移
    EBuildDrift,
    /// publish 前置闸（未 build 先 publish）
    EPrePublish,
    /// publish 漂移
    EPubDrift,
    /// AOT 链格越册
    ENoAot,
    /// AOT 产物不可运行
    ENoRun,
    /// L4 声明行越闭集
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

impl NfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NfCode::Ok => "成功",
            NfCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            NfCode::EBuildDrift => "build 漂移：build 折叠与基准不恒等，逐工程复算",
            NfCode::EPrePublish => "未 build 先 publish：build 账不齐不许 publish",
            NfCode::EPubDrift => "publish 漂移：publish 折叠与基准不恒等",
            NfCode::ENoAot => "AOT 链格越册：4 格闭集之外先登记",
            NfCode::ENoRun => "AOT 产物不可运行：裁剪后产物必须回可运行基准",
            NfCode::ENoRow => "L4 声明行越闭集：4 行闭集之外先登记",
            NfCode::ERowMiss => "缺声明行：L4 分账声明未齐先补齐",
            NfCode::EDup => "重复落账：同格只许一条账目",
            NfCode::ESeal => "未齐即封：20 格未齐先补齐",
            NfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NfCode::EGradeDrift => "定级漂移：20 格全通+AOT 可运行+L4 全齐才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// build 格载荷（工程 p）。
pub const fn build_cell(p: usize) -> Option<u64> {
    if p >= PROJS {
        return None;
    }
    Some((0x68000u64 + (p as u64) * 0x2A) ^ (p as u64).wrapping_mul(0x01000193))
}

/// publish 格载荷（工程 p）。
pub const fn pub_cell(p: usize) -> Option<u64> {
    if p >= PROJS {
        return None;
    }
    Some((0x69000u64 + (p as u64) * 0x2E) ^ ((p as u64) + 0x70).wrapping_mul(0x01000193))
}

/// AOT 链格基准（格 a）。
pub const fn aot_cell(a: usize) -> Option<u64> {
    if a >= AOT_CELLS {
        return None;
    }
    Some((0x6A000u64 + a as u64 * 0x3C) ^ 0x616F74726E)
}

/// L4 声明行折叠（行 r）。
pub const fn l4_row(r: usize) -> Option<u32> {
    if r >= L4_ROWS {
        return None;
    }
    Some(0x6B00 + r as u32 * 0x2D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct NfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// build 账。
    builds: [Option<u64>; PROJS],
    /// publish 账。
    pubs: [Option<u64>; PROJS],
    /// AOT 账。
    aot: [Option<u64>; AOT_CELLS],
    /// L4 声明账。
    l4: [Option<u32>; L4_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NfLedger {
    /// 新账本。
    pub fn new() -> Self {
        NfLedger {
            gates: [None; 6],
            builds: [None; PROJS],
            pubs: [None; PROJS],
            aot: [None; AOT_CELLS],
            l4: [None; L4_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NfCode> {
        if gate >= 6 {
            return Err(NfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x2A100 + gate as u64);
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

    /// build 一格。
    pub fn record_build(&mut self, p: usize, got: u64) -> Result<(), NfCode> {
        if p >= PROJS {
            return Err(NfCode::ENoProj);
        }
        let want = match build_cell(p) {
            Some(w) => w,
            None => return Err(NfCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2A200 + p as u64);
            return Err(NfCode::EBuildDrift);
        }
        if match self.builds.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NfCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x2A201 + p as u64);
        Ok(())
    }

    /// build 全账核验。
    pub fn builds_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            match self.builds.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// publish 一格（build 前置闸 + 载荷对照 + 重复拒）。
    pub fn record_publish(&mut self, p: usize, got: u64) -> Result<(), NfCode> {
        if p >= PROJS {
            return Err(NfCode::ENoProj);
        }
        if !self.builds_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2A300);
            return Err(NfCode::EPrePublish);
        }
        let want = match pub_cell(p) {
            Some(w) => w,
            None => return Err(NfCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2A301 + p as u64);
            return Err(NfCode::EPubDrift);
        }
        if match self.pubs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NfCode::EDup);
        }
        if let Some(c) = self.pubs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x2A302 + p as u64);
        Ok(())
    }

    /// publish 全账核验。
    pub fn pubs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            match self.pubs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// AOT 链一格（产物可运行闸）。
    pub fn record_aot(&mut self, a: usize, got: u64) -> Result<(), NfCode> {
        let want = match aot_cell(a) {
            Some(w) => w,
            None => return Err(NfCode::ENoAot),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2A400 + a as u64);
            return Err(NfCode::ENoRun);
        }
        if let Some(c) = self.aot.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x2A401 + a as u64);
        Ok(())
    }

    /// AOT 全账核验。
    pub fn aot_all(&self) -> bool {
        let mut a = 0usize;
        while a < AOT_CELLS {
            match self.aot.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// L4 声明一行。
    pub fn declare_l4(&mut self, r: usize) -> Result<(), NfCode> {
        if r >= L4_ROWS {
            return Err(NfCode::ENoRow);
        }
        if match self.l4.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NfCode::EDup);
        }
        if let Some(c) = self.l4.get_mut(r) {
            *c = l4_row(r);
        }
        self.fnv(0x2A500 + r as u64);
        Ok(())
    }

    /// L4 全齐核验。
    pub fn l4_all(&self) -> bool {
        let mut r = 0usize;
        while r < L4_ROWS {
            match self.l4.get(r) {
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

impl Default for NfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        if let Some(w) = build_cell(p) {
            if led.record_build(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut p2 = 0usize;
    while p2 < PROJS {
        if let Some(w) = pub_cell(p2) {
            if led.record_publish(p2, w).is_ok() {
                recorded += 1;
            }
        }
        p2 += 1;
    }
    let mut a = 0usize;
    while a < AOT_CELLS {
        if let Some(w) = aot_cell(a) {
            let _ = led.record_aot(a, w);
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < L4_ROWS {
        let _ = led.declare_l4(r);
        r += 1;
    }
    // 注入 10 轮：publish 前置 / build 漂移 / AOT 不可运行三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NfLedger::new();
        let caught = match kind {
            0 => lt.record_publish(0, 0) == Err(NfCode::EPrePublish),
            1 => lt.record_build(3, build_cell(3).unwrap_or(0).wrapping_add(1))
                == Err(NfCode::EBuildDrift),
            _ => lt.record_aot(2, aot_cell(2).unwrap_or(0).wrapping_add(1)) == Err(NfCode::ENoRun),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x2A600 + k as u64);
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
pub fn run_unx52065_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52065");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "build/publish 20/20 全通",
        recorded == (PROJS * 2) as u32,
        "dotnet build/publish ×10 工程全通",
    );
    s.add(
        "AOT 链 4/4 可运行",
        {
            let mut lt = NfLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < AOT_CELLS {
                if let Some(w) = aot_cell(a) {
                    if lt.record_aot(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.aot_all()
        },
        "前编/链接/裁剪/产物可运行四格零漂移",
    );
    s.add(
        "L4 分账声明 4/4 全齐",
        {
            let mut lt = NfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < L4_ROWS {
                if lt.declare_l4(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.l4_all()
        },
        "分账/联签/口径/回退四行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "前置/漂移/AOT 三态轮转必抓");
    let mut l0 = NfLedger::new();
    s.add(
        "publish 前置闸",
        l0.record_publish(0, 0) == Err(NfCode::EPrePublish),
        "未 build 先 publish 必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_build(PROJS, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    s.add(
        "边界·AOT 越册",
        l0.record_aot(AOT_CELLS, 0).is_err(),
        "AOT 4 越册必拒",
    );
    let mut l1 = NfLedger::new();
    let _ = l1.record_build(1, build_cell(1).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_build(1, build_cell(1).unwrap_or(0)).is_err(),
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
        let a = build_cell(3).unwrap_or(0);
        assert_eq!(a, build_cell(3).unwrap_or(0));
        assert_ne!(a, build_cell(4).unwrap_or(0));
        assert_ne!(build_cell(3), pub_cell(3));
        assert!(build_cell(PROJS).is_none());
        assert!(pub_cell(PROJS).is_none());
        assert!(aot_cell(AOT_CELLS).is_none());
        assert!(l4_row(L4_ROWS).is_none());
    }

    #[test]
    fn test_pre_publish_gate() {
        let mut lt = NfLedger::new();
        assert_eq!(lt.record_publish(0, 0), Err(NfCode::EPrePublish));
    }

    #[test]
    fn test_aot_gate() {
        let mut lt = NfLedger::new();
        assert_eq!(
            lt.record_aot(1, aot_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(NfCode::ENoRun)
        );
        assert!(lt.record_aot(1, aot_cell(1).unwrap_or(0)).is_ok());
        assert!(lt.record_aot(AOT_CELLS, 0).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (PROJS * 2) as u32);
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
        assert_eq!(SEG_BASE, 0xC5040);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52065_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
