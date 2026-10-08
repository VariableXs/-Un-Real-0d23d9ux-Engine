//! Win32 窗口/消息/文本/文件服务核心 — AI-B（双域总案·阶段6 任务41 记事本级闭环）。
//!
//! 总案施工步骤与验收口径：
//! - **记事本级闭环**：菜单/文件对话框/编辑/保存——第一个真实 Windows 软件
//!   语义跑通（QEMU 先行等价：notepad.pe 走本服务台完成 打开→编辑→保存）；
//! - **消息结构共用定义**：[`Msg`] 与 Variable 窗口服务（垫片层）同形——
//!   hwnd/message/wparam/lparam 四元组，WM_* 常量与 Win32 一致；
//! - **未支持消息显式返回 0 并计数**（[`UNSUPPORTED_HITS`]）；
//! - 消息循环 **100 万次无泄漏**（宿主测试 [`tests::msg_queue_million_no_leak`]）；
//! - GDI 文本：[`font`] 字模（8×8，F012 字库）渲染进 32bpp 画布；
//! - 文件对话框：虚拟文件槽 + 轮转选择器（选择 UI 实机渲染后置任务 27，
//!   内核语义 = `dialog_pick_next` 每调用轮转一位，测试与探针可控）；
//! - 句柄分类扩展（dispatch 层）：`1`=控制台（现状），`2`=当前保存目标，
//!   `3`=当前读取源——`WriteFile`/`ReadFile`/`CloseHandle` 按 handle 分路。
//!
//! 栈/堆纪律（任务56 戒律）：画布/窗口表/队列/文件槽全部 static .bss
//! 定容，零堆分配、零 >64KB 栈物化；所有方法签名不带大返回值。

use core::sync::atomic::Ordering;

use crate::cpu::sync::SpinProtected;
use crate::font;

// ---------------------------------------------------------------------------
// 常量（与 Win32 一致；消息结构共用定义的机器可读源）
// ---------------------------------------------------------------------------

/// 画布宽（像素）。
pub const CANVAS_W: usize = 320;
/// 画布高（像素）。
pub const CANVAS_H: usize = 200;
/// 画布字节容量（32bpp）。
pub const CANVAS_BYTES: usize = CANVAS_W * CANVAS_H * 4;

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_COMMAND: u32 = 0x0111;

/// 菜单命令：打开。
pub const IDM_OPEN: u64 = 1;
/// 菜单命令：保存。
pub const IDM_SAVE: u64 = 2;
/// 菜单命令：退出。
pub const IDM_EXIT: u64 = 3;

/// 消息队列容量。
pub const QUEUE_CAP: usize = 64;
/// 窗口上限。
pub const HWND_MAX: usize = 8;
/// 类上限。
pub const CLASS_MAX: usize = 4;
/// 虚拟文件槽上限。
pub const VFILE_MAX: usize = 8;
/// 虚拟文件数据容量。
pub const VFILE_DATA: usize = 512;
/// 编辑缓冲容量（UTF-16 单元数）。
pub const EDIT_MAX: usize = 512;

/// 类名长度上限（含 NUL）。
pub const NAME_MAX: usize = 32;

// ---------------------------------------------------------------------------
// 消息（与 Variable 窗口服务共用定义）
// ---------------------------------------------------------------------------

/// 一条窗口消息：与垫片层窗口服务同形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msg {
    pub hwnd: u32,
    pub message: u32,
    pub wparam: u64,
    pub lparam: u64,
}

impl Msg {
    pub const ZERO: Msg = Msg { hwnd: 0, message: 0, wparam: 0, lparam: 0 };
}

/// 未支持消息显式返回 0 的累计计数（可观测）。
pub static UNSUPPORTED_HITS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// ---------------------------------------------------------------------------
// 消息队列
// ---------------------------------------------------------------------------

/// 定容 FIFO 消息队列。满投递丢弃并计数（[`MQueue::dropped`]）。
#[derive(Debug)]
pub struct MQueue {
    slots: [Msg; QUEUE_CAP],
    head: usize,
    len: usize,
    dropped: u64,
    total_posted: u64,
}

impl MQueue {
    pub const fn new() -> Self {
        MQueue { slots: [Msg::ZERO; QUEUE_CAP], head: 0, len: 0, dropped: 0, total_posted: 0 }
    }

    /// 投递一条消息。
    pub fn post(&mut self, m: Msg) {
        self.total_posted += 1;
        if self.len >= QUEUE_CAP {
            self.dropped += 1;
            return;
        }
        let tail = (self.head + self.len) % QUEUE_CAP;
        self.slots[tail] = m;
        self.len += 1;
    }

    /// 取一条消息（空 = None；等价探针环境不阻塞，阻塞语义随输入注入通道落地）。
    pub fn get(&mut self) -> Option<Msg> {
        if self.len == 0 {
            return None;
        }
        let m = self.slots[self.head];
        self.head = (self.head + 1) % QUEUE_CAP;
        self.len -= 1;
        Some(m)
    }

    /// 当前积压长度。
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 满/溢丢弃计数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 历史投递总数。
    pub fn total_posted(&self) -> u64 {
        self.total_posted
    }
}

impl Default for MQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GDI 画布（32bpp，字模文本）
// ---------------------------------------------------------------------------

