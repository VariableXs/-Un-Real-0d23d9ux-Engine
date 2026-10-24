//! UNX-F0154 · QEMU 冒烟（只读判据 · 开发期门禁）· 诊断段 0x2610
//!
//! 判据原文（UNX-F0154-J1）：冒烟判据全只读——仅采集「启动信号串」
//! 与「退出码」两要素，不得改写镜像与工作区（红线）；门禁账：
//! 信号串出现 + 退出码归零 → 绿；任一缺失即红并记要素定位。
//!
//! 设计要点：冒烟门禁（SmokeGate）——采集器只提供 stdout 尾窗与
//! 退出码；判定闭集：NoBootSignal / BadExit / Timeout / Pass；
//! 判定账守恒（Σ判定 = Σ尝试）。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2620;

const E_ARG: u16 = SEG_BASE + 1; // 判据参数非法（空信号串/零超时）
const E_BAL: u16 = SEG_BASE + 2; // 门禁账守恒被破坏
const E_STATE: u16 = SEG_BASE + 3; // 未采集先判定
const E_SEAL: u16 = SEG_BASE + 4; // 门禁已封
const E_REDLINE: u16 = SEG_BASE + 5; // 红线：尝试改写镜像被判据拦截

/// 启动信号串（内核自报家门锚）。
pub const BOOT_SIGNAL: &str = "VARIX-BOOT-OK";

/// 冒烟尝试采集帧。
#[derive(Clone)]
pub struct SmokeSample {
    pub stdout_tail: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

/// 判定闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    NoBootSignal,
    BadExit,
    Timeout,
}

impl Verdict {
    fn name(&self) -> &'static str {
        match self {
            Verdict::Pass => "过",
            Verdict::NoBootSignal => "无启动信号",
            Verdict::BadExit => "退出码非零",
            Verdict::Timeout => "超时",
        }
    }
}

/// 冒烟门禁。
pub struct SmokeGate {
    pub signal: String,
    pub attempts: u32,
    pub verdicts: [u32; 4], // Pass/NoBootSignal/BadExit/Timeout
    pub rows: Vec<(u32, Verdict)>, // 尝试号 → 判定
    pub sealed: bool,
}

impl SmokeGate {
    pub fn new(signal: &str) -> Result<Self, u16> {
        if signal.is_empty() {
            return Err(E_ARG);
        }
        Ok(SmokeGate {
            signal: signal.to_string(),
            attempts: 0,
            verdicts: [0; 4],
            rows: Vec::new(),
            sealed: false,
        })
    }

    /// 只读采集 + 判定。采样帧由宿主传入，门禁自身零副作用。
    pub fn run_once(&mut self, s: &SmokeSample) -> Result<Verdict, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        self.attempts += 1;
        let v = if s.timed_out {
            Verdict::Timeout
        } else if !s.stdout_tail.contains(&self.signal) {
            Verdict::NoBootSignal
        } else if s.exit_code != 0 {
            Verdict::BadExit
        } else {
            Verdict::Pass
        };
        let slot = match v {
            Verdict::Pass => 0,
            Verdict::NoBootSignal => 1,
            Verdict::BadExit => 2,
            Verdict::Timeout => 3,
        };
        self.verdicts[slot] += 1;
        self.rows.push((self.attempts, v));
        Ok(v)
    }

    /// 开发期门禁绿判定：最近一次尝试必须 Pass。
    pub fn gate(&self) -> Result<(), u16> {
        match self.rows.last() {
            None => Err(E_STATE), // 未采集先判
            Some((_, v)) => {
                if *v == Verdict::Pass {
                    Ok(())
                } else {
                    Err(E_BAL)
                }
            }
        }
    }

    /// 判定账守恒：Σverdicts == attempts == rows.len()。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u32 = self.verdicts.iter().sum();
        if sum != self.attempts || self.rows.len() as u32 != self.attempts {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 渲染门禁一览。
    pub fn render(&self) -> String {
        format!(
            "冒烟门禁 信号\"{}\" 尝试{} 过{} 无信号{} 坏退出{} 超时{}",
            self.signal,
            self.attempts,
            self.verdicts[0],
            self.verdicts[1],
            self.verdicts[2],
            self.verdicts[3],
        )
    }
}

/// 只读红线检查：镜像哈希前后一致（判据不改写镜像的账面证明）。
pub fn redline_image_untouched(before: u32, after: u32) -> Result<(), u16> {
    if before != after {
        return Err(E_REDLINE);
    }
    Ok(())
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：信号 + 零退出 → Pass → 门禁绿。
    let mut g = SmokeGate::new(BOOT_SIGNAL)?;
    let s = SmokeSample {
        stdout_tail: "booting... VARIX-BOOT-OK".to_string(),
        exit_code: 0,
        timed_out: false,
    };
    if g.run_once(&s)? != Verdict::Pass {
        return Err(E_BAL);
    }
    g.gate()?;
    g.audit()?;
    hits += 1;

    // L2：三要素缺一路由——无信号 / 坏退出 / 超时各判对。
    let mut g = SmokeGate::new(BOOT_SIGNAL)?;
    let s1 = SmokeSample { stdout_tail: "panic".to_string(), exit_code: 0, timed_out: false };
    if g.run_once(&s1)? != Verdict::NoBootSignal {
        return Err(E_BAL);
    }
    let s2 = SmokeSample {
        stdout_tail: "VARIX-BOOT-OK".to_string(),
        exit_code: 3,
        timed_out: false,
    };
    if g.run_once(&s2)? != Verdict::BadExit {
        return Err(E_BAL);
    }
    let s3 = SmokeSample { stdout_tail: String::new(), exit_code: 0, timed_out: true };
    if g.run_once(&s3)? != Verdict::Timeout {
        return Err(E_BAL);
    }
    g.audit()?;
    if g.verdicts != [0, 1, 1, 1] {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：信号串前缀命中即可（尾窗截断容忍）。
    let mut g = SmokeGate::new(BOOT_SIGNAL)?;
    let s = SmokeSample {
        stdout_tail: "...VARIX-BOOT-OK".to_string(),
        exit_code: 0,
        timed_out: false,
    };
    if g.run_once(&s)? != Verdict::Pass {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：渲染账面。
    let g = SmokeGate::new(BOOT_SIGNAL)?;
    let _ = g.render();
    hits += 1;

    // I1：空信号串构造即拒。
    if !matches!(SmokeGate::new(""), Err(E_ARG)) {
        return Err(E_ARG);
    }
    hits += 1;

    // I2：未采集先门禁（E_STATE）。
    let i2 = SmokeGate::new(BOOT_SIGNAL)?;
    if i2.gate() != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I3：门禁红——最近一次非 Pass。
    let mut i3 = SmokeGate::new(BOOT_SIGNAL)?;
    let s = SmokeSample { stdout_tail: "x".to_string(), exit_code: 0, timed_out: false };
    i3.run_once(&s)?;
    if i3.gate() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：红线拦截——镜像哈希前后不一致。
    if redline_image_untouched(0xDEAD, 0xBEEF) != Err(E_REDLINE) {
        return Err(E_REDLINE);
    }
    hits += 1;

    // I5：封门禁拒跑 + 账注入（Σverdicts ≠ attempts 必红）。
    let mut i5 = SmokeGate::new(BOOT_SIGNAL)?;
    let s = SmokeSample { stdout_tail: BOOT_SIGNAL.to_string(), exit_code: 0, timed_out: false };
    i5.run_once(&s)?;
    i5.seal();
    if i5.run_once(&s) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    i5.verdicts[0] = 9; // 破账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
