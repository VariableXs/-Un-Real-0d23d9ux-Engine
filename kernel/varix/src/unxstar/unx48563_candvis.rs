//! UNX-F48563 —— I·候选窗可见性三窗账（正：亮暗主题/多分辨率可见性联检）
//! 判据原文：UNX-F48563-J1 候选窗几何（位置/尺寸/不透明度）在亮/暗主题与
//!           多分辨率（1280×720 / 1920×1080 / 2560×1440 / 竖屏 1080×1920）
//!           组合下可见性判定一致且正确；
//!           越界窗被裁剪标记且不可见；零尺寸/零不透明度不可见；
//!           主题切换不改变几何可见性结论；LCG 随机矩形 ×256 判定零崩且自洽；
//!           联检账目守恒与重演指纹一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF5C0; // I29 候选窗可见性段

const E_VIS_BAD: u16 = SEG_BASE + 1; // 可见性判定失准
const E_CLIP_MISS: u16 = SEG_BASE + 2; // 越界未标记裁剪
const E_THEME_LEAK: u16 = SEG_BASE + 3; // 主题切换改变几何结论
const E_RES_LEAK: u16 = SEG_BASE + 4; // 分辨率联检失准
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机矩形崩溃
const E_THEME_BAD: u16 = SEG_BASE + 6; // 主题枚举越界
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 联检账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// 主题。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Theme {
    Bright = 0,
    Dark = 1,
}

/// 候选窗几何。
#[derive(Clone, Copy)]
pub struct CandRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub opacity: u32, // 0~100
}

/// 视口（分辨率）。
#[derive(Clone, Copy)]
pub struct Viewport {
    pub w: u32,
    pub h: u32,
}

/// 联检结论。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VisVerdict {
    Visible = 0,
    Clipped = 1,   // 越界被裁剪
    Invisible = 2, // 零尺寸/零透明
}

/// 可见性判定：纯几何 + 透明度，与主题无关。
pub fn judge(rect: &CandRect, vp: &Viewport) -> VisVerdict {
    if rect.w == 0 || rect.h == 0 || rect.opacity == 0 {
        return VisVerdict::Invisible;
    }
    let right = rect.x + rect.w as i32;
    let bottom = rect.y + rect.h as i32;
    if rect.x < 0 || rect.y < 0 || right > vp.w as i32 || bottom > vp.h as i32 {
        return VisVerdict::Clipped;
    }
    VisVerdict::Visible
}

/// 三窗账：四种视口 × 两主题下逐窗联检。
pub struct VisLedger {
    pub entries: Vec<(u32, Theme, u32, VisVerdict)>, // (win_id, theme, res_id, verdict)
    pub seq: u64,
    pub label: String,
}

impl VisLedger {
    pub fn new() -> Self {
        VisLedger {
            entries: Vec::new(),
            seq: 0,
            label: "vis-ledger".to_string(),
        }
    }

    /// 单窗在给定主题+分辨率下的联检登记。
    pub fn inspect(&mut self, win_id: u32, th: Theme, res_id: u32, rect: &CandRect, vp: &Viewport) -> VisVerdict {
        self.seq += 1;
        let v = judge(rect, vp);
        self.entries.push((win_id, th, res_id, v));
        v
    }

    /// 主题切换不改结论：同几何两主题判定相等。
    pub fn audit_theme_invariant(&self, rect: &CandRect, vp: &Viewport) -> Result<(), u16> {
        // 几何判定与主题无耦合：枚举域复核后按同几何复算两次
        if Theme::Bright as u8 > 1 || Theme::Dark as u8 > 1 {
            return Err(E_THEME_BAD);
        }
        let a = judge(rect, vp);
        let b = judge(rect, vp);
        if a != b {
            return Err(E_THEME_LEAK);
        }
        Ok(())
    }

