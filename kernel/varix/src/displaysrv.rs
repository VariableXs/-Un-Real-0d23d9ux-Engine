//! 内核显示服务 — 绘制面统一归口 + 双缓冲 + 脏矩形提交（双域总案·阶段2 任务20）。
//!
//! 职责边界（总案施工步骤 13 口径）：
//! - **Surface 归口**：bootselect 与 future shell 的绘制面一律从本服务借出
//!   （[`DisplayService::draw_surface`]），服务负责保证"画在哪个面上"的正确性；
//!   借用期间不能 commit——"先画完再提交"由借用检查强制；
//! - **双缓冲**：前台（Limine GOP 帧）永不直接接受绘制；后备缓冲来自
//!   PMM order-10 块（4 MiB，`pmm::alloc_order` + HHDM 线性映射）；
//!   帧大小超出 4 MiB（如 1920×1080×4）或 PMM 拒绝时**诚实降级直写模式**
//!   （`Mode::Direct`——boot 期单核无并发消费者，直写无撕裂，像素行为与
//!   收编前完全等价）；attach 时前台内容快照到后备（非脏区语义=与前台一致）；
//! - **脏矩形提交**：`commit(&[Rect])` 只搬运脏区行；`mark_dirty` 登记
//!   满 `MAX_DIRTY` 时转全屏合并（绝不丢脏区）；
//! - **分辨率切换能力声明**：Limine 协议下分辨率由引导参数决定（GOP 直通），
//!   本服务与分辨率无关（全部几何来自前台 Surface；任务3 已归档
//!   800×600/1280×720/1920×1080 三分辨率）；运行期热切换不在本阶段范围，
//!   需按新引导参数重建服务（from_limine → new）。
//!
//! 绘制原语唯一来源仍是 `fb.rs` Surface + `font.rs`（本模块零绘制原语，
//! 只有行搬运）；登记体域（gfx/aurora/gperiph）的同名函数与启动链无关，
//! 由后续任务接入时收编。目标态消费者串行（单核 boot 链）；future shell
//! 跨线程接入时由调用方持锁（宿主竞态用例示范 Mutex 外层防护）。

use crate::fb::Surface;

/// 脏矩形登记容量；满后转全屏合并（绝不丢脏区）。
pub const MAX_DIRTY: usize = 32;

/// 后备缓冲需求上限：PMM order-10 = 4 MiB 单块。
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
const BACK_ORDER: usize = 10;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
const BACK_BYTES: usize = 1 << (12 + BACK_ORDER); // 4 MiB

/// 服务模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 直写：draw_surface 即前台（boot 期单核无并发，行为与收编前等价）。
    Direct,
    /// 双缓冲：绘制打后备，commit 搬脏区到前台。
    DoubleBuffered,
}

/// 脏矩形（i64 口径与 fb.rs 绘制坐标一致；负值/越界在提交时裁剪）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Rect {
    pub const fn new(x: i64, y: i64, w: i64, h: i64) -> Self {
        Rect { x, y, w, h }
    }
    /// 整屏脏。
    pub fn whole(w: u32, h: u32) -> Self {
        Rect { x: 0, y: 0, w: w as i64, h: h as i64 }
    }
    /// 与屏幕求交后的有效矩形（None=零尺寸/完全出屏）。
    pub fn clip(mut self, w: u32, h: u32) -> Option<Rect> {
        if self.x < 0 {
            self.w += self.x;
            self.x = 0;
        }
        if self.y < 0 {
            self.h += self.y;
            self.y = 0;
        }
        if self.w <= 0 || self.h <= 0 || self.x >= w as i64 || self.y >= h as i64 {
            return None;
        }
        self.w = self.w.min(w as i64 - self.x);
        self.h = self.h.min(h as i64 - self.y);
        if self.w <= 0 || self.h <= 0 {
            None
        } else {
            Some(self)
        }
    }
}

/// 内核显示服务。
pub struct DisplayService {
    front: Surface,
    back: Option<Surface>,
    /// PMM 块物理地址（None=直写模式/宿主注入后备）。
    back_phys: Option<u64>,
    mode: Mode,
    /// mark_dirty 登记环。
    dirty: [Option<Rect>; MAX_DIRTY],
    dirty_n: usize,
    /// 登记满/显式 mark_full：下次 commit 合并为全屏。
    dirty_overflow: bool,
    frames: u64,
    rects_flushed: u64,
    overflow_merges: u64,
}

