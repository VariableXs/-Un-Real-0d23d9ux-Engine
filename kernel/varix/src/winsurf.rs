//! 窗口面服务 — S2.06 渲染通路 + S2.09 多窗合成的内核侧地基（AI-4 · 三体第 2 批）。
//!
//! 职责边界（十二步施工序 6/9 口径，《AI 分工完成图》AI-4 辖区）：
//! - **客户端提交模型（copy-in 定版）**：客户端（ushell / 未来的 Servo 进程）
//!   经 `SYS_WIN`(21) 注册窗口面并按行提交像素（uaccess 拷贝进内核窗口面）；
//!   唯一写入路径 = [`WinService::stage_row`]，零旁路直写帧缓冲。协议预留
//!   共享缓冲升级位（[`CAP_SHARED_BUF`]，v1 恒 false——升级时客户端零语义
//!   变化，只是 submit 的数据源从"系统调用参数"变为"已映射缓冲"）；
//! - **合成**：[`WinService::composite`] 按 z 序把可见窗提交到显示服务
//!   （[`crate::displaysrv`]）：双缓冲模式走脏区增量（窗口脏区平移到屏幕
//!   坐标级联 mark_dirty/commit）；直写模式（>4 MiB 帧，如 1080p）全窗全量
//!   重绘（无持久后备面可增量，与 bootselect 直写语义一致）；
//! - **最小化保活**：`Minimized` 窗口缓冲与几何保留、跳过合成
//!   （vwm `display:none` 的裸机等价物；restore 后内容原样可见）；
//! - **焦点**：[`WinService::focus`] 记录焦点窗口；键盘收键方由
//!   [`crate::inputsvc`] 焦点路由按窗口属主定版（跨层映射在 sys 层），
//!   鼠标事件广播不变（前端按窗口几何命中）；
//! - **缓冲**：PMM order-10 块链（单窗 ≤[`MAX_BLOCKS`] 块 = 32 MiB，覆盖
//!   2560×1440×4 ≈ 14 MiB），行粒度寻址不跨块（单行 ≤ 16 KiB ≪ 块 4 MiB）。
//!
//! 戒律对齐：零堆（全部定容数组）；>64 KB 结构禁栈上（本模块最大结构
//! `WindowSlot` ≈ 400 B，全局单例与 displaysrv::install 同范式）；合成行
//! 拷贝与 displaysrv::blit 同纪律；宿主可测面 = 全部几何/裁剪/合成逻辑
//! 纯函数（缓冲注入），真 PMM 分配 `target_os = "none"` 编译。

use crate::displaysrv::{DisplayService, Mode, Rect};
use crate::fb::PixelFormat;

/// 窗口槽上限。第 9 个 register 明确拒绝（绝不静默挤占）。
pub const MAX_WINDOWS: usize = 8;
/// 单窗缓冲块链上限（8 × 4 MiB = 32 MiB）。
pub const MAX_BLOCKS: usize = 8;
/// 缓冲块 order（= [`crate::mem::pmm::MAX_ORDER`]）。
pub const BLOCK_ORDER: usize = 10;
/// 缓冲块字节数 = 页(4KiB) × 2^order = 4 MiB。
/// （戒律注：order 是页框粒度——1<<order 是**页数**，字节数须乘 4096。）
pub const BLOCK_BYTES: usize = 4 * 1024 * 1024;
/// 每次提交的脏矩形上限（一次 submit 一帧增量；满转全窗，绝不丢脏区）。
pub const MAX_DIRTY_PER_SUBMIT: usize = 16;
/// 窗口几何单边上限；面积上限在 register 按块链容量校验。
pub const MAX_WIN_EDGE: u32 = 4096;

/// 协议能力位：共享缓冲升级（v1 未启用——copy-in 定版，见模块头注释）。
pub const CAP_SHARED_BUF: bool = false;

/// 窗口状态：可见 / 最小化保活（缓冲保留，跳过合成）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinState {
    Visible,
    Minimized,
}

/// 窗口槽：注册即占位，unregister 回收。
struct WindowSlot {
    live: bool,
    wid: u16,
    owner_pid: u32,
    /// 屏幕绝对坐标（可负——窗口可部分出屏，裁剪在 blit）。
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    /// z 序令牌：越大越顶层。raise 重新盖章；新窗取下一令牌（置顶）。
    z_gen: u64,
    minimized: bool,
    fmt: Option<PixelFormat>,
    /// 块链物理地址（宿主注入为 None——只影响 Drop 归还）。
    block_phys: [Option<u64>; MAX_BLOCKS],
    block_ptrs: [*mut u8; MAX_BLOCKS],
    blocks_used: usize,
    /// 行主序打包字节宽（w × bpp；行寻址基准）。
    row_bytes: usize,
    /// 提交世代（每次 end_submit +1；探针/诊断用）。
    generation: u32,
    /// 待合成脏区（窗口坐标；composite 消费后清空）。
    dirty: [Option<Rect>; MAX_DIRTY_PER_SUBMIT],
    dirty_n: usize,
    dirty_overflow: bool,
}

impl WindowSlot {
    const fn empty() -> Self {
        WindowSlot {
            live: false,
            wid: 0,
            owner_pid: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            z_gen: 0,
            minimized: false,
            fmt: None,
            block_phys: [None; MAX_BLOCKS],
            block_ptrs: [core::ptr::null_mut(); MAX_BLOCKS],
            blocks_used: 0,
            row_bytes: 0,
            generation: 0,
            dirty: [None; MAX_DIRTY_PER_SUBMIT],
            dirty_n: 0,
            dirty_overflow: false,
        }
    }
}

/// 合成统计：(帧数, blit 行数, 最小化跳过, 直写全量重绘, format 失配跳过)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompositeStats {
    pub frames: u64,
    pub blit_rows: u64,
    pub minimize_skips: u64,
    pub full_redraws: u64,
    pub fmt_mismatch_skips: u64,
}

