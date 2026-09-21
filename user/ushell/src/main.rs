//! 任务27/28（AI-V）· 用户态 shell 壳程序（ushell）· AI-4 视觉对齐版。
//!
//! 内核嵌入层演示常驻进程：ring3 下经三个内核支点驱动--
//! - SYS_FRAME(16)：绘制命令（fill_rect/text/hline/vline/outline/info）；
//! - SYS_INPUT(17)：shim://input 16B 键鼠事件（AI-4：鼠标消费端闭环——
//!   光标/悬停/点击全量接入，键盘可达性完整保留，双通道对等）；
//! - SYS_SHIM(18)：KV（设置存储）/VFS（SHARED 文件列取读）/boot://event
//!   回放/时钟。
//!
//! 旅程（对应总案阶段3 步骤7/8 走查清单）：
//!   ① Loading：内核拉起即进加载动画（里程碑+平滑进度条），零按键零
//!      选项，≥2s 后直落桌面（Variable 是主系统）
//!   ② 桌面：壁纸 + 任务栏（四格 logo START + uptime 时钟）+ 桌面图标
//!   ③ START → 开始菜单（削角面板+图标色块+底部用户区）→ 文件管理器
//!   ④ START → 设置页（引导行为三参数，KV 存储即时读写，←→/Enter）
//!   ⑤ START → 关于（嵌入层自述 + 运行指标）
//!   ⑥ Restart=重启切回 Windows；Shutdown=关机断电（SYS_POWEROFF）
//!
//! 视觉契约（AI-4 · R2 对齐 Variable win11 外壳）：暗色面板系与
//! tokens.css 同向（PALETTE 索引色已对齐 accent #3874D2）；面板削角
//! 模拟圆角；窗口标题栏 accent 条+[×] 关闭钮+右下阴影；图标语义色块。
//! 全程键盘可达（↑↓←→/Enter/Esc）；鼠标与键盘能力对等；里程碑经串口
//! 打点供验收脚本断言（全部既有标记零改动）。

#![no_std]
#![no_main]

use core::arch::asm;

// ---------------------------------------------------------------------------
// syscall ABI（与内核 proc::syscall 号表逐字一致）
// ---------------------------------------------------------------------------

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 2;
const SYS_FRAME: u64 = 16;
const SYS_INPUT: u64 = 17;
const SYS_SHIM: u64 = 18;
const SYS_REBOOT: u64 = 19;
const SYS_POWEROFF: u64 = 20;

fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn exit(code: u64) -> ! {
    let _ = syscall3(SYS_EXIT, code, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}

/// 串口打点（fd=1；验收脚本断言通道）。
fn puts(buf: &[u8]) {
    let _ = syscall3(SYS_WRITE, 1, buf.as_ptr() as u64, buf.len() as u64);
}

fn marker(buf: &[u8]) {
    puts(buf);
    puts(b"\n");
}

// --- FRAME 命令（与内核 usrshell::pack_* 编码逐字段一致） ---

const OP_TEXT: u64 = 2;
const OP_OUTLINE: u64 = 5;
const OP_INFO: u64 = 6;
/// 真壁纸整屏 blit（内核最近邻缩放）。
const OP_WALLPAPER: u64 = 7;
/// 壁纸像素采样（a2=x|y → 0xRRGGBB）。
const OP_WALLPAPER_PX: u64 = 8;
/// 原色填充（a2=xywh，a3=0xRRGGBB）。
const OP_FILL_RGB: u64 = 9;

// 调色板（与内核 PALETTE 表同序同源）。
const C_WHITE: u64 = 1;
const C_DARK: u64 = 2;
const C_LGRAY: u64 = 3;
const C_BLUE: u64 = 4;
const C_RED: u64 = 6;
const C_WALL0: u64 = 8;
const C_WALL1: u64 = 9;
const C_TASKBAR: u64 = 10;
const C_PANEL: u64 = 11;
const C_HILITE: u64 = 12;
const C_DIM: u64 = 13;
const C_YELLOW: u64 = 7;
const C_CYAN: u64 = 15;

const GLYPH_W: i64 = 8;
const GLYPH_H: i64 = 16;

fn pack_xywh(x: i64, y: i64) -> u64 {
    (x as u64 & 0xFFFF) | ((y as u64 & 0xFFFF) << 16)
}

fn frame(op: u64, color: u64, len: u64, scale: u64, packed: u64, ptr: u64) -> i64 {
    syscall3(
        SYS_FRAME,
        op | (color << 8) | (len << 16) | (scale << 28),
        packed,
        ptr,
    )
}

fn fill_rect(x: i64, y: i64, w: i64, h: i64, color: u64) {
    let _ = frame(
        1,
        color,
        0,
        0,
        pack_xywh(x, y) | ((w as u64 & 0xFFFF) << 32) | ((h as u64 & 0xFFFF) << 48),
        0,
    );
}

fn outline(x: i64, y: i64, w: i64, h: i64, color: u64) {
    let _ = frame(
        OP_OUTLINE,
        color,
        0,
        0,
        pack_xywh(x, y) | ((w as u64 & 0xFFFF) << 32) | ((h as u64 & 0xFFFF) << 48),
        0,
    );
}

fn text(x: i64, y: i64, s: &[u8], color: u64) {
    let _ = frame(OP_TEXT, color, s.len() as u64, 1, pack_xywh(x, y), s.as_ptr() as u64);
}

fn text3(x: i64, y: i64, s: &[u8], color: u64) {
    let _ = frame(OP_TEXT, color, s.len() as u64, 3, pack_xywh(x, y), s.as_ptr() as u64);
}

fn disp_info() -> (i64, i64) {
    let r = frame(OP_INFO, 0, 0, 0, 0, 0);
    (r & 0xFFFF, (r >> 16) & 0xFFFF)
}

// --- INPUT：16B 事件（seq u64 | kind u8 | key u8 | dx i16 | dy i16 | btn u8 | pad u8） ---

const KIND_KEY: u8 = 0;
const KIND_MOUSE: u8 = 1;

// 键字节（与内核 inputsvc::key_byte 同表）：0=Up 1=Down 2=Enter 3=Esc
// 4=Space 5=Bksp 6=Tab 7=Left 8=Right 9/10=Shift 11..36=A..Z 37..46=1..0。
const K_UP: u8 = 0;
const K_DOWN: u8 = 1;
const K_ENTER: u8 = 2;
const K_ESC: u8 = 3;
const K_LEFT: u8 = 7;
const K_RIGHT: u8 = 8;

const IN_MAX: usize = 16;
static mut IN_BUF: [u8; IN_MAX * 16] = [0; IN_MAX * 16];

/// 输入事件（键盘与鼠标对等——S2.05 R2 消费端闭环）。
#[derive(Clone, Copy)]
pub struct InEv {
    pub key: u8,
    pub is_mouse: bool,
    pub dx: i16,
    pub dy: i16,
    pub btn: u8,
}

/// 泵取一批事件（键+鼠标混流；16B 契约逐字段解码）。
fn input_events(out: &mut [InEv; IN_MAX]) -> usize {
    let n = syscall3(SYS_INPUT, core::ptr::addr_of!(IN_BUF) as u64, IN_MAX as u64, 0);
    let n = if n < 0 { 0 } else { (n as usize).min(IN_MAX) };
    unsafe {
        let buf = &*core::ptr::addr_of!(IN_BUF);
        for i in 0..n {
            let base = i * 16;
            let kind = buf[base + 8];
            let key = buf[base + 9];
            let dx = i16::from_le_bytes([buf[base + 10], buf[base + 11]]);
            let dy = i16::from_le_bytes([buf[base + 12], buf[base + 13]]);
            let btn = buf[base + 14];
            out[i] = if kind == KIND_KEY {
                InEv { key, is_mouse: false, dx: 0, dy: 0, btn: 0 }
            } else if kind == KIND_MOUSE {
                InEv { key: 0xFF, is_mouse: true, dx, dy, btn }
            } else {
                InEv { key: 0xFF, is_mouse: false, dx: 0, dy: 0, btn: 0 } // 未知 kind 丢弃
            };
        }
    }
    n
}

// --- SHIM 命令块（布局与内核 usrshell 常量逐字段一致） ---

const CMD_KV_GET: u64 = 1;
const CMD_KV_SET: u64 = 2;
const CMD_VFS_LIST: u64 = 5;
const CMD_VFS_READ: u64 = 6;
const CMD_BOOT_EVENTS: u64 = 7;
const CMD_BOOT_MS: u64 = 8;
const CMD_VFS_SOURCE: u64 = 9;

static mut BLOCK: [u8; 4096] = [0; 4096];

fn blk() -> &'static mut [u8; 4096] {
    unsafe { &mut *core::ptr::addr_of_mut!(BLOCK) }
}

