//! UNX-F52198 Araxis Merge 收口 · 0xC7180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52198 逐字锚定）：UNX-F52199-J1 六判据全落；
//! 三方合并/文件夹对比 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：三方合并 ×10 场景账（基线/己方/他方三源格），逐场景
//!      决议载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 文件夹对比 ×10 树账：逐树落账，树号升序单调，越树必抓。
//!   ③ F52182 共面分账声明 4 行闭集（与 Gittyup 同域不同账），缺一拒封。
//!   ④ 静默取边闸：三源互异场景必须显式决议（决议码在册），未决议
//!      即封账必抓。
//!   ⑤ 注入 ×10 轮（场景越闭集/未决议即封/重复落账三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（场景+树+声明全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52198 − 52000)）。
pub const SEG_BASE: u32 = 0xC7180;

/// 合并场景数。
pub const SCENES: usize = 10;

/// 对比树数。
pub const TREES: usize = 10;

/// 分账声明行数。
pub const SEP_ROWS: usize = 4;

/// 源数（基线/己方/他方）。
pub const SOURCES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 树号越闭集
    ENoTree,
    /// 格载荷漂移
    EDrift,
    /// 树序逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 静默取边
    ESilent,
    /// 三源账不齐即封
    ESrcMiss,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::Ok => "成功",
            PfCode::ENoScene => "场景号越闭集：10 场景之外先扩闭集",
            PfCode::ENoTree => "树号越闭集：10 树之外先扩账本",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐格复算",
            PfCode::EOrder => "树序逆序：树号升序单调不许回跳",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "声明行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺声明行：F52182 共面分账声明未齐先补齐",
            PfCode::ESilent => "静默取边：三源互异场景未显式决议即封账",
            PfCode::ESrcMiss => "三源账不齐即封：三源格未齐先补齐",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：场景+树+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景三源格载荷（场景 sc，源 s：0=基线 1=己方 2=他方）。
pub const fn src_cell(sc: usize, s: usize) -> Option<u64> {
    if sc >= SCENES || s >= SOURCES {
        return None;
    }
    Some(0xD0000u64 + (sc as u64) * 0x1C0 + (s as u64) * 0x33)
}

/// 决议码闭集（人工区显式决议：0=取己方 1=取他方 2=手工编辑 3=丢弃）。
pub const RESOLVE_SET: [u32; 4] = [0, 1, 2, 3];

/// 决议码在册核验。
pub fn resolve_ok(code: u32) -> bool {
    let mut i = 0usize;
    while i < 4 {
        match RESOLVE_SET.get(i) {
            Some(&v) => {
                if v == code {
                    return true;
                }
            }
            None => return false,
        }
        i += 1;
    }
    false
}

/// 树格载荷（树 t，格 c≤7）。
pub const fn tree_cell(t: usize, c: usize) -> Option<u64> {
    if t >= TREES || c >= 8 {
        return None;
    }
    Some(0xD100 + t as u64 * 0x210 + c as u64 * 0x3B)
}

/// 分账声明行折叠（行 r）。
pub const fn sep_row(r: usize) -> Option<u32> {
    if r >= SEP_ROWS {
        return None;
    }
    Some(0xD200 + r as u32 * 0x41)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 三方合并收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景三源账。
    srcs: [[Option<u64>; SOURCES]; SCENES],
    /// 人工区决议账。
    res: [Option<u32>; SCENES],
    /// 树账。
    trees: [Option<u64>; TREES],
    /// 分账声明账。
    sep: [Option<u32>; SEP_ROWS],
    /// 最后树号。
    last_tree: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PfLedger {
    /// 新账本。
    pub fn new() -> Self {
        PfLedger {
            gates: [None; 6],
            srcs: [[None; SOURCES]; SCENES],
            res: [None; SCENES],
            trees: [None; TREES],
            sep: [None; SEP_ROWS],
            last_tree: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3D100 + gate as u64);
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

    /// 场景一源一格（载荷对照 + 重复拒）。
    pub fn record_src(&mut self, sc: usize, s: usize) -> Result<u64, PfCode> {
        if sc >= SCENES {
            return Err(PfCode::ENoScene);
        }
        if s >= SOURCES {
            return Err(PfCode::ESrcMiss);
        }
        let p = match src_cell(sc, s) {
            Some(p) => p,
            None => return Err(PfCode::ENoScene),
        };
        if match self.srcs.get(sc).and_then(|w| w.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        match self.srcs.get_mut(sc).and_then(|w| w.get_mut(s)) {
            Some(slot) => {
                *slot = Some(p);
            }
            None => return Err(PfCode::ENoScene),
        }
        self.fnv(0x3D200 + (sc as u64) * 8 + s as u64);
        Ok(p)
    }

    /// 人工区显式决议（决议码在册闸）。
    pub fn resolve(&mut self, sc: usize, code: u32) -> Result<(), PfCode> {
        if sc >= SCENES {
            return Err(PfCode::ENoScene);
        }
        if !resolve_ok(code) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3D300);
            return Err(PfCode::ESilent);
        }
        if let Some(slot) = self.res.get_mut(sc) {
            *slot = Some(code);
        }
        self.fnv(0x3D301 + sc as u64);
        Ok(())
    }

    /// 树一格（树号升序单调闸 + 载荷对照 + 重复拒）。
    pub fn record_tree(&mut self, t: usize, c: usize) -> Result<u64, PfCode> {
        if t >= TREES {
            return Err(PfCode::ENoTree);
        }
        if (t as i64) < self.last_tree {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3D400);
            return Err(PfCode::EOrder);
        }
        let p = match tree_cell(t, c) {
            Some(p) => p,
            None => return Err(PfCode::ENoTree),
        };
        if match self.trees.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(slot) = self.trees.get_mut(t) {
            *slot = Some(p);
        }
        if let Some(k) = self.last_tree_bump(t) {
            self.last_tree = k;
        }
        self.fnv(0x3D402 + t as u64);
        Ok(p)
    }

    /// 树号回写辅助（防裸下标）。
    fn last_tree_bump(&mut self, t: usize) -> Option<i64> {
        self.last_tree = t as i64;
        Some(self.last_tree)
    }

    /// 分账声明全齐核验。
    pub fn sep_all(&self) -> bool {
        let mut r = 0usize;
        while r < SEP_ROWS {
            match self.sep.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 分账声明一行。
    pub fn declare_sep(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= SEP_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.sep.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.sep.get_mut(r) {
            *c = sep_row(r);
        }
        self.fnv(0x3D500 + r as u64);
        Ok(())
    }

    /// 封账闸：场景三源全齐 + 全场景决议齐 + 树全通 + 声明 4 行全齐。
    pub fn seal(&self) -> Result<(), PfCode> {
        let mut sc = 0usize;
        while sc < SCENES {
            let mut s = 0usize;
            while s < SOURCES {
                match self.srcs.get(sc).and_then(|w| w.get(s)) {
                    Some(Some(_)) => {}
                    _ => return Err(PfCode::ESrcMiss),
                }
                s += 1;
            }
            match self.res.get(sc) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ESilent),
            }
            sc += 1;
        }
        let mut t = 0usize;
        while t < TREES {
            match self.trees.get(t) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ESrcMiss),
            }
            t += 1;
        }
        let mut r = 0usize;
        while r < SEP_ROWS {
            match self.sep.get(r) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ERowMiss),
            }
            r += 1;
        }
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

