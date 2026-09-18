//! 任务27/28（AI-V）· 用户态 shell 壳程序（ushell）。
//!
//! 内核嵌入层演示常驻进程：ring3 下经三个内核支点驱动--
//! - SYS_FRAME(16)：绘制命令（fill_rect/text/hline/vline/outline/info）；
//! - SYS_INPUT(17)：shim://input 16B 键鼠事件；
//! - SYS_SHIM(18)：KV（设置存储）/VFS（SHARED 文件列取读）/boot://event
//!   回放/时钟。
//!
//! 旅程（对应总案阶段3 步骤7/8 走查清单）：
//!   ① BootScreen：boot 事件回放 14 阶段（kernel timeline 快照）→ 任意键
//!   ② 桌面：壁纸渐变 + 任务栏（START）+ 桌面图标
//!   ③ START → 开始菜单 → 文件管理器（SHARED 列表/读文件预览，数据源
//!      如实标注 exFAT / demo-tree）
//!   ④ START → 设置页（引导行为三参数，KV 存储即时读写，←→/Enter）
//!   ⑤ START → 关于（嵌入层自述 + 运行指标）
//!
//! 全程键盘可达（↑↓←→/Enter/Esc）；里程碑经串口打点供验收脚本断言。

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

#[allow(dead_code)] // 协议位保留：OP_FILL 由 SYS_FRAME 出参外的内核 fill 语义占位
const OP_FILL: u64 = 1;
const OP_TEXT: u64 = 2;
const OP_OUTLINE: u64 = 5;
const OP_INFO: u64 = 6;

// 调色板（与内核 PALETTE 表同序同源）。
const C_WHITE: u64 = 1;
const C_DARK: u64 = 2;
const C_LGRAY: u64 = 3;
const C_BLUE: u64 = 4;
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