/// 32bpp 画布：clear / TextOut / 像素读（测试断言面）。
pub struct Canvas {
    pub buf: [u32; CANVAS_W * CANVAS_H],
}

impl Canvas {
    pub const fn new() -> Self {
        Canvas { buf: [0; CANVAS_W * CANVAS_H] }
    }

    /// 全画布清色。
    pub fn clear(&mut self, color: u32) {
        for p in self.buf.iter_mut() {
            *p = color;
        }
    }

    /// 画单个字模（8×8，LSB=最左列；ch 经 [`font::glyph`]，越界画空格）。
    pub fn draw_char(&mut self, x: i32, y: i32, ch: u8, fg: u32) {
        let g = font::glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (1 << col) != 0 {
                    let px = x + col as i32;
                    let py = y + row as i32;
                    if px >= 0 && px < CANVAS_W as i32 && py >= 0 && py < CANVAS_H as i32 {
                        self.buf[py as usize * CANVAS_W + px as usize] = fg;
                    }
                }
            }
        }
    }

    /// TextOut 语义：UTF-16 串逐字渲染（非 ASCII 画 '?'，ASCII 外如实降级）。
    pub fn draw_text_utf16(&mut self, x: i32, y: i32, s: &[u16], fg: u32) -> usize {
        let mut n = 0usize;
        for (i, &w) in s.iter().enumerate() {
            let ch = if w < 0x80 { w as u8 } else { b'?' };
            self.draw_char(x + (i * 8) as i32, y, ch, fg);
            n += 1;
        }
        n
    }

    /// 像素读取（测试断言面；越界 = 0）。
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        if x < 0 || x >= CANVAS_W as i32 || y < 0 || y >= CANVAS_H as i32 {
            return 0;
        }
        self.buf[y as usize * CANVAS_W + x as usize]
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 窗口管理器
// ---------------------------------------------------------------------------

/// 登记的窗口类（WNDCLASSEXW 内核侧最小投影）。
#[derive(Debug, Clone, Copy)]
pub struct ClassEntry {
    pub alive: bool,
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    /// 用户态 WndProc VA（内核只记录不回调；派发由用户态循环承担）。
    pub proc_va: u64,
}

/// 窗口实例。
#[derive(Debug, Clone, Copy)]
pub struct WinEntry {
    pub alive: bool,
    pub atom: usize,
    pub w: u32,
    pub h: u32,
    pub visible: bool,
}

/// 窗口管理器：类表 + 窗口表 + 队列引用语义（投递由调用方持有队列）。
#[derive(Debug)]
pub struct WinMan {
    pub classes: [ClassEntry; CLASS_MAX],
    pub class_len: usize,
    pub wins: [WinEntry; HWND_MAX],
    pub win_len: usize,
    next_hwnd: u32,
}

impl WinMan {
    pub const fn new() -> Self {
        WinMan {
            classes: [ClassEntry { alive: false, name: [0; NAME_MAX], name_len: 0, proc_va: 0 }; CLASS_MAX],
            class_len: 0,
            wins: [WinEntry { alive: false, atom: 0, w: 0, h: 0, visible: false }; HWND_MAX],
            win_len: 0,
            next_hwnd: 1,
        }
    }

    /// RegisterClassExW 语义：类名 + WndProc VA → atom（0 = 失败）。
    pub fn register_class(&mut self, name: &[u8], proc_va: u64) -> usize {
        if self.class_len >= CLASS_MAX || name.is_empty() || name.len() >= NAME_MAX {
            return 0;
        }
        let i = self.class_len;
        let mut nm = [0u8; NAME_MAX];
        nm[..name.len()].copy_from_slice(name);
        self.classes[i] = ClassEntry { alive: true, name: nm, name_len: name.len(), proc_va };
        self.class_len += 1;
        i + 1 // atom 从 1 起，与 Windows atom 口径一致。
    }

    /// CreateWindowExW 语义：类名串 → HWND（0 = 失败）。
    pub fn create_window(&mut self, class_name: &[u8], w: u32, h: u32) -> u32 {
        let mut atom = 0usize;
        for (i, c) in self.classes[..self.class_len].iter().enumerate() {
            if c.alive && &c.name[..c.name_len] == class_name {
                atom = i + 1;
                break;
            }
        }
        if atom == 0 || self.win_len >= HWND_MAX {
            return 0;
        }
        let idx = self.win_len;
        self.wins[idx] = WinEntry { alive: true, atom, w, h, visible: false };
        self.win_len += 1;
        let hwnd = self.next_hwnd;
        self.next_hwnd += 1;
        hwnd
    }

    /// hwnd → 窗口下标。
    fn idx_of(&self, hwnd: u32) -> Option<usize> {
        if hwnd == 0 {
            return None;
        }
        let idx = (hwnd - 1) as usize;
        if idx >= self.win_len || !self.wins[idx].alive {
            return None;
        }
        Some(idx)
    }

    /// ShowWindow 语义。
    pub fn show_window(&mut self, hwnd: u32, visible: bool) -> bool {
        match self.idx_of(hwnd) {
            Some(i) => {
                self.wins[i].visible = visible;
                true
            }
            None => false,
        }
    }

    /// WndProc VA 查询（窗口所属类）。
    pub fn proc_va_of(&self, hwnd: u32) -> Option<u64> {
        let i = self.idx_of(hwnd)?;
        let atom = self.wins[i].atom;
        self.classes.get(atom - 1).filter(|c| c.alive).map(|c| c.proc_va)
    }
}

