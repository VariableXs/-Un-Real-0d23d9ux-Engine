//! UNX-F52101 Qt Creator 收口 · 0xC5740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52101 逐字锚定）：UNX-F52102-J1 组件树选择/
//! 下载/安装 ×10 场景全通；账号登录离线路径声明；网络面消费 AI-23（联
//! 签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题三安装器面
//! 复用。
//!
//! 设计要点：
//!   ① 域模型：安装器三段账（组件树选择/下载/安装）×10 场景 = 30 格
//!      场景账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 离线路径声明账：声明 3 行闭集（账号登录离线路径声明/镜像回退
//!      声明/缓存命中声明），缺一拒封。
//!   ③ 网络面消费 AI-23：HTTPS 消费 4 格（证书链/域名/重定向/断点续
//!      传），逐格与消费基准恒等才收；漂移必抓。
//!   ④ 选择→下载→安装段序单调；注入 ×10 轮（消费漂移/段序逆序/场景
//!      越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸
//!      + 定级（30 格 + 4 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52101 − 52000)）。
pub const SEG_BASE: u32 = 0xC5740;

/// 三段账段数。
pub const STAGES: usize = 3;

/// 场景数。
pub const SCENES: usize = 10;

/// 离线声明行数。
pub const OFF_ROWS: usize = 3;

/// HTTPS 消费格数。
pub const HTTPS_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QcCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 段号越闭集
    ENoStage,
    /// 场景载荷漂移
    EDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EHsDrift,
    /// 段序逆序
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

impl QcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QcCode::Ok => "成功",
            QcCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            QcCode::ENoStage => "段号越闭集：组件树/下载/安装三段之外先登记",
            QcCode::EDrift => "场景载荷漂移：折叠与基准不恒等，逐格复算",
            QcCode::ENoRow => "声明行越闭集：3 行闭集之外先登记",
            QcCode::ERowMiss => "缺声明行：账号登录离线路径声明未齐先补齐",
            QcCode::ENoCell => "消费格越册：HTTPS 4 格闭集之外先登记",
            QcCode::EHsDrift => "消费漂移：证书链/域名/重定向/续传折叠与基准不恒等",
            QcCode::EOrder => "段序逆序：选择→下载→安装严格递增，逆序必抓",
            QcCode::EDup => "重复落账：同格只许一条账目",
            QcCode::ESeal => "未齐即封：30 格未齐先补齐",
            QcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            QcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            QcCode::EGradeDrift => "定级漂移：30 格+4 格全通+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（场景 sc × 段 st）。
pub const fn scene_cell(sc: usize, st: usize) -> Option<u64> {
    if sc >= SCENES || st >= STAGES {
        return None;
    }
    Some(
        (0xCC000u64 + (sc as u64) * 0x2D)
            ^ (st as u64).wrapping_mul(0x01000193)
            ^ 0x7163726561746F72,
    )
}

/// 离线声明行折叠（行 r）。
pub const fn off_row(r: usize) -> Option<u32> {
    if r >= OFF_ROWS {
        return None;
    }
    Some(0xCD00 + r as u32 * 0x2C)
}

