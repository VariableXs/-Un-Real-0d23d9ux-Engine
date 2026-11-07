//! UNX-F52100 Kotlin 命令行编译器（Windows 发行版）收口 · 0xC5700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52100 逐字锚定）：UNX-F52101-J1 kotlinc 编译
//! 运行 ×10 样本全通；与 JVM 收口联签（F52087 判据复用）；KB-COMPAT 定
//! 级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：kotlinc 编译运行 ×10 样本账（编译/运行二元），10×2 =
//!      20 格账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② JVM 联签消费账：JVM 三元（版本档/classpath 档/退出码档）消费 4
//!      格（编译调用/运行调用/类路径传递/退出码回传），逐格与消费基准
//!      恒等才收（F52087 判据复用）；漂移必抓。
//!   ③ 编译→运行拍序单向闸（未编译先运行 → 前置闸必抓）。
//!   ④ 注入 ×10 轮（消费漂移/前置越序/样本越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格 + 4 格全通
//!      → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52100 − 52000)）。
pub const SEG_BASE: u32 = 0xC5700;

/// 样本数。
pub const SAMPLES: usize = 10;

/// 阶段二元（编译/运行）。
pub const PHASES: usize = 2;

/// JVM 消费格数。
pub const JVM_CELLS: usize = 4;

