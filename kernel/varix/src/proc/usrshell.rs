//! 任务27/28/55（AI-V）· 内核嵌入层服务（usrshell）。
//!
//! 用户态 UI 进程（`user/ushell`）承载桌面三件套（桌面壳/文件管理器/
//! 设置页）的渲染与交互；本模块是它的三个内核支点（嵌入层四要素中的
//! 「命令垫片 + 帧通路 + 输入泵」，进程承载在 ring3::spawn_shell）：
//!
//! - `SYS_FRAME`(16)：绘制命令面——fill_rect/text/hline/vline/outline/
//!   info。唯一绘制路径 = 显示服务 Surface（任务20 收编归口），零旁路
//!   直写帧缓冲。
//! - `SYS_INPUT`(17)：`shim://input` 16B 事件泵取（inputsvc 订阅者，
//!   任务19 服务化；契约前 3 序号 0=Up 1=Down 2=Enter 不变，任务55 扩展）。
//! - `SYS_SHIM`(18)：命令垫片——KV（任务24 kvsrv，RAM 盘后端）/VFS
//!   列取读（任务18 exFAT SHARED 只读挂载，缺失时如实降级内置演示树）/
//!   `boot://event` 回放（任务28，timeline 快照语义对齐 bootEvents.ts）。
//!
//! 错误三段式（垫片协议 v1）：`>=0 = OK`；`<0 = MAPPED_ERR(-ErrNo)`；
//! 未知命令 = `MISSING(-ENOSYS)`。负值取 `entry::ErrNo` 小值域（与本文件
//! sys_write 同一口径），与 POSIX -38 值域并存但互不混用。
//!
//! 宿主可测面：命令编解码/参数校验/packing 全部纯函数；真绘制与真盘
//! 路径 `target_os = "none"` 编译（与 ps2/inputsvc 同纪律）。

use crate::entry::ErrNo;

// ---------------------------------------------------------------------------
// 稳定号段扩展（ring3::syscall_common 分派；表名登记见 syscall.rs）
// ---------------------------------------------------------------------------

/// SYS_FRAME：绘制命令面。
pub const SYS_FRAME: u32 = 16;
/// SYS_INPUT：输入泵取。
pub const SYS_INPUT: u32 = 17;
/// SYS_SHIM：命令垫片。
pub const SYS_SHIM: u32 = 18;
/// SYS_REBOOT：重启整机（UEFI 复位 + 8042 兜底）。
pub const SYS_REBOOT: u32 = 19;
/// SYS_POWEROFF：关机断电（UEFI ResetSystem(Shutdown) + ACPI S5 阶梯）。
pub const SYS_POWEROFF: u32 = 20;
/// SYS_WIN：窗口面服务（AI-4 · S2.06 渲染通路 + S2.09 多窗合成的
/// 系统调用面；子命令经 a1 低 8 位，编解码纯函数宿主可测）。
pub const SYS_WIN: u32 = 21;

// SYS_WIN 子命令字（a1 低 8 位；a2/a3 为参数 p1/p2）。
pub const WIN_REGISTER: u64 = 1; // p1=owner_pid p2=(w<<32)|h → wid（>0）/负错误
pub const WIN_UNREGISTER: u64 = 2; // p1=wid → 0
pub const WIN_GEO: u64 = 3; // p1=wid p2=win_pack_xy(x,y) → 0
pub const WIN_RAISE: u64 = 4; // p1=wid → 0
pub const WIN_STATE: u64 = 5; // p1=wid p2=0可见/1最小化 → 0
pub const WIN_FOCUS: u64 = 6; // p1=wid（0=清除）→ 0；联动内核级键盘焦点
pub const WIN_SUBMIT: u64 = 7; // p1=wid p2=提交块指针 → 已提交行数
pub const WIN_COMPOSITE: u64 = 8; // → 合成累计 blit 行数
pub const WIN_QUERY: u64 = 9; // p1=wid（0=服务统计）→ 打包 u64

// WIN_SUBMIT 提交块布局（用户内存，小端）：
//   [0..4)   u32 kind     0=整窗 1=脏区
//   [4..8)   u32 count    脏区数（kind=1 时 1..=16；kind=0 恒 0）
//   [8..)    Rect16[count]：i32 x, y; u32 w, h（窗口坐标，16B/条）
//   [..]     像素数据：kind=0 = 整窗按行（w*bpp/行 × h）；kind=1 = 各脏区
//            按声明顺序紧随（每区 w*bpp/行 × h，无跨区对齐填充）。
pub const WIN_SUBMIT_KIND_FULL: u32 = 0;
pub const WIN_SUBMIT_KIND_DIRTY: u32 = 1;
pub const WIN_SUBMIT_HDR: usize = 8;
pub const WIN_SUBMIT_RECT: usize = 16;
/// 单次提交脏区上限（对齐 winsurf::MAX_DIRTY_PER_SUBMIT）。
pub const WIN_SUBMIT_MAX_RECTS: u32 = 16;
/// 行拷贝分段大小（内核栈缓冲上限 4KiB——戒律 >64KB 禁栈的保守取值）。
pub const WIN_ROW_CHUNK: usize = 4096;

// 编译期对齐断言：提交块脏区上限与 winsurf 服务上限漂移即编译失败。
const _: () = assert!(WIN_SUBMIT_MAX_RECTS as usize == crate::winsurf::MAX_DIRTY_PER_SUBMIT);

/// (x,y) 打包进 u64（各 i32，LE——支持负坐标出屏窗口）。
pub const fn win_pack_xy(x: i64, y: i64) -> u64 {
    (((x as i32) as u32) as u64) | ((((y as i32) as u32) as u64) << 32)
}

/// u64 拆 (x,y)（各 i32 有符号还原）。
pub const fn win_unpack_xy(v: u64) -> (i64, i64) {
    let x = (v & 0xFFFF_FFFF) as u32 as i32;
    let y = ((v >> 32) & 0xFFFF_FFFF) as u32 as i32;
    (x as i64, y as i64)
}

/// (w,h) 打包进 u64（各 u32）。
pub const fn win_pack_wh(w: u64, h: u64) -> u64 {
    (w & 0xFFFF_FFFF) | ((h & 0xFFFF_FFFF) << 32)
}

/// u64 拆 (w,h)。
pub const fn win_unpack_wh(v: u64) -> (u32, u32) {
    ((v & 0xFFFF_FFFF) as u32, ((v >> 32) & 0xFFFF_FFFF) as u32)
}

/// 提交块头部合法性校验（纯函数宿主可测）：kind/count 值域与块总长
/// （像素区按窗口几何推得的最小长度由调用方按 kind 二次核对）。
pub const fn win_submit_hdr_ok(kind: u32, count: u32) -> bool {
    match kind {
        WIN_SUBMIT_KIND_FULL => count == 0,
        WIN_SUBMIT_KIND_DIRTY => count >= 1 && count <= WIN_SUBMIT_MAX_RECTS,
        _ => false,
    }
}

const fn einval() -> i64 {
    -(ErrNo::Einval.to_i32() as i64)
}
const fn efault() -> i64 {
    -(ErrNo::Efault.to_i32() as i64)
}
const fn enosys() -> i64 {
    -(ErrNo::Enosys.to_i32() as i64)
}
const fn enospc() -> i64 {
    -(ErrNo::Enospc.to_i32() as i64)
}
const fn eio() -> i64 {
    -(ErrNo::Eio.to_i32() as i64)
}

// ---------------------------------------------------------------------------
// FRAME：绘制命令编解码（纯函数，宿主可测）
// ---------------------------------------------------------------------------

/// 调色板（16 色，索引经 a1 高位传入——3 参 ABI 装不下 RGB 三元组）。
pub const PALETTE: [crate::fb::Color; 16] = [
    crate::fb::Color::rgb(0x10, 0x10, 0x14),   // 0 black
    crate::fb::Color::rgb(0xF2, 0xF2, 0xF2),   // 1 white
    crate::fb::Color::rgb(0x2A, 0x2A, 0x32),   // 2 dark gray
    crate::fb::Color::rgb(0x9A, 0x9A, 0xA2),   // 3 light gray
    crate::fb::Color::rgb(0x38, 0x74, 0xD2),   // 4 accent blue
    crate::fb::Color::rgb(0x3E, 0xA1, 0x4E),   // 5 green
    crate::fb::Color::rgb(0xC4, 0x3B, 0x3B),   // 6 red
    crate::fb::Color::rgb(0xD2, 0xA8, 0x38),   // 7 yellow
    crate::fb::Color::rgb(0x0C, 0x14, 0x30),   // 8 wallpaper deep
    crate::fb::Color::rgb(0x1A, 0x2A, 0x55),   // 9 wallpaper mid
    crate::fb::Color::rgb(0x1C, 0x1C, 0x24),   // 10 taskbar
    crate::fb::Color::rgb(0x24, 0x24, 0x2E),   // 11 menu bg
    crate::fb::Color::rgb(0x44, 0x86, 0xE0),   // 12 highlight
    crate::fb::Color::rgb(0xB6, 0xB6, 0xC0),   // 13 text dim
    crate::fb::Color::rgb(0xD2, 0x74, 0x38),   // 14 orange
    crate::fb::Color::rgb(0x2E, 0xA8, 0xA8),   // 15 cyan
];

