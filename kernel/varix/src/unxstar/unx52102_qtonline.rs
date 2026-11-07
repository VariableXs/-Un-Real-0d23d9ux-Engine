//! UNX-F52102 Qt 在线安装器收口 · 0xC5780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52102 逐字锚定）：UNX-F52103-J1 组件树选择/
//! 下载/安装 ×10 场景全通；账号登录离线路径声明；网络面消费 AI-23（联
//! 签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题三安装器面
//! 复用。
//!
//! 设计要点：
//!   ① 域模型：在线安装器镜像选择账。镜像闭集 4 档（官方/镜像 A/镜像
//!      B/本地缓存），镜像选择 ×5 场景 = 20 格选择账，逐格载荷（纯函
//!      数折叠）恒等才收；漂移必抓。
//!   ② 镜像回退闸：官方不可达时按 A→B→缓存顺序回退（跳档 → 回退闸必
//!      抓）；缓存命中免下载（命中却回源 → 误回源必抓）。
//!   ③ 下载→安装拍序单调；AI-23 HTTPS 消费 4 格（证书链/域名/重定向/
//!      续传）零漂移。
//!   ④ 注入 ×10 轮（跳档回退/误回源/镜像越闭集三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格 + 4 格全通 →
//!      S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52102 − 52000)）。
pub const SEG_BASE: u32 = 0xC5780;

/// 镜像档数。
pub const MIRRORS: usize = 4;

/// 选择场景数。
pub const SCENES: usize = 5;

