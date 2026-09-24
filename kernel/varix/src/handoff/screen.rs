//! 四帧交接画面（篇 2.5）——"全程有画面、全程有进度、全程说实话"。
//!
//! 画面脚本四帧（文案走字符串表；内核字库现为 ASCII 面，中文词条随表
//! 存档、CJK 字形就位后零改动切换——见各词条的 zh 注释）：
//!
//! | 帧 | 状态 | 文案（篇 2.5 原文） | 进度条 |
//! | --- | --- | --- | --- |
//! | 1 | preserving | "正在保存您的工作" | 有——保全子任务完成度 |
//! | 2 | flushing | "正在安全保存到 U 盘" | 有——冲刷各步 |
//! | 3 | arming | "正在准备切换" | **无**——这步要么几秒完成要么报错，装进度条就是撒谎 |
//! | 4 | rebooting | "正在切换到〈目标域〉" | 无——静止画面加呼吸动画 |
//!
//! 错误分支复用三要素模板（标题/说明/下一步）。四帧共用交接画面模板，
//! 与菜单同一套视觉语言（深空背板 + 亮字 + 横向进度条）。
//!
//! 实现位置（篇 2.5）：交接画面运行在合成器之上的独立全屏层——合成器
//! 属 WP-201；m1 期的渲染落点是内核自绘层（与菜单/灰显卡同层），接入
//! 合成器后本模块的绘制函数原样上移。帧时序日志是调试期的"看得见的
//! 状态机"：每一帧的进入/退出时刻入账，与 WD-040 五步时序账对账。

use super::legacy::{ACCENT, INK_DIM, INK_SUB, INK_TITLE};
use super::flush::FiveStepLedger;
use crate::fb::{Color, Surface};

/// 交接的目标域（第四帧文案里的〈目标域〉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetDomain {
    Windows,
    Varix,
}

impl TargetDomain {
    /// 文案用目标域名（ASCII 面）。
    pub fn as_str(self) -> &'static str {
        match self {
            TargetDomain::Windows => "WINDOWS",
            TargetDomain::Varix => "VARIX",
        }
    }
}

/// 四帧标识（序号即篇 2.5 的帧号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameId {
    /// 帧 1：正在保存您的工作（保全子任务完成度）。
    Preserving,
    /// 帧 2：正在安全保存到 U 盘（冲刷各步）。
    Flushing,
    /// 帧 3：正在准备切换（无进度条——装进度条就是撒谎）。
    Arming,
    /// 帧 4：正在切换到〈目标域〉（静止 + 呼吸动画）。
    Rebooting,
}

impl FrameId {
    pub fn seq(self) -> u8 {
        match self {
            FrameId::Preserving => 1,
            FrameId::Flushing => 2,
            FrameId::Arming => 3,
            FrameId::Rebooting => 4,
        }
    }
}

/// 字符串表词条。`render` 是当前字库（ASCII 面）实际绘制的行；`zh` 是
/// 篇 2.5 的中文原文（随表存档——换 CJK 字形时把 render 换成 zh 即可）。
pub struct FrameCopy {
    pub frame: FrameId,
    /// 实际渲染标题（ASCII）。
    pub render: &'static str,
    /// 篇 2.5 中文文案（存档）。
    pub zh: &'static str,
    /// 这帧有没有进度条（帧 3 = false：装进度条就是撒谎）。
    pub has_progress: bool,
}

/// 字符串表（文案的唯一来源——绘制函数只认表，不藏字符串）。
pub const FRAME_TABLE: [FrameCopy; 4] = [
    FrameCopy {
        frame: FrameId::Preserving,
        render: "SAVING YOUR WORK",
        zh: "正在保存您的工作",
        has_progress: true,
    },
    FrameCopy {
        frame: FrameId::Flushing,
        render: "WRITING SAFELY TO USB - DO NOT POWER OFF",
        zh: "正在安全保存到 U 盘",
        has_progress: true,
    },
    FrameCopy {
        frame: FrameId::Arming,
        render: "PREPARING TO SWITCH",
        zh: "正在准备切换",
        has_progress: false,
    },
    FrameCopy {
        frame: FrameId::Rebooting,
        render: "SWITCHING TO <DOMAIN>",
        zh: "正在切换到〈目标域〉",
        has_progress: false,
    },
];

/// 取词条（表里没有的帧是协议缺陷——直接 panic 让测试期就炸出来）。
pub fn copy_for(frame: FrameId) -> &'static FrameCopy {
    FRAME_TABLE.iter().find(|c| c.frame == frame).unwrap()
}