/// FRAME 子命令号。
pub const FRAME_FILL_RECT: u64 = 1;
pub const FRAME_TEXT: u64 = 2;
pub const FRAME_HLINE: u64 = 3;
pub const FRAME_VLINE: u64 = 4;
pub const FRAME_OUTLINE: u64 = 5;
pub const FRAME_INFO: u64 = 6;
/// 真壁纸整屏 blit（S4·AI-4/6：三世界同源静态帧，最近邻缩放）。
pub const FRAME_WALLPAPER: u64 = 7;
/// 壁纸像素采样（削角回填）：a2 = x|y 打包，返回 0xRRGGBB。
pub const FRAME_WALLPAPER_PX: u64 = 8;
/// 原色填充（照片色回填）：a2 = xywh 打包，a3 = 0xRRGGBB。
pub const FRAME_FILL_RGB: u64 = 9;
/// 鼠标指针底图备份：a2 = x|y 打包，把 16×16 区域读进内核 shadow
/// （2026-09-22 鼠标流畅性修复：指针局部擦/画替代全屏重绘）。
pub const FRAME_CURSOR_SAVE: u64 = 10;
/// 鼠标指针底图恢复：a2 = x|y 打包，把 shadow 写回（必须与最近一次
/// SAVE 同位置；未保存过/已恢复 = no-op 返回 1）。
pub const FRAME_CURSOR_RESTORE: u64 = 11;

/// 指针底图 shadow 尺寸（包围 9×13 箭头留余量）。
pub const CURSOR_SHADOW_SIDE: usize = 16;

/// 文本长度上限（栈缓冲预算；a1 bit16..24 装载）。
pub const FRAME_TEXT_MAX: usize = 255;

/// a1 打包：op | color<<8 |（text 专用）len<<16 | scale<<28。
#[inline]
pub fn pack_a1(op: u64, color: u64, len: u64, scale: u64) -> u64 {
    op | (color << 8) | (len << 16) | (scale << 28)
}

/// a2 打包/解包：四个 u16 槽（x|y|w|h 或 x0|x1|y）。
#[inline]
pub fn pack_xywh(x: u64, y: u64, w: u64, h: u64) -> u64 {
    (x & 0xFFFF) | ((y & 0xFFFF) << 16) | ((w & 0xFFFF) << 32) | ((h & 0xFFFF) << 48)
}

#[inline]
pub fn unpack_xywh(a2: u64) -> (i64, i64, i64, i64) {
    (
        (a2 & 0xFFFF) as i64,
        ((a2 >> 16) & 0xFFFF) as i64,
        ((a2 >> 32) & 0xFFFF) as i64,
        ((a2 >> 48) & 0xFFFF) as i64,
    )
}

/// 调色板取色（越界 = black，绝不 panic）。
pub fn palette(idx: u64) -> crate::fb::Color {
    PALETTE.get((idx & 0xFF) as usize).copied().unwrap_or(PALETTE[0])
}

/// 校验坐标矩形与屏幕相交性（0 尺寸拒绝；完全越界拒绝——绘制层自身
/// 还会逐像素裁剪，这里把显然无意义的调用挡在语义层）。
pub fn rect_plausible(x: i64, y: i64, w: i64, h: i64, sw: i64, sh: i64) -> bool {
    w > 0 && h > 0 && x < sw && y < sh && x + w > 0 && y + h > 0
}

// ---------------------------------------------------------------------------
// SHIM：命令块布局常量（纯函数，宿主可测）
// ---------------------------------------------------------------------------

pub const SHIM_KV_GET: u64 = 1;
pub const SHIM_KV_SET: u64 = 2;
pub const SHIM_KV_REMOVE: u64 = 3;
pub const SHIM_KV_KEYS: u64 = 4;
pub const SHIM_VFS_LIST: u64 = 5;
pub const SHIM_VFS_READ: u64 = 6;
pub const SHIM_BOOT_EVENTS: u64 = 7;
pub const SHIM_BOOT_MS: u64 = 8;
/// VFS 数据源查询：出参 block[0]=1（exFAT SHARED 真实挂载）/0（内置
/// 演示树）。文件管理器页脚如实标注数据源，绝不冒充。
pub const SHIM_VFS_SOURCE: u64 = 9;
/// Wine 应用拉起（交叉走查第一项入口）：key 槽放应用名（如
/// "notepad-classic"）→ 注册表查表 + 前缀实例化 + 运行时如实状态。
/// 出参：[0]=state u8 [1]=runtime u8 [2..6]=app_id [6..10]=templ_ver
/// [10..12]=msg_len [12..]=消息（winelaunch::encode_status 布局）。
pub const SHIM_WINE_RUN: u64 = 10;

/// 命令块入参区：ns[0..16] key[16..48] len@48 val[52..308]（path 复用 0..64）。
pub const BLK_NS: usize = 0;
pub const BLK_KEY: usize = 16;
pub const BLK_LEN: usize = 48;
pub const BLK_VAL: usize = 52;
pub const NS_MAX: usize = 16;
pub const KEY_MAX: usize = 32;
pub const PATH_MAX: usize = 64;
pub const VAL_MAX: usize = 256;
/// 出参区起点（入参区 512B 预留）。
pub const BLK_OUT: usize = 512;
/// vfs_read 数据上限；命令块总预算 4096。
pub const READ_MAX: usize = 2048;
pub const BLK_TOTAL: usize = 4096;

/// 命令块最小尺寸表（MISSING 之外的入参闸门第一步）。
pub fn shim_block_min(cmd: u64) -> Option<usize> {
    Some(match cmd {
        SHIM_KV_GET => BLK_OUT + 4 + VAL_MAX,
        SHIM_KV_SET => BLK_VAL + VAL_MAX,
        SHIM_KV_REMOVE => BLK_KEY + KEY_MAX,
        SHIM_KV_KEYS => BLK_OUT + 4 + 512,
        SHIM_VFS_LIST => BLK_OUT + 4 + 512,
        SHIM_VFS_READ => BLK_OUT + 4 + READ_MAX,
        SHIM_BOOT_EVENTS => 4 + 15 * 16,
        SHIM_BOOT_MS => 8,
        SHIM_VFS_SOURCE => 8,
        SHIM_WINE_RUN => BLK_OUT + 4 + 128,
        _ => return None,
    })
}

/// 从命令块提取零终止字符串（定长槽；无终止符 = EINVAL）。
pub fn zstr(block: &[u8], off: usize, max: usize) -> Result<&[u8], i64> {
    let slot = block.get(off..off + max).ok_or(einval())?;
    let end = slot.iter().position(|&b| b == 0).ok_or(einval())?;
    Ok(&slot[..end])
}

/// boot 事件记录编码（16B 定长）：idx u8 | state u8(1=begin 2=end) |
/// pad[2] | ms u32 LE | pad[8]。与 boot://event 语义对齐（阶段序号 +
/// 毫秒时刻；前端 bootEvents.ts 按 seq/进度单调夹取消费同构载荷）。
pub fn encode_boot_record(idx: u8, state: u8, ms: u32, out: &mut [u8; 16]) {
    *out = [0u8; 16];
    out[0] = idx;
    out[1] = state;
    out[4..8].copy_from_slice(&ms.to_le_bytes());
}