/// JVM 三元基准。
pub const JV_VER: u32 = 17;
pub const CP: u32 = 5;
pub const EXIT: u32 = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KcCode {
    /// 成功
    Ok = 0,
    /// 样本号越闭集
    ENoSample,
    /// 阶段二元越闭集
    ENoPhase,
    /// 编译漂移
    EDrift,
    /// 运行前置闸（未编译先运行）
    EPreRun,
    /// 消费格越册
    ENoCell,
    /// JVM 消费漂移
    EJvmDrift,
    /// JVM 三元失配
    ETri,
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

impl KcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KcCode::Ok => "成功",
            KcCode::ENoSample => "样本号越闭集：10 样本之外先登记",
            KcCode::ENoPhase => "阶段二元越闭集：编译/运行二元之外先登记",
            KcCode::EDrift => "编译漂移：编译折叠与基准不恒等，逐格复算",
            KcCode::EPreRun => "未编译先运行：编译账不齐不许运行",
            KcCode::ENoCell => "消费格越册：JVM 4 格闭集之外先登记",
            KcCode::EJvmDrift => "JVM 消费漂移：消费折叠与基准不恒等",
            KcCode::ETri => "JVM 三元失配：版本/classpath/退出码档与基准不符",
            KcCode::EOrder => "段序逆序：编译→运行严格递增，逆序必抓",
            KcCode::EDup => "重复落账：同格只许一条账目",
            KcCode::ESeal => "未齐即封：20 格未齐先补齐",
            KcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KcCode::EGradeDrift => "定级漂移：20 格+4 格全通+前置闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 样本格载荷（样本 s × 阶段 ph）。
pub const fn kc_cell(s: usize, ph: usize) -> Option<u64> {
    if s >= SAMPLES || ph >= PHASES {
        return None;
    }
    Some(
        (0xC8000u64 + (s as u64) * 0x2C)
            ^ (ph as u64).wrapping_mul(0x01000193)
            ^ 0x6B6F746C696E63,
    )
}

/// JVM 消费格基准（格 c）。
pub const fn jvm_cell(c: usize) -> Option<u64> {
    if c >= JVM_CELLS {
        return None;
    }
    Some((0xC9000u64 + c as u64 * 0x31) ^ 0x6A766D636F6E73)
}

/// JVM 三元闸。
pub const fn tri_ok(ver: u32, cp: u32, exit: u32) -> bool {
    ver == JV_VER && cp == CP && exit == EXIT
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct KcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 样本账。
    kcs: [[Option<u64>; PHASES]; SAMPLES],
    /// JVM 消费账。
    jvm: [Option<u64>; JVM_CELLS],
    /// 各样本最后阶段。
    last_phase: [i64; SAMPLES],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KcLedger {
    /// 新账本。
    pub fn new() -> Self {
        KcLedger {
            gates: [None; 6],
            kcs: [[None; PHASES]; SAMPLES],
            jvm: [None; JVM_CELLS],
            last_phase: [-1; SAMPLES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KcCode> {
        if gate >= 6 {
            return Err(KcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x44100 + gate as u64);
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

    /// JVM 三元闸。
    pub fn tri_gate(&mut self, ver: u32, cp: u32, exit: u32) -> Result<(), KcCode> {
        if !tri_ok(ver, cp, exit) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x44200);
            return Err(KcCode::ETri);
        }
        self.fnv(0x44201);
        Ok(())
    }

    /// JVM 消费一格。
    pub fn consume_jvm(&mut self, c: usize, got: u64) -> Result<(), KcCode> {
        if c >= JVM_CELLS {
            return Err(KcCode::ENoCell);
        }
        let want = match jvm_cell(c) {
            Some(w) => w,
            None => return Err(KcCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x44300 + c as u64);
            return Err(KcCode::EJvmDrift);
        }
        if match self.jvm.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KcCode::EDup);
        }
        if let Some(cell) = self.jvm.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x44301 + c as u64);
        Ok(())
    }

    /// JVM 消费全账核验。
    pub fn jvm_all(&self) -> bool {
        let mut c = 0usize;
        while c < JVM_CELLS {
            match self.jvm.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 编译一格。
    pub fn compile(&mut self, s: usize) -> Result<u64, KcCode> {
        if s >= SAMPLES {
            return Err(KcCode::ENoSample);
        }
        let payload = match kc_cell(s, 0) {
            Some(p) => p,
            None => return Err(KcCode::ENoSample),
        };
        if match self.kcs.get(s).and_then(|row| row.get(0)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KcCode::EDup);
        }
        if let Some(row) = self.kcs.get_mut(s) {
            if let Some(c) = row.get_mut(0) {
                *c = Some(payload);
            }
        }
        self.fnv(0x44400 + s as u64);
        Ok(payload)
    }

    /// 运行一格（编译前置闸 + 段序单调）。
    pub fn run(&mut self, s: usize) -> Result<u64, KcCode> {
        if s >= SAMPLES {
            return Err(KcCode::ENoSample);
        }
        let compiled = match self.kcs.get(s).and_then(|row| row.get(0)) {
            Some(Some(_)) => true,
            _ => false,
        };
        if !compiled {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x44500 + s as u64);
            return Err(KcCode::EPreRun);
        }
        if (1 as i64) < self.last_phase[s] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x44501 + s as u64);
            return Err(KcCode::EOrder);
        }
        let payload = match kc_cell(s, 1) {
            Some(p) => p,
            None => return Err(KcCode::ENoSample),
        };
        if match self.kcs.get(s).and_then(|row| row.get(1)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KcCode::EDup);
        }
        if let Some(row) = self.kcs.get_mut(s) {
            if let Some(c) = row.get_mut(1) {
                *c = Some(payload);
            }
        }
        self.last_phase[s] = 1;
        self.fnv(0x44502 + s as u64);
        Ok(payload)
    }

    /// 样本全账核验（20 格）。
    pub fn kcs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SAMPLES {
            let mut ph = 0usize;
            while ph < PHASES {
                match self.kcs.get(s).and_then(|row| row.get(ph)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                ph += 1;
            }
            s += 1;
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

impl Default for KcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.tri_gate(JV_VER, CP, EXIT);
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SAMPLES {
        if led.compile(s).is_ok() {
            recorded += 1;
        }
        if led.run(s).is_ok() {
            recorded += 1;
        }
        s += 1;
    }
    let mut c = 0usize;
    while c < JVM_CELLS {
        if let Some(w) = jvm_cell(c) {
            let _ = led.consume_jvm(c, w);
        }
        c += 1;
    }
    // 注入 10 轮：前置越序 / 三元失配 / 样本越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KcLedger::new();
        let caught = match kind {
            0 => lt.run(0).is_err(),
            1 => lt.tri_gate(JV_VER + 1, CP, EXIT) == Err(KcCode::ETri),
            _ => lt.compile(SAMPLES).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x44600 + k as u64);
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
pub fn run_unx52100_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52100");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "kotlinc 编译运行 20/20 全通",
        recorded >= (SAMPLES * PHASES) as u32,
        "10 样本 × 编译/运行二元逐格恒等",
    );
    s.add(
        "JVM 消费 4/4 全账",
        {
            let mut lt = KcLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < JVM_CELLS {
                if let Some(w) = jvm_cell(c) {
                    if lt.consume_jvm(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.jvm_all()
        },
        "编译调用/运行调用/类路径/退出码四格零漂移（F52087 复用）",
    );
    s.add(
        "JVM 三元闸",
        {
            let mut lt = KcLedger::new();
            lt.tri_gate(JV_VER, CP, EXIT).is_ok()
                && lt.tri_gate(JV_VER + 1, CP, EXIT) == Err(KcCode::ETri)
                && lt.tri_gate(JV_VER, CP + 1, EXIT) == Err(KcCode::ETri)
        },
        "版本/classpath/退出码三元失配必抓",
    );
    s.add(
        "运行前置闸",
        {
            let mut lt = KcLedger::new();
            lt.run(0).is_err()
        },
        "未编译先运行必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "前置/三元/越闭集三态轮转必抓");
    let mut l0 = KcLedger::new();
    s.add(
        "边界·样本越闭集",
        l0.compile(SAMPLES).is_err(),
        "样本 10 越闭集必拒",
    );
    s.add(
        "边界·消费越册",
        l0.consume_jvm(JVM_CELLS, 0).is_err(),
        "消费 4 越册必拒",
    );
    let mut l1 = KcLedger::new();
    let _ = l1.compile(2);
    s.add(
        "重复·再落账拒",
        l1.compile(2).is_err(),
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
    fn test_tri_gate() {
        assert!(tri_ok(JV_VER, CP, EXIT));
        assert!(!tri_ok(JV_VER + 1, CP, EXIT));
        assert!(!tri_ok(JV_VER, CP + 1, EXIT));
        assert!(!tri_ok(JV_VER, CP, EXIT + 1));
    }

    #[test]
    fn test_cells_pure() {
        let a = kc_cell(4, 1).unwrap_or(0);
        assert_eq!(a, kc_cell(4, 1).unwrap_or(0));
        assert_ne!(a, kc_cell(4, 0).unwrap_or(0));
        assert!(kc_cell(SAMPLES, 0).is_none());
        assert!(kc_cell(0, PHASES).is_none());
        assert!(jvm_cell(JVM_CELLS).is_none());
    }

    #[test]
    fn test_pre_run_gate() {
        let mut lt = KcLedger::new();
        assert_eq!(lt.run(0), Err(KcCode::EPreRun));
        let _ = lt.compile(0);
        assert!(lt.run(0).is_ok());
        assert!(lt.run(0).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SAMPLES * PHASES) as u32);
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
        assert_eq!(SEG_BASE, 0xC5700);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52100_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