/// 呼吸动画的透明度包络：周期 `period_ms` 的三角波（0→峰→0），
/// 纯函数、测试可断言。呼吸周期 2s，峰 255。
pub fn breath_alpha(phase_ms: u64, period_ms: u64) -> u8 {
    let period = period_ms.max(1);
    let t = phase_ms % period;
    let half = period / 2;
    let a = if t <= half {
        (t * 255) / half.max(1)
    } else {
        ((period - t) * 255) / (period - half).max(1)
    };
    a.min(255) as u8
}

/// 按呼吸相位调暗的强调色（帧 4 的呼吸光带）。
fn breath_color(phase_ms: u64) -> Color {
    let a = breath_alpha(phase_ms, 2_000) as u32;
    // ACCENT 与背板之间的插值：alpha 越低越接近背板深色。
    let mix = |c: u8, base: u8| -> u8 { (base as u32 * (255 - a) / 255 + c as u32 * a / 255) as u8 };
    Color::rgb(mix(0x53, 0x14), mix(0xB1, 0x1C), mix(0xFF, 0x30))
}

/// 画一帧。`pct` 是 0..=100 的进度（`has_progress=false` 的帧忽略）；
/// `breath_phase_ms` 驱动帧 4 的呼吸。返回版面底边 y（越界断言用）。
pub fn draw_frame(
    surf: &Surface,
    frame: FrameId,
    target: TargetDomain,
    pct: Option<u32>,
    breath_phase_ms: u64,
) -> i64 {
    let w = surf.width() as i64;
    let h = surf.height() as i64;
    crate::banner::paint_backdrop(surf);
    let copy = copy_for(frame);

    // 顶部小字：交接协议标识。
    let tag = "VARIX HANDOFF";
    let tw = crate::font::text_width_scaled(tag, 1);
    crate::font::draw_text_scaled(surf, (w - tw) / 2, h / 2 - 120, tag, INK_DIM, 1);

    // 主标题（帧 4 的〈目标域〉落在文案里）。
    let title = if frame == FrameId::Rebooting {
        // "SWITCHING TO <DOMAIN>" → "SWITCHING TO WINDOWS"
        alloc::format!("SWITCHING TO {}", target.as_str())
    } else {
        alloc::format!("{}", copy.render)
    };
    let tiw = crate::font::text_width_scaled(&title, 3);
    let ty = h / 2 - 72;
    crate::font::draw_text_scaled(surf, (w - tiw) / 2, ty, &title, INK_TITLE, 3);

    // 副标题：进度语义的"说实话"行。
    let sub = match frame {
        FrameId::Preserving => "window list, drafts and clipboard",
        FrameId::Flushing => "file system journal commit in progress",
        FrameId::Arming => "gate check, arming, verifying",
        FrameId::Rebooting => "the firmware takes over in a moment",
    };
    let sw = crate::font::text_width_scaled(sub, 2);
    crate::font::draw_text_scaled(surf, (w - sw) / 2, ty + 52, sub, ACCENT, 2);

    // 进度条：只有声明的帧才有——帧 3 装进度条就是撒谎。
    if copy.has_progress {
        let inset = w / 6;
        let bar_w = w - inset * 2 - 56;
        crate::progress::draw(surf, inset, ty + 116, bar_w, pct.unwrap_or(0) as usize, 100);
    } else if frame == FrameId::Rebooting {
        // 帧 4：呼吸光带（静止画面 + 呼吸动画）。
        let band_w = w / 3;
        let band_h = 8i64;
        surf.fill_rect((w - band_w) / 2, ty + 128, band_w, band_h, breath_color(breath_phase_ms));
    }

    // 底部说明：把当前步骤讲明白（帧 2 明示断电红线）。
    let foot = match frame {
        FrameId::Preserving => "capturing session state to the handoff partition",
        FrameId::Flushing => "this step cannot be cancelled",
        FrameId::Arming => "a few seconds - or an honest error",
        FrameId::Rebooting => "do not remove the usb drive",
    };
    let fw = crate::font::text_width_scaled(foot, 1);
    crate::font::draw_text_scaled(surf, (w - fw) / 2, ty + 176, foot, INK_SUB, 1);

    ty + 196
}

/// 错误分支：三要素模板（标题 / 说明 / 下一步指引）——与 refuse_boot
/// 的三要素同构（MD1 第 27.1 节），复用交接画面模板。
pub fn draw_error(surf: &Surface, title: &str, detail: &str, action: &str) -> i64 {
    let w = surf.width() as i64;
    let h = surf.height() as i64;
    crate::banner::paint_backdrop(surf);
    let ty = h / 2 - 72;

    let tw = crate::font::text_width_scaled(title, 3);
    crate::font::draw_text_scaled(surf, (w - tw) / 2, ty, title, INK_TITLE, 3);

    let dw = crate::font::text_width_scaled(detail, 2);
    crate::font::draw_text_scaled(surf, (w - dw) / 2, ty + 52, detail, ACCENT, 2);

    let aw = crate::font::text_width_scaled(action, 1);
    crate::font::draw_text_scaled(surf, (w - aw) / 2, ty + 128, action, INK_SUB, 1);

    ty + 148
}

