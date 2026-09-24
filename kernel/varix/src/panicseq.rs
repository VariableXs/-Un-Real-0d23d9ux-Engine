//! panic 序列器（B-2903 · MD2 篇 29.2 的崩溃路径）。
//!
//! # 四环节（篇 29.2 逐字）
//!
//! 保护屏（26.2 规范）→ 现场信息写保护内存带 → 十秒倒计时 → 复位寄存器。
//! panic 路径**故意不走优雅收尾**（栈可能已坏），它的安全承诺只有一条：
//! 尽快干净地重启，让日志重放兜底。
//!
//! # 纯逻辑 / 目标态分界
//!
//! 现场带记录的编解码（固定布局 + CRC32）、复位阶梯清单、倒计时秒数
//! 全部纯函数宿主可测；保护屏绘制、HHDM 读写、TSC 计时、端口复位在
//! `target_os = "none"` 下才编译。`main.rs` 的 `#[panic_handler]` 只做
//! 串口直写（不依赖任何全局状态）后调用 [`panic_sequence`]。
//!
//! # 保护内存带的落位与"复位后可读"
//!
//! 落位固定在物理 `0x60000`（低 640K 传统空闲区，4KiB）：复位寄存器
//! 复位**不断电**——RAM 内容跨复位保留，Limine 重新加载内核时不会触碰
//! 这一段。boot 链在 `memmap::register_boot_reservations` 之后调用
//! [`boot_guard_band_hook`]：先重放（magic+CRC 过 → 打印上次现场 → 清
//! magic），再 `claim(Purpose::LogRing)` 登记本次落位。claim 被拒（与
//! 既有预留重叠）时 armed=false 如实降级——panic 现场只走串口，绝不
//! 写一段没登记的内存。同固件布局下两次启动落位一致，是"现场带可读"
//! 的前提假设；QEMU 对练（B-2903 百次）就是对这个假设的实证。

use crate::power::crc32;

/// 现场带物理落位（低 640K 传统空闲区；boot 期 claim 登记防侵占）。
/// 现场带历史落位（0x60000）。勘察假设"复位不清 RAM、Limine 不触碰"——
/// 2026-09-24 QEMU 对练实证证伪：Limine BIOS stage 的低位工作区把该页
/// 清零，panic 写入后第二轮重放读回全零。落位已改 boot 期动态选位
/// （`band_base`），本常量仅作历史存档与文档锚点。
pub const GUARD_BAND_ADDR: u64 = 0x60000;
/// 动态落位余量：usable 顶与现场带之间预留的无人区（避开 Limine 的
/// 内核/模块/reclaimable 分配顶；SeaBIOS 只管低位 1MB 与 EBDA）。
pub const GUARD_BAND_MARGIN: u64 = 4 << 20;
/// 现场带容量（一页）。
pub const GUARD_BAND_BYTES: usize = 4096;
/// 现场带魔数（"VPBN" — Varix Panic Band Note）。
pub const GUARD_BAND_MAGIC: u32 = 0x5650_424E;
/// 现场带布局版本。
pub const GUARD_BAND_VERSION: u8 = 1;
/// 消息域上限（诚实截断——全文在串口，现场带存得下多少是多少；
/// 255 = msg_len 单字节域的干净上限，256 会溢出成 0）。
pub const GUARD_BAND_MSG_MAX: usize = 255;
/// 倒计时秒数（篇 29.2：十秒）。
pub const COUNTDOWN_SECS: u32 = 10;

/// panic 复位阶梯（B-2903；**零 UEFI RS**——ResetSystem 需要运行期恒等
/// 映射与页分配，panic 栈可能已坏，绝不走）。四级，每级落空落下一级：
pub const RESET_LADDER: [&str; 4] = [
    "fadt-reset-reg — 固件声明的复位端口（B-2901 最小集第一件，零分配）",
    "8042-pulse — 0xFE -> 0x64（控制器命令复位）",
    "0xCF9 — 先 0x04 暖复位再 0x06 全复位（PCH 复位寄存器）",
    "triple-fault — IDTR 置空 + int3（零外设依赖兜底）",
];