// ---------------------------------------------------------------------------
// 目标态：RAM KV 盘（usrshell 设置存储；4MiB .bss，戒律合规——禁堆物化）
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod ramblk {
    use crate::drivers::blk::{BlockDevice, BlockError};

    pub const RAMBLK_BLOCKS: u64 = 8192;

    /// 4MiB 后备存储：static .bss（戒律：>64KB 大缓冲一律 .bss 常驻，禁
    /// 栈上/Box 中转物化）。NOBITS 段不占 ELF 文件体积，Limine 装载清零，
    /// 分配不可能失败——原 PMM order-11 方案在 ushell 运行时可能拿不到
    /// 连续 8MiB（buddy 高阶块已被分裂/占用，实机复现 alloc failed），
    /// 且按设备容量（4MiB）属双倍超额。
    #[repr(C, align(4096))]
    struct Backing([u8; (RAMBLK_BLOCKS * 512) as usize]);

    static mut BACKING: Backing = Backing([0; (RAMBLK_BLOCKS * 512) as usize]);

    /// RAM 块设备（.bss 后端；单核演示语境）。
    pub struct RamBlk {
        base: *mut u8,
    }

    // SAFETY: 单核演示语境；内核当前无 SMP 用户进程并发。
    unsafe impl Send for RamBlk {}

    impl RamBlk {
        pub fn new() -> Option<RamBlk> {
            // .bss 由 Limine 装载清零，无需运行时 memset。
            Some(RamBlk { base: (&raw mut BACKING) as *mut u8 })
        }
    }

    impl BlockDevice for RamBlk {
        fn capacity_blocks(&self) -> u64 {
            RAMBLK_BLOCKS
        }
        fn block_size(&self) -> u32 {
            512
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            let n = (dst.len() / 512) as u64;
            if lba.checked_add(n).map(|e| e > RAMBLK_BLOCKS).unwrap_or(true) {
                return Err(BlockError::InvalidRange);
            }
            // SAFETY: 范围已校验；base 指向 PMM 持有帧（HHDM 可达）。
            unsafe {
                for (i, chunk) in dst.chunks_exact_mut(512).enumerate() {
                    let src = self.base.add(((lba + i as u64) * 512) as usize);
                    core::ptr::copy_nonoverlapping(src, chunk.as_mut_ptr(), 512);
                }
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            let n = (src.len() / 512) as u64;
            if lba.checked_add(n).map(|e| e > RAMBLK_BLOCKS).unwrap_or(true) {
                return Err(BlockError::InvalidRange);
            }
            unsafe {
                for (i, chunk) in src.chunks_exact(512).enumerate() {
                    let dst = self.base.add(((lba + i as u64) * 512) as usize);
                    core::ptr::copy_nonoverlapping(chunk.as_ptr(), dst, 512);
                }
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
use ramblk::RamBlk;

// ---------------------------------------------------------------------------
// 目标态：SHARED exFAT 全局只读挂载（任务18 挂载语义复用，零新解析器）
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod mount {
    use crate::cpu::sync::SpinProtected;
    use crate::drivers::nvme::target::{BarMmio, DmaBuckets};
    use crate::drivers::nvme::NvmeCtrl;
    use crate::fs::exfat_ro::ExfatVolume;

    pub type SharedVol = ExfatVolume<NvmeCtrl<BarMmio, DmaBuckets>>;

    static VOL: SpinProtected<Option<SharedVol>> = SpinProtected::new(None);
    /// SHARED 控制器 BDF（任务58：运行中移除检测登记）。
    static SHARED_BDF: SpinProtected<Option<(u8, u8, u8)>> = SpinProtected::new(None);

    /// NVMe ctrl#2（SHARED 分区）接管：探针链结束后把控制器移交全局
    /// 只读挂载。挂载失败 = 槽位保持 None（文件管理器如实降级演示树）。
    pub fn install(dev: NvmeCtrl<BarMmio, DmaBuckets>, bdf: (u8, u8, u8)) -> bool {
        match ExfatVolume::mount(dev) {
            Ok(v) => {
                *VOL.lock() = Some(v);
                *SHARED_BDF.lock() = Some(bdf);
                true
            }
            Err(_) => false,
        }
    }

    /// 使用点移除检测（任务58）：读 SHARED 控制器 PCI vendor——0xFFFF=
    /// 设备已消失（运行中拔盘）→ 卸载挂载槽并如实返回 false，后续 VFS
    /// 查询自然回落内置演示树（零冒充）。无 BDF/无法核验时保守放行
    /// （读取路径自会如实报错），不误伤。
    pub fn healthcheck() -> bool {
        let bdf = *SHARED_BDF.lock();
        let g = VOL.lock();
        if g.is_none() {
            return false;
        }
        let Some((bus, devn, func)) = bdf else {
            return true;
        };
        drop(g);
        let Some(rsdp) = crate::limine::rsdp_address() else {
            return true;
        };
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let Some(seg) = crate::drivers::pci::target::find_mcfg_phys(rsdp, hhdm)
            .and_then(|m| crate::drivers::pci::target::read_first_segment(m, hhdm))
        else {
            return true;
        };
        let mut ecam = crate::drivers::pci::target::EcamMmio::new(seg);
        let addr = crate::drivers::pci::ecam_addr(&seg, bus, devn, func, 0);
        if crate::drivers::pci::EcamAccess::read32(&mut ecam, addr) & 0xFFFF == 0xFFFF {
            *VOL.lock() = None;
            crate::kwarn!("mount: SHARED controller gone (vendor=FFFF) - uninstalled");
            false
        } else {
            true
        }
    }

    pub fn available() -> bool {
        VOL.lock().is_some()
    }

    /// IO 失效卸载（任务58）：读取路径报错（介质被移除——drive_del/
    /// 物理拔盘，guest 无 ACPI 弹出处理时 PCI 设备仍在位）→ 卸载槽位。
    /// 与 healthcheck（PCI vendor 探测）双通道，先触发者生效。
    pub fn uninstall_io_failed() {
        let mut g = VOL.lock();
        if g.is_some() {
            *g = None;
            crate::kwarn!("mount: SHARED io failed - uninstalled (media removed?)");
        }
    }

    /// 独占访问（列表/读文件；ExfatVolume 方法需要 &mut）。
    pub fn with<R>(f: impl FnOnce(&mut SharedVol) -> R) -> Option<R> {
        let mut g = VOL.lock();
        g.as_mut().map(f)
    }

    /// 底层块设备独占访问（S4.2 last_boot 写回通道：QEMU 第二 NVMe 兜底）。
    /// 挂载未就绪 = false。先过 healthcheck（拔盘即卸载，不写已消失的卷）。
    pub fn with_shared_dev(f: impl FnOnce(&mut dyn crate::drivers::blk::BlockDevice) -> bool) -> bool {
        if !healthcheck() {
            return false;
        }
        let mut g = VOL.lock();
        match g.as_mut() {
            Some(vol) => f(vol.dev_mut()),
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 目标态：syscall 处理器
// ---------------------------------------------------------------------------

/// SYS_FRAME 处理器。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_frame(a1: u64, a2: u64, a3: u64) -> i64 {
    use crate::displaysrv::target::service;
    use crate::font;

    let op = a1 & 0xFF;
    let Some(svc) = service() else { return eio() };
    let surf = svc.draw_surface();
    let (sw, sh) = (surf.width() as i64, surf.height() as i64);
    let color = palette((a1 >> 8) & 0xFF);
    match op {
        FRAME_INFO => ((surf.width() as i64) & 0xFFFF) | (((surf.height() as i64) & 0xFFFF) << 16),
        FRAME_FILL_RECT => {
            let (x, y, w, h) = unpack_xywh(a2);
            if !rect_plausible(x, y, w, h, sw, sh) {
                return einval();
            }
            surf.fill_rect(x, y, w, h, color);
            0
        }
        FRAME_OUTLINE => {
            let (x, y, w, h) = unpack_xywh(a2);
            if !rect_plausible(x, y, w, h, sw, sh) {
                return einval();
            }
            surf.rect_outline(x, y, w, h, color);
            0
        }
        FRAME_HLINE => {
            let (x0, x1, y, _) = unpack_xywh(a2);
            if y < 0 || y >= sh || x1 < x0 || x0 >= sw || x1 < 0 {
                return einval();
            }
            surf.hline(x0, x1, y, color);
            0
        }
        FRAME_VLINE => {
            let (x, y0, y1, _) = unpack_xywh(a2);
            if x < 0 || x >= sw || y1 < y0 || y0 >= sh || y1 < 0 {
                return einval();
            }
            surf.vline(x, y0, y1, color);
            0
        }
        FRAME_WALLPAPER => {
            if crate::wallpaper::blit_scaled(surf) {
                0
            } else {
                enosys() // 壁纸模块缺席：ushell 回退色带
            }
        }
        FRAME_WALLPAPER_PX => {
            let (x, y, _, _) = unpack_xywh(a2);
            if x < 0 || y < 0 || x >= sw || y >= sh {
                return einval();
            }
            crate::wallpaper::sample_at(x, y, surf.width(), surf.height())
        }
        FRAME_FILL_RGB => {
            let (x, y, w, h) = unpack_xywh(a2);
            if !rect_plausible(x, y, w, h, sw, sh) {
                return einval();
            }
            let rgb = a3 & 0xFF_FFFF;
            let c = crate::fb::Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            surf.fill_rect(x, y, w, h, c);
            0
        }
        FRAME_CURSOR_SAVE => {
            let (x, y, _, _) = unpack_xywh(a2);
            cursor_shadow_save(surf, x, y)
        }
        FRAME_CURSOR_RESTORE => {
            let (x, y, _, _) = unpack_xywh(a2);
            cursor_shadow_restore(surf, x, y)
        }
        FRAME_TEXT => {
            let len = ((a1 >> 16) & 0xFF) as usize;
            let scale = ((a1 >> 28) & 0xF) as i64;
            let scale = if scale == 0 { 1 } else { scale };
            let (x, y, _, _) = unpack_xywh(a2);
            if len == 0 {
                return 0;
            }
            let Some(end) = a3.checked_add(len as u64) else { return efault() };
            if end > crate::entry::USER_TOP || !crate::entry::is_user_ip(a3) {
                return efault();
            }
            let mut buf = [0u8; FRAME_TEXT_MAX];
            // SAFETY: a3..end 已校验用户半区；单核演示地址空间独占。
            for (i, slot) in buf[..len].iter_mut().enumerate() {
                *slot = unsafe { core::ptr::read_volatile((a3 + i as u64) as *const u8) };
            }
            let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
            font::draw_text_scaled(surf, x, y, text, color, scale);
            0
        }
        _ => enosys(),
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_frame(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

/// SYS_INPUT 处理器：泵硬件 + 16B 事件灌入用户缓冲。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_input(a1: u64, a2: u64, _a3: u64) -> i64 {
    const MAX_EVENTS: usize = 64;
    let want = a2 as usize;
    if want == 0 || want > MAX_EVENTS {
        return einval();
    }
    let end = a1.checked_add((want * 16) as u64).unwrap_or(u64::MAX);
    if end > crate::entry::USER_TOP || !crate::entry::is_user_ip(a1) {
        return efault();
    }
    static mut STAGE: [u8; MAX_EVENTS * 16] = [0u8; MAX_EVENTS * 16];
    // SAFETY: 单核 syscall 语境独占 staging。
    let n = {
        let stage = unsafe { &mut *(&raw mut STAGE) };
        crate::inputsvc::target::drain_to_shim(&mut stage[..want * 16], want)
    };
    if n == 0 {
        return 0;
    }
    // SAFETY: a1..end 已校验用户半区。
    unsafe {
        let stage = &*(&raw const STAGE);
        for i in 0..n * 16 {
            core::ptr::write_volatile((a1 + i as u64) as *mut u8, stage[i]);
        }
    }
    n as i64
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_input(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

/// 指针底图 shadow（2026-09-22 鼠标流畅性修复）：内核侧单份 16×16 快照。
/// ushell 桌面期单线程独占（ring3 主循环串行 syscall），static mut 无并发。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static mut CURSOR_SHADOW: Option<[u32; CURSOR_SHADOW_SIDE * CURSOR_SHADOW_SIDE]> = None;

/// SAVE：把 (x,y) 起的 16×16 底图读进 shadow（越界像素跳过——RESTORE
/// 同样跳过越界，视觉无差）。返回 0。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn cursor_shadow_save(surf: &mut crate::fb::Surface, x: i64, y: i64) -> i64 {
    let mut buf = [0u32; CURSOR_SHADOW_SIDE * CURSOR_SHADOW_SIDE];
    let mut i = 0usize;
    for dy in 0..CURSOR_SHADOW_SIDE as i64 {
        for dx in 0..CURSOR_SHADOW_SIDE as i64 {
            if let Some(px) = surf.get_px(x + dx, y + dy) {
                buf[i] = px;
            }
            i += 1;
        }
    }
    let slot = &raw mut CURSOR_SHADOW;
    unsafe {
        *slot = Some(buf);
    }
    0
}

/// RESTORE：把 shadow 写回 (x,y)（必须与最近一次 SAVE 同位置——ushell
/// 契约；未保存过/已消费 = no-op 返回 1）。写完消费掉 shadow，防二次
/// RESTORE 把旧底图盖到新位置。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn cursor_shadow_restore(surf: &mut crate::fb::Surface, x: i64, y: i64) -> i64 {
    let slot = &raw mut CURSOR_SHADOW;
    let taken = unsafe { (*slot).take() };
    let Some(buf) = taken else { return 1 };
    let fmt = surf.format();
    let mut i = 0usize;
    for dy in 0..CURSOR_SHADOW_SIDE as i64 {
        for dx in 0..CURSOR_SHADOW_SIDE as i64 {
            let px = buf[i];
            i += 1;
            if x + dx >= 0 && y + dy >= 0 {
                surf.set_px(x + dx, y + dy, fmt.unpack(px));
            }
        }
    }
    0
}

/// SYS_REBOOT 处理器：重启整机，交还固件引导序（Windows 默认第一项）。
///
/// 五级复位阶梯（每级落空则下一级，日志逐级留痕）：
/// ① UEFI `ResetSystem(EfiResetCold)`（`bootnext::reset_cold`；Limine 不调
///    ExitBootServices，RS 代码内部以绝对物理地址自引用——复位前先建运行
///    期恒等映射，与 main.rs boot-select windows 路径同序，两函数幂等）；
/// ② FADT RESET_REG（`bootnext::reset_via_fadt`——固件声明的复位端口，
///    B-2901 最小集第一件；零分配，panic 阶梯同源共用）；
/// ③ 8042 脉冲复位（0xFE → 0x64，状态寄存器 bit1 等空后发）；
/// ④ ACPI 复位寄存器 0xCF9（ICH9/q35 与真实 Intel PCH 同寄存器，先 0x04
///    暖复位再 0x06 全复位）；
/// ⑤ 三重故障兜底（IDT 置空 + int3）——零外设依赖，任何 x86 平台一致。
///
/// 刻意**不写 BootNext**：目标 Boot#### 项号未在实机确证前盲写，可能把
/// 重启循环回本 U 盘。纯复位后固件走默认引导序（内置盘 Windows bootmgr，
/// BCD 菜单 5s 默认进 Windows）；U 盘 limine.conf 亦有
/// `/Windows 11 (built-in disk)` chainload 项双保险。正常永不返回。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_reboot(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    crate::kinfo!("reboot: SYS_REBOOT — resetting into firmware boot order");
    let _ = crate::bootnext::prepare_runtime_identity_map();
    let blocks = crate::bootnext::identity_map_low_4gib();
    crate::kinfo!("reboot: runtime identity-mapped ({} x 2MiB)", blocks);
    // ① UEFI 主路径。
    if crate::bootnext::reset_cold() {
        // ResetSystem 正常不返回；返回 = 本固件复位路径异常，落硬件阶梯。
        crate::kinfo!("reboot: ResetSystem returned — hardware ladder next");
    } else {
        crate::kinfo!("reboot: no UEFI runtime services (BIOS boot) — hardware ladder");
    }
    // ② FADT 声明的复位寄存器（B-2901；零分配，与 panic 阶梯同源）。
    if crate::bootnext::reset_via_fadt() {
        crate::kinfo!("reboot: FADT reset returned — ladder next");
    } else {
        crate::kinfo!("reboot: no FADT reset reg — ladder next");
    }
    spin_cycles(10_000_000);
    // ③ 8042 脉冲复位。
    crate::kinfo!("reboot: 8042 pulse (0xFE -> 0x64)");
    // SAFETY: 端口 IO 单一来源（ps2::port）；0x64 状态寄存器 bit1=输入缓冲满。
    unsafe {
        for _ in 0..100_000 {
            if crate::ps2::port::inp(0x64) & 0x02 == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        crate::ps2::port::outp(0x64, 0xFE);
    }
    spin_cycles(50_000_000);
    // ④ ACPI 复位寄存器。
    crate::kinfo!("reboot: acpi reset (0xCF9 <- 0x04/0x06)");
    // SAFETY: 同上；0xCF9 为标准 PCH 复位寄存器，非 Intel 平台写入无害。
    unsafe {
        crate::ps2::port::outp(0xCF9, 0x04);
        spin_cycles(1_000);
        crate::ps2::port::outp(0xCF9, 0x06);
    }
    spin_cycles(50_000_000);
    // ⑤ 三重故障兜底。
    crate::kinfo!("reboot: triple-fault fallback");
    triple_fault_reset();
}

/// 复位生效窗口的纯自旋等待（此处已是复位不归路）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn spin_cycles(n: u64) {
    for _ in 0..n {
        core::hint::spin_loop();
    }
}

/// 三重故障复位兜底：IDTR 置空（limit=0）后 int3——#BP 向量查表超限 →
/// #DF → 三重故障 → CPU 硬复位。零外设依赖，任何 x86 平台/QEMU 一致。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn triple_fault_reset() -> ! {
    let idtr = [0u64; 2]; // limit=0, base=0
    unsafe {
        core::arch::asm!(
            "lidt [{p}]",
            "int3",
            p = in(reg) idtr.as_ptr(),
            options(noreturn)
        );
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_reboot(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

/// SYS_POWEROFF 处理器（2026-09-19 初版；WP-106 B-2902 补软件收尾链）。
///
/// 关机全链（MD2 篇 29.2 硬序）：**保全 → 冲刷 → 通知链 → ACPI S5**。
/// "可以拔电了"画面的出现条件是冲刷完成加通知链收束——画面的每一秒
/// 都有账可查（[`ShutdownLedger`] 四相 verbatim 记账，describe 进日志）。
///
/// 硬件两级阶梯（每级落空则下一级，日志逐级留痕；全败如实返回 -EIO 让
/// shell 回桌面继续可用）：
/// ① UEFI `ResetSystem(EfiResetShutdown)`（`bootnext::reset_shutdown`；
///    与 sys_reboot 同序——先建运行期恒等映射，两函数幂等）；
/// ② ACPI S5（`bootnext::poweroff_s5`：FACP→PM1a_CNT 写
///    `(SLP_TYP<<10)|SLP_EN`，SLP_TYP 取自 DSDT `\_S5` 包解码）。
///
/// 无 8042/0xCF9 类硬件兜底——S5 断电只有 UEFI/ACPI 两条正道；两者都
/// 不可用（BIOS 引导无 RS、无 FACP）时按 [`FIRMWARE_TIMEOUT_LINE`] 的
/// 诚实指引报错回桌面，绝不假装关机。写入生效后给平台一个断电窗口再
/// 判定失败。正常永不返回。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_poweroff(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    use crate::power_shutdown::{PhaseVerdict, ShutdownLedger, ShutdownPhase, UNPLUG_LINE};
    crate::kinfo!("poweroff: SYS_POWEROFF — powering off");
    let mut ledger = ShutdownLedger::new();

    // ① 保全（快照与草稿）：ushell 会话无挂起草稿（应用面随 m2 桌面
    //    落地）——账目如实记 Skipped，保全的完整实现随交接链（handoff）。
    ledger.record(
        ShutdownPhase::Preserve,
        0,
        PhaseVerdict::Skipped("no pending drafts in ushell session"),
    );

    // ② 冲刷（篇 2.4 全序列）：步序协议 WP-102 冻结、真实执行器 WP-203
    //    存储栈接线（crate::handoff::flush 的 FlushStep trait 边界）。本
    //    会话冲刷执行器尚未接线——如实在账，绝不假装冲过（B-2902 的账
    //    要每一秒经得起对账）。
    let t0 = crate::timeline::read_tsc();
    let mounted = mount::available();
    ledger.record(
        ShutdownPhase::Flush,
        crate::timeline::read_tsc().saturating_sub(t0) / crate::platform::info()
            .map(|p| p.tsc_hz)
            .unwrap_or(crate::platform::FALLBACK_TSC_HZ)
            .max(1),
        if mounted {
            PhaseVerdict::Skipped("fs mounted but flush executors wire at WP-203")
        } else {
            PhaseVerdict::Skipped("no mounted fs — nothing dirty to flush")
        },
    );

    // ③ 通知链：服务注册表现状为零（ushell 域尚无长驻服务登记面）——
    //    空集全绿记账；链路本身已就位（power_shutdown::run_notify_chain
    //    带超时强收语义），随服务增长逐个挂入。
    let mut empty: [&mut dyn crate::power_shutdown::NotifyStep; 0] = [];
    let t1 = crate::timeline::read_tsc();
    let chain = crate::power_shutdown::run_notify_chain(&mut empty, 5_000);
    let notify_ms = crate::timeline::read_tsc().saturating_sub(t1) / crate::platform::info()
        .map(|p| p.tsc_hz)
        .unwrap_or(crate::platform::FALLBACK_TSC_HZ)
        .max(1);
    ledger.record(
        ShutdownPhase::NotifyChain,
        notify_ms,
        if chain.settled { PhaseVerdict::Ok } else { PhaseVerdict::Forced },
    );
    crate::kinfo!("poweroff: notify chain settled={} (0 services registered)", chain.settled);

    // "可以拔电了"画面（篇 29.2：冲刷完成 + 通知链收束才许出现；此后的
    // 硬件阶梯每一毫秒都在账上）。画面走 console best-effort——装了才画。
    if ledger.unpluggable() {
        if let Some(c) = crate::console::installed_ref() {
            c.set_colors(crate::console::Ink::White, crate::console::Ink::Green);
            c.write_str("\n");
            c.write_str(UNPLUG_LINE);
            c.write_str("\n");
        }
        crate::kinfo!("poweroff: {}", UNPLUG_LINE);
    }

    // ④ S5 硬件阶梯。
    let _ = crate::bootnext::prepare_runtime_identity_map();
    let blocks = crate::bootnext::identity_map_low_4gib();
    crate::kinfo!("poweroff: runtime identity-mapped ({} x 2MiB)", blocks);
    // ① UEFI 主路径。
    if crate::bootnext::reset_shutdown() {
        // ResetSystem 正常不返回；返回 = 本固件关机路径异常，落 ACPI。
        crate::kinfo!("poweroff: ResetSystem returned — ACPI S5 next");
    } else {
        crate::kinfo!("poweroff: no UEFI runtime services (BIOS boot) — ACPI S5");
    }
    // ② ACPI S5。
    crate::kinfo!("poweroff: acpi s5 (PM1a_CNT <- SLP_TYP|SLP_EN)");
    let s5_ok = crate::bootnext::poweroff_s5();
    ledger.record(
        ShutdownPhase::S5,
        0,
        if s5_ok { PhaseVerdict::Ok } else { PhaseVerdict::Failed("no S5 path") },
    );
    crate::kinfo!("poweroff: {}", ledger.describe());
    // 断电生效窗口：QEMU 即刻退出；实机数秒内断电。仍运行 = 寄存器落空。
    spin_cycles(100_000_000);
    // 全败：诚实指引（29.1 超时文案）+ 报错回桌面，绝不假装关机。
    if let Some(c) = crate::console::installed_ref() {
        c.set_colors(crate::console::Ink::White, crate::console::Ink::Red);
        c.write_str("\n");
        c.write_str(crate::power_shutdown::FIRMWARE_TIMEOUT_LINE);
        c.write_str("\n");
    }
    crate::kwarn!("poweroff: all paths failed — reporting to shell");
    eio()
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_poweroff(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

/// SYS_SHIM 处理器：命令垫片（KV/VFS/boot 事件/时钟）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_shim(a1: u64, a2: u64, a3: u64) -> i64 {
    let Some(min) = shim_block_min(a1) else { return enosys() };
    if (a3 as usize) < min {
        return einval();
    }
    let end = a2.checked_add(a3).unwrap_or(u64::MAX);
    if end > crate::entry::USER_TOP || !crate::entry::is_user_ip(a2) {
        return efault();
    }
    static mut BLK: [u8; BLK_TOTAL] = [0u8; BLK_TOTAL];
    // SAFETY: 单核 syscall 语境独占块缓冲；a2..end 已校验用户半区。
    let block: &mut [u8; BLK_TOTAL] = unsafe { &mut *(&raw mut BLK) };
    unsafe {
        for i in 0..min {
            block[i] = core::ptr::read_volatile((a2 + i as u64) as *const u8);
        }
    }
    let rc = shim_dispatch(a1, block);
    // 出参回写（min 覆盖出参区——出参区在 min 之内的命令才有出参）。
    if rc >= 0 {
        unsafe {
            for i in 0..min {
                core::ptr::write_volatile((a2 + i as u64) as *mut u8, block[i]);
            }
        }
    }
    rc
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_shim(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

// ---------------------------------------------------------------------------
// WIN：窗口面服务系统调用（AI-4 · S2.06 渲染通路 + S2.09 多窗合成）
// ---------------------------------------------------------------------------

/// SYS_WIN 处理器（目标态）：子命令分派到 winsurf 服务 + inputsvc 焦点联动。
/// 用户内存访问走 sys_shim 同范式（is_user_ip/USER_TOP 校验 + volatile）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn sys_win(a1: u64, a2: u64, a3: u64) -> i64 {
    let op = a1 & 0xFF;
    let Some(wsvc) = crate::winsurf::service() else { return eio() };
    match op {
        WIN_REGISTER => {
            let (w, h) = win_unpack_wh(a3);
            let Some(disp) = crate::displaysrv::target::service() else { return eio() };
            let fmt = disp.draw_surface().format();
            match wsvc.register(a2 as u32, w, h, fmt) {
                Some(id) => id as i64,
                None => enospc(), // 槽满/几何越界/容量超限——明确拒绝
            }
        }
        WIN_UNREGISTER => {
            if wsvc.unregister(a2 as u16) { 0 } else { einval() }
        }
        WIN_GEO => {
            let (x, y) = win_unpack_xy(a3);
            if wsvc.set_geo(a2 as u16, x, y) { 0 } else { einval() }
        }
        WIN_RAISE => {
            if wsvc.raise(a2 as u16) { 0 } else { einval() }
        }
        WIN_STATE => {
            let st = if a3 & 1 == 1 {
                crate::winsurf::WinState::Minimized
            } else {
                crate::winsurf::WinState::Visible
            };
            if wsvc.set_state(a2 as u16, st) { 0 } else { einval() }
        }
        WIN_FOCUS => {
            if a2 == 0 {
                wsvc.focus(0);
                crate::inputsvc::target::clear_focus();
                return 0;
            }
            if !wsvc.focus(a2 as u16) {
                return einval();
            }
            // 跨层接线：窗口焦点属主 pid → 内核级键盘路由（S2.05）。
            // 无匹配订阅者（如属主尚未注册 KFO 订阅）不视为失败——
            // 桌面进程自持 DOM 焦点语义。
            if let Some(pid) = wsvc.focus_owner() {
                crate::inputsvc::target::focus_pid(pid);
            }
            0
        }
        WIN_SUBMIT => win_submit(wsvc, a2 as u16, a3),
        WIN_COMPOSITE => {
            let Some(disp) = crate::displaysrv::target::service() else { return eio() };
            wsvc.composite(disp);
            let st = wsvc.stats();
            (st.blit_rows & 0x7FFF_FFFF_FFFF_FFFF) as i64
        }
        WIN_QUERY => {
            if a2 == 0 {
                let st = wsvc.stats();
                return win_pack_wh(st.frames, st.blit_rows) as i64;
            }
            let Some(info) = wsvc.window_info(a2 as u16) else { return einval() };
            match a3 {
                0 => win_pack_xy(info.x, info.y) as i64,
                1 => win_pack_wh(info.w as u64, info.h as u64) as i64,
                _ => {
                    let bits = (info.minimized as u64)
                        | ((info.is_focus as u64) << 1)
                        | ((info.generation as u64 & 0x3FFF_FFFF) << 32);
                    bits as i64
                }
            }
        }
        _ => enosys(),
    }
}

/// WIN_SUBMIT 实现（目标态）：读用户提交块（头+脏区+像素），逐行分段
/// volatile 拷入内核行缓冲再 stage_row。返回已提交行数。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn win_submit(wsvc: &mut crate::winsurf::WinService, wid: u16, block: u64) -> i64 {
    // 用户指针校验（sys_shim 同范式：用户半区 + USER_TOP）。
    if !crate::entry::is_user_ip(block) {
        return efault();
    }
    let info = match wsvc.window_info(wid) {
        Some(i) => i,
        None => return einval(),
    };
    let bpp = info.bpp.max(1); // register 已绑定屏幕 fmt；0=异常诚实拒绝
    if info.bpp == 0 {
        return eio();
    }
    let rd_u32 = |addr: u64| -> u32 {
        // SAFETY: 调用方已校验 is_user_ip；块内偏移由下方总长校验兜住。
        unsafe { core::ptr::read_volatile(addr as *const u32) }
    };
    if !crate::entry::is_user_ip(block)
        || block.checked_add(WIN_SUBMIT_HDR as u64).map_or(true, |e| e > crate::entry::USER_TOP)
    {
        return efault();
    }
    let kind = rd_u32(block);
    let count = rd_u32(block + 4);
    if !win_submit_hdr_ok(kind, count) {
        return einval();
    }
    let rects_bytes = WIN_SUBMIT_RECT as u64 * count as u64;
    let mut pix = match block.checked_add(WIN_SUBMIT_HDR as u64 + rects_bytes) {
        Some(p) if p <= crate::entry::USER_TOP => p,
        _ => return efault(),
    };

    // 脏区列表（内核侧校验副本；kind=FULL 恰一条隐含全窗）。
    static mut RECTS: [(i64, i64, u32, u32); WIN_SUBMIT_MAX_RECTS as usize] =
        [(0, 0, 0, 0); WIN_SUBMIT_MAX_RECTS as usize];
    // SAFETY: 单核 syscall 语境独占；宽 ≤ WIN_SUBMIT_MAX_RECTS。
    let rects: &mut [(i64, i64, u32, u32)] = unsafe { &mut *(&raw mut RECTS) };
    let mut rect_n = 0usize;
    let mut total_px: u64 = 0;
    if kind == WIN_SUBMIT_KIND_FULL {
        rects[0] = (0, 0, info.w, info.h);
        rect_n = 1;
    } else {
        for i in 0..count as usize {
            let base = block + WIN_SUBMIT_HDR as u64 + (i * WIN_SUBMIT_RECT) as u64;
            let x = rd_u32(base) as i32 as i64;
            let y = rd_u32(base + 4) as i32 as i64;
            let w = rd_u32(base + 8);
            let h = rd_u32(base + 12);
            rects[i] = (x, y, w, h);
        }
        rect_n = count as usize;
    }
    for &(rx, ry, rw, rh) in rects[..rect_n].iter() {
        if rw == 0 || rh == 0 {
            continue;
        }
        if rx < 0 || ry < 0 || rx + rw as i64 > info.w as i64 || ry + rh as i64 > info.h as i64 {
            return einval();
        }
        total_px = total_px.saturating_add(rw as u64 * bpp as u64 * rh as u64);
    }
    // 像素区整体不得越过用户半区（先验总长，拷贝路径零再校验）。
    if pix.checked_add(total_px).map_or(true, |e| e > crate::entry::USER_TOP) {
        return efault();
    }

    // 逐区逐行：分段拷像素 → stage_row → 登记脏区。
    let mut row_buf = [0u8; WIN_ROW_CHUNK];
    let mut submitted = 0i64;
    for &(rx, ry, rw, rh) in rects[..rect_n].iter() {
        if rw == 0 || rh == 0 {
            continue;
        }
        let row_bytes = rw as usize * bpp;
        for r in 0..rh as i64 {
            let mut xoff = 0usize;
            while xoff < row_bytes {
                let n = (row_bytes - xoff).min(WIN_ROW_CHUNK);
                for (k, b) in row_buf[..n].iter_mut().enumerate() {
                    // SAFETY: pix..pix+total_px 已先验落在用户半区内。
                    *b = unsafe { core::ptr::read_volatile((pix + k as u64) as *const u8) };
                }
                if !wsvc.stage_row(wid, ry + r, rx as u32 + xoff as u32, &row_buf[..n]) {
                    return einval();
                }
                pix += n as u64;
                xoff += n;
            }
            submitted += 1;
        }
        wsvc.mark_window_dirty(wid, crate::displaysrv::Rect::new(rx, ry, rw as i64, rh as i64));
    }
    wsvc.end_submit(wid);
    submitted
}

/// SYS_WIN 宿主版：如实 ENOSYS（编解码纯函数另行单测）。
#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn sys_win(_a1: u64, _a2: u64, _a3: u64) -> i64 {
    enosys()
}

/// 命令分发（target 编译；块已拷入内核缓冲，纯内存操作后由调用方回写）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_dispatch(cmd: u64, block: &mut [u8; BLK_TOTAL]) -> i64 {
    match cmd {
        SHIM_KV_GET | SHIM_KV_SET | SHIM_KV_REMOVE | SHIM_KV_KEYS => shim_kv(cmd, block),
        SHIM_VFS_LIST => shim_vfs_list(block),
        SHIM_VFS_READ => shim_vfs_read(block),
        SHIM_VFS_SOURCE => shim_vfs_source(block),
        SHIM_WINE_RUN => shim_wine_run(block),
        SHIM_BOOT_EVENTS => shim_boot_events(block),
        SHIM_BOOT_MS => {
            let ms = boot_ms();
            block[0..8].copy_from_slice(&ms.to_le_bytes());
            8
        }
        _ => enosys(),
    }
}

/// KV 命令组：ns/key 取槽；设置存储 = kvsrv over RamBlk（RAM 后端，
/// 会话级持久——如实标注，非断电持久；断电持久 KV 见任务24 盘面探针）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_kv(cmd: u64, block: &mut [u8; BLK_TOTAL]) -> i64 {
    use core::sync::atomic::{AtomicBool, Ordering};
    static OPENED: AtomicBool = AtomicBool::new(false);
    static STORE: crate::cpu::sync::SpinProtected<
        Option<crate::kvsrv::KvStore<RamBlk>>,
    > = crate::cpu::sync::SpinProtected::new(None);

    if !OPENED.swap(true, Ordering::AcqRel) {
        // .bss 后端：new() 实际不可失败；None 分支纯防御（契约保留，
        // 打点如实——绝不静默伪造成功）。
        let Some(mut dev) = RamBlk::new() else {
            crate::kwarn!("shim-kv: ramdisk slot unavailable");
            return -(ErrNo::Enomem.to_i32() as i64);
        };
        if let Err(e) = crate::kvsrv::KvStore::format(&mut dev, 0) {
            crate::kwarn!("shim-kv: ramdisk format failed: {:?}", e);
            return eio();
        }
        match crate::kvsrv::KvStore::open(dev, 0, 256) {
            Ok((s, _)) => {
                *STORE.lock() = Some(s);
                crate::kinfo!("shim-kv: ramdisk kv ready");
            }
            Err(e) => {
                crate::kwarn!("shim-kv: ramdisk open failed: {:?}", e);
                return eio();
            }
        }
    }
    let ns = match zstr(block, BLK_NS, NS_MAX) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut g = STORE.lock();
    let Some(store) = g.as_mut() else { return eio() };
    match cmd {
        SHIM_KV_GET => {
            let key = match zstr(block, BLK_KEY, KEY_MAX) {
                Ok(s) => s,
                Err(e) => return e,
            };
            match store.get(ns, key) {
                Ok(Some(val)) => {
                    let n = val.len().min(VAL_MAX);
                    let out = &mut block[BLK_OUT..BLK_OUT + 4 + VAL_MAX];
                    out[0..4].copy_from_slice(&(n as u32).to_le_bytes());
                    out[4..4 + n].copy_from_slice(&val[..n]);
                    n as i64
                }
                Ok(None) => 0,
                Err(_) => eio(),
            }
        }
        SHIM_KV_SET => {
            let key = match zstr(block, BLK_KEY, KEY_MAX) {
                Ok(s) => s,
                Err(e) => return e,
            };
            let len = u32::from_le_bytes(
                block[BLK_LEN..BLK_LEN + 4].try_into().unwrap_or([0; 4]),
            ) as usize;
            if len > VAL_MAX {
                return enospc();
            }
            match store.set(ns, key, &block[BLK_VAL..BLK_VAL + len]) {
                Ok(()) => 0,
                Err(_) => enospc(),
            }
        }
        SHIM_KV_REMOVE => {
            let key = match zstr(block, BLK_KEY, KEY_MAX) {
                Ok(s) => s,
                Err(e) => return e,
            };
            match store.remove(ns, key) {
                Ok(()) => 0,
                Err(_) => eio(),
            }
        }
        _ => {
            // SHIM_KV_KEYS：出参 = count u32 + 零分隔键名。
            match store.keys(ns) {
                Ok(keys) => {
                    let out = &mut block[BLK_OUT..BLK_OUT + 4 + 512];
                    let mut off = 4usize;
                    let mut count = 0u32;
                    for k in keys.iter() {
                        if off + k.len() + 1 > out.len() {
                            break;
                        }
                        out[off..off + k.len()].copy_from_slice(k);
                        off += k.len();
                        out[off] = 0;
                        off += 1;
                        count += 1;
                    }
                    out[0..4].copy_from_slice(&count.to_le_bytes());
                    count as i64
                }
                Err(_) => eio(),
            }
        }
    }
}

/// VFS 数据源查询：rc=1 当且仅当 SHARED exFAT 全局只读挂载可用（任务18
/// 挂载语义）；0 = 内置演示树。页脚如实标注数据源，绝不冒充。双通道
/// 出参（rc 与 block[0] 同值）：ushell 判 rc，契约文档记 block[0]。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_vfs_source(block: &mut [u8; BLK_TOTAL]) -> i64 {
    // 任务58：使用点移除检测（拔盘→卸载→如实降级）。
    mount::healthcheck();
    let src: i64 = if mount::available() { 1 } else { 0 };
    block[0] = src as u8;
    src
}

/// Wine 应用拉起（交叉走查第一项入口）：key 槽应用名 → winelaunch
/// 受理（注册表查表 + 前缀实例化 + 运行时如实状态）→ 出参编码 +
/// kinfo 打点一行（实机证据源）。缺席/未知应用照常返回——标准报错，
/// 绝不静默伪造启动成功。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_wine_run(block: &mut [u8; BLK_TOTAL]) -> i64 {
    let app = match zstr(block, BLK_KEY, KEY_MAX) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let now_ms = boot_ms() as u64;
    let outcome = crate::winelaunch::launch(app, now_ms);
    let out_len = crate::winelaunch::encode_status(&mut block[BLK_OUT..], &outcome);
    if out_len == 0 {
        return eio();
    }
    // 实机证据打点：一行可 grep 的拉起记录（走查日志面）。
    crate::kinfo!(
        "wine-launch: state={} runtime={} app_id={:#x} templ_ver={} bytes={}",
        outcome.state as u8,
        match outcome.runtime {
            crate::winelaunch::RuntimeState::Present => 0u8,
            crate::winelaunch::RuntimeState::Absent => 1u8,
            crate::winelaunch::RuntimeState::LockMismatch => 2u8,
        },
        outcome.prefix.as_ref().map(|p| p.app_id).unwrap_or(0),
        outcome.prefix.as_ref().map(|p| p.templ_ver).unwrap_or(0),
        out_len,
    );
    out_len as i64
}

/// VFS 列表：SHARED 挂载在 → exFAT 真实目录；否则 → 内置演示树（如实
/// 降级——文件管理器页脚标注数据源，绝不冒充真实 SHARED）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_vfs_list(block: &mut [u8; BLK_TOTAL]) -> i64 {
    // 任务58：使用点移除检测（拔盘→卸载→如实降级演示树）。
    mount::healthcheck();
    let path = match zstr(block, 0, PATH_MAX) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // path 借用自 block——先拷贝出定长缓冲再交出 &mut（降级树/出参回写）。
    let mut path_buf = [0u8; PATH_MAX];
    let plen = path.len().min(PATH_MAX - 1);
    path_buf[..plen].copy_from_slice(&path[..plen]);
    let path: &[u8] = &path_buf[..plen];
    if let Some(entries) = mount::with(|vol| vol.read_dir(core::str::from_utf8(path).unwrap_or(""))) {
        match entries {
            Ok(items) => {
                let out = &mut block[BLK_OUT..BLK_OUT + 4 + 512];
                let mut off = 4usize;
                let mut count = 0u32;
                for it in items.iter() {
                    // 条目编码：name\0 size\0 is_dir("1"/"0")\0
                    let size = format_u64(it.size);
                    for piece in [
                        it.name.as_bytes(),
                        &size,
                        if it.is_dir { b"1" } else { b"0" },
                    ] {
                        if off + piece.len() + 1 > out.len() {
                            break;
                        }
                        out[off..off + piece.len()].copy_from_slice(piece);
                        off += piece.len();
                        out[off] = 0;
                        off += 1;
                    }
                    count += 1;
                }
                out[0..4].copy_from_slice(&count.to_le_bytes());
                count as i64
            }
            Err(_) => {
                // 任务58：IO 失效检测——块后端被移除（drive_del/拔盘，
                // guest 无 ACPI 弹出处理时 PCI 设备仍在位、vendor 探测
                // 探不到）→ 读取必然报错；此刻卸载挂载槽并如实降级
                // 演示树。后续 VFS_SOURCE 自然返回 0（零冒充）。
                mount::uninstall_io_failed();
                demo_tree_list(path, block)
            }
        }
    } else {
        demo_tree_list(path, block)
    }
}

/// 内置演示树（挂载缺失时的如实降级数据源；页脚由 ushell 标注
/// "DEMO TREE"，不冒充真实 SHARED）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn demo_tree_list(path: &[u8], block: &mut [u8; BLK_TOTAL]) -> i64 {
    const ROOT: [(&[u8], u64, bool); 5] = [
        (b"apps.json", 322, false),
        (b"boot-select.json", 96, false),
        (b"readme.txt", 48, false),
        (b"handoff", 0, true),
        (b"whitelist", 0, true),
    ];
    const HANDOFF: [(&[u8], u64, bool); 1] = [(b"intent-001.uxv", 4096, false)];
    let empty: [(&[u8], u64, bool); 0] = [];
    let items: &[(&[u8], u64, bool)] = if path.is_empty() || path == b"/" {
        &ROOT
    } else if path == b"handoff" || path == b"/handoff" {
        &HANDOFF
    } else if path == b"whitelist" || path == b"/whitelist" {
        &empty
    } else {
        return -(ErrNo::Eacces.to_i32() as i64);
    };
    let out = &mut block[BLK_OUT..BLK_OUT + 4 + 512];
    let mut off = 4usize;
    for (name, size, is_dir) in items.iter() {
        let size = format_u64(*size);
        for piece in [*name, &size, if *is_dir { b"1" } else { b"0" }] {
            if off + piece.len() + 1 > out.len() {
                break;
            }
            out[off..off + piece.len()].copy_from_slice(piece);
            off += piece.len();
            out[off] = 0;
            off += 1;
        }
    }
    out[0..4].copy_from_slice(&(items.len() as u32).to_le_bytes());
    items.len() as i64
}

/// VFS 读文件：SHARED 优先，降级演示树内容（逐字节确定）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_vfs_read(block: &mut [u8; BLK_TOTAL]) -> i64 {
    let path = match zstr(block, 0, PATH_MAX) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let data: alloc::vec::Vec<u8> = if let Some(res) =
        mount::with(|vol| vol.read_file(core::str::from_utf8(path).unwrap_or("")))
    {
        match res {
            Ok(d) => d,
            Err(_) => return eio(),
        }
    } else {
        match path {
            b"apps.json" | b"/apps.json" => {
                b"{\"version\":3,\"apps\":[{\"id\":\"notepad-classic\",\"channel\":\"wine\",\"tier\":\"partial\"}]}\n".to_vec()
            }
            b"boot-select.json" | b"/boot-select.json" => {
                b"{\"default_entry\":\"variable\",\"timeout_sec\":5,\"show_menu\":true}\n".to_vec()
            }
            b"readme.txt" | b"/readme.txt" => {
                b"VARIX dual-domain SHARED contract demo tree (no exFAT mount).\n".to_vec()
            }
            b"handoff/intent-001.uxv" | b"/handoff/intent-001.uxv" => {
                b"UXV-DEMO-PAYLOAD-001".to_vec()
            }
            _ => return -(ErrNo::Eacces.to_i32() as i64),
        }
    };
    let n = data.len().min(READ_MAX);
    let out = &mut block[BLK_OUT..BLK_OUT + 4 + READ_MAX];
    out[0..4].copy_from_slice(&(n as u32).to_le_bytes());
    out[4..4 + n].copy_from_slice(&data[..n]);
    n as i64
}

/// boot://event 回放：14 阶段快照（idx/state=2 done/ms）。count+16B 记录。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn shim_boot_events(block: &mut [u8; BLK_TOTAL]) -> i64 {
    const STAGES: [crate::timeline::Stage; 14] = [
        crate::timeline::Stage::Serial,
        crate::timeline::Stage::Cmdline,
        crate::timeline::Stage::Framebuffer,
        crate::timeline::Stage::Logo,
        crate::timeline::Stage::Banner,
        crate::timeline::Stage::Console,
        crate::timeline::Stage::Platform,
        crate::timeline::Stage::Acpi,
        crate::timeline::Stage::Smios,
        crate::timeline::Stage::Memmap,
        crate::timeline::Stage::Kaslr,
        crate::timeline::Stage::Integrity,
        crate::timeline::Stage::BootOpt,
        crate::timeline::Stage::SelfTest,
    ];
    let tsc_hz = crate::platform::info()
        .map(|p| p.tsc_hz)
        .unwrap_or(crate::platform::FALLBACK_TSC_HZ);
    let tl = crate::timeline::timeline();
    let mut count = 0u32;
    let mut off = 4usize;
    for st in STAGES.iter() {
        let ms = crate::timeline::ticks_to_ms(tl.stage_ticks(*st), tsc_hz) as u32;
        let mut rec = [0u8; 16];
        encode_boot_record(st.index() as u8, 2, ms, &mut rec);
        block[off..off + 16].copy_from_slice(&rec);
        off += 16;
        count += 1;
    }
    // 第 15 条（idx=14，2026-09-20 实机取证）：键盘诊断 + 活时钟。
    // [4..8]=活时钟 ms（BootScreen 超时倒计时源；total_ticks 冻结值不可用）、
    // [8..12]=kbd 诊断字（低 8 位控制器探针、[23:8] 已收键盘原始字节计数、
    // [31:24] 最后原始字节）。实机判读：按了键 raw 不涨 ⇒ 键盘信号没到
    // 控制器（内建键盘很可能走 USB）；raw 涨了没出键 ⇒ 扫描码解码问题。
    let mut rec = [0u8; 16];
    encode_boot_record(14, 2, live_ms(), &mut rec);
    rec[8..12].copy_from_slice(&crate::inputsvc::target::kbd_diag_word().to_le_bytes());
    block[off..off + 16].copy_from_slice(&rec);
    count += 1;
    block[0..4].copy_from_slice(&count.to_le_bytes());
    count as i64
}

/// 开机毫秒（boot://event 时钟源）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn boot_ms() -> u64 {
    let tsc_hz = crate::platform::info()
        .map(|p| p.tsc_hz)
        .unwrap_or(crate::platform::FALLBACK_TSC_HZ);
    crate::timeline::ticks_to_ms(crate::timeline::timeline().total_ticks(), tsc_hz)
}

