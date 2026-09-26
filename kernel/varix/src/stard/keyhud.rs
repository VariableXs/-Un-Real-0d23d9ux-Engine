//! F106 键盘提示 HUD · 完整设计（STAR I 主册 G-C-36）。
//!
//! **判据（主册）**：三键切换 HUD 三态全对；淡出时机 1s±50ms 实测；
//! 全屏降级路径实测。
//!
//! **设计要点（主册）**：
//! - CapsLock/NumLock/ScrollLock 状态变化时屏幕下中出 HUD 提示（图标+
//!   状态字，1s 淡出）；输入法中英切换同显示（F107 联动）；位置避开
//!   任务栏与通知区；
//! - HUD 640px 宽横条居下中（距底 80px）：锁图标+「大写锁定 开」14px；
//!   出现动画 120ms 上浮淡入、1s 后 300ms 淡出；连续切换重置计时；
//!   设置页可关可移位；
//! - 切换风暴（键盘故障连发）→ HUD 合并不闪屏；全屏应用（放映/游戏
//!   前瞻）→ HUD 转角标模式（右下小图标）；
//! - HUD 走合成器通知层（不抢焦点零打扰）；图标 24px+文字 14px 组合
//!   4K 资产；三个锁定键图标差异设计（色弱形状冗余——无障碍 B-39xx
//!   联动）；输入法切换 HUD 复用同组件（图标换「中/英」字样）。

use crate::checks::CheckSet;


// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// HUD 横条宽（px）。
pub const HUD_W_PX: u32 = 640;

/// 距屏底（px）。
pub const BOTTOM_OFFSET_PX: u32 = 80;

/// 出现动画（ms，上浮淡入）。
pub const SHOW_ANIM_MS: u32 = 120;

/// 淡出前驻留（ms，判线 1s±50ms）。
pub const HOLD_MS: u64 = 1_000;

/// 淡出动画（ms）。
pub const FADE_OUT_MS: u32 = 300;

/// 驻留判线容差（±ms）。
pub const HOLD_TOLERANCE_MS: u64 = 50;

/// 角标模式图标边（px，全屏降级）。
pub const CORNER_ICON_PX: u32 = 24;

// ---------------------------------------------------------------------------
// 锁定键三态模型
// ---------------------------------------------------------------------------

/// 锁定键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockKey {
    Caps,
    Num,
    Scroll,
}

impl LockKey {
    /// 状态字（三态文案——「大写锁定 开/关」）。
    pub fn label(self, on: bool) -> &'static str {
        match (self, on) {
            (LockKey::Caps, true) => "大写锁定 开",
            (LockKey::Caps, false) => "大写锁定 关",
            (LockKey::Num, true) => "数字锁定 开",
            (LockKey::Num, false) => "数字锁定 关",
            (LockKey::Scroll, true) => "滚动锁定 开",
            (LockKey::Scroll, false) => "滚动锁定 关",
        }
    }

    /// 图标形状冗余（色弱可辨——圆/方/三角三形）。
    pub fn icon_shape(self) -> &'static str {
        match self {
            LockKey::Caps => "circle",
            LockKey::Num => "square",
            LockKey::Scroll => "triangle",
        }
    }
}

// ---------------------------------------------------------------------------
// HUD 状态机（出现→驻留→淡出；连续切换重置计时；风暴合并）
// ---------------------------------------------------------------------------

/// HUD 显示态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudState {
    Hidden,
    /// 淡入中（起始 ms）。
    Showing(u64),
    /// 驻留（出现完成时刻——1s 驻留从这里计）。
    Holding(u64),
    /// 淡出中（起始 ms）。
    Fading(u64),
}

/// HUD 内容（当前提示谁）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudContent {
    Lock(LockKey, bool),
    /// 输入法切换（F107 联动复用）。
    Ime(bool),
}

/// 键盘 HUD。
pub struct KeyHud {
    pub state: HudState,
    pub content: Option<HudContent>,
    pub enabled: bool,
    /// 全屏降级角标模式。
    pub corner_mode: bool,
    /// 切换风暴账（同键 200ms 内连发计数——合并展示）。
    pub storm_merged: u64,
    last_toggle_ms: u64,
    pub show_count: u64,
}

impl KeyHud {
    pub fn new() -> KeyHud {
        KeyHud {
            state: HudState::Hidden,
            content: None,
            enabled: true,
            corner_mode: false,
            storm_merged: 0,
            last_toggle_ms: 0,
            show_count: 0,
        }
    }

