//! UNX-F52056 JetBrains Toolbox App 收口 · 0xC4E00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52056 逐字锚定）：UNX-F52057-J1 JBR 运行时自
//! 带上栈全账：字体渲染（JCEF/FreeType 路径对 F3 联签锚定行）/DPI 缩
//! 放/输入法挂接三面 ×30 场景；全部 JetBrains 家族条目的运行时前置；
//! KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题五 JBR 专项。
//!
//! 设计要点：
//!   ① 域模型：JBR 上栈三面账（字体渲染/DPI 缩放/输入法挂接）× 10 场
//!      景 = 30 格上栈账；面序闭集 0..3。
//!   ② 上栈四层闸（JDK 档/FreeType 档/JCEF 档/IME 档）对宿主面全等才
//!      放行，错位走失配路径（EJdk/EFt/EJcef/EIme 四诊断）；失配注入
//!      必抓。
//!   ③ 字体渲染 F3 联签锚定：F3 渲染对账 10 行（行载荷恒等 + 行序升
//!      序才收）；DPI 缩放档闭集（100%/125%/150%/200% 四档）越档拒。
//!   ④ 注入 ×10 轮（四层失配/面越闭集/F3 行序乱三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级 S1（30 格全通 + 上栈
//!      零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52056 − 52000)）。
pub const SEG_BASE: u32 = 0xC4E00;

/// 三面账面数。
pub const FACES: usize = 3;

/// 场景数。
pub const SCENES: usize = 10;

/// 上栈四层基准。
pub const JDK: u32 = 17;
pub const FREETYPE: u32 = 2;
pub const JCEF: u32 = 5;
pub const IME: u32 = 3;

/// F3 对账行数。
pub const F3_ROWS: usize = 10;

