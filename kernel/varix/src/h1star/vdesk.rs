//! F235 虚拟桌面 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F235（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：组合键切换虚拟桌面，任务视图顶部显示
//! 桌面条（可增删重排重命名），每个桌面独立记窗口布局与壁纸可选独立。
//!
//! **设计要点**：
//! - 桌面表定容 9 槽（主册：增删上限 9 个桌面），默认 2 个桌面；
//! - 切换状态机：左/右横移动画，时长取 F124 强调曲线 320ms（验收
//!   320ms±20ms 判定），F245 减少动效开启时降级 80ms 直切；
//! - 窗口跨桌移动：拖窗口到桌面条的投递语义——源桌面槽取出、目标
//!   桌面槽放入，目标满则回滚到源（不丢窗口）；
//! - 音频旁路：音频会话表独立于桌面归属（「视频声音不断」是架构性的：
//!   音频路由不随桌面切换），切换事件只改桌面指针不触碰音频表，
//!   以音频表指纹前后相等作判定；
//! - 布局持久化 round-trip（重启后保持，持久化面允许 alloc）。
//!
//! **依赖锚点**：F124 曲线取自 [`crate::h1star::h1base::MotionPolicy`]；
//! 事件环形日志复用 [`crate::star::sbase::RingLog`]；时间一律由调用方
//! 注入毫秒戳，本模块不持时钟；热路径全定长，零堆。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy, Rect, REDUCED_MOTION_MS};
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 桌面数量上限——主册 F235 验收「增删上限 9 个桌面」。
pub const MAX_DESKTOPS: usize = 9;

/// 默认桌面数——主册 F235「默认 2 个桌面」。
pub const DEFAULT_DESKTOPS: usize = 2;

/// 切换动画时长（ms）——主册 F235「切换动画=桌面整体横移 320ms（F124
/// 强调曲线）」，取值唯一源是 [`MotionPolicy::duration_ms`]（一处一事实）。
pub const SWITCH_MS: u32 = 320;

/// 切换时长验收容差——主册 F235 验收「横移 320ms±20ms」。
pub const SWITCH_TOLERANCE_MS: u32 = 20;

/// 每桌面窗口槽容量（交互热路径定容，防堆分配）。
const WIN_CAP: usize = 16;

/// 桌面名字节上限（重命名截断，不留长字符串）。
const NAME_CAP: usize = 16;

/// 音频会话槽（音频路由独立面的容量，与桌面表零耦合）。
const AUDIO_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 窗口几何（跨桌移动与布局记忆的最小承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinGeom {
    /// 窗口句柄（全局递增，跨桌面唯一）。
    pub id: u32,
    /// 当前几何（i32 整数像素域）。
    pub rect: Rect,
    /// 最小化标志（最小化窗仍归属原桌面）。
    pub minimized: bool,
}

/// 单个桌面槽：名字 + 独立壁纸位 + 本桌窗口布局集。
#[derive(Clone, Copy, Debug)]
pub struct DesktopSlot {
    pub id: u32,
    /// 桌面名（定容字节，UTF-8 由上层保证）。
    name: [u8; NAME_CAP],
    name_len: usize,
    /// 主册 F235「壁纸可选独立」——false 时跟随主题全局壁纸。
    pub wallpaper_independent: bool,
    /// 独立壁纸资源号（仅 wallpaper_independent 为 true 时生效）。
    pub wallpaper_id: u32,
    /// 本桌窗口布局集（定容槽，热路径零堆）。
    wins: [Option<WinGeom>; WIN_CAP],
}

impl DesktopSlot {
    /// const 构造（定容数组字面量要求编译期可求值）。
    const fn new(id: u32) -> DesktopSlot {
        DesktopSlot {
            id,
            name: [0; NAME_CAP],
            name_len: 0,
            wallpaper_independent: false,
            wallpaper_id: 0,
            wins: [const { None }; WIN_CAP],
        }
    }

    /// 重命名：超长截断（判据「重命名」且不留无界字符串）。
    pub fn set_name(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(NAME_CAP);
        self.name[..n].copy_from_slice(&bytes[..n]);
        self.name_len = n;
    }

    /// 桌面名（有效字节）。
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 放入窗口：找第一个空槽；满则 false（调用方决定回滚）。
    pub fn put_window(&mut self, w: WinGeom) -> bool {
        for slot in self.wins.iter_mut() {
            if slot.is_none() {
                *slot = Some(w);
                return true;
            }
        }
        false
    }

    /// 按 id 取出窗口（找到即摘除）。
    pub fn take_window(&mut self, win_id: u32) -> Option<WinGeom> {
        let pos = self.wins.iter().position(|s| matches!(s, Some(w) if w.id == win_id))?;
        self.wins[pos].take()
    }

    /// 按 id 查窗口（不摘除，判定用）。
    pub fn peek_window(&self, win_id: u32) -> Option<WinGeom> {
        self.wins.iter().filter_map(|s| *s).find(|w| w.id == win_id)
    }

    /// 本桌窗口数。
    pub fn window_count(&self) -> usize {
        self.wins.iter().filter(|s| s.is_some()).count()
    }

    /// 窗口布局快照（诊断/持久化面）。
    pub fn windows(&self) -> Vec<WinGeom> {
        self.wins.iter().filter_map(|s| *s).collect()
    }
}

// ---------------------------------------------------------------------------
// 切换状态机
// ---------------------------------------------------------------------------

/// 横移方向（主册「桌面整体横移」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchDir {
    /// 向左横移（切往更小序号桌面）。
    Left,
    /// 向右横移（切往更大序号桌面）。
    Right,
}

/// 一次进行中的切换。
#[derive(Clone, Copy, Debug)]
struct Switching {
    from: usize,
    to: usize,
    dir: SwitchDir,
    start_ms: u64,
    dur_ms: u32,
}

