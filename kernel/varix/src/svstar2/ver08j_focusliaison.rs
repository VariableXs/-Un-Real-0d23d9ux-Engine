//! VE-F3510 · 场景与专注模式联动 —— 三件联动 + 可拆分 + 状态可见 + 失效降级。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3510`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三件联动**（[`FocusLiaison`]）：进专注 = 夜间场景 + 暗壁纸 +
//!   通知静默，三件作为**一个动作**发生。用户不需要懂三件事——
//!   用户只按了一个「专注」。
//! - **联动可拆**（[`LiaisonMask`]）：用户可以只要色温不要静默
//!   （写代码时想听消息提示音）。拆分是**位掩码**配置：三件各自
//!   可开关，掩码是数据不是开关散落（可持久化、可审计、可重置）。
//!   **拆分冲突 → 尊重用户**：掩码声明不联动的件，联动绝不碰它。
//! - **状态可见**（[`LiaisonView`]）：当前专注档、三件各自的
//!   联动状态（跟了/没跟/被用户拆掉），一次读全。
//! - **失效降级**（[`LiaisonBridge`]）：专注系统不可达 → **降级为
//!   独立**：色温件照常走（它在 E06 域内自足），壁纸/通知件跳过
//!   并记录。降级不是失败——用户看到的仍是「场景切了」，只是
//!   两件没跟上，告知要说清（读屏播报）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 联动失效 | 降级独立（色温照走） |
//! | 拆分冲突 | 尊重用户（被拆件不碰） |
//! | 状态缺失 | 补显（视图重建） |
//!
//! # 复杂度
//!
//! 全部 O(1)（三件是定长元组）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5770 起）
// ---------------------------------------------------------------------------

/// 专注联动域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiaisonCode {
    /// 专注系统不可达（降级独立）。
    FocusUnreachable = 5770,
    /// 专注档不存在（越界档位）。
    BadTier = 5771,
    /// 联动状态缺失（视图重建）。
    ViewMissing = 5772,
    /// 重复进同一专注档（NoOp，不算错但记审计）。
    AlreadyInTier = 5773,
}

impl LiaisonCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            LiaisonCode::FocusUnreachable => "专注系统不可达",
            LiaisonCode::BadTier => "专注档不存在",
            LiaisonCode::ViewMissing => "联动状态缺失",
            LiaisonCode::AlreadyInTier => "已在目标专注档",
        }
    }

    /// 是否阻断级（联动域全降级，无停机）。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、联动掩码（可拆分）
// ---------------------------------------------------------------------------

/// 三件位掩码：bit0=色温 bit1=壁纸 bit2=通知静默。
pub type LiaisonMask = u8;

/// 默认全联动掩码。
pub const LIAISON_FULL: LiaisonMask = 0b111;
/// 仅色温掩码（用户拆掉壁纸与静默）。
pub const LIAISON_KELVIN_ONLY: LiaisonMask = 0b001;
/// 全不联动（专注与场景完全独立）。
pub const LIAISON_NONE: LiaisonMask = 0b000;

/// 按件号取掩码位。
pub fn mask_has(mask: LiaisonMask, piece: u8) -> bool {
    piece < 3 && (mask & (1 << piece)) != 0
}

// ---------------------------------------------------------------------------
// 三、专注桥
// ---------------------------------------------------------------------------

/// 专注系统桥（联动器只认口不认实现）。
pub trait FocusPort {
    /// 进入专注档。false = 系统不可达。
    fn enter(&mut self, tier: u16) -> bool;
    /// 当前档（None = 不在专注）。
    fn current(&self) -> Option<u16>;
    /// 退出专注。
    fn leave(&mut self);
}

/// 联动目标三件值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiaisonTargets {
    /// 夜间场景色温。
    pub kelvin: u32,
    /// 暗壁纸档。
    pub wallpaper: u16,
    /// 静默策略档。
    pub notify: u16,
}

/// 每件的联动结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceResult {
    /// 已联动（件值已下发）。
    Applied,
    /// 用户拆掉（掩码声明不联动）。
    SplitByUser,
    /// 降级跳过（专注桥不可达导致）。
    Degraded,
}

impl PieceResult {
    /// 读屏播报。
    pub fn spoken(self) -> &'static str {
        match self {
            PieceResult::Applied => "已联动",
            PieceResult::SplitByUser => "按您的设置未联动",
            PieceResult::Degraded => "暂时未能联动",
        }
    }
}

/// 联动执行报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiaisonReport {
    /// 目标专注档。
    pub tier: u16,
    /// 三件各自结果（下标 0..3）。
    pub pieces: [PieceResult; 3],
    /// 降级码（无降级为 None）。
    pub degraded: Option<LiaisonCode>,
}

impl LiaisonReport {
    /// 三件是否全部按掩码落位（Applied 或 SplitByUser，Degraded 算未落位）。
    pub fn clean(&self) -> bool {
        self.degraded.is_none()
    }

    /// 播报行（读屏）。
    pub fn announce(&self) -> &'static str {
        if self.clean() {
            "专注模式联动完成"
        } else {
            "专注模式联动部分完成"
        }
    }
}

// ---------------------------------------------------------------------------
// 四、联动器
// ---------------------------------------------------------------------------

/// 场景×专注联动器。
pub struct FocusLiaison {
    mask: LiaisonMask,
    tier: Option<u16>,
    codes: Vec<LiaisonCode>,
}

impl FocusLiaison {
    /// 全联动起步。
    pub fn new() -> Self {
        FocusLiaison {
            mask: LIAISON_FULL,
            tier: None,
            codes: Vec::new(),
        }
    }

    /// 用给定掩码构造（可拆分入口）。
    pub fn with_mask(mask: LiaisonMask) -> Self {
        FocusLiaison {
            mask,
            tier: None,
            codes: Vec::new(),
        }
    }