fn shim(cmd: u64) -> i64 {
    syscall3(SYS_SHIM, cmd, blk() as *mut [u8; 4096] as u64, 4096)
}

fn put(off: usize, s: &[u8]) {
    let b = blk();
    let end = (off + s.len()).min(b.len() - 1);
    b[off..end].copy_from_slice(&s[..end - off]);
    b[end] = 0;
}

fn out_u32(off: usize) -> u32 {
    u32::from_le_bytes([blk()[off], blk()[off + 1], blk()[off + 2], blk()[off + 3]])
}

/// 键盘诊断记录（idx=14，2026-09-20 实机取证）解码：Some((live_ms, diag))。
/// 调用前提：BLOCK 刚被 CMD_BOOT_EVENTS 填充。位格式与内核逐位一致：
/// 记录 16B=[idx][state][4..8 活时钟 ms][8..12 诊断字]；诊断字=低 8 位
/// 控制器探针、[23:8] 已收键盘原始字节计数、[31:24] 最后原始字节。
/// 旧内核无此记录 → None（加载动画走帧数兜底，绝不死等）。
fn decode_kbd_rec() -> Option<(u32, u32)> {
    let b = blk();
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    for i in 0..n {
        let off = 4 + i * 16;
        if off + 16 > b.len() {
            break;
        }
        if b[off] == 14 {
            let ms = u32::from_le_bytes([b[off + 4], b[off + 5], b[off + 6], b[off + 7]]);
            let diag = u32::from_le_bytes([b[off + 8], b[off + 9], b[off + 10], b[off + 11]]);
            return Some((ms, diag));
        }
    }
    None
}

/// 事件读取后 BLOCK 的临时复用规则：每条 shim 命令前 `BLOCK = [0;4096]`
/// 清零（out_pieces 依赖零终止语义）。
fn blk_clear() {
    unsafe {
        core::ptr::write_bytes(core::ptr::addr_of_mut!(BLOCK), 0, 1);
    }
}

/// 从出参区（off 起）读零分隔片段序列到固定槽位。
fn out_pieces(off: usize, out: &mut [&[u8]; 24]) -> usize {
    let b = blk();
    let mut n = 0usize;
    let mut start = off;
    let mut i = off;
    while i < b.len() && n < out.len() {
        if b[i] == 0 {
            if i > start {
                out[n] = &b[start..i];
                n += 1;
            } else if i > off {
                break; // 空片段 = 末尾
            }
            start = i + 1;
            if i + 1 < b.len() && b[i + 1] == 0 {
                break;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

fn parse_u64(s: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => v = v * 10 + (b - b'0') as u64,
            _ => break,
        }
    }
    v
}

fn u64_bytes(mut v: u64, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if v == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    // 防御性钳制：调用方缓冲最小 8 字节，≥10^8 的值截高位不越界。
    let n = n.min(out.len());
    for i in 0..n {
        out[i] = tmp[n - 1 - i];
    }
    n
}

struct Files {
    names: [[u8; 40]; 16],
    sizes: [u64; 16],
    is_dir: [bool; 16],
    count: usize,
    sel: usize,
    source: bool, // true = exFAT SHARED 真实挂载
}

impl Files {
    fn refresh(&mut self) {
        blk_clear();
        put(0, b"/");
        let rc = shim(CMD_VFS_LIST);
        self.count = 0;
        if rc >= 0 {
            let mut slots: [&[u8]; 24] = [b""; 24];
            let n = out_pieces(512 + 4, &mut slots);
            // 三元组：name\0 size\0 dirflag\0
            let mut t = 0usize;
            while self.count < 16 && t + 2 < n {
                let name = slots[t];
                let size = slots[t + 1];
                let flag = slots[t + 2];
                let mut st = [0u8; 40];
                let m = name.len().min(39);
                st[..m].copy_from_slice(&name[..m]);
                self.names[self.count] = st;
                self.sizes[self.count] = parse_u64(size);
                self.is_dir[self.count] = flag == b"1";
                self.count += 1;
                t += 3;
            }
        }
        self.sel = 0;
        blk_clear();
        self.source = shim(CMD_VFS_SOURCE) == 1;
    }

    fn name(&self) -> &[u8] {
        let i = self.sel.min(self.count.saturating_sub(1));
        let slot = &self.names[i];
        &slot[..slot.iter().position(|&b| b == 0).unwrap_or(40)]
    }
}

const DEFAULTS: [&[u8]; 3] = [b"variable", b"windows", b"last"];

struct Settings {
    timeout: u64,
    show_menu: bool,
    default_idx: usize,
    loaded: bool,
}

impl Settings {
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if kv_get(b"settings", b"boot_timeout") {
            self.timeout = parse_u64(kv_val());
        }
        if kv_get(b"settings", b"show_menu") {
            self.show_menu = kv_val() == b"1";
        }
        if kv_get(b"settings", b"default_entry") {
            let v = kv_val();
            self.default_idx = if v == b"windows" {
                1
            } else if v == b"last" {
                2
            } else {
                0
            };
        }
    }
}

// KV 值缓冲（kv_get 出参拷贝）。
static mut KV_VAL: [u8; 256] = [0; 256];
static mut KV_LEN: usize = 0;

fn kv_val() -> &'static [u8] {
    unsafe {
        let n = KV_LEN.min(256);
        &*core::ptr::slice_from_raw_parts(core::ptr::addr_of!(KV_VAL) as *const u8, n)
    }
}

