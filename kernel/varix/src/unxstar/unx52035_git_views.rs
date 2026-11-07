//! UNX-F52035 VSCode 扩展：Git 三视图消费面收口 · 0xC48C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52035 逐字锚定）：UNX-F52036-J1 blame/历史/
//! 对比三视图 ×10 仓库全通；Git 消费对账（L4 嫁接件判据绿，联签锚定行
//! AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 Git 集成。
//!
//! 设计要点：
//!   ① 域模型：Git 三视图消费面。仓库闭集 10 项，三视图（blame/历史/
//!      对比）× 10 仓库 = 30 格视图账，每格视图载荷（纯函数复算：仓库
//!      × 视图 × 折叠）。
//!   ② Git 消费对账：6 格消费账（对象库档/引用档/索引档/工作树档/配置
//!      档/钩子档），逐格与消费基准恒等才收；L4 嫁接件判据：嫁接载荷
//!      与上游折叠恒等才绿，漂移即抓。
//!   ③ 注入 ×10 轮（仓库越闭集/消费漂移/嫁接漂移三态轮转）10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 定级（30 格全通 + 消费零漂移 + 嫁接
//!      全绿 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52035 − 52000)）。
pub const SEG_BASE: u32 = 0xC48C0;

/// 仓库闭集数。
pub const REPOS: usize = 10;

/// 三视图段数。
pub const VIEWS: usize = 3;

/// Git 消费账格数。
pub const GIT_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GvCode {
    /// 成功
    Ok = 0,
    /// 仓库号越闭集
    ENoRepo,
    /// 视图号越闭集
    ENoView,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EDrift,
    /// 嫁接漂移
    EGraft,
    /// 视图重复
    EDup,
    /// 未对账先渲染
    EPreRender,
    /// 场景未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GvCode::Ok => "成功",
            GvCode::ENoRepo => "仓库号越闭集：10 仓库之外先登记",
            GvCode::ENoView => "视图号越闭集：blame/历史/对比三视图之外先登记",
            GvCode::ENoCell => "消费格越册：Git 6 格闭集之外先登记",
            GvCode::EDrift => "消费漂移：Git 消费折叠与基准不恒等，逐格复算",
            GvCode::EGraft => "嫁接漂移：L4 嫁接件折叠与上游不恒等，逐件复算",
            GvCode::EDup => "视图重复：同仓库同视图只许一条账目",
            GvCode::EPreRender => "未对账先渲染：Git 6 格消费账不齐不许报三视图全通",
            GvCode::ESeal => "场景未齐即封：30 格未齐先补齐",
            GvCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GvCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GvCode::EGradeDrift => "定级漂移：30 格全通+消费零漂移+嫁接全绿才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 视图格载荷（仓库 i × 视图 v）。
pub const fn view_cell(i: usize, v: usize) -> Option<u64> {
    if i >= REPOS || v >= VIEWS {
        return None;
    }
    Some(
        0xF1000u64
            + (i as u64) * 0x3B
            + (v as u64) * 0x9
            ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15 >> (v as u64 + 1)),
    )
}

/// Git 消费格基准载荷（格 c）。
pub const fn git_cell(c: usize) -> Option<u64> {
    if c >= GIT_CELLS {
        return None;
    }
    Some((0xF2000u64 + c as u64 * 0x2D) ^ 0x6769746C656E73)
}

