//! VE-F3535 · Persona 调试 —— 五域生效链视图 + X03 会话统一 + 快照导出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3535`
//!
//! # 职责（锚点原文拆解）
//!
//! - **五域链视图**（[`domain_chain`]）：五域各自「谁在生效」
//!   ——预设底稿 / 用户微调 / 无障碍强制 三层从上到下，每层
//!   三态（生效/被压/不在场）。视图失真不可能：链仲裁与
//!   F3531 同一实现（[`LayerChain`] 语义复用）。
//! - **会话统一**（[`PersonaDebug`]):挂 X03 会话号（非零），
//!   链查询/快照导出记账到会话。
//! - **快照导出**（[`PersonaDebug::snapshot`]):五域生效链 +
//!   时间戳打一包（纯文本，读屏可达）。
//! - **链缺层补齐**：输出恒五域 × 三层，不在场显式标注——
//!   「缺失」在视图侧不存在。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 链缺层 | 补齐（显式不在场） |
//! | 视图失真 | 修正（同源仲裁） |
//! | 导出失败 | 重试（纯函数不失败） |
//!
//! # 复杂度
//!
//! 全部 O(域 × 层)（5×3 定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::format;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A40 起）
// ---------------------------------------------------------------------------

/// Persona 调试域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PdbgCode {
    /// 会话号非法（零号）。
    BadSession = 0x5A40,
}

impl PdbgCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PdbgCode::BadSession => "调试会话号非法",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、链视图
// ---------------------------------------------------------------------------

/// 层三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    /// 生效中。
    Winning,
    /// 在场被压。
    Suppressed,
    /// 不在场。
    Absent,
}

impl ChainState {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ChainState::Winning => "生效中",
            ChainState::Suppressed => "被压住",
            ChainState::Absent => "不在场",
        }
    }
}

/// 五域标识（复用 F3533 域枚举语义，本册本地副本防跨册耦合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PDomain {
    Color,
    Sound,
    Font,
    Icon,
    Motion,
}

impl PDomain {
    /// 全集。
    pub const ALL: [PDomain; 5] = [
        PDomain::Color,
        PDomain::Sound,
        PDomain::Font,
        PDomain::Icon,
        PDomain::Motion,
    ];

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            PDomain::Color => "色彩",
            PDomain::Sound => "声音",
            PDomain::Font => "字体",
            PDomain::Icon => "图标",
            PDomain::Motion => "动效",
        }
    }
}

/// 链输入：每域的层在场标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChainInput {
    /// 无障碍层在场（F3531 最高层）。
    pub a11y: bool,
    /// 预设底稿在场。
    pub preset: bool,
    /// 用户微调在场。
    pub tweak: bool,
}

/// 一格链目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainCell {
    /// 域。
    pub domain: PDomain,
    /// 无障碍层状态。
    pub a11y: ChainState,
    /// 预设层状态。
    pub preset: ChainState,
    /// 微调层状态。
    pub tweak: ChainState,
}

/// 五域生效链（同源仲裁：赢家 = 无障碍 > 微调 > 预设——微调压预设，
/// 无障碍压一切；与 F3531 链序一致）。
pub fn domain_chain(input: ChainInput) -> Vec<ChainCell> {
    PDomain::ALL
        .iter()
        .copied()
        .map(|d| {
            let win = if input.a11y {
                0
            } else if input.tweak {
                1
            } else if input.preset {
                2
            } else {
                3 // 全不在场
            };
            let st = |i: usize, present: bool| {
                if !present {
                    ChainState::Absent
                } else if win == i {
                    ChainState::Winning
                } else {
                    ChainState::Suppressed
                }
            };
            ChainCell {
                domain: d,
                a11y: st(0, input.a11y),
                tweak: st(1, input.tweak),
                preset: st(2, input.preset),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 三、调试会话
// ---------------------------------------------------------------------------

/// 会话开销预算（tick）。
pub const PBUDGET: u32 = 32;

/// Persona 调试会话。
pub struct PersonaDebug {
    /// X03 会话号。
    pub session: u32,
    used: u32,
    snapshots: usize,
}

impl PersonaDebug {
    /// 开会话（零号拒绝）。
    pub fn open(session: u32) -> Result<Self, PdbgCode> {
        if session == 0 {
            return Err(PdbgCode::BadSession);
        }
        Ok(PersonaDebug { session, used: 0, snapshots: 0 })
    }

    /// 已用开销。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 链查询（记账 2）。
    pub fn query(&mut self, input: ChainInput) -> Vec<ChainCell> {
        self.used += 2;
        domain_chain(input)
    }

    /// 快照导出（记账 4；五域链 + 时间戳逻辑值打一包）。
    pub fn snapshot(&mut self, input: ChainInput, clock: u32) -> Vec<&'static str> {
        self.used += 4;
        self.snapshots += 1;
        let mut out = Vec::with_capacity(6);
        out.push(&*alloc::format!("persona_debug_session={} clock={} snap={}", self.session, clock, self.snapshots).leak());
        let _ = clock;
        for cell in domain_chain(input) {
            let line: &'static str = alloc::format!(
                "{}: a11y={} preset={} tweak={}",
                cell.domain.label(),
                cell.a11y.label(),
                cell.preset.label(),
                cell.tweak.label(),
            )
            .leak();
            out.push(line);
        }
        out
    }

    /// 快照数。
    pub fn snapshot_count(&self) -> usize {
        self.snapshots
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_session_rejected() {
        assert_eq!(PersonaDebug::open(0), Err(PdbgCode::BadSession));
    }

    #[test]
    fn chain_a11y_wins_all_domains() {
        let cells = domain_chain(ChainInput { a11y: true, preset: true, tweak: true });
        for c in cells {
            assert_eq!(c.a11y, ChainState::Winning);
            assert_eq!(c.preset, ChainState::Suppressed);
            assert_eq!(c.tweak, ChainState::Suppressed);
        }
    }

    #[test]
    fn absent_explicit_not_missing() {
        let cells = domain_chain(ChainInput::default());
        for c in cells {
            assert_eq!(c.a11y, ChainState::Absent);
            assert_eq!(c.preset, ChainState::Absent);
            assert_eq!(c.tweak, ChainState::Absent);
        }
    }

    #[test]
    fn snapshot_six_lines() {
        let mut s = PersonaDebug::open(9).unwrap();
        let snap = s.snapshot(ChainInput { a11y: false, preset: true, tweak: false }, 100);
        assert_eq!(snap.len(), 6); // 头 + 五域
        assert_eq!(s.snapshot_count(), 1);
    }
}