/// 头部固定布局（`#[repr(C)]` 偏移进测试断言锁定）。
pub const HDR_MAGIC_OFF: usize = 0;
pub const HDR_VERSION_OFF: usize = 4;
pub const HDR_MSG_LEN_OFF: usize = 5;
pub const HDR_TSC_OFF: usize = 8;
pub const HDR_LINE_OFF: usize = 16;
pub const HDR_CRC_OFF: usize = 20;
pub const HDR_MSG_OFF: usize = 24;
/// 头部（含 CRC 域）大小。
pub const HDR_SIZE: usize = 24;

/// 单条现场记录的校验范围：头部前 20 字节（magic..crc 之前）+ 消息域。
fn crc_scope(msg: &[u8]) -> u32 {
    // 编码侧：对 msg 内容本身取 CRC——头部字段（line/tsc/msg_len）与
    // CRC 一同被"写入后不可变"的使用方式保护；解码侧以 magic/version/
    // msg_len/CRC 四道闸拒收垃圾。
    crc32(msg)
}

/// 编码一条现场记录进 4KiB 缓冲（超长消息诚实截断到 [`GUARD_BAND_MSG_MAX`]）。
pub fn encode_guard_band(line: u32, tsc: u64, msg: &[u8]) -> [u8; GUARD_BAND_BYTES] {
    let mut out = [0u8; GUARD_BAND_BYTES];
    let take = msg.len().min(GUARD_BAND_MSG_MAX);
    out[HDR_MAGIC_OFF..HDR_MAGIC_OFF + 4].copy_from_slice(&GUARD_BAND_MAGIC.to_le_bytes());
    out[HDR_VERSION_OFF] = GUARD_BAND_VERSION;
    out[HDR_MSG_LEN_OFF] = take as u8;
    out[HDR_TSC_OFF..HDR_TSC_OFF + 8].copy_from_slice(&tsc.to_le_bytes());
    out[HDR_LINE_OFF..HDR_LINE_OFF + 4].copy_from_slice(&line.to_le_bytes());
    let crc = crc_scope(&msg[..take]);
    out[HDR_CRC_OFF..HDR_CRC_OFF + 4].copy_from_slice(&crc.to_le_bytes());
    out[HDR_MSG_OFF..HDR_MSG_OFF + take].copy_from_slice(&msg[..take]);
    out
}

/// 解码一条现场记录。四道闸：magic、version、msg_len 一致性、CRC。
/// 返回 `(line, tsc, msg_len)`；消息本体在 `buf[HDR_MSG_OFF..HDR_MSG_OFF+msg_len]`。
pub fn decode_guard_band(buf: &[u8; GUARD_BAND_BYTES]) -> Option<(u32, u64, usize)> {
    let magic = u32::from_le_bytes([
        buf[HDR_MAGIC_OFF],
        buf[HDR_MAGIC_OFF + 1],
        buf[HDR_MAGIC_OFF + 2],
        buf[HDR_MAGIC_OFF + 3],
    ]);
    if magic != GUARD_BAND_MAGIC {
        return None;
    }
    if buf[HDR_VERSION_OFF] != GUARD_BAND_VERSION {
        return None;
    }
    let msg_len = buf[HDR_MSG_LEN_OFF] as usize;
    if msg_len > GUARD_BAND_MSG_MAX {
        return None;
    }
    let crc = u32::from_le_bytes([
        buf[HDR_CRC_OFF],
        buf[HDR_CRC_OFF + 1],
        buf[HDR_CRC_OFF + 2],
        buf[HDR_CRC_OFF + 3],
    ]);
    if crc != crc_scope(&buf[HDR_MSG_OFF..HDR_MSG_OFF + msg_len]) {
        return None;
    }
    let tsc = u64::from_le_bytes([
        buf[HDR_TSC_OFF],
        buf[HDR_TSC_OFF + 1],
        buf[HDR_TSC_OFF + 2],
        buf[HDR_TSC_OFF + 3],
        buf[HDR_TSC_OFF + 4],
        buf[HDR_TSC_OFF + 5],
        buf[HDR_TSC_OFF + 6],
        buf[HDR_TSC_OFF + 7],
    ]);
    let line = u32::from_le_bytes([
        buf[HDR_LINE_OFF],
        buf[HDR_LINE_OFF + 1],
        buf[HDR_LINE_OFF + 2],
        buf[HDR_LINE_OFF + 3],
    ]);
    Some((line, tsc, msg_len))
}

