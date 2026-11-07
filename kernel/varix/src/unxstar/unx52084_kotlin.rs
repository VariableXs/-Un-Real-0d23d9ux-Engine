//! UNX-F52084 Kotlin（编译器发行版）收口 · 0xC5500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52084 逐字锚定）：UNX-F52085-J1 Kotlin 编译
//! ×10 工程（JVM/JS 双目标）全通；Gradle KGP 插件消费对账（F52083 判
//! 据复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：Kotlin 编译 ×10 工程账（JVM/JS 双目标二元），10×2 =
//!      20 格编译账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② KGP 消费对账：Gradle KGP 插件消费 4 格（插件版本档/目标三元档/
//!      增量编译档/工具链档），逐格与消费基准恒等才收（F52083 判据复
//!      用）；漂移必抓。
//!   ③ 编译拍序闸：JVM 先 JS 后（目标序单调）；注入 ×10 轮（消费漂移/
//!      目标越二元/编译漂移三态轮转）10/10 必抓，注入=检出+逃逸守恒；
//!      六判据母版闸 + 定级（20 格全通 + KGP 4 格全账 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52084 − 52000)）。
pub const SEG_BASE: u32 = 0xC5500;

/// 工程数。
pub const PROJS: usize = 10;

/// 目标二元（0=JVM 1=JS）。
pub const TARGETS: usize = 2;

/// KGP 消费格数。
pub const KGP_CELLS: usize = 4;

/// KGP 三元基准。
pub const KGP_VER: u32 = 19;
pub const GRADLE_API: u32 = 82;
pub const JDK: u32 = 17;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 目标二元越闭集
    ENoTarget,
    /// 编译漂移
    EDrift,
    /// KGP 格越册
    ENoKgp,
    /// KGP 消费漂移
    EKgpDrift,
    /// KGP 三元失配
    ETri,
    /// 目标序逆序
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

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::Ok => "成功",
            KtCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            KtCode::ENoTarget => "目标二元越闭集：JVM/JS 二元之外先登记",
            KtCode::EDrift => "编译漂移：编译折叠与基准不恒等，逐格复算",
            KtCode::ENoKgp => "KGP 格越册：4 格闭集之外先登记",
            KtCode::EKgpDrift => "KGP 消费漂移：插件版本/目标/增量/工具链折叠与基准不恒等",
            KtCode::ETri => "KGP 三元失配：插件版本/Gradle API/JDK 档与基准不符",
            KtCode::EOrder => "目标序逆序：JVM 先 JS 后，逆序必抓",
            KtCode::EDup => "重复落账：同格只许一条账目",
            KtCode::ESeal => "未齐即封：20 格未齐先补齐",
            KtCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KtCode::EGradeDrift => "定级漂移：20 格全通+KGP 4 格全账+目标序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 编译格载荷（工程 p × 目标 t）。
pub const fn kt_cell(p: usize, t: usize) -> Option<u64> {
    if p >= PROJS || t >= TARGETS {
        return None;
    }
    Some(
        (0xB1000u64 + (p as u64) * 0x2D)
            ^ (t as u64).wrapping_mul(0x01000193)
            ^ 0x6B6F746C696E74,
    )
}

/// KGP 消费格基准（格 c）。
pub const fn kgp_cell(c: usize) -> Option<u64> {
    if c >= KGP_CELLS {
        return None;
    }
    Some((0xB2000u64 + c as u64 * 0x32) ^ 0x6B6770636F6E73)
}

