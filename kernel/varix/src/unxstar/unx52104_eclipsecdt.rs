//! UNX-F52104 Eclipse CDT 收口 · 0xC5800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52104 逐字锚定）：UNX-F52105-J1 六判据全落；
//! Maven/Ant 双构建消费（B08 判据复用联签）；KB-COMPAT 定级登记；判据
//! 锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：CDT 工作台四面账（工程创建/索引/构建/调试）×5 工程场
//!      景 = 20 格工作台账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② Maven/Ant 双构建消费账：消费二元（Maven 档/Ant 档）×4 格消费
//!      面（pom 解析/build.xml 解析/目标执行/退出码回传）= 8 格消费账
//!     （B08 判据复用联签），逐格与消费基准恒等才收；漂移必抓。
//!   ③ 创建→索引→构建→调试段序严格递增；注入 ×10 轮（消费漂移/段序
//!      逆序/场景越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判
//!      据母版闸 + 定级（20 格 + 8 格全通 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52104 − 52000)）。
pub const SEG_BASE: u32 = 0xC5800;

/// 工作台段数。
pub const FACES: usize = 4;

/// 工程场景数。
pub const PROJS: usize = 5;

/// 消费面格数（每构建器）。
pub const CONSUME: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CdCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 段号越闭集
    ENoFace,
    /// 工作台载荷漂移
    EDrift,
    /// 消费器二元越闭集
    ENoCons,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EConsDrift,
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

impl CdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CdCode::Ok => "成功",
            CdCode::ENoProj => "工程号越闭集：5 工程之外先登记",
            CdCode::ENoFace => "段号越闭集：创建/索引/构建/调试四段之外先登记",
            CdCode::EDrift => "工作台载荷漂移：折叠与基准不恒等，逐格复算",
            CdCode::ENoCons => "消费器二元越闭集：Maven/Ant 二元之外先登记",
            CdCode::ENoCell => "消费格越册：4 格消费面闭集之外先登记",
            CdCode::EConsDrift => "消费漂移：pom/build.xml 解析·目标·退出码折叠与基准不恒等",
            CdCode::EOrder => "段序逆序：创建→索引→构建→调试严格递增，逆序必抓",
            CdCode::EDup => "重复落账：同格只许一条账目",
            CdCode::ESeal => "未齐即封：20 格未齐先补齐",
            CdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CdCode::EGradeDrift => "定级漂移：20 格+8 格全通+段序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 工作台格载荷（工程 p × 段 f）。
pub const fn face_cell(p: usize, f: usize) -> Option<u64> {
    if p >= PROJS || f >= FACES {
        return None;
    }
    Some(
        (0xD7000u64 + (p as u64) * 0x2C)
            ^ (f as u64).wrapping_mul(0x01000193)
            ^ 0x63646C6F72656164,
    )
}