/// 窗口面服务。全局单例经 [`target::install`]；消费者串行（轮询内核
/// 单核 boot 链口径，与 displaysrv 相同；跨线程接入由调用方持锁）。
pub struct WinService {
    slots: [WindowSlot; MAX_WINDOWS],
    next_wid: u16,
    z_clock: u64,
    focus_wid: Option<u16>,
    /// 宿主注入模式（真 PMM 分配停用——ktest 确定性）。
    host_injected: bool,
    stats: CompositeStats,
    submit_rows: u64,
    hit_tests: u64,
}

impl WinService {
    pub fn new() -> Self {
        WinService {
            slots: [
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
                WindowSlot::empty(),
            ],
            next_wid: 1,
            z_clock: 0,
            focus_wid: None,
            host_injected: false,
            stats: CompositeStats::default(),
            submit_rows: 0,
            hit_tests: 0,
        }
    }

    /// 宿主测试模式：register 不做 PMM 分配（缓冲由测试 [`Self::attach_block`] 注入）。
    pub fn new_host() -> Self {
        let mut s = WinService::new();
        s.host_injected = true;
        s
    }

    /// 注册窗口：分配块链（目标态）或等待注入（宿主）。返回 wid（自 1 单调）。
    ///
    /// 拒绝条件（全部显式，绝不静默降级）：槽满 / 几何越界 / 面积超块链
    /// 容量 / 行宽超块容量 / 目标态 PMM 分配失败（已分块回滚）。
    pub fn register(&mut self, owner_pid: u32, w: u32, h: u32, fmt: PixelFormat) -> Option<u16> {
        if w == 0 || h == 0 || w > MAX_WIN_EDGE || h > MAX_WIN_EDGE {
            return None;
        }
        let slot = self.slots.iter().position(|s| !s.live)?;
        let row_bytes = w as usize * fmt.bytes_per_pixel() as usize;
        if row_bytes == 0 || row_bytes > BLOCK_BYTES {
            return None; // 行宽超过块容量——诚实拒绝
        }
        let block_rows = BLOCK_BYTES / row_bytes;
        let need_blocks = h as usize / block_rows + usize::from(h as usize % block_rows != 0);
        if need_blocks > MAX_BLOCKS {
            return None;
        }
        let wid = self.next_wid;
        self.next_wid = self.next_wid.wrapping_add(1).max(1); // 0 保留（focus 清除语义）
        self.z_clock += 1;
        let s = &mut self.slots[slot];
        s.live = true;
        s.wid = wid;
        s.owner_pid = owner_pid;
        s.x = 0;
        s.y = 0;
        s.w = w;
        s.h = h;
        s.z_gen = self.z_clock;
        s.minimized = false;
        s.fmt = Some(fmt);
        s.row_bytes = row_bytes;
        s.generation = 0;
        s.dirty_n = 0;
        s.dirty_overflow = false;
        if self.host_injected {
            s.blocks_used = 0; // 宿主：attach_block 按序注入
        } else {
            #[cfg(target_os = "none")]
            {
                let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
                for i in 0..need_blocks {
                    let Some(phys) = crate::mem::pmm::alloc_order(BLOCK_ORDER) else {
                        // 分配失败：回滚已分块，整窗拒绝（不留半成品）。
                        for j in 0..i {
                            if let Some(p) = s.block_phys[j].take() {
                                crate::mem::pmm::free_order(p, BLOCK_ORDER);
                                s.block_ptrs[j] = core::ptr::null_mut();
                            }
                        }
                        s.blocks_used = 0;
                        s.live = false;
                        return None;
                    };
                    // SAFETY: PMM 持有的 order-10 连续块经 HHDM 线性映射可写，
                    // 生命周期由窗口槽持有至 unregister/Drop 归还。
                    s.block_ptrs[i] = unsafe { (phys + hhdm) as *mut u8 };
                    s.block_phys[i] = Some(phys);
                    crate::kinfo!("win-probe: pmm block {} phys={:#x}", i, phys);
                }
                s.blocks_used = need_blocks;
            }
            #[cfg(not(target_os = "none"))]
            {
                // 宿主非注入模式（ktest 全走 new_host）：拒绝以免假成功。
                s.live = false;
                return None;
            }
        }
        Some(wid)
    }

    /// 宿主测试注入第 `idx` 块缓冲（调用方保证块容量可写）。仅 host 模式可用；
    /// 必须按序注入（idx == 已注入块数）。
    pub fn attach_block(&mut self, wid: u16, idx: usize, base: *mut u8) -> bool {
        if !self.host_injected {
            return false;
        }
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        if idx >= MAX_BLOCKS || s.blocks_used != idx {
            return false;
        }
        s.block_ptrs[idx] = base;
        s.blocks_used = idx + 1;
        true
    }

    /// 注销窗口：归还块链（目标态），焦点联动清空。
    pub fn unregister(&mut self, wid: u16) -> bool {
        let Some(slot) = self.slots.iter().position(|s| s.live && s.wid == wid) else {
            return false;
        };
        #[cfg(target_os = "none")]
        for i in 0..self.slots[slot].blocks_used {
            if let Some(p) = self.slots[slot].block_phys[i].take() {
                crate::mem::pmm::free_order(p, BLOCK_ORDER);
            }
        }
        self.slots[slot] = WindowSlot::empty();
        if self.focus_wid == Some(wid) {
            self.focus_wid = None;
        }
        true
    }

    /// 设置窗口位置（屏幕绝对坐标；可负）。尺寸 v1 不可变（resize = 重新 register）。
    pub fn set_geo(&mut self, wid: u16, x: i64, y: i64) -> bool {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        s.x = x;
        s.y = y;
        true
    }

    /// 置顶（z 序重新盖章）。
    pub fn raise(&mut self, wid: u16) -> bool {
        self.z_clock += 1;
        let z = self.z_clock;
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        s.z_gen = z;
        true
    }