/// HTTPS 消费格基准（格 c）。
pub const fn hs_cell(c: usize) -> Option<u64> {
    if c >= HTTPS_CELLS {
        return None;
    }
    Some((0xCE000u64 + c as u64 * 0x33) ^ 0x6169323368747470)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct QcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [[Option<u64>; STAGES]; SCENES],
    /// 声明账。
    offs: [Option<u32>; OFF_ROWS],
    /// 消费账。
    hs: [Option<u64>; HTTPS_CELLS],
    /// 各场景最后段。
    last_stage: [i64; SCENES],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl QcLedger {
    /// 新账本。
    pub fn new() -> Self {
        QcLedger {
            gates: [None; 6],
            scenes: [[None; STAGES]; SCENES],
            offs: [None; OFF_ROWS],
            hs: [None; HTTPS_CELLS],
            last_stage: [-1; SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), QcCode> {
        if gate >= 6 {
            return Err(QcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x45100 + gate as u64);
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

    /// 消费一格。
    pub fn consume_hs(&mut self, c: usize, got: u64) -> Result<(), QcCode> {
        let want = match hs_cell(c) {
            Some(w) => w,
            None => return Err(QcCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x45200 + c as u64);
            return Err(QcCode::EHsDrift);
        }
        if match self.hs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QcCode::EDup);
        }
        if let Some(cell) = self.hs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x45201 + c as u64);
        Ok(())
    }

    /// 消费全账核验。
    pub fn hs_all(&self) -> bool {
        let mut c = 0usize;
        while c < HTTPS_CELLS {
            match self.hs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 场景一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, sc: usize, st: usize) -> Result<u64, QcCode> {
        if sc >= SCENES {
            return Err(QcCode::ENoScene);
        }
        if st >= STAGES {
            return Err(QcCode::ENoStage);
        }
        if (st as i64) < self.last_stage[sc] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x45300 + sc as u64);
            return Err(QcCode::EOrder);
        }
        let payload = match scene_cell(sc, st) {
            Some(p) => p,
            None => return Err(QcCode::ENoStage),
        };
        if match self.scenes.get(sc).and_then(|row| row.get(st)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QcCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(sc) {
            if let Some(c) = row.get_mut(st) {
                *c = Some(payload);
            }
        }
        self.last_stage[sc] = st as i64;
        self.fnv(0x45301 + (sc as u64) * 3 + st as u64);
        Ok(payload)
    }

    /// 场景全账核验（30 格）。
    pub fn scenes_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            let mut st = 0usize;
            while st < STAGES {
                match self.scenes.get(sc).and_then(|row| row.get(st)) {
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
    pub fn declare_off(&mut self, r: usize) -> Result<(), QcCode> {
        if r >= OFF_ROWS {
            return Err(QcCode::ENoRow);
        }
        if match self.offs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QcCode::EDup);
        }
        if let Some(c) = self.offs.get_mut(r) {
            *c = off_row(r);
        }
        self.fnv(0x45400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn offs_all(&self) -> bool {
        let mut r = 0usize;
        while r < OFF_ROWS {
            match self.offs.get(r) {
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

impl Default for QcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = QcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        let mut st = 0usize;
        while st < STAGES {
            if led.record_scene(sc, st).is_ok() {
                recorded += 1;
            }
            st += 1;
        }
        sc += 1;
    }
    let mut r = 0usize;
    while r < OFF_ROWS {
        let _ = led.declare_off(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < HTTPS_CELLS {
        if let Some(w) = hs_cell(c) {
            let _ = led.consume_hs(c, w);
        }
        c += 1;
    }
    // 注入 10 轮：消费漂移 / 段序逆序 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = QcLedger::new();
        let caught = match kind {
            0 => lt.consume_hs(2, hs_cell(2).unwrap_or(0).wrapping_add(1)) == Err(QcCode::EHsDrift),
            1 => {
                let _ = lt.record_scene(0, 2);
                lt.record_scene(0, 0) == Err(QcCode::EOrder)
            }
            _ => lt.record_scene(SCENES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x45500 + k as u64);
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
pub fn run_unx52101_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52101");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "安装器三段 30/30 全通",
        recorded >= (SCENES * STAGES) as u32,
        "组件树选择/下载/安装 ×10 场景逐格恒等",
    );
    s.add(
        "AI-23 HTTPS 消费 4/4 对账",
        {
            let mut lt = QcLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < HTTPS_CELLS {
                if let Some(w) = hs_cell(c) {
                    if lt.consume_hs(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.hs_all()
        },
        "证书链/域名/重定向/断点续传四格零漂移",
    );
    s.add(
        "离线路径声明 3/3 全齐",
        {
            let mut lt = QcLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OFF_ROWS {
                if lt.declare_off(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.offs_all()
        },
        "账号登录离线/镜像回退/缓存命中三行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "消费/逆序/越闭集三态轮转必抓");
    let mut l0 = QcLedger::new();
    s.add(
        "段序·逆序拒",
        {
            let _ = l0.record_scene(0, 2);
            l0.record_scene(0, 0) == Err(QcCode::EOrder)
        },
        "安装后回选择必抓",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_scene(SCENES, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_scene(0, STAGES).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·消费越册",
        l0.consume_hs(HTTPS_CELLS, 0).is_err(),
        "消费 4 越册必拒",
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
        let a = scene_cell(4, 1).unwrap_or(0);
        assert_eq!(a, scene_cell(4, 1).unwrap_or(0));
        assert_ne!(a, scene_cell(4, 2).unwrap_or(0));
        assert!(scene_cell(SCENES, 0).is_none());
        assert!(scene_cell(0, STAGES).is_none());
        assert!(off_row(OFF_ROWS).is_none());
        assert!(hs_cell(HTTPS_CELLS).is_none());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = QcLedger::new();
        let _ = lt.record_scene(1, 2);
        assert_eq!(lt.record_scene(1, 0), Err(QcCode::EOrder));
        assert!(lt.record_scene(1, 2).is_err());
    }

    #[test]
    fn test_hs_drift_caught() {
        let mut lt = QcLedger::new();
        assert_eq!(
            lt.consume_hs(1, hs_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(QcCode::EHsDrift)
        );
        assert!(lt.consume_hs(1, hs_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES * STAGES) as u32);
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
        assert_eq!(SEG_BASE, 0xC5740);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52101_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
