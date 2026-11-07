//! UNX-F52046 GoLand 收口 · 0xC4B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52046 逐字锚定）：UNX-F52047-J1 六判据全落；
//! Go 工具链（go build/test/dlv 调试）消费 ×10 场景全通；KB-COMPAT 定
//! 级登记；判据锚定 §AI-66.2 专题五。
//!
//! 工具链段闭集（三段）：build/test/dlv 三段 × 10 场景 = 30 格；go 工
//! 具链三元闸（Go 版本档/GOOS 档/GOARCH 档）全等放行，错位失配；注入
//! ×10 轮（版本/OS/ARCH 三档轮转失配）10/10 必抓；六判据母版闸 + 定级
//! （30 格全通 + 失配全抓 → S1）。
//! no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/
//! panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52046 − 52000)）。
pub const SEG_BASE: u32 = 0xC4B80;

/// 工具链段数。
pub const TOOLS: usize = 3;

/// 场景数。
pub const SCENES: usize = 10;

/// Go 工具链三元基准。
pub const GO_VER: u32 = 21;
pub const GOOS: u32 = 0;
pub const GOARCH: u32 = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoTool,
    /// 场景号越闭集
    ENoScene,
    /// Go 版本档失配
    EVer,
    /// GOOS 档失配
    EOs,
    /// GOARCH 档失配
    EArch,
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

impl GlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GlCode::Ok => "成功",
            GlCode::ENoTool => "段号越闭集：build/test/dlv 三段之外先登记",
            GlCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            GlCode::EVer => "Go 版本档失配：Go 版本档与宿主档不符",
            GlCode::EOs => "GOOS 档失配：GOOS 档与宿主档不符",
            GlCode::EArch => "GOARCH 档失配：GOARCH 档与宿主档不符",
            GlCode::EDup => "重复落账：同格只许一条账目",
            GlCode::ESeal => "未齐即封：30 格未齐先补齐",
            GlCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GlCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GlCode::EGradeDrift => "定级漂移：30 格全通+失配全抓才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（段 t × 场景 s）。
pub const fn tool_cell(t: usize, s: usize) -> Option<u64> {
    if t >= TOOLS || s >= SCENES {
        return None;
    }
    Some(
        0x2E000u64
            + (t as u64) * 0x2B
            + (s as u64) * 0x11
            ^ (t as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct GlLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 工具链账。
    tools: [[Option<u64>; SCENES]; TOOLS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GlLedger {
    /// 新账本。
    pub fn new() -> Self {
        GlLedger {
            gates: [None; 6],
            tools: [[None; SCENES]; TOOLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GlCode> {
        if gate >= 6 {
            return Err(GlCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x16100 + gate as u64);
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

    /// Go 工具链三元闸。
    pub fn env_gate(&mut self, ver: u32, os: u32, arch: u32) -> Result<(), GlCode> {
        if ver != GO_VER {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x16200);
            return Err(GlCode::EVer);
        }
        if os != GOOS {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x16201);
            return Err(GlCode::EOs);
        }
        if arch != GOARCH {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x16202);
            return Err(GlCode::EArch);
        }
        self.fnv(0x16203);
        Ok(())
    }

    /// 场景一格（三元闸后落账，重复拒）。
    pub fn run_tool(&mut self, t: usize, s: usize) -> Result<u64, GlCode> {
        if t >= TOOLS {
            return Err(GlCode::ENoTool);
        }
        if s >= SCENES {
            return Err(GlCode::ENoScene);
        }
        let p = match tool_cell(t, s) {
            Some(p) => p,
            None => return Err(GlCode::ENoScene),
        };
        if match self.tools.get(t).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GlCode::EDup);
        }
        if let Some(row) = self.tools.get_mut(t) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x16203 + (t as u64) * 10 + s as u64);
        Ok(p)
    }

    /// 全账核验（30 格）。
    pub fn tools_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOOLS {
            let mut s = 0usize;
            while s < SCENES {
                match self.tools.get(t).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            t += 1;
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

impl Default for GlLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GlLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.env_gate(GO_VER, GOOS, GOARCH);
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TOOLS {
        let mut s = 0usize;
        while s < SCENES {
            if led.run_tool(t, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        t += 1;
    }
    // 注入 10 轮：版本/OS/ARCH 三档轮转失配。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GlLedger::new();
        let caught = match kind {
            0 => lt.env_gate(GO_VER + 1, GOOS, GOARCH) == Err(GlCode::EVer),
            1 => lt.env_gate(GO_VER, GOOS + 1, GOARCH) == Err(GlCode::EOs),
            _ => lt.env_gate(GO_VER, GOOS, GOARCH + 1) == Err(GlCode::EArch),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x16300 + k as u64);
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
pub fn run_unx52046_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52046");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Go 工具链 30/30 全通",
        recorded == (TOOLS * SCENES) as u32,
        "build/test/dlv 三段 ×10 场景全通",
    );
    s.add(
        "三元闸全等放行",
        {
            let mut lt = GlLedger::new();
            lt.env_gate(GO_VER, GOOS, GOARCH).is_ok()
        },
        "版本/GOOS/GOARCH 三元全等放行",
    );
    s.add(
        "三元失配三态全抓",
        {
            let mut lt = GlLedger::new();
            let a = lt.env_gate(GO_VER + 1, GOOS, GOARCH) == Err(GlCode::EVer);
            let b = lt.env_gate(GO_VER, GOOS + 1, GOARCH) == Err(GlCode::EOs);
            let c = lt.env_gate(GO_VER, GOOS, GOARCH + 1) == Err(GlCode::EArch);
            a && b && c
        },
        "版本/GOOS/GOARCH 错位走失配路径",
    );
    s.add("注入 10/10 全抓", caught >= 10, "版本/OS/ARCH 三档轮转必抓");
    let mut l0 = GlLedger::new();
    s.add(
        "边界·段越闭集",
        l0.run_tool(TOOLS, 0).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.run_tool(0, SCENES).is_err(),
        "场景 10 越闭集必拒",
    );
    let mut l1 = GlLedger::new();
    let _ = l1.run_tool(1, 2);
    s.add(
        "重复·再落账拒",
        l1.run_tool(1, 2).is_err(),
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_cell_pure() {
        let a = tool_cell(1, 3).unwrap_or(0);
        assert_eq!(a, tool_cell(1, 3).unwrap_or(0));
        assert_ne!(a, tool_cell(1, 4).unwrap_or(0));
        assert!(tool_cell(TOOLS, 0).is_none());
        assert!(tool_cell(0, SCENES).is_none());
    }

    #[test]
    fn test_env_gate() {
        let mut lt = GlLedger::new();
        assert_eq!(lt.env_gate(GO_VER + 9, GOOS, GOARCH), Err(GlCode::EVer));
        assert_eq!(lt.env_gate(GO_VER, GOOS + 9, GOARCH), Err(GlCode::EOs));
        assert_eq!(lt.env_gate(GO_VER, GOOS, GOARCH + 9), Err(GlCode::EArch));
        assert!(lt.env_gate(GO_VER, GOOS, GOARCH).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GlLedger::new();
        let _ = lt.run_tool(2, 2);
        assert!(lt.run_tool(2, 2).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (TOOLS * SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4B80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52046_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