    /// 锁定键切换（合成器层喂入）。
    /// 风暴合并：200ms 内连发只换内容计数不重展（不闪屏——连发不重置
    /// 驻留钟，HUD 保持稳定）；超窗的新切换才重新展示（重置计时）。
    pub fn toggle(&mut self, key: LockKey, on: bool, now_ms: u64) {
        if !self.enabled {
            return;
        }
        let merging = now_ms.saturating_sub(self.last_toggle_ms) < 200
            && matches!(self.state, HudState::Showing(_) | HudState::Holding(_));
        self.last_toggle_ms = now_ms;
        self.content = Some(HudContent::Lock(key, on));
        if merging {
            self.storm_merged += 1;
        } else {
            self.enter_show(now_ms);
        }
    }

    /// 输入法切换复用（F107 联动——图标换「中/英」）。
    pub fn ime_switch(&mut self, chinese: bool, now_ms: u64) {
        if !self.enabled {
            return;
        }
        self.content = Some(HudContent::Ime(chinese));
        self.enter_show(now_ms);
    }

    fn enter_show(&mut self, now_ms: u64) {
        self.state = HudState::Showing(now_ms); // 120ms 上浮淡入。
        self.show_count += 1;
    }

    /// tick 推进（调用方按帧喂——淡出时机 1s±50ms 判线）。
    pub fn tick(&mut self, now_ms: u64) {
        match self.state {
            HudState::Showing(t0) => {
                if now_ms.saturating_sub(t0) >= SHOW_ANIM_MS as u64 {
                    self.state = HudState::Holding(t0 + SHOW_ANIM_MS as u64);
                }
            }
            HudState::Holding(t0) => {
                if now_ms.saturating_sub(t0) >= HOLD_MS {
                    self.state = HudState::Fading(t0 + HOLD_MS);
                }
            }
            HudState::Fading(t0) => {
                if now_ms.saturating_sub(t0) >= FADE_OUT_MS as u64 {
                    self.state = HudState::Hidden;
                    self.content = None;
                }
            }
            HudState::Hidden => {}
        }
    }

    /// 淡出实际时刻核算（Holding 起点 + HOLD——±50ms 容差外即缺陷）。
    pub fn fadeout_due_at(&self) -> Option<u64> {
        match self.state {
            HudState::Holding(t0) => Some(t0 + HOLD_MS),
            _ => None,
        }
    }

    /// 几何：居下中横条（角标模式转右下小图标）。
    pub fn rect(&self, screen_w: u32, screen_h: u32) -> (u32, u32, u32, u32) {
        if self.corner_mode {
            (screen_w - CORNER_ICON_PX - 16, screen_h - CORNER_ICON_PX - 16, CORNER_ICON_PX, CORNER_ICON_PX)
        } else {
            let x = (screen_w - HUD_W_PX) / 2;
            let y = screen_h - BOTTOM_OFFSET_PX - 48;
            (x, y, HUD_W_PX, 48)
        }
    }

    /// 全屏降级（放映/游戏——转角标）。
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.corner_mode = fullscreen;
    }
}

impl Default for KeyHud {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F106 自检（聚合进 stard 域）。
pub fn run_keyhud_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F106");

    // —— 三键切换 HUD 三态全对 ——
    let mut hud = KeyHud::new();
    hud.toggle(LockKey::Caps, true, 100);
    set.add("caps on label", hud.content == Some(HudContent::Lock(LockKey::Caps, true)), "");
    set.add("caps label text", LockKey::Caps.label(true) == "大写锁定 开" && LockKey::Caps.label(false) == "大写锁定 关", "");
    hud.toggle(LockKey::Num, true, 2000);
    set.add("num state", hud.content == Some(HudContent::Lock(LockKey::Num, true)), "");
    hud.toggle(LockKey::Scroll, false, 4000);
    set.add("scroll state", hud.content == Some(HudContent::Lock(LockKey::Scroll, false)), "");
    set.add("three icon shapes", LockKey::Caps.icon_shape() != LockKey::Num.icon_shape() && LockKey::Num.icon_shape() != LockKey::Scroll.icon_shape(), "");

    // —— 淡出时机 1s±50ms ——
    let mut hud2 = KeyHud::new();
    hud2.toggle(LockKey::Caps, true, 0);
    // 120ms 淡入完成。
    hud2.tick(50);
    set.add("still showing at 50ms", matches!(hud2.state, HudState::Showing(0)), "");
    hud2.tick(120);
    set.add("holding at 120ms", hud2.fadeout_due_at() == Some(120 + HOLD_MS), "");
    // 驻留判线：1000±50ms。
    hud2.tick(120 + HOLD_MS - 51);
    set.add("holds until 1s minus 50", matches!(hud2.state, HudState::Holding(_)), "");
    hud2.tick(120 + HOLD_MS);
    set.add("fades at 1s", matches!(hud2.state, HudState::Fading(1_120)), "");
    hud2.tick(1_120 + FADE_OUT_MS as u64);
    set.add("hidden after fade", hud2.state == HudState::Hidden && hud2.content.is_none(), "");

