//! VE-F3526 · 声音人格化 —— 音效集替换 + 完整性校验 + 音量归一（音频三律延续）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3526`
//!
//! # 职责（锚点原文拆解）
//!
//! - **音效集替换**（[`SfxPack`]）：皮肤包提供系统音效集，
//!   按事件号映射到自定义音（覆盖表可查）。
//! - **完整性校验**（[`SfxPack::validate`]）：缺哪个事件的声音
//!   自动回退默认——校验产出缺口清单，替换时缺口事件用默认音，
//!   不静音、不报错（系统音效必须始终有声音）。
//! - **音量归一**（[`GainGuard`]）：个性化音效不炸耳——每条音的
//!   增益钳制到安全上限（[`GAIN_CAP_PERMILLE`]），音频三律延续。
//! - **覆盖表可查**（[`SfxBook::mapping`]）：哪个事件用哪个音
//!   一张表读全（读屏可达）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 音效缺失 | 回退默认 |
//! | 音量超标 | 钳制 |
//! | 解码失败 | 默认 |
//!
//! # 复杂度
//!
//! 全部 O(事件数)（事件数固定小集合）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5950 起）
// ---------------------------------------------------------------------------

/// 声音人格化域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SfxCode {
    /// 音效集标识非法。
    BadPackId = 5950,
    /// 增益超上限（已钳制）。
    GainClamped = 5951,
    /// 音效解码失败（回退默认）。
    DecodeFailed = 5952,
}

impl SfxCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SfxCode::BadPackId => "音效集标识非法",
            SfxCode::GainClamped => "增益超限已钳制",
            SfxCode::DecodeFailed => "音效解码失败",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、事件与增益
// ---------------------------------------------------------------------------

/// 增益安全上限（千分比，1000 = 0 dB）。
pub const GAIN_CAP_PERMILLE: u32 = 1400;

/// 系统音效事件（固定集合）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SfxEvent {
    /// 提示音。
    Notify,
    /// 警告音。
    Warn,
    /// 出错音。
    Error,
    /// 开机音。
    Startup,
    /// 清空回收站。
    Trash,
}

impl SfxEvent {
    /// 全集（固定顺序，覆盖表按此序输出）。
    pub const ALL: [SfxEvent; 5] = [
        SfxEvent::Notify,
        SfxEvent::Warn,
        SfxEvent::Error,
        SfxEvent::Startup,
        SfxEvent::Trash,
    ];

    /// 中文名（读屏可达）。
    pub fn label(self) -> &'static str {
        match self {
            SfxEvent::Notify => "提示音",
            SfxEvent::Warn => "警告音",
            SfxEvent::Error => "出错音",
            SfxEvent::Startup => "开机音",
            SfxEvent::Trash => "回收站",
        }
    }
}

/// 音效片段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SfxClip {
    /// 内容指纹（0 = 无效）。
    pub fp: u64,
    /// 增益（千分比）。
    pub gain: u32,
}

impl SfxClip {
    /// 构造（增益钳制）。
    pub fn build(fp: u64, gain: u32) -> (Self, bool) {
        let clamped = gain > GAIN_CAP_PERMILLE;
        (SfxClip { fp, gain: gain.min(GAIN_CAP_PERMILLE) }, clamped)
    }
}

// ---------------------------------------------------------------------------
// 三、音效集
// ---------------------------------------------------------------------------

/// 音效集：事件 → 片段（缺口 = 未覆盖）。
#[derive(Clone, Debug)]
pub struct SfxPack {
    /// 皮肤包标识。
    pub id: &'static str,
    pub clips: Vec<(SfxEvent, SfxClip)>,
}

impl SfxPack {
    /// 装载校验。
    pub fn load(id: &'static str, clips: Vec<(SfxEvent, SfxClip)>) -> Result<Self, SfxCode> {
        if id.is_empty() {
            return Err(SfxCode::BadPackId);
        }
        Ok(SfxPack { id, clips })
    }

    /// 查事件片段。
    pub fn get(&self, ev: SfxEvent) -> Option<&SfxClip> {
        self.clips.iter().find(|(e, _)| *e == ev).map(|(_, c)| c)
    }