/// KV 读：命中返回 true（值在 kv_val()）。
fn kv_get(ns: &[u8], key: &[u8]) -> bool {
    blk_clear();
    put(0, ns);
    put(16, key);
    let rc = shim(CMD_KV_GET);
    if rc < 0 {
        return false;
    }
    let n = (out_u32(512) as usize).min(256);
    unsafe {
        let b = &*core::ptr::addr_of!(BLOCK);
        core::ptr::copy_nonoverlapping(
            b[516..516 + n].as_ptr(),
            core::ptr::addr_of_mut!(KV_VAL) as *mut u8,
            n,
        );
        KV_LEN = n;
    }
    rc > 0
}

fn kv_set(ns: &[u8], key: &[u8], val: &[u8]) -> i64 {
    blk_clear();
    put(0, ns);
    put(16, key);
    let len = val.len().min(256);
    {
        let b = blk();
        b[48..52].copy_from_slice(&(len as u32).to_le_bytes());
        b[52..52 + len].copy_from_slice(&val[..len]);
    }
    shim(CMD_KV_SET)
}

// ---------------------------------------------------------------------------
// 绘制基元扩展（AI-4 · 视觉契约件）
// ---------------------------------------------------------------------------

struct Ui {
    w: i64,
    h: i64,
}

impl Ui {
    /// 真壁纸整屏 blit（S4·AI-4/6：与 Windows WE 当前壁纸同源的静态帧，
    /// 内核最近邻缩放）。壁纸模块缺席 → 回退三段色带（内核诚实契约）。
    fn wallpaper(&self) {
        if frame(OP_WALLPAPER, 0, 0, 0, 0, 0) < 0 {
            let h1 = self.h * 55 / 100;
            let h2 = self.h * 85 / 100;
            fill_rect(0, 0, self.w, h1, C_WALL0);
            fill_rect(0, h1, self.w, h2 - h1, C_WALL1);
            fill_rect(0, h2, self.w, self.h - h2, C_BLUE);
        }
    }
}

/// 回退段色（壁纸缺席时与旧三段色带逐值一致）。
fn wall_fallback(y: i64, ui: &Ui) -> u64 {
    let h1 = ui.h * 55 / 100;
    let h2 = ui.h * 85 / 100;
    if y < h1 {
        0x0C_14_30
    } else if y < h2 {
        0x1A_2A_55
    } else {
        0x38_74_D2
    }
}

/// 壁纸像素采样：屏幕坐标 → 0xRRGGBB（削角回填依据——浮层永远浮在
/// 「壁纸+任务栏」底图上，角块回填 = 该处壁纸真色，逐像素无痕）。
fn wall_px(x: i64, y: i64, ui: &Ui) -> u64 {
    let r = frame(OP_WALLPAPER_PX, 0, 0, 0, pack_xywh(x, y), 0);
    if r >= 0 {
        return r as u64 & 0xFF_FFFF;
    }
    wall_fallback(y, ui)
}

/// 原色填充（照片色；palette 之外的真实色彩通道）。
fn fill_rgb(x: i64, y: i64, w: i64, h: i64, rgb: u64) {
    let _ = frame(
        OP_FILL_RGB,
        0,
        0,
        0,
        pack_xywh(x, y) | ((w as u64 & 0xFFFF) << 32) | ((h as u64 & 0xFFFF) << 48),
        rgb,
    );
}

/// 面板四角削角（两段阶梯≈8px 圆角；角块回填壁纸段色）。
/// 底部两角以任务栏上缘为界——越界部分不回填（任务栏会覆盖）。
fn cut_corners(x: i64, y: i64, w: i64, h: i64, ui: &Ui) {
    let tb_y = ui.h - 48;
    for (dy, cw) in [(0i64, 8i64), (2, 3), (4, 2)] {
        // 左上 / 右上
        fill_rgb(x, y + dy, cw, 2, wall_px(x, y + dy, ui));
        fill_rgb(x + w - cw, y + dy, cw, 2, wall_px(x + w - cw, y + dy, ui));
        // 左下 / 右下（不越过任务栏）
        let by = y + h - 2 - dy;
        if by < tb_y {
            fill_rgb(x, by, cw, 2, wall_px(x, by, ui));
            fill_rgb(x + w - cw, by, cw, 2, wall_px(x + w - cw, by, ui));
        }
    }
}

/// 浮层面板（削角+描边）。装饰性 1px 边框让位削角（描边画在削角内）。
fn float_panel(x: i64, y: i64, w: i64, h: i64, border: u64, ui: &Ui) {
    fill_rect(x, y, w, h, C_PANEL);
    cut_corners(x, y, w, h, ui);
    // 描边四直边（角部留白由削角负责）。
    fill_rect(x + 2, y, w - 4, 1, border);
    fill_rect(x + 2, y + h - 1, w - 4, 1, border);
    fill_rect(x, y + 2, 1, h - 4, border);
    fill_rect(x + w - 1, y + 2, 1, h - 4, border);
}

/// 任务栏 START 四格 logo（win11 徽标语义：2×2 窗格）。
fn draw_win_logo(x: i64, y: i64, hot: bool) {
    let c1 = if hot { C_HILITE } else { C_BLUE };
    let c2 = if hot { C_WHITE } else { C_CYAN };
    fill_rect(x, y, 9, 9, c1);
    fill_rect(x + 11, y, 9, 9, c2);
    fill_rect(x, y + 11, 9, 9, c2);
    fill_rect(x + 11, y + 11, 9, 9, c1);
}

/// 20×20 语义图标（菜单/桌面共用；色=语义：文件蓝/设置青/关于灰/
/// 重启黄/关机红——与 Variable iconRegistry 语义色一致的方向）。
fn draw_glyph(kind: usize, x: i64, y: i64) {
    match kind {
        0 => {
            // Files：文件夹（体+突舌+两条内容线）
            fill_rect(x, y + 5, 20, 12, C_BLUE);
            fill_rect(x, y + 2, 9, 4, C_BLUE);
            fill_rect(x + 3, y + 9, 14, 2, C_WHITE);
            fill_rect(x + 3, y + 13, 10, 2, C_WHITE);
        }
        1 => {
            // Settings：齿轮（四向齿+中心）
            fill_rect(x + 7, y, 6, 20, C_CYAN);
            fill_rect(x, y + 7, 20, 6, C_CYAN);
            fill_rect(x + 5, y + 5, 10, 10, C_CYAN);
            fill_rect(x + 8, y + 8, 4, 4, C_PANEL);
        }
        2 => {
            // About：信息 i
            fill_rect(x, y, 20, 20, C_LGRAY);
            fill_rect(x + 9, y + 3, 2, 3, C_DARK);
            fill_rect(x + 8, y + 8, 4, 8, C_DARK);
        }
        3 => {
            // Restart：循环箭头（四段环+右上三角）
            fill_rect(x + 4, y + 2, 12, 3, C_YELLOW);
            fill_rect(x + 2, y + 4, 3, 12, C_YELLOW);
            fill_rect(x + 15, y + 4, 3, 12, C_YELLOW);
            fill_rect(x + 4, y + 15, 12, 3, C_YELLOW);
            fill_rect(x + 14, y, 6, 5, C_WALL0);
            fill_rect(x + 13, y + 1, 6, 3, C_YELLOW);
        }
        _ => {
            // Shutdown：电源（竖条+环口）
            fill_rect(x + 9, y + 2, 2, 8, C_RED);
            fill_rect(x + 4, y + 5, 2, 7, C_RED);
            fill_rect(x + 14, y + 5, 2, 7, C_RED);
            fill_rect(x + 3, y + 11, 14, 3, C_RED);
            fill_rect(x + 5, y + 14, 10, 2, C_RED);
        }
    }
}

