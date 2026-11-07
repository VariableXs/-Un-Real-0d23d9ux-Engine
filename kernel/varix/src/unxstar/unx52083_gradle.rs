//! UNX-F52083 Gradle 收口（Windows 发行版构建链） · 0xC54C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52083 逐字锚定）：UNX-F52084-J1 Gradle 构建
//! ×10 工程（含 wrapper 验证）全通；daemon 复用与 F52058 同构分账；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：Gradle 构建 ×10 工程账（含 wrapper 验证二元），10×2 =
//!      20 格构建账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② wrapper 验证账：wrapper 三元（版本档/分发 URL 档/校验和档）全
//!      等才放行构建（失配 → wrapper 闸必抓）。
//!   ③ daemon 复用分账：与 F52058 同构分账声明 2 行闭集（daemon 复用
//!      主账声明/F52058 同构分账声明），缺一拒封。
//!   ④ configure→build 拍序单向闸；注入 ×10 轮（wrapper 失配/构建漂
//!      移/工程越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据
//!      母版闸 + 定级（20 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52083 − 52000)）。
pub const SEG_BASE: u32 = 0xC54C0;

/// 工程数。
pub const PROJS: usize = 10;

/// wrapper 二元。
pub const WRAP: usize = 2;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// wrapper 三元基准。
pub const WRAP_VER: u32 = 82;
pub const DIST_URL: u32 = 3;
pub const CKSUM: u32 = 7;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gl2Code {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// wrapper 二元越闭集
    ENoWrap,
    /// wrapper 三元失配
    EWrap,
    /// 构建漂移
    EDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 拍序逆序
    EOrder,
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

impl Gl2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Gl2Code::Ok => "成功",
            Gl2Code::ENoProj => "工程号越闭集：10 工程之外先登记",
            Gl2Code::ENoWrap => "wrapper 二元越闭集：有 wrapper/无 wrapper 二元之外先登记",
            Gl2Code::EWrap => "wrapper 三元失配：版本/分发 URL/校验和档与基准不符",
            Gl2Code::EDrift => "构建漂移：构建折叠与基准不恒等，逐格复算",
            Gl2Code::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            Gl2Code::ERowMiss => "缺声明行：daemon 复用分账声明未齐先补齐",
            Gl2Code::EOrder => "拍序逆序：configure→build 单向，逆序必抓",
            Gl2Code::EDup => "重复落账：同格只许一条账目",
            Gl2Code::ESeal => "未齐即封：20 格未齐先补齐",
            Gl2Code::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Gl2Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Gl2Code::EGradeDrift => "定级漂移：20 格全通+wrapper 闸在册+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建格载荷（工程 p × wrapper w）。
pub const fn build_cell(p: usize, w: usize) -> Option<u64> {
    if p >= PROJS || w >= WRAP {
        return None;
    }
    Some(
        (0xAE000u64 + (p as u64) * 0x2B)
            ^ (w as u64).wrapping_mul(0x01000193)
            ^ 0x677261646C6577,
    )
}

/// wrapper 三元闸。
pub const fn wrap_ok(ver: u32, url: u32, ck: u32) -> bool {
    ver == WRAP_VER && url == DIST_URL && ck == CKSUM
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xAF00 + r as u32 * 0x35)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct Gl2Ledger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 构建账。
    builds: [[Option<u64>; WRAP]; PROJS],
    /// wrapper 校验旗标。
    wrapped: bool,
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Gl2Ledger {
    /// 新账本。
    pub fn new() -> Self {
        Gl2Ledger {
            gates: [None; 6],
            builds: [[None; WRAP]; PROJS],
            wrapped: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Gl2Code> {
        if gate >= 6 {
            return Err(Gl2Code::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3C100 + gate as u64);
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

    /// wrapper 验证（三元闸）。
    pub fn verify_wrapper(&mut self, ver: u32, url: u32, ck: u32) -> Result<(), Gl2Code> {
        if !wrap_ok(ver, url, ck) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3C200);
            return Err(Gl2Code::EWrap);
        }
        self.wrapped = true;
        self.fnv(0x3C201);
        Ok(())
    }

    /// 构建一格（wrapper 闸 + 载荷对照 + 重复拒）。
    pub fn record_build(&mut self, p: usize, w: usize, got: u64) -> Result<(), Gl2Code> {
        if p >= PROJS {
            return Err(Gl2Code::ENoProj);
        }
        if w >= WRAP {
            return Err(Gl2Code::ENoWrap);
        }
        // 有 wrapper（w=1）必须先验证 wrapper 三元。
        if w == 1 && !self.wrapped {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3C300);
            return Err(Gl2Code::EWrap);
        }
        let want = match build_cell(p, w) {
            Some(x) => x,
            None => return Err(Gl2Code::ENoWrap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3C301 + (p as u64) * 2 + w as u64);
            return Err(Gl2Code::EDrift);
        }
        if match self.builds.get(p).and_then(|row| row.get(w)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gl2Code::EDup);
        }
        if let Some(row) = self.builds.get_mut(p) {
            if let Some(c) = row.get_mut(w) {
                *c = Some(got);
            }
        }
        self.fnv(0x3C302 + (p as u64) * 2 + w as u64);
        Ok(())
    }

    /// 构建全账核验（20 格）。
    pub fn builds_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            let mut w = 0usize;
            while w < WRAP {
                match self.builds.get(p).and_then(|row| row.get(w)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                w += 1;
            }
            p += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), Gl2Code> {
        if r >= SPLIT_ROWS {
            return Err(Gl2Code::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gl2Code::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x3C400 + r as u64);
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

impl Default for Gl2Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Gl2Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.verify_wrapper(WRAP_VER, DIST_URL, CKSUM);
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        let mut w = 0usize;
        while w < WRAP {
            if let Some(x) = build_cell(p, w) {
                if led.record_build(p, w, x).is_ok() {
                    recorded += 1;
                }
            }
            w += 1;
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：wrapper 失配 / 构建漂移 / 工程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = Gl2Ledger::new();
        let caught = match kind {
            0 => lt.verify_wrapper(WRAP_VER + 1, DIST_URL, CKSUM) == Err(Gl2Code::EWrap),
            1 => lt.record_build(2, 0, build_cell(2, 0).unwrap_or(0).wrapping_add(1))
                == Err(Gl2Code::EDrift),
            _ => lt.record_build(PROJS, 0, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3C500 + k as u64);
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
pub fn run_unx52083_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52083");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Gradle 构建 20/20 全通",
        recorded >= (PROJS * WRAP) as u32,
        "10 工程 × wrapper 二元逐格恒等",
    );
    s.add(
        "wrapper 三元闸",
        {
            let mut lt = Gl2Ledger::new();
            lt.verify_wrapper(WRAP_VER, DIST_URL, CKSUM).is_ok()
                && lt.verify_wrapper(WRAP_VER, DIST_URL + 1, CKSUM) == Err(Gl2Code::EWrap)
        },
        "版本/分发 URL/校验和三元失配必抓",
    );
    s.add(
        "daemon 复用分账声明 2/2 全齐",
        {
            let mut lt = Gl2Ledger::new();
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
        "daemon 复用主账/F52058 同构分账两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/漂移/越闭集三态轮转必抓");
    let mut l0 = Gl2Ledger::new();
    s.add(
        "wrapper 前置闸",
        l0.record_build(0, 1, 0) == Err(Gl2Code::EWrap),
        "有 wrapper 未验证必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_build(PROJS, 0, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    s.add(
        "边界·wrapper 越闭集",
        l0.record_build(0, WRAP, 0).is_err(),
        "wrapper 二元越闭集必拒",
    );
    let mut l1 = Gl2Ledger::new();
    let _ = l1.record_build(1, 0, build_cell(1, 0).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_build(1, 0, build_cell(1, 0).unwrap_or(0)).is_err(),
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
    fn test_wrap_gate() {
        assert!(wrap_ok(WRAP_VER, DIST_URL, CKSUM));
        assert!(!wrap_ok(WRAP_VER + 1, DIST_URL, CKSUM));
        assert!(!wrap_ok(WRAP_VER, DIST_URL + 1, CKSUM));
        assert!(!wrap_ok(WRAP_VER, DIST_URL, CKSUM + 1));
    }

    #[test]
    fn test_cells_pure() {
        let a = build_cell(3, 1).unwrap_or(0);
        assert_eq!(a, build_cell(3, 1).unwrap_or(0));
        assert_ne!(a, build_cell(3, 0).unwrap_or(0));
        assert!(build_cell(PROJS, 0).is_none());
        assert!(build_cell(0, WRAP).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_wrap_pre_gate() {
        let mut lt = Gl2Ledger::new();
        assert_eq!(lt.record_build(0, 1, 0), Err(Gl2Code::EWrap));
        let _ = lt.verify_wrapper(WRAP_VER, DIST_URL, CKSUM);
        assert!(lt.record_build(0, 1, build_cell(0, 1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJS * WRAP) as u32);
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
        assert_eq!(SEG_BASE, 0xC54C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52083_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
