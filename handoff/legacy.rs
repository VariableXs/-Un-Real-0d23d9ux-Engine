//! 需求 2 · 内核→Variable 交接（A 卡 VARIX + VARIABLE 的落点）。
//!
//! # 为什么不是"内核里直接跑 Tauri"
//!
//! Variable 是 Tauri 应用：Rust 宿主 + **WebView2** + 一整套 Windows API。
//! 内核（`x86_64-unknown-none`，无 Windows API、无 WebView2、无字体/网络栈）
//! 里跑不起来，这不是工作量问题而是结构性不可能。
//!
//! 而 UEFI 架构决定了另一条硬约束：内核在早期就调了 `ExitBootServices`，
//! 引导服务已交还固件，**无法再跳转到 Windows Boot Manager**——唯一的路是
//! `ResetSystem` 重走固件引导。所以"内核 → Variable"物理上必然包含一次复位。
//!
//! # 交接链
//!
//! ```text
//! 三卡菜单选 A 卡 → 内核加载（HUD 进度条走满）→ 本模块的交接画面
//!   → 写 UEFI BootNext = <Windows 引导项>（一次性，固件消费后自动清除）
//!   → ResetSystem → 固件引导 Windows → Windows 自启 Variable 全屏
//! ```
//!
//! 从用户视角就是：开机选 VARIX + VARIABLE → 看到 VARIX 加载 → 进 Variable 桌面。
//!
//! # 绝不假装
//!
//! 交接任一环失败（固件无 Runtime Services / BootNext 写不进 / ResetSystem
//! 不可用）一律**如实降级到内核自绘 ushell** 并打 WARN——宁可让用户看到
//! 一个能用的 ushell，也不留一块黑屏或死循环。

use crate::fb::{Color, Surface};

/// A 卡路径的终点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffPlan {
    /// 交接给 Windows：由那边的 Variable 自启全屏（需求 2 的落点）。
    ToWindows,
    /// 落内核自绘 ushell（交接被配置关掉时的保留路径）。
    ToKernelShell,
}

/// 决策纯函数（宿主可测）。
///
/// - `chosen` = 三卡菜单的选中项 id；`None` 表示走 A 卡（含"未显示菜单、
///   默认项就是 varix"的情形）。
/// - `handoff_enabled` = 配置开关（`boot-select.json` 的 `handoff` /
///   cmdline `handoff=1`），默认开。
///
/// `Some("windows")` / `Some("uefi")` 由 `main.rs` 里既有的
/// `boot_next_action` 早一步处理，不会落到本函数；真落到也如实兜底成
/// shell 而不是二次交接（避免一次引导里写两遍 BootNext）。
pub fn plan(chosen: Option<&str>, handoff_enabled: bool) -> HandoffPlan {
    match chosen {
        Some("varix") | None => {
            if handoff_enabled {
                HandoffPlan::ToWindows
            } else {
                HandoffPlan::ToKernelShell
            }
        }
        _ => HandoffPlan::ToKernelShell,
    }
}

// ---------------------------------------------------------------------------
// 交接画面（与三卡菜单同一套视觉语言：深空背板 + 亮字 + 横向进度条）
// ---------------------------------------------------------------------------

pub(crate) const INK_TITLE: Color = Color::rgb(0xF2, 0xF5, 0xFA);
pub(crate) const INK_SUB: Color = Color::rgb(0x9A, 0xA6, 0xB8);
pub(crate) const INK_DIM: Color = Color::rgb(0x6B, 0x74, 0x86);
pub(crate) const ACCENT: Color = Color::rgb(0x53, 0xB1, 0xFF);

/// 交接画面的停留时长（毫秒）。给用户看清"VARIX 已加载完、正在进入
/// Variable"，而不是一黑屏就跳走——一帧不留会让人以为机器挂了。
pub const PROMPT_MS: u64 = 2400;
/// 画面刷新片数（每片重画一次进度条）。
pub const PROMPT_SLICES: u32 = 60;

/// 画交接画面。`pct` 为 0..=100 的进度。
///
/// 版式：居中 wordmark + 副标题 + 进度条 + 底部说明行。返回底边 y。
pub fn draw_prompt(surf: &Surface, pct: u32) -> i64 {
    let w = surf.width() as i64;
    let h = surf.height() as i64;
    crate::banner::paint_backdrop(surf);

    // 主标题：与菜单标题同级（scale 3 比菜单的 2 更醒目，交接是终点事件）
    let title = "VARIX";
    let tw = crate::font::text_width_scaled(title, 3);
    let ty = h / 2 - 96;
    crate::font::draw_text_scaled(surf, (w - tw) / 2, ty, title, INK_TITLE, 3);

    // 副标题
    let sub = "ENTERING VARIABLE SYSTEM";
    let sw = crate::font::text_width_scaled(sub, 2);
    crate::font::draw_text_scaled(surf, (w - sw) / 2, ty + 52, sub, ACCENT, 2);

    // 进度条（复用 progress 单一来源：轨道 + 填充 + 光泽 + 右侧百分比）
    let inset = w / 6;
    let bar_w = w - inset * 2 - 56; // 右侧留给百分比读数
    crate::progress::draw(surf, inset, ty + 116, bar_w, pct as usize, 100);

    // 底部说明：把"为什么中间会重启一次"讲明白，不让用户以为机器出问题
    let foot1 = "HANDING OFF TO THE WINDOWS SESSION";
    let f1w = crate::font::text_width_scaled(foot1, 1);
    crate::font::draw_text_scaled(surf, (w - f1w) / 2, ty + 196, foot1, INK_SUB, 1);
    let foot2 = "the kernel cannot host the Tauri desktop - the session resumes there";
    let f2w = crate::font::text_width_scaled(foot2, 1);
    crate::font::draw_text_scaled(surf, (w - f2w) / 2, ty + 216, foot2, INK_DIM, 1);

    ty + 236
}

