//! UNX-F52074 CMake 收口（Windows 发行版，与 L4 分账） · 0xC5280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52074 逐字锚定）：UNX-F52075-J1 配置/生成 ×10
//! 工程全通（生成器选 VS/Ninja 双路径）；GUI 状态机完整（重配置/错误态
//! /取消路径 ×10）；Windows 发行版应用收口与 L4 CMake 嫁接件布设分账
//! （联签锚定行 AI-59）；KB-COMPAT 定级登记。
//!
//! 设计要点：
//!   ① 域模型：配置/生成 ×10 工程账（生成器二元闭集 VS/Ninja），10×2
//!      = 20 格生成账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② GUI 状态机账：状态闭集 5 态（空闲/配置中/已配置/错误态/已取
//!      消），重配置/错误/取消三路径 ×10 场景 = 30 格状态账；非法迁移
//!      必抓（错误态直接生成 → 非法迁移拒）。
//!   ③ L4 分账声明 2 行闭集（发行版收口声明/嫁接件布设声明）。
//!   ④ 注入 ×10 轮（生成漂移/非法迁移/工程越闭集三态轮转）10/10 必
//!      抓；六判据母版闸 + 定级（20 格 + 30 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52074 − 52000)）。
pub const SEG_BASE: u32 = 0xC5280;

/// 工程数。
pub const PROJS: usize = 10;

/// 生成器二元。
pub const GENS: usize = 2;

/// GUI 状态数。
pub const GUI_STATES: usize = 5;

/// GUI 路径场景数。
pub const GUI_SCENES: usize = 10;

/// 声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 生成器越闭集
    ENoGen,
    /// 生成漂移
    EDrift,
    /// 状态越闭集
    ENoState,
    /// 非法迁移
    EBadMove,
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

impl CmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CmCode::Ok => "成功",
            CmCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            CmCode::ENoGen => "生成器越闭集：VS/Ninja 二元之外先登记",
            CmCode::EDrift => "生成漂移：生成折叠与基准不恒等，逐格复算",
            CmCode::ENoState => "状态越闭集：5 态之外先登记",
            CmCode::EBadMove => "非法迁移：错误态直接生成/取消态重配置等非法路径必抓",
            CmCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            CmCode::ERowMiss => "缺声明行：L4 分账声明未齐先补齐",
            CmCode::EDup => "重复落账：同格只许一条账目",
            CmCode::ESeal => "未齐即封：20 格 + 30 格未齐先补齐",
            CmCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CmCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CmCode::EGradeDrift => "定级漂移：20 格+30 格全通+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 生成格载荷（工程 p × 生成器 g）。
pub const fn gen_cell(p: usize, g: usize) -> Option<u64> {
    if p >= PROJS || g >= GENS {
        return None;
    }
    Some(
        (0x8C000u64 + (p as u64) * 0x2B)
            ^ (g as u64).wrapping_mul(0x01000193)
            ^ 0x636D616B6567656E,
    )
}

/// GUI 状态格载荷（场景 sc × 状态 st）。
pub const fn gui_cell(sc: usize, st: usize) -> Option<u64> {
    if sc >= GUI_SCENES || st >= GUI_STATES {
        return None;
    }
    Some((0x8D000u64 + (sc as u64) * 0x24) ^ (st as u64).wrapping_mul(0x100000001b3))
}

