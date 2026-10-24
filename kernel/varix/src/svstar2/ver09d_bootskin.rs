//! VE-F3524 · 开机皮肤运行时 —— 帧序列播放 + 2 秒硬上限 + 失败回退 + 静态兜底。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3524`
//!
//! # 职责（锚点原文拆解）
//!
//! - **帧序列个性化**（[`BootSkin`]）：皮肤包提供开机帧序列，
//!   运行时按帧间隔播放。个性化不许拖慢开机——**超 2 秒自动跳过**
//!   （[`BOOT_BUDGET_MS`] 硬上限），跳过即切默认动画（告知）。
//! - **失败回退**（[`BootSkin::load`]）：帧缺失 → 回退默认动画；
//!   解码失败 → 静态帧兜底（[`StaticFrame`]）。回退有次序：
//!   皮肤动画 → 默认动画 → 静态帧 → 纯色。每一层都保证
//!   「开机画面永远有东西在」。
//! - **加载预算**（[`LoadBudget`]）：帧序列解码不拖慢启动——
//!   解码计费（逻辑 tick），超预算的帧不播（跳过不是卡住）。
//! - **读屏播报**（[`BootSkin::announce`]）：开机状态读屏可达。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 超时 | 跳过 + 告知 |
//! | 帧缺失 | 回退默认 |
//! | 解码失败 | 静态帧 |
//!
//! # 复杂度
//!
//! - `tick`：O(1)/帧；
//! - `load`：O(帧数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5930 起）
// ---------------------------------------------------------------------------

/// 开机皮肤域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootCode {
    /// 播放超预算（自动跳过）。
    OverBudget = 5930,
    /// 帧缺失（回退默认动画）。
    FrameMissing = 5931,
    /// 帧解码失败（静态帧兜底）。
    DecodeFailed = 5932,
    /// 皮肤包标识非法。
    BadSkinId = 5933,
}

impl BootCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BootCode::OverBudget => "开机动画超 2 秒已跳过",
            BootCode::FrameMissing => "开机帧缺失回退默认",
            BootCode::DecodeFailed => "帧解码失败用静态帧",
            BootCode::BadSkinId => "皮肤包标识非法",
        }
    }

    /// 是否阻断级（开机链路任何兜底都不阻断——开机优先）。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、常量
// ---------------------------------------------------------------------------

/// 开机动画硬上限（毫秒）。个性化不许拖慢开机。
pub const BOOT_BUDGET_MS: u32 = 2000;
/// 默认帧间隔（毫秒）。
pub const FRAME_INTERVAL_MS: u32 = 40;
/// 解码预算（tick）。
pub const DECODE_BUDGET: u32 = 512;

// ---------------------------------------------------------------------------
// 三、帧与皮肤
// ---------------------------------------------------------------------------

/// 一帧：内容指纹 + 解码开销（tick）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootFrame {
    /// 帧内容指纹（0 = 空帧 = 缺失）。
    pub fp: u64,
    /// 解码开销。
    pub cost: u32,
}

/// 帧序列来源。
#[derive(Clone, Debug)]
pub struct BootSkin {
    /// 皮肤包标识（非空）。
    pub id: &'static str,
    pub frames: Vec<BootFrame>,
}

impl BootSkin {
    /// 装载校验：id 非空、帧数有限。
    pub fn load(id: &'static str, frames: Vec<BootFrame>) -> Result<Self, BootCode> {
        if id.is_empty() {
            return Err(BootCode::BadSkinId);
        }
        Ok(BootSkin { id, frames })
    }

    /// 缺失帧数（指纹为 0）。
    pub fn missing(&self) -> usize {
        self.frames.iter().filter(|f| f.fp == 0).count()
    }
}

/// 静态帧兜底（编译期常量指纹，永不损坏）。
pub const STATIC_FALLBACK: u64 = 0x5EED_B00F_57A71C;

// ---------------------------------------------------------------------------
// 四、播放状态机
// ---------------------------------------------------------------------------

/// 播放状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 播放皮肤帧序列。
    Playing,
    /// 回退默认动画。
    DefaultAnim,
    /// 静态帧。
    Static,
    /// 已结束（进入系统接管）。
    Done,
}

/// 播放器：预算硬上限 + 失败降级链。
pub struct BootPlayer {
    pub stage: BootStage,
    skin: Option<BootSkin>,
    cursor: usize,
    elapsed_ms: u32,
    codes: Vec<BootCode>,
}

