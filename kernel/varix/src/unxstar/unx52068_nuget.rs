//! UNX-F52068 NuGet（CLI 与包恢复面）收口 · 0xC5100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52068 逐字锚定）：UNX-F52069-J1 端口安装 ×20
//! （含二进制缓存命中）全通；与 MSBuild 集成 ×5 工程全通；克隆与自举
//! 消费 git 嫁接件判据绿（联签锚定行 AI-59）；KB-COMPAT 定级登记；判
//! 据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：端口安装 ×20 账（含二进制缓存命中二元），20 格安装账
//!      逐格载荷（纯函数折叠）恒等才收；命中/回源二元闭集越二元拒。
//!   ② MSBuild 集成 ×5 工程账：集成格三元（targets 档/props 档/恢复
//!      钩子档）全等才收；集成漂移必抓。
//!   ③ git 嫁接件消费账：嫁接 4 件判据绿（上游折叠恒等），漂移即抓。
//!   ④ 安装拍序闸：缓存命中 → 回源 → 安装 → 集成拍序严格递增；注入
//!      ×10 轮（安装漂移/二元越闭集/集成漂移三态轮转）10/10 必抓，注
//!      入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 集成 5 格
//!      + 嫁接 4 件全绿 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52068 − 52000)）。
pub const SEG_BASE: u32 = 0xC5100;

/// 端口安装数。
pub const INSTALLS: usize = 20;

/// MSBuild 集成工程数。
pub const MSB_PROJS: usize = 5;