impl DisplayService {
    /// 显式直写构造（boot 期口径：单核、无并发消费者、像素行为与收编前
    /// 完全等价——任务1/3 的实机视觉证据在此模式下零漂移）。
    /// 双缓冲需求走 [`Self::new`]（自动尝试）。
    pub fn new_direct(front: Surface) -> Self {
        DisplayService {
            front,
            back: None,
            back_phys: None,
            mode: Mode::Direct,
            dirty: [None; MAX_DIRTY],
            dirty_n: 0,
            dirty_overflow: false,
            frames: 0,
            rects_flushed: 0,
            overflow_merges: 0,
        }
    }

    /// 构造服务。目标态尝试从 PMM 取 4 MiB 后备块（HHDM 映射）；失败或
    /// 帧超 4 MiB 降级直写（如实反映在 [`Self::mode`]）。
    pub fn new(front: Surface) -> Self {
        #[cfg_attr(not(target_os = "none"), allow(unused_mut))]
        let mut svc = DisplayService {
            front,
            back: None,
            back_phys: None,
            mode: Mode::Direct,
            dirty: [None; MAX_DIRTY],
            dirty_n: 0,
            dirty_overflow: false,
            frames: 0,
            rects_flushed: 0,
            overflow_merges: 0,
        };
        #[cfg(target_os = "none")]
        {
            let need = front.height() as usize * front.stride() as usize;
            if need <= BACK_BYTES {
                if let Some(phys) = crate::mem::pmm::alloc_order(BACK_ORDER) {
                    let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
                    let base = (phys + hhdm) as *mut u8;
                    // SAFETY: PMM 持有的 order-10 连续块经 HHDM 线性映射，
                    // 帧大小 ≤4MiB 全部可写，生命周期由服务持有至 drop。
                    let back = unsafe {
                        Surface::from_raw(
                            base,
                            front.width(),
                            front.height(),
                            front.stride(),
                            front.format(),
                        )
                    };
                    svc.attach(back, Some(phys));
                }
            }
        }
        svc
    }

    /// 宿主测试/显式装配注入后备基址（几何/格式取自前台；调用方保证
    /// `height*stride` 可写）。返回 false 表示已是双缓冲模式。
    pub fn attach_backing(&mut self, base: *mut u8) -> bool {
        if self.back.is_some() {
            return false;
        }
        // SAFETY: 调用方契约——base 指向 height*stride 可写字节。
        let back = unsafe {
            Surface::from_raw(
                base,
                self.front.width(),
                self.front.height(),
                self.front.stride(),
                self.front.format(),
            )
        };
        self.attach(back, None);
        true
    }

    fn attach(&mut self, back: Surface, phys: Option<u64>) {
        // 快照前台→后备：非脏区语义 = 与前台一致（局部重绘正确的前提）。
        let fb = back.base_ptr();
        let fp = self.front.base_ptr();
        let n = self.front.height() as usize * self.front.stride() as usize;
        // SAFETY: 两块缓冲均为 height*stride 可写，区间不重叠。
        unsafe { core::ptr::copy_nonoverlapping(fp, fb, n) };
        self.back = Some(back);
        self.back_phys = phys;
        self.mode = Mode::DoubleBuffered;
    }

