//! F590 深浅壁纸配对 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：双槽切换联动；单槽兜底；压暗协同；多屏叠加；
//! 持久化与备份（F396）。
//!
//! **设计要点（主册）**：
//! - 深浅主题各配一张壁纸：设置页壁纸区双槽（浅色壁纸/深色壁纸）——
//!   主题切换（F225/F153 自动档）壁纸跟随换（不是一张壁纸硬扛两种主题）；
//! - 只配一张时另一主题沿用同图（F297 压暗兜底）；双槽预览即时；
//! - 与 F286 多屏模式叠加（每屏×每主题都成立）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 主题槽（双槽枚举——槽位唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSlot {
    Light,
    Dark,
}

/// 压暗兜底系数（%，F297 同源——深色主题沿用浅色壁纸时的压暗档）。
pub const DIM_FALLBACK_PCT: u8 = 30;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 壁纸双槽账。
pub struct WallPair {
    light: Option<String>,
    dark: Option<String>,
    /// 当前主题。
    current: ThemeSlot,
    /// 多屏叠加：屏序 → 独立双槽覆盖（None = 跟随主槽）。
    per_screen: Vec<(usize, Option<String>, Option<String>)>,
    /// 持久化落账次数（F396 备份对账面）。
    persists: u32,
}

impl WallPair {
    pub fn new() -> WallPair {
        WallPair {
            light: None,
            dark: None,
            current: ThemeSlot::Light,
            per_screen: Vec::new(),
            persists: 0,
        }
    }

    /// 设槽（即时预览——设完当前主题若命中该槽立即生效）。
    pub fn set_wall(&mut self, slot: ThemeSlot, image: &str) {
        let was = self.active_image();
        match slot {
            ThemeSlot::Light => self.light = Some(String::from(image)),
            ThemeSlot::Dark => self.dark = Some(String::from(image)),
        }
        if was != self.active_image() {
            self.persists += 1; // 生效变化才落盘（F396 备份账）
        }
    }

    /// 主题切换（F225/F153 联动——壁纸跟随换）。
    pub fn switch_theme(&mut self, to: ThemeSlot) {
        self.current = to;
    }

    /// 当前生效壁纸。
    pub fn active_image(&self) -> String {
        let (slot, other) = match self.current {
            ThemeSlot::Light => (&self.light, &self.dark),
            ThemeSlot::Dark => (&self.dark, &self.light),
        };
        slot.clone()
            .or_else(|| other.clone())
            .unwrap_or_else(|| String::from(""))
    }

    /// 单槽兜底判定：当前主题槽空但另一槽有图 → 沿用 + 压暗（F297 协同）。
    pub fn dim_fallback_active(&self) -> bool {
        let (slot, other) = match self.current {
            ThemeSlot::Light => (&self.light, &self.dark),
            ThemeSlot::Dark => (&self.dark, &self.light),
        };
        slot.is_none() && other.is_some()
    }

    /// 压暗参数（兜底时返回 Some(压暗%)；双槽齐备 None——压暗只属于兜底）。
    pub fn dim_percent(&self) -> Option<u8> {
        if self.dim_fallback_active() && self.current == ThemeSlot::Dark {
            Some(DIM_FALLBACK_PCT)
        } else {
            None
        }
    }

    /// 多屏叠加：屏级独立配置（每屏 × 每主题都成立——屏配置覆盖主槽）。
    pub fn set_screen(&mut self, screen: usize, slot: ThemeSlot, image: &str) {
        let entry = match self.per_screen.iter_mut().find(|(s, ..)| *s == screen) {
            Some(e) => e,
            None => {
                self.per_screen.push((screen, None, None));
                self.per_screen.last_mut().unwrap()
            }
        };
        match slot {
            ThemeSlot::Light => entry.1 = Some(String::from(image)),
            ThemeSlot::Dark => entry.2 = Some(String::from(image)),
        }
    }

    /// 屏级生效图（屏配置优先，无配置回落主账）。
    pub fn active_image_on(&self, screen: usize) -> String {
        if let Some((_, l, d)) = self.per_screen.iter().find(|(s, ..)| *s == screen) {
            let (slot, other) = match self.current {
                ThemeSlot::Light => (l, d),
                ThemeSlot::Dark => (d, l),
            };
            if let Some(img) = slot.clone().or_else(|| other.clone()) {
                return img;
            }
        }
        self.active_image()
    }