/// 泵取按键（kind!=Key 的事件映射 0xFF=忽略；返回事件数）。
fn input(out: &mut [u8; IN_MAX]) -> usize {
    let n = syscall3(SYS_INPUT, core::ptr::addr_of!(IN_BUF) as u64, IN_MAX as u64, 0);
    let n = if n < 0 { 0 } else { (n as usize).min(IN_MAX) };
    unsafe {
        let buf = &*core::ptr::addr_of!(IN_BUF);
        for i in 0..n {
            let base = i * 16;
            let kind = buf[base + 8];
            let key = buf[base + 9];
            out[i] = if kind == KIND_KEY { key } else { 0xFF };
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
// 绘制
// ---------------------------------------------------------------------------

struct Ui {
    w: i64,
    h: i64,
}

impl Ui {
    // 验收轮修复：原实现按 2px 条带循环 400 次 fill_rect（TCG 下 >50ms，
    // 桌面帧长期处于「壁纸画完、图标未画」的中间态——实机截图与人眼均见
    // 闪烁缺件）。三色带本就是纯色分段，改为 3 次整段填充，视觉逐像素等价。
    fn wallpaper(&self) {
        let h1 = self.h * 55 / 100;
        let h2 = self.h * 85 / 100;
        fill_rect(0, 0, self.w, h1, C_WALL0);
        fill_rect(0, h1, self.w, h2 - h1, C_WALL1);
        fill_rect(0, h2, self.w, self.h - h2, C_BLUE);
    }

    fn panel(&self, x: i64, y: i64, w: i64, h: i64, border: u64, title: &[u8]) {
        fill_rect(x, y, w, h, C_PANEL);
        outline(x, y, w, h, border);
        fill_rect(x + 1, y + 1, w - 2, 24, C_DARK);
        text(x + 8, y + 4, title, C_WHITE);
    }
}

const BOOT_STAGES: [&[u8]; 14] = [
    b"serial", b"cmdline", b"framebuffer", b"logo", b"banner", b"console", b"platform", b"acpi",
    b"smbios", b"memmap", b"kaslr", b"integrity", b"bootopt", b"selftest",
];

fn draw_bootscreen(ui: &Ui, stages: usize) -> ([u8; 40], usize) {
    fill_rect(0, 0, ui.w, ui.h, C_WALL0);
    let logo_x = (ui.w - 5 * GLYPH_W * 3) / 2;
    text3(logo_x, ui.h / 4, b"VARIX", C_WHITE);
    text((ui.w - 24 * GLYPH_W) / 2, ui.h / 4 + 64, b"embedding layer demo", C_DIM);
    let bar_w = ui.w * 2 / 3;
    let bar_x = (ui.w - bar_w) / 2;
    let bar_y = ui.h / 2;
    outline(bar_x - 1, bar_y - 1, bar_w + 2, 18, C_LGRAY);
    for st in 0..stages.min(14) {
        let frac = ((st as i64 + 1) * bar_w) / 14;
        fill_rect(bar_x, bar_y, frac, 16, C_BLUE);
        fill_rect(bar_x, bar_y + 28, 16 * GLYPH_W, GLYPH_H, C_WALL0);
        text(bar_x, bar_y + 28, BOOT_STAGES[st], C_LGRAY);
    }
    // boot 毫秒行（CMD_BOOT_MS 出参）。
    blk_clear();
    let rc = shim(CMD_BOOT_MS);
    let ms = if rc >= 0 {
        let b = blk();
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    } else {
        0
    };
    let mut line = [0u8; 40];
    let head = b"boot completed ";
    line[..head.len()].copy_from_slice(head);
    let mut nb = [0u8; 8];
    let d = u64_bytes(ms, &mut nb);
    line[head.len()..head.len() + d].copy_from_slice(&nb[..d]);
    let tail = b" ms";
    let p = head.len() + d;
    line[p..p + 3].copy_from_slice(tail);
    let total = p + 3;
    text((ui.w - (total as i64) * GLYPH_W) / 2, bar_y + 56, &line[..total], C_WHITE);
    text(
        (ui.w - 25 * GLYPH_W) / 2,
        bar_y + 88,
        b"press any key to continue",
        C_YELLOW,
    );
    (line, total)
}

const MENU_ITEMS: [&[u8]; 3] = [b"Files", b"Settings", b"About"];

/// 桌面底图=壁纸+任务栏。验收轮修复：窗口页（files/settings/about）此前
/// 只画自身窗口、不重绘背景——从菜单态切页时菜单浮层/任务栏像素残留
/// （实机截图 06/07/08 叠着菜单三项文字）。切页时统一重建底图。
fn draw_backdrop(ui: &Ui) {
    ui.wallpaper();
    let tb_y = ui.h - 48;
    fill_rect(0, tb_y, ui.w, 48, C_TASKBAR);
    fill_rect(0, tb_y, ui.w, 2, C_BLUE);
    fill_rect(8, tb_y + 8, 88, 32, C_BLUE);
    text(24, tb_y + 16, b"START", C_WHITE);
    text(120, tb_y + 16, b"VARIX DESKTOP", C_LGRAY);
    text(ui.w - 104, tb_y + 16, b"ring3 shell", C_DIM);
}

fn draw_desktop(ui: &Ui, menu_open: bool, menu_sel: usize) {
    draw_backdrop(ui);
    let icons: [&[u8]; 3] = [b"FILES", b"SETTINGS", b"ABOUT"];
    for (i, label) in icons.iter().enumerate() {
        let x = 24;
        let y = 24 + (i as i64) * 72;
        fill_rect(x, y, 96, 56, C_PANEL);
        outline(x, y, 96, 56, C_LGRAY);
        text(x + 8, y + 20, label, C_WHITE);
    }
    if menu_open {
        let (mx, my, mw, mh) = (8, ui.h - 48 - 164, 280, 160);
        fill_rect(mx, my, mw, mh, C_PANEL);
        outline(mx, my, mw, mh, C_LGRAY);
        for (i, it) in MENU_ITEMS.iter().enumerate() {
            let ry = my + 8 + (i as i64) * 48;
            if i == menu_sel {
                fill_rect(mx + 4, ry, mw - 8, 44, C_HILITE);
            }
            text(mx + 16, ry + 12, it, C_WHITE);
        }
    }
}

fn draw_files(ui: &Ui, f: &Files) {
    let w = ui.w.min(760) - 40;
    let x = 60;
    let y = 60;
    let h = ui.h - 60 - 120;
    ui.panel(x, y, w, h, C_LGRAY, b"FILES - SHARED");
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
            }
            let slot = &f.names[idx];
            let name = &slot[..slot.iter().position(|&b| b == 0).unwrap_or(40)];
            text(x + 12, ry + 4, name, C_WHITE);
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

fn draw_file_view(ui: &Ui, f: &Files) {
    let w = ui.w.min(760) - 40;
    let x = 60;
    let y = 60;
    let h = ui.h - 60 - 120;
    ui.panel(x, y, w, h, C_CYAN, b"FILE VIEW");
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

fn draw_settings(ui: &Ui, s: &Settings, sel: usize) {
    let w = ui.w.min(640) - 40;
    let x = 80;
    let y = 80;
    let h = 300;
    ui.panel(x, y, w, h, C_LGRAY, b"SETTINGS - BOOT BEHAVIOR");
    let labels: [&[u8]; 3] = [b"BOOT TIMEOUT", b"SHOW MENU", b"DEFAULT ENTRY"];
    for (i, label) in labels.iter().enumerate() {
        let ry = y + 40 + (i as i64) * 56;
        if i == sel {
            fill_rect(x + 4, ry, w - 8, 52, C_HILITE);
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
        b"STORE: KV-RAM (session) - disk-backed KV: task24",
        C_YELLOW,
    );
}

fn draw_about(ui: &Ui, info: &[u8]) {
    let w = ui.w.min(640) - 40;
    let x = 80;
    let y = 80;
    let h = 280;
    ui.panel(x, y, w, h, C_CYAN, b"ABOUT - EMBEDDING LAYER");
    let lines: [&[u8]; 6] = [
        b"VARIX kernel embedding layer demo",
        b"UI host: ushell.elf (ring3 process)",
        b"render: SYS_FRAME -> display service",
        b"input: SYS_INPUT (shim://input 16B)",
        b"shim: SYS_SHIM (KV/VFS/boot events)",
        info,
    ];
    for (i, l) in lines.iter().enumerate() {
        text(x + 16, y + 36 + (i as i64) * 28, l, if i == 5 { C_YELLOW } else { C_LGRAY });
    }
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

    // ① BootScreen：boot://event 回放（任务28：内核 timeline 快照 → 记录
    //    idx 单调推进；渲染端容错=未知记录跳过）。
    blk_clear();
    let n_ev = shim(CMD_BOOT_EVENTS);
    let n_stages = if n_ev < 0 { 0 } else { n_ev as usize };
    let (_l, _t) = draw_bootscreen(&ui, n_stages);
    // 任务71 首帧口径：BootScreen 第一帧落屏即打点，毫秒取自内核时钟
    //（CMD_BOOT_MS，与屏上 "boot completed N ms" 同源同值）——TCG 演示链
    // 的前置探针耗时不计入首帧，口径如实践录。
    {
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
    marker(b"SHELL: boot-replay done - press any key");

    let mut ev = [0u8; IN_MAX];
    loop {
        let n = input(&mut ev);
        let mut pressed = false;
        for i in 0..n {
            if ev[i] != 0xFF {
                pressed = true;
            }
        }
        if pressed {
            break;
        }
    }

    // ② 桌面
    let mut phase = 0u8; // 0=desktop 1=startmenu 2=files 3=fileview 4=settings 5=about
    let mut menu_sel = 0usize;
    let mut files = Files {
        names: [[0; 40]; 16],
        sizes: [0; 16],
        is_dir: [false; 16],
        count: 0,
        sel: 0,
        source: false,
    };
    files.refresh();
    marker(b"SHELL: desktop-ready");
    report_count(&files);

    let mut settings = Settings { timeout: 5, show_menu: true, default_idx: 0, loaded: false };
    let mut setting_sel = 0usize;
    let mut about_info = [0u8; 64];

    // 验收轮修复 · 切页重建底图：prev_phase 初始取不可能值强制首帧重建。
    // 窗口页只画自身窗口，背景（壁纸+任务栏）由切页瞬间一次性画好；
    // 同页内循环重画窗口自清（浮层如菜单随 phase 切换消失无残留）。
    //
    // 验收轮修复 · 按需重绘：原实现每轮循环无条件重画当前页——无按键时
    // 也以全速空转重绘（TCG 下循环内壁纸三段 fill 约 55 万像素/轮是绝对
    // 大头），实机截图/人眼会落在「壁纸已覆盖上帧图标、图标未重画」的
    // 中间态（02 桌面缺图标、03 缺菜单、06 缺设置文字皆此机理：走查脚
    // 本固定延时命中循环不同相位）。改为仅状态变化时重画：稳态画面静
    // 止为完整帧（任何时刻截图完整），空闲 CPU 占用同步归零。
    let mut prev_phase = 99u8;
    let mut need_redraw = true;
    loop {
        if phase != prev_phase {
            draw_backdrop(&ui);
            prev_phase = phase;
            need_redraw = true;
        }
        if need_redraw {
            match phase {
                0 | 1 => draw_desktop(&ui, phase == 1, menu_sel),
                2 => draw_files(&ui, &files),
                3 => draw_file_view(&ui, &files),
                4 => draw_settings(&ui, &settings, setting_sel),
                _ => draw_about(&ui, &about_info[..info_len(&about_info)]),
            }
            need_redraw = false;
        }

        let n = input(&mut ev);
        if n == 0 {
            continue;
        }
        // 任一有效按键（含未改变状态的键）都触发下轮重绘：重画幂等，
        // 漏判状态变化的代价（画面陈旧）远大于多画一帧。
        let mut handled = false;
        'keys: for i in 0..n {
            let key = ev[i];
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
                        break 'keys;
                    }
                }
                1 => match key {
                    K_UP => menu_sel = (menu_sel + MENU_ITEMS.len() - 1) % MENU_ITEMS.len(),
                    K_DOWN => menu_sel = (menu_sel + 1) % MENU_ITEMS.len(),
                    K_ESC => phase = 0,
                    K_ENTER => {
                        match menu_sel {
                            0 => {
                                files.refresh();
                                // 每次进文件页都重报数据源（任务58 拔出全链
                                // 演练依赖：拔 SHARED 后如实降级 demo-tree）。
                                report_count(&files);
                                phase = 2;
                                marker(b"SHELL: files opened");
                            }
                            1 => {
                                settings.load();
                                // 重进设置页光标归零（与开始菜单 menu_sel、
                                // 文件页 files.sel 同一约定：页面重进=顶部）。
                                setting_sel = 0;
                                phase = 4;
                                marker(b"SHELL: settings opened");
                            }
                            _ => {
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
                        }
                        break 'keys;
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
                    K_ENTER => {
                        if files.count > 0 && !files.is_dir[files.sel] {
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
                            break 'keys;
                        }
                    }
                    _ => {}
                },
                3 => {
                    if key == K_ESC || key == K_ENTER {
                        phase = 2;
                        break 'keys;
                    }
                }
                4 => match key {
                    K_UP => setting_sel = (setting_sel + 2) % 3,
                    K_DOWN => setting_sel = (setting_sel + 1) % 3,
                    K_ESC => phase = 0,
                    K_LEFT => adjust_setting(&mut settings, setting_sel, -1),
                    K_RIGHT => adjust_setting(&mut settings, setting_sel, 1),
                    K_ENTER => {
                        if setting_sel == 1 {
                            settings.show_menu = !settings.show_menu;
                            let rc = kv_set(
                                b"settings",
                                b"show_menu",
                                if settings.show_menu { b"1" } else { b"0" },
                            );
                            report_kv(b"show_menu", rc);
                            break 'keys;
                        }
                    }
                    _ => {}
                },
                _ => {
                    if key == K_ESC || key == K_ENTER {
                        phase = 0;
                        break 'keys;
                    }
                }
            }
        }
        if handled {
            need_redraw = true;
        }
    }
}

fn info_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
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
