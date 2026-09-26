//! S4 · ushell 桌面真壁纸（AI-4/6 · 三世界同源视觉）
//!
//! **来源契约**：`wallpaper.rgb565` = Windows 侧 Wallpaper Engine 当前
//! 视频壁纸《与你相恋到生命尽头 席娜美美》(workshop 3276921258) 的代表
//! 帧（1280×720，RGB565 小端）——「进内核桌面看到与 Windows 一样的壁纸」
//! 的静态帧实现（视频壁纸动态播放属第 4 批 GPU/解码栈，如实降级）。
//!
//! **装载通道（Limine internal module，与 boot-select.json 同款契约）**：
//! 壁纸不进内核映像——ISO `/boot/wallpaper.rgb565` 经 `MODULE_REQUEST`
//! flags=0 可选装载，缺失/损坏 = `ready()==false`，桌面回退三段色带
//! （绝不阻塞引导，绝不假装有壁纸）。装载后复制进 .bss 常驻缓冲。
//! **为什么不用 include_bytes**：1.8MiB 进 .rodata 使内核映像 RW 段
//! 整体后移，实测引导期 .bss 写落只读页 fatal #PF（2026-09-22 取证，
//! `_attic/t7-crash-serial.log`）；资产走模块通道既绕开映像布局敏感
//! 性，又让壁纸可独立替换（不重编内核）。
//!
//! 构建链：`_attic/t6-extract-frames.py`（视频抽帧 → LANCZOS 缩放 →
//! RGB565）→ `make-iso-qemu.py` 装入 ISO。

use crate::fb::{Color, Surface};

pub const WALL_W: usize = 1280;
pub const WALL_H: usize = 720;
pub const WALL_BYTES: usize = WALL_W * WALL_H * 2;

/// .bss 常驻缓冲（零初始化；装载由 `init_from_module` 完成一次）。
static mut WALL: [u8; WALL_BYTES] = [0; WALL_BYTES];
static mut LOADED: bool = false;

/// 从 Limine 模块装载壁纸（幂等；引导期单线程）。模块缺失/损坏 =
/// false（桌面回退色带，绝不阻塞引导）。
pub fn init_from_module() -> bool {
    unsafe {
        if LOADED {
            return true;
        }
        let Some(bytes) = crate::limine::module_by_path("/wallpaper.rgb565") else {
            crate::kinfo!("wallpaper: module absent - fallback to color bands");
            return false;
        };
        if bytes.len() != WALL_BYTES {
            crate::kwarn!(
                "wallpaper: module size {} != expected {} - fallback",
                bytes.len(),
                WALL_BYTES
            );
            return false;
        }
        // 非零校验（全零 = 损坏：纯黑壁纸不当作有效资产）。
        let nonzero = bytes.iter().take(4096).any(|&b| b != 0);
        if !nonzero {
            crate::kwarn!("wallpaper: module content empty - fallback");
            return false;
        }
        let dst = &raw mut WALL;
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, WALL_BYTES);
        LOADED = true;
        crate::kinfo!("wallpaper: loaded {} bytes from boot module", WALL_BYTES);
        true
    }
}

pub fn ready() -> bool {
    unsafe { LOADED }
}

/// RGB565 → 调色板无关 Color（位扩展到 8bit）。
#[inline]
fn color565(v: u16) -> Color {
    let r = ((v >> 11) & 0x1F) as u8;
    let g = ((v >> 5) & 0x3F) as u8;
    let b = (v & 0x1F) as u8;
    Color::rgb((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
}

#[inline]
fn px(sx: usize, sy: usize) -> Color {
    let off = (sy * WALL_W + sx) * 2;
    unsafe { color565(u16::from_le_bytes([WALL[off], WALL[off + 1]])) }
}

/// 整屏 blit（最近邻缩放到当前帧缓冲）。未装载 = false。
pub fn blit_scaled(surf: &Surface) -> bool {
    if !ready() {
        return false;
    }
    let sw = surf.width() as i64;
    let sh = surf.height() as i64;
    for y in 0..sh {
        let sy = ((y as u64) * WALL_H as u64 / sh.max(1) as u64).min(WALL_H as u64 - 1) as usize;
        for x in 0..sw {
            let sx = ((x as u64) * WALL_W as u64 / sw.max(1) as u64).min(WALL_W as u64 - 1) as usize;
            surf.set_px(x, y, px(sx, sy));
        }
    }
    true
}

/// 采样屏幕坐标处的壁纸色（ushell 削角回填用；少量调用，非热路径）。
/// 返回 0xRRGGBB；未装载 = -1（调用方回退）。
pub fn sample_at(x: i64, y: i64, sw: u32, sh: u32) -> i64 {
    if !ready() {
        return -1;
    }
    let sx = ((x.max(0) as u64) * WALL_W as u64 / sw.max(1) as u64).min(WALL_W as u64 - 1) as usize;
    let sy = ((y.max(0) as u64) * WALL_H as u64 / sh.max(1) as u64).min(WALL_H as u64 - 1) as usize;
    let c = px(sx, sy);
    (((c.r as i64) << 16) | ((c.g as i64) << 8) | (c.b as i64)) & 0xFF_FFFF
}