    /// 最小化 / 恢复（保活语义：缓冲与几何保留）。
    pub fn set_state(&mut self, wid: u16, st: WinState) -> bool {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        s.minimized = st == WinState::Minimized;
        true
    }

    /// 焦点窗口（收键归属的内核侧记录；wid=0 清除）。
    pub fn focus(&mut self, wid: u16) -> bool {
        if wid == 0 {
            self.focus_wid = None;
            return true;
        }
        if self.slot_by_wid(wid).is_none() {
            return false;
        }
        self.focus_wid = Some(wid);
        true
    }

    pub fn focus_wid(&self) -> Option<u16> {
        self.focus_wid
    }

    /// 焦点窗口属主 pid（sys 层映射到 inputsvc 订阅者）。
    pub fn focus_owner(&self) -> Option<u32> {
        let wid = self.focus_wid?;
        self.slot_by_wid(wid).map(|s| s.owner_pid)
    }

    /// 提交一行像素（窗口坐标；data = 从 (x0, y) 起的行主序字节，
    /// 长度须为 bpp 整倍且不超行宽）。行寻址不跨块：整行落单块。
    pub fn stage_row(&mut self, wid: u16, y: i64, x0: u32, data: &[u8]) -> bool {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        if y < 0 || y >= s.h as i64 || s.blocks_used == 0 {
            return false;
        }
        let bpp = s.fmt.map(|f| f.bytes_per_pixel() as usize).unwrap_or(4);
        if x0 as usize + data.len() > s.row_bytes || data.len() % bpp != 0 {
            return false;
        }
        let stride = s.row_bytes;
        let block_rows = BLOCK_BYTES / stride;
        let block_idx = y as usize / block_rows;
        let in_row = y as usize % block_rows;
        if block_idx >= s.blocks_used {
            return false;
        }
        let base = s.block_ptrs[block_idx];
        if base.is_null() {
            return false;
        }
        // SAFETY: 块容量 4 MiB；in_row*stride + x0*bpp + data.len() ≤ h*stride ≤ blocks_used*4MiB。
        unsafe {
            let dst = base.add(in_row * stride + x0 as usize * bpp);
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        self.submit_rows += 1;
        true
    }

    /// 登记窗口脏区（窗口坐标；零尺寸忽略；满转全窗）。
    pub fn mark_window_dirty(&mut self, wid: u16, r: Rect) -> bool {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        if r.w == 0 || r.h == 0 {
            return true;
        }
        if s.dirty_n >= MAX_DIRTY_PER_SUBMIT {
            s.dirty_overflow = true;
            return true;
        }
        s.dirty[s.dirty_n] = Some(r);
        s.dirty_n += 1;
        true
    }

    /// 结束一次提交（世代 +1；脏区保留至 composite 消费）。
    pub fn end_submit(&mut self, wid: u16) -> bool {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return false;
        };
        s.generation = s.generation.wrapping_add(1);
        true
    }

    /// 命中测试：可见窗按 z 从高到低，返回第一个含屏幕点的 wid。
    pub fn hit_test(&mut self, sx: i64, sy: i64) -> Option<u16> {
        self.hit_tests += 1;
        let mut best: Option<(u64, u16)> = None;
        for s in self.slots.iter().filter(|s| s.live && !s.minimized) {
            if sx >= s.x && sx < s.x + s.w as i64 && sy >= s.y && sy < s.y + s.h as i64 {
                if best.map(|(z, _)| s.z_gen > z).unwrap_or(true) {
                    best = Some((s.z_gen, s.wid));
                }
            }
        }
        best.map(|(_, wid)| wid)
    }

    /// 合成一帧：按 z 从低到高把可见窗提交到显示服务。
    ///
    /// 双缓冲：消费各窗脏区（平移+裁剪到窗几何∩屏幕），行拷贝到
    /// `disp.draw_surface()`，级联 `mark_dirty` + `commit(&[])`；脏区溢出
    /// 转全窗。直写模式：全窗全量重绘 + `mark_full`（无持久后备面）。
    /// 窗口 format 与屏幕不一致的窗跳过并计数（绝不猜格式写屏）。
    pub fn composite(&mut self, disp: &mut DisplayService) {
        self.stats.frames += 1;
        let direct = disp.mode() == Mode::Direct;
        let (sw, sh, screen_fmt) = {
            let s = disp.draw_surface();
            (s.width(), s.height(), s.format())
        };

        // z 从低到高（稳定：z_gen 升序，同令牌按槽序）。最小化窗也入列——
        // 合成循环里跳过并计数（保活语义的可见证据）。
        let mut order: [usize; MAX_WINDOWS] = [0; MAX_WINDOWS];
        let mut n = 0;
        for (i, s) in self.slots.iter().enumerate() {
            if s.live {
                order[n] = i;
                n += 1;
            }
        }
        order[..n].sort_by_key(|&i| self.slots[i].z_gen);

        for &i in order[..n].iter() {
            if self.slots[i].minimized {
                self.stats.minimize_skips += 1;
                continue;
            }
            let (wid, fmt) = (self.slots[i].wid, self.slots[i].fmt);
            if fmt != Some(screen_fmt) {
                self.stats.fmt_mismatch_skips += 1;
                continue;
            }
            let (wx, wy, ww, wh) = (self.slots[i].x, self.slots[i].y, self.slots[i].w, self.slots[i].h);

            // 待 blit 区列表：双缓冲=脏区增量（溢出转全窗）；直写=全窗。
            let mut regions: [Option<Rect>; MAX_DIRTY_PER_SUBMIT] = [None; MAX_DIRTY_PER_SUBMIT];
            let mut rn = 0usize;
            let mut full = false;
            if direct {
                full = true;
            } else {
                let (dirty, overflow) = self.take_dirty(wid);
                if overflow {
                    full = true;
                } else {
                    for r in dirty.iter().flatten() {
                        if rn >= MAX_DIRTY_PER_SUBMIT {
                            full = true;
                            break;
                        }
                        regions[rn] = Some(*r);
                        rn += 1;
                    }
                }
            }

            if full {
                self.stats.full_redraws += 1;
                if let Some((cx0, cx1, ry0, rows)) = clip_window_region(wx, wy, ww, wh, 0, 0, ww as i64, wh as i64, sw as i64, sh as i64) {
                    self.blit_rows(disp, wid, cx0, cx1, ry0, rows);
                    disp.mark_dirty(Rect::new(wx + cx0 as i64, wy + ry0, (cx1 - cx0) as i64, rows));
                }
                if direct {
                    disp.mark_full();
                }
                continue;
            }

            for r in regions[..rn].iter().flatten() {
                let Some((cx0, cx1, ry0, rows)) =
                    clip_window_region(wx, wy, ww, wh, r.x, r.y, r.w, r.h, sw as i64, sh as i64)
                else {
                    continue;
                };
                self.blit_rows(disp, wid, cx0, cx1, ry0, rows);
                disp.mark_dirty(Rect::new(wx + cx0 as i64, wy + ry0, (cx1 - cx0) as i64, rows));
            }
        }
        disp.commit(&[]);
    }