// ---------------------------------------------------------------------------
// 帧时序日志（调试期把每一帧的时序打到日志——看得见的状态机）
// ---------------------------------------------------------------------------

/// 一帧的时序记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameEvent {
    pub frame: FrameId,
    pub enter_ms: u64,
    pub leave_ms: u64,
}

impl FrameEvent {
    pub fn duration_ms(&self) -> u64 {
        self.leave_ms.saturating_sub(self.enter_ms)
    }
}

/// 帧日志：会话期间逐帧入账（顺序、衔接、总账）。
#[derive(Debug, Default)]
pub struct FrameLog {
    events: alloc::vec::Vec<FrameEvent>,
}

impl FrameLog {
    pub fn new() -> FrameLog {
        FrameLog { events: alloc::vec::Vec::new() }
    }

    /// 记一帧（enter ≤ leave；乱序由 [`Self::is_consistent`] 拦）。
    pub fn record(&mut self, frame: FrameId, enter_ms: u64, leave_ms: u64) {
        self.events.push(FrameEvent { frame, enter_ms, leave_ms });
    }

    pub fn events(&self) -> &[FrameEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 一致性：恰好四帧、帧号递增、时间衔接无缝（上一帧 leave == 下一帧
    /// enter）、每帧时长非负。
    pub fn is_consistent(&self) -> bool {
        if self.events.len() != 4 {
            return false;
        }
        for (i, e) in self.events.iter().enumerate() {
            if e.frame.seq() != (i + 1) as u8 {
                return false;
            }
            if e.leave_ms < e.enter_ms {
                return false;
            }
            if i > 0 && e.enter_ms != self.events[i - 1].leave_ms {
                return false;
            }
        }
        true
    }

    /// 与 WD-040 五步时序账对账：四帧覆盖五步（帧 3 覆盖闸门+写变量
    /// 两段——arming 状态就是这两段的画面），总账逐毫秒相等。
    pub fn covers_ledger(&self, ledger: &FiveStepLedger) -> bool {
        if !self.is_consistent() {
            return false;
        }
        self.events.last().unwrap().leave_ms == ledger.total_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::PixelFormat;
    use std::vec::Vec;

    fn surface(w: u32, h: u32) -> (Surface, Vec<u8>) {
        let mut v: Vec<u8> = std::vec![0u8; (w * h * 4) as usize];
        let s = unsafe { Surface::from_raw(v.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32) };
        (s, v)
    }

    fn count_px(s: &Surface, c: Color) -> u64 {
        let mut n = 0u64;
        for y in 0..s.height() as i64 {
            for x in 0..s.width() as i64 {
                if s.get_px(x, y) == Some(PixelFormat::Bgr32.pack(c)) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn b205_string_table_is_the_single_copy_source() {
        // 四帧词条齐、帧号连续、文案与篇 2.5 原文对表（zh 列）。
        assert_eq!(FRAME_TABLE.len(), 4);
        for (i, c) in FRAME_TABLE.iter().enumerate() {
            assert_eq!(c.frame.seq(), (i + 1) as u8);
            assert!(!c.render.is_empty(), "每帧都要有实际渲染文案");
        }
        assert_eq!(FRAME_TABLE[0].zh, "正在保存您的工作");
        assert_eq!(FRAME_TABLE[1].zh, "正在安全保存到 U 盘");
        assert_eq!(FRAME_TABLE[2].zh, "正在准备切换");
        assert!(FRAME_TABLE[3].zh.contains("正在切换到"));
        // 进度条语义：帧 1/2 有，帧 3/4 无——帧 3 装进度条就是撒谎。
        assert!(FRAME_TABLE[0].has_progress && FRAME_TABLE[1].has_progress);
        assert!(!FRAME_TABLE[2].has_progress && !FRAME_TABLE[3].has_progress);
    }

    #[test]
    fn b205_all_four_frames_render() {
        for frame in [FrameId::Preserving, FrameId::Flushing, FrameId::Arming, FrameId::Rebooting] {
            let (s, _b) = surface(1280, 800);
            let bottom = draw_frame(&s, frame, TargetDomain::Windows, Some(50), 500);
            assert!(bottom > 0 && bottom < 800, "{:?} 版面越界: {}", frame, bottom);
            // 每帧都有主标题亮字（全程有画面）。
            assert!(count_px(&s, INK_TITLE) > 100, "{:?} 主标题必须可见", frame);
        }
    }

    #[test]
    fn b205_frame3_has_no_progress_bar() {
        // 帧 3（arming）：无论传什么进度值，画面上不得出现进度条。
        let (s, _b) = surface(1280, 800);
        draw_frame(&s, FrameId::Arming, TargetDomain::Windows, Some(99), 0);
        assert_eq!(count_px(&s, crate::progress::FILL), 0, "帧 3 装进度条就是撒谎");
        assert_eq!(count_px(&s, crate::progress::TRACK), 0);
        // 对照：帧 2（flushing）同进度有进度条。
        let (s2, _b2) = surface(1280, 800);
        draw_frame(&s2, FrameId::Flushing, TargetDomain::Windows, Some(50), 0);
        assert!(count_px(&s2, crate::progress::FILL) > 500, "帧 2 必须有进度条");
    }

    #[test]
    fn b205_frame4_breathes_and_names_the_domain() {
        // 呼吸动画：不同相位的呼吸光带像素不同（静止画面 + 呼吸）。
        let (a, _ba) = surface(1280, 800);
        let (b, _bb) = surface(1280, 800);
        draw_frame(&a, FrameId::Rebooting, TargetDomain::Windows, None, 0);
        draw_frame(&b, FrameId::Rebooting, TargetDomain::Windows, None, 1_000);
        let pa = breath_color(0);
        let pb = breath_color(1_000);
        assert_ne!(pa, pb, "呼吸相位必须改变光带颜色");
        assert_ne!(count_px(&a, pa), count_px(&b, pb), "两个相位的像素分布必须不同");
        // 呼吸包络是周期三角波：峰谷对称、周期回归。
        assert_eq!(breath_alpha(0, 2_000), 0);
        assert_eq!(breath_alpha(1_000, 2_000), 255);
        assert_eq!(breath_alpha(2_000, 2_000), 0);
        assert_eq!(breath_alpha(500, 2_000), 127);
        // 目标域文案：〈目标域〉落在帧 4（用像素差证明文案不同）。
        let (c, _bc) = surface(1280, 800);
        draw_frame(&c, FrameId::Rebooting, TargetDomain::Varix, None, 500);
        assert_ne!(count_px(&a, INK_TITLE), count_px(&c, INK_TITLE), "目标域不同，标题像素分布不同");
    }

    #[test]
    fn b205_error_branch_uses_three_part_template() {
        // 错误分支：标题/说明/下一步三要素齐（与 refuse_boot 同构）。
        let (s, _b) = surface(1280, 800);
        let bottom = draw_error(
            &s,
            "HANDOFF ABORTED",
            "arming failed - nothing happened",
            "the system continues normally - see handoff diagnostics",
        );
        assert!(bottom > 0 && bottom < 800);
        assert!(count_px(&s, INK_TITLE) > 100, "标题必须可见");
        assert!(count_px(&s, ACCENT) > 100, "说明必须可见");
        assert!(count_px(&s, INK_SUB) > 50, "下一步指引必须可见");
    }

    #[test]
    fn b205_frame_log_covers_wd040_ledger() {
        // 四帧覆盖五步：帧1=保全、帧2=冲刷、帧3=闸门+写变量、帧4=重启。
        let mut log = FrameLog::new();
        log.record(FrameId::Preserving, 0, 2_000);
        log.record(FrameId::Flushing, 2_000, 6_000);
        log.record(FrameId::Arming, 6_000, 9_800);
        log.record(FrameId::Rebooting, 9_800, 12_300);
        assert!(log.is_consistent());
        let ledger = FiveStepLedger {
            preserve_ms: 2_000,
            flush_ms: 4_000,
            gate_ms: 300,
            arm_ms: 3_500,
            reboot_ms: 2_500,
        };
        assert!(log.covers_ledger(&ledger), "四帧总账必须与五步时序账逐毫秒对上");
        assert_eq!(log.events()[2].duration_ms(), 3_800, "帧 3 时长=闸门+写变量");
        // 缺帧/乱序/断档都要被拦。
        let mut broken = FrameLog::new();
        broken.record(FrameId::Preserving, 0, 2_000);
        broken.record(FrameId::Flushing, 2_000, 6_000);
        assert!(!broken.is_consistent());
        let mut gap = FrameLog::new();
        gap.record(FrameId::Preserving, 0, 2_000);
        gap.record(FrameId::Flushing, 2_100, 6_000);
        assert!(!gap.is_consistent(), "时间断档必须被拦");
    }

    #[test]
    fn b205_layout_fits_common_screens() {
        for (w, h) in [(640u32, 480u32), (1024, 768), (1280, 800), (1920, 1080), (2560, 1440)] {
            for frame in [FrameId::Preserving, FrameId::Flushing, FrameId::Arming, FrameId::Rebooting] {
                let (s, _b) = surface(w, h);
                let bottom = draw_frame(&s, frame, TargetDomain::Windows, Some(100), 800);
                assert!(bottom < h as i64, "{}x{} {:?} 版面越界", w, h, frame);
            }
        }
    }
}