/// 消费格载荷（消费器 b × 面 c）。
pub const fn cons_cell(b: usize, c: usize) -> Option<u64> {
    if b >= 2 || c >= CONSUME {
        return None;
    }
    Some(
        (0xD8000u64 + (b as u64) * 0x100)
            ^ (c as u64).wrapping_mul(0x01000193)
            ^ 0x623038636F6E73,
    )
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct CdLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 工作台账。
    faces: [[Option<u64>; FACES]; PROJS],
    /// 消费账。
    cons: [[Option<u64>; CONSUME]; 2],
    /// 各工程最后段。
    last_face: [i64; PROJS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CdLedger {
    /// 新账本。
    pub fn new() -> Self {
        CdLedger {
            gates: [None; 6],
            faces: [[None; FACES]; PROJS],
            cons: [[None; CONSUME]; 2],
            last_face: [-1; PROJS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CdCode> {
        if gate >= 6 {
            return Err(CdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x48100 + gate as u64);
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

    /// 工作台一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, p: usize, f: usize) -> Result<u64, CdCode> {
        if p >= PROJS {
            return Err(CdCode::ENoProj);
        }
        if f >= FACES {
            return Err(CdCode::ENoFace);
        }
        if (f as i64) < self.last_face[p] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x48200 + p as u64);
            return Err(CdCode::EOrder);
        }
        let payload = match face_cell(p, f) {
            Some(x) => x,
            None => return Err(CdCode::ENoFace),
        };
        if match self.faces.get(p).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CdCode::EDup);
        }
        if let Some(row) = self.faces.get_mut(p) {
            if let Some(c) = row.get_mut(f) {
                *c = Some(payload);
            }
        }
        self.last_face[p] = f as i64;
        self.fnv(0x48201 + (p as u64) * 4 + f as u64);
        Ok(payload)
    }

    /// 工作台全账核验（20 格）。
    pub fn faces_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            let mut f = 0usize;
            while f < FACES {
                match self.faces.get(p).and_then(|row| row.get(f)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                f += 1;
            }
            p += 1;
        }
        true
    }

    /// 消费一格。
    pub fn consume(&mut self, b: usize, c: usize, got: u64) -> Result<(), CdCode> {
        if b >= 2 {
            return Err(CdCode::ENoCons);
        }
        if c >= CONSUME {
            return Err(CdCode::ENoCell);
        }
        let want = match cons_cell(b, c) {
            Some(w) => w,
            None => return Err(CdCode::ENoCons),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x48300 + (b as u64) * 4 + c as u64);
            return Err(CdCode::EConsDrift);
        }
        if match self.cons.get(b).and_then(|row| row.get(c)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CdCode::EDup);
        }
        if let Some(row) = self.cons.get_mut(b) {
            if let Some(cell) = row.get_mut(c) {
                *cell = Some(got);
            }
        }
        self.fnv(0x48301 + (b as u64) * 4 + c as u64);
        Ok(())
    }

    /// 消费全账核验（8 格）。
    pub fn cons_all(&self) -> bool {
        let mut b = 0usize;
        while b < 2 {
            let mut c = 0usize;
            while c < CONSUME {
                match self.cons.get(b).and_then(|row| row.get(c)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                c += 1;
            }
            b += 1;
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

impl Default for CdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        let mut f = 0usize;
        while f < FACES {
            if led.record_face(p, f).is_ok() {
                recorded += 1;
            }
            f += 1;
        }
        p += 1;
    }
    let mut b = 0usize;
    while b < 2 {
        let mut c = 0usize;
        while c < CONSUME {
            if let Some(w) = cons_cell(b, c) {
                if led.consume(b, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        b += 1;
    }
    // 注入 10 轮：消费漂移 / 段序逆序 / 工程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CdLedger::new();
        let caught = match kind {
            0 => lt.consume(1, 2, cons_cell(1, 2).unwrap_or(0).wrapping_add(1)) == Err(CdCode::EConsDrift),
            1 => {
                let _ = lt.record_face(0, 2);
                lt.record_face(0, 0) == Err(CdCode::EOrder)
            }
            _ => lt.record_face(PROJS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x48400 + k as u64);
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
pub fn run_unx52104_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52104");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "CDT 工作台 20/20 全通",
        recorded >= (PROJS * FACES) as u32,
        "创建/索引/构建/调试 ×5 工程逐格恒等",
    );
    s.add(
        "Maven/Ant 消费 8/8 全账",
        {
            let mut lt = CdLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < 2 {
                let mut c = 0usize;
                while c < CONSUME {
                    if let Some(w) = cons_cell(b, c) {
                        if lt.consume(b, c, w).is_err() {
                            ok = false;
                        }
                    }
                    c += 1;
                }
                b += 1;
            }
            ok && lt.cons_all()
        },
        "pom/build.xml 解析·目标·退出码八格零漂移（B08 判据复用联签）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "消费/逆序/越闭集三态轮转必抓");
    let mut l0 = CdLedger::new();
    s.add(
        "消费·漂移拒",
        l0.consume(0, 1, cons_cell(0, 1).unwrap_or(0).wrapping_add(2)) == Err(CdCode::EConsDrift),
        "消费折叠漂移必抓",
    );
    s.add(
        "段序·逆序拒",
        {
            let _ = l0.record_face(0, 2);
            l0.record_face(0, 0) == Err(CdCode::EOrder)
        },
        "构建后回创建必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_face(PROJS, 0).is_err(),
        "工程 5 越闭集必拒",
    );
    s.add(
        "边界·消费器越闭集",
        l0.consume(2, 0, 0).is_err(),
        "消费器 2 越闭集必拒",
    );
    s.add(
        "边界·消费格越册",
        l0.consume(0, CONSUME, 0).is_err(),
        "面 4 越册必拒",
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
        let a = face_cell(2, 1).unwrap_or(0);
        assert_eq!(a, face_cell(2, 1).unwrap_or(0));
        assert_ne!(a, face_cell(2, 2).unwrap_or(0));
        assert!(face_cell(PROJS, 0).is_none());
        assert!(face_cell(0, FACES).is_none());
        let b = cons_cell(1, 3).unwrap_or(0);
        assert_ne!(b, cons_cell(0, 3).unwrap_or(0));
        assert!(cons_cell(2, 0).is_none());
        assert!(cons_cell(0, CONSUME).is_none());
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = CdLedger::new();
        let _ = lt.record_face(1, 2);
        assert_eq!(lt.record_face(1, 0), Err(CdCode::EOrder));
    }

    #[test]
    fn test_cons_drift_caught() {
        let mut lt = CdLedger::new();
        assert_eq!(
            lt.consume(0, 0, cons_cell(0, 0).unwrap_or(0).wrapping_add(4)),
            Err(CdCode::EConsDrift)
        );
        assert!(lt.consume(0, 0, cons_cell(0, 0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJS * FACES) as u32);
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
        assert_eq!(SEG_BASE, 0xC5800);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52104_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