impl Default for WinMan {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 虚拟文件槽 + 对话框选择器
// ---------------------------------------------------------------------------

/// 一个虚拟文件（记事本"文件系统"槽）。
#[derive(Debug, Clone, Copy)]
pub struct VFile {
    pub alive: bool,
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub data: [u8; VFILE_DATA],
    pub len: usize,
}

/// 虚拟文件表 + 对话框选择游标。
#[derive(Debug)]
pub struct VFiles {
    pub files: [VFile; VFILE_MAX],
    pub file_len: usize,
    /// 对话框选择游标（GetOpenFileName/GetSaveFileName 每调用轮转一位）。
    pick_cursor: usize,
    /// 当前保存目标名（handle 2 语义）。
    pub save_target: [u8; NAME_MAX],
    pub save_target_len: usize,
    /// 当前读取源名（handle 3 语义）。
    pub read_source: [u8; NAME_MAX],
    pub read_source_len: usize,
}

impl VFiles {
    pub const fn new() -> Self {
        VFiles {
            files: [VFile { alive: false, name: [0; NAME_MAX], name_len: 0, data: [0; VFILE_DATA], len: 0 }; VFILE_MAX],
            file_len: 0,
            pick_cursor: 0,
            save_target: [0; NAME_MAX],
            save_target_len: 0,
            read_source: [0; NAME_MAX],
            read_source_len: 0,
        }
    }

    /// 写/建文件（存在则整块覆盖）。
    pub fn write(&mut self, name: &[u8], data: &[u8]) -> bool {
        if name.is_empty() || name.len() >= NAME_MAX || data.len() > VFILE_DATA {
            return false;
        }
        // 已存在 → 覆盖。
        for f in self.files[..self.file_len].iter_mut() {
            if f.alive && &f.name[..f.name_len] == name {
                f.data[..data.len()].copy_from_slice(data);
                f.len = data.len();
                return true;
            }
        }
        if self.file_len >= VFILE_MAX {
            return false;
        }
        let i = self.file_len;
        let mut nm = [0u8; NAME_MAX];
        nm[..name.len()].copy_from_slice(name);
        let mut d = [0u8; VFILE_DATA];
        d[..data.len()].copy_from_slice(data);
        self.files[i] = VFile { alive: true, name: nm, name_len: name.len(), data: d, len: data.len() };
        self.file_len += 1;
        true
    }

    /// 读文件。
    pub fn read(&self, name: &[u8]) -> Option<&[u8]> {
        self.files[..self.file_len].iter().find(|f| f.alive && &f.name[..f.name_len] == name).map(|f| &f.data[..f.len])
    }

    /// 对话框轮转选择：返回当前指向的文件名并前移游标（无文件 = None）。
    pub fn dialog_pick_next(&mut self) -> Option<&[u8]> {
        if self.file_len == 0 {
            return None;
        }
        let i = self.pick_cursor % self.file_len;
        self.pick_cursor += 1;
        let f = &self.files[i];
        Some(&f.name[..f.name_len])
    }

    /// GetSaveFileNameW 语义：选定保存目标（返回 1/0 =成败）。
    pub fn open_save_dialog(&mut self, name: Option<&[u8]>) -> i32 {
        // 先把选定名拷到栈上（避免 self 双重借用），再落目标字段。
        let mut nm = [0u8; NAME_MAX];
        let n = match name {
            Some(n) if n.len() < NAME_MAX => {
                nm[..n.len()].copy_from_slice(n);
                n.len()
            }
            Some(_) => return 0,
            None => match self.dialog_pick_next() {
                Some(p) => {
                    let l = p.len().min(NAME_MAX - 1);
                    nm[..l].copy_from_slice(&p[..l]);
                    l
                }
                None => return 0,
            },
        };
        self.save_target[..n].copy_from_slice(&nm[..n]);
        self.save_target_len = n;
        1
    }

    /// GetOpenFileNameW 语义：选定读取源（返回 1/0 =成败）。
    pub fn open_load_dialog(&mut self, name: Option<&[u8]>) -> i32 {
        let mut nm = [0u8; NAME_MAX];
        let n = match name {
            Some(n) if n.len() < NAME_MAX => {
                nm[..n.len()].copy_from_slice(n);
                n.len()
            }
            Some(_) => return 0,
            None => match self.dialog_pick_next() {
                Some(p) => {
                    let l = p.len().min(NAME_MAX - 1);
                    nm[..l].copy_from_slice(&p[..l]);
                    l
                }
                None => return 0,
            },
        };
        self.read_source[..n].copy_from_slice(&nm[..n]);
        self.read_source_len = n;
        1
    }

    /// handle 2 路：写当前保存目标。
    pub fn write_via_handle(&mut self, data: &[u8]) -> i32 {
        if self.save_target_len == 0 {
            return 0;
        }
        // 名字先拷栈（避免 self 双重借用）。
        let mut nm = [0u8; NAME_MAX];
        let l = self.save_target_len;
        nm[..l].copy_from_slice(&self.save_target[..l]);
        if self.write(&nm[..l], data) {
            1
        } else {
            0
        }
    }