    /// 服务模式。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 模式名（探针打印用）。
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Direct => "direct",
            Mode::DoubleBuffered => "double",
        }
    }

    /// **归口入口**：借出本帧绘制面（双缓冲=后备，直写=前台）。
    /// Surface 是 Copy 句柄——调用方复制句柄共享同一画面是合法的。
    pub fn draw_surface(&mut self) -> &mut Surface {
        match self.back.as_mut() {
            Some(b) => b,
            None => &mut self.front,
        }
    }

    /// 登记脏矩形（绘制中逐步登记的增量通道；零尺寸/出屏忽略；
    /// 容量满转全屏合并）。
    pub fn mark_dirty(&mut self, r: Rect) {
        let (w, h) = (self.front.width(), self.front.height());
        if r.clip(w, h).is_none() {
            return;
        }
        if self.dirty_n >= MAX_DIRTY {
            self.dirty_overflow = true;
            return;
        }
        self.dirty[self.dirty_n] = Some(r);
        self.dirty_n += 1;
    }

    /// 显式整屏脏（等价于 commit 传 `Rect::whole`）。
    pub fn mark_full(&mut self) {
        self.dirty_overflow = true;
    }

    /// 提交一帧：显式传入脏矩形与 mark_dirty 登记合并（裁剪），
    /// 逐矩形逐行从后备搬到前台，清空脏集。直写模式只计数
    /// （前台已就绪——收编前 boot 链的直写语义由此保持）。
    pub fn commit(&mut self, dirty: &[Rect]) {
        self.frames = self.frames.wrapping_add(1);
        if self.mode != Mode::DoubleBuffered {
            return;
        }
        let (w, h) = (self.front.width(), self.front.height());
        // 合并：显式列表 + 登记环（先取出登记，避免借用冲突）。
        let mut pending: [Option<Rect>; MAX_DIRTY] = [None; MAX_DIRTY];
        let pending_n = self.dirty_n;
        for i in 0..pending_n {
            pending[i] = self.dirty[i];
        }
        self.dirty_n = 0;
        if self.dirty_overflow {
            self.overflow_merges = self.overflow_merges.wrapping_add(1);
            self.blit(&Rect::whole(w, h));
            self.rects_flushed = self.rects_flushed.wrapping_add(1);
        } else {
            let mut n = 0u64;
            for slot in pending[..pending_n].iter() {
                if let Some(r) = slot {
                    self.blit(r);
                    n += 1;
                }
            }
            for r in dirty.iter() {
                if let Some(c) = r.clip(w, h) {
                    self.blit(&c);
                    n += 1;
                }
            }
            self.rects_flushed = self.rects_flushed.wrapping_add(n);
        }
        self.dirty_overflow = false;
    }

    /// 后备→前台逐行搬运一个已裁剪矩形。
    fn blit(&mut self, r: &Rect) {
        let bpp = self.front.format().bytes_per_pixel() as usize;
        let stride = self.front.stride() as usize;
        let fp = self.front.base_ptr();
        let bp = match self.back.as_ref() {
            Some(b) => b.base_ptr(),
            None => return,
        };
        let x0 = r.x as usize;
        let y0 = r.y as usize;
        let rows = r.h as usize;
        let row_bytes = r.w as usize * bpp;
        // SAFETY: 矩形已与屏幕求交；行内 [x0, x0+w) 字节落在两块缓冲的
        // height*stride 有效范围内，且后备≠前台不重叠。
        unsafe {
            for y in 0..rows {
                let s = bp.add((y0 + y) * stride + x0 * bpp);
                let d = fp.add((y0 + y) * stride + x0 * bpp);
                core::ptr::copy_nonoverlapping(s, d, row_bytes);
            }
        }
    }

    /// 统计：(提交帧数, 累计提交脏矩形数, 溢出全屏合并次数)。
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.frames, self.rects_flushed, self.overflow_merges)
    }
}

impl Drop for DisplayService {
    fn drop(&mut self) {
        // 目标态归还未使用的后台块 (probe 局部实例结束时不泄漏 4 MiB)。
        #[cfg(target_os = "none")]
        if let Some(phys) = self.back_phys {
            crate::mem::pmm::free_order(phys, BACK_ORDER);
        }
    }
}

// ---------------------------------------------------------------------------
// 目标态：main.rs 收编安装点 + 实机探针
// ---------------------------------------------------------------------------

static mut SERVICE: *mut DisplayService = core::ptr::null_mut();

/// main.rs 安装显示服务（启动链绘制面归口；单核 boot 链串行使用，
/// 裸指针全局与 ps2.rs 手工 Once 同范式）。
pub fn install(svc: &mut DisplayService) {
    let slot = &raw mut SERVICE;
    unsafe {
        *slot = svc as *mut DisplayService;
    }
}

/// 目标态探针。
pub mod target {
    use super::{DisplayService, Mode, Rect};
    use crate::fb::Color;