    fn take_dirty(&mut self, wid: u16) -> ([Option<Rect>; MAX_DIRTY_PER_SUBMIT], bool) {
        let Some(s) = self.slot_by_wid_mut(wid) else {
            return ([None; MAX_DIRTY_PER_SUBMIT], false);
        };
        let mut out = [None; MAX_DIRTY_PER_SUBMIT];
        for i in 0..s.dirty_n {
            out[i] = s.dirty[i];
        }
        let ov = s.dirty_overflow;
        s.dirty_n = 0;
        s.dirty_overflow = false;
        (out, ov)
    }

    /// 窗口面列区间 [cx0, cx1) × 行区间 [ry0, ry0+rows) → 显示服务
    /// （窗口坐标；调用方已完成屏幕裁剪）。行粒度块寻址，逐行拷贝。
    fn blit_rows(&mut self, disp: &mut DisplayService, wid: u16, cx0: usize, cx1: usize, ry0: i64, rows: i64) {
        if rows <= 0 || cx1 <= cx0 {
            return;
        }
        let Some(s) = self.slot_by_wid(wid) else {
            return;
        };
        let stride = s.row_bytes;
        let block_rows = BLOCK_BYTES / stride;
        let (wx, wy) = (s.x, s.y);
        let bpp = s.fmt.map(|f| f.bytes_per_pixel() as usize).unwrap_or(4);
        let col_bytes = (cx1 - cx0) * bpp;
        let dst_stride = disp.draw_surface().stride() as usize;
        for r in 0..rows {
            let y = ry0 + r;
            let block_idx = y as usize / block_rows;
            let in_row = y as usize % block_rows;
            let Some(slot) = self.slot_by_wid(wid) else { return };
            if block_idx >= slot.blocks_used {
                continue;
            }
            let base = slot.block_ptrs[block_idx];
            if base.is_null() {
                continue;
            }
            // SAFETY: 源行在块容量内（block_rows*stride = 4MiB 精确整除）；
            // 目标行/列已由 clip_window_region 裁剪进屏幕几何（先 i64 加法
            // 再转 usize——负窗位直接 as usize 会环绕溢出）。
            unsafe {
                let src = base.add(in_row * stride + cx0 * bpp);
                let dst_row = (wy + y) as usize * dst_stride;
                let dst_col = (wx + cx0 as i64) as usize * bpp;
                let dst = disp.draw_surface().base_ptr().add(dst_row + dst_col);
                core::ptr::copy_nonoverlapping(src, dst, col_bytes);
            }
            self.stats.blit_rows += 1;
        }
    }

    /// 槽内查找（不可变）。
    fn slot_by_wid(&self, wid: u16) -> Option<&WindowSlot> {
        self.slots.iter().find(|s| s.live && s.wid == wid)
    }

    /// 槽内查找（可变）。
    fn slot_by_wid_mut(&mut self, wid: u16) -> Option<&mut WindowSlot> {
        self.slots.iter_mut().find(|s| s.live && s.wid == wid)
    }

    /// 合成统计快照。
    pub fn stats(&self) -> CompositeStats {
        self.stats
    }

    /// 提交行数累计（sys 探针用）。
    pub fn submit_rows(&self) -> u64 {
        self.submit_rows
    }

    /// 命中测试次数（探针用）。
    pub fn hit_tests(&self) -> u64 {
        self.hit_tests
    }
}

impl Drop for WinService {
    fn drop(&mut self) {
        #[cfg(target_os = "none")]
        for s in self.slots.iter_mut() {
            if s.live {
                for i in 0..s.blocks_used {
                    if let Some(p) = s.block_phys[i].take() {
                        crate::mem::pmm::free_order(p, BLOCK_ORDER);
                    }
                }
            }
        }
    }
}

/// 脏区（窗口坐标 x,y,w,h）→ 可 blit 窗口列区间与行区间：
/// 与窗几何 [0,w)×[0,h) 和屏幕 [0,sw)×[0,sh)（经窗口偏移 wx,wy）三方求交。
/// 返回 `(cx0, cx1, ry0, rows)`（窗口坐标列区间 / 行起点与行数）；空 = None。
fn clip_window_region(
    wx: i64,
    wy: i64,
    ww: u32,
    wh: u32,
    rx: i64,
    ry: i64,
    rw: i64,
    rh: i64,
    sw: i64,
    sh: i64,
) -> Option<(usize, usize, i64, i64)> {
    // 列：[rx, rx+rw) ∩ [0, ww) 平移屏幕后 ∩ [−wx, sw−wx)
    let cx0 = rx.max(0).max(-wx).max(0);
    let cx1 = (rx + rw).min(ww as i64).min(sw - wx);
    if cx0 >= cx1 || cx0 < 0 {
        return None;
    }
    // 行：[ry, ry+rh) ∩ [0, wh) 平移屏幕后 ∩ [−wy, sh−wy)
    let ry0 = ry.max(0).max(-wy).max(0);
    let ry1 = (ry + rh).min(wh as i64).min(sh - wy);
    if ry0 >= ry1 || ry0 < 0 {
        return None;
    }
    Some((cx0 as usize, cx1 as usize, ry0, ry1 - ry0))
}

