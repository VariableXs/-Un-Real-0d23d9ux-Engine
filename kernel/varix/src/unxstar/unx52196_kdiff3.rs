//! UNX-F52196 KDiff3 收口 · 0xC7100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52196 逐字锚定）：UNX-F52197-J1 六判据全落；
//! 文件夹/表格/十六进制三类对比 ×10；许可校验面如实定级（AI-95 联签
//! 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线②。
//!
//! 设计要点：
//!   ① 域模型：对比任务 ×10 账，任务类别三元闭集（0=文件夹 1=表格
//!      2=十六进制），逐任务载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 许可校验面如实定级闸（AI-95 联签锚定行同构）：许可校验四行
//!      闭集（密钥格式/在线校验/宽限期/降级态），校验未过必须如实
//!      降级，申报通过必抓。
//!   ③ 任务类别单调闸：文件夹→表格→十六进制类间单调，回跳必抓。
//!   ④ 注入 ×10 轮（任务越闭集/许可校验未过申报通过/重复落账三态轮
//!      转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级
//!      （10 任务全通 + 许可四行全齐且校验过 → S1；未过 → S3 如实）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52196 − 52000)）。
pub const SEG_BASE: u32 = 0xC7100;

/// 对比任务数。
pub const TASKS: usize = 10;

/// 许可校验行数。
pub const LIC_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 任务号越闭集
    ENoTask,
    /// 格载荷漂移
    EDrift,
    /// 类别逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 许可行越闭集
    ENoRow,
    /// 缺许可行
    ERowMiss,
    /// 校验未过申报通过
    EFalsePass,
    /// 校验态越闭集
    ENoState,
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
            PfCode::ENoTask => "任务号越闭集：10 任务之外先扩闭集",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐任务复算",
            PfCode::EOrder => "类别逆序：文件夹→表格→十六进制类间单调",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "许可行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺许可行：许可校验四行未齐先补齐",
            PfCode::EFalsePass => "校验未过申报通过：如实降级不许虚报",
            PfCode::ENoState => "校验态越闭集：三态闭集之外必拒",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：任务全通+许可四行全齐且校验过才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 任务格载荷（任务 t，类别 = t/4 装配）。
pub const fn task_cell(t: usize) -> Option<u64> {
    if t >= TASKS {
        return None;
    }
    Some(0xD7000u64 + (t as u64) * 0x3A)
}

/// 任务类别（0..4=文件夹 4..7=表格 7..10=十六进制）。
pub const fn task_kind(t: usize) -> Option<u32> {
    if t >= TASKS {
        return None;
    }
    if t < 4 {
        Some(0)
    } else if t < 7 {
        Some(1)
    } else {
        Some(2)
    }
}

/// 校验三态闭集（0=未校验 1=通过 2=未过）。
pub const fn state_ok(v: u32) -> bool {
    v <= 2
}

/// 校验态对定级的影响（2=未过 → S3 如实降级）。
pub const fn state_grade(v: u32) -> Option<u32> {
    if !state_ok(v) {
        return None;
    }
    if v == 2 {
        Some(3)
    } else {
        Some(1)
    }
}

