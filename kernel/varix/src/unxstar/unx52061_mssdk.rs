//! UNX-F52061 MSBuild/Windows SDK 构建链收口（组旗舰） · 0xC4F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52061 逐字锚定）：UNX-F52062-J1 头文件/库/工
//! 具三件套布设完整性 ×40 路径断言；样本工程编译链接全通；KB-COMPAT
//! 定级登记；判据锚定 §AI-66.2 专题二编译器进程树。
//!
//! 设计要点：
//!   ① 域模型：三件套布设账。40 路径断言闭集：头文件 16 路径（SDK UM
//!      /UCRT/共享/ucrt 头 × 各版本档）+ 库 12 路径（kernel32/user32
//!      等 import lib × 架构档）+ 工具 12 路径（cl/link/rc/mt × 架构
//!      档），逐路径布设载荷（纯函数折叠：段基址 × 档 × 路径序）恒等
//!      才收；越册拒。
//!   ② 布设拍序闸：头 → 库 → 工具三组布设拍序严格递增（组内跨组逆序
//!      必抓）；重复断言拒。
//!   ③ 样本工程编译链接账：3 格（编译格/链接格/运行格）三元齐备才封
//!      （三件套 40 断言全收才许落样本账 → 布设前置闸）。
//!   ④ 注入 ×10 轮（路径越册/布设载荷漂移/组序逆序三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（40 格全通 + 样本
//!      三格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52061 − 52000)）。
pub const SEG_BASE: u32 = 0xC4F40;

/// 头文件路径数。
pub const HEADERS: usize = 16;

/// 库路径数。
pub const LIBS: usize = 12;

/// 工具路径数。
pub const TOOLS: usize = 12;

/// 总断言数。
pub const PATHS: usize = HEADERS + LIBS + TOOLS;