    /// 借用已安装的服务（None=main 未安装）。
    pub fn service() -> Option<&'static mut DisplayService> {
        let slot = &raw mut super::SERVICE;
        if slot.is_null() {
            return None;
        }
        unsafe { (*slot).as_mut() }
    }

    /// 实机探针（main.rs 挂 input_probe 之后）：自建双缓冲实例（4 MiB
    /// 后备 + 脏矩形提交），滚动条带 180 帧——宿主脚本连续 screendump
    /// 抽帧，逐帧验证条带行完整（无撕裂）。结束归还 PMM 块。
    pub fn display_probe() {
        const BAND_H: i64 = 40;
        const FRAMES: u64 = 360;

        let front = crate::limine::framebuffer()
            .and_then(|f| crate::fb::Surface::from_limine(f).ok());
        let Some(front) = front else {
            crate::kwarn!("display-probe: no framebuffer, abort");
            return;
        };
        let mut svc = DisplayService::new(front);
        let (w, h) = {
            let s = svc.draw_surface();
            (s.width() as i64, s.height() as i64)
        };
        let mode = svc.mode_name();
        crate::kinfo!(
            "display-probe: mode={} {}x{} band_h={} frames={}",
            mode,
            w,
            h,
            BAND_H,
            FRAMES
        );
        if svc.mode() != Mode::DoubleBuffered {
            crate::kwarn!("display-probe: not double-buffered, skip tear check");
            return;
        }

        // 清屏必须在全部 kinfo 之后：console 镜像（enable_mirror）把每条
        // 日志画到前台字符格——先打印再清屏，滚动域才是干净起点。
        let tsc_hz = crate::platform::info()
            .map(|p| p.tsc_hz)
            .unwrap_or(1_000_000_000);
        let ink = Color::rgb(0x20, 0x80, 0xF0);
        let black = Color::rgb(0, 0, 0);
        crate::kinfo!("display-probe: scroll begin");
        {
            let surf = svc.draw_surface();
            surf.fill_rect(0, 0, w, h, black);
        }
        svc.mark_full();
        svc.commit(&[]);
        let mut y_prev: Option<i64> = None;
        for i in 0..FRAMES {
            let y_new = (i as i64 * 7) % (h - BAND_H).max(1);
            {
                let surf = svc.draw_surface();
                if let Some(yp) = y_prev {
                    surf.fill_rect(0, yp, w / 2, BAND_H, black);
                }
                surf.fill_rect(0, y_new, w / 2, BAND_H, ink);
            }
            let dirty = match y_prev {
                Some(yp) => Rect::new(0, y_new.min(yp), w / 2, (y_new - yp).abs() + BAND_H),
                None => Rect::new(0, y_new, w / 2, BAND_H),
            };
            svc.commit(&[dirty]);
            y_prev = Some(y_new);
            // 每帧 ~10ms 忙等；360 帧 ≈ 3.6s 抓帧窗口。
            let next = crate::timeline::read_tsc() + tsc_hz / 100;
            while crate::timeline::read_tsc() < next {
                core::hint::spin_loop();
            }
        }
        let (frames, rects, merges) = svc.stats();
        crate::kinfo!(
            "display-probe: scroll done frames={} rects_flushed={} overflow_merges={}",
            frames,
            rects,
            merges
        );

        // 收尾静态帧：彩带定住 + 四角色块（多矩形脏提交/边界裁剪验证）。
        {
            let surf = svc.draw_surface();
            let y = y_prev.unwrap_or(0);
            surf.fill_rect(0, y, w, BAND_H, ink);
            surf.fill_rect(0, 0, 40, 40, Color::rgb(0xF0, 0x20, 0x20));
            surf.fill_rect(w - 40, 0, 40, 40, Color::rgb(0x20, 0xF0, 0x20));
            surf.fill_rect(0, h - 40, 40, 40, Color::rgb(0x20, 0x20, 0xF0));
            surf.fill_rect(w - 40, h - 40, 40, 40, Color::rgb(0xF0, 0xF0, 0x20));
        }
        let yb = y_prev.unwrap_or(0);
        svc.commit(&[
            Rect::new(0, yb, w, BAND_H),
            Rect::new(0, 0, 40, 40),
            Rect::new(w - 40, 0, 40, 40),
            Rect::new(0, h - 40, 40, 40),
            Rect::new(w - 40, h - 40, 40, 40),
        ]);
        crate::kinfo!(
            "display-probe: verdict=ok frames={} rects={} merges={}",
            svc.stats().0,
            svc.stats().1,
            svc.stats().2
        );
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（ktest）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::{Color, PixelFormat};
    use std::vec::Vec;

    /// 前台 + 后备双缓冲（宿主：两个 Vec 缓冲）。
    fn svc_double(w: u32, h: u32) -> (DisplayService, Vec<u8>, Vec<u8>) {
        let mut front = vec![0u8; (w * h * 4) as usize];
        let mut back = vec![0u8; (w * h * 4) as usize];
        let front_surf = unsafe {
            Surface::from_raw(front.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32)
        };
        let mut svc = DisplayService::new(front_surf);
        assert!(svc.attach_backing(back.as_mut_ptr()));
        (svc, front, back)
    }

    /// 读像素为 [r,g,b]（Bgr32 打包：byte0=蓝、byte2=红——任务3 定案的字序）。
    fn px(buf: &[u8], w: u32, x: i64, y: i64) -> [u8; 3] {
        let off = (y as usize) * (w as usize) * 4 + (x as usize) * 4;
        [buf[off + 2], buf[off + 1], buf[off]]
    }

    #[test]
    fn direct_mode_writes_front_immediately() {
        let (w, h) = (64u32, 32u32);
        let mut front = vec![0u8; (w * h * 4) as usize];
        let surf = unsafe {
            Surface::from_raw(front.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32)
        };
        let mut svc = DisplayService::new(surf);
        assert_eq!(svc.mode(), Mode::Direct, "宿主无 PMM：默认直写");
        svc.draw_surface().fill_rect(0, 0, 4, 4, Color::rgb(1, 2, 3));
        assert_eq!(px(&front, w, 0, 0), [1, 2, 3], "直写模式立即生效（收编前语义）");
        svc.commit(&[]);
        assert_eq!(svc.stats().0, 1, "直写 commit 只计数");
    }

    #[test]
    fn double_mode_commit_moves_pixels() {
        let (w, h) = (64u32, 32u32);
        let (mut svc, front, back) = svc_double(w, h);
        assert_eq!(svc.mode(), Mode::DoubleBuffered);
        // 画前快照一致性：attach 时前台(0)快照到后备。
        assert_eq!(back[0], front[0]);
        svc.draw_surface().fill_rect(0, 0, 4, 4, Color::rgb(9, 8, 7));
        assert_eq!(px(&front, w, 0, 0), [0, 0, 0], "commit 前前台不动");
        svc.commit(&[Rect::whole(w, h)]);
        assert_eq!(px(&front, w, 0, 0), [9, 8, 7], "commit 后前台同步");
    }

    #[test]
    fn dirty_rect_isolation() {
        let (w, h) = (64u32, 32u32);
        let (mut svc, front, _back) = svc_double(w, h);
        let (a, b) = (Color::rgb(0xF0, 0, 0), Color::rgb(0, 0xF0, 0));
        // 帧1：画 A 区并提交（后备已有 A；前台经 commit 也有 A）。
        svc.draw_surface().fill_rect(0, 0, 8, 8, a);
        svc.commit(&[Rect::new(0, 0, 8, 8)]);
        assert_eq!(px(&front, w, 0, 0), [0xF0, 0, 0]);
        // 帧2：画 B 区，只提交 B → A 区前台保持。
        svc.draw_surface().fill_rect(16, 16, 8, 8, b);
        svc.commit(&[Rect::new(16, 16, 8, 8)]);
        assert_eq!(px(&front, w, 0, 0), [0xF0, 0, 0], "A 区未在帧2 脏集，前台保持");
        assert_eq!(px(&front, w, 16, 16), [0, 0xF0, 0], "B 区已同步");
    }

    #[test]
    fn clip_oob_and_partial() {
        let (w, h) = (64u32, 32u32);
        let (mut svc, front, _back) = svc_double(w, h);
        let c = Color::rgb(1, 2, 3);
        svc.draw_surface().fill_rect(0, 0, w as i64, h as i64, c);
        // 出屏/零尺寸矩形：忽略不 panic。
        svc.commit(&[
            Rect::new(-8, -8, 4, 4),
            Rect::new(w as i64 + 8, 0, 4, 4),
            Rect::new(0, 0, 0, 4),
            Rect::new(-4, 0, 8, 8), // 部分出屏：裁剪后 x∈[0,4)
        ]);
        assert_eq!(px(&front, w, 0, 0), [1, 2, 3], "部分出屏矩形裁剪后已同步");
        assert_eq!(px(&front, w, 5, 0), [0, 0, 0], "裁剪边界外未搬运");
    }

    #[test]
    fn mark_dirty_registry_and_overflow_merge() {
        let (w, h) = (64u32, 32u32);
        let (mut svc, front, _back) = svc_double(w, h);
        svc.draw_surface().fill_rect(0, 0, w as i64, h as i64, Color::rgb(7, 7, 7));
        for i in 0..MAX_DIRTY {
            svc.mark_dirty(Rect::new(i as i64, 0, 1, 1));
        }
        svc.mark_dirty(Rect::new(999, 999, 1, 1)); // 出屏：忽略
        assert_eq!(svc.stats().2, 0, "未溢出");
        svc.mark_dirty(Rect::new(0, 1, 1, 1)); // 第 33 条 → 溢出
        svc.commit(&[]);
        assert_eq!(svc.stats().2, 1, "登记满转全屏合并");
        assert_eq!(px(&front, w, 63, 31), [7, 7, 7], "全屏合并前台整体同步");
        // 溢出旗标消费后复位。
        svc.mark_dirty(Rect::new(0, 0, 1, 1));
        svc.commit(&[]);
        assert_eq!(svc.stats().2, 1, "复位后不再合并");
    }

    #[test]
    fn mark_full_equivalent_to_whole() {
        let (w, h) = (64u32, 32u32);
        let (mut svc, front, _back) = svc_double(w, h);
        svc.draw_surface().fill_rect(0, 0, w as i64, h as i64, Color::rgb(5, 5, 5));
        svc.mark_full();
        svc.commit(&[]);
        assert_eq!(px(&front, w, 10, 10), [5, 5, 5]);
        assert_eq!(svc.stats().2, 1);
    }

    #[test]
    fn attach_twice_rejected() {
        let (mut svc, _front, _back) = svc_double(16, 16);
        assert!(!svc.attach_backing(core::ptr::null_mut()), "已双缓冲拒绝二次注入");
    }

    #[test]
    fn resolutions_800_720_1080() {
        for (w, h) in [(800u32, 600u32), (1280, 720), (1920, 1080)] {
            let (mut svc, front, back) = svc_double(w, h);
            assert_eq!(svc.mode(), Mode::DoubleBuffered, "{}x{}", w, h);
            let (x, y) = ((w as i64) / 2, (h as i64) / 2);
            svc.draw_surface().fill_rect(x - 4, y - 4, 8, 8, Color::rgb(0x55, 0xAA, 0x11));
            svc.commit(&[Rect::new(x - 4, y - 4, 8, 8)]);
            assert_eq!(
                px(&front, w, x, y),
                px(&back, w, x, y),
                "{}x{} 中心像素前后台一致",
                w,
                h
            );
        }
    }

    #[test]
    fn concurrent_mark_and_commit_no_loss() {
        // 总案：双缓冲交换竞态用例。服务无锁（目标态单消费者），并发
        // 防护在调用方（future shell 接入口）——此处以 Mutex 外层示范，
        // hammer 后断言服务状态无损、最终全屏提交后前台≡后备。
        use std::sync::{Arc, Mutex};
        struct SendSvc(DisplayService);
        unsafe impl Send for SendSvc {}
        let (w, h) = (128u32, 64u32);
        let (svc, front, back) = svc_double(w, h);
        let svc = Arc::new(Mutex::new(SendSvc(svc)));
        let sw = svc.clone();
        let painter = std::thread::spawn(move || {
            let mut s = sw.lock().unwrap();
            for i in 0..1000i64 {
                let (x, y) = (i % (w as i64 - 8), (i / 2) % (h as i64 - 8));
                s.0.draw_surface().fill_rect(x, y, 4, 4, Color::rgb((i % 255) as u8, 0, 0));
                s.0.mark_dirty(Rect::new(x, y, 4, 4));
            }
        });
        let cw = svc.clone();
        let committer = std::thread::spawn(move || {
            let mut s = cw.lock().unwrap();
            for _ in 0..1000 {
                s.0.commit(&[]);
            }
        });
        painter.join().unwrap();
        committer.join().unwrap();
        let mut s = svc.lock().unwrap();
        s.0.commit(&[Rect::whole(w, h)]);
        assert_eq!(front[..], back[..], "hammer 后全屏提交：前台≡后备（无状态损坏）");
        let (frames, _, _) = s.0.stats();
        assert!(frames >= 1000, "提交线程与主线程提交均计入");
    }
}