/// 桌面事件（环形日志条目，全小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskEvent {
    /// 开始切换。
    SwitchStarted { from: u8, to: u8, dir: u8 },
    /// 切换完成（actual_ms 供 ±20ms 验收）。
    SwitchFinished { from: u8, to: u8, actual_ms: u32 },
    /// 窗口跨桌移动（拖到桌面条投递）。
    WindowMoved { win: u32, from: u8, to: u8 },
    /// 新增桌面。
    DesktopAdded,
    /// 移除桌面（窗口并入邻桌）。
    DesktopRemoved { merged_into: u8 },
    /// 重命名。
    Renamed { desk: u8 },
    /// 重排。
    Reordered { from: u8, to: u8 },
    /// 壁纸独立位变更。
    WallpaperSet { desk: u8, independent: u8 },
    /// 布局持久化恢复完成。
    LayoutRestored,
}

// ---------------------------------------------------------------------------
// 虚拟桌面管理器
// ---------------------------------------------------------------------------

/// 虚拟桌面管理器：9 槽桌面表 + 切换状态机 + 音频旁路面。
pub struct VirtualDesk {
    desks: [DesktopSlot; MAX_DESKTOPS],
    count: usize,
    current: usize,
    switching: Option<Switching>,
    policy: MotionPolicy,
    next_win: u32,
    next_desk: u32,
    /// 音频会话表——与桌面归属零耦合（F235「声音不断」的架构实现）。
    audio: [Option<u32>; AUDIO_CAP],
    /// 最近一次真实切换时长（tick 实测，供 ±20ms 判定）。
    last_switch_ms: Option<u32>,
    events: RingLog<DeskEvent, 32>,
}