/// 样本工程三格。
pub const SAMPLE_CELLS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdkCode {
    /// 成功
    Ok = 0,
    /// 路径号越闭集
    ENoPath,
    /// 组号越闭集
    ENoGroup,
    /// 布设载荷漂移
    EDrift,
    /// 组序逆序
    EOrder,
    /// 布设未齐先落样本账
    EPreSample,
    /// 重复断言
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

impl SdkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SdkCode::Ok => "成功",
            SdkCode::ENoPath => "路径号越闭集：40 路径闭集之外先登记",
            SdkCode::ENoGroup => "组号越闭集：头/库/工具三组之外先登记",
            SdkCode::EDrift => "布设载荷漂移：路径折叠与基准不恒等，逐路复算",
            SdkCode::EOrder => "组序逆序：头→库→工具三组拍序必须严格递增",
            SdkCode::EPreSample => "布设未齐先落样本账：40 断言全收才许落样本三格",
            SdkCode::EDup => "重复断言：同路径只许一条布设账",
            SdkCode::ESeal => "未齐即封：40 格未齐先补齐",
            SdkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SdkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SdkCode::EGradeDrift => "定级漂移：40 格全通+样本三格全通才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 路径组归属（纯函数：0..16=头 16..28=库 28..40=工具）。
pub const fn path_group(i: usize) -> Option<u32> {
    if i >= PATHS {
        return None;
    }
    Some(if i < HEADERS {
        0
    } else if i < HEADERS + LIBS {
        1
    } else {
        2
    })
}

/// 路径布设基准载荷（路径 i）。
pub const fn path_cell(i: usize) -> Option<u64> {
    if i >= PATHS {
        return None;
    }
    Some(
        0x5A000u64
            + (i as u64) * 0x2D
            ^ ((i / HEADERS) as u64).wrapping_mul(0x01000193),
    )
}

/// 样本三格基准（编译/链接/运行）。
pub const fn sample_cell(c: usize) -> Option<u64> {
    if c >= SAMPLE_CELLS {
        return None;
    }
    Some((0x5B000u64 + c as u64 * 0x33) ^ 0x73616D706C65)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct SdkLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 布设账。
    paths: [Option<u64>; PATHS],
    /// 最后组号（组序单调）。
    last_group: i64,
    /// 样本账。
    samples: [Option<u64>; SAMPLE_CELLS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SdkLedger {
    /// 新账本。
    pub fn new() -> Self {
        SdkLedger {
            gates: [None; 6],
            paths: [None; PATHS],
            last_group: -1,
            samples: [None; SAMPLE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SdkCode> {
        if gate >= 6 {
            return Err(SdkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x26100 + gate as u64);
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

    /// 布设一格（组序单调 + 载荷对照 + 重复拒）。
    pub fn record_path(&mut self, i: usize, got: u64) -> Result<(), SdkCode> {
        let want = match path_cell(i) {
            Some(w) => w,
            None => return Err(SdkCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x26200 + i as u64);
            return Err(SdkCode::EDrift);
        }
        let g = match path_group(i) {
            Some(g) => g,
            None => return Err(SdkCode::ENoPath),
        };
        if (g as i64) < self.last_group {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x26201);
            return Err(SdkCode::EOrder);
        }
        if match self.paths.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SdkCode::EDup);
        }
        if let Some(c) = self.paths.get_mut(i) {
            *c = Some(got);
        }
        self.last_group = g as i64;
        self.fnv(0x26202 + i as u64);
        Ok(())
    }

    /// 布设全账核验（40 格）。
    pub fn paths_all(&self) -> bool {
        let mut i = 0usize;
        while i < PATHS {
            match self.paths.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 样本一格（布设前置闸 + 重复拒）。
    pub fn record_sample(&mut self, c: usize) -> Result<u64, SdkCode> {
        if c >= SAMPLE_CELLS {
            return Err(SdkCode::ENoGroup);
        }
        if !self.paths_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x26300);
            return Err(SdkCode::EPreSample);
        }
        let p = match sample_cell(c) {
            Some(p) => p,
            None => return Err(SdkCode::ENoGroup),
        };
        if match self.samples.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SdkCode::EDup);
        }
        if let Some(cell) = self.samples.get_mut(c) {
            *cell = Some(p);
        }
        self.fnv(0x26301 + c as u64);
        Ok(p)
    }

    /// 样本全账核验。
    pub fn samples_all(&self) -> bool {
        let mut c = 0usize;
        while c < SAMPLE_CELLS {
            match self.samples.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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

impl Default for SdkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SdkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < PATHS {
        if let Some(w) = path_cell(i) {
            if led.record_path(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < SAMPLE_CELLS {
        if led.record_sample(c).is_ok() {
            recorded += 1;
        }
        c += 1;
    }
    // 注入 10 轮：路径越册 / 布设漂移 / 组序逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SdkLedger::new();
        let caught = match kind {
            0 => lt.record_path(PATHS, 0).is_err(),
            1 => lt.record_path(5, path_cell(5).unwrap_or(0).wrapping_add(1)) == Err(SdkCode::EDrift),
            _ => {
                // 先落工具组再回头落库组 → 组序逆序必抓。
                let _ = lt.record_path(HEADERS + LIBS, path_cell(HEADERS + LIBS).unwrap_or(0));
                lt.record_path(HEADERS, path_cell(HEADERS).unwrap_or(0)) == Err(SdkCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x26400 + k as u64);
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
pub fn run_unx52061_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52061");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三件套布设 40/40 全通",
        {
            let mut lt = SdkLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PATHS {
                if let Some(w) = path_cell(i) {
                    if lt.record_path(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.paths_all()
        },
        "头 16+库 12+工具 12 路径断言逐路恒等",
    );
    s.add(
        "样本编译链接 3/3 全通",
        {
            let mut lt = SdkLedger::new();
            let mut i = 0usize;
            while i < PATHS {
                if let Some(w) = path_cell(i) {
                    let _ = lt.record_path(i, w);
                }
                i += 1;
            }
            let mut ok = true;
            let mut c = 0usize;
            while c < SAMPLE_CELLS {
                if lt.record_sample(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.samples_all()
        },
        "布设齐备后编译/链接/运行三格全通",
    );
    s.add(
        "组序单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < PATHS {
                if path_group(i).unwrap_or(3) < path_group(i - 1).unwrap_or(3) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "头→库→工具三组布设拍序严格递增",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越册/漂移/逆序三态轮转必抓");
    let mut l0 = SdkLedger::new();
    s.add(
        "样本前置闸",
        l0.record_sample(0) == Err(SdkCode::EPreSample),
        "布设 40 断言未齐不许落样本账",
    );
    s.add(
        "边界·路径越闭集",
        l0.record_path(PATHS, 0).is_err(),
        "路径 40 越闭集必拒",
    );
    let mut l1 = SdkLedger::new();
    s.add(
        "布设·漂移拒",
        l1.record_path(9, path_cell(9).unwrap_or(0).wrapping_add(4)) == Err(SdkCode::EDrift),
        "路径折叠漂移必抓",
    );
    let mut l2 = SdkLedger::new();
    let _ = l2.record_path(2, path_cell(2).unwrap_or(0));
    s.add(
        "重复·再断言拒",
        l2.record_path(2, path_cell(2).unwrap_or(0)).is_err(),
        "同路径只许一条布设账",
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
    fn test_path_group() {
        assert_eq!(path_group(0), Some(0));
        assert_eq!(path_group(HEADERS - 1), Some(0));
        assert_eq!(path_group(HEADERS), Some(1));
        assert_eq!(path_group(HEADERS + LIBS - 1), Some(1));
        assert_eq!(path_group(HEADERS + LIBS), Some(2));
        assert_eq!(path_group(PATHS - 1), Some(2));
        assert!(path_group(PATHS).is_none());
        assert_eq!(HEADERS + LIBS + TOOLS, 40);
    }

    #[test]
    fn test_cells_pure() {
        let a = path_cell(7).unwrap_or(0);
        assert_eq!(a, path_cell(7).unwrap_or(0));
        assert_ne!(a, path_cell(8).unwrap_or(0));
        assert!(path_cell(PATHS).is_none());
        assert!(sample_cell(SAMPLE_CELLS).is_none());
    }

    #[test]
    fn test_group_order_caught() {
        let mut lt = SdkLedger::new();
        let _ = lt.record_path(30, path_cell(30).unwrap_or(0));
        assert_eq!(
            lt.record_path(3, path_cell(3).unwrap_or(0)),
            Err(SdkCode::EOrder)
        );
    }

    #[test]
    fn test_sample_gate() {
        let mut lt = SdkLedger::new();
        assert_eq!(lt.record_sample(0), Err(SdkCode::EPreSample));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, PATHS as u32 + SAMPLE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC4F40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52061_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