// ---------------------------------------------------------------------------
// 目标态：跑完画面并真正交接
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
mod target {
    use super::{draw_prompt, PROMPT_MS, PROMPT_SLICES};
    use crate::fb::Surface;

    /// 走完交接画面（不返回除非被要求停）。
    fn run_prompt(surf: &Surface) {
        // 交接画面是独占屏幕的终局画面：先关 console 镜像，否则内核日志的
        // 字符格滚动会把画面扫花（与 ushell 接管屏幕前同一处理，幂等）。
        crate::console::disable_mirror();
        let tsc_hz = crate::platform::info()
            .map(|p| p.tsc_hz)
            .unwrap_or(crate::platform::FALLBACK_TSC_HZ);
        let slice_ticks = (tsc_hz / 1000) * (PROMPT_MS / PROMPT_SLICES as u64);
        for i in 0..=PROMPT_SLICES {
            let pct = (i * 100) / PROMPT_SLICES;
            draw_prompt(surf, pct);
            let start = crate::timeline::read_tsc();
            // 引导期中断尚未参与调度（sched 域在交接点之前已上线，但这里
            // 只做限时绘制，不依赖时钟中断），TSC 忙等即正确。
            while crate::timeline::read_tsc().wrapping_sub(start) < slice_ticks {
                core::hint::spin_loop();
            }
        }
    }

    /// 交接给 Windows 上的 Variable。**成功即永不返回**；返回 `false`
    /// 表示交接不可用，调用方应如实降级。
    ///
    /// 全程复用既有 `bootnext` 域（项号按固件 BootOrder 逐个读 Boot####
    /// 匹配 Windows，不写死）——与三卡菜单选 WINDOWS 走的是同一条通道，
    /// 区别只在触发时机（这里是内核加载完之后）与语义（那边是"进 Windows"，
    /// 这里是"进 Windows 上的 Variable"）。`target=usb` 时只认设备路径
    /// 含 `usb_windows_esp_guid` 的项（S1.3 登记制）——GUID 缺失/解析失败
    /// 时如实拒绝落 ushell，绝不蒙一个内置盘项。
    pub fn run(
        surf: &Surface,
        target: crate::bootopt::HandoffTarget,
        usb_guid: Option<[u8; 16]>,
    ) -> bool {
        crate::kinfo!("handoff: A-card selected — entering the Variable handoff path");

        // 先把「能不能交接」问清楚，再画画面——确认不了就直接落 ushell，
        // 不让用户白看一场"正在进入 Variable"。
        let _ = crate::bootnext::prepare_runtime_identity_map();
        let blocks = crate::bootnext::identity_map_low_4gib();
        crate::kinfo!("handoff: low-memory identity-mapped ({} x 2MiB)", blocks);

        let guid_ref = match target {
            crate::bootopt::HandoffTarget::Usb => match usb_guid.as_ref() {
                Some(g) => {
                    crate::kinfo!("handoff: target=usb (ESP GUID registered) — GUID-pinned match");
                    Some(g)
                }
                None => {
                    crate::kwarn!(
                        "handoff: handoff_target=usb but usb_windows_esp_guid missing/unparseable \
                         — refusing to guess; falling back to ushell"
                    );
                    return false;
                }
            },
            crate::bootopt::HandoffTarget::Internal => None,
        };
        let entry = crate::bootnext::resolve_windows_entry_for(
            crate::cmdline::init().source(),
            guid_ref,
        );
        if !entry.verified() {
            // **防自锁闸门**：交接是自动动作、无人值守——项号没在固件
            // BootOrder 里得到证实就盲写 BootNext，一旦那个项无效，固件会
            // 回退默认引导顺序；如果 U 盘排在前面就再次进菜单、再次交接，
            // 变成**无限复位循环**。宁可不交接（落 ushell），也不赌。
            // 需要强行指定时用 cmdline `boot_next=<num>`（那条路走
            // `WindowsEntry::Resolved`，视为已验证）。
            crate::kwarn!(
                "handoff: no verified Windows boot option in BootOrder — refusing to guess \
                 (would risk a reset loop); falling back to ushell. PIN IT with cmdline \
                 boot_next=<num> if you are sure."
            );
            return false;
        }
        crate::kinfo!(
            "handoff: Windows boot option resolved to 0x{:04X}",
            entry.number()
        );

        run_prompt(surf);

        match crate::bootnext::write_bootnext(entry.number()) {
            crate::bootnext::BootNextOutcome::Written { entry } => {
                crate::kinfo!(
                    "handoff: BootNext=0x{:04X} written & verified — resetting into Windows",
                    entry
                );
                if crate::bootnext::reset_cold() {
                    // ResetSystem 正常不返回；保险停在死循环。
                    loop {
                        core::hint::spin_loop();
                    }
                }
                crate::kwarn!("handoff: firmware reset unavailable — falling back to ushell");
                false
            }
            crate::bootnext::BootNextOutcome::NoRuntimeServices => {
                crate::kwarn!(
                    "handoff: BIOS boot — UEFI BootNext unavailable, falling back to ushell"
                );
                false
            }
            other => {
                crate::kwarn!(
                    "handoff: BootNext failed ({:?}) — falling back to ushell",
                    other
                );
                false
            }
        }
    }
}