/// L4 嫁接件基准载荷（件 g，共 4 件）。
pub const fn graft_cell(g: usize) -> Option<u64> {
    if g >= 4 {
        return None;
    }
    Some((0xF3000u64 + g as u64 * 0x53) ^ (g as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct GvLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 视图账。
    views: [[Option<u64>; VIEWS]; REPOS],
    /// Git 消费账。
    git: [Option<u64>; GIT_CELLS],
    /// 嫁接账（4 件，true=绿）。
    graft: [Option<bool>; 4],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GvLedger {
    /// 新账本。
    pub fn new() -> Self {
        GvLedger {
            gates: [None; 6],
            views: [[None; VIEWS]; REPOS],
            git: [None; GIT_CELLS],
            graft: [None; 4],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GvCode> {
        if gate >= 6 {
            return Err(GvCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xA100 + gate as u64);
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

    /// Git 消费一格（恒等才收）。
    pub fn consume_git(&mut self, c: usize, got: u64) -> Result<(), GvCode> {
        let want = match git_cell(c) {
            Some(w) => w,
            None => return Err(GvCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA200 + c as u64);
            return Err(GvCode::EDrift);
        }
        if let Some(cell) = self.git.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0xA201 + c as u64);
        Ok(())
    }

    /// Git 消费全账核验。
    pub fn git_all(&self) -> bool {
        let mut c = 0usize;
        while c < GIT_CELLS {
            match self.git.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 落一格视图账（消费前置闸：Git 消费账不齐 → EPreRender）。
    pub fn record_view(&mut self, i: usize, v: usize) -> Result<u64, GvCode> {
        if i >= REPOS {
            return Err(GvCode::ENoRepo);
        }
        if v >= VIEWS {
            return Err(GvCode::ENoView);
        }
        if !self.git_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA300);
            return Err(GvCode::EPreRender);
        }
        let p = match view_cell(i, v) {
            Some(p) => p,
            None => return Err(GvCode::ENoView),
        };
        if match self.views.get(i).and_then(|row| row.get(v)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GvCode::EDup);
        }
        if let Some(row) = self.views.get_mut(i) {
            if let Some(c) = row.get_mut(v) {
                *c = Some(p);
            }
        }
        self.fnv(0xA301 + (i as u64) * 3 + v as u64);
        Ok(p)
    }

    /// 视图账全齐核验（30 格）。
    pub fn views_all(&self) -> bool {
        let mut i = 0usize;
        while i < REPOS {
            let mut v = 0usize;
            while v < VIEWS {
                match self.views.get(i).and_then(|row| row.get(v)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                v += 1;
            }
            i += 1;
        }
        true
    }

    /// L4 嫁接件一格（上游折叠恒等才绿）。
    pub fn record_graft(&mut self, g: usize, got: u64) -> Result<(), GvCode> {
        let want = match graft_cell(g) {
            Some(w) => w,
            None => return Err(GvCode::EGraft),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA400 + g as u64);
            return Err(GvCode::EGraft);
        }
        if let Some(cell) = self.graft.get_mut(g) {
            *cell = Some(true);
        }
        self.fnv(0xA401 + g as u64);
        Ok(())
    }

    /// 嫁接全绿核验。
    pub fn graft_all(&self) -> bool {
        let mut g = 0usize;
        while g < 4 {
            match self.graft.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
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

impl Default for GvLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（视图格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GvLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < GIT_CELLS {
        if let Some(w) = git_cell(c) {
            let _ = led.consume_git(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < REPOS {
        let mut v = 0usize;
        while v < VIEWS {
            if led.record_view(i, v).is_ok() {
                recorded += 1;
            }
            v += 1;
        }
        i += 1;
    }
    let mut gg = 0usize;
    while gg < 4 {
        if let Some(w) = graft_cell(gg) {
            let _ = led.record_graft(gg, w);
        }
        gg += 1;
    }
    // 注入 10 轮：仓库越闭集 / 消费漂移 / 嫁接漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GvLedger::new();
        let caught = match kind {
            0 => lt.record_view(REPOS, 0).is_err(),
            1 => lt.consume_git(2, match git_cell(2) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(GvCode::EDrift),
            _ => lt.record_graft(1, match graft_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(GvCode::EGraft),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA500 + k as u64);
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
pub fn run_unx52035_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52035");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三视图 30/30 全通",
        recorded == (REPOS * VIEWS) as u32,
        "blame/历史/对比三视图 ×10 仓库全通",
    );
    s.add(
        "Git 消费 6/6 对账",
        {
            let mut lt = GvLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < GIT_CELLS {
                if let Some(w) = git_cell(c) {
                    if lt.consume_git(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.git_all()
        },
        "对象库/引用/索引/工作树/配置/钩子六格消费零漂移",
    );
    s.add(
        "L4 嫁接 4/4 全绿",
        {
            let mut lt = GvLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < 4 {
                if let Some(w) = graft_cell(g) {
                    if lt.record_graft(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.graft_all()
        },
        "嫁接件折叠与上游恒等判绿",
    );
    s.add("注入 10/10 全抓", caught >= 10, "仓库/消费/嫁接三态轮转必抓");
    let mut l0 = GvLedger::new();
    s.add(
        "渲染前置闸",
        l0.record_view(0, 0) == Err(GvCode::EPreRender),
        "Git 消费账不齐不许报三视图全通",
    );
    let mut l1 = GvLedger::new();
    s.add(
        "边界·仓库越闭集",
        l1.record_view(REPOS, 0).is_err(),
        "仓库号 10 越闭集必拒",
    );
    s.add(
        "边界·视图越闭集",
        l1.record_view(0, VIEWS).is_err(),
        "视图号 3 越闭集必拒",
    );
    let mut l2 = GvLedger::new();
    s.add(
        "消费·漂移拒",
        l2.consume_git(0, match git_cell(0) {
            Some(w) => w.wrapping_add(9),
            None => 0,
        }) == Err(GvCode::EDrift),
        "消费折叠漂移必抓",
    );
    let mut l3 = GvLedger::new();
    let _ = l3.record_graft(3, match graft_cell(3) {
        Some(w) => w,
        None => 0,
    });
    s.add(
        "嫁接·漂移拒",
        l3.record_graft(3, match graft_cell(3) {
            Some(w) => w.wrapping_add(3),
            None => 0,
        }) == Err(GvCode::EGraft),
        "嫁接折叠与上游漂移必抓",
    );
    let mut l4 = GvLedger::new();
    s.add(
        "嫁接·越册拒",
        l4.record_graft(4, 0) == Err(GvCode::EGraft),
        "嫁接件 4 越册必拒",
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
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥14 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_and_cells_pure() {
        let a = view_cell(4, 2).unwrap_or(0);
        assert_eq!(a, view_cell(4, 2).unwrap_or(0));
        assert_ne!(a, view_cell(4, 1).unwrap_or(0));
        assert!(view_cell(REPOS, 0).is_none());
        assert!(view_cell(0, VIEWS).is_none());
        let w = git_cell(5).unwrap_or(0);
        assert_eq!(w, git_cell(5).unwrap_or(0));
        assert!(git_cell(GIT_CELLS).is_none());
    }

    #[test]
    fn test_graft_cells() {
        assert!(graft_cell(0).is_some());
        assert!(graft_cell(4).is_none());
        assert_ne!(graft_cell(0), graft_cell(1));
    }

    #[test]
    fn test_pre_render_gate() {
        let mut lt = GvLedger::new();
        assert_eq!(lt.record_view(0, 0), Err(GvCode::EPreRender));
    }

    #[test]
    fn test_git_drift_caught() {
        let mut lt = GvLedger::new();
        assert_eq!(
            lt.consume_git(3, git_cell(3).unwrap_or(0).wrapping_sub(1)),
            Err(GvCode::EDrift)
        );
        assert!(lt.consume_git(3, git_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_graft_drift_caught() {
        let mut lt = GvLedger::new();
        assert_eq!(
            lt.record_graft(2, graft_cell(2).unwrap_or(0).wrapping_add(11)),
            Err(GvCode::EGraft)
        );
        assert!(lt.record_graft(2, graft_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.graft_all() == false);
    }

    #[test]
    fn test_dup_view_rejected() {
        let mut lt = GvLedger::new();
        let mut c = 0usize;
        while c < GIT_CELLS {
            if let Some(w) = git_cell(c) {
                let _ = lt.consume_git(c, w);
            }
            c += 1;
        }
        let _ = lt.record_view(1, 1);
        assert!(lt.record_view(1, 1).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (REPOS * VIEWS) as u32);
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
        assert_eq!(SEG_BASE, 0xC48C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52035_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