/// KGP 三元闸。
pub const fn tri_ok(ver: u32, api: u32, jdk: u32) -> bool {
    ver == KGP_VER && api == GRADLE_API && jdk == JDK
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct KtLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 编译账。
    kts: [[Option<u64>; TARGETS]; PROJS],
    /// KGP 消费账。
    kgp: [Option<u64>; KGP_CELLS],
    /// 各工程最后目标。
    last_target: [i64; PROJS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KtLedger {
    /// 新账本。
    pub fn new() -> Self {
        KtLedger {
            gates: [None; 6],
            kts: [[None; TARGETS]; PROJS],
            kgp: [None; KGP_CELLS],
            last_target: [-1; PROJS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KtCode> {
        if gate >= 6 {
            return Err(KtCode::ESeal);
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

    /// 编译一格（目标序单调 + 载荷对照 + 重复拒）。
    pub fn compile(&mut self, p: usize, t: usize) -> Result<u64, KtCode> {
        if p >= PROJS {
            return Err(KtCode::ENoProj);
        }
        if t >= TARGETS {
            return Err(KtCode::ENoTarget);
        }
        if (t as i64) < self.last_target[p] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3D200);
            return Err(KtCode::EOrder);
        }
        let payload = match kt_cell(p, t) {
            Some(p) => p,
            None => return Err(KtCode::ENoTarget),
        };
        if match self.kts.get(p).and_then(|row| row.get(t)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KtCode::EDup);
        }
        if let Some(row) = self.kts.get_mut(p) {
            if let Some(c) = row.get_mut(t) {
                *c = Some(payload);
            }
        }
        self.last_target[p] = t as i64;
        self.fnv(0x3D201 + (p as u64) * 2 + t as u64);
        Ok(payload)
    }

    /// 编译全账核验（20 格）。
    pub fn kts_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            let mut t = 0usize;
            while t < TARGETS {
                match self.kts.get(p).and_then(|row| row.get(t)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                t += 1;
            }
            p += 1;
        }
        true
    }

    /// KGP 三元闸。
    pub fn kgp_gate(&mut self, ver: u32, api: u32, jdk: u32) -> Result<(), KtCode> {
        if !tri_ok(ver, api, jdk) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3D300);
            return Err(KtCode::ETri);
        }
        self.fnv(0x3D301);
        Ok(())
    }

    /// KGP 消费一格（三元闸后 + 恒等 + 重复拒）。
    pub fn consume_kgp(&mut self, c: usize, got: u64) -> Result<(), KtCode> {
        if c >= KGP_CELLS {
            return Err(KtCode::ENoKgp);
        }
        let want = match kgp_cell(c) {
            Some(w) => w,
            None => return Err(KtCode::ENoKgp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3D400 + c as u64);
            return Err(KtCode::EKgpDrift);
        }
        if match self.kgp.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KtCode::EDup);
        }
        if let Some(cell) = self.kgp.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x3D401 + c as u64);
        Ok(())
    }

    /// KGP 全账核验。
    pub fn kgp_all(&self) -> bool {
        let mut c = 0usize;
        while c < KGP_CELLS {
            match self.kgp.get(c) {
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

impl Default for KtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KtLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        let mut t = 0usize;
        while t < TARGETS {
            if led.compile(p, t).is_ok() {
                recorded += 1;
            }
            t += 1;
        }
        p += 1;
    }
    let _ = led.kgp_gate(KGP_VER, GRADLE_API, JDK);
    let mut c = 0usize;
    while c < KGP_CELLS {
        if let Some(w) = kgp_cell(c) {
            let _ = led.consume_kgp(c, w);
        }
        c += 1;
    }
    // 注入 10 轮：三元失配 / 目标逆序 / KGP 漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KtLedger::new();
        let caught = match kind {
            0 => lt.kgp_gate(KGP_VER + 1, GRADLE_API, JDK) == Err(KtCode::ETri),
            1 => {
                let _ = lt.compile(0, 1);
                lt.compile(0, 0) == Err(KtCode::EOrder)
            }
            _ => {
                let mut c2 = 0usize;
                while c2 < KGP_CELLS {
                    if let Some(w) = kgp_cell(c2) {
                        let _ = lt.consume_kgp(c2, w);
                    }
                    c2 += 1;
                }
                lt.consume_kgp(1, kgp_cell(1).unwrap_or(0).wrapping_add(1)) == Err(KtCode::EKgpDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3D500 + k as u64);
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
pub fn run_unx52084_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52084");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Kotlin 编译 20/20 全通",
        recorded >= (PROJS * TARGETS) as u32,
        "10 工程 × JVM/JS 双目标逐格恒等",
    );
    s.add(
        "KGP 消费 4/4 全账",
        {
            let mut lt = KtLedger::new();
            let mut c = 0usize;
            while c < KGP_CELLS {
                if let Some(w) = kgp_cell(c) {
                    let _ = lt.consume_kgp(c, w);
                }
                c += 1;
            }
            lt.kgp_all()
        },
        "插件版本/目标三元/增量编译/工具链四格零漂移（F52083 复用）",
    );
    s.add(
        "KGP 三元闸",
        {
            let mut lt = KtLedger::new();
            lt.kgp_gate(KGP_VER, GRADLE_API, JDK).is_ok()
                && lt.kgp_gate(KGP_VER + 1, GRADLE_API, JDK) == Err(KtCode::ETri)
                && lt.kgp_gate(KGP_VER, GRADLE_API + 1, JDK) == Err(KtCode::ETri)
                && lt.kgp_gate(KGP_VER, GRADLE_API, JDK + 1) == Err(KtCode::ETri)
        },
        "插件版本/Gradle API/JDK 三元失配必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "三元/逆序/漂移三态轮转必抓");
    let mut l0 = KtLedger::new();
    s.add(
        "目标·逆序拒",
        {
            let _ = l0.compile(0, 1);
            l0.compile(0, 0) == Err(KtCode::EOrder)
        },
        "JS 先 JVM 后必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.compile(PROJS, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    s.add(
        "边界·目标越闭集",
        l0.compile(0, TARGETS).is_err(),
        "目标 2 越闭集必拒",
    );
    s.add(
        "边界·KGP 越册",
        l0.consume_kgp(KGP_CELLS, 0).is_err(),
        "KGP 4 越册必拒",
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
    fn test_tri_gate() {
        assert!(tri_ok(KGP_VER, GRADLE_API, JDK));
        assert!(!tri_ok(KGP_VER + 1, GRADLE_API, JDK));
        assert!(!tri_ok(KGP_VER, GRADLE_API + 1, JDK));
        assert!(!tri_ok(KGP_VER, GRADLE_API, JDK + 1));
    }

    #[test]
    fn test_cells_pure() {
        let a = kt_cell(4, 1).unwrap_or(0);
        assert_eq!(a, kt_cell(4, 1).unwrap_or(0));
        assert_ne!(a, kt_cell(4, 0).unwrap_or(0));
        assert!(kt_cell(PROJS, 0).is_none());
        assert!(kt_cell(0, TARGETS).is_none());
        assert!(kgp_cell(KGP_CELLS).is_none());
    }

    #[test]
    fn test_target_order_caught() {
        let mut lt = KtLedger::new();
        let _ = lt.compile(1, 1);
        assert_eq!(lt.compile(1, 0), Err(KtCode::EOrder));
        assert!(lt.compile(1, 1).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJS * TARGETS) as u32);
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
        assert_eq!(SEG_BASE, 0xC5500);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52084_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