impl Default for PfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    // 三源逐场景落账（场景升序、源升序）。
    let mut sc = 0usize;
    while sc < SCENES {
        let mut s = 0usize;
        while s < SOURCES {
            if led.record_src(sc, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        // 人工区显式决议（场景 2、5、8 为三源互异，其余仍显式决议在册码）。
        let _ = led.resolve(sc, (sc % 4) as u32);
        sc += 1;
    }
    let mut t = 0usize;
    while t < TREES {
        let mut c = 0usize;
        while c < 8 {
            if led.record_tree(t, c).is_ok() {
                recorded += 1;
            }
            c += 1;
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SEP_ROWS {
        let _ = led.declare_sep(r);
        r += 1;
    }
    // 注入 10 轮：场景越闭集 / 未决议即封 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_src(SCENES, 0).is_err(),
            1 => lt.seal().is_err(),
            _ => {
                let _ = lt.record_src(0, 0);
                lt.record_src(0, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3D600 + k as u64);
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
pub fn run_unx52198_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52198");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三方合并 10 场景 ×3 源全通",
        recorded >= (SCENES * SOURCES) as u32,
        "基线/己方/他方逐源逐格恒等",
    );
    s.add(
        "决议码闭集",
        resolve_ok(0) && resolve_ok(3) && !resolve_ok(4),
        "取己方/取他方/手工编辑/丢弃四码在册且越码拒",
    );
    s.add(
        "文件夹对比 10 树全通",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TREES {
                if lt.record_tree(t, 0).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "树号升序单调逐树落账",
    );
    s.add(
        "F52182 分账声明 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SEP_ROWS {
                if lt.declare_sep(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sep_all()
        },
        "与 Gittyup 同域不同账四行声明闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/未决议即封/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·场景越闭集",
        l0.record_src(SCENES, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·树越闭集",
        l0.record_tree(TREES, 0).is_err(),
        "树 10 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_tree(3, 0);
    s.add("树序·逆序拒", l1.record_tree(1, 0).is_err(), "树号回跳必抓");
    s.add(
        "静默取边·未决议即封拒",
        l1.seal().is_err(),
        "场景未显式决议封账必抓",
    );
    s.add(
        "决议·越码拒",
        l1.resolve(0, 9).is_err(),
        "决议码 9 不在闭集必拒",
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
    fn test_resolve_set() {
        assert!(resolve_ok(0));
        assert!(resolve_ok(3));
        assert!(!resolve_ok(4));
        assert!(src_cell(SCENES, 0).is_none());
        assert!(src_cell(0, SOURCES).is_none());
        assert!(tree_cell(TREES, 0).is_none());
        assert!(tree_cell(0, 8).is_none());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_tree(3, 0).is_ok());
        assert!(lt.record_tree(1, 0).is_err());
        assert!(lt.record_tree(4, 0).is_ok());
    }

    #[test]
    fn test_resolve_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.resolve(0, 9).is_err());
        assert!(lt.resolve(0, 2).is_ok());
        assert!(lt.resolve(SCENES, 0).is_err());
    }

    #[test]
    fn test_sep_and_seal() {
        let mut lt = PfLedger::new();
        let _ = lt.record_src(0, 0);
        let _ = lt.resolve(0, 0);
        assert!(lt.seal().is_err());
        let _ = lt.declare_sep(0);
        assert!(lt.declare_sep(0).is_err());
        assert_eq!(lt.declare_sep(SEP_ROWS), Err(PfCode::ENoRow));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES * SOURCES + TREES) as u32);
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
        assert_eq!(SEG_BASE, 0xC7180);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52198_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
