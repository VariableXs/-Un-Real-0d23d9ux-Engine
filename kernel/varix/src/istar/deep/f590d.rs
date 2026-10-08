//! 深化层 · F590 深浅壁纸配对（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F590 节）：
//! ①「每屏×每主题都成立」的**矩阵一致性校验器**——多屏各自配双槽后，
//!   主题切换必须让**每一屏**都换（一屏漏切 = 矩阵残缺，立红）；
//! ②「只配一张时另一主题沿用同图（F297 压暗兜底）」的**兜底复核**——
//!   单槽机的切主题不换图、压暗账激活、压暗幅度在册；
//! ③「持久化与备份（F396）」的**配置幂等账**——同一配置重复下发
//!   不产生第二份持久化账（备份去重，不生产垃圾账）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::wallpair::{ThemeSlot, WallPair, DIM_FALLBACK_PCT};

// ---------------------------------------------------------------------------
// 每屏×每主题矩阵一致性
// ---------------------------------------------------------------------------

/// 矩阵核对：双槽齐配时切主题后逐屏核对壁纸是否跟随
/// （一屏没变 = 矩阵残缺；单槽兜底机的「不变」由调用方先排除）。
pub fn matrix_consistent(before: &[String], after: &[String]) -> bool {
    if before.len() != after.len() || before.is_empty() {
        return false;
    }
    before.iter().zip(after.iter()).all(|(b, a)| b != a)
}

/// 配置幂等账：同屏同槽同图重复下发，账面只记一次。
pub struct IdempotentConfig {
    seen: [Option<(usize, ThemeSlot, u64)>; 8],
    len: usize,
    writes: u32,
}

impl IdempotentConfig {
    pub fn new() -> IdempotentConfig {
        IdempotentConfig { seen: [None; 8], len: 0, writes: 0 }
    }

    /// 配置下发：重复配置只记一次（返回 true = 本次真实落账）；
    /// 账满 8 屏后新配置诚实拒绝（不静默丢）。
    pub fn apply(&mut self, screen: usize, slot: ThemeSlot, image_hash: u64) -> bool {
        let dup = self.seen[..self.len]
            .iter()
            .any(|e| e.map(|(s, sl, h)| s == screen && sl == slot && h == image_hash).unwrap_or(false));
        if dup || self.len >= 8 {
            return false;
        }
        self.seen[self.len] = Some((screen, slot, image_hash));
        self.len += 1;
        self.writes += 1;
        true
    }

    pub fn writes(&self) -> u32 {
        self.writes
    }
}

impl Default for IdempotentConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f590_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 双槽齐配：切主题 → 每一屏都换（矩阵一致性）。
    let mut w = WallPair::new();
    w.set_screen(0, ThemeSlot::Light, "light-a.png");
    w.set_screen(0, ThemeSlot::Dark, "dark-a.png");
    w.set_screen(1, ThemeSlot::Light, "light-b.png");
    w.set_screen(1, ThemeSlot::Dark, "dark-b.png");
    let before: alloc::vec::Vec<String> =
        (0..2).map(|s| w.active_image_on(s)).collect();
    w.switch_theme(ThemeSlot::Dark);
    let after: alloc::vec::Vec<String> = (0..2).map(|s| w.active_image_on(s)).collect();
    cs.add(
        "matrix all screens follow theme",
        matrix_consistent(&before, &after),
        "",
    );

    // 2) 单槽兜底：只配浅槽 → 切深沿用同图 + 压暗激活 + 幅度在册。
    let mut w2 = WallPair::new();
    w2.set_wall(ThemeSlot::Light, "only.png");
    w2.switch_theme(ThemeSlot::Dark);
    cs.add(
        "single slot fallback dims",
        w2.active_image() == "only.png"
            && w2.dim_fallback_active()
            && w2.dim_percent() == Some(DIM_FALLBACK_PCT),
        "",
    );

    // 3) 双槽机切主题压暗不激活（兜底只属于单槽机——语义不越界）。
    cs.add("dual slot no dim", !w.dim_fallback_active(), "");

    // 4) 配置幂等账：同配置重复下发只落一次账。
    let mut idem = IdempotentConfig::new();
    let a = idem.apply(0, ThemeSlot::Light, 111);
    let b = idem.apply(0, ThemeSlot::Light, 111);
    let c = idem.apply(0, ThemeSlot::Light, 222);
    cs.add(
        "config idempotent",
        a && !b && c && idem.writes() == 2,
        "",
    );

    // 5) 持久化账随真实配置增长（F396 备份面有账可对）。
    let _ = w2.set_wall(ThemeSlot::Dark, "dark-late.png");
    cs.add("persist ledger grows", w2.persist_count() >= 2, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idem_capacity_8() {
        let mut c = IdempotentConfig::new();
        for i in 0..8u64 {
            assert!(c.apply(i as usize, ThemeSlot::Light, i));
        }
        assert!(!c.apply(0, ThemeSlot::Light, 999)); // 满 8 屏诚实拒绝
    }
}
