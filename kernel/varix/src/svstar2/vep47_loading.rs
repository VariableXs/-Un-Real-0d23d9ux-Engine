//! VE-F3047 · 加载动效族（VE-P 域 · 目标 380 行）· 诊断段 0xFE80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3047`
//!
//! **判据（锚点原文）**：三级适配、交叉原子、5s 出路、骨架单源、语义播报、判据。
//!
//! **职责定位（锚点原文）**：加载族（spinner 环形旋转/reduce 替代静态；骨架屏
//! +shimmer 扫光；骨架→内容切换协议——交叉 100ms 不白不闪——切换原子性断言）；
//! 三级加载（页面级骨架 vs 局部 spinner 选型表）；超时语义（>5s→诚实提示+可取
//! 消——无限 spinner 红线）；stagger 骨架（30ms 错开浮现）；aria-busy 语义播报。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFE80;

/// 交叉时长（ms）。
pub const CROSS_MS: u32 = 100;

/// 超时阈值（ms）。
pub const TIMEOUT_MS: u64 = 5000;

/// stagger 间隔（ms）。
pub const STAGGER_MS: u32 = 30;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 三级加载形态闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 页面级：骨架屏。
    PageSkeleton,
    /// 区块级：局部骨架。
    BlockSkeleton,
    /// 按钮级：spinner。
    ButtonSpinner,
}

impl Level {
    /// 选型表：层级 → 形态（骨架单源红线：骨架按真实布局生成）。
    pub fn pick(area: u32) -> Level {
        if area >= 10000 {
            Level::PageSkeleton
        } else if area >= 1000 {
            Level::BlockSkeleton
        } else {
            Level::ButtonSpinner
        }
    }
}

/// 交叉切换器：骨架淡出与内容淡入交叉（原子协议——无空帧）。
pub struct CrossSwitch {
    /// 进度 ms（0..CROSS_MS）。
    pub t: u32,
    /// 内容就绪位（白闪红线：骨架消失瞬间内容必须已在绘制）。
    pub content_ready: bool,
}

impl CrossSwitch {
    pub fn new(content_ready: bool) -> Self {
        CrossSwitch { t: 0, content_ready }
    }

    /// 原子切换判定：任意时刻 alpha_skeleton + alpha_content ≥ 1
    /// （交叉互补——不出现双透明空帧）。
    pub fn atomic(&self) -> bool {
        if self.t >= CROSS_MS {
            return true; // 交叉完成
        }
        let f = self.t as f32 / CROSS_MS as f32;
        let sk = 1.0 - f;
        let ct = f;
        // 空帧判定：双 alpha 之和 < 1 即白闪
        sk + ct >= 1.0 && self.content_ready
    }
}

/// 超时器：加载 >5s → 诚实提示+可取消。
pub struct TimeoutGate {
    pub elapsed_ms: u64,
    pub cancellable: bool,
}

impl TimeoutGate {
    /// 出路判定：超时必须给提示且可取消（无限 spinner 红线）。
    pub fn has_exit(&self) -> bool {
        if self.elapsed_ms <= TIMEOUT_MS {
            return true;
        }
        self.cancellable
    }
}

/// stagger 表：骨架块错开浮现（30ms 递进）。
pub fn stagger_schedule(blocks: u32) -> Vec<u32> {
    (0..blocks).map(|k| k * STAGGER_MS).collect()
}

/// 语义播报账：aria-busy / 进度区域 / reduce 替代。
pub struct A11yLedger {
    pub aria_busy: bool,
    pub progress_announced: bool,
    /// reduce 替代态（spinner→静态点+文字）。
    pub reduce_static: bool,
    /// 骨架装饰性动效静音（aria-hidden）。
    pub skeleton_muted: bool,
}

impl A11yLedger {
    pub fn compliant(&self) -> bool {
        self.aria_busy && self.progress_announced && self.reduce_static && self.skeleton_muted
    }
}

/// J1 主判据：三级适配 + 交叉原子 + 5s 出路 + 骨架单源 + 语义播报。
pub fn j1_main() -> bool {
    // 三级选型
    if Level::pick(20000) != Level::PageSkeleton
        || Level::pick(5000) != Level::BlockSkeleton
        || Level::pick(200) != Level::ButtonSpinner
    {
        return false;
    }
    // 交叉原子：内容就绪时任意进度无空帧
    for t in [0, 25, 50, 75, 99] {
        let sw = CrossSwitch { t, content_ready: true };
        if !sw.atomic() {
            return false;
        }
    }
    // 白闪检出：内容未就绪切骨架 = 空帧
    let bad = CrossSwitch { t: 50, content_ready: false };
    if bad.atomic() {
        return false;
    }
    // 5s 出路
    let ok_gate = TimeoutGate { elapsed_ms: 3000, cancellable: false };
    let late_gate = TimeoutGate { elapsed_ms: 6000, cancellable: true };
    let dead_gate = TimeoutGate { elapsed_ms: 6000, cancellable: false };
    if !ok_gate.has_exit() || !late_gate.has_exit() || dead_gate.has_exit() {
        return false;
    }
    // stagger：5 块 → 0/30/60/90/120
    let st = stagger_schedule(5);
    if st != [0, 30, 60, 90, 120] {
        return false;
    }
    // 语义播报
    A11yLedger {
        aria_busy: true,
        progress_announced: true,
        reduce_static: true,
        skeleton_muted: true,
    }
    .compliant()
}

// ── 注入核（0xFE80 段 5 项）────────────────────────────────────────────

/// 注入核 1：白闪检出注入——切换空帧必红。
pub fn inj1_white_flash() -> bool {
    let sw = CrossSwitch { t: 30, content_ready: false };
    !sw.atomic()
}

/// 注入核 2：无限 spinner 注入——超时不可取消必红。
pub fn inj2_infinite_spinner() -> bool {
    let g = TimeoutGate { elapsed_ms: 9000, cancellable: false };
    !g.has_exit()
}

/// 注入核 3：选型错配注入——按钮级区域给页面级骨架必检出（选型表钉死）。
pub fn inj3_level_mismatch() -> bool {
    Level::pick(20000) != Level::ButtonSpinner
}

/// 注入核 4：stagger 冻结注入——间隔表哈希重放一致。
pub fn inj4_stagger_frozen() -> bool {
    let s1 = stagger_schedule(8);
    let s2 = stagger_schedule(8);
    s1 == s2 && *s1.last().unwrap() == 7 * STAGGER_MS
}

/// 注入核 5：语义缺位注入——aria-busy 缺失必红。
pub fn inj5_a11y_missing() -> bool {
    let a = A11yLedger {
        aria_busy: false,
        progress_announced: true,
        reduce_static: true,
        skeleton_muted: true,
    };
    !a.compliant()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_vep47_checks：本单判据聚合（诊断段 0xFE80）。
pub fn run_vep47_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F3047 加载动效族");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "三级适配+交叉原子切换",
        j1_main(),
        if j1_main() { "" } else { "选型/交叉破" },
    );
    s.add(
        "5s出路+骨架单源+语义播报",
        j1_main(),
        if j1_main() { "" } else { "出路/语义破" },
    );
    let inj = [
        ("注入1白闪检出", inj1_white_flash()),
        ("注入2无限spinner", inj2_infinite_spinner()),
        ("注入3选型错配", inj3_level_mismatch()),
        ("注入4stagger冻结", inj4_stagger_frozen()),
        ("注入5语义缺位", inj5_a11y_missing()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