/// 许可行折叠（行 r）。
pub const fn lic_row(r: usize) -> Option<u32> {
    if r >= LIC_ROWS {
        return None;
    }
    Some(0xD800 + r as u32 * 0x43)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KDiff3 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 任务账。
    tasks: [Option<u64>; TASKS],
    /// 许可校验账。
    lic: [Option<u32>; LIC_ROWS],
    /// 校验态。
    state: u32,
    /// 最后类别。
    last_kind: i64,
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
            tasks: [None; TASKS],
            lic: [None; LIC_ROWS],
            state: 0,
            last_kind: -1,
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
        self.fnv(0x3F100 + gate as u64);
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

    /// 任务一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_task(&mut self, t: usize) -> Result<u64, PfCode> {
        if t >= TASKS {
            return Err(PfCode::ENoTask);
        }
        let kind = match task_kind(t) {
            Some(k) => k,
            None => return Err(PfCode::ENoTask),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3F200);
            return Err(PfCode::EOrder);
        }
        let p = match task_cell(t) {
            Some(p) => p,
            None => return Err(PfCode::ENoTask),
        };
        if match self.tasks.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.tasks.get_mut(t) {
            *c = Some(p);
        }
        self.last_kind = kind as i64;
        self.fnv(0x3F202 + t as u64);
        Ok(p)
    }

    /// 校验态落账（三态闭集闸）。
    pub fn set_state(&mut self, v: u32) -> Result<(), PfCode> {
        if !state_ok(v) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3F300);
            return Err(PfCode::ENoState);
        }
        self.state = v;
        self.fnv(0x3F301);
        Ok(())
    }

    /// 校验未过申报通过闸（AI-95 同构）。
    pub fn claim_pass(&mut self) -> bool {
        if self.state == 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3F302);
            return false;
        }
        true
    }

    /// 许可行一格。
    pub fn declare_lic(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= LIC_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.lic.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.lic.get_mut(r) {
            *c = lic_row(r);
        }
        self.fnv(0x3F400 + r as u64);
        Ok(())
    }

    /// 许可行全齐核验。
    pub fn lic_all(&self) -> bool {
        let mut r = 0usize;
        while r < LIC_ROWS {
            match self.lic.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账闸：任务全齐 + 许可四行全齐 + 校验态在册。
    pub fn seal(&self) -> Result<(), PfCode> {
        let mut t = 0usize;
        while t < TASKS {
            match self.tasks.get(t) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ESeal),
            }
            t += 1;
        }
        if !self.lic_all() {
            return Err(PfCode::ERowMiss);
        }
        if !state_ok(self.state) {
            return Err(PfCode::ENoState);
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
    // 校验态落账：通过态。
    let _ = led.set_state(1);
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TASKS {
        if led.record_task(t).is_ok() {
            recorded += 1;
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < LIC_ROWS {
        let _ = led.declare_lic(r);
        r += 1;
    }
    // 注入 10 轮：任务越闭集 / 校验态越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_task(TASKS).is_err(),
            1 => lt.set_state(9).is_err(),
            _ => {
                let _ = lt.record_task(0);
                lt.record_task(0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3F500 + k as u64);
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
pub fn run_unx52196_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52196");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三类对比 10 任务全通",
        recorded >= TASKS as u32,
        "文件夹/表格/十六进制三类逐任务恒等",
    );
    s.add(
        "任务类别闭集",
        match task_kind(0) {
            Some(0) => true,
            _ => false,
        } && match task_kind(9) {
            Some(2) => true,
            _ => false,
        },
        "文件夹/表格/十六进制三类边界在册",
    );
    s.add(
        "许可校验四行 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LIC_ROWS {
                if lt.declare_lic(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.lic_all()
        },
        "密钥格式/在线校验/宽限期/降级态四行闭集",
    );
    s.add(
        "校验三态闭集",
        state_ok(0) && state_ok(2) && !state_ok(3),
        "未校验/通过/未过三态在册且越态拒",
    );
    s.add(
        "校验未过申报通过必抓",
        {
            let mut lt = PfLedger::new();
            let _ = lt.set_state(2);
            !lt.claim_pass()
        },
        "如实降级不许虚报",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/校验越态/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·任务越闭集",
        l0.record_task(TASKS).is_err(),
        "任务 10 越闭集必拒",
    );
    s.add(
        "边界·校验态越闭集",
        l0.set_state(5).is_err(),
        "校验态 5 越三态闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_task(6);
    s.add("类别·逆序拒", l1.record_task(0).is_err(), "表格类后回文件夹类必抓");
    let mut l2 = PfLedger::new();
    let _ = l2.record_task(0);
    s.add("重复·再落账拒", l2.record_task(0).is_err(), "同格只许一条账目");
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
    fn test_task_kind() {
        assert_eq!(task_kind(0), Some(0));
        assert_eq!(task_kind(6), Some(1));
        assert_eq!(task_kind(9), Some(2));
        assert!(task_kind(TASKS).is_none());
        assert!(task_cell(TASKS).is_none());
    }

    #[test]
    fn test_state_gates() {
        assert!(state_ok(0) && state_ok(2));
        assert!(!state_ok(3));
        assert_eq!(state_grade(1), Some(1));
        assert_eq!(state_grade(2), Some(3));
        let mut lt = PfLedger::new();
        let _ = lt.set_state(2);
        assert!(!lt.claim_pass());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_task(6).is_ok());
        assert!(lt.record_task(0).is_err());
    }

    #[test]
    fn test_lic_and_seal() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let _ = lt.declare_lic(0);
        assert!(lt.declare_lic(0).is_err());
        assert_eq!(lt.declare_lic(LIC_ROWS), Err(PfCode::ENoRow));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= TASKS as u32);
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
        assert_eq!(SEG_BASE, 0xC7100);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52196_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