/// 活时钟毫秒（2026-09-20）：`total_ticks` 是各阶段时长的冻结和（演示屏
/// "boot completed N ms" 同源），不能驱动倒计时——BootScreen 超时倒计时
/// 用当前 TSC 换算，随诊断记录（idx=14）每次 CMD_BOOT_EVENTS 实时带回。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn live_ms() -> u32 {
    let tsc_hz = crate::platform::info()
        .map(|p| p.tsc_hz)
        .unwrap_or(crate::platform::FALLBACK_TSC_HZ);
    crate::timeline::ticks_to_ms(crate::timeline::read_tsc(), tsc_hz) as u32
}

/// 无堆 u64 → 十进制（演示树条目尺寸）。
fn format_u64(mut v: u64) -> alloc::vec::Vec<u8> {
    if v == 0 {
        return alloc::vec![b'0'];
    }
    let mut buf = alloc::vec::Vec::with_capacity(20);
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    buf
}

// ---------------------------------------------------------------------------
// 宿主测试：编解码/校验/降级树纯逻辑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrip() {
        let a1 = pack_a1(FRAME_TEXT, 4, 200, 2);
        assert_eq!(a1 & 0xFF, FRAME_TEXT);
        assert_eq!((a1 >> 8) & 0xFF, 4);
        assert_eq!((a1 >> 16) & 0xFF, 200);
        assert_eq!((a1 >> 28) & 0xF, 2);
        let packed = pack_xywh(100, 200, 640, 480);
        assert_eq!(unpack_xywh(packed), (100, 200, 640, 480));
        // 16 位槽位：65535 不失真，65536 截断（调用方语义层校验兜底）。
        assert_eq!(unpack_xywh(pack_xywh(65535, 0, 0, 0)).0, 65535);
    }

    #[test]
    fn palette_out_of_range_is_black_no_panic() {
        assert_eq!(palette(0), PALETTE[0]);
        assert_eq!(palette(15), PALETTE[15]);
        assert_eq!(palette(16), PALETTE[0]);
        assert_eq!(palette(u64::MAX), PALETTE[0]);
    }

    #[test]
    fn rect_plausible_boundaries() {
        assert!(rect_plausible(0, 0, 100, 50, 1280, 800));
        assert!(!rect_plausible(0, 0, 0, 50, 1280, 800), "零宽拒绝");
        assert!(!rect_plausible(1300, 0, 10, 10, 1280, 800), "完全越界拒绝");
        assert!(rect_plausible(1270, 0, 20, 10, 1280, 800), "部分相交放行（绘制层裁剪）");
        assert!(!rect_plausible(-50, 0, 10, 10, 1280, 800));
    }

    #[test]
    fn shim_block_min_table() {
        assert_eq!(shim_block_min(SHIM_KV_GET), Some(BLK_OUT + 4 + VAL_MAX));
        assert_eq!(shim_block_min(SHIM_KV_SET), Some(BLK_VAL + VAL_MAX));
        assert_eq!(shim_block_min(99), None, "未知命令 = MISSING");
        assert!(shim_block_min(SHIM_VFS_READ).unwrap() >= BLK_OUT + 4 + READ_MAX);
    }

    #[test]
    fn zstr_rejects_unterminated_and_oob() {
        let mut b = [0u8; 64];
        b[0..5].copy_from_slice(b"hello");
        assert_eq!(zstr(&b, 0, 16), Ok(&b"hello"[..]));
        b[16..32].fill(b'x'); // 槽内无终止符
        assert!(zstr(&b, 16, 16).is_err());
        assert!(zstr(&b, 60, 16).is_err(), "越槽 = EINVAL");
    }

    #[test]
    fn boot_record_layout() {
        let mut r = [0u8; 16];
        encode_boot_record(7, 2, 1234, &mut r);
        assert_eq!(r[0], 7);
        assert_eq!(r[1], 2);
        assert_eq!(&r[4..8], &1234u32.to_le_bytes());
        assert!(r[2..4].iter().all(|&b| b == 0) && r[8..].iter().all(|&b| b == 0));
    }

    #[test]
    fn format_u64_decimal() {
        assert_eq!(format_u64(0), b"0".to_vec());
        assert_eq!(format_u64(322), b"322".to_vec());
        assert_eq!(format_u64(1_048_576), b"1048576".to_vec());
    }

    #[test]
    fn stable_numbers_do_not_clash_win32() {
        // 稳定号 16/17/18/19/20 与 Win32 服务台号段（0x40 起）永不相交。
        assert!(SYS_FRAME < super::super::winapi::WIN32_NR_BASE);
        assert!(SYS_SHIM < super::super::winapi::WIN32_NR_BASE);
        assert!(SYS_REBOOT < super::super::winapi::WIN32_NR_BASE);
        assert!(SYS_POWEROFF < super::super::winapi::WIN32_NR_BASE);
        assert!(SYS_WIN < super::super::winapi::WIN32_NR_BASE);
    }

    // ---------------- SYS_WIN 编解码纯函数（AI-4 · S2.06/S2.09） ----------------

    #[test]
    fn win_pack_xy_roundtrip_with_negative() {
        // 负坐标（出屏窗口）有符号还原。
        assert_eq!(win_unpack_xy(win_pack_xy(0, 0)), (0, 0));
        assert_eq!(win_unpack_xy(win_pack_xy(1920, 1080)), (1920, 1080));
        assert_eq!(win_unpack_xy(win_pack_xy(-4, -2)), (-4, -2));
        assert_eq!(win_unpack_xy(win_pack_xy(i32::MIN as i64, i32::MAX as i64)), (i32::MIN as i64, i32::MAX as i64));
    }

    #[test]
    fn win_pack_wh_roundtrip() {
        assert_eq!(win_unpack_wh(win_pack_wh(8, 6)), (8, 6));
        assert_eq!(win_unpack_wh(win_pack_wh(2560, 1440)), (2560, 1440));
        assert_eq!(win_unpack_wh(win_pack_wh(u32::MAX as u64, 0)), (u32::MAX, 0));
    }

    #[test]
    fn win_submit_hdr_validation() {
        // kind=FULL 恒 count=0；kind=DIRTY 1..=16；未知 kind 拒绝。
        assert!(win_submit_hdr_ok(WIN_SUBMIT_KIND_FULL, 0));
        assert!(!win_submit_hdr_ok(WIN_SUBMIT_KIND_FULL, 1));
        assert!(win_submit_hdr_ok(WIN_SUBMIT_KIND_DIRTY, 1));
        assert!(win_submit_hdr_ok(WIN_SUBMIT_KIND_DIRTY, WIN_SUBMIT_MAX_RECTS));
        assert!(!win_submit_hdr_ok(WIN_SUBMIT_KIND_DIRTY, 0));
        assert!(!win_submit_hdr_ok(WIN_SUBMIT_KIND_DIRTY, WIN_SUBMIT_MAX_RECTS + 1));
        assert!(!win_submit_hdr_ok(99, 1));
    }
}