/// 窗口查询信息（SYS_WIN WIN_QUERY 返回源；字段与 S2.09 支持矩阵对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinInfo {
    pub owner_pid: u32,
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    /// 窗口面字节/像素（sys 层像素拷贝宽度依据）。
    pub bpp: usize,
    pub minimized: bool,
    pub is_focus: bool,
    pub generation: u32,
}

impl WinService {
    /// 窗口查询（SYS_WIN WIN_QUERY；None=wid 不存在）。
    pub fn window_info(&self, wid: u16) -> Option<WinInfo> {
        let s = self.slot_by_wid(wid)?;
        Some(WinInfo {
            owner_pid: s.owner_pid,
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
            bpp: s.fmt.map(|f| f.bytes_per_pixel() as usize).unwrap_or(0),
            minimized: s.minimized,
            is_focus: self.focus_wid == Some(wid),
            generation: s.generation,
        })
    }
}

// ---------------------------------------------------------------------------
// 目标态：全局单例 + 探针（displaysrv 同范式）
// ---------------------------------------------------------------------------

static mut SERVICE: *mut WinService = core::ptr::null_mut();

/// main.rs 安装窗口面服务（启动链收编；单核串行口径）。
pub fn install(svc: &mut WinService) {
    let slot = &raw mut SERVICE;
    unsafe {
        *slot = svc as *mut WinService;
    }
}

/// 借用已安装的服务（None=main 未安装）。
pub fn service() -> Option<&'static mut WinService> {
    let slot = &raw mut SERVICE;
    if slot.is_null() {
        return None;
    }
    unsafe { (*slot).as_mut() }
}

/// 实机探针（main.rs 挂 display_probe 之后）：独立实例真实走 PMM 块链 →
/// 注册 160×90 测试窗 → 行提交渐变 → 合成 3 帧 → 打印统计 → 归还。
/// 宿主脚本 serial 断言 `win-probe:` 各行出现且统计非零。
#[cfg(target_os = "none")]
pub fn win_probe() {
    let Some(disp) = crate::displaysrv::target::service() else {
        crate::kwarn!("win-probe: display service not installed, abort");
        return;
    };
    let (sw, sh, fmt) = {
        let s = disp.draw_surface();
        (s.width(), s.height(), s.format())
    };
    crate::kinfo!("win-probe: screen {}x{}", sw, sh);
    crate::kinfo!("win-probe: t1 new begin");
    let mut svc = WinService::new();
    crate::kinfo!("win-probe: t2 new ok — register begin (PMM order-10)");
    let (w, h) = (160u32, 90u32);
    let Some(wid) = svc.register(0x5749_4E31, w, h, fmt) else {
        crate::kwarn!("win-probe: register failed, abort");
        return;
    };
    crate::kinfo!("win-probe: t3 register ok wid={}", wid);
    svc.set_geo(wid, (sw as i64 - w as i64) / 2, (sh as i64 - h as i64) / 2);
    crate::kinfo!("win-probe: t4 geo ok");
    let mut row = [0u8; 160 * 4];
    for y in 0..h as i64 {
        if y % 30 == 0 {
            crate::kinfo!("win-probe: t5 stage y={}", y);
        }
        for x in 0..w as usize {
            row[x * 4] = (x * 255 / w as usize) as u8; // B（Bgr32 布局）
            row[x * 4 + 1] = (y as usize * 255 / h as usize) as u8; // G
            row[x * 4 + 2] = 0x40; // R
            row[x * 4 + 3] = 0xFF;
        }
        if !svc.stage_row(wid, y, 0, &row) {
            crate::kwarn!("win-probe: stage_row({}) failed", y);
            break;
        }
    }
    svc.mark_window_dirty(wid, Rect::new(0, 0, w as i64, h as i64));
    svc.end_submit(wid);
    crate::kinfo!("win-probe: t6 stage+submit done — composite1 begin");
    svc.composite(disp);
    crate::kinfo!("win-probe: t7 composite1 done");
    svc.composite(disp);
    crate::kinfo!("win-probe: t8 composite2 done");
    svc.composite(disp);
    crate::kinfo!("win-probe: t9 composite3 done");
    let st = svc.stats();
    crate::kinfo!(
        "win-probe: frames={} blit_rows={} full_redraws={} submit_rows={}",
        st.frames,
        st.blit_rows,
        st.full_redraws,
        svc.submit_rows()
    );
    if !svc.unregister(wid) {
        crate::kwarn!("win-probe: unregister failed");
        return;
    }
    crate::kinfo!("win-probe: PASS");
}