/// 鼠标光标（箭头：黑描底 + 白面；11 行逐行 fill，逐行加宽）。
fn draw_cursor(cx: i64, cy: i64) {
    // 黑描边（整体偏移 1px）
    for i in 0..7 {
        fill_rect(cx + 1, cy + 1 + i, i + 2, 1, C_DARK);
    }
    fill_rect(cx + 1, cy + 8, 4, 4, C_DARK);
    fill_rect(cx + 3, cy + 8, 3, 4, C_DARK);
    // 白面
    for i in 0..7 {
        fill_rect(cx, cy + i, i + 1, 1, C_WHITE);
    }
    fill_rect(cx, cy + 7, 2, 4, C_WHITE);
    fill_rect(cx + 1, cy + 8, 1, 3, C_WHITE);
}

// --- 既有绘制件（里程碑文案等） ---

/// 加载动画里程碑文案（stage 索引 0~2，随进度分段切换）。
const LOAD_STAGES: [&[u8]; 3] = [
    b"loading boot events",
    b"reading shared storage",
    b"starting desktop",
];

/// 加载动画屏（2026-09-19 用户验收：Variable 是主系统，内核加载完的
/// 瞬间自动进入，零选项）：VARIX 品牌大字 + 平滑进度条 + 里程碑状态行
/// （带 0~3 个动画点循环）。pct=当前平滑百分比（0~100，只升不降），
/// stage=里程碑索引，dots=动画点数。每帧全量重画（进度与点每帧都在变）。
fn draw_loading(ui: &Ui, pct: i64, stage: usize, dots: usize) {
    fill_rect(0, 0, ui.w, ui.h, C_WALL0);
    let logo_x = (ui.w - 5 * GLYPH_W * 3) / 2;
    text3(logo_x, ui.h / 2 - 64, b"VARIX", C_WHITE);
    text((ui.w - 15 * GLYPH_W) / 2, ui.h / 2 - 8, b"Variable System", C_LGRAY);
    let bar_w = ui.w / 3;
    let bar_x = (ui.w - bar_w) / 2;
    let bar_y = ui.h / 2 + 48;
    let frac = (bar_w * pct.clamp(0, 100)) / 100;
    outline(bar_x - 1, bar_y - 1, bar_w + 2, 12, C_LGRAY);
    fill_rect(bar_x, bar_y, frac, 10, C_BLUE);
    // 里程碑状态行 + 动画点。
    let mut line = [0u8; 32];
    let s = LOAD_STAGES[stage.min(2)];
    line[..s.len()].copy_from_slice(s);
    let mut p = s.len();
    for _ in 0..dots.min(3) {
        line[p] = b'.';
        p += 1;
    }
    text((ui.w - 23 * GLYPH_W) / 2, bar_y + 28, &line[..p], C_DIM);
}

const MENU_ITEMS: [&[u8]; 5] = [b"Files", b"Settings", b"About", b"Restart", b"Shutdown"];
/// 菜单项图标语义色（与 draw_glyph kind 同序）。
const MENU_KINDS: [usize; 5] = [0, 1, 2, 3, 4];

/// 几何常量（命中测试与绘制同源——S2.09 语义：几何即契约）。
const TB_H: i64 = 48;
const MENU_W: i64 = 300;
const MENU_ITEM_H: i64 = 48;
const ICON_W: i64 = 96;
const ICON_H: i64 = 72;

/// 任务栏（win11 化：四格 logo START + 品牌 + 右侧 uptime 时钟）。
/// hover_start=START 悬停；up_m/s=uptime 分秒（无钟=不画时钟行）。
fn draw_taskbar(ui: &Ui, menu_open: bool, hot_start: bool, up: Option<(u64, u64)>) {
    let tb_y = ui.h - TB_H;
    fill_rect(0, tb_y, ui.w, TB_H, C_TASKBAR);
    fill_rect(0, tb_y, ui.w, 2, if menu_open { C_HILITE } else { C_BLUE });
    // START 按钮：四格 logo + 文案（hover 提亮，开启态常亮描边）。
    let btn_c = if menu_open || hot_start { C_HILITE } else { C_BLUE };
    fill_rect(8, tb_y + 8, 108, 32, btn_c);
    draw_win_logo(16, tb_y + 14, hot_start && !menu_open);
    text(44, tb_y + 16, b"START", C_WHITE);
    text(132, tb_y + 16, b"VARIABLE SYSTEM", C_LGRAY);
    // 右侧：uptime 时钟 + 系统标识（内核无 RTC，活时钟=诚实运行时长）。
    if let Some((m, s)) = up {
        let mut line = [0u8; 12];
        let head = b"UP ";
        line[..head.len()].copy_from_slice(head);
        let mut p = head.len();
        let mut nb = [0u8; 8];
        let d = u64_bytes(m, &mut nb);
        line[p..p + d].copy_from_slice(&nb[..d]);
        p += d;
        line[p] = b':';
        p += 1;
        if s < 10 {
            line[p] = b'0';
            p += 1;
        }
        let d = u64_bytes(s, &mut nb);
        line[p..p + d].copy_from_slice(&nb[..d]);
        p += d;
        text(ui.w - 190, tb_y + 16, &line[..p], C_LGRAY);
    }
    text(ui.w - 104, tb_y + 16, b"VARIX", C_DIM);
}

/// 桌面底图=壁纸+任务栏。切页时统一重建底图（残留修复语义保持）。
fn draw_backdrop(ui: &Ui, menu_open: bool, hot_start: bool, up: Option<(u64, u64)>) {
    ui.wallpaper();
    draw_taskbar(ui, menu_open, hot_start, up);
}

/// 悬停语义（命中测试纯数据；绘制与点击共用同一几何常量）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hot {
    None,
    StartBtn,
    MenuItem(usize),
    DesktopIcon(usize),
    WinClose,
    FileRow(usize),
    SetRow(usize),
}

