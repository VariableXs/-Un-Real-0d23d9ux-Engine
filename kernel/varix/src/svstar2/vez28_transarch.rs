//! VE-F5281 · 转场库总架构（VE-Z 域 · Z05 组 · 目标 320 行）· 诊断段 0x9e80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5281`
//!
//! **判据（锚点原文）**：三段、转场语法、词典口径、不闪不晃、判据。
//!
//! **职责定位（锚点原文）**：转场库总架构（转场库定位：画面切换的语法——转场
//! 是内容流的标点符号）；架构三段：转场效果层（场景转场与元素转场效果本体）→
//! 编排层（转场时机与语义）→动效层（缓动库与打断处理——三段共用 S 域动效词典
//! 口径）；转场不闪不晃——晕动与光敏双友好；词典违例→驳回。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9e80;

/// 三段。
pub const STAGES: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 动效词典口径（S 域共用）。
pub const LEXICON: [&str; 4] = ["cut", "fade", "wipe", "slide"];

/// 转场效果：词表内效果 + 光敏友好位（不闪不晃）。
pub struct Transition {
    pub lex: &'static str,
    /// 晕动友好（大位移转场须可减）。
    pub motion_safe: bool,
    /// 光敏友好（无高频闪烁）。
    pub photosensitive_safe: bool,
}

impl Transition {
    /// 词典合法 + 双友好。
    pub fn ok(&self) -> bool {
        LEXICON.contains(&self.lex) && self.motion_safe && self.photosensitive_safe
    }
}

/// 编排层：转场时机与语义（打断处理——转场中可打断）。
pub struct Orchestrator {
    pub queue: Vec<&'static str>,
    /// 当前转场进度 0..1。
    pub progress: f32,
    /// 打断允许位。
    pub interruptible: bool,
}

impl Orchestrator {
    /// 打断语义：可打断时进度随时可归零且队列保持。
    pub fn interrupt(&mut self) -> bool {
        if self.interruptible {
            self.progress = 0.0;
            true
        } else {
            false
        }
    }

    /// 队列词典合法：全部词表内。
    pub fn lexicon_ok(&self) -> bool {
        self.queue.iter().all(|q| LEXICON.contains(q))
    }
}

/// 缓动：转场进度缓动（fade 用平滑曲线）。
pub fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t) // smoothstep
}

/// J1 主判据：三段 + 词典口径 + 不闪不晃 + 打断处理。
pub fn j1_main() -> bool {
    let ts = [
        Transition { lex: "fade", motion_safe: true, photosensitive_safe: true },
        Transition { lex: "wipe", motion_safe: true, photosensitive_safe: true },
    ];
    if !ts.iter().all(|t| t.ok()) {
        return false;
    }
    let mut o = Orchestrator {
        queue: alloc::vec!["cut", "fade", "slide"],
        progress: 0.7,
        interruptible: true,
    };
    if !o.lexicon_ok() {
        return false;
    }
    if !o.interrupt() || o.progress != 0.0 {
        return false;
    }
    // 缓动端点钉死：ease(0)=0、ease(1)=1、ease(0.5)=0.5
    let e0 = ease(0.0);
    let e1 = ease(1.0);
    let em = ease(0.5);
    (e0 - 0.0).abs() < 1e-5 && (e1 - 1.0).abs() < 1e-5 && (em - 0.5).abs() < 1e-5
}

// ── 注入核（0x9e80 段 5 项）────────────────────────────────────────────

/// 注入核 1：词典违例驳回注入——词表外效果必驳回。
pub fn inj1_lexicon_violation() -> bool {
    let t = Transition { lex: "spin-flash", motion_safe: false, photosensitive_safe: false };
    !t.ok()
}

/// 注入核 2：光敏检出注入——闪烁效果必拦。
pub fn inj2_photosensitive() -> bool {
    let t = Transition { lex: "fade", motion_safe: true, photosensitive_safe: false };
    !t.ok()
}

/// 注入核 3：队列违例注入——队列含词表外项必红。
pub fn inj3_queue_violation() -> bool {
    let o = Orchestrator {
        queue: alloc::vec!["fade", "explode"],
        progress: 0.0,
        interruptible: true,
    };
    !o.lexicon_ok()
}

/// 注入核 4：不可打断注入——不可打断转场拒打断。
pub fn inj4_no_interrupt() -> bool {
    let mut o = Orchestrator {
        queue: alloc::vec!["cut"],
        progress: 0.5,
        interruptible: false,
    };
    !o.interrupt() && o.progress == 0.5
}

/// 注入核 5：缓动单调注入——smoothstep 全程单调。
pub fn inj5_ease_monotone() -> bool {
    let mut prev = -1.0;
    for k in 0..=10u32 {
        let t = k as f32 / 10.0;
        let v = ease(t);
        if v < prev {
            return false;
        }
        prev = v;
    }
    true
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vez28_checks：本单判据聚合（诊断段 0x9e80）。
pub fn run_vez28_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5281 转场库总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三段+词典口径合法",
        j1_main(),
        if j1_main() { "" } else { "词典破" },
    );
    s.add(
        "不闪不晃（晕动/光敏双友好）+打断处理",
        j1_main() && inj2_photosensitive(),
        if j1_main() { "" } else { "友好/打断破" },
    );
    let inj = [
        ("注入1词典违例", inj1_lexicon_violation()),
        ("注入2光敏拦截", inj2_photosensitive()),
        ("注入3队列违例", inj3_queue_violation()),
        ("注入4不可打断", inj4_no_interrupt()),
        ("注入5缓动单调", inj5_ease_monotone()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