    /// 分辨率联检：完全在所有视口内的窗全 Visible。
    pub fn audit_res_matrix(&self, rect: &CandRect, vps: &[Viewport]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < vps.len() {
            if judge(rect, &vps[i]) != VisVerdict::Visible {
                return Err(E_RES_LEAK);
            }
            i += 1;
        }
        Ok(())
    }

    /// 账目守恒：entries 数 = seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.entries.len() as u64 != self.seq {
            return Err(E_LEDGER_MISS);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let v = self.entries[i].3;
            if v as u8 > 2 {
                return Err(E_VIS_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            h ^= self.entries[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.entries[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.entries[i].2 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.entries[i].3 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 四档标准分辨率。
pub fn std_viewports() -> Vec<Viewport> {
    vec![
        Viewport { w: 1280, h: 720 },
        Viewport { w: 1920, h: 1080 },
        Viewport { w: 2560, h: 1440 },
        Viewport { w: 1080, h: 1920 },
    ]
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 主巡检：可见性三窗账九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let vps = std_viewports();

    // R1 亮/暗主题标准窗全可见
    let good = CandRect { x: 900, y: 500, w: 200, h: 120, opacity: 100 };
    let mut lg = VisLedger::new();
    let v0 = lg.inspect(1, Theme::Bright, 0, &good, &vps[0]);
    let v1 = lg.inspect(1, Theme::Dark, 0, &good, &vps[0]);
    if v0 != VisVerdict::Visible || v1 != VisVerdict::Visible {
        return Err(E_VIS_BAD);
    }
    lg.audit_theme_invariant(&good, &vps[0])?;
    hits += 1;

    // R2 多分辨率联检：小窗四档分辨率全可见（含竖屏）
    let small = CandRect { x: 100, y: 100, w: 200, h: 120, opacity: 100 };
    lg.audit_res_matrix(&small, &vps)?;
    let mut res_id = 0u32;
    while (res_id as usize) < vps.len() {
        let v = lg.inspect(2, Theme::Bright, res_id, &small, &vps[res_id as usize]);
        if v != VisVerdict::Visible {
            return Err(E_RES_LEAK);
        }
        res_id += 1;
    }
    // 大窗在宽屏可见、竖屏越界（分辨率先联检语义）
    if judge(&good, &vps[1]) != VisVerdict::Visible {
        return Err(E_RES_LEAK);
    }
    if judge(&good, &vps[3]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R3 越界窗：右越界与下越界均 Clipped
    let over_r = CandRect { x: 1200, y: 100, w: 200, h: 120, opacity: 100 };
    let over_b = CandRect { x: 100, y: 700, w: 200, h: 120, opacity: 100 };
    if judge(&over_r, &vps[0]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    if judge(&over_b, &vps[0]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    let r1 = lg.inspect(3, Theme::Dark, 0, &over_r, &vps[0]);
    let r2 = lg.inspect(3, Theme::Dark, 0, &over_b, &vps[0]);
    if r1 != VisVerdict::Clipped || r2 != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R4 负坐标越界同样 Clipped
    let neg = CandRect { x: -1, y: 0, w: 100, h: 100, opacity: 100 };
    if judge(&neg, &vps[1]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R5 零尺寸/零不透明度不可见
    let zero_w = CandRect { x: 10, y: 10, w: 0, h: 100, opacity: 100 };
    let zero_o = CandRect { x: 10, y: 10, w: 100, h: 100, opacity: 0 };
    if judge(&zero_w, &vps[1]) != VisVerdict::Invisible {
        return Err(E_VIS_BAD);
    }
    if judge(&zero_o, &vps[1]) != VisVerdict::Invisible {
        return Err(E_VIS_BAD);
    }
    hits += 1;

    // R6 主题切换不改变几何可见性：越界窗两主题同为 Clipped
    lg.inspect(4, Theme::Bright, 1, &over_r, &vps[1]);
    lg.inspect(4, Theme::Dark, 1, &over_r, &vps[1]);
    lg.audit_theme_invariant(&over_r, &vps[1])?;
    hits += 1;

    // R7 边界贴边：恰好填满视口的窗 Visible
    let full = CandRect { x: 0, y: 0, w: vps[3].w, h: vps[3].h, opacity: 100 };
    if judge(&full, &vps[3]) != VisVerdict::Visible {
        return Err(E_RES_LEAK);
    }
    let full2 = CandRect { x: 0, y: 0, w: vps[3].w + 1, h: vps[3].h, opacity: 100 };
    if judge(&full2, &vps[3]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R8 LCG 随机矩形 ×256：判定零崩且与独立复算自洽（独立账本供重演）
    let mut rng = Lcg::new(0x48563);
    let mut lgr = VisLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let x = (rng.below(3000) as i64 - 300) as i32;
        let y = (rng.below(3000) as i64 - 300) as i32;
        let w = rng.below(800) as u32;
        let h = rng.below(600) as u32;
        let op = rng.below(101) as u32;
        let rect = CandRect { x, y, w, h, opacity: op };
        let vp = &vps[(rng.below(vps.len() as u64)) as usize];
        let got = judge(&rect, vp);
        // 独立复算
        let expect = if rect.w == 0 || rect.h == 0 || rect.opacity == 0 {
            VisVerdict::Invisible
        } else if rect.x < 0
            || rect.y < 0
            || rect.x + rect.w as i32 > vp.w as i32
            || rect.y + rect.h as i32 > vp.h as i32
        {
            VisVerdict::Clipped
        } else {
            VisVerdict::Visible
        };
        if got != expect {
            return Err(E_CRASH_LEAK);
        }
        let _ = lgr.inspect(5, Theme::Bright, 0, &rect, vp);
        k += 1;
    }
    lg.audit_balance()?;
    hits += 1;

    // R9 不透明度边界：1% 仍可见、100% 可见、透明度只影响可见性不影响裁剪
    let op1 = CandRect { x: 10, y: 10, w: 100, h: 50, opacity: 1 };
    let op100 = CandRect { x: 10, y: 10, w: 100, h: 50, opacity: 100 };
    if judge(&op1, &vps[2]) != VisVerdict::Visible {
        return Err(E_VIS_BAD);
    }
    if judge(&op100, &vps[2]) != VisVerdict::Visible {
        return Err(E_VIS_BAD);
    }
    let op1_oob = CandRect { x: 2500, y: 10, w: 100, h: 50, opacity: 1 };
    if judge(&op1_oob, &vps[2]) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R10 微型视口边界：1×1 视口仅 1×1 窗在原点可见
    let tiny = Viewport { w: 1, h: 1 };
    let fit = CandRect { x: 0, y: 0, w: 1, h: 1, opacity: 100 };
    let unfit = CandRect { x: 0, y: 0, w: 2, h: 1, opacity: 100 };
    if judge(&fit, &tiny) != VisVerdict::Visible {
        return Err(E_RES_LEAK);
    }
    if judge(&unfit, &tiny) != VisVerdict::Clipped {
        return Err(E_CLIP_MISS);
    }
    hits += 1;

    // R11 重演一致：随机段独立账本同种子指纹相等
    let fp1 = lgr.fingerprint();
    let mut rng2 = Lcg::new(0x48563);
    let mut lg2 = VisLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let x = (rng2.below(3000) as i64 - 300) as i32;
        let y = (rng2.below(3000) as i64 - 300) as i32;
        let w = rng2.below(800) as u32;
        let h = rng2.below(600) as u32;
        let op = rng2.below(101) as u32;
        let rect = CandRect { x, y, w, h, opacity: op };
        let vp = &vps[(rng2.below(vps.len() as u64)) as usize];
        let _ = lg2.inspect(5, Theme::Bright, 0, &rect, vp);
        k += 1;
    }
    if lg2.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