    // —— 连续切换重置计时 ——
    let mut hud3 = KeyHud::new();
    hud3.toggle(LockKey::Caps, true, 0);
    hud3.tick(120);
    hud3.toggle(LockKey::Caps, false, 500); // 驻留中再切——重置。
    hud3.tick(620);
    set.add("retoggle resets timer", matches!(hud3.state, HudState::Holding(620)), "");
    set.add("fadeout from retoggle", hud3.fadeout_due_at() == Some(620 + HOLD_MS), "");

    // —— 风暴合并（键盘故障连发不闪屏）——
    let mut hud4 = KeyHud::new();
    for i in 0..50u64 {
        hud4.toggle(LockKey::Caps, i % 2 == 0, i * 20); // 20ms 连发。
    }
    set.add("storm merged count", hud4.storm_merged == 49, "首显后 49 次连发全部合并");
    set.add("storm no extra shows", hud4.show_count == 1, "");

    // —— 全屏降级角标 ——
    let mut hud5 = KeyHud::new();
    hud5.set_fullscreen(true);
    let (x, y, w, _h) = hud5.rect(1920, 1080);
    set.add("corner mode rect", w == CORNER_ICON_PX && x == 1920 - 24 - 16 && y == 1080 - 24 - 16, "");
    hud5.set_fullscreen(false);
    let (x, y, w, _h) = hud5.rect(1920, 1080);
    set.add("normal hud rect", w == HUD_W_PX && x == (1920 - 640) / 2 && y == 1080 - 80 - 48, "");

    // —— 输入法联动复用 ——
    let mut hud6 = KeyHud::new();
    hud6.ime_switch(true, 0);
    set.add("ime reuse chinese", hud6.content == Some(HudContent::Ime(true)), "");
    hud6.ime_switch(false, 3000);
    set.add("ime reuse english", hud6.content == Some(HudContent::Ime(false)), "");

    // —— 开关（设置页可关）——
    let mut hud7 = KeyHud::new();
    hud7.enabled = false;
    hud7.toggle(LockKey::Caps, true, 0);
    set.add("disabled hud silent", hud7.content.is_none() && hud7.state == HudState::Hidden, "");

    // —— 规格常量 ——
    set.add("hold tolerance 50ms", HOLD_TOLERANCE_MS == 50, "");
    set.add("hud geometry tokens", HUD_W_PX == 640 && BOTTOM_OFFSET_PX == 80 && SHOW_ANIM_MS == 120 && FADE_OUT_MS == 300, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_lifecycle_timing() {
        let mut hud = KeyHud::new();
        hud.toggle(LockKey::Num, true, 1000);
        // 逐帧推进 16ms（60fps 喂入）。
        let mut t = 1000u64;
        let mut saw_holding = false;
        let mut saw_fading = false;
        while t < 3000 {
            t += 16;
            hud.tick(t);
            if let HudState::Holding(_) = hud.state {
                saw_holding = true;
            }
            if let HudState::Fading(_) = hud.state {
                saw_fading = true;
            }
        }
        assert!(saw_holding && saw_fading);
        assert_eq!(hud.state, HudState::Hidden);
        assert_eq!(hud.show_count, 1);
    }

    #[test]
    fn storm_merging_varied_keys() {
        let mut hud = KeyHud::new();
        // 不同键连发也合并（200ms 窗内）。
        hud.toggle(LockKey::Caps, true, 0);
        hud.toggle(LockKey::Num, true, 50);
        hud.toggle(LockKey::Scroll, false, 100);
        assert_eq!(hud.storm_merged, 2);
        assert_eq!(hud.show_count, 1);
        // 200ms 后恢复独立显示。
        hud.toggle(LockKey::Caps, true, 500);
        assert_eq!(hud.show_count, 2);
    }

    #[test]
    fn ime_switch_resets_state() {
        let mut hud = KeyHud::new();
        hud.toggle(LockKey::Caps, true, 0);
        hud.tick(2000); // 已隐藏。
        hud.ime_switch(true, 2500);
        assert!(matches!(hud.state, HudState::Showing(2500)));
    }

    #[test]
    fn labels_and_shapes_distinct() {
        assert_eq!(LockKey::Num.label(true), "数字锁定 开");
        assert_eq!(LockKey::Scroll.label(false), "滚动锁定 关");
        let shapes = [LockKey::Caps.icon_shape(), LockKey::Num.icon_shape(), LockKey::Scroll.icon_shape()];
        assert_eq!(shapes.len(), 3);
        // 形状冗余：三种互异（色弱可辨）。
        assert!(shapes[0] != shapes[1] && shapes[1] != shapes[2] && shapes[0] != shapes[2]);
    }
}