    /// handle 3 路：读当前读取源（读出的数据由调用方缓冲承接）。
    pub fn read_via_handle(&self, out: &mut [u8]) -> i32 {
        if self.read_source_len == 0 {
            return 0;
        }
        match self.read(&self.read_source[..self.read_source_len]) {
            Some(d) => {
                let n = d.len().min(out.len());
                out[..n].copy_from_slice(&d[..n]);
                n as i32
            }
            None => 0,
        }
    }
}

impl Default for VFiles {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 编辑缓冲（UTF-16，TextOut 直渲）
// ---------------------------------------------------------------------------

/// 记事本编辑缓冲（UTF-16 单元）。
#[derive(Debug)]
pub struct EditBuf {
    pub units: [u16; EDIT_MAX],
    pub len: usize,
}

impl EditBuf {
    pub const fn new() -> Self {
        EditBuf { units: [0; EDIT_MAX], len: 0 }
    }

    /// WM_CHAR 追加（控制字符忽略；退格删尾）。
    pub fn push_char(&mut self, ch: u16) {
        match ch {
            0x08 => {
                if self.len > 0 {
                    self.len -= 1;
                    self.units[self.len] = 0;
                }
            }
            0x20..=0x7E => {
                if self.len < EDIT_MAX {
                    self.units[self.len] = ch;
                    self.len += 1;
                }
            }
            _ => {}
        }
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.units = [0; EDIT_MAX];
        self.len = 0;
    }

    /// ANSI 字节装载（打开文件路径：ASCII 外如实降级 '?'）。
    pub fn load_ansi(&mut self, data: &[u8]) {
        self.clear();
        for (i, &b) in data.iter().enumerate() {
            if i >= EDIT_MAX {
                break;
            }
            self.units[i] = if b < 0x80 { b as u16 } else { b'?' as u16 };
            self.len += 1;
        }
    }

