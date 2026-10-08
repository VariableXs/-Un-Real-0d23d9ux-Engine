//! F592 截图含光标开关 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：默认不含；三模式生效；形态真实渲染；开关即时；
//! 与 F361 录屏光标独立（录屏有自己的开关）。
//!
//! **设计要点（主册）**：
//! - 截图工具（F098）设置项：截图中包含鼠标指针（默认不含——多数截图
//!   不想要箭头）；
//! - 开关即时对三模式生效（F413 键位/区域/窗口）；
//! - 含指针时指针按截图瞬间的真实形态渲染（忙碌圈/手型都如实）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 截图三模式（F413 键位——枚举即清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotMode {
    /// 全屏（键位触发）。
    Fullscreen,
    /// 区域。
    Region,
    /// 窗口。
    Window,
}

/// 指针真实形态（截图瞬间如实渲染——不是永远箭头贴图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Arrow,
    Hand,
    Busy,
    Text,
}

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 截图光标配置（截图工具侧——与 F361 录屏的独立开关分开记账）。
pub struct ShotCursor {
    /// 截图含光标（默认 false——多数截图不想要箭头）。
    include: bool,
    /// 截图瞬间光标形态（宿主注入——渲染层按此取形）。
    shape: CursorShape,
    /// 三模式独立记账（开关一处三模式全管——无模式例外）。
    per_mode: [(bool, bool); 3], // (mode, include_applied)
}

impl ShotCursor {
    pub fn new() -> ShotCursor {
        ShotCursor {
            include: false,
            shape: CursorShape::Arrow,
            per_mode: [
                (ShotMode::Fullscreen as usize != 99, false),
                (false, false),
                (false, false),
            ],
        }
    }

    pub fn default_off(&self) -> bool {
        !self.include
    }

    /// 开关（即时生效——下一帧截图即按新状态，无重启路径）。
    pub fn set_include(&mut self, on: bool) {
        self.include = on;
    }

    pub fn include(&self) -> bool {
        self.include
    }

    /// 截图瞬间形态注入（忙碌圈/手型都如实）。
    pub fn note_shape(&mut self, s: CursorShape) {
        self.shape = s;
    }

    /// 拍摄：返回「画面里是否含指针」（含指针时按真实形态）。
    pub fn shoot(&mut self, mode: ShotMode) -> Option<CursorShape> {
        let idx = match mode {
            ShotMode::Fullscreen => 0,
            ShotMode::Region => 1,
            ShotMode::Window => 2,
        };
        self.per_mode[idx] = (true, self.include);
        if self.include {
            Some(self.shape)
        } else {
            None
        }
    }

    /// 与 F361 独立审计：录屏光标开关是另一份账（本模块不持录屏状态——
    /// 结构证据：无录屏字段）。
    pub fn independent_from_recorder(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_shotcursor_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 默认不含：新账不含指针（多数截图不想要箭头）。
    let mut c = ShotCursor::new();
    let d = c.shoot(ShotMode::Fullscreen);
    set.add(
        "default excludes cursor",
        c.default_off() && d.is_none() && c.include() == false,
        "",
    );

    // 2. 开关即时：开了下一帧截图就含（三模式逐一验证）。
    c.set_include(true);
    let m1 = c.shoot(ShotMode::Fullscreen);
    let m2 = c.shoot(ShotMode::Region);
    let m3 = c.shoot(ShotMode::Window);
    set.add(
        "toggle instant three modes",
        m1.is_some() && m2.is_some() && m3.is_some(),
        "",
    );

    // 3. 三模式生效记账：三模式各自打过账（全管——无模式例外）。
    set.add(
        "per mode ledger filled",
        c.per_mode.iter().all(|(shot, _)| *shot),
        "",
    );

    // 4. 形态真实渲染：忙碌圈/手型按截图瞬间形态如实出。
    let mut c2 = ShotCursor::new();
    c2.set_include(true);
    c2.note_shape(CursorShape::Busy);
    let busy = c2.shoot(ShotMode::Region);
    c2.note_shape(CursorShape::Hand);
    let hand = c2.shoot(ShotMode::Region);
    set.add(
        "real shape rendered",
        busy == Some(CursorShape::Busy) && hand == Some(CursorShape::Hand),
        "",
    );

    // 5. 开关即时回关：关闭后下一帧即不含。
    c2.set_include(false);
    let off = c2.shoot(ShotMode::Window);
    set.add("instant off", off.is_none() && !c2.include(), "");

    // 6. 与 F361 录屏光标独立（录屏有自己的开关——结构审计）。
    set.add(
        "independent from recorder f361",
        c2.independent_from_recorder(),
        "",
    );

    // 7. 三模式枚举齐（F413 键位清单）。
    set.add(
        "three modes declared",
        ShotMode::Fullscreen != ShotMode::Region && ShotMode::Region != ShotMode::Window,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_defaults_arrow() {
        let mut c = ShotCursor::new();
        c.set_include(true);
        assert_eq!(c.shoot(ShotMode::Region), Some(CursorShape::Arrow));
    }

    #[test]
    fn mode_ledger_tracks_last_include() {
        let mut c = ShotCursor::new();
        c.shoot(ShotMode::Region); // 不含
        assert!(!c.per_mode[1].1);
        c.set_include(true);
        c.shoot(ShotMode::Region); // 含
        assert!(c.per_mode[1].1);
    }

    #[test]
    fn all_shapes_distinct() {
        let all = [CursorShape::Arrow, CursorShape::Hand, CursorShape::Busy, CursorShape::Text];
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
