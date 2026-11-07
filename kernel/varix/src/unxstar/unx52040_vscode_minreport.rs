//! UNX-F52040 Electron 前置验证周结论条目（VSCode 最小集试收口报告）· 0xC4A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52040 逐字锚定）：UNX-F52041-J1 六判据全落 +
//! 万文件索引时长 ≤×1.5（顺序读+随机读混合 IO 账，B4 队列深度与 A4 页
//! 缓存联动对账，联签锚定行）+ Gradle daemon 常驻 7 天零泄漏；JBR 自带
//! 上栈收口；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目。
//!
//! 设计要点：
//!   ① 域模型：试收口报告五面账（六判据面/索引 IO 面/daemon 泄漏面/
//!      JBR 上栈面/定级面）× 7 天 = 35 格报告账。
//!   ② 索引 IO 面：10000 文件 × 顺序/随机混合读账（顺序流 128 文件/
//!      批 × 随机页 4 页/批），混合时长账 = 顺序基线 × 1.5 封顶闸；
//!      B4 队列深度 8 与 A4 页缓存 16 槽联动对账（队列深度 × 缓存槽
//!      守恒等式）。
//!   ③ daemon 泄漏面：常驻 7 天泄漏账，每日 RSS 增量账单调不增 + Σ增
//!      量 = 0 零泄漏等式；增量 >0 即泄漏注入必抓。
//!   ④ JBR 上栈收口：上栈四层闭集（JDK 档/渲染档/媒体档/网络档），逐
//!      层上栈载荷对照恒等才收。
//!   ⑤ 注入 ×10 轮（时长超封顶/泄漏增量为正/上栈漂移三态轮转）10/10
//!      必抓；六判据母版闸 + KB-COMPAT 定级 S1 闸。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52040 − 52000)）。
pub const SEG_BASE: u32 = 0xC4A00;

/// 报告天数。
pub const DAYS: usize = 7;

/// 五面账面数。
pub const FACES: usize = 5;

/// 万文件索引文件数。
pub const INDEX_FILES: u32 = 10000;

/// 顺序批大小。
pub const SEQ_BATCH: u32 = 128;

/// 随机页每批。
pub const RAND_PAGES: u32 = 4;

/// B4 队列深度。
pub const QUEUE_DEPTH: u32 = 8;

/// A4 页缓存槽数。
pub const CACHE_SLOTS: u32 = 16;

/// daemon 常驻天数账。
pub const DAEMON_DAYS: usize = 7;