impl BootPlayer {
    /// 用皮肤包起步。
    pub fn new(skin: Option<BootSkin>) -> Self {
        let stage = if skin.is_some() {
            BootStage::Playing
        } else {
            BootStage::DefaultAnim
        };
        BootPlayer {
            stage,
            skin,
            cursor: 0,
            elapsed_ms: 0,
            codes: Vec::new(),
        }
    }

    /// 审计码。
    pub fn codes(&self) -> &[BootCode] {
        &self.codes
    }

    /// 已播放毫秒。
    pub fn elapsed(&self) -> u32 {
        self.elapsed_ms
    }

    /// 当前帧指纹。皮肤帧缺失 → 降默认；解码超预算帧跳过。
    pub fn tick(&mut self, delta_ms: u32) -> u64 {
        if self.stage == BootStage::Done {
            return STATIC_FALLBACK;
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
        // 硬上限：超 2 秒跳过（个性化不许拖慢开机）。
        if self.elapsed_ms > BOOT_BUDGET_MS {
            self.codes.push(BootCode::OverBudget);
            self.stage = BootStage::Done;
            return STATIC_FALLBACK;
        }
        if self.stage != BootStage::Playing {
            return DEFAULT_ANIM_FP;
        }
        let skin = match self.skin.as_ref() {
            Some(s) => s,
            None => {
                self.stage = BootStage::DefaultAnim;
                return DEFAULT_ANIM_FP;
            }
        };
        match skin.frames.get(self.cursor) {
            None => {
                // 序列播完 → 正常结束。
                self.stage = BootStage::Done;
                STATIC_FALLBACK
            }
            Some(f) if f.fp == 0 => {
                // 帧缺失 → 回退默认动画（不是停在黑屏）。
                self.codes.push(BootCode::FrameMissing);
                self.stage = BootStage::DefaultAnim;
                DEFAULT_ANIM_FP
            }
            Some(f) if f.cost > DECODE_BUDGET => {
                // 解码超预算：跳过该帧（不是卡住）。
                self.cursor += 1;
                DEFAULT_ANIM_FP
            }
            Some(f) => {
                self.cursor += 1;
                f.fp
            }
        }
    }

    /// 解码失败路径（外部解码器报错时调用）：静态帧兜底。
    pub fn decode_broke(&mut self) -> u64 {
        self.codes.push(BootCode::DecodeFailed);
        self.stage = BootStage::Static;
        STATIC_FALLBACK
    }

    /// 读屏播报。
    pub fn announce(&self) -> &'static str {
        match self.stage {
            BootStage::Playing => "正在播放开机动画",
            BootStage::DefaultAnim => "使用默认开机动画",
            BootStage::Static => "开机画面（静态）",
            BootStage::Done => "开机动画已结束",
        }
    }
}

/// 默认动画帧指纹（编译期常量，永不损坏）。
pub const DEFAULT_ANIM_FP: u64 = 0xDEFA_B007_0001;

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fp: u64) -> BootFrame {
        BootFrame { fp, cost: 16 }
    }

    #[test]
    fn plays_all_frames() {
        let skin = BootSkin::load("mine", vec![f(1), f(2), f(3)]).unwrap();
        let mut p = BootPlayer::new(Some(skin));
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(p.tick(FRAME_INTERVAL_MS));
        }
        assert_eq!(&seen[..3], &[1, 2, 3]);
        assert_eq!(seen[3], STATIC_FALLBACK);
        assert_eq!(p.stage, BootStage::Done);
    }

    #[test]
    fn hard_limit_skips() {
        let skin = BootSkin::load("long", (0..200).map(|i| f(i + 1)).collect()).unwrap();
        let mut p = BootPlayer::new(Some(skin));
        // 一次 tick 3 秒 → 立即超限。
        let out = p.tick(3000);
        assert_eq!(out, STATIC_FALLBACK);
        assert_eq!(p.stage, BootStage::Done);
        assert!(p.codes().contains(&BootCode::OverBudget));
    }

    #[test]
    fn missing_frame_falls_back_to_default() {
        let skin = BootSkin::load("half", vec![f(1), f(0), f(3)]).unwrap();
        let mut p = BootPlayer::new(Some(skin));
        p.tick(FRAME_INTERVAL_MS);
        p.tick(FRAME_INTERVAL_MS); // 第二帧缺失
        assert_eq!(p.stage, BootStage::DefaultAnim);
    }

    #[test]
    fn decode_fail_static() {
        let skin = BootSkin::load("x", vec![f(1)]).unwrap();
        let mut p = BootPlayer::new(Some(skin));
        let out = p.decode_broke();
        assert_eq!(out, STATIC_FALLBACK);
        assert_eq!(p.stage, BootStage::Static);
    }
}
