//! VE-F4305 · 一致性×主题生态（U06 场景应用组）
//! 职责：主题上架基线三项（令牌完整覆盖/对比度达标/暗色等价）+ 生态巡检
//! 同步提醒（三次未响应降权）+ 劣化梯度（降权直至下架）+ 申诉复审改判
//! + 对比度基线即无障碍硬线（域色）+ 门禁复用同域第二用。
//! 段位：0x3060（续表 idx539）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3060;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmCode {
    /// 基线缺项（拒收）。
    BaseMissing = SEG_BASE,
    /// 对比度不达标（无障碍硬线——域色）。
    ContrastFail = SEG_BASE + 0x01,
    /// 下架。
    OffShelf = SEG_BASE + 0x02,
}

/// 主题基线三项。
#[derive(Clone, Copy, Debug)]
pub struct ThemeBase {
    pub token_cover: u32,   // 令牌覆盖率 bp
    pub contrast_bp: u32,   // 对比度 bp（域色硬线）
    pub dark_equiv: bool,   // 暗色等价
}

impl ThemeBase {
    pub fn ok(&self, cover_cap: u32, contrast_cap: u32) -> bool {
        self.token_cover >= cover_cap && self.contrast_bp >= contrast_cap && self.dark_equiv
    }
}

/// 主题资产。
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub id: u16,
    pub base: ThemeBase,
    pub listed: bool,
    pub weight: u8, // 权重（降权梯度 0-100）
    pub misses: u8, // 巡检未响应次数
    pub off: bool,
}

/// 主题生态器。
pub struct ThemeEco {
    pub themes: Vec<Theme>,
    /// 令牌全集数。
    pub token_total: u32,
    pub contrast_cap_bp: u32,
    pub cover_cap_bp: u32,
    pub appeals: Vec<(u16, bool)>,
}

impl ThemeEco {
    pub fn new(token_total: u32, cover_cap_bp: u32, contrast_cap_bp: u32) -> Self {
        ThemeEco { themes: Vec::new(), token_total, cover_cap_bp, contrast_cap_bp, appeals: Vec::new() }
    }

    /// 上架申请：三项基线。
    pub fn apply(&mut self, id: u16, covered_tokens: u32, contrast_bp: u32, dark: bool) -> Result<(), TmCode> {
        let cover = covered_tokens * 10_000 / self.token_total.max(1);
        let base = ThemeBase { token_cover: cover, contrast_bp, dark_equiv: dark };
        if contrast_bp < self.contrast_cap_bp {
            return Err(TmCode::ContrastFail); // 域色硬线
        }
        if !base.ok(self.cover_cap_bp, self.contrast_cap_bp) {
            return Err(TmCode::BaseMissing);
        }
        self.themes.push(Theme { id, base, listed: true, weight: 100, misses: 0, off: false });
        Ok(())
    }

    /// 巡检提醒：未响应计数。
    pub fn patrol_miss(&mut self, id: u16) -> Result<(), TmCode> {
        let t = self.themes.iter_mut().find(|t| t.id == id).ok_or(TmCode::OffShelf)?;
        t.misses += 1;
        if t.misses >= 3 {
            t.weight = t.weight.saturating_sub(30); // 降权梯度
            if t.weight == 0 {
                t.listed = false;
                t.off = true;
                return Err(TmCode::OffShelf);
            }
        }
        Ok(())
    }

    /// 申诉复审改判。
    pub fn appeal(&mut self, id: u16, ok: bool) -> Result<(), TmCode> {
        let t = self.themes.iter_mut().find(|t| t.id == id).ok_or(TmCode::OffShelf)?;
        self.appeals.push((id, ok));
        if ok {
            t.misses = 0;
            t.weight = 100;
            if t.off {
                t.off = false;
                t.listed = true;
            }
            Ok(())
        } else {
            Err(TmCode::OffShelf)
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C539-01..12。
pub fn run_ves539_checks() -> CheckSet {
    let mut set = CheckSet::new("ves539");

    let mut te = ThemeEco::new(100, 9_000, 4_500);
    chk(&mut set, "C539-01", te.apply(1, 95, 4_000, true) == Err(TmCode::ContrastFail), "对比度不达标=域色硬线拒收");
    chk(&mut set, "C539-02", te.apply(1, 50, 5_000, true) == Err(TmCode::BaseMissing), "令牌覆盖不足拒收");
    chk(&mut set, "C539-03", te.apply(1, 95, 5_000, false) == Err(TmCode::BaseMissing), "暗色不等价拒收");
    chk(&mut set, "C539-04", te.apply(1, 95, 5_000, true).is_ok(), "三项基线全过上架");
    chk(&mut set, "C539-05", te.themes[0].weight == 100, "初始权重满");

    // 巡检降权梯度
    let _ = te.patrol_miss(1);
    let _ = te.patrol_miss(1);
    chk(&mut set, "C539-06", te.themes[0].misses == 2 && te.themes[0].weight == 100, "两次未响应仍满权（3 次触发）");
    chk(&mut set, "C539-07", te.patrol_miss(1).is_ok(), "第三次触发降权 30");
    chk(&mut set, "C539-08", te.themes[0].weight == 70, "降权梯度在册");

    // 申诉复位
    chk(&mut set, "C539-09", te.appeal(1, true).is_ok() && te.themes[0].weight == 100, "申诉成立复位");
    chk(&mut set, "C539-10", te.appeals.len() == 1 && te.appeals[0].1, "改判留痕");

    // 极端降权至下架
    for _ in 0..9 {
        let _ = te.patrol_miss(1);
    }
    chk(&mut set, "C539-11", te.themes[0].off, "多次未响应权重归零下架");
    chk(&mut set, "C539-12", te.appeal(1, true).is_ok() && !te.themes[0].off, "申诉复探下架复位");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_eco() {
        let mut te = ThemeEco::new(10, 9_000, 4_500);
        assert!(te.apply(1, 10, 5_000, true).is_ok());
        assert_eq!(te.apply(2, 10, 4_000, true), Err(TmCode::ContrastFail));
        for _ in 0..3 {
            let _ = te.patrol_miss(1);
        }
        assert_eq!(te.themes[0].weight, 70);
    }
}