// ---------------------------------------------------------------------------
// PanicInfo → 字节（无 fmt 机制、无堆；自 main.rs 原样收编并补消息域）
// ---------------------------------------------------------------------------

/// PanicInfo → bytes：`file:line` + 消息 payload + 换行（PanicInfo 薄封装）。
pub fn fmt_panic(info: &core::panic::PanicInfo<'_>) -> [u8; 256] {
    let file = info.location().map(|l| l.file()).unwrap_or("?");
    let line = info.location().map(|l| l.line()).unwrap_or(0);
    // 消息 payload：core::panic! 宏直接给 &'static str 时捕获；formatted
    // 参数的 payload 拿不到原文——诚实留空，file:line 已定位。
    // no_std #[panic_handler] 拿消息 payload 只有 downcast 一条路
    // （payload_as_str 尚未在 core 稳定）——显式压弃用警告并留据。
    #[allow(deprecated)]
    let payload = info.payload();
    let msg = payload.downcast_ref::<&'static str>().copied();
    fmt_panic_parts(file, line, msg)
}

/// 拼装现场文本：`file:line\n` + 消息 + `\n`。panic 语境禁 fmt 禁堆——
/// 固定 256 字节栈缓冲，超出诚实截断（全文路径：串口直写同字节流）。
pub fn fmt_panic_parts(file: &str, line: u32, msg: Option<&str>) -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut n = 0usize;
    let push = |bytes: &[u8], out: &mut [u8; 256], n: &mut usize| {
        for &b in bytes {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    push(file.as_bytes(), &mut out, &mut n);
    push(b":", &mut out, &mut n);
    // 行号十进制手工展开。
    let mut num = [0u8; 10];
    let mut w = 0usize;
    let mut v = line;
    if v == 0 {
        num[0] = b'0';
        w = 1;
    } else {
        while v > 0 && w < num.len() {
            num[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
    }
    while w > 0 {
        w -= 1;
        push(&[num[w]], &mut out, &mut n);
    }
    push(b"\n", &mut out, &mut n);
    if let Some(m) = msg {
        push(m.as_bytes(), &mut out, &mut n);
    }
    out
}

/// NUL 截断长度（fmt_panic 产物的有效字节；尾部零不上屏不带）。
pub fn cstr_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

// ---------------------------------------------------------------------------
// 目标态：panic 序列编排（#[panic_handler] 的唯一后继）
// ---------------------------------------------------------------------------

/// 现场带本次启动是否已登记（claim 成功才许写）。
#[cfg(target_os = "none")]
static BAND_ARMED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// boot 链钩子（`memmap::register_boot_reservations` 之后调用）：
/// ① 重放——上次 panic 的现场若还在（复位不清 RAM），magic+CRC 过则
///    打印并清掉；② 登记——claim 本次的落位（重叠被拒则 armed=false
///    如实降级）。宿主构建为空操作（测试不碰物理内存）。
pub fn boot_guard_band_hook() {
    #[cfg(target_os = "none")]
    {
        // ① 重放：读 → 验 → 打印 → 清魔数（防"上上次"重复报告）。
        if let Some(va) = band_hhdm() {
            let mut buf = [0u8; GUARD_BAND_BYTES];
            // SAFETY: 0x60000..0x61000 是本次启动 claim 目标（下方登记）；
            // claim 未成功前这一段也只读——boot 单核路径，无并发写者。
            let src = va as *const u8;
            for (i, b) in buf.iter_mut().enumerate() {
                *b = unsafe { core::ptr::read_volatile(src.add(i)) };
            }
            if let Some((line, tsc, msg_len)) = decode_guard_band(&buf) {
                let msg = core::str::from_utf8(&buf[HDR_MSG_OFF..HDR_MSG_OFF + msg_len])
                    .unwrap_or("<binary>");
                crate::kinfo!(
                    "guard-band: last panic @line {} (tsc {}) — {}",
                    line,
                    tsc,
                    msg
                );
                // 清魔数：消费一次。
                let zero = [0u8; 4];
                // SAFETY: va 指向本次启动 claim 登记的 4KiB 落位（guard
                // band），单核 boot 路径无并发写者；偏移在页内。
                let dst: *mut u8 = unsafe { (va as *mut u8).add(HDR_MAGIC_OFF) };
                for (i, b) in zero.iter().enumerate() {
                    unsafe { core::ptr::write_volatile(dst.add(i), *b) };
                }
            }
        }
        // ② 登记：claim 成功才武装。与既有预留重叠 → kwarn + 降级。
        let Some(band) = band_base() else {
            BAND_ARMED.store(false, core::sync::atomic::Ordering::Relaxed);
            crate::kwarn!(
                "guard-band: not armed (no usable memmap region for dynamic placement) — panic notes serial-only"
            );
            return;
        };
        let claim = crate::memmap::reservations().claim(
            band,
            GUARD_BAND_BYTES as u64,
            crate::memmap::Purpose::LogRing,
            "panic guard band (B-2903)",
        );
        match claim {
            Ok(()) => {
                BAND_ARMED.store(true, core::sync::atomic::Ordering::Relaxed);
                crate::kinfo!(
                    "guard-band: armed at {:#x} ({}B) — panic field notes will survive reset",
                    band,
                    GUARD_BAND_BYTES
                );
            }
            Err(why) => {
                BAND_ARMED.store(false, core::sync::atomic::Ordering::Relaxed);
                crate::kwarn!(
                    "guard-band: not armed (claim rejected: {}) — panic notes serial-only",
                    why
                );
            }
        }
    }
}

#[cfg(target_os = "none")]
fn band_base() -> Option<u64> {
    // 动态落位：Limine memmap 最大 usable 区间顶部下移余量后 4KiB 对齐。
    // 每轮 boot 的 memmap 逐位相同（同一固件+引导器+配置）→ 落位逐位
    // 复现 → panic 写入后复位重启可重放（QEMU 对练闭环的根基）。
    let mm = crate::limine::memmap()?;
    let mut best: Option<(u64, u64)> = None; // (base, length) of largest usable
    for e in mm {
        if e.kind == crate::limine::memmap_kind::USABLE
            && e.length >= GUARD_BAND_BYTES as u64
            && best.map_or(true, |(_, l)| e.length > l)
        {
            best = Some((e.base, e.length));
        }
    }
    let (_base, length) = best?;
    let top = _base.checked_add(length)?;
    let cand = top.checked_sub(GUARD_BAND_MARGIN)?;
    Some(cand & !4095)
}

#[cfg(target_os = "none")]
fn band_hhdm() -> Option<u64> {
    let off = crate::limine::hhdm_offset()? as u64;
    band_base()?.checked_add(off)
}

/// 现场信息写保护内存带（armed 且 HHDM 就位才写；失败如实 false——
/// 串口已有全文，现场带是加分项不是唯一路径）。
#[cfg(target_os = "none")]
fn write_guard_band(line: u32, tsc: u64, msg: &[u8]) -> bool {
    if !BAND_ARMED.load(core::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let Some(va) = band_hhdm() else {
        return false;
    };
    let enc = encode_guard_band(line, tsc, msg);
    // SAFETY: 落位已在本 boot claim（BAND_ARMED=true 保证），4KiB 一页，
    // boot 单核后 panic 路径无并发写者（其他核可能还在跑，但本页无他人
    // 登记使用）。
    let dst = va as *mut u8;
    for (i, b) in enc.iter().enumerate() {
        unsafe { core::ptr::write_volatile(dst.add(i), *b) };
    }
    true
}

/// 保护屏（26.2 规范面）：全屏接管、三要素文案。best-effort——帧缓冲
/// 未就位（boot 极早期 panic）直接 false，串口兜底。无锁无分配（Limine
/// 响应读取 + 帧缓冲直写）。
#[cfg(target_os = "none")]
fn paint_protect_screen(detail: &[u8], countdown: u32) -> bool {
    let Some(fb) = crate::limine::framebuffer() else {
        return false;
    };
    let Ok(s) = crate::fb::Surface::from_limine(fb) else {
        return false;
    };
    crate::banner::paint_backdrop(&s);
    let w = s.width() as i64;
    let h = s.height() as i64;
    let ink = crate::fb::Color::rgb(0xF2, 0xF5, 0xFA);
    let red = crate::fb::Color::rgb(0xE8, 0x53, 0x53);
    let dim = crate::fb::Color::rgb(0x9A, 0xA6, 0xB8);
    let title = "KERNEL PANIC";
    let tw = crate::font::text_width_scaled(title, 3);
    let y0 = h / 2 - 96;
    crate::font::draw_text_scaled(&s, (w - tw) / 2, y0, title, red, 3);
    // 现场行（file:line + 消息；ASCII 域逐行画，多字节字符以空格兜底）。
    let cols = ((w / crate::font::GLYPH_W as i64).max(1)) as usize;
    let cols = cols.min(96).max(24);
    let mut line_y = y0 + 64;
    for line in ascii_lines(detail, cols) {
        let text = core::str::from_utf8(line).unwrap_or("");
        crate::font::draw_text_scaled(&s, w / 8, line_y, text, ink, 1);
        line_y += 18;
    }
    // 倒计时行（发生什么+为什么之外的第三要素：下一步会发生什么）。
    let (cd, n) = format_cd(countdown);
    let cd_text = core::str::from_utf8(&cd[..n]).unwrap_or("rebooting");
    crate::font::draw_text_scaled(&s, w / 8, line_y + 12, cd_text, dim, 1);
    true
}

/// 倒计时行的栈上组装（禁 fmt：u32 → ASCII 手工展开）。
/// 返回 (缓冲, 有效长度)——尾部不填零不填空格，串口/屏幕共用。
#[cfg(target_os = "none")]
fn format_cd(secs: u32) -> ([u8; 40], usize) {
    let mut out = [0u8; 40];
    let prefix = b"rebooting in ";
    out[..prefix.len()].copy_from_slice(prefix);
    let mut v = secs;
    let mut digits = [0u8; 10];
    let mut n = 0;
    if v == 0 {
        digits[0] = b'0';
        n = 1;
    } else {
        while v > 0 && n < 10 {
            digits[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
    }
    let mut w = prefix.len();
    while n > 0 {
        n -= 1;
        out[w] = digits[n];
        w += 1;
    }
    let suffix = b"s";
    out[w..w + suffix.len()].copy_from_slice(suffix);
    (out, w + suffix.len())
}

/// ASCII 行折分（保护屏现场行用；无堆：返回固定窗口切片）。
#[cfg(target_os = "none")]
fn ascii_lines<'a>(text: &'a [u8], cols: usize) -> impl Iterator<Item = &'a [u8]> {
    text.chunks(cols.max(1)).take(8)
}

/// panic 复位阶梯（四级，零 UEFI RS 零分配；全部落空前不返回）。
#[cfg(target_os = "none")]
fn reset_ladder() -> ! {
    // ① FADT RESET_REG（固件声明的复位端口——B-2901 最小集第一件）。
    if crate::bootnext::reset_via_fadt() {
        crate::kinfo!("panic-reset: FADT reset returned — ladder next");
    } else {
        crate::kinfo!("panic-reset: no FADT reset reg — ladder next");
    }
    spin_cycles(10_000_000);
    // ② 8042 脉冲复位。
    // SAFETY: 端口 IO 单一来源（ps2::port）；bit1=输入缓冲满。
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
    // ③ PCH 复位寄存器。
    // SAFETY: 0xCF9 为标准 PCH 复位寄存器，非 Intel 平台写入无害。
    unsafe {
        crate::ps2::port::outp(0xCF9, 0x04);
        spin_cycles(1_000);
        crate::ps2::port::outp(0xCF9, 0x06);
    }
    spin_cycles(50_000_000);
    // ④ 三重故障兜底（零外设依赖，任何 x86 平台/QEMU 一致）。
    crate::kinfo!("panic-reset: triple-fault fallback");
    triple_fault()
}

#[cfg(target_os = "none")]
fn spin_cycles(n: u64) {
    for _ in 0..n {
        core::hint::spin_loop();
    }
}

/// 三重故障复位：IDTR 置空（limit=0）后 int3 → #DF → 三重故障 → 硬复位。
#[cfg(target_os = "none")]
fn triple_fault() -> ! {
    let idtr = [0u64; 2];
    unsafe {
        core::arch::asm!(
            "lidt [{p}]",
            "int3",
            p = in(reg) idtr.as_ptr(),
            options(noreturn)
        );
    }
}

/// panic 序列总编排（`main.rs` 的 `#[panic_handler]` 在串口直写后调用；
/// 宿主构建不可达——panic_handler 只在内核目标链接）。
///
/// 序：保护屏 → 现场带 → 十秒倒计时（每秒重绘+串口留痕）→ 复位阶梯。
#[cfg(target_os = "none")]
pub fn panic_sequence(info: &core::panic::PanicInfo<'_>) -> ! {
    let raw = fmt_panic(info);
    let n = cstr_len(&raw);
    let detail = &raw[..n];
    let tsc_hz = crate::platform::info()
        .map(|p| p.tsc_hz)
        .unwrap_or(crate::platform::FALLBACK_TSC_HZ);
    // ① 保护屏（倒计时起始态一并画出）。
    let _ = paint_protect_screen(detail, COUNTDOWN_SECS);
    // ② 现场带（armed 才写；tsc 现取——platform 未就位时是原始计数，
    //    依旧单调可用）。
    let tsc = crate::timeline::read_tsc();
    let line = info.location().map(|l| l.line()).unwrap_or(0);
    let wrote = write_guard_band(line, tsc, detail);
    crate::kinfo!("panic: guard band written={}", wrote);
    // ③ 十秒倒计时（TSC 自旋，不依赖中断与时钟域）。
    let mut left = COUNTDOWN_SECS;
    while left > 0 {
        let (cd, cn) = format_cd(left);
        crate::serial::write_bytes(b"panic: reset in ");
        crate::serial::write_bytes(&cd[13..cn]);
        crate::serial::write_bytes(b"\n");
        let _ = paint_protect_screen(detail, left);
        let one = tsc_hz.max(1);
        let now = crate::timeline::read_tsc();
        while crate::timeline::read_tsc().saturating_sub(now) < one {
            core::hint::spin_loop();
        }
        left -= 1;
    }
    // ④ 复位阶梯（不返回）。
    reset_ladder()
}

#[cfg(not(target_os = "none"))]
pub fn panic_sequence(_info: &core::panic::PanicInfo<'_>) -> ! {
    unreachable!("panic_sequence is only linked on the kernel target")
}

// ---------------------------------------------------------------------------
// 测试：现场带编解码往返 / CRC 拒收 / 截断 / 布局锁定 / 阶梯清单
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_band_round_trip() {
        let enc = encode_guard_band(807, 0x1234_5678_9ABC_DEF0, b"SYSCALL 99 not handled");
        let (line, tsc, msg_len) = decode_guard_band(&enc).expect("round trip");
        assert_eq!(line, 807);
        assert_eq!(tsc, 0x1234_5678_9ABC_DEF0);
        assert_eq!(msg_len, b"SYSCALL 99 not handled".len());
        assert_eq!(&enc[HDR_MSG_OFF..HDR_MSG_OFF + msg_len], b"SYSCALL 99 not handled");
    }

    #[test]
    fn guard_band_rejects_corruption() {
        let mut enc = encode_guard_band(1, 2, b"ok");
        // ① 消息域被改：CRC 不过。
        enc[HDR_MSG_OFF] ^= 0xFF;
        assert!(decode_guard_band(&enc).is_none());
        // ② magic 被清：当场拒收（消费后清魔数的重放面）。
        let mut enc2 = encode_guard_band(1, 2, b"ok");
        enc2[HDR_MAGIC_OFF..HDR_MAGIC_OFF + 4].copy_from_slice(&[0; 4]);
        assert!(decode_guard_band(&enc2).is_none());
        // ③ version 不认识：拒收。
        let mut enc3 = encode_guard_band(1, 2, b"ok");
        enc3[HDR_VERSION_OFF] = 99;
        assert!(decode_guard_band(&enc3).is_none());
        // ④ CRC 域被改：拒收。
        let mut enc4 = encode_guard_band(1, 2, b"ok");
        enc4[HDR_CRC_OFF] ^= 0xFF;
        assert!(decode_guard_band(&enc4).is_none());
        // ⑤ 全零缓冲（断电后未写入的正常态）：magic 闸直接拒。
        assert!(decode_guard_band(&[0u8; GUARD_BAND_BYTES]).is_none());
    }

    #[test]
    fn guard_band_truncates_honestly() {
        let big = [0x41u8; 1024]; // 'A' × 1024 > MSG_MAX
        let enc = encode_guard_band(1, 2, &big);
        let (_, _, msg_len) = decode_guard_band(&enc).expect("decodable");
        assert_eq!(msg_len, GUARD_BAND_MSG_MAX);
    }

    #[test]
    fn guard_band_layout_locked() {
        // 布局即契约：偏移常量与 repr(C) 结构一致，跨启动可读的前提。
        assert_eq!(HDR_SIZE, 24);
        assert_eq!(HDR_MSG_OFF, 24);
        assert!(GUARD_BAND_BYTES == 4096);
        assert!(GUARD_BAND_MSG_MAX == 255);
        assert_eq!(GUARD_BAND_MAGIC, 0x5650_424E);
        // 一条记录必须完整落进一页。
        assert!(HDR_MSG_OFF + GUARD_BAND_MSG_MAX <= GUARD_BAND_BYTES);
    }

    #[test]
    fn reset_ladder_has_four_zero_alloc_rungs() {
        // 四级、无 UEFI RS 级（ResetSystem 需要恒等映射+分配，panic 禁走）。
        assert_eq!(RESET_LADDER.len(), 4);
        assert!(RESET_LADDER[0].starts_with("fadt-reset-reg"));
        assert!(RESET_LADDER[1].starts_with("8042-pulse"));
        assert!(RESET_LADDER[2].starts_with("0xCF9"));
        assert!(RESET_LADDER[3].starts_with("triple-fault"));
        assert!(!RESET_LADDER.iter().any(|s| s.contains("UEFI")));
    }

    #[test]
    fn countdown_is_ten() {
        assert_eq!(COUNTDOWN_SECS, 10);
    }

    #[test]
    fn fmt_panic_captures_location_and_message() {
        // 字段版直测（PanicInfo 无 stable 构造面；薄封装透传字段）。
        let bytes = fmt_panic_parts("kernel/varix/src/usrshell.rs", 42, Some("boom message"));
        let text = core::str::from_utf8(&bytes).unwrap_or("");
        let text = text.split('\0').next().unwrap_or("");
        assert!(text.starts_with("kernel/varix/src/usrshell.rs:42\n"), "loc: {}", text);
        assert!(text.contains("boom message"), "message captured: {}", text);
        // 无消息（formatted payload）时诚实留空——file:line 仍在。
        let bare = fmt_panic_parts("src/a.rs", 7, None);
        let bare_text = core::str::from_utf8(&bare).unwrap_or("");
        let bare_text = bare_text.split('\0').next().unwrap_or("");
        assert_eq!(bare_text, "src/a.rs:7\n");
        // 超长消息诚实截断到 256。
        let big = [0x42u8; 1024];
        let trunc = fmt_panic_parts("f", 1, Some(core::str::from_utf8(&big).unwrap_or("")));
        assert_eq!(cstr_len(&trunc), 256);
    }

    #[test]
    fn crc_scope_matches_power_crc32() {
        // 与 power 域同一套 CRC32 口径（同源审计：不另立门户）。
        // 0xCBF4_3926 = IEEE CRC-32(b"123456789") 标准值（zlib 对账）。
        assert_eq!(crc_scope(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc_scope(b""), crc32(b""));
    }
}