impl VirtualDesk {
    /// 建管理器：默认 2 个桌面（主册）。
    pub fn new(policy: MotionPolicy) -> VirtualDesk {
        let mut vd = VirtualDesk {
            desks: [const { DesktopSlot::new(0) }; MAX_DESKTOPS],
            count: 0,
            current: 0,
            switching: None,
            policy,
            next_win: 1,
            next_desk: 1,
            audio: [const { None }; AUDIO_CAP],
            last_switch_ms: None,
            events: RingLog::new(),
        };
        for _ in 0..DEFAULT_DESKTOPS {
            vd.add_desktop();
        }
        vd
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn slot(&self, idx: usize) -> Option<&DesktopSlot> {
        if idx < self.count { Some(&self.desks[idx]) } else { None }
    }

    pub fn events(&self) -> Vec<DeskEvent> {
        self.events.newest_first()
    }

    /// 新增桌面（上限 9，满则 false——判据「增删上限 9 个桌面」）。
    pub fn add_desktop(&mut self) -> bool {
        if self.count >= MAX_DESKTOPS {
            return false;
        }
        let id = self.next_desk;
        self.next_desk += 1;
        let mut slot = DesktopSlot::new(id);
        // 名字「Desktop N」：容量内构造，重命名接口对上层开放。
        let mut buf = [0u8; NAME_CAP];
        let prefix = b"Desktop ";
        buf[..prefix.len()].copy_from_slice(prefix);
        buf[prefix.len()] = b'0' + (id % 10) as u8;
        slot.set_name(&buf[..prefix.len() + 1]);
        self.desks[self.count] = slot;
        self.count += 1;
        self.events.push(DeskEvent::DesktopAdded);
        true
    }

    /// 移除桌面：窗口并入邻桌（与拖移语义同向），至少保留 1 个。
    pub fn remove_desktop(&mut self, idx: usize) -> bool {
        if idx >= self.count || self.count <= 1 {
            return false;
        }
        let neighbor = if idx + 1 < self.count { idx + 1 } else { idx - 1 };
        let orphans = self.desks[idx].windows();
        for w in orphans {
            if !self.desks[neighbor].put_window(w) {
                break;
            }
        }
        for i in idx..self.count - 1 {
            self.desks[i] = self.desks[i + 1];
        }
        self.count -= 1;
        if self.current >= self.count {
            self.current = self.count - 1;
        }
        if self.switching.is_some() {
            // 切换目标可能被删：直接落定，避免悬空状态。
            self.switching = None;
        }
        self.events.push(DeskEvent::DesktopRemoved { merged_into: neighbor as u8 });
        true
    }

    /// 重命名（判据「重命名」，超长截断）。
    pub fn rename(&mut self, idx: usize, name: &[u8]) -> bool {
        if idx >= self.count {
            return false;
        }
        self.desks[idx].set_name(name);
        self.events.push(DeskEvent::Renamed { desk: idx as u8 });
        true
    }

    /// 重排（判据「重排」）：把 from 槽整体搬到 to 槽，中间槽顺移。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.count || to >= self.count || from == to {
            return false;
        }
        let moving = self.desks[from];
        if from < to {
            for i in from..to {
                self.desks[i] = self.desks[i + 1];
            }
        } else {
            for i in (to + 1..=from).rev() {
                self.desks[i] = self.desks[i - 1];
            }
        }
        self.desks[to] = moving;
        self.events.push(DeskEvent::Reordered { from: from as u8, to: to as u8 });
        true
    }

    /// 壁纸独立位（判据「壁纸可选独立」）。
    pub fn set_wallpaper(&mut self, idx: usize, independent: bool, wall_id: u32) -> bool {
        if idx >= self.count {
            return false;
        }
        self.desks[idx].wallpaper_independent = independent;
        self.desks[idx].wallpaper_id = wall_id;
        self.events.push(DeskEvent::WallpaperSet { desk: idx as u8, independent: independent as u8 });
        true
    }

    /// 在某桌面登记一个新窗口（占位分配全局句柄）。
    pub fn place_window(&mut self, desk: usize, rect: Rect, minimized: bool) -> Option<u32> {
        if desk >= self.count {
            return None;
        }
        let id = self.next_win;
        self.next_win += 1;
        if self.desks[desk].put_window(WinGeom { id, rect, minimized }) {
            Some(id)
        } else {
            self.next_win -= 1;
            None
        }
    }

    /// 拖窗口到桌面条的投递语义：源桌取出 → 目标桌放入；目标满则回滚。
    pub fn move_window(&mut self, win_id: u32, from: usize, to: usize) -> bool {
        if from >= self.count || to >= self.count || from == to {
            return false;
        }
        let w = match self.desks[from].take_window(win_id) {
            Some(w) => w,
            None => return false,
        };
        if !self.desks[to].put_window(w) {
            // 回滚：目标满，窗口回源桌（不丢窗口）。
            let _ = self.desks[from].put_window(w);
            return false;
        }
        self.events.push(DeskEvent::WindowMoved { win: win_id, from: from as u8, to: to as u8 });
        true
    }

    /// 窗口是否在任意桌面槽中（判定/诊断用）。
    pub fn has_window(&self, win_id: u32) -> bool {
        (0..self.count).any(|i| self.desks[i].peek_window(win_id).is_some())
    }

    /// 发起切换（判据「组合键切换」）：目标合法且非切换中才受理。
    pub fn switch_to(&mut self, idx: usize, now_ms: u64) -> bool {
        if idx >= self.count || idx == self.current || self.switching.is_some() {
            return false;
        }
        let dir = if idx < self.current { SwitchDir::Left } else { SwitchDir::Right };
        let dur = self.policy.duration_ms(Curve::Emphasis, 0);
        self.switching = Some(Switching { from: self.current, to: idx, dir, start_ms: now_ms, dur_ms: dur });
        self.events.push(DeskEvent::SwitchStarted { from: self.current as u8, to: idx as u8, dir: dir as u8 });
        true
    }

    /// 切换动画进度（0..=1000 整数定点；空闲即 1000）。
    pub fn progress_at(&self, now_ms: u64) -> u32 {
        match self.switching {
            None => 1000,
            Some(s) => {
                let elapsed = now_ms.saturating_sub(s.start_ms) as u32;
                self.policy.progress(Curve::Emphasis, elapsed.min(s.dur_ms), 0)
            }
        }
    }

    /// 横移方向（渲染面：-1 左移 / +1 右移 / 0 静止）。
    pub fn switch_dir(&self) -> i8 {
        match self.switching {
            None => 0,
            Some(s) => match s.dir {
                SwitchDir::Left => -1,
                SwitchDir::Right => 1,
            },
        }
    }

    /// 推进状态机：到时落定 current，记录实测时长。
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let s = match self.switching {
            Some(s) => s,
            None => return false,
        };
        let elapsed = now_ms.saturating_sub(s.start_ms) as u32;
        if elapsed < s.dur_ms {
            return false;
        }
        self.switching = None;
        self.current = s.to;
        self.last_switch_ms = Some(elapsed);
        self.events
            .push(DeskEvent::SwitchFinished { from: s.from as u8, to: s.to as u8, actual_ms: elapsed });
        true
    }

    /// ±20ms 验收判定：最近一次切换实测时长落在 320±20（常态策略）。
    /// F245 降级策略下按 80ms 直切判定（去位移、保状态信号）。
    pub fn last_switch_in_tolerance(&self) -> bool {
        let actual = match self.last_switch_ms {
            Some(a) => a,
            None => return false,
        };
        if self.policy.reduced {
            return actual <= REDUCED_MOTION_MS;
        }
        let target = SWITCH_MS as i32;
        (actual as i32 - target).abs() <= SWITCH_TOLERANCE_MS as i32
    }

    // -- 音频旁路面 ---------------------------------------------------------

    /// 打开音频会话（音频路由独立于桌面归属）。
    pub fn audio_open(&mut self, handle: u32) -> bool {
        for slot in self.audio.iter_mut() {
            if slot.is_none() {
                *slot = Some(handle);
                return true;
            }
        }
        false
    }

    /// 关闭音频会话。
    pub fn audio_close(&mut self, handle: u32) -> bool {
        for slot in self.audio.iter_mut() {
            if *slot == Some(handle) {
                *slot = None;
                return true;
            }
        }
        false
    }

    /// 音频表指纹（FNV-1a）——切换前后相等即「声音不断」。
    pub fn audio_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for slot in self.audio.iter() {
            let v = slot.unwrap_or(0xFFFF_FFFF);
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }

    /// 音频会话数（诊断面）。
    pub fn audio_sessions(&self) -> usize {
        self.audio.iter().filter(|s| s.is_some()).count()
    }

    // -- 持久化面（alloc 允许：非热路径，重启恢复一次） ---------------------

    /// 布局持久化：桌面数/名字/壁纸位/每窗几何（重启后保持）。
    pub fn persist(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.count * 512);
        out.push(self.count as u8);
        for i in 0..self.count {
            let d = &self.desks[i];
            out.push(d.name_len as u8);
            out.extend_from_slice(&d.name[..d.name_len]);
            out.push(d.wallpaper_independent as u8);
            out.extend_from_slice(&d.wallpaper_id.to_le_bytes());
            let wins = d.windows();
            out.push(wins.len() as u8);
            for w in wins {
                out.extend_from_slice(&w.id.to_le_bytes());
                out.extend_from_slice(&w.rect.x.to_le_bytes());
                out.extend_from_slice(&w.rect.y.to_le_bytes());
                out.extend_from_slice(&w.rect.w.to_le_bytes());
                out.extend_from_slice(&w.rect.h.to_le_bytes());
                out.push(w.minimized as u8);
            }
        }
        out
    }

    /// 从持久化字节恢复布局（校验失败整体拒绝，不留半态）。
    pub fn restore(&mut self, buf: &[u8]) -> bool {
        let mut pos = 0usize;
        let mut rd_u32 = |buf: &[u8], pos: &mut usize| -> Option<u32> {
            if *pos + 4 > buf.len() {
                return None;
            }
            let v = u32::from_le_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]]);
            *pos += 4;
            Some(v)
        };
        if buf.is_empty() {
            return false;
        }
        let count = buf[pos] as usize;
        pos += 1;
        if count == 0 || count > MAX_DESKTOPS {
            return false;
        }
        let mut desks = [const { DesktopSlot::new(0) }; MAX_DESKTOPS];
        for i in 0..count {
            let nlen = match buf.get(pos) {
                Some(&n) if (n as usize) <= NAME_CAP => n as usize,
                _ => return false,
            };
            pos += 1;
            if pos + nlen + 6 > buf.len() {
                return false;
            }
            let mut slot = DesktopSlot::new(i as u32 + 1);
            slot.set_name(&buf[pos..pos + nlen]);
            pos += nlen;
            slot.wallpaper_independent = buf[pos] != 0;
            pos += 1;
            slot.wallpaper_id = match rd_u32(buf, &mut pos) {
                Some(v) => v,
                None => return false,
            };
            let wcount = buf[pos] as usize;
            pos += 1;
            for _ in 0..wcount {
                let id = match rd_u32(buf, &mut pos) {
                    Some(v) => v,
                    None => return false,
                };
                let mut xyzw = [0i32; 4];
                for k in 0..4 {
                    xyzw[k] = match rd_u32(buf, &mut pos) {
                        Some(v) => v as i32,
                        None => return false,
                    };
                }
                let minimized = match buf.get(pos) {
                    Some(&m) => m != 0,
                    None => return false,
                };
                pos += 1;
                if xyzw[2] <= 0 || xyzw[3] <= 0 {
                    return false;
                }
                let geom = WinGeom {
                    id,
                    rect: Rect::new(xyzw[0], xyzw[1], xyzw[2], xyzw[3]),
                    minimized,
                };
                if !slot.put_window(geom) {
                    return false;
                }
            }
            desks[i] = slot;
        }
        if pos != buf.len() {
            return false;
        }
        self.desks = desks;
        self.count = count;
        self.current = self.current.min(count - 1);
        self.switching = None;
        self.events.push(DeskEvent::LayoutRestored);
        true
    }

    /// 全域窗口总数（fuzz 不变量用）。
    pub fn total_windows(&self) -> usize {
        (0..self.count).map(|i| self.desks[i].window_count()).sum()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// F235 自检（判据：桌面条增删重排重命名 + 320ms±20ms 横移 + 音频不断
/// + 布局独立持久化）。
pub fn run_vdesk_checks() -> CheckSet {
    let mut set = CheckSet::new("F235-vdesk");

    // 1. 默认 2 个桌面（主册原文）。
    let vd = VirtualDesk::new(MotionPolicy::normal());
    set.add("default 2 desktops", vd.count() == DEFAULT_DESKTOPS, "");

    // 2. 增删上限 9：从 2 加到 9 成功，第 10 次拒绝。
    let mut vd2 = VirtualDesk::new(MotionPolicy::normal());
    let mut added = 0;
    while vd2.add_desktop() {
        added += 1;
    }
    set.add("cap 9 desktops enforced", vd2.count() == MAX_DESKTOPS && added == 7, "");

    // 3. 移除桌面窗口并入邻桌（不丢窗口）。
    let r1 = Rect::new(0, 0, 800, 600);
    let wid = vd2.place_window(3, r1, false).unwrap();
    let before = vd2.total_windows();
    assert!(vd2.remove_desktop(3));
    set.add(
        "remove merges windows into neighbor",
        vd2.total_windows() == before && vd2.has_window(wid),
        "",
    );

    // 4. 重命名（含超长截断）。
    let mut vd3 = VirtualDesk::new(MotionPolicy::normal());
    let long = [b'A'; NAME_CAP + 8];
    assert!(vd3.rename(1, &long));
    set.add(
        "rename truncates to cap",
        vd3.slot(1).unwrap().name().len() == NAME_CAP && vd3.slot(1).unwrap().name()[0] == b'A',
        "",
    );

    // 5. 重排：内容跟随槽位移动。
    assert!(vd3.place_window(0, r1, false).is_some());
    assert!(vd3.reorder(0, 1));
    set.add(
        "reorder moves slot contents",
        vd3.slot(0).unwrap().window_count() == 0 && vd3.slot(1).unwrap().window_count() == 1,
        "",
    );

    // 6. 切换动画时长=F124 强调曲线 320ms（常态）/ 80ms（F245 降级）。
    let mut vd4 = VirtualDesk::new(MotionPolicy::normal());
    assert!(vd4.switch_to(1, 10_000));
    set.add(
        "switch duration = Emphasis 320ms",
        vd4.progress_at(10_100) < 1000 && !vd4.tick(10_100),
        "",
    );
    let mut vdr = VirtualDesk::new(MotionPolicy::reduced());
    assert!(vdr.switch_to(1, 0));
    set.add("reduced motion 80ms cut", vdr.tick(80), "");

    // 7. ±20ms 判定：tick 间隔 335ms 落容差内；350ms 越界。
    let mut vd5 = VirtualDesk::new(MotionPolicy::normal());
    assert!(vd5.switch_to(1, 0) && vd5.tick(335));
    let inside = vd5.last_switch_in_tolerance();
    let mut vd6 = VirtualDesk::new(MotionPolicy::normal());
    assert!(vd6.switch_to(1, 0) && vd6.tick(350));
    let outside = !vd6.last_switch_in_tolerance();
    set.add("switch tolerance 320±20ms judged", inside && outside, "");

    // 8. 横移进度单调（渲染面：进度随时间不减、终点 1000）。
    let mut vd7 = VirtualDesk::new(MotionPolicy::normal());
    assert!(vd7.switch_to(1, 0));
    let mut mono = true;
    let mut prev = 0u32;
    for ms in (0..=320).step_by(20) {
        let p = vd7.progress_at(ms);
        if p < prev {
            mono = false;
        }
        prev = p;
    }
    set.add("switch progress monotonic", mono && vd7.progress_at(320) == 1000, "");

    // 9. 拖窗口到桌面条投递：源减目标增；非法投递拒绝。
    let mut vd8 = VirtualDesk::new(MotionPolicy::normal());
    let wid8 = vd8.place_window(0, r1, false).unwrap();
    set.add(
        "drop on strip moves window",
        vd8.move_window(wid8, 0, 1)
            && vd8.slot(0).unwrap().window_count() == 0
            && vd8.slot(1).unwrap().window_count() == 1,
        "",
    );
    set.add("move unknown window rejected", !vd8.move_window(wid8, 0, 1), "");

    // 10. 音频旁路：完整切换周期音频表指纹不变（「声音不断」架构判定）。
    assert!(vd8.audio_open(0x1001) && vd8.audio_open(0x1002));
    let fp0 = vd8.audio_fingerprint();
    assert!(vd8.switch_to(1, 0) && vd8.tick(320));
    let fp1 = vd8.audio_fingerprint();
    set.add(
        "audio uninterrupted across switch",
        fp0 == fp1 && vd8.audio_sessions() == 2 && vd8.current() == 1,
        "",
    );

    // 11. 布局持久化 round-trip（重启后保持）。
    let snap = vd8.persist();
    let mut vd9 = VirtualDesk::new(MotionPolicy::normal());
    let restored = vd9.restore(&snap)
        && vd9.count() == vd8.count()
        && vd9.total_windows() == vd8.total_windows()
        && vd9.slot(1).unwrap().peek_window(wid8).map(|w| w.rect) == vd8.slot(1).unwrap().peek_window(wid8).map(|w| w.rect);
    set.add("persist round-trip restores layout", restored, "");
    set.add("persist rejects truncated buf", !vd9.restore(&snap[..snap.len() - 3]), "");

    // 12. 壁纸独立位（「壁纸可选独立」）。
    assert!(vd9.set_wallpaper(1, true, 7));
    set.add(
        "per-desk wallpaper independent",
        vd9.slot(1).unwrap().wallpaper_independent
            && vd9.slot(1).unwrap().wallpaper_id == 7
            && !vd9.slot(0).unwrap().wallpaper_independent,
        "",
    );

    // 13. xors32 fuzz：随机增删/重排/移动/切换/放窗 1000 轮，不变量=
    //     桌面数 1..=9、current 合法、每桌窗口数不超容量、不 panic。
    let mut vdf = VirtualDesk::new(MotionPolicy::normal());
    let mut x: u32 = 0x853C_49E6;
    let mut ok = true;
    for i in 0..1000u32 {
        let op = xors32(&mut x) % 6;
        let n = vdf.count();
        match op {
            0 => {
                vdf.add_desktop();
            }
            1 if n > 1 => {
                let idx = (xors32(&mut x) as usize) % n;
                vdf.remove_desktop(idx);
            }
            2 if n > 1 => {
                let a = (xors32(&mut x) as usize) % n;
                let b = (xors32(&mut x) as usize) % n;
                let _ = vdf.reorder(a, b);
            }
            3 if n > 1 => {
                let a = (xors32(&mut x) as usize) % n;
                let b = (xors32(&mut x) as usize) % n;
                let w = 1 + (xors32(&mut x) % 64);
                let _ = vdf.move_window(w, a, b);
            }
            4 if n > 1 => {
                let b = (xors32(&mut x) as usize) % n;
                if vdf.switch_to(b, i as u64 * 1000) {
                    vdf.tick(i as u64 * 1000 + 340);
                }
            }
            _ => {
                let d = (xors32(&mut x) as usize) % n;
                let _ = vdf.place_window(d, r1, false);
            }
        }
        let overflow = (0..vdf.count()).any(|i| vdf.slot(i).unwrap().window_count() > WIN_CAP);
        if vdf.count() == 0
            || vdf.count() > MAX_DESKTOPS
            || vdf.current() >= vdf.count()
            || overflow
        {
            ok = false;
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_cap_boundaries() {
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        assert_eq!(vd.count(), 2);
        for _ in 0..7 {
            assert!(vd.add_desktop());
        }
        assert!(!vd.add_desktop(), "第 10 个桌面必须被拒");
        // 至少保留 1 个桌面。
        while vd.count() > 1 {
            assert!(vd.remove_desktop(0));
        }
        assert!(!vd.remove_desktop(0));
        assert_eq!(vd.count(), 1);
    }

    #[test]
    fn switch_timing_within_tolerance() {
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        assert_eq!(vd.switch_dir(), 0);
        assert!(vd.switch_to(1, 0));
        assert_eq!(vd.switch_dir(), 1, "1→2 桌面向右横移");
        assert!(!vd.tick(319), "319ms 未到时长不落定");
        assert!(vd.tick(340));
        assert_eq!(vd.current(), 1);
        assert!(vd.last_switch_in_tolerance(), "340ms 落在 320±20 容差内");
        // 切换中不能二次发起。
        assert!(vd.switch_to(0, 400));
        assert!(!vd.switch_to(1, 410));
    }

    #[test]
    fn audio_table_independent_of_desks() {
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        // 8 槽会话表：8 次成功、第 9 次拒绝。
        for h in 1..=8u32 {
            assert!(vd.audio_open(h));
        }
        assert!(!vd.audio_open(9), "会话表满必须拒绝");
        let fp = vd.audio_fingerprint();
        for i in 0..5 {
            assert!(vd.add_desktop());
            assert!(vd.switch_to(1 + i, i as u64 * 1000));
            assert!(vd.tick(i as u64 * 1000 + 330));
        }
        assert_eq!(vd.audio_fingerprint(), fp, "音频表与桌面切换零耦合");
        assert!(vd.audio_close(7));
        assert_ne!(vd.audio_fingerprint(), fp);
    }

    #[test]
    fn move_window_rollback_when_full() {
        let r = Rect::new(0, 0, 100, 100);
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        let mut ids = [0u32; WIN_CAP];
        for k in 0..WIN_CAP {
            ids[k] = vd.place_window(1, r, false).unwrap();
        }
        let w0 = vd.place_window(0, r, false).unwrap();
        assert!(!vd.move_window(w0, 0, 1), "目标满必须拒绝");
        assert!(vd.has_window(w0), "回滚后窗口仍在源桌");
        assert!(vd.move_window(ids[0], 1, 0));
        assert!(vd.move_window(w0, 0, 1), "腾位后投递成功");
    }

    #[test]
    fn persist_round_trip_full() {
        let r = Rect::new(10, 20, 640, 480);
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        let a = vd.place_window(0, r, false).unwrap();
        let b = vd.place_window(1, r, true).unwrap();
        vd.set_wallpaper(1, true, 42);
        vd.rename(0, "工作台".as_bytes());
        let snap = vd.persist();
        let mut back = VirtualDesk::new(MotionPolicy::reduced());
        assert!(back.restore(&snap));
        assert_eq!(back.count(), 2);
        assert_eq!(back.total_windows(), 2);
        assert_eq!(back.slot(0).unwrap().peek_window(a).unwrap().rect, r);
        assert!(back.slot(1).unwrap().peek_window(b).unwrap().minimized);
        assert_eq!(back.slot(1).unwrap().wallpaper_id, 42);
        assert_eq!(back.slot(0).unwrap().name(), &"工作台".as_bytes()[..]);
        // 截断/空缓冲整体拒绝。
        assert!(!back.restore(&[]));
        assert!(!back.restore(&snap[..10]));
        assert!(!back.restore(&snap[..snap.len() - 1]));
    }

    #[test]
    fn vdesk_selfcheck_all_green() {
        let set = run_vdesk_checks();
        assert!(set.all_passed(), "F235 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F235。持久化面 = 九桌面布局记忆 framed 记录（每桌面名+窗口
// 归属位图）；壳接线面 = 横移动画帧清单（F124 强调曲线）+ 桌面切换
// 音频豁免判定；判定面 = run_vdesk_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（与 audio_fingerprint 同族常数——一处一事实）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录容量上限在册：count u8 + 9 桌 × (1+16 名 + 1+4 壁纸 + 1
/// + 16 窗 × 21B) + checksum（变长，按实际内容写）。
pub const V2_PAYLOAD_MAX: usize = 1 + MAX_DESKTOPS * (2 + NAME_CAP + 5 + 1 + WIN_CAP * 21);
pub const V2_REC_MAX: usize = 5 + V2_PAYLOAD_MAX + 4;

/// 单窗持久化条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2WinRec {
    pub id: u32,
    pub rect: Rect,
    pub minimized: bool,
}

/// 单桌面持久化条目（名 + 壁纸位 + 窗口集）。
#[derive(Clone, Copy, Debug)]
pub struct V2DeskRec {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub wallpaper_independent: bool,
    pub wallpaper_id: u32,
    pub wins: [Option<V2WinRec>; WIN_CAP],
    pub win_count: usize,
}

/// 九桌面布局记忆记录（主册 F235 v2：每桌面名 + 窗口归属——重启后
/// 桌面条与各桌窗口归属原样还原）。
#[derive(Clone, Copy, Debug)]
pub struct V2LayoutRec {
    pub desks: [V2DeskRec; MAX_DESKTOPS],
    pub count: usize,
}

impl V2LayoutRec {
    /// 从管理器导出（只走公开读取面：count/slot/name/windows）。
    pub fn capture(vd: &VirtualDesk) -> V2LayoutRec {
        let mut rec = V2LayoutRec {
            desks: [const { V2DeskRec { name: [0; NAME_CAP], name_len: 0, wallpaper_independent: false, wallpaper_id: 0, wins: [const { None }; WIN_CAP], win_count: 0 } }; MAX_DESKTOPS],
            count: 0,
        };
        for i in 0..vd.count() {
            let d = match vd.slot(i) {
                Some(d) => d,
                None => break,
            };
            let mut dr = V2DeskRec {
                name: [0; NAME_CAP],
                name_len: d.name().len().min(NAME_CAP),
                wallpaper_independent: d.wallpaper_independent,
                wallpaper_id: d.wallpaper_id,
                wins: [const { None }; WIN_CAP],
                win_count: 0,
            };
            dr.name[..dr.name_len].copy_from_slice(&d.name()[..dr.name_len]);
            for w in d.windows() {
                if dr.win_count < WIN_CAP {
                    dr.wins[dr.win_count] = Some(V2WinRec { id: w.id, rect: w.rect, minimized: w.minimized });
                    dr.win_count += 1;
                }
            }
            rec.desks[rec.count] = dr;
            rec.count += 1;
        }
        rec
    }

    /// 窗口归属位图（主册 v2 锚原文：每桌面窗口归属位图——
    /// bit i = 本桌第 i 槽有窗）。
    pub fn win_bitmap(dr: &V2DeskRec) -> u16 {
        let mut m = 0u16;
        for (i, w) in dr.wins.iter().enumerate().take(16) {
            if w.is_some() {
                m |= 1 << i;
            }
        }
        m
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < 6 {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.count as u8;
        let mut n = 6usize;
        for i in 0..self.count {
            let d = &self.desks[i];
            if out.len() < n + 2 + d.name_len + 6 + 1 {
                return None;
            }
            out[n] = d.name_len as u8;
            out[n + 1..n + 1 + d.name_len].copy_from_slice(&d.name[..d.name_len]);
            n += 1 + d.name_len;
            out[n] = d.wallpaper_independent as u8;
            out[n + 1..n + 5].copy_from_slice(&d.wallpaper_id.to_le_bytes());
            n += 5;
            out[n] = d.win_count as u8;
            n += 1;
            for w in d.wins.iter().flatten() {
                if out.len() < n + 21 {
                    return None;
                }
                out[n..n + 4].copy_from_slice(&w.id.to_le_bytes());
                out[n + 4..n + 8].copy_from_slice(&w.rect.x.to_le_bytes());
                out[n + 8..n + 12].copy_from_slice(&w.rect.y.to_le_bytes());
                out[n + 12..n + 16].copy_from_slice(&w.rect.w.to_le_bytes());
                out[n + 16..n + 20].copy_from_slice(&w.rect.h.to_le_bytes());
                out[n + 20] = w.minimized as u8;
                n += 21;
            }
        }
        if out.len() < n + 4 {
            return None;
        }
        let sum = v2_fnv1a32(&out[..n]);
        out[n..n + 4].copy_from_slice(&sum.to_le_bytes());
        Some(n + 4)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<V2LayoutRec, V2SaveErr> {
        if buf.len() < 10 {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let body = buf.len() - 4;
        let sum = u32::from_le_bytes([buf[body], buf[body + 1], buf[body + 2], buf[body + 3]]);
        if v2_fnv1a32(&buf[..body]) != sum {
            return Err(V2SaveErr::BadChecksum);
        }
        let count = buf[5] as usize;
        if count == 0 || count > MAX_DESKTOPS {
            return Err(V2SaveErr::BadLen);
        }
        let mut rec = V2LayoutRec {
            desks: [const { V2DeskRec { name: [0; NAME_CAP], name_len: 0, wallpaper_independent: false, wallpaper_id: 0, wins: [const { None }; WIN_CAP], win_count: 0 } }; MAX_DESKTOPS],
            count: 0,
        };
        let mut n = 6usize;
        for di in 0..count {
            if n + 1 > body {
                return Err(V2SaveErr::BadLen);
            }
            let nlen = buf[n] as usize;
            n += 1;
            if nlen > NAME_CAP || n + nlen + 6 > body {
                return Err(V2SaveErr::BadLen);
            }
            let mut dr = V2DeskRec {
                name: [0; NAME_CAP],
                name_len: nlen,
                wallpaper_independent: buf[n + nlen] != 0,
                wallpaper_id: u32::from_le_bytes([buf[n + nlen + 1], buf[n + nlen + 2], buf[n + nlen + 3], buf[n + nlen + 4]]),
                wins: [const { None }; WIN_CAP],
                win_count: 0,
            };
            dr.name[..nlen].copy_from_slice(&buf[n..n + nlen]);
            // [缺陷账本] 现象：9 桌布局 round-trip 红。根因：解码侧壁纸
            // 块步进多加 1（+6，应为 +5：1 标志位 + 4 id），win_count
            // 读到窗记录首字节/下桌名长——编码布局 [nlen][名][旗1][id4]
            // [wcount][窗..] 的偏移对不齐，属实现缺陷。修法：步进改
            // nlen + 5，界检同步收紧到恰好所需（nlen+6）。
            n += nlen + 5;
            let wcount = buf[n] as usize;
            n += 1;
            if wcount > WIN_CAP {
                return Err(V2SaveErr::BadLen);
            }
            for _ in 0..wcount {
                if n + 21 > body {
                    return Err(V2SaveErr::BadLen);
                }
                let id = u32::from_le_bytes([buf[n], buf[n + 1], buf[n + 2], buf[n + 3]]);
                let x = i32::from_le_bytes([buf[n + 4], buf[n + 5], buf[n + 6], buf[n + 7]]);
                let y = i32::from_le_bytes([buf[n + 8], buf[n + 9], buf[n + 10], buf[n + 11]]);
                let w = i32::from_le_bytes([buf[n + 12], buf[n + 13], buf[n + 14], buf[n + 15]]);
                let h = i32::from_le_bytes([buf[n + 16], buf[n + 17], buf[n + 18], buf[n + 19]]);
                if w <= 0 || h <= 0 {
                    return Err(V2SaveErr::BadLen);
                }
                let minimized = buf[n + 20] != 0;
                dr.wins[dr.win_count] = Some(V2WinRec { id, rect: Rect::new(x, y, w, h), minimized });
                dr.win_count += 1;
                n += 21;
            }
            rec.desks[di] = dr;
            rec.count += 1;
        }
        if n != body {
            return Err(V2SaveErr::BadLen);
        }
        Ok(rec)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 横移动画帧清单帧数（F124 强调曲线 320ms，60fps 采样 ≈ 20 帧）。
pub const V2_SWITCH_FRAMES: usize = 20;

/// 横移帧偏移清单（F124 Emphasis 曲线）：t 在 [0,dur) 均匀采样，
/// 偏移 = span × progress / 1000，方向由 dir 符号给出（-1 左 / +1 右）。
/// 合成器按帧取值——与 progress_at 同一曲线取值点（一处一事实）。
pub fn v2_switch_frames(policy: MotionPolicy, dir: i8, span_px: i32, dur_ms: u32) -> [i32; V2_SWITCH_FRAMES] {
    let mut out = [0i32; V2_SWITCH_FRAMES];
    let dur = dur_ms.max(1);
    for (i, slot) in out.iter_mut().enumerate() {
        let t = dur * i as u32 / V2_SWITCH_FRAMES as u32;
        let p = policy.progress(Curve::Emphasis, t, 0) as i32;
        *slot = if dir < 0 { -(span_px * p) / 1000 } else { (span_px * p) / 1000 };
    }
    out
}

/// 桌面切换门判定（音频流豁免）：切换前后音频表指纹相等 = 切换没有
/// 触碰音频路由（「视频声音不断」的架构判定，记录式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2AudioExempt {
    pub fp_before: u64,
    pub fp_after: u64,
}

impl V2AudioExempt {
    pub fn ok(&self) -> bool {
        self.fp_before == self.fp_after
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// F235 v2 自检（首条必为持久化 round-trip）。
pub fn run_vdesk_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F235-vdesk-v2");
    let r = Rect::new(0, 0, 800, 600);

    // 1. 持久化 round-trip（验主册「每桌面独立记窗口布局」framed 还原：
    //    桌面数/名/壁纸位/窗口几何与归属位图逐项相等）。
    let mut vd = VirtualDesk::new(MotionPolicy::normal());
    let w1 = vd.place_window(0, r, false).unwrap_or(0);
    let _ = vd.place_window(1, Rect::new(10, 20, 640, 480), true);
    let _ = vd.rename(0, "工作台".as_bytes());
    let _ = vd.set_wallpaper(1, true, 7);
    let rec = V2LayoutRec::capture(&vd);
    let mut buf = [0u8; V2_REC_MAX];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2LayoutRec::from_bytes(&buf[..wrote]);
    // [缺陷账本] 现象：round-trip 断言红于窗口 id。根因：检查项硬编码
    // 桌 1 窗口 id == 0，而 place_window 的句柄自 1 起配（next_win: 1，
    // 本桌窗 id = 2）——句柄值是实现细节，round-trip 应对拍快照本身。
    // 修法：改检查项，id 与 capture 快照对拍，几何/最小化态仍按场景
    // 硬断言。
    let want_win1 = rec.desks[1].wins[0].map(|w| (w.id, w.rect, w.minimized));
    let same = match &back {
        Ok(b) => {
            b.count == rec.count
                && b.desks[0].name_len == rec.desks[0].name_len
                && b.desks[0].name[..b.desks[0].name_len] == rec.desks[0].name[..rec.desks[0].name_len]
                && b.desks[1].wallpaper_independent
                && b.desks[1].wallpaper_id == 7
                && b.desks[1].wins[0].map(|w| (w.id, w.rect, w.minimized)) == want_win1
                && want_win1.map(|(_, r, m)| r == Rect::new(10, 20, 640, 480) && m).unwrap_or(false)
        }
        Err(_) => false,
    };
    set.add(
        "v2 persist round-trip: 9-desk layout record",
        wrote > 0 && same && back.map(|b| V2LayoutRec::win_bitmap(&b.desks[1])).unwrap_or(0) == 1,
        "",
    );

    // 2. 四类损坏全拒绝。
    //    [缺陷账本] 现象：BadLen 分支红。根因：检查项用「截短 1 字节」
    //    构造长度损坏，但实现（变长 framed 记录）先验校验和后逐槽
    //    解析，截短必先撞 BadChecksum——BadLen 分支（count 越界等）
    //    未被测到，属检查项构造缺陷。修法：改检查项——count 翻到
    //    MAX_DESKTOPS 外并重算校验和，真测 BadLen 分支。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 2;
    let mut b3 = [0u8; V2_REC_MAX];
    b3[..wrote].copy_from_slice(&buf[..wrote]);
    b3[5] = (MAX_DESKTOPS + 1) as u8; // 桌面数越界
    let body3 = wrote - 4;
    let sum3 = v2_fnv1a32(&b3[..body3]);
    b3[body3..body3 + 4].copy_from_slice(&sum3.to_le_bytes());
    let mut b4 = buf;
    b4[6] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        wrote > 0
            && matches!(V2LayoutRec::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2LayoutRec::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2LayoutRec::from_bytes(&b3[..wrote]), Err(V2SaveErr::BadLen))
            && matches!(V2LayoutRec::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 横移帧清单（验主册「桌面整体横移 320ms（F124 强调曲线）」：
    //    首帧 0、末帧未到满程、幅度单调不减、方向带符号）。
    let frames = v2_switch_frames(MotionPolicy::normal(), -1, 1920, 320);
    let mono = frames.windows(2).all(|p| p[1].abs() >= p[0].abs());
    set.add(
        "v2 switch frames: Emphasis curve, signed, monotonic",
        frames[0] == 0 && frames[19].abs() < 1920 && frames[19] < 0 && mono,
        "",
    );

    // 4. 音频豁免（验主册「视频声音不断」：真实切换前后音频指纹相等）。
    let mut vd2 = VirtualDesk::new(MotionPolicy::normal());
    let _ = vd2.audio_open(0x2001);
    let fp0 = vd2.audio_fingerprint();
    let _ = vd2.switch_to(1, 0);
    let _ = vd2.tick(320);
    let exempt = V2AudioExempt { fp_before: fp0, fp_after: vd2.audio_fingerprint() };
    set.add("v2 audio exempt across real switch", exempt.ok() && w1 > 0, "");

    // 5. 归属位图与窗口数一致（位图置位数 = win_count——两面同一事实）。
    let bits = rec.desks[..rec.count]
        .iter()
        .all(|d| V2LayoutRec::win_bitmap(d).count_ones() as usize == d.win_count);
    set.add("v2 win bitmap popcount == win_count", bits && rec.count == 2, "");

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_layout_rec_roundtrip_exact() {
        let mut vd = VirtualDesk::new(MotionPolicy::normal());
        let _ = vd.place_window(0, Rect::new(1, 2, 3, 4), false);
        let rec = V2LayoutRec::capture(&vd);
        let mut buf = [0u8; V2_REC_MAX];
        let n = rec.to_bytes(&mut buf).unwrap();
        let back = V2LayoutRec::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back.count, rec.count);
        assert_eq!(back.desks[0].wins[0].map(|w| w.rect), Some(Rect::new(1, 2, 3, 4)));
    }

    #[test]
    fn v2_frames_direction_signs() {
        let left = v2_switch_frames(MotionPolicy::normal(), -1, 1000, 320);
        let right = v2_switch_frames(MotionPolicy::normal(), 1, 1000, 320);
        assert!(left[10] < 0 && right[10] > 0);
        assert_eq!(left[10].abs(), right[10].abs());
    }

    #[test]
    fn vdesk_v2_selfcheck_all_green() {
        let s = run_vdesk_v2_checks();
        assert!(s.all_passed(), "F235 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