/// 命中测试（光标几何 → Hot；与绘制函数同一套常量）。
fn hit_test(ui: &Ui, phase: u8, menu_sel: usize, cx: i64, cy: i64, f: &Files, hot_close: bool) -> Hot {
    let tb_y = ui.h - TB_H;
    // 任务栏优先（浮层之下）。
    if cy >= tb_y {
        if cx >= 8 && cx < 116 {
            return Hot::StartBtn;
        }
        return Hot::None;
    }
    // 开始菜单浮层（浮于一切之上）。
    if phase == 1 {
        let (mx, my) = (8i64, ui.h - TB_H - 312);
        if cx >= mx && cx < mx + MENU_W && cy >= my && cy < my + 5 * MENU_ITEM_H + 40 {
            let row = ((cy - my - 32) / MENU_ITEM_H) as usize; // 顶部 32px=标题行
            if (cy - my) >= 32 && row < MENU_ITEMS.len() {
                return Hot::MenuItem(row);
            }
            return Hot::None;
        }
    }
    // 桌面图标（phase 0/1 桌面可见）。
    if phase == 0 || phase == 1 {
        for i in 0..3 {
            let x = 24;
            let y = 24 + (i as i64) * (ICON_H + 16);
            if cx >= x && cx < x + ICON_W && cy >= y && cy < y + ICON_H {
                return Hot::DesktopIcon(i);
            }
        }
    }
    // 窗口页命中（关闭钮在标题栏右上）。
    let (wx, wy, ww) = win_geo(ui, phase);
    if hot_close && cx >= wx + ww - 32 && cx < wx + ww - 12 && cy >= wy + 4 && cy < wy + 22 {
        return Hot::WinClose;
    }
    if phase == 2 && f.count > 0 {
        // 文件行命中
        let (x, y, w, h) = win_body(ui, 2);
        let _ = (w, h);
        if cx >= x + 4 && cx < x + w - 4 && cy >= y + 32 && cy < y + h - 28 {
            let rows = ((h - 64) / 28) as usize;
            let row = ((cy - y - 32) / 28) as usize;
            let start = if f.sel >= rows { f.sel + 1 - rows } else { 0 };
            let idx = start + row;
            if idx < f.count {
                return Hot::FileRow(idx);
            }
        }
    }
    if phase == 4 {
        let (x, y, w, h) = win_body(ui, 4);
        if cx >= x + 4 && cx < x + w - 4 && cy >= y + 40 && cy < y + h - 28 {
            let row = ((cy - y - 40) / 56) as usize;
            if row < 3 {
                return Hot::SetRow(row);
            }
        }
    }
    let _ = menu_sel;
    Hot::None
}

/// 窗口几何（phase 2/3/4/5 共用；绘制与命中同源）。
fn win_geo(ui: &Ui, phase: u8) -> (i64, i64, i64) {
    match phase {
        2 | 3 => (60, 60, ui.w.min(760) - 40),
        4 => (80, 80, ui.w.min(640) - 40),
        _ => (80, 80, ui.w.min(640) - 40),
    }
}

/// 窗口内容区几何（标题栏 28px 之下）。
fn win_body(ui: &Ui, phase: u8) -> (i64, i64, i64, i64) {
    let (x, y, w) = win_geo(ui, phase);
    let h = if phase == 2 || phase == 3 { ui.h - 60 - 120 } else if phase == 4 { 300 } else { 336 };
    (x, y, w, h)
}

/// 窗口面板（win11 化：accent 标题条 + [×] 关闭钮 + 右下阴影）。
/// 返回关闭钮几何（命中测试同源）。
fn draw_window(ui: &Ui, phase: u8, border: u64, title: &[u8], hot_close: bool) {
    let (x, y, w) = win_geo(ui, phase);
    let (_, by, _, bh) = win_body(ui, phase);
    // 阴影（右+下 3px，先画被窗口覆盖大半只露边缘）。
    fill_rect(x + 3, y + 3, w, bh + 25, C_DARK);
    // 主体。
    fill_rect(x, y, w, bh, C_PANEL);
    cut_corners(x, y, w, bh, ui);
    // 标题栏：accent 条 + 深色底 + 标题。
    fill_rect(x + 1, y + 1, w - 2, 26, C_DARK);
    fill_rect(x + 1, y + 1, 4, 26, C_BLUE);
    text(x + 12, y + 5, title, C_WHITE);
    // [×] 关闭钮（hot=红底；点击=Esc 等价）。
    fill_rect(x + w - 32, y + 4, 20, 20, if hot_close { C_RED } else { C_PANEL });
    text(x + w - 27, y + 4, b"X", if hot_close { C_WHITE } else { C_DIM });
    // 描边。
    outline(x, y, w, bh, border);
    let _ = by;
}

fn draw_desktop(
    ui: &Ui,
    menu_open: bool,
    menu_sel: usize,
    hot: Hot,
    up: Option<(u64, u64)>,
) {
    draw_backdrop(ui, menu_open, hot == Hot::StartBtn, up);
    // 桌面图标（图形区+标签+hover 淡高亮）。
    let icons: [&[u8]; 3] = [b"FILES", b"SETTINGS", b"ABOUT"];
    for (i, label) in icons.iter().enumerate() {
        let x = 24;
        let y = 24 + (i as i64) * (ICON_H + 16);
        let hovered = hot == Hot::DesktopIcon(i);
        if hovered {
            fill_rect(x - 4, y - 4, ICON_W + 8, ICON_H + 8, C_HILITE);
        }
        fill_rect(x, y, ICON_W, ICON_H, C_PANEL);
        cut_corners(x, y, ICON_W, ICON_H, ui);
        draw_glyph(i, x + (ICON_W - 20) / 2, y + 10);
        let lw = label.len() as i64 * GLYPH_W;
        text(x + (ICON_W - lw) / 2, y + 44, label, C_WHITE);
    }
    if menu_open {
        // 五项菜单：32px 标题行 + 5×48 行 + 削角面板；距任务栏 16px。
        let (mx, my) = (8i64, ui.h - TB_H - 312);
        let mh = 32 + 5 * MENU_ITEM_H + 24;
        float_panel(mx, my, MENU_W, mh, C_LGRAY, ui);
        text(mx + 12, my + 8, b"START", C_DIM);
        for (i, it) in MENU_ITEMS.iter().enumerate() {
            let ry = my + 32 + (i as i64) * MENU_ITEM_H;
            let focused = i == menu_sel;
            let hovered = hot == Hot::MenuItem(i);
            if focused {
                fill_rect(mx + 6, ry, MENU_W - 12, MENU_ITEM_H - 6, C_HILITE);
            } else if hovered {
                fill_rect(mx + 6, ry, MENU_W - 12, MENU_ITEM_H - 6, C_DARK);
            }
            draw_glyph(MENU_KINDS[i], mx + 14, ry + 12);
            text(mx + 44, ry + 14, it, C_WHITE);
        }
        // 底部用户区（win11 菜单尾行语义）。
        let uy = my + 32 + 5 * MENU_ITEM_H + 4;
        fill_rect(mx + 6, uy, MENU_W - 12, 1, C_DARK);
        text(mx + 14, uy + 6, b"user @ varix", C_DIM);
    }
}

fn draw_files(ui: &Ui, f: &Files, hot: Hot) {
    let (x, y, w, h) = win_body(ui, 2);
    draw_window(ui, 2, C_LGRAY, b"FILES - SHARED", hot == Hot::WinClose);
    let rows = ((h - 64) / 28) as usize;
    let count = f.count;
    if count > 0 {
        let start = if f.sel >= rows { f.sel + 1 - rows } else { 0 };
        let shown = rows.min(count - start);
        for r in 0..shown {
            let idx = start + r;
            let ry = y + 32 + (r as i64) * 28;
            if idx == f.sel {
                fill_rect(x + 4, ry, w - 8, 26, C_HILITE);
            } else if hot == Hot::FileRow(idx) {
                fill_rect(x + 4, ry, w - 8, 26, C_DARK);
            }
            let slot = &f.names[idx];
            let name = &slot[..slot.iter().position(|&b| b == 0).unwrap_or(40)];
            // 文件/目录语义点（目录=青点，文件=灰点）。
            fill_rect(x + 12, ry + 9, 8, 8, if f.is_dir[idx] { C_CYAN } else { C_LGRAY });
            text(x + 28, ry + 4, name, C_WHITE);
            let mut nb = [0u8; 12];
            let d = u64_bytes(f.sizes[idx], &mut nb);
            let tag: &[u8] = if f.is_dir[idx] { b"<DIR>" } else { &nb[..d] };
            text(x + w - 120, ry + 4, tag, C_DIM);
        }
    } else {
        text(x + 12, y + 40, b"(empty)", C_DIM);
    }
    fill_rect(x + 1, y + h - 28, w - 2, 26, C_DARK);
    let src: &[u8] = if f.source {
        b"SOURCE: SHARED exFAT (read-only)"
    } else {
        b"SOURCE: demo tree (no exFAT mount)"
    };
    text(x + 8, y + h - 22, src, C_YELLOW);
    text(
        x + 8 + (src.len() as i64 + 2) * GLYPH_W,
        y + h - 22,
        b"Esc=back Enter=open",
        C_DIM,
    );
}