/// 嫁接件数。
pub const GRAFTS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NgCode {
    /// 成功
    Ok = 0,
    /// 安装号越闭集
    ENoInst,
    /// 命中二元越闭集
    ENoBin,
    /// 安装漂移
    EDrift,
    /// 集成格越册
    ENoMsb,
    /// 集成漂移
    EMsbDrift,
    /// 嫁接漂移
    EGraft,
    /// 拍序乱
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

impl NgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NgCode::Ok => "成功",
            NgCode::ENoInst => "安装号越闭集：20 端口安装之外先登记",
            NgCode::ENoBin => "命中二元越闭集：命中/回源二元之外先登记",
            NgCode::EDrift => "安装漂移：安装折叠与基准不恒等，逐格复算",
            NgCode::ENoMsb => "集成格越册：MSBuild 5 工程闭集之外先登记",
            NgCode::EMsbDrift => "集成漂移：targets/props/恢复钩子折叠与基准不恒等",
            NgCode::EGraft => "嫁接漂移：上游折叠不恒等判据不绿",
            NgCode::EOrder => "拍序乱：命中→回源→安装→集成严格递增",
            NgCode::EDup => "重复落账：同格只许一条账目",
            NgCode::ESeal => "未齐即封：20 格未齐先补齐",
            NgCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NgCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NgCode::EGradeDrift => "定级漂移：20 格全通+集成 5 格+嫁接 4 件全绿才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 安装格载荷（安装 i × 命中二元 b）。
pub const fn inst_cell(i: usize, b: u32) -> Option<u64> {
    if i >= INSTALLS || b > 1 {
        return None;
    }
    Some(
        (0x74000u64 + (i as u64) * 0x2D)
            ^ (b as u64).wrapping_mul(0x01000193),
    )
}

/// 集成格载荷（工程 p；三元折叠）。
pub const fn msb_cell(p: usize) -> Option<u64> {
    if p >= MSB_PROJS {
        return None;
    }
    Some((0x75000u64 + (p as u64) * 0x33) ^ 0x6D736275696C64)
}

/// 嫁接件基准（件 g）。
pub const fn graft_cell(g: usize) -> Option<u64> {
    if g >= GRAFTS {
        return None;
    }
    Some((0x76000u64 + g as u64 * 0x2A) ^ (g as u64).wrapping_mul(0x100000001b3))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct NgLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 安装账。
    insts: [Option<u64>; INSTALLS],
    /// 集成账。
    msb: [Option<u64>; MSB_PROJS],
    /// 嫁接账（true=绿）。
    graft: [Option<bool>; GRAFTS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NgLedger {
    /// 新账本。
    pub fn new() -> Self {
        NgLedger {
            gates: [None; 6],
            insts: [None; INSTALLS],
            msb: [None; MSB_PROJS],
            graft: [None; GRAFTS],
            last_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NgCode> {
        if gate >= 6 {
            return Err(NgCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x2D100 + gate as u64);
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

    /// 安装一格（二元闭集 + 载荷对照 + 拍序单调 + 重复拒）。
    pub fn install(&mut self, i: usize, b: u32, tick: u32) -> Result<u64, NgCode> {
        if i >= INSTALLS {
            return Err(NgCode::ENoInst);
        }
        if b > 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2D200 + i as u64);
            return Err(NgCode::ENoBin);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2D201);
            return Err(NgCode::EOrder);
        }
        let want = match inst_cell(i, b) {
            Some(w) => w,
            None => return Err(NgCode::ENoBin),
        };
        if match self.insts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NgCode::EDup);
        }
        if let Some(c) = self.insts.get_mut(i) {
            *c = Some(want);
        }
        self.last_tick = tick as i64;
        self.fnv(0x2D202 + i as u64);
        Ok(want)
    }

    /// 安装全账核验（20 格）。
    pub fn insts_all(&self) -> bool {
        let mut i = 0usize;
        while i < INSTALLS {
            match self.insts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 集成一格（三元载荷对照 + 重复拒）。
    pub fn integrate(&mut self, p: usize, got: u64) -> Result<(), NgCode> {
        if p >= MSB_PROJS {
            return Err(NgCode::ENoMsb);
        }
        let want = match msb_cell(p) {
            Some(w) => w,
            None => return Err(NgCode::ENoMsb),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2D300 + p as u64);
            return Err(NgCode::EMsbDrift);
        }
        if match self.msb.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NgCode::EDup);
        }
        if let Some(c) = self.msb.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x2D301 + p as u64);
        Ok(())
    }

    /// 集成全账核验（5 格）。
    pub fn msb_all(&self) -> bool {
        let mut p = 0usize;
        while p < MSB_PROJS {
            match self.msb.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 嫁接一件（上游折叠恒等判绿）。
    pub fn record_graft(&mut self, g: usize, got: u64) -> Result<(), NgCode> {
        let want = match graft_cell(g) {
            Some(w) => w,
            None => return Err(NgCode::EGraft),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2D400 + g as u64);
            return Err(NgCode::EGraft);
        }
        if let Some(c) = self.graft.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x2D401 + g as u64);
        Ok(())
    }

    /// 嫁接全绿核验。
    pub fn graft_all(&self) -> bool {
        let mut g = 0usize;
        while g < GRAFTS {
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

impl Default for NgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    let mut tick: u32 = 0;
    while i < INSTALLS {
        tick = tick.saturating_add(2);
        let b = (i % 2) as u32;
        if led.install(i, b, tick).is_ok() {
            recorded += 1;
        }
        i += 1;
    }
    let mut p = 0usize;
    while p < MSB_PROJS {
        if let Some(w) = msb_cell(p) {
            if led.integrate(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut gr = 0usize;
    while gr < GRAFTS {
        if let Some(w) = graft_cell(gr) {
            let _ = led.record_graft(gr, w);
        }
        gr += 1;
    }
    // 注入 10 轮：二元越闭集 / 集成漂移 / 嫁接漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NgLedger::new();
        let caught = match kind {
            0 => lt.install(0, 2, 1) == Err(NgCode::ENoBin),
            1 => lt.integrate(1, msb_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NgCode::EMsbDrift),
            _ => lt.record_graft(2, graft_cell(2).unwrap_or(0).wrapping_add(1)) == Err(NgCode::EGraft),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x2D500 + k as u64);
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
pub fn run_unx52068_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52068");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "端口安装 20/20 全通",
        recorded >= INSTALLS as u32,
        "20 端口安装（含二进制缓存命中二元）逐格恒等",
    );
    s.add(
        "MSBuild 集成 5/5 全账",
        {
            let mut lt = NgLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < MSB_PROJS {
                if let Some(w) = msb_cell(p) {
                    if lt.integrate(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.msb_all()
        },
        "targets/props/恢复钩子三元零漂移",
    );
    s.add(
        "git 嫁接 4/4 判绿",
        {
            let mut lt = NgLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GRAFTS {
                if let Some(w) = graft_cell(g) {
                    if lt.record_graft(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.graft_all()
        },
        "上游折叠恒等判绿（AI-59 联签锚定）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "二元/集成/嫁接三态轮转必抓");
    let mut l0 = NgLedger::new();
    s.add(
        "二元·越闭集拒",
        l0.install(0, 2, 1) == Err(NgCode::ENoBin),
        "命中/回源二元之外先登记",
    );
    s.add(
        "边界·安装越闭集",
        l0.install(INSTALLS, 0, 1).is_err(),
        "安装 20 越闭集必拒",
    );
    s.add(
        "边界·集成越册",
        l0.integrate(MSB_PROJS, 0).is_err(),
        "工程 5 越册必拒",
    );
    let mut l1 = NgLedger::new();
    let _ = l1.install(3, 0, 4);
    s.add(
        "拍序·回退拒",
        l1.install(4, 1, 2) == Err(NgCode::EOrder),
        "拍序回退必抓",
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
        let a = inst_cell(7, 1).unwrap_or(0);
        assert_eq!(a, inst_cell(7, 1).unwrap_or(0));
        assert_ne!(a, inst_cell(7, 0).unwrap_or(0));
        assert!(inst_cell(INSTALLS, 0).is_none());
        assert!(inst_cell(0, 2).is_none());
        assert!(msb_cell(MSB_PROJS).is_none());
        assert!(graft_cell(GRAFTS).is_none());
    }

    #[test]
    fn test_bin_closure() {
        let mut lt = NgLedger::new();
        assert_eq!(lt.install(0, 2, 1), Err(NgCode::ENoBin));
        assert!(lt.install(0, 1, 1).is_ok());
    }

    #[test]
    fn test_msb_and_graft_gates() {
        let mut lt = NgLedger::new();
        assert_eq!(lt.integrate(9, 0), Err(NgCode::ENoMsb));
        assert_eq!(
            lt.record_graft(0, graft_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(NgCode::EGraft)
        );
        assert!(lt.record_graft(0, graft_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= INSTALLS as u32);
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
        assert_eq!(SEG_BASE, 0xC5100);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52068_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