#[cfg(target_os = "none")]
pub use target::run;

/// 宿主态：交接是固件层动作，宿主构建里没有这条路径（如实返回 false）。
#[cfg(not(target_os = "none"))]
pub fn run(_surf: &Surface) -> bool {
    false
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

    #[test]
    fn a_card_hands_off_when_enabled() {
        assert_eq!(plan(None, true), HandoffPlan::ToWindows, "无菜单默认项走 A 卡");
        assert_eq!(plan(Some("varix"), true), HandoffPlan::ToWindows);
    }

    #[test]
    fn a_card_keeps_ushell_when_disabled() {
        assert_eq!(plan(None, false), HandoffPlan::ToKernelShell);
        assert_eq!(plan(Some("varix"), false), HandoffPlan::ToKernelShell);
    }

    #[test]
    fn other_entries_never_hand_off() {
        // windows/uefi 由既有 boot_next_action 早处理；落到这里必须如实兜底，
        // 绝不二次写 BootNext（一次引导里写两遍 = 引导行为不可预期）。
        assert_eq!(plan(Some("windows"), true), HandoffPlan::ToKernelShell);
        assert_eq!(plan(Some("uefi"), true), HandoffPlan::ToKernelShell);
        assert_eq!(plan(Some("other"), true), HandoffPlan::ToKernelShell);
    }

    #[test]
    fn prompt_renders_title_and_progress() {
        let (s, _b) = surface(1280, 800);
        let bottom = draw_prompt(&s, 50);
        assert!(bottom > 0 && bottom < 800, "版面不能越界: {bottom}");
        // 进度条填充色、主标题亮字、强调色副标题都必须出现
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
        assert!(count_px(&s, crate::progress::FILL) > 1000, "进度条必须可见");
        assert!(count_px(&s, INK_TITLE) > 100, "主标题必须可见");
        assert!(count_px(&s, ACCENT) > 100, "副标题必须可见");
    }

    #[test]
    fn prompt_progress_is_monotonic_visually() {
        let (a, _ba) = surface(1280, 800);
        let (b, _bb) = surface(1280, 800);
        draw_prompt(&a, 10);
        draw_prompt(&b, 100);
        fn filled(s: &Surface) -> u64 {
            let mut n = 0u64;
            for y in 0..s.height() as i64 {
                for x in 0..s.width() as i64 {
                    let p = s.get_px(x, y);
                    if p == Some(PixelFormat::Bgr32.pack(crate::progress::FILL))
                        || p == Some(PixelFormat::Bgr32.pack(crate::progress::FILL_BRIGHT))
                    {
                        n += 1;
                    }
                }
            }
            n
        }
        assert!(filled(&b) > filled(&a), "100% 的填充必须多于 10%");
    }

    #[test]
    fn prompt_fits_small_and_large_screens() {
        for (w, h) in [(640u32, 480u32), (1024, 768), (1280, 800), (1920, 1080), (2560, 1440)] {
            let (s, _b) = surface(w, h);
            let bottom = draw_prompt(&s, 100);
            assert!(bottom < h as i64, "{}x{} 版面越界 (bottom={})", w, h, bottom);
        }
    }

    /// 交接画面渲染归档（与 bootselect 同一范式）：
    /// `VARIX_RENDER_HANDOFF=1 cargo ktest -- handoff::` 产出 PPM 供目检。
    #[test]
    fn render_archive_prompt() {
        if std::env::var("VARIX_RENDER_HANDOFF").unwrap_or_default() != "1" {
            return;
        }
        let dir = "docs/acceptance/2026-09-20-需求2-内核交接画面";
        std::fs::create_dir_all(dir).unwrap();
        for (w, h) in [(1280u32, 800u32), (1920, 1080)] {
            let (s, buf) = surface(w, h);
            for pct in [25u32, 60, 100] {
                draw_prompt(&s, pct);
                let path = format!("{}/handoff-{}x{}-p{}.ppm", dir, w, h, pct);
                let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                for px in buf.chunks_exact(4) {
                    out.push(px[2]);
                    out.push(px[1]);
                    out.push(px[0]);
                }
                std::fs::write(&path, out).unwrap();
            }
        }
    }
}