fn draw_file_view(ui: &Ui, f: &Files, hot: Hot) {
    let (x, y, w, h) = win_body(ui, 3);
    draw_window(ui, 3, C_CYAN, b"FILE VIEW", hot == Hot::WinClose);
    let b = blk();
    let n = (out_u32(512) as usize).min(2048);
    let data = &b[516..516 + n];
    let cols = ((w - 24) / GLYPH_W) as usize;
    let max_rows = ((h - 72) / GLYPH_H) as usize;
    let mut row = 0usize;
    let mut i = 0usize;
    while i < data.len() && row < max_rows {
        let mut end = data[i..]
            .iter()
            .position(|&c| c == b'\n')
            .map(|p| i + p)
            .unwrap_or(data.len());
        end = end.min(i + cols);
        text(x + 12, y + 32 + (row as i64) * GLYPH_H, &data[i..end], C_WHITE);
        row += 1;
        i = if end < data.len() && data[end] == b'\n' { end + 1 } else { end };
    }
    fill_rect(x + 1, y + h - 28, w - 2, 26, C_DARK);
    text(x + 8, y + h - 22, b"read-only preview - Esc=back", C_YELLOW);
    let _ = f;
}

fn draw_settings(ui: &Ui, s: &Settings, sel: usize, hot: Hot) {
    let (x, y, w, h) = win_body(ui, 4);
    draw_window(ui, 4, C_LGRAY, b"SETTINGS - BOOT BEHAVIOR", hot == Hot::WinClose);
    let labels: [&[u8]; 3] = [b"BOOT TIMEOUT", b"SHOW MENU", b"DEFAULT ENTRY"];
    for (i, label) in labels.iter().enumerate() {
        let ry = y + 40 + (i as i64) * 56;
        if i == sel {
            fill_rect(x + 4, ry, w - 8, 52, C_HILITE);
        } else if hot == Hot::SetRow(i) {
            fill_rect(x + 4, ry, w - 8, 52, C_DARK);
        }
        text(x + 16, ry + 6, label, C_WHITE);
        let mut vb = [0u8; 24];
        let val: &[u8] = match i {
            0 => {
                let d = u64_bytes(s.timeout, &mut vb);
                &vb[..d]
            }
            1 => {
                if s.show_menu {
                    b"ON"
                } else {
                    b"OFF"
                }
            }
            _ => DEFAULTS[s.default_idx],
        };
        text(x + w - 160, ry + 6, val, C_CYAN);
        if i == sel {
            let grip: &[u8] = if i == 1 { b"[ ]" } else { b"< >" };
            text(x + w - 200, ry + 6, grip, C_YELLOW);
        }
    }
    fill_rect(x + 1, y + h - 28, w - 2, 26, C_DARK);
    text(
        x + 8,
        y + h - 22,
        b"STORE: KV (task24) - settings persist",
        C_YELLOW,
    );
}

fn draw_about(ui: &Ui, info: &[u8], hot: Hot) {
    let (x, y, _, _) = win_body(ui, 5);
    draw_window(ui, 5, C_CYAN, b"ABOUT - VARIABLE SYSTEM", hot == Hot::WinClose);
    let lines: [&[u8]; 8] = [
        b"Variable System - VARIX kernel",
        b"UI host: ushell.elf (ring3 process)",
        b"render: SYS_FRAME -> display service",
        b"input: SYS_INPUT (keys + mouse)",
        b"shim: SYS_SHIM (KV/VFS/boot events)",
        b"restart: SYS_REBOOT -> Windows",
        b"shutdown: SYS_POWEROFF -> ACPI S5",
        info,
    ];
    for (i, l) in lines.iter().enumerate() {
        text(x + 16, y + 36 + (i as i64) * 28, l, if i == 7 { C_YELLOW } else { C_LGRAY });
    }
    let _ = hot;
}

// ---------------------------------------------------------------------------
// 主流程
// ---------------------------------------------------------------------------

fn report_kv(key: &[u8], rc: i64) {
    let mut line = [0u8; 64];
    let head = b"SHELL: settings set key=";
    let mut p = 0usize;
    line[p..p + head.len()].copy_from_slice(head);
    p += head.len();
    let n = key.len().min(line.len() - p - 12);
    line[p..p + n].copy_from_slice(&key[..n]);
    p += n;
    let tail = b" rc=";
    line[p..p + 4].copy_from_slice(tail);
    p += 4;
    let rcb: &[u8] = if rc == 0 { b"0" } else { b"ERR" };
    line[p..p + rcb.len()].copy_from_slice(rcb);
    p += rcb.len();
    puts(&line[..p]);
    puts(b"\n");
}

fn report_count(f: &Files) {
    let mut line = [0u8; 56];
    let head = b"SHELL: fm count=";
    let mut p = 0usize;
    line[p..p + head.len()].copy_from_slice(head);
    p += head.len();
    let mut nb = [0u8; 8];
    let d = u64_bytes(f.count as u64, &mut nb);
    line[p..p + d].copy_from_slice(&nb[..d]);
    p += d;
    let tail: &[u8] = if f.source {
        b" source=shared-exfat"
    } else {
        b" source=demo-tree"
    };
    line[p..p + tail.len()].copy_from_slice(tail);
    p += tail.len();
    puts(&line[..p]);
    puts(b"\n");
}