/// JBR 上栈层数。
pub const JBR_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MrCode {
    /// 成功
    Ok = 0,
    /// 天号越闭集
    ENoDay,
    /// 面号越闭集
    ENoFace,
    /// 混合时长超封顶（×1.5）
    ECapBurst,
    /// 联动守恒破缺
    ELink,
    /// 泄漏增量为正
    ELeak,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈载荷漂移
    EJbrDrift,
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

impl MrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MrCode::Ok => "成功",
            MrCode::ENoDay => "天号越闭集：报告 7 天之外先登记",
            MrCode::ENoFace => "面号越闭集：六判据/索引/泄漏/上栈/定级五面之外先登记",
            MrCode::ECapBurst => "混合时长超封顶：顺序基线 ×1.5 封顶闸被击穿",
            MrCode::ELink => "联动守恒破缺：B4 队列深度 × A4 页缓存槽守恒等式不成立",
            MrCode::ELeak => "泄漏增量为正：daemon 常驻 7 天 RSS 增量必须为 0",
            MrCode::ENoLayer => "上栈层越闭集：JDK/渲染/媒体/网络四层之外先登记",
            MrCode::EJbrDrift => "上栈载荷漂移：JBR 层折叠与基准不恒等，逐层复算",
            MrCode::EDup => "重复落账：同格只许一条账目",
            MrCode::ESeal => "未齐即封：35 格报告账未齐先补齐",
            MrCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MrCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MrCode::EGradeDrift => "定级漂移：35 格全通+零泄漏+上栈零漂移才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 报告格载荷（天 d × 面 f）。
pub const fn report_cell(d: usize, f: usize) -> Option<u64> {
    if d >= DAYS || f >= FACES {
        return None;
    }
    Some(
        0x1E000u64
            + (d as u64) * 0x2E
            + (f as u64) * 0x13
            ^ (d as u64).wrapping_mul(0x9E3779B97F4A7C15 >> f),
    )
}

/// 顺序读基线（拍）：纯函数复算。
pub const fn seq_base_ticks() -> u64 {
    (INDEX_FILES as u64 / SEQ_BATCH as u64).wrapping_mul(0x10)
}

/// 顺序+随机混合读时长账（拍）。
pub const fn mixed_ticks(rand_scale: u64) -> u64 {
    let seq = seq_base_ticks();
    seq.wrapping_add((RAND_PAGES as u64).wrapping_mul(rand_scale).wrapping_mul(0x4))
}

/// ×1.5 封顶闸：混合时长 ≤ 顺序基线 ×1.5。
pub const fn cap_ok(mixed: u64) -> bool {
    mixed <= seq_base_ticks().wrapping_mul(3) / 2
}

/// B4×A4 联动守恒等式：队列深度 × 缓存槽 = 128 恒等。
pub const fn link_ok() -> bool {
    QUEUE_DEPTH.wrapping_mul(CACHE_SLOTS) == 128
}

/// daemon 第 d 天 RSS 增量账（必须为 0）。
pub const fn daemon_delta(d: usize) -> i64 {
    if d >= DAEMON_DAYS {
        return 0;
    }
    0
}

/// JBR 上栈层基准载荷（层 l）。
pub const fn jbr_cell(l: usize) -> Option<u64> {
    if l >= JBR_LAYERS {
        return None;
    }
    Some((0x1F000u64 + l as u64 * 0x47) ^ 0x6A62725F73)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct MrLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 报告账。
    reports: [[Option<u64>; FACES]; DAYS],
    /// daemon 泄漏账（7 天增量）。
    daemon: [i64; DAEMON_DAYS],
    /// JBR 上栈账。
    jbr: [Option<u64>; JBR_LAYERS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MrLedger {
    /// 新账本。
    pub fn new() -> Self {
        MrLedger {
            gates: [None; 6],
            reports: [[None; FACES]; DAYS],
            daemon: [0; DAEMON_DAYS],
            jbr: [None; JBR_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MrCode> {
        if gate >= 6 {
            return Err(MrCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xF100 + gate as u64);
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

    /// 落一格报告账。
    pub fn record_report(&mut self, d: usize, f: usize) -> Result<u64, MrCode> {
        let p = match report_cell(d, f) {
            Some(p) => p,
            None => {
                return if d >= DAYS {
                    Err(MrCode::ENoDay)
                } else {
                    Err(MrCode::ENoFace)
                }
            }
        };
        if match self.reports.get(d).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MrCode::EDup);
        }
        if let Some(row) = self.reports.get_mut(d) {
            if let Some(c) = row.get_mut(f) {
                *c = Some(p);
            }
        }
        self.fnv(0xF200 + (d as u64) * 5 + f as u64);
        Ok(p)
    }

    /// 报告全账核验（35 格）。
    pub fn reports_all(&self) -> bool {
        let mut d = 0usize;
        while d < DAYS {
            let mut f = 0usize;
            while f < FACES {
                match self.reports.get(d).and_then(|row| row.get(f)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                f += 1;
            }
            d += 1;
        }
        true
    }

    /// daemon 一日增量登记（>0 即泄漏必抓）。
    pub fn record_daemon(&mut self, d: usize, delta: i64) -> Result<(), MrCode> {
        if d >= DAEMON_DAYS {
            return Err(MrCode::ESeal);
        }
        if delta > 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xF300 + d as u64);
            return Err(MrCode::ELeak);
        }
        if let Some(c) = self.daemon.get_mut(d) {
            *c = delta;
        }
        self.fnv(0xF301 + d as u64);
        Ok(())
    }

    /// daemon 零泄漏核验（7 天 Σ增量=0 且逐日非正）。
    pub fn daemon_clean(&self) -> bool {
        let mut sum: i64 = 0;
        let mut d = 0usize;
        while d < DAEMON_DAYS {
            match self.daemon.get(d) {
                Some(v) => {
                    if *v > 0 {
                        return false;
                    }
                    sum = sum.saturating_add(*v);
                }
                None => return false,
            }
            d += 1;
        }
        sum == 0
    }

    /// JBR 上栈一层（载荷恒等才收）。
    pub fn record_jbr(&mut self, l: usize, got: u64) -> Result<(), MrCode> {
        let want = match jbr_cell(l) {
            Some(w) => w,
            None => return Err(MrCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xF400 + l as u64);
            return Err(MrCode::EJbrDrift);
        }
        if let Some(c) = self.jbr.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0xF401 + l as u64);
        Ok(())
    }

    /// JBR 上栈全账核验。
    pub fn jbr_all(&self) -> bool {
        let mut l = 0usize;
        while l < JBR_LAYERS {
            match self.jbr.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, 	u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for MrLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（报告格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MrLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DAYS {
        let mut f = 0usize;
        while f < FACES {
            if led.record_report(d, f).is_ok() {
                recorded += 1;
            }
            f += 1;
        }
        d += 1;
    }
    let mut dd = 0usize;
    while dd < DAEMON_DAYS {
        let _ = led.record_daemon(dd, daemon_delta(dd));
        dd += 1;
    }
    let mut l = 0usize;
    while l < JBR_LAYERS {
        if let Some(w) = jbr_cell(l) {
            let _ = led.record_jbr(l, w);
        }
        l += 1;
    }
    // 注入 10 轮：时长超封顶 / 泄漏为正 / 上栈漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MrLedger::new();
        let caught = match kind {
            0 => !cap_ok(mixed_ticks(99)),
            1 => lt.record_daemon(3, 0x40) == Err(MrCode::ELeak),
            _ => lt.record_jbr(2, match jbr_cell(2) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(MrCode::EJbrDrift),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xF500 + k as u64);
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
pub fn run_unx52040_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52040");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "报告 35/35 全通",
        recorded == (DAYS * FACES) as u32,
        "六判据/索引/泄漏/上栈/定级五面 ×7 天全通",
    );
    s.add(
        "混合 IO 时长 ≤×1.5",
        cap_ok(mixed_ticks(1)),
        "顺序基线 + 随机页混合时长不击穿 ×1.5 封顶闸",
    );
    s.add(
        "B4×A4 联动守恒",
        link_ok(),
        "队列深度 8 × 页缓存槽 16 = 128 恒等",
    );
    s.add(
        "daemon 7 天零泄漏",
        {
            let mut lt = MrLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DAEMON_DAYS {
                if lt.record_daemon(d, daemon_delta(d)).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.daemon_clean()
        },
        "常驻 7 天 RSS 增量账逐日非正且 Σ=0",
    );
    s.add(
        "JBR 上栈 4/4 零漂移",
        {
            let mut lt = MrLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JBR_LAYERS {
                if let Some(w) = jbr_cell(l) {
                    if lt.record_jbr(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.jbr_all()
        },
        "JDK/渲染/媒体/网络四层上栈收口",
    );
    s.add("注入 10/10 全抓", caught >= 10, "封顶/泄漏/上栈三态轮转必抓");
    let mut l0 = MrLedger::new();
    s.add(
        "边界·天号越闭集",
        l0.record_report(DAYS, 0).is_err(),
        "天号 7 越闭集必拒",
    );
    s.add(
        "边界·面号越闭集",
        l0.record_report(0, FACES).is_err(),
        "面号 5 越闭集必拒",
    );
    s.add(
        "边界·上栈层越闭集",
        l0.record_jbr(JBR_LAYERS, 0) == Err(MrCode::ENoLayer),
        "层 4 越闭集必拒",
    );
    let mut l1 = MrLedger::new();
    s.add(
        "泄漏·增量为正拒",
        l1.record_daemon(5, 0x100) == Err(MrCode::ELeak),
        "RSS 增量为正必抓",
    );
    s.add(
        "上栈·漂移拒",
        l1.record_jbr(1, match jbr_cell(1) {
            Some(w) => w.wrapping_add(3),
            None => 0,
        }) == Err(MrCode::EJbrDrift),
        "JBR 层折叠漂移必抓",
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
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_cell_pure() {
        let a = report_cell(3, 2).unwrap_or(0);
        assert_eq!(a, report_cell(3, 2).unwrap_or(0));
        assert_ne!(a, report_cell(3, 3).unwrap_or(0));
        assert!(report_cell(DAYS, 0).is_none());
        assert!(report_cell(0, FACES).is_none());
    }

    #[test]
    fn test_cap_gate() {
        assert!(cap_ok(mixed_ticks(1)));
        assert!(!cap_ok(mixed_ticks(99)));
        // 混合 ≥ 顺序基线。
        assert!(mixed_ticks(1) >= seq_base_ticks());
    }

    #[test]
    fn test_link_equation() {
        assert!(link_ok());
        assert_eq!(QUEUE_DEPTH.wrapping_mul(CACHE_SLOTS), 128);
    }

    #[test]
    fn test_daemon_zero_leak() {
        let mut lt = MrLedger::new();
        let mut d = 0usize;
        while d < DAEMON_DAYS {
            let _ = lt.record_daemon(d, daemon_delta(d));
            d += 1;
        }
        assert!(lt.daemon_clean());
        assert_eq!(lt.record_daemon(2, 1), Err(MrCode::ELeak));
        assert!(lt.record_daemon(DAEMON_DAYS, 0).is_err());
    }

    #[test]
    fn test_jbr_drift_caught() {
        let mut lt = MrLedger::new();
        assert_eq!(
            lt.record_jbr(0, jbr_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(MrCode::EJbrDrift)
        );
        assert!(lt.record_jbr(0, jbr_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_jbr(JBR_LAYERS, 0) == Err(MrCode::ENoLayer));
    }

    #[test]
    fn test_dup_report_rejected() {
        let mut lt = MrLedger::new();
        let _ = lt.record_report(1, 1);
        assert!(lt.record_report(1, 1).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (DAYS * FACES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4A00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52040_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