    /// 完整性校验：返回未覆盖事件清单（自动回退默认的依据）。
    pub fn gaps(&self) -> Vec<SfxEvent> {
        SfxEvent::ALL
            .iter()
            .copied()
            .filter(|e| self.get(*e).map(|c| c.fp == 0).unwrap_or(true))
            .collect()
    }

    /// 无效片段数（指纹为 0 但在表内）。
    pub fn broken(&self) -> usize {
        self.clips.iter().filter(|(_, c)| c.fp == 0).count()
    }
}

// ---------------------------------------------------------------------------
// 四、音效账
// ---------------------------------------------------------------------------

/// 生效映射：每个事件的最终音（自定义或默认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectiveClip {
    /// 事件。
    pub ev: SfxEvent,
    /// 片段指纹（默认音为 DEFAULT_SFX_FP）。
    pub fp: u64,
    /// 生效增益（已钳制）。
    pub gain: u32,
    /// 是否走了默认回退。
    pub fallback: bool,
}

/// 默认音指纹（编译期常量）。
pub const DEFAULT_SFX_FP: u64 = 0xDEFA_5F00_0001;

/// 音效账：替换 + 缺口回退 + 覆盖表。
pub struct SfxBook {
    pub pack: Option<SfxPack>,
    pub codes: Vec<SfxCode>,
}

impl SfxBook {
    /// 无包起步（全部默认）。
    pub fn new() -> Self {
        SfxBook { pack: None, codes: Vec::new() }
    }

    /// 装包（校验 + 增益钳制审计）。
    pub fn mount(&mut self, pack: SfxPack) -> Result<(), SfxCode> {
        let mut bad = false;
        for (_, c) in &pack.clips {
            let (cc, clamped) = SfxClip::build(c.fp, c.gain);
            if clamped {
                self.codes.push(SfxCode::GainClamped);
            }
            let _ = cc;
            bad = bad || c.fp == 0;
        }
        if pack.id.is_empty() {
            return Err(SfxCode::BadPackId);
        }
        if bad {
            self.codes.push(SfxCode::DecodeFailed);
        }
        self.pack = Some(pack);
        Ok(())
    }

    /// 覆盖表（全事件生效映射，可查/读屏）。
    pub fn mapping(&self) -> Vec<EffectiveClip> {
        SfxEvent::ALL
            .iter()
            .copied()
            .map(|ev| {
                let c = self
                    .pack
                    .as_ref()
                    .and_then(|p| p.get(ev))
                    .filter(|c| c.fp != 0);
                match c {
                    Some(c) => EffectiveClip {
                        ev,
                        fp: c.fp,
                        gain: c.gain.min(GAIN_CAP_PERMILLE),
                        fallback: false,
                    },
                    None => EffectiveClip {
                        ev,
                        fp: DEFAULT_SFX_FP,
                        gain: 1000,
                        fallback: true,
                    },
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_fall_back_to_default() {
        let mut b = SfxBook::new();
        let pack = SfxPack::load(
            "mine",
            vec![
                (SfxEvent::Notify, SfxClip { fp: 0xAA, gain: 1000 }),
                (SfxEvent::Warn, SfxClip { fp: 0, gain: 1000 }), // 无效
            ],
        )
        .unwrap();
        b.mount(pack).unwrap();
        let m = b.mapping();
        let notify = m.iter().find(|c| c.ev == SfxEvent::Notify).unwrap();
        assert_eq!(notify.fp, 0xAA);
        assert!(!notify.fallback);
        let trash = m.iter().find(|c| c.ev == SfxEvent::Trash).unwrap();
        assert_eq!(trash.fp, DEFAULT_SFX_FP);
        assert!(trash.fallback);
    }

    #[test]
    fn gain_clamped() {
        let (c, clamped) = SfxClip::build(0xBB, 9999);
        assert!(clamped);
        assert_eq!(c.gain, GAIN_CAP_PERMILLE);
    }

    #[test]
    fn bad_id_rejected() {
        assert_eq!(
            SfxPack::load("", vec![]),
            Err(SfxCode::BadPackId)
        );
    }
}