// ---------------------------------------------------------------------------
// 宿主测试（ktest 单跑判绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::Surface;

    const W: u32 = 8;
    const H: u32 = 6;
    const BPP: usize = 4;
    const SW: u32 = 32;
    const SH: u32 = 24;

    /// 泄漏一块测试缓冲（ktest 进程生命周期 = 测试生命周期，无回收需求）。
    fn leak_buf(rows: usize, stride: usize) -> *mut u8 {
        let v = vec![0u8; rows * stride].into_boxed_slice();
        Box::leak(v).as_mut_ptr()
    }

    /// 装一个 8×6 宿主窗口（1 块注入），返回 (svc, wid)。
    fn mk_window() -> (WinService, u16) {
        let mut svc = WinService::new_host();
        let wid = svc
            .register(0x1234, W, H, PixelFormat::Bgr32)
            .expect("register");
        assert!(svc.attach_block(wid, 0, leak_buf(H as usize, W as usize * BPP)));
        (svc, wid)
    }

    /// 建宿主屏幕面（32×24 Bgr32，1 块注入显示服务双缓冲）。
    fn mk_display() -> DisplayService {
        // SAFETY: 两块 leak 缓冲均 SH*stride 可写，生命周期 = 测试进程。
        let front = unsafe {
            Surface::from_raw(leak_buf(SH as usize, SW as usize * BPP), SW, SH, SW * BPP as u32, PixelFormat::Bgr32)
        };
        let mut d = DisplayService::new_direct(front);
        assert!(d.attach_backing(leak_buf(SH as usize, SW as usize * BPP)));
        d
    }

    /// 生成一条测试行：像素 i = 0x01000000 * (base + i)（Bgr32 内存字节序 [b,g,r,x]）。
    fn test_row(base: u8) -> Vec<u8> {
        let mut v = Vec::with_capacity(W as usize * BPP);
        for i in 0..W {
            let px = (base as u32).wrapping_add(i as u32);
            v.extend_from_slice(&px.to_le_bytes());
        }
        v
    }

    #[test]
    fn register_lifecycle_and_rejects() {
        let mut svc = WinService::new_host();
        // 几何拒绝。
        assert!(svc.register(1, 0, 10, PixelFormat::Bgr32).is_none());
        assert!(svc.register(1, 5000, 10, PixelFormat::Bgr32).is_none());
        // 正常注册 → wid 自增。
        let w1 = svc.register(1, W, H, PixelFormat::Bgr32).unwrap();
        let w2 = svc.register(2, W, H, PixelFormat::Bgr32).unwrap();
        assert_eq!(w2, w1 + 1);
        // 宿主模式必须显式注入缓冲后才可提交。
        assert!(svc.attach_block(w1, 0, leak_buf(H as usize, W as usize * BPP)));
        assert!(!svc.attach_block(w1, 0, leak_buf(H as usize, W as usize * BPP)), "重复注入拒绝");
        assert!(!svc.attach_block(w1, 2, leak_buf(H as usize, W as usize * BPP)), "跳序注入拒绝");
        // 未注入缓冲的 w2 提交拒绝。
        assert!(!svc.stage_row(w2, 0, 0, &test_row(1)));
        // 注销 → 焦点联动清空。
        assert!(svc.focus(w1));
        assert_eq!(svc.focus_wid(), Some(w1));
        assert!(svc.unregister(w1));
        assert_eq!(svc.focus_wid(), None);
        assert!(!svc.unregister(w1), "重复注销拒绝");
    }

    #[test]
    fn stage_row_bounds_and_content() {
        let (mut svc, wid) = mk_window();
        let row = test_row(7);
        assert!(svc.stage_row(wid, 0, 0, &row));
        assert!(!svc.stage_row(wid, -1, 0, &row), "负行拒绝");
        assert!(!svc.stage_row(wid, H as i64, 0, &row), "越下界拒绝");
        assert!(!svc.stage_row(wid, 0, 1, &row), "x0+行宽超窗拒绝");
        let mut short = row.clone();
        short.pop();
        assert!(!svc.stage_row(wid, 1, 0, &short[..(W as usize * BPP - 1)]), "非 bpp 整倍拒绝");
        // 回读验证落盘内容（不信回执信回读）。
        let s = svc.slot_by_wid(wid).unwrap();
        let base = s.block_ptrs[0];
        for i in 0..W as usize {
            let got = unsafe { core::ptr::read_unaligned(base.add(i * BPP) as *const u32) };
            assert_eq!(got, (7u32 + i as u32).to_le());
        }
    }

    #[test]
    fn composite_single_window_fullscreen_pixel_exact() {
        // S2.06 单窗全屏基准：整窗提交 → 合成 → 屏幕逐像素对照。
        let (mut svc, wid) = mk_window();
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(10 + y as u8)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        let mut disp = mk_display();
        svc.composite(&mut disp);
        // 屏幕前 8×6 = 窗口内容逐像素一致（原始 u32 = test_row 的 LE 字节）；
        // 窗外区域保持 0（双缓冲快照语义）。
        let front = disp.draw_surface();
        for y in 0..H {
            for x in 0..W {
                let got = front.get_px(x as i64, y as i64).unwrap();
                let want = (10u32 + y + x).to_le();
                assert_eq!(got, want, "pixel ({x},{y}) mismatch");
            }
        }
        assert_eq!(front.get_px(W as i64, 0), Some(0));
        assert_eq!(front.get_px(0, H as i64), Some(0));
        let st = svc.stats();
        assert_eq!(st.frames, 1);
        assert_eq!(st.blit_rows, H as u64);
    }

    #[test]
    fn composite_two_windows_zorder_overlap() {
        // S2.09 多窗 Z 序：低窗红底、高窗蓝底，重叠区=高窗。
        let mut svc = WinService::new_host();
        let wa = svc.register(1, W, H, PixelFormat::Bgr32).unwrap();
        let wb = svc.register(2, W, H, PixelFormat::Bgr32).unwrap();
        assert!(svc.attach_block(wa, 0, leak_buf(H as usize, W as usize * BPP)));
        assert!(svc.attach_block(wb, 0, leak_buf(H as usize, W as usize * BPP)));
        svc.set_geo(wa, 0, 0);
        svc.set_geo(wb, 4, 2); // 与 wa 重叠 4×4
        for y in 0..H as i64 {
            assert!(svc.stage_row(wa, y, 0, &vec![0xFFu8; W as usize * BPP])); // 蓝（整行）
            // 红整行：每像素字节 [00 00 FF FF]（Bgr32 内存序 R=0xFF）。
            let mut red_row = vec![0u8; W as usize * BPP];
            for px in red_row.chunks_mut(BPP) {
                px.copy_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
            }
            assert!(svc.stage_row(wb, y, 0, &red_row));
        }
        svc.mark_window_dirty(wa, Rect::new(0, 0, W as i64, H as i64));
        svc.mark_window_dirty(wb, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wa);
        svc.end_submit(wb);
        let mut disp = mk_display();
        svc.composite(&mut disp);
        // 重叠区 (4..8, 2..6) = wb 红高窗；wa 独占区 (0..4, 0..2) = 蓝。
        // get_px 返回内存原始 u32（LE）：wa 全 FF 字节 = 0xFFFF_FFFF；
        // wb 红 = 字节 [00 00 FF FF] LE 读 = 0xFFFF_0000。
        assert_eq!(disp.draw_surface().get_px(1, 1).unwrap(), 0xFFFF_FFFF, "wa 独占区=蓝");
        assert_eq!(disp.draw_surface().get_px(5, 3).unwrap(), 0xFFFF_0000, "重叠区=wb 红（wb 高）");
        assert_eq!(disp.draw_surface().get_px(6, 4).unwrap(), 0xFFFF_0000, "重叠区=红（wb 高）");
        // raise wa → wa 置顶，重叠区变蓝。
        assert!(svc.raise(wa));
        svc.mark_window_dirty(wa, Rect::new(0, 0, W as i64, H as i64));
        svc.mark_window_dirty(wb, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wa);
        svc.end_submit(wb);
        svc.composite(&mut disp);
        assert_eq!(disp.draw_surface().get_px(5, 3).unwrap(), 0xFFFF_FFFF, "raise 后重叠区=蓝（wa 高）");
    }

    #[test]
    fn minimize_keeps_alive_and_restore() {
        // S2.09 最小化保活：缓冲保留、跳过合成；restore 后原样可见。
        let (mut svc, wid) = mk_window();
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(42)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        let mut disp = mk_display();
        svc.composite(&mut disp);
        assert_eq!(disp.draw_surface().get_px(0, 0).unwrap(), 42u32.to_le());
        // 最小化 → 清屏区重合成：窗内容不出现（跳过），但缓冲未动。
        assert!(svc.set_state(wid, WinState::Minimized));
        // 用第二窗盖满屏幕模拟"屏幕归他人"。
        let w2 = svc.register(2, SW, SH, PixelFormat::Bgr32).unwrap();
        assert!(svc.attach_block(w2, 0, leak_buf(SH as usize, SW as usize * BPP)));
        for y in 0..SH as i64 {
            assert!(svc.stage_row(w2, y, 0, &vec![0x11u8; SW as usize * BPP]));
        }
        svc.mark_window_dirty(w2, Rect::new(0, 0, SW as i64, SH as i64));
        svc.end_submit(w2);
        svc.composite(&mut disp);
        let st = svc.stats();
        assert_eq!(st.minimize_skips, 1, "最小化窗跳过合成");
        // restore → 再合成，窗内容原样回来（保活证据）。
        assert!(svc.set_state(wid, WinState::Visible));
        assert!(svc.raise(wid));
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        svc.composite(&mut disp);
        assert_eq!(disp.draw_surface().get_px(0, 0).unwrap(), 42u32.to_le(), "restore 后内容原样");
    }

    #[test]
    fn dirty_rect_incremental_only_blits_dirty_rows() {
        // 脏矩形增量：只提交 2 行脏区 → blit_rows 恰好 2。
        let (mut svc, wid) = mk_window();
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(1)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 2, W as i64, 2));
        svc.end_submit(wid);
        let mut disp = mk_display();
        svc.composite(&mut disp);
        let st = svc.stats();
        assert_eq!(st.blit_rows, 2, "只搬脏行");
        assert_eq!(st.full_redraws, 0);
        // 溢出转全窗：17 个脏区 > MAX_DIRTY_PER_SUBMIT(16)。
        for i in 0..(MAX_DIRTY_PER_SUBMIT + 1) as i64 {
            svc.mark_window_dirty(wid, Rect::new(0, i, 1, 1));
        }
        svc.end_submit(wid);
        svc.composite(&mut disp);
        let st = svc.stats();
        assert_eq!(st.full_redraws, 1, "溢出转全窗");
        assert_eq!(st.blit_rows, 2 + H as u64);
    }

    #[test]
    fn offscreen_clip_negative_and_overflow() {
        // 出屏裁剪：负坐标 + 超右下边界。
        let (mut svc, wid) = mk_window();
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(9)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        svc.set_geo(wid, -4, -2); // 左上出屏
        let mut disp = mk_display();
        svc.composite(&mut disp);
        // 可见部分 (0..4, 0..4) = 窗口 (4..8, 2..6)（test_row(9) 像素 = 9+x）。
        assert_eq!(disp.draw_surface().get_px(0, 0).unwrap(), (9u32 + 4).to_le(), "裁剪后可见区首像素=窗口(4,2)");
        assert_eq!(disp.draw_surface().get_px(3, 3).unwrap(), (9u32 + 7).to_le(), "窗口(7,5)=9+7");
        assert_eq!(disp.draw_surface().get_px(4, 0), Some(0), "窗右界外不受影响");
        // 超右下。
        svc.set_geo(wid, SW as i64 - 3, SH as i64 - 2);
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        svc.composite(&mut disp);
        assert_eq!(
            disp.draw_surface().get_px(SW as i64 - 1, SH as i64 - 1).unwrap(),
            (9u32 + 2).to_le(),
            "右下裁剪落点"
        );
    }

    #[test]
    fn hit_test_zorder_and_focus_owner() {
        let mut svc = WinService::new_host();
        let wa = svc.register(0xAA, W, H, PixelFormat::Bgr32).unwrap();
        let wb = svc.register(0xBB, W, H, PixelFormat::Bgr32).unwrap();
        assert!(svc.attach_block(wa, 0, leak_buf(H as usize, W as usize * BPP)));
        assert!(svc.attach_block(wb, 0, leak_buf(H as usize, W as usize * BPP)));
        svc.set_geo(wa, 0, 0);
        svc.set_geo(wb, 2, 2); // 重叠区命中 wb（后注册=z 高）
        assert_eq!(svc.hit_test(1, 1), Some(wa));
        assert_eq!(svc.hit_test(3, 3), Some(wb));
        assert_eq!(svc.hit_test(100, 100), None);
        // raise wa → 重叠区命中翻转。
        svc.raise(wa);
        assert_eq!(svc.hit_test(3, 3), Some(wa));
        // 焦点属主映射（sys 层键盘路由依据）。
        assert!(svc.focus(wb));
        assert_eq!(svc.focus_owner(), Some(0xBB));
        assert!(!svc.focus(999), "焦点指向不存在窗口拒绝");
        assert!(svc.focus(0));
        assert_eq!(svc.focus_owner(), None);
        // 最小化窗不参与命中（点 (1,1) 只在 wa 内 → wa 隐身后无命中）。
        svc.set_state(wa, WinState::Minimized);
        assert_eq!(svc.hit_test(1, 1), None, "最小化窗不可命中且不误命中他人");
        assert_eq!(svc.hit_test(3, 3), Some(wb), "wb 区域命中不受影响");
        assert_eq!(svc.hit_tests(), 6);
    }

    #[test]
    fn capacity_eighth_window_rejected() {
        let mut svc = WinService::new_host();
        let mut last = 0;
        for i in 0..MAX_WINDOWS {
            let wid = svc
                .register(i as u32, 4, 4, PixelFormat::Bgr32)
                .unwrap_or_else(|| panic!("window {i} should register"));
            assert!(svc.attach_block(wid, 0, leak_buf(4, 4 * BPP)));
            last = wid;
        }
        assert!(svc.register(99, 4, 4, PixelFormat::Bgr32).is_none(), "第 9 窗明确拒绝");
        assert!(svc.unregister(last));
        assert!(svc.register(99, 4, 4, PixelFormat::Bgr32).is_some(), "回收后可再注册");
    }

    #[test]
    fn direct_mode_full_redraw() {
        // 直写模式（>4MiB 帧降级形态）：全窗全量重绘 + mark_full。
        let (mut svc, wid) = mk_window();
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(3)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        // SAFETY: leak 缓冲 SH*stride 可写，生命周期 = 测试进程。
        let front = unsafe {
            Surface::from_raw(leak_buf(SH as usize, SW as usize * BPP), SW, SH, SW * BPP as u32, PixelFormat::Bgr32)
        };
        let mut disp = DisplayService::new_direct(front);
        svc.composite(&mut disp);
        let st = svc.stats();
        assert_eq!(st.full_redraws, 1, "直写=全量重绘");
        assert_eq!(front.get_px(0, 0).unwrap(), 3u32.to_le());
    }

    #[test]
    fn fmt_mismatch_skipped_honestly() {
        // 窗口 format 与屏幕不一致 → 跳过并计数，绝不猜格式写屏。
        let mut svc = WinService::new_host();
        let wid = svc.register(1, W, H, PixelFormat::Rgb32).unwrap(); // 屏幕=Bgr32
        assert!(svc.attach_block(wid, 0, leak_buf(H as usize, W as usize * BPP)));
        for y in 0..H as i64 {
            assert!(svc.stage_row(wid, y, 0, &test_row(5)));
        }
        svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
        svc.end_submit(wid);
        let mut disp = mk_display();
        svc.composite(&mut disp);
        assert_eq!(svc.stats().fmt_mismatch_skips, 1);
        assert_eq!(disp.draw_surface().get_px(0, 0), Some(0), "失配窗零写入");
    }

    #[test]
    fn window_toggle_100_no_leak() {
        // S2.09 验收口径「窗口开关 ×100 无泄漏」内核侧水位：100 轮
        // register+submit+composite+unregister 后槽位全空、账本守恒。
        let mut svc = WinService::new_host();
        for round in 0..100u32 {
            let wid = svc
                .register(0x1000 + round as u32, 8, 6, PixelFormat::Bgr32)
                .unwrap_or_else(|| panic!("round {round}: register"));
            assert!(svc.attach_block(wid, 0, leak_buf(H as usize, W as usize * BPP)));
            for y in 0..H as i64 {
                assert!(svc.stage_row(wid, y, 0, &test_row(1)));
            }
            svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
            assert!(svc.end_submit(wid));
            let mut disp = mk_display();
            svc.composite(&mut disp);
            assert!(svc.unregister(wid), "round {round}: unregister");
            assert_eq!(svc.focus_wid(), None);
            // 水位：合成帧计数单调（每轮恰 1 帧）。
            assert_eq!(svc.stats().frames, round as u64 + 1, "frames monotonic");
        }
        // 100 轮后：任意注册仍成功（槽位零泄漏的直接证据）。
        let wid = svc.register(0xDEAD, W, H, PixelFormat::Bgr32).expect("slots must be free");
        assert!(svc.unregister(wid));
        // 行账本守恒：100 轮 × 6 行 = 600。
        assert_eq!(svc.submit_rows(), 600);
    }

    #[test]
    fn clip_window_region_unit() {
        // 纯函数三方求交单元用例（含全出屏/半出屏/负窗位）。
        assert_eq!(clip_window_region(0, 0, 8, 6, 0, 0, 8, 6, 32, 24), Some((0, 8, 0, 6)));
        assert_eq!(clip_window_region(-4, -2, 8, 6, 0, 0, 8, 6, 32, 24), Some((4, 8, 2, 4)));
        assert_eq!(clip_window_region(30, 0, 8, 6, 0, 0, 8, 6, 32, 24), Some((0, 2, 0, 6)));
        assert_eq!(clip_window_region(-10, 0, 8, 6, 0, 0, 8, 6, 32, 24), None, "全左出屏");
        // 窗口 y=20 高 6（屏幕行 20..24 可见 4 行）；脏区行 0..2 全可见。
        assert_eq!(clip_window_region(0, 20, 8, 6, 0, 0, 8, 2, 32, 24), Some((0, 8, 0, 2)), "窗下缘裁行");
        // 脏区行 4..8 = 屏幕 24..28 全出屏 → None。
        assert_eq!(clip_window_region(0, 20, 8, 6, 0, 4, 8, 4, 32, 24), None, "脏区全出屏");
    }
}