/// 鼠标点击动作结果（与键盘动作同表分派——双通道能力对等）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Click {
    None,
    ToggleMenu,
    OpenFiles,
    OpenSettings,
    OpenAbout,
    Reboot,
    PowerOff,
    CloseWin,
    OpenSelFile,
    PickFile(usize),
    PickSetting(usize),
    SetToggle,
    Adjust(i64),
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let (w, h) = disp_info();
    if w <= 0 || h <= 0 {
        marker(b"SHELL: display info unavailable - exit");
        exit(1);
    }
    // 验收轮诊断：报告显示几何协商结果（disp_info vs 内核 Surface）。
    {
        let mut line = [0u8; 48];
        let head = b"SHELL: disp=";
        let mut p = 0usize;
        line[p..p + head.len()].copy_from_slice(head);
        p += head.len();
        let mut nb = [0u8; 8];
        let d = u64_bytes(w as u64, &mut nb);
        line[p..p + d].copy_from_slice(&nb[..d]);
        p += d;
        line[p..p + 1].copy_from_slice(b"x");
        p += 1;
        let d = u64_bytes(h as u64, &mut nb);
        line[p..p + d].copy_from_slice(&nb[..d]);
        p += d;
        line[p] = b'\n';
        p += 1;
        puts(&line[..p]);
    }
    let ui = Ui { w, h };

    let mut files = Files {
        names: [[0; 40]; 16],
        sizes: [0; 16],
        is_dir: [false; 16],
        count: 0,
        sel: 0,
        source: false,
    };
    let mut evs = [InEv { key: 0, is_mouse: false, dx: 0, dy: 0, btn: 0 }; IN_MAX];
    marker(b"SHELL: entering variable-system");
    blk_clear();
    let _ = shim(CMD_BOOT_EVENTS);
    let t0 = decode_kbd_rec().map(|(ms, _)| ms);
    let mut pct: i64 = 0;
    let mut target: i64 = 5;
    let mut stage: usize = 0;
    let mut did_files = false;
    let mut frame: u64 = 0;
    loop {
        // 吞键吞鼠：加载期任何输入都不跳过动画（零选项直达桌面）。
        let _ = input_events(&mut evs);
        // 活时钟：每帧重取（诊断记录由内核实时注入）。
        blk_clear();
        let _ = shim(CMD_BOOT_EVENTS);
        let now_ms = decode_kbd_rec().map(|(ms, _)| ms);
        let elapsed = match (now_ms, t0) {
            (Some(now), Some(start)) => now.wrapping_sub(start),
            _ => 0,
        };
        // 里程碑推进：帧 0 起 boot 事件随每帧时钟刷新在线 →35%；帧 2 枚
        // 举共享存储（files.refresh，桌面文件页数据就此就绪）→80%；
        // ≥2s（无钟=24 帧兜底）→100% 落桌面。
        if frame == 0 {
            target = target.max(35);
        }
        if frame == 2 && !did_files {
            files.refresh();
            did_files = true;
            stage = 1;
            target = target.max(80);
        }
        let ready = if t0.is_some() { elapsed >= 2_000 } else { frame >= 24 };
        if ready {
            stage = 2;
            target = 100;
        }
        // 平滑推进：指数逼近 target，每帧至少 +1（低帧率下 ~30 帧收尾）。
        let gap = target - pct;
        if gap > 0 {
            pct += ((gap + 7) / 8).max(1).min(gap);
        }
        let dots = ((frame / 6) % 4) as usize;
        draw_loading(&ui, pct, stage, dots);
        if frame == 0 {
            // 任务71 首帧口径：Loading 第一帧落屏即打点，毫秒取自内核
            // 时钟（CMD_BOOT_MS）——口径如实践录。
            blk_clear();
            let frc = shim(CMD_BOOT_MS);
            let fms = if frc >= 0 {
                let b = blk();
                u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
            } else {
                0
            };
            let mut fline = [0u8; 48];
            let fhead = b"SHELL: first-frame ms=";
            fline[..fhead.len()].copy_from_slice(fhead);
            let mut fnb = [0u8; 8];
            let fd = u64_bytes(fms, &mut fnb);
            fline[fhead.len()..fhead.len() + fd].copy_from_slice(&fnb[..fd]);
            let fp = fhead.len() + fd;
            marker(&fline[..fp]);
        }
        frame += 1;
        if pct >= 100 && target >= 100 {
            break;
        }
        // 无钟兜底节奏：每帧短自旋，24 帧≈2s 量级；有钟路径完全由活时钟
        // 驱动，不自旋（TCG 单帧绘制本身就慢）。
        if t0.is_none() {
            for _ in 0..200_000 {
                core::hint::spin_loop();
            }
        }
    }
    marker(b"SHELL: boot-replay done");

    // ② 桌面（files 已在 Loading 里程碑中枚举就绪，直接上报）
    let mut phase = 0u8; // 0=desktop 1=startmenu 2=files 3=fileview 4=settings 5=about
    let mut menu_sel = 0usize;
    marker(b"SHELL: desktop-ready");
    report_count(&files);

    let mut settings = Settings { timeout: 5, show_menu: true, default_idx: 0, loaded: false };
    let mut setting_sel = 0usize;
    let mut about_info = [0u8; 64];

    // 鼠标状态（S2.05 R2 消费端）：光标初始屏幕中心；按钮位边沿检测。
    let mut cur = (w / 2, h / 2);
    let mut prev_btn = 0u8;
    let mut prev_hot = Hot::None;
    let mut prev_up = (u64::MAX, u64::MAX);

    // 验收轮修复 · 切页重建底图：prev_phase 初始取不可能值强制首帧重建。
    let mut prev_phase = 99u8;
    let mut need_redraw = true;
    loop {
        let up = uptime_tuple();
        // 悬停/时钟变化检测（先于重绘判定）。
        let hot = hit_test(&ui, phase, menu_sel, cur.0, cur.1, &files, true);
        if phase != prev_phase || hot != prev_hot || up != prev_up {
            draw_backdrop(&ui, phase == 1, hot == Hot::StartBtn && phase == 0, Some(up));
            prev_phase = phase;
            prev_hot = hot;
            prev_up = up;
            need_redraw = true;
        }
        if need_redraw {
            match phase {
                0 | 1 => draw_desktop(&ui, phase == 1, menu_sel, hot, Some(up)),
                2 => draw_files(&ui, &files, hot),
                3 => draw_file_view(&ui, &files, hot),
                4 => draw_settings(&ui, &settings, setting_sel, hot),
                _ => draw_about(&ui, &about_info[..info_len(&about_info)], hot),
            }
            draw_cursor(cur.0, cur.1);
            need_redraw = false;
        }

        let n = input_events(&mut evs);
        if n == 0 {
            continue;
        }
        let mut handled = false;
        let mut click = Click::None;
        'input: for i in 0..n {
            let e = evs[i];
            if e.is_mouse {
                // 鼠标：位移累加+视口钳制；按钮位边沿→点击。
                if e.dx != 0 || e.dy != 0 {
                    cur.0 = (cur.0 + e.dx as i64).clamp(0, w - 1);
                    cur.1 = (cur.1 - e.dy as i64).clamp(0, h - 1); // PS/2 语义 dy 正=向上（与 bootselect 同契约）
                    handled = true;
                }
                let pressed = e.btn & !prev_btn;
                prev_btn = e.btn;
                if pressed & 1 != 0 {
                    // 左键按下：按当前 hover 分派（悬停与命中同一几何）。
                    let h2 = hit_test(&ui, phase, menu_sel, cur.0, cur.1, &files, true);
                    click = match h2 {
                        Hot::StartBtn => Click::ToggleMenu,
                        Hot::MenuItem(idx) => match idx {
                            0 => Click::OpenFiles,
                            1 => Click::OpenSettings,
                            2 => Click::OpenAbout,
                            3 => Click::Reboot,
                            _ => Click::PowerOff,
                        },
                        Hot::DesktopIcon(idx) => match idx {
                            0 => Click::OpenFiles,
                            1 => Click::OpenSettings,
                            _ => Click::OpenAbout,
                        },
                        Hot::WinClose => Click::CloseWin,
                        Hot::FileRow(idx) => {
                            if idx == files.sel {
                                Click::OpenSelFile
                            } else {
                                Click::PickFile(idx)
                            }
                        }
                        Hot::SetRow(row) => {
                            if row == setting_sel {
                                if row == 1 {
                                    Click::SetToggle
                                } else {
                                    // 已选中行再点：行中点右侧=+1 左侧=-1
                                    let (_, _, ww) = win_geo(&ui, 4);
                                    let (x, _, _, _) = win_body(&ui, 4);
                                    if cur.0 > x + ww - 220 {
                                        Click::Adjust(1)
                                    } else {
                                        Click::Adjust(-1)
                                    }
                                }
                            } else {
                                Click::PickSetting(row)
                            }
                        }
                        Hot::None => Click::None,
                    };
                    break 'input;
                }
                continue;
            }
            let key = e.key;
            if key == 0xFF {
                continue;
            }
            handled = true;
            match phase {
                0 => {
                    if key == K_ENTER || key == 4 {
                        phase = 1;
                        menu_sel = 0;
                        marker(b"SHELL: startmenu opened");
                        break 'input;
                    }
                }
                1 => match key {
                    K_UP => menu_sel = (menu_sel + MENU_ITEMS.len() - 1) % MENU_ITEMS.len(),
                    K_DOWN => menu_sel = (menu_sel + 1) % MENU_ITEMS.len(),
                    K_ESC => phase = 0,
                    K_ENTER => {
                        click = match menu_sel {
                            0 => Click::OpenFiles,
                            1 => Click::OpenSettings,
                            2 => Click::OpenAbout,
                            3 => Click::Reboot,
                            _ => Click::PowerOff,
                        };
                        break 'input;
                    }
                    _ => {}
                },
                2 => match key {
                    K_UP => {
                        if files.sel > 0 {
                            files.sel -= 1;
                        }
                    }
                    K_DOWN => {
                        if files.sel + 1 < files.count {
                            files.sel += 1;
                        }
                    }
                    K_ESC => phase = 0,
                    K_ENTER => click = Click::OpenSelFile,
                    _ => {}
                },
                3 => {
                    if key == K_ESC || key == K_ENTER {
                        phase = 2;
                        break 'input;
                    }
                }
                4 => match key {
                    K_UP => setting_sel = (setting_sel + 2) % 3,
                    K_DOWN => setting_sel = (setting_sel + 1) % 3,
                    K_ESC => phase = 0,
                    K_LEFT => click = Click::Adjust(-1),
                    K_RIGHT => click = Click::Adjust(1),
                    K_ENTER => {
                        if setting_sel == 1 {
                            click = Click::SetToggle;
                        }
                    }
                    _ => {}
                },
                _ => {
                    if key == K_ESC || key == K_ENTER {
                        phase = 0;
                        break 'input;
                    }
                }
            }
        }
        // 点击/键盘动作统一执行（双通道同表）。
        match click {
            Click::None => {}
            Click::ToggleMenu => {
                phase = if phase == 1 { 0 } else { 1 };
                menu_sel = 0;
                if phase == 1 {
                    marker(b"SHELL: startmenu opened");
                }
            }
            Click::OpenFiles => {
                files.refresh();
                report_count(&files);
                setting_sel = 0;
                phase = 2;
                marker(b"SHELL: files opened");
            }
            Click::OpenSettings => {
                settings.load();
                setting_sel = 0;
                phase = 4;
                marker(b"SHELL: settings opened");
            }
            Click::OpenAbout => {
                let (nk, _) = kv_count(b"settings");
                let head = b"kv-keys=";
                let mut nb = [0u8; 8];
                let d = u64_bytes(nk, &mut nb);
                let total = head.len() + d;
                about_info[..head.len()].copy_from_slice(head);
                about_info[head.len()..total].copy_from_slice(&nb[..d]);
                phase = 5;
                marker(b"SHELL: about opened");
            }
            Click::Reboot => {
                marker(b"SHELL: restart requested - rebooting");
                let _ = syscall3(SYS_REBOOT, 0, 0, 0);
                marker(b"SHELL: restart failed - still running");
            }
            Click::PowerOff => {
                marker(b"SHELL: shutdown requested - powering off");
                let _ = syscall3(SYS_POWEROFF, 0, 0, 0);
                marker(b"SHELL: shutdown failed - still running");
            }
            Click::CloseWin => phase = 0,
            Click::OpenSelFile => {
                if phase == 2 && files.count > 0 && !files.is_dir[files.sel] {
                    blk_clear();
                    put(0, files.name());
                    let rc = shim(CMD_VFS_READ);
                    if rc >= 0 {
                        let mut line = [0u8; 72];
                        let head = b"SHELL: file opened name=";
                        let mut p = 0usize;
                        line[p..p + head.len()].copy_from_slice(head);
                        p += head.len();
                        let nm = files.name();
                        let m = nm.len().min(line.len() - p - 12);
                        line[p..p + m].copy_from_slice(&nm[..m]);
                        p += m;
                        let mid = b" bytes=";
                        line[p..p + 7].copy_from_slice(mid);
                        p += 7;
                        let mut nb = [0u8; 8];
                        let d = u64_bytes(rc as u64, &mut nb);
                        line[p..p + d].copy_from_slice(&nb[..d]);
                        p += d;
                        puts(&line[..p]);
                        puts(b"\n");
                        phase = 3;
                    }
                }
            }
            Click::PickFile(idx) => files.sel = idx,
            Click::PickSetting(row) => setting_sel = row,
            Click::SetToggle => {
                settings.show_menu = !settings.show_menu;
                let rc = kv_set(
                    b"settings",
                    b"show_menu",
                    if settings.show_menu { b"1" } else { b"0" },
                );
                report_kv(b"show_menu", rc);
            }
            Click::Adjust(dir) => {
                let sel = if phase == 4 { setting_sel } else { 0 };
                adjust_setting(&mut settings, sel, dir);
            }
        }
        let _ = click;
        if handled {
            need_redraw = true;
        }
    }
}