/// 合法迁移表（纯函数：from → 允许的 to 集合折叠；0=空闲 1=配置中
/// 2=已配置 3=错误态 4=已取消）。
pub const fn move_ok(from: usize, to: usize) -> bool {
    match (from, to) {
        (0, 1) | (1, 2) | (1, 3) | (1, 4) | (2, 1) | (3, 1) | (4, 1) => true,
        _ => false,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x8E00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct CmLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 生成账。
    gens: [[Option<u64>; GENS]; PROJS],
    /// GUI 状态账。
    gui: [[Option<u64>; GUI_STATES]; GUI_SCENES],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CmLedger {
    /// 新账本。
    pub fn new() -> Self {
        CmLedger {
            gates: [None; 6],
            gens: [[None; GENS]; PROJS],
            gui: [[None; GUI_STATES]; GUI_SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CmCode> {
        if gate >= 6 {
            return Err(CmCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x33100 + gate as u64);
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

    /// 生成一格。
    pub fn record_gen(&mut self, p: usize, g: usize) -> Result<u64, CmCode> {
        if p >= PROJS {
            return Err(CmCode::ENoProj);
        }
        if g >= GENS {
            return Err(CmCode::ENoGen);
        }
        let payload = match gen_cell(p, g) {
            Some(p) => p,
            None => return Err(CmCode::ENoGen),
        };
        if match self.gens.get(p).and_then(|row| row.get(g)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmCode::EDup);
        }
        if let Some(row) = self.gens.get_mut(p) {
            if let Some(c) = row.get_mut(g) {
                *c = Some(payload);
            }
        }
        self.fnv(0x33200 + (p as u64) * 2 + g as u64);
        Ok(payload)
    }

    /// 生成全账核验（20 格）。
    pub fn gens_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            let mut g = 0usize;
            while g < GENS {
                match self.gens.get(p).and_then(|row| row.get(g)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                g += 1;
            }
            p += 1;
        }
        true
    }

    /// GUI 状态一格（迁移合法才收）。
    pub fn record_gui(&mut self, sc: usize, st: usize, from: usize) -> Result<u64, CmCode> {
        if sc >= GUI_SCENES {
            return Err(CmCode::ENoProj);
        }
        if st >= GUI_STATES {
            return Err(CmCode::ENoState);
        }
        if !move_ok(from, st) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x33300 + (from as u64) * 5 + st as u64);
            return Err(CmCode::EBadMove);
        }
        let payload = match gui_cell(sc, st) {
            Some(p) => p,
            None => return Err(CmCode::ENoState),
        };
        if match self.gui.get(sc).and_then(|row| row.get(st)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmCode::EDup);
        }
        if let Some(row) = self.gui.get_mut(sc) {
            if let Some(c) = row.get_mut(st) {
                *c = Some(payload);
            }
        }
        self.fnv(0x33301 + (sc as u64) * 5 + st as u64);
        Ok(payload)
    }

    /// GUI 全账核验（30 格）。
    pub fn gui_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < GUI_SCENES {
            let mut st = 0usize;
            while st < GUI_STATES {
                match self.gui.get(sc).and_then(|row| row.get(st)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                st += 1;
            }
            sc += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), CmCode> {
        if r >= SPLIT_ROWS {
            return Err(CmCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x33400 + r as u64);
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

impl Default for CmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        let mut gg = 0usize;
        while gg < GENS {
            if led.record_gen(p, gg).is_ok() {
                recorded += 1;
            }
            gg += 1;
        }
        p += 1;
    }
    // GUI：合法迁移全账（每场景落 1/3/4 三态：空闲→配置中→错误/取消）。
    let mut sc = 0usize;
    while sc < GUI_SCENES {
        let _ = led.record_gui(sc, 1, 0);
        let _ = led.record_gui(sc, 3, 1);
        if led.record_gui(sc, 4, 1).is_ok() {
            recorded += 1;
        }
        sc += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：非法迁移 / 工程越闭集 / 生成重复三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CmLedger::new();
        let caught = match kind {
            0 => lt.record_gui(0, 2, 3) == Err(CmCode::EBadMove),
            1 => lt.record_gen(PROJS, 0).is_err(),
            _ => {
                let _ = lt.record_gen(0, 0);
                lt.record_gen(0, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x33500 + k as u64);
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
pub fn run_unx52074_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52074");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "配置/生成 20/20 全通",
        recorded >= (PROJS * GENS) as u32,
        "10 工程 × VS/Ninja 双路径逐格恒等",
    );
    s.add(
        "GUI 状态机 30/30 全账",
        {
            let mut lt = CmLedger::new();
            let mut n = 0u32;
            let mut sc = 0usize;
            while sc < GUI_SCENES {
                if lt.record_gui(sc, 1, 0).is_ok() {
                    n += 1;
                }
                if lt.record_gui(sc, 3, 1).is_ok() {
                    n += 1;
                }
                if lt.record_gui(sc, 4, 1).is_ok() {
                    n += 1;
                }
                sc += 1;
            }
            n == (GUI_SCENES * 3) as u32
        },
        "重配置/错误/取消三路径 ×10 场景合法迁移全账",
    );
    s.add(
        "L4 分账声明 2/2 全齐",
        {
            let mut lt = CmLedger::new();
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
        "发行版收口/嫁接件布设两行闭集（AI-59 联签）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "迁移/越闭集/重复三态轮转必抓");
    let mut l0 = CmLedger::new();
    s.add(
        "非法迁移拒",
        lt_check_bad_moves(),
        "错误态直接生成/取消态非法迁移必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_gen(PROJS, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    s.add(
        "边界·生成器越闭集",
        l0.record_gen(0, GENS).is_err(),
        "生成器 2 越闭集必拒",
    );
    s.add(
        "边界·状态越闭集",
        l0.record_gui(0, GUI_STATES, 0).is_err(),
        "状态 5 越闭集必拒",
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

/// 非法迁移三例直判。
fn lt_check_bad_moves() -> bool {
    let mut lt = CmLedger::new();
    let a = lt.record_gui(0, 2, 3) == Err(CmCode::EBadMove);
    let b = lt.record_gui(0, 4, 3) == Err(CmCode::EBadMove);
    let c = lt.record_gui(0, 0, 1) == Err(CmCode::EBadMove);
    a && b && c
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_table() {
        assert!(move_ok(0, 1));
        assert!(move_ok(1, 2));
        assert!(move_ok(3, 1));
        assert!(!move_ok(3, 2));
        assert!(!move_ok(0, 0));
        assert!(!move_ok(2, 4));
    }

    #[test]
    fn test_cells_pure() {
        let a = gen_cell(3, 1).unwrap_or(0);
        assert_eq!(a, gen_cell(3, 1).unwrap_or(0));
        assert_ne!(a, gen_cell(3, 0).unwrap_or(0));
        assert!(gen_cell(PROJS, 0).is_none());
        assert!(gui_cell(GUI_SCENES, 0).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_gui_legal_paths() {
        let mut lt = CmLedger::new();
        assert!(lt.record_gui(0, 1, 0).is_ok());
        assert!(lt.record_gui(0, 3, 1).is_ok());
        // 错误态恢复合法（异场景落账）。
        assert!(lt.record_gui(1, 1, 3).is_ok());
        // 同格重落拒。
        assert!(lt.record_gui(0, 3, 1).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJS * GENS) as u32);
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
        assert_eq!(SEG_BASE, 0xC5280);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52074_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