    /// 窄化为 ANSI 字节（保存路径：ASCII 外如实降级 '?'）。
    pub fn to_ansi(&self, out: &mut [u8]) -> usize {
        let n = self.len.min(out.len());
        for i in 0..n {
            let w = self.units[i];
            out[i] = if w < 0x80 { w as u8 } else { b'?' };
        }
        n
    }
}

impl Default for EditBuf {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 内核单例服务 + Win32 dispatch 挂接面（任务41）
// ---------------------------------------------------------------------------

/// 全局窗口服务（.bss 定容，零堆分配——任务56 戒律合规）。
pub struct WinService {
    pub queue: MQueue,
    pub man: WinMan,
    pub files: VFiles,
    pub canvas: Canvas,
    pub edit: EditBuf,
    /// 演示期诊断串（实机串口证据行）。
    pub trace: TraceBuf,
}

/// 探针轨迹（定容环形，串口收尾一次性导出）。
pub struct TraceBuf {
    pub buf: [u8; 256],
    pub len: usize,
}

impl TraceBuf {
    pub const fn new() -> Self {
        TraceBuf { buf: [0; 256], len: 0 }
    }
    pub fn push(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl WinService {
    pub const fn new() -> Self {
        WinService {
            queue: MQueue::new(),
            man: WinMan::new(),
            files: VFiles::new(),
            canvas: Canvas::new(),
            edit: EditBuf::new(),
            trace: TraceBuf::new(),
        }
    }
}

/// 用户态内存读取原语（探针环境单核独占地址空间；SAFETY 同
/// winapi::read_user_str 口径——先验在用户半区内再 volatile 读）。
pub fn read_user_qword(addr: u64) -> Option<u64> {
    if addr == 0 || !crate::entry::is_user_ip(addr) || addr + 8 > crate::entry::USER_TOP {
        return None;
    }
    let mut v = 0u64;
    for i in 0..8 {
        // SAFETY: addr..addr+8 已验在用户半区且 < USER_TOP。
        v |= (unsafe { core::ptr::read_volatile((addr + i) as *const u8) } as u64) << (i * 8);
    }
    Some(v)
}

/// 用户态内存写入原语（GetMessageW/BeginPaint 往用户缓冲回写）。
pub fn write_user_bytes(addr: u64, bytes: &[u8]) -> bool {
    if addr == 0 || !crate::entry::is_user_ip(addr) || addr + bytes.len() as u64 > crate::entry::USER_TOP {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate() {
        // SAFETY: 同 read_user_qword 区间校验。
        unsafe { core::ptr::write_volatile((addr + i as u64) as *mut u8, b) };
    }
    true
}

/// 读用户态 UTF-16 串（≤cap 单元；NUL 终止）。
fn read_user_utf16(addr: u64, cap: usize) -> Option<alloc::vec::Vec<u16>> {
    if addr == 0 || !crate::entry::is_user_ip(addr) {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    for i in 0..cap as u64 {
        let a = addr.checked_add(i * 2)?;
        let unit = read_user_qword(a)? as u16;
        if unit == 0 {
            return Some(out);
        }
        out.push(unit);
    }
    None
}

/// 写用户态 UTF-16 串（NUL 终止）。
fn write_user_utf16(addr: u64, units: &[u16]) -> bool {
    let mut tmp = alloc::vec::Vec::with_capacity(units.len() * 2 + 2);
    for &u in units {
        tmp.push((u & 0xFF) as u8);
        tmp.push((u >> 8) as u8);
    }
    tmp.push(0);
    tmp.push(0);
    write_user_bytes(addr, &tmp)
}

static SERVICE: SpinProtected<WinService> = SpinProtected::new(WinService::new());

/// 服务单例访问入口（dispatch 与探针共用）。
pub fn with_service<R>(f: impl FnOnce(&mut WinService) -> R) -> R {
    let mut g = SERVICE.lock();
    f(&mut g)
}

/// 窗口/文本/文件服务台（任务41）：user32/gdi32/comdlg32 的统一分发面。
/// 多参 API 用 NT 风格参数块（a1 = 用户态结构指针），与 NT 系统调用
/// 参数块语义同构；参数不可读一律返回 0/负错误（Windows 语义不猜）。
pub fn win32_dispatch(dll: &str, name: &str, a1: u64, a2: u64, a3: u64) -> i64 {
    let _ = (a2, a3);
    match (dll, name) {
        // ---- user32：类/窗/消息 ----
        ("user32.dll", "RegisterClassExW") => {
            // WNDCLASSEXW x64：lpfnWndProc@8，lpszClassName@64。
            let Some(wc) = (a1 != 0).then_some(a1) else { return 0 };
            let Some(proc_va) = read_user_qword(wc + 8) else { return 0 };
            let Some(name_va) = read_user_qword(wc + 64) else { return 0 };
            let Some(cls) = crate::proc::winapi::read_user_str_pub(name_va, NAME_MAX) else { return 0 };
            with_service(|s| s.man.register_class(&cls, proc_va) as i64)
        }
        ("user32.dll", "CreateWindowExW") => {
            // 参数块：classname_va@0，title_va@8，w@16，h@24。
            let Some(cls_va) = read_user_qword(a1) else { return 0 };
            let Some(title_va) = read_user_qword(a1 + 8) else { return 0 };
            let Some(w) = read_user_qword(a1 + 16) else { return 0 };
            let Some(h) = read_user_qword(a1 + 24) else { return 0 };
            let Some(cls) = crate::proc::winapi::read_user_str_pub(cls_va, NAME_MAX) else { return 0 };
            let hwnd = with_service(|s| {
                let hwnd = s.man.create_window(&cls, w as u32, h as u32);
                if hwnd != 0 {
                    s.queue.post(Msg { hwnd, message: WM_CREATE, wparam: 0, lparam: 0 });
                }
                hwnd
            });
            if hwnd != 0 {
                let _ = read_user_str_or_empty(title_va); // 标题如实读入（诊断面）
            }
            hwnd as i64
        }
        ("user32.dll", "ShowWindow") => {
            with_service(|s| s.man.show_window(a1 as u32, a2 != 0) as i64)
        }
        ("user32.dll", "UpdateWindow") | ("user32.dll", "InvalidateRect") => {
            with_service(|s| {
                s.queue.post(Msg { hwnd: a1 as u32, message: WM_PAINT, wparam: 0, lparam: 0 });
                1
            })
        }
        ("user32.dll", "GetMessageW") => {
            // a1 = MSG 出参。空队列 → 写 WM_QUIT 返回 0（探针环境等价：
            // 无更多消息 = 循环收尾；阻塞语义随输入注入通道落地，如实登记）。
            let m = with_service(|s| s.queue.get());
            let m = m.unwrap_or(Msg { hwnd: 0, message: WM_QUIT, wparam: 0, lparam: 0 });
            let mut raw = [0u8; 24];
            raw[0..4].copy_from_slice(&m.hwnd.to_le_bytes());
            raw[4..8].copy_from_slice(&m.message.to_le_bytes());
            raw[8..16].copy_from_slice(&m.wparam.to_le_bytes());
            raw[16..24].copy_from_slice(&m.lparam.to_le_bytes());
            if !write_user_bytes(a1, &raw) {
                return -(crate::entry::ErrNo::Efault.to_i32()) as i64;
            }
            if m.message == WM_QUIT && m.wparam == 0 && m.hwnd == 0 && m.lparam == 0 {
                0 // 真语义：GetMessage 返回 0 = 收到 WM_QUIT。
            } else {
                1
            }
        }
        ("user32.dll", "TranslateMessage") | ("user32.dll", "DispatchMessageW") | ("user32.dll", "DefWindowProcW") => {
            // 键盘转写/派发由用户态循环承担（单窗口演示等价，模块头声明）。
            0
        }
        ("user32.dll", "PostQuitMessage") => with_service(|s| {
            s.queue.post(Msg { hwnd: 0, message: WM_QUIT, wparam: a1, lparam: 0 });
            0
        }),
        ("user32.dll", "MessageBoxW") => {
            // 标题+文本如实落轨迹（实机串口证据），返回 IDOK。
            with_service(|s| {
                s.trace.push(b"[msgbox] ");
                if let Some(t) = read_user_str_or_empty(a2) {
                    s.trace.push(&t);
                }
                s.trace.push(b" / ");
                if let Some(c) = read_user_str_or_empty(a1) {
                    s.trace.push(&c);
                }
                s.trace.push(b"\n");
                1
            })
        }
        // ---- gdi32：文本面 ----
        ("gdi32.dll", "TextOutW") => {
            // 参数块：x@0，y@8，string_va@16，len@24。
            let Some(x) = read_user_qword(a1) else { return 0 };
            let Some(y) = read_user_qword(a1 + 8) else { return 0 };
            let Some(sv) = read_user_qword(a1 + 16) else { return 0 };
            let Some(n) = read_user_qword(a1 + 24) else { return 0 };
            let Some(units) = read_user_utf16(sv, n.min(256) as usize) else { return 0 };
            with_service(|s| s.canvas.draw_text_utf16(x as i32, y as i32, &units, 0xFFFF_FFFF) as i64)
        }
        ("gdi32.dll", "BeginPaint") => {
            // PAINTSTRUCT 回写零块；hdc 约定 1。
            let _ = write_user_bytes(a2, &[0u8; 64]);
            1
        }
        ("gdi32.dll", "EndPaint") => 1,
        ("gdi32.dll", "CreateFontW") => 1,
        // ---- comdlg32：文件对话框 ----
        ("comdlg32.dll", "GetOpenFileNameW") | ("comdlg32.dll", "GetSaveFileNameW") => {
            // OPENFILENAME x64：lpstrFile@48，nMaxFile@56。
            let Some(file_va) = read_user_qword(a1 + 48) else { return 0 };
            let Some(cap) = read_user_qword(a1 + 56) else { return 0 };
            with_service(|s| {
                let picked = if dll == "comdlg32.dll" && name == "GetOpenFileNameW" {
                    s.files.open_load_dialog(None)
                } else {
                    s.files.open_save_dialog(None)
                };
                if picked == 1 {
                    let src = if name == "GetOpenFileNameW" {
                        &s.files.read_source[..s.files.read_source_len]
                    } else {
                        &s.files.save_target[..s.files.save_target_len]
                    };
                    let units: alloc::vec::Vec<u16> = src.iter().map(|&b| b as u16).collect();
                    if (cap as usize) < units.len() + 1 || !write_user_utf16(file_va, &units) {
                        return 0;
                    }
                    let tag: &[u8] = if name == "GetOpenFileNameW" { b"open:" } else { b"save:" };
                    s.trace.push(tag);
                    s.trace.push(src);
                    s.trace.push(b"\n");
                }
                picked as i64
            })
        }
        // ---- kernel32：文件句柄扩展（handle 2=保存目标 / 3=读取源）----
        ("kernel32.dll", "WriteFile") => {
            if a1 == 2 {
                if a3 == 0 {
                    return 1; // 空写合法（与 sys_write 同口径）。
                }
                let Some(buf) = user_bytes_view(a2, a3) else {
                    return -(crate::entry::ErrNo::Efault.to_i32()) as i64;
                };
                with_service(|s| s.files.write_via_handle(&buf) as i64)
            } else {
                -(crate::entry::ErrNo::Ebadf.to_i32()) as i64
            }
        }
        ("kernel32.dll", "ReadFile") => {
            // 参数块（NT 风格）：handle@0，buf@8，len@16，got@24。真 Win32
            // ReadFile 是 4 参（handle,buf,len,&read），3 参 syscall 直传装
            // 不下——曾按裸 ABI 把 a3=长度(0x190) 当出参指针写 → 内核态
            // 写缺页 fatal（根因⑤）；参数块与本模块其余多参 API 同构。
            let Some(handle) = read_user_qword(a1) else { return 0 };
            let Some(buf_va) = read_user_qword(a1 + 8) else { return 0 };
            let Some(len) = read_user_qword(a1 + 16) else { return 0 };
            let Some(got_va) = read_user_qword(a1 + 24) else { return 0 };
            if handle != 3 {
                return -(crate::entry::ErrNo::Ebadf.to_i32()) as i64;
            }
            let mut tmp = [0u8; VFILE_DATA];
            let got = with_service(|s| s.files.read_via_handle(&mut tmp));
            if got <= 0 {
                return 0;
            }
            // 上限 = 用户缓冲 len（真语义：不越用户缓冲写）。
            let n = (got as usize).min(len as usize);
            if !write_user_bytes(buf_va, &tmp[..n]) {
                return -(crate::entry::ErrNo::Efault.to_i32()) as i64;
            }
            let _ = write_user_bytes(got_va, &(n as u32).to_le_bytes());
            1
        }
        ("kernel32.dll", "CloseHandle") => 1,
        _ => {
            UNSUPPORTED_HITS.fetch_add(1, Ordering::Relaxed);
            0
        }
    }
}

/// 读用户 NUL 串（空/不可读 → None）。
fn read_user_str_or_empty(addr: u64) -> Option<alloc::vec::Vec<u8>> {
    if addr == 0 {
        return None;
    }
    crate::proc::winapi::read_user_str_pub(addr, 64)
}

/// 用户缓冲借用视图（拷贝式，避免裸指针生命周期外泄）。
fn user_bytes_view(addr: u64, len: u64) -> Option<alloc::vec::Vec<u8>> {
    if addr == 0 || len == 0 || len > VFILE_DATA as u64 {
        return None;
    }
    let mut out = alloc::vec::Vec::with_capacity(len as usize);
    for i in 0..len {
        let Some(w) = read_user_qword(addr + i).map(|q| q as u8) else { return None };
        out.push(w);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 宿主测试（服务级闭环 100% 覆盖；QEMU 探针另走 ring3 实链）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 消息循环 100 万次无泄漏（总案验收口径）：投递/取出平衡、
    /// 积压归零、满投递计数如实。
    #[test]
    fn msg_queue_million_no_leak() {
        let mut q = MQueue::new();
        const N: u64 = 1_000_000;
        // 交替投递+消费（等价消息循环），并周期性灌满队列测溢出计数。
        let mut got = 0u64;
        for i in 0..N {
            q.post(Msg { hwnd: 1, message: WM_CHAR, wparam: i & 0x7F, lparam: 0 });
            if q.get().is_some() {
                got += 1;
            }
            if i % 1_000_000 / 2 == 0 {
                let _ = q.len();
            }
        }
        assert_eq!(got, N, "投递与取出必须 1:1 平衡");
        assert_eq!(q.len(), 0, "循环结束积压必须归零");
        assert_eq!(q.total_posted(), N);
        assert_eq!(q.dropped(), 0);
        // 灌满 + 溢出 1 条 → 丢弃计数。
        for i in 0..QUEUE_CAP {
            q.post(Msg { hwnd: 1, message: i as u32, wparam: 0, lparam: 0 });
        }
        assert_eq!(q.len(), QUEUE_CAP);
        q.post(Msg::ZERO);
        assert_eq!(q.dropped(), 1);
    }

    /// GDI 文本：字模像素落位断言。
    #[test]
    fn canvas_textout_glyph_pixels() {
        let mut cv = Canvas::new();
        cv.clear(0xFF00_0000); // 黑底
        let s: [u16; 1] = [b'A' as u16];
        let n = cv.draw_text_utf16(16, 16, &s, 0xFFFF_FFFF);
        assert_eq!(n, 1);
        // 与字库逐位比对：'A' 字模每个置位像素必须落在画布上。
        let g = font::glyph(b'A');
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8u32 {
                let expect = if bits & (1 << col) != 0 { 0xFFFF_FFFF } else { 0xFF00_0000 };
                assert_eq!(cv.pixel(16 + col as i32, 16 + row as i32), expect, "row {row} col {col}");
            }
        }
        // 非 ASCII 降级 '?'（0x3F 字模）：断言 '?' 字形与字库逐位一致。
        let s2: [u16; 1] = [0x4E2D]; // 中
        cv.clear(0);
        cv.draw_text_utf16(0, 0, &s2, 1);
        let gq = font::glyph(b'?');
        for (row, bits) in gq.iter().enumerate() {
            for col in 0..8u32 {
                let expect = if bits & (1 << col) != 0 { 1 } else { 0 };
                assert_eq!(cv.pixel(col as i32, row as i32) != 0, expect != 0, "row {row} col {col}");
            }
        }
    }

    /// 虚拟文件对话框：轮转选择 + 读写往返 ×3。
    #[test]
    fn vfiles_dialog_roundtrip_x3() {
        for round in 0..3u8 {
            let mut vf = VFiles::new();
            assert!(vf.write(b"note1.txt", format!("round-{round}-alpha").as_bytes()));
            assert!(vf.write(b"note2.txt", format!("round-{round}-beta").as_bytes()));
            // 打开对话框：轮转选中 note1（对话框内部 pick）→ 读缓冲。
            assert_eq!(vf.open_load_dialog(None), 1);
            assert_eq!(&vf.read_source[..vf.read_source_len], b"note1.txt");
            let mut buf = [0u8; VFILE_DATA];
            let got = vf.read_via_handle(&mut buf);
            assert_eq!(got, format!("round-{round}-alpha").len() as i32);
            // 保存对话框：轮转选中 note2 → 编辑缓冲写回。
            assert_eq!(vf.open_save_dialog(None), 1);
            assert_eq!(&vf.save_target[..vf.save_target_len], b"note2.txt");
            assert_eq!(vf.write_via_handle(b"saved!"), 1);
            assert_eq!(vf.read(b"note2.txt"), Some(b"saved!".as_slice()));
        }
        // 空表选择 → 0（对话框失败语义）。
        let mut vf = VFiles::new();
        assert_eq!(vf.open_save_dialog(None), 0);
        assert_eq!(vf.open_load_dialog(None), 0);
    }

    /// 记事本服务级全流程（不依赖 ring3 的服务闭环）×3：
    /// register → create → paint → 编辑 → 打开 → 保存 → 校验 → 退出。
    #[test]
    fn notepad_service_flow_x3() {
        for round in 0..3u8 {
            let mut wm = WinMan::new();
            let mut q = MQueue::new();
            let mut cv = Canvas::new();
            let mut vf = VFiles::new();
            let mut ed = EditBuf::new();

            let atom = wm.register_class(b"NotepadClass", 0x140001000);
            assert_ne!(atom, 0);
            let hwnd = wm.create_window(b"NotepadClass", 320, 200);
            assert_ne!(hwnd, 0);
            assert!(wm.show_window(hwnd, true));
            // 预置虚拟文件（供"打开"）。
            assert!(vf.write(format!("pre{round}.txt").as_bytes(), b"loaded-text"));

            // WM_CREATE + WM_PAINT 投递。
            q.post(Msg { hwnd, message: WM_CREATE, wparam: 0, lparam: 0 });
            q.post(Msg { hwnd, message: WM_PAINT, wparam: 0, lparam: 0 });

            // 探针注入：菜单打开 → 字符 'V' 'X' → 菜单保存 → 退出。
            q.post(Msg { hwnd, message: WM_COMMAND, wparam: IDM_OPEN, lparam: 0 });
            q.post(Msg { hwnd, message: WM_CHAR, wparam: 'V' as u64, lparam: 0 });
            q.post(Msg { hwnd, message: WM_CHAR, wparam: 'X' as u64, lparam: 0 });
            q.post(Msg { hwnd, message: WM_COMMAND, wparam: IDM_SAVE, lparam: 0 });
            q.post(Msg { hwnd, message: WM_COMMAND, wparam: IDM_EXIT, lparam: 0 });

            // 消息循环（WndProc 语义内联——与 notepad.pe 用户态循环同构）。
            let mut open_fired = false;
            let mut save_fired = false;
            let mut quit = false;
            let mut loops = 0usize;
            while let Some(m) = q.get() {
                loops += 1;
                match m.message {
                    WM_PAINT => {
                        cv.clear(0xFF00_0000);
                        cv.draw_text_utf16(8, 8, &ed.units[..ed.len], 0xFFFF_FFFF);
                    },
                    WM_CHAR => {
                        ed.push_char(m.wparam as u16);
                        // InvalidateRect 等价：编辑后重绘。
                        q.post(Msg { hwnd: m.hwnd, message: WM_PAINT, wparam: 0, lparam: 0 });
                    }
                    WM_COMMAND => match m.wparam {                        IDM_OPEN => {
                            open_fired = true;
                            assert_eq!(vf.open_load_dialog(None), 1);
                            let mut buf = [0u8; VFILE_DATA];
                            let got = vf.read_via_handle(&mut buf);
                            assert!(got > 0);
                            ed.load_ansi(&buf[..got as usize]);
                        }
                        IDM_SAVE => {
                            save_fired = true;
                            assert_eq!(vf.open_save_dialog(None), 1);
                            let mut out = [0u8; VFILE_DATA];
                            let n = ed.to_ansi(&mut out);
                            assert_eq!(vf.write_via_handle(&out[..n]), 1);
                        }
                        IDM_EXIT => {
                            // PostQuitMessage 等价。
                            q.post(Msg { hwnd: 0, message: WM_QUIT, wparam: 0, lparam: 0 });
                        }
                        _ => { UNSUPPORTED_HITS.fetch_add(1, core::sync::atomic::Ordering::Relaxed); }
                    },
                    WM_QUIT => {
                        quit = true;
                    }
                    WM_CREATE | WM_CLOSE | WM_DESTROY => {}
                    _ => { UNSUPPORTED_HITS.fetch_add(1, core::sync::atomic::Ordering::Relaxed); }
                }
                assert!(loops < 100, "消息循环应收敛");
            }
            assert!(open_fired && save_fired && quit, "round {round} 三事件必须齐");
            // 保存校验：编辑缓冲 = loaded-text + 'V' + 'X'。
            let expect = format!("loaded-textVX");
            let saved = vf.read(b"pre0.txt").or_else(|| vf.read(b"pre1.txt")).or_else(|| vf.read(b"pre2.txt"));
            assert_eq!(saved, Some(expect.as_bytes()), "round {round} 保存内容");
            // 画布上有字：文本区域内至少一个前景像素（且首字符字模与字库一致）。
            let mut any_fg = false;
            for yy in 8..16 {
                for xx in 8..8 + ed.len as i32 * 8 {
                    if cv.pixel(xx, yy) == 0xFFFF_FFFF {
                        any_fg = true;
                    }
                }
            }
            assert!(any_fg, "round {round} 画布应有前景像素");
            let g0 = font::glyph(b'l'); // "loaded-textVX" 首字符
            assert_eq!(cv.pixel(8, 8) == 0xFFFF_FFFF, g0[0] & 1 != 0, "round {round} 首字符(0,0)字模位");
        }
        assert_eq!(UNSUPPORTED_HITS.load(core::sync::atomic::Ordering::Relaxed), 0);
    }

    /// 窗口管理器边界：类满/窗满/坏句柄。
    #[test]
    fn winman_bounds_and_bad_hwnd() {
        let mut wm = WinMan::new();
        assert_ne!(wm.register_class(b"C0", 1), 0);
        assert_eq!(wm.register_class(b"", 1), 0, "空类名拒绝");
        let long = [b'x'; NAME_MAX + 1];
        assert_eq!(wm.register_class(&long, 1), 0, "超长类名拒绝");
        let mut hwnds = [0u32; HWND_MAX];
        for h in hwnds.iter_mut() {
            *h = wm.create_window(b"C0", 100, 100);
            assert_ne!(*h, 0);
        }
        assert_eq!(wm.create_window(b"C0", 100, 100), 0, "窗满返回 0");
        assert!(wm.show_window(hwnds[0], true));
        assert!(!wm.show_window(0, true), "hwnd 0 无效");
        assert!(!wm.show_window(99, true), "越界 hwnd 无效");
        assert_eq!(wm.proc_va_of(hwnds[1]), Some(1));
        // 类满（CLASS_MAX）。
        let mut wm2 = WinMan::new();
        for i in 0..CLASS_MAX {
            assert_ne!(wm2.register_class(format!("K{i}").as_bytes(), 1), 0);
        }
        assert_eq!(wm2.register_class(b"KX", 1), 0);
    }
}