fn info_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

/// uptime（活时钟 ms → (分, 秒)；无钟=None 不画时钟行——诚实缺数据）。
fn uptime_tuple() -> (u64, u64) {
    blk_clear();
    let rc = shim(CMD_BOOT_MS);
    if rc < 0 {
        return (u64::MAX, u64::MAX);
    }
    let ms = {
        let b = blk();
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    };
    let s = ms / 1000;
    ((s / 60) % 1000, s % 60)
}

/// kv_keys 计数（About 页指标）。
fn kv_count(ns: &[u8]) -> (u64, bool) {
    blk_clear();
    put(0, ns);
    let rc = shim(4);
    if rc < 0 {
        return (0, false);
    }
    (out_u32(512) as u64, true)
}

fn adjust_setting(s: &mut Settings, sel: usize, dir: i64) {
    match sel {
        0 => {
            let nv = (s.timeout as i64 + dir).clamp(0, 60) as u64;
            if nv != s.timeout {
                s.timeout = nv;
                let mut nb = [0u8; 8];
                let d = u64_bytes(nv, &mut nb);
                let rc = kv_set(b"settings", b"boot_timeout", &nb[..d]);
                report_kv(b"boot_timeout", rc);
            }
        }
        2 => {
            let ni = (s.default_idx as i64 + dir).rem_euclid(3) as usize;
            if ni != s.default_idx {
                s.default_idx = ni;
                let rc = kv_set(b"settings", b"default_entry", DEFAULTS[ni]);
                report_kv(b"default_entry", rc);
            }
        }
        _ => {}
    }
}

#[panic_handler]
fn ph(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