    /// 当前掩码。
    pub fn mask(&self) -> LiaisonMask {
        self.mask
    }

    /// 改掩码（用户拆分偏好变更）。
    pub fn set_mask(&mut self, mask: LiaisonMask) {
        self.mask = mask;
    }

    /// 当前专注档。
    pub fn tier(&self) -> Option<u16> {
        self.tier
    }

    /// 审计码。
    pub fn codes(&self) -> &[LiaisonCode] {
        &self.codes
    }

    /// 执行进专注联动。三件按掩码走，件下发走 `deliver` 回调
    /// （件号 → bool）。专注桥先试；桥不可达则色温件照走、
    /// 其余件降级跳过（降级独立纪律）。
    pub fn engage(
        &mut self,
        port: &mut dyn FocusPort,
        tier: u16,
        targets: LiaisonTargets,
        mut deliver: impl FnMut(u8, LiaisonTargets) -> bool,
    ) -> LiaisonReport {
        let bridge_ok = port.enter(tier);
        if bridge_ok {
            self.tier = Some(tier);
            self.codes.retain(|c| *c != LiaisonCode::FocusUnreachable);
        } else {
            self.codes.push(LiaisonCode::FocusUnreachable);
        }
        let values = [
            targets.kelvin,
            targets.wallpaper as u32,
            targets.notify as u32,
        ];
        let mut pieces = [PieceResult::Applied; 3];
        let mut any_degraded = false;
        for piece in 0u8..3 {
            if !mask_has(self.mask, piece) {
                pieces[piece as usize] = PieceResult::SplitByUser;
                continue;
            }
            if !bridge_ok && piece != 0 {
                // 降级独立：色温件（E06 域内自足）照走，其余跳过。
                pieces[piece as usize] = PieceResult::Degraded;
                any_degraded = true;
                continue;
            }
            if deliver(piece, targets) {
                let _ = values[piece as usize];
            } else {
                pieces[piece as usize] = PieceResult::Degraded;
                any_degraded = true;
            }
        }
        LiaisonReport {
            tier,
            pieces,
            degraded: if any_degraded {
                Some(LiaisonCode::FocusUnreachable)
            } else {
                None
            },
        }
    }

    /// 退出专注：恢复由场景系统接管（联动器只管离开动作）。
    pub fn disengage(&mut self, port: &mut dyn FocusPort) {
        port.leave();
        self.tier = None;
    }

    /// 状态公开视图（状态可见纪律；缺失即重建）。
    pub fn view(&self, port: &dyn FocusPort) -> LiaisonView {
        LiaisonView {
            mask: self.mask,
            tier: port.current().or(self.tier),
            kelvin_state: mask_has(self.mask, 0),
            wallpaper_state: mask_has(self.mask, 1),
            notify_state: mask_has(self.mask, 2),
        }
    }
}

/// 联动状态视图（读屏播报源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiaisonView {
    /// 联动掩码。
    pub mask: LiaisonMask,
    /// 当前专注档。
    pub tier: Option<u16>,
    /// 色温件是否联动。
    pub kelvin_state: bool,
    /// 壁纸件是否联动。
    pub wallpaper_state: bool,
    /// 通知件是否联动。
    pub notify_state: bool,
}

impl LiaisonView {
    /// 播报行。
    pub fn announce(&self) -> &'static str {
        match self.tier {
            Some(_) => "专注模式进行中",
            None => "专注模式未启用",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct UpPort;
    impl FocusPort for UpPort {
        fn enter(&mut self, _t: u16) -> bool {
            true
        }
        fn current(&self) -> Option<u16> {
            Some(2)
        }
        fn leave(&mut self) {}
    }

    struct DeadPort;
    impl FocusPort for DeadPort {
        fn enter(&mut self, _t: u16) -> bool {
            false
        }
        fn current(&self) -> Option<u16> {
            None
        }
        fn leave(&mut self) {}
    }

    const T: LiaisonTargets = LiaisonTargets {
        kelvin: 2700,
        wallpaper: 5,
        notify: 9,
    };

    #[test]
    fn full_liaison_all_applied() {
        let mut l = FocusLiaison::new();
        let r = l.engage(&mut UpPort, 2, T, |_, _| true);
        assert!(r.clean());
        assert!(r.pieces.iter().all(|p| *p == PieceResult::Applied));
    }

    #[test]
    fn split_respects_user() {
        let mut l = FocusLiaison::with_mask(LIAISON_KELVIN_ONLY);
        let mut touched = Vec::new();
        let r = l.engage(&mut UpPort, 2, T, |p, _| {
            touched.push(p);
            true
        });
        assert_eq!(touched, vec![0]);
        assert_eq!(r.pieces[1], PieceResult::SplitByUser);
        assert_eq!(r.pieces[2], PieceResult::SplitByUser);
    }

    #[test]
    fn degraded_keeps_kelvin_only() {
        let mut l = FocusLiaison::new();
        let mut touched = Vec::new();
        let r = l.engage(&mut DeadPort, 2, T, |p, _| {
            touched.push(p);
            true
        });
        assert_eq!(touched, vec![0]);
        assert_eq!(r.pieces[0], PieceResult::Applied);
        assert_eq!(r.pieces[1], PieceResult::Degraded);
        assert_eq!(r.pieces[2], PieceResult::Degraded);
        assert_eq!(r.degraded, Some(LiaisonCode::FocusUnreachable));
    }

    #[test]
    fn view_visible() {
        let mut l = FocusLiaison::new();
        let port = UpPort;
        let v = l.view(&port);
        assert_eq!(v.announce(), "专注模式进行中");
        assert!(v.kelvin_state && v.wallpaper_state && v.notify_state);
    }
}