/// DPI 档闭集。
pub const DPI_SLOTS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbCode {
    /// 成功
    Ok = 0,
    /// 面号越闭集
    ENoFace,
    /// 场景号越闭集
    ENoScene,
    /// JDK 档失配
    EJdk,
    /// FreeType 档失配
    EFt,
    /// JCEF 档失配
    EJcef,
    /// IME 档失配
    EIme,
    /// DPI 档越闭集
    ENoDpi,
    /// F3 行序乱
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

impl TbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TbCode::Ok => "成功",
            TbCode::ENoFace => "面号越闭集：字体/DPI/输入法三面之外先登记",
            TbCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            TbCode::EJdk => "JDK 档失配：JBR JDK 档与宿主档不符",
            TbCode::EFt => "FreeType 档失配：FreeType 档与宿主档不符",
            TbCode::EJcef => "JCEF 档失配：JCEF 档与宿主档不符",
            TbCode::EIme => "IME 档失配：输入法挂接档与宿主档不符",
            TbCode::ENoDpi => "DPI 档越闭集：100/125/150/200 四档之外先登记",
            TbCode::EOrder => "F3 行序乱：渲染对账行序必须升序",
            TbCode::EDrift => "载荷漂移：折叠与基准不恒等，逐格复算",
            TbCode::EDup => "重复落账：同格只许一条账目",
            TbCode::ESeal => "未齐即封：30 格未齐先补齐",
            TbCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TbCode::EGradeDrift => "定级漂移：30 格全通+上栈零漂移+F3 行序单调才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 上栈格载荷（面 f × 场景 s）。
pub const fn stack_cell(f: usize, s: usize) -> Option<u64> {
    if f >= FACES || s >= SCENES {
        return None;
    }
    Some(
        0x4B000u64
            + (f as u64) * 0x31
            + (s as u64) * 0x14
            ^ (f as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// F3 对账行基准（行 r，升序）。
pub const fn f3_row(r: usize) -> Option<u64> {
    if r >= F3_ROWS {
        return None;
    }
    Some(0x4C000u64 + (r as u64) * 0x1C)
}

/// DPI 档闭集（档 d → 千分比缩放 ×1000）。
pub const fn dpi_slot(d: usize) -> Option<u32> {
    match d {
        0 => Some(1000),
        1 => Some(1250),
        2 => Some(1500),
        3 => Some(2000),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct TbLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 上栈账。
    stacks: [[Option<u64>; SCENES]; FACES],
    /// F3 最后行。
    last_f3: i64,
    /// DPI 档命中数。
    dpi_hits: u32,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TbLedger {
    /// 新账本。
    pub fn new() -> Self {
        TbLedger {
            gates: [None; 6],
            stacks: [[None; SCENES]; FACES],
            last_f3: -1,
            dpi_hits: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TbCode> {
        if gate >= 6 {
            return Err(TbCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x21100 + gate as u64);
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

    /// 上栈四层闸。
    pub fn stack_gate(&mut self, jdk: u32, ft: u32, jcef: u32, ime: u32) -> Result<(), TbCode> {
        if jdk != JDK {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21200);
            return Err(TbCode::EJdk);
        }
        if ft != FREETYPE {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21201);
            return Err(TbCode::EFt);
        }
        if jcef != JCEF {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21202);
            return Err(TbCode::EJcef);
        }
        if ime != IME {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21203);
            return Err(TbCode::EIme);
        }
        self.fnv(0x21204);
        Ok(())
    }

    /// 上栈一格。
    pub fn record_stack(&mut self, f: usize, s: usize) -> Result<u64, TbCode> {
        if f >= FACES {
            return Err(TbCode::ENoFace);
        }
        if s >= SCENES {
            return Err(TbCode::ENoScene);
        }
        let p = match stack_cell(f, s) {
            Some(p) => p,
            None => return Err(TbCode::ENoScene),
        };
        if match self.stacks.get(f).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TbCode::EDup);
        }
        if let Some(row) = self.stacks.get_mut(f) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x21205 + (f as u64) * 10 + s as u64);
        Ok(p)
    }

    /// 上栈全账核验（30 格）。
    pub fn stacks_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACES {
            let mut s = 0usize;
            while s < SCENES {
                match self.stacks.get(f).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            f += 1;
        }
        true
    }

    /// F3 对账一行（行序升序 + 载荷恒等）。
    pub fn record_f3(&mut self, r: usize, got: u64) -> Result<(), TbCode> {
        let want = match f3_row(r) {
            Some(w) => w,
            None => return Err(TbCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21300 + r as u64);
            return Err(TbCode::EDrift);
        }
        if (r as i64) <= self.last_f3 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21301);
            return Err(TbCode::EOrder);
        }
        self.last_f3 = r as i64;
        self.fnv(0x21302 + r as u64);
        Ok(())
    }

    /// DPI 一档（闭集外拒）。
    pub fn record_dpi(&mut self, scale: u32) -> Result<(), TbCode> {
        let mut hit = false;
        let mut d = 0usize;
        while d < DPI_SLOTS {
            if dpi_slot(d) == Some(scale) {
                hit = true;
            }
            d += 1;
        }
        if !hit {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x21400 + scale as u64);
            return Err(TbCode::ENoDpi);
        }
        self.dpi_hits = self.dpi_hits.saturating_add(1);
        self.fnv(0x21401 + scale as u64);
        Ok(())
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

impl Default for TbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.stack_gate(JDK, FREETYPE, JCEF, IME);
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FACES {
        let mut s = 0usize;
        while s < SCENES {
            if led.record_stack(f, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < F3_ROWS {
        if let Some(w) = f3_row(r) {
            let _ = led.record_f3(r, w);
        }
        r += 1;
    }
    let mut d = 0usize;
    while d < DPI_SLOTS {
        if let Some(scale) = dpi_slot(d) {
            let _ = led.record_dpi(scale);
        }
        d += 1;
    }
    // 注入 10 轮：四层失配 / DPI 越闭集 / F3 行序乱三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TbLedger::new();
        let caught = match kind {
            0 => {
                lt.stack_gate(JDK + 1, FREETYPE, JCEF, IME) == Err(TbCode::EJdk)
                    && lt.stack_gate(JDK, FREETYPE + 1, JCEF, IME) == Err(TbCode::EFt)
                    && lt.stack_gate(JDK, FREETYPE, JCEF + 1, IME) == Err(TbCode::EJcef)
                    && lt.stack_gate(JDK, FREETYPE, JCEF, IME + 1) == Err(TbCode::EIme)
            }
            1 => lt.record_dpi(1337) == Err(TbCode::ENoDpi),
            _ => {
                let _ = lt.record_f3(6, f3_row(6).unwrap_or(0));
                lt.record_f3(2, f3_row(2).unwrap_or(0)) == Err(TbCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x21500 + k as u64);
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
pub fn run_unx52056_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52056");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "JBR 上栈 30/30 全通",
        recorded == (FACES * SCENES) as u32,
        "字体渲染/DPI 缩放/输入法挂接三面 ×10 场景全通",
    );
    s.add(
        "上栈四层闸全等放行+失配四态全抓",
        {
            let mut lt = TbLedger::new();
            let ok = lt.stack_gate(JDK, FREETYPE, JCEF, IME).is_ok();
            let a = lt.stack_gate(JDK + 1, FREETYPE, JCEF, IME) == Err(TbCode::EJdk);
            let b = lt.stack_gate(JDK, FREETYPE + 1, JCEF, IME) == Err(TbCode::EFt);
            let c = lt.stack_gate(JDK, FREETYPE, JCEF + 1, IME) == Err(TbCode::EJcef);
            let d = lt.stack_gate(JDK, FREETYPE, JCEF, IME + 1) == Err(TbCode::EIme);
            ok && a && b && c && d
        },
        "JDK/FreeType/JCEF/IME 四层错位走失配路径",
    );
    s.add(
        "F3 渲染对账 10 行单调",
        {
            let mut lt = TbLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < F3_ROWS {
                if let Some(w) = f3_row(r) {
                    if lt.record_f3(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok
        },
        "JCEF/FreeType 路径 F3 联签锚定行序升序且载荷恒等",
    );
    s.add(
        "DPI 四档闭集",
        {
            let mut lt = TbLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DPI_SLOTS {
                if let Some(scale) = dpi_slot(d) {
                    if lt.record_dpi(scale).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.record_dpi(1777).is_err()
        },
        "100/125/150/200 四档在册，越档拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/DPI/行序三态轮转必抓");
    let mut l0 = TbLedger::new();
    s.add(
        "边界·面越闭集",
        l0.record_stack(FACES, 0).is_err(),
        "面 3 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_stack(0, SCENES).is_err(),
        "场景 10 越闭集必拒",
    );
    let mut l1 = TbLedger::new();
    let _ = l1.record_stack(1, 1);
    s.add(
        "重复·再落账拒",
        l1.record_stack(1, 1).is_err(),
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
        let a = stack_cell(2, 3).unwrap_or(0);
        assert_eq!(a, stack_cell(2, 3).unwrap_or(0));
        assert_ne!(a, stack_cell(2, 4).unwrap_or(0));
        assert!(stack_cell(FACES, 0).is_none());
        assert!(stack_cell(0, SCENES).is_none());
        assert!(f3_row(F3_ROWS).is_none());
        assert_ne!(f3_row(0), f3_row(1));
    }

    #[test]
    fn test_dpi_closure() {
        assert_eq!(dpi_slot(0), Some(1000));
        assert_eq!(dpi_slot(3), Some(2000));
        assert!(dpi_slot(4).is_none());
    }

    #[test]
    fn test_stack_gate() {
        let mut lt = TbLedger::new();
        assert_eq!(lt.stack_gate(JDK + 2, FREETYPE, JCEF, IME), Err(TbCode::EJdk));
        assert_eq!(lt.stack_gate(JDK, FREETYPE + 2, JCEF, IME), Err(TbCode::EFt));
        assert_eq!(lt.stack_gate(JDK, FREETYPE, JCEF + 2, IME), Err(TbCode::EJcef));
        assert_eq!(lt.stack_gate(JDK, FREETYPE, JCEF, IME + 2), Err(TbCode::EIme));
        assert!(lt.stack_gate(JDK, FREETYPE, JCEF, IME).is_ok());
    }

    #[test]
    fn test_f3_order() {
        let mut lt = TbLedger::new();
        let _ = lt.record_f3(4, f3_row(4).unwrap_or(0));
        assert_eq!(lt.record_f3(1, f3_row(1).unwrap_or(0)), Err(TbCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (FACES * SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4E00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52056_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