    /// 持久化落账次数（F396）。
    pub fn persist_count(&self) -> u32 {
        self.persists
    }
}

impl Default for WallPair {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_wallpair_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 双槽切换联动：浅配亮图深配暗图，切主题壁纸跟着换。
    let mut w = WallPair::new();
    w.set_wall(ThemeSlot::Light, "亮图.png");
    w.set_wall(ThemeSlot::Dark, "暗图.png");
    let light_img = w.active_image() == "亮图.png";
    w.switch_theme(ThemeSlot::Dark);
    let dark_img = w.active_image() == "暗图.png";
    set.add("dual slot follows theme", light_img && dark_img, "");

    // 2. 单槽兜底：深槽空 → 沿用亮图 + 压暗 30%（F297 协同）。
    let mut w2 = WallPair::new();
    w2.set_wall(ThemeSlot::Light, "亮图.png");
    w2.switch_theme(ThemeSlot::Dark);
    let fallback_img = w2.active_image() == "亮图.png";
    let dimmed = w2.dim_fallback_active() && w2.dim_percent() == Some(DIM_FALLBACK_PCT);
    set.add(
        "single slot fallback with dim",
        fallback_img && dimmed,
        "",
    );

    // 3. 压暗协同边界：浅色主题沿用暗图不压暗（压暗只属于深色兜底）。
    let mut w3 = WallPair::new();
    w3.set_wall(ThemeSlot::Dark, "暗图.png");
    w3.switch_theme(ThemeSlot::Light);
    set.add(
        "dim only for dark fallback",
        w3.dim_fallback_active() && w3.dim_percent().is_none(),
        "",
    );

    // 4. 双槽齐备：不压暗（压暗是兜底专属）。
    let mut w4 = WallPair::new();
    w4.set_wall(ThemeSlot::Light, "a.png");
    w4.set_wall(ThemeSlot::Dark, "b.png");
    w4.switch_theme(ThemeSlot::Dark);
    set.add(
        "both slots no dim",
        w4.dim_percent().is_none() && w4.active_image() == "b.png",
        "",
    );

    // 5. 多屏叠加：屏 1 独立配置覆盖；屏 2 无配置回落主账。
    let mut w5 = WallPair::new();
    w5.set_wall(ThemeSlot::Light, "主亮.png");
    w5.set_screen(1, ThemeSlot::Light, "屏1亮.png");
    w5.switch_theme(ThemeSlot::Light);
    let s1 = w5.active_image_on(1);
    let s2 = w5.active_image_on(2);
    set.add(
        "multi screen overlay",
        s1 == "屏1亮.png" && s2 == "主亮.png",
        "",
    );

    // 6. 持久化与备份：生效变化落账、同值重设不落账（F396 账面不虚增）。
    let before = w5.persist_count();
    w5.set_wall(ThemeSlot::Light, "主亮.png"); // 同值
    let after_same = w5.persist_count();
    w5.set_wall(ThemeSlot::Light, "新亮.png"); // 变值
    set.add(
        "persist on effective change",
        after_same == before && w5.persist_count() == before + 1,
        "",
    );

    // 7. 双槽预览即时：设槽后 active_image 即按当前主题取新值（同取数口）。
    w5.switch_theme(ThemeSlot::Dark);
    w5.set_wall(ThemeSlot::Dark, "预览暗.png");
    set.add(
        "instant preview same source",
        w5.active_image() == "预览暗.png",
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pair_empty_image() {
        let w = WallPair::new();
        assert_eq!(w.active_image(), "");
        assert!(!w.dim_fallback_active());
    }

    #[test]
    fn screen_overlay_falls_back_to_fallback() {
        // 屏配置空 + 主槽单配 → 屏走主账兜底。
        let mut w = WallPair::new();
        w.set_wall(ThemeSlot::Dark, "暗.png");
        w.switch_theme(ThemeSlot::Dark);
        assert_eq!(w.active_image_on(9), "暗.png");
    }

    #[test]
    fn theme_switch_alone_no_persist() {
        // 切主题不落盘（壁纸槽没变）——落盘只随生效变化。
        let mut w = WallPair::new();
        w.set_wall(ThemeSlot::Light, "a.png");
        w.set_wall(ThemeSlot::Dark, "b.png");
        let n = w.persist_count();
        w.switch_theme(ThemeSlot::Dark);
        assert_eq!(w.persist_count(), n);
    }
}