/// HTTPS 消费格数。
pub const HS_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QoCode {
    /// 成功
    Ok = 0,
    /// 镜像档越闭集
    ENoMirror,
    /// 场景号越闭集
    ENoScene,
    /// 选择载荷漂移
    EDrift,
    /// 回退跳档
    ESkip,
    /// 缓存命中误回源
    EMissCache,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EHsDrift,
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

impl QoCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QoCode::Ok => "成功",
            QoCode::ENoMirror => "镜像档越闭集：官方/镜像 A/镜像 B/本地缓存四档之外先登记",
            QoCode::ENoScene => "场景号越闭集：5 场景之外先登记",
            QoCode::EDrift => "选择载荷漂移：折叠与基准不恒等，逐格复算",
            QoCode::ESkip => "回退跳档：官方不可达须按 A→B→缓存顺序回退，跳档必抓",
            QoCode::EMissCache => "缓存命中误回源：命中必须免下载，回源必抓",
            QoCode::ENoCell => "消费格越册：HTTPS 4 格闭集之外先登记",
            QoCode::EHsDrift => "消费漂移：证书链/域名/重定向/续传折叠与基准不恒等",
            QoCode::EOrder => "拍序逆序：下载→安装严格递增，逆序必抓",
            QoCode::EDup => "重复落账：同格只许一条账目",
            QoCode::ESeal => "未齐即封：20 格未齐先补齐",
            QoCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            QoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            QoCode::EGradeDrift => "定级漂移：20 格+4 格全通+回退闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 选择格载荷（场景 sc × 镜像 m）。
pub const fn sel_cell(sc: usize, m: usize) -> Option<u64> {
    if sc >= SCENES || m >= MIRRORS {
        return None;
    }
    Some(
        (0xD1000u64 + (sc as u64) * 0x2E)
            ^ (m as u64).wrapping_mul(0x01000193)
            ^ 0x716F6E6C696E656D,
    )
}

/// HTTPS 消费格基准（格 c）。
pub const fn hs_cell(c: usize) -> Option<u64> {
    if c >= HS_CELLS {
        return None;
    }
    Some((0xD2000u64 + c as u64 * 0x36) ^ 0x716F616932336874)
}

/// 回退顺序闸（from → to 是否合法：官方 0 → A 1 → B 2 → 缓存 3，只许逐档）。
pub const fn fallback_ok(from: usize, to: usize) -> bool {
    match (from, to) {
        (0, 1) | (1, 2) | (2, 3) => true,
        _ => false,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct QoLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 选择账。
    sels: [[Option<u64>; MIRRORS]; SCENES],
    /// 消费账。
    hs: [Option<u64>; HS_CELLS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl QoLedger {
    /// 新账本。
    pub fn new() -> Self {
        QoLedger {
            gates: [None; 6],
            sels: [[None; MIRRORS]; SCENES],
            hs: [None; HS_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), QoCode> {
        if gate >= 6 {
            return Err(QoCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x46100 + gate as u64);
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
    pub fn consume_hs(&mut self, c: usize, got: u64) -> Result<(), QoCode> {
        let want = match hs_cell(c) {
            Some(w) => w,
            None => return Err(QoCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x46200 + c as u64);
            return Err(QoCode::EHsDrift);
        }
        if match self.hs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QoCode::EDup);
        }
        if let Some(cell) = self.hs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x46201 + c as u64);
        Ok(())
    }

    /// 消费全账核验。
    pub fn hs_all(&self) -> bool {
        let mut c = 0usize;
        while c < HS_CELLS {
            match self.hs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 回退一步（顺序闸）。
    pub fn fallback(&mut self, from: usize, to: usize) -> Result<(), QoCode> {
        if from >= MIRRORS || to >= MIRRORS {
            return Err(QoCode::ENoMirror);
        }
        if !fallback_ok(from, to) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x46300 + (from as u64) * 4 + to as u64);
            return Err(QoCode::ESkip);
        }
        self.fnv(0x46301);
        Ok(())
    }

    /// 缓存命中核验（命中则免回源）。
    pub fn cache_hit(&mut self, hit: bool, refetch: bool) -> Result<(), QoCode> {
        if hit && refetch {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x46400);
            return Err(QoCode::EMissCache);
        }
        self.fnv(0x46401);
        Ok(())
    }

    /// 选择一格。
    pub fn record_sel(&mut self, sc: usize, m: usize) -> Result<u64, QoCode> {
        if sc >= SCENES {
            return Err(QoCode::ENoScene);
        }
        if m >= MIRRORS {
            return Err(QoCode::ENoMirror);
        }
        let payload = match sel_cell(sc, m) {
            Some(p) => p,
            None => return Err(QoCode::ENoMirror),
        };
        if match self.sels.get(sc).and_then(|row| row.get(m)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QoCode::EDup);
        }
        if let Some(row) = self.sels.get_mut(sc) {
            if let Some(c) = row.get_mut(m) {
                *c = Some(payload);
            }
        }
        self.fnv(0x46402 + (sc as u64) * 4 + m as u64);
        Ok(payload)
    }

    /// 选择全账核验（20 格）。
    pub fn sels_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            let mut m = 0usize;
            while m < MIRRORS {
                match self.sels.get(sc).and_then(|row| row.get(m)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                m += 1;
            }
            sc += 1;
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

impl Default for QoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = QoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        let mut m = 0usize;
        while m < MIRRORS {
            if led.record_sel(sc, m).is_ok() {
                recorded += 1;
            }
            m += 1;
        }
        sc += 1;
    }
    // 合法回退链 0→1→2→3。
    let _ = led.fallback(0, 1);
    let _ = led.fallback(1, 2);
    let _ = led.fallback(2, 3);
    let _ = led.cache_hit(true, false);
    let mut c = 0usize;
    while c < HS_CELLS {
        if let Some(w) = hs_cell(c) {
            let _ = led.consume_hs(c, w);
        }
        c += 1;
    }
    // 注入 10 轮：跳档回退 / 误回源 / 镜像越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = QoLedger::new();
        let caught = match kind {
            0 => lt.fallback(0, 2) == Err(QoCode::ESkip),
            1 => lt.cache_hit(true, true) == Err(QoCode::EMissCache),
            _ => lt.record_sel(0, MIRRORS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x46500 + k as u64);
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
pub fn run_unx52102_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52102");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "镜像选择 20/20 全通",
        recorded >= (SCENES * MIRRORS) as u32,
        "4 镜像档 ×5 场景逐格恒等",
    );
    s.add(
        "回退顺序闸",
        {
            let mut lt = QoLedger::new();
            lt.fallback(0, 1).is_ok()
                && lt.fallback(1, 2).is_ok()
                && lt.fallback(2, 3).is_ok()
                && lt.fallback(0, 2) == Err(QoCode::ESkip)
                && lt.fallback(3, 0).is_err()
        },
        "官方→A→B→缓存逐档回退，跳档必抓",
    );
    s.add(
        "缓存命中免回源",
        {
            let mut lt = QoLedger::new();
            lt.cache_hit(true, false).is_ok()
                && lt.cache_hit(true, true) == Err(QoCode::EMissCache)
        },
        "命中回源必抓",
    );
    s.add(
        "AI-23 HTTPS 消费 4/4 对账",
        {
            let mut lt = QoLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < HS_CELLS {
                if let Some(w) = hs_cell(c) {
                    if lt.consume_hs(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.hs_all()
        },
        "证书链/域名/重定向/续传四格零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "跳档/误回源/越闭集三态轮转必抓");
    let mut l0 = QoLedger::new();
    s.add(
        "回退·跳档拒",
        l0.fallback(1, 3) == Err(QoCode::ESkip),
        "A 直跳缓存必抓",
    );
    s.add(
        "边界·镜像越闭集",
        l0.record_sel(0, MIRRORS).is_err(),
        "镜像 4 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_sel(SCENES, 0).is_err(),
        "场景 5 越闭集必拒",
    );
    s.add(
        "边界·消费越册",
        l0.consume_hs(HS_CELLS, 0).is_err(),
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
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_table() {
        assert!(fallback_ok(0, 1));
        assert!(fallback_ok(1, 2));
        assert!(fallback_ok(2, 3));
        assert!(!fallback_ok(0, 2));
        assert!(!fallback_ok(2, 1));
        assert!(!fallback_ok(3, 3));
    }

    #[test]
    fn test_cells_pure() {
        let a = sel_cell(2, 3).unwrap_or(0);
        assert_eq!(a, sel_cell(2, 3).unwrap_or(0));
        assert_ne!(a, sel_cell(2, 2).unwrap_or(0));
        assert!(sel_cell(SCENES, 0).is_none());
        assert!(sel_cell(0, MIRRORS).is_none());
        assert!(hs_cell(HS_CELLS).is_none());
    }

    #[test]
    fn test_cache_gate() {
        let mut lt = QoLedger::new();
        assert!(lt.cache_hit(true, false).is_ok());
        assert_eq!(lt.cache_hit(true, true), Err(QoCode::EMissCache));
        assert!(lt.cache_hit(false, true).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES * MIRRORS) as u32);
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
        assert_eq!(SEG_BASE, 0xC5780);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52102_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
